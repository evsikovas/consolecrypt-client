//! An open profile ([`Session`]) and its unlocked-vault state
//! ([`Unlocked`]): storage, device identity, API client (synced profiles),
//! codec, working set, sync engine, WebSocket events and the SSH runtime.

use crate::codec::VaultCodec;
use crate::dto::{AppEvent, ChangeOriginDto, SyncStatusDto};
use crate::error::{AppError, AppResult};
use crate::prompts::PromptBroker;
use crate::secrets::{self, SecureTokenStore};
use crate::ssh::SshRuntime;
use crate::working_set::{Readback, WorkingSet};
use crate::writer::{EngineSlot, VaultWriter};
use crate::{AppConfig, KdfPolicy};
use cc_models::{KekClass, ObjectKind};
use cc_platform_core::{OsAuthenticator, SecureStore};
use cc_protocol::envelopes::KeyEnvelope;
use cc_protocol::events::ServerEvent;
use cc_protocol::{DeviceId, DeviceRequestId, VaultId};
use cc_storage_core::{Profile, ProfileId, ProfileKind, Storage};
use cc_sync_core::{
    ApiClient, ApiConfig, EventStream, EventStreamConfig, ObjectStore, StopReason, SyncEngine,
    SyncEngineConfig, SyncEvent, WsEvent,
};
use cc_vault_core::{
    Argon2Params, DeviceIdentity, PendingApproval, RecoveryKit, RecoveryKitCheck, UnlockedVault,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

/// Local setting holding the profile's vault id.
pub(crate) const VAULT_ID_SETTING: &str = "app_core.vault_id";
/// Local setting: this (new) device identity must attest on next unlock.
pub(crate) const NEEDS_ATTEST_SETTING: &str = "app_core.needs_attest";
/// Local setting: a Recovery Kit was shown and its 3-word check is still
/// pending (survives restarts; the kit itself is kept in memory only).
pub(crate) const KIT_PENDING_SETTING: &str = "app_core.recovery_kit_pending";
/// Explicit device-local opt-in; absent (including older installations) = off.
pub(crate) const DEVICE_UNLOCK_SETTING: &str = "app_core.device_unlock_enabled";

/// App-wide context shared by every session.
pub(crate) struct AppCtx {
    pub config: AppConfig,
    /// Data layout (per-profile paths, e.g. the AI search index).
    pub dirs: cc_platform_core::AppDirs,
    pub secure: Arc<dyn SecureStore>,
    pub os_auth: Arc<dyn OsAuthenticator>,
    pub events: broadcast::Sender<AppEvent>,
    pub prompts: Arc<PromptBroker>,
    /// Opens / reveals local files (edit sessions); replaceable in tests.
    pub file_opener: RwLock<cc_platform_core::FileOpener>,
}

impl std::fmt::Debug for AppCtx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppCtx")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl AppCtx {
    pub(crate) fn emit(&self, e: AppEvent) {
        let _ = self.events.send(e);
    }

    pub(crate) fn file_opener(&self) -> cc_platform_core::FileOpener {
        self.file_opener
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Argon2id parameters for new password envelopes (blocking when
    /// calibrating).
    pub(crate) async fn kdf(&self) -> AppResult<Argon2Params> {
        match self.config.kdf {
            KdfPolicy::Floor => Ok(Argon2Params::for_tests()),
            KdfPolicy::Default => Ok(Argon2Params::DEFAULT),
            KdfPolicy::Calibrate => {
                secrets::blocking(|| {
                    Ok(cc_vault_core::calibrate_argon2(
                        cc_vault_core::DEFAULT_CALIBRATION_TARGET,
                    )?)
                })
                .await
            }
        }
    }

    fn api_config(&self, server_url: &str) -> AppResult<ApiConfig> {
        Ok(ApiConfig::with_http_policy(
            server_url,
            self.config.client_version.clone(),
            self.config.platform,
            self.config.allow_insecure_http,
        )?)
    }

    /// Validate a server URL against the HTTP policy (no client created).
    pub(crate) fn check_server_url(&self, server_url: &str) -> AppResult<()> {
        self.api_config(server_url).map(|_| ())
    }

    /// API client of a profile: tokens in the secure store, and every
    /// authenticated request signed with the profile's current device key
    /// (protocol 1.5 request proofs; `identity` is shared with the session,
    /// so a regenerated identity signs from then on).
    pub(crate) fn api_for(
        &self,
        profile: ProfileId,
        server_url: &str,
        identity: &IdentityCell,
    ) -> AppResult<ApiClient> {
        Ok(ApiClient::new(
            self.api_config(server_url)?,
            Arc::new(SecureTokenStore::new(self.secure.clone(), profile)),
        )?
        .with_request_signer(Arc::new(DeviceRequestSigner {
            identity: identity.clone(),
        })))
    }
}

/// The profile's current device identity (replaced on re-authentication
/// with a new identity), shared by the session and its request signer.
pub(crate) type IdentityCell = Arc<RwLock<Arc<DeviceIdentity>>>;

pub(crate) fn identity_cell(identity: Arc<DeviceIdentity>) -> IdentityCell {
    Arc::new(RwLock::new(identity))
}

/// Per-request device proofs (`x-cc-device-proof`, protocol 1.5) with the
/// profile's device key: a stolen access/refresh token is useless without
/// this installation's key.
struct DeviceRequestSigner {
    identity: IdentityCell,
}

impl cc_sync_core::RequestSigner for DeviceRequestSigner {
    fn request_proof(
        &self,
        method: &str,
        path_and_query: &str,
        body: &[u8],
    ) -> Result<cc_protocol::devices::RequestProof, cc_sync_core::SignerError> {
        let identity = self
            .identity
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        identity
            .request_proof(method, path_and_query, body, chrono::Utc::now())
            .map_err(|e| cc_sync_core::SignerError::Failed(e.to_string()))
    }
}

/// A Recovery Kit waiting for the onboarding check.
pub(crate) struct PendingKit {
    pub kit: RecoveryKit,
    pub check: Option<RecoveryKitCheck>,
}

/// An open profile.
pub(crate) struct Session {
    pub ctx: Arc<AppCtx>,
    pub profile_id: ProfileId,
    pub storage: Storage,
    identity: IdentityCell,
    api: RwLock<Option<ApiClient>>,
    vault_id: RwLock<Option<VaultId>>,
    unlocked: tokio::sync::RwLock<Option<Arc<Unlocked>>>,
    pub pending_kit: Mutex<Option<PendingKit>>,
    /// Serializes sharing actions; contains no plaintext or device keys.
    pub sharing_gate: tokio::sync::Mutex<()>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("profile_id", &self.profile_id)
            .finish_non_exhaustive()
    }
}

/// The unlocked vault of a session.
pub(crate) struct Unlocked {
    pub vault_id: VaultId,
    pub codec: Arc<VaultCodec>,
    pub writer: VaultWriter,
    pub ssh: Arc<SshRuntime>,
    /// AI runtime: search index, providers, conversations, approvals.
    pub ai: Arc<crate::ai::AiSession>,
    /// "Edit in the default app" sessions (ADR-0108).
    pub edit: Arc<crate::edit::EditRuntime>,
    /// SFTP transfer jobs.
    pub transfers: Arc<crate::transfers::TransferManager>,
    /// In-flight sharing operations drop transient plaintext immediately on
    /// lock, before potentially slower SSH/edit shutdown finishes.
    pub sharing_shutdown: tokio::sync::watch::Sender<bool>,
    pub enrollment: crate::sharing_api::enrollment::EnrollmentRuntime,
    event_stream: tokio::sync::Mutex<Option<EventStream>>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    engine_tasks: Mutex<Vec<JoinHandle<()>>>,
    pub approvals: Mutex<HashMap<DeviceRequestId, PendingApproval>>,
}

impl std::fmt::Debug for Unlocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Unlocked")
            .field("vault_id", &self.vault_id)
            .finish_non_exhaustive()
    }
}

