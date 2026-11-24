use std::path::{Path, PathBuf};

use tokio::process::Command;

const ENTITLEMENTS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>com.apple.security.get-task-allow</key><true/></dict></plist>
"#;
const ORIGINAL_SUFFIX: &str = "bobba-original";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientState {
    NotInstalled,
    NeedsPreparation(PathBuf),
    Ready(PathBuf),
}

#[derive(Debug, thiserror::Error)]
pub enum PrepareError {
    #[error("Habbo is running; quit it before preparing the client")]
    Running,
    #[error("codesign failed: {0}")]
    Codesign(String),
    #[error(
        "macOS blocked changing Habbo.app; allow Bobba under System Settings › Privacy & Security › App Management ({0})"
    )]
    Blocked(String),
}

pub fn launcher_air_dir() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join("Library/Application Support/Habbo Launcher/downloads/air"))
}

pub fn find_client(air_dir: &Path) -> Option<PathBuf> {
    let mut apps: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(air_dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path().join("Habbo.app"))
        .filter(|app| app.join("Contents/MacOS/Habbo").is_file())
        .filter_map(|app| Some((std::fs::metadata(&app).ok()?.modified().ok()?, app)))
        .collect();
    apps.sort();
    apps.pop().map(|(_, app)| app)
}

pub fn is_prepared(entitlements_xml: &str, signature_details: &str) -> bool {
    let compact: String = entitlements_xml.split_whitespace().collect();
    let debuggable = compact.contains("<key>com.apple.security.get-task-allow</key><true/>");
    let adhoc = signature_details
        .lines()
        .any(|line| line.trim() == "Signature=adhoc");
    let hardened = signature_details
        .lines()
        .filter(|line| line.starts_with("CodeDirectory"))
        .any(|line| line.contains("runtime"));
    debuggable && adhoc && !hardened
}

async fn codesign_output(args: &[&str], target: &Path) -> String {
    Command::new("codesign")
        .args(args)
        .arg(target)
        .output()
        .await
        .map(|out| {
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
        })
        .unwrap_or_default()
}

pub async fn client_state() -> ClientState {
    let Some(app) = launcher_air_dir().as_deref().and_then(find_client) else {
        return ClientState::NotInstalled;
    };
    let entitlements = codesign_output(&["-d", "--entitlements", "-", "--xml"], &app).await;
    let details = codesign_output(&["-dv", "--verbose=4"], &app).await;
    if is_prepared(&entitlements, &details) {
        ClientState::Ready(app)
    } else {
        ClientState::NeedsPreparation(app)
    }
}

async fn habbo_is_running() -> bool {
    Command::new("pgrep")
        .args(["-x", "Habbo"])
        .output()
        .await
        .is_ok_and(|out| out.status.success())
}

pub async fn prepare(app: &Path) -> Result<PathBuf, PrepareError> {
    if habbo_is_running().await {
        return Err(PrepareError::Running);
    }
    let parent = app
        .parent()
        .ok_or_else(|| PrepareError::Blocked(app.display().to_string()))?;
    let staging = tempfile_dir().map_err(|e| PrepareError::Blocked(e.to_string()))?;
    let copy = staging.join("Habbo.app");
    run("ditto", &[app.as_os_str(), copy.as_os_str()])
        .await
        .map_err(PrepareError::Blocked)?;
    let entitlements = staging.join("entitlements.plist");
    std::fs::write(&entitlements, ENTITLEMENTS)
        .map_err(|e| PrepareError::Blocked(e.to_string()))?;
    let executable = copy.join("Contents/MacOS/Habbo");
    run(
        "codesign",
        &[
            "--force".as_ref(),
            "--sign".as_ref(),
            "-".as_ref(),
            "--entitlements".as_ref(),
            entitlements.as_os_str(),
            executable.as_os_str(),
        ],
    )
    .await
    .map_err(PrepareError::Codesign)?;
    let mut original = parent.join(format!("Habbo.{ORIGINAL_SUFFIX}.app"));
    let mut n = 1;
    while original.exists() {
        original = parent.join(format!("Habbo.{ORIGINAL_SUFFIX}-{n}.app"));
        n += 1;
    }
    std::fs::rename(app, &original).map_err(|e| PrepareError::Blocked(e.to_string()))?;
    if let Err(error) = run("ditto", &[copy.as_os_str(), app.as_os_str()]).await {
        let _ = std::fs::rename(&original, app);
        return Err(PrepareError::Blocked(error));
    }
    let _ = std::fs::remove_dir_all(&staging);
    Ok(original)
}

fn tempfile_dir() -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("bobba-resign-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

async fn run(program: &str, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PREPARED_ENTITLEMENTS: &str = "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict>\n  <key>com.apple.security.get-task-allow</key>\n  <true/>\n</dict></plist>";
    const ADHOC: &str = "Executable=/x/Habbo\nCodeDirectory v=20400 size=936 flags=0x2(adhoc) hashes=18+7 location=embedded\nSignature=adhoc\n";
    const SHIPPED: &str = "CodeDirectory v=20500 size=1234 flags=0x10000(runtime) hashes=30+7\nAuthority=Developer ID Application: Sulake Oy\nTeamIdentifier=XXXX\n";

    #[test]
    fn recognises_a_prepared_client() {
        assert!(is_prepared(PREPARED_ENTITLEMENTS, ADHOC));
    }

    #[test]
    fn the_shipped_client_needs_preparation() {
        assert!(!is_prepared("", SHIPPED));
        assert!(!is_prepared(PREPARED_ENTITLEMENTS, SHIPPED));
        assert!(!is_prepared("<dict></dict>", ADHOC));
    }

    #[test]
    fn picks_the_newest_client_with_an_executable() {
        let air = tempfile::tempdir().expect("temp dir");
        for (version, executable) in [("1", true), ("2", true), ("3", false)] {
            let macos = air.path().join(version).join("Habbo.app/Contents/MacOS");
            std::fs::create_dir_all(&macos).expect("creates");
            if executable {
                std::fs::write(macos.join("Habbo"), b"").expect("writes");
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let newest = air.path().join("2").join("Habbo.app");
        assert_eq!(find_client(air.path()), Some(newest));
    }
}
