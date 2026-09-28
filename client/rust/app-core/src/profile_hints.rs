//! Non-secret account hints of synced profiles
//! (`<data>/profiles/<id>/account-hint.json`, 0600): server URL and account
//! e-mail, so closed profiles can be labelled in the profile switcher
//! without opening their (encrypted) database. Written whenever the open
//! profile is listed and differs; removed for local profiles.
//!
//! Trade-off (documented in ADR-0107): the e-mail is PII in a plaintext
//! file next to the (plaintext) profile index; it is not a secret and grants
//! nothing without the account password and the device key.

use cc_platform_core::AppDirs;
use cc_storage_core::{Profile, ProfileId, ProfileKind};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const FILE: &str = "account-hint.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AccountHint {
    pub server_url: String,
    #[serde(default)]
    pub email: Option<String>,
}

fn path(dirs: &AppDirs, pid: ProfileId) -> Option<PathBuf> {
    dirs.profile(&pid.to_string())
        .ok()
        .map(|p| p.dir().join(FILE))
}

fn read_blocking(p: &PathBuf) -> Option<AccountHint> {
    let bytes = std::fs::read(p).ok()?;
    if bytes.len() > 16 * 1024 {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

fn write_blocking(p: &PathBuf, hint: &AccountHint) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = p.with_extension("json.tmp");
    {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        let mut f = o.open(&tmp)?;
        f.write_all(&serde_json::to_vec(hint).map_err(std::io::Error::other)?)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, p)
}

/// Hint of a (closed) profile.
pub(crate) async fn read(dirs: &AppDirs, pid: ProfileId) -> Option<AccountHint> {
    let p = path(dirs, pid)?;
    tokio::task::spawn_blocking(move || read_blocking(&p))
        .await
        .ok()
        .flatten()
}

/// Bring the hint in line with the open profile's row (best effort).
pub(crate) async fn sync(dirs: &AppDirs, pid: ProfileId, profile: &Profile) {
    let Some(p) = path(dirs, pid) else { return };
    let want = match (&profile.kind, &profile.server_url) {
        (ProfileKind::Synced, Some(url)) => Some(AccountHint {
            server_url: url.clone(),
            email: profile.email.clone(),
        }),
        _ => None,
    };
    let _ = tokio::task::spawn_blocking(move || {
        let have = read_blocking(&p);
        match want {
            Some(h) if have.as_ref() != Some(&h) => {
                if let Err(e) = write_blocking(&p, &h) {
                    tracing::debug!(error = %e, "account hint not written");
                }
            }
            None if p.exists() => {
                let _ = std::fs::remove_file(&p);
            }
            _ => {}
        }
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hints_follow_the_profile_kind() {
        let dir = tempfile::tempdir().unwrap();
        let dirs = AppDirs::with_root(dir.path());
        let pid = ProfileId::new();
        dirs.ensure_profile(&pid.to_string()).unwrap();
        assert!(read(&dirs, pid).await.is_none());
        let synced = Profile::new_synced(
            pid,
            cc_protocol::DeviceId::new(),
            "https://sync.example",
            None,
            Some("a@example.com".into()),
        );
        sync(&dirs, pid, &synced).await;
        let h = read(&dirs, pid).await.unwrap();
        assert_eq!(h.server_url, "https://sync.example");
        assert_eq!(h.email.as_deref(), Some("a@example.com"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let p = path(&dirs, pid).unwrap();
            assert_eq!(
                std::fs::metadata(p).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let local = Profile::new_local(pid, cc_protocol::DeviceId::new(), None);
        sync(&dirs, pid, &local).await;
        assert!(read(&dirs, pid).await.is_none());
    }
}
