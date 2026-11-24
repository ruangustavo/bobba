use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use bobba_extensions::{ExtensionId, HostEvent, Permission, PermissionRequest};
use bobba_protocol::extension_message::ExtensionInfo;
use bobba_protocol::{Direction, Expression, PacketInfo};
use bobba_session::{ConnectionInfo, KeyRecoveryFailure, LogEntry, SessionEvent, Upstream};
use rune::{TextEdit, WindowHandle};

use crate::backend::{AutomaticStep, Backend, Command, DirectTarget, InstalledState, UiEvent};
use crate::log_feed::LogFeed;
use crate::recent::{self, Recent};
use crate::repeat::{Interval, Repeats};
use crate::setup::{Progress, Step};

use crate::store::{CatalogState, Showing, StoreState};

const CONSOLE_LIMIT: usize = 500;
pub const DEFAULT_HOTEL: &str = "game-br.habbo.com";
pub const DEFAULT_GAME_PORT: &str = "30000";
pub const INJECTION_NAME: &str = "Injection";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GEarthOffer {
    None,
    Found { folder: PathBuf, count: usize },
    Importing,
    Dismissed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Connection,
    Injection,
    Extensions,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Connection, Tab::Injection, Tab::Extensions];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Connection => "Connection",
            Tab::Injection => "Injection",
            Tab::Extensions => "Extensions",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionField {
    Hotel,
    Client,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Automatic,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Preparing(String),
    Working(Step),
    NeedsResign(PathBuf),
    Prepared(PathBuf),
    AwaitingApproval,
    WaitingForClient(usize),
    Listening(SocketAddr),
    Connected(ConnectionInfo),
    Ended { packets: usize },
    SetupFailed { at: Step, reason: String },
    Failed(String),
}

