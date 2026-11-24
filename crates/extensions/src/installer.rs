use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

use crate::host::{ExtensionHost, PacketInfoLoader};

const COOKIE_LENGTH: usize = 40;
const SUFFIX_LENGTH: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    Java,
    Python,
    Node,
    Executable,
}

impl Runtime {
    pub fn for_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "jar" => Some(Runtime::Java),
            "py" | "py3" => Some(Runtime::Python),
            "js" => Some(Runtime::Node),
            "sh" | "exe" => Some(Runtime::Executable),
            _ => None,
        }
    }

    fn command(self, path: &Path) -> Command {
        match self {
            Runtime::Java => {
                let mut command = Command::new("java");
                command.arg("-jar").arg(path);
                command
            }
            Runtime::Python => {
                let mut command = Command::new("python3");
                command.arg(path);
                command
            }
            Runtime::Node => {
                let mut command = Command::new("node");
                command.arg(path);
                command
            }
            Runtime::Executable => Command::new(path),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledExtension {
    pub file_name: String,
    pub path: PathBuf,
    pub runtime: Runtime,
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("{0} is not a supported extension (jar, py, py3, js, sh, exe)")]
    UnsupportedType(String),
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Debug, Clone)]
pub struct ExtensionOutput {
    pub file_name: String,
    pub line: String,
}

pub struct Installer {
    dir: PathBuf,
}

impl Installer {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn with_default_dir() -> Self {
        let dir = dirs::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("Bobba")
            .join("extensions");
        Self::new(dir)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn installed(&self) -> Vec<InstalledExtension> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut found: Vec<InstalledExtension> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .filter_map(|path| {
                let runtime = Runtime::for_path(&path)?;
                let file_name = path.file_name()?.to_str()?.to_owned();
                Some(InstalledExtension {
                    file_name,
                    path,
                    runtime,
                })
            })
            .collect();
        found.sort_by(|a, b| a.file_name.cmp(&b.file_name));
        found
    }

    pub fn install(&self, source: &Path) -> Result<InstalledExtension, InstallError> {
        let display = source.display().to_string();
        let runtime = Runtime::for_path(source)
            .ok_or_else(|| InstallError::UnsupportedType(display.clone()))?;
        let stem = source
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| InstallError::UnsupportedType(display.clone()))?;
        let extension = source
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        let file_name = format!("{stem}-{}.{extension}", random_digits(SUFFIX_LENGTH));
        std::fs::create_dir_all(&self.dir)?;
        let path = self.dir.join(&file_name);
        std::fs::copy(source, &path)?;
        if runtime == Runtime::Executable {
            make_executable(&path)?;
        }
        Ok(InstalledExtension {
            file_name,
            path,
            runtime,
        })
    }

    pub fn uninstall(&self, file_name: &str) -> io::Result<()> {
        let path = self.dir.join(file_name);
        if path.parent() != Some(self.dir.as_path()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an installed extension",
            ));
        }
        std::fs::remove_file(path)
    }
}

pub fn launch<L: PacketInfoLoader>(
    extension: &InstalledExtension,
    port: u16,
    host: &ExtensionHost<L>,
    output: mpsc::UnboundedSender<ExtensionOutput>,
) -> io::Result<Child> {
    let cookie = random_digits(COOKIE_LENGTH);
    host.issue_install_cookie(&extension.file_name, cookie.clone());
    let mut child = extension
        .runtime
        .command(&extension.path)
        .args([
            "-p",
            &port.to_string(),
            "-f",
            &extension.file_name,
            "-c",
            &cookie,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    if let Some(stdout) = child.stdout.take() {
        tokio::spawn(forward_lines(
            stdout,
            extension.file_name.clone(),
            output.clone(),
        ));
    }
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(forward_lines(stderr, extension.file_name.clone(), output));
    }
    Ok(child)
}

async fn forward_lines(
    stream: impl tokio::io::AsyncRead + Unpin,
    file_name: String,
    output: mpsc::UnboundedSender<ExtensionOutput>,
) {
    let mut lines = BufReader::new(stream).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let sent = output.send(ExtensionOutput {
            file_name: file_name.clone(),
            line,
        });
        if sent.is_err() {
            return;
        }
    }
}

fn random_digits(count: usize) -> String {
    let mut bytes = vec![0u8; count];
    if getrandom::fill(&mut bytes).is_err() {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        bytes
            .iter_mut()
            .enumerate()
            .for_each(|(i, b)| *b = (seed >> (i % 4 * 8)) as u8 ^ i as u8);
    }
    bytes.iter().map(|b| char::from(b'0' + b % 10)).collect()
}

#[cfg(unix)]
fn make_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(permissions.mode() | 0o755);
    std::fs::set_permissions(path, permissions)
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_runtime_from_the_file_type() {
        assert_eq!(Runtime::for_path(Path::new("a.JAR")), Some(Runtime::Java));
        assert_eq!(Runtime::for_path(Path::new("a.py3")), Some(Runtime::Python));
        assert_eq!(Runtime::for_path(Path::new("a.js")), Some(Runtime::Node));
        assert_eq!(
            Runtime::for_path(Path::new("a.sh")),
            Some(Runtime::Executable)
        );
        assert_eq!(Runtime::for_path(Path::new("a.txt")), None);
    }

    #[test]
    fn install_copies_with_a_unique_name_and_uninstall_removes_it() {
        let source_dir = tempfile::tempdir().expect("temp dir");
        let install_dir = tempfile::tempdir().expect("temp dir");
        let source = source_dir.path().join("antispam.py");
        std::fs::write(&source, "print('hi')").expect("writes");
        let installer = Installer::new(install_dir.path());

        let first = installer.install(&source).expect("installs");
        let second = installer.install(&source).expect("installs again");
        assert_ne!(first.file_name, second.file_name);
        assert!(first.file_name.starts_with("antispam-") && first.file_name.ends_with(".py"));
        assert_eq!(installer.installed().len(), 2);

        installer.uninstall(&first.file_name).expect("uninstalls");
        assert_eq!(installer.installed(), vec![second]);
    }

    #[test]
    fn refuses_unknown_types_and_paths_outside_the_dir() {
        let install_dir = tempfile::tempdir().expect("temp dir");
        let installer = Installer::new(install_dir.path());
        assert!(matches!(
            installer.install(Path::new("/tmp/readme.txt")),
            Err(InstallError::UnsupportedType(_))
        ));
        assert!(installer.uninstall("../escape.py").is_err());
    }
}
