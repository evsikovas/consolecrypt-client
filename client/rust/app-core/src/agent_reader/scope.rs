//! Staged original Native context fences. No credential, issuer or transport.
use super::authentication::{ReaderAuthEpoch, ReaderOwnerBinding};
use crate::session::{Session, Unlocked};
use crate::{AppCore, AppError, AppResult};
use cc_crypto_core::agent_reader::AgentReaderHash;
use cc_models::ObjectId;
use cc_vault_core::DeviceIdentity;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[cfg(test)]
#[path = "scope_tests.rs"]
mod tests;

fn invalid() -> AppError {
    AppError::invalid("agent_reader", "reader_context_changed")
}
fn canonical_id(value: &str) -> AppResult<()> {
    let id = Uuid::parse_str(value).map_err(|_| invalid())?;
    if id.is_nil() || id.to_string() != value {
        return Err(invalid());
    }
    Ok(())
}

struct HashWriter {
    hash: Sha256,
    size: usize,
    limit: usize,
}
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let size = self
            .size
            .checked_add(bytes.len())
            .ok_or_else(|| std::io::Error::other("reader metadata budget"))?;
        if size > self.limit {
            return Err(std::io::Error::other("reader metadata budget"));
        }
        self.hash.update(bytes);
        self.size = size;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn metadata_hash(domain: &[u8], value: &impl Serialize) -> AppResult<String> {
    let mut writer = HashWriter {
        hash: Sha256::new(),
        size: 0,
        limit: 262144,
    };
    writer.hash.update(domain);
    writer.hash.update([0]);
    serde_json::to_writer(&mut writer, value).map_err(|_| invalid())?;
    Ok(hex::encode(writer.hash.finalize()))
}

/// Selection identifies existing Native records; it is never an approval.
#[derive(Clone)]
pub(super) struct ReaderSourceSelection {
    pub profile_id: String,
    pub host_id: String,
    pub source_id: String,
    pub source_revision: u64,
    pub binding_id: String,
    pub binding_revision: u64,
}

/// Public metadata frozen under the original unlock. Hashes contain no keys.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ReaderContextSnapshot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub personal_owner: Option<AgentReaderHash>,
    pub native_profile_id: String,
    pub notification_scope_id: String,
    pub native_host_id: String,
    pub native_host_revision: u64,
    pub host_body_digest: String,
    pub native_source_id: String,
    pub native_source_revision: u64,
    pub source_configuration_digest: String,
    pub native_binding_id: String,
    pub native_binding_revision: u64,
    pub binding_body_digest: String,
    pub native_binding_digest: String,
    pub device_id: String,
    pub device_signing_key: [u8; 32],
}