fn lock_mutex<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Unlocked {
    pub(crate) fn vault(&self) -> AppResult<Arc<UnlockedVault>> {
        self.codec.vault()
    }

    pub(crate) fn working(&self) -> &Arc<WorkingSet> {
        &self.writer.working
    }

    pub(crate) fn store(&self) -> &ObjectStore {
        &self.writer.store
    }

    pub(crate) fn engine(&self) -> Option<SyncEngine> {
        self.writer.engine()
    }

    /// Hand server vault info (list / get vault results) to the sync engine,
    /// which detects a server restored from an older backup (epoch changed,
    /// sequence regressed) and starts the rollback recovery.
    pub(crate) async fn observe_vault_info(&self, infos: &[cc_protocol::vaults::VaultInfo]) {
        let Some(engine) = self.engine() else { return };
        for info in infos.iter().filter(|i| i.vault_id == self.vault_id) {
            if let Err(e) = engine.observe_vault_info(info).await {
                tracing::warn!(error = %e, "vault info check failed");
            }
        }
    }

    fn push_task(&self, t: JoinHandle<()>) {
        lock_mutex(&self.tasks).push(t);
    }

    /// Stop the sync engine and the WebSocket (keeps the vault unlocked).
    pub(crate) async fn stop_engine(&self) {
        let engine = self
            .writer
            .engine
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(e) = engine {
            e.stop().await;
        }
        if let Some(es) = self.event_stream.lock().await.take() {
            es.shutdown().await;
        }
        for t in lock_mutex(&self.engine_tasks).drain(..) {
            t.abort();
        }
    }

    /// Lock: stop everything, zeroize keys, drop caches. Edit sessions go
    /// first (their final uploads need SFTP), then transfers.
    async fn shutdown(&self) {
        self.sharing_shutdown.send_replace(true);
        self.edit.begin_shutdown();
        self.enrollment.clear();
        self.edit.stop_all().await;
        self.transfers.cancel_all();
        self.ai.shutdown().await;
        self.ssh.shutdown().await;
        self.stop_engine().await;
        for t in lock_mutex(&self.tasks).drain(..) {
            t.abort();
        }
        lock_mutex(&self.approvals).clear();
        self.codec.clear();
        self.writer.working.clear();
    }
}

