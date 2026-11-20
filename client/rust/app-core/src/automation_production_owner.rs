//! Fixed OwnerConfirmation policy, held ONLY in the existing PreparedReview.
//! No grant/registry/persisted authority/OS-presence substitute is introduced.
use crate::automation_prepare::{invalid, PreparedReview};
use crate::session::{Session, Unlocked};
use crate::{AppCore, AppError, AppResult, StepUp};
use cc_automation_core::{EnrolledJobGrant, ManualOwnerGrant};
use cc_crypto_core::production_manifest_digest;
use cc_models::automation::JobManifest;
use cc_storage_core::ProfileId;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Weak,
};
use uuid::Uuid;

pub(crate) struct FrozenOwnerReview {
    manifest: JobManifest,
    digest: String,
    manual: ManualOwnerGrant,
    scope_id: String,
    request_fingerprint: [u8; 32],
    policy_fingerprint: [u8; 32],
    targets_fingerprint: [u8; 32],
    checking: Arc<AtomicBool>,
    #[cfg(test)]
    pause: Option<Arc<OwnerPassphrasePause>>,
}
impl FrozenOwnerReview {
    pub(crate) fn freeze(
        review: &PreparedReview,
        manual: ManualOwnerGrant,
        grant: &EnrolledJobGrant,
        scope_id: String,
        now: i64,
    ) -> AppResult<Self> {
        if !review.policy.production
            || review.service_origin.is_some()
            || review.service_delegation.is_some()
            || !manual.matches_enrolled(grant)
            || Uuid::parse_str(&scope_id)
                .map(|id| id.is_nil())
                .unwrap_or(true)
        {
            return Err(invalid("manual production owner scope required"));
        }
        let manifest = manifest_for_review(review, grant, now)?;
        let digest = production_manifest_digest(&manifest).map_err(AppError::from)?;
        Ok(Self {
            manifest,
            digest,
            manual,
            scope_id,
            request_fingerprint: fingerprint(&review.request)?,
            policy_fingerprint: fingerprint(&review.policy)?,
            targets_fingerprint: fingerprint(&review.targets)?,
            checking: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            pause: None,
        })
    }
    #[cfg(test)]
    pub(crate) fn pause_after_passphrase(&mut self) -> Arc<OwnerPassphrasePause> {
        let pause = Arc::new(OwnerPassphrasePause {
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        });
        self.pause = Some(pause.clone());
        pause
    }
    pub(crate) fn view(&self) -> Value {
        json!({"policy":"ownerConfirmationVaultPassphraseV1","kind":"ownerConfirmation",
            "required":true,"manifest_digest":self.digest,"job_id":self.manifest.job_id,
            "issued_at":self.manifest.issued_at,"expires_at":self.manifest.expires_at})
    }
    pub(crate) fn manifest(&self) -> &JobManifest {
        &self.manifest
    }
    pub(crate) fn manual(&self) -> &ManualOwnerGrant {
        &self.manual
    }
    pub(crate) fn authorize(
        &self,
        witness: &OwnerPassphraseWitness,
        owner: &Session,
        review: &PreparedReview,
        grant: &EnrolledJobGrant,
    ) -> AppResult<()> {
        witness.target.matches_review(owner, review)?;
        if witness.target.digest != self.digest
            || !self.manual.matches_enrolled(grant)
            || self.request_fingerprint != fingerprint(&review.request)?
            || self.policy_fingerprint != fingerprint(&review.policy)?
            || self.targets_fingerprint != fingerprint(&review.targets)?
        {
            return Err(invalid("production owner agreement changed"));
        }
        // Re-derive only deterministic frozen content, not the random nonce,
        // job ID or deadlines. Source/probe checks run in normal confirmation.
        if self.manifest.project_id.to_string() != review.request.project_id
            || self.manifest.runner_id.to_string() != review.request.runner_id
            || self.manifest.revision_digest != review.revision.digest
            || self.manifest.inventory_digest != review.validation.inventory_digest()
            || self.manifest.parameters_digest != review.validation.parameters_digest()
            || self.manifest.environment_digest != review.validation.environment_digest()
            || self.manifest.policy.production != review.policy.production
            || production_manifest_digest(&self.manifest).map_err(AppError::from)? != self.digest
        {
            return Err(invalid("production manifest changed"));
        }
        Ok(())
    }
}

