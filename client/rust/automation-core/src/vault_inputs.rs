//! Reference-only Ansible Vault inputs. Reading a project is never credential
//! authority: the caller must supply the independently selected Native UUID.
use crate::{AutomationError, ProjectEntry, ProjectRevision, ProjectRoot, Result, VerifiedJob};
use cc_models::secret::SecretValue;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    io::{Read, Write},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub const VAULT_INPUT_FILE: &str = ".consolecrypt-secret-inputs.json";
const MAX_REF_BYTES: usize = 4096;
const MAX_PASSWORD_BYTES: usize = 4096;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    ansible_vault: Reference,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    credential_id: String,
}

pub(crate) fn password_line_supported(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_PASSWORD_BYTES
        && value.trim() == value
        && !value
            .chars()
            .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
}

/// Non-deserializable evidence from the existing capability-relative project
/// reader and complete revision walker. Contains identifiers/hashes only.
#[derive(Debug)]
pub struct VaultInputBinding {
    credential: Uuid,
    revision: String,
    reference_sha256: String,
}
impl VaultInputBinding {
    pub fn is_ephemeral(&self) -> bool { self.credential.is_nil() }
    pub fn credential_id(&self) -> Uuid {
        self.credential
    }
    pub fn revision_digest(&self) -> &str {
        &self.revision
    }
    pub fn reference_sha256(&self) -> &str {
        &self.reference_sha256
    }
}

impl ProjectRoot {
    /// Distinct Native-owned one-launch input, never a saved credential or a
    /// fabricated project file. Absence of a saved reference is part of proof.
    pub fn bind_ephemeral_vault_input(&self, captured: &ProjectRevision) -> Result<VaultInputBinding> {
        if self.revision()? != *captured || captured.entries.iter().any(|e|e.path()==VAULT_INPUT_FILE) {
            return Err(AutomationError::FileChanged);
        }
        let mut hasher=Sha256::new();
        hasher.update(b"cc-ansible-vault-ephemeral/v1\0");
        hasher.update(captured.digest.as_bytes());
        Ok(VaultInputBinding{credential:Uuid::nil(),revision:captured.digest.clone(),reference_sha256:hex::encode(hasher.finalize())})
    }
    /// An explicit Native selection must match the ref; the ref cannot request
    /// a different credential. No resolver, password, URL or helper is involved.
    pub fn bind_vault_input(
        &self,
        captured: &ProjectRevision,
        explicitly_selected: Uuid,
    ) -> Result<VaultInputBinding> {
        let binding = self
            .vault_input_reference(captured)?
            .ok_or(AutomationError::Invalid(
                "bounded Ansible Vault reference is required",
            ))?;
        if explicitly_selected.is_nil() || binding.credential != explicitly_selected {
            return Err(AutomationError::Invalid(
                "Ansible Vault reference does not match Native selection",
            ));
        }
        Ok(binding)
    }

