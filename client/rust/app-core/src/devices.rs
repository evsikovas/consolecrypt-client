//! Trusted devices (ADR-0004): list, request trust (new device), approve
//! with the mandatory verification-code comparison (vault-core's two-step
//! `PendingApproval` → `confirm_verification_code` → `build_request`),
//! reject, revoke (incl. this device), rename.

use crate::app::{parse_vault_id, pick_envelopes, AppCore};
use crate::dto::*;
use crate::error::{AppError, AppResult};
use crate::key_trust::KeySource;
use crate::secrets::blocking;
use crate::session::Session;
use cc_crypto_core::{pairing_code_v2, DevicePublicKeys};
use cc_protocol::devices::{
    ApprovalIntentRequest, CreateDeviceTrustRequest, DeviceInfo, DeviceStatus, DeviceTrustRequest,
    RejectDeviceRequest, RevokeDeviceRequest, UpdateDeviceRequest,
};
use cc_protocol::{Bytes, DeviceId, DeviceRequestId, VaultId};
use cc_vault_core::{unlock_approved_device, unlock_with_device, PendingApproval, PinnedApprover};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Local setting: vault this device asked to be approved for.
const PENDING_TRUST_VAULT: &str = "app_core.pending_trust_vault";
/// Local setting (new device, protocol 1.11): the approver whose pairing
/// code v2 the user confirmed on this device. Never taken from the server
/// without that confirmation.
const PINNED_APPROVER: &str = "app_core.pinned_approver";

/// Local setting (new device): the approver last announced for this
/// device's request, shown until the user confirmed or rejected the code.
const SEEN_APPROVER: &str = "app_core.seen_approver";

/// Local setting (new device): the server announced approval v2
/// (`capabilities.vrk_commitment`) at some point. From then on an approval
/// without a signed v2 record is never taken for an older server (SEC-11).
const SERVER_APPROVAL_V2: &str = "app_core.server_approval_v2";

