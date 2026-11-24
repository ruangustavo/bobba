use std::io;
use std::path::{Path, PathBuf};

use crate::installer::{InstalledExtension, Installer, Package, Runtime};

const EXTENSIONS_DIR: &str = "extensions";
const GEARTH_PREFIX: &str = "g-earth";
const SEARCH_DEPTH: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GEarthImport {
    pub folder: PathBuf,
    pub extensions: Vec<PathBuf>,
}

impl GEarthImport {
    pub fn find(roots: &[PathBuf], installer: &Installer) -> Option<Self> {
        roots
            .iter()
            .flat_map(|root| gearth_folders(root, SEARCH_DEPTH))
            .find_map(|folder| Self::from_folder(&folder, installer))
    }

    pub fn from_folder(folder: &Path, installer: &Installer) -> Option<Self> {
        let extensions_dir = child_named(folder, EXTENSIONS_DIR)?;
        let have: Vec<String> = installer
            .installed()
            .iter()
            .map(|e| match e {
                InstalledExtension::Single(single) => single.file_name.to_lowercase(),
                InstalledExtension::Package(package) => package.title.to_lowercase(),
            })
            .collect();
        let mut extensions: Vec<PathBuf> = std::fs::read_dir(&extensions_dir)
            .ok()?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| match identity(path) {
                Some(name) => !have.contains(&name.to_lowercase()),
                None => false,
            })
            .collect();
        extensions.sort();
        (!extensions.is_empty()).then(|| GEarthImport {
            folder: folder.to_owned(),
            extensions,
        })
    }

    pub fn import(&self, installer: &Installer) -> io::Result<Vec<String>> {
        std::fs::create_dir_all(installer.dir())?;
        let mut imported = Vec::new();
        for source in &self.extensions {
            let Some(name) = source.file_name() else {
                continue;
            };
            let target = installer.dir().join(name);
            if target.exists() {
                continue;
            }
            if source.is_dir() {
                copy_dir(source, &target)?;
            } else {
                std::fs::copy(source, &target)?;
            }
            imported.push(name.to_string_lossy().into_owned());
        }
        Ok(imported)
    }
}

fn identity(path: &Path) -> Option<String> {
    if path.is_dir() {
        return Package::read(path.to_owned()).map(|package| package.title);
    }
    Runtime::for_path(path)?;
    path.file_name()?.to_str().map(str::to_owned)
}

fn child_named(folder: &Path, wanted: &str) -> Option<PathBuf> {
    std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.eq_ignore_ascii_case(wanted))
        })
}

fn has_gearth(folder: &Path) -> bool {
    std::fs::read_dir(folder).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            let path = entry.path();
            path.is_file()
                && path
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.to_lowercase().starts_with(GEARTH_PREFIX))
        })
    })
}

fn gearth_folders(root: &Path, depth: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if has_gearth(root) && child_named(root, EXTENSIONS_DIR).is_some() {
        found.push(root.to_owned());
    }
    if depth == 0 {
        return found;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    let mut children: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    children.sort();
    for child in children {
        found.extend(gearth_folders(&child, depth - 1));
    }
    found
}

fn copy_dir(source: &Path, target: &Path) -> io::Result<()> {
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)?.flatten() {
        let from = entry.path();
        let to = target.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gearth(root: &Path, at: &str) -> PathBuf {
        let folder = root.join(at);
        let extensions = folder.join("Extensions");
        std::fs::create_dir_all(&extensions).expect("creates");
        std::fs::write(folder.join("G-Earth.jar"), b"jar").expect("writes");
        std::fs::write(extensions.join("antispam-48120.py"), b"print()").expect("writes");
        std::fs::write(extensions.join("notes.txt"), b"x").expect("writes");
        let package = extensions.join("Room Mapper_2.0.1");
        std::fs::create_dir_all(package.join("extension/bin")).expect("creates");
        std::fs::write(package.join("command.txt"), r#"["./bin/run"]"#).expect("writes");
        std::fs::write(package.join("extension/bin/run"), b"#!/bin/sh").expect("writes");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                package.join("extension/bin/run"),
                std::fs::Permissions::from_mode(0o755),
            )
            .expect("chmod");
        }
        folder
    }

    #[test]
    fn finds_gearth_two_levels_under_a_root_and_lists_only_new_extensions() {
        let home = tempfile::tempdir().expect("temp dir");
        let bobba = tempfile::tempdir().expect("temp dir");
        let folder = gearth(home.path(), "Tools/G-Earth 1.5.4");
        let installer = Installer::new(bobba.path());

        let found = GEarthImport::find(&[home.path().to_owned()], &installer).expect("found");

        assert_eq!(found.folder, folder);
        let names: Vec<String> = found
            .extensions
            .iter()
            .filter_map(|p| p.file_name()?.to_str().map(str::to_owned))
            .collect();
        assert_eq!(names, ["Room Mapper_2.0.1", "antispam-48120.py"]);
    }

    #[test]
    fn ignores_folders_deeper_than_two_levels_and_without_gearth() {
        let home = tempfile::tempdir().expect("temp dir");
        let bobba = tempfile::tempdir().expect("temp dir");
        gearth(home.path(), "a/b/c");
        std::fs::create_dir_all(home.path().join("Other/Extensions")).expect("creates");
        let installer = Installer::new(bobba.path());
        assert_eq!(
            GEarthImport::find(&[home.path().to_owned()], &installer),
            None
        );
    }

    #[test]
    fn importing_copies_both_kinds_and_then_nothing_is_left_to_offer() {
        let home = tempfile::tempdir().expect("temp dir");
        let bobba = tempfile::tempdir().expect("temp dir");
        let folder = gearth(home.path(), "G-Earth");
        let installer = Installer::new(bobba.path());
        let found = GEarthImport::from_folder(&folder, &installer).expect("found");

        let imported = found.import(&installer).expect("imports");

        assert_eq!(imported.len(), 2);
        let ids: Vec<String> = installer
            .installed()
            .iter()
            .map(|e| e.id().to_owned())
            .collect();
        assert_eq!(ids, ["Room Mapper_2.0.1", "antispam-48120.py"]);
        assert!(folder.join("Extensions/antispam-48120.py").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(bobba.path().join("Room Mapper_2.0.1/extension/bin/run"))
                .expect("copied")
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111);
        }
        assert_eq!(GEarthImport::from_folder(&folder, &installer), None);
    }

    #[test]
    fn a_store_extension_already_in_bobba_is_not_offered_again_whatever_its_case() {
        let home = tempfile::tempdir().expect("temp dir");
        let bobba = tempfile::tempdir().expect("temp dir");
        let folder = gearth(home.path(), "G-Earth");
        std::fs::create_dir_all(bobba.path().join("room mapper_1.0")).expect("creates");
        std::fs::write(bobba.path().join("room mapper_1.0/command.txt"), r#"["x"]"#)
            .expect("writes");
        let installer = Installer::new(bobba.path());
        let found = GEarthImport::from_folder(&folder, &installer).expect("found");
        assert_eq!(found.extensions.len(), 1);
    }
}
