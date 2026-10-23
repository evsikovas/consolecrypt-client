//! Pinned keys of colleagues' devices and their verification (audit H3).
//!
//! * TR-12: the keys of a colleague's device are pinned the first time it
//!   is chosen (as a recipient, or as the owner of an accepted share) with
//!   status `chosen`. Other keys reported later for the same device turn
//!   the pin into `keys_changed`: no new access until the new keys are
//!   verified; existing shares are never re-encrypted for them.
//! * TR-11: verification outside the server — the long safety number (12
//!   groups of 5 digits from both parties' keys, typed in) or the QR
//!   payload with the colleague's full keys. A failed comparison is
//!   journalled and never changes a pin.
//! * TR-14: "only verified recipients" (vault setting), on by default for
//!   secrets, Kubernetes contexts and database connections.
//! * TR-16: grants, accept and reconcile take a `verification_id` (the pin);
//!   the status is checked here, never in the UI.
//! * TR-18: existing shares: participants and owners are pinned `chosen`
//!   once; differing keys of one device across shares → `keys_changed`; one
//!   banner lists unverified recipients of sensitive shares.
//! * TR-20: pins live in the encrypted profile database (see
//!   `cc_models::peer_pin`).

use super::*;
use crate::key_trust::hex as lower_hex;
use cc_crypto_core::sharing_verification::{
    parse_verification_qr, sharing_safety_number, verification_qr_payload, SharingParty,
};
use cc_models::peer_pin::{PeerPin, PeerPinStatus, PeerVerificationMethod};
use cc_models::settings::SharingVerificationPolicy;
use cc_models::VaultObject;

/// Encrypted profile database keys.
const PINS_KEY: &str = "cc.sharing.peer_pins.v1";
const MIGRATED_KEY: &str = "cc.sharing.peer_pins.migrated.v1";
/// `true` while the one-time "verify colleagues" banner is due.
const BANNER_KEY: &str = "cc.sharing.verify_banner.v1";
const MAX_PINS: usize = 5_000;

fn status_name(s: PeerPinStatus) -> &'static str {
    match s {
        PeerPinStatus::Chosen => "chosen",
        PeerPinStatus::Verified => "verified",
        PeerPinStatus::KeysChanged => "keys_changed",
        PeerPinStatus::Unknown => "unknown",
    }
}

fn method_name(m: PeerVerificationMethod) -> &'static str {
    match m {
        PeerVerificationMethod::QrCode => "qr_code",
        PeerVerificationMethod::SafetyNumber => "safety_number",
        PeerVerificationMethod::Unknown => "unknown",
    }
}

pub(crate) fn pin_dto(p: &PeerPin) -> PeerPinDto {
    PeerPinDto {
        verification_id: p.id.to_string(),
        server_instance_id: p.instance_id.to_string(),
        user_id: p.user_id.to_string(),
        device_id: p.device_id.to_string(),
        status: status_name(p.status).to_owned(),
        method: p.method.map(|m| method_name(m).to_owned()),
        verified_at_ms: p.verified_at.map(|t| t.timestamp_millis()),
        first_seen_ms: p.first_seen.timestamp_millis(),
        email: p.email.clone(),
    }
}

fn recipient_unverified(device: DeviceId, reason: &str) -> AppError {
    AppError::RecipientUnverified {
        device_id: device.to_string(),
        reason: reason.to_owned(),
    }
}

fn keys_match(pin: &PeerPin, member: &SharingMember) -> bool {
    pin.user_id == member.user_id
        && pin.encryption_public_key == member.encryption_public_key
        && pin.signing_public_key == member.signing_public_key
}

/// The pins of this profile (encrypted database).
pub(super) struct Pins {
    pins: Vec<PeerPin>,
    changed: bool,
}

impl Pins {
    pub(super) async fn load(env: &Environment) -> AppResult<Self> {
        let raw: Option<Vec<PeerPin>> = env
            .wait(env.session.storage()?.setting_get(PINS_KEY))
            .await?;
        Ok(Self {
            pins: raw.unwrap_or_default(),
            changed: false,
        })
    }

