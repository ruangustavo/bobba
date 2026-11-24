#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(not(any(target_os = "macos", windows)), path = "unsupported.rs")]
mod platform;

mod command_line;

use std::ffi::OsString;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use crate::hotels::Redirect;

pub const TAG: &str = "# Bobba";
pub const HELPER_COMMAND: &str = "hosts-helper";
const READY_MARKER: &str = "ready";
const DONE_MARKER: &str = "done";
const GIVE_UP_AFTER: Duration = Duration::from_secs(300);
const WAIT_FOR_APPROVAL: Duration = Duration::from_secs(120);
const WAIT_FOR_CLEANUP: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(200);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HostsError {
    #[error("refusing to redirect {0}: only habbo.com game hosts and 127.x addresses are allowed")]
    Unsafe(String),
    #[error("the administrator prompt was cancelled or failed")]
    NotApproved,
    #[error("could not prepare the redirect: {0}")]
    Io(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostLine {
    host: String,
    loopback: Ipv4Addr,
}

impl HostLine {
    pub fn parse(host: &str, loopback: Ipv4Addr) -> Result<Self, HostsError> {
        let host_ok = host.ends_with(".habbo.com")
            && host
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-');
        if !host_ok || loopback.octets()[0] != 127 {
            return Err(HostsError::Unsafe(host.to_owned()));
        }
        Ok(Self {
            host: host.to_owned(),
            loopback,
        })
    }

    fn from_argument(argument: &str) -> Result<Self, HostsError> {
        let unsafe_argument = || HostsError::Unsafe(argument.to_owned());
        let (host, loopback) = argument.split_once('=').ok_or_else(unsafe_argument)?;
        let loopback = loopback.parse().map_err(|_| unsafe_argument())?;
        Self::parse(host, loopback)
    }

    fn argument(&self) -> String {
        format!("{}={}", self.host, self.loopback)
    }
}

pub fn with_redirects(contents: &str, lines: &[HostLine]) -> String {
    let newline = if contents.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut edited: String = lines
        .iter()
        .map(|line| format!("{}\t{}\t{TAG}{newline}", line.loopback, line.host))
        .collect();
    edited.push_str(&without_tagged(contents));
    edited
}

pub fn without_tagged(contents: &str) -> String {
    contents
        .split_inclusive('\n')
        .filter(|line| !line.trim_end().ends_with(TAG))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HelperRequest {
    markers: PathBuf,
    lines: Vec<HostLine>,
}

impl HelperRequest {
    fn arguments(&self) -> Result<Vec<String>, HostsError> {
        let markers = self
            .markers
            .to_str()
            .ok_or_else(|| HostsError::Unsafe(self.markers.display().to_string()))?;
        Ok([HELPER_COMMAND.to_owned(), markers.to_owned()]
            .into_iter()
            .chain(self.lines.iter().map(HostLine::argument))
            .collect())
    }

    fn parse(arguments: &[OsString]) -> Result<Self, HostsError> {
        let mut texts = arguments.iter().map(|argument| {
            argument
                .to_str()
                .ok_or_else(|| HostsError::Unsafe(argument.to_string_lossy().into_owned()))
        });
        let markers = texts
            .next()
            .ok_or_else(|| HostsError::Unsafe("missing markers folder".to_owned()))??;
        let lines = texts
            .map(|text| HostLine::from_argument(text?))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            markers: PathBuf::from(markers),
            lines,
        })
    }
}

pub fn serve(arguments: &[OsString]) -> ExitCode {
    let Ok(request) = HelperRequest::parse(arguments) else {
        return ExitCode::FAILURE;
    };
    let Ok(hosts) = platform::hosts_file() else {
        return ExitCode::FAILURE;
    };
    for line in &request.lines {
        platform::alias_loopback(line.loopback);
    }
    match hold(&hosts, &request, platform::flush_dns) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

fn hold(hosts: &Path, request: &HelperRequest, flush_dns: impl Fn()) -> std::io::Result<()> {
    let done = request.markers.join(DONE_MARKER);
    if done.exists() {
        return Ok(());
    }
    let original = std::fs::read_to_string(hosts)?;
    std::fs::write(hosts, with_redirects(&original, &request.lines))?;
    flush_dns();
    let announced = std::fs::write(request.markers.join(READY_MARKER), b"").is_ok();
    let deadline = Instant::now() + GIVE_UP_AFTER;
    while announced && !done.exists() && Instant::now() < deadline {
        std::thread::sleep(POLL);
    }
    let current = std::fs::read_to_string(hosts)?;
    std::fs::write(hosts, without_tagged(&current))?;
    flush_dns();
    Ok(())
}

pub struct HostsRedirect {
    markers: PathBuf,
    helper: platform::Elevated,
}

impl HostsRedirect {
    pub async fn apply(redirects: &[Redirect]) -> Result<Self, HostsError> {
        let lines = redirects
            .iter()
            .map(|redirect| HostLine::parse(&redirect.host, redirect.loopback))
            .collect::<Result<Vec<_>, _>>()?;
        let markers = std::env::temp_dir().join(format!("bobba-hosts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&markers);
        std::fs::create_dir_all(&markers).map_err(|e| HostsError::Io(e.to_string()))?;
        let arguments = HelperRequest {
            markers: markers.clone(),
            lines,
        }
        .arguments()?;
        let program = std::env::current_exe().map_err(|e| HostsError::Io(e.to_string()))?;
        let deadline = tokio::time::Instant::now() + WAIT_FOR_APPROVAL;
        let elevating =
            tokio::task::spawn_blocking(move || platform::elevate(&program, &arguments));
        let mut helper = match tokio::time::timeout_at(deadline, elevating).await {
            Ok(Ok(elevated)) => elevated?,
            Ok(Err(error)) => return Err(HostsError::Io(error.to_string())),
            Err(_) => {
                let _ = std::fs::write(markers.join(DONE_MARKER), b"");
                return Err(HostsError::NotApproved);
            }
        };
        let ready = markers.join(READY_MARKER);
        while !ready.exists() {
            if helper.has_exited() {
                return Err(HostsError::NotApproved);
            }
            if tokio::time::Instant::now() > deadline {
                helper.kill();
                let _ = std::fs::write(markers.join(DONE_MARKER), b"");
                return Err(HostsError::NotApproved);
            }
            tokio::time::sleep(POLL).await;
        }
        Ok(Self { markers, helper })
    }

    pub async fn remove(mut self) {
        let _ = std::fs::write(self.markers.join(DONE_MARKER), b"");
        let deadline = tokio::time::Instant::now() + WAIT_FOR_CLEANUP;
        while !self.helper.has_exited() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(POLL).await;
        }
        let _ = std::fs::remove_dir_all(&self.markers);
    }
}

impl Drop for HostsRedirect {
    fn drop(&mut self) {
        let _ = std::fs::write(self.markers.join(DONE_MARKER), b"");
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    const ORIGINAL: &str = "127.0.0.1\tlocalhost\n255.255.255.255\tbroadcasthost\n";

    fn line(host: &str) -> HostLine {
        HostLine::parse(host, Ipv4Addr::new(127, 0, 0, 2)).expect("safe")
    }

    #[test]
    fn redirects_come_first_tagged_and_replace_older_ones() {
        let stale = format!("127.0.0.9\tgame-us.habbo.com\t{TAG}\n{ORIGINAL}");
        let edited = with_redirects(&stale, &[line("game-br.habbo.com")]);
        assert_eq!(
            edited,
            format!("127.0.0.2\tgame-br.habbo.com\t{TAG}\n{ORIGINAL}")
        );
        assert_eq!(without_tagged(&edited), ORIGINAL);
    }

    #[test]
    fn keeps_windows_line_endings() {
        let original = "127.0.0.1 localhost\r\n";
        let edited = with_redirects(original, &[line("game-br.habbo.com")]);
        assert_eq!(
            edited,
            format!("127.0.0.2\tgame-br.habbo.com\t{TAG}\r\n{original}")
        );
        assert_eq!(without_tagged(&edited), original);
    }

    #[test]
    fn refuses_hosts_outside_habbo_and_addresses_outside_loopback() {
        for host in [
            "evil.example.com",
            "game-br.habbo.com;rm -rf /",
            "GAME-BR.habbo.com",
        ] {
            assert_eq!(
                HostLine::parse(host, Ipv4Addr::new(127, 0, 0, 2)),
                Err(HostsError::Unsafe(host.into()))
            );
        }
        assert!(HostLine::parse("game-br.habbo.com", Ipv4Addr::new(10, 0, 0, 2)).is_err());
        assert!(HostLine::from_argument("game-br.habbo.com=10.0.0.2").is_err());
        assert!(HostLine::from_argument("game-br.habbo.com").is_err());
    }

    #[test]
    fn the_helper_reads_back_the_request_it_was_given() {
        let request = HelperRequest {
            markers: PathBuf::from("/tmp/bobba hosts 1"),
            lines: vec![line("game-br.habbo.com"), line("game-us.habbo.com")],
        };
        let arguments: Vec<OsString> = request
            .arguments()
            .expect("utf-8 path")
            .into_iter()
            .map(OsString::from)
            .collect();
        assert_eq!(arguments[0], HELPER_COMMAND);
        assert_eq!(HelperRequest::parse(&arguments[1..]), Ok(request));
    }

    #[test]
    fn holds_the_redirect_until_done_then_restores_the_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let hosts = dir.path().join("hosts");
        std::fs::write(&hosts, ORIGINAL).expect("writes");
        let request = HelperRequest {
            markers: dir.path().to_path_buf(),
            lines: vec![line("game-br.habbo.com")],
        };
        let watcher = {
            let hosts = hosts.clone();
            let markers = request.markers.clone();
            std::thread::spawn(move || {
                while !markers.join(READY_MARKER).exists() {
                    std::thread::sleep(Duration::from_millis(10));
                }
                let during = std::fs::read_to_string(&hosts).expect("reads");
                std::fs::write(markers.join(DONE_MARKER), b"").expect("writes");
                during
            })
        };
        let flushes = Cell::new(0);
        hold(&hosts, &request, || flushes.set(flushes.get() + 1)).expect("holds");
        let during = watcher.join().expect("watcher");
        assert!(during.starts_with("127.0.0.2\tgame-br.habbo.com\t# Bobba\n"));
        assert_eq!(std::fs::read_to_string(&hosts).expect("reads"), ORIGINAL);
        assert_eq!(flushes.get(), 2);
    }

    #[test]
    fn a_helper_approved_after_giving_up_leaves_the_file_alone() {
        let dir = tempfile::tempdir().expect("temp dir");
        let hosts = dir.path().join("hosts");
        std::fs::write(&hosts, ORIGINAL).expect("writes");
        std::fs::write(dir.path().join(DONE_MARKER), b"").expect("writes");
        let request = HelperRequest {
            markers: dir.path().to_path_buf(),
            lines: vec![line("game-br.habbo.com")],
        };
        hold(&hosts, &request, || {}).expect("returns");
        assert_eq!(std::fs::read_to_string(&hosts).expect("reads"), ORIGINAL);
        assert!(!dir.path().join(READY_MARKER).exists());
    }
}
