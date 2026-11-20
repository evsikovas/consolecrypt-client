//! Private explicit-selection capsule issuer. No Bridge, dispatch or resolver.
//! The reviewed recipient/delegation fields are metadata, not a job grant.
use crate::session::{Session, Unlocked};
use crate::working_set::VaultCredentialMetadata;
use crate::{AppCore, AppError, AppResult};
use cc_crypto_core::{
    ServiceCredentialBinding, ServiceCredentialCapsule, ServiceCredentialKind, ServiceTenantKind,
};
use cc_models::{ObjectId, secret::SecretKind};
#[cfg(test)]
use cc_storage_core::ProfileKind;
mod owner_scope;
#[cfg(test)]
pub(crate) use owner_scope::tests::Fixture as PersonalServiceFixture;
pub(crate) use owner_scope::{OwnerScope, metadata_personal_pin};
use sha2::{Digest, Sha256};
use std::{
    sync::{Arc, Weak},
    time::{Duration, Instant},
};
use tokio::sync::OwnedSemaphorePermit;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroizing;

fn invalid() -> AppError {
    AppError::invalid("service_credential", "service_credential_unavailable")
}
fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn now() -> AppResult<u64> {
    chrono::Utc::now()
        .timestamp()
        .try_into()
        .map_err(|_| invalid())
}

/// Explicit local Native selection. Not Deserialize, not permission, not a
/// server directory/guest claim. The subsequent owner action consumes review.
pub(crate) struct ServiceRecipientSelection {
    pub service_id: Uuid,
    pub recipient_public_key: [u8; 32],
    pub recipient_key_epoch: u64,
    pub recipient_certificate_epoch: u64,
    pub delegation_digest: String,
    pub policy_revision: u64,
    pub revocation_epoch: u64,
    pub delegation_expires_at: u64,
}

/// Bounded opaque one-use owner review. No plaintext, key, general lookup, or
/// serializable approval token. Profile/account/team remain distinct: initial
/// issuer supports localProfile ONLY, not a ProfileId disguised as account ID.
pub(crate) struct ServiceCredentialReview {
    owner_scope: Arc<OwnerScope>,
    owner: Arc<Session>,
    unlock: Weak<Unlocked>,
    generation: u64,
    scope: String,
    owner_signing_key: [u8; 32],
    metadata: VaultCredentialMetadata,
    binding: ServiceCredentialBinding,
    recipient: [u8; 32],
    revoked: CancellationToken,
    deadline: Instant,
    _slot: OwnedSemaphorePermit,
}
impl Drop for ServiceCredentialReview {
    fn drop(&mut self) {
        self.revoked.cancel();
    }
}
impl std::fmt::Debug for ServiceCredentialReview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceCredentialReview")
            .finish_non_exhaustive()
    }
}
impl ServiceCredentialReview {
    /// Exact public context for explicit user review; contains no secret value.
    pub(crate) fn binding(&self) -> &ServiceCredentialBinding {
        &self.binding
    }
    fn check_held(&self, u: &Unlocked) -> AppResult<()> {
        if self.revoked.is_cancelled()
            || Instant::now() >= self.deadline
            || self.owner.automation.stopped.is_cancelled()
            || self.owner.automation.unlocked_generation()? != self.generation
            || u.notices.scope_id().as_deref() != Some(self.scope.as_str())
            || *u.sharing_shutdown.borrow()
            || self.owner.identity().public_keys().signing != self.owner_signing_key
            || self.owner.device_id().to_string() != self.binding.owner_device_id
            || self.owner_scope.check(&self.owner, u).is_err()
            || u.working()
                .vault_credential_metadata(self.metadata.credential())
                .as_ref()
                != Some(&self.metadata)
            || self
                .metadata
                .fences()
                .iter()
                .any(|(id, rev)| u.working().revision_of(*id) != Some(*rev))
            || now()? >= self.binding.delegation_expires_at
        {
            return Err(invalid());
        }
        Ok(())
    }

    /// Separate explicit final Native action, consuming this exact review.
    /// Successful return is a capsule-export linearization, NOT a network send
    /// or dispatch. A recipient that has received a capsule can retain it until
    /// expiry; later jobs still require fresh signed head, quota/slot admission.
    pub(crate) async fn approve_and_seal(
        self,
        app: &AppCore,
    ) -> AppResult<ServiceCredentialCapsule> {
        let deadline = tokio::time::Instant::from_std(self.deadline);
        let u = self.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let mut shutdown = u.sharing_shutdown.subscribe();
        let writer = {
            let _profile = app.hold_session(&self.owner).await?;
            let _unlock = self.owner.hold_unlocked(&u).await?;
            self.check_held(&u)?;
            u.writer.clone()
        };
        drop(u); // No strong Unlocked/guard retained across secret-store I/O.
        let issue = async {
            self.owner_scope
                .verify(&self.owner, self.unlock.clone())
                .await?;
            let _io = self
                .owner
                .automation
                .io_slots
                .clone()
                .try_acquire_owned()
                .map_err(|_| invalid())?;
            let secret = writer
                .read_secret(self.metadata.secret())
                .await
                .map_err(|_| invalid())?;
            if secret.id != self.metadata.secret() || secret.kind != SecretKind::Password {
                return Err(invalid());
            }
            let u = self.unlock.upgrade().ok_or(AppError::VaultLocked)?;
            let _profile = app.hold_session(&self.owner).await?;
            let _unlock = self.owner.hold_unlocked(&u).await?;
            let profile = self.owner.profile().await?;
            self.owner_scope.check_profile(&profile)?;
            // No await after this synchronous final fence through seal/return.
            let version = u.working().version();
            self.check_held(&u)?;
            let identity = self.owner.identity();
            let value = secret.value.expose_secret();
            if value.is_empty() || value.len() > 16 * 1024 {
                return Err(invalid());
            }
            let issued = now()?;
            let capsule = self.owner_scope.with_current(&self.owner, || {
                identity
                    .seal_service_credential(
                        self.binding.clone(),
                        Zeroizing::new(value.as_bytes().to_vec()),
                        &self.recipient,
                        issued,
                        self.binding.delegation_expires_at,
                    )
                    .map_err(|_| invalid())
            })?;
            self.check_held(&u)?;
            if u.working().version() != version {
                return Err(invalid());
            }
            Ok(capsule)
        };
        tokio::select! {
            biased;
            _ = self.revoked.cancelled() => Err(invalid()),
            _ = self.owner.automation.stopped.cancelled() => Err(invalid()),
            _ = shutdown.changed() => Err(invalid()),
            _ = tokio::time::sleep_until(deadline) => Err(invalid()),
            result = issue => result,
        }
    }
}

