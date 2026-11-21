//! Preferences and container templates in `%APPDATA%\WSLc Desk\settings.json`.
use crate::models::*;
use serde::{Deserialize, Deserializer};
use std::path::{Path, PathBuf};

pub fn default_path() -> AppResult<PathBuf> {
    std::env::var_os("APPDATA")
        .map(|dir| Path::new(&dir).join("WSLc Desk").join("settings.json"))
        .ok_or_else(|| {
            AppError::new(
                "settings",
                "settings_path",
                "Не найдена папка профиля (APPDATA)",
            )
        })
}

/// A template that no longer parses (say, from a future version) is dropped,
/// not allowed to take every other setting down with it.
pub fn lenient_templates<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Vec<ContainerTemplate>, D::Error> {
    let raw = Vec::<serde_json::Value>::deserialize(d)?;
    Ok(raw
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect())
}

/// Missing file: defaults. Damaged file: kept aside as `settings.json.bad` for the
/// user to inspect, and defaults are used; it is never silently overwritten.
pub fn load_from(path: &Path) -> DeskSettings {
    let Ok(text) = std::fs::read_to_string(path) else {
        return DeskSettings::default();
    };
    serde_json::from_str(&text).unwrap_or_else(|_| {
        let _ = std::fs::rename(path, path.with_extension("json.bad"));
        DeskSettings::default()
    })
}

fn failed(e: impl std::fmt::Display) -> AppError {
    let mut error = AppError::new(
        "settings",
        "save_settings",
        "Не удалось сохранить настройки",
    );
    error.diagnostics = Some(e.to_string());
    error
}

/// Written to a temporary file and moved over the old one, so a crash mid-write
/// never leaves half a file behind.
pub fn save_to(path: &Path, settings: &DeskSettings) -> AppResult<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(failed)?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_vec_pretty(settings).map_err(failed)?;
    std::fs::write(&tmp, text).map_err(failed)?;
    std::fs::rename(&tmp, path).map_err(failed)
}
