use std::collections::VecDeque;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bobba_protocol::key_recovery::{Probation, ProbationVerdict, SBox, find_candidates, shortlist};
use bobba_protocol::{Direction, Framer, HMessage, Packet, PacketReader, Rc4};
use bytes::Bytes;
use futures::StreamExt;
use futures::stream::FuturesOrdered;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::interceptor::{ConnectionInfo, Interceptor, Verdict};
use crate::log::{LogEntry, Outcome};
use crate::table_source::TableSource;

const UPSTREAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const READ_BUFFER_SIZE: usize = 64 * 1024;
const VERSION_PACKET_MAX_LENGTH: u32 = 100;
const ENCRYPTED_READS_BEFORE_RECOVERY: u32 = 3;
const KEY_RECOVERY_ATTEMPTS: usize = 2;
const PROBATION_LIMIT: Duration = Duration::from_secs(2);
const KEY_EXCHANGE_LAST_INDEX: u32 = 1;
const KEY_EXCHANGE_WITH_FLAG_MIN_LENGTH: usize = 500;
const KEY_EXCHANGE_MIN_LENGTH: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upstream {
    pub host: String,
    pub port: u16,
}

pub struct DirectProxy {
    listener: TcpListener,
    upstream: Upstream,
}

impl DirectProxy {
    pub async fn bind(listen: SocketAddr, upstream: Upstream) -> io::Result<Self> {
        Ok(Self {
            listener: TcpListener::bind(listen).await?,
            upstream,
        })
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    pub async fn accept(&self) -> io::Result<Session> {
        let (client, _) = self.listener.accept().await?;
        let address = (self.upstream.host.as_str(), self.upstream.port);
        let server = tokio::time::timeout(UPSTREAM_CONNECT_TIMEOUT, TcpStream::connect(address))
            .await
            .map_err(|_| {
                io::Error::new(io::ErrorKind::TimedOut, "upstream did not accept in time")
            })??;
        client.set_nodelay(true)?;
        server.set_nodelay(true)?;
        Ok(Session {
            client,
            server,
            upstream: self.upstream.clone(),
        })
    }
}

pub struct Session {
    client: TcpStream,
    server: TcpStream,
    upstream: Upstream,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    Connected(ConnectionInfo),
    KeyRecovered {
        candidates: usize,
        elapsed: Duration,
    },
    KeyRecoveryFailed,
    Ended,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("the session has ended")]
pub struct SessionClosed;

struct Injection {
    packet: Packet,
    by: String,
}

#[derive(Clone)]
pub struct SessionHandle {
    to_server: mpsc::UnboundedSender<Injection>,
    to_client: mpsc::UnboundedSender<Injection>,
    cancel: CancellationToken,
    dropped_log_entries: Arc<AtomicU64>,
}

impl std::fmt::Debug for SessionHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionHandle")
            .field("closed", &self.cancel.is_cancelled())
            .finish()
    }
}

impl SessionHandle {
    pub fn inject(
        &self,
        direction: Direction,
        packet: Packet,
        by: impl Into<String>,
    ) -> Result<(), SessionClosed> {
        let sender = match direction {
            Direction::ToServer => &self.to_server,
            Direction::ToClient => &self.to_client,
        };
        sender
            .send(Injection {
                packet,
                by: by.into(),
            })
            .map_err(|_| SessionClosed)
    }

    pub fn abort(&self) {
        self.cancel.cancel();
    }

    pub fn is_closed(&self) -> bool {
        self.cancel.is_cancelled()
    }

    pub fn dropped_log_entries(&self) -> u64 {
        self.dropped_log_entries.load(Ordering::Relaxed)
    }
}

type PendingWrite = Pin<Box<dyn Future<Output = WriteAction> + Send>>;

enum WriteAction {
    Raw(Bytes),
    Packet {
        index: u32,
        original: Packet,
        verdict: Verdict,
    },
    StartEncrypting(Rc4),
}

fn ready(action: WriteAction) -> PendingWrite {
    Box::pin(std::future::ready(action))
}

struct Shared<I> {
    interceptor: Arc<I>,
    tables: Arc<dyn TableSource>,
    events: mpsc::UnboundedSender<SessionEvent>,
    log: mpsc::Sender<LogEntry>,
    confirmed: AtomicBool,
    client_encrypts: AtomicBool,
    server_encrypts: AtomicBool,
    fresh_key: watch::Sender<Option<Rc4>>,
    upstream: Upstream,
    handle: SessionHandle,
}

