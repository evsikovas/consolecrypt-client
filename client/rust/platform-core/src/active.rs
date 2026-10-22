//! "Active content": files whose OS default action runs code instead of
//! showing a document (security audit H4).
//!
//! A remote file opened with "Open" lands in a local working copy and is
//! handed to the OS. For a `.js`, `.hta`, `.lnk`, `.command`, `.desktop`,
//! an executable script… the default action (ShellExecute "open",
//! LaunchServices, `xdg-open`) *runs* it, so [`crate::FileOpener::open_path`]
//! opens such files as text instead. This module is the single place that
//! decides what counts as active.
//!
//! One extension list serves every OS on purpose: a Windows `.exe` / `.msi`
//! / `.lnk` reaches Wine or CrossOver on Linux and macOS, `.jar` reaches a
//! Java launcher anywhere, `.py` the Python launcher, and the only cost of a
//! false positive is a file opened in a text editor.

use std::path::Path;

/// Extensions (lower case, without the dot) whose default action runs code,
/// installs software, changes system settings or follows a link to
/// something that may. Grouped by origin, matched on every OS.
const ACTIVE_EXTENSIONS: &[&str] = &[
    // Windows executables and installers.
    "exe",
    "com",
    "scr",
    "pif",
    "cpl",
    "msi",
    "msp",
    "mst",
    "msc",
    "msix",
    "msixbundle",
    "appx",
    "appxbundle",
    "application",
    "appref-ms",
    "xbap",
    "vsto",
    "gadget",
    "dll",
    "ocx",
    "sys",
    "drv",
    "cab",
    "diagcab",
    "xll",
    "reg",
    "inf",
    "chm",
    "hlp",
    "hta",
    "htt",
    // Windows script hosts and shells.
    "bat",
    "cmd",
    "btm",
    "js",
    "jse",
    "vb",
    "vbs",
    "vbe",
    "wsf",
    "wsh",
    "wsc",
    "ws",
    "sct",
    "ps1",
    "ps1xml",
    "ps2",
    "ps2xml",
    "psc1",
    "psc2",
    "psd1",
    "psm1",
    "pssc",
    "cdxml",
    "msh",
    "msh1",
    "msh2",
    "mshxml",
    "msh1xml",
    "msh2xml",
    "mof",
    // Windows shortcuts and shell links.
    "lnk",
    "url",
    "website",
    "scf",
    "shb",
    "shs",
    "settingcontent-ms",
    "library-ms",
    "searchconnector-ms",
    "search-ms",
    "glk",
    "theme",
    "themepack",
    "rdp",
    // macOS applications, scripts, installers and links.
    "app",
    "command",
    "tool",
    "terminal",
    "workflow",
    "action",
    "definition",
    "scpt",
    "scptd",
    "applescript",
    "osax",
    "prefpane",
    "pkg",
    "mpkg",
    "kext",
    "plugin",
    "bundle",
    "saver",
    "qlgenerator",
    "mdimporter",
    "dylib",
    "mobileconfig",
    "shortcut",
    "webloc",
    "inetloc",
    "fileloc",
    "afploc",
    "ftploc",
    // Linux / freedesktop launchers, packages and images.
    "desktop",
    "directory",
    "run",
    "bin",
    "out",
    "elf",
    "so",
    "ko",
    "appimage",
    "deb",
    "rpm",
    "snap",
    "flatpak",
    "flatpakref",
    "flatpakrepo",
    "kwinscript",
    "plasmoid",
    // Unix shells.
    "sh",
    "bash",
    "zsh",
    "ksh",
    "csh",
    "tcsh",
    "fish",
    "dash",
    "ash",
    // Interpreters with OS launchers (Python launcher, Java, …).
    "py",
    "pyw",
    "pyc",
    "pyo",
    "pyz",
    "pyzw",
    "jar",
    "class",
    "jnlp",
    "pl",
    "pm",
    "rb",
    "rbw",
    "php",
    "tcl",
    "lua",
    "wasm",
];

