use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use super::{HostsError, command_line};

const HOSTS_FILE: &str = "/etc/hosts";
const PROMPT: &str = "Bobba redirects Habbo's game servers to itself until your client connects.";

pub struct Elevated {
    osascript: Child,
}

impl Elevated {
    pub fn has_exited(&mut self) -> bool {
        !matches!(self.osascript.try_wait(), Ok(None))
    }

    pub fn kill(&mut self) {
        let _ = self.osascript.kill();
    }
}

pub fn elevate(program: &Path, arguments: &[String]) -> Result<Elevated, HostsError> {
    let program = program
        .to_str()
        .ok_or_else(|| HostsError::Unsafe(program.display().to_string()))?;
    let command =
        command_line::posix(std::iter::once(program).chain(arguments.iter().map(String::as_str)));
    let script = format!(
        "do shell script \"{}\" with administrator privileges with prompt \"{PROMPT}\"",
        applescript_string(&command)
    );
    let osascript = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .spawn()
        .map_err(|e| HostsError::Io(e.to_string()))?;
    Ok(Elevated { osascript })
}

pub fn hosts_file() -> std::io::Result<PathBuf> {
    Ok(PathBuf::from(HOSTS_FILE))
}

pub fn alias_loopback(address: Ipv4Addr) {
    let _ = Command::new("ifconfig")
        .args(["lo0", "alias", &address.to_string(), "up"])
        .output();
}

pub fn flush_dns() {
    let _ = Command::new("dscacheutil").arg("-flushcache").output();
    let _ = Command::new("killall")
        .args(["-HUP", "mDNSResponder"])
        .output();
}

fn applescript_string(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applescript_strings_escape_quotes_and_backslashes() {
        assert_eq!(applescript_string(r#"a"b\c"#), r#"a\"b\\c"#);
    }
}
