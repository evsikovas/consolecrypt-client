//! Original Native owner metadata. Ledger rows never reconstruct authority.
use super::{ConfirmedServiceDelegation, invalid, now};
use crate::service_credential_issuer::{OwnerScope, metadata_personal_pin};
use crate::session::{Session, Unlocked};
use crate::{AppCore, AppError, AppResult};
use cc_storage_core::{
    ProfileKind, ServiceOwnerDelivery, ServiceOwnerIdentity, ServiceOwnerRecord,
    ServiceOwnerSnapshot, ServiceOwnerWriteError, Storage, Tx,
};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Weak};
use uuid::Uuid;

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn unknown() -> AppError {
    AppError::invalid("service_owner", "service_owner_delivery_unknown")
}

pub(super) struct OwnerLedger {
    owner: Arc<Session>,
    unlock: Weak<Unlocked>,
    storage: Storage,
    identity: ServiceOwnerIdentity,
    signing: [u8; 32],
    generation: u64,
    scope: String,
    fresh: Option<Arc<OwnerScope>>,
}
#[derive(Clone, Copy)]
enum WriteKind {
    Fresh,
    LocalBlock(Uuid, Option<u64>),
}
/// Local denial DATA only; remote status is explicitly UNKNOWN.
pub(super) struct LocallyBlockedRemotelyUnknown;