    /// Read-only reference data, NEVER credential authority. Unknown/duplicate
    /// schema is rejected through the SAME parser used by bind_vault_input.
    pub fn vault_input_reference(
        &self,
        captured: &ProjectRevision,
    ) -> Result<Option<VaultInputBinding>> {
        if self.revision()? != *captured {
            return Err(AutomationError::FileChanged);
        }
        if !captured
            .entries
            .iter()
            .any(|entry| entry.path() == VAULT_INPUT_FILE)
        {
            return Ok(None);
        }
        let expected = captured
            .entries
            .iter()
            .find_map(|entry| match entry {
                ProjectEntry::File {
                    path,
                    sha256,
                    bytes,
                    ..
                } if path == VAULT_INPUT_FILE && *bytes <= MAX_REF_BYTES as u64 => Some(sha256),
                _ => None,
            })
            .ok_or(AutomationError::Invalid(
                "bounded Ansible Vault reference is required",
            ))?;
        let file = self.open_file(VAULT_INPUT_FILE)?;
        let mut bytes = Vec::new();
        file.take((MAX_REF_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_REF_BYTES {
            return Err(AutomationError::Invalid(
                "Ansible Vault reference exceeds budget",
            ));
        }
        let sha256 = hex::encode(Sha256::digest(&bytes));
        if &sha256 != expected {
            return Err(AutomationError::FileChanged);
        }
        // Reuse strict depth/duplicate-key/trailing-content rules. Never embed
        // attacker-controlled JSON or parse errors in diagnostics.
        let value = crate::json::parse(&bytes)?;
        let document: Document = serde_json::from_value(value)
            .map_err(|_| AutomationError::Invalid("invalid Ansible Vault reference"))?;
        let credential = Uuid::parse_str(&document.ansible_vault.credential_id)
            .map_err(|_| AutomationError::Invalid("invalid Ansible Vault credential reference"))?;
        if document.version != 1
            || credential.is_nil()
            || credential.to_string() != document.ansible_vault.credential_id
        {
            return Err(AutomationError::Invalid(
                "Ansible Vault reference does not match Native selection",
            ));
        }
        if self.revision()? != *captured {
            return Err(AutomationError::FileChanged);
        }
        Ok(Some(VaultInputBinding {
            credential,
            revision: captured.digest.clone(),
            reference_sha256: sha256,
        }))
    }
}

/// Fixed-purpose request data for a future PRIVATE job broker operation. It
/// contains no arbitrary secret ID, host ID, URL, path, expression or operation.
/// This type alone grants nothing and is not yet dispatched by HelperPeer.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultPasswordRequest {
    pub version: u32,
    pub purpose: VaultPasswordPurpose,
    pub job_id: Uuid,
    pub manifest_digest: String,
    pub revision_digest: String,
    pub reference_sha256: String,
}
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VaultPasswordPurpose {
    AnsibleVaultPassword,
}

/// One-use, non-Clone/non-Serde Native material. SecretValue zeroizes on drop;
/// no plaintext getter, generic JSON response, store or resolver is retained.
/// Only an already verified signed job can bind this lease. A future transport
/// MUST also retain AppCore's fresh profile/unlock fences at its send boundary.
/// ```compile_fail
/// fn serializable<T: serde::Serialize>() {}
/// serializable::<cc_automation_core::VaultPasswordLease>();
/// ```
pub struct VaultPasswordLease {
    job: Uuid,
    manifest: String,
    revision: String,
    reference: String,
    deadline: Instant,
    revoked: CancellationToken,
    job_cancel: CancellationToken,
    password: Option<SecretValue>,
}
impl fmt::Debug for VaultPasswordLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VaultPasswordLease")
            .field("job", &self.job)
            .field("consumed", &self.password.is_none())
            .finish_non_exhaustive()
    }
}
impl Drop for VaultPasswordLease {
    fn drop(&mut self) {
        self.revoked.cancel();
    }
}
impl VaultPasswordLease {
    /// Native calls ONLY after the exact signed job's process-atomic admission ACK
    /// and existing live-job cancellation registration, under its original
    /// scope guards. This is not a serializable approval/SQL proof constructor.
    /// Move the already-issued value; never resolve/copy/renew after lock.
    pub fn into_admitted_job(
        mut self,
        approval: &VerifiedJob,
        material_cancel: CancellationToken,
    ) -> Result<AdmittedVaultPasswordLease> {
        if self.password.is_none()
            || self.revoked.is_cancelled()
            || self.job_cancel.is_cancelled()
            || material_cancel.is_cancelled()
            || Instant::now() >= self.deadline
            || approval.is_access_expired()
            || approval.manifest().job_id != self.job
            || approval.digest() != self.manifest
            || approval.manifest().revision_digest != self.revision
        {
            return Err(AutomationError::Invalid(
                "Vault job material admission changed",
            ));
        }
        // Existing Native live_jobs policy owns this ORIGINAL job token:
        // cancel_on_lock cancels it, continue-on-lock may retain ONLY this
        // previously sealed one-use value. General vault authority is dropped.
        self.revoked = material_cancel.child_token();
        Ok(AdmittedVaultPasswordLease {
            inner: self,
            send_authority: material_cancel,
        })
    }
    /// Trusted Native issuer only. This does not make a project ref authority;
    /// Native must have resolved exactly binding.credential_id() from the
    /// current encrypted store after its independent user selection.
    pub fn issue(
        approval: &VerifiedJob,
        binding: &VaultInputBinding,
        password: SecretValue,
        revoked: CancellationToken,
        job_cancel: CancellationToken,
        ttl: Duration,
    ) -> Result<Self> {
        if approval.is_access_expired()
            || revoked.is_cancelled()
            || job_cancel.is_cancelled()
            || approval.manifest().revision_digest != binding.revision
            || ttl.is_zero()
            || ttl > Duration::from_secs(30)
            || !password_line_supported(password.expose_secret())
        {
            return Err(AutomationError::Invalid(
                "invalid Ansible Vault password lease",
            ));
        }
        Ok(Self {
            job: approval.manifest().job_id,
            manifest: approval.digest().to_owned(),
            revision: binding.revision.clone(),
            reference: binding.reference_sha256.clone(),
            deadline: (Instant::now() + ttl).min(approval.access_deadline()),
            revoked,
            job_cancel,
            password: Some(password),
        })
    }