impl<I> Shared<I> {
    fn sender_encrypts(&self, direction: Direction) -> bool {
        match direction {
            Direction::ToServer => self.client_encrypts.load(Ordering::Acquire),
            Direction::ToClient => self.server_encrypts.load(Ordering::Acquire),
        }
    }
}

impl Session {
    pub fn start<I: Interceptor>(
        self,
        interceptor: Arc<I>,
        tables: Arc<dyn TableSource>,
        events: mpsc::UnboundedSender<SessionEvent>,
        log: mpsc::Sender<LogEntry>,
    ) -> (SessionHandle, tokio::task::JoinHandle<()>) {
        let (to_server, to_server_rx) = mpsc::unbounded_channel();
        let (to_client, to_client_rx) = mpsc::unbounded_channel();
        let handle = SessionHandle {
            to_server,
            to_client,
            cancel: CancellationToken::new(),
            dropped_log_entries: Arc::new(AtomicU64::new(0)),
        };
        let shared = Arc::new(Shared {
            interceptor,
            tables,
            events,
            log,
            confirmed: AtomicBool::new(false),
            client_encrypts: AtomicBool::new(false),
            server_encrypts: AtomicBool::new(false),
            fresh_key: watch::Sender::new(None),
            upstream: self.upstream,
            handle: handle.clone(),
        });
        let (client_read, client_write) = self.client.into_split();
        let (server_read, server_write) = self.server.into_split();
        let (outgoing_tx, outgoing_rx) = mpsc::unbounded_channel();
        let (incoming_tx, incoming_rx) = mpsc::unbounded_channel();

        let mut tasks = JoinSet::new();
        tasks.spawn(
            Reader::new(
                Direction::ToServer,
                client_read,
                outgoing_tx,
                shared.clone(),
            )
            .run(),
        );
        tasks.spawn(
            Reader::new(
                Direction::ToClient,
                server_read,
                incoming_tx,
                shared.clone(),
            )
            .run(),
        );
        tasks.spawn(write_loop(
            Direction::ToServer,
            server_write,
            outgoing_rx,
            to_server_rx,
            shared.clone(),
        ));
        tasks.spawn(write_loop(
            Direction::ToClient,
            client_write,
            incoming_rx,
            to_client_rx,
            shared.clone(),
        ));

        let supervisor = tokio::spawn(async move {
            tokio::select! {
                _ = tasks.join_next() => {}
                () = shared.handle.cancel.cancelled() => {}
            }
            shared.handle.cancel.cancel();
            tasks.shutdown().await;
            if shared.confirmed.load(Ordering::Acquire) {
                shared.interceptor.on_disconnected().await;
            }
            let _ = shared.events.send(SessionEvent::Ended);
        });
        (handle, supervisor)
    }
}

fn parse_version_packet(chunk: &[u8]) -> Option<(String, String)> {
    let length = u32::from_be_bytes(chunk.get(..4)?.try_into().ok()?);
    if chunk.len() <= Packet::HEADER_LEN || length >= VERSION_PACKET_MAX_LENGTH {
        return None;
    }
    let mut reader = PacketReader::new(&chunk[Packet::HEADER_LEN..]);
    Some((reader.read_string().ok()?, reader.read_string().ok()?))
}

enum Decryption {
    Plain,
    AwaitingKey {
        encrypted: Vec<u8>,
        reads: u32,
    },
    Probation {
        encrypted: Vec<u8>,
        probation: Probation,
        deadline: tokio::time::Instant,
    },
    Active(Rc4),
}

struct Reader<I> {
    direction: Direction,
    socket: OwnedReadHalf,
    writer: mpsc::UnboundedSender<PendingWrite>,
    shared: Arc<Shared<I>>,
    framer: Framer,
    next_index: u32,
    decryption: Decryption,
    buffer: Vec<u8>,
}

enum Flow {
    Continue,
    Stop,
}

impl<I: Interceptor> Reader<I> {
    fn new(
        direction: Direction,
        socket: OwnedReadHalf,
        writer: mpsc::UnboundedSender<PendingWrite>,
        shared: Arc<Shared<I>>,
    ) -> Self {
        Self {
            direction,
            socket,
            writer,
            shared,
            framer: Framer::new(),
            next_index: 0,
            decryption: Decryption::Plain,
            buffer: vec![0u8; READ_BUFFER_SIZE],
        }
    }

