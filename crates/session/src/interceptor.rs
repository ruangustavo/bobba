use std::future::Future;

use bobba_protocol::{HMessage, Packet};

use crate::session::SessionHandle;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionInfo {
    pub host: String,
    pub port: u16,
    pub hotel_version: String,
    pub client_identifier: String,
    pub client_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Forward,
    Edited { packet: Packet, by: String },
    Blocked { by: String },
}

pub trait Interceptor: Send + Sync + 'static {
    fn on_connected(
        &self,
        connection: ConnectionInfo,
        session: SessionHandle,
    ) -> impl Future<Output = ()> + Send;

    fn intercept(&self, message: HMessage) -> impl Future<Output = Verdict> + Send;

    fn on_disconnected(&self) -> impl Future<Output = ()> + Send;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct PassThrough;

impl Interceptor for PassThrough {
    async fn on_connected(&self, _: ConnectionInfo, _: SessionHandle) {}

    async fn intercept(&self, _: HMessage) -> Verdict {
        Verdict::Forward
    }

    async fn on_disconnected(&self) {}
}
