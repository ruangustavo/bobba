mod catalog;

use std::path::PathBuf;
use std::time::Duration;

pub use catalog::{Catalog, Category, Framework, Listing, Os, is_newer};

pub const STORE_SOURCE: &str =
    "https://raw.githubusercontent.com/sirjonasxx/G-ExtensionStore/repo/1.5.3";
const CATALOG_TIMEOUT: Duration = Duration::from_secs(10);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);
const CONFIG_FILE: &str = "config.json";
const EXTENSIONS_FILE: &str = "extensions.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Fresh,
    Saved,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("the store is unreachable and there is no saved copy of the catalog")]
    Unreachable,
    #[error("the store sent a catalog Bobba cannot read: {0}")]
    Unreadable(#[from] serde_json::Error),
    #[error("could not download {title}: {reason}")]
    Download { title: String, reason: String },
}

pub struct Store {
    http: reqwest::Client,
    base_url: String,
    cache_dir: PathBuf,
    os: Os,
}

impl Store {
    pub fn new(base_url: impl Into<String>, cache_dir: impl Into<PathBuf>, os: Os) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("Bobba/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Self {
            http,
            base_url: base_url.into(),
            cache_dir: cache_dir.into(),
            os,
        }
    }

    pub fn with_default_cache() -> Self {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("bobba")
            .join("store");
        Self::new(STORE_SOURCE, cache_dir, Os::current())
    }

    pub fn os(&self) -> Os {
        self.os
    }

    pub async fn load(&self) -> Result<(Catalog, Origin), StoreError> {
        let config = self.fetch_text("store/config.json").await;
        let extensions = self.fetch_text(".auto-generated/extensions.json").await;
        if let (Some(config), Some(extensions)) = (config, extensions)
            && let Ok(catalog) = Catalog::parse(&config, &extensions, self.os)
        {
            let _ = std::fs::create_dir_all(&self.cache_dir);
            let _ = std::fs::write(self.cache_dir.join(CONFIG_FILE), &config);
            let _ = std::fs::write(self.cache_dir.join(EXTENSIONS_FILE), &extensions);
            return Ok((catalog, Origin::Fresh));
        }
        let saved = std::fs::read_to_string(self.cache_dir.join(CONFIG_FILE))
            .ok()
            .zip(std::fs::read_to_string(self.cache_dir.join(EXTENSIONS_FILE)).ok());
        let Some((config, extensions)) = saved else {
            return Err(StoreError::Unreachable);
        };
        Ok((
            Catalog::parse(&config, &extensions, self.os)?,
            Origin::Saved,
        ))
    }

    pub async fn download(&self, title: &str) -> Result<Vec<u8>, StoreError> {
        let failed = |reason: String| StoreError::Download {
            title: title.to_owned(),
            reason,
        };
        let url = format!(
            "{}/store/extensions/{}/extension.zip",
            self.base_url,
            encode_component(title)
        );
        let response = self
            .http
            .get(url)
            .timeout(DOWNLOAD_TIMEOUT)
            .send()
            .await
            .map_err(|e| failed(e.to_string()))?;
        if !response.status().is_success() {
            return Err(failed(response.status().to_string()));
        }
        let bytes = response.bytes().await.map_err(|e| failed(e.to_string()))?;
        Ok(bytes.to_vec())
    }

    async fn fetch_text(&self, path: &str) -> Option<String> {
        let url = format!("{}/{path}", self.base_url);
        let response = self
            .http
            .get(url)
            .timeout(CATALOG_TIMEOUT)
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        response.text().await.ok()
    }
}

fn encode_component(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    const CONFIG: &str = r#"{"categories": [], "frameworks": []}"#;
    const EXTENSIONS: &str = r#"[{"title": "Room Mapper", "version": "2.0.1",
        "framework": {"name": "Native"}, "commands": {"default": ["java", "-jar", "m.jar"]},
        "compatibility": {"systems": ["Mac", "Windows"], "clients": ["Flash"]}}]"#;

    async fn fake_store() -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("binds");
        let url = format!("http://{}", listener.local_addr().expect("address"));
        let paths = Arc::new(Mutex::new(Vec::new()));
        let seen = paths.clone();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut request = [0u8; 2048];
                let read = stream.read(&mut request).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&request[..read]).to_string();
                let path = head.split(' ').nth(1).unwrap_or_default().to_owned();
                seen.lock().expect("lock").push(path.clone());
                let body: &[u8] = if path.ends_with("config.json") {
                    CONFIG.as_bytes()
                } else if path.ends_with("extensions.json") {
                    EXTENSIONS.as_bytes()
                } else {
                    b"PK-zip-bytes"
                };
                let head = format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes()).await;
                let _ = stream.write_all(body).await;
            }
        });
        (url, paths)
    }

    #[tokio::test]
    async fn loads_the_catalog_and_keeps_a_copy_for_offline_use() {
        let cache = tempfile::tempdir().expect("temp dir");
        let (url, _) = fake_store().await;

        let (online, origin) = Store::new(url, cache.path(), Os::Mac)
            .load()
            .await
            .expect("loads");
        assert_eq!(origin, Origin::Fresh);
        assert_eq!(online.listings[0].title, "Room Mapper");

        let (offline, origin) = Store::new("http://127.0.0.1:9", cache.path(), Os::Mac)
            .load()
            .await
            .expect("uses the saved copy");
        assert_eq!(origin, Origin::Saved);
        assert_eq!(offline, online);
    }

    #[tokio::test]
    async fn offline_without_a_saved_copy_is_unreachable() {
        let cache = tempfile::tempdir().expect("temp dir");
        let store = Store::new("http://127.0.0.1:9", cache.path(), Os::Mac);
        assert!(matches!(store.load().await, Err(StoreError::Unreachable)));
    }

    #[tokio::test]
    async fn downloads_the_archive_by_its_encoded_title() {
        let cache = tempfile::tempdir().expect("temp dir");
        let (url, paths) = fake_store().await;
        let store = Store::new(url, cache.path(), Os::Mac);

        let bytes = store.download("Room Mapper").await.expect("downloads");

        assert_eq!(bytes, b"PK-zip-bytes");
        assert_eq!(
            paths.lock().expect("lock").last().map(String::as_str),
            Some("/store/extensions/Room%20Mapper/extension.zip")
        );
    }

    #[tokio::test]
    #[ignore = "hits raw.githubusercontent.com"]
    async fn real_store_lists_flash_extensions() {
        let cache = tempfile::tempdir().expect("temp dir");
        let store = Store::new(STORE_SOURCE, cache.path(), Os::Mac);
        let (catalog, _) = store.load().await.expect("loads");
        assert!(catalog.listings.len() > 40, "{}", catalog.listings.len());
        let qx = catalog
            .listing("QX Scripter")
            .expect("QX Scripter is listed");
        assert_eq!(qx.command[0], "/bin/sh");
        let archive = store.download("QX Scripter").await.expect("downloads");
        assert!(archive.starts_with(b"PK"));
    }
}
