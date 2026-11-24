#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(not(any(target_os = "macos", windows)), path = "unsupported.rs")]
mod platform;

use bobba_protocol::key_recovery::SBox;
use bobba_protocol::table_scan::{TABLE_BYTES, scan_tables};
use bobba_session::table_source::TablesFuture;
use bobba_session::{TableSource, TablesUnavailable};

const CHUNK_BYTES: usize = 16 * 1024 * 1024;
const SLOT_BYTES: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub base: u64,
    pub len: u64,
}

pub trait HabboMemory {
    fn writable_regions(&self) -> Vec<Region>;
    fn read(&self, at: u64, into: &mut [u8]) -> usize;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unreadable {
    NotRunning,
    Denied,
    Unsupported,
}

impl From<Unreadable> for TablesUnavailable {
    fn from(reason: Unreadable) -> Self {
        let text = match reason {
            Unreadable::NotRunning => "Habbo isn't running",
            Unreadable::Denied => platform::ACCESS_DENIED,
            Unreadable::Unsupported => "reading Habbo's memory isn't supported on this OS",
        };
        TablesUnavailable(text.to_owned())
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MemoryScan;

impl TableSource for MemoryScan {
    fn tables(&self) -> TablesFuture<'_> {
        Box::pin(async {
            tokio::task::spawn_blocking(scan_habbo)
                .await
                .unwrap_or_else(|error| Err(TablesUnavailable(error.to_string())))
        })
    }
}

pub fn scan_habbo() -> Result<Vec<SBox>, TablesUnavailable> {
    let habbo = platform::open_habbo()?;
    Ok(scan_memory(&habbo, CHUNK_BYTES))
}

fn scan_memory(memory: &impl HabboMemory, chunk_bytes: usize) -> Vec<SBox> {
    let overlap = TABLE_BYTES - SLOT_BYTES;
    let mut buffer = vec![0u8; chunk_bytes];
    let mut in_memory_order = Vec::new();
    for region in memory.writable_regions() {
        let end = region.base + region.len;
        let mut at = region.base;
        while at < end {
            let len = usize::try_from(end - at).map_or(chunk_bytes, |left| left.min(chunk_bytes));
            let read = memory.read(at, &mut buffer[..len]);
            in_memory_order.extend(scan_tables(&buffer[..read]).into_iter().rev());
            if len < chunk_bytes {
                break;
            }
            at += (len - overlap) as u64;
        }
    }
    in_memory_order.dedup();
    in_memory_order.reverse();
    in_memory_order
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeMemory {
        base: u64,
        bytes: Vec<u8>,
    }

    impl HabboMemory for FakeMemory {
        fn writable_regions(&self) -> Vec<Region> {
            vec![Region {
                base: self.base,
                len: self.bytes.len() as u64,
            }]
        }

        fn read(&self, at: u64, into: &mut [u8]) -> usize {
            let start = usize::try_from(at - self.base).unwrap_or(usize::MAX);
            let available = self.bytes.get(start..).unwrap_or_default();
            let len = into.len().min(available.len());
            into[..len].copy_from_slice(&available[..len]);
            len
        }
    }

    fn permutation(seed: u8) -> SBox {
        let mut table = [0u8; 256];
        for (slot, value) in table.iter_mut().zip(0u8..=u8::MAX) {
            *slot = value.wrapping_mul(37).wrapping_add(seed);
        }
        table
    }

    fn as_slots(table: &SBox) -> Vec<u8> {
        table
            .iter()
            .flat_map(|value| u32::from(*value).to_le_bytes())
            .collect()
    }

    fn filler(bytes: usize) -> Vec<u8> {
        vec![0xFF; bytes]
    }

    #[test]
    fn finds_a_table_that_straddles_two_chunks_once() {
        let chunk = 4096;
        let table = permutation(3);
        let bytes = [filler(chunk - 512), as_slots(&table), filler(chunk)].concat();
        let memory = FakeMemory {
            base: 0x1000,
            bytes,
        };
        assert_eq!(scan_memory(&memory, chunk), vec![table]);
    }

    #[test]
    fn returns_tables_from_every_chunk_last_first() {
        let chunk = 4096;
        let first = permutation(3);
        let second = permutation(11);
        let bytes = [
            as_slots(&first),
            filler(chunk * 2),
            as_slots(&second),
            filler(64),
        ]
        .concat();
        let memory = FakeMemory {
            base: 0x1000,
            bytes,
        };
        assert_eq!(scan_memory(&memory, chunk), vec![second, first]);
    }
}
