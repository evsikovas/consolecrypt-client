//! Shared account/profile lifetime fences for Automation/settings writes.
//! Extracted from the withdrawn Reader feature; no Reader transport or grants.
use crate::session::{Session, Unlocked};
use crate::{AppCore, AppError, AppResult};
use cc_crypto_core::agent_reader::{AgentReaderContextV1, AgentReaderHash, AgentReaderTenantV1};
use cc_protocol::vaults::{VaultRole, VaultState};
use cc_protocol::{DeviceId, UserId, VaultId};
use cc_storage_core::{Profile, ProfileKind};
use cc_sync_core::ReaderAuthenticatedSubject;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

fn invalid() -> AppError {
    AppError::invalid("agent_reader", "reader_authenticated_owner_changed")
}
fn hash(domain: &[u8], value: &BTreeMap<&str, String>) -> AppResult<AgentReaderHash> {
    let bytes = serde_json::to_vec(value).map_err(|_| invalid())?;
    if bytes.len() > 8192 {
        return Err(invalid());
    }
    let mut h = Sha256::new();
    h.update(domain);
    h.update([0]);
    h.update(bytes);
    Ok(AgentReaderHash::from_bytes(h.finalize().into()))
}

/// Shared by the existing AppCore-owned NativeReaderRuntime. Bumped before
/// session().await at authentication transition entry, not by a late UI event.
#[derive(Clone)]
pub(crate) struct OriginalAuthTransitions(Arc<TransitionInner>);
struct TransitionInner {
    gate: Mutex<CancellationToken>,
    generation: AtomicU64,
    pending: AtomicU64,
    blocked: AtomicBool,
}
impl Default for OriginalAuthTransitions {
    fn default() -> Self {
        Self(Arc::new(TransitionInner {
            gate: Mutex::new(CancellationToken::new()),
            generation: AtomicU64::new(0),
            pending: AtomicU64::new(0),
            blocked: AtomicBool::new(false),
        }))
    }
}
pub(crate) struct OriginalAuthTransitionGuard(Option<Arc<TransitionInner>>);
impl Drop for OriginalAuthTransitionGuard {
    fn drop(&mut self) {
        if let Some(inner) = self.0.take() {
            if let Ok(_gate) = inner.gate.lock() {
                let n = inner.pending.load(Ordering::SeqCst);
                if n == 0 {
                    inner.blocked.store(true, Ordering::SeqCst);
                } else {
                    inner.pending.store(n - 1, Ordering::SeqCst);
                }
            }
        }
    }
}
#[derive(Clone)]
pub(super) struct OriginalAuthEpoch {
    inner: Arc<TransitionInner>,
    generation: u64,
    cancelled: CancellationToken,
}
impl OriginalAuthTransitions {
    pub fn begin(&self) -> OriginalAuthTransitionGuard {
        let Ok(mut gate) = self.0.gate.lock() else {
            return OriginalAuthTransitionGuard(None);
        };
        gate.cancel();
        let next = self.0.generation.load(Ordering::SeqCst).checked_add(1);
        let count = self.0.pending.load(Ordering::SeqCst).checked_add(1);
        match (next, count) {
            (Some(next), Some(count)) if !self.0.blocked.load(Ordering::SeqCst) => {
                self.0.generation.store(next, Ordering::SeqCst);
                self.0.pending.store(count, Ordering::SeqCst);
                *gate = CancellationToken::new();
                OriginalAuthTransitionGuard(Some(self.0.clone()))
            }
            _ => {
                self.0.blocked.store(true, Ordering::SeqCst);
                OriginalAuthTransitionGuard(None)
            }
        }
    }
    pub(super) fn capture(&self) -> AppResult<OriginalAuthEpoch> {
        let gate = self.0.gate.lock().map_err(|_| invalid())?;
        if self.0.blocked.load(Ordering::SeqCst) || self.0.pending.load(Ordering::SeqCst) != 0 {
            return Err(invalid());
        }
        Ok(OriginalAuthEpoch {
            inner: self.0.clone(),
            generation: self.0.generation.load(Ordering::SeqCst),
            cancelled: gate.clone(),
        })
    }
}
impl OriginalAuthEpoch {
    // Lock-free: with_current callbacks call ctx.now()/is_live too.
    pub fn is_current(&self) -> bool {
        !self.cancelled.is_cancelled()
            && !self.inner.blocked.load(Ordering::SeqCst)
            && self.inner.pending.load(Ordering::SeqCst) == 0
            && self.inner.generation.load(Ordering::SeqCst) == self.generation
    }
    pub fn with_current<T>(&self, f: impl FnOnce() -> AppResult<T>) -> AppResult<T> {
        let _gate = self.inner.gate.lock().map_err(|_| invalid())?;
        if !self.is_current() {
            return Err(invalid());
        }
        f()
    }
    pub fn try_with_current<T>(&self, f: impl FnOnce() -> AppResult<T>) -> AppResult<T> {
        let _gate = self.inner.gate.try_lock().map_err(|_| invalid())?;
        if !self.is_current() {
            return Err(invalid());
        }
        f()
    }
    pub async fn cancelled(&self) {
        self.cancelled.cancelled().await;
    }
}
impl AppCore {
    /// Opaque guard remains alive across logout/reauth/attach/detach awaits.
    pub(crate) fn original_auth_transition(&self) -> impl Drop + Send + 'static {
        self.inner.original_auth.begin()
    }
}