impl OwnerLedger {
    pub(super) fn original_owner_scope(&self) -> AppResult<Arc<OwnerScope>> {
        self.fresh.clone().ok_or_else(invalid)
    }
    #[cfg(test)]
    pub(super) async fn capture(
        app: &AppCore,
        owner: Arc<Session>,
        u: &Arc<Unlocked>,
    ) -> AppResult<Self> {
        Self::capture_weak(app, owner, Arc::downgrade(u)).await
    }
    pub(super) async fn capture_weak(
        app: &AppCore,
        owner: Arc<Session>,
        weak: Weak<Unlocked>,
    ) -> AppResult<Self> {
        let fresh = OwnerScope::capture(app, &owner, weak.clone()).await?;
        Self::capture_scope(app, owner, weak, Some(fresh)).await
    }
    /// Existing metadata read/local denial only. No network/new-owner proof.
    pub(super) async fn capture_stale(
        app: &AppCore,
        owner: Arc<Session>,
        weak: Weak<Unlocked>,
    ) -> AppResult<Self> {
        Self::capture_scope(app, owner, weak, None).await
    }
    async fn capture_scope(
        app: &AppCore,
        owner: Arc<Session>,
        weak: Weak<Unlocked>,
        fresh: Option<Arc<OwnerScope>>,
    ) -> AppResult<Self> {
        let u = weak.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = app.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        let profile = owner.profile().await?;
        let personal = metadata_personal_pin(&profile, &u)?;
        if let Some(fresh) = &fresh {
            fresh.check_profile(&profile)?;
            fresh.check(&owner, &u)?;
            if fresh.personal_pin() != personal {
                return Err(invalid());
            }
        }
        let signing = owner.identity().public_keys().signing;
        let result = Self {
            storage: owner.storage()?,
            identity: ServiceOwnerIdentity {
                profile_id: owner.profile_id,
                vault_id: u.vault_id,
                device_id: owner.device_id(),
                signing_fingerprint: digest(&signing),
                personal,
            },
            signing,
            generation: owner.automation.unlocked_generation()?,
            scope: u.notices.scope_id().ok_or_else(invalid)?,
            owner: owner.clone(),
            unlock: weak,
            fresh,
        };
        result.check_base(&u)?;
        Ok(result)
    }
    fn check_base(&self, u: &Unlocked) -> AppResult<()> {
        if self.owner.profile_id != self.identity.profile_id
            || self.owner.require_vault_id()? != self.identity.vault_id
            || u.vault_id != self.identity.vault_id
            || self.owner.device_id() != self.identity.device_id
            || self.owner.identity().public_keys().signing != self.signing
            || self.owner.automation.stopped.is_cancelled()
            || self.owner.automation.unlocked_generation()? != self.generation
            || *u.sharing_shutdown.borrow()
            || u.notices.scope_id().as_deref() != Some(self.scope.as_str())
            || (self.identity.personal.is_none() && self.owner.api_opt().is_some())
        {
            return Err(invalid());
        }
        Ok(())
    }
    fn check(&self, u: &Unlocked, fresh: bool) -> AppResult<()> {
        self.check_base(u)?;
        if fresh {
            self.fresh
                .as_ref()
                .ok_or_else(invalid)?
                .check(&self.owner, u)?;
        }
        Ok(())
    }
    fn check_sql(&self, tx: &Tx<'_>, fresh: bool) -> AppResult<()> {
        let profile = tx.get_profile()?.ok_or_else(invalid)?;
        let vault = tx.get_vault(self.identity.vault_id)?.ok_or_else(invalid)?;
        let u = self.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        if profile.profile_id != self.identity.profile_id
            || profile.device_id != self.identity.device_id
            || metadata_personal_pin(&profile, &u)? != self.identity.personal
            || vault.role != cc_protocol::vaults::VaultRole::Owner
            || vault.state != cc_protocol::vaults::VaultState::Active
        {
            return Err(invalid());
        }
        match &self.identity.personal {
            None if profile.kind == ProfileKind::Local && vault.owner_user_id.is_none() => {}
            Some(pin)
                if profile.kind == ProfileKind::Synced
                    && vault.owner_user_id == Some(pin.user_id)
                    && vault.caller_trusted => {}
            _ => return Err(invalid()),
        }
        self.check(&u, fresh)
    }
    fn with_fresh<T>(&self, fresh: bool, f: impl FnOnce() -> AppResult<T>) -> AppResult<T> {
        if fresh {
            self.fresh
                .as_ref()
                .ok_or_else(invalid)?
                .with_current(&self.owner, f)
        } else {
            f()
        }
    }
    pub(super) async fn read(self: &Arc<Self>, app: &AppCore) -> AppResult<ServiceOwnerSnapshot> {
        self.read_impl(app, true).await
    }
    pub(super) async fn read_stale_metadata(
        self: &Arc<Self>,
        app: &AppCore,
    ) -> AppResult<ServiceOwnerSnapshot> {
        self.read_impl(app, false).await
    }
    pub(super) async fn schedule_metadata(
        self: &Arc<Self>,
        app: &AppCore,
    ) -> AppResult<Vec<serde_json::Value>> {
        let u = self.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = app.hold_session(&self.owner).await?;
        let _unlock = self.owner.hold_unlocked(&u).await?;
        self.check(&u, false)?;
        let original = self.clone();
        let rows = self
            .storage
            .read(move |tx| -> AppResult<_> {
                original.with_fresh(false, || {
                    original.check_sql(tx, false)?;
                    super::schedules::list(tx, &original.identity)
                })
            })
            .await?;
        self.check(&u, false)?;
        Ok(rows)
    }
    async fn read_impl(
        self: &Arc<Self>,
        app: &AppCore,
        fresh: bool,
    ) -> AppResult<ServiceOwnerSnapshot> {
        let u = self.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = app.hold_session(&self.owner).await?;
        let _unlock = self.owner.hold_unlocked(&u).await?;
        self.check(&u, fresh)?;
        let original = self.clone();
        let snapshot = self
            .storage
            .read(move |tx| -> AppResult<_> {
                original.with_fresh(fresh, || {
                    original.check_sql(tx, fresh)?;
                    Ok(tx.service_owner_snapshot(&original.identity)?)
                })
            })
            .await?;
        self.check(&u, fresh)?;
        Ok(snapshot)
    }
    async fn replace(
        self: &Arc<Self>,
        app: &AppCore,
        expected: ServiceOwnerSnapshot,
        rows: Vec<ServiceOwnerRecord>,
    ) -> AppResult<ServiceOwnerSnapshot> {
        self.replace_impl(app, expected, rows, WriteKind::Fresh, None)
            .await
    }
    async fn replace_impl(
        self: &Arc<Self>,
        app: &AppCore,
        expected: ServiceOwnerSnapshot,
        rows: Vec<ServiceOwnerRecord>,
        kind: WriteKind,
        schedule: Option<super::schedules::Metadata>,
    ) -> AppResult<ServiceOwnerSnapshot> {
        let fresh = matches!(kind, WriteKind::Fresh);
        let u = self.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = app.hold_session(&self.owner).await?;
        let _unlock = self.owner.hold_unlocked(&u).await?;
        self.check(&u, fresh)?;
        let original = self.clone();
        let result = self
            .storage
            .write_service_owner(move |tx| -> AppResult<_> {
                original.with_fresh(fresh, || {
                    original.check_sql(tx, fresh)?;
                    if let WriteKind::LocalBlock(id, runner_revision) = kind {
                        if let Some(revision) = runner_revision {
                            let runner =
                                crate::automation_runners::stored_runner(tx, &id.to_string())?;
                            if runner.revision != Some(revision) {
                                return Err(invalid());
                            }
                            runner
                                .configuration
                                .as_ref()
                                .ok_or_else(invalid)?
                                .service_remote()?;
                        }
                        // Denial cannot create/replace an owner/record, change policy,
                        // erase tombstones, or elevate any other delivery metadata.
                        let old = expected.records();
                        if old.len() != rows.len() || !old.iter().any(|r| r.service_id == id) {
                            return Err(invalid());
                        }
                        for (before, after) in old.iter().zip(&rows) {
                            let mut allowed = before.clone();
                            if before.service_id == id {
                                allowed.revocation_epoch =
                                    before.revocation_epoch.checked_add(1).ok_or_else(invalid)?;
                                allowed.clock_floor = before.clock_floor.max(now()?);
                                allowed.delivery = ServiceOwnerDelivery::RevokedLocally;
                                // A clock tick between construction and SQL is not a
                                // reason to retry: preserve caller's already-safe floor.
                                if after.clock_floor < before.clock_floor
                                    || after.clock_floor > 253_402_300_799
                                {
                                    return Err(invalid());
                                }
                                allowed.clock_floor = after.clock_floor;
                            }
                            if &allowed != after {
                                return Err(invalid());
                            }
                        }
                    }
                    tx.service_owner_compare_exchange(&expected, rows)?;
                    if let Some(schedule) = schedule {
                        super::schedules::put(tx, &original.identity, schedule)?;
                    }
                    original.check_sql(tx, fresh)?;
                    Ok(tx.service_owner_snapshot(&original.identity)?)
                })
            })
            .await;
        let snapshot = match result {
            Ok(snapshot) => snapshot,
            Err(ServiceOwnerWriteError::BeforeWrite(e)) => return Err(e.into()),
            Err(ServiceOwnerWriteError::Rejected(e)) => return Err(e),
            Err(ServiceOwnerWriteError::Unknown) => return Err(unknown()),
        };
        self.check(&u, fresh).map_err(|_| unknown())?;
        Ok(snapshot)
    }
    /// No fresh auth required for this denial of an EXISTING exact owner row.
    /// This API cannot return a signature or claim remote revocation/ACK.
    pub(super) async fn block_existing_local(
        self: &Arc<Self>,
        app: &AppCore,
        service: Uuid,
    ) -> AppResult<LocallyBlockedRemotelyUnknown> {
        self.block_original_runner(app, service, None).await
    }
    pub(super) async fn block_original_runner(
        self: &Arc<Self>,
        app: &AppCore,
        service: Uuid,
        runner_revision: Option<u64>,
    ) -> AppResult<LocallyBlockedRemotelyUnknown> {
        let snapshot = self.read_stale_metadata(app).await?;
        let mut rows = snapshot.records().to_vec();
        let row = rows
            .iter_mut()
            .find(|r| r.service_id == service)
            .ok_or_else(invalid)?;
        row.revocation_epoch = row.revocation_epoch.checked_add(1).ok_or_else(invalid)?;
        row.clock_floor = row.clock_floor.max(now()?);
        row.delivery = ServiceOwnerDelivery::RevokedLocally;
        self.replace_impl(
            app,
            snapshot,
            rows,
            WriteKind::LocalBlock(service, runner_revision),
            None,
        )
        .await?;
        Ok(LocallyBlockedRemotelyUnknown)
    }
    #[cfg(test)]
    pub(super) async fn revoke_local(
        self: &Arc<Self>,
        app: &AppCore,
        service: Uuid,
    ) -> AppResult<()> {
        self.block_existing_local(app, service).await?;
        Ok(())
    }
    pub(super) async fn revoke_and_sign(
        self: &Arc<Self>,
        app: &AppCore,
        signed: &cc_crypto_core::SignedServiceDelegation,
    ) -> AppResult<cc_crypto_core::SignedServiceRevocation> {
        let scope = self.fresh.as_ref().ok_or_else(invalid)?;
        scope.verify(&self.owner, self.unlock.clone()).await?;
        let p = signed.body();
        if p.owner_id != self.identity.device_id.to_string() || p.tenant != *scope.tenant() {
            return Err(invalid());
        }
        cc_crypto_core::SignedServiceDelegation::verify_json(
            &signed.to_json().map_err(|_| invalid())?,
            &p.owner_id,
            &self.owner.identity().public_keys(),
            &p.tenant,
            p.issued_at,
        )
        .map_err(|_| invalid())?;
        let id = Uuid::parse_str(&p.service_id).map_err(|_| invalid())?;
        let snapshot = self.read(app).await?;
        let mut rows = snapshot.records().to_vec();
        let row = rows
            .iter_mut()
            .find(|r| r.service_id == id)
            .ok_or_else(invalid)?;
        if row.policy_digest != signed.digest().map_err(|_| invalid())?
            || row.owner_epoch != p.owner_epoch
            || row.policy_revision < p.policy_revision
            || row.revocation_epoch < p.revocation_epoch
        {
            return Err(invalid());
        }
        row.revocation_epoch = row.revocation_epoch.checked_add(1).ok_or_else(invalid)?;
        row.clock_floor = row.clock_floor.max(now()?);
        row.delivery = ServiceOwnerDelivery::RevokedLocally;
        let (policy, revocation, floor) =
            (row.policy_revision, row.revocation_epoch, row.clock_floor);
        self.replace(app, snapshot, rows).await?;
        let u = self.unlock.upgrade().ok_or_else(unknown)?;
        let _profile = app.hold_session(&self.owner).await.map_err(|_| unknown())?;
        let _unlock = self.owner.hold_unlocked(&u).await.map_err(|_| unknown())?;
        self.check(&u, true).map_err(|_| unknown())?;
        scope.with_current(&self.owner, || {
            self.owner
                .identity()
                .sign_service_revocation(signed, policy, revocation, floor, now()?)
                .map_err(|_| unknown())
        })
    }
}

