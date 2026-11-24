use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use bobba_extensions::{ExtensionHost, HostConfig, HostEvent, PermissionRequest, serve};
use bobba_protocol::extension_message::HostInfo;
use bobba_session::{DirectProxy, LogEntry, SessionEvent, SessionHandle, Upstream};
use bobba_sulek::Sulek;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

pub type Host = ExtensionHost<Sulek>;

const EXTENSION_PORT_START: u16 = 9092;
const EXTENSION_PORT_ATTEMPTS: u16 = 100;
const LOG_BATCH_INTERVAL: Duration = Duration::from_millis(33);
const LOG_CHANNEL_CAPACITY: usize = 65_536;
const INTERCEPT_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectTarget {
    pub listen_port: u16,
    pub hotel: Upstream,
}

impl DirectTarget {
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Option<Self> {
        let args: Vec<String> = args.into_iter().collect();
        let value = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
                .cloned()
        };
        let (host, port) = value("--connect")?
            .rsplit_once(':')
            .map(|(h, p)| (h.to_owned(), p.parse().ok()))?;
        let listen_port = value("--listen")
            .and_then(|p| p.parse().ok())
            .unwrap_or(30000);
        Some(Self {
            listen_port,
            hotel: Upstream { host, port: port? },
        })
    }
}

#[derive(Debug)]
pub enum Command {
    Connect(DirectTarget),
    Abort,
}

#[derive(Debug)]
pub enum UiEvent {
    ExtensionPort(u16),
    Listening(SocketAddr),
    Session(SessionEvent),
    Log(Vec<LogEntry>),
    Host(HostEvent),
    Permission(PermissionRequest),
    Failed(String),
}

#[derive(Clone)]
pub struct Backend {
    pub host: Arc<Host>,
    commands: mpsc::UnboundedSender<Command>,
}

impl Backend {
    pub fn spawn(ui: smol::channel::Sender<UiEvent>) -> Self {
        let (host_events, host_events_rx) = mpsc::unbounded_channel();
        let (permissions, permissions_rx) = mpsc::unbounded_channel();
        let config = HostConfig {
            intercept_timeout: Some(INTERCEPT_TIMEOUT),
            host_info: HostInfo {
                packet_logger: "Bobba".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                attributes: vec![
                    ("theme".into(), "dark".into()),
                    ("language".into(), "en".into()),
                ],
            },
        };
        let host = Arc::new(ExtensionHost::new(
            Sulek::with_default_cache(),
            config,
            host_events,
            permissions,
        ));
        let (commands, commands_rx) = mpsc::unbounded_channel();
        let runtime_host = host.clone();
        std::thread::Builder::new()
            .name("bobba-backend".into())
            .spawn(move || {
                let Ok(runtime) = tokio::runtime::Runtime::new() else {
                    let _ = ui.send_blocking(UiEvent::Failed(
                        "could not start the network runtime".into(),
                    ));
                    return;
                };
                runtime.block_on(run(
                    runtime_host,
                    ui,
                    commands_rx,
                    host_events_rx,
                    permissions_rx,
                ));
            })
            .ok();
        Self { host, commands }
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

async fn bind_extension_port() -> Option<TcpListener> {
    for offset in 0..EXTENSION_PORT_ATTEMPTS {
        let port = EXTENSION_PORT_START + offset;
        if let Ok(listener) = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await {
            return Some(listener);
        }
    }
    None
}

async fn run(
    host: Arc<Host>,
    ui: smol::channel::Sender<UiEvent>,
    mut commands: mpsc::UnboundedReceiver<Command>,
    mut host_events: mpsc::UnboundedReceiver<HostEvent>,
    mut permissions: mpsc::UnboundedReceiver<PermissionRequest>,
) {
    match bind_extension_port().await {
        Some(listener) => {
            if let Ok(address) = listener.local_addr() {
                let _ = ui.send(UiEvent::ExtensionPort(address.port())).await;
            }
            tokio::spawn(serve(listener, host.clone()));
        }
        None => {
            let _ = ui
                .send(UiEvent::Failed("no free port for extensions".into()))
                .await;
        }
    }
    let forward_ui = ui.clone();
    tokio::spawn(async move {
        loop {
            let event = tokio::select! {
                Some(event) = host_events.recv() => UiEvent::Host(event),
                Some(request) = permissions.recv() => UiEvent::Permission(request),
                else => return,
            };
            if forward_ui.send(event).await.is_err() {
                return;
            }
        }
    });

    let mut pending: Option<tokio::task::JoinHandle<()>> = None;
    while let Some(command) = commands.recv().await {
        match command {
            Command::Connect(target) => {
                if let Some(task) = pending.take() {
                    task.abort();
                }
                pending = Some(tokio::spawn(connect(target, host.clone(), ui.clone())));
            }
            Command::Abort => {
                if let Some(task) = pending.take() {
                    task.abort();
                    let _ = ui.send(UiEvent::Session(SessionEvent::Ended)).await;
                }
            }
        }
    }
}

async fn connect(target: DirectTarget, host: Arc<Host>, ui: smol::channel::Sender<UiEvent>) {
    let listen = SocketAddr::from((Ipv4Addr::LOCALHOST, target.listen_port));
    let proxy = match DirectProxy::bind(listen, target.hotel).await {
        Ok(proxy) => proxy,
        Err(error) => {
            let _ = ui
                .send(UiEvent::Failed(format!(
                    "cannot listen on {listen}: {error}"
                )))
                .await;
            return;
        }
    };
    if let Ok(address) = proxy.local_addr() {
        let _ = ui.send(UiEvent::Listening(address)).await;
    }
    let session = match proxy.accept().await {
        Ok(session) => session,
        Err(error) => {
            let _ = ui
                .send(UiEvent::Failed(format!("hotel unreachable: {error}")))
                .await;
            return;
        }
    };
    drop(proxy);
    let (session_events, mut session_events_rx) = mpsc::unbounded_channel();
    let (log, mut log_rx) = mpsc::channel(LOG_CHANNEL_CAPACITY);
    let (handle, finished) = session.start(host, session_events, log);
    let abort_on_drop = AbortOnDrop(handle);
    let mut ticker = tokio::time::interval(LOG_BATCH_INTERVAL);
    let mut batch = Vec::new();
    let mut finished = std::pin::pin!(finished);
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                while let Ok(entry) = log_rx.try_recv() {
                    batch.push(entry);
                }
                if !batch.is_empty() && ui.send(UiEvent::Log(std::mem::take(&mut batch))).await.is_err() {
                    return;
                }
            }
            Some(event) = session_events_rx.recv() => {
                let _ = ui.send(UiEvent::Session(event)).await;
            }
            _ = &mut finished => break,
        }
    }
    while let Ok(entry) = log_rx.try_recv() {
        batch.push(entry);
    }
    if !batch.is_empty() {
        let _ = ui.send(UiEvent::Log(batch)).await;
    }
    while let Ok(event) = session_events_rx.try_recv() {
        let _ = ui.send(UiEvent::Session(event)).await;
    }
    drop(abort_on_drop);
}

struct AbortOnDrop(SessionHandle);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}
