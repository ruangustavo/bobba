use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

const GEARTH_PREFIX: &str = "g-earth";
const SEARCH_DEPTH: usize = 2;
const JAVAFX_MODULE: &str = "javafx.controls";
const JAVAFX_OPENED: &[&str] = &[
    "--add-opens=javafx.graphics/javafx.scene.text=ALL-UNNAMED",
    "--add-opens=javafx.graphics/com.sun.javafx.text=ALL-UNNAMED",
    "--add-exports=javafx.graphics/com.sun.javafx.text=ALL-UNNAMED",
    "--add-opens=javafx.graphics/com.sun.javafx.scene.text=ALL-UNNAMED",
    "--add-exports=javafx.graphics/com.sun.javafx.scene.text=ALL-UNNAMED",
    "--add-opens=javafx.graphics/com.sun.javafx.geom=ALL-UNNAMED",
    "--add-exports=javafx.graphics/com.sun.javafx.geom=ALL-UNNAMED",
];

static FOUND: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn java_with_javafx(on_path: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(found) = FOUND.lock().ok().and_then(|found| found.clone()) {
        return Some(found);
    }
    let found = gearth_installs(&search_roots())
        .into_iter()
        .filter_map(|install| bundled_java(&install))
        .chain(on_path)
        .find(|java| has_javafx(java))?;
    if let Ok(mut cached) = FOUND.lock() {
        *cached = Some(found.clone());
    }
    Some(found)
}

pub fn java_environment(
    java: &Path,
    players_options: Option<OsString>,
) -> Vec<(&'static str, OsString)> {
    let mut options = players_options.unwrap_or_default();
    for opened in JAVAFX_OPENED {
        if !options.is_empty() {
            options.push(" ");
        }
        options.push(opened);
    }
    java_home(java)
        .map(|home| ("JAVA_HOME", home.into_os_string()))
        .into_iter()
        .chain([("JDK_JAVA_OPTIONS", options)])
        .collect()
}

pub fn java_home(java: &Path) -> Option<PathBuf> {
    java.parent()?.parent().map(Path::to_owned)
}

fn has_javafx(java: &Path) -> bool {
    let mut command = Command::new(java);
    command
        .arg("--list-modules")
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    crate::installer::hide_console(&mut command);
    let listed = command
        .output()
        .ok()
        .filter(|output| output.status.success())
        .is_some_and(|output| String::from_utf8_lossy(&output.stdout).contains(JAVAFX_MODULE));
    listed || java_home(java).is_some_and(|home| bundles_jfxrt(&home))
}

fn bundles_jfxrt(home: &Path) -> bool {
    ["jre/lib/ext/jfxrt.jar", "lib/ext/jfxrt.jar"]
        .iter()
        .any(|relative| home.join(relative).is_file())
}

fn bundled_java(install: &Path) -> Option<PathBuf> {
    BUNDLED_JAVA
        .iter()
        .map(|relative| install.join(relative))
        .find(|java| java.is_file())
}

fn gearth_installs(roots: &[PathBuf]) -> Vec<PathBuf> {
    roots
        .iter()
        .flat_map(|root| installs_under(root, SEARCH_DEPTH))
        .collect()
}

fn installs_under(folder: &Path, depth: usize) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut children: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    children.sort();
    let mut found: Vec<PathBuf> = children
        .iter()
        .filter(|path| is_gearth_install(path))
        .cloned()
        .collect();
    if depth > 0 {
        for child in children.iter().filter(|path| path.is_dir()) {
            found.extend(installs_under(child, depth - 1));
        }
    }
    found
}

fn named_gearth(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.to_lowercase().starts_with(GEARTH_PREFIX))
}

#[cfg(target_os = "macos")]
const BUNDLED_JAVA: &[&str] = &["Contents/PlugIns/jre.jre/Contents/Home/bin/java"];

#[cfg(target_os = "macos")]
fn is_gearth_install(path: &Path) -> bool {
    path.is_dir() && named_gearth(path) && path.extension().is_some_and(|ext| ext == "app")
}

#[cfg(target_os = "macos")]
fn search_roots() -> Vec<PathBuf> {
    let home = dirs::home_dir();
    [
        Some(PathBuf::from("/Applications")),
        home.as_ref().map(|home| home.join("Applications")),
        dirs::download_dir(),
        dirs::desktop_dir(),
        dirs::document_dir(),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[cfg(target_os = "windows")]
const BUNDLED_JAVA: &[&str] = &["runtime/bin/java.exe", "jre/bin/java.exe"];

#[cfg(target_os = "windows")]
fn is_gearth_install(path: &Path) -> bool {
    path.is_dir()
        && std::fs::read_dir(path).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                let file = entry.path();
                file.is_file()
                    && named_gearth(&file)
                    && file
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
            })
        })
}

#[cfg(target_os = "windows")]
fn search_roots() -> Vec<PathBuf> {
    [
        std::env::var_os("ProgramFiles").map(PathBuf::from),
        dirs::data_local_dir().map(|local| local.join("Programs")),
        dirs::download_dir(),
        dirs::desktop_dir(),
        dirs::document_dir(),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const BUNDLED_JAVA: &[&str] = &[];

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn is_gearth_install(_: &Path) -> bool {
    false
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn search_roots() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    const INSTALL: &str = "G-Earth-1.5.4-beta-29/G-Earth.app";
    #[cfg(target_os = "windows")]
    const INSTALL: &str = "G-Earth-1.5.4-beta-29";

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn finds_the_java_bundled_with_a_gearth_install_in_a_root() {
        let root = tempfile::tempdir().expect("temp dir");
        let install = root.path().join(INSTALL);
        let java = install.join(BUNDLED_JAVA[0]);
        std::fs::create_dir_all(java.parent().expect("bin")).expect("creates bin");
        std::fs::write(&java, "").expect("writes java");
        std::fs::write(install.join("G-Earth.exe"), "").expect("writes launcher");
        std::fs::create_dir_all(root.path().join("Unrelated/Thing.app")).expect("creates other");

        let found: Vec<PathBuf> = gearth_installs(&[root.path().to_owned()])
            .iter()
            .filter_map(|install| bundled_java(install))
            .collect();
        assert_eq!(found, [java]);
    }

    #[test]
    fn a_java_extension_runs_with_its_home_and_javafx_opened_after_the_players_options() {
        let java = Path::new("jre").join("bin").join("java");
        let environment = java_environment(&java, Some(OsString::from("-Xmx512m")));
        let value = |name: &str| {
            environment
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.to_string_lossy().into_owned())
        };
        assert_eq!(value("JAVA_HOME").as_deref(), Some("jre"));
        let options = value("JDK_JAVA_OPTIONS").expect("sets the options");
        assert!(options.starts_with("-Xmx512m "), "{options}");
        assert!(
            options.contains("--add-opens=javafx.graphics/javafx.scene.text=ALL-UNNAMED"),
            "{options}"
        );
    }

    #[test]
    fn a_java_home_is_two_folders_above_the_binary() {
        let java = Path::new("jre").join("bin").join("java");
        assert_eq!(java_home(&java), Some(PathBuf::from("jre")));
    }
}
