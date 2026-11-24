use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bobba_extensions::PacketInfoLoader;
use bobba_protocol::PacketInfo;

pub const SULEK_API: &str = "https://api.sulek.dev";
const FETCH_TIMEOUT: Duration = Duration::from_secs(5);
const UNSURE_CACHE_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);

pub struct Sulek {
    http: reqwest::Client,
    base_url: String,
    cache_dir: PathBuf,
}

impl Sulek {
    pub fn new(base_url: impl Into<String>, cache_dir: impl Into<PathBuf>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(FETCH_TIMEOUT)
            .user_agent(concat!("Bobba/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Self {
            http,
            base_url: base_url.into(),
            cache_dir: cache_dir.into(),
        }
    }

    pub fn with_default_cache() -> Self {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("bobba")
            .join("sulek");
        Self::new(SULEK_API, cache_dir)
    }

    fn cache_path(&self, hotel_version: &str) -> Option<PathBuf> {
        let safe = !hotel_version.is_empty()
            && hotel_version
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        safe.then(|| self.cache_dir.join(format!("{hotel_version}.json")))
    }

    async fn fetch(&self, hotel_version: &str) -> Option<String> {
        let url = format!("{}/releases/{hotel_version}/messages", self.base_url);
        let response = self.http.get(url).send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }
        response.text().await.ok()
    }
}

fn read_cache(path: &Path, hotel_version: &str) -> Option<(PacketInfo, Duration)> {
    let json = std::fs::read_to_string(path).ok()?;
    let info = PacketInfo::from_sulek_json(hotel_version, &json).ok()?;
    let age = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .unwrap_or(Duration::MAX);
    Some((info, age))
}

impl PacketInfoLoader for Sulek {
    async fn load(&self, hotel_version: &str) -> PacketInfo {
        let Some(path) = self.cache_path(hotel_version) else {
            return PacketInfo::empty(hotel_version);
        };
        let cached = read_cache(&path, hotel_version);
        if let Some((info, age)) = &cached
            && (info.is_fully_confident() || *age < UNSURE_CACHE_LIFETIME)
        {
            return info.clone();
        }
        let fetched = self.fetch(hotel_version).await.and_then(|json| {
            PacketInfo::from_sulek_json(hotel_version, &json)
                .ok()
                .map(|info| (info, json))
        });
        match (fetched, cached) {
            (Some((info, json)), _) => {
                let _ = std::fs::create_dir_all(&self.cache_dir);
                let _ = std::fs::write(&path, json);
                info
            }
            (None, Some((stale, _))) => stale,
            (None, None) => PacketInfo::empty(hotel_version),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use bobba_protocol::Direction;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    const BODY: &str = r#"{"messages":{"incoming":[],"outgoing":[{"id":597,"name":"ChatMessageComposer","confident":true}]}}"#;
    const UNSURE_BODY: &str =
        r#"{"messages":{"incoming":[],"outgoing":[{"id":597,"name":"_-4IS","confident":false}]}}"#;

    async fn fake_sulek(body: &'static str) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("binds");
        let url = format!("http://{}", listener.local_addr().expect("address"));
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                counter.fetch_add(1, Ordering::SeqCst);
                let mut request = [0u8; 1024];
                let _ = stream.read(&mut request).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });
        (url, hits)
    }

    #[tokio::test]
    async fn fetches_then_serves_confident_names_from_cache() {
        let cache = tempfile::tempdir().expect("temp dir");
        let (url, hits) = fake_sulek(BODY).await;
        let sulek = Sulek::new(url, cache.path());
        let first = sulek.load("MAC63-1").await;
        let second = sulek.load("MAC63-1").await;
        assert_eq!(first.name(Direction::ToServer, 597), Some("Chat"));
        assert_eq!(second.name(Direction::ToServer, 597), Some("Chat"));
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn unsure_names_are_cached_for_a_day_only() {
        let cache = tempfile::tempdir().expect("temp dir");
        let (url, hits) = fake_sulek(UNSURE_BODY).await;
        let sulek = Sulek::new(url, cache.path());
        sulek.load("MAC63-2").await;
        sulek.load("MAC63-2").await;
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        let path = cache.path().join("MAC63-2.json");
        let old = SystemTime::now() - Duration::from_secs(2 * 24 * 60 * 60);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|f| f.set_modified(old))
            .expect("ages the cache");
        sulek.load("MAC63-2").await;
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn offline_without_cache_falls_back_to_ids_only() {
        let cache = tempfile::tempdir().expect("temp dir");
        let sulek = Sulek::new("http://127.0.0.1:9", cache.path());
        assert!(sulek.load("MAC63-3").await.is_empty());
    }

    #[tokio::test]
    async fn refuses_versions_that_would_escape_the_cache_dir() {
        let cache = tempfile::tempdir().expect("temp dir");
        let (url, hits) = fake_sulek(BODY).await;
        let sulek = Sulek::new(url, cache.path());
        assert!(sulek.load("../../etc/passwd").await.is_empty());
        assert_eq!(hits.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    #[ignore = "hits api.sulek.dev"]
    async fn real_sulek_names_a_known_mac_build() {
        let cache = tempfile::tempdir().expect("temp dir");
        let sulek = Sulek::new(SULEK_API, cache.path());
        let info = sulek.load("MAC63-202611041140-63291706").await;
        assert_eq!(info.name(Direction::ToServer, 597), Some("Chat"));
        assert_eq!(info.name(Direction::ToClient, 2350), Some("Chat"));
    }
}
