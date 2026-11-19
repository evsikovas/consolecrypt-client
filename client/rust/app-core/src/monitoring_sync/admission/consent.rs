//! Original-owner policy metadata, never panel writer admission.
use super::*;
use crate::session_provenance::{OriginalSessionAuth, OriginalSessionProvenance};
use crate::settings_review_runtime::{SettingsReviewRuntime, OwnedSettingsReviewRegistration};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    version: u8,
    profile_id: String,
    notification_scope_id: String,
}
fn request(raw: &str) -> AppResult<Request> {
    if raw.len() > 1024 {
        return Err(changed());
    }
    let r: Request = serde_json::from_str(raw).map_err(|_| changed())?;
    if r.version != 1
        || !uuid::Uuid::parse_str(&r.profile_id)
            .is_ok_and(|v| !v.is_nil() && v.to_string() == r.profile_id)
        || r.notification_scope_id.is_empty()
        || r.notification_scope_id.len() > 256
        || r.notification_scope_id.chars().any(char::is_control)
    {
        return Err(changed());
    }
    Ok(r)
}
fn scope(context: &Context, u: &Unlocked, nonce: &str) -> AppResult<()> {
    if u.notices.scope_id().as_deref() != Some(nonce) {
        return Err(changed());
    }
    if context
        .unlock
        .upgrade()
        .is_none_or(|original| !std::ptr::eq(original.as_ref(), u))
    {
        return Err(changed());
    }
    Ok(())
}
// Only ORIGINAL weak owners survive the prepare call. Neither an AppCore
// handle nor a Session Arc can indirectly keep an unlocked vault alive here.
#[derive(Clone)]
struct DetachedContext {
    owner: Weak<Session>,
    profile_id: String,
    unlock: Weak<Unlocked>,
    identity: Weak<DeviceIdentity>,
    device: DeviceId,
    account: UserId,
    vault: VaultId,
    server: String,
    working_version: u64,
    catalog: MonitoringCatalogCheckpoint,
    shutdown: tokio::sync::watch::Receiver<bool>,
    expires: Instant,
}
impl DetachedContext {
    fn detach(c: Context) -> Self {
        Self {
            owner: Arc::downgrade(&c.owner),
            profile_id: c.owner.profile_id.to_string(),
            unlock: c.unlock,
            identity: c.identity,
            device: c.device,
            account: c.account,
            vault: c.vault,
            server: c.server,
            working_version: c.working_version,
            catalog: c.catalog,
            shutdown: c.shutdown,
            expires: c.expires,
        }
    }
    fn original(&self) -> AppResult<Context> {
        let owner = self.owner.upgrade().ok_or_else(changed)?;
        // Same ORIGINAL Session only. Its API is checked against the captured
        // provenance's exact client identity before any I/O/publication; a
        // replacement API fails that fence. Retain no ApiClient/request signer
        // in the idle handle after the originating Core is dropped.
        let api = owner.api()?;
        Ok(Context {
            owner,
            unlock: self.unlock.clone(),
            identity: self.identity.clone(),
            device: self.device,
            account: self.account,
            vault: self.vault,
            server: self.server.clone(),
            api,
            working_version: self.working_version,
            catalog: self.catalog.clone(),
            shutdown: self.shutdown.clone(),
            expires: self.expires,
        })
    }
}
struct Review {
    context: DetachedContext,
    policy: MonitoringAccountPolicy,
    auth: OriginalSessionAuth,
    provenance: OriginalSessionProvenance,
    nonce: String,
    readiness: MonitoringSyncReadiness,
    _registration: OwnedSettingsReviewRegistration,
}
fn final_check(
    c: &Context,
    u: &Arc<Unlocked>,
    auth: &OriginalSessionAuth,
    provenance: &OriginalSessionProvenance,
    nonce: &str,
    cancel: &CancellationToken,
    catalog_too: bool,
) -> AppResult<()> {
    if cancel.is_cancelled()
        || Instant::now() >= c.expires
        || *c.shutdown.borrow()
        || !auth.is_current()
        || !provenance.is_current(&c.owner)
        || c.owner.device_id() != c.device
        || c.owner.require_vault_id()? != c.vault
        || u.vault_id != c.vault
        || !Weak::ptr_eq(&c.identity, &Arc::downgrade(&c.owner.identity()))
    {
        return Err(changed());
    }
    scope(c, u, nonce)?;
    provenance.check_vault(u)?;
    if catalog_too
        && (u.working().version() != c.working_version
            || catalog_checkpoint(u)? != c.catalog
            || u.working().version() != c.working_version)
    {
        return Err(changed());
    }
    if cancel.is_cancelled() || Instant::now() >= c.expires || *c.shutdown.borrow() {
        return Err(changed());
    }
    Ok(())
}
impl Review {
    fn check(
        &self,
        c: &Context,
        u: &Arc<Unlocked>,
        cancel: &CancellationToken,
        catalog_too: bool,
    ) -> AppResult<()> {
        if !self
            .context
            .owner
            .upgrade()
            .is_some_and(|original| Arc::ptr_eq(&original, &c.owner))
        {
            return Err(changed());
        }
        final_check(
            c,
            u,
            &self.auth,
            &self.provenance,
            &self.nonce,
            cancel,
            catalog_too,
        )
    }
    fn details(&self) -> Value {
        let c = &self.context;
        let p = &self.policy;
        let floor = RELEASED_PRESERVATION_FLOOR.and_then(ReleasedFloor::parse);
        let can_confirm = floor.is_some()
            && self.readiness.state == MonitoringSyncState::ServerAdmissionUnverified
            && p.state() == MonitoringPolicyState::Off
            && floor.as_ref().is_some_and(|floor| {
                p.offer_minimum_client_version() == Some(floor.0.to_string().as_str())
            });
        let can_recheck = p.state() == MonitoringPolicyState::Enabled;
        let state = match p.state() {
            MonitoringPolicyState::Off => "off",
            MonitoringPolicyState::Enabled => "enabled",
            MonitoringPolicyState::Quarantined => "quarantined",
        };
        json!({"version":1,"profileId":c.profile_id,"notificationScopeId":self.nonce,
            "kind":if can_confirm {"ownerConsent"} else if can_recheck {"checkpointRecapture"} else {"blocked"},
            "scope":"allPersonalVaults","accountId":c.account.to_string(),"accountState":state,
            "policyRevision":p.revision().to_string(),"rosterDigest":p.roster_digest(),
            "releasedMinimumClientVersion":floor.map(|v|v.0.to_string()),"offeredMinimumClientVersion":p.offer_minimum_client_version(),"minimumClientVersion":p.minimum_client_version(),
            "checkpoint":{"vaultId":p.checkpoint().vault_id().to_string(),"vaultEpoch":p.checkpoint().epoch().to_string(),"vaultSequence":p.checkpoint().sequence().to_string()},
            "catalog":{"settingsId":c.catalog.settings_id.to_string(),"objectRevision":c.catalog.object_revision.to_string(),"schema":c.catalog.schema,"revision":c.catalog.revision.to_string()},
            "readiness":self.readiness,"remainingMs":c.expires.saturating_duration_since(Instant::now()).as_millis().min(300_000) as u64,
            "canConfirm":can_confirm,"canRecheck":can_recheck})
    }
}
struct Control {
    review: Mutex<Option<Review>>,
    cancel: CancellationToken,
}
impl Control {
    fn arm(
        self: &Arc<Self>,
        expires: Instant,
        auth: OriginalSessionAuth,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) {
        let weak = Arc::downgrade(self);
        let cancel = self.cancel.clone();
        tokio::spawn(async move {
            tokio::select! { biased;
                _=cancel.cancelled()=>{}, _=auth.cancelled()=>{}, _=shutdown.changed()=>{}, _=tokio::time::sleep_until(expires.into())=>{},
            }
            if let Some(c) = weak.upgrade() {
                c.retire();
            }
        });
    }
    fn retire(&self) {
        self.cancel.cancel();
        self.review.lock().unwrap_or_else(|e| e.into_inner()).take();
    }
    fn take(&self) -> AppResult<Review> {
        if self.cancel.is_cancelled() {
            return Err(changed());
        }
        self.review
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .ok_or_else(changed)
    }
}
/// An opaque, original-owner, one-use local review. No keys/approval JSON.
/// Cancellation retires the SAME runtime lease; it does not opt out the account.
pub struct MonitoringSettingsConsent {
    core: Weak<crate::app::Inner>,
    control: Arc<Control>,
}
impl Drop for MonitoringSettingsConsent {
    fn drop(&mut self) {
        self.control.retire();
    }
}
impl MonitoringSettingsConsent {
    fn app(&self) -> AppResult<AppCore> {
        Ok(AppCore {
            inner: self.core.upgrade().ok_or_else(changed)?,
        })
    }
    pub fn cancel(&self) {
        self.control.retire();
    }
    pub async fn details_json(&self) -> AppResult<String> {
        let app = self.app()?;
        let context = self
            .control
            .review
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .ok_or_else(changed)?
            .context
            .original()?;
        let u = context
            .unlock
            .upgrade()
            .ok_or(crate::AppError::VaultLocked)?;
        let _profile = app.hold_session(&context.owner).await?;
        let _unlock = context.owner.hold_unlocked(&u).await?;
        context.check_held(&u, true).await?;
        let guard = self
            .control
            .review
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let review = guard.as_ref().ok_or_else(changed)?;
        review.auth.with_current(|| {
            review.provenance.with_current(&context.owner, || {
                review.check(&context, &u, &self.control.cancel, true)?;
                let details = review.details();
                if details["remainingMs"].as_u64().is_none_or(|ms| ms == 0) {
                    return Err(changed());
                }
                let encoded = serde_json::to_string(&details).map_err(crate::AppError::internal)?;
                review.check(&context, &u, &self.control.cancel, true)?;
                Ok(encoded)
            })
        })
    }
    pub async fn confirm(&self) -> AppResult<String> {
        let app = self.app()?;
        let review = self.control.take()?;
        // Native release metadata None means ZERO key proof/PUT.
        let floor = RELEASED_PRESERVATION_FLOOR
            .and_then(ReleasedFloor::parse)
            .ok_or_else(|| {
                crate::AppError::invalid("monitoring_sync", "preservation_floor_undeclared")
            })?;
        if review.readiness.state != MonitoringSyncState::ServerAdmissionUnverified
            || review.policy.state() != MonitoringPolicyState::Off
            || review.policy.offer_minimum_client_version() != Some(floor.0.to_string().as_str())
        {
            return Err(changed());
        }
        let mut c = review.context.original()?;
        let key = {
            let u = c.unlock.upgrade().ok_or(crate::AppError::VaultLocked)?;
            let _profile = app.hold_session(&c.owner).await?;
            let _unlock = c.owner.hold_unlocked(&u).await?;
            c.check_held(&u, true).await?;
            review.auth.with_current(|| {
                review.provenance.with_current(&c.owner, || {
                    review.check(&c, &u, &self.control.cancel, true)?;
                    Ok(u.vault()?.vault_access_key())
                })
            })?
        };
        let response = tokio::select! {biased;
            _=self.control.cancel.cancelled()=>return Err(changed()),_=review.auth.cancelled()=>return Err(changed()),
            _=c.shutdown.changed()=>return Err(changed()),_=tokio::time::sleep_until(c.expires.into())=>return Err(changed()),
            value=tokio::time::timeout(Duration::from_secs(10),c.api.confirm_monitoring_compatibility(&review.policy,key))=>value,
        };
        let policy = response.map_err(|_| unavailable())?.map_err(policy_error)?;
        let u = c.unlock.upgrade().ok_or(crate::AppError::VaultLocked)?;
        let _profile = app.hold_session(&c.owner).await?;
        let _unlock = c.owner.hold_unlocked(&u).await?;
        c.check_held(&u, true).await?;
        review.auth.with_current(||review.provenance.with_current(&c.owner,||{
            review.check(&c,&u,&self.control.cancel,true)?;
            let encoded=serde_json::to_string(&json!({"version":1,"profileId":c.owner.profile_id.to_string(),"notificationScopeId":review.nonce,
                "scope":"allPersonalVaults","action":"accountPolicyConfirmed","accountId":policy.account_id().to_string(),"policyRevision":policy.revision().to_string()})).map_err(crate::AppError::internal)?;
            review.check(&c,&u,&self.control.cancel,true)?;Ok(encoded)
        }))
    }
    pub async fn recheck_checkpoint(&self) -> AppResult<String> {
        let app = self.app()?;
        let review = self.control.take()?;
        let c = review.context.original()?;
        {
            let u = c.unlock.upgrade().ok_or(crate::AppError::VaultLocked)?;
            let _profile = app.hold_session(&c.owner).await?;
            let _unlock = c.owner.hold_unlocked(&u).await?;
            c.check_held(&u, true).await?;
            review.auth.with_current(|| {
                review.provenance.with_current(&c.owner, || {
                    review.check(&c, &u, &self.control.cancel, true)
                })
            })?;
        }
        let Review {
            policy,
            auth,
            provenance,
            nonce,
            _registration,
            ..
        } = review;
        let observed = MonitoringPolicyReview {
            context: c.clone(),
            policy,
        };
        let result = tokio::select! {biased;
            _=self.control.cancel.cancelled()=>return Err(changed()),_=auth.cancelled()=>return Err(changed()),
            _=tokio::time::sleep_until(c.expires.into())=>return Err(changed()),
            value=app.monitoring_settings_checkpoint_recapture(observed)=>value?,
        };
        let u = c.unlock.upgrade().ok_or(crate::AppError::VaultLocked)?;
        let _profile = app.hold_session(&c.owner).await?;
        let _unlock = c.owner.hold_unlocked(&u).await?;
        c.check_held(&u, false).await?;
        auth.with_current(||provenance.with_current(&c.owner,||{
            // Legitimate ciphertext reload changed catalog: do NOT compare old
            // catalog here. Identity/nonce/deadline/cancel remain original.
            final_check(&c,&u,&auth,&provenance,&nonce,&self.control.cancel,false)?;
            let encoded=serde_json::to_string(&json!({"version":1,"profileId":c.owner.profile_id.to_string(),"notificationScopeId":nonce,
                "scope":"allPersonalVaults","action":"checkpointRecaptured","accountId":result.policy.account_id().to_string(),"policyRevision":result.policy.revision().to_string(),
                "checkpoint":{"vaultId":result.policy.checkpoint().vault_id().to_string(),"vaultEpoch":result.policy.checkpoint().epoch().to_string(),"vaultSequence":result.policy.checkpoint().sequence().to_string()},
                "catalog":{"settingsId":result.catalog.settings_id.to_string(),"objectRevision":result.catalog.object_revision.to_string(),"schema":result.catalog.schema,"revision":result.catalog.revision.to_string()}})).map_err(crate::AppError::internal)?;
            final_check(&c,&u,&auth,&provenance,&nonce,&self.control.cancel,false)?;Ok(encoded)
        }))
    }
}
async fn check_original(
    app: &AppCore,
    context: &Context,
    auth: &OriginalSessionAuth,
    provenance: &OriginalSessionProvenance,
    nonce: &str,
    cancel: &CancellationToken,
) -> AppResult<()> {
    let u = context
        .unlock
        .upgrade()
        .ok_or(crate::AppError::VaultLocked)?;
    let _profile = app.hold_session(&context.owner).await?;
    let _unlock = context.owner.hold_unlocked(&u).await?;
    context.check_held(&u, true).await?;
    auth.with_current(|| {
        provenance.with_current(&context.owner, || {
            final_check(context, &u, auth, provenance, nonce, cancel, true)
        })
    })
}
impl AppCore {
    pub async fn monitoring_settings_prepare_consent_json(
        &self,
        raw: &str,
    ) -> AppResult<MonitoringSettingsConsent> {
        let input = request(raw)?;
        let auth = OriginalSessionAuth::capture(self)?;
        let (owner, u) = self.unlocked().await?;
        if owner.profile_id.to_string() != input.profile_id
            || u.notices.scope_id().as_deref() != Some(input.notification_scope_id.as_str())
        {
            return Err(changed());
        }
        let weak = Arc::downgrade(&u);
        let (registration, cancel) = SettingsReviewRuntime::begin_owned(weak)?;
        let mut context = Context::capture_original(self, owner, u).await?;
        let profile = context.owner.profile().await?;
        let provenance = tokio::select! { biased;
            _=cancel.cancelled()=>return Err(changed()),
            _=auth.cancelled()=>return Err(changed()),
            _=context.shutdown.changed()=>return Err(changed()),
            value=tokio::time::timeout(Duration::from_secs(30),OriginalSessionProvenance::capture(&context.owner,&profile,context.vault))=>value.map_err(|_|unavailable())??,
        };
        if provenance.personal_user_id() != Some(context.account) {
            return Err(changed());
        }
        check_original(
            self,
            &context,
            &auth,
            &provenance,
            &input.notification_scope_id,
            &cancel,
        )
        .await?;
        let response = tokio::select! { biased;
            _=cancel.cancelled()=>return Err(changed()),
            _=auth.cancelled()=>return Err(changed()),
            _=context.shutdown.changed()=>return Err(changed()),
            value=tokio::time::timeout(Duration::from_secs(10),context.api.monitoring_compatibility_once(context.vault))=>value,
        };
        let policy = response.map_err(|_| unavailable())?.map_err(policy_error)?;
        if policy.account_id() != context.account || policy.checkpoint().vault_id() != context.vault
        {
            return Err(changed());
        }
        let readiness = match RELEASED_PRESERVATION_FLOOR.and_then(ReleasedFloor::parse) {
            None => MonitoringSyncReadiness::state(MonitoringSyncState::FloorUndeclared),
            Some(floor) => {
                check_original(
                    self,
                    &context,
                    &auth,
                    &provenance,
                    &input.notification_scope_id,
                    &cancel,
                )
                .await?;
                let response = tokio::select! { biased;
                    _=cancel.cancelled()=>return Err(changed()),
                    _=auth.cancelled()=>return Err(changed()),
                    _=context.shutdown.changed()=>return Err(changed()),
                    _=tokio::time::sleep_until(context.expires.into())=>return Err(changed()),
                    value=tokio::time::timeout(Duration::from_secs(10),context.api.list_devices())=>value,
                };
                match response {
                    Ok(Ok(r)) => evaluate(&r.devices, context.device, &floor),
                    _ => MonitoringSyncReadiness::state(MonitoringSyncState::RosterUnavailable),
                }
            }
        };
        let expires = context.expires;
        let u = context
            .unlock
            .upgrade()
            .ok_or(crate::AppError::VaultLocked)?;
        let _profile = self.hold_session(&context.owner).await?;
        let _unlock = context.owner.hold_unlocked(&u).await?;
        context.check_held(&u, true).await?;
        let review = Review {
            context: DetachedContext::detach(context.clone()),
            policy,
            auth: auth.clone(),
            provenance,
            nonce: input.notification_scope_id,
            readiness,
            _registration: registration,
        };
        review.check(&context, &u, &cancel, true)?;
        let control = Arc::new(Control {
            review: Mutex::new(Some(review)),
            cancel: cancel.clone(),
        });
        control.arm(expires, auth, context.shutdown.clone());
        Ok(MonitoringSettingsConsent {
            core: Arc::downgrade(&self.inner),
            control,
        })
    }
}
#[cfg(test)]
mod tests;
