use std::ffi::c_void;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY,
    PAGE_GUARD, PAGE_PROTECTION_FLAGS, PAGE_READWRITE, PAGE_WRITECOPY, VirtualQueryEx,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
};

use super::{HabboMemory, Region, Unreadable};

pub const ACCESS_DENIED: &str =
    "Windows denied access to Habbo's memory; run Bobba as the same user as Habbo";
const HABBO_EXECUTABLE: &str = "Habbo.exe";
const WRITABLE: PAGE_PROTECTION_FLAGS =
    PAGE_READWRITE | PAGE_EXECUTE_READWRITE | PAGE_WRITECOPY | PAGE_EXECUTE_WRITECOPY;

pub struct Habbo {
    process: HANDLE,
}

pub fn open_habbo() -> Result<Habbo, Unreadable> {
    let pid = habbo_pid().ok_or(Unreadable::NotRunning)?;
    let process = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid) };
    if process.is_null() {
        return Err(Unreadable::Denied);
    }
    Ok(Habbo { process })
}

fn habbo_pid() -> Option<u32> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(std::mem::size_of::<PROCESSENTRY32W>()).unwrap_or(u32::MAX),
        ..Default::default()
    };
    let mut found = None;
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while more {
        if executable_name(&entry).eq_ignore_ascii_case(HABBO_EXECUTABLE) {
            found = Some(entry.th32ProcessID);
            break;
        }
        more = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    unsafe { CloseHandle(snapshot) };
    found
}

fn executable_name(entry: &PROCESSENTRY32W) -> String {
    let len = entry
        .szExeFile
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(entry.szExeFile.len());
    String::from_utf16_lossy(&entry.szExeFile[..len])
}

impl HabboMemory for Habbo {
    fn writable_regions(&self) -> Vec<Region> {
        let mut regions = Vec::new();
        let mut address: usize = 0;
        loop {
            let mut info = MEMORY_BASIC_INFORMATION::default();
            let written = unsafe {
                VirtualQueryEx(
                    self.process,
                    address as *const c_void,
                    &mut info,
                    std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
                )
            };
            if written == 0 || info.RegionSize == 0 {
                return regions;
            }
            let committed = info.State == MEM_COMMIT;
            let writable = info.Protect & WRITABLE != 0 && info.Protect & PAGE_GUARD == 0;
            if committed && writable {
                regions.push(Region {
                    base: info.BaseAddress as u64,
                    len: info.RegionSize as u64,
                });
            }
            let Some(next) = (info.BaseAddress as usize).checked_add(info.RegionSize) else {
                return regions;
            };
            address = next;
        }
    }

    fn read(&self, at: u64, into: &mut [u8]) -> usize {
        let Ok(at) = usize::try_from(at) else {
            return 0;
        };
        let mut read: usize = 0;
        let succeeded = unsafe {
            ReadProcessMemory(
                self.process,
                at as *const c_void,
                into.as_mut_ptr().cast(),
                into.len(),
                &mut read,
            )
        } != 0;
        if succeeded { read.min(into.len()) } else { 0 }
    }
}

impl Drop for Habbo {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.process) };
    }
}