    async fn run(mut self) {
        let mut key_updates = self.shared.fresh_key.subscribe();
        loop {
            let waiting_for_other_side = self.direction == Direction::ToClient
                && matches!(self.decryption, Decryption::AwaitingKey { .. });
            let probation_deadline = match &self.decryption {
                Decryption::Probation { deadline, .. } => Some(*deadline),
                _ => None,
            };
            let read = tokio::select! {
                read = self.socket.read(&mut self.buffer) => read,
                () = tokio::time::sleep_until(probation_deadline.unwrap_or_else(tokio::time::Instant::now)), if probation_deadline.is_some() => {
                    if let Flow::Stop = self.settle_probation(true) {
                        return;
                    }
                    continue;
                }
                Ok(()) = key_updates.changed(), if waiting_for_other_side => {
                    let key = key_updates.borrow_and_update().clone();
                    if let Some(fresh) = key
                        && let Flow::Stop = self.start_decrypting(fresh)
                    {
                        return;
                    }
                    continue;
                }
                () = self.shared.handle.cancel.cancelled() => return,
            };
            let n = match read {
                Ok(0) | Err(_) => return,
                Ok(n) => n,
            };
            let chunk = self.buffer[..n].to_vec();
            if let Flow::Stop = self.on_chunk(chunk).await {
                return;
            }
        }
    }

    async fn on_chunk(&mut self, chunk: Vec<u8>) -> Flow {
        if !self.shared.confirmed.load(Ordering::Acquire) {
            let version = (self.direction == Direction::ToServer)
                .then(|| parse_version_packet(&chunk))
                .flatten();
            let Some((hotel_version, client_identifier)) = version else {
                return self.send(ready(WriteAction::Raw(Bytes::from(chunk))));
            };
            self.confirm(hotel_version, client_identifier).await;
        }
        if self.shared.sender_encrypts(self.direction)
            && matches!(self.decryption, Decryption::Plain)
        {
            self.decryption = Decryption::AwaitingKey {
                encrypted: Vec::new(),
                reads: 0,
            };
        }
        match &mut self.decryption {
            Decryption::Plain => self.dispatch(&chunk),
            Decryption::Active(cipher) => {
                let mut plain = chunk;
                cipher.apply(&mut plain);
                self.dispatch(&plain)
            }
            Decryption::Probation {
                encrypted,
                probation,
                ..
            } => {
                encrypted.extend_from_slice(&chunk);
                probation.feed(&chunk);
                self.settle_probation(false)
            }
            Decryption::AwaitingKey { encrypted, reads } => {
                encrypted.extend_from_slice(&chunk);
                *reads += 1;
                if self.direction == Direction::ToClient {
                    let known = self.shared.fresh_key.borrow().clone();
                    return match known {
                        Some(fresh) => self.start_decrypting(fresh),
                        None => Flow::Continue,
                    };
                }
                if *reads < ENCRYPTED_READS_BEFORE_RECOVERY {
                    return Flow::Continue;
                }
                self.recover_key().await
            }
        }
    }

    async fn confirm(&mut self, hotel_version: String, client_identifier: String) {
        let connection = ConnectionInfo {
            host: self.shared.upstream.host.clone(),
            port: self.shared.upstream.port,
            hotel_version,
            client_identifier,
            client_type: "FLASH".to_owned(),
        };
        self.shared.confirmed.store(true, Ordering::Release);
        self.shared
            .interceptor
            .on_connected(connection.clone(), self.shared.handle.clone())
            .await;
        let _ = self.shared.events.send(SessionEvent::Connected(connection));
    }

    async fn recover_key(&mut self) -> Flow {
        let started = Instant::now();
        for _ in 0..KEY_RECOVERY_ATTEMPTS {
            let Some(tables) = self.scan_while_reading().await else {
                return Flow::Stop;
            };
            self.drain_ready_bytes();
            let Decryption::AwaitingKey { encrypted, .. } = &self.decryption else {
                return Flow::Stop;
            };
            let snapshot = encrypted.clone();
            let candidates =
                tokio::task::spawn_blocking(move || find_candidates(&tables, &snapshot))
                    .await
                    .unwrap_or_default();
            let candidate_count = candidates.len();
            let interceptor = self.shared.interceptor.clone();
            let states = shortlist(candidates, |id| {
                interceptor.is_known_header(Direction::ToServer, id)
            });
            if states.is_empty() {
                continue;
            }
            let _ = self.shared.events.send(SessionEvent::KeyRecovered {
                candidates: candidate_count,
                elapsed: started.elapsed(),
            });
            let encrypted = match std::mem::replace(&mut self.decryption, Decryption::Plain) {
                Decryption::AwaitingKey { encrypted, .. } => encrypted,
                _ => return Flow::Stop,
            };
            let probation = Probation::new(states, &encrypted);
            self.decryption = Decryption::Probation {
                encrypted,
                probation,
                deadline: tokio::time::Instant::now() + PROBATION_LIMIT,
            };
            return self.settle_probation(false);
        }
        self.fail_recovery()
    }

