//! Temporary CLI capability files contain a revocable loopback token, never
//! the selected cluster's identity. Native consumers own the lease.
use crate::{AppCore, AppError, AppResult};
use cc_k8s_core::local_proxy::LocalApiProxy;
use std::{
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::OwnedSemaphorePermit;
use tokio_util::sync::CancellationToken;

pub struct CliProxyLease {
    proxy: LocalApiProxy,
    path: PathBuf,
    directory: Mutex<Option<tempfile::TempDir>>,
    _slot: OwnedSemaphorePermit,
}
impl std::fmt::Debug for CliProxyLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CliProxyLease").finish_non_exhaustive()
    }
}
impl CliProxyLease {
    pub fn path(&self) -> PathBuf {
        self.path.clone()
    }
    pub fn cancellation(&self) -> CancellationToken {
        self.proxy.cancellation()
    }
    pub fn close(&self) {
        self.proxy.close();
        // Taking the directory unlinks the capability before a CLI lease can
        // be reused after lock. The token has already been revoked.
        self.directory
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
    }
}
impl Drop for CliProxyLease {
    fn drop(&mut self) {
        self.close();
    }
}

impl AppCore {
    pub async fn kube_cli_proxy(&self, session_id: String) -> AppResult<Arc<CliProxyLease>> {
        self.kube_cli_proxy_namespace(session_id, None).await
    }

    pub async fn kube_cli_proxy_in_namespace(
        &self,
        session_id: String,
        namespace: String,
    ) -> AppResult<Arc<CliProxyLease>> {
        cc_models::kubernetes::validate_namespace(&namespace)?;
        self.kube_cli_proxy_namespace(session_id, Some(namespace))
            .await
    }

    async fn kube_cli_proxy_namespace(
        &self,
        session_id: String,
        namespace: Option<String>,
    ) -> AppResult<Arc<CliProxyLease>> {
        if cfg!(any(target_os = "android", target_os = "ios")) {
            return Err(AppError::invalid("kubernetes", "desktop_only"));
        }
        let (owner, unlocked) = self.unlocked().await?;
        let live = unlocked.kube.get(&session_id)?;
        let slot = live
            .cli_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| AppError::invalid("kubernetes", "too_many_cli_sessions"))?;
        let proxy = LocalApiProxy::start(&live.session)
            .await
            .map_err(|_| AppError::invalid("kubernetes", "proxy_unavailable"))?;
        let parent = self
            .inner
            .dirs
            .profile(&owner.profile_id.to_string())
            .map_err(|_| AppError::invalid("kubernetes", "proxy_unavailable"))?
            .cache_dir();
        let directory = tempfile::Builder::new()
            .prefix("kubernetes-cli-")
            .tempdir_in(parent)
            .map_err(|_| AppError::invalid("kubernetes", "proxy_unavailable"))?;
        restrict_directory(directory.path()).await?;
        let path = directory.path().join("config.json");
        let bytes = proxy
            .kubeconfig(namespace.as_deref().unwrap_or(&live.info.namespace))
            .map_err(|_| AppError::VaultLocked)?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&path)
            .map_err(|_| AppError::invalid("kubernetes", "proxy_unavailable"))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| AppError::invalid("kubernetes", "proxy_unavailable"))?;
        drop(file);
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        if live.cancel.is_cancelled() {
            return Err(AppError::VaultLocked);
        }
        let lease = Arc::new(CliProxyLease {
            proxy,
            path,
            directory: Mutex::new(Some(directory)),
            _slot: slot,
        });
        let cancelled = lease.cancellation();
        let weak = Arc::downgrade(&lease);
        tokio::spawn(async move {
            cancelled.cancelled().await;
            if let Some(lease) = weak.upgrade() {
                lease.close();
            }
        });
        Ok(lease)
    }
}

#[cfg(not(windows))]
pub(crate) async fn restrict_directory(path: &std::path::Path) -> AppResult<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| AppError::invalid("kubernetes", "proxy_permissions"))?;
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) async fn restrict_directory(path: &std::path::Path) -> AppResult<()> {
    use std::{process::Stdio, time::Duration};
    let fail = || AppError::invalid("kubernetes", "proxy_permissions");
    let system = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .ok_or_else(fail)?
        .join("System32");
    // Resolve the SID, not a localized account name. Both programs are the OS
    // utilities at absolute paths and are invoked directly, without a shell.
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new(system.join("whoami.exe"))
            .args(["/user", "/fo", "csv", "/nh"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| fail())?
    .map_err(|_| fail())?;
    if !output.status.success() || output.stdout.len() > 16384 {
        return Err(fail());
    }
    let sid = whoami_sid(&output.stdout).ok_or_else(fail)?;
    let status = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new(system.join("icacls.exe"))
            .arg(path)
            .args(["/inheritance:r", "/grant:r", &format!("*{sid}:(OI)(CI)F")])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .status(),
    )
    .await
    .map_err(|_| fail())?
    .map_err(|_| fail())?;
    if !status.success() {
        return Err(fail());
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn whoami_sid(output: &[u8]) -> Option<String> {
    // The account name uses the Windows output code page; the SID field is
    // ASCII regardless of that code page. Never decode the whole CSV as UTF-8.
    if output.len() > 16384 {
        return None;
    }
    let output = output.trim_ascii();
    let mut fields = output.strip_suffix(b"\"")?.rsplitn(2, |byte| *byte == b'"');
    let sid = fields.next()?;
    let prefix = fields.next()?;
    if !prefix.starts_with(b"\"") || !prefix.ends_with(b",") || !sid.starts_with(b"S-1-") {
        return None;
    }
    if sid.len() > 255
        || !sid
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'S' | b'-'))
    {
        return None;
    }
    let text = std::str::from_utf8(sid).ok()?;
    let parts = text.split('-').collect::<Vec<_>>();
    if !(4..=18).contains(&parts.len())
        || parts[0] != "S"
        || parts[1] != "1"
        || parts[2].parse::<u64>().ok()? > 0xffff_ffff_ffff
        || parts[3..]
            .iter()
            .any(|part| part.is_empty() || part.parse::<u32>().is_err())
    {
        return None;
    }
    Some(text.to_owned())
}

#[cfg(test)]
mod sid_tests {
    use super::whoami_sid;
    #[test]
    fn windows_oem_account_name_does_not_break_ascii_sid_extraction() {
        let csv = b"\"HOST\\\x80\xaf\xa0\xad\",\"S-1-5-21-123-456-789-1001\"\r\n";
        assert!(csv.iter().any(|byte| !byte.is_ascii()));
        assert_eq!(
            whoami_sid(csv).as_deref(),
            Some("S-1-5-21-123-456-789-1001")
        );
        assert_eq!(
            whoami_sid(b"\"S-1-5-19\",\"S-1-5-21-123-1001\"").as_deref(),
            Some("S-1-5-21-123-1001")
        );
        assert!(whoami_sid(b"\"name\",\"S-1-5--1001\"").is_none());
        assert!(whoami_sid(b"\"name\",\"S-1-5-1001:(OI)F\"").is_none());
        assert!(whoami_sid(b"\"name\",\"S-1-5-99999999999\"").is_none());
        assert!(whoami_sid(b"\"S-1-5-19\",\"invalid\"").is_none());
    }
}
