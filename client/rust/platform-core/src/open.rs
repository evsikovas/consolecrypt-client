//! Open local files with the OS default application, a chosen application,
//! or reveal them in Finder / Explorer / the file manager
//! (docs/design/SFTP_BROWSER_SPEC.md §2 "Open" / "Open With…").
//!
//! * [`FileOpener::open_path`] — default app (macOS LaunchServices via
//!   `/usr/bin/open`, Windows `ShellExecuteW("open")` via the `opener` crate,
//!   Linux/BSD `xdg-open` best-effort via `opener`).
//! * [`FileOpener::open_with`] — a specific application ([`AppRef`]).
//! * [`FileOpener::choose_app_and_open`] — the platform's application chooser
//!   (macOS AppleScript `choose application`, Windows "Open with" dialog via
//!   `rundll32 shell32.dll,OpenAs_RunDLL`; unsupported elsewhere).
//! * [`FileOpener::reveal`] — select the file in Finder / Explorer (Linux:
//!   opens the containing folder).
//!
//! **Active content** (security audit H4): a file whose OS default action
//! would run code ([`crate::active`] — `.js`, `.hta`, `.lnk`, `.command`,
//! `.desktop`, executable scripts…) is never handed to the default handler
//! by [`FileOpener::open_path`]. It opens as text instead: macOS TextEdit
//! (`open -e`), Windows `%SystemRoot%\System32\notepad.exe`, Linux the
//! desktop's `text/plain` application (its `.desktop` `Exec`, run as an
//! argument vector), or [`OpenError::NoTextEditor`]. The Windows "Open
//! with" chooser, which preselects the default (running) handler, is not
//! offered for such files either. Opening with an explicitly chosen
//! application ([`FileOpener::open_with`]) stays the user's decision.
//!
//! Every launch is an argument vector — never a shell command line — and paths
//! are made absolute first, so a file name can never be parsed as an option.
//! What to launch is computed as a [`LaunchSpec`] (pure, per [`OsFamily`], so
//! every platform's behaviour is unit-tested on any host) and executed by a
//! [`Launcher`]; tests inject a recording launcher and never start apps.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Platform family that decides how files are opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsFamily {
    MacOs,
    Windows,
    /// Linux and other Unix desktops (freedesktop / `xdg-open`).
    Linux,
}

impl OsFamily {
    /// The platform this binary was built for.
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// An application to open a file with.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AppRef {
    /// macOS: an `.app` bundle (or any path `open -a` accepts);
    /// Windows / Linux: an executable, started as `<exe> <file>`.
    Path(PathBuf),
    /// macOS only: application name resolved by LaunchServices
    /// (`open -a "TextEdit"`).
    Name(String),
    /// macOS only: bundle identifier (`open -b com.apple.TextEdit`).
    BundleId(String),
}

/// How a [`LaunchSpec::Exec`] process is awaited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitMode {
    /// Wait for exit; a non-zero status is an error (stderr is reported).
    Status,
    /// Wait for exit and return stdout (dialogs that report a choice).
    Capture,
    /// Do not wait (the process is an editor / dialog that stays open); the
    /// child is reaped on a background thread.
    Detach,
}

/// A process to start (argument vector, no shell).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// Windows only: appended verbatim (`CommandExt::raw_arg`), for
    /// `rundll32`, which hands the raw rest of the command line to the DLL
    /// entry point. Elsewhere it is passed as one more argument.
    pub raw_tail: Option<OsString>,
    pub wait: WaitMode,
}

/// What a [`Launcher`] should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchSpec {
    /// Hand the path to the OS default handler (`opener::open`: Windows
    /// `ShellExecuteW("open")`, Linux `xdg-open`).
    ShellOpen(PathBuf),
    /// Start a process.
    Exec(ExecSpec),
}

/// Output of a successful launch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchOutput {
    /// Captured stdout ([`WaitMode::Capture`] only).
    pub stdout: String,
}

/// Errors opening / revealing files.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum OpenError {
    /// The operation is not available on this platform.
    #[error("not supported on this platform: {0}")]
    Unsupported(&'static str),
    /// The file to open does not exist.
    #[error("no such file: {0}")]
    NotFound(PathBuf),
    /// The launched program reported a failure.
    #[error("{program} failed: {message}")]
    Failed { program: String, message: String },
    /// The file could run code when opened, and no text editor was found to
    /// open it safely (Linux without a usable `text/plain` application).
    /// The user should choose an editor ([`FileOpener::open_with`]).
    #[error("no text editor found to open this file safely")]
    NoTextEditor,
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}

/// Result of [`FileOpener::choose_app_and_open`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChooseOutcome {
    /// The file was handed to an application. `app` is the chosen one when
    /// the platform reports it (macOS), `None` otherwise (the Windows dialog
    /// opens the file itself).
    Opened { app: Option<AppRef> },
    /// The user dismissed the chooser.
    Cancelled,
}

/// Executes [`LaunchSpec`]s. Implemented by [`SystemLauncher`]; tests use a
/// recording fake so no application is ever started.
pub trait Launcher: Send + Sync + fmt::Debug {
    fn launch(&self, spec: &LaunchSpec) -> Result<LaunchOutput, OpenError>;
}

/// The real launcher (`std::process::Command` / `opener`).
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemLauncher;

/// Keep error messages from misbehaving tools short.
const MAX_MESSAGE: usize = 512;