fn fingerprint(value: &impl serde::Serialize) -> AppResult<[u8; 32]> {
    let raw = serde_json::to_vec(value)
        .map_err(|_| invalid("production review fingerprint unavailable"))?;
    Ok(Sha256::digest(raw).into())
}

/// Existing nonproduction behavior remains byte-for-byte equivalent. Production
/// invokes this ONCE at prepare; later passphrase attempts cannot renew it.
pub(crate) fn manifest_for_review(
    review: &PreparedReview,
    grant: &EnrolledJobGrant,
    now: i64,
) -> AppResult<JobManifest> {
    let manifest = JobManifest {
        version: 1,
        job_id: Uuid::new_v4(),
        tenant_id: grant.tenant_id,
        runner_id: grant.runner_id,
        project_id: Uuid::parse_str(&review.request.project_id)
            .map_err(|_| invalid("invalid project identifier"))?,
        revision_digest: review.revision.digest.clone(),
        inventory_digest: review.validation.inventory_digest().into(),
        environment_digest: review.validation.environment_digest().into(),
        parameters_digest: review.validation.parameters_digest().into(),
        playbook: review.request.playbook.clone(),
        policy: review.policy.clone(),
        mode: review.request.mode,
        targets: review.targets.clone(),
        issued_at: now,
        expires_at: if review.policy.production {
            review.expires_at.min(now + 600)
        } else {
            now + 600
        },
        access_expires_at: (now + i64::from(review.policy.timeout_seconds)).min(grant.expires_at),
        nonce: Uuid::new_v4(),
    };
    manifest.canonical_bytes().map_err(invalid)?;
    Ok(manifest)
}

