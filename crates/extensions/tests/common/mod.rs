#![allow(dead_code)]
use std::sync::Arc;
use std::time::Duration;

use bobba_extensions::{
    ExtensionHost, HostConfig, HostEvent, PacketInfoLoader, Permission, PermissionRequest, serve,
};
use bobba_protocol::extension_message::{ExtensionInfo, ExtensionToHost, HostInfo};
use bobba_protocol::{Framer, HMessage, Packet, PacketInfo};
use bobba_session::{DirectProxy, LogEntry, Upstream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

pub const TIMEOUT: Duration = Duration::from_secs(5);
const SULEK: &str = r#"{"messages":{"incoming":[{"id":2350,"name":"ChatMessageEvent","confident":true}],
    "outgoing":[{"id":597,"name":"ChatMessageComposer","confident":true}]}}"#;

pub struct FixtureLoader;

impl PacketInfoLoader for FixtureLoader {
    async fn load(&self, hotel_version: &str) -> PacketInfo {
        PacketInfo::from_sulek_json(hotel_version, SULEK).expect("fixture parses")
    }
}

pub fn version_packet() -> Packet {
    Packet::builder(4000)
        .string("WIN63-1")
        .string("FLASH31")
        .i32(6)
        .i32(4)
        .build()
}

pub struct World {
    pub host: Arc<ExtensionHost<FixtureLoader>>,
    pub extension_port: u16,
    pub client: TcpStream,
    pub server: TcpStream,
    pub log: mpsc::Receiver<LogEntry>,
    pub events: mpsc::UnboundedReceiver<HostEvent>,
    pub permissions: mpsc::UnboundedReceiver<PermissionRequest>,
}

pub async fn world(intercept_timeout: Option<Duration>) -> World {
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
    let _ = session
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

pub struct FakeExtension {
    stream: TcpStream,
    framer: Framer,
}

impl FakeExtension {
    pub async fn connect(world: &mut World, title: &str) -> Self {
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

    pub async fn send(&mut self, message: ExtensionToHost) {
        self.stream
            .write_all(message.encode().frame())
            .await
            .expect("writes");
    }

    pub async fn next(&mut self) -> Packet {
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

    pub async fn next_intercept(&mut self) -> HMessage {
        loop {
            let packet = self.next().await;
            if packet.header_id() == 3 {
                let mut reader = packet.reader();
                return HMessage::parse(reader.read_long_string_bytes().expect("payload"))
                    .expect("valid message");
            }
        }
    }

    pub async fn answer(&mut self, message: HMessage) {
        self.send(ExtensionToHost::ManipulatedPacket(message)).await;
    }
}

pub async fn read_packets(stream: &mut TcpStream, count: usize) -> Vec<Packet> {
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

pub async fn send(stream: &mut TcpStream, packets: &[Packet]) {
    let bytes: Vec<u8> = packets.iter().flat_map(|p| p.frame().to_vec()).collect();
    stream.write_all(&bytes).await.expect("writes");
}

pub async fn connect_hotel(world: &mut World, extension: &mut FakeExtension) {
    send(&mut world.client, &[version_packet()]).await;
    let version = extension.next_intercept().await;
    extension.answer(version).await;
    read_packets(&mut world.server, 1).await;
}
