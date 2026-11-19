//! Validation is NOT a signed job. Dedicated phase evidence/password material
//! has no host/target, job signature, arbitrary resolver or execution authority.
use crate::{
    AutomationError, ProjectEntry, ProjectRevision, ProjectRoot, Result, VaultInputBinding,
};
use cc_models::secret::SecretValue;
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    io::Write,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Reviewed loader census compatibility evidence, NOT remote attestation or
/// credential authority. No deserialization/Clone; each Native observation is
/// bounded to 30s. Actual private peer owner must supply a fresh observation.
#[derive(Debug)]
pub struct VaultValidationRuntime {
    environment: String,
    helper: String,
    deadline: Instant,
}
impl VaultValidationRuntime {
    pub fn environment_digest(&self) -> &str {
        &self.environment
    }
    pub fn helper_digest(&self) -> &str {
        &self.helper
    }
    pub(crate) fn limit_to(&mut self, deadline: Instant) {
        self.deadline = self.deadline.min(deadline);
    }
    pub fn require_fresh(&self) -> Result<()> {
        if Instant::now() >= self.deadline {
            return Err(AutomationError::Invalid(
                "Vault runtime observation expired",
            ));
        }
        Ok(())
    }
    pub fn parse_pinned_probe(value: serde_json::Value) -> Result<Self> {
        use sha2::{Digest, Sha256};
        let bytes = serde_json::to_vec(&value)
            .map_err(|_| AutomationError::Invalid("invalid Vault runtime"))?;
        if bytes.len() > 64 * 1024 {
            return Err(AutomationError::Invalid(
                "Vault runtime report exceeds budget",
            ));
        }
        let parsed = crate::EnvironmentProbe::parse(value.clone())?;
        let pin: serde_json::Value =
            serde_json::from_str(include_str!("vault_validation/runtime-pin-v1.json"))
                .map_err(|_| AutomationError::Invalid("invalid reviewed Vault runtime pin"))?;
        let legacy:serde_json::Value=serde_json::from_str(include_str!("vault_validation/runtime-pin-v1-legacy.json"))
            .map_err(|_|AutomationError::Invalid("invalid reviewed legacy Vault runtime pin"))?;
        let approved_code = [&pin, &legacy].iter().any(|approved|
            value["environment_digest"] == approved["environment_digest"]
                && value["helper_digest"] == approved["helper_digest"])
            // Keep the exact previously reviewed Linux environment usable
            // after expanding the bundled helper to include macOS.
            || (value["environment_digest"] == "bae8a4ae4e6b78bcbcac5ca6174c50e188e022dc969476f1466ad7fe3e1d7331"
                && value["helper_digest"] == "7ae56c6de89564d6d4420c679b4a9e377a56fe731971e4c217a0b562d5e5ad6b");
        let mut identity = serde_json::Map::new();
        for key in [
            "helper",
            "protocol",
            "python",
            "system",
            "machine",
            "packages",
            "installed",
            "helper_digest",
            "dependency_lock",
            "dependency_files_digest",
        ] {
            identity.insert(
                key.into(),
                value
                    .get(key)
                    .ok_or(AutomationError::Invalid(
                        "incomplete Vault runtime identity",
                    ))?
                    .clone(),
            );
        }
        let canonical = serde_json::to_vec(&crate::project::canonical(serde_json::Value::Object(
            identity,
        )))
        .map_err(|_| AutomationError::Invalid("invalid Vault runtime identity"))?;
        let derived = hex::encode(Sha256::digest(canonical));
        if parsed.environment_digest != derived
            || !approved_code
            || !parsed.dependencies_ready
            || !parsed.non_root
            || !parsed.sandbox.filesystem
            || !parsed.sandbox.pid
            || !parsed.sandbox.network
            || parsed.capabilities.get("validate") != Some(&true)
            || parsed.capabilities.get("target_match_v1") != Some(&true)
            || parsed.require_trusted_worker_bootstrap().is_err()
        {
            return Err(AutomationError::Invalid(
                "unsupported Vault loader runtime; reviewed proof required",
            ));
        }
        Ok(Self {
            environment: derived,
            helper: value["helper_digest"]
                .as_str()
                .ok_or(AutomationError::Invalid("invalid helper identity"))?
                .into(),
            deadline: Instant::now() + Duration::from_secs(30),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultValidationPhase {
    Syntax,
    Lint,
}

/// Non-Clone/non-Deserialize evidence, not authorization from an ID or label.
/// Native still owns original profile/credential/phase registration/send fences.
#[derive(Debug)]
pub struct VaultValidationBinding {
    reference: VaultInputBinding,
    scope: Uuid,
    phase: VaultValidationPhase,
    nonce: Uuid,
    call_index: u8,
    helper: String,
    playbook: String,
    environment: String,
    deadline: Instant,
}
impl VaultValidationBinding {
    pub fn require_live(&self) -> Result<()> {
        if Instant::now() >= self.deadline {
            return Err(AutomationError::Invalid("Vault child expired"));
        }
        Ok(())
    }
    pub fn credential_id(&self) -> Uuid {
        self.reference.credential_id()
    }
    pub fn revision_digest(&self) -> &str {
        self.reference.revision_digest()
    }
    pub fn reference_sha256(&self) -> &str {
        self.reference.reference_sha256()
    }
    pub fn playbook(&self) -> &str {
        &self.playbook
    }
    pub fn environment_digest(&self) -> &str {
        &self.environment
    }
    pub fn phase(&self) -> VaultValidationPhase {
        self.phase
    }
    pub fn diagnostics_marker(&self) -> VaultValidationMarker {
        VaultValidationMarker {
            revision: self.revision_digest().into(),
            environment: self.environment.clone(),
            playbook: self.playbook.clone(),
        }
    }
    pub fn request(&self) -> VaultValidationPasswordRequest {
        VaultValidationPasswordRequest {
            version: 1,
            purpose: VaultValidationPurpose::AnsibleVaultValidationPassword,
            scope_id: self.scope,
            phase: self.phase,
            call_index: self.call_index,
            environment_digest: self.environment.clone(),
            helper_digest: self.helper.clone(),
            phase_nonce: self.nonce,
            revision_digest: self.revision_digest().into(),
            reference_sha256: self.reference_sha256().into(),
        }
    }
}
impl ProjectRoot {
    /// Reuses the actual project/reference reader. Caller supplies independent
    /// Native credential selection and Native-created scope/phase identity.
    /// Those identifiers alone never approve a lookup; AppCore must register
    /// the original live Native context before any private password request.
    pub fn bind_vault_validation(
        &self,
        captured: &ProjectRevision,
        selected: Uuid,
        scope: Uuid,
        phase: VaultValidationPhase,
        nonce: Uuid,
        playbook: &str,
        runtime: &VaultValidationRuntime,
        call_index: u8,
    ) -> Result<VaultValidationBinding> {
        self.bind_vault_validation_input(captured,Some(selected),scope,phase,nonce,playbook,runtime,call_index)
    }
    pub fn bind_ephemeral_vault_validation(
        &self, captured:&ProjectRevision, scope:Uuid, phase:VaultValidationPhase,
        nonce:Uuid, playbook:&str, runtime:&VaultValidationRuntime, call_index:u8,
    ) -> Result<VaultValidationBinding> {
        self.bind_vault_validation_input(captured,None,scope,phase,nonce,playbook,runtime,call_index)
    }
    fn bind_vault_validation_input(
        &self, captured:&ProjectRevision, selected:Option<Uuid>, scope:Uuid, phase:VaultValidationPhase,
        nonce:Uuid, playbook:&str, runtime:&VaultValidationRuntime, call_index:u8,
    ) -> Result<VaultValidationBinding> {
        crate::validate_project_path(playbook)?;
        runtime.require_fresh()?;
        let environment = runtime.environment_digest();
        if call_index >= 2
            || scope.is_nil()
            || nonce.is_nil()
            || !cc_models::automation::is_digest(environment)
            || !(playbook.ends_with(".yml") || playbook.ends_with(".yaml"))
            || !captured
                .entries
                .iter()
                .any(|entry| matches!(entry,ProjectEntry::File{path,..} if path==playbook))
        {
            return Err(AutomationError::Invalid("invalid Vault validation binding"));
        }
        let reference = match selected {
            Some(id)=>self.bind_vault_input(captured,id)?,
            None=>self.bind_ephemeral_vault_input(captured)?,
        };
        Ok(VaultValidationBinding {
            reference,
            scope,
            phase,
            nonce,
            call_index,
            helper: runtime.helper_digest().into(),
            playbook: playbook.into(),
            environment: environment.into(),
            deadline: runtime
                .deadline
                .min(Instant::now() + Duration::from_secs(30)),
        })
    }
}

/// Native-derived sensitivity marker. No deserialization or guest-supplied
/// flag can construct it. This marks POTENTIAL use before any password issue.
#[derive(Debug)]
pub struct VaultValidationMarker {
    pub(crate) revision: String,
    pub(crate) environment: String,
    pub(crate) playbook: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VaultValidationPurpose {
    AnsibleVaultValidationPassword,
}

/// Fixed-purpose PRIVATE request data. No credential ID, host ID, path, URL,
/// signed-job ID or caller 'approved/used_vault' flag. Registry/context gates
/// decide authority; possession of scope_id/phase_nonce is never sufficient.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultValidationPasswordRequest {
    pub version: u32,
    pub purpose: VaultValidationPurpose,
    pub scope_id: Uuid,
    pub phase: VaultValidationPhase,
    pub call_index: u8,
    pub environment_digest: String,
    pub helper_digest: String,
    pub phase_nonce: Uuid,
    pub revision_digest: String,
    pub reference_sha256: String,
}

/// Original scope revocation/deadline, independent of local one-use consumption.
/// Minted only inside an issued lease; no public/Serde authority constructor.
pub(crate) struct VaultWriteFence {
    pub(crate) revoked: CancellationToken,
    pub(crate) job_cancel: Option<CancellationToken>,
    pub(crate) deadline: Instant,
}
impl VaultWriteFence {
    pub(crate) fn denied(&self) -> bool {
        self.revoked.is_cancelled()
            || self
                .job_cancel
                .as_ref()
                .is_some_and(CancellationToken::is_cancelled)
            || Instant::now() >= self.deadline
    }
    pub(crate) async fn cancelled(&self) {
        match &self.job_cancel {
            Some(job) => tokio::select! {_=self.revoked.cancelled()=>(),_=job.cancelled()=>()},
            None => self.revoked.cancelled().await,
        }
    }
}

pub struct VaultValidationPasswordLease {
    request: VaultValidationPasswordRequest,
    password: Option<SecretValue>,
    deadline: Instant,
    revoked: CancellationToken,
    send_authority: CancellationToken,
}
impl fmt::Debug for VaultValidationPasswordLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VaultValidationPasswordLease")
            .field("phase", &self.request.phase)
            .field("consumed", &self.password.is_none())
            .finish_non_exhaustive()
    }
}
impl Drop for VaultValidationPasswordLease {
    fn drop(&mut self) {
        self.revoked.cancel();
    }
}
impl VaultValidationPasswordLease {
    pub(crate) fn send_fence(&self) -> VaultWriteFence {
        VaultWriteFence {
            revoked: self.send_authority.clone(),
            job_cancel: None,
            deadline: self.deadline,
        }
    }
    /// Trusted Native JIT issuer only, after fresh original profile/FS/SQL/
    /// credential fences. Caller must provide a DISTINCT phase/call child
    /// cancellation token; consuming syntax cannot cancel/reuse a lint lease.
    pub fn issue(
        binding: &VaultValidationBinding,
        password: SecretValue,
        revoked: CancellationToken,
    ) -> Result<Self> {
        if revoked.is_cancelled()
            || Instant::now() >= binding.deadline
            || !crate::vault_inputs::password_line_supported(password.expose_secret())
        {
            return Err(AutomationError::Invalid(
                "unsupported Vault password line or expired phase",
            ));
        }
        Ok(Self {
            request: binding.request(),
            password: Some(password),
            deadline: binding.deadline,
            revoked: revoked.child_token(),
            send_authority: revoked,
        })
    }
    /// Only Ansible's PRIVATE password client pipe. Consume before any write,
    /// fail closed on a changed phase/reference or partial write; never trim.
    pub fn write_once(
        &mut self,
        request: &VaultValidationPasswordRequest,
        private_pipe: &mut impl Write,
    ) -> Result<()> {
        let password = self
            .password
            .take()
            .ok_or(AutomationError::Invalid("Vault phase unavailable"))?;
        if self.revoked.is_cancelled()
            || Instant::now() >= self.deadline
            || request.version != 1
            || request.purpose != self.request.purpose
            || request.call_index != self.request.call_index
            || request.environment_digest != self.request.environment_digest
            || request.helper_digest != self.request.helper_digest
            || request.scope_id != self.request.scope_id
            || request.phase != self.request.phase
            || request.phase_nonce != self.request.phase_nonce
            || request.revision_digest != self.request.revision_digest
            || request.reference_sha256 != self.request.reference_sha256
        {
            self.revoked.cancel();
            return Err(AutomationError::Invalid("Vault phase unavailable"));
        }
        let sent = private_pipe
            .write_all(password.expose_secret().as_bytes())
            .and_then(|()| private_pipe.write_all(b"\n"))
            .and_then(|()| private_pipe.flush());
        self.revoked.cancel();
        sent.map_err(|_| AutomationError::Invalid("Vault private pipe failed"))
    }
}

#[cfg(test)]
mod tests;