fn snapshot(
    owner: &Arc<Session>,
    u: &Arc<Unlocked>,
    selected: &ReaderSourceSelection,
    owner_binding: &ReaderOwnerBinding,
) -> AppResult<(ReaderContextSnapshot, BTreeMap<ObjectId, i64>, u64)> {
    owner_binding.check_vault(u)?;
    if owner.profile_id.to_string() != selected.profile_id
        || !owner_binding.is_current(owner)
        || owner.require_vault_id()? != u.vault_id
        || u.vault()?.vault_id() != u.vault_id
        || selected.source_revision == 0
        || selected.binding_revision == 0
    {
        return Err(invalid());
    }
    let working = u.working();
    let before = working.version();
    let host_id = crate::dto::parse_id("agent_reader_host", &selected.host_id)?;
    let host = working.host(host_id).ok_or_else(invalid)?;
    if owner_binding.is_personal()
        && (working.vault_of(host_id).is_some()
            || u.writer.teams.get(u.vault_id).is_some()
            || crate::teams::membership_of(&u.writer, u.vault_id).is_some())
    {
        return Err(invalid());
    }
    let (host_revision, protocol) = working
        .correlation_host_metadata(host_id)
        .ok_or_else(invalid)?;
    if host_revision <= 0 || protocol != "ssh" {
        return Err(invalid());
    }
    let binding = crate::monitoring_sources::binding_for_request(
        u,
        &selected.binding_id,
        selected.binding_revision,
        selected.source_revision,
    )?;
    if binding.host_id().to_string() != selected.host_id
        || binding.source_id() != selected.source_id
    {
        return Err(invalid());
    }
    let source = crate::monitoring_sources::source_for_request(
        u,
        &selected.source_id,
        selected.source_revision,
    )?;
    if owner_binding.is_personal() {
        use cc_monitoring_core::AuthRef;
        let reference = match &source.auth {
            AuthRef::None => None,
            AuthRef::BasicRef { credential_id } | AuthRef::BearerRef { credential_id } => {
                Some(credential_id)
            }
        };
        if let Some(reference) = reference {
            let id = crate::dto::parse_id("agent_reader_source_credential", reference)?;
            let credential = working.credential(id).ok_or_else(invalid)?;
            for object in std::iter::once(id)
                .chain(credential.secret_id)
                .chain(credential.passphrase_secret_id)
            {
                if working.kind_of(object).is_none()
                    || working.revision_of(object).is_none_or(|r| r <= 0)
                    || working.vault_of(object).is_some()
                {
                    return Err(invalid());
                }
            }
        }
    }
    let settings = working
        .all_vault_settings()
        .into_iter()
        .max_by_key(|s| s.updated_at)
        .ok_or_else(invalid)?;
    let settings_revision = working
        .revision_of(settings.id)
        .filter(|v| *v > 0)
        .ok_or_else(invalid)?;
    if owner_binding.is_personal() && working.vault_of(settings.id).is_some() {
        return Err(invalid());
    }
    let mut fences = BTreeMap::new();
    fences.insert(host_id, host_revision);
    fences.insert(settings.id, settings_revision);
    let identity = owner.identity();
    let mut result = ReaderContextSnapshot {
        personal_owner: owner_binding.snapshot_hash(),
        native_profile_id: selected.profile_id.clone(),
        notification_scope_id: u.notices.scope_id().ok_or_else(invalid)?,
        native_host_id: selected.host_id.clone(),
        native_host_revision: host_revision as u64,
        host_body_digest: metadata_hash(b"consolecrypt.native.reader.host-snapshot.v1", &host)?,
        native_source_id: selected.source_id.clone(),
        native_source_revision: selected.source_revision,
        source_configuration_digest: metadata_hash(
            b"consolecrypt.native.reader.source-snapshot.v1",
            &source,
        )?,
        native_binding_id: selected.binding_id.clone(),
        native_binding_revision: selected.binding_revision,
        binding_body_digest: metadata_hash(
            b"consolecrypt.native.reader.binding-snapshot.v1",
            &binding,
        )?,
        native_binding_digest: String::new(),
        device_id: identity.device_id().to_string(),
        device_signing_key: identity.public_keys().signing,
    };
    // Explicit sorted Native-only body. It is not the Prometheus binding's
    // authority, an Agent Context cast, or the Agent ReaderStatement encoding.
    let mut body = BTreeMap::from([
        ("format", serde_json::json!("cc-native-reader-binding/v1")),
        ("profileId", serde_json::json!(result.native_profile_id)),
        ("hostId", serde_json::json!(result.native_host_id)),
        (
            "hostRevision",
            serde_json::json!(result.native_host_revision),
        ),
        ("hostBodyDigest", serde_json::json!(result.host_body_digest)),
        ("sourceId", serde_json::json!(result.native_source_id)),
        (
            "sourceRevision",
            serde_json::json!(result.native_source_revision),
        ),
        (
            "sourceConfigurationDigest",
            serde_json::json!(result.source_configuration_digest),
        ),
        ("bindingId", serde_json::json!(result.native_binding_id)),
        (
            "bindingRevision",
            serde_json::json!(result.native_binding_revision),
        ),
        (
            "bindingBodyDigest",
            serde_json::json!(result.binding_body_digest),
        ),
    ]);
    if let Some(owner_hash) = result.personal_owner {
        body.insert("format", serde_json::json!("cc-native-reader-binding/v2"));
        body.insert("personalOwner", serde_json::json!(owner_hash));
    }
    result.native_binding_digest = metadata_hash(b"consolecrypt.native.reader.binding.v1", &body)?;
    if before != working.version() {
        return Err(invalid());
    }
    Ok((result, fences, before))
}

