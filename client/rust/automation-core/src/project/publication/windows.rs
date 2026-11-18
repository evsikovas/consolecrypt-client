//! Small audited Win32 boundary. No ambient paths, handle ownership transfer,
//! overwrite/delete, network, token privileges or global configuration changes.
#![cfg(windows)]
#![deny(unsafe_op_in_unsafe_fn)]
use std::{
    io,
    mem::size_of,
    os::windows::io::{AsRawHandle, BorrowedHandle, OwnedHandle},
    ptr,
    sync::atomic::{AtomicBool, Ordering},
};
use windows_sys::Wdk::Storage::FileSystem::{
    FILE_RENAME_INFORMATION, FileIsRemoteDeviceInformation, FileRenameInformation,
    NtQueryInformationFile, NtSetInformationFile,
};
use windows_sys::Win32::{
    Foundation::{RtlNtStatusToDosError, WAIT_OBJECT_0},
    Storage::FileSystem::GetVolumeInformationByHandleW,
    System::{
        IO::IO_STATUS_BLOCK,
        Threading::{INFINITE, WaitForSingleObject},
    },
};

pub(super) const DELETE_ACCESS: u32 = 0x0001_0000;
pub(super) const GENERIC_READ_ACCESS: u32 = 0x8000_0000;
pub(super) const BACKUP_SEMANTICS: u32 = 0x0200_0000;
pub(super) const WRITE_THROUGH: u32 = 0x8000_0000;
pub(super) const SHARE_READ_WRITE: u32 = 3;
struct CompletionHealth(AtomicBool);
impl CompletionHealth {
    const fn new() -> Self {
        Self(AtomicBool::new(false))
    }
    fn run<T>(&self, work: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
        if self.0.load(Ordering::Acquire) {
            return Err(io::Error::other(
                "publication filesystem completion is unconfirmed; restart and explicit recovery required",
            ));
        }
        work()
    }
    fn indeterminate(&self) {
        self.0.store(true, Ordering::Release);
    }
}
static HEALTH: CompletionHealth = CompletionHealth::new();

/// The only constructor fixes synchronous non-OVERLAPPED creation flags and
/// DELETE|SYNCHRONIZE access. No caller-provided raw/async handle can mint it.
pub(super) struct SynchronousStage {
    file: cap_std::fs::File,
}
pub(super) fn open_stage(
    parent: &cap_std::fs::Dir,
    name: &str,
    expected: &super::DirectoryIdentity,
) -> io::Result<SynchronousStage> {
    HEALTH.run(|| open_stage_admitted(parent, name, expected))
}
fn open_stage_admitted(
    parent: &cap_std::fs::Dir,
    name: &str,
    expected: &super::DirectoryIdentity,
) -> io::Result<SynchronousStage> {
    use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
    use cap_std::fs::{OpenOptions, OpenOptionsExt};
    valid_name(name)?;
    let mut options = OpenOptions::new();
    options
        .access_mode(GENERIC_READ_ACCESS | DELETE_ACCESS | 0x0010_0000)
        .share_mode(SHARE_READ_WRITE)
        .custom_flags(BACKUP_SEMANTICS)
        .follow(FollowSymlinks::No);
    let file = parent.open_with(name, &options)?;
    let metadata = file.metadata()?;
    let copy = file.try_clone()?.into_std();
    let (volume, file_id) = crate::project_identity_windows::stable(&copy)?;
    let actual = super::DirectoryIdentity::Windows { volume, file_id };
    let parent_identity =
        super::identity(parent).map_err(|_| io::Error::other("unsupported parent identity"))?;
    let same_volume = matches!(parent_identity, super::DirectoryIdentity::Windows { volume: parent_volume, .. } if volume == parent_volume);
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || cap_std::fs::MetadataExt::file_attributes(&metadata) & 0x400 != 0
        || actual != *expected
        || !same_volume
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "stage identity/type/volume changed",
        ));
    }
    Ok(SynchronousStage { file })
}

