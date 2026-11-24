use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use bobba_protocol::{Direction, Framer, HMessage, Packet, PacketReader};
use bytes::Bytes;
use futures::StreamExt;
use futures::stream::FuturesOrdered;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::interceptor::{ConnectionInfo, Interceptor, Verdict};
use crate::log::{LogEntry, Outcome};

const UPSTREAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const READ_BUFFER_SIZE: usize = 64 * 1024;
const VERSION_PACKET_MAX_LENGTH: u32 = 100;

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
}

struct Shared<I> {
    interceptor: Arc<I>,
    events: mpsc::UnboundedSender<SessionEvent>,
    log: mpsc::Sender<LogEntry>,
    confirmed: AtomicBool,
    upstream: Upstream,
    handle: SessionHandle,
}

impl Session {
    pub fn start<I: Interceptor>(
        self,
        interceptor: Arc<I>,
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
            events,
            log,
            confirmed: AtomicBool::new(false),
            upstream: self.upstream,
            handle: handle.clone(),
        });
        let (client_read, client_write) = self.client.into_split();
        let (server_read, server_write) = self.server.into_split();
        let (outgoing_tx, outgoing_rx) = mpsc::unbounded_channel();
        let (incoming_tx, incoming_rx) = mpsc::unbounded_channel();

        let mut tasks = JoinSet::new();
        tasks.spawn(read_loop(
            Direction::ToServer,
            client_read,
            outgoing_tx,
            shared.clone(),
        ));
        tasks.spawn(read_loop(
            Direction::ToClient,
            server_read,
            incoming_tx,
            shared.clone(),
        ));
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

async fn read_loop<I: Interceptor>(
    direction: Direction,
    mut reader: OwnedReadHalf,
    writer: mpsc::UnboundedSender<PendingWrite>,
    shared: Arc<Shared<I>>,
) {
    let mut framer = Framer::new();
    let mut next_index = 0u32;
    let mut buffer = vec![0u8; READ_BUFFER_SIZE];
    loop {
        let read = tokio::select! {
            read = reader.read(&mut buffer) => read,
            () = shared.handle.cancel.cancelled() => return,
        };
        let chunk = match read {
            Ok(0) | Err(_) => return,
            Ok(n) => &buffer[..n],
        };
        if !shared.confirmed.load(Ordering::Acquire) {
            let version = (direction == Direction::ToServer)
                .then(|| parse_version_packet(chunk))
                .flatten();
            let Some((hotel_version, client_identifier)) = version else {
                let raw = WriteAction::Raw(Bytes::copy_from_slice(chunk));
                let _ = writer.send(Box::pin(std::future::ready(raw)));
                continue;
            };
            let connection = ConnectionInfo {
                host: shared.upstream.host.clone(),
                port: shared.upstream.port,
                hotel_version,
                client_identifier,
                client_type: "FLASH".to_owned(),
            };
            shared.confirmed.store(true, Ordering::Release);
            shared
                .interceptor
                .on_connected(connection.clone(), shared.handle.clone())
                .await;
            let _ = shared.events.send(SessionEvent::Connected(connection));
        }
        framer.push(chunk);
        while let Some(next) = framer.next_packet() {
            let Ok(packet) = next else {
                shared.handle.abort();
                return;
            };
            let index = next_index;
            next_index = next_index.wrapping_add(1);
            let interceptor = shared.interceptor.clone();
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
            if writer.send(Box::pin(pending)).is_err() {
                return;
            }
        }
    }
}

async fn write_loop<I: Interceptor>(
    direction: Direction,
    mut writer: OwnedWriteHalf,
    mut pending: mpsc::UnboundedReceiver<PendingWrite>,
    mut injections: mpsc::UnboundedReceiver<Injection>,
    shared: Arc<Shared<I>>,
) {
    let mut ordered = FuturesOrdered::new();
    let mut reader_open = true;
    loop {
        let result = tokio::select! {
            biased;
            () = shared.handle.cancel.cancelled() => return,
            Some(action) = ordered.next(), if !ordered.is_empty() => apply(direction, &mut writer, action, &shared).await,
            next = pending.recv(), if reader_open => {
                match next {
                    Some(write) => ordered.push_back(write),
                    None => reader_open = false,
                }
                Ok(())
            }
            Some(injection) = injections.recv() => inject(direction, &mut writer, injection, &shared).await,
        };
        if result.is_err() || (!reader_open && ordered.is_empty()) {
            break;
        }
    }
    let _ = writer.shutdown().await;
}

async fn apply<I>(
    direction: Direction,
    writer: &mut OwnedWriteHalf,
    action: WriteAction,
    shared: &Shared<I>,
) -> io::Result<()> {
    let (index, original, verdict) = match action {
        WriteAction::Raw(bytes) => return writer.write_all(&bytes).await,
        WriteAction::Packet {
            index,
            original,
            verdict,
        } => (index, original, verdict),
    };
    let outcome = match verdict {
        Verdict::Forward => {
            writer.write_all(original.frame()).await?;
            Outcome::Forwarded
        }
        Verdict::Edited { packet, by } => {
            writer.write_all(packet.frame()).await?;
            Outcome::Edited { by, sent: packet }
        }
        Verdict::Blocked { by } => Outcome::Blocked { by },
    };
    record(
        shared,
        LogEntry::Intercepted {
            index,
            direction,
            packet: original,
            outcome,
        },
    );
    Ok(())
}

async fn inject<I>(
    direction: Direction,
    writer: &mut OwnedWriteHalf,
    injection: Injection,
    shared: &Shared<I>,
) -> io::Result<()> {
    writer.write_all(injection.packet.frame()).await?;
    record(
        shared,
        LogEntry::Injected {
            direction,
            packet: injection.packet,
            by: injection.by,
        },
    );
    Ok(())
}

fn record<I>(shared: &Shared<I>, entry: LogEntry) {
    if shared.log.try_send(entry).is_err() {
        shared
            .handle
            .dropped_log_entries
            .fetch_add(1, Ordering::Relaxed);
    }
}