/// Extensions of plain documents: an executable bit or a program-like first
/// line does not make these run, because the OS picks a viewer / editor by
/// the extension.
const DOCUMENT_EXTENSIONS: &[&str] = &[
    "txt",
    "text",
    "md",
    "markdown",
    "rst",
    "adoc",
    "org",
    "log",
    "csv",
    "tsv",
    "json",
    "jsonc",
    "json5",
    "yaml",
    "yml",
    "toml",
    "ini",
    "cfg",
    "conf",
    "properties",
    "xml",
    "html",
    "htm",
    "css",
    "scss",
    "less",
    "svg",
    "pdf",
    "png",
    "jpg",
    "jpeg",
    "gif",
    "bmp",
    "webp",
    "tif",
    "tiff",
    "heic",
    "ico",
    "mp3",
    "mp4",
    "m4a",
    "mov",
    "wav",
    "avi",
    "mkv",
    "webm",
    "ogg",
    "flac",
    "zip",
    "gz",
    "tgz",
    "bz2",
    "xz",
    "zst",
    "7z",
    "tar",
    "diff",
    "patch",
    "sql",
    "c",
    "h",
    "cc",
    "cpp",
    "hpp",
    "rs",
    "go",
    "java",
    "kt",
    "swift",
    "ts",
    "dart",
    "lock",
    "pem",
    "crt",
    "pub",
];

/// Why a file is treated as active content.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ActiveContent {
    /// Its (effective, last) extension runs code; lower case.
    Extension(String),
    /// The name hides what the OS will see: an NTFS stream separator (`:`)
    /// or a CLSID extension (`name.{…}`).
    DeceptiveName,
    /// Unix execute permission and no plain-document extension.
    ExecutableBit,
    /// The file starts like a program (`#!`, ELF, PE, Mach-O, Java class)
    /// and has no plain-document extension.
    ExecutableContent,
}

/// The name as Windows resolves it: trailing dots and spaces are dropped
/// (`evil.js. .` opens as `evil.js`).
fn effective_name(name: &str) -> &str {
    name.trim_end_matches(['.', ' '])
}

/// Lower-case last extension of `name` after [`effective_name`]; dot-files
/// (`.bashrc`) have none.
fn extension(name: &str) -> Option<String> {
    let name = effective_name(name);
    let i = name.rfind('.')?;
    if name[..i].trim_start_matches('.').is_empty() {
        return None;
    }
    Some(name[i + 1..].to_ascii_lowercase())
}

/// Classifies a file by its name alone (a remote file before download).
/// `name` is the last path component; separators are not interpreted.
pub fn classify_name(name: &str) -> Option<ActiveContent> {
    let name = effective_name(name);
    // NTFS alternate data streams (`x.txt:evil.js`, `x.js::$DATA`).
    if name.contains(':') {
        return Some(ActiveContent::DeceptiveName);
    }
    let ext = extension(name)?;
    // `name.{CLSID}`: Explorer dispatches on the class id, not the name.
    if ext.starts_with('{') {
        return Some(ActiveContent::DeceptiveName);
    }
    ACTIVE_EXTENSIONS
        .contains(&ext.as_str())
        .then_some(ActiveContent::Extension(ext))
}

/// Whether `name` is active content by name alone ([`classify_name`]).
pub fn is_active_name(name: &str) -> bool {
    classify_name(name).is_some()
}

fn is_document(name: &str) -> bool {
    extension(name).is_some_and(|e| DOCUMENT_EXTENSIONS.contains(&e.as_str()))
}

