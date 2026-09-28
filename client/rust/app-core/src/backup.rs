//! Encrypted backups (`.ccbackup`, ADR-0106 / ADR-0102): ciphertext +
//! password/recovery envelopes + manifest MAC. No plaintext, no device keys.
//! Restorable on any machine with the passphrase or the Recovery Key into a
//! new Local profile.

use crate::app::AppCore;
use crate::dto::{ms, AppEvent, BackupSummaryDto, BackupUnlock, CreatedProfile};
use crate::error::{AppError, AppResult};
use crate::secrets::blocking;
use crate::session::{Session, VAULT_ID_SETTING};
use cc_storage_core::{local_key_envelope, ExportedObject, Profile, ProfileKind, VaultExport};
use cc_vault_core::{
    decode_backup, encode_backup, restore_backup_with_passphrase, restore_backup_with_recovery,
    BackupObject, SecretString, MAX_BACKUP_BYTES,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Write `bytes` to `path` atomically with owner-only permissions.
fn write_private(path: &PathBuf, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("ccbackup.tmp");
    {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        let mut f = o.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// Settings key of the backup schedule (profile database).
const SCHEDULE_SETTING: &str = "app_core.backup_schedule";
/// Settings key of the recent-backups list (profile database).
const RECENT_SETTING: &str = "app_core.recent_backups";
/// Recent backups remembered per profile.
const MAX_RECENT: usize = 50;

/// How often scheduled backups run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BackupFrequencyDto {
    #[default]
    Daily,
    Weekly,
}

impl BackupFrequencyDto {
    fn interval(self) -> chrono::Duration {
        match self {
            Self::Daily => chrono::Duration::days(1),
            Self::Weekly => chrono::Duration::days(7),
        }
    }
}

/// Scheduled auto-backup of the open profile (stored in its database; the
/// core runs due backups while the vault is unlocked, also with the window
/// hidden).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupScheduleDto {
    pub enabled: bool,
    pub folder: Option<String>,
    pub frequency: BackupFrequencyDto,
    /// Automatic backups kept (older ones are deleted).
    pub keep_last: u32,
    pub last_run_at_ms: Option<i64>,
    pub next_run_at_ms: Option<i64>,
    /// English diagnostic of the last failed run.
    pub last_error: Option<String>,
}

impl Default for BackupScheduleDto {
    fn default() -> Self {
        Self {
            enabled: false,
            folder: None,
            frequency: BackupFrequencyDto::Daily,
            keep_last: 7,
            last_run_at_ms: None,
            next_run_at_ms: None,
            last_error: None,
        }
    }
}

/// A backup file written on this device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupInfoDto {
    pub path: String,
    pub vault_id: String,
    pub created_at_ms: i64,
    pub objects: u64,
    pub size_bytes: u64,
    /// `ccbackup-v1` → 1.
    pub format_version: u32,
    pub app_version: String,
    /// Written by the scheduler / `backup_now`.
    pub automatic: bool,
}

fn format_version(format: &str) -> u32 {
    let digits: String = format
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse().unwrap_or(1)
}

/// `name-2026-09-27-14-03-11.ccbackup` (file-name safe).
fn backup_file_name(profile_name: &str, now: chrono::DateTime<chrono::Utc>) -> String {
    let mut name: String = profile_name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    if name.trim_matches(['_', '.']).is_empty() {
        name = "vault".into();
    }
    format!("{name}-{}.ccbackup", now.format("%Y-%m-%d-%H-%M-%S"))
}

impl AppCore {
    /// Export the open vault to an encrypted `.ccbackup` file (remembered
    /// in the recent-backups list).
    pub async fn export_backup(&self, path: String) -> AppResult<BackupSummaryDto> {
        let (summary, info) = self.write_backup(path, false).await?;
        self.remember_backup(info).await?;
        Ok(summary)
    }