    /// Trusted already-issued value only. Copies into opaque per-job log
    /// custody before one-use pipe consumption; no raw getter/serialization,
    /// new resolver, renewal or material send authority is introduced.
    pub fn retain_diagnostic(&self,capture:&cc_ssh_core::DiagnosticCapture)->Result<()>{
        if self.revoked.is_cancelled()||self.job_cancel.is_cancelled()||Instant::now()>=self.deadline{return Err(AutomationError::Invalid("Vault diagnostic custody unavailable"))}
        let value=self.password.as_ref().ok_or(AutomationError::Invalid("Vault diagnostic custody unavailable"))?;
        if capture.remember(value.expose_secret()).is_err(){capture.clear();}
        Ok(())
    }

    /// Consume BEFORE any write, including failed/partial writes. Call only
    /// with Ansible's private client pipe; never stdout, a log or guest data.
    /// The bounded password is neither returned as text nor serialized as JSON.
    /// Invalid/replayed/expired requests revoke the lease, with generic errors.
    pub fn write_once(
        &mut self,
        request: &VaultPasswordRequest,
        private_pipe: &mut impl Write,
    ) -> Result<()> {
        let password = self.password.take().ok_or(AutomationError::Invalid(
            "Ansible Vault lease is unavailable",
        ))?;
        if self.revoked.is_cancelled()
            || self.job_cancel.is_cancelled()
            || Instant::now() >= self.deadline
            || request.version != 1
            || request.purpose != VaultPasswordPurpose::AnsibleVaultPassword
            || request.job_id != self.job
            || request.manifest_digest != self.manifest
            || request.revision_digest != self.revision
            || request.reference_sha256 != self.reference
        {
            self.revoked.cancel();
            return Err(AutomationError::Invalid(
                "Ansible Vault lease is unavailable",
            ));
        }
        let written = private_pipe
            .write_all(password.expose_secret().as_bytes())
            .and_then(|()| private_pipe.write_all(b"\n"))
            .and_then(|()| private_pipe.flush());
        self.revoked.cancel();
        written.map_err(|_| AutomationError::Invalid("Ansible Vault private pipe failed"))
    }
}

/// Exact admitted job material. No credential resolver, root, Unlocked, Clone,
/// Deserialize, JSON response or plaintext getter. Original 30s TTL is retained.
pub struct AdmittedVaultPasswordLease {
    inner: VaultPasswordLease,
    send_authority: CancellationToken,
}
impl fmt::Debug for AdmittedVaultPasswordLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AdmittedVaultPasswordLease([REDACTED])")
    }
}
impl AdmittedVaultPasswordLease {
    pub(crate) fn send_fence(&self) -> crate::vault_validation::VaultWriteFence {
        // Consumption cancels only the local one-use child, not this original
        // job authority. Capture this fence BEFORE write_once takes the value.
        crate::vault_validation::VaultWriteFence {
            revoked: self.send_authority.clone(),
            job_cancel: Some(self.inner.job_cancel.clone()),
            deadline: self.inner.deadline,
        }
    }
    pub fn deadline(&self) -> Instant {
        self.inner.deadline
    }
    pub fn write_once(
        &mut self,
        request: &VaultPasswordRequest,
        private_pipe: &mut impl Write,
    ) -> Result<()> {
        self.inner.write_once(request, private_pipe)
    }
}

#[cfg(test)]
mod tests;
