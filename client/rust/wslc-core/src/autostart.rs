//! Start with Windows through the per-user `Run` key; nothing machine-wide is touched.
use crate::models::*;
use std::path::Path;

pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const VALUE_NAME: &str = "WSLc Desk";
/// The window stays hidden; the app lives in the notification area.
pub const HIDDEN_ARG: &str = "--minimized";

pub fn command(exe: &Path) -> String {
    format!(
        "\"{}\" {HIDDEN_ARG}",
        crate::desktop::simplify_path(exe).display()
    )
}

#[cfg(windows)]
fn failed(operation: &str, e: std::io::Error) -> AppError {
    let mut error = AppError::new("autostart", operation, "Не удалось изменить автозапуск");
    error.diagnostics = Some(e.to_string());
    error
}

/// The entry exists and starts this very executable, not an old copy elsewhere.
#[cfg(windows)]
pub fn enabled_in(key: &str, exe: &Path) -> bool {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(key)
        .and_then(|run| run.get_value::<String, _>(VALUE_NAME))
        .is_ok_and(|value| value.eq_ignore_ascii_case(&command(exe)))
}

#[cfg(windows)]
pub fn set_in(key: &str, exe: &Path, enabled: bool) -> AppResult<()> {
    use std::io::ErrorKind;
    use winreg::{
        RegKey,
        enums::{HKEY_CURRENT_USER, KEY_SET_VALUE},
    };
    let user = RegKey::predef(HKEY_CURRENT_USER);
    if enabled {
        let (run, _) = user
            .create_subkey(key)
            .map_err(|e| failed("enable_autostart", e))?;
        return run
            .set_value(VALUE_NAME, &command(exe))
            .map_err(|e| failed("enable_autostart", e));
    }
    match user
        .open_subkey_with_flags(key, KEY_SET_VALUE)
        .and_then(|run| run.delete_value(VALUE_NAME))
    {
        // Already absent is the requested state.
        Err(e) if e.kind() != ErrorKind::NotFound => Err(failed("disable_autostart", e)),
        _ => Ok(()),
    }
}

#[cfg(not(windows))]
pub fn enabled_in(_key: &str, _exe: &Path) -> bool {
    false
}

#[cfg(not(windows))]
pub fn set_in(_key: &str, _exe: &Path, _enabled: bool) -> AppResult<()> {
    Err(AppError::new(
        "unsupported",
        "autostart",
        "Автозапуск доступен только в Windows",
    ))
}