struct PassphraseTarget {
    review_id: Uuid,
    profile_id: ProfileId,
    unlock: Weak<Unlocked>,
    generation: u64,
    digest: String,
    scope_id: String,
    device_id: cc_protocol::DeviceId,
    public_key: [u8; 32],
    checking: Arc<AtomicBool>,
    #[cfg(test)]
    pause: Option<Arc<OwnerPassphrasePause>>,
}
impl PassphraseTarget {
    fn capture(
        owner: &Session,
        review: &PreparedReview,
        original: &Arc<Unlocked>,
    ) -> AppResult<Self> {
        let frozen = review
            .production_owner
            .as_ref()
            .ok_or_else(|| invalid("production review unavailable"))?;
        if !review.policy.production
            || review.service_origin.is_some()
            || review.service_delegation.is_some()
            || !Weak::ptr_eq(&review.context.unlock, &Arc::downgrade(original))
        {
            return Err(invalid("original manual owner review required"));
        }
        let identity = owner.identity();
        let target = Self {
            review_id: review.id,
            profile_id: owner.profile_id,
            unlock: review.context.unlock.clone(),
            generation: review.context.generation,
            digest: frozen.digest.clone(),
            scope_id: frozen.scope_id.clone(),
            device_id: identity.device_id(),
            public_key: identity.public_keys().signing,
            checking: frozen.checking.clone(),
            #[cfg(test)]
            pause: frozen.pause.clone(),
        };
        target.matches_review(owner, review)?;
        Ok(target)
    }
    fn matches_review(&self, owner: &Session, review: &PreparedReview) -> AppResult<()> {
        let identity = owner.identity();
        let original = self.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let frozen = review
            .production_owner
            .as_ref()
            .ok_or_else(|| invalid("production review unavailable"))?;
        if self.profile_id != owner.profile_id
            || self.review_id != review.id
            || !Weak::ptr_eq(&self.unlock, &review.context.unlock)
            || !review.usable(owner.automation.unlocked_generation()?)
            || self.generation != review.context.generation
            || self.digest != frozen.digest
            || self.scope_id != frozen.scope_id
            || original.notices.scope_id() != Some(self.scope_id.clone())
            || identity.device_id() != self.device_id
            || identity.public_keys().signing != self.public_key
            || !review.policy.production
            || !review.validation.is_valid()
            || review.service_origin.is_some()
            || review.service_delegation.is_some()
        {
            return Err(invalid("original production owner review changed"));
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) struct OwnerPassphrasePause {
    pub entered: tokio::sync::Notify,
    pub release: tokio::sync::Notify,
}

struct OwnerCheckGuard(Arc<AtomicBool>);
impl Drop for OwnerCheckGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
impl PassphraseTarget {
    fn begin_check(&self) -> AppResult<OwnerCheckGuard> {
        self.checking
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| invalid("production owner confirmation already pending"))?;
        Ok(OwnerCheckGuard(self.checking.clone()))
    }
}

/// Non-Deserialize/private construction: no caller/model/presence bool can mint
/// this witness. It does not own keys and cannot outlive the original review.
pub(crate) struct OwnerPassphraseWitness {
    target: PassphraseTarget,
    password_envelope_digest: [u8; 32],
}
fn password_envelope_digest(envelope: &cc_protocol::envelopes::KeyEnvelope) -> AppResult<[u8; 32]> {
    let raw = serde_json::to_vec(envelope).map_err(|_| invalid("password envelope unavailable"))?;
    Ok(Sha256::digest(raw).into())
}
impl OwnerPassphraseWitness {
    pub(crate) async fn check_password_envelope(&self, original: &Session) -> AppResult<()> {
        let (_, envelope, _) = original.unlock_envelopes().await?;
        let envelope = envelope.ok_or(AppError::PassphraseRequired)?;
        if password_envelope_digest(&envelope)? != self.password_envelope_digest {
            return Err(AppError::PassphraseRequired);
        }
        Ok(())
    }
}

impl AppCore {
    /// Separate explicit OWNER confirmation with current-vault passphrase ONLY.
    /// Wrong passwords leave the SAME original review/deadline untouched.
    pub async fn automation_confirm_owner_run(
        &self,
        review_id: String,
        proof: StepUp,
    ) -> AppResult<Value> {
        let passphrase = match proof {
            StepUp::Passphrase(value) => crate::app::secret(value),
            StepUp::Presence(_) => return Err(AppError::PassphraseRequired),
        };
        let id = Uuid::parse_str(&review_id).map_err(|_| invalid("invalid review identifier"))?;
        let (owner, original) = self.unlocked().await?;
        let target = {
            let reviews = owner
                .automation
                .reviews
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let review = reviews
                .get(&id)
                .ok_or_else(|| invalid("review expired, was cancelled or was already consumed"))?;
            PassphraseTarget::capture(&owner, review, &original)?
        };
        drop(original);
        let check = target.begin_check()?;
        // ORIGINAL session storage, never self.verify_passphrase()/latest
        // self.session() after an async profile transition.
        let (vault_id, envelope, _) = owner.unlock_envelopes().await?;
        let envelope = envelope.ok_or(AppError::PassphraseRequired)?;
        let password_envelope_digest = password_envelope_digest(&envelope)?;
        self.check_original_owner_target(&owner, &target).await?;
        // The single-flight guard follows the actual Argon2 worker. Dropping
        // its async caller cannot fake worker completion/release the busy slot.
        let _check = crate::secrets::blocking(move || {
            let unlocked = cc_vault_core::unlock_with_passphrase(vault_id, &envelope, &passphrase)?;
            unlocked.lock();
            Ok(check)
        })
        .await?;
        #[cfg(test)]
        if let Some(pause) = &target.pause {
            pause.entered.notify_one();
            pause.release.notified().await;
        }
        self.check_original_owner_target(&owner, &target).await?;
        let original = target.unlock.clone();
        let witness = OwnerPassphraseWitness {
            target,
            password_envelope_digest,
        };
        witness.check_password_envelope(&owner).await?;
        self.automation_confirm_original(owner, original, id, Some(witness))
            .await
    }
    async fn check_original_owner_target(
        &self,
        owner: &Arc<Session>,
        target: &PassphraseTarget,
    ) -> AppResult<()> {
        let original = target.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = self.hold_session(owner).await?;
        let _epoch = owner.hold_unlocked(&original).await?;
        let reviews = owner
            .automation
            .reviews
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let review = reviews
            .get(&target.review_id)
            .ok_or_else(|| invalid("review expired, was cancelled or was already consumed"))?;
        target.matches_review(owner, review)
    }
}
