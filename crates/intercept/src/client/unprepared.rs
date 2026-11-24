use std::path::{Path, PathBuf};

use super::{ClientState, PrepareError};

pub async fn client_state() -> ClientState {
    ClientState::Ready
}

pub async fn prepare(_: &Path) -> Result<PathBuf, PrepareError> {
    Err(PrepareError::NotNeeded)
}
