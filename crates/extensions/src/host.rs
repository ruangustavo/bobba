use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use bobba_protocol::expression::HeaderLabel;
use bobba_protocol::extension_message::{ExtensionInfo, HostInfo, HostToExtension};
use bobba_protocol::prediction::predicted_expression;
use bobba_protocol::{Direction, Expression, ExpressionError, HMessage, Packet, PacketInfo};
use bobba_session::{ConnectionInfo, Interceptor, SessionHandle, Verdict};
use tokio::sync::{mpsc, oneshot};

pub type ExtensionId = u64;

const EXPRESSION_SIZE_LIMIT: usize = 3000;

pub trait PacketInfoLoader: Send + Sync + 'static {
    fn load(&self, hotel_version: &str) -> impl Future<Output = PacketInfo> + Send;
}

#[derive(Debug, Clone)]
pub enum HostEvent {
    ExtensionConnected {
        id: ExtensionId,
        info: ExtensionInfo,
    },
    ExtensionDisconnected {
        id: ExtensionId,
    },
    ConsoleLog {
        id: ExtensionId,
        text: String,
    },
    PacketInfoLoaded(Arc<PacketInfo>),
    InterceptTimedOut {
        direction: Direction,
        index: u32,
        waiting_on: Vec<String>,
    },
    SendRefused {
        by: String,
        reason: SendError,
    },
}

#[derive(Debug)]
pub struct PermissionRequest {
    pub info: ExtensionInfo,
    pub respond: oneshot::Sender<Permission>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    Allow,
    AllowAlways,
    Deny,
}

#[derive(Debug, Clone)]
pub struct HostConfig {
    pub intercept_timeout: Option<Duration>,
    pub host_info: HostInfo,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum SendError {
    #[error("not connected to a hotel")]
    NotConnected,
    #[error(
        "header {header_id} has not been seen going {direction:?} yet; enable developer mode to send it"
    )]
    Unsafe {
        header_id: u16,
        direction: Direction,
    },
    #[error(transparent)]
    Expression(#[from] ExpressionError),
    #[error("{name} only exists going {exists:?}")]
    WrongDirection { name: String, exists: Direction },
}

struct Connected {
    info: ExtensionInfo,
    outbox: mpsc::UnboundedSender<HostToExtension>,
}

struct Pending {
    original: Packet,
    waiting: BTreeSet<ExtensionId>,
    blocked_by: Option<String>,
    edited: Option<(Packet, String)>,
    done: oneshot::Sender<Verdict>,
}

impl Pending {
    fn verdict(blocked_by: Option<String>, edited: Option<(Packet, String)>) -> Verdict {
        match (blocked_by, edited) {
            (Some(by), _) => Verdict::Blocked { by },
            (None, Some((packet, by))) => Verdict::Edited { packet, by },
            (None, None) => Verdict::Forward,
        }
    }

    fn finish(self) {
        let _ = self.done.send(Self::verdict(self.blocked_by, self.edited));
    }
}

struct State {
    next_id: ExtensionId,
    extensions: BTreeMap<ExtensionId, Connected>,
    pending: HashMap<(Direction, u32), Pending>,
    connection: Option<(ConnectionInfo, SessionHandle)>,
    packet_info: Arc<PacketInfo>,
    seen_headers: HashSet<(Direction, u16)>,
    install_cookies: HashMap<String, String>,
    remembered_permission: bool,
}

pub struct ExtensionHost<L> {
    loader: L,
    config: HostConfig,
    events: mpsc::UnboundedSender<HostEvent>,
    permissions: mpsc::UnboundedSender<PermissionRequest>,
    developer_mode: AtomicBool,
    state: Mutex<State>,
}