fn short(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    let s = s.trim();
    match s.char_indices().nth(MAX_MESSAGE) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

impl Launcher for SystemLauncher {
    fn launch(&self, spec: &LaunchSpec) -> Result<LaunchOutput, OpenError> {
        match spec {
            LaunchSpec::ShellOpen(path) => {
                opener::open(path).map_err(|e| OpenError::Failed {
                    program: "default application".into(),
                    message: e.to_string(),
                })?;
                Ok(LaunchOutput::default())
            }
            LaunchSpec::Exec(e) => run_exec(e),
        }
    }
}

fn run_exec(e: &ExecSpec) -> Result<LaunchOutput, OpenError> {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(&e.program);
    cmd.args(&e.args).stdin(Stdio::null());
    if let Some(tail) = &e.raw_tail {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.raw_arg(tail);
        }
        #[cfg(not(windows))]
        {
            cmd.arg(tail);
        }
    }
    let program = e.program.display().to_string();
    match e.wait {
        WaitMode::Detach => {
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
            let mut child = cmd.spawn()?;
            // Reap the child without blocking the caller.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            Ok(LaunchOutput::default())
        }
        WaitMode::Status | WaitMode::Capture => {
            if e.wait == WaitMode::Capture {
                cmd.stdout(Stdio::piped());
            } else {
                cmd.stdout(Stdio::null());
            }
            cmd.stderr(Stdio::piped());
            let out = cmd.output()?;
            if !out.status.success() {
                return Err(OpenError::Failed {
                    program,
                    message: short(&out.stderr),
                });
            }
            Ok(LaunchOutput {
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            })
        }
    }
}

const MAC_OPEN: &str = "/usr/bin/open";
const MAC_OSASCRIPT: &str = "/usr/bin/osascript";

/// AppleScript for "Open With…": shows the standard application chooser and
/// prints the POSIX path of the chosen app (empty on cancel). The prompt is
/// passed as `argv` — nothing is interpolated into the script.
const MAC_CHOOSE_SCRIPT: [&str; 8] = [
    "on run argv",
    "try",
    "set chosen to choose application with prompt (item 1 of argv) as alias",
    "on error number -128",
    "return \"\"",
    "end try",
    "return POSIX path of chosen",
    "end run",
];

/// Opens / reveals local files. Cheap to clone.
#[derive(Debug, Clone)]
pub struct FileOpener {
    os: OsFamily,
    launcher: Arc<dyn Launcher>,
    /// `%SystemRoot%` (Windows system binaries are started by absolute path).
    windows_root: String,
    /// Linux: `$XDG_DATA_HOME` then `$XDG_DATA_DIRS`, searched for the text
    /// editor's `applications/*.desktop` entry.
    xdg_data_dirs: Vec<PathBuf>,
}

