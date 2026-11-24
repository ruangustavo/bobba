use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const PINNED_URL: &str = "https://raw.githubusercontent.com/G-Realm/G-Earth/3aaa926e2b9b281f8af934226462e13027847620/G-Earth/src/main/resources/build/mac/G-MemZ";
const PINNED_SHA256: &str = "9a7d6bb255626b0f4385dccf527f0e57f717c6e8bfcdcb40ad106cb5c256a17b";

#[derive(Debug, thiserror::Error)]
pub enum GMemZError {
    #[error("could not download G-MemZ: {0}")]
    Download(String),
    #[error("downloaded G-MemZ does not match the pinned checksum")]
    Checksum,
    #[error("could not install G-MemZ: {0}")]
    Io(#[from] std::io::Error),
}

pub fn default_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Bobba")
        .join("G-MemZ")
}

pub fn is_installed(path: &Path) -> bool {
    std::fs::read(path).is_ok_and(|bytes| sha256_hex(&bytes) == PINNED_SHA256)
}

pub async fn ensure_installed(path: &Path) -> Result<(), GMemZError> {
    if is_installed(path) {
        return Ok(());
    }
    let bytes = reqwest::get(PINNED_URL)
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| GMemZError::Download(e.to_string()))?
        .bytes()
        .await
        .map_err(|e| GMemZError::Download(e.to_string()))?;
    if sha256_hex(&bytes) != PINNED_SHA256 {
        return Err(GMemZError::Checksum);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, &bytes)?;
    make_executable(path)?;
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(unix)]
fn make_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_with_other_content_does_not_count_as_installed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("G-MemZ");
        assert!(!is_installed(&path));
        std::fs::write(&path, b"not the real binary").expect("writes");
        assert!(!is_installed(&path));
    }

    #[tokio::test]
    #[ignore = "downloads from GitHub"]
    async fn downloads_the_pinned_binary() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("G-MemZ");
        ensure_installed(&path).await.expect("downloads");
        assert!(is_installed(&path));
    }
}
