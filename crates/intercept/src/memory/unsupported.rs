use super::{HabboMemory, Region, Unreadable};

pub const ACCESS_DENIED: &str = "this OS denied access to Habbo's memory";

pub enum Habbo {}

pub fn open_habbo() -> Result<Habbo, Unreadable> {
    Err(Unreadable::Unsupported)
}

impl HabboMemory for Habbo {
    fn writable_regions(&self) -> Vec<Region> {
        match *self {}
    }

    fn read(&self, _: u64, _: &mut [u8]) -> usize {
        match *self {}
    }
}