impl OwnerLedger {
    /// DATA binding for status/original-policy revocation only, not new consent.
    pub(super) fn recorded_tenant(&self) -> cc_crypto_core::ServicePolicyTenant {
        match &self.identity.personal {
            None => cc_crypto_core::ServicePolicyTenant {
                kind: cc_crypto_core::ServiceTenantKind::LocalProfile,
                id: self.identity.profile_id.to_string(),
            },
            Some(pin) => cc_crypto_core::ServicePolicyTenant {
                kind: cc_crypto_core::ServiceTenantKind::PersonalAccount,
                id: pin.user_id.to_string(),
            },
        }
    }
    /// Caller obtains the genuine original signed policy from the ORIGINAL
    /// authenticated service peer (never UI JSON). Rows alone cannot supply it.
    /// The existing sticky block/floors must already have committed; output is
    /// fixed revoked=true and cannot grant/renew/resolve credentials.
    pub(super) async fn sign_blocked_original(
        self: &Arc<Self>,
        app: &AppCore,
        signed: &cc_crypto_core::SignedServiceDelegation,
    ) -> AppResult<cc_crypto_core::SignedServiceRevocation> {
        let p = signed.body();
        if p.owner_id != self.identity.device_id.to_string() || p.tenant != self.recorded_tenant() {
            return Err(invalid());
        }
        cc_crypto_core::SignedServiceDelegation::verify_json(
            &signed.to_json().map_err(|_| invalid())?,
            &self.identity.device_id.to_string(),
            &self.owner.identity().public_keys(),
            &self.recorded_tenant(),
            p.issued_at,
        )
        .map_err(|_| invalid())?;
        let id = Uuid::parse_str(&p.service_id).map_err(|_| invalid())?;
        let snapshot = self.read_stale_metadata(app).await?;
        let row = snapshot
            .records()
            .iter()
            .find(|r| r.service_id == id)
            .ok_or_else(invalid)?;
        if row.delivery != ServiceOwnerDelivery::RevokedLocally
            || row.policy_digest != signed.digest().map_err(|_| invalid())?
            || row.owner_epoch != p.owner_epoch
            || row.policy_revision < p.policy_revision
            || row.revocation_epoch <= p.revocation_epoch
        {
            return Err(invalid());
        }
        let (policy, revocation, floor) =
            (row.policy_revision, row.revocation_epoch, row.clock_floor);
        let u = self.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = app.hold_session(&self.owner).await?;
        let _unlock = self.owner.hold_unlocked(&u).await?;
        self.check(&u, false)?;
        // No await from original scope check through the purpose-specific denial.
        self.owner
            .identity()
            .sign_service_revocation(signed, policy, revocation, floor, now()?)
            .map_err(|_| invalid())
    }
}

