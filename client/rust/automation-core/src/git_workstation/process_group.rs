//! Narrow owned-child platform adapter. All handles belong to the Git command
//! spawned by the parent; no caller-provided PID, process handle or command.
use super::*;
#[cfg(windows)]
pub(super) struct ProcessGroup(std::os::windows::io::OwnedHandle);
#[cfg(windows)]
impl ProcessGroup {
    pub(super) fn capture(child: &tokio::process::Child) -> Result<Self> {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::System::JobObjects::*;
        // SAFETY: CreateJobObject returns a fresh owned handle; OwnedHandle
        // closes it on every path. FFI pointers reference live exact-size PODs.
        unsafe {
            let raw = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if raw.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            let job = OwnedHandle::from_raw_handle(raw);
            let mut policy: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            policy.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&policy as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&policy) as u32,
            ) == 0
                || child.raw_handle().is_none_or(|process| {
                    AssignProcessToJobObject(job.as_raw_handle(), process) == 0
                })
            {
                return Err(std::io::Error::last_os_error().into());
            }
            Ok(Self(job))
        }
    }
}
#[cfg(unix)]
pub(super) struct ProcessGroup {
    pid: i32,
    pub(super) exited: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
#[cfg(unix)]
impl ProcessGroup {
    pub(super) fn capture(child: &tokio::process::Child) -> Result<Self> {
        Ok(Self {
            pid: child.id().ok_or_else(invalid)? as i32,
            exited: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        })
    }
    pub(super) fn stop(&self) {
        if !self.exited.load(std::sync::atomic::Ordering::SeqCst) {
            // SAFETY: this PID came from our unreaped child started with
            // process_group(0). The wait path retires the guard before reuse.
            unsafe {
                libc::kill(-self.pid, libc::SIGKILL);
            }
        }
    }
}
#[cfg(unix)]
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        self.stop();
    }
}
