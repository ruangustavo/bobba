use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

use crate::host::{ExtensionHost, PacketInfoLoader};

const COOKIE_LENGTH: usize = 40;
const SUFFIX_LENGTH: usize = 12;
const COMMAND_FILE: &str = "command.txt";
const FILES_DIR: &str = "extension";

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
pub struct SingleFile {
    pub file_name: String,
    pub path: PathBuf,
    pub runtime: Runtime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub folder: String,
    pub title: String,
    pub version: String,
    pub dir: PathBuf,
    pub command: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstalledExtension {
    Single(SingleFile),
    Package(Package),
}

impl InstalledExtension {
    pub fn id(&self) -> &str {
        match self {
            InstalledExtension::Single(single) => &single.file_name,
            InstalledExtension::Package(package) => &package.folder,
        }
    }
}

impl Package {
    fn read(dir: PathBuf) -> Option<Self> {
        let folder = dir.file_name()?.to_str()?.to_owned();
        let (title, version) = folder.rsplit_once('_')?;
        let raw = std::fs::read_to_string(dir.join(COMMAND_FILE)).ok()?;
        let command: Vec<String> = serde_json::from_str(&raw).ok()?;
        if command.is_empty() {
            return None;
        }
        Some(Package {
            title: title.to_owned(),
            version: version.to_owned(),
            folder,
            dir,
            command,
        })
    }

    pub fn files(&self) -> PathBuf {
        self.dir.join(FILES_DIR)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("{0} is not a supported extension (jar, py, py3, js, sh, exe)")]
    UnsupportedType(String),
    #[error("\"{0}\" is not a valid extension name")]
    InvalidName(String),
    #[error("the extension archive is invalid: {0}")]
    InvalidArchive(String),
    #[error("{0} is already installed")]
    AlreadyInstalled(String),
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
            .filter_map(|path| {
                if path.is_dir() {
                    return Package::read(path).map(InstalledExtension::Package);
                }
                let runtime = Runtime::for_path(&path)?;
                let file_name = path.file_name()?.to_str()?.to_owned();
                Some(InstalledExtension::Single(SingleFile {
                    file_name,
                    path,
                    runtime,
                }))
            })
            .collect();
        found.sort_by(|a, b| a.id().cmp(b.id()));
        found
    }

    pub fn versions_of(&self, title: &str) -> Vec<Package> {
        self.installed()
            .into_iter()
            .filter_map(|e| match e {
                InstalledExtension::Package(package) if package.title == title => Some(package),
                _ => None,
            })
            .collect()
    }

    pub fn install_package(
        &self,
        title: &str,
        version: &str,
        command: &[String],
        archive: &[u8],
    ) -> Result<InstalledExtension, InstallError> {
        let folder = format!("{title}_{version}");
        let plain = |name: &str| {
            !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':'])
        };
        if !plain(title) || !plain(version) {
            return Err(InstallError::InvalidName(folder));
        }
        let dir = self.dir.join(&folder);
        if command.is_empty() {
            return Err(InstallError::InvalidArchive(
                "the extension has no command".to_owned(),
            ));
        }
        if dir.exists() {
            return Err(InstallError::AlreadyInstalled(title.to_owned()));
        }
        let files = dir.join(FILES_DIR);
        std::fs::create_dir_all(&files)?;
        let written = extract(archive, &files).and_then(|()| {
            let json = serde_json::to_string(command)
                .map_err(|error| InstallError::InvalidArchive(error.to_string()))?;
            std::fs::write(dir.join(COMMAND_FILE), json)?;
            Ok(())
        });
        if let Err(error) = written {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(error);
        }
        for older in self.versions_of(title) {
            if older.folder != folder {
                std::fs::remove_dir_all(&older.dir)?;
            }
        }
        Ok(InstalledExtension::Package(Package {
            folder,
            title: title.to_owned(),
            version: version.to_owned(),
            dir,
            command: command.to_vec(),
        }))
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
        Ok(InstalledExtension::Single(SingleFile {
            file_name,
            path,
            runtime,
        }))
    }

    pub fn uninstall(&self, id: &str) -> io::Result<()> {
        let path = self.dir.join(id);
        if id.is_empty() || path.parent() != Some(self.dir.as_path()) || id.contains("..") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an installed extension",
            ));
        }
        if path.is_dir() {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        }
    }
}

