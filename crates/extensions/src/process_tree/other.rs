use std::io;

use tokio::process::Child;

pub struct ProcessTree;

impl ProcessTree {
    pub fn adopt(_: &Child) -> io::Result<Self> {
        Ok(Self)
    }

    pub fn end(self) {}
}
