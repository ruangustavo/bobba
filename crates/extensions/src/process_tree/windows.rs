use std::io;

use tokio::process::Child;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};

pub struct ProcessTree {
    job: HANDLE,
}

unsafe impl Send for ProcessTree {}

impl ProcessTree {
    pub fn adopt(child: &Child) -> io::Result<Self> {
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(io::Error::last_os_error());
        }
        let tree = Self { job };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let size = u32::try_from(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
            .unwrap_or(u32::MAX);
        let limited = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size,
            )
        };
        if limited == 0 {
            return Err(io::Error::last_os_error());
        }
        let Some(process) = child.raw_handle() else {
            return Ok(tree);
        };
        if unsafe { AssignProcessToJobObject(job, process) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(tree)
    }

    pub fn end(self) {
        drop(self);
    }
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.job) };
    }
}
