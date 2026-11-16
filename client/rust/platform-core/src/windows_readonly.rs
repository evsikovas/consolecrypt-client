//! Fixed OS observations behind the existing platform-core native boundary.
//! No elevation, impersonation, program selection, token export or mutation.
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsTokenElevation {
    Elevated,
    NotElevated,
    Unknown,
}

/// Created only by the fixed OS queries; callers cannot supply a system path.
#[derive(Debug)]
pub struct WindowsPrerequisiteContext {
    system_directory: PathBuf,
    token_elevation: WindowsTokenElevation,
}
impl WindowsPrerequisiteContext {
    pub fn system_directory(&self) -> &Path {
        &self.system_directory
    }
    pub fn token_elevation(&self) -> WindowsTokenElevation {
        self.token_elevation
    }
}

#[allow(unsafe_code)]
fn token_elevation() -> WindowsTokenElevation {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: GetCurrentProcess returns the current process pseudo-handle;
    // token points to one writable HANDLE. TOKEN_QUERY requests no mutation.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return WindowsTokenElevation::Unknown;
    }
    let mut value = TOKEN_ELEVATION::default();
    let mut returned = 0;
    // SAFETY: token is the live handle just opened for query. The buffer is
    // exactly one writable TOKEN_ELEVATION and returned is a writable DWORD.
    let success = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&mut value as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    };
    // SAFETY: token is owned here, closed once on every successful-open path,
    // and no token handle or pointer escapes this function.
    unsafe { CloseHandle(token) };
    if success == 0 || returned as usize != std::mem::size_of::<TOKEN_ELEVATION>() {
        WindowsTokenElevation::Unknown
    } else if value.TokenIsElevated != 0 {
        WindowsTokenElevation::Elevated
    } else {
        WindowsTokenElevation::NotElevated
    }
}

/// Safe fixed read-only Windows context. Failure to query TokenElevation is
/// Unknown, never an assumed non-elevated/elevated state. No env/path fallback.
#[allow(unsafe_code)]
pub fn windows_prerequisite_context() -> io::Result<WindowsPrerequisiteContext> {
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
    let mut buffer = vec![0u16; 32768];
    // SAFETY: buffer is live writable UTF-16 storage for the supplied bounded
    // character count. It is interpreted only after a complete valid result.
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    if length >= buffer.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "system directory length invalid",
        ));
    }
    let directory = PathBuf::from(String::from_utf16(&buffer[..length]).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "system directory encoding invalid",
        )
    })?);
    if !directory.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "system directory must be absolute",
        ));
    }
    Ok(WindowsPrerequisiteContext {
        system_directory: directory,
        token_elevation: token_elevation(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_windows_context_is_absolute_existing_and_reports_only_real_token_observation() {
        let context = windows_prerequisite_context().unwrap();
        assert!(context.system_directory().is_absolute());
        assert!(context.system_directory().is_dir());
        assert!(matches!(
            context.token_elevation(),
            WindowsTokenElevation::Elevated
                | WindowsTokenElevation::NotElevated
                | WindowsTokenElevation::Unknown
        ));
    }
}
