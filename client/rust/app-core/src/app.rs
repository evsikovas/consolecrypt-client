//! [`AppCore`]: construction, profiles (ADR-0106), vault create / join /
//! unlock / lock and the Recovery Kit onboarding check.

use crate::dto::*;
use crate::error::{AppError, AppResult};
use crate::key_trust::{adopt_own, KeySource};
use crate::prompts::PromptBroker;
use crate::secrets::{self, blocking};
use crate::session::{AppCtx, PendingKit, Session, Unlocked, VAULT_ID_SETTING};
use crate::{AppConfig, SecureStoreKind};
use cc_models::settings::VaultSettings;
use cc_models::{ObjectId, VaultObject};
use cc_platform_core::{
    AppDirs, DeviceKeyProtector, InMemorySecureStore, SecureStore, UnsupportedDeviceKeys,
};
use cc_protocol::auth::{LoginRequest, RegisterRequest};
use cc_protocol::envelopes::{KeyEnvelope, RecipientType};
use cc_protocol::recovery::VaultRecoveryMaterial;
use cc_protocol::vaults::VaultRole;
use cc_protocol::VaultId;
use cc_storage_core::{
    local_key_envelope, Profile, ProfileDirectory, ProfileEntry, ProfileId, ProfileKind, Storage,
    VaultRecord,
};
use cc_sync_core::ApiClient;
use cc_vault_core::{
    create_vault, unlock_with_passphrase, unlock_with_recovery_input, DeviceIdentity, SecretString,
    UnlockedVault,
};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::broadcast;

pub(crate) struct Inner {
    pub ctx: Arc<AppCtx>,
    pub dirs: AppDirs,
    pub profiles: ProfileDirectory,
    active: tokio::sync::Mutex<Option<Arc<Session>>>,
    /// Held while a scheduled backup runs.
    pub backup_busy: tokio::sync::Mutex<()>,
    /// Browser sign-ins (enterprise OIDC) waiting for their redirect.
    pub enterprise: crate::enterprise_auth::PendingFlows,
}

/// The application facade used by the Flutter UI (via flutter_rust_bridge)
/// and by the `cc` CLI. Cheap to clone; all methods are `async` and return
/// plain DTOs or [`AppError`]. One profile is open ("active") at a time.
#[derive(Clone)]
pub struct AppCore {
    pub(crate) inner: Arc<Inner>,
}

impl std::fmt::Debug for AppCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppCore")
            .field("data_dir", &self.inner.dirs.root())
            .finish_non_exhaustive()
    }
}

pub(crate) fn secret(s: String) -> SecretString {
    SecretString::from(s)
}

pub(crate) fn parse_profile_id(s: &str) -> AppResult<ProfileId> {
    ProfileId::from_str(s.trim()).map_err(|_| AppError::invalid("profile_id", "not a valid id"))
}

pub(crate) fn parse_vault_id(s: &str) -> AppResult<VaultId> {
    VaultId::from_str(s.trim()).map_err(|_| AppError::invalid("vault_id", "not a valid id"))
}

/// Pick the password / recovery / own device envelopes from a list.
pub(crate) fn pick_envelopes(
    envs: &[KeyEnvelope],
    device: cc_protocol::DeviceId,
) -> (
    Option<KeyEnvelope>,
    Option<KeyEnvelope>,
    Option<KeyEnvelope>,
) {
    let live = |t: RecipientType| {
        envs.iter()
            .filter(|e| e.recipient_type == t && e.revoked_at.is_none())
            .max_by_key(|e| e.created_at)
            .cloned()
    };
    let own = envs
        .iter()
        .filter(|e| {
            e.recipient_type == RecipientType::Device
                && e.revoked_at.is_none()
                && e.recipient_id == Some(*device.as_uuid())
        })
        .max_by_key(|e| e.created_at)
        .cloned();
    (
        live(RecipientType::Password),
        live(RecipientType::Recovery),
        own,
    )
}

impl AppCore {
    /// Build the facade from `config` (secure store per
    /// [`AppConfig::secure_store`]; device unlock through the platform's
    /// OS protector with the OS store, unsupported otherwise).
    pub fn new(config: AppConfig) -> AppResult<Self> {
        Self::with_device_keys(config, None)
    }

    /// Like [`AppCore::new`] with an explicit device unlock protector —
    /// e.g. the Android app's `ExternalDeviceKeys`. `None` = platform
    /// default. Rust-only.
    pub fn with_device_keys(
        config: AppConfig,
        device_keys: Option<Arc<dyn DeviceKeyProtector>>,
    ) -> AppResult<Self> {
        let dirs = match &config.data_dir {
            Some(d) => AppDirs::with_root(d),
            None => AppDirs::from_env_or_system()?,
        };
        let secure: Arc<dyn SecureStore> = match config.secure_store {
            SecureStoreKind::InMemory => Arc::new(InMemorySecureStore::new()),
            SecureStoreKind::Os => os_store(&config)?,
            SecureStoreKind::InsecureFile => file_store(&dirs)?,
        };
        let device_keys = device_keys.unwrap_or_else(|| match config.secure_store {
            SecureStoreKind::Os => cc_platform_core::platform_device_keys(secure.clone()),
            _ => Arc::new(UnsupportedDeviceKeys),
        });
        Self::with_platform(config, secure, device_keys)
    }

    /// Rust-only constructor with explicit platform services (tests,
    /// embedders providing a device unlock protector). Not part of the FRB
    /// surface.
    pub fn with_platform(
        config: AppConfig,
        secure: Arc<dyn SecureStore>,
        device_keys: Arc<dyn DeviceKeyProtector>,
    ) -> AppResult<Self> {
        let dirs = match &config.data_dir {
            Some(d) => AppDirs::with_root(d),
            None => AppDirs::from_env_or_system()?,
        };
        let profiles = ProfileDirectory::new(dirs.root());
        let prompts = Arc::new(PromptBroker::new(std::time::Duration::from_secs(
            config.prompt_timeout_secs.max(1),
        )));
        let ctx = Arc::new(AppCtx {
            config,
            dirs: dirs.clone(),
            secure,
            device_keys,
            events: broadcast::channel(1024).0,
            prompts,
            file_opener: std::sync::RwLock::new(cc_platform_core::FileOpener::system()),
        });
        let app = Self {
            inner: Arc::new(Inner {
                ctx,
                dirs,
                profiles,
                active: tokio::sync::Mutex::new(None),
                backup_busy: tokio::sync::Mutex::new(()),
                enterprise: Default::default(),
            }),
        };
        if app.inner.ctx.config.backup_scheduler {
            app.spawn_backup_scheduler();
        }
        Ok(app)
    }