impl Session {
    /// Open an existing profile database (vault stays locked).
    pub(crate) async fn open(
        ctx: Arc<AppCtx>,
        profile_id: ProfileId,
        db_path: std::path::PathBuf,
    ) -> AppResult<Arc<Self>> {
        let key = secrets::load_db_key(ctx.secure.clone(), profile_id).await?;
        let storage = Storage::open(db_path, key).await?;
        Self::with_storage(ctx, profile_id, storage).await
    }

    pub(crate) async fn with_storage(
        ctx: Arc<AppCtx>,
        profile_id: ProfileId,
        storage: Storage,
    ) -> AppResult<Arc<Self>> {
        let identity = identity_cell(Arc::new(
            secrets::load_or_create_identity(ctx.secure.clone(), profile_id).await?,
        ));
        let profile = storage.get_profile().await?;
        let vault_id: Option<VaultId> = storage.setting_get(VAULT_ID_SETTING).await?;
        let api = match &profile {
            Some(p) if p.kind == ProfileKind::Synced => {
                let url = p
                    .server_url
                    .as_deref()
                    .ok_or_else(|| AppError::Storage("synced profile without server url".into()))?;
                Some(ctx.api_for(profile_id, url, &identity)?)
            }
            _ => None,
        };
        Ok(Arc::new(Self {
            ctx,
            profile_id,
            storage,
            identity,
            api: RwLock::new(api),
            vault_id: RwLock::new(vault_id),
            unlocked: tokio::sync::RwLock::new(None),
            pending_kit: Mutex::new(None),
            sharing_gate: tokio::sync::Mutex::new(()),
        }))
    }