    async fn write_backup(
        &self,
        path: String,
        automatic: bool,
    ) -> AppResult<(BackupSummaryDto, BackupInfoDto)> {
        let path = crate::validate::file_path("path", &path)?;
        let (s, u) = self.unlocked().await?;
        let vault_id = u.vault_id;
        let export = s.storage.read(move |tx| tx.export_vault(vault_id)).await?;
        let pw = export
            .password_envelope
            .ok_or_else(|| AppError::Storage("no password envelope stored".into()))?;
        let rec = export
            .recovery_envelope
            .ok_or_else(|| AppError::Storage("no recovery envelope stored".into()))?;
        let objects: Vec<BackupObject> = export
            .objects
            .into_iter()
            .map(|o| BackupObject {
                object_id: o.object_id,
                revision: o.revision,
                deleted: false,
                body: Some(o.body),
            })
            .collect();
        let count = objects.len() as u64;
        let vault = u.vault()?;
        let version = self.inner.ctx.config.client_version.clone();
        let created_at = chrono::Utc::now();
        let p2 = path.clone();
        let v2 = version.clone();
        let (format, size) = blocking(move || {
            let backup = vault.create_backup(&pw, &rec, objects, created_at, &v2)?;
            let format = backup.format.clone();
            let bytes = encode_backup(&backup)?;
            write_private(&p2, &bytes)?;
            Ok((format, bytes.len() as u64))
        })
        .await?;
        tracing::info!(vault_id = %vault_id, objects = count, automatic, "backup exported");
        let path_s = path.to_string_lossy().to_string();
        Ok((
            BackupSummaryDto {
                path: path_s.clone(),
                vault_id: vault_id.to_string(),
                objects: count,
                created_at_ms: ms(&created_at),
            },
            BackupInfoDto {
                path: path_s,
                vault_id: vault_id.to_string(),
                created_at_ms: ms(&created_at),
                objects: count,
                size_bytes: size,
                format_version: format_version(&format),
                app_version: version,
                automatic,
            },
        ))
    }

    // ---- schedule & recent backups ---------------------------------------------

    /// The open profile's backup schedule.
    pub async fn backup_schedule(&self) -> AppResult<BackupScheduleDto> {
        let s = self.session().await?;
        Ok(s.storage
            .setting_get::<BackupScheduleDto>(SCHEDULE_SETTING)
            .await?
            .unwrap_or_default())
    }

    async fn store_schedule(&self, s: &Session, sch: &BackupScheduleDto) -> AppResult<()> {
        s.storage.setting_set(SCHEDULE_SETTING, sch.clone()).await?;
        self.inner.ctx.emit(AppEvent::BackupScheduleChanged);
        Ok(())
    }

    /// Replace the schedule (`folder` required when enabled — reason
    /// `backup_folder_required`; `keep_last` ≥ 1 — reason
    /// `keep_at_least_one`). The next run is due one interval from now; the
    /// last run / error are kept.
    pub async fn set_backup_schedule(
        &self,
        schedule: BackupScheduleDto,
    ) -> AppResult<BackupScheduleDto> {
        let folder = schedule
            .folder
            .as_deref()
            .map(str::trim)
            .filter(|f| !f.is_empty())
            .map(|f| crate::validate::file_path("backup_folder", f))
            .transpose()?;
        if schedule.enabled && folder.is_none() {
            return Err(AppError::invalid("backup_folder", "required"));
        }
        if schedule.keep_last < 1 {
            return Err(AppError::invalid("keep_last", "must be at least 1"));
        }
        let s = self.session().await?;
        let old = self.backup_schedule().await?;
        let now = chrono::Utc::now();
        let sch = BackupScheduleDto {
            enabled: schedule.enabled,
            folder: folder.map(|f| f.to_string_lossy().into_owned()),
            frequency: schedule.frequency,
            keep_last: schedule.keep_last.min(1000),
            last_run_at_ms: old.last_run_at_ms,
            next_run_at_ms: schedule
                .enabled
                .then(|| ms(&(now + schedule.frequency.interval()))),
            last_error: old.last_error,
        };
        self.store_schedule(&s, &sch).await?;
        Ok(sch)
    }

    /// Backups of the open profile written on this device, newest first.
    pub async fn recent_backups(&self) -> AppResult<Vec<BackupInfoDto>> {
        let s = self.session().await?;
        Ok(s.storage
            .setting_get::<Vec<BackupInfoDto>>(RECENT_SETTING)
            .await?
            .unwrap_or_default())
    }

