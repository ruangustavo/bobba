use std::sync::Arc;
use std::time::Duration;

use bobba_extensions::{
    ExtensionHost, HostConfig, HostEvent, PacketInfoLoader, Permission, PermissionRequest,
    SendError, serve,
};
use bobba_protocol::extension_message::{ExtensionInfo, ExtensionToHost, HostInfo};
use bobba_protocol::{Direction, Framer, HMessage, Packet, PacketInfo};
use bobba_session::{DirectProxy, LogEntry, Upstream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

const TIMEOUT: Duration = Duration::from_secs(5);
const SULEK: &str = r#"{"messages":{"incoming":[{"id":2350,"name":"ChatMessageEvent","confident":true}],
    "outgoing":[{"id":597,"name":"ChatMessageComposer","confident":true}]}}"#;

struct FixtureLoader;

impl PacketInfoLoader for FixtureLoader {
    async fn load(&self, hotel_version: &str) -> PacketInfo {
        PacketInfo::from_sulek_json(hotel_version, SULEK).expect("fixture parses")
    }
}

fn version_packet() -> Packet {
    Packet::builder(4000)
        .string("WIN63-1")
        .string("FLASH31")
        .i32(6)
        .i32(4)
        .build()
}

struct World {
    host: Arc<ExtensionHost<FixtureLoader>>,
    extension_port: u16,
    client: TcpStream,
    server: TcpStream,
    log: mpsc::Receiver<LogEntry>,
    events: mpsc::UnboundedReceiver<HostEvent>,
    permissions: mpsc::UnboundedReceiver<PermissionRequest>,
}

async fn world(intercept_timeout: Option<Duration>) -> World {
    let (events_tx, events) = mpsc::unbounded_channel();
    let (permissions_tx, permissions) = mpsc::unbounded_channel();
    let config = HostConfig {
        intercept_timeout,
        host_info: HostInfo {
            packet_logger: "Bobba".into(),
            version: "test".into(),
            attributes: Vec::new(),
        },
    };
    let host = Arc::new(ExtensionHost::new(
        FixtureLoader,
        config,
        events_tx,
        permissions_tx,
    ));
    let extension_listener = TcpListener::bind("127.0.0.1:0").await.expect("binds");
    let extension_port = extension_listener.local_addr().expect("address").port();
    tokio::spawn(serve(extension_listener, host.clone()));

    let hotel = TcpListener::bind("127.0.0.1:0").await.expect("binds");
    let upstream = Upstream {
        host: "127.0.0.1".into(),
        port: hotel.local_addr().expect("address").port(),
    };
    let proxy = DirectProxy::bind("127.0.0.1:0".parse().expect("address"), upstream)
        .await
        .expect("binds");
    let proxy_address = proxy.local_addr().expect("address");
    let (client, session, server) =
        tokio::join!(TcpStream::connect(proxy_address), proxy.accept(), async {
            hotel.accept().await.map(|(s, _)| s)
        });
    let (session_events, _) = mpsc::unbounded_channel();
    let (log_tx, log) = mpsc::channel(1024);
    let (session, _) = session
        .expect("session")
        .start(host.clone(), session_events, log_tx);
    World {
        host,
        extension_port,
        client: client.expect("client"),
        server: server.expect("server"),
        log,
        events,
        permissions,
    }
}

struct FakeExtension {
    stream: TcpStream,
    framer: Framer,
}

impl FakeExtension {
    async fn connect(world: &mut World, title: &str) -> Self {
        let stream = TcpStream::connect(("127.0.0.1", world.extension_port))
            .await
            .expect("connects");
        let mut extension = Self {
            stream,
            framer: Framer::new(),
        };
        assert_eq!(
            extension.next().await.header_id(),
            2,
            "host asks for info first"
        );
        extension
            .send(ExtensionToHost::ExtensionInfo(ExtensionInfo {
                title: title.into(),
                author: "test".into(),
                version: "1".into(),
                description: String::new(),
                on_click_used: false,
                file: None,
                cookie: None,
                can_leave: true,
                can_delete: true,
            }))
            .await;
        let request = tokio::time::timeout(TIMEOUT, world.permissions.recv())
            .await
            .expect("permission asked")
            .expect("channel open");
        let _ = request.respond.send(Permission::Allow);
        assert_eq!(
            extension.next().await.header_id(),
            7,
            "host sends Init after accepting"
        );
        extension
    }

    async fn send(&mut self, message: ExtensionToHost) {
        self.stream
            .write_all(message.encode().frame())
            .await
            .expect("writes");
    }

    async fn next(&mut self) -> Packet {
        let mut buffer = [0u8; 4096];
        loop {
            if let Some(packet) = self.framer.next_packet() {
                return packet.expect("valid frame");
            }
            let n = tokio::time::timeout(TIMEOUT, self.stream.read(&mut buffer))
                .await
                .expect("message arrives")
                .expect("open");
            assert!(n > 0, "host closed the connection");
            self.framer.push(&buffer[..n]);
        }
    }