    async fn scan_while_reading(&mut self) -> Option<Vec<SBox>> {
        let source = self.shared.tables.clone();
        let mut scan = source.tables();
        loop {
            tokio::select! {
                tables = &mut scan => return Some(tables),
                read = self.socket.read(&mut self.buffer) => match read {
                    Ok(0) | Err(_) => return None,
                    Ok(n) => self.collect_encrypted(n),
                },
                () = self.shared.handle.cancel.cancelled() => return None,
            }
        }
    }

    fn drain_ready_bytes(&mut self) {
        while let Ok(n) = self.socket.try_read(&mut self.buffer) {
            if n == 0 {
                return;
            }
            self.collect_encrypted(n);
        }
    }

    fn collect_encrypted(&mut self, n: usize) {
        if let Decryption::AwaitingKey { encrypted, .. } = &mut self.decryption {
            encrypted.extend_from_slice(&self.buffer[..n]);
        }
    }

    fn settle_probation(&mut self, out_of_time: bool) -> Flow {
        let Decryption::Probation { probation, .. } = &self.decryption else {
            return Flow::Continue;
        };
        let chosen = match probation.verdict() {
            ProbationVerdict::Decided(fresh) => *fresh,
            ProbationVerdict::Undecided(_) if !out_of_time => return Flow::Continue,
            ProbationVerdict::Undecided(_) => match probation.best_guess() {
                Some(fresh) => fresh,
                None => return self.fail_recovery(),
            },
            ProbationVerdict::Failed => return self.fail_recovery(),
        };
        let Decryption::Probation { encrypted, .. } =
            std::mem::replace(&mut self.decryption, Decryption::Plain)
        else {
            return Flow::Stop;
        };
        self.decryption = Decryption::AwaitingKey {
            encrypted,
            reads: 0,
        };
        self.shared.fresh_key.send_replace(Some(chosen.clone()));
        self.start_decrypting(chosen)
    }

    fn fail_recovery(&mut self) -> Flow {
        let _ = self.shared.events.send(SessionEvent::KeyRecoveryFailed);
        self.shared.handle.abort();
        Flow::Stop
    }

    fn start_decrypting(&mut self, fresh: Rc4) -> Flow {
        let Decryption::AwaitingKey { encrypted, .. } =
            std::mem::replace(&mut self.decryption, Decryption::Active(fresh.clone()))
        else {
            return Flow::Continue;
        };
        if let Flow::Stop = self.send(ready(WriteAction::StartEncrypting(fresh))) {
            return Flow::Stop;
        }
        let mut plain = encrypted;
        if let Decryption::Active(cipher) = &mut self.decryption {
            cipher.apply(&mut plain);
        }
        self.dispatch(&plain)
    }

    fn dispatch(&mut self, plain: &[u8]) -> Flow {
        self.framer.push(plain);
        while let Some(next) = self.framer.next_packet() {
            let Ok(packet) = next else {
                self.shared.handle.abort();
                return Flow::Stop;
            };
            let index = self.next_index;
            self.next_index = self.next_index.wrapping_add(1);
            let server_started_encrypting = self.watch_key_exchange(index, &packet);
            if let Flow::Stop = self.forward(index, packet) {
                return Flow::Stop;
            }
            if server_started_encrypting {
                let rest = self.framer.take_remaining().to_vec();
                self.decryption = Decryption::AwaitingKey {
                    encrypted: Vec::new(),
                    reads: 0,
                };
                if rest.is_empty() {
                    return Flow::Continue;
                }
                if let Decryption::AwaitingKey { encrypted, .. } = &mut self.decryption {
                    encrypted.extend_from_slice(&rest);
                }
                let known = self.shared.fresh_key.borrow().clone();
                return match known {
                    Some(fresh) => self.start_decrypting(fresh),
                    None => Flow::Continue,
                };
            }
        }
        Flow::Continue
    }