    /// Remember a Recovery Kit awaiting its check (memory + persisted flag).
    pub(crate) async fn set_pending_kit(&self, kit: Option<PendingKit>) -> AppResult<()> {
        let pending = kit.is_some();
        *lock_mutex(&self.pending_kit) = kit;
        self.storage
            .setting_set(KIT_PENDING_SETTING, pending)
            .await?;
        if pending {
            self.ctx.emit(AppEvent::ProfilesChanged);
        }
        Ok(())
    }

    /// Whether the Recovery Kit check is still pending (persisted flag).
    pub(crate) async fn kit_check_pending(&self) -> bool {
        self.storage
            .setting_get::<bool>(KIT_PENDING_SETTING)
            .await
            .ok()
            .flatten()
            .unwrap_or(false)
    }

    pub(crate) fn identity(&self) -> Arc<DeviceIdentity> {
        self.identity
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub(crate) fn set_identity(&self, identity: Arc<DeviceIdentity>) {
        *self.identity.write().unwrap_or_else(|p| p.into_inner()) = identity;
    }

    /// The identity cell shared with this profile's request signer.
    pub(crate) fn identity_cell(&self) -> IdentityCell {
        self.identity.clone()
    }

    pub(crate) fn device_id(&self) -> DeviceId {
        self.identity().device_id()
    }

    pub(crate) fn api_opt(&self) -> Option<ApiClient> {
        self.api.read().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// API client of a synced profile.
    pub(crate) fn api(&self) -> AppResult<ApiClient> {
        self.api_opt().ok_or(AppError::LocalProfile)
    }

    pub(crate) fn set_api(&self, api: Option<ApiClient>) {
        *self.api.write().unwrap_or_else(|p| p.into_inner()) = api;
    }

    pub(crate) fn vault_id(&self) -> Option<VaultId> {
        *self.vault_id.read().unwrap_or_else(|p| p.into_inner())
    }

    pub(crate) fn require_vault_id(&self) -> AppResult<VaultId> {
        self.vault_id().ok_or(AppError::NoVault)
    }

    pub(crate) async fn set_vault_id(&self, vault_id: VaultId) -> AppResult<()> {
        self.storage.setting_set(VAULT_ID_SETTING, vault_id).await?;
        *self.vault_id.write().unwrap_or_else(|p| p.into_inner()) = Some(vault_id);
        Ok(())
    }

    pub(crate) async fn profile(&self) -> AppResult<Profile> {
        self.storage
            .get_profile()
            .await?
            .ok_or_else(|| AppError::Storage("profile row missing".into()))
    }

    pub(crate) async fn kind(&self) -> AppResult<ProfileKind> {
        Ok(self.profile().await?.kind)
    }

    pub(crate) async fn unlocked_opt(&self) -> Option<Arc<Unlocked>> {
        self.unlocked.read().await.clone()
    }

    pub(crate) async fn unlocked(&self) -> AppResult<Arc<Unlocked>> {
        match self.unlocked_opt().await {
            Some(u) => Ok(u),
            None if self.vault_id().is_none() => Err(AppError::NoVault),
            None => Err(AppError::VaultLocked),
        }
    }

    /// Cache envelopes (as the server/local `KeyEnvelope` shape).
    pub(crate) async fn cache_envelopes(
        &self,
        vault_id: VaultId,
        password: Option<KeyEnvelope>,
        recovery: Option<KeyEnvelope>,
        device: Option<KeyEnvelope>,
    ) -> AppResult<()> {
        Ok(self
            .storage
            .cache_vault_envelopes(vault_id, password, recovery, device)
            .await?)
    }

    /// Install an unlocked vault: codec, store, working set, SSH runtime,
    /// event forwarding and (synced + background sync) the engine.
    pub(crate) async fn install_unlocked(
        self: &Arc<Self>,
        vault: UnlockedVault,
    ) -> AppResult<Arc<Unlocked>> {
        let vault_id = vault.vault_id();
        if let Some(expected) = self.vault_id() {
            if expected != vault_id {
                return Err(AppError::internal(
                    "unlocked vault does not match the profile",
                ));
            }
        }
        if let Some(existing) = self.unlocked_opt().await {
            return Ok(existing);
        }
        let codec = VaultCodec::new(vault);
        let store = ObjectStore::new(vault_id, self.storage.clone(), codec.clone());
        let engine_slot: EngineSlot = Arc::new(RwLock::new(None));
        let writer = VaultWriter {
            store: store.clone(),
            engine: engine_slot,
            working: Arc::new(WorkingSet::new()),
        };
        // Subscribe before loading so no change is missed.
        let rx = store.subscribe();
        writer.reload_all().await?;
        let ssh = Arc::new(SshRuntime::new(writer.clone(), self.ctx.prompts.clone()));
        let ai = crate::ai::AiSession::new(
            self.ctx.clone(),
            self.profile_id,
            self.ctx
                .dirs
                .profile(&self.profile_id.to_string())
                .ok()
                .map(|p| p.search_db()),
            writer.clone(),
            ssh.clone(),
        );
        let edit_root = self
            .ctx
            .dirs
            .profile(&self.profile_id.to_string())
            .ok()
            .map(|p| cc_sftp_core::edit::edit_root(&p));
        let edit = crate::edit::EditRuntime::new(&self.ctx, edit_root);
        let unlocked = Arc::new(Unlocked {
            vault_id,
            codec,
            writer: writer.clone(),
            ssh: ssh.clone(),
            ai: ai.clone(),
            edit: edit.clone(),
            transfers: crate::transfers::TransferManager::new(self.ctx.clone()),
            sharing_shutdown: tokio::sync::watch::channel(false).0,
            enrollment: crate::sharing_api::enrollment::EnrollmentRuntime::default(),
            event_stream: tokio::sync::Mutex::new(None),
            tasks: Mutex::new(Vec::new()),
            engine_tasks: Mutex::new(Vec::new()),
            approvals: Mutex::new(HashMap::new()),
        });
        unlocked.push_task(tokio::spawn(object_event_task(
            writer,
            rx,
            self.ctx.clone(),
            self.device_id(),
        )));
        unlocked.push_task(tokio::spawn(terminal_event_task(
            ssh.clone(),
            self.ctx.clone(),
        )));
        unlocked.push_task(tokio::spawn(tunnel_event_task(ssh, self.ctx.clone())));
        if let Some(t) = edit.spawn_forwarder(self.ctx.clone()) {
            unlocked.push_task(t);
        }
        ai.start();
        {
            let mut slot = self.unlocked.write().await;
            if let Some(existing) = slot.as_ref() {
                // Lost a race with a concurrent unlock: keep the first one.
                let existing = existing.clone();
                drop(slot);
                unlocked.shutdown().await;
                return Ok(existing);
            }
            *slot = Some(unlocked.clone());
        }
        let _ = self
            .storage
            .write(move |tx| tx.mark_vault_unlocked(vault_id))
            .await;
        if self.kind().await? == ProfileKind::Synced {
            self.attest_if_needed(&unlocked).await;
        }
        if self.kind().await? == ProfileKind::Synced && self.ctx.config.background_sync {
            if let Err(e) = self.start_engine(&unlocked, true).await {
                tracing::warn!(error = %e, "sync engine not started");
            }
        }
        self.ctx.emit(AppEvent::VaultUnlocked {
            profile_id: self.profile_id.to_string(),
        });
        let leftovers = unlocked.edit.leftover_count();
        if leftovers > 0 {
            self.ctx.emit(AppEvent::EditLeftovers {
                count: u32::try_from(leftovers).unwrap_or(u32::MAX),
            });
        }
        if self.ctx.config.auto_start_tunnels {
            let u = unlocked.clone();
            let ctx = self.ctx.clone();
            unlocked.push_task(tokio::spawn(async move {
                crate::ssh_api::auto_start_tunnels(&u, &ctx).await;
            }));
        }
        Ok(unlocked)
    }

    /// Create (and optionally start) the sync engine + WebSocket for a
    /// synced profile. Returns the engine (existing one if present).
    pub(crate) async fn start_engine(
        self: &Arc<Self>,
        unlocked: &Arc<Unlocked>,
        background: bool,
    ) -> AppResult<SyncEngine> {
        if let Some(e) = unlocked.engine() {
            if e.stop_reason().is_none() {
                if background {
                    e.start();
                }
                return Ok(e);
            }
            unlocked.stop_engine().await;
        }
        let engine =
            SyncEngine::new(unlocked.store().clone(), self.api()?, self.engine_config()).await?;
        self.install_engine(unlocked, engine.clone(), background)
            .await?;
        Ok(engine)
    }

    pub(crate) fn engine_config(&self) -> SyncEngineConfig {
        let mut cfg = SyncEngineConfig::new(self.device_id());
        cfg.periodic_interval = match self.ctx.config.sync_interval_secs {
            0 => None,
            s => Some(Duration::from_secs(s)),
        };
        cfg
    }

    /// Put `engine` into the session (status forwarding; with `background`
    /// also the WebSocket event stream and the worker).
    pub(crate) async fn install_engine(
        self: &Arc<Self>,
        unlocked: &Arc<Unlocked>,
        engine: SyncEngine,
        background: bool,
    ) -> AppResult<()> {
        let api = self.api()?;
        *unlocked
            .writer
            .engine
            .write()
            .unwrap_or_else(|p| p.into_inner()) = Some(engine.clone());
        let mut tasks = vec![tokio::spawn(status_task(engine.clone(), self.ctx.clone()))];
        if background {
            let es = EventStream::spawn(api, EventStreamConfig::default());
            tasks.push(engine.attach_events(es.subscribe()));
            tasks.push(tokio::spawn(ws_event_task(
                es.subscribe(),
                self.ctx.clone(),
                self.device_id(),
            )));
            *unlocked.event_stream.lock().await = Some(es);
            engine.start();
        }
        lock_mutex(&unlocked.engine_tasks).extend(tasks);
        Ok(())
    }

    /// Attest this device for the vault if a new device identity was
    /// generated while the vault was locked (best effort, online only).
    async fn attest_if_needed(&self, unlocked: &Unlocked) {
        let needed: bool = self
            .storage
            .setting_get(NEEDS_ATTEST_SETTING)
            .await
            .ok()
            .flatten()
            .unwrap_or(false);
        if !needed {
            return;
        }
        let Some(api) = self.api_opt() else { return };
        let r = async {
            let vault = unlocked.vault()?;
            let identity = self.identity();
            let req = vault.attest_device_request(&identity)?;
            let env = api.attest_device(identity.device_id(), &req).await?;
            self.cache_envelopes(unlocked.vault_id, None, None, Some(env))
                .await?;
            self.storage
                .setting_set(NEEDS_ATTEST_SETTING, false)
                .await?;
            Ok::<_, AppError>(())
        }
        .await;
        match r {
            Ok(()) => tracing::info!("device attested for the vault after identity change"),
            Err(e) => tracing::warn!(error = %e, "deferred attestation failed"),
        }
    }

    /// Lock the vault (no-op if locked).
    pub(crate) async fn lock(&self) {
        let taken = self.unlocked.write().await.take();
        if let Some(u) = taken {
            u.shutdown().await;
            self.ctx.emit(AppEvent::VaultLocked {
                profile_id: self.profile_id.to_string(),
            });
        }
        lock_mutex(&self.pending_kit).take();
    }

    /// Close the profile: lock, then close the database synchronously
    /// (the session is unusable afterwards).
    pub(crate) async fn close(&self) {
        self.lock().await;
        self.storage.close().await;
    }

    /// Current sync status.
    pub(crate) async fn sync_status(&self) -> AppResult<SyncStatusDto> {
        if self.kind().await? == ProfileKind::Local {
            return Ok(SyncStatusDto::local_only());
        }
        match self.unlocked_opt().await.and_then(|u| u.engine()) {
            Some(e) => Ok(SyncStatusDto::from_status(&e.status())),
            None => Ok(SyncStatusDto::with_phase(
                crate::dto::SyncPhaseDto::NotRunning,
            )),
        }
    }
}

// ---- background tasks ------------------------------------------------------------

async fn emit_reload(writer: &VaultWriter, ctx: &AppCtx) {
    if let Err(e) = writer.reload_all().await {
        tracing::warn!(error = %e, "working set reload failed");
        return;
    }
    let mut by_kind: HashMap<ObjectKind, Vec<String>> = HashMap::new();
    for kind in [
        ObjectKind::Host,
        ObjectKind::Group,
        ObjectKind::JumpProfile,
        ObjectKind::Proxy,
        ObjectKind::Credential,
        ObjectKind::Tunnel,
        ObjectKind::KnownHost,
        ObjectKind::Snippet,
        ObjectKind::Note,
        ObjectKind::VaultSettings,
        ObjectKind::AiProvider,
    ] {
        let ids: Vec<String> = writer
            .working
            .ids_of(kind)
            .iter()
            .map(ToString::to_string)
            .collect();
        by_kind.insert(kind, ids);
    }
    for (kind, ids) in by_kind {
        ctx.emit(AppEvent::ObjectsChanged {
            kind,
            ids,
            origin: ChangeOriginDto::Remote,
        });
    }
}

/// Keeps the working set consistent with store/engine events and forwards
/// them as [`AppEvent`]s.
async fn object_event_task(
    writer: VaultWriter,
    mut rx: broadcast::Receiver<SyncEvent>,
    ctx: Arc<AppCtx>,
    own_device: DeviceId,
) {
    loop {
        match rx.recv().await {
            Ok(SyncEvent::ObjectChanged {
                object_id, origin, ..
            }) => {
                let origin = match origin {
                    cc_sync_core::ChangeOrigin::Local => ChangeOriginDto::Local,
                    cc_sync_core::ChangeOrigin::Remote => ChangeOriginDto::Remote,
                    cc_sync_core::ChangeOrigin::ConflictResolution => {
                        ChangeOriginDto::ConflictResolution
                    }
                };
                match writer.refresh(object_id).await {
                    Ok(Some(kind)) => ctx.emit(AppEvent::ObjectsChanged {
                        kind,
                        ids: vec![object_id.to_string()],
                        origin,
                    }),
                    Ok(None) => {}
                    Err(AppError::VaultLocked) => break,
                    Err(e) => {
                        tracing::warn!(object_id = %object_id, error = %e, "object refresh failed")
                    }
                }
            }
            Ok(SyncEvent::ObjectRemoved { object_id, .. }) => {
                if let Some(kind) = writer.working.apply(object_id, Readback::Missing) {
                    ctx.emit(AppEvent::ObjectsChanged {
                        kind,
                        ids: vec![object_id.to_string()],
                        origin: ChangeOriginDto::Remote,
                    });
                }
            }
            Ok(SyncEvent::ConflictResolved {
                object_id,
                conflict_copy,
                kek_class,
                ..
            }) => {
                let mut kinds: Vec<(ObjectKind, ObjectId)> = Vec::new();
                for id in std::iter::once(object_id).chain(conflict_copy) {
                    if let Ok(Some(kind)) = writer.refresh(id).await {
                        kinds.push((kind, id));
                    }
                }
                for (kind, id) in kinds {
                    ctx.emit(AppEvent::ObjectsChanged {
                        kind,
                        ids: vec![id.to_string()],
                        origin: ChangeOriginDto::ConflictResolution,
                    });
                }
                ctx.emit(AppEvent::ConflictResolved {
                    object_id: object_id.to_string(),
                    conflict_copy_id: conflict_copy.map(|c| c.to_string()),
                    is_secret: kek_class == Some(KekClass::Secrets),
                });
            }
            Ok(SyncEvent::IntegrityWarning { object_id, .. }) => {
                tracing::warn!(object_id = %object_id, "server object failed verification");
                ctx.emit(AppEvent::IntegrityWarning {
                    object_id: object_id.to_string(),
                });
            }
            Ok(SyncEvent::ServerRollbackDetected { vault_id, reason }) => {
                tracing::warn!(reason = ?reason, "server state rolled back; re-uploading local data");
                ctx.emit(AppEvent::ServerRollbackDetected {
                    vault_id: vault_id.to_string(),
                    reason: crate::error::protocol_code_name_of(&reason),
                });
            }
            Ok(SyncEvent::SnapshotCompleted { .. }) => emit_reload(&writer, &ctx).await,
            Ok(SyncEvent::Stopped { reason, .. }) => {
                ctx.emit(AppEvent::SyncStopped {
                    reason: reason.into(),
                });
                match reason {
                    StopReason::DeviceRevoked => ctx.emit(AppEvent::DeviceRevoked {
                        device_id: own_device.to_string(),
                        is_self: true,
                    }),
                    StopReason::ReauthRequired => ctx.emit(AppEvent::ReauthRequired),
                    _ => {}
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => emit_reload(&writer, &ctx).await,
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

async fn status_task(engine: SyncEngine, ctx: Arc<AppCtx>) {
    let mut rx = engine.subscribe_status();
    loop {
        let s = SyncStatusDto::from_status(&rx.borrow_and_update().clone());
        ctx.emit(AppEvent::SyncStatus(s));
        if rx.changed().await.is_err() {
            break;
        }
    }
}

async fn ws_event_task(mut rx: broadcast::Receiver<WsEvent>, ctx: Arc<AppCtx>, own: DeviceId) {
    loop {
        match rx.recv().await {
            Ok(WsEvent::Event(ev)) => match ev {
                ServerEvent::DeviceApprovalRequested {
                    request_id,
                    device_id,
                } => ctx.emit(AppEvent::DeviceApprovalRequested {
                    request_id: request_id.to_string(),
                    device_id: device_id.to_string(),
                }),
                ServerEvent::DeviceApproved { device_id, .. } => {
                    ctx.emit(AppEvent::DeviceApproved {
                        device_id: device_id.to_string(),
                        is_self: device_id == own,
                    })
                }
                ServerEvent::DeviceRevoked { device_id } => ctx.emit(AppEvent::DeviceRevoked {
                    device_id: device_id.to_string(),
                    is_self: device_id == own,
                }),
                ServerEvent::RecoveryChanged { .. } => ctx.emit(AppEvent::RecoveryChanged),
                _ => {}
            },
            Ok(WsEvent::Terminated { reason }) => {
                if reason == StopReason::ReauthRequired {
                    ctx.emit(AppEvent::ReauthRequired);
                }
                break;
            }
            Ok(_) => {}
            Err(broadcast::error::RecvError::Lagged(_)) => {}
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

async fn terminal_event_task(ssh: Arc<SshRuntime>, ctx: Arc<AppCtx>) {
    let mut rx = ssh.terminals.events();
    loop {
        match rx.recv().await {
            Ok(ev) => ctx.emit(AppEvent::TerminalStatus {
                terminal_id: ev.id.to_string(),
                status: (&ev.status).into(),
            }),
            Err(broadcast::error::RecvError::Lagged(_)) => {}
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

async fn tunnel_event_task(ssh: Arc<SshRuntime>, ctx: Arc<AppCtx>) {
    use cc_tunnel_core::TunnelEvent;
    let mut rx = ssh.tunnels.events();
    loop {
        match rx.recv().await {
            Ok(TunnelEvent::Started(id)) => ctx.emit(AppEvent::TunnelStarted {
                tunnel_id: id.to_string(),
            }),
            Ok(TunnelEvent::Stopped(id)) => ctx.emit(AppEvent::TunnelStopped {
                tunnel_id: id.to_string(),
            }),
            Ok(TunnelEvent::Failed { id, reason }) => ctx.emit(AppEvent::TunnelFailed {
                tunnel_id: id.to_string(),
                reason,
            }),
            Err(broadcast::error::RecvError::Lagged(_)) => {}
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

use cc_models::ObjectId;