impl Phase {
    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            Phase::Preparing(_)
                | Phase::Working(_)
                | Phase::AwaitingApproval
                | Phase::WaitingForClient(_)
                | Phase::Listening(_)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Plain,
    Decrypted {
        candidates: usize,
        elapsed: Duration,
    },
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Traffic {
    pub to_server: u64,
    pub to_client: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Draft {
    Empty,
    Broken(String),
    Ready,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendMode {
    Once,
    Repeat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Named(Direction),
    Raw(Direction),
}

impl Target {
    pub fn direction(self) -> Direction {
        match self {
            Target::Named(direction) | Target::Raw(direction) => direction,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendResult {
    Sent(Direction),
    Refused(String),
}

pub struct App {
    pub backend: Option<Backend>,
    pub events: Option<Receiver<UiEvent>>,
    pub tab: Tab,
    pub mode: Mode,
    pub phase: Phase,
    pub copied: Option<SessionField>,
    pub key: KeyState,
    pub packet_info: Option<Arc<PacketInfo>>,
    pub traffic: Traffic,
    pub extension_port: Option<u16>,
    pub extensions: Vec<(ExtensionId, ExtensionInfo)>,
    pub installed: Vec<InstalledState>,
    pub console: VecDeque<String>,
    pub last_output: HashMap<String, String>,
    pub confirm_uninstall: Option<String>,
    pub permission_requests: VecDeque<PermissionRequest>,
    pub hotel_host: TextEdit,
    pub hotel_port: TextEdit,
    pub listen_port: TextEdit,
    pub feed: LogFeed,
    logger: Option<WindowHandle>,
    pub injection: TextEdit,
    pub last_send: Option<SendResult>,
    pub recent: Recent,
    pub recent_open: bool,
    pub send_mode: SendMode,
    pub every: Interval,
    pub raw_direction: Direction,
    pub repeats: Repeats,
    pub store: StoreState,
    pub gearth: GEarthOffer,
}

impl App {
    pub fn new() -> Self {
        Self {
            backend: None,
            events: None,
            tab: Tab::Connection,
            mode: Mode::Automatic,
            phase: Phase::Idle,
            copied: None,
            key: KeyState::Plain,
            packet_info: None,
            traffic: Traffic::default(),
            extension_port: None,
            extensions: Vec::new(),
            installed: Vec::new(),
            console: VecDeque::new(),
            last_output: HashMap::new(),
            confirm_uninstall: None,
            permission_requests: VecDeque::new(),
            hotel_host: TextEdit::new(DEFAULT_HOTEL),
            hotel_port: TextEdit::new(DEFAULT_GAME_PORT),
            listen_port: TextEdit::new(DEFAULT_GAME_PORT),
            feed: LogFeed::new(),
            logger: None,
            injection: TextEdit::new(""),
            last_send: None,
            recent: Recent::default(),
            recent_open: false,
            send_mode: SendMode::Once,
            every: Interval::default(),
            raw_direction: Direction::ToServer,
            repeats: Repeats::default(),
            store: StoreState::default(),
            gearth: GEarthOffer::None,
        }
    }

    pub fn attach(&mut self, backend: Backend, events: Receiver<UiEvent>) {
        self.backend = Some(backend);
        self.events = Some(events);
    }

    pub fn listen(&mut self, events: Receiver<UiEvent>) {
        self.events = Some(events);
    }

    pub fn drain_events(&mut self) -> bool {
        let Some(events) = self.events.take() else {
            return false;
        };
        let mut changed = false;
        while let Ok(event) = events.try_recv() {
            self.handle(event);
            changed = true;
        }
        self.events = Some(events);
        changed
    }

    pub fn open_extension(&self, id: ExtensionId) {
        if let Some(backend) = &self.backend {
            backend.host.double_click(id);
        }
    }

    pub fn start_extension(&mut self, id: String) {
        self.last_output.remove(&id);
        self.send(Command::StartExtension(id));
    }

    pub fn send(&self, command: Command) {
        if let Some(backend) = &self.backend {
            backend.send(command);
        }
    }

    pub fn handle(&mut self, event: UiEvent) {
        match event {
            UiEvent::ExtensionPort(port) => self.extension_port = Some(port),
            UiEvent::Listening(address) => self.phase = Phase::Listening(address),
            UiEvent::Session(event) => self.handle_session(event),
            UiEvent::Log(entries) => self.record(entries),
            UiEvent::Host(event) => self.handle_host(event),
            UiEvent::Permission(request) => self.permission_requests.push_back(request),
            UiEvent::Installed(states) => self.installed = states,
            UiEvent::ExtensionOutput(output) => {
                self.log_console(format!("[{}] {}", output.file_name, output.line));
                self.last_output.insert(output.file_name, output.line);
            }
            UiEvent::Notice(text) => self.log_console(text),
            UiEvent::Automatic(step) => self.handle_step(step),
            UiEvent::Failed(reason) => self.phase = Phase::Failed(reason),
            UiEvent::Catalog(event) => self.store.handle_catalog(event),
            UiEvent::MissingRuntimes(missing) => self.store.handle_missing(missing),
            UiEvent::StoreInstall { title, progress } => self.store.handle_install(title, progress),
            UiEvent::GEarthFound { folder, count } => {
                if self.gearth == GEarthOffer::None {
                    self.gearth = GEarthOffer::Found { folder, count };
                }
            }
            UiEvent::GEarthImported(result) => {
                self.gearth = GEarthOffer::Dismissed;
                self.log_console(match result {
                    Ok(count) => format!("Imported {count} extensions from G-Earth"),
                    Err(reason) => format!("Could not import from G-Earth: {reason}"),
                });
            }
        }
    }

    fn handle_step(&mut self, step: AutomaticStep) {
        self.phase = match step {
            AutomaticStep::Working(step) => Phase::Working(step),
            AutomaticStep::AwaitingApproval => Phase::AwaitingApproval,
            AutomaticStep::NeedsPreparation(app) => Phase::NeedsResign(app),
            AutomaticStep::Prepared(original) => Phase::Prepared(original),
            AutomaticStep::WaitingForClient(hotels) => Phase::WaitingForClient(hotels),
            AutomaticStep::Failed(reason) => Phase::SetupFailed {
                at: Progress::of(&self.phase).step().unwrap_or(Step::Prepare),
                reason,
            },
        };
    }

    fn handle_session(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::Connected(info) => {
                self.phase = Phase::Connected(info);
                self.copied = None;
                self.traffic = Traffic::default();
                self.key = KeyState::Plain;
            }
            SessionEvent::KeyRecovered {
                candidates,
                elapsed,
            } => {
                self.key = KeyState::Decrypted {
                    candidates,
                    elapsed,
                };
            }
            SessionEvent::KeyRecoveryFailed(KeyRecoveryFailure::TablesUnavailable(reason)) => {
                self.phase = Phase::Failed(format!("Couldn't read Habbo's memory: {reason}"));
            }
            SessionEvent::KeyRecoveryFailed(KeyRecoveryFailure::NoMatchingKey) => {
                self.phase = Phase::Failed("Could not find Habbo's encryption key".into());
            }
            SessionEvent::Ended => {
                self.key = KeyState::Plain;
                self.repeats.pause_all();
                if matches!(self.phase, Phase::Connected(_)) {
                    self.phase = Phase::Ended {
                        packets: self.feed.log().total(),
                    };
                } else if !matches!(self.phase, Phase::Failed(_)) {
                    self.phase = Phase::Idle;
                }
            }
        }
    }

    fn record(&mut self, entries: Vec<LogEntry>) {
        for entry in &entries {
            match entry.direction() {
                Direction::ToServer => self.traffic.to_server += 1,
                Direction::ToClient => self.traffic.to_client += 1,
            }
        }
        self.feed.record(entries);
    }

    fn handle_host(&mut self, event: HostEvent) {
        match event {
            HostEvent::ExtensionConnected { id, info } => self.extensions.push((id, info)),
            HostEvent::ExtensionDisconnected { id } => self.extensions.retain(|(e, _)| *e != id),
            HostEvent::ConsoleLog { id, text } => {
                let title = self.extension_title(id);
                self.log_console(format!("[{title}] {text}"));
            }
            HostEvent::PacketInfoLoaded(info) => {
                self.feed.log_mut().set_packet_info(info.clone());
                self.packet_info = Some(info);
            }
            HostEvent::InterceptTimedOut {
                direction,
                index,
                waiting_on,
            } => self.log_console(format!(
                "Packet #{index} {direction:?} went through after waiting too long for {}",
                waiting_on.join(", ")
            )),
            HostEvent::SendRefused { by, reason } => {
                self.log_console(format!("[{by}] send refused: {reason}"))
            }
        }
    }

    fn extension_title(&self, id: ExtensionId) -> String {
        self.extensions
            .iter()
            .find(|(e, _)| *e == id)
            .map_or_else(|| format!("extension {id}"), |(_, info)| info.title.clone())
    }

    pub fn log_console(&mut self, line: String) {
        if self.console.len() >= CONSOLE_LIMIT {
            self.console.pop_front();
        }
        self.console.push_back(line);
    }

    pub fn connect(&mut self) {
        match self.mode {
            Mode::Automatic => {
                self.phase = Phase::Working(Step::Prepare);
                self.send(Command::ConnectAutomatically);
            }
            Mode::Manual => {
                let host = self.hotel_host.text.trim().to_owned();
                let ports = (
                    self.hotel_port.text.trim().parse(),
                    self.listen_port.text.trim().parse(),
                );
                let (Ok(port), Ok(listen_port)) = ports else {
                    self.phase = Phase::Failed("Ports must be numbers between 1 and 65535".into());
                    return;
                };
                self.phase = Phase::Preparing(format!("Opening 127.0.0.1:{listen_port}"));
                self.send(Command::Connect(DirectTarget {
                    listen_port,
                    hotel: Upstream { host, port },
                    tables: None,
                }));
            }
        }
    }

    pub fn connect_to(&mut self, target: DirectTarget) {
        self.mode = Mode::Manual;
        self.hotel_host.replace_text(target.hotel.host.clone());
        self.hotel_port.replace_text(target.hotel.port.to_string());
        self.listen_port
            .replace_text(target.listen_port.to_string());
        self.phase = Phase::Preparing(format!("Opening 127.0.0.1:{}", target.listen_port));
        self.send(Command::Connect(target));
    }

    pub fn abort(&mut self) {
        self.send(Command::Abort);
        self.phase = Phase::Idle;
    }

    pub fn resign(&mut self, app: PathBuf) {
        self.phase = Phase::Working(Step::Prepare);
        self.send(Command::PrepareClient(app));
    }

    pub fn answer_permission(&mut self, permission: Permission) {
        if let Some(request) = self.permission_requests.pop_front() {
            let _ = request.respond.send(permission);
        }
    }

    pub fn show(&mut self, showing: Showing) {
        self.store.showing = showing;
        if showing == Showing::Store {
            self.load_store();
            self.check_runtimes();
        }
    }

    pub fn check_runtimes(&self) {
        if self.store.misses_runtimes() {
            self.send(Command::CheckRuntimes);
        }
    }

    pub fn load_store(&mut self) {
        if self.store.needs_catalog() {
            self.store.catalog = CatalogState::Loading;
            self.send(Command::BrowseStore);
        }
    }

    pub fn import_from_gearth(&mut self, folder: PathBuf) {
        self.gearth = GEarthOffer::Importing;
        self.send(Command::ImportFromGEarth(folder));
    }

    pub fn toggle_store_detail(&mut self, title: &str) {
        self.store.detail = if self.store.detail.as_deref() == Some(title) {
            None
        } else {
            Some(title.to_owned())
        };
    }

    pub fn install_from_store(&mut self, title: String) {
        self.send(Command::InstallFromStore(title));
    }

    pub fn open_logger(&mut self) {
        match &self.logger {
            Some(window) if window.is_ready() => window.focus(),
            _ => self.logger = Some(crate::logger::open(&self.feed)),
        }
    }

    pub fn take_logger_requests(&mut self) -> bool {
        let Some(expression) = self.feed.take_injection() else {
            return false;
        };
        self.use_in_injection(expression);
        true
    }

    pub fn use_in_injection(&mut self, expression: String) {
        self.injection.replace_text(expression);
        self.last_send = None;
        self.tab = Tab::Injection;
    }

    pub fn draft(&self) -> Draft {
        let text = self.injection.text.trim();
        if text.is_empty() {
            return Draft::Empty;
        }
        let empty = PacketInfo::empty("");
        let info = self.packet_info.as_deref().unwrap_or(&empty);
        match Expression::parse(text).and_then(|expression| expression.resolve(info)) {
            Ok(_) => Draft::Ready,
            Err(error) => Draft::Broken(error.to_string()),
        }
    }

    pub fn target(&self) -> Option<Target> {
        if self.draft() != Draft::Ready {
            return None;
        }
        let expression = Expression::parse(self.injection.text.trim()).ok()?;
        Some(match expression.direction() {
            Some(direction) => Target::Named(direction),
            None => Target::Raw(self.raw_direction),
        })
    }

    fn deliver(&self, direction: Direction, expression: &str) -> Result<(), String> {
        let Some(backend) = &self.backend else {
            return Err("not connected to a hotel".into());
        };
        backend
            .host
            .send_expression(INJECTION_NAME, direction, expression)
            .map_err(|error| error.to_string())
    }

    pub fn send_injection(&mut self) {
        let Some(target) = self.target() else {
            return;
        };
        let direction = target.direction();
        let expression = self.injection.text.trim().to_owned();
        self.last_send = Some(match self.deliver(direction, &expression) {
            Ok(()) => {
                self.recent
                    .remember(direction, &expression, self.packet_info.as_deref());
                SendResult::Sent(direction)
            }
            Err(reason) => SendResult::Refused(reason),
        });
    }

    pub fn start_repeat(&mut self, now: f64) {
        let Some(target) = self.target() else {
            return;
        };
        let direction = target.direction();
        let expression = self.injection.text.trim().to_owned();
        let label = recent::label(direction, &expression, self.packet_info.as_deref());
        self.recent
            .remember(direction, &expression, self.packet_info.as_deref());
        self.repeats
            .start(label, expression, direction, self.every, now);
        self.last_send = None;
        self.fire_repeats(now);
    }

    pub fn fire_repeats(&mut self, now: f64) -> bool {
        let due = self.repeats.due(now);
        let fired = !due.is_empty();
        for packet in due {
            match self.deliver(packet.direction, &packet.expression) {
                Ok(()) => self.repeats.sent(packet.id),
                Err(reason) => self.repeats.stop(packet.id, reason),
            }
        }
        fired
    }

    pub fn toggle_repeat(&mut self, id: u64, now: f64) {
        self.repeats.toggle(id, now);
        self.fire_repeats(now);
    }

    pub fn edit_repeat(&mut self, id: u64) {
        if let Some(repeat) = self.repeats.get(id) {
            let expression = repeat.expression.clone();
            self.injection.replace_text(expression);
            self.last_send = None;
        }
    }

    pub fn use_recent(&mut self, expression: String) {
        self.injection.replace_text(expression);
        self.last_send = None;
        self.recent_open = false;
    }

    pub fn status_line(&self) -> String {
        match &self.phase {
            Phase::Idle => "Not connected".into(),
            Phase::Preparing(text) => text.clone(),
            Phase::Working(Step::Prepare) => "Preparing Habbo".into(),
            Phase::Working(Step::Allow) => "Getting permission".into(),
            Phase::Working(Step::Open) => "Waiting for Habbo".into(),
            Phase::NeedsResign(_) => "Action needed".into(),
            Phase::Prepared(_) => "Client ready".into(),
            Phase::AwaitingApproval => "Waiting for approval".into(),
            Phase::WaitingForClient(_) => "Waiting for Habbo".into(),
            Phase::Listening(address) => format!("Listening on {address}"),
            Phase::Connected(_) => "Connected".into(),
            Phase::Ended { .. } => "Session ended".into(),
            Phase::SetupFailed { .. } => "Setup stopped".into(),
            Phase::Failed(_) => "Connection failed".into(),
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
