//! Real original Session/unlock/device/auth capture, independent of Reader grants.
use crate::agent_reader::{OriginalSessionAuth, OriginalSessionProvenance};
use crate::{
    AppCore, AppError, AppResult,
    session::{Session, Unlocked},
};
use cc_agent_node_wire::{
    Now, OwnerPin, Tenant, Timestamp, TunnelClientSubject, TunnelRoutePolicy,
};
use cc_vault_core::DeviceIdentity;
use std::{
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
fn denied() -> AppError {
    AppError::invalid("agent_tunnel", "original_session_changed")
}
/// Stable metadata only. Captured authenticated account is never inferred
/// from a caller ID, an event, a username or a newly selected ApiClient.
#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(super) enum OriginalOwnerContext {
    LocalProfile {
        profile_id: cc_storage_core::ProfileId,
    },
    PersonalAccount {
        user_id: cc_protocol::UserId,
    },
}
pub(super) struct OriginalSessionDeviceIdentity {
    owner: Weak<Session>,
    unlock: Weak<Unlocked>,
    identity: Weak<DeviceIdentity>,
    provenance: OriginalSessionProvenance,
    auth: OriginalSessionAuth,
    generation: u64,
    working_version: u64,
    profile: String,
    expected_notice: Option<String>,
    cancel: CancellationToken,
    retired: CancellationToken,
    shutdown: watch::Receiver<bool>,
    deadline: Instant,
    wall: Mutex<Option<Duration>>,
}
impl std::fmt::Debug for OriginalSessionDeviceIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OriginalSessionDeviceIdentity([redacted])")
    }
}
impl Drop for OriginalSessionDeviceIdentity {
    fn drop(&mut self) {
        self.retired.cancel();
    }
}
impl OriginalSessionDeviceIdentity {
    /// Borrow only the actual captured owner/unlock; never recapture its TTL.
    pub(crate) fn matches_monitoring_owner(&self, owner: &Arc<Session>, unlock: &Arc<Unlocked>) -> AppResult<bool> {
        self.check_current()?;
        Ok(Weak::ptr_eq(&self.owner, &Arc::downgrade(owner))
            && Weak::ptr_eq(&self.unlock, &Arc::downgrade(unlock)))
    }
    /// Compare two live captures of the same actual owner, not their public IDs.
    /// The newer capture never replaces this operation's original deadline.
    pub(crate) fn same_current_capture(&self, other: &Self) -> AppResult<bool> {
        self.check_current()?;
        other.check_current()?;
        Ok(Weak::ptr_eq(&self.owner, &other.owner)
            && Weak::ptr_eq(&self.unlock, &other.unlock)
            && Weak::ptr_eq(&self.identity, &other.identity)
            && self.generation == other.generation
            && self.working_version == other.working_version
            && self.profile == other.profile
            && self.expected_notice == other.expected_notice)
    }

