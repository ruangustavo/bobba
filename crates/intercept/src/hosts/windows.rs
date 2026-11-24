use std::ffi::{OsStr, OsString};
use std::net::Ipv4Addr;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_CANCELLED, GetLastError, HANDLE, MAX_PATH, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, TerminateProcess, WaitForSingleObject,
};
use windows_sys::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

use super::{HostsError, command_line};

pub struct Elevated {
    process: HANDLE,
}

unsafe impl Send for Elevated {}

impl Elevated {
    pub fn has_exited(&mut self) -> bool {
        let waited = unsafe { WaitForSingleObject(self.process, 0) };
        waited != WAIT_TIMEOUT
    }

    pub fn kill(&mut self) {
        unsafe { TerminateProcess(self.process, 1) };
    }
}

impl Drop for Elevated {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.process) };
    }
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}

pub fn elevate(program: &Path, arguments: &[String]) -> Result<Elevated, HostsError> {
    let verb = wide(OsStr::new("runas"));
    let file = wide(program.as_os_str());
    let parameters = wide(OsStr::new(&command_line::windows(
        arguments.iter().map(String::as_str),
    )));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: u32::try_from(std::mem::size_of::<SHELLEXECUTEINFOW>()).unwrap_or(u32::MAX),
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: verb.as_ptr(),
        lpFile: file.as_ptr(),
        lpParameters: parameters.as_ptr(),
        nShow: SW_HIDE,
        ..Default::default()
    };
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        let error = unsafe { GetLastError() };
        return Err(if error == ERROR_CANCELLED {
            HostsError::NotApproved
        } else {
            HostsError::Io(std::io::Error::from_raw_os_error(error.cast_signed()).to_string())
        });
    }
    if info.hProcess.is_null() {
        return Err(HostsError::NotApproved);
    }
    Ok(Elevated {
        process: info.hProcess,
    })
}

pub fn hosts_file() -> std::io::Result<PathBuf> {
    let mut buffer = [0u16; MAX_PATH as usize];
    let written = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), MAX_PATH) } as usize;
    if written == 0 || written > buffer.len() {
        return Err(std::io::Error::last_os_error());
    }
    Ok(PathBuf::from(OsString::from_wide(&buffer[..written]))
        .join("drivers")
        .join("etc")
        .join("hosts"))
}

pub fn alias_loopback(_: Ipv4Addr) {}

pub fn flush_dns() {
    let _ = Command::new("ipconfig")
        .arg("/flushdns")
        .creation_flags(CREATE_NO_WINDOW)
        .output();
}