    async fn next_intercept(&mut self) -> HMessage {
        loop {
            let packet = self.next().await;
            if packet.header_id() == 3 {
                let mut reader = packet.reader();
                return HMessage::parse(reader.read_long_string_bytes().expect("payload"))
                    .expect("valid message");
            }
        }
    }

    async fn answer(&mut self, message: HMessage) {
        self.send(ExtensionToHost::ManipulatedPacket(message)).await;
    }
}

async fn read_packets(stream: &mut TcpStream, count: usize) -> Vec<Packet> {
    let mut framer = Framer::new();
    let mut packets = Vec::new();
    let mut buffer = [0u8; 4096];
    while packets.len() < count {
        let n = tokio::time::timeout(TIMEOUT, stream.read(&mut buffer))
            .await
            .expect("arrives")
            .expect("open");
        assert!(n > 0, "closed");
        framer.push(&buffer[..n]);
        while let Some(p) = framer.next_packet() {
            packets.push(p.expect("valid"));
        }
    }
    packets
}

async fn send(stream: &mut TcpStream, packets: &[Packet]) {
    let bytes: Vec<u8> = packets.iter().flat_map(|p| p.frame().to_vec()).collect();
    stream.write_all(&bytes).await.expect("writes");
}

async fn connect_hotel(world: &mut World, extension: &mut FakeExtension) {
    send(&mut world.client, &[version_packet()]).await;
    let version = extension.next_intercept().await;
    extension.answer(version).await;
    read_packets(&mut world.server, 1).await;
}

#[tokio::test]
async fn extension_can_block_edit_and_forward() {
    let mut w = world(None).await;
    let mut ext = FakeExtension::connect(&mut w, "Editor").await;
    connect_hotel(&mut w, &mut ext).await;

    let spam = Packet::builder(597).string("spam").build();
    let hello = Packet::builder(597).string("hello").build();
    let walk = Packet::builder(3320).i32(1).i32(2).build();
    send(&mut w.client, &[spam, hello, walk.clone()]).await;

    let mut blocked = ext.next_intercept().await;
    blocked.blocked = true;
    ext.answer(blocked).await;
    let mut edited = ext.next_intercept().await;
    edited.packet = Packet::builder(597).string("HELLO").build();
    ext.answer(edited).await;
    let untouched = ext.next_intercept().await;
    ext.answer(untouched).await;

    let received = read_packets(&mut w.server, 2).await;
    assert_eq!(
        received,
        vec![Packet::builder(597).string("HELLO").build(), walk]
    );
}

#[tokio::test]
async fn replies_out_of_order_still_reach_the_server_in_order() {
    let mut w = world(None).await;
    let mut ext = FakeExtension::connect(&mut w, "Async").await;
    connect_hotel(&mut w, &mut ext).await;

    let first = Packet::new(1, b"first");
    let second = Packet::new(2, b"second");
    send(&mut w.client, &[first.clone(), second.clone()]).await;
    let one = ext.next_intercept().await;
    let two = ext.next_intercept().await;
    ext.answer(two).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    ext.answer(one).await;
    assert_eq!(read_packets(&mut w.server, 2).await, vec![first, second]);
}

#[tokio::test]
async fn silent_extension_times_out_and_the_packet_goes_through() {
    let mut w = world(Some(Duration::from_millis(100))).await;
    let mut ext = FakeExtension::connect(&mut w, "Sleepy").await;
    connect_hotel(&mut w, &mut ext).await;

    let packet = Packet::new(5, b"x");
    send(&mut w.client, std::slice::from_ref(&packet)).await;
    assert_eq!(read_packets(&mut w.server, 1).await, vec![packet]);
    let timed_out = loop {
        match tokio::time::timeout(TIMEOUT, w.events.recv())
            .await
            .expect("event")
            .expect("open")
        {
            HostEvent::InterceptTimedOut { waiting_on, .. } => break waiting_on,
            _ => continue,
        }
    };
    assert_eq!(timed_out, vec!["Sleepy".to_owned()]);
}

#[tokio::test]
async fn disconnecting_extension_releases_its_pending_packets() {
    let mut w = world(None).await;
    let mut ext = FakeExtension::connect(&mut w, "Crashy").await;
    connect_hotel(&mut w, &mut ext).await;

    let packet = Packet::new(5, b"x");
    send(&mut w.client, std::slice::from_ref(&packet)).await;
    let _ = ext.next_intercept().await;
    drop(ext);
    assert_eq!(read_packets(&mut w.server, 1).await, vec![packet]);
}

