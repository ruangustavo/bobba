use std::sync::{Arc, Mutex};
use std::time::Duration;

use bobba_protocol::key_recovery::SBox;
use bobba_protocol::{Direction, Framer, HMessage, Packet, Rc4};
use bobba_session::table_source::TablesFuture;
use bobba_session::{
    ConnectionInfo, DirectProxy, Interceptor, KeyRecoveryFailure, LogEntry, Outcome, SessionEvent,
    SessionHandle, TableSource, TablesUnavailable, Upstream, Verdict,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

const TIMEOUT: Duration = Duration::from_secs(5);
const KEY: &[u8] = b"diffie-hellman shared secret bytes";

struct ClientMemory(Arc<Mutex<Option<Rc4>>>);

impl TableSource for ClientMemory {
    fn tables(&self) -> TablesFuture<'_> {
        let table: Option<SBox> = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(|cipher| *cipher.state());
        Box::pin(std::future::ready(Ok(table.into_iter().collect())))
    }
}

struct UnreadableMemory;

impl TableSource for UnreadableMemory {
    fn tables(&self) -> TablesFuture<'_> {
        Box::pin(std::future::ready(Err(TablesUnavailable(
            "Habbo isn't running".into(),
        ))))
    }
}

struct BlockShout;

impl Interceptor for BlockShout {
    async fn on_connected(&self, _: ConnectionInfo, _: SessionHandle) {}

    async fn intercept(&self, message: HMessage) -> Verdict {
        if message.direction == Direction::ToServer && message.packet.header_id() == 481 {
            Verdict::Blocked { by: "test".into() }
        } else {
            Verdict::Forward
        }
    }

    async fn on_disconnected(&self) {}
}

fn hex_block(len: usize) -> String {
    "ab".repeat(len / 2)
}

struct Peer {
    stream: TcpStream,
    framer: Framer,
    decrypt: Option<Rc4>,
}

impl Peer {
    fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            framer: Framer::new(),
            decrypt: None,
        }
    }

    async fn next(&mut self) -> Packet {
        let mut buffer = [0u8; 4096];
        loop {
            if let Some(packet) = self.framer.next_packet() {
                return packet.expect("valid frame");
            }
            let n = tokio::time::timeout(TIMEOUT, self.stream.read(&mut buffer))
                .await
                .expect("data arrives")
                .expect("open");
            assert!(n > 0, "peer closed");
            let mut data = buffer[..n].to_vec();
            if let Some(cipher) = &mut self.decrypt {
                cipher.apply(&mut data);
            }
            self.framer.push(&data);
        }
    }
}

async fn write_encrypted(
    stream: &mut TcpStream,
    cipher: &Arc<Mutex<Option<Rc4>>>,
    packet: &Packet,
) {
    let mut bytes = packet.frame().to_vec();
    if let Some(cipher) = cipher
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_mut()
    {
        cipher.apply(&mut bytes);
    }
    stream.write_all(&bytes).await.expect("writes");
}

struct Handshaken {
    client: Peer,
    hotel: Peer,
    client_cipher: Arc<Mutex<Option<Rc4>>>,
    handle: SessionHandle,
    events: mpsc::UnboundedReceiver<SessionEvent>,
    log: mpsc::Receiver<LogEntry>,
}

async fn handshake(server_encrypts: bool) -> Handshaken {
    handshake_reading(server_encrypts, |cipher| Arc::new(ClientMemory(cipher))).await
}

async fn handshake_reading(
    server_encrypts: bool,
    memory: impl FnOnce(Arc<Mutex<Option<Rc4>>>) -> Arc<dyn TableSource>,
) -> Handshaken {
    let hotel_listener = TcpListener::bind("127.0.0.1:0").await.expect("binds");
    let upstream = Upstream {
        host: "127.0.0.1".into(),
        port: hotel_listener.local_addr().expect("address").port(),
    };
    let proxy = DirectProxy::bind("127.0.0.1:0".parse().expect("address"), upstream)
        .await
        .expect("binds");
    let address = proxy.local_addr().expect("address");
    let (client, session, hotel) =
        tokio::join!(TcpStream::connect(address), proxy.accept(), async {
            hotel_listener.accept().await.map(|(s, _)| s)
        });
    let client_cipher = Arc::new(Mutex::new(None));
    let (events_tx, events) = mpsc::unbounded_channel();
    let (log_tx, log) = mpsc::channel(1024);
    let (handle, _) = session.expect("session").start(
        Arc::new(BlockShout),
        memory(client_cipher.clone()),
        events_tx,
        log_tx,
    );
    let mut client = Peer::new(client.expect("client"));
    let mut hotel = Peer::new(hotel.expect("hotel"));

    let hello = Packet::builder(4000)
        .string("MAC63-1")
        .string("FLASH31")
        .i32(5)
        .i32(4)
        .build();
    client
        .stream
        .write_all(hello.frame())
        .await
        .expect("writes");
    client
        .stream
        .write_all(Packet::new(1309, &[]).frame())
        .await
        .expect("writes");
    assert_eq!(hotel.next().await, hello);
    assert_eq!(hotel.next().await.header_id(), 1309);

    let init = Packet::builder(3480)
        .string(&hex_block(512))
        .string(&hex_block(512))
        .build();
    hotel.stream.write_all(init.frame()).await.expect("writes");
    assert_eq!(client.next().await, init);

    let complete_request = Packet::builder(3176).string(&hex_block(512)).build();
    client
        .stream
        .write_all(complete_request.frame())
        .await
        .expect("writes");
    assert_eq!(hotel.next().await, complete_request);

    let complete = Packet::builder(2377)
        .string(&hex_block(512))
        .bool(server_encrypts)
        .build();
    hotel
        .stream
        .write_all(complete.frame())
        .await
        .expect("writes");
    assert_eq!(client.next().await, complete);

    *client_cipher
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Rc4::new(KEY);
    hotel.decrypt = Rc4::new(KEY);
    Handshaken {
        client,
        hotel,
        client_cipher,
        handle,
        events,
        log,
    }
}

