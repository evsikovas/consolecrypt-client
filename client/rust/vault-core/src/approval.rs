//! Approving a new device from a trusted one (ADR-0004 §Device approval).
//!
//! Two explicit steps, so the mandatory verification-code comparison cannot
//! be skipped by accident:
//!
//! 1. [`PendingApproval::new`] validates the server's trust request and
//!    derives the [`VerificationCode`] from the keys *the server reports*.
//!    The UI shows it; the user compares it with the code on the new device.
//! 2. Only after the user confirmed the match, the UI calls
//!    [`PendingApproval::confirm_verification_code`] and then
//!    [`ConfirmedApproval::build_request`], which encrypts the VRK of every
//!    approved vault to the new device and signs the approval.

use crate::error::{Result, VaultError};
use crate::identity::DeviceIdentity;
use crate::unlock::{envelope_for, EnvelopeSource};
use crate::unlocked::UnlockedVault;
use cc_crypto_core::{
    device_envelope_sha256, open_device_envelope, pairing_code_v2, verify_device_approval_v2,
    ApprovedVaultKey, CryptoError, DeviceApproval, DeviceApprovalV2, DevicePublicKeys,
    VerificationCode,
};
use cc_protocol::devices::{
    ApprovalV2, ApprovalVaultCommitment, ApproveDeviceRequest, DeviceApprovalRecord,
    DeviceRequestStatus, DeviceStatus, DeviceTrustRequest, VaultEnvelope,
};
use cc_protocol::{Bytes, DeviceId, DeviceRequestId, Timestamp, VaultId};
use std::collections::HashSet;

/// A validated pending trust request, before the user compared codes.
#[derive(Debug, Clone)]
pub struct PendingApproval {
    request_id: DeviceRequestId,
    new_device_id: DeviceId,
    new_device_name: String,
    new_device_keys: DevicePublicKeys,
    requested_vaults: Vec<VaultId>,
    expires_at: Timestamp,
    code: VerificationCode,
}

impl PendingApproval {
    /// Validate `request` (pending, unexpired at `now`, device active, not
    /// the approver itself, well-formed keys) and compute its verification
    /// code.
    pub fn new(
        request: &DeviceTrustRequest,
        approver: &DeviceIdentity,
        now: Timestamp,
    ) -> Result<Self> {
        if request.status != DeviceRequestStatus::Pending {
            return Err(VaultError::Approval("request is not pending"));
        }
        if request.expires_at <= now {
            return Err(VaultError::Approval("request has expired"));
        }
        if request.device.status != DeviceStatus::Active {
            return Err(VaultError::Approval("device is revoked"));
        }
        if request.device.device_id == approver.device_id() {
            return Err(VaultError::Approval("a device cannot approve itself"));
        }
        let keys = DevicePublicKeys::from_device_info(&request.device)?;
        Ok(Self {
            request_id: request.request_id,
            new_device_id: request.device.device_id,
            new_device_name: request.device.name.clone(),
            new_device_keys: keys,
            requested_vaults: request.vault_ids.clone(),
            expires_at: request.expires_at,
            code: keys.verification_code(request.device.device_id),
        })
    }

    /// The code to show next to "Does this match the code on
    /// `<new_device_name>`?".
    pub fn verification_code(&self) -> VerificationCode {
        self.code
    }

    /// The device asking to be trusted.
    pub fn new_device_id(&self) -> DeviceId {
        self.new_device_id
    }

    /// Its user-visible name (as reported by the server).
    pub fn new_device_name(&self) -> &str {
        &self.new_device_name
    }

    /// Vaults requested (empty = every vault of the account).
    pub fn requested_vault_ids(&self) -> &[VaultId] {
        &self.requested_vaults
    }

    /// The request id.
    pub fn request_id(&self) -> DeviceRequestId {
        self.request_id
    }

    /// Mutual pairing code v2 (protocol 1.11) between the new device and
    /// `approver`: shown on both screens instead of the v1 code when the
    /// server supports approval intents.
    pub fn pairing_code_v2(&self, approver: &DeviceIdentity) -> VerificationCode {
        pairing_code_v2(
            self.request_id,
            self.new_device_id,
            &self.new_device_keys,
            approver.device_id(),
            &approver.public_keys(),
        )
    }