fn extract(archive: &[u8], into: &Path) -> Result<(), InstallError> {
    let invalid = |error: zip::result::ZipError| InstallError::InvalidArchive(error.to_string());
    let mut zip = zip::ZipArchive::new(io::Cursor::new(archive)).map_err(invalid)?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(invalid)?;
        let Some(relative) = entry.enclosed_name() else {
            return Err(InstallError::InvalidArchive(
                "an entry points outside the extension folder".to_owned(),
            ));
        };
        let target = into.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::File::create(&target)?;
        io::copy(&mut entry, &mut file)?;
        if entry.unix_mode().is_some_and(|mode| mode & 0o111 != 0) {
            make_executable(&target)?;
        }
    }
    Ok(())
}

pub fn launch<L: PacketInfoLoader>(
    extension: &InstalledExtension,
    port: u16,
    host: &ExtensionHost<L>,
    output: mpsc::UnboundedSender<ExtensionOutput>,
) -> io::Result<Child> {
    let id = extension.id().to_owned();
    let cookie = random_digits(COOKIE_LENGTH);
    host.issue_install_cookie(&id, cookie.clone());
    let mut command = match extension {
        InstalledExtension::Single(single) => {
            let mut command = single.runtime.command(&single.path);
            command.args(["-p", &port.to_string(), "-f", &id, "-c", &cookie]);
            command
        }
        InstalledExtension::Package(package) => {
            let port = port.to_string();
            let args: Vec<String> = package
                .command
                .iter()
                .map(|arg| {
                    arg.replace("{port}", &port)
                        .replace("{filename}", &id)
                        .replace("{cookie}", &cookie)
                })
                .collect();
            let Some((program, rest)) = args.split_first() else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "the extension has no command",
                ));
            };
            let mut command = Command::new(resolve_program(program));
            command.args(rest).current_dir(package.files());
            command
        }
    };
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    if let Some(stdout) = child.stdout.take() {
        tokio::spawn(forward_lines(stdout, id.clone(), output.clone()));
    }
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(forward_lines(stderr, id, output));
    }
    Ok(child)
}

#[cfg(target_os = "windows")]
const PYTHON_ALTERNATIVES: &[&str] = &["py", "python3"];
#[cfg(not(target_os = "windows"))]
const PYTHON_ALTERNATIVES: &[&str] = &["python3"];

fn resolve_with(program: &str, available: impl Fn(&str) -> bool) -> String {
    if program == "python"
        && !available(program)
        && let Some(alternative) = PYTHON_ALTERNATIVES.iter().find(|name| available(name))
    {
        return (*alternative).to_owned();
    }
    program.to_owned()
}

pub fn resolve_program(program: &str) -> String {
    resolve_with(program, program_available)
}