    /// Background task running due scheduled backups (every 30 s) for as
    /// long as this facade lives — also while the UI window is hidden.
    fn spawn_backup_scheduler(&self) {
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            tracing::debug!("no tokio runtime: backup scheduler not started");
            return;
        };
        let weak = Arc::downgrade(&self.inner);
        rt.spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                let Some(inner) = weak.upgrade() else { break };
                let app = AppCore { inner };
                if let Err(e) = app.run_due_backup().await {
                    tracing::debug!(code = e.code(), "scheduled backup check failed");
                }
            }
        });
    }

    /// Data root of this instance.
    pub fn data_dir(&self) -> String {
        self.inner.dirs.root().to_string_lossy().to_string()
    }

    /// App events (object changes, sync status, devices, terminals, …).
    pub fn subscribe_events(&self) -> broadcast::Receiver<AppEvent> {
        self.inner.ctx.events.subscribe()
    }

    /// Questions the UI must answer (unknown host keys, key passphrases).
    /// Without a subscriber, prompts are declined immediately.
    pub fn subscribe_prompts(&self) -> broadcast::Receiver<PromptRequest> {
        self.inner.ctx.prompts.subscribe()
    }

    /// Answer a [`PromptRequest::HostKey`].
    pub fn answer_host_key_prompt(
        &self,
        request_id: String,
        decision: HostKeyDecision,
    ) -> AppResult<()> {
        self.inner
            .ctx
            .prompts
            .answer_host_key(&request_id, decision)
    }

    /// Answer a [`PromptRequest::Passphrase`] (`None` = cancel).
    pub fn answer_passphrase_prompt(
        &self,
        request_id: String,
        passphrase: Option<String>,
    ) -> AppResult<()> {
        self.inner
            .ctx
            .prompts
            .answer_secret(&request_id, passphrase.map(secret))
    }

    /// Answer a [`PromptRequest::Password`] (`None` = cancel). The password
    /// is used for this connection only and never stored.
    pub fn answer_password_prompt(
        &self,
        request_id: String,
        password: Option<String>,
    ) -> AppResult<()> {
        self.inner
            .ctx
            .prompts
            .answer_secret(&request_id, password.map(secret))
    }

    // ---- session helpers -----------------------------------------------------

    pub(crate) async fn session(&self) -> AppResult<Arc<Session>> {
        self.inner
            .active
            .lock()
            .await
            .clone()
            .ok_or(AppError::NoActiveProfile)
    }

    pub(crate) async fn unlocked(&self) -> AppResult<(Arc<Session>, Arc<Unlocked>)> {
        let s = self.session().await?;
        let u = s.unlocked().await?;
        Ok((s, u))
    }

    /// Fence an approved, bounded commit to the profile that produced its
    /// preview. Profile activation waits until the guard is dropped.
    pub(crate) async fn hold_session(
        &self,
        expected: &Arc<Session>,
    ) -> AppResult<tokio::sync::MutexGuard<'_, Option<Arc<Session>>>> {
        let active = self.inner.active.lock().await;
        if !active.as_ref().is_some_and(|s| Arc::ptr_eq(s, expected)) {
            return Err(AppError::VaultLocked);
        }
        Ok(active)
    }

    /// Make `session` the active profile (closing the previous one).
    pub(crate) async fn activate(&self, session: Arc<Session>) -> AppResult<()> {
        let pid = session.profile_id;
        let old = {
            let mut active = self.inner.active.lock().await;
            active.replace(session)
        };
        if let Some(old) = old {
            if old.profile_id != pid {
                old.close().await;
            }
        }
        let profiles = self.inner.profiles.clone();
        blocking(move || {
            if let Some(mut e) = profiles.get(pid)? {
                e.last_opened_at = Some(chrono::Utc::now());
                profiles.update(&e)?;
            }
            profiles.set_active(Some(pid))?;
            Ok(())
        })
        .await?;
        self.inner.ctx.emit(AppEvent::ProfilesChanged);
        Ok(())
    }

    pub(crate) async fn profile_info_for(
        &self,
        entry: &ProfileEntry,
        session: Option<&Arc<Session>>,
    ) -> AppResult<ProfileInfo> {
        let mut info = ProfileInfo {
            id: entry.profile_id.to_string(),
            display_name: entry.display_name.clone(),
            kind: entry.kind,
            active: false,
            created_at_ms: ms(&entry.created_at),
            last_opened_at_ms: opt_ms(&entry.last_opened_at),
            server_url: None,
            email: None,
            device_id: None,
            vault_id: None,
            vault_state: VaultStateDto::Closed,
            recovery_kit_pending: false,
        };
        if let Some(s) = session.filter(|s| s.profile_id == entry.profile_id) {
            let p = s.profile().await?;
            info.active = true;
            info.kind = p.kind;
            info.server_url = p.server_url.clone();
            info.email = p.email.clone();
            info.recovery_kit_pending = s.kit_check_pending().await;
            crate::profile_hints::sync(&self.inner.dirs, entry.profile_id, &p).await;
            info.device_id = Some(s.device_id().to_string());
            info.vault_id = s.vault_id().map(|v| v.to_string());
            info.vault_state = if s.unlocked_opt().await.is_some() {
                VaultStateDto::Unlocked
            } else if s.vault_id().is_some() {
                VaultStateDto::Locked
            } else {
                VaultStateDto::NoVault
            };
        } else if entry.kind == ProfileKind::Synced {
            // Closed synced profile: non-secret account hint (switcher).
            if let Some(h) = crate::profile_hints::read(&self.inner.dirs, entry.profile_id).await {
                info.server_url = Some(h.server_url);
                info.email = h.email;
            }
        }
        Ok(info)
    }

    async fn entry(&self, pid: ProfileId) -> AppResult<ProfileEntry> {
        let profiles = self.inner.profiles.clone();
        blocking(move || profiles.get(pid).map_err(Into::into))
            .await?
            .ok_or_else(|| AppError::not_found("profile", pid))
    }

    // ---- profiles ------------------------------------------------------------

    /// All profiles (details filled for the open one).
    pub async fn list_profiles(&self) -> AppResult<Vec<ProfileInfo>> {
        let profiles = self.inner.profiles.clone();
        let entries = blocking(move || profiles.list().map_err(Into::into)).await?;
        let active = self.inner.active.lock().await.clone();
        let mut out = Vec::with_capacity(entries.len());
        for e in &entries {
            out.push(self.profile_info_for(e, active.as_ref()).await?);
        }
        Ok(out)
    }

    /// The open profile, if any.
    pub async fn active_profile(&self) -> AppResult<Option<ProfileInfo>> {
        let Some(s) = self.inner.active.lock().await.clone() else {
            return Ok(None);
        };
        let e = self.entry(s.profile_id).await?;
        self.profile_info_for(&e, Some(&s)).await.map(Some)
    }

    /// Profile remembered as active in the index (to reopen at start-up).
    pub async fn last_active_profile_id(&self) -> AppResult<Option<String>> {
        let profiles = self.inner.profiles.clone();
        Ok(blocking(move || profiles.active().map_err(Into::into))
            .await?
            .map(|p| p.to_string()))
    }

    /// Resolve a profile by id or (unique, case-insensitive) display name.
    pub async fn find_profile(&self, id_or_name: String) -> AppResult<ProfileInfo> {
        let all = self.list_profiles().await?;
        if let Some(p) = all.iter().find(|p| p.id == id_or_name.trim()) {
            return Ok(p.clone());
        }
        let matches: Vec<&ProfileInfo> = all
            .iter()
            .filter(|p| p.display_name.eq_ignore_ascii_case(id_or_name.trim()))
            .collect();
        match matches.as_slice() {
            [one] => Ok((*one).clone()),
            [] => Err(AppError::not_found("profile", id_or_name)),
            _ => Err(AppError::invalid(
                "profile",
                "name is ambiguous; use the id",
            )),
        }
    }

    /// Open (switch to) a profile. Its vault starts locked.
    pub async fn open_profile(&self, profile_id: String) -> AppResult<ProfileInfo> {
        let pid = parse_profile_id(&profile_id)?;
        let entry = self.entry(pid).await?;
        let current = self.inner.active.lock().await.clone();
        let session = match current {
            Some(s) if s.profile_id == pid => s,
            _ => {
                Session::open(
                    self.inner.ctx.clone(),
                    pid,
                    self.inner.profiles.db_path(pid),
                )
                .await?
            }
        };
        self.activate(session.clone()).await?;
        self.profile_info_for(&entry, Some(&session)).await
    }

    /// Close the open profile (locks the vault).
    pub async fn close_profile(&self) -> AppResult<()> {
        let s = self.inner.active.lock().await.take();
        if let Some(s) = s {
            s.close().await;
            self.inner.ctx.emit(AppEvent::ProfilesChanged);
        }
        Ok(())
    }

    /// Close the open profile and release every resource (call before the
    /// process exits; the Flutter app on quit, the CLI on exit).
    pub async fn shutdown(&self) -> AppResult<()> {
        self.close_profile().await
    }

    /// Rename a profile.
    pub async fn rename_profile(
        &self,
        profile_id: String,
        display_name: String,
    ) -> AppResult<ProfileInfo> {
        let pid = parse_profile_id(&profile_id)?;
        let name = crate::validate::display_name("display_name", &display_name)?;
        let mut entry = self.entry(pid).await?;
        entry.display_name = name.clone();
        let profiles = self.inner.profiles.clone();
        let e2 = entry.clone();
        blocking(move || profiles.update(&e2).map_err(Into::into)).await?;
        let active = self.inner.active.lock().await.clone();
        if let Some(s) = active.as_ref().filter(|s| s.profile_id == pid) {
            let mut p = s.profile().await?;
            p.display_name = Some(name);
            s.put_profile(p).await?;
        }
        self.inner.ctx.emit(AppEvent::ProfilesChanged);
        self.profile_info_for(&entry, active.as_ref()).await
    }

    /// Delete a profile: its database, index entry and secure-store items.
    /// Local-only data without a backup is gone for good.
    pub async fn remove_profile(&self, profile_id: String) -> AppResult<()> {
        let pid = parse_profile_id(&profile_id)?;
        self.entry(pid).await?;
        let active = {
            let mut a = self.inner.active.lock().await;
            if a.as_ref().is_some_and(|s| s.profile_id == pid) {
                a.take()
            } else {
                None
            }
        };
        if let Some(s) = active {
            s.close().await;
            drop(s);
        }
        // An OS device unlock key outlives the files (Windows Hello).
        let dir = self.inner.profiles.profile_dir(pid);
        if let Ok(Some(kf)) = blocking(move || crate::keyfile::load(&dir)).await {
            if let Some(record) = kf.device_unlock.filter(|r| !r.external) {
                let keys = self.inner.ctx.device_keys.clone();
                let _ = blocking(move || Ok(keys.destroy(&record.handle.0)?)).await;
            }
        }
        let profiles = self.inner.profiles.clone();
        blocking(move || profiles.remove(pid).map(|_| ()).map_err(Into::into)).await?;
        if let Err(e) = secrets::delete_profile_secrets(self.inner.ctx.secure.clone(), pid).await {
            tracing::warn!(error = %e, "could not remove all secure-store items of the profile");
        }
        self.inner.ctx.emit(AppEvent::ProfilesChanged);
        Ok(())
    }

    /// Create the directory, DB key, database and identity of a new profile.
    pub(crate) async fn new_profile_storage(
        &self,
        display_name: &str,
        kind: ProfileKind,
    ) -> AppResult<(ProfileId, Storage, Arc<DeviceIdentity>)> {
        let name = crate::validate::display_name("display_name", display_name)?;
        let profiles = self.inner.profiles.clone();
        let dirs = self.inner.dirs.clone();
        let entry = blocking(move || {
            let e = profiles.create(name, kind)?;
            dirs.ensure_profile(&e.profile_id.to_string())?;
            Ok(e)
        })
        .await?;
        let pid = entry.profile_id;
        let r = async {
            let key = secrets::create_db_key(self.inner.ctx.secure.clone(), pid).await?;
            let storage = Storage::open(self.inner.profiles.db_path(pid), key).await?;
            let identity =
                secrets::load_or_create_identity(self.inner.ctx.secure.clone(), pid).await?;
            Ok::<_, AppError>((storage, Arc::new(identity)))
        }
        .await;
        match r {
            Ok((storage, identity)) => Ok((pid, storage, identity)),
            Err(e) => {
                self.discard_profile(pid, None).await;
                Err(e)
            }
        }
    }

    /// Roll back a half-created profile.
    pub(crate) async fn discard_profile(&self, pid: ProfileId, storage: Option<Storage>) {
        if let Some(s) = storage {
            s.close().await;
        }
        let profiles = self.inner.profiles.clone();
        let _ = blocking(move || profiles.remove(pid).map(|_| ()).map_err(Into::into)).await;
        let _ = secrets::delete_profile_secrets(self.inner.ctx.secure.clone(), pid).await;
    }

    /// Create a **Local** profile (no server, no account): device identity,
    /// vault with the full ADR-0002 hierarchy, envelopes stored locally.
    /// The profile becomes active and unlocked; show the returned Recovery
    /// Kit and run the 3-word check ([`AppCore::recovery_kit_start_check`]).
    pub async fn create_local_profile(
        &self,
        display_name: String,
        passphrase: String,
    ) -> AppResult<CreatedProfile> {
        crate::validate::display_name("display_name", &display_name)?;
        crate::validate::secret_text("passphrase", &passphrase)?;
        let pass = secret(passphrase);
        let (pid, storage, identity) = self
            .new_profile_storage(&display_name, ProfileKind::Local)
            .await?;
        let r = async {
            storage
                .put_profile(Profile::new_local(
                    pid,
                    identity.device_id(),
                    Some(display_name.trim().to_owned()),
                ))
                .await?;
            let kdf = self.inner.ctx.kdf().await?;
            let id = identity.clone();
            let created =
                blocking(move || Ok(create_vault(&id, &pass, kdf, None, chrono::Utc::now())?))
                    .await?;
            let vault_id = created.create_request.vault_id;
            let envs = created.local_envelopes();
            let device_id = identity.device_id();
            storage
                .write(move |tx| {
                    let mut rec = VaultRecord::new(vault_id, VaultRole::Owner);
                    rec.password_envelope =
                        Some(local_key_envelope(vault_id, envs.password, Some(device_id)));
                    rec.recovery_envelope =
                        Some(local_key_envelope(vault_id, envs.recovery, Some(device_id)));
                    tx.put_vault(&rec)?;
                    tx.setting_set(VAULT_ID_SETTING, &vault_id)
                })
                .await?;
            let session =
                Session::with_storage(self.inner.ctx.clone(), pid, storage.clone()).await?;
            let pin = session
                .check_vault_key(&created.unlocked, KeySource::Created)
                .await?;
            session.store_pin(pin).await?;
            // The database key moves under the vault key (S5).
            session.seal_storage(&created.unlocked).await?;
            let unlocked = session.install_unlocked(created.unlocked).await?;
            seed_vault_settings(&unlocked, display_name.trim()).await?;
            let kit = RecoveryKitDto::from_kit(&created.recovery_kit);
            session
                .set_pending_kit(Some(PendingKit {
                    kit: created.recovery_kit,
                    check: None,
                }))
                .await?;
            Ok::<_, AppError>((session, kit))
        }
        .await;
        let (session, kit) = match r {
            Ok(v) => v,
            Err(e) => {
                self.discard_profile(pid, Some(storage)).await;
                return Err(e);
            }
        };
        self.activate(session.clone()).await?;
        tracing::info!(profile_id = %pid, "local profile created");
        let entry = self.entry(pid).await?;
        Ok(CreatedProfile {
            profile: self.profile_info_for(&entry, Some(&session)).await?,
            recovery_kit: Some(kit),
        })
    }

    /// Create a **Synced** profile: register or log in at `server_url` with
    /// this installation's new device identity. Afterwards either
    /// [`AppCore::create_vault`], join an existing vault
    /// ([`AppCore::join_vault_with_passphrase`] /
    /// [`AppCore::join_vault_with_recovery_key`]) or
    /// [`AppCore::request_device_approval`].
    pub async fn create_synced_profile(
        &self,
        display_name: String,
        server_url: String,
        email: String,
        account_password: String,
        mode: AccountMode,
    ) -> AppResult<SyncedAccountDto> {
        crate::validate::display_name("display_name", &display_name)?;
        let email = crate::validate::email(&email)?;
        crate::validate::secret_text("account_password", &account_password)?;
        let credential = Credential::Password {
            email,
            password: cc_protocol::auth::SecretString::new(account_password),
            mode,
        };
        self.create_synced_profile_with(Some(display_name), server_url, credential, None)
            .await
    }

    /// [`AppCore::create_synced_profile`] for any [`Credential`]. `prepared`
    /// is the profile storage + device identity created before a browser
    /// sign-in (its `login_code` is bound to that device id); it is discarded
    /// on failure like a storage created here. Without `display_name` the
    /// profile is named after the account e-mail.
    pub(crate) async fn create_synced_profile_with(
        &self,
        display_name: Option<String>,
        server_url: String,
        credential: Credential,
        prepared: Option<(ProfileId, Storage, Arc<DeviceIdentity>)>,
    ) -> AppResult<SyncedAccountDto> {
        let server_url = crate::validate::server_url(&server_url)?;
        let (pid, storage, identity) = match prepared {
            Some(p) => p,
            None => {
                // Validate the URL before touching the disk.
                self.inner.ctx.check_server_url(server_url.trim())?;
                let name = display_name
                    .clone()
                    .unwrap_or_else(|| server_host(&server_url));
                self.new_profile_storage(&name, ProfileKind::Synced).await?
            }
        };
        let r = async {
            let cell = crate::session::identity_cell(identity.clone());
            let api = self.inner.ctx.api_for(pid, server_url.trim(), &cell)?;
            let (auth, identity) = sign_in(self, &api, pid, identity, &credential).await?;
            // A regenerated identity signs the requests that follow.
            *cell.write().unwrap_or_else(|p| p.into_inner()) = identity.clone();
            let email = account_email(&api, &credential).await?;
            let mut profile = Profile::new_synced(
                pid,
                identity.device_id(),
                server_url.trim(),
                Some(auth.user_id),
                Some(email.clone()),
            );
            let name = display_name
                .as_deref()
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .unwrap_or(&email)
                .to_owned();
            profile.display_name = Some(name.clone());
            storage.put_profile(profile).await?;
            let vaults = api.list_vaults().await?.vaults;
            let session =
                Session::with_storage(self.inner.ctx.clone(), pid, storage.clone()).await?;
            Ok::<_, AppError>((session, auth, vaults, name))
        }
        .await;
        let (session, auth, vaults, name) = match r {
            Ok(v) => v,
            Err(e) => {
                self.discard_profile(pid, Some(storage)).await;
                return Err(e);
            }
        };
        self.activate(session.clone()).await?;
        if display_name.is_none() {
            // The directory entry was created before the e-mail was known.
            let _ = self.rename_profile(pid.to_string(), name).await;
        }
        tracing::info!(profile_id = %pid, "synced profile created");
        let entry = self.entry(pid).await?;
        Ok(SyncedAccountDto {
            profile: self.profile_info_for(&entry, Some(&session)).await?,
            user_id: auth.user_id.to_string(),
            device_id: auth.device_id.to_string(),
            email_verified: auth.email_verified,
            vaults: vaults.iter().map(remote_vault).collect(),
        })
    }

    /// Vaults of the signed-in account (synced profiles).
    pub async fn list_remote_vaults(&self) -> AppResult<Vec<RemoteVaultDto>> {
        let s = self.session().await?;
        let vaults = s.api()?.list_vaults().await?.vaults;
        if let Some(u) = s.unlocked_opt().await {
            u.observe_vault_info(&vaults).await;
        }
        Ok(vaults.iter().map(remote_vault).collect())
    }

    /// Create a new vault for a synced profile that has none; uploads it
    /// and starts syncing. Show the returned Recovery Kit once.
    pub async fn create_vault(&self, passphrase: String) -> AppResult<RecoveryKitDto> {
        crate::validate::secret_text("passphrase", &passphrase)?;
        let s = self.session().await?;
        if s.vault_id().is_some() {
            return Err(AppError::invalid(
                "vault",
                "this profile already has a vault",
            ));
        }
        let api = s.api()?;
        let profile = s.profile().await?;
        let identity = s.identity();
        let kdf = self.inner.ctx.kdf().await?;
        let pass = secret(passphrase);
        let url = profile.server_url.clone();
        let created = blocking(move || {
            Ok(create_vault(
                &identity,
                &pass,
                kdf,
                url.as_deref(),
                chrono::Utc::now(),
            )?)
        })
        .await?;
        let vault_id = created.create_request.vault_id;
        let pin = s
            .check_vault_key(&created.unlocked, KeySource::Created)
            .await?;
        let info = api.create_vault(&created.create_request).await?;
        s.storage()?.upsert_vault_info(info).await?;
        let (pw, rec, _) = match api.list_envelopes(vault_id).await {
            Ok(l) => pick_envelopes(&l.envelopes, s.device_id()),
            Err(_) => (None, None, None),
        };
        // TR-4: cache this device's own envelopes (the server copy only when
        // it is byte-identical, for its envelope id); no device envelope.
        let envs = created.local_envelopes();
        let did = s.device_id();
        s.cache_envelopes(
            vault_id,
            Some(adopt_own(vault_id, &envs.password, pw, did)),
            Some(adopt_own(vault_id, &envs.recovery, rec, did)),
            None,
        )
        .await?;
        s.set_vault_id(vault_id).await?;
        s.store_pin(pin).await?;
        s.seal_storage(&created.unlocked).await?;
        let unlocked = s.install_unlocked(created.unlocked).await?;
        let name = profile.display_name.unwrap_or_else(|| "Vault".into());
        seed_vault_settings(&unlocked, &name).await?;
        let kit = RecoveryKitDto::from_kit(&created.recovery_kit);
        s.set_pending_kit(Some(PendingKit {
            kit: created.recovery_kit,
            check: None,
        }))
        .await?;
        tracing::info!(vault_id = %vault_id, "vault created on server");
        Ok(kit)
    }

    /// Join an existing vault on this (new) device with the vault
    /// passphrase: unlock the password envelope, attest this device with the
    /// vault access key (ADR-0004), then sync.
    pub async fn join_vault_with_passphrase(
        &self,
        vault_id: String,
        passphrase: String,
    ) -> AppResult<ProfileInfo> {
        let s = self.session().await?;
        let vault_id = parse_vault_id(&vault_id)?;
        let api = s.api()?;
        let material = api.recovery_vault_envelope(vault_id).await?;
        let env = material
            .password_envelope
            .clone()
            .ok_or_else(|| AppError::not_found("password envelope", vault_id))?;
        let pass = secret(passphrase);
        let vault = blocking(move || Ok(unlock_with_passphrase(vault_id, &env, &pass)?)).await?;
        self.attest_and_install(&s, &api, vault, material, KeySource::Passphrase)
            .await?;
        self.active_profile()
            .await?
            .ok_or(AppError::NoActiveProfile)
    }

    /// Join an existing vault with the Recovery Key (24 words or QR
    /// payload); optionally set a new vault passphrase right away.
    pub async fn join_vault_with_recovery_key(
        &self,
        vault_id: String,
        recovery_input: String,
        new_passphrase: Option<String>,
    ) -> AppResult<ProfileInfo> {
        let s = self.session().await?;
        let vault_id = parse_vault_id(&vault_id)?;
        let api = s.api()?;
        let material = api.recovery_vault_envelope(vault_id).await?;
        let env = material
            .recovery_envelope
            .clone()
            .ok_or_else(|| AppError::not_found("recovery envelope", vault_id))?;
        let input = secret(recovery_input);
        let vault =
            blocking(move || Ok(unlock_with_recovery_input(vault_id, &env, &input)?)).await?;
        self.attest_and_install(&s, &api, vault, material, KeySource::RecoveryKey)
            .await?;
        if let Some(p) = new_passphrase {
            self.set_new_passphrase(p).await?;
        }
        self.active_profile()
            .await?
            .ok_or(AppError::NoActiveProfile)
    }

    pub(crate) async fn attest_and_install(
        &self,
        s: &Arc<Session>,
        api: &ApiClient,
        vault: UnlockedVault,
        material: VaultRecoveryMaterial,
        source: KeySource,
    ) -> AppResult<Arc<Unlocked>> {
        let vault_id = vault.vault_id();
        // TR-3 before anything is sent or stored.
        let pin = s.check_vault_key(&vault, source).await?;
        let identity = s.identity();
        let attest = vault.attest_device_request(&identity)?;
        // TR-4: the attest response is a confirmation only, never cached.
        api.attest_device(identity.device_id(), &attest).await?;
        let info = api.get_vault(vault_id).await?;
        s.storage()?.upsert_vault_info(info.clone()).await?;
        s.cache_envelopes(
            vault_id,
            material.password_envelope,
            material.recovery_envelope,
            None,
        )
        .await?;
        s.set_vault_id(vault_id).await?;
        s.store_pin(pin).await?;
        s.seal_storage(&vault).await?;
        tracing::info!(vault_id = %vault_id, "device attested for vault");
        let u = s.install_unlocked(vault).await?;
        u.observe_vault_info(std::slice::from_ref(&info)).await;
        Ok(u)
    }

    // ---- unlock / lock -------------------------------------------------------

    /// Open the database for `vault` and install it: a sealed profile
    /// unwraps its database key; a ≤ 0.8.x profile is sealed now (S5
    /// migration: re-key under the vault key, OS-stored key deleted, device
    /// unlock off until switched on again, server envelope replaced by the
    /// trust marker on the next attestation).
    pub(crate) async fn finish_unlock(
        &self,
        s: &Arc<Session>,
        vault: UnlockedVault,
        source: KeySource,
    ) -> AppResult<()> {
        if s.unlocked_opt().await.is_some() {
            return Ok(());
        }
        // TR-3 / TR-17: compare with the pinned key before the database is
        // opened with it; the first unlock after the update pins it.
        let pin = s.check_vault_key(&vault, source).await?;
        if s.has_keyfile() {
            s.open_sealed_storage(&vault).await?;
            s.reconcile_header().await?;
            s.store_pin(pin).await?;
        } else if s.vault_id().is_some() {
            // Into the database first: sealing copies it into the key file.
            s.store_pin(pin).await?;
            s.seal_storage(&vault).await?;
            if s.kind().await? == ProfileKind::Synced {
                s.set_needs_attest(true).await?;
            }
        } else {
            s.store_pin(pin).await?;
        }
        s.install_unlocked(vault).await?;
        Ok(())
    }

    /// Unlock with the vault passphrase (works offline from the cached
    /// envelope; a synced profile retries once with a fresh envelope from
    /// the server if the passphrase was changed elsewhere).
    pub async fn unlock_with_passphrase(&self, passphrase: String) -> AppResult<()> {
        let s = self.session().await?;
        if s.unlocked_opt().await.is_some() {
            return Ok(());
        }
        let (vault_id, cached, _) = s.unlock_envelopes().await?;
        let env = cached
            .clone()
            .ok_or_else(|| AppError::Storage("no password envelope stored".into()))?;
        let pass = Arc::new(secret(passphrase));
        let p = pass.clone();
        let opened = env.clone();
        let first = blocking(move || Ok(unlock_with_passphrase(vault_id, &opened, &p)?)).await;
        let (vault, fresh_env) = match first {
            Ok(v) => (v, None),
            Err(AppError::WrongPassphrase) if s.api_opt().is_some() => {
                let fresh = match s.api()?.recovery_vault_envelope(vault_id).await {
                    Ok(m) => m.password_envelope,
                    Err(_) => None,
                };
                match fresh {
                    Some(env) if Some(&env) != cached.as_ref() => {
                        let e2 = env.clone();
                        let p = pass.clone();
                        let v = blocking(move || Ok(unlock_with_passphrase(vault_id, &e2, &p)?))
                            .await?;
                        (v, Some(env))
                    }
                    _ => return Err(AppError::WrongPassphrase),
                }
            }
            Err(e) => return Err(e),
        };
        // TR-3: the key (also from a fresh server envelope) is checked
        // against the pin before anything is opened or cached; a fresh
        // envelope is cached only after its key passed.
        self.finish_unlock(&s, vault, KeySource::Passphrase).await?;
        if let Some(env) = &fresh_env {
            s.cache_envelopes(vault_id, Some(env.clone()), None, None)
                .await?;
        }
        self.rewrap_password_envelope(&s, fresh_env.unwrap_or(env), &pass)
            .await;
        Ok(())
    }

    /// Unlock with the Recovery Key against the stored recovery envelope.
    pub async fn unlock_with_recovery_key(&self, recovery_input: String) -> AppResult<()> {
        let s = self.session().await?;
        if s.unlocked_opt().await.is_some() {
            return Ok(());
        }
        let vault = self.recovery_unlock(&s, recovery_input).await?;
        self.finish_unlock(&s, vault, KeySource::RecoveryKey).await
    }

    pub(crate) async fn recovery_unlock(
        &self,
        s: &Session,
        recovery_input: String,
    ) -> AppResult<UnlockedVault> {
        let (vault_id, _, cached) = s.unlock_envelopes().await?;
        let env = match cached {
            Some(e) => e,
            None => s
                .api()?
                .recovery_vault_envelope(vault_id)
                .await?
                .recovery_envelope
                .ok_or_else(|| AppError::not_found("recovery envelope", vault_id))?,
        };
        let input = secret(recovery_input);
        blocking(move || Ok(unlock_with_recovery_input(vault_id, &env, &input)?)).await
    }

    /// Check a passphrase against the stored envelope without unlocking.
    pub async fn verify_passphrase(&self, passphrase: String) -> AppResult<bool> {
        let s = self.session().await?;
        let (vault_id, env, _) = s.unlock_envelopes().await?;
        let env = env.ok_or_else(|| AppError::Storage("no password envelope stored".into()))?;
        let pass = secret(passphrase);
        match blocking(move || Ok(unlock_with_passphrase(vault_id, &env, &pass)?)).await {
            Ok(v) => {
                v.lock();
                Ok(true)
            }
            Err(AppError::WrongPassphrase) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Lock the vault: stop sync and tunnels, zeroize keys, drop the
    /// decrypted working set. Open terminals keep running (no new SSH shell
    /// until the next unlock); closing the profile closes them.
    pub async fn lock(&self) -> AppResult<()> {
        let s = self.session().await?;
        s.lock().await;
        Ok(())
    }

    /// Whether the open profile's vault is unlocked.
    pub async fn is_unlocked(&self) -> bool {
        match self.session().await {
            Ok(s) => s.unlocked_opt().await.is_some(),
            Err(_) => false,
        }
    }

    // ---- recovery kit onboarding ---------------------------------------------

    /// The Recovery Kit still awaiting its onboarding check, if any.
    pub async fn pending_recovery_kit(&self) -> AppResult<Option<RecoveryKitDto>> {
        let s = self.session().await?;
        let g = s.pending_kit.lock().unwrap_or_else(|p| p.into_inner());
        Ok(g.as_ref().map(|p| RecoveryKitDto::from_kit(&p.kit)))
    }

    /// Start the mandatory "re-enter 3 random words" check.
    pub async fn recovery_kit_start_check(&self) -> AppResult<RecoveryCheckDto> {
        let s = self.session().await?;
        let mut g = s.pending_kit.lock().unwrap_or_else(|p| p.into_inner());
        let p = g
            .as_mut()
            .ok_or_else(|| AppError::not_found("pending recovery kit", "-"))?;
        let check = p.kit.start_check()?;
        let positions = check.positions().iter().map(|&x| x as u32).collect();
        p.check = Some(check);
        Ok(RecoveryCheckDto { positions })
    }

    /// Verify the 3 words (in position order). Returns the wrong positions;
    /// empty = passed (the kit is then forgotten).
    pub async fn recovery_kit_verify(&self, answers: Vec<String>) -> AppResult<Vec<u32>> {
        let s = self.session().await?;
        let wrong: Vec<u32> = {
            let g = s.pending_kit.lock().unwrap_or_else(|p| p.into_inner());
            let p = g
                .as_ref()
                .ok_or_else(|| AppError::not_found("pending recovery kit", "-"))?;
            let check = p
                .check
                .as_ref()
                .ok_or_else(|| AppError::invalid("check", "call recovery_kit_start_check first"))?;
            let [a, b, c] = answers.as_slice() else {
                return Err(AppError::invalid("answers", "exactly 3 words are required"));
            };
            check
                .wrong_positions([a.as_str(), b.as_str(), c.as_str()])
                .into_iter()
                .map(|x| x as u32)
                .collect()
        };
        if wrong.is_empty() {
            s.set_pending_kit(None).await?;
        }
        Ok(wrong)
    }

    /// Forget the pending kit without the check (e.g. CLI after printing,
    /// or the UI after the check when the kit was lost with a restart).
    /// Clears the persisted "check pending" flag.
    pub async fn recovery_kit_acknowledge(&self) -> AppResult<()> {
        let s = self.session().await?;
        s.set_pending_kit(None).await
    }

    /// Whether the mandatory Recovery Kit check of the open profile is
    /// still pending — persisted, so it survives restarts (the kit itself
    /// is kept in memory only; after a restart the UI regenerates it).
    pub async fn recovery_kit_check_pending(&self) -> AppResult<bool> {
        Ok(self.session().await?.kit_check_pending().await)
    }
}

/// What proves the account to the server.
pub(crate) enum Credential {
    /// Standard mode: account e-mail + password (register or log in).
    Password {
        email: String,
        password: cc_protocol::auth::SecretString,
        mode: AccountMode,
    },
    /// Enterprise LDAP / Active Directory: directory login + password.
    Ldap {
        login: String,
        password: cc_protocol::auth::SecretString,
    },
    /// Enterprise OIDC: the single-use `login_code` from the browser
    /// redirect and the PKCE verifier of that sign-in.
    Oidc {
        login_code: cc_protocol::auth::SecretString,
        code_verifier: cc_protocol::auth::SecretString,
    },
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Credential::Password { .. } => "Credential::Password",
            Credential::Ldap { .. } => "Credential::Ldap",
            Credential::Oidc { .. } => "Credential::Oidc",
        })
    }
}