/// Cannot be deserialized or made from a frontend permission flag.
pub(super) struct ReaderOriginalContext {
    pub(super) owner_binding: ReaderOwnerBinding,
    authentication_epoch: ReaderAuthEpoch,
    attempt_identity: Arc<()>,
    owner: Weak<Session>,
    unlock: Weak<Unlocked>,
    identity: Weak<DeviceIdentity>,
    selection: ReaderSourceSelection,
    captured: ReaderContextSnapshot,
    working_version: u64,
    generation: u64,
    revoked: CancellationToken,
    user_cancel: CancellationToken,
    shutdown: watch::Receiver<bool>,
    deadline: Instant,
    wall_highwater: Mutex<Option<Duration>>,
}

/// Opaque original-attempt identity for owned transport values. It is only a
/// fence, never approval. Cloning retains no Session, Unlocked or device key.
#[derive(Clone)]
pub(super) struct ReaderScopeIdentity {
    owner_binding: ReaderOwnerBinding,
    authentication_epoch: ReaderAuthEpoch,
    identity: Arc<()>,
    revoked: CancellationToken,
    user_cancel: CancellationToken,
    shutdown: watch::Receiver<bool>,
    owner: Weak<Session>,
    unlock: Weak<Unlocked>,
    working_version: u64,
    device_identity: Weak<DeviceIdentity>,
    generation: u64,
    deadline: Instant,
}
impl std::fmt::Debug for ReaderScopeIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReaderScopeIdentity([redacted])")
    }
}
// A synchronous observation only; the temporary upgrade/read guard is gone
// BEFORE cancellation or any owner/API/runtime lock. No retained Unlocked.
fn working_set_current(
    unlock: &Weak<Unlocked>,
    expected: u64,
    revoked: &CancellationToken,
) -> bool {
    let current = unlock
        .upgrade()
        .is_some_and(|u| u.working().version() == expected);
    if !current {
        revoked.cancel(); // Sticky: restoring the body never revives a scope.
    }
    current
}

impl ReaderScopeIdentity {
    /// Observation only; this cannot approve an action or replace with_held.
    pub fn is_live(&self) -> bool {
        if !working_set_current(&self.unlock, self.working_version, &self.revoked)
            || !self.authentication_epoch.is_current()
            || self.revoked.is_cancelled()
            || self.user_cancel.is_cancelled()
            || Instant::now() >= self.deadline
            || *self.shutdown.borrow()
            || self.shutdown.has_changed().unwrap_or(true)
        {
            return false;
        }
        let (Some(owner), Some(identity)) = (self.owner.upgrade(), self.device_identity.upgrade())
        else {
            return false;
        };
        !owner.automation.stopped.is_cancelled()
            && self.owner_binding.is_current(&owner)
            && owner.automation.unlocked_generation().ok() == Some(self.generation)
            && Arc::ptr_eq(&identity, &owner.identity())
            && working_set_current(&self.unlock, self.working_version, &self.revoked)
    }
    pub fn check(&self, original: &ReaderOriginalContext) -> AppResult<()> {
        if !Arc::ptr_eq(&self.identity, &original.attempt_identity) || !self.is_live() {
            return Err(invalid());
        }
        original.unchanged()
    }
    pub async fn cancelled(&self) {
        if !self.is_live() {
            return;
        }
        let stopped = {
            let Some(owner) = self.owner.upgrade() else { return; };
            owner.automation.stopped.clone()
        }; // Drop the upgraded Session BEFORE this idle cancellation wait.
        let mut shutdown = self.shutdown.clone();
        tokio::select! { biased;
            _ = self.authentication_epoch.cancelled() => {},
            _ = self.owner_binding.cancelled() => {},
            _ = self.revoked.cancelled() => {},
            _ = self.user_cancel.cancelled() => {},
            _ = stopped.cancelled() => {},
            _ = shutdown.changed() => {},
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(self.deadline)) => {},
        }
    }
}
impl std::fmt::Debug for ReaderOriginalContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReaderOriginalContext([redacted])")
    }
}
impl Drop for ReaderOriginalContext {
    fn drop(&mut self) {
        self.revoked.cancel();
    }
}

