use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;

use bobba_protocol::key_recovery::{SBox, parse_table_dump};

pub type TablesFuture<'a> = Pin<Box<dyn Future<Output = Vec<SBox>> + Send + 'a>>;

pub trait TableSource: Send + Sync + 'static {
    fn tables(&self) -> TablesFuture<'_>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoTables;

impl TableSource for NoTables {
    fn tables(&self) -> TablesFuture<'_> {
        Box::pin(std::future::ready(Vec::new()))
    }
}

#[derive(Debug, Clone)]
pub struct GMemZ {
    pub binary: PathBuf,
}

impl TableSource for GMemZ {
    fn tables(&self) -> TablesFuture<'_> {
        Box::pin(async move {
            let output = tokio::process::Command::new(&self.binary)
                .arg("flash")
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .await;
            output
                .map(|output| parse_table_dump(&String::from_utf8_lossy(&output.stdout)))
                .unwrap_or_default()
        })
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
                .unwrap_or_default()
        })
    }
}