impl Credential {
    /// A browser `login_code` can be redeemed once, by one device id.
    fn single_use(&self) -> bool {
        matches!(self, Credential::Oidc { .. })
    }
}

/// The account e-mail after a successful sign-in (enterprise: from
/// `GET /v1/auth/me`; the IdP/directory decides it).
pub(crate) async fn account_email(api: &ApiClient, credential: &Credential) -> AppResult<String> {
    match credential {
        Credential::Password { email, .. } => Ok(email.clone()),
        _ => Ok(api.me().await?.email),
    }
}

/// Host of a server URL (placeholder profile name before the e-mail is known).
pub(crate) fn server_host(url: &str) -> String {
    url::Url::parse(url.trim())
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_else(|| url.trim().to_owned())
}

/// One sign-in request with a fresh device proof (ADR-0006; nonces are
/// single-use, so every attempt signs anew). A `stale` proof (clock skew /
/// slow network) is retried once — except for a browser `login_code`, which
/// is single-use; other rejections are returned as
/// [`AppError::DeviceProofRejected`] — never retried without a proof.
pub(crate) async fn authenticate(
    app: &AppCore,
    api: &ApiClient,
    identity: &DeviceIdentity,
    credential: &Credential,
) -> AppResult<cc_protocol::auth::AuthResponse> {
    use cc_sync_core::api::enterprise::{LdapLoginRequest, OidcExchangeRequest};
    let cfg = &app.inner.ctx.config;
    let attempts = if credential.single_use() { 1 } else { 2 };
    let mut last = None;
    for _ in 0..attempts {
        let device = identity.registration(
            &cfg.device_name,
            cfg.platform,
            Some(cfg.client_version.clone()),
        )?;
        let device_proof = Some(identity.login_proof(chrono::Utc::now())?);
        let r = match credential {
            Credential::Password {
                email,
                password,
                mode: AccountMode::Register,
            } => {
                api.register(&RegisterRequest {
                    email: email.clone(),
                    password: password.clone(),
                    device,
                    device_proof,
                })
                .await
            }
            Credential::Password {
                email,
                password,
                mode: AccountMode::Login,
            } => {
                api.login(&LoginRequest {
                    email: email.clone(),
                    password: password.clone(),
                    device,
                    device_proof,
                })
                .await
            }
            Credential::Ldap { login, password } => {
                api.ldap_login(&LdapLoginRequest {
                    login: login.clone(),
                    password: password.clone(),
                    device,
                    device_proof,
                })
                .await
            }
            Credential::Oidc {
                login_code,
                code_verifier,
            } => {
                api.oidc_exchange(&OidcExchangeRequest {
                    login_code: login_code.clone(),
                    code_verifier: code_verifier.clone(),
                    device,
                    device_proof,
                })
                .await
            }
        };
        // The IdP or directory picked the account: a device id owned by
        // another account means another user signed in on this profile.
        // Never regenerate this profile's device identity for that.
        if matches!(
            (&r, credential),
            (
                Err(cc_sync_core::ApiError::DeviceIdentityConflict),
                Credential::Ldap { .. } | Credential::Oidc { .. }
            )
        ) {
            return Err(AppError::invalid("account", "account_mismatch"));
        }
        match r.map_err(AppError::from) {
            Err(AppError::DeviceProofRejected(reason)) if reason == "stale" => {
                tracing::warn!("device login proof rejected as stale; retrying once");
                last = Some(AppError::DeviceProofRejected(reason));
            }
            other => return other,
        }
    }
    Err(last.unwrap_or(AppError::DeviceProofRejected("stale".into())))
}