#[derive(Clone)]
pub(super) enum OriginalOwnerBinding {
    Local { hash: AgentReaderHash },
    Personal(Arc<PersonalOwner>),
}
pub(super) struct PersonalOwner {
    authenticated: ReaderAuthenticatedSubject,
    user: UserId,
    device: DeviceId,
    vault: VaultId,
    // Public VRK commitment only. Never an unlocked vault or private key.
    vault_commitment: [u8; 32],
    hash: AgentReaderHash,
}
impl OriginalOwnerBinding {
    pub async fn capture(
        owner: &Arc<Session>,
        profile: &Profile,
        vault: VaultId,
    ) -> AppResult<Self> {
        if profile.profile_id != owner.profile_id
            || profile.device_id != owner.device_id()
            || owner.require_vault_id()? != vault
        {
            return Err(invalid());
        }
        match profile.kind {
            ProfileKind::Local if profile.user_id.is_none() && owner.api_opt().is_none() => {
                let hash = hash(
                    b"consolecrypt.native.reader.local-owner.v1",
                    &BTreeMap::from([
                        ("format", "cc-native-reader-local-owner/v1".into()),
                        ("profileId", owner.profile_id.to_string()),
                    ]),
                )?;
                Ok(Self::Local { hash })
            }
            ProfileKind::Synced => {
                let api = owner.api_opt().ok_or_else(invalid)?;
                let authenticated = api.capture_reader_auth().await.map_err(|_| invalid())?;
                let user = profile.user_id.ok_or_else(invalid)?;
                if user == UserId::NIL
                    || authenticated.user_id() != user
                    || authenticated.device_id() != owner.device_id()
                    || authenticated.session_id() == cc_protocol::SessionId::NIL
                    || canonical_profile_server(profile)? != authenticated.server_url()
                    || api.config().server_url.as_str() != authenticated.server_url()
                {
                    return Err(invalid());
                }
                let server_hash = hash(
                    b"consolecrypt.native.reader.account-server.v1",
                    &BTreeMap::from([("serverUrl", authenticated.server_url().to_owned())]),
                )?;
                let hash = hash(
                    b"consolecrypt.native.reader.personal-owner.v1",
                    &BTreeMap::from([
                        ("format", "cc-native-reader-personal-owner/v1".into()),
                        ("userId", user.to_string()),
                        (
                            "serverOriginHash",
                            serde_json::to_value(server_hash)
                                .map_err(|_| invalid())?
                                .as_str()
                                .ok_or_else(invalid)?
                                .to_owned(),
                        ),
                        ("personalVaultId", vault.to_string()),
                    ]),
                )?;
                let binding = Self::Personal(Arc::new(PersonalOwner {
                    device: owner.device_id(),
                    authenticated,
                    user,
                    vault,
                    vault_commitment: confirmed_vault_commitment(owner).await?,
                    hash,
                }));
                binding.verify(owner, profile).await?;
                Ok(binding)
            }
            _ => Err(invalid()),
        }
    }
    pub fn is_personal(&self) -> bool {
        matches!(self, Self::Personal(_))
    }
    pub fn hash(&self) -> AgentReaderHash {
        match self {
            Self::Local { hash } => *hash,
            Self::Personal(p) => p.hash,
        }
    }
    pub fn snapshot_hash(&self) -> Option<AgentReaderHash> {
        self.is_personal().then(|| self.hash())
    }
    pub fn check_vault(&self, unlocked: &Arc<Unlocked>) -> AppResult<()> {
        if let Self::Personal(p) = self {
            if unlocked.vault_id != p.vault
                || unlocked.vault()?.key_commitment() != p.vault_commitment
                || !unlocked.key_conflicts().is_empty()
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
    pub fn check_context(&self, context: &AgentReaderContextV1) -> AppResult<()> {
        match (self, &context.tenant) {
            (Self::Local { .. }, AgentReaderTenantV1::LocalProfile { .. }) => Ok(()),
            (Self::Personal(p), AgentReaderTenantV1::PersonalAccount { id })
                if id.as_uuid().to_string() == p.user.to_string() =>
            {
                Ok(())
            }
            _ => Err(invalid()),
        }
    }
    pub fn is_current(&self, owner: &Arc<Session>) -> bool {
        match self {
            Self::Local { .. } => owner.api_opt().is_none(),
            Self::Personal(p) => {
                owner
                    .api_opt()
                    .is_some_and(|api| p.authenticated.check_current(&api).is_ok())
                    && owner.device_id() == p.device
                    && owner.require_vault_id().ok() == Some(p.vault)
            }
        }
    }
    pub fn with_current<T>(
        &self,
        owner: &Arc<Session>,
        f: impl FnOnce() -> AppResult<T>,
    ) -> AppResult<T> {
        match self {
            Self::Local { .. } if owner.api_opt().is_none() => f(),
            Self::Personal(p) => {
                let api = owner.api_opt().ok_or_else(invalid)?;
                p.authenticated
                    .with_current(&api, f)
                    .map_err(|_| invalid())?
            }
            _ => Err(invalid()),
        }
    }
    pub fn try_with_current<T>(
        &self,
        owner: &Arc<Session>,
        f: impl FnOnce() -> AppResult<T>,
    ) -> AppResult<T> {
        match self {
            Self::Local { .. } if owner.api_opt().is_none() => f(),
            Self::Personal(p) => {
                let api = owner.api_opt().ok_or_else(invalid)?;
                p.authenticated
                    .try_with_current(&api, f)
                    .map_err(|_| invalid())?
            }
            _ => Err(invalid()),
        }
    }
    pub async fn cancelled(&self) {
        match self {
            Self::Local { .. } => std::future::pending::<()>().await,
            Self::Personal(p) => p.authenticated.cancelled().await,
        }
    }
    pub async fn verify(&self, owner: &Arc<Session>, profile: &Profile) -> AppResult<()> {
        if profile.profile_id != owner.profile_id
            || profile.device_id != owner.device_id()
            || !self.is_current(owner)
        {
            return Err(invalid());
        }
        match self {
            Self::Local { .. }
                if profile.kind == ProfileKind::Local && profile.user_id.is_none() =>
            {
                Ok(())
            }
            Self::Personal(p)
                if profile.kind == ProfileKind::Synced
                    && profile.user_id == Some(p.user)
                    && canonical_profile_server(profile)? == p.authenticated.server_url() =>
            {
                if confirmed_vault_commitment(owner).await? != p.vault_commitment {
                    return Err(invalid());
                }
                // Current server contract /v1/vaults lists PERSONAL vaults only.
                // Require membership there in addition to get_vault's owner ACL.
                let api = owner.api_opt().ok_or_else(invalid)?;
                p.authenticated.check_current(&api).map_err(|_| invalid())?;
                let personal = api.list_vaults().await.map_err(|_| invalid())?;
                if personal.vaults.len() > 128 {
                    return Err(invalid());
                }
                let matches: Vec<_> = personal
                    .vaults
                    .iter()
                    .filter(|v| v.vault_id == p.vault)
                    .collect();
                if matches.len() != 1 {
                    return Err(invalid());
                }
                let selected = matches[0];
                let fresh = api.get_vault(p.vault).await.map_err(|_| invalid())?;
                for info in [selected, &fresh] {
                    if info.vault_id != p.vault
                        || info.owner_user_id != p.user
                        || info.role != VaultRole::Owner
                        || info.state != VaultState::Active
                        || !info.caller_trusted
                    {
                        return Err(invalid());
                    }
                }
                if confirmed_vault_commitment(owner).await? != p.vault_commitment
                    || p.authenticated.check_current(&api).is_err()
                    || !self.is_current(owner)
                {
                    return Err(invalid());
                }
                Ok(())
            }
            _ => Err(invalid()),
        }
    }
}
async fn confirmed_vault_commitment(owner: &Arc<Session>) -> AppResult<[u8; 32]> {
    let pin = owner.pinned_vault_key().await.ok_or_else(invalid)?;
    if pin.unconfirmed {
        return Err(invalid());
    }
    pin.commitment().ok_or_else(invalid)
}
fn canonical_profile_server(profile: &Profile) -> AppResult<String> {
    let raw = profile.server_url.as_deref().ok_or_else(invalid)?;
    if raw.len() > 4096 {
        return Err(invalid());
    }
    let url = url::Url::parse(raw.trim()).map_err(|_| invalid())?;
    Ok(url.to_string())
}

/// Shared original-session provenance only; never Reader or Node authority.
#[derive(Clone)]
pub(crate) struct OriginalSessionAuth(OriginalAuthEpoch);
impl OriginalSessionAuth {
    pub(crate) fn capture(app: &AppCore) -> AppResult<Self> {
        Ok(Self(app.inner.original_auth.capture()?))
    }
    pub(crate) fn is_current(&self) -> bool {
        self.0.is_current()
    }
    pub(crate) fn with_current<T>(&self, f: impl FnOnce() -> AppResult<T>) -> AppResult<T> {
        self.0.with_current(f)
    }
    pub(crate) fn try_with_current<T>(&self, f: impl FnOnce() -> AppResult<T>) -> AppResult<T> {
        self.0.try_with_current(f)
    }
    pub(crate) async fn cancelled(&self) {
        self.0.cancelled().await
    }
}
pub(crate) struct OriginalSessionProvenance(OriginalOwnerBinding);
impl OriginalSessionProvenance {
    pub(crate) async fn capture(
        owner: &Arc<Session>,
        profile: &Profile,
        vault: VaultId,
    ) -> AppResult<Self> {
        Ok(Self(
            OriginalOwnerBinding::capture(owner, profile, vault).await?,
        ))
    }
    /// Original account subject only; never captures or refreshes authorization.
    pub(crate) async fn cancelled(&self) {
        match &self.0 {
            OriginalOwnerBinding::Local { .. } => std::future::pending::<()>().await,
            OriginalOwnerBinding::Personal(owner) => owner.authenticated.cancelled().await,
        }
    }
    pub(crate) fn personal_user_id(&self) -> Option<UserId> {
        match &self.0 {
            OriginalOwnerBinding::Local { .. } => None,
            OriginalOwnerBinding::Personal(p) => Some(p.user),
        }
    }
    pub(crate) fn is_current(&self, owner: &Arc<Session>) -> bool {
        self.0.is_current(owner)
    }
    pub(crate) fn check_vault(&self, u: &Arc<Unlocked>) -> AppResult<()> {
        self.0.check_vault(u)
    }
    pub(crate) fn with_current<T>(
        &self,
        owner: &Arc<Session>,
        f: impl FnOnce() -> AppResult<T>,
    ) -> AppResult<T> {
        self.0.with_current(owner, f)
    }
    pub(crate) fn try_with_current<T>(
        &self,
        owner: &Arc<Session>,
        f: impl FnOnce() -> AppResult<T>,
    ) -> AppResult<T> {
        self.0.try_with_current(owner, f)
    }
    pub(crate) async fn verify(&self, owner: &Arc<Session>, profile: &Profile) -> AppResult<()> {
        self.0.verify(owner, profile).await
    }
}
