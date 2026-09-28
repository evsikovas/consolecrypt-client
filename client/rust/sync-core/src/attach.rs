//! Profile kind transitions (ADR-0106): "enable sync" for a local vault
//! (with the reconnect path when the vault already exists server-side) and
//! "disconnect" back to a local profile.

use crate::api::{ApiClient, ApiError};
use crate::engine::{SyncEngine, SyncEngineConfig, SyncReport};
use crate::error::SyncError;
use crate::store::{check_size, ObjectStore};
use cc_protocol::auth::LogoutRequest;
use cc_protocol::devices::{AttestDeviceRequest, RevokeDeviceRequest};
use cc_protocol::vaults::CreateVaultRequest;
use cc_protocol::ErrorCode;
use cc_storage_core::{AttachMode, AttachSummary, DetachSummary, Profile, ProfileKind};

/// Input for [`enable_sync`]. The caller (app-core + vault-core) builds the
/// DTOs from the unlocked vault: VAK, existing password/recovery envelopes
/// and a device envelope for the registered device.
#[derive(Debug, Clone)]
pub struct EnableSync {
    /// `POST /v1/vaults` body; `vault_id` must be the local vault's id.
    pub create_vault: CreateVaultRequest,
    /// Used if the vault already exists (reconnect path). `None` → the call
    /// fails with [`SyncError::VaultExists`] in that case.
    pub attest: Option<AttestDeviceRequest>,
    /// The `Synced` profile row to store (server URL, user id, device id).
    pub profile: Profile,
}

/// Which path [`enable_sync`] took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnableSyncPath {
    /// Vault created on the server; everything uploaded as creates.
    Created,
    /// Vault existed; attested, re-snapshotted and merged.
    Reconnected,
    /// The profile was already attached; the pending upload was resumed.
    Resumed,
}

/// Result of [`enable_sync`].
#[derive(Debug)]
pub struct EnableSyncOutcome {
    pub path: EnableSyncPath,
    /// Conversion summary (`None` when resuming).
    pub attach: Option<AttachSummary>,
    /// First sync cycle. On error the vault is still attached and the upload
    /// is queued: start the engine (or call [`enable_sync`] again) to resume.
    pub upload: Result<SyncReport, SyncError>,
}

/// Attach the store's local vault to a server (ADR-0106 "enable sync").
///
/// 1. If the profile is already `Synced`, only resumes the upload.
/// 2. `POST /v1/vaults`; on `already_exists` attests with `req.attest` and
///    takes the reconnect path.
/// 3. In one local transaction: re-encrypts every live object for its new
///    revision via the codec, queues it with a persisted `mutation_id`,
///    drops tombstones (fresh) and switches the profile to `Synced`.
/// 4. Runs a sync cycle (push in batches, resolve, pull).
///
/// The vault must be unlocked (codec usable).
pub async fn enable_sync(
    store: &ObjectStore,
    api: &ApiClient,
    req: EnableSync,
    engine_config: SyncEngineConfig,
) -> Result<(SyncEngine, EnableSyncOutcome), SyncError> {
    let vault_id = store.vault_id();
    if req.create_vault.vault_id != vault_id {
        return Err(SyncError::Invalid(
            "create request is for another vault".into(),
        ));
    }
    if req.profile.kind != ProfileKind::Synced {
        return Err(SyncError::Invalid("target profile must be synced".into()));
    }
    if req.profile.device_id != engine_config.device_id {
        return Err(SyncError::Invalid("device id mismatch".into()));
    }
    let current = store.storage().get_profile().await?;
    if current
        .as_ref()
        .is_some_and(|p| p.kind == ProfileKind::Synced)
    {
        let engine = SyncEngine::new(store.clone(), api.clone(), engine_config).await?;
        let upload = engine.sync_now().await;
        return Ok((
            engine,
            EnableSyncOutcome {
                path: EnableSyncPath::Resumed,
                attach: None,
                upload,
            },
        ));
    }

    let (mode, path) = match api.create_vault(&req.create_vault).await {
        Ok(info) => {
            store.storage().upsert_vault_info(info).await?;
            (AttachMode::Fresh, EnableSyncPath::Created)
        }
        Err(e) if e.is_code(ErrorCode::AlreadyExists) => {
            let attest = req.attest.as_ref().ok_or(SyncError::VaultExists)?;
            api.attest_device(req.profile.device_id, attest).await?;
            if let Ok(info) = api.get_vault(vault_id).await {
                store.storage().upsert_vault_info(info).await?;
            }
            (AttachMode::Reconnect, EnableSyncPath::Reconnected)
        }
        Err(e) => return Err(e.into()),
    };

    let codec = store.codec().clone();
    let profile = req.profile.clone();
    let summary = store
        .storage()
        .write(move |tx| {
            tx.attach_to_server::<SyncError, _>(vault_id, &profile, mode, |obj, revision| {
                let body = obj
                    .body
                    .as_ref()
                    .ok_or_else(|| SyncError::Invalid("live object without body".into()))?;
                let payload =
                    codec.decrypt_hinted(obj.object_id, obj.revision, body, obj.kek_class_hint)?;
                let body = codec.encrypt(obj.object_id, revision, &payload)?;
                check_size(&body)?;
                Ok(body)
            })
        })
        .await?;
    tracing::info!(vault_id = %vault_id, ?path, queued = summary.queued, "vault attached to server");

    let engine = SyncEngine::new(store.clone(), api.clone(), engine_config).await?;
    let upload = engine.sync_now().await;
    Ok((
        engine,
        EnableSyncOutcome {
            path,
            attach: Some(summary),
            upload,
        },
    ))
}

/// Options for [`disconnect`].
#[derive(Debug, Clone, Default)]
pub struct DisconnectOptions {
    /// Revoke this device on the server (otherwise just log out).
    pub revoke_device: bool,
    /// Audit reason for the revocation.
    pub reason: Option<String>,
}

/// Result of [`disconnect`].
#[derive(Debug)]
pub struct DisconnectOutcome {
    pub detach: DetachSummary,
    /// Server call (logout / revoke) failure, if any — the local conversion
    /// happens regardless.
    pub server_error: Option<ApiError>,
}

/// Convert a synced vault back to local-only (ADR-0106 "disconnect"): stop
/// the engine, optionally revoke this device (else log out), forget tokens,
/// keep all local data and switch the profile to `local_profile`. The server
/// copy is left untouched.
pub async fn disconnect(
    store: &ObjectStore,
    engine: Option<&SyncEngine>,
    api: Option<&ApiClient>,
    local_profile: Profile,
    options: DisconnectOptions,
) -> Result<DisconnectOutcome, SyncError> {
    if local_profile.kind != ProfileKind::Local {
        return Err(SyncError::Invalid("target profile must be local".into()));
    }
    if let Some(engine) = engine {
        engine.stop().await;
    }
    let mut server_error = None;
    if let Some(api) = api {
        let r = if options.revoke_device {
            api.revoke_device(
                local_profile.device_id,
                &RevokeDeviceRequest {
                    reason: options.reason.clone(),
                },
            )
            .await
        } else {
            api.logout(&LogoutRequest::default()).await
        };
        if let Err(e) = r {
            tracing::warn!(error = %e, "server call during disconnect failed");
            server_error = Some(e);
        }
        let _ = api.clear_tokens().await;
    }
    let vault_id = store.vault_id();
    let detach = store
        .storage()
        .write(move |tx| tx.detach_to_local(vault_id, &local_profile))
        .await?;
    Ok(DisconnectOutcome {
        detach,
        server_error,
    })
}
