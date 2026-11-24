use std::io;
use std::path::{Path, PathBuf};

const MARKER: &str = "store-hint-dismissed";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreHint {
    Loading,
    Open,
    Dismissed,
}

impl StoreHint {
    pub fn default_dir() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("Bobba")
    }

    pub fn load(dir: &Path) -> Self {
        if dir.join(MARKER).exists() {
            StoreHint::Dismissed
        } else {
            StoreHint::Open
        }
    }

    pub fn remember_dismissed(dir: &Path) -> io::Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join(MARKER), "")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_install_shows_the_hint() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(StoreHint::load(dir.path()), StoreHint::Open);
    }

    #[test]
    fn a_dismissed_hint_stays_dismissed_on_the_next_run() {
        let dir = tempfile::tempdir().expect("temp dir");
        let bobba = dir.path().join("Bobba");
        StoreHint::remember_dismissed(&bobba).expect("marker written");
        assert_eq!(StoreHint::load(&bobba), StoreHint::Dismissed);
    }
}