fn reported_local(remote: u8, returned_bytes: usize) -> bool {
    returned_bytes == 1 && remote == 0
}
/// Query device locality FROM THE OPENED HANDLE before considering NTFS name.
/// API availability or a redirector-reported filesystem string is not locality.
pub(super) fn is_local_ntfs(handle: BorrowedHandle<'_>) -> io::Result<bool> {
    HEALTH.run(|| is_local_ntfs_admitted(handle))
}
fn is_local_ntfs_admitted(handle: BorrowedHandle<'_>) -> io::Result<bool> {
    let owned = handle.try_clone_to_owned()?;
    let mut iosb = Box::new(IO_STATUS_BLOCK::default());
    // Raw byte rather than Rust bool lets us reject an invalid BOOLEAN encoding.
    let mut remote = Box::new([0xff_u8; 1]);
    // SAFETY: fixed-size writable heap outputs and owned duplicate handle stay
    // alive through completion, including the defensive STATUS_PENDING wait.
    let status = unsafe {
        NtQueryInformationFile(
            owned.as_raw_handle(),
            &mut *iosb,
            remote.as_mut_ptr().cast(),
            1,
            FileIsRemoteDeviceInformation,
        )
    };
    let (status, iosb, remote) = completed(status, owned, iosb, remote)?;
    nt_success(status)?;
    if !reported_local(remote[0], iosb.Information) {
        return Ok(false);
    }
    is_ntfs_name(handle)
}
fn is_ntfs_name(handle: BorrowedHandle<'_>) -> io::Result<bool> {
    let mut name = [0u16; 32];
    // SAFETY: borrowed handle remains live; all optional outputs are null;
    // name is writable for the complete explicitly supplied WCHAR count.
    let ok = unsafe {
        GetVolumeInformationByHandleW(
            handle.as_raw_handle(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            name.as_mut_ptr(),
            name.len() as u32,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(name[..name.iter().position(|c| *c == 0).unwrap_or(name.len())] == [78, 84, 70, 83])
}

/// Rename this exact opened source into this exact opened parent. The caller
/// must open source with DELETE and deny delete-sharing, pin parent ancestry,
/// validate no reparse points and verify source identity/tree before and after.
/// The admission caller creates a synchronous non-OVERLAPPED file handle with
/// GENERIC_READ (including SYNCHRONIZE) + DELETE. No user/guest handle is accepted.
pub(super) fn rename_new(
    source: &SynchronousStage,
    parent: BorrowedHandle<'_>,
    name: &str,
) -> io::Result<()> {
    use std::os::windows::io::AsHandle;
    valid_name(name)?;
    HEALTH.run(|| rename_admitted(source.file.as_handle(), parent, name))
}

pub(in crate::project) fn open_project_move_file(
    parent: &cap_std::fs::Dir,
    name: &str,
) -> io::Result<cap_std::fs::File> {
    use cap_fs_ext::{FollowSymlinks, MetadataExt, OpenOptionsFollowExt};
    use cap_std::fs::{OpenOptions, OpenOptionsExt};
    let mut options = OpenOptions::new();
    options
        .access_mode(GENERIC_READ_ACCESS | DELETE_ACCESS)
        .share_mode(1)
        .follow(FollowSymlinks::No);
    let file = parent.open_with(name, &options)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || cap_std::fs::MetadataExt::file_attributes(&metadata) & 0x400 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "select a regular project file",
        ));
    }
    Ok(file)
}

pub(in crate::project) fn rename_project_move_file(
    file: &cap_std::fs::File,
    target: &cap_std::fs::Dir,
    name: &str,
) -> io::Result<()> {
    use std::os::windows::io::AsHandle;
    HEALTH.run(|| rename_admitted(file.as_handle(), target.as_handle(), name))
}

