use crate::{AutomationError, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use cc_models::automation::{JobManifest, JOB_SIGNATURE_DOMAIN};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct EnrolledJobGrant {
    pub signer_id: Uuid,
    pub public_key: [u8; 32],
    pub tenant_id: Uuid,
    pub runner_id: Uuid,
    pub projects: HashSet<Uuid>,
    pub hosts: HashSet<Uuid>,
    pub jobs_execute: bool,
    pub expires_at: i64,
    pub max_job_seconds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedJobApproval {
    pub signer_id: Uuid,
    pub manifest: JobManifest,
    pub signature: String,
}

/// Owned authority, constructed only by signature+grant verification. Callers
/// receive references; they cannot mutate a verified manifest after the fact.
#[derive(Debug)]
pub struct VerifiedJob {
    manifest: JobManifest,
    digest: String,
    access_deadline: Instant,
    submission_deadline: Instant,
    owner_confirmation: Option<cc_crypto_core::OwnerConfirmation>,
    manual_owner: Option<crate::ManualOwnerGrant>,
}

impl VerifiedJob {
    pub fn manifest(&self) -> &JobManifest {
        &self.manifest
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn is_access_expired(&self) -> bool {
        Instant::now() >= self.access_deadline || self.owner_retired()
    }
    pub fn is_submission_expired(&self) -> bool {
        Instant::now() >= self.submission_deadline || self.owner_retired()
    }
    pub fn dispatch_remaining(&self) -> Duration {
        if self.owner_retired() { return Duration::ZERO; }
        self.access_deadline
            .min(self.submission_deadline)
            .saturating_duration_since(Instant::now())
    }
    pub(crate) fn access_deadline(&self) -> Instant {
        self.access_deadline
    }

    pub fn verify(
        approval: &SignedJobApproval,
        grant: &EnrolledJobGrant,
        environment_digest: &str,
        now: i64,
    ) -> Result<Self> {
        Self::verify_internal(approval, grant, environment_digest, now, None)
    }

    /// Extra evidence is admitted ONLY with the original opaque manual-owner
    /// grant. Service/Node/plain enrolled grants cannot call this path.
    pub fn verify_owner_confirmed(
        approval: &SignedJobApproval,
        original: &crate::ManualOwnerGrant,
        confirmation: &cc_crypto_core::OwnerConfirmation,
        environment_digest: &str,
        now: i64,
    ) -> Result<Self> {
        let grant = original.original()?;
        if !approval.manifest.policy.production { return Err(AutomationError::Invalid("owner confirmation purpose mismatch")); }
        confirmation.verify(&approval.manifest, grant.signer_id, &grant.public_key)
            .map_err(|_| AutomationError::Invalid("invalid production owner confirmation"))?;
        let mut verified = Self::verify_internal(approval, grant, environment_digest, now, Some(confirmation.clone()))?;
        verified.manual_owner = Some(original.clone_original());
        Ok(verified)
    }
    pub fn owner_confirmation(&self) -> Option<&cc_crypto_core::OwnerConfirmation> {
        self.owner_confirmation.as_ref()
    }
    fn owner_retired(&self) -> bool {
        self.manual_owner.as_ref().is_some_and(|owner| owner.original().is_err())
    }
    fn verify_internal(
        approval: &SignedJobApproval,
        grant: &EnrolledJobGrant,
        environment_digest: &str,
        now: i64,
        owner_confirmation: Option<cc_crypto_core::OwnerConfirmation>,
    ) -> Result<Self> {
        let verification_started = Instant::now();
        let manifest = &approval.manifest;
        if approval.signer_id != grant.signer_id
            || !grant.jobs_execute
            || grant.expires_at <= now
            || manifest.tenant_id != grant.tenant_id
            || manifest.runner_id != grant.runner_id
            || !grant.projects.contains(&manifest.project_id)
            || manifest
                .targets
                .iter()
                .any(|target| !grant.hosts.contains(&target.host_id))
        {
            return Err(AutomationError::Invalid(
                "approval is outside the enrolled grant",
            ));
        }
        if manifest.environment_digest != environment_digest
            || manifest.issued_at > now.saturating_add(30)
            || now.saturating_sub(manifest.issued_at) > 600
            || manifest.expires_at <= now
            || manifest.policy.timeout_seconds > grant.max_job_seconds
            || manifest.access_expires_at <= now
            || manifest.access_expires_at > grant.expires_at
            || (manifest.policy.production && owner_confirmation.is_none())
        {
            return Err(AutomationError::Invalid(
                "expired, changed or unsupported execution approval",
            ));
        }
        let signature: [u8; 64] = STANDARD
            .decode(&approval.signature)
            .ok()
            .and_then(|raw| raw.try_into().ok())
            .ok_or(AutomationError::Invalid(
                "invalid approval signature encoding",
            ))?;
        cc_crypto_core::verify_automation_manifest(&grant.public_key, manifest, &signature)
            .map_err(|_| AutomationError::Invalid("invalid job signature"))?;
        let mut hasher = Sha256::new();
        hasher.update(JOB_SIGNATURE_DOMAIN);
        hasher.update(
            manifest
                .canonical_bytes()
                .map_err(AutomationError::Invalid)?,
        );
        Ok(Self {
            owner_confirmation,
            manual_owner: None,
            manifest: manifest.clone(),
            digest: hex::encode(hasher.finalize()),
            access_deadline: verification_started
                .checked_add(Duration::from_secs(
                    (manifest.access_expires_at.saturating_sub(now) as u64)
                        .min(manifest.policy.timeout_seconds as u64),
                ))
                .ok_or(AutomationError::Invalid("invalid access deadline"))?,
            submission_deadline: verification_started
                .checked_add(Duration::from_secs(
                    manifest.expires_at.saturating_sub(now) as u64
                ))
                .ok_or(AutomationError::Invalid("invalid submission deadline"))?,
        })
    }
}
