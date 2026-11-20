//! Read caller-available space for an existing application-owned directory.
use std::{io, path::Path};

#[cfg(unix)]
#[allow(unsafe_code)]
pub fn available_disk_space(path: &Path) -> io::Result<u64> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))?;
    let mut info = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: path is NUL-terminated and live, and info points to writable
    // storage for exactly one statvfs. It is read only after successful return.
    if unsafe { libc::statvfs(path.as_ptr(), info.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let info = unsafe { info.assume_init() };
    let bytes = u128::from(info.f_bavail) * u128::from(info.f_frsize);
    Ok(u64::try_from(bytes).unwrap_or(u64::MAX))
}

#[cfg(windows)]
#[allow(unsafe_code)]
pub fn available_disk_space(path: &Path) -> io::Result<u64> {
    use std::os::windows::ffi::OsStrExt;
    let mut path = path.as_os_str().encode_wide().collect::<Vec<_>>();
    if path.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains NUL",
        ));
    }
    path.push(0);
    let mut available = 0u64;
    // SAFETY: path is a live NUL-terminated UTF-16 string. The one output is
    // writable u64 storage; optional outputs are explicitly null.
    if unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            path.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(available)
}

#[cfg(not(any(unix, windows)))]
pub fn available_disk_space(_path: &Path) -> io::Result<u64> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "disk capacity is unavailable",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_owned_directory_reports_capacity_and_missing_path_is_an_error() {
        let folder = tempfile::tempdir().unwrap();
        assert!(available_disk_space(folder.path()).is_ok());
        assert!(available_disk_space(&folder.path().join("not-created")).is_err());
    }
    #[test]
    fn nul_paths_never_reach_the_os_api() {
        assert!(available_disk_space(Path::new("invalid\0path")).is_err());
    }
}