    /// Add backups to the recent list (newest first, one entry per path) —
    /// also used to import the list the UI kept before the core did.
    pub async fn remember_backups(&self, backups: Vec<BackupInfoDto>) -> AppResult<()> {
        let s = self.session().await?;
        let mut list = self.recent_backups().await?;
        for b in backups {
            crate::validate::file_path("path", &b.path)?;
            list.retain(|x| x.path != b.path);
            list.push(b);
        }
        list.sort_by_key(|b| std::cmp::Reverse(b.created_at_ms));
        list.truncate(MAX_RECENT);
        s.storage.setting_set(RECENT_SETTING, list).await?;
        self.inner.ctx.emit(AppEvent::BackupScheduleChanged);
        Ok(())
    }

    async fn remember_backup(&self, info: BackupInfoDto) -> AppResult<()> {
        self.remember_backups(vec![info]).await
    }

    /// Keep the newest `keep` automatic backups; delete the older files.
    async fn apply_retention(&self, keep: u32) -> AppResult<()> {
        let s = self.session().await?;
        let list = self.recent_backups().await?;
        let mut automatic: Vec<&BackupInfoDto> = list.iter().filter(|b| b.automatic).collect();
        automatic.sort_by_key(|b| std::cmp::Reverse(b.created_at_ms));
        let drop: Vec<String> = automatic
            .iter()
            .skip(keep as usize)
            .map(|b| b.path.clone())
            .collect();
        if drop.is_empty() {
            return Ok(());
        }
        let files = drop.clone();
        let _ = blocking(move || {
            for f in files.iter().filter(|f| f.ends_with(".ccbackup")) {
                if let Err(e) = std::fs::remove_file(f) {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        tracing::warn!(error = %e, "old automatic backup not deleted");
                    }
                }
            }
            Ok(())
        })
        .await;
        let kept: Vec<BackupInfoDto> = list
            .into_iter()
            .filter(|b| !drop.contains(&b.path))
            .collect();
        s.storage.setting_set(RECENT_SETTING, kept).await?;
        Ok(())
    }

    /// Run the scheduled backup now (to the schedule's folder), apply
    /// retention and move the schedule on. Failures are recorded in
    /// `last_error` and reported with [`AppEvent::BackupFailed`].
    pub async fn backup_now(&self) -> AppResult<BackupInfoDto> {
        let s = self.session().await?;
        let sch = self.backup_schedule().await?;
        let Some(folder) = sch.folder.clone().filter(|f| !f.trim().is_empty()) else {
            return Err(AppError::invalid("backup_folder", "required"));
        };
        let name = s
            .profile()
            .await
            .ok()
            .and_then(|p| p.display_name)
            .unwrap_or_else(|| "vault".into());
        let now = chrono::Utc::now();
        let path = PathBuf::from(&folder).join(backup_file_name(&name, now));
        let r = async {
            let (_, info) = self
                .write_backup(path.to_string_lossy().into_owned(), true)
                .await?;
            self.remember_backup(info.clone()).await?;
            self.apply_retention(sch.keep_last).await?;
            Ok::<_, AppError>(info)
        }
        .await;
        let mut next = self.backup_schedule().await?;
        match &r {
            Ok(info) => {
                next.last_run_at_ms = Some(ms(&now));
                next.last_error = None;
                next.next_run_at_ms = next.enabled.then(|| ms(&(now + next.frequency.interval())));
                self.store_schedule(&s, &next).await?;
                self.inner.ctx.emit(AppEvent::BackupCompleted {
                    backup: info.clone(),
                });
            }
            Err(e) => {
                tracing::warn!(code = e.code(), "backup failed");
                next.last_error = Some(e.message());
                self.store_schedule(&s, &next).await?;
                self.inner.ctx.emit(AppEvent::BackupFailed {
                    error: crate::transfers::ErrorInfoDto::from(e),
                });
            }
        }
        r
    }

    /// Run the scheduled backup if it is due and the vault is unlocked
    /// (called by the core's scheduler every 30 s; public for embedders
    /// and tests). `Some` when a backup was written.
    pub async fn run_due_backup(&self) -> AppResult<Option<BackupInfoDto>> {
        let Ok(s) = self.session().await else {
            return Ok(None);
        };
        if s.unlocked_opt().await.is_none() {
            return Ok(None);
        }
        let sch = self.backup_schedule().await?;
        let due = sch.enabled
            && sch
                .next_run_at_ms
                .is_some_and(|t| t <= ms(&chrono::Utc::now()));
        if !due {
            return Ok(None);
        }
        let _busy = match self.inner.backup_busy.try_lock() {
            Ok(g) => g,
            Err(_) => return Ok(None),
        };
        self.backup_now().await.map(Some)
    }

    /// Restore a `.ccbackup` into a **new Local profile** (becomes active
    /// and unlocked). With the Recovery Key, pass `new_passphrase` to
    /// replace the forgotten one.
    pub async fn import_backup(
        &self,
        path: String,
        display_name: String,
        unlock: BackupUnlock,
        new_passphrase: Option<String>,
    ) -> AppResult<CreatedProfile> {
        let path = crate::validate::file_path("path", &path)?;
        crate::validate::display_name("display_name", &display_name)?;
        if let Some(p) = &new_passphrase {
            crate::validate::secret_text("new_passphrase", p)?;
        }
        let restored = blocking(move || {
            let meta = std::fs::metadata(&path)?;
            if meta.len() as usize > MAX_BACKUP_BYTES {
                return Err(AppError::Backup("file too large".into()));
            }
            let bytes = std::fs::read(&path)?;
            let backup = decode_backup(&bytes)?;
            Ok(match &unlock {
                BackupUnlock::Passphrase(p) => {
                    restore_backup_with_passphrase(backup, &SecretString::from(p.clone()))?
                }
                BackupUnlock::RecoveryKey(k) => {
                    restore_backup_with_recovery(backup, &SecretString::from(k.clone()))?
                }
            })
        })
        .await?;
        let vault_id = restored.backup.vault_id;
        let (pid, storage, identity) = self
            .new_profile_storage(&display_name, ProfileKind::Local)
            .await?;
        let r = async {
            storage
                .put_profile(Profile::new_local(
                    pid,
                    identity.device_id(),
                    Some(display_name.trim().to_owned()),
                ))
                .await?;
            let classes: HashMap<_, _> = restored.classes.iter().copied().collect();
            let did = Some(identity.device_id());
            let export = VaultExport {
                vault_id,
                password_envelope: Some(local_key_envelope(
                    vault_id,
                    restored.backup.password_envelope.clone(),
                    did,
                )),
                recovery_envelope: Some(local_key_envelope(
                    vault_id,
                    restored.backup.recovery_envelope.clone(),
                    did,
                )),
                objects: restored
                    .backup
                    .objects
                    .iter()
                    .filter(|o| !o.deleted)
                    .filter_map(|o| {
                        o.body.clone().map(|body| ExportedObject {
                            object_id: o.object_id,
                            revision: o.revision,
                            body,
                            kek_class_hint: classes.get(&o.object_id).copied(),
                        })
                    })
                    .collect(),
                exported_at: restored.backup.created_at,
            };
            let device_env = local_key_envelope(
                vault_id,
                restored.unlocked.device_envelope_for(&identity)?,
                did,
            );
            storage
                .write(move |tx| {
                    tx.import_vault(&export)?;
                    tx.cache_vault_envelopes(vault_id, None, None, Some(&device_env))?;
                    tx.setting_set(VAULT_ID_SETTING, &vault_id)
                })
                .await?;
            let session =
                Session::with_storage(self.inner.ctx.clone(), pid, storage.clone()).await?;
            session.install_unlocked(restored.unlocked).await?;
            Ok::<_, AppError>(session)
        }
        .await;
        let session = match r {
            Ok(s) => s,
            Err(e) => {
                self.discard_profile(pid, Some(storage)).await;
                return Err(e);
            }
        };
        self.activate(session.clone()).await?;
        if let Some(p) = new_passphrase {
            self.set_new_passphrase(p).await?;
        }
        tracing::info!(profile_id = %pid, vault_id = %vault_id, "backup restored");
        let profile = self
            .active_profile()
            .await?
            .ok_or(AppError::NoActiveProfile)?;
        Ok(CreatedProfile {
            profile,
            recovery_kit: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_and_versions() {
        let t = chrono::DateTime::from_timestamp(1_790_000_000, 0).unwrap();
        let n = backup_file_name("My Vault/../x", t);
        assert!(
            n.starts_with("My_Vault_.._x-") && n.ends_with(".ccbackup"),
            "{n}"
        );
        assert!(!n.contains('/'));
        assert!(backup_file_name("///", t).starts_with("vault-"));
        assert_eq!(format_version("ccbackup-v1"), 1);
        assert_eq!(format_version("ccbackup-v12"), 12);
        assert_eq!(format_version("weird"), 1);
    }
}