/// Sign in `identity` (regenerating it once if the server refuses the
/// device id — not for a single-use browser `login_code`, which is bound to
/// the device id: the caller starts a new browser sign-in).
pub(crate) async fn sign_in(
    app: &AppCore,
    api: &ApiClient,
    pid: ProfileId,
    identity: Arc<DeviceIdentity>,
    credential: &Credential,
) -> AppResult<(cc_protocol::auth::AuthResponse, Arc<DeviceIdentity>)> {
    let mut identity = identity;
    for attempt in 0..2 {
        match authenticate(app, api, &identity, credential).await {
            Ok(auth) => return Ok((auth, identity)),
            Err(AppError::NewDeviceIdentityRequired) if attempt == 0 => {
                identity = Arc::new(
                    secrets::regenerate_identity(app.inner.ctx.secure.clone(), pid).await?,
                );
                if credential.single_use() {
                    return Err(AppError::NewDeviceIdentityRequired);
                }
            }
            Err(e) => return Err(e),
        }
    }
    Err(AppError::NewDeviceIdentityRequired)
}

fn remote_vault(v: &cc_protocol::vaults::VaultInfo) -> RemoteVaultDto {
    RemoteVaultDto {
        vault_id: v.vault_id.to_string(),
        role: format!("{:?}", v.role).to_ascii_lowercase(),
        caller_trusted: v.caller_trusted,
        latest_sequence: v.latest_sequence,
        created_at_ms: ms(&v.created_at),
    }
}