    fn watch_key_exchange(&self, index: u32, packet: &Packet) -> bool {
        if self.direction != Direction::ToClient
            || index > KEY_EXCHANGE_LAST_INDEX
            || self.shared.client_encrypts.load(Ordering::Acquire)
        {
            return false;
        }
        let length = packet.frame().len() - 4;
        if packet.has_structure("sb") && length > KEY_EXCHANGE_WITH_FLAG_MIN_LENGTH {
            let mut reader = packet.reader();
            let server_encrypts =
                reader.read_string_bytes().is_ok() && reader.read_bool().unwrap_or(false);
            self.shared.client_encrypts.store(true, Ordering::Release);
            if server_encrypts {
                self.shared.server_encrypts.store(true, Ordering::Release);
            }
            return server_encrypts;
        }
        if packet.has_structure("s") && length > KEY_EXCHANGE_MIN_LENGTH {
            self.shared.client_encrypts.store(true, Ordering::Release);
        }
        false
    }

    fn forward(&mut self, index: u32, packet: Packet) -> Flow {
        let interceptor = self.shared.interceptor.clone();
        let direction = self.direction;
        let pending = async move {
            let verdict = interceptor
                .intercept(HMessage::new(packet.clone(), direction, index))
                .await;
            WriteAction::Packet {
                index,
                original: packet,
                verdict,
            }
        };
        self.send(Box::pin(pending))
    }

    fn send(&self, write: PendingWrite) -> Flow {
        match self.writer.send(write) {
            Ok(()) => Flow::Continue,
            Err(_) => Flow::Stop,
        }
    }
}

struct Writer<'a, I> {
    direction: Direction,
    socket: OwnedWriteHalf,
    encryption: Option<Rc4>,
    held_injections: VecDeque<Injection>,
    shared: &'a Shared<I>,
}

async fn write_loop<I: Interceptor>(
    direction: Direction,
    socket: OwnedWriteHalf,
    mut pending: mpsc::UnboundedReceiver<PendingWrite>,
    mut injections: mpsc::UnboundedReceiver<Injection>,
    shared: Arc<Shared<I>>,
) {
    let mut writer = Writer {
        direction,
        socket,
        encryption: None,
        held_injections: VecDeque::new(),
        shared: &shared,
    };
    let mut ordered = FuturesOrdered::new();
    let mut reader_open = true;
    loop {
        let result = tokio::select! {
            biased;
            () = shared.handle.cancel.cancelled() => return,
            Some(action) = ordered.next(), if !ordered.is_empty() => writer.apply(action).await,
            next = pending.recv(), if reader_open => {
                match next {
                    Some(write) => ordered.push_back(write),
                    None => reader_open = false,
                }
                Ok(())
            }
            Some(injection) = injections.recv() => writer.inject(injection).await,
        };
        if result.is_err() || (!reader_open && ordered.is_empty()) {
            break;
        }
    }
    let _ = writer.socket.shutdown().await;
}

impl<I> Writer<'_, I> {
    async fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        match &mut self.encryption {
            Some(cipher) => {
                let mut encrypted = bytes.to_vec();
                cipher.apply(&mut encrypted);
                self.socket.write_all(&encrypted).await
            }
            None => self.socket.write_all(bytes).await,
        }
    }

    async fn apply(&mut self, action: WriteAction) -> io::Result<()> {
        let (index, original, verdict) = match action {
            WriteAction::Raw(bytes) => return self.socket.write_all(&bytes).await,
            WriteAction::StartEncrypting(cipher) => {
                self.encryption = Some(cipher);
                while let Some(injection) = self.held_injections.pop_front() {
                    self.inject(injection).await?;
                }
                return Ok(());
            }
            WriteAction::Packet {
                index,
                original,
                verdict,
            } => (index, original, verdict),
        };
        let outcome = match verdict {
            Verdict::Forward => {
                self.write(original.frame()).await?;
                Outcome::Forwarded
            }
            Verdict::Edited { packet, by } => {
                self.write(packet.frame()).await?;
                Outcome::Edited { by, sent: packet }
            }
            Verdict::Blocked { by } => Outcome::Blocked { by },
        };
        record(
            self.shared,
            LogEntry::Intercepted {
                index,
                direction: self.direction,
                packet: original,
                outcome,
            },
        );
        Ok(())
    }

    async fn inject(&mut self, injection: Injection) -> io::Result<()> {
        if self.shared.sender_encrypts(self.direction) && self.encryption.is_none() {
            self.held_injections.push_back(injection);
            return Ok(());
        }
        self.write(injection.packet.frame()).await?;
        record(
            self.shared,
            LogEntry::Injected {
                direction: self.direction,
                packet: injection.packet,
                by: injection.by,
            },
        );
        Ok(())
    }
}

fn record<I>(shared: &Shared<I>, entry: LogEntry) {
    if shared.log.try_send(entry).is_err() {
        shared
            .handle
            .dropped_log_entries
            .fetch_add(1, Ordering::Relaxed);
    }
}