impl FileOpener {
    /// The current platform with the real launcher.
    pub fn system() -> Self {
        let windows_root = std::env::var("SystemRoot")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| r"C:\Windows".to_string());
        Self {
            os: OsFamily::current(),
            launcher: Arc::new(SystemLauncher),
            windows_root,
            xdg_data_dirs: xdg_data_dirs_from_env(),
        }
    }

    /// Explicit platform family and launcher (tests, other front ends).
    pub fn new(os: OsFamily, launcher: Arc<dyn Launcher>) -> Self {
        Self {
            os,
            launcher,
            windows_root: r"C:\Windows".to_string(),
            xdg_data_dirs: Vec::new(),
        }
    }

    /// Directories searched for Linux `.desktop` entries (tests).
    pub fn with_xdg_data_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.xdg_data_dirs = dirs;
        self
    }

    /// Platform family this opener targets.
    pub fn os(&self) -> OsFamily {
        self.os
    }

    /// Whether [`FileOpener::open_with`] accepts `app` here.
    pub fn supports_app(&self, app: &AppRef) -> bool {
        matches!((self.os, app), (OsFamily::MacOs, _) | (_, AppRef::Path(_)))
    }

    /// Whether [`FileOpener::choose_app_and_open`] is available here.
    pub fn supports_choose(&self) -> bool {
        matches!(self.os, OsFamily::MacOs | OsFamily::Windows)
    }

    /// Open `path` with its default application — or, for active content
    /// ([`crate::active::classify_path`]), as text ([`OpenError::NoTextEditor`]
    /// when no text editor is found).
    pub fn open_path(&self, path: &Path) -> Result<(), OpenError> {
        let path = existing_absolute(path)?;
        let spec = self.open_spec(&path)?;
        self.launcher.launch(&spec).map(|_| ())
    }

    /// Open `path` in a text editor, whatever its type (macOS TextEdit,
    /// Windows Notepad, Linux the `text/plain` application).
    pub fn open_as_text(&self, path: &Path) -> Result<(), OpenError> {
        let path = existing_absolute(path)?;
        let spec = self.text_spec(&path)?;
        self.launcher.launch(&spec).map(|_| ())
    }

    /// Open `path` with `app`.
    pub fn open_with(&self, path: &Path, app: &AppRef) -> Result<(), OpenError> {
        let path = existing_absolute(path)?;
        let spec = self.open_with_spec(&path, app)?;
        self.launcher.launch(&spec).map(|_| ())
    }

    /// Let the user pick an application, then open `path` with it.
    /// Blocks until the chooser is dismissed on macOS.
    pub fn choose_app_and_open(&self, path: &Path) -> Result<ChooseOutcome, OpenError> {
        let path = existing_absolute(path)?;
        match self.os {
            OsFamily::MacOs => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let mut args = Vec::with_capacity(MAC_CHOOSE_SCRIPT.len() * 2 + 1);
                for line in MAC_CHOOSE_SCRIPT {
                    args.push(OsString::from("-e"));
                    args.push(OsString::from(line));
                }
                args.push(OsString::from(format!(
                    "Choose an application to open \u{201c}{name}\u{201d}:"
                )));
                let out = self.launcher.launch(&LaunchSpec::Exec(ExecSpec {
                    program: MAC_OSASCRIPT.into(),
                    args,
                    raw_tail: None,
                    wait: WaitMode::Capture,
                }))?;
                let chosen = out.stdout.trim_end_matches(['\n', '\r']);
                if chosen.is_empty() {
                    return Ok(ChooseOutcome::Cancelled);
                }
                let app = AppRef::Path(PathBuf::from(chosen));
                self.launcher.launch(&self.open_with_spec(&path, &app)?)?;
                Ok(ChooseOutcome::Opened { app: Some(app) })
            }
            // The "Open with" dialog preselects the default handler, which
            // runs active content: open it as text instead.
            OsFamily::Windows if crate::active::is_active_path(&path) => {
                self.launcher.launch(&self.text_spec(&path)?)?;
                Ok(ChooseOutcome::Opened { app: None })
            }
            OsFamily::Windows => {
                self.launcher.launch(&LaunchSpec::Exec(ExecSpec {
                    program: self.windows_bin(r"System32\rundll32.exe"),
                    args: vec![OsString::from("shell32.dll,OpenAs_RunDLL")],
                    raw_tail: Some(path.into_os_string()),
                    wait: WaitMode::Detach,
                }))?;
                Ok(ChooseOutcome::Opened { app: None })
            }
            OsFamily::Linux => Err(OpenError::Unsupported(
                "application chooser (use open_with with an executable)",
            )),
        }
    }

    /// Show `path` selected in the file manager.
    pub fn reveal(&self, path: &Path) -> Result<(), OpenError> {
        let path = existing_absolute(path)?;
        let spec = match self.os {
            OsFamily::MacOs => exec(
                MAC_OPEN,
                [os("-R"), os("--"), path.into()],
                WaitMode::Status,
            ),
            // explorer.exe returns 1 even on success — do not check its status.
            OsFamily::Windows => LaunchSpec::Exec(ExecSpec {
                program: self.windows_bin("explorer.exe"),
                args: vec![os("/select,"), path.into()],
                raw_tail: None,
                wait: WaitMode::Detach,
            }),
            // No portable "select" without D-Bus: open the containing folder.
            OsFamily::Linux => {
                LaunchSpec::ShellOpen(path.parent().map(Path::to_path_buf).unwrap_or(path))
            }
        };
        self.launcher.launch(&spec).map(|_| ())
    }

    /// What "Open" launches for the absolute, existing `path`.
    fn open_spec(&self, path: &Path) -> Result<LaunchSpec, OpenError> {
        if crate::active::is_active_path(path) {
            return self.text_spec(path);
        }
        Ok(match self.os {
            OsFamily::MacOs => exec(MAC_OPEN, [os("--"), path.into()], WaitMode::Status),
            OsFamily::Windows | OsFamily::Linux => LaunchSpec::ShellOpen(path.to_path_buf()),
        })
    }

    /// A text editor for `path`; never the default handler.
    fn text_spec(&self, path: &Path) -> Result<LaunchSpec, OpenError> {
        Ok(match self.os {
            OsFamily::MacOs => exec(
                MAC_OPEN,
                [os("-e"), os("--"), path.into()],
                WaitMode::Status,
            ),
            OsFamily::Windows => LaunchSpec::Exec(ExecSpec {
                program: self.windows_bin(r"System32\notepad.exe"),
                args: vec![path.into()],
                raw_tail: None,
                wait: WaitMode::Detach,
            }),
            OsFamily::Linux => self.linux_text_editor_spec(path)?,
        })
    }

    /// `xdg-mime query default text/plain` → that `.desktop` entry's `Exec`
    /// as an argument vector (never `xdg-open`, which would dispatch on the
    /// file's own type again).
    fn linux_text_editor_spec(&self, path: &Path) -> Result<LaunchSpec, OpenError> {
        let out = self
            .launcher
            .launch(&exec(
                "xdg-mime",
                [os("query"), os("default"), os("text/plain")],
                WaitMode::Capture,
            ))
            .map_err(|_| OpenError::NoTextEditor)?;
        // The first entry wins; the tool may list several separated by `;`.
        let id = out
            .stdout
            .lines()
            .next()
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        let entry = desktop::find_entry(&self.xdg_data_dirs, id).ok_or(OpenError::NoTextEditor)?;
        desktop::launch_spec(&entry, path).ok_or(OpenError::NoTextEditor)
    }

    fn open_with_spec(&self, path: &Path, app: &AppRef) -> Result<LaunchSpec, OpenError> {
        Ok(match (self.os, app) {
            (OsFamily::MacOs, AppRef::Path(p)) => exec(
                MAC_OPEN,
                [os("-a"), p.into(), os("--"), path.into()],
                WaitMode::Status,
            ),
            (OsFamily::MacOs, AppRef::Name(n)) => exec(
                MAC_OPEN,
                [os("-a"), n.into(), os("--"), path.into()],
                WaitMode::Status,
            ),
            (OsFamily::MacOs, AppRef::BundleId(b)) => exec(
                MAC_OPEN,
                [os("-b"), b.into(), os("--"), path.into()],
                WaitMode::Status,
            ),
            (_, AppRef::Path(exe)) => LaunchSpec::Exec(ExecSpec {
                program: exe.clone(),
                args: vec![path.into()],
                raw_tail: None,
                wait: WaitMode::Detach,
            }),
            (_, AppRef::Name(_)) => {
                return Err(OpenError::Unsupported("application names (macOS only)"))
            }
            (_, AppRef::BundleId(_)) => {
                return Err(OpenError::Unsupported("bundle identifiers (macOS only)"))
            }
        })
    }

    fn windows_bin(&self, rel: &str) -> PathBuf {
        PathBuf::from(format!(
            r"{}\{rel}",
            self.windows_root.trim_end_matches('\\')
        ))
    }
}

