use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use bobba_protocol::key_recovery::{SBox, parse_table_dump};

pub type TablesFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<SBox>, TablesUnavailable>> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct TablesUnavailable(pub String);

pub trait TableSource: Send + Sync + 'static {
    fn tables(&self) -> TablesFuture<'_>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoTables;

impl TableSource for NoTables {
    fn tables(&self) -> TablesFuture<'_> {
        Box::pin(std::future::ready(Ok(Vec::new())))
    }
}

#[derive(Debug, Clone)]
pub struct DumpFile {
    pub path: PathBuf,
}

impl TableSource for DumpFile {
    fn tables(&self) -> TablesFuture<'_> {
        Box::pin(async move {
            tokio::fs::read_to_string(&self.path)
                .await
                .map(|text| parse_table_dump(&text))
                .map_err(|error| {
                    TablesUnavailable(format!("could not read {}: {error}", self.path.display()))
                })
        })
    }
}
