//! File transfer through `wslc exec`: `cat` streams bytes out, `cat > "$1"` takes them in.
//! Paths are positional arguments of the in-container shell, never part of its script.
use crate::{
    desktop::validate_container_path, models::*, runtime::cli::validate_id, runtime::process,
};
use std::{path::Path, time::Duration};

pub const FILE_LIMIT: usize = 64 * 1024 * 1024;
pub const TEXT_LIMIT: usize = 2 * 1024 * 1024;
const WRITE_SCRIPT: &str = r#"cat > "$1""#;

pub fn read_args(id: &str, path: &str) -> AppResult<Vec<String>> {
    validate_id(id)?;
    validate_container_path(path)?;
    Ok(["exec", id, "cat", "--", path].map(String::from).to_vec())
}

pub fn write_args(id: &str, path: &str) -> AppResult<Vec<String>> {
    validate_id(id)?;
    validate_container_path(path)?;
    if path.ends_with('/') {
        return Err(AppError::new(
            "invalid_path",
            "write_file",
            "Укажите путь к файлу, а не к каталогу",
        ));
    }
    Ok([
        "exec",
        "-i",
        id,
        "sh",
        "-c",
        WRITE_SCRIPT,
        "wslc-desk-put",
        path,
    ]
    .map(String::from)
    .to_vec())
}

/// A single path segment for an uploaded file name.
pub fn validate_file_name(name: &str) -> AppResult<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\0']) {
        return Err(AppError::new(
            "invalid_name",
            "upload",
            "Некорректное имя файла",
        ));
    }
    Ok(())
}

pub fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// Text for the editor: rejects binary content and invalid UTF-8.
pub fn as_text(bytes: Vec<u8>) -> AppResult<String> {
    if bytes.contains(&0) {
        return Err(AppError::new(
            "binary",
            "read_text",
            "Это двоичный файл — его можно только скачать",
        ));
    }
    String::from_utf8(bytes).map_err(|_| {
        AppError::new(
            "encoding",
            "read_text",
            "Файл не в UTF-8 — его можно только скачать",
        )
    })
}

pub async fn read(cli: &Path, id: &str, path: &str, limit: usize) -> AppResult<Vec<u8>> {
    process::run_bytes(
        cli,
        &read_args(id, path)?,
        None,
        Duration::from_secs(120),
        "read_file",
        limit,
    )
    .await
}

pub async fn write(cli: &Path, id: &str, path: &str, data: &[u8]) -> AppResult<()> {
    process::run_bytes(
        cli,
        &write_args(id, path)?,
        Some(data),
        Duration::from_secs(120),
        "write_file",
        64 * 1024,
    )
    .await
    .map(drop)
}

#[cfg(windows)]
fn dialog_error(operation: &str) -> AppError {
    AppError::new("dialog_failed", operation, "Ошибка системного диалога")
}

/// Asks where to save, then copies the container file there.
#[cfg(windows)]
pub async fn download(cli: &Path, id: &str, path: &str) -> AppResult<Option<String>> {
    let data = read(cli, id, path, FILE_LIMIT).await?;
    let name = path.rsplit('/').next().unwrap_or("file").to_owned();
    tokio::task::spawn_blocking(move || {
        let Some(target) = rfd::FileDialog::new()
            .set_title("Сохранить файл из контейнера")
            .set_file_name(name)
            .save_file()
        else {
            return Ok(None);
        };
        std::fs::write(&target, data)
            .map_err(|_| AppError::new("save_failed", "download", "Не удалось сохранить файл"))?;
        Ok(Some(target.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|_| dialog_error("download"))?
}

#[cfg(not(windows))]
pub async fn download(_cli: &Path, _id: &str, _path: &str) -> AppResult<Option<String>> {
    Err(AppError::new(
        "unsupported",
        "download",
        "WSLc доступен только в Windows",
    ))
}

/// Copies an existing Windows file to `target` inside the container.
pub async fn upload(cli: &Path, id: &str, local: &str, target: &str) -> AppResult<()> {
    validate_container_path(target)?;
    let local = crate::desktop::host_path(local)?;
    let data = tokio::task::spawn_blocking(move || -> AppResult<Vec<u8>> {
        let unreadable = || AppError::new("read_failed", "upload", "Не удалось прочитать файл");
        let size = std::fs::metadata(&local).map_err(|_| unreadable())?.len();
        if size > FILE_LIMIT as u64 {
            return Err(AppError::new("too_large", "upload", "Файл больше 64 МБ"));
        }
        std::fs::read(&local).map_err(|_| unreadable())
    })
    .await
    .map_err(|_| AppError::new("read_failed", "upload", "Не удалось прочитать файл"))??;
    write(cli, id, target, &data).await
}