    pub(super) async fn save(&mut self, env: &Environment) -> AppResult<()> {
        if !self.changed {
            return Ok(());
        }
        if self.pins.len() > MAX_PINS {
            return Err(invalid("too many pinned colleague devices"));
        }
        let pins = self.pins.clone();
        env.wait(env.session.storage()?.setting_set(PINS_KEY, pins))
            .await?;
        self.changed = false;
        Ok(())
    }

    fn find(&self, instance: Uuid, device: DeviceId) -> Option<&PeerPin> {
        self.pins
            .iter()
            .find(|p| p.instance_id == instance && p.device_id == device)
    }

    fn find_mut(&mut self, instance: Uuid, device: DeviceId) -> Option<&mut PeerPin> {
        self.pins
            .iter_mut()
            .find(|p| p.instance_id == instance && p.device_id == device)
    }

    fn by_id(&self, id: &str) -> Option<&PeerPin> {
        let id = Uuid::from_str(id.trim()).ok()?;
        self.pins.iter().find(|p| p.id == id)
    }

    /// TR-12: pin `member` (`create`) or only notice changed keys of an
    /// existing pin. Returns the pin after the update.
    fn observe(
        &mut self,
        instance: Uuid,
        member: &SharingMember,
        email: Option<String>,
        create: bool,
    ) -> Option<PeerPin> {
        if let Some(pin) = self.find_mut(instance, member.device_id) {
            if !keys_match(pin, member) {
                let reported = (
                    Some(member.encryption_public_key.clone()),
                    Some(member.signing_public_key.clone()),
                );
                if pin.status != PeerPinStatus::KeysChanged
                    || (
                        pin.reported_encryption_public_key.clone(),
                        pin.reported_signing_public_key.clone(),
                    ) != reported
                {
                    tracing::warn!(
                        target: "security",
                        device_id = %member.device_id,
                        "a colleague device reported other keys than the pinned ones"
                    );
                    pin.status = PeerPinStatus::KeysChanged;
                    pin.reported_encryption_public_key = reported.0;
                    pin.reported_signing_public_key = reported.1;
                    self.changed = true;
                }
            } else if pin.email.is_none() && email.is_some() {
                pin.email = email;
                self.changed = true;
            }
            return self.find(instance, member.device_id).cloned();
        }
        if !create {
            return None;
        }
        let pin = PeerPin {
            id: Uuid::new_v4(),
            instance_id: instance,
            user_id: member.user_id,
            device_id: member.device_id,
            encryption_public_key: member.encryption_public_key.clone(),
            signing_public_key: member.signing_public_key.clone(),
            status: PeerPinStatus::Chosen,
            method: None,
            verified_at: None,
            first_seen: chrono::Utc::now(),
            email,
            reported_encryption_public_key: None,
            reported_signing_public_key: None,
        };
        self.pins.push(pin.clone());
        self.changed = true;
        Some(pin)
    }

    /// The user verified exactly these keys: pin them as verified
    /// (replacing changed keys).
    fn verify(
        &mut self,
        instance: Uuid,
        member: &SharingMember,
        method: PeerVerificationMethod,
        email: Option<String>,
    ) -> PeerPin {
        let now = chrono::Utc::now();
        self.changed = true;
        if let Some(pin) = self.find_mut(instance, member.device_id) {
            pin.user_id = member.user_id;
            pin.encryption_public_key = member.encryption_public_key.clone();
            pin.signing_public_key = member.signing_public_key.clone();
            pin.status = PeerPinStatus::Verified;
            pin.method = Some(method);
            pin.verified_at = Some(now);
            pin.reported_encryption_public_key = None;
            pin.reported_signing_public_key = None;
            if pin.email.is_none() {
                pin.email = email;
            }
            return pin.clone();
        }
        let pin = PeerPin {
            id: Uuid::new_v4(),
            instance_id: instance,
            user_id: member.user_id,
            device_id: member.device_id,
            encryption_public_key: member.encryption_public_key.clone(),
            signing_public_key: member.signing_public_key.clone(),
            status: PeerPinStatus::Verified,
            method: Some(method),
            verified_at: Some(now),
            first_seen: now,
            email,
            reported_encryption_public_key: None,
            reported_signing_public_key: None,
        };
        self.pins.push(pin.clone());
        pin
    }