    /// Record that the user confirmed both codes match. Call this only from
    /// the UI action where the user explicitly confirmed the comparison.
    pub fn confirm_verification_code(self) -> ConfirmedApproval {
        ConfirmedApproval { pending: self }
    }
}

/// A trust request whose verification code the user confirmed.
#[derive(Debug, Clone)]
pub struct ConfirmedApproval {
    pending: PendingApproval,
}

impl ConfirmedApproval {
    /// Build `POST /v1/devices/{new_device_id}/approve` for exactly `vaults`
    /// (each must be unlocked on this device and, if the request names
    /// vaults, be one of them). `now` becomes `issued_at`.
    pub fn build_request(
        &self,
        approver: &DeviceIdentity,
        vaults: &[&UnlockedVault],
        now: Timestamp,
    ) -> Result<ApproveDeviceRequest> {
        let p = &self.pending;
        if p.expires_at <= now {
            return Err(VaultError::Approval("request has expired"));
        }
        if vaults.is_empty() {
            return Err(VaultError::Approval("no vaults to approve"));
        }
        let mut seen = HashSet::new();
        for v in vaults {
            if !seen.insert(v.vault_id()) {
                return Err(VaultError::Approval("duplicate vault"));
            }
            if !p.requested_vaults.is_empty() && !p.requested_vaults.contains(&v.vault_id()) {
                return Err(VaultError::Approval("vault was not requested"));
            }
        }
        let envelopes = vaults
            .iter()
            .map(|v| {
                Ok(VaultEnvelope {
                    vault_id: v.vault_id(),
                    envelope: v.device_envelope(p.new_device_id, &p.new_device_keys.encryption)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let vault_ids: Vec<VaultId> = vaults.iter().map(|v| v.vault_id()).collect();
        let issued_at = now.timestamp();
        let signature = approver.sign_approval(&DeviceApproval {
            request_id: p.request_id,
            approver_device_id: approver.device_id(),
            new_device_id: p.new_device_id,
            new_device_keys: &p.new_device_keys,
            issued_at,
            vault_ids: &vault_ids,
        });
        Ok(ApproveDeviceRequest {
            request_id: p.request_id,
            issued_at,
            signature: Bytes::new(signature.to_vec()),
            envelopes,
            v2: None,
        })
    }

    /// [`ConfirmedApproval::build_request`] plus the v2 approval (protocol
    /// 1.11): per vault the key commitment and the hash of the envelope
    /// sealed for the new device, signed together with both devices' keys.
    /// Use when the server advertises `vrk_commitment` and the user compared
    /// the pairing code v2.
    pub fn build_request_v2(
        &self,
        approver: &DeviceIdentity,
        vaults: &[&UnlockedVault],
        now: Timestamp,
    ) -> Result<ApproveDeviceRequest> {
        let mut req = self.build_request(approver, vaults, now)?;
        let p = &self.pending;
        let mut keys = Vec::with_capacity(req.envelopes.len());
        for env in &req.envelopes {
            let vault = vaults
                .iter()
                .find(|v| v.vault_id() == env.vault_id)
                .ok_or(VaultError::Approval("vault was not unlocked"))?;
            keys.push(ApprovedVaultKey {
                vault_id: env.vault_id,
                commitment: vault.key_commitment(),
                envelope_sha256: device_envelope_sha256(&env.envelope)?,
            });
        }
        let approver_keys = approver.public_keys();
        let signature_v2 = approver.sign_approval_v2(&DeviceApprovalV2 {
            request_id: p.request_id,
            approver_device_id: approver.device_id(),
            approver_keys: &approver_keys,
            new_device_id: p.new_device_id,
            new_device_keys: &p.new_device_keys,
            issued_at: req.issued_at,
            vaults: &keys,
        });
        req.v2 = Some(ApprovalV2 {
            vault_commitments: keys
                .iter()
                .map(|k| ApprovalVaultCommitment {
                    vault_id: k.vault_id,
                    commitment: Bytes::new(k.commitment.to_vec()),
                    envelope_sha256: Bytes::new(k.envelope_sha256.to_vec()),
                })
                .collect(),
            signature_v2: Bytes::new(signature_v2.to_vec()),
        });
        Ok(req)
    }

    /// The verification code the user confirmed.
    pub fn verification_code(&self) -> VerificationCode {
        self.pending.code
    }
}

/// The approver a new device pinned after the user confirmed the pairing
/// code v2 on both screens (protocol 1.11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinnedApprover {
    pub request_id: DeviceRequestId,
    pub device_id: DeviceId,
    pub keys: DevicePublicKeys,
}

fn mismatch(reason: &'static str) -> VaultError {
    VaultError::KeyMismatch {
        reason,
        received: None,
    }
}

/// New device, approval v2 (protocol 1.11): verify `record` with the
/// approver key **pinned on this device**, check that `vault_id` is in the
/// signed list and the envelope is exactly the signed one, open it and check
/// `HKDF(VRK)` against the signed commitment. Any difference →
/// [`VaultError::KeyMismatch`]; nothing is returned that could be used.
///
/// Returns the vault and the commitment to pin.
pub fn unlock_approved_device(
    vault_id: VaultId,
    device_envelope: &(impl EnvelopeSource + ?Sized),
    record: &DeviceApprovalRecord,
    approver: &PinnedApprover,
    identity: &DeviceIdentity,
) -> Result<(UnlockedVault, [u8; 32])> {
    if record.request_id != approver.request_id || record.approver_device_id != approver.device_id {
        return Err(mismatch("approval_from_another_device"));
    }
    let mut keys = Vec::with_capacity(record.vault_commitments.len());
    for c in &record.vault_commitments {
        keys.push(ApprovedVaultKey {
            vault_id: c.vault_id,
            commitment: c
                .commitment
                .to_array()
                .ok_or_else(|| mismatch("approval_malformed"))?,
            envelope_sha256: c
                .envelope_sha256
                .to_array()
                .ok_or_else(|| mismatch("approval_malformed"))?,
        });
    }
    let own_keys = identity.public_keys();
    verify_device_approval_v2(
        &approver.keys.signing,
        &DeviceApprovalV2 {
            request_id: record.request_id,
            approver_device_id: approver.device_id,
            approver_keys: &approver.keys,
            new_device_id: identity.device_id(),
            new_device_keys: &own_keys,
            issued_at: record.issued_at,
            vaults: &keys,
        },
        record.signature_v2.as_slice(),
    )
    .map_err(|_| mismatch("approval_signature"))?;
    let signed = keys
        .iter()
        .find(|k| k.vault_id == vault_id)
        .ok_or_else(|| mismatch("vault_not_signed"))?;
    let env = envelope_for(device_envelope, vault_id)?;
    if device_envelope_sha256(&env).map_err(|_| mismatch("envelope_hash"))?
        != signed.envelope_sha256
    {
        return Err(mismatch("envelope_hash"));
    }
    let vrk = open_device_envelope(&env, vault_id, identity.device_id(), identity.secret_keys())
        .map_err(|e| match e {
            CryptoError::Decrypt | CryptoError::WrongRecipientDevice => {
                VaultError::DeviceNotAuthorized
            }
            other => VaultError::Crypto(other),
        })?;
    let vault = UnlockedVault::from_vrk(vault_id, vrk);
    let received = vault.key_commitment();
    if received != signed.commitment {
        return Err(VaultError::KeyMismatch {
            reason: "commitment",
            received: Some(received),
        });
    }
    Ok((vault, received))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_crypto_core::Argon2Params;
    use cc_protocol::devices::DeviceInfo;
    use cc_protocol::version::Platform;
    use secrecy::SecretString;

    struct Fixture {
        approver: DeviceIdentity,
        new: DeviceIdentity,
        vault: UnlockedVault,
        request: DeviceTrustRequest,
    }

    fn fixture() -> Fixture {
        let approver = DeviceIdentity::generate().unwrap();
        let new = DeviceIdentity::generate().unwrap();
        let created = crate::create_vault(
            &approver,
            &SecretString::from("correct horse battery"),
            Argon2Params::for_tests(),
            None,
            chrono::Utc::now(),
        )
        .unwrap();
        let keys = new.public_keys();
        let now = chrono::Utc::now();
        let request = DeviceTrustRequest {
            request_id: DeviceRequestId::new(),
            device: DeviceInfo {
                device_id: new.device_id(),
                name: "New".into(),
                platform: Platform::Linux,
                encryption_public_key: keys.encryption_bytes(),
                signing_public_key: keys.signing_bytes(),
                status: DeviceStatus::Active,
                trusted_vaults: vec![],
                created_at: now,
                last_seen_at: None,
                revoked_at: None,
                is_current: false,
                trusted: false,
                quarantine_until: None,
            },
            vault_ids: vec![created.unlocked.vault_id()],
            status: DeviceRequestStatus::Pending,
            created_at: now,
            expires_at: now + chrono::Duration::hours(1),
            approved_by_device_id: None,
            approver: None,
        };
        Fixture {
            approver,
            new,
            vault: created.unlocked,
            request,
        }
    }

    fn approve(f: &Fixture) -> (ApproveDeviceRequest, DeviceApprovalRecord, PinnedApprover) {
        let pending = PendingApproval::new(&f.request, &f.approver, chrono::Utc::now()).unwrap();
        let req = pending
            .confirm_verification_code()
            .build_request_v2(&f.approver, &[&f.vault], chrono::Utc::now())
            .unwrap();
        let v2 = req.v2.clone().unwrap();
        let record = DeviceApprovalRecord {
            request_id: req.request_id,
            approver_device_id: f.approver.device_id(),
            issued_at: req.issued_at,
            vault_commitments: v2.vault_commitments,
            signature_v2: v2.signature_v2,
        };
        let pinned = PinnedApprover {
            request_id: req.request_id,
            device_id: f.approver.device_id(),
            keys: f.approver.public_keys(),
        };
        (req, record, pinned)
    }

    #[test]
    fn pairing_codes_match_on_both_devices() {
        let f = fixture();
        let pending = PendingApproval::new(&f.request, &f.approver, chrono::Utc::now()).unwrap();
        let on_approver = pending.pairing_code_v2(&f.approver);
        let on_new = pairing_code_v2(
            f.request.request_id,
            f.new.device_id(),
            &f.new.public_keys(),
            f.approver.device_id(),
            &f.approver.public_keys(),
        );
        assert_eq!(on_approver, on_new);
    }

    #[test]
    fn genuine_v2_approval_opens_and_pins_the_commitment() {
        let f = fixture();
        let (req, record, pinned) = approve(&f);
        let env = &req.envelopes[0].envelope;
        let (vault, c) =
            unlock_approved_device(f.vault.vault_id(), env, &record, &pinned, &f.new).unwrap();
        assert_eq!(c, f.vault.key_commitment());
        assert_eq!(vault.vault_access_key(), f.vault.vault_access_key());
    }

    #[test]
    fn substituted_envelope_vault_or_signature_is_refused() {
        let f = fixture();
        let (req, record, pinned) = approve(&f);
        let vault_id = f.vault.vault_id();

        // A server seals its own VRK for the new device: hash differs.
        let other = fixture();
        let forged = other
            .vault
            .device_envelope(f.new.device_id(), &f.new.public_keys().encryption)
            .unwrap();
        let e = unlock_approved_device(vault_id, &forged, &record, &pinned, &f.new).unwrap_err();
        assert!(matches!(e, VaultError::KeyMismatch { .. }), "{e:?}");

        // A vault id outside the signed list.
        let e = unlock_approved_device(
            VaultId::new(),
            &req.envelopes[0].envelope,
            &record,
            &pinned,
            &f.new,
        )
        .unwrap_err();
        assert_eq!(
            e,
            VaultError::KeyMismatch {
                reason: "vault_not_signed",
                received: None
            }
        );

        // A record "signed" for a forged commitment does not verify.
        let mut bad = record.clone();
        bad.vault_commitments[0].commitment = Bytes::new(vec![0; 32]);
        let e = unlock_approved_device(vault_id, &req.envelopes[0].envelope, &bad, &pinned, &f.new)
            .unwrap_err();
        assert_eq!(
            e,
            VaultError::KeyMismatch {
                reason: "approval_signature",
                received: None
            }
        );

        // Signed by a key the new device did not pin.
        let impostor = PinnedApprover {
            keys: DeviceIdentity::generate().unwrap().public_keys(),
            ..pinned
        };
        assert!(matches!(
            unlock_approved_device(
                vault_id,
                &req.envelopes[0].envelope,
                &record,
                &impostor,
                &f.new
            ),
            Err(VaultError::KeyMismatch { .. })
        ));
    }
}
