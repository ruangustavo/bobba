use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;

use bobba_extensions::{ExtensionId, HostEvent, Permission, PermissionRequest};
use bobba_protocol::extension_message::ExtensionInfo;
use bobba_protocol::{Direction, PacketInfo};
use bobba_session::{ConnectionInfo, LogEntry, SessionEvent, Upstream};
use gpui_kit::TestSupportExt as _;
use gpui_kit::component::Selectable as _;
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{
    ActiveTheme as _, InteractiveElementExt as _, Sizable as _, TitleBar, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::backend::{AutomaticStep, Backend, Command, DirectTarget, InstalledState, UiEvent};
use crate::injection::{INJECTION_NAME, InjectionPanel};
use crate::logger::{LoggerEvent, LoggerPanel};

const CONSOLE_LIMIT: usize = 500;
const SKULL_PNG: &[u8] = include_bytes!("../assets/skull.png");
const DEFAULT_HOTEL: &str = "game-br.habbo.com";
const DEFAULT_GAME_PORT: &str = "30000";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppTab {
    Connection,
    Logger,
    Injection,
    Extensions,
}

impl AppTab {
    const ALL: [AppTab; 4] = [
        AppTab::Connection,
        AppTab::Logger,
        AppTab::Injection,
        AppTab::Extensions,
    ];

    fn label(self) -> &'static str {
        match self {
            AppTab::Connection => "Connection",
            AppTab::Logger => "Logger",
            AppTab::Injection => "Injection",
            AppTab::Extensions => "Extensions",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionStatus {
    Idle,
    Listening(SocketAddr),
    Connected(ConnectionInfo),
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionMode {
    Automatic,
    Manual,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Traffic {
    pub to_server: u64,
    pub to_client: u64,
}

pub struct BobbaApp {
    backend: Backend,
    tab: AppTab,
    status: ConnectionStatus,
    mode: ConnectionMode,
    automatic: Option<AutomaticStep>,
    packet_info: Option<Arc<PacketInfo>>,
    traffic: Traffic,
    encrypted: bool,
    extension_port: Option<u16>,
    extensions: Vec<(ExtensionId, ExtensionInfo)>,
    installed: Vec<InstalledState>,
    console: VecDeque<String>,
    permission_requests: VecDeque<PermissionRequest>,
    hotel_host: Entity<InputState>,
    hotel_port: Entity<InputState>,
    listen_port: Entity<InputState>,
    logger: Entity<LoggerPanel>,
    injection: Entity<InjectionPanel>,
    _tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl BobbaApp {
    pub fn new(
        backend: Backend,
        events: smol::channel::Receiver<UiEvent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let logger = cx.new(|cx| LoggerPanel::new(window, cx));
        let host = backend.host.clone();
        let injection = cx.new(|cx| InjectionPanel::new(host, window, cx));
        let input = |value: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).default_value(value))
        };
        let hotel_host = input(DEFAULT_HOTEL, window, cx);
        let hotel_port = input(DEFAULT_GAME_PORT, window, cx);
        let listen_port = input(DEFAULT_GAME_PORT, window, cx);
        let pump = cx.spawn_in(window, async move |this, cx| {
            while let Ok(event) = events.recv().await {
                if this
                    .update_in(cx, |this, window, cx| this.handle(event, window, cx))
                    .is_err()
                {
                    return;
                }
            }
        });
        let subscriptions = vec![cx.subscribe_in(
            &logger,
            window,
            |this, _, event: &LoggerEvent, window, cx| match event {
                LoggerEvent::UseInInjection { expression, .. } => {
                    let expression = expression.clone();
                    this.injection
                        .update(cx, |panel, cx| panel.load(expression, window, cx));
                    this.select_tab(AppTab::Injection, cx);
                }
            },
        )];
        Self {
            backend,
            tab: AppTab::Connection,
            status: ConnectionStatus::Idle,
            mode: ConnectionMode::Automatic,
            automatic: None,
            packet_info: None,
            traffic: Traffic::default(),
            encrypted: false,
            extension_port: None,
            extensions: Vec::new(),
            installed: Vec::new(),
            console: VecDeque::new(),
            permission_requests: VecDeque::new(),
            hotel_host,
            hotel_port,
            listen_port,
            logger,
            injection,
            _tasks: vec![pump],
            _subscriptions: subscriptions,
        }
    }

    pub fn status(&self) -> &ConnectionStatus {
        &self.status
    }

    pub fn traffic(&self) -> Traffic {
        self.traffic
    }

    pub fn logger(&self) -> &Entity<LoggerPanel> {
        &self.logger
    }

    pub fn injection(&self) -> &Entity<InjectionPanel> {
        &self.injection
    }

    pub fn tab(&self) -> AppTab {
        self.tab
    }

    pub fn select_tab(&mut self, tab: AppTab, cx: &mut Context<Self>) {
        self.tab = tab;
        cx.notify();
    }

    pub fn handle(&mut self, event: UiEvent, _: &mut Window, cx: &mut Context<Self>) {
        match event {
            UiEvent::ExtensionPort(port) => self.extension_port = Some(port),
            UiEvent::Listening(address) => self.status = ConnectionStatus::Listening(address),
            UiEvent::Session(SessionEvent::Connected(info)) => {
                self.automatic = None;
                self.status = ConnectionStatus::Connected(info);
                self.traffic = Traffic::default();
            }
            UiEvent::Session(SessionEvent::KeyRecovered {
                candidates,
                elapsed,
            }) => {
                self.encrypted = true;
                self.log_console(format!(
                    "Found the client's RC4 key in {} ms ({candidates} candidate states)",
                    elapsed.as_millis()
                ));
            }
            UiEvent::Session(SessionEvent::KeyRecoveryFailed) => {
                self.status = ConnectionStatus::Failed(
                    "Could not find the client's RC4 key, so the connection was closed".into(),
                );
            }
            UiEvent::Session(SessionEvent::Ended) => {
                self.encrypted = false;
                if !matches!(self.status, ConnectionStatus::Failed(_)) {
                    self.status = ConnectionStatus::Idle;
                }
            }
            UiEvent::Log(entries) => self.record(entries, cx),
            UiEvent::Host(event) => self.handle_host(event, cx),
            UiEvent::Permission(request) => self.permission_requests.push_back(request),
            UiEvent::Installed(states) => self.installed = states,
            UiEvent::ExtensionOutput(output) => {
                self.log_console(format!("[{}] {}", output.file_name, output.line))
            }
            UiEvent::Notice(text) => self.log_console(text),
            UiEvent::Automatic(step) => {
                if let AutomaticStep::Failed(reason) = &step {
                    self.status = ConnectionStatus::Failed(reason.clone());
                }
                self.automatic = Some(step);
            }
            UiEvent::Failed(reason) => self.status = ConnectionStatus::Failed(reason),
        }
        cx.notify();
    }

    fn record(&mut self, entries: Vec<LogEntry>, cx: &mut Context<Self>) {
        for entry in &entries {
            match entry.direction() {
                Direction::ToServer => self.traffic.to_server += 1,
                Direction::ToClient => self.traffic.to_client += 1,
            }
        }
        self.logger
            .update(cx, |logger, cx| logger.append(entries, cx));
    }

    fn handle_host(&mut self, event: HostEvent, cx: &mut Context<Self>) {
        match event {
            HostEvent::ExtensionConnected { id, info } => self.extensions.push((id, info)),
            HostEvent::ExtensionDisconnected { id } => self.extensions.retain(|(e, _)| *e != id),
            HostEvent::ConsoleLog { id, text } => {
                let title = self.extension_title(id);
                self.log_console(format!("[{title}] {text}"));
            }
            HostEvent::PacketInfoLoaded(info) => {
                self.logger
                    .update(cx, |logger, cx| logger.set_packet_info(info.clone(), cx));
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

    fn log_console(&mut self, line: String) {
        if self.console.len() >= CONSOLE_LIMIT {
            self.console.pop_front();
        }
        self.console.push_back(line);
    }

    fn connect(&mut self, cx: &mut Context<Self>) {
        let read = |input: &Entity<InputState>| input.read(cx).value().trim().to_owned();
        let hotel_host = read(&self.hotel_host);
        let (Ok(hotel_port), Ok(listen_port)) = (
            read(&self.hotel_port).parse(),
            read(&self.listen_port).parse(),
        ) else {
            self.status =
                ConnectionStatus::Failed("ports must be numbers between 1 and 65535".into());
            cx.notify();
            return;
        };
        self.backend.send(Command::Connect(DirectTarget {
            listen_port,
            hotel: Upstream {
                host: hotel_host,
                port: hotel_port,
            },
            tables: None,
        }));
        self.status = ConnectionStatus::Idle;
        cx.notify();
    }

    pub fn connect_to(
        &mut self,
        target: DirectTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let fields = [
            (&self.hotel_host, target.hotel.host.clone()),
            (&self.hotel_port, target.hotel.port.to_string()),
            (&self.listen_port, target.listen_port.to_string()),
        ];
        for (input, value) in fields {
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        }
        self.backend.send(Command::Connect(target));
        cx.notify();
    }

    fn abort(&mut self, cx: &mut Context<Self>) {
        self.backend.send(Command::Abort);
        self.automatic = None;
        self.status = ConnectionStatus::Idle;
        cx.notify();
    }

    fn answer_permission(&mut self, permission: Permission, cx: &mut Context<Self>) {
        if let Some(request) = self.permission_requests.pop_front() {
            let _ = request.respond.send(permission);
        }
        cx.notify();
    }

    fn status_text(&self) -> String {
        match &self.status {
            ConnectionStatus::Idle => "Not connected".into(),
            ConnectionStatus::Listening(address) => format!("Waiting for the client on {address}"),
            ConnectionStatus::Connected(info) => format!("Connected · {}", info.hotel_version),
            ConnectionStatus::Failed(reason) => reason.clone(),
        }
    }

    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let dot = match self.status {
            ConnectionStatus::Connected(_) => cx.theme().green,
            ConnectionStatus::Listening(_) => cx.theme().yellow,
            ConnectionStatus::Failed(_) => cx.theme().red,
            ConnectionStatus::Idle => cx.theme().muted_foreground,
        };
        let skull = Arc::new(Image::from_bytes(ImageFormat::Png, SKULL_PNG.to_vec()));
        TitleBar::new()
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(img(skull).size(px(18.)).flex_none())
                    .child(
                        TabBar::new("tabs")
                            .segmented()
                            .px_0()
                            .py(px(2.))
                            .bg(cx.theme().title_bar)
                            .selected_index(self.tab.index())
                            .on_click(cx.listener(|this, ix: &usize, _, cx| {
                                let tab =
                                    AppTab::ALL.get(*ix).copied().unwrap_or(AppTab::Connection);
                                this.select_tab(tab, cx);
                            }))
                            .children(AppTab::ALL.map(|tab| Tab::new().label(tab.label()))),
                    ),
            )
            .child(
                h_flex()
                    .mr_4()
                    .gap_2()
                    .items_center()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(div().size_2().rounded_full().bg(dot))
                    .child(
                        div()
                            .id("title-status")
                            .test_support()
                            .child(self.status_text()),
                    ),
            )
    }

    fn render_permission_banner(
        &self,
        request: &PermissionRequest,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let info = &request.info;
        h_flex()
            .id("permission-banner")
            .test_support()
            .gap_3()
            .px_4()
            .py_2()
            .items_center()
            .bg(cx.theme().warning.opacity(0.15))
            .border_b_1()
            .border_color(cx.theme().warning)
            .child(div().flex_1().text_sm().child(format!(
                "“{}” by {} wants to connect and see your traffic.",
                info.title, info.author
            )))
            .child(
                Button::new("permission-allow")
                    .label("Allow")
                    .small()
                    .primary()
                    .on_click(
                        cx.listener(|this, _, _, cx| this.answer_permission(Permission::Allow, cx)),
                    ),
            )
            .child(
                Button::new("permission-always")
                    .label("Always allow")
                    .small()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.answer_permission(Permission::AllowAlways, cx)
                    })),
            )
            .child(
                Button::new("permission-deny")
                    .label("Deny")
                    .small()
                    .danger()
                    .on_click(
                        cx.listener(|this, _, _, cx| this.answer_permission(Permission::Deny, cx)),
                    ),
            )
    }

    fn render_connection(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let modes = ButtonGroup::new("connection-mode")
            .outline()
            .small()
            .child(
                Button::new("mode-automatic")
                    .label("Habbo AIR client")
                    .selected(self.mode == ConnectionMode::Automatic),
            )
            .child(
                Button::new("mode-manual")
                    .label("Manual host and port")
                    .selected(self.mode == ConnectionMode::Manual),
            )
            .on_click(cx.listener(|this, selected: &Vec<usize>, _, cx| {
                this.mode = match selected.first() {
                    Some(1) => ConnectionMode::Manual,
                    _ => ConnectionMode::Automatic,
                };
                cx.notify();
            }));
        v_flex()
            .child(h_flex().px_4().pt_4().child(modes))
            .child(match self.mode {
                ConnectionMode::Automatic => self.render_automatic(cx).into_any_element(),
                ConnectionMode::Manual => self.render_manual(cx).into_any_element(),
            })
    }

    fn render_automatic(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = matches!(self.status, ConnectionStatus::Connected(_))
            || matches!(
                self.automatic,
                Some(AutomaticStep::Working(_) | AutomaticStep::WaitingForClient(_))
            );
        let muted = cx.theme().muted_foreground;
        let step = match &self.automatic {
            None if matches!(self.status, ConnectionStatus::Connected(_)) => self.status_text(),
            None => "Click Connect, then open Habbo from the Habbo Launcher.".to_owned(),
            Some(AutomaticStep::Working(text)) => text.clone(),
            Some(AutomaticStep::WaitingForClient(hotels)) => {
                format!("Now open Habbo from the Habbo Launcher. Waiting on {hotels} hotels…")
            }
            Some(AutomaticStep::NeedsPreparation(_)) => {
                "Habbo.app must be re-signed once so G-MemZ can read its encryption key.".to_owned()
            }
            Some(AutomaticStep::Prepared(original)) => {
                format!(
                    "Habbo.app is ready. The original is kept at {}. Connect again.",
                    original.display()
                )
            }
            Some(AutomaticStep::Failed(reason)) => reason.clone(),
        };
        let prepare = match &self.automatic {
            Some(AutomaticStep::NeedsPreparation(app)) => {
                let app = app.clone();
                Some(
                    Button::new("prepare-client")
                        .label("Re-sign Habbo.app")
                        .primary()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.backend.send(Command::PrepareClient(app.clone()));
                            cx.notify();
                        })),
                )
            }
            _ => None,
        };
        v_flex()
            .p_4()
            .gap_4()
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(if busy {
                        Button::new("abort")
                            .label("Abort")
                            .danger()
                            .on_click(cx.listener(|this, _, _, cx| this.abort(cx)))
                    } else {
                        Button::new("connect-automatic")
                            .label("Connect")
                            .primary()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.status = ConnectionStatus::Idle;
                                this.automatic = Some(AutomaticStep::Working("Starting".into()));
                                this.backend.send(Command::ConnectAutomatically);
                                cx.notify();
                            }))
                    })
                    .children(prepare),
            )
            .child(div().id("automatic-step").test_support().text_sm().child(step))
            .child(
                div().text_xs().text_color(muted).child(
                    "Bobba points the Habbo game servers at itself while the client connects (one administrator prompt), then removes the redirect.",
                ),
            )
    }

    fn render_manual(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let connected = matches!(
            self.status,
            ConnectionStatus::Connected(_) | ConnectionStatus::Listening(_)
        );
        let field =
            |label: &'static str, id: &'static str, input: &Entity<InputState>, width: f32| {
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(label),
                    )
                    .child(div().w(px(width)).child(Input::new(input).id(id)))
            };
        let names = self.packet_info.as_ref().map(|info| {
            if info.is_empty() {
                format!(
                    "No names for {} (Sulek unreachable or unknown build); showing header ids",
                    info.hotel_version()
                )
            } else {
                format!(
                    "{} packet names loaded for {}",
                    info.entries().len(),
                    info.hotel_version()
                )
            }
        });
        v_flex()
            .p_4()
            .gap_4()
            .child(
                h_flex()
                    .gap_3()
                    .items_end()
                    .child(field("Hotel host", "hotel-host", &self.hotel_host, 240.))
                    .child(field("Hotel port", "hotel-port", &self.hotel_port, 90.))
                    .child(field(
                        "Listen on 127.0.0.1 port",
                        "listen-port",
                        &self.listen_port,
                        90.,
                    ))
                    .child(if connected {
                        Button::new("abort")
                            .label("Abort")
                            .danger()
                            .on_click(cx.listener(|this, _, _, cx| this.abort(cx)))
                    } else {
                        Button::new("connect")
                            .label("Connect")
                            .primary()
                            .on_click(cx.listener(|this, _, _, cx| this.connect(cx)))
                    }),
            )
            .child(
                div()
                    .id("connection-status")
                    .test_support()
                    .text_sm()
                    .child(self.status_text()),
            )
            .when_some(names, |this, names| {
                this.child(
                    div()
                        .id("connection-names")
                        .test_support()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(names),
                )
            })
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Point the client at the listen port; Bobba forwards to the hotel."),
            )
    }

    fn install_extension(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Install extension".into()),
        });
        let backend = self.backend.clone();
        cx.spawn(async move |_, _| {
            if let Ok(Ok(Some(paths))) = picked.await {
                for path in paths {
                    backend.send(Command::InstallExtension(path));
                }
            }
        })
        .detach();
    }

    fn render_extensions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let row = |title: String, detail: String, id: ElementId| {
            h_flex()
                .id(id)
                .gap_3()
                .px_3()
                .py_2()
                .items_center()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(div().w(px(220.)).text_sm().truncate().child(title))
                .child(
                    div()
                        .flex_1()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .truncate()
                        .child(detail),
                )
        };
        let port = self.extension_port.map_or_else(
            || "Extension port not open".to_owned(),
            |p| format!("Extensions connect on port {p}"),
        );
        let connected_file = |file: &str| {
            self.extensions
                .iter()
                .find(|(_, info)| info.file.as_deref() == Some(file))
        };
        let installed_rows = self.installed.iter().map(|state| {
            let connected = connected_file(&state.file_name);
            let title =
                connected.map_or_else(|| state.file_name.clone(), |(_, info)| info.title.clone());
            let status = match (connected.is_some(), state.running) {
                (true, _) => "connected",
                (false, true) => "starting",
                (false, false) => "stopped",
            };
            let detail = format!("{:?} · {status} · {}", state.runtime, state.file_name);
            let file = state.file_name.clone();
            let toggle = if state.running {
                let file = file.clone();
                Button::new(SharedString::from(format!("stop-{file}")))
                    .label("Stop")
                    .small()
                    .ghost()
                    .on_click(cx.listener(move |this, _, _, _| {
                        this.backend.send(Command::StopExtension(file.clone()))
                    }))
            } else {
                let file = file.clone();
                Button::new(SharedString::from(format!("start-{file}")))
                    .label("Start")
                    .small()
                    .on_click(cx.listener(move |this, _, _, _| {
                        this.backend.send(Command::StartExtension(file.clone()))
                    }))
            };
            let uninstall = Button::new(SharedString::from(format!("uninstall-{file}")))
                .label("Uninstall")
                .small()
                .ghost()
                .on_click(cx.listener(move |this, _, _, _| {
                    this.backend.send(Command::UninstallExtension(file.clone()))
                }));
            row(
                title,
                detail,
                SharedString::from(format!("installed-{}", state.file_name)).into(),
            )
            .child(toggle)
            .child(uninstall)
        });
        let manual_rows = self
            .extensions
            .iter()
            .filter(|(_, info)| info.file.is_none())
            .map(|(id, info)| {
                let detail = format!(
                    "{} · {} · v{} · started outside Bobba",
                    info.author, info.description, info.version
                );
                let id = *id;
                row(info.title.clone(), detail, ElementId::Integer(id)).on_double_click(
                    cx.listener(move |this, _, _, _| this.backend.host.double_click(id)),
                )
            });
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .px_3()
                    .py_2()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(port),
                    )
                    .child(
                        Button::new("install-extension")
                            .label("Install…")
                            .small()
                            .primary()
                            .on_click(cx.listener(|this, _, _, cx| this.install_extension(cx))),
                    ),
            )
            .child(row(
                "Logger".into(),
                "built-in · observer".into(),
                "ext-logger".into(),
            ))
            .child(row(
                INJECTION_NAME.into(),
                "built-in".into(),
                "ext-injection".into(),
            ))
            .children(installed_rows)
            .children(manual_rows)
            .child(
                v_flex()
                    .id("extension-console")
                    .flex_1()
                    .p_3()
                    .gap_1()
                    .overflow_y_scroll()
                    .text_xs()
                    .font_family("Menlo")
                    .children(self.console.iter().map(|line| div().child(line.clone()))),
            )
    }

    fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let port = self
            .extension_port
            .map_or_else(|| "—".to_owned(), |p| p.to_string());
        h_flex()
            .px_3()
            .h_7()
            .gap_4()
            .items_center()
            .text_xs()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().tab_bar)
            .text_color(cx.theme().muted_foreground)
            .child(
                div()
                    .id("traffic-out")
                    .test_support()
                    .child(format!("OUT {}", self.traffic.to_server)),
            )
            .child(
                div()
                    .id("traffic-in")
                    .test_support()
                    .child(format!("IN {}", self.traffic.to_client)),
            )
            .child(div().child(format!(
                "{} extensions · port {port}",
                self.extensions.len()
            )))
    }
}

impl Render for BobbaApp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.tab {
            AppTab::Connection => self.render_connection(cx).into_any_element(),
            AppTab::Logger => self.logger.clone().into_any_element(),
            AppTab::Injection => self.injection.clone().into_any_element(),
            AppTab::Extensions => self.render_extensions(cx).into_any_element(),
        };
        let banner = self.permission_requests.front().map(|request| {
            self.render_permission_banner(request, cx)
                .into_any_element()
        });
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_title_bar(cx))
            .children(banner)
            .child(div().flex_1().min_h_0().child(content))
            .child(self.render_status_bar(cx))
    }
}
