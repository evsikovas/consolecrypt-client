//! Trusted devices (ADR-0004): list, request trust (new device), approve
//! with the mandatory verification-code comparison (vault-core's two-step
//! `PendingApproval` → `confirm_verification_code` → `build_request`),
//! reject, revoke (incl. this device), rename.

use crate::app::{parse_vault_id, pick_envelopes, AppCore};
use crate::dto::*;
use crate::error::{AppError, AppResult};
use crate::secrets::blocking;
use cc_protocol::devices::{
    CreateDeviceTrustRequest, DeviceInfo, DeviceStatus, DeviceTrustRequest, RejectDeviceRequest,
    RevokeDeviceRequest, UpdateDeviceRequest,
};
use cc_protocol::{DeviceId, DeviceRequestId, VaultId};
use cc_vault_core::{unlock_with_device, PendingApproval};
use std::str::FromStr;

/// Local setting: vault this device asked to be approved for.
const PENDING_TRUST_VAULT: &str = "app_core.pending_trust_vault";

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
        let r = s.api()?.list_devices().await?;
        let vault = s.vault_id();
        Ok(DeviceListDto {
            devices: r.devices.iter().map(|d| device_dto(d, vault)).collect(),
            pending_requests: r.pending_requests.iter().map(request_dto).collect(),
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
                s.storage.setting_set(PENDING_TRUST_VAULT, v).await?;
                vec![v]
            }
            None => Vec::new(),
        };
        let r = api
            .create_trust_request(&CreateDeviceTrustRequest { vault_ids })
            .await?;
        Ok(OwnTrustRequestDto {
            request_id: r.request_id.to_string(),
            verification_code: s.identity().verification_code().to_string(),
            expires_at_ms: ms(&r.expires_at),
        })
    }

    /// New device, after approval: fetch this device's envelope, open the
    /// vault with the device key and start syncing. `Ok(false)` = not
    /// approved yet.
    pub async fn finish_device_approval(&self, vault_id: Option<String>) -> AppResult<bool> {
        let s = self.session().await?;
        if s.unlocked_opt().await.is_some() {
            return Ok(true);
        }
        let api = s.api()?;
        let vault_id = match vault_id.as_deref() {
            Some(v) => parse_vault_id(v)?,
            None => match s
                .storage
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
        let identity = s.identity();
        let vault = match blocking(move || Ok(unlock_with_device(vault_id, &dev, &identity)?)).await
        {
            Ok(v) => v,
            Err(AppError::DeviceNotAuthorized) => return Ok(false),
            Err(e) => return Err(e),
        };
        let info = api.get_vault(vault_id).await?;
        s.storage.upsert_vault_info(info.clone()).await?;
        let (pw, rec, own) = match api.list_envelopes(vault_id).await {
            Ok(l) => pick_envelopes(&l.envelopes, s.device_id()),
            Err(_) => (None, None, None),
        };
        s.cache_envelopes(
            vault_id,
            pw.or(material.password_envelope),
            rec.or(material.recovery_envelope),
            own.or(material.device_envelope),
        )
        .await?;
        s.set_vault_id(vault_id).await?;
        let u = s.install_unlocked(vault).await?;
        u.observe_vault_info(std::slice::from_ref(&info)).await;
        tracing::info!(vault_id = %vault_id, "device approval completed");
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
        let dto = PendingApprovalDto {
            request_id: rid.to_string(),
            device_id: pending.new_device_id().to_string(),
            device_name: pending.new_device_name().to_owned(),
            verification_code: pending.verification_code().to_string(),
            requested_vault_ids: pending
                .requested_vault_ids()
                .iter()
                .map(ToString::to_string)
                .collect(),
        };
        u.approvals
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(rid, pending);
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
        let confirmed = pending.confirm_verification_code();
        let vault = u.vault()?;
        let req = confirmed.build_request(&s.identity(), &[vault.as_ref()], chrono::Utc::now())?;
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
