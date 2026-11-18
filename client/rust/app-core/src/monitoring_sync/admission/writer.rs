//! Native panel mutation admission for the local candidate floor.
//! A policy/version/checkpoint DTO ALONE is never a write permission.
use super::*;
use crate::session_provenance::{OriginalSessionAuth, OriginalSessionProvenance};
use cc_models::VaultObject;
use cc_storage_core::{LocalState, Tx};
use cc_sync_core::SyncError;

#[derive(Clone, Copy)]
pub(crate) enum PanelWritePolicy {
    Released,
    // SAME explicit owned fixture candidate used by existing MON07 Native tests.
    // No production enum variant, frontend field, environment override or floor.
    #[cfg(test)]
    ReviewedCandidate,
}
impl PanelWritePolicy {
    fn floor(self, client: &str) -> AppResult<ReleasedFloor> {
        let value = match self {
            Self::Released => RELEASED_PRESERVATION_FLOOR,
            #[cfg(test)]
            Self::ReviewedCandidate if client == "2.0.0" => Some("2.0.0"),
            #[cfg(test)]
            Self::ReviewedCandidate => None,
        };
        let floor = value
            .and_then(ReleasedFloor::parse)
            .ok_or_else(admission_required)?;
        if stable_version(client).map_err(|_| admission_required())? < floor.0 {
            return Err(admission_required());
        }
        Ok(floor)
    }
}
fn admission_required() -> crate::AppError {
    crate::AppError::invalid("monitoring_panels", "panel_sync_admission_required")
}
fn failed() -> SyncError {
    SyncError::Invalid("monitoring panel write busy or context changed".into())
}
fn pending() -> crate::AppError {
    crate::AppError::invalid("monitoring_sync", "checkpoint_recheck_required")
}

struct OriginalWrite {
    app: Weak<crate::app::Inner>,
    owner: Weak<Session>,
    unlock: Weak<Unlocked>,
    identity: Weak<DeviceIdentity>,
    auth: OriginalSessionAuth,
    provenance: OriginalSessionProvenance,
    profile: cc_storage_core::Profile,
    vault: VaultId,
    nonce: String,
    catalog: MonitoringCatalogCheckpoint,
    working_version: u64,
    policy: MonitoringAccountPolicy,
    until: Instant,
}
/// Single consumed mutation witness; no serde/public constructor/idle registry.
pub(crate) struct SettingsWriteAdmission(Arc<OriginalWrite>);
pub(crate) type PanelWriteAdmission = SettingsWriteAdmission;
impl OriginalWrite {
    fn live(&self, owner: &Arc<Session>, u: &Arc<Unlocked>, draft: bool) -> AppResult<()> {
        if Instant::now() >= self.until
            || !self.auth.is_current()
            || !self.provenance.is_current(owner)
            || *u.sharing_shutdown.borrow()
            || u.notices.scope_id().as_deref() != Some(self.nonce.as_str())
            || u.vault_id != self.vault
            || owner.profile_id != self.profile.profile_id
            || owner.device_id() != self.profile.device_id
            || !Weak::ptr_eq(&self.identity, &Arc::downgrade(&owner.identity()))
            || self
                .unlock
                .upgrade()
                .is_none_or(|old| !Arc::ptr_eq(&old, u))
            || self
                .owner
                .upgrade()
                .is_none_or(|old| !Arc::ptr_eq(&old, owner))
            || draft && u.working().version() != self.working_version
        {
            return Err(changed());
        }
        self.provenance.check_vault(u)?;
        Ok(())
    }
    fn run(&self, write: &mut dyn FnMut() -> Result<(), SyncError>) -> Result<(), SyncError> {
        // Never block a SQL worker on a holder that may itself await SQL.
        self.auth
            .try_with_current(|| {
                let app = self.app.upgrade().ok_or_else(changed)?;
                let owner = self.owner.upgrade().ok_or_else(changed)?;
                let u = self.unlock.upgrade().ok_or(crate::AppError::VaultLocked)?;
                self.provenance.try_with_current(&owner, || {
                    let active = app.active.try_lock().map_err(|_| changed())?;
                    if !active.as_ref().is_some_and(|old| Arc::ptr_eq(old, &owner)) {
                        return Err(changed());
                    }
                    owner.try_with_unlocked(&u, || {
                        self.live(&owner, &u, true)?;
                        write().map_err(crate::AppError::from)
                    })
                })
            })
            .map_err(|_| failed())
    }
    fn check_tx(&self, tx: &Tx<'_>) -> Result<(), SyncError> {
        if Instant::now() >= self.until || !self.auth.is_current() {
            return Err(failed());
        }
        let actual = tx.get_profile()?.ok_or_else(failed)?;
        if actual.kind != ProfileKind::Synced
            || actual.profile_id != self.profile.profile_id
            || actual.user_id != self.profile.user_id
            || actual.device_id != self.profile.device_id
            || actual.server_url != self.profile.server_url
        {
            return Err(failed());
        }
        let cursor = tx.get_sync_cursor(self.vault)?;
        if !cursor.snapshot_complete
            || cursor.recovery.is_some()
            || cursor.epoch != Some(self.policy.checkpoint().epoch())
            || cursor.last_sequence < self.policy.checkpoint().sequence()
            || tx.outbox_counts(self.vault)?.total() != 0
            || !tx.conflicted_objects(self.vault)?.is_empty()
        {
            return Err(failed());
        }
        let head = tx
            .get_object(self.vault, self.catalog.settings_id)?
            .ok_or_else(failed)?;
        if head.deleted
            || head.local_state != LocalState::Synced
            || head.revision != self.catalog.object_revision
            || head.server_revision != self.catalog.object_revision
        {
            return Err(failed());
        }
        Ok(())
    }
}
impl SettingsWriteAdmission {
    /// SAME account-wide preservation gate, original settings head and encrypted
    /// outbox writer. The legacy endpoint name does not restrict ciphertext type.
    pub(crate) async fn prepare_settings(
        app: &AppCore,
        owner: Arc<Session>,
        u: Arc<Unlocked>,
        expected_settings_revision: i64,
        choice: PanelWritePolicy,
        auth: OriginalSessionAuth,
        original_until: Instant,
    ) -> AppResult<Self> {
        let checkpoint = catalog_checkpoint(&u)?;
        if checkpoint.object_revision != expected_settings_revision {
            return Err(changed());
        }
        let until = original_until.min(Instant::now() + Duration::from_secs(10));
        let value =
            Self::prepare_original(app, owner, u, checkpoint.revision, choice, auth, until).await?;
        if value.0.catalog.object_revision != expected_settings_revision {
            return Err(changed());
        }
        Ok(value)
    }