impl<L: PacketInfoLoader> ExtensionHost<L> {
    pub fn new(
        loader: L,
        config: HostConfig,
        events: mpsc::UnboundedSender<HostEvent>,
        permissions: mpsc::UnboundedSender<PermissionRequest>,
    ) -> Self {
        Self {
            loader,
            config,
            events,
            permissions,
            developer_mode: AtomicBool::new(false),
            state: Mutex::new(State {
                next_id: 1,
                extensions: BTreeMap::new(),
                pending: HashMap::new(),
                connection: None,
                packet_info: Arc::new(PacketInfo::empty("")),
                seen_headers: HashSet::new(),
                install_cookies: HashMap::new(),
                remembered_permission: false,
            }),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn emit(&self, event: HostEvent) {
        let _ = self.events.send(event);
    }

    pub fn set_developer_mode(&self, enabled: bool) {
        self.developer_mode.store(enabled, Ordering::Relaxed);
    }

    pub fn packet_info(&self) -> Arc<PacketInfo> {
        self.state().packet_info.clone()
    }

    pub fn is_connected(&self) -> bool {
        self.state().connection.is_some()
    }

    pub fn extensions(&self) -> Vec<(ExtensionId, ExtensionInfo)> {
        self.state()
            .extensions
            .iter()
            .map(|(id, c)| (*id, c.info.clone()))
            .collect()
    }

    pub fn issue_install_cookie(&self, file_name: &str, cookie: String) {
        self.state()
            .install_cookies
            .insert(file_name.to_owned(), cookie);
    }

    pub fn double_click(&self, id: ExtensionId) {
        self.send_to(id, HostToExtension::OnDoubleClick);
    }

    fn send_to(&self, id: ExtensionId, message: HostToExtension) {
        if let Some(extension) = self.state().extensions.get(&id) {
            let _ = extension.outbox.send(message);
        }
    }

    pub fn parse_expression(&self, text: &str) -> Result<(Packet, Option<Direction>), SendError> {
        let expression = Expression::parse(text)?;
        let direction = expression.direction();
        let packet = expression.resolve(&self.packet_info())?;
        Ok((packet, direction))
    }

    pub fn send_expression(
        &self,
        by: &str,
        direction: Direction,
        text: &str,
    ) -> Result<(), SendError> {
        let (packet, named_direction) = self.parse_expression(text)?;
        if let Some(exists) = named_direction.filter(|d| *d != direction) {
            return Err(SendError::WrongDirection {
                name: text.to_owned(),
                exists,
            });
        }
        self.send(by, direction, packet)
    }

    pub fn send(&self, by: &str, direction: Direction, packet: Packet) -> Result<(), SendError> {
        let state = self.state();
        let Some((_, session)) = &state.connection else {
            return Err(SendError::NotConnected);
        };
        let header_id = packet.header_id();
        let developer_mode = self.developer_mode.load(Ordering::Relaxed);
        if !developer_mode && !state.seen_headers.contains(&(direction, header_id)) {
            return Err(SendError::Unsafe {
                header_id,
                direction,
            });
        }
        session
            .inject(direction, packet, by)
            .map_err(|_| SendError::NotConnected)
    }

    pub(crate) fn send_from_extension(
        &self,
        id: ExtensionId,
        direction: Direction,
        packet: Packet,
    ) {
        let Some(by) = self.title(id) else { return };
        if let Err(reason) = self.send(&by, direction, packet) {
            self.emit(HostEvent::SendRefused { by, reason });
        }
    }

    fn title(&self, id: ExtensionId) -> Option<String> {
        self.state()
            .extensions
            .get(&id)
            .map(|c| c.info.title.clone())
    }

    pub(crate) async fn authorize(&self, info: &ExtensionInfo) -> bool {
        {
            let mut state = self.state();
            if let (Some(file), Some(cookie)) = (&info.file, &info.cookie)
                && state.install_cookies.get(file) == Some(cookie)
            {
                state.install_cookies.remove(file);
                return true;
            }
            if state.remembered_permission {
                return true;
            }
        }
        let (respond, answer) = oneshot::channel();
        let request = PermissionRequest {
            info: info.clone(),
            respond,
        };
        if self.permissions.send(request).is_err() {
            return false;
        }
        match answer.await {
            Ok(Permission::AllowAlways) => {
                self.state().remembered_permission = true;
                true
            }
            Ok(Permission::Allow) => true,
            Ok(Permission::Deny) | Err(_) => false,
        }
    }

    pub(crate) fn register(
        &self,
        info: ExtensionInfo,
        outbox: mpsc::UnboundedSender<HostToExtension>,
    ) -> ExtensionId {
        let mut state = self.state();
        let id = state.next_id;
        state.next_id += 1;
        let _ = outbox.send(HostToExtension::Init {
            connected: state.connection.is_some(),
            host_info: self.config.host_info.clone(),
        });
        if let Some((connection, _)) = &state.connection {
            let _ = outbox.send(connection_start(connection, &state.packet_info));
        }
        state.extensions.insert(
            id,
            Connected {
                info: info.clone(),
                outbox,
            },
        );
        drop(state);
        self.emit(HostEvent::ExtensionConnected { id, info });
        id
    }

    pub(crate) fn unregister(&self, id: ExtensionId) {
        let finished: Vec<Pending> = {
            let mut state = self.state();
            if state.extensions.remove(&id).is_none() {
                return;
            }
            let done: Vec<_> = state
                .pending
                .iter_mut()
                .filter_map(|(key, pending)| {
                    (pending.waiting.remove(&id) && pending.waiting.is_empty()).then_some(*key)
                })
                .collect();
            done.iter()
                .filter_map(|key| state.pending.remove(key))
                .collect()
        };
        finished.into_iter().for_each(Pending::finish);
        self.emit(HostEvent::ExtensionDisconnected { id });
    }

    pub(crate) fn reply(&self, id: ExtensionId, message: HMessage) {
        let mut state = self.state();
        let Some(title) = state.extensions.get(&id).map(|c| c.info.title.clone()) else {
            return;
        };
        let key = (message.direction, message.index);
        let Some(pending) = state.pending.get_mut(&key) else {
            return;
        };
        if !pending.waiting.remove(&id) {
            return;
        }
        if message.blocked {
            pending.blocked_by.get_or_insert_with(|| title.clone());
        }
        if message.packet != pending.original {
            pending.edited = Some((message.packet, title));
        }
        if pending.waiting.is_empty()
            && let Some(done) = state.pending.remove(&key)
        {
            drop(state);
            done.finish();
        }
    }

    pub(crate) fn log_from_extension(&self, id: ExtensionId, text: String) {
        self.emit(HostEvent::ConsoleLog { id, text });
    }

    pub(crate) fn describe_packet(&self, packet: &Packet) -> HostToExtension {
        let display = bobba_protocol::expression::display_bytes(packet.frame());
        let info = self.packet_info();
        let id = packet.header_id();
        let names: Vec<(Direction, &str)> = [Direction::ToClient, Direction::ToServer]
            .into_iter()
            .filter_map(|d| info.name(d, id).map(|n| (d, n)))
            .collect();
        let label = match names.as_slice() {
            [(direction, name)] => HeaderLabel::Name(*direction, name),
            _ => HeaderLabel::Id(id),
        };
        let expression = if packet.frame().len() < EXPRESSION_SIZE_LIMIT {
            predicted_expression(packet, label)
        } else {
            String::new()
        };
        HostToExtension::PacketToStringResponse {
            display,
            expression,
        }
    }

    pub(crate) fn packet_from_text(&self, text: &str) -> HostToExtension {
        let packet = match Expression::parse(text) {
            Ok(Expression::Complete(packet)) => Some(packet),
            Ok(Expression::Named {
                direction,
                identifier,
                body,
            }) => {
                let header_id = self
                    .packet_info()
                    .header_id(direction, &identifier)
                    .unwrap_or(u16::MAX);
                Some(Packet::new(header_id, &body))
            }
            Err(_) => None,
        };
        HostToExtension::StringToPacketResponse(packet)
    }

    fn broadcast(&self, state: &State, message: impl Fn() -> HostToExtension) {
        for extension in state.extensions.values() {
            let _ = extension.outbox.send(message());
        }
    }
}

fn connection_start(connection: &ConnectionInfo, packet_info: &Arc<PacketInfo>) -> HostToExtension {
    HostToExtension::ConnectionStart {
        host: connection.host.clone(),
        port: connection.port,
        hotel_version: connection.hotel_version.clone(),
        client_identifier: connection.client_identifier.clone(),
        client_type: connection.client_type.clone(),
        packet_info: packet_info.clone(),
    }
}

impl<L: PacketInfoLoader> Interceptor for ExtensionHost<L> {
    async fn on_connected(&self, connection: ConnectionInfo, session: SessionHandle) {
        let packet_info = Arc::new(self.loader.load(&connection.hotel_version).await);
        {
            let mut state = self.state();
            state.packet_info = packet_info.clone();
            state.seen_headers.clear();
            self.broadcast(&state, || connection_start(&connection, &packet_info));
            state.connection = Some((connection, session));
        }
        self.emit(HostEvent::PacketInfoLoaded(packet_info));
    }

    async fn intercept(&self, message: HMessage) -> Verdict {
        let key = (message.direction, message.index);
        let answer = {
            let mut state = self.state();
            state
                .seen_headers
                .insert((message.direction, message.packet.header_id()));
            if state.extensions.is_empty() {
                return Verdict::Forward;
            }
            let (done, answer) = oneshot::channel();
            let waiting = state.extensions.keys().copied().collect();
            state.pending.insert(
                key,
                Pending {
                    original: message.packet.clone(),
                    waiting,
                    blocked_by: None,
                    edited: None,
                    done,
                },
            );
            self.broadcast(&state, || HostToExtension::PacketIntercept(message.clone()));
            answer
        };
        let Some(limit) = self.config.intercept_timeout else {
            return answer.await.unwrap_or(Verdict::Forward);
        };
        match tokio::time::timeout(limit, answer).await {
            Ok(verdict) => verdict.unwrap_or(Verdict::Forward),
            Err(_) => self.give_up_waiting(key),
        }
    }

    async fn on_disconnected(&self) {
        let mut state = self.state();
        state.connection = None;
        self.broadcast(&state, || HostToExtension::ConnectionEnd);
    }
}

impl<L: PacketInfoLoader> ExtensionHost<L> {
    fn give_up_waiting(&self, key: (Direction, u32)) -> Verdict {
        let Some(pending) = self.state().pending.remove(&key) else {
            return Verdict::Forward;
        };
        let waiting_on = pending
            .waiting
            .iter()
            .filter_map(|id| self.title(*id))
            .collect();
        self.emit(HostEvent::InterceptTimedOut {
            direction: key.0,
            index: key.1,
            waiting_on,
        });
        Pending::verdict(pending.blocked_by, pending.edited)
    }
}
