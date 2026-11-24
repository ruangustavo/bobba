use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::process::Command;

use crate::hotels::Redirect;

pub const TAG: &str = "# Bobba";
const READY_MARKER: &str = "ready";
const DONE_MARKER: &str = "done";
const GIVE_UP_AFTER_HALF_SECONDS: u32 = 600;
const WAIT_FOR_APPROVAL: Duration = Duration::from_secs(120);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HostsError {
    #[error("refusing to redirect {0}: only habbo.com game hosts and 127.x addresses are allowed")]
    Unsafe(String),
    #[error("the administrator prompt was cancelled or failed")]
    NotApproved,
    #[error("could not prepare the redirect: {0}")]
    Io(String),
}

pub fn privileged_script(redirects: &[Redirect], markers: &Path) -> Result<String, HostsError> {
    let markers_text = markers
        .to_str()
        .ok_or_else(|| HostsError::Unsafe(markers.display().to_string()))?;
    if !markers_text
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c))
    {
        return Err(HostsError::Unsafe(markers_text.to_owned()));
    }
    let mut aliases = String::new();
    let mut lines = String::new();
    for redirect in redirects {
        let host_ok = redirect.host.ends_with(".habbo.com")
            && redirect
                .host
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-');
        if !host_ok || redirect.loopback.octets()[0] != 127 {
            return Err(HostsError::Unsafe(redirect.host.clone()));
        }
        aliases.push_str(&format!(
            "ifconfig lo0 alias {} up 2>/dev/null || true\n",
            redirect.loopback
        ));
        lines.push_str(&format!(
            "{}\t{}\t{TAG}\\n",
            redirect.loopback, redirect.host
        ));
    }
    Ok(format!(
        "set -e
{aliases}remove_tagged() {{ sed -i '' '/{TAG}$/d' /etc/hosts; dscacheutil -flushcache; killall -HUP mDNSResponder 2>/dev/null || true; }}
remove_tagged
{{ printf '{lines}'; cat /etc/hosts; }} > /etc/hosts.bobba
cat /etc/hosts.bobba > /etc/hosts
rm -f /etc/hosts.bobba
dscacheutil -flushcache; killall -HUP mDNSResponder 2>/dev/null || true
touch {markers_text}/{READY_MARKER}
i=0
while [ ! -f {markers_text}/{DONE_MARKER} ] && [ $i -lt {GIVE_UP_AFTER_HALF_SECONDS} ]; do sleep 0.5; i=$((i+1)); done
remove_tagged
"
    ))
}

fn applescript_string(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

pub struct HostsRedirect {
    markers: PathBuf,
    process: tokio::process::Child,
}

impl HostsRedirect {
    pub async fn apply(redirects: &[Redirect]) -> Result<Self, HostsError> {
        let markers = std::env::temp_dir().join(format!("bobba-hosts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&markers);
        std::fs::create_dir_all(&markers).map_err(|e| HostsError::Io(e.to_string()))?;
        let script = privileged_script(redirects, &markers)?;
        let applescript = format!(
            "do shell script \"{}\" with administrator privileges with prompt \"Bobba redirects Habbo's game servers to itself until your client connects.\"",
            applescript_string(&script)
        );
        let mut process = Command::new("osascript")
            .arg("-e")
            .arg(applescript)
            .kill_on_drop(false)
            .spawn()
            .map_err(|e| HostsError::Io(e.to_string()))?;
        let ready = markers.join(READY_MARKER);
        let deadline = tokio::time::Instant::now() + WAIT_FOR_APPROVAL;
        while !ready.exists() {
            if let Ok(Some(_)) = process.try_wait() {
                return Err(HostsError::NotApproved);
            }
            if tokio::time::Instant::now() > deadline {
                let _ = process.kill().await;
                return Err(HostsError::NotApproved);
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        Ok(Self { markers, process })
    }

    pub async fn remove(mut self) {
        let _ = std::fs::write(self.markers.join(DONE_MARKER), b"");
        let _ = tokio::time::timeout(Duration::from_secs(10), self.process.wait()).await;
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
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;

    fn redirect(host: &str) -> Redirect {
        Redirect {
            host: host.into(),
            real_address: IpAddr::V4(Ipv4Addr::new(52, 1, 2, 3)),
            loopback: Ipv4Addr::new(127, 0, 0, 2),
        }
    }

    #[test]
    fn script_aliases_loopback_tags_lines_and_cleans_up_on_done() {
        let script = privileged_script(
            &[redirect("game-br.habbo.com")],
            Path::new("/tmp/bobba-hosts-1"),
        )
        .expect("safe");
        assert!(script.contains("ifconfig lo0 alias 127.0.0.2 up"));
        assert!(script.contains("127.0.0.2\tgame-br.habbo.com\t# Bobba"));
        assert!(script.contains("touch /tmp/bobba-hosts-1/ready"));
        assert!(script.contains("[ ! -f /tmp/bobba-hosts-1/done ]"));
        assert!(script.trim_end().ends_with("remove_tagged"));
    }

    #[test]
    fn refuses_hosts_outside_habbo_and_shell_metacharacters() {
        for host in [
            "evil.example.com",
            "game-br.habbo.com;rm -rf /",
            "GAME-BR.habbo.com",
        ] {
            assert_eq!(
                privileged_script(&[redirect(host)], Path::new("/tmp/x")),
                Err(HostsError::Unsafe(host.into()))
            );
        }
        assert!(
            privileged_script(&[redirect("game-br.habbo.com")], Path::new("/tmp/a b")).is_err()
        );
    }

    #[test]
    fn script_is_valid_shell() {
        let script = privileged_script(
            &[redirect("game-br.habbo.com")],
            Path::new("/tmp/bobba-hosts-1"),
        )
        .expect("safe");
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("script.sh");
        std::fs::write(&path, script).expect("writes");
        let checked = std::process::Command::new("sh")
            .arg("-n")
            .arg(&path)
            .status()
            .expect("sh runs");
        assert!(checked.success());
    }

    #[test]
    fn applescript_strings_escape_quotes_and_backslashes() {
        assert_eq!(applescript_string(r#"a"b\c"#), r#"a\"b\\c"#);
    }
}