impl AppCore {
    pub(crate) async fn service_credential_review(
        &self,
        expected_profile: String,
        explicitly_selected: String,
        recipient: ServiceRecipientSelection,
        cancel: &CancellationToken,
    ) -> AppResult<ServiceCredentialReview> {
        let owner = self.session().await?;
        let u = owner.unlocked().await?;
        let weak = Arc::downgrade(&u);
        drop(u);
        self.service_credential_review_original(
            expected_profile,
            explicitly_selected,
            recipient,
            cancel,
            owner,
            weak,
        )
        .await
    }
    pub(crate) async fn service_credential_review_original(
        &self,
        expected_profile: String,
        explicitly_selected: String,
        recipient: ServiceRecipientSelection,
        cancel: &CancellationToken,
        owner: Arc<Session>,
        weak: Weak<Unlocked>,
    ) -> AppResult<ServiceCredentialReview> {
        let selected = Uuid::parse_str(&explicitly_selected).map_err(|_| invalid())?;
        if selected.is_nil() || selected.to_string() != explicitly_selected || cancel.is_cancelled()
        {
            return Err(invalid());
        }
        let id: ObjectId = crate::dto::parse_id("credential_id", &explicitly_selected)?;
        if owner.profile_id.to_string() != expected_profile {
            return Err(invalid());
        }
        {
            let u = weak.upgrade().ok_or(AppError::VaultLocked)?;
            let _profile = self.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&u).await?;
        }
        let slot = owner
            .automation
            .review_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| invalid())?;
        let owner_scope = OwnerScope::capture(self, &owner, weak.clone()).await?;
        let u = weak.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        let profile = owner.profile().await?;
        owner_scope.check_profile(&profile)?;
        owner_scope.check(&owner, &u)?;
        let version = u.working().version();
        let metadata = u
            .working()
            .vault_credential_metadata(id)
            .ok_or_else(invalid)?;
        let revisions = metadata.fences();
        let revision: u64 = (*revisions.get(&id).ok_or_else(invalid)?)
            .try_into()
            .map_err(|_| invalid())?;
        let identity = owner.identity();
        let public = identity.public_keys();
        let issued = now()?;
        if recipient.delegation_expires_at <= issued
            || recipient.delegation_expires_at - issued > 86400
            || recipient.recipient_public_key == [0; 32]
            || recipient.recipient_public_key == public.encryption
            || recipient.recipient_public_key == public.signing
        {
            return Err(invalid());
        }
        let binding = ServiceCredentialBinding {
            version: 1,
            owner_device_id: identity.device_id().to_string(),
            owner_signing_key_sha256: hash(&public.signing),
            tenant_kind: owner_scope.tenant().kind.clone(),
            tenant_id: owner_scope.tenant().id.clone(),
            service_id: recipient.service_id.to_string(),
            delegation_digest: recipient.delegation_digest,
            policy_revision: recipient.policy_revision,
            revocation_epoch: recipient.revocation_epoch,
            credential_id: explicitly_selected,
            credential_revision: revision,
            kind: ServiceCredentialKind::Password,
            recipient_key_sha256: hash(&recipient.recipient_public_key),
            recipient_key_epoch: recipient.recipient_key_epoch,
            recipient_certificate_epoch: recipient.recipient_certificate_epoch,
            delegation_expires_at: recipient.delegation_expires_at,
        };
        binding.validate().map_err(|_| invalid())?;
        let revoked = cancel.child_token();
        if !u.working().watch_revisions(revisions, revoked.clone()) {
            return Err(invalid());
        }
        let review = ServiceCredentialReview {
            owner_scope,
            owner: owner.clone(),
            unlock: Arc::downgrade(&u),
            generation: owner.automation.unlocked_generation()?,
            scope: u.notices.scope_id().ok_or_else(invalid)?,
            owner_signing_key: public.signing,
            metadata,
            binding,
            recipient: recipient.recipient_public_key,
            revoked,
            deadline: Instant::now() + Duration::from_secs(60),
            _slot: slot,
        };
        review.check_held(&u)?;
        if version != u.working().version() {
            return Err(invalid());
        }
        Ok(review)
    }
}

#[cfg(test)]
mod tests;
