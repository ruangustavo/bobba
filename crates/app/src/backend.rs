use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bobba_extensions::{
    ExtensionHost, ExtensionOutput, HostConfig, HostEvent, Installer, PermissionRequest, Runtime,
    launch, serve,
};
use bobba_protocol::extension_message::HostInfo;
use bobba_session::{DirectProxy, LogEntry, SessionEvent, SessionHandle, Upstream};
use bobba_sulek::Sulek;
use tokio::net::TcpListener;
use tokio::process::Child;
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
    InstallExtension(PathBuf),
    StartExtension(String),
    StopExtension(String),
    UninstallExtension(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledState {
    pub file_name: String,
    pub runtime: Runtime,
    pub running: bool,
}

#[derive(Debug)]
pub enum UiEvent {
    ExtensionPort(u16),
    Listening(SocketAddr),
    Session(SessionEvent),
    Log(Vec<LogEntry>),
    Host(HostEvent),
    Permission(PermissionRequest),
    Installed(Vec<InstalledState>),
    ExtensionOutput(ExtensionOutput),
    Notice(String),
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
    let extension_port = match bind_extension_port().await {
        Some(listener) => {
            let port = listener.local_addr().ok().map(|address| address.port());
            if let Some(port) = port {
                let _ = ui.send(UiEvent::ExtensionPort(port)).await;
            }
            tokio::spawn(serve(listener, host.clone()));
            port
        }
        None => {
            let _ = ui
                .send(UiEvent::Failed("no free port for extensions".into()))
                .await;
            None
        }
    };
    let (output, mut output_rx) = mpsc::unbounded_channel::<ExtensionOutput>();
    let output_ui = ui.clone();
    tokio::spawn(async move {
        while let Some(line) = output_rx.recv().await {
            if output_ui
                .send(UiEvent::ExtensionOutput(line))
                .await
                .is_err()
            {
                return;
            }
        }
    });
    let mut runner = Runner {
        installer: Installer::with_default_dir(),
        port: extension_port,
        host: host.clone(),
        output,
        running: HashMap::new(),
    };
    for installed in runner.installer.installed() {
        runner.start(&installed.file_name, &ui).await;
    }
    runner.publish(&ui).await;
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
            Command::InstallExtension(path) => match runner.installer.install(&path) {
                Ok(installed) => runner.start(&installed.file_name, &ui).await,
                Err(error) => {
                    let _ = ui.send(UiEvent::Notice(error.to_string())).await;
                }
            },
            Command::StartExtension(file_name) => runner.start(&file_name, &ui).await,
            Command::StopExtension(file_name) => runner.stop(&file_name).await,
            Command::UninstallExtension(file_name) => {
                runner.stop(&file_name).await;
                if let Err(error) = runner.installer.uninstall(&file_name) {
                    let _ = ui
                        .send(UiEvent::Notice(format!("could not uninstall: {error}")))
                        .await;
                }
            }
            Command::Abort => {
                if let Some(task) = pending.take() {
                    task.abort();
                    let _ = ui.send(UiEvent::Session(SessionEvent::Ended)).await;
                }
            }
        }
        runner.publish(&ui).await;
    }
}

struct Runner {
    installer: Installer,
    port: Option<u16>,
    host: Arc<Host>,
    output: mpsc::UnboundedSender<ExtensionOutput>,
    running: HashMap<String, Child>,
}

impl Runner {
    async fn start(&mut self, file_name: &str, ui: &smol::channel::Sender<UiEvent>) {
        if self.is_running(file_name) {
            return;
        }
        let Some(port) = self.port else { return };
        let Some(installed) = self
            .installer
            .installed()
            .into_iter()
            .find(|e| e.file_name == file_name)
        else {
            return;
        };
        match launch(&installed, port, &self.host, self.output.clone()) {
            Ok(child) => {
                self.running.insert(installed.file_name, child);
            }
            Err(error) => {
                let _ = ui
                    .send(UiEvent::Notice(format!(
                        "could not start {file_name}: {error}"
                    )))
                    .await;
            }
        }
    }

    async fn stop(&mut self, file_name: &str) {
        if let Some(mut child) = self.running.remove(file_name) {
            let _ = child.kill().await;
        }
    }

    fn is_running(&mut self, file_name: &str) -> bool {
        self.running
            .get_mut(file_name)
            .is_some_and(|child| matches!(child.try_wait(), Ok(None)))
    }

    async fn publish(&mut self, ui: &smol::channel::Sender<UiEvent>) {
        let installed = self.installer.installed();
        let states = installed
            .into_iter()
            .map(|e| InstalledState {
                running: self.is_running(&e.file_name),
                file_name: e.file_name,
                runtime: e.runtime,
            })
            .collect();
        let _ = ui.send(UiEvent::Installed(states)).await;
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
    let (handle, finished) =
        session.start(host, Arc::new(bobba_session::NoTables), session_events, log);
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