/// One-use pending delivery, owned by the actual consumed PreparedReview path.
/// Opaque snapshot is retained; no ledger read can reconstruct this value.
pub(super) struct PendingDelivery {
    ledger: Arc<OwnerLedger>,
    snapshot: ServiceOwnerSnapshot,
    service: Uuid,
    delegation_digest: String,
    revision_id: Uuid,
}

impl PendingDelivery {
    /// Namespace comes from the rechecked persistent holder on the original
    /// authenticated ServicePeer, never request JSON or a Desktop UUID cast.
    pub(super) async fn before_send(
        app: &AppCore,
        confirmed: &ConfirmedServiceDelegation,
        namespace: &cc_automation_core::ServiceLocalNamespace,
    ) -> AppResult<Self> {
        let node_namespace = namespace.namespace.as_str();
        let t = &confirmed.template;
        if t.namespace.as_ref() != Some(namespace) {
            return Err(invalid());
        }
        {
            let u = t.unlock.upgrade().ok_or(AppError::VaultLocked)?;
            let _profile = app.hold_session(&t.owner).await?;
            let _unlock = t.owner.hold_unlocked(&u).await?;
            confirmed.original_held(&u)?;
        }
        let ledger = Arc::new(
            OwnerLedger::capture_scope(
                app,
                t.owner.clone(),
                t.unlock.clone(),
                Some(t.owner_scope.clone()),
            )
            .await?,
        );
        let p = confirmed.delegation.body();
        if p.owner_id != ledger.identity.device_id.to_string()
            || p.tenant != *t.owner_scope.tenant()
            || node_namespace == ledger.identity.profile_id.to_string()
        {
            return Err(invalid());
        }
        let snapshot = ledger.read(app).await?;
        let service = Uuid::parse_str(&p.service_id).map_err(|_| invalid())?;
        if snapshot.records().iter().any(|r| {
            r.service_id == service
                && (r.delivery == ServiceOwnerDelivery::RevokedLocally
                    || p.policy_revision <= r.policy_revision
                    || p.revocation_epoch < r.revocation_epoch
                    || p.issued_at < r.clock_floor)
        }) {
            return Err(invalid());
        }
        let delegation_digest = confirmed.delegation.digest().map_err(|_| invalid())?;
        let mut rows = snapshot.records().to_vec();
        rows.retain(|r| r.service_id != service);
        rows.push(ServiceOwnerRecord {
            service_id: service,
            node_local_profile: node_namespace.into(),
            owner_epoch: p.owner_epoch,
            policy_revision: p.policy_revision,
            revocation_epoch: p.revocation_epoch,
            clock_floor: p.issued_at,
            signer_fingerprint: digest(&hex::decode(&p.signer.public_key).map_err(|_| invalid())?),
            recipient_fingerprint: digest(
                &hex::decode(&p.recipient.public_key).map_err(|_| invalid())?,
            ),
            route_trust_digest: digest(
                &serde_json::to_vec(&(&p.route_inputs, &p.execution_origin))
                    .map_err(AppError::internal)?,
            ),
            policy_digest: delegation_digest.clone(),
            status_digest: digest(&confirmed.initial_status.to_json().map_err(|_| invalid())?),
            delivery: ServiceOwnerDelivery::DeliveryUnknown,
        });
        let schedule = super::schedules::Metadata::from_confirmed(confirmed)?;
        let snapshot = ledger
            .replace_impl(app, snapshot, rows, WriteKind::Fresh, schedule)
            .await?;
        Ok(Self {
            ledger,
            snapshot,
            service,
            delegation_digest,
            revision_id: t.snapshot.uploaded.revision_id,
        })
    }

    /// Called ONLY with the actual response from the original service_enroll
    /// request. Error/drop/lost ACK leaves durable DeliveryUnknown, no retry.
    pub(super) async fn acknowledge(
        self,
        app: &AppCore,
        response: &serde_json::Value,
    ) -> AppResult<()> {
        if response.get("enrolled") != Some(&serde_json::json!(true))
            || response.get("delegationDigest").and_then(|v| v.as_str())
                != Some(self.delegation_digest.as_str())
            || response.get("revisionId").and_then(|v| v.as_str())
                != Some(self.revision_id.to_string().as_str())
        {
            return Err(unknown());
        }
        let mut rows = self.snapshot.records().to_vec();
        let row = rows
            .iter_mut()
            .find(|r| r.service_id == self.service)
            .ok_or_else(unknown)?;
        if row.delivery != ServiceOwnerDelivery::DeliveryUnknown {
            return Err(unknown());
        }
        row.delivery = ServiceOwnerDelivery::Acknowledged;
        self.ledger
            .replace(app, self.snapshot, rows)
            .await
            .map_err(|_| unknown())?;
        Ok(())
    }
}

#[cfg(test)]
mod personal_tests;
#[cfg(test)]
mod tests;
