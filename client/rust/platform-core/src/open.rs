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
        }
    }

    /// Explicit platform family and launcher (tests, other front ends).
    pub fn new(os: OsFamily, launcher: Arc<dyn Launcher>) -> Self {
        Self {
            os,
            launcher,
            windows_root: r"C:\Windows".to_string(),
        }
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

    /// Open `path` with its default application.
    pub fn open_path(&self, path: &Path) -> Result<(), OpenError> {
        let path = existing_absolute(path)?;
        self.launcher.launch(&self.open_spec(&path)).map(|_| ())
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

    fn open_spec(&self, path: &Path) -> LaunchSpec {
        match self.os {
            OsFamily::MacOs => exec(MAC_OPEN, [os("--"), path.into()], WaitMode::Status),
            OsFamily::Windows | OsFamily::Linux => LaunchSpec::ShellOpen(path.to_path_buf()),
        }
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