/// Magic numbers of programs: script shebang, ELF, PE (`MZ`), Mach-O (both
/// byte orders, 32/64 bit), fat Mach-O / Java class (`CAFEBABE`).
fn looks_executable(head: &[u8]) -> bool {
    const MAGIC: &[&[u8]] = &[
        b"#!",
        b"\x7fELF",
        b"MZ",
        b"\xfe\xed\xfa\xce",
        b"\xfe\xed\xfa\xcf",
        b"\xce\xfa\xed\xfe",
        b"\xcf\xfa\xed\xfe",
        b"\xca\xfe\xba\xbe",
    ];
    MAGIC.iter().any(|m| head.starts_with(m))
}

fn read_head(path: &Path) -> Option<[u8; 4]> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = [0u8; 4];
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
    Some(buf)
}

/// Classifies a local file: its name ([`classify_name`]), and unless the
/// extension is a plain document's, the Unix execute bit and the first
/// bytes. A symlink is judged by its own name and by its target. A
/// directory is active only by name (macOS bundles: `x.app`, `x.workflow`).
pub fn classify_path(path: &Path) -> Option<ActiveContent> {
    let mut names = vec![path.to_path_buf()];
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        // Unresolvable link: judge by name only (the OS cannot open it either).
        if let Ok(target) = std::fs::canonicalize(path) {
            names.push(target);
        }
    }
    let file_name = |p: &Path| {
        p.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    for p in &names {
        if let Some(c) = classify_name(&file_name(p)) {
            return Some(c);
        }
    }
    let target = names.last().map(|p| p.as_path()).unwrap_or(path);
    let meta = std::fs::metadata(target).ok()?;
    // Exempt only when every name the OS may dispatch on is a document's.
    if !meta.is_file() || names.iter().all(|p| is_document(&file_name(p))) {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 != 0 {
            return Some(ActiveContent::ExecutableBit);
        }
    }
    read_head(target)
        .filter(|h| looks_executable(h))
        .map(|_| ActiveContent::ExecutableContent)
}

