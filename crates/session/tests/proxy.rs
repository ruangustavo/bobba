use std::sync::Arc;
use std::time::Duration;

use bobba_protocol::{Direction, Framer, HMessage, Packet};
use bobba_session::{
    ConnectionInfo, DirectProxy, Interceptor, LogEntry, Outcome, PassThrough, SessionEvent,
    SessionHandle, Upstream, Verdict,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

const TIMEOUT: Duration = Duration::from_secs(5);

fn version_packet() -> Packet {
    Packet::builder(4000)
        .string("WIN63-202611041140-327405090")
        .string("FLASH31")
        .i32(6)
        .i32(4)
        .build()
}

struct Harness {
    client: TcpStream,
    server: TcpStream,
    handle: SessionHandle,
    events: mpsc::UnboundedReceiver<SessionEvent>,
    log: mpsc::Receiver<LogEntry>,
}

async fn connect<I: Interceptor>(interceptor: I) -> Harness {
    let hotel = TcpListener::bind("127.0.0.1:0").await.expect("hotel binds");
    let upstream = Upstream {
        host: "127.0.0.1".into(),
        port: hotel.local_addr().expect("hotel address").port(),
    };
    let proxy = DirectProxy::bind("127.0.0.1:0".parse().expect("valid address"), upstream)
        .await
        .expect("proxy binds");
    let proxy_address = proxy.local_addr().expect("proxy address");
    let (client, session, server) =
        tokio::join!(TcpStream::connect(proxy_address), proxy.accept(), async {
            hotel.accept().await.map(|(stream, _)| stream)
        },);
    let (events_tx, events) = mpsc::unbounded_channel();
    let (log_tx, log) = mpsc::channel(1024);
    let (handle, _) = session.expect("session starts").start(
        Arc::new(interceptor),
        Arc::new(bobba_session::NoTables),
        events_tx,
        log_tx,
    );
    Harness {
        client: client.expect("client connects"),
        server: server.expect("hotel accepts"),
        handle,
        events,
        log,
    }
}

async fn read_packets(stream: &mut TcpStream, count: usize) -> Vec<Packet> {
    let mut framer = Framer::new();
    let mut packets = Vec::new();
    let mut buffer = [0u8; 4096];
    while packets.len() < count {
        let n = tokio::time::timeout(TIMEOUT, stream.read(&mut buffer))
            .await
            .expect("packets arrive in time")
            .expect("stream stays open");
        assert!(n > 0, "stream closed after {} packets", packets.len());
        framer.push(&buffer[..n]);
        while let Some(packet) = framer.next_packet() {
            packets.push(packet.expect("valid frame"));
        }
    }
    packets
}

async fn send(stream: &mut TcpStream, packets: &[Packet]) {
    let bytes: Vec<u8> = packets.iter().flat_map(|p| p.frame().to_vec()).collect();
    stream.write_all(&bytes).await.expect("write succeeds");
}

async fn next_log(log: &mut mpsc::Receiver<LogEntry>) -> LogEntry {
    tokio::time::timeout(TIMEOUT, log.recv())
        .await
        .expect("log entry arrives")
        .expect("log open")
}

#[tokio::test]
async fn forwards_both_directions_and_reports_the_connection() {
    let mut h = connect(PassThrough).await;
    let chat = Packet::builder(597).string("oi").i32(0).i32(-1).build();
    send(&mut h.client, &[version_packet(), chat.clone()]).await;
    assert_eq!(
        read_packets(&mut h.server, 2).await,
        vec![version_packet(), chat]
    );

    let reply = Packet::builder(2350).i32(1).string("oi").build();
    send(&mut h.server, std::slice::from_ref(&reply)).await;
    assert_eq!(read_packets(&mut h.client, 1).await, vec![reply]);

    let event = h.events.recv().await.expect("connected event");
    let SessionEvent::Connected(ConnectionInfo {
        hotel_version,
        client_identifier,
        ..
    }) = event
    else {
        panic!("expected Connected, got {event:?}");
    };
    assert_eq!(
        (hotel_version.as_str(), client_identifier.as_str()),
        ("WIN63-202611041140-327405090", "FLASH31")
    );
}

struct SlowFirstPacket;

impl Interceptor for SlowFirstPacket {
    async fn on_connected(&self, _: ConnectionInfo, _: SessionHandle) {}

    async fn intercept(&self, message: HMessage) -> Verdict {
        if message.direction == Direction::ToServer && message.index == 1 {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        Verdict::Forward
    }

    async fn on_disconnected(&self) {}
}

#[tokio::test]
async fn keeps_packet_order_when_an_earlier_decision_is_slower() {
    let mut h = connect(SlowFirstPacket).await;
    let first = Packet::new(1, b"first");
    let second = Packet::new(2, b"second");
    send(
        &mut h.client,
        &[version_packet(), first.clone(), second.clone()],
    )
    .await;
    assert_eq!(
        read_packets(&mut h.server, 3).await,
        vec![version_packet(), first, second]
    );
}

struct BlockHeader(u16);

impl Interceptor for BlockHeader {
    async fn on_connected(&self, _: ConnectionInfo, _: SessionHandle) {}

    async fn intercept(&self, message: HMessage) -> Verdict {
        if message.packet.header_id() == self.0 {
            Verdict::Blocked {
                by: "AntiSpam".into(),
            }
        } else {
            Verdict::Forward
        }
    }

    async fn on_disconnected(&self) {}
}

#[tokio::test]
async fn blocked_packets_never_reach_the_server_and_are_logged() {
    let mut h = connect(BlockHeader(597)).await;
    let spam = Packet::builder(597).string("spam").build();
    let walk = Packet::builder(3320).i32(5).i32(7).build();
    send(
        &mut h.client,
        &[version_packet(), spam.clone(), walk.clone()],
    )
    .await;
    assert_eq!(
        read_packets(&mut h.server, 2).await,
        vec![version_packet(), walk]
    );

    let _version = next_log(&mut h.log).await;
    assert_eq!(
        next_log(&mut h.log).await,
        LogEntry::Intercepted {
            index: 1,
            direction: Direction::ToServer,
            packet: spam,
            outcome: Outcome::Blocked {
                by: "AntiSpam".into()
            },
        }
    );
}

#[tokio::test]
async fn injected_packets_reach_their_destination_and_are_logged() {
    let mut h = connect(PassThrough).await;
    send(&mut h.client, &[version_packet()]).await;
    read_packets(&mut h.server, 1).await;

    let whisper = Packet::builder(1543).string("oi").build();
    h.handle
        .inject(Direction::ToClient, whisper.clone(), "Injection")
        .expect("session is open");
    assert_eq!(read_packets(&mut h.client, 1).await, vec![whisper.clone()]);

    let _version = next_log(&mut h.log).await;
    assert_eq!(
        next_log(&mut h.log).await,
        LogEntry::Injected {
            direction: Direction::ToClient,
            packet: whisper,
            by: "Injection".into(),
        }
    );
}

#[tokio::test]
async fn bytes_before_the_version_packet_pass_through_untouched() {
    let mut h = connect(BlockHeader(4000)).await;
    let policy_request = b"<policy-file-request/>\0".repeat(10);
    h.client
        .write_all(&policy_request)
        .await
        .expect("write succeeds");
    let mut received = vec![0u8; policy_request.len()];
    tokio::time::timeout(TIMEOUT, h.server.read_exact(&mut received))
        .await
        .expect("bytes arrive")
        .expect("stream open");
    assert_eq!(received, policy_request);
}

#[tokio::test]
async fn closing_one_side_ends_the_session() {
    let mut h = connect(PassThrough).await;
    drop(h.client);
    let mut buffer = [0u8; 1];
    let read = tokio::time::timeout(TIMEOUT, h.server.read(&mut buffer))
        .await
        .expect("server notices");
    assert_eq!(read.expect("clean close"), 0);
    let ended = tokio::time::timeout(TIMEOUT, h.events.recv())
        .await
        .expect("event arrives");
    assert_eq!(ended, Some(SessionEvent::Ended));
}