/// Create the vault's settings singleton (display name lives in the vault).
async fn seed_vault_settings(u: &Unlocked, name: &str) -> AppResult<()> {
    if !u.working().all_vault_settings().is_empty() {
        return Ok(());
    }
    let now = chrono::Utc::now();
    u.writer
        .put(VaultObject::VaultSettings(VaultSettings {
            id: ObjectId::new(),
            vault_name: if name.trim().is_empty() {
                "Vault".into()
            } else {
                name.trim().to_owned()
            },
            terminal_history_mode: Default::default(),
            default_privacy_profile: Default::default(),
            sync_ai_conversations: false,
            color: None,
            icon: None,
            description: None,
            policies: Default::default(),
            sharing_policy: Default::default(),
            created_at: now,
            updated_at: now,
        }))
        .await
}

#[cfg(feature = "os-keychain")]
fn os_store(config: &AppConfig) -> AppResult<Arc<dyn SecureStore>> {
    let s = match &config.keychain_service {
        Some(svc) => cc_platform_core::OsSecureStore::with_service(svc)?,
        None => cc_platform_core::OsSecureStore::new()?,
    };
    Ok(Arc::new(s))
}

#[cfg(not(feature = "os-keychain"))]
fn os_store(_config: &AppConfig) -> AppResult<Arc<dyn SecureStore>> {
    Err(AppError::Unsupported(
        "this build has no OS keychain support (feature os-keychain)".into(),
    ))
}

#[cfg(feature = "insecure-file-store")]
fn file_store(dirs: &AppDirs) -> AppResult<Arc<dyn SecureStore>> {
    tracing::warn!("using the INSECURE plaintext file secure store (testing/headless only)");
    Ok(Arc::new(secrets::InsecureFileSecureStore::new(
        dirs.root().join("secure-store.json"),
    )))
}

#[cfg(not(feature = "insecure-file-store"))]
fn file_store(_dirs: &AppDirs) -> AppResult<Arc<dyn SecureStore>> {
    Err(AppError::Unsupported(
        "the insecure file secure store is not compiled into this build".into(),
    ))
}
