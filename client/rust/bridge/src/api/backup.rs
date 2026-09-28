//! Encrypted `.ccbackup` export / inspect / import (ADR-0106).

use crate::api::error::BridgeError;
use crate::state::{from_json, opt_secret_string, run, secret_string, to_json, with_core};
use cc_app_core::{AppError, BackupInfoDto, BackupScheduleDto, BackupUnlock};

/// How a backup being restored is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupUnlockKind {
    Passphrase,
    /// 24 words or the recovery QR text (a new passphrase must be set).
    RecoveryKey,
}

/// Header facts of a backup file (nothing is decrypted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupHeader {
    pub path: String,
    pub vault_id: String,
    pub created_at_ms: i64,
    pub objects: u64,
    pub size_bytes: u64,
    /// e.g. `ccbackup-v1`.
    pub format: String,
    pub app_version: String,
}

/// Write the open vault to `path` → `BackupSummaryDto`.
pub async fn backup_export(path: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.export_backup(path).await?) }).await
}

/// Read a backup's header without decrypting anything.
pub async fn backup_inspect(path: String) -> Result<BackupHeader, BridgeError> {
    run(async move {
        let p = path.clone();
        tokio::task::spawn_blocking(move || inspect_file(&p))
            .await
            .map_err(|e| BridgeError::internal(e.to_string()))?
    })
    .await
}

pub(crate) fn inspect_file(path: &str) -> Result<BackupHeader, BridgeError> {
    let meta = std::fs::metadata(path).map_err(|e| AppError::Io(e.to_string()))?;
    if meta.len() as usize > cc_vault_core::MAX_BACKUP_BYTES {
        return Err(AppError::Backup("file too large".into()).into());
    }
    let bytes = std::fs::read(path).map_err(|e| AppError::Io(e.to_string()))?;
    let b = cc_vault_core::decode_backup(&bytes)
        .map_err(|_| AppError::Backup("not a ConsoleCrypt backup".into()))?;
    Ok(BackupHeader {
        path: path.to_owned(),
        vault_id: b.vault_id.to_string(),
        created_at_ms: b.created_at.timestamp_millis(),
        objects: b.objects.iter().filter(|o| !o.deleted).count() as u64,
        size_bytes: meta.len(),
        format: b.format.clone(),
        app_version: b.app_version.clone(),
    })
}

/// The open profile's backup schedule → `BackupScheduleDto` (stored in the
/// profile; the core runs due backups while unlocked, also with the window
/// hidden).
pub async fn backup_schedule_get() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.backup_schedule().await?) }).await
}

/// Replace the schedule (`BackupScheduleDto` JSON) → the stored schedule
/// (next run computed by the core).
pub async fn backup_schedule_set(schedule_json: String) -> Result<String, BridgeError> {
    let s: BackupScheduleDto = from_json("backup_schedule", &schedule_json)?;
    with_core(move |c| async move { to_json(&c.set_backup_schedule(s).await?) }).await
}

/// Run the scheduled backup now (retention applied) → `BackupInfoDto`.
pub async fn backup_now() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.backup_now().await?) }).await
}

/// Backups written on this device for the open profile →
/// `Vec<BackupInfoDto>` (newest first).
pub async fn backup_recent() -> Result<String, BridgeError> {
    with_core(move |c| async move {
        match c.recent_backups().await {
            Ok(v) => to_json(&v),
            Err(AppError::NoActiveProfile) => Ok("[]".to_owned()),
            Err(e) => Err(e.into()),
        }
    })
    .await
}

/// Add entries (`Vec<BackupInfoDto>` JSON) to the recent list (one-time
/// import of the list the UI kept before the core did).
pub async fn backup_remember(backups_json: String) -> Result<(), BridgeError> {
    let list: Vec<BackupInfoDto> = from_json("backups", &backups_json)?;
    with_core(move |c| async move { Ok(c.remember_backups(list).await?) }).await
}

/// Restore into a **new Local profile** (active, unlocked) →
/// `CreatedProfile`.
pub async fn backup_import(
    path: String,
    display_name: String,
    unlock_kind: BackupUnlockKind,
    secret: Vec<u8>,
    new_passphrase: Option<Vec<u8>>,
) -> Result<String, BridgeError> {
    let secret = secret_string("secret", secret)?;
    let new_passphrase = opt_secret_string("new_passphrase", new_passphrase)?;
    let unlock = match unlock_kind {
        BackupUnlockKind::Passphrase => BackupUnlock::Passphrase(secret),
        BackupUnlockKind::RecoveryKey => BackupUnlock::RecoveryKey(secret),
    };
    with_core(move |c| async move {
        to_json(
            &c.import_backup(path, display_name, unlock, new_passphrase)
                .await?,
        )
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspect_rejects_non_backups() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.ccbackup");
        std::fs::write(&p, b"{\"hello\":1}").unwrap();
        let e = inspect_file(p.to_str().unwrap()).unwrap_err();
        assert_eq!(e.code, "backup");
        let e = inspect_file(dir.path().join("missing").to_str().unwrap()).unwrap_err();
        assert_eq!(e.code, "io");
    }
}
