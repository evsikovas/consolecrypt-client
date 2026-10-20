//! A suspended process is assigned to a private kill-on-close job before its
//! first instruction. Closing the job also terminates plugin descendants.

#![allow(unsafe_code)]

use super::AuthError;
use std::{
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{HANDLE, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
        },
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
        Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME},
    },
};

#[derive(Debug)]
pub(super) struct Job(OwnedHandle);

fn owned(handle: HANDLE) -> Result<OwnedHandle, AuthError> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(AuthError::Process);
    }
    // SAFETY: all callers pass a fresh owned handle from a successful Win32
    // constructor. OwnedHandle closes it exactly once.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

impl Job {
    pub(super) fn attach_and_resume(child: &tokio::process::Child) -> Result<Self, AuthError> {
        let process = child.raw_handle().ok_or(AuthError::Process)?;
        let process_id = child.id().ok_or(AuthError::Process)?;
        // SAFETY: null security attributes select non-inheritable defaults; a
        // null name creates an unnamed job that cannot be reopened by name.
        let job = owned(unsafe { CreateJobObjectW(ptr::null(), ptr::null()) })?;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: the handle is valid, and the pointer and size describe the
        // exact initialized information structure required by this class.
        if unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                ptr::from_ref(&limits).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        } == 0
        {
            return Err(AuthError::Process);
        }
        // SAFETY: the process is still suspended and both handles stay owned
        // across this synchronous call. No breakaway limit is enabled.
        if unsafe { AssignProcessToJobObject(job.as_raw_handle(), process) } == 0 {
            return Err(AuthError::Process);
        }
        let result = Self(job);
        result.resume(process_id)?;
        Ok(result)
    }

    fn resume(&self, process_id: u32) -> Result<(), AuthError> {
        // SAFETY: this API returns a snapshot handle without pointer inputs.
        let snapshot = owned(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) })?;
        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        // SAFETY: entry has the documented size and remains valid throughout
        // the enumeration. The snapshot is owned until this method returns.
        let mut found = unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) };
        let mut count = 0;
        while found != 0 && count < 131_072 {
            count += 1;
            if entry.th32OwnerProcessID == process_id {
                // SAFETY: OpenThread provides only the right needed to resume
                // this child's thread, with handle inheritance disabled.
                let thread =
                    owned(unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) })?;
                // SAFETY: the thread belongs to our suspended child, which is
                // already assigned to the live job.
                if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
                    return Err(AuthError::Process);
                }
                return Ok(());
            }
            entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            // SAFETY: same initialized snapshot/structure as Thread32First.
            found = unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) };
        }
        Err(AuthError::Process)
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // Keep the owning handle observably live until drop; OwnedHandle's
        // destructor closes it and enforces JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE.
        let _ = self.0.as_raw_handle();
    }
}