    /// Put the pin status of `identity` into the DTO.
    pub(super) fn annotate(&self, instance: Uuid, identity: &mut SharingIdentityDto) {
        let Ok(device) = DeviceId::from_str(&identity.device_id) else {
            return;
        };
        let Some(pin) = self.find(instance, device) else {
            return;
        };
        let same = pin.user_id.to_string() == identity.user_id
            && lower_hex(pin.encryption_public_key.as_slice()) == identity.encryption_public_key
            && lower_hex(pin.signing_public_key.as_slice()) == identity.signing_public_key;
        let status = if same {
            pin.status
        } else {
            PeerPinStatus::KeysChanged
        };
        identity.trust = Some(status_name(status).to_owned());
        identity.verification_id = Some(pin.id.to_string());
        identity.verified_at_ms = pin
            .verified_at
            .filter(|_| status == PeerPinStatus::Verified)
            .map(|t| t.timestamp_millis());
    }

    /// TR-16: `verification_id` must name the pin of exactly `member`'s
    /// keys, usable under the policy.
    fn require(
        &self,
        instance: Uuid,
        member: &SharingMember,
        verification_id: Option<&str>,
        verified_only: bool,
    ) -> AppResult<()> {
        let pin = verification_id
            .and_then(|id| self.by_id(id))
            .filter(|p| p.instance_id == instance && p.device_id == member.device_id)
            .ok_or_else(|| recipient_unverified(member.device_id, "not_chosen"))?;
        if pin.status == PeerPinStatus::KeysChanged || !keys_match(pin, member) {
            return Err(recipient_unverified(member.device_id, "keys_changed"));
        }
        match pin.status {
            PeerPinStatus::Verified => Ok(()),
            PeerPinStatus::Chosen if !verified_only => Ok(()),
            PeerPinStatus::Chosen => Err(recipient_unverified(member.device_id, "unverified")),
            _ => Err(recipient_unverified(member.device_id, "unverified")),
        }
    }
}

/// TR-14: secrets, Kubernetes contexts, database connections and anything
/// that carries secrets.
pub(super) fn is_sensitive(kind: SharedItemKind, carries_secrets: bool) -> bool {
    carries_secrets
        || matches!(
            kind,
            SharedItemKind::Secret | SharedItemKind::KubeContext | SharedItemKind::DbConnection
        )
}

/// The vault's "only verified recipients" policy.
pub(super) fn policy_of(unlocked: &Unlocked) -> SharingVerificationPolicy {
    unlocked
        .working()
        .all_vault_settings()
        .into_iter()
        .max_by_key(|s| s.updated_at)
        .map(|s| s.sharing_policy)
        .unwrap_or_default()
}

/// TR-14/16: every recipient that is not yet a member of the share with the
/// same keys needs a usable pin (verified, or chosen where the policy
/// allows). Existing members keep their access (decision of 22.10.2026).
pub(super) fn check_grants(
    env: &Environment,
    pins: &Pins,
    sensitive: bool,
    recipients: &[SharingGrantDto],
    existing: &[SharingMember],
) -> AppResult<()> {
    let policy = policy_of(&env.unlocked);
    let verified_only = if sensitive {
        policy.sensitive_verified_only
    } else {
        policy.other_verified_only
    };
    for grant in recipients {
        let member = member_of(env.pin.instance, &grant.identity, SharingRole::Reader)?;
        if member.user_id == env.pin.user {
            // The owner's own devices are admitted through enrollment.
            continue;
        }
        let kept = existing.iter().any(|m| {
            m.device_id == member.device_id
                && m.user_id == member.user_id
                && m.encryption_public_key == member.encryption_public_key
                && m.signing_public_key == member.signing_public_key
        });
        if kept {
            continue;
        }
        pins.require(
            env.pin.instance,
            &member,
            grant.verification_id.as_deref(),
            verified_only,
        )?;
    }
    Ok(())
}

