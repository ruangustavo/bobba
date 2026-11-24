use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

use super::HostsError;

pub enum Elevated {}

impl Elevated {
    pub fn has_exited(&mut self) -> bool {
        match *self {}
    }

    pub fn kill(&mut self) {
        match *self {}
    }
}

pub fn elevate(_: &Path, _: &[String]) -> Result<Elevated, HostsError> {
    Err(HostsError::Io(
        "redirecting Habbo isn't supported on this OS".to_owned(),
    ))
}

pub fn hosts_file() -> std::io::Result<PathBuf> {
    Err(std::io::ErrorKind::Unsupported.into())
}

pub fn alias_loopback(_: Ipv4Addr) {}

pub fn flush_dns() {}