fn os(s: impl AsRef<OsStr>) -> OsString {
    s.as_ref().to_os_string()
}

fn exec<const N: usize>(program: &str, args: [OsString; N], wait: WaitMode) -> LaunchSpec {
    LaunchSpec::Exec(ExecSpec {
        program: program.into(),
        args: args.into(),
        raw_tail: None,
        wait,
    })
}

/// `$XDG_DATA_HOME` (default `~/.local/share`), then `$XDG_DATA_DIRS`
/// (default `/usr/local/share:/usr/share`); relative entries are ignored.
fn xdg_data_dirs_from_env() -> Vec<PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
    let mut dirs = Vec::new();
    match var("XDG_DATA_HOME") {
        Some(h) => dirs.push(PathBuf::from(h)),
        None => {
            if let Some(home) = var("HOME") {
                dirs.push(PathBuf::from(home).join(".local/share"));
            }
        }
    }
    let system = var("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    dirs.extend(std::env::split_paths(&system));
    dirs.retain(|d| d.is_absolute());
    dirs
}

/// Freedesktop `.desktop` entries: lookup and `Exec` expansion
/// (Desktop Entry Specification 1.5, "The Exec key").
mod desktop {
    use super::{ExecSpec, LaunchSpec, WaitMode};
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    /// A parsed `[Desktop Entry]`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) struct Entry {
        pub path: PathBuf,
        pub exec: String,
        pub terminal: bool,
        pub name: Option<String>,
        pub icon: Option<String>,
    }

    /// The entry for desktop file id `id` (`org.gnome.TextEditor.desktop`;
    /// a `-` may stand for a subdirectory: `kde4-kate.desktop` →
    /// `kde4/kate.desktop`) in the first data dir that has it.
    pub(super) fn find_entry(dirs: &[PathBuf], id: &str) -> Option<Entry> {
        let valid = id.ends_with(".desktop")
            && !id.contains('/')
            && !id.contains('\\')
            && !id.starts_with('.')
            && !id.chars().any(char::is_control);
        if !valid {
            return None;
        }
        let mut candidates = vec![id.to_string()];
        let mut rel = id.to_string();
        while let Some(i) = rel.find('-') {
            rel.replace_range(i..=i, "/");
            candidates.push(rel.clone());
        }
        for dir in dirs {
            for c in &candidates {
                let p = dir.join("applications").join(c);
                if let Ok(text) = std::fs::read_to_string(&p) {
                    return parse(&p, &text);
                }
            }
        }
        None
    }

    /// `[Desktop Entry]` of an application with an `Exec` key; `None` for
    /// hidden entries, other types or a missing `Exec`.
    pub(super) fn parse(path: &Path, text: &str) -> Option<Entry> {
        let mut in_group = false;
        let (mut exec, mut terminal, mut name, mut icon, mut kind, mut hidden) =
            (None, false, None, None, None, false);
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_group = line == "[Desktop Entry]";
                continue;
            }
            if !in_group || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let v = v.trim();
            match k.trim() {
                "Exec" => exec = Some(unescape_string(v)),
                "Terminal" => terminal = v == "true",
                "Name" => name = Some(unescape_string(v)),
                "Icon" => icon = Some(unescape_string(v)),
                "Type" => kind = Some(v.to_string()),
                "Hidden" => hidden = v == "true",
                _ => {}
            }
        }
        if hidden || kind.as_deref().is_some_and(|k| k != "Application") {
            return None;
        }
        Some(Entry {
            path: path.to_path_buf(),
            exec: exec.filter(|e| !e.trim().is_empty())?,
            terminal,
            name,
            icon,
        })
    }

    /// The `string` value escapes: `\s`, `\n`, `\t`, `\r`, `\\`.
    fn unescape_string(v: &str) -> String {
        let mut out = String::with_capacity(v.len());
        let mut it = v.chars();
        while let Some(c) = it.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match it.next() {
                Some('s') => out.push(' '),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(o) => {
                    out.push('\\');
                    out.push(o);
                }
                None => out.push('\\'),
            }
        }
        out
    }

    /// A token of `Exec`: its text and whether any part was quoted (field
    /// codes are expanded only in unquoted text).
    #[derive(Debug, PartialEq, Eq)]
    struct Token {
        text: String,
        quoted: bool,
    }

    /// Splits `Exec` into arguments: whitespace separates, `"…"` quotes,
    /// and inside quotes `\` escapes `"`, `` ` ``, `$` and `\`.
    fn tokenize(exec: &str) -> Option<Vec<Token>> {
        let mut tokens = Vec::new();
        let mut cur: Option<Token> = None;
        let mut it = exec.chars();
        while let Some(c) = it.next() {
            match c {
                ' ' | '\t' | '\n' => {
                    if let Some(t) = cur.take() {
                        tokens.push(t);
                    }
                }
                '"' => {
                    let t = cur.get_or_insert(Token {
                        text: String::new(),
                        quoted: true,
                    });
                    t.quoted = true;
                    loop {
                        match it.next()? {
                            '"' => break,
                            '\\' => t.text.push(it.next()?),
                            o => t.text.push(o),
                        }
                    }
                }
                o => cur
                    .get_or_insert(Token {
                        text: String::new(),
                        quoted: false,
                    })
                    .text
                    .push(o),
            }
        }
        tokens.extend(cur);
        Some(tokens)
    }

    /// The process for opening `file` with `entry`: field codes expanded
    /// (`%f %F %u %U` → the path, as one argument; `%i %c %k` per the
    /// spec; deprecated codes dropped), the path appended when `Exec` takes
    /// no file. `None` for terminal applications and malformed `Exec`.
    pub(super) fn launch_spec(entry: &Entry, file: &Path) -> Option<LaunchSpec> {
        if entry.terminal {
            return None;
        }
        let mut args: Vec<OsString> = Vec::new();
        let mut file_used = false;
        for t in tokenize(&entry.exec)? {
            if t.quoted {
                args.push(t.text.into());
                continue;
            }
            match t.text.as_str() {
                "%f" | "%F" | "%u" | "%U" => {
                    if !file_used {
                        args.push(file.into());
                        file_used = true;
                    }
                    continue;
                }
                "%i" => {
                    if let Some(icon) = &entry.icon {
                        args.push("--icon".into());
                        args.push(icon.into());
                    }
                    continue;
                }
                "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => continue,
                _ => {}
            }
            // Codes inside a longer argument (`--file=%f`).
            let mut out = String::new();
            let mut it = t.text.chars();
            let mut path_in_arg = false;
            while let Some(c) = it.next() {
                if c != '%' {
                    out.push(c);
                    continue;
                }
                match it.next() {
                    Some('%') => out.push('%'),
                    Some('f' | 'F' | 'u' | 'U') if !file_used => {
                        out.push_str(&file.to_string_lossy());
                        file_used = true;
                        path_in_arg = true;
                    }
                    Some('c') => out.push_str(entry.name.as_deref().unwrap_or("")),
                    Some('k') => out.push_str(&entry.path.to_string_lossy()),
                    _ => {}
                }
            }
            // A non-UTF-8 path cannot be embedded: pass it whole instead.
            if path_in_arg && file.to_str().is_none() {
                return None;
            }
            args.push(out.into());
        }
        if args.is_empty() {
            return None;
        }
        if !file_used {
            args.push(file.into());
        }
        let program = PathBuf::from(args.remove(0));
        Some(LaunchSpec::Exec(ExecSpec {
            program,
            args,
            raw_tail: None,
            wait: WaitMode::Detach,
        }))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn entry(exec: &str) -> Entry {
            Entry {
                path: "/usr/share/applications/ed.desktop".into(),
                exec: exec.into(),
                terminal: false,
                name: Some("Text Editor".into()),
                icon: Some("accessories-text-editor".into()),
            }
        }

        fn argv(exec: &str, file: &str) -> Option<Vec<String>> {
            match launch_spec(&entry(exec), Path::new(file))? {
                LaunchSpec::Exec(e) => Some(
                    std::iter::once(e.program.to_string_lossy().into_owned())
                        .chain(e.args.iter().map(|a| a.to_string_lossy().into_owned()))
                        .collect(),
                ),
                LaunchSpec::ShellOpen(_) => panic!("never ShellOpen"),
            }
        }

        #[test]
        fn exec_expansion() {
            let f = "/tmp/a b/$(x);`y`.sh";
            assert_eq!(
                argv("gnome-text-editor %U", f).unwrap(),
                vec!["gnome-text-editor", f]
            );
            assert_eq!(argv("gedit", f).unwrap(), vec!["gedit", f]);
            assert_eq!(
                argv("kate -b %U %i %c", f).unwrap(),
                vec![
                    "kate",
                    "-b",
                    f,
                    "--icon",
                    "accessories-text-editor",
                    "Text Editor"
                ],
                "tokens are split before expansion"
            );
            assert_eq!(
                argv(r#""/opt/My Editor/ed" --new %f %%"#, f).unwrap(),
                vec!["/opt/My Editor/ed", "--new", f, "%"]
            );
            assert_eq!(
                argv("ed --file=%f", f).unwrap(),
                vec!["ed".to_string(), format!("--file={f}")]
            );
            // Quoted text is never expanded (no injection into `sh -c`).
            assert_eq!(
                argv(r#"sh -c "ed \"%f\"""#, f).unwrap(),
                vec!["sh", "-c", r#"ed "%f""#, f]
            );
            // Flatpak wrapper.
            assert_eq!(
                argv(
                    "/usr/bin/flatpak run --command=gnome-text-editor org.gnome.TextEditor --file-forwarding org.gnome.TextEditor @@ %U @@",
                    f
                )
                .unwrap()[5..],
                ["org.gnome.TextEditor".to_string(), "@@".into(), f.into(), "@@".into()]
            );
            assert!(argv("", f).is_none());
            assert!(argv(r#"ed "unterminated"#, f).is_none());
            assert!(argv("%d %n", f).is_none(), "no program left");
        }

        #[test]
        fn terminal_apps_are_refused() {
            let mut e = entry("vim %F");
            e.terminal = true;
            assert!(launch_spec(&e, Path::new("/tmp/x.sh")).is_none());
        }

        #[test]
        fn parse_and_find() {
            let dir = tempfile::tempdir().unwrap();
            let apps = dir.path().join("applications");
            std::fs::create_dir_all(apps.join("kde4")).unwrap();
            std::fs::write(
                apps.join("kde4/kate.desktop"),
                "[Desktop Entry]\nType=Application\nName=Kate\nExec=kate\\s-b %U\n[Desktop Action new]\nExec=evil\n",
            )
            .unwrap();
            std::fs::write(
                apps.join("hidden.desktop"),
                "[Desktop Entry]\nExec=ed\nHidden=true\n",
            )
            .unwrap();
            std::fs::write(
                apps.join("link.desktop"),
                "[Desktop Entry]\nType=Link\nURL=x\n",
            )
            .unwrap();
            let dirs = vec![PathBuf::from("/nonexistent"), dir.path().to_path_buf()];
            let e = find_entry(&dirs, "kde4-kate.desktop").unwrap();
            assert_eq!(e.exec, "kate -b %U");
            assert_eq!(e.name.as_deref(), Some("Kate"));
            assert!(find_entry(&dirs, "hidden.desktop").is_none());
            assert!(find_entry(&dirs, "link.desktop").is_none());
            for bad in ["", "kate", "../kate.desktop", "a/b.desktop", ".desktop"] {
                assert!(find_entry(&dirs, bad).is_none(), "{bad}");
            }
        }
    }
}

fn existing_absolute(path: &Path) -> Result<PathBuf, OpenError> {
    let abs = std::path::absolute(path)?;
    if std::fs::symlink_metadata(&abs).is_err() {
        return Err(OpenError::NotFound(abs));
    }
    Ok(abs)
}

/// [`FileOpener::open_path`] on the current platform.
pub fn open_path(path: &Path) -> Result<(), OpenError> {
    FileOpener::system().open_path(path)
}

/// [`FileOpener::open_with`] on the current platform.
pub fn open_with(path: &Path, app: &AppRef) -> Result<(), OpenError> {
    FileOpener::system().open_with(path, app)
}

/// [`FileOpener::choose_app_and_open`] on the current platform.
pub fn choose_app_and_open(path: &Path) -> Result<ChooseOutcome, OpenError> {
    FileOpener::system().choose_app_and_open(path)
}

/// [`FileOpener::reveal`] on the current platform.
pub fn reveal(path: &Path) -> Result<(), OpenError> {
    FileOpener::system().reveal(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Debug, Default)]
    struct Recorder {
        specs: Mutex<Vec<LaunchSpec>>,
        stdout: Mutex<Vec<String>>,
    }

    impl Launcher for Recorder {
        fn launch(&self, spec: &LaunchSpec) -> Result<LaunchOutput, OpenError> {
            self.specs.lock().unwrap().push(spec.clone());
            let stdout = self.stdout.lock().unwrap().pop().unwrap_or_default();
            Ok(LaunchOutput { stdout })
        }
    }

    fn setup(os: OsFamily) -> (FileOpener, Arc<Recorder>, tempfile::TempDir, PathBuf) {
        let rec = Arc::new(Recorder::default());
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("my file.txt");
        std::fs::write(&file, b"x").unwrap();
        (FileOpener::new(os, rec.clone()), rec, dir, file)
    }

    fn args(spec: &LaunchSpec) -> (String, Vec<String>, WaitMode) {
        match spec {
            LaunchSpec::Exec(e) => (
                e.program.to_string_lossy().into_owned(),
                e.args
                    .iter()
                    .chain(e.raw_tail.iter())
                    .map(|a| a.to_string_lossy().into_owned())
                    .collect(),
                e.wait,
            ),
            other => panic!("expected exec, got {other:?}"),
        }
    }

    #[test]
    fn macos_specs_use_open_with_argument_vectors() {
        let (o, rec, _d, file) = setup(OsFamily::MacOs);
        let f = file.to_string_lossy().into_owned();
        o.open_path(&file).unwrap();
        o.open_with(&file, &AppRef::Path("/Applications/Text Edit.app".into()))
            .unwrap();
        o.open_with(&file, &AppRef::Name("TextEdit".into()))
            .unwrap();
        o.open_with(&file, &AppRef::BundleId("com.apple.TextEdit".into()))
            .unwrap();
        o.reveal(&file).unwrap();
        let specs = rec.specs.lock().unwrap().clone();
        let s: Vec<_> = specs.iter().map(args).collect();
        assert_eq!(
            s[0],
            (
                MAC_OPEN.into(),
                vec!["--".into(), f.clone()],
                WaitMode::Status
            )
        );
        assert_eq!(
            s[1].1,
            vec!["-a", "/Applications/Text Edit.app", "--", f.as_str()]
        );
        assert_eq!(s[2].1, vec!["-a", "TextEdit", "--", f.as_str()]);
        assert_eq!(s[3].1, vec!["-b", "com.apple.TextEdit", "--", f.as_str()]);
        assert_eq!(s[4].1, vec!["-R", "--", f.as_str()]);
        assert!(s.iter().all(|x| x.0 == MAC_OPEN && x.2 == WaitMode::Status));
    }

    #[test]
    fn macos_editor_bundle_paths_bypass_applescript_and_preserve_arguments() {
        let (o, rec, dir, _) = setup(OsFamily::MacOs);
        let file = dir.path().join("docker compose 'тест'.yml");
        std::fs::write(&file, b"services: {}\n").unwrap();
        for app in [
            "/Applications/Visual Studio Code.app",
            "/Applications/Zed.app",
        ] {
            o.open_with(&file, &AppRef::Path(app.into())).unwrap();
            let specs = rec.specs.lock().unwrap();
            let (program, args, wait) = args(specs.last().unwrap());
            assert_eq!(program, MAC_OPEN);
            assert_eq!(args, vec!["-a", app, "--", file.to_str().unwrap()]);
            assert_eq!(wait, WaitMode::Status);
        }
    }

    #[test]
    fn macos_chooser_reports_choice_or_cancel() {
        let (o, rec, _d, file) = setup(OsFamily::MacOs);
        rec.stdout
            .lock()
            .unwrap()
            .push("/Applications/BBEdit.app/\n".into());
        let out = o.choose_app_and_open(&file).unwrap();
        let app = AppRef::Path("/Applications/BBEdit.app/".into());
        assert_eq!(out, ChooseOutcome::Opened { app: Some(app) });
        {
            let specs = rec.specs.lock().unwrap();
            assert_eq!(specs.len(), 2);
            let (prog, a, wait) = args(&specs[0]);
            assert_eq!(prog, MAC_OSASCRIPT);
            assert_eq!(wait, WaitMode::Capture);
            // The file name only appears in the argv prompt, never in a script line.
            let script: Vec<_> = a.iter().skip(1).step_by(2).take(8).collect();
            assert!(script.iter().all(|l| !l.contains("my file")));
            assert!(a.last().unwrap().contains("my file.txt"));
            let (_, a2, _) = args(&specs[1]);
            assert_eq!(
                a2[..2],
                ["-a".to_string(), "/Applications/BBEdit.app/".into()]
            );
        }
        rec.specs.lock().unwrap().clear();
        rec.stdout.lock().unwrap().push("\n".into());
        assert_eq!(
            o.choose_app_and_open(&file).unwrap(),
            ChooseOutcome::Cancelled
        );
        assert_eq!(
            rec.specs.lock().unwrap().len(),
            1,
            "nothing opened on cancel"
        );
    }

    #[test]
    fn windows_specs() {
        let (o, rec, _d, file) = setup(OsFamily::Windows);
        let f = file.to_string_lossy().into_owned();
        o.open_path(&file).unwrap();
        o.open_with(&file, &AppRef::Path(r"C:\Tools\notepad++.exe".into()))
            .unwrap();
        assert_eq!(
            o.choose_app_and_open(&file).unwrap(),
            ChooseOutcome::Opened { app: None }
        );
        o.reveal(&file).unwrap();
        assert!(matches!(
            o.open_with(&file, &AppRef::Name("Notepad".into())),
            Err(OpenError::Unsupported(_))
        ));
        let specs = rec.specs.lock().unwrap().clone();
        assert_eq!(specs[0], LaunchSpec::ShellOpen(file.clone()));
        assert_eq!(
            args(&specs[1]),
            (
                r"C:\Tools\notepad++.exe".into(),
                vec![f.clone()],
                WaitMode::Detach
            )
        );
        let LaunchSpec::Exec(ch) = &specs[2] else {
            panic!()
        };
        assert_eq!(
            ch.program,
            PathBuf::from(r"C:\Windows\System32\rundll32.exe")
        );
        assert_eq!(ch.args, vec![OsString::from("shell32.dll,OpenAs_RunDLL")]);
        assert_eq!(ch.raw_tail, Some(file.clone().into_os_string()));
        assert_eq!(
            args(&specs[3]),
            (
                r"C:\Windows\explorer.exe".into(),
                vec!["/select,".into(), f],
                WaitMode::Detach
            )
        );
    }

    #[test]
    fn linux_specs() {
        let (o, rec, dir, file) = setup(OsFamily::Linux);
        o.open_path(&file).unwrap();
        o.open_with(&file, &AppRef::Path("/usr/bin/gedit".into()))
            .unwrap();
        o.reveal(&file).unwrap();
        assert!(matches!(
            o.choose_app_and_open(&file),
            Err(OpenError::Unsupported(_))
        ));
        assert!(!o.supports_choose());
        assert!(!o.supports_app(&AppRef::BundleId("x".into())));
        assert!(o.supports_app(&AppRef::Path("/usr/bin/gedit".into())));
        let specs = rec.specs.lock().unwrap().clone();
        assert_eq!(specs[0], LaunchSpec::ShellOpen(file.clone()));
        assert_eq!(args(&specs[1]).1, vec![file.to_string_lossy().into_owned()]);
        assert_eq!(
            specs[2],
            LaunchSpec::ShellOpen(std::path::absolute(dir.path()).unwrap())
        );
    }

    #[test]
    fn missing_and_relative_paths() {
        let (o, rec, _d, _f) = setup(OsFamily::MacOs);
        let missing = Path::new("/definitely/not/here.txt");
        assert!(matches!(o.open_path(missing), Err(OpenError::NotFound(_))));
        assert!(rec.specs.lock().unwrap().is_empty());
        // Relative paths are made absolute, so they can never look like options.
        let rel = Path::new("Cargo.toml");
        o.open_path(rel).unwrap();
        let (_, a, _) = args(&rec.specs.lock().unwrap()[0]);
        assert!(Path::new(&a[1]).is_absolute());
    }

    /// Never the default handler: no `ShellOpen`, no plain `open -- path`,
    /// no Windows chooser.
    fn assert_not_default(spec: &LaunchSpec) {
        match spec {
            LaunchSpec::ShellOpen(p) => panic!("default handler for {p:?}"),
            LaunchSpec::Exec(e) => {
                assert!(!e.program.to_string_lossy().contains("rundll32"), "{e:?}");
                if e.program == Path::new(MAC_OPEN) {
                    assert_eq!(e.args[0], OsString::from("-e"), "{e:?}");
                }
            }
        }
    }

    fn active_files(dir: &Path) -> Vec<PathBuf> {
        let mut v = Vec::new();
        for name in [
            "run.command",
            "x.txt.js",
            "launch.desktop",
            "setup.exe",
            "evil.js.",
            "Thing.app",
        ] {
            let p = dir.join(name);
            std::fs::write(&p, b"x").unwrap();
            v.push(p);
        }
        let script = dir.join("deploy");
        std::fs::write(&script, b"#!/bin/sh\necho\n").unwrap();
        v.push(script);
        v
    }

    #[test]
    fn active_content_never_reaches_the_default_handler() {
        for os_family in [OsFamily::MacOs, OsFamily::Windows, OsFamily::Linux] {
            let (o, rec, dir, _) = setup(os_family);
            let apps = dir.path().join("share/applications");
            std::fs::create_dir_all(&apps).unwrap();
            std::fs::write(
                apps.join("org.gnome.TextEditor.desktop"),
                "[Desktop Entry]\nType=Application\nExec=gnome-text-editor %U\n",
            )
            .unwrap();
            let o = o.with_xdg_data_dirs(vec![dir.path().join("share")]);
            for file in active_files(dir.path()) {
                rec.specs.lock().unwrap().clear();
                rec.stdout
                    .lock()
                    .unwrap()
                    .push("org.gnome.TextEditor.desktop\n".into());
                o.open_path(&file).unwrap();
                let specs = rec.specs.lock().unwrap().clone();
                specs.iter().for_each(assert_not_default);
                // Windows drops the trailing dot of `evil.js.` when making it absolute.
                let f = std::path::absolute(&file)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                let last = args(specs.last().unwrap());
                match os_family {
                    OsFamily::MacOs => {
                        assert_eq!(
                            last,
                            (
                                MAC_OPEN.into(),
                                vec!["-e".into(), "--".into(), f],
                                WaitMode::Status
                            )
                        )
                    }
                    OsFamily::Windows => assert_eq!(
                        last,
                        (
                            r"C:\Windows\System32\notepad.exe".into(),
                            vec![f],
                            WaitMode::Detach
                        )
                    ),
                    OsFamily::Linux => {
                        assert_eq!(args(&specs[0]).1, vec!["query", "default", "text/plain"]);
                        assert_eq!(
                            last,
                            ("gnome-text-editor".into(), vec![f], WaitMode::Detach)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn linux_without_a_text_editor_refuses() {
        let (o, rec, dir, _) = setup(OsFamily::Linux);
        let o = o.with_xdg_data_dirs(vec![dir.path().to_path_buf()]);
        let file = dir.path().join("install.sh");
        std::fs::write(&file, b"rm -rf ~").unwrap();
        // No default, an unknown id, a terminal editor.
        for answer in ["", "missing.desktop", "vim.desktop"] {
            let apps = dir.path().join("applications");
            std::fs::create_dir_all(&apps).unwrap();
            std::fs::write(
                apps.join("vim.desktop"),
                "[Desktop Entry]\nExec=vim %F\nTerminal=true\n",
            )
            .unwrap();
            rec.specs.lock().unwrap().clear();
            rec.stdout.lock().unwrap().push(format!("{answer}\n"));
            assert!(
                matches!(o.open_path(&file), Err(OpenError::NoTextEditor)),
                "{answer}"
            );
            assert_eq!(rec.specs.lock().unwrap().len(), 1, "only the query ran");
        }
    }

    #[test]
    fn windows_chooser_is_not_offered_for_active_content() {
        let (o, rec, dir, _) = setup(OsFamily::Windows);
        let file = dir.path().join("invoice.pdf.js");
        std::fs::write(&file, b"WScript.Echo(1)").unwrap();
        assert_eq!(
            o.choose_app_and_open(&file).unwrap(),
            ChooseOutcome::Opened { app: None }
        );
        let specs = rec.specs.lock().unwrap().clone();
        assert_eq!(specs.len(), 1);
        assert_not_default(&specs[0]);
        assert_eq!(
            args(&specs[0]).0,
            r"C:\Windows\System32\notepad.exe".to_string()
        );
    }

    #[test]
    fn explicit_apps_and_documents_are_unchanged() {
        let (o, rec, dir, doc) = setup(OsFamily::MacOs);
        let script = dir.path().join("run.command");
        std::fs::write(&script, b"x").unwrap();
        // "Open With…" a chosen application stays the user's decision.
        o.open_with(&script, &AppRef::Name("BBEdit".into()))
            .unwrap();
        // Documents keep their default application.
        o.open_path(&doc).unwrap();
        o.open_as_text(&doc).unwrap();
        let s: Vec<_> = rec.specs.lock().unwrap().iter().map(args).collect();
        assert_eq!(s[0].1[..2], ["-a".to_string(), "BBEdit".into()]);
        assert_eq!(s[1].1[0], "--");
        assert_eq!(s[2].1[0], "-e");
    }

    #[cfg(unix)]
    #[test]
    fn system_launcher_runs_argument_vectors() {
        let l = SystemLauncher;
        let out = l
            .launch(&exec(
                "/bin/echo",
                [os("a b;$(rm -rf /)"), os("'c'")],
                WaitMode::Capture,
            ))
            .unwrap();
        assert_eq!(
            out.stdout, "a b;$(rm -rf /) 'c'\n",
            "no shell interpretation"
        );
        let err = l
            .launch(&exec(
                "/bin/sh",
                [os("-c"), os("echo boom >&2; exit 3")],
                WaitMode::Status,
            ))
            .unwrap_err();
        assert!(matches!(err, OpenError::Failed { ref message, .. } if message == "boom"));
        l.launch(&exec("/usr/bin/true", [], WaitMode::Detach))
            .unwrap();
        assert!(l
            .launch(&exec("/nonexistent/bin", [], WaitMode::Status))
            .is_err());
    }
}
