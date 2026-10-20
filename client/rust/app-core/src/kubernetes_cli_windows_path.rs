//! An explicit user action modifies only HKCU\Environment\Path. No subprocess
//! or shell interprets the existing PATH or installation directory.
#![allow(unsafe_code)]
use super::*;
use std::{ffi::OsStr, os::windows::ffi::OsStrExt, ptr};
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
    System::Registry::{
        RegCloseKey, RegGetValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_QUERY_VALUE, KEY_SET_VALUE, REG_EXPAND_SZ, REG_SZ, RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ,
        RRF_RT_REG_SZ,
    },
    UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    },
};
fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: Key owns the successful RegOpenKeyExW handle, closes it once,
        // and never exposes it beyond this module.
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
fn failed() -> AppError {
    AppError::invalid("kubernetes", "cli_path_failed")
}
pub(super) fn add_user_path() -> AppResult<()> {
    let directory = cli::destination()
        .map_err(super::super::kubernetes_flux::error)?
        .parent()
        .ok_or_else(failed)?
        .to_string_lossy()
        .into_owned();
    if directory.contains([';', '\0', '\r', '\n']) {
        return Err(failed());
    }
    let subkey = wide("Environment");
    let value = wide("Path");
    let mut key = ptr::null_mut();
    // SAFETY: all pointers reference live, terminated UTF-16 buffers and a
    // writable handle out-parameter. Access is restricted to the current user.
    if unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            &mut key,
        )
    } != ERROR_SUCCESS
    {
        return Err(failed());
    }
    let key = Key(key);
    let mut kind = REG_EXPAND_SZ;
    let mut bytes = 0u32;
    let flags = RRF_NOEXPAND | RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
    // SAFETY: the key is live, the name is terminated UTF-16, and the size/type
    // out-pointers are writable; a null data pointer requests only the size.
    let status = unsafe {
        RegGetValueW(
            key.0,
            ptr::null(),
            value.as_ptr(),
            flags,
            &mut kind,
            ptr::null_mut(),
            &mut bytes,
        )
    };
    let current = if status == ERROR_FILE_NOT_FOUND {
        kind = REG_EXPAND_SZ;
        String::new()
    } else {
        if status != ERROR_SUCCESS || bytes > 65534 || bytes % 2 != 0 {
            return Err(failed());
        }
        let mut buffer = vec![0u16; (bytes / 2) as usize + 1];
        // SAFETY: the allocated buffer is at least `bytes` bytes long and
        // remains live during the call. Growth returns ERROR_MORE_DATA, which
        // is rejected before reading from the buffer.
        let status = unsafe {
            RegGetValueW(
                key.0,
                ptr::null(),
                value.as_ptr(),
                flags,
                &mut kind,
                buffer.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        if status != ERROR_SUCCESS || ![REG_SZ, REG_EXPAND_SZ].contains(&kind) {
            return Err(failed());
        }
        let count = (bytes / 2) as usize;
        if count == 0 || buffer[count - 1] != 0 || buffer[..count - 1].contains(&0) {
            return Err(failed());
        }
        String::from_utf16(&buffer[..count - 1]).map_err(|_| failed())?
    };
    if current
        .split(';')
        .any(|entry| entry.trim_matches('"').eq_ignore_ascii_case(&directory))
    {
        return Ok(());
    }
    let next = wide(&if current.is_empty() {
        directory
    } else {
        format!("{};{directory}", current.trim_end_matches(';'))
    });
    if next.len() > 32767 {
        return Err(failed());
    }
    // SAFETY: key and terminated UTF-16 name/value buffers are live, the byte
    // count exactly matches next, and kind is restricted to the string types.
    let status = unsafe {
        RegSetValueExW(
            key.0,
            value.as_ptr(),
            0,
            kind,
            next.as_ptr().cast(),
            (next.len() * 2) as u32,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(failed());
    }
    // The running process environment is deliberately unchanged. New terminal
    // windows receive the registry update after the bounded OS notification.
    let mut result = 0usize;
    // SAFETY: this synchronous bounded call receives a live terminated
    // "Environment" buffer and writable result pointer, both retained until
    // it returns. WM_SETTINGCHANGE marshals this system message cross-process.
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            subkey.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            1000,
            &mut result,
        );
    }
    Ok(())
}