/// Whether approval v2 is expected: the server announces it now, or did so
/// earlier for this profile (remembered, so it cannot be switched off).
async fn approval_v2_expected(s: &Session, api: &cc_sync_core::ApiClient) -> AppResult<bool> {
    let now = api
        .meta()
        .await
        .is_ok_and(|m| m.capabilities.vrk_commitment);
    let storage = s.storage()?;
    if now {
        storage.setting_set(SERVER_APPROVAL_V2, true).await?;
        return Ok(true);
    }
    Ok(storage
        .setting_get::<bool>(SERVER_APPROVAL_V2)
        .await?
        .unwrap_or(false))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SeenApprover {
    request_id: DeviceRequestId,
    device_id: DeviceId,
    name: String,
    platform: String,
    encryption_public_key: Bytes,
    signing_public_key: Bytes,
}

impl SeenApprover {
    fn keys(&self) -> AppResult<DevicePublicKeys> {
        Ok(DevicePublicKeys::from_slices(
            self.encryption_public_key.as_slice(),
            self.signing_public_key.as_slice(),
        )?)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredApprover {
    request_id: DeviceRequestId,
    device_id: DeviceId,
    encryption_public_key: Bytes,
    signing_public_key: Bytes,
}

impl StoredApprover {
    fn pinned(&self) -> AppResult<PinnedApprover> {
        Ok(PinnedApprover {
            request_id: self.request_id,
            device_id: self.device_id,
            keys: DevicePublicKeys::from_slices(
                self.encryption_public_key.as_slice(),
                self.signing_public_key.as_slice(),
            )?,
        })
    }
}

fn parse_device_id(s: &str) -> AppResult<DeviceId> {
    DeviceId::from_str(s.trim()).map_err(|_| AppError::invalid("device_id", "not a valid id"))
}

fn parse_request_id(s: &str) -> AppResult<DeviceRequestId> {
    DeviceRequestId::from_str(s.trim())
        .map_err(|_| AppError::invalid("request_id", "not a valid id"))
}

fn device_dto(d: &DeviceInfo, vault: Option<VaultId>) -> DeviceDto {
    DeviceDto {
        device_id: d.device_id.to_string(),
        name: d.name.clone(),
        platform: d.platform.as_str().to_owned(),
        status: match d.status {
            DeviceStatus::Active => "active".into(),
            DeviceStatus::Revoked => "revoked".into(),
        },
        trusted_for_vault: vault.is_some_and(|v| d.trusted_vaults.contains(&v)),
        is_current: d.is_current,
        created_at_ms: ms(&d.created_at),
        last_seen_at_ms: opt_ms(&d.last_seen_at),
        revoked_at_ms: opt_ms(&d.revoked_at),
        // Servers before 1.7 omit `trusted` (false): derive it.
        trusted: d.trusted || !d.trusted_vaults.is_empty(),
        quarantine_until_ms: opt_ms(&d.quarantine_until),
    }
}

fn request_dto(r: &DeviceTrustRequest) -> TrustRequestDto {
    TrustRequestDto {
        request_id: r.request_id.to_string(),
        device_id: r.device.device_id.to_string(),
        device_name: r.device.name.clone(),
        platform: r.device.platform.as_str().to_owned(),
        vault_ids: r.vault_ids.iter().map(ToString::to_string).collect(),
        status: crate::error::protocol_code_name_of(&r.status),
        created_at_ms: ms(&r.created_at),
        expires_at_ms: ms(&r.expires_at),
    }
}

impl AppCore {
    /// Devices of the account and pending trust requests.
    pub async fn list_devices(&self) -> AppResult<DeviceListDto> {
        let s = self.session().await?;
        let api = s.api()?;
        let r = api.list_devices().await?;
        // Capability gates (protocol 1.7, 1.11); an unreachable meta keeps
        // the old UI.
        let meta = api.meta().await.ok();
        let device_protection = meta
            .as_ref()
            .is_some_and(|m| m.capabilities.device_protection);
        let vrk_commitment = meta.as_ref().is_some_and(|m| m.capabilities.vrk_commitment);
        let vault = s.vault_id();
        let pin = s.pinned_vault_key().await;
        let vault_key_conflicts = match s.unlocked_opt().await {
            Some(u) => u.key_conflicts().into_iter().map(|c| c.device_id).collect(),
            None => Vec::new(),
        };
        Ok(DeviceListDto {
            devices: r.devices.iter().map(|d| device_dto(d, vault)).collect(),
            pending_requests: r.pending_requests.iter().map(request_dto).collect(),
            device_protection,
            vrk_commitment,
            vault_key_unconfirmed: pin.is_some_and(|p| p.unconfirmed),
            vault_key_conflicts,
        })
    }

    /// This installation's verification code (shown on the new device
    /// while another device approves it).
    pub async fn device_verification_code(&self) -> AppResult<String> {
        Ok(self
            .session()
            .await?
            .identity()
            .verification_code()
            .to_string())
    }

    /// New device: ask trusted devices to approve this one for `vault_id`
    /// (default: every vault of the account). Show the returned code.
    pub async fn request_device_approval(
        &self,
        vault_id: Option<String>,
    ) -> AppResult<OwnTrustRequestDto> {
        let s = self.session().await?;
        let api = s.api()?;
        let vault_ids = match vault_id.as_deref() {
            Some(v) => {
                let v = parse_vault_id(v)?;
                s.storage()?.setting_set(PENDING_TRUST_VAULT, v).await?;
                vec![v]
            }
            None => Vec::new(),
        };
        let r = api
            .create_trust_request(&CreateDeviceTrustRequest { vault_ids })
            .await?;
        // A new request starts without a confirmed approver.
        s.storage()?
            .write(|tx| {
                tx.setting_delete(PINNED_APPROVER)?;
                tx.setting_delete(SEEN_APPROVER).map(|_| ())
            })
            .await?;
        let vrk_commitment = approval_v2_expected(&s, &api).await?;
        Ok(OwnTrustRequestDto {
            request_id: r.request_id.to_string(),
            verification_code: s.identity().verification_code().to_string(),
            expires_at_ms: ms(&r.expires_at),
            vrk_commitment,
        })
    }

    /// New device (protocol 1.11): the approval progress of `request_id` —
    /// once a trusted device announced itself, the pairing code v2 over both
    /// devices' keys to compare with the code on the approving device. The
    /// announced approver is remembered, so the code stays available after
    /// the approving device already approved.
    pub async fn device_approval_status(
        &self,
        request_id: String,
    ) -> AppResult<ApprovalProgressDto> {
        let s = self.session().await?;
        let api = s.api()?;
        let rid = parse_request_id(&request_id)?;
        let vrk_commitment = approval_v2_expected(&s, &api).await?;
        let own = s.identity();
        let approver = self.announced_approver(&s, rid).await?;
        let pairing_code = approver
            .as_ref()
            .map(|a| -> AppResult<String> {
                Ok(pairing_code_v2(
                    rid,
                    own.device_id(),
                    &own.public_keys(),
                    a.device_id,
                    &a.keys()?,
                )
                .to_string())
            })
            .transpose()?;
        let stored: Option<StoredApprover> = s.storage()?.setting_get(PINNED_APPROVER).await?;
        Ok(ApprovalProgressDto {
            request_id: rid.to_string(),
            vrk_commitment,
            own_code: own.verification_code().to_string(),
            approver_device_id: approver.as_ref().map(|a| a.device_id.to_string()),
            approver_name: approver.as_ref().map(|a| a.name.clone()),
            approver_platform: approver.as_ref().map(|a| a.platform.clone()),
            pairing_code,
            approver_confirmed: stored.is_some_and(|p| {
                p.request_id == rid
                    && approver.as_ref().is_some_and(|a| {
                        a.device_id == p.device_id
                            && a.encryption_public_key == p.encryption_public_key
                            && a.signing_public_key == p.signing_public_key
                    })
            }),
        })
    }

    /// The approver announced for this device's request `rid`: from the
    /// pending request, or — once it is no longer pending — the device
    /// remembered from it, with the keys the server lists for that device.
    async fn announced_approver(
        &self,
        s: &Session,
        rid: DeviceRequestId,
    ) -> AppResult<Option<SeenApprover>> {
        let own = s.identity().device_id();
        let list = s.api()?.list_devices().await?;
        if let Some(a) = list
            .pending_requests
            .iter()
            .find(|r| r.request_id == rid && r.device.device_id == own)
            .and_then(|r| r.approver.as_ref())
        {
            let seen = SeenApprover {
                request_id: rid,
                device_id: a.device_id,
                name: a.name.clone(),
                platform: a.platform.as_str().to_owned(),
                encryption_public_key: a.encryption_public_key.clone(),
                signing_public_key: a.signing_public_key.clone(),
            };
            s.storage()?
                .setting_set(SEEN_APPROVER, seen.clone())
                .await?;
            return Ok(Some(seen));
        }
        let remembered: Option<SeenApprover> = s.storage()?.setting_get(SEEN_APPROVER).await?;
        Ok(remembered.filter(|r| r.request_id == rid).and_then(|r| {
            let d = list
                .devices
                .iter()
                .find(|d| d.device_id == r.device_id && d.status == DeviceStatus::Active)?;
            Some(SeenApprover {
                encryption_public_key: d.encryption_public_key.clone(),
                signing_public_key: d.signing_public_key.clone(),
                ..r
            })
        }))
    }

    /// New device (protocol 1.11), ONLY after the user confirmed that the
    /// pairing code on this screen matches the one on `approver_device_id`:
    /// pin that device's keys for verifying its signed approval.
    /// `shown_code` is the code the user compared; it is recomputed from the
    /// keys being pinned and must match, so the confirmation is bound to
    /// what was on the screen.
    pub async fn confirm_device_approver(
        &self,
        request_id: String,
        approver_device_id: String,
        shown_code: String,
    ) -> AppResult<()> {
        let s = self.session().await?;
        let rid = parse_request_id(&request_id)?;
        let approver_id = parse_device_id(&approver_device_id)?;
        let approver = self
            .announced_approver(&s, rid)
            .await?
            .filter(|a| a.device_id == approver_id)
            .ok_or_else(|| AppError::Approval("the approving device changed".into()))?;
        let own = s.identity();
        let code = pairing_code_v2(
            rid,
            own.device_id(),
            &own.public_keys(),
            approver.device_id,
            &approver.keys()?,
        );
        if !code.matches_str(&shown_code) {
            return Err(AppError::Approval(
                "the pairing code changed; compare it again".into(),
            ));
        }
        s.storage()?
            .setting_set(
                PINNED_APPROVER,
                StoredApprover {
                    request_id: rid,
                    device_id: approver.device_id,
                    encryption_public_key: approver.encryption_public_key,
                    signing_public_key: approver.signing_public_key,
                },
            )
            .await?;
        tracing::info!(approver = %approver_id, "approver pinned after the pairing code matched");
        Ok(())
    }

    /// New device, after approval: fetch this device's envelope and open
    /// the vault with the device key — on a 1.11 server only after the
    /// signed approval v2 verified with the approver key pinned when the
    /// pairing code matched (envelope hash, vault id from the signed list,
    /// `HKDF(VRK)` against the signed commitment). Older servers and
    /// approvers: trust on first use, marked "vault key not confirmed" until
    /// the next passphrase unlock (TR-7). Any mismatch → `vault_key_mismatch`
    /// before anything is attested, stored or synced. Then: replace the
    /// envelope with this device's trust marker (S5), seal the local
    /// database and start syncing. `Ok(false)` = not approved yet.
    pub async fn finish_device_approval(&self, vault_id: Option<String>) -> AppResult<bool> {
        let s = self.session().await?;
        if s.unlocked_opt().await.is_some() || s.vault_id().is_some() {
            // Already joined (the server envelope may be the trust marker
            // by now: never open it again).
            return Ok(true);
        }
        let api = s.api()?;
        let vault_id = match vault_id.as_deref() {
            Some(v) => parse_vault_id(v)?,
            None => match s
                .storage()?
                .setting_get::<VaultId>(PENDING_TRUST_VAULT)
                .await?
            {
                Some(v) => v,
                None => {
                    let vaults = api.list_vaults().await?.vaults;
                    match vaults.as_slice() {
                        [one] => one.vault_id,
                        _ => {
                            return Err(AppError::invalid(
                                "vault_id",
                                "the account has several vaults; pass the vault id",
                            ))
                        }
                    }
                }
            },
        };
        let material = api.recovery_vault_envelope(vault_id).await?;
        let Some(dev) = material.device_envelope.clone() else {
            return Ok(false);
        };
        let pinned: Option<StoredApprover> = s.storage()?.setting_get(PINNED_APPROVER).await?;
        let v2_expected = approval_v2_expected(&s, &api).await?;
        let identity = s.identity();
        let (vault, source) = match (&material.device_approval, &pinned) {
            // Approval v2 checked with the key the user confirmed.
            (Some(record), Some(approver)) => {
                let approver = approver.pinned()?;
                let record = record.clone();
                let id = identity.clone();
                match blocking(move || {
                    Ok(unlock_approved_device(
                        vault_id, &dev, &record, &approver, &id,
                    )?)
                })
                .await
                {
                    Ok((v, _)) => (v, KeySource::ApprovalVerified),
                    Err(AppError::DeviceNotAuthorized) => return Ok(false),
                    Err(AppError::VaultKeyMismatch {
                        reason, received, ..
                    }) => {
                        let pinned = s.pinned_vault_key().await.and_then(|p| p.commitment());
                        let received = received.as_deref().and_then(unhex32);
                        return Err(s.key_mismatch(vault_id, pinned, received, &reason).await);
                    }
                    Err(e) => return Err(e),
                }
            }
            // A v2 approval exists but the pairing code was not confirmed
            // on this device yet.
            (Some(_), None) => {
                return Err(AppError::Approval("confirm_pairing_code".into()));
            }
            // The user confirmed an approver that signs v2, but the server
            // shows no v2 approval: it was withheld.
            (None, Some(_)) => {
                let pinned = s.pinned_vault_key().await.and_then(|p| p.commitment());
                return Err(s
                    .key_mismatch(vault_id, pinned, None, "approval_missing")
                    .await);
            }
            // The server announced approval v2 before (or does now) but
            // shows none: it is withheld, or the approver is too old to
            // sign one. Never fall back to trust on first use (SEC-11).
            (None, None) if v2_expected => {
                let pinned = s.pinned_vault_key().await.and_then(|p| p.commitment());
                return Err(s
                    .key_mismatch(vault_id, pinned, None, "approval_missing")
                    .await);
            }
            // Older server or approver (TR-7): trust on first use.
            (None, None) => {
                let id = identity.clone();
                match blocking(move || Ok(unlock_with_device(vault_id, &dev, &id)?)).await {
                    Ok(v) => (v, KeySource::ApprovalUnverified),
                    Err(AppError::DeviceNotAuthorized) => return Ok(false),
                    Err(e) => return Err(e),
                }
            }
        };
        // TR-3 against an existing pin (re-joining profile) before anything
        // is sent or stored.
        let pin = s.check_vault_key(&vault, source).await?;
        let attest = vault.attest_device_request(&s.identity())?;
        // TR-4: the attest response is a confirmation only, never cached.
        api.attest_device(s.device_id(), &attest).await?;
        let info = api.get_vault(vault_id).await?;
        s.storage()?.upsert_vault_info(info.clone()).await?;
        let (pw, rec, _) = match api.list_envelopes(vault_id).await {
            Ok(l) => pick_envelopes(&l.envelopes, s.device_id()),
            Err(_) => (None, None, None),
        };
        // Password / recovery envelopes are cached for offline unlock; every
        // later use checks the key they yield against the pin (TR-3).
        s.cache_envelopes(
            vault_id,
            pw.or(material.password_envelope),
            rec.or(material.recovery_envelope),
            None,
        )
        .await?;
        s.set_vault_id(vault_id).await?;
        s.store_pin(pin).await?;
        s.storage()?
            .write(|tx| {
                tx.setting_delete(PINNED_APPROVER)?;
                tx.setting_delete(SEEN_APPROVER).map(|_| ())
            })
            .await?;
        s.seal_storage(&vault).await?;
        let u = s.install_unlocked(vault).await?;
        u.observe_vault_info(std::slice::from_ref(&info)).await;
        tracing::info!(
            vault_id = %vault_id,
            verified = source == KeySource::ApprovalVerified,
            "device approval completed"
        );
        Ok(true)
    }

    /// Trusted device, step 1: validate a pending request and compute its
    /// verification code from the keys **the server reports**. Show it and
    /// ask the user to compare with the code on the new device.
    pub async fn start_device_approval(&self, request_id: String) -> AppResult<PendingApprovalDto> {
        let (s, u) = self.unlocked().await?;
        let rid = parse_request_id(&request_id)?;
        let list = s.api()?.list_devices().await?;
        let req = list
            .pending_requests
            .iter()
            .find(|r| r.request_id == rid)
            .ok_or_else(|| AppError::not_found("trust request", rid))?;
        let pending = PendingApproval::new(req, &s.identity(), chrono::Utc::now())?;
        if !pending.requested_vault_ids().is_empty()
            && !pending.requested_vault_ids().contains(&u.vault_id)
        {
            return Err(AppError::Approval(
                "the request is for another vault".into(),
            ));
        }
        // Protocol 1.11: announce this device as the approver; both devices
        // then show the pairing code v2 over both devices' keys.
        let api = s.api()?;
        let v2 = api
            .meta()
            .await
            .is_ok_and(|m| m.capabilities.vrk_commitment);
        if v2 {
            api.approval_intent(
                pending.new_device_id(),
                &ApprovalIntentRequest { request_id: rid },
            )
            .await?;
        }
        let code = if v2 {
            pending.pairing_code_v2(&s.identity())
        } else {
            pending.verification_code()
        };
        let dto = PendingApprovalDto {
            request_id: rid.to_string(),
            device_id: pending.new_device_id().to_string(),
            device_name: pending.new_device_name().to_owned(),
            verification_code: code.to_string(),
            requested_vault_ids: pending
                .requested_vault_ids()
                .iter()
                .map(ToString::to_string)
                .collect(),
            code_version: if v2 { 2 } else { 1 },
        };
        u.approvals
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(rid, pending);
        let mut v2_set = u.approvals_v2.lock().unwrap_or_else(|p| p.into_inner());
        if v2 {
            v2_set.insert(rid);
        } else {
            v2_set.remove(&rid);
        }
        Ok(dto)
    }

    /// Trusted device, step 2 — call ONLY after the user confirmed that both
    /// verification codes match: encrypts the vault key to the new device,
    /// signs and sends the approval.
    pub async fn confirm_device_approval(&self, request_id: String) -> AppResult<()> {
        let (s, u) = self.unlocked().await?;
        let rid = parse_request_id(&request_id)?;
        let pending = u
            .approvals
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&rid)
            .ok_or_else(|| {
                AppError::Approval("call start_device_approval and compare codes first".into())
            })?;
        let new_device = pending.new_device_id();
        let v2 = u
            .approvals_v2
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&rid);
        let confirmed = pending.confirm_verification_code();
        let vault = u.vault()?;
        let req = if v2 {
            confirmed.build_request_v2(&s.identity(), &[vault.as_ref()], chrono::Utc::now())?
        } else {
            confirmed.build_request(&s.identity(), &[vault.as_ref()], chrono::Utc::now())?
        };
        s.api()?.approve_device(new_device, &req).await?;
        tracing::info!(device_id = %new_device, "device approved");
        Ok(())
    }

    /// Reject a pending trust request.
    pub async fn reject_device_request(&self, request_id: String) -> AppResult<()> {
        let s = self.session().await?;
        let api = s.api()?;
        let rid = parse_request_id(&request_id)?;
        let list = api.list_devices().await?;
        let req = list
            .pending_requests
            .iter()
            .find(|r| r.request_id == rid)
            .ok_or_else(|| AppError::not_found("trust request", rid))?;
        if let Ok((_, u)) = self.unlocked().await {
            u.approvals
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&rid);
        }
        api.reject_device(
            req.device.device_id,
            &RejectDeviceRequest { request_id: rid },
        )
        .await?;
        Ok(())
    }

    /// Revoke a device of the account (a stolen laptop). Revoking this
    /// device stops syncing and signs out; reconnecting needs a new device
    /// identity ([`AppCore::reauthenticate`]).
    pub async fn revoke_device(&self, device_id: String, reason: Option<String>) -> AppResult<()> {
        let s = self.session().await?;
        let api = s.api()?;
        let id = parse_device_id(&device_id)?;
        api.revoke_device(id, &RevokeDeviceRequest { reason })
            .await?;
        tracing::info!(device_id = %id, "device revoked");
        if id == s.device_id() {
            if let Some(u) = s.unlocked_opt().await {
                u.stop_engine().await;
            }
            let _ = api.clear_tokens().await;
            self.inner.ctx.emit(AppEvent::DeviceRevoked {
                device_id: id.to_string(),
                is_self: true,
            });
        }
        Ok(())
    }

    /// Rename a device (the server allows renaming only the current one).
    pub async fn rename_device(&self, device_id: String, name: String) -> AppResult<DeviceDto> {
        let s = self.session().await?;
        let name = crate::validate::display_name("name", &name)?;
        let d = s
            .api()?
            .update_device(parse_device_id(&device_id)?, &UpdateDeviceRequest { name })
            .await?;
        Ok(device_dto(&d, s.vault_id()))
    }
}

fn unhex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}