    pub async fn capture(
        app: &AppCore,
        cancel: &CancellationToken,
        ttl: Duration,
    ) -> AppResult<Self> {
        Self::capture_inner(app, cancel, ttl, None).await
    }
    /// Public caller intent is checked on the FIRST captured owner, before
    /// account/SSH I/O. No current-profile lookup is repeated after this capture.
    pub(super) async fn capture_expected(
        app: &AppCore,
        cancel: &CancellationToken,
        ttl: Duration,
        profile: &str,
        nonce: &str,
    ) -> AppResult<Self> {
        if !uuid::Uuid::parse_str(profile).is_ok_and(|id| !id.is_nil() && id.to_string() == profile)
            || nonce.is_empty()
            || nonce.len() > 256
            || nonce.chars().any(char::is_control)
        {
            return Err(denied());
        }
        Self::capture_inner(app, cancel, ttl, Some((profile, nonce))).await
    }
    async fn capture_inner(
        app: &AppCore,
        cancel: &CancellationToken,
        ttl: Duration,
        expected: Option<(&str, &str)>,
    ) -> AppResult<Self> {
        if ttl.is_zero() || ttl > Duration::from_secs(300) {
            return Err(denied());
        }
        let deadline = Instant::now() + ttl;
        let auth = OriginalSessionAuth::capture(app)?;
        let acquire = async {
            let owner = app.session().await?;
            let profile = owner.profile().await?;
            if expected.is_some_and(|(id, _)| owner.profile_id.to_string() != id) {
                return Err(denied());
            }
            let u = owner.unlocked().await?;
            if expected.is_some_and(|(_, nonce)| u.notices.scope_id().as_deref() != Some(nonce)) {
                return Err(denied());
            }
            let vault = u.vault_id;
            let generation = owner.automation.unlocked_generation()?;
            let weak = Arc::downgrade(&u);
            let shutdown = u.sharing_shutdown.subscribe();
            drop(u);
            let provenance = OriginalSessionProvenance::capture(&owner, &profile, vault).await?;
            let u = weak.upgrade().ok_or_else(denied)?;
            let _active = app.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&u).await?;
            let identity = owner.identity();
            provenance.check_vault(&u)?;
            if owner.automation.unlocked_generation()? != generation
                || !auth.is_current()
                || !provenance.is_current(&owner)
            {
                return Err(denied());
            }
            let result = Self {
                owner: Arc::downgrade(&owner),
                unlock: Arc::downgrade(&u),
                identity: Arc::downgrade(&identity),
                provenance,
                auth: auth.clone(),
                generation,
                working_version: u.working().version(),
                profile: owner.profile_id.to_string(),
                expected_notice: expected.map(|(_, nonce)| nonce.to_owned()),
                cancel: cancel.clone(),
                retired: CancellationToken::new(),
                shutdown,
                deadline,
                wall: Mutex::new(None),
            };
            result.check_current()?;
            Ok(result)
        };
        tokio::select! {biased;_=cancel.cancelled()=>Err(denied()),_=auth.cancelled()=>Err(denied()),_=tokio::time::sleep_until(tokio::time::Instant::from_std(deadline))=>Err(denied()),result=acquire=>result}
    }
    pub fn check_current(&self) -> AppResult<()> {
        let check = || {
            if self.cancel.is_cancelled()
                || self.retired.is_cancelled()
                || Instant::now() >= self.deadline
                || *self.shutdown.borrow()
                || !self.auth.is_current()
            {
                return Err(denied());
            }
            let owner = self.owner.upgrade().ok_or_else(denied)?;
            let u = self.unlock.upgrade().ok_or_else(denied)?;
            let identity = self.identity.upgrade().ok_or_else(denied)?;
            if owner.automation.stopped.is_cancelled()
                || owner.automation.unlocked_generation()? != self.generation
                || !Arc::ptr_eq(&identity, &owner.identity())
                || owner.profile_id.to_string() != self.profile
                || self
                    .expected_notice
                    .as_ref()
                    .is_some_and(|nonce| u.notices.scope_id().as_deref() != Some(nonce.as_str()))
                || u.working().version() != self.working_version
                || !self.provenance.is_current(&owner)
            {
                return Err(denied());
            }
            self.provenance.check_vault(&u)
        };
        let result = check();
        if result.is_err() {
            self.retired.cancel();
        }
        result
    }
    pub(super) fn owner_context(&self) -> AppResult<OriginalOwnerContext> {
        self.check_current()?;
        let value = match self.provenance.personal_user_id() {
            Some(user_id) => OriginalOwnerContext::PersonalAccount { user_id },
            None => OriginalOwnerContext::LocalProfile {
                profile_id: self.owner.upgrade().ok_or_else(denied)?.profile_id,
            },
        };
        self.check_current()?;
        Ok(value)
    }
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
    /// Same original auth/provenance/version guards through ONE synchronous
    /// nonblocking transport poll. All upgraded owners and guards are dropped
    /// before returning Pending; this captures no new scope or authority.
    pub fn with_io_current<T>(&self, poll: impl FnOnce(Duration) -> AppResult<T>) -> AppResult<T> {
        let owner = self.owner.upgrade().ok_or_else(denied)?;
        let u = self.unlock.upgrade().ok_or_else(denied)?;
        let identity = self.identity.upgrade().ok_or_else(denied)?;
        let result = self.auth.with_current(|| {
            self.provenance.with_current(&owner, || {
                u.working()
                    .tunnel_io_at_version(self.working_version, || {
                        // Avoid recursively acquiring WorkingSet::read while a writer
                        // is queued; version equality is held by the outer read guard.
                        if self.cancel.is_cancelled()
                            || self.retired.is_cancelled()
                            || Instant::now() >= self.deadline
                            || *self.shutdown.borrow()
                            || !self.auth.is_current()
                            || owner.automation.stopped.is_cancelled()
                            || owner.automation.unlocked_generation()? != self.generation
                            || !Arc::ptr_eq(&identity, &owner.identity())
                            || owner.profile_id.to_string() != self.profile
                            || self.expected_notice.as_ref().is_some_and(|nonce| {
                                u.notices.scope_id().as_deref() != Some(nonce.as_str())
                            })
                            || !self.provenance.is_current(&owner)
                        {
                            return Err(denied());
                        }
                        self.provenance.check_vault(&u)?;
                        poll(self.sample_wall_held()?)
                    })
                    .unwrap_or_else(|| Err(denied()))
            })
        });
        if result.is_err() {
            self.retired.cancel();
        }
        result
    }
    /// Closed Relay API SETUP only, not TLS/DATA/SSH socket polling.
    /// Keep original Native AppAuth/unlock/version/vault fences; API internal
    /// subject.with_current remains the sole ReaderAuthState mutex owner.
    pub(super) fn with_api_setup_current<T>(&self, poll: impl FnOnce(Duration) -> AppResult<T>) -> AppResult<T> {
        let owner = self.owner.upgrade().ok_or_else(denied)?;
        let u = self.unlock.upgrade().ok_or_else(denied)?;
        let identity = self.identity.upgrade().ok_or_else(denied)?;
        let result = self.auth.with_current(|| {
            // API setup takes its OWN original ReaderAuthState capture/proof
            // mutex below. Never hold that same Personal mutex around its poll.
            if !self.provenance.is_current(&owner) { return Err(denied()); }
            let result = u.working()
                    .tunnel_io_at_version(self.working_version, || {
                        // Avoid recursively acquiring WorkingSet::read while a writer
                        // is queued; version equality is held by the outer read guard.
                        if self.cancel.is_cancelled()
                            || self.retired.is_cancelled()
                            || Instant::now() >= self.deadline
                            || *self.shutdown.borrow()
                            || !self.auth.is_current()
                            || owner.automation.stopped.is_cancelled()
                            || owner.automation.unlocked_generation()? != self.generation
                            || !Arc::ptr_eq(&identity, &owner.identity())
                            || owner.profile_id.to_string() != self.profile
                            || self.expected_notice.as_ref().is_some_and(|nonce| {
                                u.notices.scope_id().as_deref() != Some(nonce.as_str())
                            })
                            || !self.provenance.is_current(&owner)
                        {
                            return Err(denied());
                        }
                        self.provenance.check_vault(&u)?;
                        let result = poll(self.sample_wall_held()?);
                        // Only atomics here: the maintained API owns its actual
                        // subject.with_current/capture locks, without recursion.
                        if !self.provenance.is_current(&owner) { return Err(denied()); }
                        result
                    })
                    .unwrap_or_else(|| Err(denied()));
            if !self.provenance.is_current(&owner) { return Err(denied()); }
            result
        });
        if result.is_err() {
            self.retired.cancel();
        }
        result
    }
    fn sample_wall(&self) -> AppResult<Duration> {
        self.check_current()?;
        self.sample_wall_held()
    }
    fn sample_wall_held(&self) -> AppResult<Duration> {
        // Serialize clock sampling with publication: concurrent callers must not
        // mistake an earlier sample published later for a real clock rollback.
        let mut high = self.wall.lock().map_err(|_| denied())?;
        let raw = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| {
            self.retired.cancel();
            denied()
        })?;
        if raw.as_secs() > 253_402_300_799 || high.is_some_and(|old| raw < old) {
            self.retired.cancel();
            return Err(denied());
        }
        *high = Some(raw);
        Ok(raw)
    }
    pub fn now(&self) -> AppResult<Now> {
        Ok(Now(
            Timestamp::new(self.sample_wall()?.as_secs()).map_err(|_| denied())?
        ))
    }
    pub fn remaining_until(&self, expiry: Timestamp) -> AppResult<Duration> {
        Duration::from_secs(expiry.seconds())
            .checked_sub(self.sample_wall()?)
            .filter(|v| !v.is_zero())
            .ok_or_else(denied)
    }
    pub fn bind_relay(&self, relay: &cc_sync_core::api::RelayBinaryStream) -> AppResult<()> {
        self.check_current()?;
        let owner = self.owner.upgrade().ok_or_else(denied)?;
        let api = owner.api_opt().ok_or_else(denied)?;
        relay.check_original(&api).map_err(|_| denied())?;
        if self.provenance.personal_user_id() != Some(relay.original_user_id())
            || owner.device_id() != relay.original_device_id()
        {
            return Err(denied());
        }
        Ok(())
    }
    pub fn bind_owner_pin(&self, pin: &OwnerPin) -> AppResult<()> {
        self.check_current()?;
        let id = self.identity.upgrade().ok_or_else(denied)?;
        cc_agent_node_wire::validate_pin(pin, self.now()?).map_err(|_| denied())?;
        if *pin.key.as_bytes() != id.public_keys().signing {
            return Err(denied());
        }
        match (self.provenance.personal_user_id(), &pin.context.tenant) {
            (None, Tenant::LocalProfile { .. }) => Ok(()),
            (Some(actual), Tenant::PersonalAccount { id })
                if serde_json::to_value(id)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    == Some(actual.to_string()) =>
            {
                Ok(())
            }
            _ => Err(denied()),
        }
    }
    pub fn bind_owner_policy(&self, pin: &OwnerPin, policy: &TunnelRoutePolicy) -> AppResult<()> {
        self.bind_owner_pin(pin)?;
        let identity = self.identity.upgrade().ok_or_else(denied)?;
        if *pin.key.as_bytes() != identity.public_keys().signing
            || policy.context != pin.context
            || policy.client_device_id != identity.device_id().to_string()
        {
            return Err(denied());
        }
        match (
            self.provenance.personal_user_id(),
            &policy.client_subject,
            &pin.context.tenant,
        ) {
            (None, TunnelClientSubject::LocalProfile { id }, Tenant::LocalProfile { .. })
                if id == &self.profile => {}
            (
                Some(actual),
                TunnelClientSubject::PersonalAccount { id },
                Tenant::PersonalAccount { id: node },
            ) if id == &actual.to_string()
                && serde_json::to_value(node)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    == Some(actual.to_string()) => {}
            _ => return Err(denied()),
        }
        policy.validate().map_err(|_| denied())
    }
    /// SAME original resources inside synchronous per-poll guards only.
    /// All weak upgrades die before this call returns; no Core or Session is
    /// retained across a storage/network Pending by the caller.
    /// Callback must not re-enter scope checks or WorkingSet read methods.
    pub(super) fn with_current_resources<T>(
        &self, f: impl FnOnce(&Session, &Unlocked, &DeviceIdentity) -> AppResult<T>,
    ) -> AppResult<T> {
        self.with_io_current(|_| {
            let owner = self.owner.upgrade().ok_or_else(denied)?;
            let unlock = self.unlock.upgrade().ok_or_else(denied)?;
            let identity = self.identity.upgrade().ok_or_else(denied)?;
            f(&owner, &unlock, &identity)
        })
    }
    pub async fn with_loaded<T>(
        &self,
        app: &AppCore,
        f: impl FnOnce(&DeviceIdentity) -> AppResult<T>,
    ) -> AppResult<T> {
        self.with_resources(app, |_, _, identity| f(identity)).await
    }
    /// Native resources only while SAME original guards are held. No async callback.
    pub(super) async fn with_resources<T>(
        &self,
        app: &AppCore,
        f: impl FnOnce(&Session, &Unlocked, &DeviceIdentity) -> AppResult<T>,
    ) -> AppResult<T> {
        let run = async {
            self.check_current()?;
            let owner = self.owner.upgrade().ok_or_else(denied)?;
            let profile = owner.profile().await?;
            self.provenance.verify(&owner, &profile).await?;
            let u = self.unlock.upgrade().ok_or_else(denied)?;
            let _active = app.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&u).await?;
            let identity = owner.identity();
            self.check_current()?;
            let result = self.auth.with_current(|| {
                self.provenance
                    .with_current(&owner, || f(&owner, &u, &identity))
            })?;
            self.check_current()?;
            Ok(result)
        };
        tokio::select! {biased;_=self.cancelled()=>Err(denied()),result=run=>result}
    }
    pub async fn check(&self, app: &AppCore) -> AppResult<()> {
        self.with_loaded(app, |_| Ok(())).await
    }
    pub async fn cancelled(&self) {
        let mut shutdown = self.shutdown.clone();
        // A waiting route must not keep the loaded Session/IdentityCell alive.
        let stopped = {
            let Some(owner) = self.owner.upgrade() else {
                return;
            };
            owner.automation.stopped.clone()
        };
        if self.check_current().is_err() {
            return;
        }
        tokio::select! {
            _=self.cancel.cancelled()=>{},
            _=self.retired.cancelled()=>{},
            _=self.auth.cancelled()=>{},
            _=self.provenance.cancelled()=>{},
            _=stopped.cancelled()=>{},
            _=shutdown.changed()=>{},
            _=tokio::time::sleep_until(tokio::time::Instant::from_std(self.deadline))=>{}
        }
    }
}
