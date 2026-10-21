//! Display/storage form of local paths.
//!
//! `std::fs::canonicalize` on Windows returns verbatim paths (`\\?\C:\…`,
//! `\\?\UNC\server\share\…`). They are valid for the OS but confusing in the
//! UI and break naive comparisons with ordinary paths. Canonicalize with
//! `dunce` where a path is shown or stored; this string helper migrates values
//! persisted by earlier versions. It is deliberately independent of `cfg`, so
//! the Windows forms are tested on every OS.

/// `\\?\C:\x` → `C:\x`, `\\?\UNC\server\share\x` → `\\server\share\x`.
/// Anything else (including device paths such as `\\?\Volume{…}\` or
/// `\\.\pipe\…`, which have no ordinary spelling) is returned unchanged.
pub fn strip_verbatim_prefix(path: &str) -> String {
    let Some(rest) = path.strip_prefix(r"\\?\") else {
        return path.to_owned();
    };
    let bytes = rest.as_bytes();
    if bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes.len() == 2 || bytes[2] == b'\\')
    {
        return rest.to_owned();
    }
    if rest.len() > 4 && rest[..4].eq_ignore_ascii_case(r"UNC\") {
        let unc = &rest[4..];
        // \\?\UNC\server\share… only; never produce `\\` + empty server.
        if !unc.starts_with('\\') && unc.contains('\\') {
            return format!(r"\\{unc}");
        }
    }
    path.to_owned()
}

#[cfg(test)]
mod tests {
    use super::strip_verbatim_prefix;

    #[test]
    fn strips_drive_and_unc_verbatim_prefixes() {
        assert_eq!(
            strip_verbatim_prefix(r"\\?\C:\ProgramData\chocolatey\bin\flux.exe"),
            r"C:\ProgramData\chocolatey\bin\flux.exe"
        );
        assert_eq!(strip_verbatim_prefix(r"\\?\d:\"), r"d:\");
        assert_eq!(strip_verbatim_prefix(r"\\?\C:"), r"C:");
        assert_eq!(
            strip_verbatim_prefix(r"\\?\UNC\server\share\tools\flux.exe"),
            r"\\server\share\tools\flux.exe"
        );
        assert_eq!(
            strip_verbatim_prefix(r"\\?\unc\server\share"),
            r"\\server\share"
        );
    }

    #[test]
    fn leaves_ordinary_and_device_paths_unchanged() {
        for path in [
            r"C:\Users\me\flux.exe",
            r"\\server\share\flux.exe",
            "/usr/local/bin/flux",
            "",
            r"\\?\Volume{6f0e6d1a-0000-0000-0000-100000000000}\flux.exe",
            r"\\.\pipe\docker_engine",
            r"\\?\UNC\",
            r"\\?\UNC\server",
            r"\\?\C:relative",
            r"\\?\1:\x",
        ] {
            assert_eq!(strip_verbatim_prefix(path), path);
        }
    }
}