/// A sharing identity DTO as a member (structural checks only).
fn member_of(
    instance: Uuid,
    identity: &SharingIdentityDto,
    role: SharingRole,
) -> AppResult<SharingMember> {
    if identity.server_instance_id != instance.to_string() {
        return Err(invalid("identity belongs to another server instance"));
    }
    let member = SharingMember {
        user_id: UserId::from_str(&identity.user_id).map_err(|_| invalid("invalid account"))?,
        device_id: DeviceId::from_str(&identity.device_id)
            .map_err(|_| invalid("invalid device"))?,
        encryption_public_key: Bytes::from(unhex(&identity.encryption_public_key)?),
        signing_public_key: Bytes::from(unhex(&identity.signing_public_key)?),
        role,
    };
    DevicePublicKeys::from_slices(
        member.encryption_public_key.as_slice(),
        member.signing_public_key.as_slice(),
    )?;
    Ok(member)
}

fn party(member: &SharingMember) -> AppResult<SharingParty> {
    Ok(SharingParty {
        user_id: member.user_id,
        device_id: member.device_id,
        keys: DevicePublicKeys::from_slices(
            member.encryption_public_key.as_slice(),
            member.signing_public_key.as_slice(),
        )?,
    })
}

/// TR-16 for accept / reconcile: the pin of the owner's keys.
pub(super) fn require_owner_pin(
    env: &Environment,
    pins: &Pins,
    owner: &SharingOwnerAnchor,
    verification_id: &str,
) -> AppResult<()> {
    // This device's own items need no pin of itself.
    if *owner == env.own_anchor() {
        return Ok(());
    }
    let instance = env.pin.instance;
    let member = SharingMember {
        user_id: owner.user_id,
        device_id: owner.device_id,
        encryption_public_key: owner.public_keys.encryption_bytes(),
        signing_public_key: owner.public_keys.signing_bytes(),
        role: SharingRole::Editor,
    };
    pins.require(instance, &member, Some(verification_id), false)
}

