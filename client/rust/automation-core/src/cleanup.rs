//! Only built-in managed UUID inspection/deletion over a live private pipe.
//! Native owns approval, profile references, cancellation and route selection.
use crate::{AutomationError, Result, SetupTarget};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeSet, process::Stdio, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedCleanupIdentity {
    pub uid: u32,
    pub user: String,
    pub home: String,
    pub parent: String,
    pub device: u64,
    pub inode: u64,
    pub mode: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedCleanupPaths {
    pub python: String,
    pub module_dir: String,
    pub root: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedCleanupCandidate {
    pub id: String,
    pub environment_id: String,
    pub path: String,
    pub quarantined: bool,
    pub eligible: bool,
    pub blocked_reason: Option<String>,
    pub paths: Option<ManagedCleanupPaths>,
    pub revision: Option<String>,
    pub receipt_sha256: Option<String>,
    pub device: Option<u64>,
    pub inode: Option<u64>,
    pub nodes: Option<u64>,
    pub bytes: Option<u64>,
    pub profile_ownership: Option<String>,
    pub contains_state: Option<bool>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedCleanupScan {
    pub version: u32,
    pub identity: Option<ManagedCleanupIdentity>,
    pub candidates: Vec<ManagedCleanupCandidate>,
    pub other_profiles_checked: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedCleanupApproval {
    pub id: String,
    pub revision: String,
}
/// Deny-only metadata copied from Native's saved current-profile Runner catalog.
/// Never accepted from cleanup UI JSON as caller authority or deletion paths.
#[derive(Clone, Debug)]
pub struct ManagedCleanupProtection {
    pub paths: Vec<String>,
    pub selected_python: Option<String>,
}
impl ManagedCleanupProtection {
    pub fn validate(&self) -> Result<()> {
        if self.paths.len() > 96
            || self.paths.iter().map(|path| path.len()).sum::<usize>() > 16384
            || self
                .paths
                .iter()
                .chain(self.selected_python.iter())
                .any(|path| {
                    !path.starts_with('/')
                        || path.len() > 4096
                        || path.chars().any(char::is_control)
                })
        {
            return Err(invalid());
        }
        Ok(())
    }
    fn scan_payload(&self) -> Result<Value> {
        self.validate()?;
        Ok(
            json!({"action":"cleanup_scan","protected_paths":self.paths,"selected_python":self.selected_python}),
        )
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedCleanupRemoved {
    pub id: String,
    pub removed: bool,
    pub nodes: u64,
    pub bytes: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedCleanupResult {
    pub version: u32,
    pub removed: Vec<ManagedCleanupRemoved>,
    pub other_profiles_checked: bool,
}

fn invalid() -> AutomationError {
    AutomationError::Remote("cleanup_invalid_request".into())
}
fn bootstrap_error(error: AutomationError) -> AutomationError {
    match error {
        AutomationError::Remote(code) if code == "setup_bootstrap_failed" => {
            AutomationError::Remote("cleanup_bootstrap_failed".into())
        }
        AutomationError::Remote(code) if code == "setup_command_too_large" => {
            AutomationError::Remote("cleanup_command_too_large".into())
        }
        other => other,
    }
}
fn uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value)
        .is_ok_and(|id| !id.is_nil() && id.hyphenated().to_string() == value)
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn unix(value: &str) -> bool {
    value.starts_with('/')
        && !value.starts_with("//")
        && value.len() <= 4096
        && !value.chars().any(char::is_control)
        && value[1..]
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}
impl ManagedCleanupScan {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 || self.other_profiles_checked || self.candidates.len() > 128 {
            return Err(invalid());
        }
        let Some(owner) = &self.identity else {
            return if self.candidates.is_empty() {
                Ok(())
            } else {
                Err(invalid())
            };
        };
        if owner.uid == 0
            || owner.user.is_empty()
            || owner.user.len() > 128
            || owner.user.chars().any(char::is_control)
            || !unix(&owner.home)
            || owner.home.len() > 3500
            || owner.parent
                != format!(
                    "{}/.local/share/consolecrypt/runner-environments",
                    owner.home
                )
            || owner.device == 0
            || owner.inode == 0
            || owner.mode != 0o700
        {
            return Err(invalid());
        }
        let mut ids = BTreeSet::new();
        for row in &self.candidates {
            if !uuid(&row.id) || !uuid(&row.environment_id) || !ids.insert(&row.id) {
                return Err(invalid());
            }
            let name = if row.quarantined {
                format!(".cleanup-{}-{}", row.environment_id, row.id)
            } else {
                if row.id != row.environment_id {
                    return Err(invalid());
                }
                row.id.clone()
            };
            if row.path != format!("{}/{name}", owner.parent) {
                return Err(invalid());
            }
            if row.eligible {
                let original = format!("{}/{}", owner.parent, row.environment_id);
                let expected = ManagedCleanupPaths {
                    python: format!("{original}/venv/bin/python"),
                    module_dir: format!("{original}/src"),
                    root: format!("{original}/state"),
                };
                if row.paths.as_ref() != Some(&expected)
                    || !row.revision.as_deref().is_some_and(digest)
                    || !row.receipt_sha256.as_deref().is_some_and(digest)
                    || row.device != Some(owner.device)
                    || row.inode.is_none_or(|n| n == 0)
                    || row.nodes.is_none_or(|n| n == 0 || n > 100000)
                    || row.bytes.is_none_or(|n| n > 2 * 1024 * 1024 * 1024)
                    || row.profile_ownership.as_deref() != Some("unknown")
                    || row.contains_state != Some(true)
                    || row.blocked_reason.is_some()
                {
                    return Err(invalid());
                }
            } else if !row.blocked_reason.as_deref().is_some_and(|v| {
                v.starts_with("cleanup_")
                    && v.len() <= 64
                    && v.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
            }) {
                return Err(invalid());
            }
        }
        Ok(())
    }
}

fn payload(
    identity: &ManagedCleanupIdentity,
    candidates: &[ManagedCleanupApproval],
    protection: &ManagedCleanupProtection,
) -> Result<Value> {
    protection.validate()?;
    if candidates.is_empty() || candidates.len() > 8 {
        return Err(invalid());
    }
    let mut ids = BTreeSet::new();
    for candidate in candidates {
        if !uuid(&candidate.id) || !digest(&candidate.revision) || !ids.insert(&candidate.id) {
            return Err(invalid());
        }
    }
    Ok(
        json!({"action":"cleanup_delete","identity":identity,"candidates":candidates,"protected_paths":protection.paths,"selected_python":protection.selected_python}),
    )
}
fn encode(value: Value) -> Result<Vec<u8>> {
    let mut raw = serde_json::to_vec(&value).map_err(|_| invalid())?;
    if raw.len() > 32 * 1024 {
        return Err(invalid());
    }
    raw.push(b'\n');
    Ok(raw)
}
fn response(output: &[u8], success: bool) -> Result<Value> {
    if output.len() > 65536 {
        return Err(AutomationError::Remote("cleanup_budget".into()));
    }
    let value = crate::json::parse(output)?;
    if !success || value.get("ok") != Some(&Value::Bool(true)) {
        let code = value
            .get("error")
            .and_then(Value::as_str)
            .filter(|v| {
                v.starts_with("cleanup_")
                    && v.len() <= 64
                    && v.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
            })
            .or_else(|| {
                (value.get("error").and_then(Value::as_str) == Some("setup_bootstrap_failed"))
                    .then_some("cleanup_bootstrap_failed")
            })
            .unwrap_or("cleanup_failed");
        return Err(AutomationError::Remote(code.into()));
    }
    value.get("result").cloned().ok_or_else(invalid)
}
async fn local(target: &SetupTarget, value: Value) -> Result<Value> {
    match target {
        SetupTarget::Native if cfg!(target_os = "linux") => {}
        SetupTarget::Wsl {
            user: Some(user), ..
        } if cfg!(target_os = "windows") && user != "root" => {}
        _ => return Err(AutomationError::Remote("cleanup_unsupported".into())),
    }
    let raw = encode(value)?;
    let mut command = tokio::process::Command::from(target.command().map_err(bootstrap_error)?);
    command
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn()?;
    let mut stdin = child.stdin.take().ok_or(AutomationError::Closed)?;
    let mut stdout = child.stdout.take().ok_or(AutomationError::Closed)?;
    tokio::time::timeout(Duration::from_secs(25), async {
        stdin.write_all(&raw).await?;
        stdin.flush().await?;
        let mut output = Vec::new();
        (&mut stdout).take(65537).read_to_end(&mut output).await?;
        let status = child.wait().await?;
        // Keep the writer live until Python exits; EOF is cancellation, never submit.
        drop(stdin);
        response(&output, status.success())
    })
    .await
    .map_err(|_| AutomationError::Timeout)?
}
async fn remote(session: &cc_ssh_core::SshSession, value: Value) -> Result<Value> {
    let raw = encode(value)?;
    let command=format!("for cc_python in /usr/bin/python3.12 /usr/bin/python3.14 /usr/bin/python3; do if [ -x \"$cc_python\" ]; then exec \"$cc_python\" -I -c '{}'; fi; done; exit 69",crate::setup::builtin_setup_script().map_err(bootstrap_error)?.replace('\'',"'\"'\"'"));
    let output = tokio::time::timeout(
        Duration::from_secs(25),
        session.exec_with_private_input(&command, &raw, 65536),
    )
    .await
    .map_err(|_| AutomationError::Timeout)?
    .map_err(|_| AutomationError::Remote("cleanup_failed".into()))?;
    response(
        &output.stdout,
        output.exit_status == Some(0) && output.exit_signal.is_none(),
    )
}
fn scan(value: Value) -> Result<ManagedCleanupScan> {
    let result: ManagedCleanupScan = serde_json::from_value(value).map_err(|_| invalid())?;
    result.validate()?;
    Ok(result)
}
fn removed(value: Value, approved: &[ManagedCleanupApproval]) -> Result<ManagedCleanupResult> {
    let result: ManagedCleanupResult = serde_json::from_value(value).map_err(|_| invalid())?;
    let ids = approved
        .iter()
        .map(|c| c.id.as_str())
        .collect::<BTreeSet<_>>();
    let actual = result
        .removed
        .iter()
        .map(|c| c.id.as_str())
        .collect::<BTreeSet<_>>();
    if result.version != 1
        || result.other_profiles_checked
        || ids != actual
        || result.removed.len() != approved.len()
        || result
            .removed
            .iter()
            .any(|row| !row.removed || row.nodes > 100000 || row.bytes > 2 * 1024 * 1024 * 1024)
    {
        return Err(invalid());
    }
    Ok(result)
}
pub async fn cleanup_scan_environment(
    target: &SetupTarget,
    protection: &ManagedCleanupProtection,
) -> Result<ManagedCleanupScan> {
    scan(local(target, protection.scan_payload()?).await?)
}
pub async fn cleanup_scan_remote_environment(
    session: &cc_ssh_core::SshSession,
    protection: &ManagedCleanupProtection,
) -> Result<ManagedCleanupScan> {
    scan(remote(session, protection.scan_payload()?).await?)
}
pub async fn cleanup_delete_environment(
    target: &SetupTarget,
    identity: &ManagedCleanupIdentity,
    approved: &[ManagedCleanupApproval],
    protection: &ManagedCleanupProtection,
) -> Result<ManagedCleanupResult> {
    removed(
        local(target, payload(identity, approved, protection)?).await?,
        approved,
    )
}
pub async fn cleanup_delete_remote_environment(
    session: &cc_ssh_core::SshSession,
    identity: &ManagedCleanupIdentity,
    approved: &[ManagedCleanupApproval],
    protection: &ManagedCleanupProtection,
) -> Result<ManagedCleanupResult> {
    removed(
        remote(session, payload(identity, approved, protection)?).await?,
        approved,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bootstrap_failure_and_command_ceiling_have_specific_safe_cleanup_codes() {
        let error =
            response(br#"{"ok":false,"error":"setup_bootstrap_failed"}"#, false).unwrap_err();
        assert!(matches!(error,AutomationError::Remote(code) if code=="cleanup_bootstrap_failed"));
        let error = bootstrap_error(AutomationError::Remote("setup_command_too_large".into()));
        assert!(matches!(error,AutomationError::Remote(code) if code=="cleanup_command_too_large"));
    }
    fn scan_fixture() -> ManagedCleanupScan {
        let id = "11111111-1111-4111-8111-111111111111";
        let parent = "/home/fixture/.local/share/consolecrypt/runner-environments";
        serde_json::from_value(json!({"version":1,"identity":{"uid":10001,"user":"fixture","home":"/home/fixture","parent":parent,"device":1,"inode":1,"mode":448},"candidates":[{
            "id":id,"environment_id":id,"path":format!("{parent}/{id}"),"paths":{"python":format!("{parent}/{id}/venv/bin/python"),"module_dir":format!("{parent}/{id}/src"),"root":format!("{parent}/{id}/state")},
            "revision":"a".repeat(64),"receipt_sha256":"b".repeat(64),"device":1,"inode":2,"nodes":10,"bytes":100,"quarantined":false,"eligible":true,"profile_ownership":"unknown","contains_state":true
        }],"other_profiles_checked":false})).unwrap()
    }
    #[test]
    fn helper_report_cannot_smuggle_paths_ownership_or_weaker_schema() {
        scan_fixture().validate().unwrap();
        for change in ["path", "python", "owner", "version", "count", "bytes"] {
            let mut value = scan_fixture();
            match change {
                "path" => value.candidates[0].path = "/unapproved".into(),
                "python" => {
                    value.candidates[0].paths.as_mut().unwrap().python = "/usr/bin/python3".into()
                }
                "owner" => value.other_profiles_checked = true,
                "version" => value.version = 2,
                "count" => value.candidates[0].nodes = Some(100001),
                "bytes" => value.candidates[0].bytes = Some(2 * 1024 * 1024 * 1024 + 1),
                _ => unreachable!(),
            }
            assert!(value.validate().is_err(), "{change}");
        }
    }
    #[test]
    fn deny_only_path_protection_and_batch_frames_are_bounded() {
        let scan = scan_fixture();
        let owner = scan.identity.unwrap();
        let approved = vec![ManagedCleanupApproval {
            id: scan.candidates[0].id.clone(),
            revision: "a".repeat(64),
        }];
        let protection = ManagedCleanupProtection {
            paths: vec!["/usr/bin/python3".into()],
            selected_python: None,
        };
        let raw = encode(payload(&owner, &approved, &protection).unwrap()).unwrap();
        assert!(raw.ends_with(b"\n"));
        assert!(raw.len() < 32 * 1024);
        let too_many = ManagedCleanupProtection {
            paths: vec!["/usr/bin/python3".into(); 97],
            selected_python: None,
        };
        assert!(too_many.validate().is_err());
        let too_large = ManagedCleanupProtection {
            paths: vec![format!("/{}", "x".repeat(4095)); 5],
            selected_python: None,
        };
        assert!(too_large.validate().is_err());
        let relative = ManagedCleanupProtection {
            paths: vec!["../arbitrary".into()],
            selected_python: None,
        };
        assert!(relative.validate().is_err());
        assert!(payload(
            &owner,
            &[approved[0].clone(), approved[0].clone()],
            &protection
        )
        .is_err());
        assert!(payload(&owner, &[], &protection).is_err());
    }
    #[test]
    fn unknown_or_echoed_cleanup_error_text_never_becomes_a_remote_diagnostic() {
        let value =
            json!({"ok":false,"error":"cleanup_secret path /home/user/private"}).to_string();
        assert!(
            matches!(response(value.as_bytes(),false),Err(AutomationError::Remote(code)) if code=="cleanup_failed")
        );
        assert!(response(&vec![b' '; 65537], true).is_err());
        let scan = scan_fixture();
        let approved = [ManagedCleanupApproval {
            id: scan.candidates[0].id.clone(),
            revision: "a".repeat(64),
        }];
        let result = json!({"version":1,"removed":[{"id":"22222222-2222-4222-8222-222222222222","removed":true,"nodes":1,"bytes":1}],"other_profiles_checked":false});
        assert!(removed(result, &approved).is_err());
    }
}
