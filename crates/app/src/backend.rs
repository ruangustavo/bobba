use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bobba_extensions::{
    ExtensionHost, ExtensionOutput, HostConfig, HostEvent, InstalledExtension, Installer,
    PermissionRequest, Runtime, launch, serve,
};
use bobba_intercept::client::{self, ClientState};
use bobba_intercept::{HOTEL_HOSTS, HostsRedirect, HotelListeners, MemoryScan, resolve_hotels};
use bobba_protocol::extension_message::HostInfo;
use bobba_session::{
    DirectProxy, DumpFile, LogEntry, NoTables, SessionEvent, SessionHandle, TableSource, Upstream,
};
use bobba_store::{Catalog, Listing, Origin, Store, StoreError};
use bobba_sulek::Sulek;
use tokio::net::TcpListener;
use tokio::process::Child;
use tokio::sync::mpsc;

use crate::setup::Step;

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
    pub tables: Option<PathBuf>,
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
        let tables = value("--rc4-dump").map(PathBuf::from);
        Some(Self {
            listen_port,
            hotel: Upstream { host, port: port? },
            tables,
        })
    }
}

#[derive(Debug)]
pub enum Command {
    Connect(DirectTarget),
    ConnectAutomatically,
    PrepareClient(PathBuf),
    Abort,
    InstallExtension(PathBuf),
    BrowseStore,
    InstallFromStore(String),
    StartExtension(String),
    StopExtension(String),
    UninstallExtension(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomaticStep {
    Working(Step),
    AwaitingApproval,
    NeedsPreparation(PathBuf),
    Prepared(PathBuf),
    WaitingForClient(usize),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledState {
    pub id: String,
    pub kind: InstalledKind,
    pub running: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstalledKind {
    Single(Runtime),
    Package { title: String, version: String },
}

impl InstalledKind {
    fn of(extension: &InstalledExtension) -> Self {
        match extension {
            InstalledExtension::Single(single) => InstalledKind::Single(single.runtime),
            InstalledExtension::Package(package) => InstalledKind::Package {
                title: package.title.clone(),
                version: package.version.clone(),
            },
        }
    }
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
    Automatic(AutomaticStep),
    Failed(String),
    Catalog(CatalogEvent),
    StoreInstall {
        title: String,
        progress: StoreInstall,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogEvent {
    Loaded { catalog: Catalog, origin: Origin },
    Unreachable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreInstall {
    Downloading,
    Installed,
    Failed(String),
}

enum StoreResult {
    Loaded(Result<(Catalog, Origin), StoreError>),
    Downloaded {
        listing: Listing,
        archive: Result<Vec<u8>, StoreError>,
    },
}

#[derive(Clone)]
pub struct UiSink {
    events: std::sync::mpsc::Sender<UiEvent>,
    wake: rune::Waker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    Delivered,
    Closed,
}

impl Delivery {
    pub fn is_closed(self) -> bool {
        self == Delivery::Closed
    }
}

impl UiSink {
    pub fn new(wake: rune::Waker) -> (Self, std::sync::mpsc::Receiver<UiEvent>) {
        let (events, receiver) = std::sync::mpsc::channel();
        (Self { events, wake }, receiver)
    }

    pub fn emit(&self, event: UiEvent) -> Delivery {
        match self.events.send(event) {
            Ok(()) => {
                (self.wake)();
                Delivery::Delivered
            }
            Err(_) => Delivery::Closed,
        }
    }
}

#[derive(Clone)]
pub struct Backend {
    pub host: Arc<Host>,
    commands: mpsc::UnboundedSender<Command>,
}

impl Backend {
    pub fn spawn(ui: UiSink) -> Self {
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
                    ui.emit(UiEvent::Failed(
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
    ui: UiSink,
    mut commands: mpsc::UnboundedReceiver<Command>,
    mut host_events: mpsc::UnboundedReceiver<HostEvent>,
    mut permissions: mpsc::UnboundedReceiver<PermissionRequest>,
) {
    let extension_port = match bind_extension_port().await {
        Some(listener) => {
            let port = listener.local_addr().ok().map(|address| address.port());
            if let Some(port) = port {
                ui.emit(UiEvent::ExtensionPort(port));
            }
            tokio::spawn(serve(listener, host.clone()));
            port
        }
        None => {
            ui.emit(UiEvent::Failed("no free port for extensions".into()));
            None
        }
    };
    let (output, mut output_rx) = mpsc::unbounded_channel::<ExtensionOutput>();
    let output_ui = ui.clone();
    tokio::spawn(async move {
        while let Some(line) = output_rx.recv().await {
            if output_ui.emit(UiEvent::ExtensionOutput(line)).is_closed() {
                return;
            }
        }
    });
    let (store_results, mut store_rx) = mpsc::unbounded_channel::<StoreResult>();
    let mut runner = Runner {
        installer: Installer::with_default_dir(),
        port: extension_port,
        host: host.clone(),
        output,
        running: HashMap::new(),
        store: Arc::new(Store::with_default_cache()),
        catalog: None,
        store_results,
    };
    for installed in runner.installer.installed() {
        runner.start(installed.id(), &ui).await;
    }
    runner.publish(&ui).await;
    runner.browse();
    let forward_ui = ui.clone();
    tokio::spawn(async move {
        loop {
            let event = tokio::select! {
                Some(event) = host_events.recv() => UiEvent::Host(event),
                Some(request) = permissions.recv() => UiEvent::Permission(request),
                else => return,
            };
            if forward_ui.emit(event).is_closed() {
                return;
            }
        }
    });

    let mut pending: Option<tokio::task::JoinHandle<()>> = None;
    loop {
        let command = tokio::select! {
            Some(command) = commands.recv() => command,
            Some(result) = store_rx.recv() => {
                runner.finish(result, &ui).await;
                runner.publish(&ui).await;
                continue;
            }
            else => return,
        };
        match command {
            Command::Connect(target) => {
                if let Some(task) = pending.take() {
                    task.abort();
                }
                pending = Some(tokio::spawn(connect(target, host.clone(), ui.clone())));
            }
            Command::ConnectAutomatically => {
                if let Some(task) = pending.take() {
                    task.abort();
                }
                pending = Some(tokio::spawn(connect_automatically(
                    host.clone(),
                    ui.clone(),
                )));
            }
            Command::PrepareClient(app) => {
                tokio::spawn(prepare_client(app, ui.clone()));
            }
            Command::InstallExtension(path) => match runner.installer.install(&path) {
                Ok(installed) => runner.start(installed.id(), &ui).await,
                Err(error) => {
                    ui.emit(UiEvent::Notice(error.to_string()));
                }
            },
            Command::BrowseStore => runner.browse(),
            Command::InstallFromStore(title) => runner.download(&title, &ui),
            Command::StartExtension(file_name) => runner.start(&file_name, &ui).await,
            Command::StopExtension(file_name) => runner.stop(&file_name).await,
            Command::UninstallExtension(file_name) => {
                runner.stop(&file_name).await;
                if let Err(error) = runner.installer.uninstall(&file_name) {
                    ui.emit(UiEvent::Notice(format!("could not uninstall: {error}")));
                }
            }
            Command::Abort => {
                if let Some(task) = pending.take() {
                    task.abort();
                    ui.emit(UiEvent::Session(SessionEvent::Ended));
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
    store: Arc<Store>,
    catalog: Option<Catalog>,
    store_results: mpsc::UnboundedSender<StoreResult>,
}

impl Runner {
    fn browse(&self) {
        let store = self.store.clone();
        let results = self.store_results.clone();
        tokio::spawn(async move {
            let _ = results.send(StoreResult::Loaded(store.load().await));
        });
    }

    fn download(&self, title: &str, ui: &UiSink) {
        let Some(listing) = self
            .catalog
            .as_ref()
            .and_then(|catalog| catalog.listing(title))
            .cloned()
        else {
            ui.emit(UiEvent::StoreInstall {
                title: title.to_owned(),
                progress: StoreInstall::Failed("it is not in the store anymore".into()),
            });
            return;
        };
        ui.emit(UiEvent::StoreInstall {
            title: title.to_owned(),
            progress: StoreInstall::Downloading,
        });
        let store = self.store.clone();
        let results = self.store_results.clone();
        tokio::spawn(async move {
            let archive = store.download(&listing.title).await;
            let _ = results.send(StoreResult::Downloaded { listing, archive });
        });
    }

    async fn finish(&mut self, result: StoreResult, ui: &UiSink) {
        match result {
            StoreResult::Loaded(Ok((catalog, origin))) => {
                self.catalog = Some(catalog.clone());
                ui.emit(UiEvent::Catalog(CatalogEvent::Loaded { catalog, origin }));
            }
            StoreResult::Loaded(Err(error)) => {
                ui.emit(UiEvent::Catalog(CatalogEvent::Unreachable(
                    error.to_string(),
                )));
            }
            StoreResult::Downloaded { listing, archive } => {
                let title = listing.title.clone();
                let progress = match self.install_listing(&listing, archive, ui).await {
                    Ok(id) => {
                        self.start(&id, ui).await;
                        StoreInstall::Installed
                    }
                    Err(reason) => StoreInstall::Failed(reason),
                };
                ui.emit(UiEvent::StoreInstall { title, progress });
            }
        }
    }

    async fn install_listing(
        &mut self,
        listing: &Listing,
        archive: Result<Vec<u8>, StoreError>,
        ui: &UiSink,
    ) -> Result<String, String> {
        let archive = archive.map_err(|error| error.to_string())?;
        let older = self.installer.versions_of(&listing.title);
        for package in &older {
            self.stop(&package.folder).await;
        }
        let installed = self.installer.install_package(
            &listing.title,
            &listing.version,
            &listing.command,
            &archive,
        );
        match installed {
            Ok(installed) => Ok(installed.id().to_owned()),
            Err(error) => {
                for package in &older {
                    self.start(&package.folder, ui).await;
                }
                Err(error.to_string())
            }
        }
    }

    async fn start(&mut self, id: &str, ui: &UiSink) {
        if self.is_running(id) {
            return;
        }
        let Some(port) = self.port else { return };
        let Some(installed) = self
            .installer
            .installed()
            .into_iter()
            .find(|e| e.id() == id)
        else {
            return;
        };
        match launch(&installed, port, &self.host, self.output.clone()) {
            Ok(child) => {
                self.running.insert(installed.id().to_owned(), child);
            }
            Err(error) => {
                ui.emit(UiEvent::Notice(format!("could not start {id}: {error}")));
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

    async fn publish(&mut self, ui: &UiSink) {
        let installed = self.installer.installed();
        let states = installed
            .into_iter()
            .map(|e| InstalledState {
                running: self.is_running(e.id()),
                kind: InstalledKind::of(&e),
                id: e.id().to_owned(),
            })
            .collect();
        ui.emit(UiEvent::Installed(states));
    }
}

async fn connect(target: DirectTarget, host: Arc<Host>, ui: UiSink) {
    let listen = SocketAddr::from((Ipv4Addr::LOCALHOST, target.listen_port));
    let proxy = match DirectProxy::bind(listen, target.hotel).await {
        Ok(proxy) => proxy,
        Err(error) => {
            ui.emit(UiEvent::Failed(format!(
                "cannot listen on {listen}: {error}"
            )));
            return;
        }
    };
    if let Ok(address) = proxy.local_addr() {
        ui.emit(UiEvent::Listening(address));
    }
    let session = match proxy.accept().await {
        Ok(session) => session,
        Err(error) => {
            ui.emit(UiEvent::Failed(format!("hotel unreachable: {error}")));
            return;
        }
    };
    drop(proxy);
    let tables: Arc<dyn TableSource> = match target.tables {
        Some(path) => Arc::new(DumpFile { path }),
        None => Arc::new(NoTables),
    };
    run_session(session, host, tables, ui).await;
}

async fn connect_automatically(host: Arc<Host>, ui: UiSink) {
    let step = |step| UiEvent::Automatic(AutomaticStep::Working(step));
    let fail = |text: String| UiEvent::Automatic(AutomaticStep::Failed(text));
    ui.emit(step(Step::Prepare));
    match client::client_state().await {
        ClientState::Ready(_) => {}
        ClientState::NotInstalled => {
            ui.emit(fail(
                "Install the Habbo Launcher and open Habbo once, then try again".into(),
            ));
            return;
        }
        ClientState::NeedsPreparation(app) => {
            ui.emit(UiEvent::Automatic(AutomaticStep::NeedsPreparation(app)));
            return;
        }
    }
    ui.emit(step(Step::Allow));
    let redirects = resolve_hotels(&HOTEL_HOSTS).await;
    if redirects.is_empty() {
        ui.emit(fail(
            "Could not resolve any Habbo game server; check your internet".into(),
        ));
        return;
    }
    ui.emit(UiEvent::Automatic(AutomaticStep::AwaitingApproval));
    let redirect = match HostsRedirect::apply(&redirects).await {
        Ok(redirect) => redirect,
        Err(error) => {
            ui.emit(fail(error.to_string()));
            return;
        }
    };
    let listeners = match HotelListeners::bind(&redirects).await {
        Ok(listeners) => listeners,
        Err(error) => {
            redirect.remove().await;
            ui.emit(fail(format!("could not listen for the client: {error}")));
            return;
        }
    };
    ui.emit(UiEvent::Automatic(AutomaticStep::WaitingForClient(
        redirects.len(),
    )));
    let accepted = listeners.accept_first().await;
    redirect.remove().await;
    let session = match accepted {
        Ok((session, _)) => session,
        Err(error) => {
            ui.emit(fail(format!("hotel unreachable: {error}")));
            return;
        }
    };
    run_session(session, host, Arc::new(MemoryScan), ui).await;
}

async fn prepare_client(app: PathBuf, ui: UiSink) {
    ui.emit(UiEvent::Automatic(AutomaticStep::Working(Step::Prepare)));
    let event = match client::prepare(&app).await {
        Ok(original) => UiEvent::Automatic(AutomaticStep::Prepared(original)),
        Err(error) => UiEvent::Automatic(AutomaticStep::Failed(error.to_string())),
    };
    ui.emit(event);
}

async fn run_session(
    session: bobba_session::Session,
    host: Arc<Host>,
    tables: Arc<dyn TableSource>,
    ui: UiSink,
) {
    let (session_events, mut session_events_rx) = mpsc::unbounded_channel();
    let (log, mut log_rx) = mpsc::channel(LOG_CHANNEL_CAPACITY);
    let (handle, finished) = session.start(host, tables, session_events, log);
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
                if !batch.is_empty() && ui.emit(UiEvent::Log(std::mem::take(&mut batch))).is_closed() {
                    return;
                }
            }
            Some(event) = session_events_rx.recv() => {
                ui.emit(UiEvent::Session(event));
            }
            _ = &mut finished => break,
        }
    }
    while let Ok(entry) = log_rx.try_recv() {
        batch.push(entry);
    }
    if !batch.is_empty() {
        ui.emit(UiEvent::Log(batch));
    }
    while let Ok(event) = session_events_rx.try_recv() {
        ui.emit(UiEvent::Session(event));
    }
    drop(abort_on_drop);
}

struct AbortOnDrop(SessionHandle);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}
