use std::sync::Arc;

use bobba_extensions::{ExtensionHost, HostConfig, PacketInfoLoader};
use bobba_protocol::PacketInfo;
use bobba_protocol::extension_message::HostInfo;
use bobba_session::{DirectProxy, NoTables, Upstream};
use tokio::sync::mpsc;

struct NoNames;

impl PacketInfoLoader for NoNames {
    async fn load(&self, hotel_version: &str) -> PacketInfo {
        PacketInfo::empty(hotel_version)
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut args = std::env::args()
        .skip(1)
        .filter_map(|a| a.parse::<u16>().ok());
    let listen = args.next().unwrap_or(31002);
    let hotel = args.next().unwrap_or(31000);
    let (events, _events_rx) = mpsc::unbounded_channel();
    let (permissions, _permissions_rx) = mpsc::unbounded_channel();
    let config = HostConfig {
        intercept_timeout: None,
        host_info: HostInfo {
            packet_logger: "Bobba".into(),
            version: "bench".into(),
            attributes: Vec::new(),
        },
    };
    let host = Arc::new(ExtensionHost::new(NoNames, config, events, permissions));
    let proxy = DirectProxy::bind(
        ([127, 0, 0, 1], listen).into(),
        Upstream {
            host: "127.0.0.1".into(),
            port: hotel,
        },
    )
    .await?;
    println!("bobba proxy listening on {listen}");
    loop {
        let session = proxy.accept().await?;
        let (session_events, mut session_events_rx) = mpsc::unbounded_channel();
        let (log, mut log_rx) = mpsc::channel(65_536);
        let _ = session.start(host.clone(), Arc::new(NoTables), session_events, log);
        tokio::spawn(async move { while session_events_rx.recv().await.is_some() {} });
        tokio::spawn(async move { while log_rx.recv().await.is_some() {} });
    }
}
