use std::ffi::c_int;

use libc::{PROC_PIDPATHINFO_MAXSIZE, proc_listallpids, proc_pidpath};
use mach2::kern_return::KERN_SUCCESS;
use mach2::mach_port::mach_port_deallocate;
use mach2::port::{MACH_PORT_NULL, mach_port_name_t, mach_port_t};
use mach2::traps::{mach_task_self, task_for_pid};
use mach2::vm::{mach_vm_read_overwrite, mach_vm_region};
use mach2::vm_prot::{VM_PROT_READ, VM_PROT_WRITE, vm_prot_t};
use mach2::vm_region::{VM_REGION_BASIC_INFO_64, vm_region_basic_info_64};
use mach2::vm_types::{mach_vm_address_t, mach_vm_size_t};

use super::{HabboMemory, Region, Unreadable};

pub const ACCESS_DENIED: &str = "macOS denied access to Habbo's memory; prepare Habbo again";
const HABBO_EXECUTABLE: &str = "Habbo.app/Contents/MacOS/Habbo";
const HEAP_PROTECTION: vm_prot_t = VM_PROT_READ | VM_PROT_WRITE;

pub struct Habbo {
    task: mach_port_name_t,
}

pub fn open_habbo() -> Result<Habbo, Unreadable> {
    let pid = running_pids()
        .into_iter()
        .find(|pid| executable_path(*pid).is_some_and(|path| path.ends_with(HABBO_EXECUTABLE)))
        .ok_or(Unreadable::NotRunning)?;
    let mut task: mach_port_name_t = MACH_PORT_NULL;
    let result = unsafe { task_for_pid(mach_task_self(), pid, &mut task) };
    if result != KERN_SUCCESS {
        return Err(Unreadable::Denied);
    }
    Ok(Habbo { task })
}

fn running_pids() -> Vec<c_int> {
    let estimate = unsafe { proc_listallpids(std::ptr::null_mut(), 0) };
    let Ok(estimate) = usize::try_from(estimate) else {
        return Vec::new();
    };
    let mut pids: Vec<c_int> = vec![0; estimate + 64];
    let capacity = c_int::try_from(std::mem::size_of_val(pids.as_slice())).unwrap_or(c_int::MAX);
    let count = unsafe { proc_listallpids(pids.as_mut_ptr().cast(), capacity) };
    pids.truncate(usize::try_from(count).unwrap_or(0));
    pids
}

fn executable_path(pid: c_int) -> Option<String> {
    let mut buffer = [0u8; PROC_PIDPATHINFO_MAXSIZE as usize];
    let capacity = u32::try_from(buffer.len()).ok()?;
    let len = unsafe { proc_pidpath(pid, buffer.as_mut_ptr().cast(), capacity) };
    let len = usize::try_from(len).ok().filter(|len| *len > 0)?;
    Some(String::from_utf8_lossy(&buffer[..len]).into_owned())
}

impl HabboMemory for Habbo {
    fn writable_regions(&self) -> Vec<Region> {
        let mut regions = Vec::new();
        let mut address: mach_vm_address_t = 0;
        loop {
            let mut size: mach_vm_size_t = 0;
            let mut info = vm_region_basic_info_64::default();
            let mut count = vm_region_basic_info_64::count();
            let mut object: mach_port_t = MACH_PORT_NULL;
            let result = unsafe {
                mach_vm_region(
                    self.task,
                    &mut address,
                    &mut size,
                    VM_REGION_BASIC_INFO_64,
                    (&raw mut info).cast(),
                    &mut count,
                    &mut object,
                )
            };
            if result != KERN_SUCCESS {
                return regions;
            }
            let protection = info.protection;
            if protection & HEAP_PROTECTION == HEAP_PROTECTION {
                regions.push(Region {
                    base: address,
                    len: size,
                });
            }
            address = address.saturating_add(size);
        }
    }

    fn read(&self, at: u64, into: &mut [u8]) -> usize {
        let mut read: mach_vm_size_t = 0;
        let result = unsafe {
            mach_vm_read_overwrite(
                self.task,
                at,
                into.len() as mach_vm_size_t,
                into.as_mut_ptr() as mach_vm_address_t,
                &mut read,
            )
        };
        if result != KERN_SUCCESS {
            return 0;
        }
        usize::try_from(read).map_or(0, |read| read.min(into.len()))
    }
}

impl Drop for Habbo {
    fn drop(&mut self) {
        unsafe {
            mach_port_deallocate(mach_task_self(), self.task);
        }
    }
}