impl AppCore {
    /// TR-12: the user chose this colleague device (as a recipient or the
    /// owner to accept): pin its keys `chosen` on first use. Returns the pin
    /// — `keys_changed` when the device is known with other keys.
    pub async fn sharing_choose_peer(
        &self,
        identity: SharingIdentityDto,
        email: Option<String>,
    ) -> AppResult<PeerPinDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), false).await?;
        let member = member_of(env.pin.instance, &identity, SharingRole::Reader)?;
        if member.device_id == env.pin.device {
            return Err(invalid("this device cannot pin itself"));
        }
        let mut pins = Pins::load(&env).await?;
        let pin = pins
            .observe(
                env.pin.instance,
                &member,
                email.map(|e| e.trim().to_owned()),
                true,
            )
            .ok_or_else(|| AppError::internal("pin not created"))?;
        pins.save(&env).await?;
        Ok(pin_dto(&pin))
    }

    /// TR-11: the safety number with this colleague device (computed here
    /// from both parties' keys) and this device's own QR payload.
    pub async fn sharing_safety_number(
        &self,
        identity: SharingIdentityDto,
    ) -> AppResult<SafetyNumberDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), false).await?;
        let member = member_of(env.pin.instance, &identity, SharingRole::Reader)?;
        let own = party(&env.own_member())?;
        let number = sharing_safety_number(env.pin.instance, &own, &party(&member)?)
            .map_err(|_| invalid("cannot compare a device with itself"))?;
        let pins = Pins::load(&env).await?;
        let mut annotated = identity.clone();
        pins.annotate(env.pin.instance, &mut annotated);
        Ok(SafetyNumberDto {
            safety_number: number.to_string(),
            own_qr_payload: verification_qr_payload(env.pin.instance, &own),
            trust: annotated.trust,
        })
    }

    /// TR-11: verify a colleague device. `method` = `safety_number` (the
    /// number the colleague read out, typed by the user) or `qr_code` (the
    /// payload scanned or entered from the colleague's screen). A match pins
    /// exactly these keys as verified; a mismatch is journalled and changes
    /// nothing (`peer_verification_failed`).
    pub async fn sharing_verify_peer(
        &self,
        identity: SharingIdentityDto,
        method: String,
        input: String,
        email: Option<String>,
    ) -> AppResult<PeerPinDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), false).await?;
        let member = member_of(env.pin.instance, &identity, SharingRole::Reader)?;
        let peer = party(&member)?;
        let (ok, method) = match method.as_str() {
            "safety_number" => {
                let own = party(&env.own_member())?;
                let number = sharing_safety_number(env.pin.instance, &own, &peer)
                    .map_err(|_| invalid("cannot compare a device with itself"))?;
                (
                    number.matches_str(&input),
                    PeerVerificationMethod::SafetyNumber,
                )
            }
            "qr_code" => {
                let ok = match parse_verification_qr(&input) {
                    Ok((instance, scanned)) => instance == env.pin.instance && scanned == peer,
                    Err(_) => false,
                };
                (ok, PeerVerificationMethod::QrCode)
            }
            _ => return Err(invalid("unknown verification method")),
        };
        if !ok {
            tracing::warn!(
                target: "security",
                device_id = %member.device_id,
                method = method_name(method),
                "colleague key verification failed"
            );
            env.session
                .journal(serde_json::json!({
                    "event": "peer_verification_failed",
                    "device_id": member.device_id.to_string(),
                    "user_id": member.user_id.to_string(),
                    "method": method_name(method),
                }))
                .await;
            return Err(AppError::PeerVerificationFailed);
        }
        let mut pins = Pins::load(&env).await?;
        let pin = pins.verify(
            env.pin.instance,
            &member,
            method,
            email.map(|e| e.trim().to_owned()),
        );
        pins.save(&env).await?;
        tracing::info!(device_id = %member.device_id, "colleague device verified");
        Ok(pin_dto(&pin))
    }

    /// Pinned colleague devices of this profile.
    pub async fn sharing_peer_pins(&self) -> AppResult<Vec<PeerPinDto>> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), false).await?;
        let pins = Pins::load(&env).await?;
        Ok(pins
            .pins
            .iter()
            .filter(|p| p.instance_id == env.pin.instance)
            .map(pin_dto)
            .collect())
    }

    /// TR-18: the one-time banner "Verify colleagues who have access to
    /// secrets" — unverified recipients of this profile's secrets,
    /// Kubernetes contexts and database connections. Empty once dismissed.
    pub async fn sharing_verification_banner(&self) -> AppResult<Vec<PeerPinDto>> {
        let session = self.session().await?;
        let due: bool = session
            .storage()?
            .setting_get(BANNER_KEY)
            .await?
            .unwrap_or(false);
        if !due {
            return Ok(Vec::new());
        }
        let list = self.sharing_list_report(false).await?;
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), false).await?;
        let pins = Pins::load(&env).await?;
        let mut out: Vec<PeerPinDto> = Vec::new();
        for item in list.items.iter().filter(|i| i.owned) {
            let kind = match item.kind {
                SharingKindDto::Secret => SharedItemKind::Secret,
                SharingKindDto::KubeContext => SharedItemKind::KubeContext,
                SharingKindDto::DbConnection => SharedItemKind::DbConnection,
                _ => continue,
            };
            if !is_sensitive(kind, false) {
                continue;
            }
            for m in &item.members {
                let Ok(device) = DeviceId::from_str(&m.device_id) else {
                    continue;
                };
                if m.user_id == env.pin.user.to_string() {
                    continue;
                }
                if let Some(pin) = pins.find(env.pin.instance, device) {
                    if !pin.is_verified() && !out.iter().any(|p| p.device_id == m.device_id) {
                        out.push(pin_dto(pin));
                    }
                }
            }
        }
        Ok(out)
    }

    /// Hide the TR-18 banner for good.
    pub async fn sharing_dismiss_verification_banner(&self) -> AppResult<()> {
        let session = self.session().await?;
        session.storage()?.setting_set(BANNER_KEY, false).await?;
        Ok(())
    }

    /// TR-14: the vault's "only verified recipients" policy.
    pub async fn sharing_policy(&self) -> AppResult<SharingPolicyDto> {
        let (_, u) = self.unlocked().await?;
        let p = policy_of(&u);
        Ok(SharingPolicyDto {
            sensitive_verified_only: p.sensitive_verified_only,
            other_verified_only: p.other_verified_only,
        })
    }

    /// Change the policy (synced with the vault settings).
    pub async fn sharing_set_policy(
        &self,
        policy: SharingPolicyDto,
    ) -> AppResult<SharingPolicyDto> {
        let (_, u) = self.unlocked().await?;
        let mut settings = u
            .working()
            .all_vault_settings()
            .into_iter()
            .max_by_key(|s| s.updated_at)
            .ok_or_else(|| AppError::not_found("vault settings", "-"))?;
        settings.sharing_policy = SharingVerificationPolicy {
            sensitive_verified_only: policy.sensitive_verified_only,
            other_verified_only: policy.other_verified_only,
        };
        settings.updated_at = chrono::Utc::now();
        u.writer.put(VaultObject::VaultSettings(settings)).await?;
        Ok(policy)
    }

    /// After a list: TR-18 once (pin participants and owners of existing
    /// shares), afterwards only notice changed keys of pinned devices;
    /// annotate members and owners with their pin status.
    pub(super) async fn sharing_apply_pins(
        &self,
        env: &Environment,
        items: &mut [SharingItemDto],
    ) -> AppResult<()> {
        let mut pins = Pins::load(env).await?;
        let migrated: bool = env
            .wait(env.session.storage()?.setting_get(MIGRATED_KEY))
            .await?
            .unwrap_or(false);
        let own_user = env.pin.user.to_string();
        let mut banner = false;
        for item in items.iter() {
            let accepted = matches!(
                item.trust,
                SharingTrustDto::Verified | SharingTrustDto::Deleted | SharingTrustDto::Blocked
            );
            if !accepted || item.members.is_empty() {
                continue;
            }
            for m in &item.members {
                if m.user_id == own_user {
                    continue;
                }
                // Received items: only the owner was chosen by this user.
                if !item.owned && m.device_id != item.owner_device_id {
                    continue;
                }
                let Ok(member) = member_of(env.pin.instance, m, SharingRole::Reader) else {
                    continue;
                };
                let pin = pins.observe(env.pin.instance, &member, None, !migrated);
                let sensitive = matches!(
                    item.kind,
                    SharingKindDto::Secret
                        | SharingKindDto::KubeContext
                        | SharingKindDto::DbConnection
                );
                if !migrated && item.owned && sensitive && pin.is_some_and(|p| !p.is_verified()) {
                    banner = true;
                }
            }
        }
        pins.save(env).await?;
        if !migrated {
            let store = env.session.storage()?;
            env.wait(store.write(move |tx| {
                tx.setting_set(MIGRATED_KEY, &true)?;
                if banner {
                    tx.setting_set(BANNER_KEY, &true)?;
                }
                Ok::<_, cc_storage_core::StorageError>(())
            }))
            .await?;
            if banner {
                tracing::info!("existing shares: colleagues pinned unverified; banner due");
            }
        }
        for item in items.iter_mut() {
            for m in item.members.iter_mut() {
                pins.annotate(env.pin.instance, m);
            }
            for m in item.member_roles.iter_mut() {
                pins.annotate(env.pin.instance, &mut m.identity);
            }
            if !item.owned {
                item.owner_trust = item
                    .members
                    .iter()
                    .find(|m| m.device_id == item.owner_device_id)
                    .and_then(|m| m.trust.clone());
            }
        }
        Ok(())
    }
}