#[tokio::test]
async fn recovers_the_key_and_keeps_both_ciphers_in_step() {
    let mut h = handshake(false).await;
    let login = Packet::builder(3226).string("sso-ticket").i32(1).build();
    let walk = Packet::builder(3320).i32(3).i32(4).build();
    let shout = Packet::builder(481)
        .string("blocked by the proxy")
        .i32(0)
        .build();
    let chat = Packet::builder(597)
        .string("after the block")
        .i32(0)
        .i32(-1)
        .build();
    for packet in [&login, &walk, &shout] {
        write_encrypted(&mut h.client.stream, &h.client_cipher, packet).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    write_encrypted(&mut h.client.stream, &h.client_cipher, &chat).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let latency = Packet::builder(295).i32(1).build();
    write_encrypted(&mut h.client.stream, &h.client_cipher, &latency).await;

    assert_eq!(h.hotel.next().await, login);
    assert_eq!(h.hotel.next().await, walk);
    assert_eq!(
        h.hotel.next().await,
        chat,
        "the blocked shout did not desync the server's cipher"
    );
    assert_eq!(h.hotel.next().await, latency);

    let injected = Packet::builder(597)
        .string("injected")
        .i32(0)
        .i32(-1)
        .build();
    h.handle
        .inject(Direction::ToServer, injected.clone(), "test")
        .expect("open");
    assert_eq!(h.hotel.next().await, injected);

    let recovered = loop {
        match tokio::time::timeout(TIMEOUT, h.events.recv())
            .await
            .expect("event")
            .expect("open")
        {
            SessionEvent::KeyRecovered { candidates, .. } => break candidates,
            _ => continue,
        }
    };
    assert!(recovered >= 1);
    let blocked = loop {
        match tokio::time::timeout(TIMEOUT, h.log.recv())
            .await
            .expect("log")
            .expect("open")
        {
            LogEntry::Intercepted {
                packet,
                outcome: Outcome::Blocked { .. },
                ..
            } => break packet,
            _ => continue,
        }
    };
    assert_eq!(blocked, shout, "the log shows the decrypted packet");
}

#[tokio::test]
async fn server_side_encryption_reuses_the_same_fresh_key() {
    let mut h = handshake(true).await;
    let mut server_cipher = Rc4::new(KEY).expect("key");
    let welcome = Packet::builder(2350)
        .i32(7)
        .string("bem-vindo")
        .i32(0)
        .i32(0)
        .i32(-1)
        .build();
    let mut bytes = welcome.frame().to_vec();
    server_cipher.apply(&mut bytes);
    h.hotel.stream.write_all(&bytes).await.expect("writes");

    let login = Packet::builder(3226).string("sso-ticket").i32(1).build();
    let ping = Packet::builder(2596).build();
    let pong = Packet::builder(10).build();
    for packet in [&login, &ping, &pong] {
        write_encrypted(&mut h.client.stream, &h.client_cipher, packet).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(h.hotel.next().await, login);

    h.client.decrypt = Rc4::new(KEY);
    assert_eq!(h.client.next().await, welcome);
}

#[tokio::test]
async fn unreadable_memory_fails_recovery_with_the_reason() {
    let mut h = handshake_reading(false, |_| Arc::new(UnreadableMemory)).await;
    let login = Packet::builder(3226).string("sso-ticket").i32(1).build();
    let walk = Packet::builder(3320).i32(3).i32(4).build();
    for packet in [&login, &walk, &login] {
        write_encrypted(&mut h.client.stream, &h.client_cipher, packet).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let failure = loop {
        match tokio::time::timeout(TIMEOUT, h.events.recv())
            .await
            .expect("event")
            .expect("open")
        {
            SessionEvent::KeyRecoveryFailed(failure) => break failure,
            _ => continue,
        }
    };
    assert_eq!(
        failure,
        KeyRecoveryFailure::TablesUnavailable(TablesUnavailable("Habbo isn't running".into()))
    );
}
