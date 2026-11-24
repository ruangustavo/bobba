use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use bobba_extensions::{ExtensionId, HostEvent, Permission, PermissionRequest};
use bobba_protocol::extension_message::ExtensionInfo;
use bobba_protocol::{Direction, PacketInfo};
use bobba_session::{ConnectionInfo, KeyRecoveryFailure, LogEntry, SessionEvent, Upstream};
use rune::TextEdit;
use rune_kit::TableState;

use crate::backend::{AutomaticStep, Backend, Command, DirectTarget, InstalledState, UiEvent};
use crate::packet_log::{DEFAULT_CAPACITY, Filter, LogRow, PacketLog};

const CONSOLE_LIMIT: usize = 500;
pub const DEFAULT_HOTEL: &str = "game-br.habbo.com";
pub const DEFAULT_GAME_PORT: &str = "30000";
pub const INJECTION_NAME: &str = "Injection";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Connection,
    Logger,
    Injection,
    Extensions,
}

impl Tab {
    pub const ALL: [Tab; 4] = [
        Tab::Connection,
        Tab::Logger,
        Tab::Injection,
        Tab::Extensions,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Connection => "Connection",
            Tab::Logger => "Logger",
            Tab::Injection => "Injection",
            Tab::Extensions => "Extensions",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
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
    NeedsResign(PathBuf),
    Prepared(PathBuf),
    AwaitingApproval,
    WaitingForClient(usize),
    Listening(SocketAddr),
    Connected(ConnectionInfo),
    Ended { packets: usize },
    Failed(String),
}

impl Phase {
    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            Phase::Preparing(_)
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
    pub key: KeyState,
    pub packet_info: Option<Arc<PacketInfo>>,
    pub traffic: Traffic,
    pub extension_port: Option<u16>,
    pub extensions: Vec<(ExtensionId, ExtensionInfo)>,
    pub installed: Vec<InstalledState>,
    pub console: VecDeque<String>,
    pub permission_requests: VecDeque<PermissionRequest>,
    pub hotel_host: TextEdit,
    pub hotel_port: TextEdit,
    pub listen_port: TextEdit,
    pub log: PacketLog,
    pub table: TableState,
    pub filter: TextEdit,
    pub follow: bool,
    pub paused: bool,
    pub injection: TextEdit,
    pub last_send: Option<SendResult>,
}

impl App {
    pub fn new() -> Self {
        Self {
            backend: None,
            events: None,
            tab: Tab::Connection,
            mode: Mode::Automatic,
            phase: Phase::Idle,
            key: KeyState::Plain,
            packet_info: None,
            traffic: Traffic::default(),
            extension_port: None,
            extensions: Vec::new(),
            installed: Vec::new(),
            console: VecDeque::new(),
            permission_requests: VecDeque::new(),
            hotel_host: TextEdit::new(DEFAULT_HOTEL),
            hotel_port: TextEdit::new(DEFAULT_GAME_PORT),
            listen_port: TextEdit::new(DEFAULT_GAME_PORT),
            log: PacketLog::new(DEFAULT_CAPACITY),
            table: TableState::new().key("packets"),
            filter: TextEdit::new(""),
            follow: true,
            paused: false,
            injection: TextEdit::new(""),
            last_send: None,
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
                self.log_console(format!("[{}] {}", output.file_name, output.line))
            }
            UiEvent::Notice(text) => self.log_console(text),
            UiEvent::Automatic(step) => self.handle_step(step),
            UiEvent::Failed(reason) => self.phase = Phase::Failed(reason),
        }
    }

    fn handle_step(&mut self, step: AutomaticStep) {
        self.phase = match step {
            AutomaticStep::Working(text) => Phase::Preparing(text),
            AutomaticStep::AwaitingApproval => Phase::AwaitingApproval,
            AutomaticStep::NeedsPreparation(app) => Phase::NeedsResign(app),
            AutomaticStep::Prepared(original) => Phase::Prepared(original),
            AutomaticStep::WaitingForClient(hotels) => Phase::WaitingForClient(hotels),
            AutomaticStep::Failed(reason) => Phase::Failed(reason),
        };
    }

    fn handle_session(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::Connected(info) => {
                self.phase = Phase::Connected(info);
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
                if matches!(self.phase, Phase::Connected(_)) {
                    self.phase = Phase::Ended {
                        packets: self.log.total(),
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
        if !self.paused {
            self.log.push(entries);
        }
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
                self.log.set_packet_info(info.clone());
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
                self.phase = Phase::Preparing("Starting".into());
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
        self.phase = Phase::Preparing("Re-signing Habbo.app".into());
        self.send(Command::PrepareClient(app));
    }

    pub fn answer_permission(&mut self, permission: Permission) {
        if let Some(request) = self.permission_requests.pop_front() {
            let _ = request.respond.send(permission);
        }
    }

    pub fn set_filter(&mut self) {
        self.log.set_filter(Filter::new(&self.filter.text));
    }

    pub fn clear_log(&mut self) {
        self.log.clear();
        self.table.clear_selection();
    }

    pub fn selected_row(&self) -> Option<&LogRow> {
        let number: u64 = self.table.cursor()?.parse().ok()?;
        self.log.row_by_number(number)
    }

    pub fn use_in_injection(&mut self, expression: String) {
        self.injection.replace_text(expression);
        self.last_send = None;
        self.tab = Tab::Injection;
    }

    pub fn send_injection(&mut self, direction: Direction) {
        let Some(backend) = &self.backend else {
            self.last_send = Some(SendResult::Refused("not connected to a hotel".into()));
            return;
        };
        self.last_send = Some(
            match backend.host.send_expression(
                INJECTION_NAME,
                direction,
                self.injection.text.trim(),
            ) {
                Ok(()) => SendResult::Sent(direction),
                Err(error) => SendResult::Refused(error.to_string()),
            },
        );
    }

    pub fn status_line(&self) -> String {
        match &self.phase {
            Phase::Idle => "Not connected".into(),
            Phase::Preparing(text) => text.clone(),
            Phase::NeedsResign(_) => "Action needed".into(),
            Phase::Prepared(_) => "Client ready".into(),
            Phase::AwaitingApproval => "Waiting for approval".into(),
            Phase::WaitingForClient(_) => "Waiting for Habbo".into(),
            Phase::Listening(address) => format!("Listening on {address}"),
            Phase::Connected(info) => format!("Connected · {}", info.hotel_version),
            Phase::Ended { .. } => "Session ended".into(),
            Phase::Failed(_) => "Connection failed".into(),
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