#[tokio::test]
async fn extension_sends_only_headers_already_seen_unless_developer_mode() {
    let mut w = world(None).await;
    let mut ext = FakeExtension::connect(&mut w, "Sender").await;
    connect_hotel(&mut w, &mut ext).await;

    let unseen = Packet::builder(2350).i32(1).string("oi").build();
    ext.send(ExtensionToHost::SendMessage {
        direction: Direction::ToClient,
        packet: unseen.clone(),
    })
    .await;
    let refused = loop {
        match tokio::time::timeout(TIMEOUT, w.events.recv())
            .await
            .expect("event")
            .expect("open")
        {
            HostEvent::SendRefused { reason, .. } => break reason,
            _ => continue,
        }
    };
    assert_eq!(
        refused,
        SendError::Unsafe {
            header_id: 2350,
            direction: Direction::ToClient
        }
    );

    w.host.set_developer_mode(true);
    ext.send(ExtensionToHost::SendMessage {
        direction: Direction::ToClient,
        packet: unseen.clone(),
    })
    .await;
    assert_eq!(read_packets(&mut w.client, 1).await, vec![unseen.clone()]);
    let injected = loop {
        match tokio::time::timeout(TIMEOUT, w.log.recv())
            .await
            .expect("log")
            .expect("open")
        {
            entry @ LogEntry::Injected { .. } => break entry,
            _ => continue,
        }
    };
    assert_eq!(
        injected,
        LogEntry::Injected {
            direction: Direction::ToClient,
            packet: unseen,
            by: "Sender".into()
        }
    );
}

#[tokio::test]
async fn connection_start_carries_sulek_names_and_expressions_resolve() {
    let mut w = world(None).await;
    let mut ext = FakeExtension::connect(&mut w, "Names").await;
    send(&mut w.client, &[version_packet()]).await;
    let start = loop {
        let packet = ext.next().await;
        if packet.header_id() == 5 {
            break packet;
        }
    };
    let mut reader = start.reader();
    assert_eq!(reader.read_string().as_deref(), Ok("127.0.0.1"));
    let _port = reader.read_i32();
    assert_eq!(reader.read_string().as_deref(), Ok("WIN63-1"));
    assert_eq!(reader.read_string().as_deref(), Ok("FLASH31"));
    assert_eq!(reader.read_string().as_deref(), Ok("FLASH"));
    assert_eq!(reader.read_i32(), Ok(2));

    let version = ext.next_intercept().await;
    ext.answer(version).await;
    read_packets(&mut w.server, 1).await;
    let chat = w
        .host
        .parse_expression("{out:Chat}{s:\"oi\"}{i:0}{i:-1}")
        .expect("resolves");
    assert_eq!(
        chat,
        (
            Packet::builder(597).string("oi").i32(0).i32(-1).build(),
            Some(Direction::ToServer)
        )
    );
}

#[tokio::test]
async fn denied_extensions_are_disconnected() {
    let mut w = world(None).await;
    let mut stream = TcpStream::connect(("127.0.0.1", w.extension_port))
        .await
        .expect("connects");
    let info = ExtensionToHost::ExtensionInfo(ExtensionInfo {
        title: "Rogue".into(),
        author: String::new(),
        version: String::new(),
        description: String::new(),
        on_click_used: false,
        file: None,
        cookie: None,
        can_leave: true,
        can_delete: true,
    });
    stream
        .write_all(info.encode().frame())
        .await
        .expect("writes");
    let request = w.permissions.recv().await.expect("asked");
    assert_eq!(request.info.title, "Rogue");
    let _ = request.respond.send(Permission::Deny);
    let mut rest = Vec::new();
    let closed = tokio::time::timeout(TIMEOUT, stream.read_to_end(&mut rest)).await;
    assert!(closed.is_ok(), "host closes the connection");
    assert!(w.host.extensions().is_empty());
}

#[tokio::test]
async fn installed_extension_with_its_cookie_skips_the_prompt() {
    let mut w = world(None).await;
    w.host
        .issue_install_cookie("antispam-x1.py", "secret".into());
    let mut stream = TcpStream::connect(("127.0.0.1", w.extension_port))
        .await
        .expect("connects");
    let info = ExtensionToHost::ExtensionInfo(ExtensionInfo {
        title: "AntiSpam".into(),
        author: String::new(),
        version: String::new(),
        description: String::new(),
        on_click_used: false,
        file: Some("antispam-x1.py".into()),
        cookie: Some("secret".into()),
        can_leave: true,
        can_delete: true,
    });
    stream
        .write_all(info.encode().frame())
        .await
        .expect("writes");
    let connected = loop {
        match tokio::time::timeout(TIMEOUT, w.events.recv())
            .await
            .expect("event")
            .expect("open")
        {
            HostEvent::ExtensionConnected { info, .. } => break info.title,
            _ => continue,
        }
    };
    assert_eq!(connected, "AntiSpam");
    assert!(w.permissions.try_recv().is_err());
}
