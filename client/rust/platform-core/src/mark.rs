//! Marks files downloaded from a remote host as coming from the Internet
//! (security audit H4), so the OS applies its own checks when someone later
//! runs them: Windows Mark-of-the-Web (SmartScreen, Office Protected View,
//! "unblock" prompts), macOS quarantine (Gatekeeper). Linux has no
//! equivalent; marking is a no-op there.
//!
//! The mark lives outside the file's data — an NTFS alternate data stream /
//! an extended attribute — so content, size and the edit-session watcher's
//! view of the file are unchanged by it.

use std::io;
use std::path::Path;

/// `com.apple.quarantine` value: flags `0081` (downloaded, not yet
/// approved), the time in hex seconds, the agent, no event UUID.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn quarantine_value(unix_secs: u64) -> String {
    format!("0081;{unix_secs:08x};ConsoleCrypt;")
}

/// Contents of the `Zone.Identifier` stream: the Internet zone. No
/// `HostUrl` / `ReferrerUrl` — the server and path stay private.
#[cfg_attr(not(windows), allow(dead_code))]
const ZONE_IDENTIFIER: &str = "[ZoneTransfer]\r\nZoneId=3\r\n";

/// Mark `path` (a regular file) as downloaded from the Internet. Fails on
/// file systems without streams / extended attributes; callers treat the
/// mark as best effort.
pub fn mark_downloaded(path: &Path) -> io::Result<()> {
    // Never create the file (a Windows stream write would).
    if !std::fs::symlink_metadata(path)?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    #[cfg(target_os = "macos")]
    {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        macos::set_xattr(path, macos::QUARANTINE, quarantine_value(secs).as_bytes())
    }
    #[cfg(windows)]
    {
        std::fs::write(zone_stream(path), ZONE_IDENTIFIER)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = path;
        Ok(())
    }
}

/// Whether `path` carries the mark of [`mark_downloaded`] (`true` on
/// platforms without one is never returned: they report `false`).
pub fn is_marked_downloaded(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::get_xattr(path, macos::QUARANTINE).is_some()
    }
    #[cfg(windows)]
    {
        std::fs::read_to_string(zone_stream(path))
            .is_ok_and(|s| s.lines().any(|l| l.trim() == "ZoneId=3"))
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = path;
        false
    }
}

#[cfg(windows)]
fn zone_stream(path: &Path) -> std::path::PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(":Zone.Identifier");
    s.into()
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod macos {
    use std::ffi::CString;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    pub(super) const QUARANTINE: &str = "com.apple.quarantine";

    fn c_path(path: &Path) -> io::Result<CString> {
        CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))
    }

    pub(super) fn set_xattr(path: &Path, name: &str, value: &[u8]) -> io::Result<()> {
        let p = c_path(path)?;
        let n = CString::new(name).map_err(|_| io::ErrorKind::InvalidInput)?;
        // SAFETY: `p` and `n` are NUL-terminated and outlive the call;
        // `value` is a valid buffer of `value.len()` bytes.
        let rc = unsafe {
            libc::setxattr(
                p.as_ptr(),
                n.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                libc::XATTR_NOFOLLOW,
            )
        };
        if rc == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub(super) fn get_xattr(path: &Path, name: &str) -> Option<Vec<u8>> {
        let p = c_path(path).ok()?;
        let n = CString::new(name).ok()?;
        let mut buf = vec![0u8; 256];
        // SAFETY: as above; `buf` is writable for `buf.len()` bytes.
        let rc = unsafe {
            libc::getxattr(
                p.as_ptr(),
                n.as_ptr(),
                buf.as_mut_ptr().cast(),
                buf.len(),
                0,
                libc::XATTR_NOFOLLOW,
            )
        };
        let len = usize::try_from(rc).ok()?;
        buf.truncate(len);
        Some(buf)
    }

    #[cfg(test)]
    pub(super) fn get(path: &Path) -> Option<String> {
        get_xattr(path, QUARANTINE).map(|v| String::from_utf8_lossy(&v).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarantine_value_format() {
        assert_eq!(quarantine_value(0x6543_21ab), "0081;654321ab;ConsoleCrypt;");
        assert_eq!(quarantine_value(15), "0081;0000000f;ConsoleCrypt;");
        assert!(ZONE_IDENTIFIER.contains("ZoneId=3"));
        assert!(!ZONE_IDENTIFIER.contains("Url"));
    }

    #[test]
    fn mark_keeps_content_and_size() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("dl.js");
        std::fs::write(&p, b"alert(1)").unwrap();
        assert!(!is_marked_downloaded(&p));
        mark_downloaded(&p).unwrap();
        // Idempotent.
        mark_downloaded(&p).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"alert(1)");
        assert_eq!(std::fs::metadata(&p).unwrap().len(), 8);
        let marked = cfg!(any(target_os = "macos", windows));
        assert_eq!(is_marked_downloaded(&p), marked);
        #[cfg(target_os = "macos")]
        {
            let v = macos::get(&p).unwrap();
            assert!(
                v.starts_with("0081;") && v.ends_with(";ConsoleCrypt;"),
                "{v}"
            );
        }
        // Writes to the file keep working.
        std::fs::write(&p, b"changed").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"changed");
    }

    #[test]
    fn only_existing_regular_files_are_marked() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        assert!(mark_downloaded(&missing).is_err());
        assert!(!missing.exists(), "never created");
        assert!(
            mark_downloaded(dir.path()).is_err(),
            "directories are not marked"
        );
    }
}
