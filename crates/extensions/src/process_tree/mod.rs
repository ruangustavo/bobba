#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(not(windows), path = "other.rs")]
mod platform;

pub(crate) use platform::ProcessTree;