    /// The caller passes the already captured ORIGINAL Session/Unlocked once.
    /// No profile lookup or automatic pull/opt-in/re-encryption happens here.
    pub(crate) async fn prepare(
        app: &AppCore,
        owner: Arc<Session>,
        u: Arc<Unlocked>,
        expected_catalog_revision: u64,
        choice: PanelWritePolicy,
    ) -> AppResult<Self> {
        let auth = OriginalSessionAuth::capture(app)?;
        let until = Instant::now() + Duration::from_secs(10);
        Self::prepare_original(
            app,
            owner,
            u,
            expected_catalog_revision,
            choice,
            auth,
            until,
        )
        .await
    }
    async fn prepare_original(
        app: &AppCore,
        owner: Arc<Session>,
        u: Arc<Unlocked>,
        expected_catalog_revision: u64,
        choice: PanelWritePolicy,
        auth: OriginalSessionAuth,
        until: Instant,
    ) -> AppResult<Self> {
        // None fails BEFORE any network read or material access in production.
        let api = owner.api()?;
        let floor = choice.floor(&api.config().client_version)?;
        let mut context = tokio::select! { biased;
            _=auth.cancelled()=>return Err(changed()),
            _=tokio::time::sleep_until(until.into())=>return Err(unavailable()),
            result=Context::capture_original(app, owner.clone(), u.clone())=>result?,
        };
        if context.catalog.revision != expected_catalog_revision {
            return Err(changed());
        }
        let nonce = u.notices.scope_id().ok_or_else(changed)?;
        let profile = tokio::select! { biased;
            _=auth.cancelled()=>return Err(changed()),
            _=context.shutdown.changed()=>return Err(crate::AppError::VaultLocked),
            _=tokio::time::sleep_until(until.into())=>return Err(unavailable()),
            result=owner.profile()=>result?,
        };
        drop(u);
        let provenance = tokio::select! { biased;
            _=auth.cancelled()=>return Err(changed()),
            _=context.shutdown.changed()=>return Err(crate::AppError::VaultLocked),
            _=tokio::time::sleep_until(until.into())=>return Err(unavailable()),
            result=OriginalSessionProvenance::capture(&owner, &profile, context.vault)=>result?,
        };
        let response = tokio::select! { biased;
            _=auth.cancelled()=>return Err(changed()),
            _=provenance.cancelled()=>return Err(changed()),
            _=context.shutdown.changed()=>return Err(crate::AppError::VaultLocked),
            _=tokio::time::sleep_until(until.into())=>return Err(unavailable()),
            result=api.monitoring_compatibility_once(context.vault)=>result.map_err(policy_error)?,
        };
        let minimum = response
            .minimum_client_version()
            .and_then(ReleasedFloor::parse)
            .ok_or_else(admission_required)?;
        if response.state() != MonitoringPolicyState::Enabled
            || response.revision() == 0
            || response.account_id() != context.account
            || response.checkpoint().vault_id() != context.vault
            || minimum.0 < floor.0
            || stable_version(&api.config().client_version).map_err(|_| admission_required())?
                < minimum.0
        {
            return Err(admission_required());
        }
        let u = context
            .unlock
            .upgrade()
            .ok_or(crate::AppError::VaultLocked)?;
        let value = Arc::new(OriginalWrite {
            app: Arc::downgrade(&app.inner),
            owner: Arc::downgrade(&owner),
            unlock: context.unlock.clone(),
            identity: context.identity.clone(),
            auth,
            provenance,
            profile,
            vault: context.vault,
            nonce,
            catalog: context.catalog,
            working_version: context.working_version,
            policy: response,
            until,
        });
        value.live(&owner, &u, true)?;
        // Read actual cursor/outbox/head now, then repeat INSIDE the mutation.
        let checked = value.clone();
        let storage = owner.storage()?;
        drop(u);
        tokio::select! { biased;
            _=value.auth.cancelled()=>return Err(changed()),
            _=value.provenance.cancelled()=>return Err(changed()),
            _=context.shutdown.changed()=>return Err(crate::AppError::VaultLocked),
            _=tokio::time::sleep_until(until.into())=>return Err(unavailable()),
            result=storage.read(move |tx| checked.check_tx(tx))=>result.map_err(|_| pending())?,
        }
        let u = value.unlock.upgrade().ok_or(crate::AppError::VaultLocked)?;
        value.live(&owner, &u, true)?;
        Ok(Self(value))
    }
    pub(crate) async fn original(&self, app: &AppCore) -> AppResult<(Arc<Session>, Arc<Unlocked>)> {
        if !Weak::ptr_eq(&self.0.app, &Arc::downgrade(&app.inner)) {
            return Err(changed());
        }
        let owner = self.0.owner.upgrade().ok_or_else(changed)?;
        let u = self
            .0
            .unlock
            .upgrade()
            .ok_or(crate::AppError::VaultLocked)?;
        tokio::select! { biased;
            _=self.0.auth.cancelled()=>return Err(changed()),
            _=self.0.provenance.cancelled()=>return Err(changed()),
            _=tokio::time::sleep_until(self.0.until.into())=>return Err(unavailable()),
            result=async {
                let _profile=app.hold_session(&owner).await?;
                let _unlock=owner.hold_unlocked(&u).await?;
                self.0.live(&owner,&u,true)
            }=>result?,
        }
        Ok((owner, u))
    }
    pub(crate) async fn commit<T>(
        self,
        app: &AppCore,
        object: VaultObject,
        finish: impl FnOnce(&Unlocked) -> AppResult<T>,
    ) -> AppResult<T> {
        self.commit_checked(app, object, |_| Ok(()), finish).await
    }
    pub(crate) async fn commit_checked<T>(
        self,
        app: &AppCore,
        object: VaultObject,
        extra: impl FnOnce(&Tx<'_>) -> Result<(), SyncError> + Send + 'static,
        finish: impl FnOnce(&Unlocked) -> AppResult<T>,
    ) -> AppResult<T> {
        let (owner, u) = self.original(app).await?;
        if !matches!(&object, VaultObject::VaultSettings(_))
            || object.id() != self.0.catalog.settings_id
        {
            return Err(changed());
        }
        let writer = u.writer.clone();
        drop(u);
        drop(owner);
        let guard = self.0.clone();
        let check = self.0.clone();
        writer
            .put_checked(
                object,
                move |write| guard.run(write),
                move |tx| {
                    check.check_tx(tx)?;
                    extra(tx)
                },
            )
            .await?;
        let owner = self.0.owner.upgrade().ok_or_else(changed)?;
        let u = self
            .0
            .unlock
            .upgrade()
            .ok_or(crate::AppError::VaultLocked)?;
        // The actual mutation/readback has JOINED above. If its result cannot
        // be published under the original scope, fail without claiming rollback
        // or automatically reissuing the write.
        tokio::select! { biased;
            _=self.0.auth.cancelled()=>Err(changed()),
            _=self.0.provenance.cancelled()=>Err(changed()),
            _=tokio::time::sleep_until(self.0.until.into())=>Err(unavailable()),
            result=async {
                let _profile=app.hold_session(&owner).await?;
                let _unlock=owner.hold_unlocked(&u).await?;
                self.0.auth.try_with_current(||self.0.provenance.try_with_current(&owner,|| {
                    self.0.live(&owner,&u,false)?;
                    finish(&u)
                }))
            }=>result,
        }
    }
}
#[cfg(test)]
mod tests;