/// Whether `path` is active content ([`classify_path`]).
pub fn is_active_path(path: &Path) -> bool {
    classify_path(path).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ext(name: &str) -> Option<ActiveContent> {
        classify_name(name)
    }

    #[test]
    fn lists_are_lower_case_unique_and_disjoint() {
        for list in [ACTIVE_EXTENSIONS, DOCUMENT_EXTENSIONS] {
            let mut seen = std::collections::HashSet::new();
            for e in list {
                assert_eq!(*e, e.to_ascii_lowercase(), "{e}");
                assert!(!e.starts_with('.'), "{e}");
                assert!(seen.insert(*e), "duplicate {e}");
            }
        }
        for e in DOCUMENT_EXTENSIONS {
            assert!(!ACTIVE_EXTENSIONS.contains(e), "{e} in both lists");
        }
    }

    #[test]
    fn active_names_per_os_family() {
        for name in [
            // Windows
            "setup.exe",
            "report.js",
            "a.vbs",
            "a.hta",
            "a.lnk",
            "a.bat",
            "a.cmd",
            "a.ps1",
            "a.msi",
            "a.scr",
            "a.url",
            "a.reg",
            "a.wsf",
            "a.settingcontent-ms",
            // macOS
            "Run Me.command",
            "Thing.app",
            "a.terminal",
            "a.workflow",
            "a.scpt",
            "a.pkg",
            "a.webloc",
            // Linux
            "launcher.desktop",
            "install.sh",
            "tool.AppImage",
            "pkg.deb",
            // Cross-platform launchers
            "app.jar",
            "tool.py",
        ] {
            assert!(is_active_name(name), "{name}");
        }
    }

    #[test]
    fn tricky_names() {
        // Case-insensitive.
        assert_eq!(
            ext("INVOICE.PDF.JS"),
            Some(ActiveContent::Extension("js".into()))
        );
        assert!(is_active_name("Setup.ExE"));
        // Double extensions: the last one decides.
        assert!(is_active_name("x.txt.js"));
        assert!(is_active_name("photo.jpg.exe"));
        assert!(!is_active_name("script.js.txt"));
        assert!(!is_active_name("archive.tar.gz"));
        // Windows drops trailing dots and spaces.
        assert!(is_active_name("evil.js."));
        assert!(is_active_name("evil.js . ."));
        assert!(is_active_name("evil.exe "));
        // NTFS alternate data streams.
        assert_eq!(ext("notes.txt:evil.js"), Some(ActiveContent::DeceptiveName));
        assert_eq!(ext("run.js::$DATA"), Some(ActiveContent::DeceptiveName));
        // CLSID extensions.
        assert_eq!(
            ext("readme.{3050f4d8-98B5-11CF-BB82-00AA00BDCE0B}"),
            Some(ActiveContent::DeceptiveName)
        );
        // Right-to-left override cannot hide the real (last) extension.
        assert!(is_active_name("invoice\u{202e}fdp.exe"));
        // Not active.
        for name in [
            "notes.txt",
            "README",
            ".bashrc",
            ".sh",
            "Makefile",
            "docker-compose.yml",
            "config.json",
            "page.html",
            "a.",
            "...",
            "",
            "js",
            "x.jsx",
            "x.exe2",
        ] {
            assert!(!is_active_name(name), "{name:?}");
        }
    }

    #[test]
    fn local_files_by_content_and_mode() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, data: &[u8]| {
            let p = dir.path().join(name);
            std::fs::write(&p, data).unwrap();
            p
        };
        let shebang = write("deploy", b"#!/bin/sh\necho hi\n");
        assert_eq!(
            classify_path(&shebang),
            Some(ActiveContent::ExecutableContent)
        );
        let elf = write("tool", b"\x7fELF\x02\x01");
        assert!(is_active_path(&elf));
        let pe = write("thing.dat", b"MZ\x90\x00");
        assert!(is_active_path(&pe));
        // A document extension wins over the content.
        let doc = write("notes.txt", b"#!/bin/sh\n");
        assert!(!is_active_path(&doc));
        let plain = write("plain", b"hello");
        assert!(!is_active_path(&plain));
        let empty = write("empty", b"");
        assert!(!is_active_path(&empty));
        let by_name = write("a.vbs", b"hello");
        assert_eq!(
            classify_path(&by_name),
            Some(ActiveContent::Extension("vbs".into()))
        );
        // Missing files: by name only.
        assert!(is_active_path(&dir.path().join("missing.exe")));
        assert!(!is_active_path(&dir.path().join("missing")));
        // Directories: bundles by name only.
        let bundle = dir.path().join("Thing.app");
        std::fs::create_dir(&bundle).unwrap();
        assert!(is_active_path(&bundle));
        let folder = dir.path().join("folder");
        std::fs::create_dir(&folder).unwrap();
        assert!(!is_active_path(&folder));
    }

    #[cfg(unix)]
    #[test]
    fn unix_exec_bit_and_symlinks() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let set_exec =
            |p: &Path| std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
        let bin = dir.path().join("runme");
        std::fs::write(&bin, b"echo").unwrap();
        set_exec(&bin);
        assert_eq!(classify_path(&bin), Some(ActiveContent::ExecutableBit));
        let unknown = dir.path().join("data.xyz");
        std::fs::write(&unknown, b"echo").unwrap();
        set_exec(&unknown);
        assert!(is_active_path(&unknown));
        let doc = dir.path().join("notes.md");
        std::fs::write(&doc, b"# hi").unwrap();
        set_exec(&doc);
        assert!(!is_active_path(&doc), "document extension wins");
        // A harmless-looking link to an active target.
        let target = dir.path().join("evil.command");
        std::fs::write(&target, b"echo").unwrap();
        let link = dir.path().join("notes.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(
            classify_path(&link),
            Some(ActiveContent::Extension("command".into()))
        );
        // A document-named link to an extensionless executable.
        let link2 = dir.path().join("readme.txt");
        std::os::unix::fs::symlink(&bin, &link2).unwrap();
        assert_eq!(classify_path(&link2), Some(ActiveContent::ExecutableBit));
    }
}