pub fn program_available(program: &str) -> bool {
    which::which(program).is_ok()
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
        assert_ne!(first.id(), second.id());
        assert!(first.id().starts_with("antispam-") && first.id().ends_with(".py"));
        assert_eq!(installer.installed().len(), 2);

        installer.uninstall(first.id()).expect("uninstalls");
        assert_eq!(installer.installed(), vec![second]);
    }

    fn archive(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
        use std::io::Write;
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, contents, mode) in entries {
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
                .unix_permissions(*mode);
            writer.start_file(*name, options).expect("starts entry");
            writer.write_all(contents).expect("writes entry");
        }
        writer.finish().expect("finishes archive").into_inner()
    }

    fn command() -> Vec<String> {
        ["java", "-jar", "tool.jar", "-p", "{port}"]
            .map(String::from)
            .to_vec()
    }

    #[test]
    fn installs_a_store_package_into_its_own_folder() {
        let install_dir = tempfile::tempdir().expect("temp dir");
        let installer = Installer::new(install_dir.path());
        let zip = archive(&[
            ("tool.jar", b"jar", 0o644),
            ("bin/run.sh", b"#!/bin/sh", 0o755),
        ]);

        let installed = installer
            .install_package("Room Mapper", "2.0.1", &command(), &zip)
            .expect("installs");

        let InstalledExtension::Package(package) = &installed else {
            panic!("expected a package, got {installed:?}");
        };
        assert_eq!(package.folder, "Room Mapper_2.0.1");
        assert_eq!(package.title, "Room Mapper");
        assert_eq!(package.version, "2.0.1");
        assert_eq!(package.command, command());
        let files = install_dir
            .path()
            .join("Room Mapper_2.0.1")
            .join("extension");
        assert_eq!(
            std::fs::read(files.join("tool.jar")).expect("extracted"),
            b"jar"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(files.join("bin/run.sh"))
                .expect("extracted")
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111);
        }
        assert_eq!(installer.installed(), vec![installed.clone()]);

        installer.uninstall(installed.id()).expect("uninstalls");
        assert!(installer.installed().is_empty());
        assert!(!install_dir.path().join("Room Mapper_2.0.1").exists());
    }

    #[test]
    fn a_new_version_replaces_the_old_one_only_once_it_is_in_place() {
        let install_dir = tempfile::tempdir().expect("temp dir");
        let installer = Installer::new(install_dir.path());
        let zip = archive(&[("tool.jar", b"jar", 0o644)]);
        installer
            .install_package("Room Mapper", "1.0", &command(), &zip)
            .expect("installs");

        let broken = installer.install_package("Room Mapper", "2.0", &command(), b"not a zip");
        assert!(broken.is_err());
        assert_eq!(installer.installed()[0].id(), "Room Mapper_1.0");

        let updated = installer
            .install_package("Room Mapper", "2.0", &command(), &zip)
            .expect("updates");
        assert_eq!(installer.installed(), vec![updated]);
    }

    #[test]
    fn refuses_archives_that_escape_the_folder_and_leaves_nothing_behind() {
        let install_dir = tempfile::tempdir().expect("temp dir");
        let installer = Installer::new(install_dir.path());
        let zip = archive(&[("tool.jar", b"jar", 0o644), ("../evil.sh", b"x", 0o755)]);

        let result = installer.install_package("Evil", "1.0", &command(), &zip);

        assert!(matches!(result, Err(InstallError::InvalidArchive(_))));
        assert!(!install_dir.path().join("evil.sh").exists());
        assert!(!install_dir.path().join("Evil_1.0").exists());
    }

    #[test]
    fn refuses_titles_that_are_not_a_plain_folder_name() {
        let install_dir = tempfile::tempdir().expect("temp dir");
        let installer = Installer::new(install_dir.path());
        let zip = archive(&[("tool.jar", b"jar", 0o644)]);
        for title in ["../up", "a/b", "", ".."] {
            assert!(matches!(
                installer.install_package(title, "1.0", &command(), &zip),
                Err(InstallError::InvalidName(_))
            ));
        }
    }

    #[test]
    fn python_falls_back_to_this_system_s_launcher_only_when_python_is_missing() {
        let fallback = PYTHON_ALTERNATIVES[0];
        let only_fallback = |name: &str| name == fallback;
        let both = |name: &str| name == "python" || name == fallback;
        assert_eq!(resolve_with("python", only_fallback), fallback);
        assert_eq!(resolve_with("python", both), "python");
        assert_eq!(resolve_with("python", |_| false), "python");
        assert_eq!(resolve_with("java", only_fallback), "java");
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