/// This short-lived borrowed value exists only while real profile/unlock
/// guards are held. All methods used by authority must remain synchronous.
pub(super) struct HeldNativeReaderContext<'a> {
    pub original: &'a ReaderOriginalContext,
    pub owner: &'a Arc<Session>,
    pub unlock: &'a Arc<Unlocked>,
    pub identity: &'a Arc<DeviceIdentity>,
    pub snapshot: &'a ReaderContextSnapshot,
}
impl HeldNativeReaderContext<'_> {
    pub fn require_personal_object(&self, id: ObjectId) -> AppResult<()> {
        if self.original.is_personal_account() {
            let working = self.unlock.working();
            if working.kind_of(id).is_none()
                || working.revision_of(id).is_none_or(|r| r <= 0)
                || working.vault_of(id).is_some()
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
}

impl ReaderOriginalContext {
    pub async fn capture(
        app: &AppCore,
        selection: ReaderSourceSelection,
        cancel: &CancellationToken,
    ) -> AppResult<Self> {
        Self::capture_checked(app, selection, cancel, None).await
    }
    /// Opaque notice nonce belongs to an actual original unlock, never a UUID supplied as authority.
    pub(super) fn notification_matches(expected: &str, actual: Option<&str>) -> AppResult<()> {
        if expected.is_empty() || expected.len() > 256 || actual != Some(expected) {
            return Err(invalid());
        }
        Ok(())
    }
    /// Metadata guards only, before account/storage/probe/file I/O. Capture repeats this check.
    pub(super) async fn check_notification_scope(
        app: &AppCore,
        profile: &str,
        expected: &str,
    ) -> AppResult<()> {
        Self::notification_matches(expected, Some(expected))?;
        let owner = app.session().await?;
        if owner.profile_id.to_string() != profile {
            return Err(invalid());
        }
        let u = owner.unlocked().await?;
        let _profile = app.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        Self::notification_matches(expected, u.notices.scope_id().as_deref())
    }
    pub(super) async fn capture_for_notification(
        app: &AppCore,
        selection: ReaderSourceSelection,
        cancel: &CancellationToken,
        expected: &str,
    ) -> AppResult<Self> {
        Self::notification_matches(expected, Some(expected))?;
        Self::capture_checked(app, selection, cancel, Some(expected)).await
    }
    async fn capture_checked(
        app: &AppCore,
        selection: ReaderSourceSelection,
        cancel: &CancellationToken,
        expected_notification: Option<&str>,
    ) -> AppResult<Self> {
        // Include session/storage/guard acquisition in the same attempt budget.
        let deadline = Instant::now() + Duration::from_secs(60);
        let authentication_epoch = app.inner.reader_native.auth_transitions.capture()?;
        tokio::select! { biased;
            _ = authentication_epoch.cancelled() => Err(invalid()),
            _ = cancel.cancelled() => Err(invalid()),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => Err(invalid()),
            result = Self::capture_before(app, selection, cancel, deadline, authentication_epoch.clone(), expected_notification) => result,
        }
    }

    async fn capture_before(
        app: &AppCore,
        selection: ReaderSourceSelection,
        cancel: &CancellationToken,
        deadline: Instant,
        authentication_epoch: ReaderAuthEpoch,
        expected_notification: Option<&str>,
    ) -> AppResult<Self> {
        for id in [
            &selection.profile_id,
            &selection.host_id,
            &selection.source_id,
            &selection.binding_id,
        ] {
            canonical_id(id)?;
        }
        if cancel.is_cancelled() {
            return Err(invalid());
        }
        let owner = app.session().await?;
        if owner.profile_id.to_string() != selection.profile_id {
            return Err(invalid());
        }
        // Bind exact ORIGINAL unlock before storage/account I/O. No strong Unlocked is held over I/O.
        let original_unlock = tokio::select! { biased;
            _ = owner.automation.stopped.cancelled() => return Err(invalid()),
            result = owner.unlocked() => result?,
        };
        let unlock = Arc::downgrade(&original_unlock);
        let (original_vault, original_generation, identity, shutdown) = {
            let _profile = app.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&original_unlock).await?;
            if let Some(expected) = expected_notification {
                Self::notification_matches(
                    expected,
                    original_unlock.notices.scope_id().as_deref(),
                )?;
            }
            (
                original_unlock.vault_id,
                owner.automation.unlocked_generation()?,
                owner.identity(),
                original_unlock.sharing_shutdown.subscribe(),
            )
        };
        drop(original_unlock);
        if *shutdown.borrow() {
            return Err(invalid());
        }
        // The nonce was checked under actual original guards BEFORE this first storage/account I/O.
        let profile = tokio::select! { biased;
            _ = owner.automation.stopped.cancelled() => return Err(invalid()),
            result = owner.profile() => result?,
        };
        let mut shutdown_wait = shutdown.clone();
        let acquire = async {
            let owner_binding =
                ReaderOwnerBinding::capture(&owner, &profile, original_vault).await?;
            if !authentication_epoch.is_current() {
                return Err(invalid());
            }
            let u = unlock.upgrade().ok_or_else(invalid)?;
            let _profile = app.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&u).await?;
            if owner.automation.unlocked_generation()? != original_generation
                || !Arc::ptr_eq(&identity, &owner.identity())
            {
                return Err(invalid());
            }
            if let Some(expected) = expected_notification {
                Self::notification_matches(expected, u.notices.scope_id().as_deref())?;
            }
            let (captured, fences, working_version) =
                snapshot(&owner, &u, &selection, &owner_binding)?;
            let revoked = CancellationToken::new();
            if !u.working().watch_revisions(fences, revoked.clone()) {
                return Err(invalid());
            }
            let result = Self {
                owner_binding,
                authentication_epoch,
                attempt_identity: Arc::new(()),
                owner: Arc::downgrade(&owner),
                unlock: Arc::downgrade(&u),
                identity: Arc::downgrade(&identity),
                selection,
                captured,
                working_version,
                generation: original_generation,
                revoked,
                user_cancel: cancel.clone(),
                shutdown,
                deadline,
                wall_highwater: Mutex::new(None),
            };
            result.check_held(&owner, &u, &identity)?;
            Ok(result)
        };
        tokio::select! { biased;
            _ = owner.automation.stopped.cancelled() => Err(invalid()),
            _ = shutdown_wait.changed() => Err(invalid()),
            result = acquire => result,
        }
    }

    fn unchanged(&self) -> AppResult<()> {
        if !working_set_current(&self.unlock, self.working_version, &self.revoked)
            || !self.authentication_epoch.is_current()
            || self.revoked.is_cancelled()
            || self.user_cancel.is_cancelled()
            || Instant::now() >= self.deadline
            || *self.shutdown.borrow()
            || self.shutdown.has_changed().unwrap_or(true)
        {
            return Err(invalid());
        }
        let owner = self.owner.upgrade().ok_or_else(invalid)?;
        if !self.owner_binding.is_current(&owner)
            || !working_set_current(&self.unlock, self.working_version, &self.revoked)
        {
            return Err(invalid());
        }
        Ok(())
    }
    fn check_held(
        &self,
        owner: &Arc<Session>,
        u: &Arc<Unlocked>,
        identity: &Arc<DeviceIdentity>,
    ) -> AppResult<()> {
        self.unchanged()?;
        let original = self.identity.upgrade().ok_or_else(invalid)?;
        if owner.automation.stopped.is_cancelled()
            || owner.automation.unlocked_generation()? != self.generation
            || !Arc::ptr_eq(&original, identity)
            || !Arc::ptr_eq(&original, &owner.identity())
            || snapshot(owner, u, &self.selection, &self.owner_binding)?.0 != self.captured
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub async fn with_held<T>(
        &self,
        app: &AppCore,
        f: impl FnOnce(HeldNativeReaderContext<'_>) -> AppResult<T>,
    ) -> AppResult<T> {
        // Dropping a blocked read/guard-acquisition future releases its held
        // references. A queued read may finish, but cannot publish authority.
        tokio::select! { biased;
            _ = self.cancelled() => Err(invalid()),
            result = self.with_held_before(app, f) => result,
        }
    }
    async fn with_held_before<T>(
        &self,
        app: &AppCore,
        f: impl FnOnce(HeldNativeReaderContext<'_>) -> AppResult<T>,
    ) -> AppResult<T> {
        self.unchanged()?;
        let owner = self.owner.upgrade().ok_or_else(invalid)?;
        let profile = owner.profile().await?;
        self.owner_binding.verify(&owner, &profile).await?;
        let u = self.unlock.upgrade().ok_or_else(invalid)?;
        let _profile = app.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        let identity = owner.identity();
        self.check_held(&owner, &u, &identity)?;
        let version = u.working().version();
        let result = self.authentication_epoch.with_current(|| {
            self.owner_binding.with_current(&owner, || {
                f(HeldNativeReaderContext {
                    original: self,
                    owner: &owner,
                    unlock: &u,
                    identity: &identity,
                    snapshot: &self.captured,
                })
            })
        })?;
        self.check_held(&owner, &u, &identity)?;
        if version != u.working().version() {
            return Err(invalid());
        }
        Ok(result)
    }
    pub async fn check(&self, app: &AppCore) -> AppResult<()> {
        self.with_held(app, |_| Ok(())).await
    }
    pub fn snapshot(&self) -> &ReaderContextSnapshot {
        &self.captured
    }
    pub fn scope_identity(&self) -> ReaderScopeIdentity {
        ReaderScopeIdentity {
            owner_binding: self.owner_binding.clone(),
            authentication_epoch: self.authentication_epoch.clone(),
            identity: self.attempt_identity.clone(),
            revoked: self.revoked.clone(),
            user_cancel: self.user_cancel.clone(),
            shutdown: self.shutdown.clone(),
            owner: self.owner.clone(),
            unlock: self.unlock.clone(),
            working_version: self.working_version,
            device_identity: self.identity.clone(),
            generation: self.generation,
            deadline: self.deadline,
        }
    }
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
    pub fn retire(&self) {
        self.revoked.cancel();
    }
    /// Fixed native OS clock, including fractional rollback observation. It is
    /// never provided by frontend JSON; persistent issuer ledger floors remain
    /// the authority owner's independent obligation.
    pub fn now(&self) -> AppResult<cc_protocol::Timestamp> {
        self.unchanged()?;
        let raw = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| invalid())?;
        let mut highwater = self.wall_highwater.lock().map_err(|_| invalid())?;
        if raw > Duration::from_secs(253402300799) || highwater.is_some_and(|old| raw < old) {
            self.revoked.cancel();
            return Err(invalid());
        }
        *highwater = Some(raw);
        chrono::DateTime::<chrono::Utc>::from_timestamp(raw.as_secs() as i64, raw.subsec_nanos())
            .ok_or_else(invalid)
    }
    pub fn cancelled(&self) -> impl std::future::Future<Output = ()> + '_ {
        async move {
            if self.unchanged().is_err() {
                return;
            }
            let mut shutdown = self.shutdown.clone();
            let stopped = {
                let Some(owner) = self.owner.upgrade() else { return; };
                owner.automation.stopped.clone()
            }; // No Session/Unlocked/device key is retained across await.
            {
                tokio::select! { biased;
                    _ = self.authentication_epoch.cancelled() => {},
                    _ = self.owner_binding.cancelled() => {},
                    _ = self.revoked.cancelled() => {}, _ = self.user_cancel.cancelled() => {},
                    _ = stopped.cancelled() => {}, _ = shutdown.changed() => {},
                    _ = tokio::time::sleep_until(tokio::time::Instant::from_std(self.deadline)) => {},
                }
            }
        }
    }
}
