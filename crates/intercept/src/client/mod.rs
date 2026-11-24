#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(not(target_os = "macos"), path = "unprepared.rs")]
mod platform;

use std::path::PathBuf;

pub use platform::{client_state, prepare};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientState {
    NotInstalled,
    NeedsPreparation(PathBuf),
    Ready,
}

#[derive(Debug, thiserror::Error)]
pub enum PrepareError {
    #[error("Habbo is running; quit it before preparing the client")]
    Running,
    #[error("codesign failed: {0}")]
    Codesign(String),
    #[error(
        "macOS blocked changing Habbo.app; allow Bobba under System Settings › Privacy & Security › App Management ({0})"
    )]
    Blocked(String),
    #[error("Habbo runs on this system without being prepared")]
    NotNeeded,
}
