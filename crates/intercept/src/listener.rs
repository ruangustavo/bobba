use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use bobba_session::{Session, Upstream};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;

use crate::hotels::{GAME_PORT, Redirect};

const UPSTREAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

pub struct HotelListeners {
    listeners: Vec<(Redirect, TcpListener)>,
    upstream_port: u16,
}

impl HotelListeners {
    pub async fn bind(redirects: &[Redirect]) -> io::Result<Self> {
        Self::bind_with_ports(redirects, GAME_PORT, GAME_PORT).await
    }

    pub async fn bind_with_ports(
        redirects: &[Redirect],
        port: u16,
        upstream_port: u16,
    ) -> io::Result<Self> {
        let mut listeners = Vec::with_capacity(redirects.len());
        for redirect in redirects {
            let listener = TcpListener::bind(SocketAddr::from((redirect.loopback, port))).await?;
            listeners.push((redirect.clone(), listener));
        }
        Ok(Self {
            listeners,
            upstream_port,
        })
    }

    pub fn local_ports(&self) -> Vec<u16> {
        self.listeners
            .iter()
            .filter_map(|(_, listener)| listener.local_addr().ok().map(|a| a.port()))
            .collect()
    }

    pub async fn accept_first(self) -> io::Result<(Session, Redirect)> {
        let upstream_port = self.upstream_port;
        let mut accepts = JoinSet::new();
        for (redirect, listener) in self.listeners {
            accepts.spawn(async move {
                listener
                    .accept()
                    .await
                    .map(|(stream, _)| (stream, redirect))
            });
        }
        let (client, redirect) = loop {
            match accepts.join_next().await {
                Some(Ok(Ok(accepted))) => break accepted,
                Some(_) => continue,
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotConnected,
                        "no hotel listener left",
                    ));
                }
            }
        };
        accepts.abort_all();
        let real = SocketAddr::new(redirect.real_address, upstream_port);
        let server = tokio::time::timeout(UPSTREAM_CONNECT_TIMEOUT, TcpStream::connect(real))
            .await
            .map_err(|_| {
                io::Error::new(io::ErrorKind::TimedOut, "hotel did not accept in time")
            })??;
        let upstream = Upstream {
            host: redirect.host.clone(),
            port: GAME_PORT,
        };
        Ok((Session::from_streams(client, server, upstream)?, redirect))
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::Arc;

    use bobba_session::{NoTables, PassThrough};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::mpsc;

    use super::*;

    fn local_redirect(loopback: Ipv4Addr) -> Redirect {
        Redirect {
            host: "game-br.habbo.com".into(),
            real_address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            loopback,
        }
    }

    #[tokio::test]
    async fn binding_fails_cleanly_when_a_loopback_alias_is_missing() {
        let bound =
            HotelListeners::bind_with_ports(&[local_redirect(Ipv4Addr::new(127, 0, 9, 9))], 0, 0)
                .await;
        if cfg!(target_os = "macos") {
            assert!(
                bound.is_err(),
                "macOS only answers on 127.0.0.1 until an alias exists"
            );
        }
    }

    #[tokio::test]
    async fn first_client_is_bridged_to_the_real_hotel_address() {
        let hotel = TcpListener::bind("127.0.0.1:0").await.expect("binds");
        let hotel_port = hotel.local_addr().expect("address").port();
        let listeners =
            HotelListeners::bind_with_ports(&[local_redirect(Ipv4Addr::LOCALHOST)], 0, hotel_port)
                .await
                .expect("binds");
        let listen_port = listeners.local_ports()[0];
        let bridge = tokio::spawn(listeners.accept_first());
        let mut client = TcpStream::connect(("127.0.0.1", listen_port))
            .await
            .expect("connects");
        let (mut server, _) = hotel.accept().await.expect("bridge dials the hotel");
        let (session, redirect) = bridge.await.expect("task").expect("bridged");
        assert_eq!(redirect.host, "game-br.habbo.com");
        let (events, _) = mpsc::unbounded_channel();
        let (log, _log_rx) = mpsc::channel(16);
        let _running = session.start(Arc::new(PassThrough), Arc::new(NoTables), events, log);

        client
            .write_all(b"<policy-file-request/>\0")
            .await
            .expect("writes");
        let mut received = [0u8; 23];
        server
            .read_exact(&mut received)
            .await
            .expect("bytes cross the bridge");
        assert_eq!(&received, b"<policy-file-request/>\0");
    }
}
