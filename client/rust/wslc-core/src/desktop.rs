//! Windows shell integration: browser, Explorer and a console for `wslc exec`.
//! Every target is rebuilt from validated parts; nothing is passed through a shell.
use crate::{models::*, runtime::cli::validate_id};
use std::{
    net::IpAddr,
    path::{Path, PathBuf},
    process::Command,
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

/// `canonicalize` returns verbatim `\\?\C:\…` paths on Windows; show and use the plain form.
pub fn simplify_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

fn explorer() -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    Path::new(&root).join("explorer.exe")
}

fn failed(operation: &str, message: &str, e: std::io::Error) -> AppError {
    let mut error = AppError::new("shell", operation, message);
    error.diagnostics = Some(e.to_string());
    error
}

/// URL for a published port. Hosts are IP literals or `localhost` only.
pub fn port_url(host: &str, port: u16, https: bool) -> AppResult<String> {
    let invalid = || AppError::new("invalid_url", "open_url", "Некорректный адрес порта");
    if port == 0 {
        return Err(invalid());
    }
    let host = match host {
        "localhost" => host.to_owned(),
        other => match other.parse::<IpAddr>().map_err(|_| invalid())? {
            IpAddr::V4(ip) => ip.to_string(),
            IpAddr::V6(ip) => format!("[{ip}]"),
        },
    };
    let scheme = if https { "https" } else { "http" };
    Ok(format!("{scheme}://{host}:{port}/"))
}

pub fn open_url(host: &str, port: u16, https: bool) -> AppResult<()> {
    let url = port_url(host, port, https)?;
    // Explorer hands URLs to the default browser; its exit code is not meaningful.
    Command::new(explorer())
        .arg(url)
        .spawn()
        .map(drop)
        .map_err(|e| failed("open_url", "Не удалось открыть браузер", e))
}

/// Accepts absolute local or UNC paths that exist; files are selected in their folder.
pub fn host_path(raw: &str) -> AppResult<PathBuf> {
    let invalid = || {
        AppError::new(
            "invalid_path",
            "open_folder",
            "Папка недоступна на этом компьютере",
        )
    };
    let path = PathBuf::from(raw.trim());
    let text = path.to_string_lossy();
    let drive = text.as_bytes().get(1) == Some(&b':') && text.as_bytes().get(2) == Some(&b'\\');
    if !(drive || text.starts_with(r"\\")) || text.contains('"') || !path.is_absolute() {
        return Err(invalid());
    }
    if !path.exists() {
        return Err(invalid());
    }
    Ok(path)
}

/// A file to be written: absolute local or UNC path whose folder already exists.
pub fn output_path(raw: &str) -> AppResult<PathBuf> {
    let invalid = || {
        AppError::new(
            "invalid_path",
            "output",
            "Укажите путь к файлу в существующей папке",
        )
    };
    let path = PathBuf::from(raw.trim());
    let text = path.to_string_lossy();
    let drive = text.as_bytes().get(1) == Some(&b':') && text.as_bytes().get(2) == Some(&b'\\');
    if !(drive || text.starts_with(r"\\")) || text.contains('"') || path.is_dir() {
        return Err(invalid());
    }
    match path.parent() {
        Some(dir) if dir.is_dir() => Ok(path),
        _ => Err(invalid()),
    }
}

pub fn open_folder(raw: &str) -> AppResult<()> {
    let path = host_path(raw)?;
    let mut command = Command::new(explorer());
    // Windows paths cannot contain quotes, so the raw argument cannot be broken out of.
    #[cfg(windows)]
    if path.is_dir() {
        command.raw_arg(format!("\"{}\"", path.display()));
    } else {
        command.raw_arg(format!("/select,\"{}\"", path.display()));
    }
    #[cfg(not(windows))]
    command.arg(&path);
    command
        .spawn()
        .map(drop)
        .map_err(|e| failed("open_folder", "Не удалось открыть проводник", e))
}

pub fn validate_container_path(path: &str) -> AppResult<()> {
    if !path.starts_with('/') || path.len() > 4096 || path.contains('\0') {
        return Err(AppError::new(
            "invalid_path",
            "container_path",
            "Путь внутри контейнера должен быть абсолютным",
        ));
    }
    Ok(())
}

/// Interactive shell in a new console window; prefers bash and falls back to sh.
pub fn terminal_args(id: &str, workdir: Option<&str>) -> AppResult<Vec<String>> {
    validate_id(id)?;
    let mut args = vec!["exec".into(), "-it".into()];
    if let Some(dir) = workdir {
        validate_container_path(dir)?;
        args.extend(["-w".into(), dir.into()]);
    }
    args.extend([
        id.into(),
        "sh".into(),
        "-c".into(),
        "if command -v bash >/dev/null 2>&1; then exec bash; else exec sh; fi".into(),
    ]);
    Ok(args)
}

pub fn open_terminal(cli: &Path, id: &str, workdir: Option<&str>) -> AppResult<()> {
    let args = terminal_args(id, workdir)?;
    let mut command = Command::new(cli);
    command.args(args);
    #[cfg(windows)]
    command.creation_flags(CREATE_NEW_CONSOLE);
    // The console owns the session; dropping the handle does not end it.
    command
        .spawn()
        .map(drop)
        .map_err(|e| failed("open_terminal", "Не удалось открыть терминал", e))
}
