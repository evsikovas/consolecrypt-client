//! An open profile ([`Session`]) and its unlocked-vault state
//! ([`Unlocked`]): storage, device identity, API client (synced profiles),
//! codec, working set, sync engine, WebSocket events and the SSH runtime.

use crate::codec::VaultCodec;
use crate::dto::{AppEvent, ChangeOriginDto, SyncStatusDto};
use crate::error::{AppError, AppResult};
use crate::keyfile::{self, KeyFile, LockedHeader, ReofferReason};
use crate::prompts::PromptBroker;
use crate::secrets::{self, SecureTokenStore};
use crate::ssh::SshRuntime;
use crate::working_set::{Readback, WorkingSet};
use crate::writer::{EngineSlot, VaultWriter};
use crate::{AppConfig, KdfPolicy};
use cc_models::{KekClass, ObjectKind};
use cc_platform_core::{DeviceKeyProtector, SecureStore};
use cc_protocol::envelopes::KeyEnvelope;
use cc_protocol::events::ServerEvent;
use cc_protocol::{DeviceId, DeviceRequestId, VaultId};
use cc_storage_core::{DatabaseKey, Profile, ProfileId, ProfileKind, Storage, StorageError};
use cc_sync_core::{
    ApiClient, ApiConfig, EventStream, EventStreamConfig, ObjectStore, StopReason, SyncEngine,
    SyncEngineConfig, SyncEvent, WsEvent,
};
use cc_vault_core::{
    Argon2Params, DeviceIdentity, PendingApproval, RecoveryKit, RecoveryKitCheck, UnlockedVault,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use zeroize::Zeroizing;

/// Local setting holding the profile's vault id.
pub(crate) const VAULT_ID_SETTING: &str = "app_core.vault_id";
/// Local setting: this (new) device identity must attest on next unlock.
pub(crate) const NEEDS_ATTEST_SETTING: &str = "app_core.needs_attest";
/// Local setting: a Recovery Kit was shown and its 3-word check is still
/// pending (survives restarts; the kit itself is kept in memory only).
pub(crate) const KIT_PENDING_SETTING: &str = "app_core.recovery_kit_pending";
/// Legacy (≤ 0.8.x) device-unlock opt-in. Read once while migrating a
/// profile to OS-protected device unlock (S5), then removed.
pub(crate) const LEGACY_DEVICE_UNLOCK_SETTING: &str = "app_core.device_unlock_enabled";

/// App-wide context shared by every session.
pub(crate) struct AppCtx {
    pub config: AppConfig,
    /// Data layout (per-profile paths, e.g. the AI search index).
    pub dirs: cc_platform_core::AppDirs,
    pub secure: Arc<dyn SecureStore>,
    /// OS-protected device unlock keys (Secure Enclave, Windows Hello, …).
    pub device_keys: Arc<dyn DeviceKeyProtector>,
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
///
/// With a key file (S5) the database is **sealed** until the vault is
/// unlocked: `storage` is `None`, the locked state is served from the key
/// file, and locking closes the database again. Legacy (≤ 0.8.x) and
/// vault-less profiles open their database with the OS-stored key until
/// they are sealed ([`Session::seal_storage`]).
pub(crate) struct Session {
    pub ctx: Arc<AppCtx>,
    pub profile_id: ProfileId,
    storage: RwLock<Option<Storage>>,
    /// Profile directory (`None` for in-memory test storage).
    dir: Option<PathBuf>,
    /// Cached key file (`None` = legacy or vault-less profile).
    keyfile: Mutex<Option<KeyFile>>,
    /// Search index key derived from the database key (sealed profiles).
    index_key: Mutex<Option<Zeroizing<[u8; 32]>>>,
    /// Serializes database open / seal / close transitions.
    transition: tokio::sync::Mutex<()>,
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
    /// RDP sessions belong to this unlock lifetime, never to the global bridge.
    pub rdp: Arc<cc_rdp_core::RdpManager>,
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
    pub kube_imports: Arc<crate::kubernetes_import::ImportRuntime>,
    pub kube: crate::kubernetes_runtime::KubeRuntime,
    pub db: crate::database_runtime::DbRuntime,
    pub(crate) db_imports: crate::database_import::DbImports,
    pub(crate) db_ai: crate::database_ai::DbAiRuntime,
    pub(crate) docker: crate::docker_runtime::DockerRuntime,
    pub db_history_lock: tokio::sync::Mutex<()>,
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
        self.ssh.terminals.begin_shutdown();
        // Fence in-flight RDP connects before waiting for SFTP/SSH cleanup.
        self.rdp.shutdown();
        self.edit.begin_shutdown();
        self.enrollment.clear();
        self.kube_imports.close();
        self.kube.close();
        self.db.close();
        self.db_imports.clear();
        self.db_ai.close();
        self.docker.close();
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
    /// Open an existing profile (vault stays locked). With a key file the
    /// database stays sealed until unlock; otherwise (legacy / vault-less)
    /// it opens with the OS-stored key.
    pub(crate) async fn open(
        ctx: Arc<AppCtx>,
        profile_id: ProfileId,
        db_path: std::path::PathBuf,
    ) -> AppResult<Arc<Self>> {
        let dir = db_path
            .parent()
            .map(std::path::Path::to_path_buf)
            .ok_or_else(|| AppError::internal("profile database without a directory"))?;
        let d = dir.clone();
        if let Some(kf) = secrets::blocking(move || keyfile::load(&d)).await? {
            return Self::sealed(ctx, profile_id, dir, kf).await;
        }
        let key = secrets::load_db_key(ctx.secure.clone(), profile_id).await?;
        let storage = Storage::open(db_path, key).await?;
        Self::with_storage(ctx, profile_id, storage).await
    }

    /// A profile whose database stays closed until unlock.
    async fn sealed(
        ctx: Arc<AppCtx>,
        profile_id: ProfileId,
        dir: PathBuf,
        kf: KeyFile,
    ) -> AppResult<Arc<Self>> {
        let identity = identity_cell(Arc::new(
            secrets::load_or_create_identity(ctx.secure.clone(), profile_id).await?,
        ));
        let api = match (&kf.header.kind, &kf.header.server_url) {
            (ProfileKind::Synced, Some(url)) => Some(ctx.api_for(profile_id, url, &identity)?),
            (ProfileKind::Synced, None) => {
                return Err(AppError::Storage(
                    "synced profile without server url".into(),
                ))
            }
            _ => None,
        };
        let vault_id = kf.vault_id;
        Ok(Arc::new(Self {
            ctx,
            profile_id,
            storage: RwLock::new(None),
            dir: Some(dir),
            keyfile: Mutex::new(Some(kf)),
            index_key: Mutex::new(None),
            transition: tokio::sync::Mutex::new(()),
            identity,
            api: RwLock::new(api),
            vault_id: RwLock::new(Some(vault_id)),
            unlocked: tokio::sync::RwLock::new(None),
            pending_kit: Mutex::new(None),
            sharing_gate: tokio::sync::Mutex::new(()),
        }))
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
        let dir = storage
            .path()
            .and_then(std::path::Path::parent)
            .map(std::path::Path::to_path_buf);
        let kf = match &dir {
            Some(d) => {
                let d = d.clone();
                secrets::blocking(move || keyfile::load(&d)).await?
            }
            None => None,
        };
        Ok(Arc::new(Self {
            ctx,
            profile_id,
            storage: RwLock::new(Some(storage)),
            dir,
            keyfile: Mutex::new(kf),
            index_key: Mutex::new(None),
            transition: tokio::sync::Mutex::new(()),
            identity,
            api: RwLock::new(api),
            vault_id: RwLock::new(vault_id),
            unlocked: tokio::sync::RwLock::new(None),
            pending_kit: Mutex::new(None),
            sharing_gate: tokio::sync::Mutex::new(()),
        }))
    }

    // ---- storage & key file ------------------------------------------------------------

    /// The open database; [`AppError::VaultLocked`] while sealed.
    pub(crate) fn storage(&self) -> AppResult<Storage> {
        self.storage_opt().ok_or(AppError::VaultLocked)
    }

    pub(crate) fn storage_opt(&self) -> Option<Storage> {
        self.storage
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Whether the profile uses a key file (its database is sealed while
    /// locked).
    pub(crate) fn has_keyfile(&self) -> bool {
        lock_mutex(&self.keyfile).is_some()
    }

    /// A copy of the key file.
    pub(crate) fn keyfile(&self) -> Option<KeyFile> {
        lock_mutex(&self.keyfile).clone()
    }

    /// Change and persist the key file (no-op without one).
    pub(crate) async fn update_keyfile<F>(&self, f: F) -> AppResult<()>
    where
        F: FnOnce(&mut KeyFile),
    {
        let (dir, kf) = {
            let mut g = lock_mutex(&self.keyfile);
            let Some(kf) = g.as_mut() else {
                return Ok(());
            };
            f(kf);
            (self.dir.clone(), kf.clone())
        };
        let dir = dir.ok_or_else(|| AppError::internal("key file without a directory"))?;
        secrets::blocking(move || keyfile::store(&dir, &kf)).await
    }

    async fn write_keyfile(&self, kf: KeyFile) -> AppResult<()> {
        let dir = self
            .dir
            .clone()
            .ok_or_else(|| AppError::internal("in-memory profile cannot be sealed"))?;
        let k = kf.clone();
        secrets::blocking(move || keyfile::store(&dir, &k)).await?;
        *lock_mutex(&self.keyfile) = Some(kf);
        Ok(())
    }

    /// Search index key of a sealed profile (derived from the database key);
    /// `None` = legacy OS-store key.
    pub(crate) fn index_key(&self) -> Option<Zeroizing<[u8; 32]>> {
        lock_mutex(&self.index_key).clone()
    }

    fn set_db_key(&self, key: &DatabaseKey) {
        *lock_mutex(&self.index_key) = Some(cc_crypto_core::derive_local_subkey(
            key.expose_bytes(),
            b"search-index",
        ));
    }

    /// Open the sealed database with the key unwrapped by `vault` (any
    /// unlock method). Finishes an interrupted re-key: `db_key_next` is
    /// tried when `db_key` does not fit, and promoted. No-op if open.
    pub(crate) async fn open_sealed_storage(&self, vault: &UnlockedVault) -> AppResult<()> {
        let _t = self.transition.lock().await;
        if self.storage_opt().is_some() {
            return Ok(());
        }
        let kf = self.keyfile().ok_or(AppError::VaultLocked)?;
        if kf.vault_id != vault.vault_id() {
            return Err(AppError::internal(
                "unlocked vault does not match the profile",
            ));
        }
        let dir = self
            .dir
            .clone()
            .ok_or_else(|| AppError::internal("sealed profile without a directory"))?;
        let path = dir.join(cc_storage_core::PROFILE_DB_FILE);
        let ctx = keyfile::db_key_context(self.profile_id);
        let unwrap = |sealed: &cc_vault_core::LocalSealed| -> AppResult<DatabaseKey> {
            let bytes = vault.open_local_secret(&ctx, sealed)?;
            DatabaseKey::from_slice(&bytes)
                .ok_or_else(|| AppError::Storage("stored database key is corrupted".into()))
        };
        let current = unwrap(&kf.db_key)?;
        let next = kf.db_key_next.as_ref().map(unwrap).transpose()?;
        let (storage, key) = match Storage::open(&path, current.clone()).await {
            Ok(st) => match next {
                // Interrupted before the re-key: run it now.
                Some(next) => {
                    st.rekey(next.clone()).await?;
                    (st, next)
                }
                None => (st, current),
            },
            Err(StorageError::WrongKeyOrNotADatabase) if next.is_some() => {
                // Interrupted after the re-key.
                let next = next.unwrap_or(current);
                (Storage::open(&path, next.clone()).await?, next)
            }
            Err(e) => return Err(e.into()),
        };
        if kf.db_key_next.is_some() {
            let sealed = vault.seal_local_secret(&ctx, key.expose_bytes())?;
            self.update_keyfile(|k| {
                k.db_key = sealed;
                k.db_key_next = None;
            })
            .await?;
        }
        self.set_db_key(&key);
        *self.storage.write().unwrap_or_else(|p| p.into_inner()) = Some(storage);
        Ok(())
    }

    /// Seal the database of a legacy or vault-less profile under `vault`
    /// (S5 TR-4/TR-5), transactionally:
    /// 1. key file with the current key and the next key (both sealed);
    /// 2. `PRAGMA rekey` to the next key (atomic);
    /// 3. key file with only the next key;
    /// 4. the OS-stored database / search-index keys are deleted.
    ///
    /// A crash between any two steps leaves a profile that opens: the key
    /// file always holds a key that fits ([`Session::open_sealed_storage`]).
    /// A legacy device-unlock opt-in becomes a "turn it on again" offer;
    /// the legacy local device envelope is dropped.
    pub(crate) async fn seal_storage(&self, vault: &UnlockedVault) -> AppResult<()> {
        let _t = self.transition.lock().await;
        if self.has_keyfile() {
            return Ok(());
        }
        let storage = self.storage()?;
        let vault_id = vault.vault_id();
        let ctx = keyfile::db_key_context(self.profile_id);
        let old = secrets::load_db_key(self.ctx.secure.clone(), self.profile_id).await?;
        let next = secrets::random_db_key()?;
        let profile = storage
            .get_profile()
            .await?
            .ok_or_else(|| AppError::Storage("profile row missing".into()))?;
        let record = storage.get_vault(vault_id).await?;
        let kit_pending: bool = storage
            .setting_get(KIT_PENDING_SETTING)
            .await?
            .unwrap_or(false);
        let needs_attest: bool = storage
            .setting_get(NEEDS_ATTEST_SETTING)
            .await?
            .unwrap_or(false);
        let legacy_device_unlock: bool = storage
            .setting_get(LEGACY_DEVICE_UNLOCK_SETTING)
            .await?
            .unwrap_or(false);
        let mut kf = KeyFile::new(
            vault_id,
            LockedHeader::from_profile(&profile),
            vault.seal_local_secret(&ctx, old.expose_bytes())?,
        );
        kf.db_key_next = Some(vault.seal_local_secret(&ctx, next.expose_bytes())?);
        if let Some(rec) = &record {
            kf.password_envelope = rec.password_envelope.clone();
            kf.recovery_envelope = rec.recovery_envelope.clone();
        }
        kf.flags.kit_pending = kit_pending;
        kf.flags.needs_attest = needs_attest;
        if legacy_device_unlock {
            kf.flags.reoffer_device_unlock = Some(ReofferReason::Migrated);
        }
        // 1.
        self.write_keyfile(kf).await?;
        // 2.
        storage.rekey(next.clone()).await?;
        // 3.
        let sealed_next = vault.seal_local_secret(&ctx, next.expose_bytes())?;
        self.update_keyfile(|k| {
            k.db_key = sealed_next;
            k.db_key_next = None;
        })
        .await?;
        self.set_db_key(&next);
        // Legacy leftovers inside the (now re-keyed) database.
        storage
            .write(move |tx| {
                if let Some(mut rec) = tx.get_vault(vault_id)? {
                    if rec.device_envelope.is_some() {
                        rec.device_envelope = None;
                        tx.put_vault(&rec)?;
                    }
                }
                tx.setting_delete(LEGACY_DEVICE_UNLOCK_SETTING)
            })
            .await?;
        // 4.
        if let Err(e) = secrets::delete_storage_keys(self.ctx.secure.clone(), self.profile_id).await
        {
            tracing::warn!(error = %e, "old OS-stored database key could not be deleted");
        }
        tracing::info!(profile_id = %self.profile_id, "profile database sealed under the vault key");
        Ok(())
    }

    /// Close the database of a sealed profile (after lock). Keeps the
    /// locked-state header current.
    async fn reseal(&self) {
        let _t = self.transition.lock().await;
        if !self.has_keyfile() {
            return;
        }
        let taken = self
            .storage
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(storage) = taken {
            if let Ok(Some(p)) = storage.get_profile().await {
                let header = LockedHeader::from_profile(&p);
                if let Err(e) = self.update_keyfile(|k| k.header = header).await {
                    tracing::warn!(error = %e, "profile key file header not updated");
                }
            }
            storage.close().await;
        }
        *lock_mutex(&self.index_key) = None;
    }

    /// After a sealed unlock: the header may be newer than the database
    /// (renamed / re-authenticated while locked).
    pub(crate) async fn reconcile_header(&self) -> AppResult<()> {
        let (Some(storage), Some(kf)) = (self.storage_opt(), self.keyfile()) else {
            return Ok(());
        };
        let Some(db) = storage.get_profile().await? else {
            return Ok(());
        };
        if kf.header.updated_at > db.updated_at {
            storage
                .put_profile(kf.header.to_profile(self.profile_id))
                .await?;
        } else if LockedHeader::from_profile(&db) != kf.header {
            let header = LockedHeader::from_profile(&db);
            self.update_keyfile(|k| k.header = header).await?;
        }
        Ok(())
    }

    /// Store the profile row (database when open, key-file header while
    /// sealed).
    pub(crate) async fn put_profile(&self, mut profile: Profile) -> AppResult<()> {
        profile.updated_at = chrono::Utc::now();
        match self.storage_opt() {
            Some(st) => {
                st.put_profile(profile.clone()).await?;
                if self.has_keyfile() {
                    let header = LockedHeader::from_profile(&profile);
                    self.update_keyfile(|k| k.header = header).await?;
                }
                Ok(())
            }
            None => {
                let header = LockedHeader::from_profile(&profile);
                self.update_keyfile(|k| k.header = header).await
            }
        }
    }

    /// "Attest on the next unlock" flag.
    pub(crate) async fn needs_attest(&self) -> bool {
        if let Some(kf) = self.keyfile() {
            return kf.flags.needs_attest;
        }
        match self.storage_opt() {
            Some(st) => st
                .setting_get(NEEDS_ATTEST_SETTING)
                .await
                .ok()
                .flatten()
                .unwrap_or(false),
            None => false,
        }
    }

    pub(crate) async fn set_needs_attest(&self, needed: bool) -> AppResult<()> {
        if let Some(st) = self.storage_opt() {
            st.setting_set(NEEDS_ATTEST_SETTING, needed).await?;
        }
        self.update_keyfile(|k| k.flags.needs_attest = needed).await
    }

    /// Password / recovery envelopes to unlock from (key file when sealed,
    /// else the database).
    pub(crate) async fn unlock_envelopes(
        &self,
    ) -> AppResult<(VaultId, Option<KeyEnvelope>, Option<KeyEnvelope>)> {
        let vault_id = self.require_vault_id()?;
        if let Some(kf) = self.keyfile() {
            return Ok((vault_id, kf.password_envelope, kf.recovery_envelope));
        }
        let rec = self
            .storage()?
            .get_vault(vault_id)
            .await?
            .ok_or(AppError::NoVault)?;
        Ok((vault_id, rec.password_envelope, rec.recovery_envelope))
    }

    /// Remember a Recovery Kit awaiting its check (memory + persisted flag).
    pub(crate) async fn set_pending_kit(&self, kit: Option<PendingKit>) -> AppResult<()> {
        let pending = kit.is_some();
        *lock_mutex(&self.pending_kit) = kit;
        if let Some(st) = self.storage_opt() {
            st.setting_set(KIT_PENDING_SETTING, pending).await?;
        }
        self.update_keyfile(|k| k.flags.kit_pending = pending)
            .await?;
        if pending {
            self.ctx.emit(AppEvent::ProfilesChanged);
        }
        Ok(())
    }

    /// Whether the Recovery Kit check is still pending (persisted flag).
    pub(crate) async fn kit_check_pending(&self) -> bool {
        if let Some(kf) = self.keyfile() {
            return kf.flags.kit_pending;
        }
        match self.storage_opt() {
            Some(st) => st
                .setting_get::<bool>(KIT_PENDING_SETTING)
                .await
                .ok()
                .flatten()
                .unwrap_or(false),
            None => false,
        }
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
        self.storage()?
            .setting_set(VAULT_ID_SETTING, vault_id)
            .await?;
        *self.vault_id.write().unwrap_or_else(|p| p.into_inner()) = Some(vault_id);
        Ok(())
    }

    pub(crate) async fn profile(&self) -> AppResult<Profile> {
        match self.storage_opt() {
            Some(st) => st
                .get_profile()
                .await?
                .ok_or_else(|| AppError::Storage("profile row missing".into())),
            None => self
                .keyfile()
                .map(|k| k.header.to_profile(self.profile_id))
                .ok_or(AppError::VaultLocked),
        }
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

    /// Prevent a stale approval from being committed after lock/re-unlock.
    /// Keep this guard only for the final bounded transaction, never file I/O.
    pub(crate) async fn hold_unlocked(
        &self,
        expected: &Arc<Unlocked>,
    ) -> AppResult<tokio::sync::RwLockReadGuard<'_, Option<Arc<Unlocked>>>> {
        let current = self.unlocked.read().await;
        if !current.as_ref().is_some_and(|u| Arc::ptr_eq(u, expected)) {
            return Err(AppError::VaultLocked);
        }
        Ok(current)
    }

    /// Cache envelopes (as the server/local `KeyEnvelope` shape): in the
    /// database and, for password / recovery, in the key file the next
    /// unlock starts from. `device` is this installation's server trust
    /// marker (database only).
    pub(crate) async fn cache_envelopes(
        &self,
        vault_id: VaultId,
        password: Option<KeyEnvelope>,
        recovery: Option<KeyEnvelope>,
        device: Option<KeyEnvelope>,
    ) -> AppResult<()> {
        if let Some(st) = self.storage_opt() {
            st.cache_vault_envelopes(vault_id, password.clone(), recovery.clone(), device)
                .await?;
        }
        if password.is_some() || recovery.is_some() {
            self.update_keyfile(|k| {
                if k.vault_id != vault_id {
                    return;
                }
                if let Some(p) = password {
                    k.password_envelope = Some(p);
                }
                if let Some(r) = recovery {
                    k.recovery_envelope = Some(r);
                }
            })
            .await?;
        }
        Ok(())
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
        let store = ObjectStore::new(vault_id, self.storage()?, codec.clone());
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
            self.index_key(),
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
            rdp: Arc::new(cc_rdp_core::RdpManager::new()),
            ai: ai.clone(),
            edit: edit.clone(),
            transfers: crate::transfers::TransferManager::new(self.ctx.clone()),
            sharing_shutdown: tokio::sync::watch::channel(false).0,
            enrollment: crate::sharing_api::enrollment::EnrollmentRuntime::default(),
            kube_imports: Arc::new(crate::kubernetes_import::ImportRuntime::default()),
            kube: crate::kubernetes_runtime::KubeRuntime::default(),
            db: crate::database_runtime::DbRuntime::default(),
            db_imports: Default::default(),
            db_ai: Default::default(),
            docker: Default::default(),
            db_history_lock: tokio::sync::Mutex::new(()),
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
        unlocked.push_task(tokio::spawn(crate::kubernetes_history::retention_task(
            self.storage()?,
        )));
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
        if let Some(st) = self.storage_opt() {
            let _ = st.write(move |tx| tx.mark_vault_unlocked(vault_id)).await;
        }
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

    /// Attest this device for the vault (upload its trust envelope) if a new
    /// device identity was generated while the vault was locked, or the
    /// server still holds a pre-S5 device envelope with the real VRK
    /// (migration, newly approved device). Best effort, online only.
    async fn attest_if_needed(&self, unlocked: &Unlocked) {
        if !self.needs_attest().await {
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
            self.set_needs_attest(false).await?;
            Ok::<_, AppError>(())
        }
        .await;
        match r {
            Ok(()) => tracing::info!("device attested for the vault after identity change"),
            Err(e) => tracing::warn!(error = %e, "deferred attestation failed"),
        }
    }

    /// Lock the vault (no-op if locked). A sealed profile closes its
    /// database too: nothing of it is readable until the next unlock.
    pub(crate) async fn lock(&self) {
        let taken = self.unlocked.write().await.take();
        if let Some(u) = taken {
            u.shutdown().await;
            self.ctx.emit(AppEvent::VaultLocked {
                profile_id: self.profile_id.to_string(),
            });
        }
        lock_mutex(&self.pending_kit).take();
        self.reseal().await;
    }

    /// Close the profile: lock, then close the database synchronously
    /// (the session is unusable afterwards).
    pub(crate) async fn close(&self) {
        self.lock().await;
        if let Some(st) = self.storage_opt() {
            st.close().await;
        }
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
        ObjectKind::RdpHost,
        ObjectKind::KubeCluster,
        ObjectKind::KubeContext,
        ObjectKind::DbConnection,
        ObjectKind::DbHistoryEntry,
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