fn rename_admitted(
    source: BorrowedHandle<'_>,
    parent: BorrowedHandle<'_>,
    name: &str,
) -> io::Result<()> {
    let wide: Vec<u16> = name.encode_utf16().collect();
    let bytes = size_of::<FILE_RENAME_INFORMATION>() + wide.len() * 2;
    // usize allocation provides at least pointer alignment for FILE_RENAME_INFO.
    let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    let mut iosb = Box::new(IO_STATUS_BLOCK::default());
    let source = source.try_clone_to_owned()?;
    let parent = parent.try_clone_to_owned()?;
    // SAFETY: aligned, zeroed storage is large enough for the documented header
    // and flexible WCHAR tail. Every read field is initialized. No reference to
    // the nominal one-element tail is made. Buffers and owned source remain live
    // through synchronous completion or the explicit pending wait below.
    let status = unsafe {
        (*info).Anonymous.ReplaceIfExists = false;
        (*info).RootDirectory = parent.as_raw_handle();
        (*info).FileNameLength = (wide.len() * 2) as u32;
        ptr::copy_nonoverlapping(
            wide.as_ptr(),
            ptr::addr_of_mut!((*info).FileName).cast::<u16>(),
            wide.len(),
        );
        NtSetInformationFile(
            source.as_raw_handle(),
            &mut *iosb,
            info.cast(),
            bytes as u32,
            FileRenameInformation,
        )
    };
    let (status, _iosb, _keep) = completed(status, source, iosb, (storage, parent))?;
    nt_success(status)
}
fn nt_success(status: i32) -> io::Result<()> {
    if status != 0 {
        // SAFETY: this conversion is a pure NTSTATUS→Win32 error mapping.
        let error = unsafe { RtlNtStatusToDosError(status) };
        Err(io::Error::from_raw_os_error(error as i32))
    } else {
        Ok(())
    }
}
// Shared lifetime guard for both synchronous metadata query and rename. Keep
// all kernel-visible allocations and relevant handles through completion.
fn completed<T>(
    mut status: i32,
    handle: OwnedHandle,
    iosb: Box<IO_STATUS_BLOCK>,
    keep: T,
) -> io::Result<(i32, Box<IO_STATUS_BLOCK>, T)> {
    if status == 0x103 {
        // SAFETY: the duplicate stays owned and live; Windows file handles are
        // waitable, and the required source access includes SYNCHRONIZE.
        let waited = unsafe { WaitForSingleObject(handle.as_raw_handle(), INFINITE) };
        if waited != WAIT_OBJECT_0 {
            // Defensive safety for an OS failure: never free kernel-owned
            // pending buffers/handle. Process restart closes this retained
            // handle; publication remains blocked by its immutable intent.
            // There can be at most the already-admitted operations in flight.
            // Do not let released Core slots admit repeated uncertain I/O.
            HEALTH.indeterminate();
            std::mem::forget((keep, iosb, handle));
            return Err(io::Error::other(
                "pending filesystem operation could not be awaited; explicit recovery required",
            ));
        }
        // SAFETY: successful wait establishes completion before IOSB is read.
        status = unsafe { iosb.Anonymous.Status };
        if status == 0x103 {
            HEALTH.indeterminate();
            std::mem::forget((keep, iosb, handle));
            return Err(io::Error::other(
                "filesystem completion remained pending; explicit recovery required",
            ));
        }
    }
    Ok((status, iosb, keep))
}

pub(super) fn valid_name(name: &str) -> io::Result<()> {
    let stem = name.split('.').next().unwrap_or("").to_ascii_lowercase();
    let device = matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || (stem.len() == 4
            && (stem.starts_with("com") || stem.starts_with("lpt"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if name.is_empty()
        || device
        || name.len() > 240
        || !name.is_ascii()
        || name == "."
        || name == ".."
        || name.ends_with([' ', '.'])
        || name
            .bytes()
            .any(|b| b < 32 || b == 127 || b"/\\:<>\"|?*".contains(&b))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "single portable name required",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indeterminate_completion_blocks_new_handles_buffers_and_never_reissues_reserved_slots() {
        // Exact production admission/poison seam with local state so a fault
        // injection cannot poison parallel filesystem tests. No invalid NT
        // handle or real pending-I/O failure is fabricated/claimed.
        let health = CompletionHealth::new();
        let slots = std::sync::Arc::new(tokio::sync::Semaphore::new(2));
        let ready = slots.clone().try_acquire_owned().unwrap();
        let sql_handoff = slots.clone().try_acquire_owned().unwrap();
        let mut opens = 0;
        let mut buffers = 0;
        let mut retained = None;
        let result: io::Result<()> = health.run(|| {
            let file = std::fs::File::open("NUL")?;
            opens += 1;
            let buffer = vec![0u8; 128];
            buffers += 1;
            retained = Some((file, buffer));
            // This is the SAME transition completed() calls before retaining
            // kernel-visible allocations on unconfirmed wait/completion.
            health.indeterminate();
            Err(io::Error::other("synthetic indeterminate completion"))
        });
        assert!(result.is_err());
        assert_eq!(slots.available_permits(), 0);
        for _ in 0..3 {
            let denied: io::Result<()> = health.run(|| {
                opens += 1;
                buffers += 1;
                panic!("no new handle/buffer work after indeterminate completion");
            });
            assert!(denied.is_err());
        }
        assert_eq!((opens, buffers), (1, 1));
        assert!(retained.is_some());
        assert_eq!(slots.available_permits(), 0);
        drop(ready);
        assert_eq!(slots.available_permits(), 1);
        assert!(
            health.run(|| Ok(())).is_err(),
            "slot release is not a fresh I/O grant or poison reset"
        );
        drop((sql_handoff, retained));
        assert_eq!(slots.available_permits(), 2);
        assert!(health.run(|| Ok(())).is_err());
    }
}

// Reuse the SAME process-wide completion poison before ACL/create allocations.
// No new registry, permit, reset or unsupported-I/O fallback.
pub(super) fn acl_io<T>(work: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    HEALTH.run(work)
}
pub(super) fn acl_indeterminate() {
    HEALTH.indeterminate();
}
