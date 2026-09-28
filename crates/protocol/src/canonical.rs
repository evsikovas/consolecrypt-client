//! Canonical byte encodings that more than one party must reproduce exactly:
//! signed messages (client signs, server verifies), AEAD associated data and
//! KDF labels (client ↔ client, across platforms). No cryptography happens
//! here — only deterministic byte layouts. Formats are specified in ADR-0002
//! and ADR-0004; changing any of them is a breaking protocol change.

use crate::ids::{DeviceId, DeviceRequestId, ObjectId, VaultId};

/// Domain-separation labels. Every label is unique and versioned.
pub mod labels {
    /// Signed device approval message prefix.
    pub const DEVICE_APPROVAL: &[u8] = b"consolecrypt/v1/device-approval\0";
    /// Input prefix for the device verification code (safety number).
    pub const DEVICE_FINGERPRINT: &[u8] = b"consolecrypt/v1/device-fingerprint\0";
    /// Signed proof that a login comes from the holder of a device's
    /// Ed25519 key (protocol 1.4, ADR-0006).
    pub const DEVICE_LOGIN: &[u8] = b"consolecrypt/v1/device-login\0";
    /// Per-request proof of possession of the device key (protocol 1.5).
    pub const REQUEST_PROOF: &[u8] = b"consolecrypt/v1/request-proof\0";

    /// HKDF `info` for KEKs derived from the VRK (salt = vault_id bytes).
    pub const KEK_INVENTORY: &[u8] = b"consolecrypt/v1/kek/inventory";
    pub const KEK_SECRETS: &[u8] = b"consolecrypt/v1/kek/secrets";
    pub const KEK_SNIPPETS: &[u8] = b"consolecrypt/v1/kek/snippets";
    pub const KEK_HISTORY: &[u8] = b"consolecrypt/v1/kek/history";
    pub const KEK_SETTINGS: &[u8] = b"consolecrypt/v1/kek/settings";
    /// HKDF `info` for the vault access key (server-verifiable proof of VRK).
    pub const VAULT_ACCESS_KEY: &[u8] = b"consolecrypt/v1/vault-access-key";
    /// HKDF `info` for the recovery-envelope key (ikm = Recovery Key).
    pub const RECOVERY_KEK: &[u8] = b"consolecrypt/v1/recovery-kek";
    /// HKDF `info` prefix for device-envelope keys (ikm = X25519 shared secret).
    pub const DEVICE_ENVELOPE: &[u8] = b"consolecrypt/v1/device-envelope";

    /// AEAD associated-data prefixes.
    pub const AAD_ENVELOPE: &[u8] = b"consolecrypt/v1/aad/envelope\0";
    pub const AAD_OBJECT: &[u8] = b"consolecrypt/v1/aad/object\0";
    pub const AAD_WRAPPED_DEK: &[u8] = b"consolecrypt/v1/aad/wrapped-dek\0";

    /// HKDF `info` for the `.ccbackup` manifest MAC key (ikm = VRK, salt = vault_id).
    pub const BACKUP_MANIFEST_KEY: &[u8] = b"consolecrypt/v1/backup-manifest-key";
    /// Prefix of the canonical `.ccbackup` manifest that is MACed (ADR-0102/ADR-0106).
    pub const BACKUP_MANIFEST: &[u8] = b"consolecrypt/v1/backup-manifest\0";
}

/// Message an approving device signs with its Ed25519 key.
///
/// ```text
/// DEVICE_APPROVAL
/// || request_id (16) || approver_device_id (16) || new_device_id (16)
/// || new_encryption_public_key (32) || new_signing_public_key (32)
/// || issued_at (i64 big-endian, unix seconds)
/// || vault_count (u32 big-endian) || vault_id (16) * vault_count   // sorted ascending
/// ```
pub fn device_approval_message(
    request_id: DeviceRequestId,
    approver_device_id: DeviceId,
    new_device_id: DeviceId,
    new_encryption_public_key: &[u8; 32],
    new_signing_public_key: &[u8; 32],
    issued_at: i64,
    vault_ids: &[VaultId],
) -> Vec<u8> {
    let mut vaults: Vec<VaultId> = vault_ids.to_vec();
    vaults.sort();
    vaults.dedup();
    let mut m = Vec::with_capacity(labels::DEVICE_APPROVAL.len() + 124 + 16 * vaults.len());
    m.extend_from_slice(labels::DEVICE_APPROVAL);
    m.extend_from_slice(request_id.as_bytes());
    m.extend_from_slice(approver_device_id.as_bytes());
    m.extend_from_slice(new_device_id.as_bytes());
    m.extend_from_slice(new_encryption_public_key);
    m.extend_from_slice(new_signing_public_key);
    m.extend_from_slice(&issued_at.to_be_bytes());
    m.extend_from_slice(&(vaults.len() as u32).to_be_bytes());
    for v in &vaults {
        m.extend_from_slice(v.as_bytes());
    }
    m
}

/// Message a device signs with its Ed25519 key to prove possession when it
/// logs in (`devices::DeviceProof`, protocol 1.4, ADR-0006).
///
/// ```text
/// DEVICE_LOGIN || device_id (16) || issued_at (i64 big-endian, unix seconds) || nonce (32)
/// ```
///
/// `nonce` is 32 fresh random bytes; the server accepts each (device, nonce)
/// once and `issued_at` only within `limits::MAX_SIGNATURE_SKEW_SECONDS`.
pub fn device_login_message(device_id: DeviceId, issued_at: i64, nonce: &[u8; 32]) -> Vec<u8> {
    let mut m = Vec::with_capacity(labels::DEVICE_LOGIN.len() + 56);
    m.extend_from_slice(labels::DEVICE_LOGIN);
    m.extend_from_slice(device_id.as_bytes());
    m.extend_from_slice(&issued_at.to_be_bytes());
    m.extend_from_slice(nonce);
    m
}

/// Message a device signs for every authenticated request (protocol 1.5,
/// header [`crate::version::HEADER_DEVICE_PROOF`]), binding the request to
/// the device key so a stolen bearer token is useless without it.
///
/// ```text
/// REQUEST_PROOF
/// || device_id (16)
/// || u32 BE len(method) || method            (as sent, e.g. "GET")
/// || u32 BE len(path_and_query) || path_and_query   (request-target as sent,
///                                               e.g. "/v1/sync/changes?vault_id=…&after=0")
/// || body_sha256 (32)                          (SHA-256 of the exact body bytes; of "" if none)
/// || issued_at (i64 BE, unix seconds)
/// || nonce (32)
/// ```
pub fn request_proof_message(
    device_id: DeviceId,
    method: &str,
    path_and_query: &str,
    body_sha256: &[u8; 32],
    issued_at: i64,
    nonce: &[u8; 32],
) -> Vec<u8> {
    let mut m = Vec::with_capacity(
        labels::REQUEST_PROOF.len() + 16 + 8 + method.len() + path_and_query.len() + 72,
    );
    m.extend_from_slice(labels::REQUEST_PROOF);
    m.extend_from_slice(device_id.as_bytes());
    m.extend_from_slice(&(method.len() as u32).to_be_bytes());
    m.extend_from_slice(method.as_bytes());
    m.extend_from_slice(&(path_and_query.len() as u32).to_be_bytes());
    m.extend_from_slice(path_and_query.as_bytes());
    m.extend_from_slice(body_sha256);
    m.extend_from_slice(&issued_at.to_be_bytes());
    m.extend_from_slice(nonce);
    m
}

/// Input to the device verification code. The client computes
/// `h = SHA-256(device_fingerprint_input(..))` and renders 6 groups of 5
/// decimal digits: group `i` = big-endian u40 of `h[5i..5i+5]` mod 100000,
/// zero-padded (ADR-0004). Both the new and the approving device show the
/// code; the user must confirm they match before approval is signed.
///
/// ```text
/// DEVICE_FINGERPRINT || device_id (16) || encryption_public_key (32) || signing_public_key (32)
/// ```
pub fn device_fingerprint_input(
    device_id: DeviceId,
    encryption_public_key: &[u8; 32],
    signing_public_key: &[u8; 32],
) -> Vec<u8> {
    let mut m = Vec::with_capacity(labels::DEVICE_FINGERPRINT.len() + 80);
    m.extend_from_slice(labels::DEVICE_FINGERPRINT);
    m.extend_from_slice(device_id.as_bytes());
    m.extend_from_slice(encryption_public_key);
    m.extend_from_slice(signing_public_key);
    m
}

/// AEAD associated data for a VRK envelope.
///
/// ```text
/// AAD_ENVELOPE || vault_id (16) || recipient_type (u8) || recipient_id (16, zeros if none)
/// ```
///
/// `recipient_type` codes: password = 1, recovery = 2, device = 3, user = 4.
pub fn envelope_aad(
    vault_id: VaultId,
    recipient_type_code: u8,
    recipient_id: Option<&[u8; 16]>,
) -> Vec<u8> {
    let mut m = Vec::with_capacity(labels::AAD_ENVELOPE.len() + 33);
    m.extend_from_slice(labels::AAD_ENVELOPE);
    m.extend_from_slice(vault_id.as_bytes());
    m.push(recipient_type_code);
    m.extend_from_slice(recipient_id.unwrap_or(&[0u8; 16]));
    m
}

/// AEAD associated data for an object payload. Binding `revision` prevents a
/// hostile server from replaying an older ciphertext under a newer revision.
///
/// ```text
/// AAD_OBJECT || vault_id (16) || object_id (16) || revision (i64 BE) || format (u16 BE)
/// ```
pub fn object_aad(vault_id: VaultId, object_id: ObjectId, revision: i64, format: u16) -> Vec<u8> {
    let mut m = Vec::with_capacity(labels::AAD_OBJECT.len() + 42);
    m.extend_from_slice(labels::AAD_OBJECT);
    m.extend_from_slice(vault_id.as_bytes());
    m.extend_from_slice(object_id.as_bytes());
    m.extend_from_slice(&revision.to_be_bytes());
    m.extend_from_slice(&format.to_be_bytes());
    m
}

/// AEAD associated data for a wrapped DEK.
///
/// ```text
/// AAD_WRAPPED_DEK || vault_id (16) || object_id (16) || revision (i64 BE)
/// ```
pub fn wrapped_dek_aad(vault_id: VaultId, object_id: ObjectId, revision: i64) -> Vec<u8> {
    let mut m = Vec::with_capacity(labels::AAD_WRAPPED_DEK.len() + 40);
    m.extend_from_slice(labels::AAD_WRAPPED_DEK);
    m.extend_from_slice(vault_id.as_bytes());
    m.extend_from_slice(object_id.as_bytes());
    m.extend_from_slice(&revision.to_be_bytes());
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn approval_message_is_order_independent_over_vaults() {
        let r = DeviceRequestId::new();
        let a = DeviceId::new();
        let n = DeviceId::new();
        let v1 = VaultId::new();
        let v2 = VaultId::new();
        let m1 = device_approval_message(r, a, n, &[1; 32], &[2; 32], 1_700_000_000, &[v1, v2]);
        let m2 = device_approval_message(r, a, n, &[1; 32], &[2; 32], 1_700_000_000, &[v2, v1, v2]);
        assert_eq!(m1, m2);
        assert_eq!(
            m1.len(),
            labels::DEVICE_APPROVAL.len() + 16 * 3 + 64 + 8 + 4 + 32
        );
    }

    /// Fixed vector: other language implementations (Dart, Swift, Kotlin)
    /// must produce exactly these bytes.
    #[test]
    fn object_aad_vector() {
        let vault = VaultId::from_str("00000000-0000-7000-8000-000000000001").unwrap();
        let obj = ObjectId::from_str("00000000-0000-7000-8000-000000000002").unwrap();
        let aad = object_aad(vault, obj, 3, 1);
        let tail = &aad[labels::AAD_OBJECT.len()..];
        assert_eq!(
            tail,
            &[
                0, 0, 0, 0, 0, 0, 0x70, 0, 0x80, 0, 0, 0, 0, 0, 0, 1, //
                0, 0, 0, 0, 0, 0, 0x70, 0, 0x80, 0, 0, 0, 0, 0, 0, 2, //
                0, 0, 0, 0, 0, 0, 0, 3, //
                0, 1
            ][..]
        );
    }

    /// Fixed vector for other implementations.
    #[test]
    fn device_login_message_vector() {
        let d = DeviceId::from_str("00000000-0000-7000-8000-000000000003").unwrap();
        let m = device_login_message(d, 1_700_000_000, &[0xab; 32]);
        let tail = &m[labels::DEVICE_LOGIN.len()..];
        assert_eq!(&tail[..16], d.as_bytes());
        assert_eq!(&tail[16..24], &[0, 0, 0, 0, 0x65, 0x53, 0xf1, 0x00]);
        assert_eq!(&tail[24..], &[0xab; 32]);
        assert_eq!(m.len(), labels::DEVICE_LOGIN.len() + 56);
    }

    /// Fixed vector for other implementations.
    #[test]
    fn request_proof_message_vector() {
        let d = DeviceId::from_str("00000000-0000-7000-8000-000000000004").unwrap();
        let m = request_proof_message(
            d,
            "GET",
            "/v1/auth/me",
            &[0x11; 32],
            1_700_000_000,
            &[0x22; 32],
        );
        let mut expected = labels::REQUEST_PROOF.to_vec();
        expected.extend_from_slice(d.as_bytes());
        expected.extend_from_slice(&[0, 0, 0, 3]);
        expected.extend_from_slice(b"GET");
        expected.extend_from_slice(&[0, 0, 0, 11]);
        expected.extend_from_slice(b"/v1/auth/me");
        expected.extend_from_slice(&[0x11; 32]);
        expected.extend_from_slice(&[0, 0, 0, 0, 0x65, 0x53, 0xf1, 0x00]);
        expected.extend_from_slice(&[0x22; 32]);
        assert_eq!(m, expected);
        // Length prefixes keep method/path boundaries unambiguous.
        assert_ne!(
            request_proof_message(d, "GE", "T/x", &[0; 32], 0, &[0; 32]),
            request_proof_message(d, "GET", "/x", &[0; 32], 0, &[0; 32])
        );
    }

    #[test]
    fn labels_are_unique() {
        let all = [
            labels::DEVICE_APPROVAL,
            labels::DEVICE_FINGERPRINT,
            labels::KEK_INVENTORY,
            labels::KEK_SECRETS,
            labels::KEK_SNIPPETS,
            labels::KEK_HISTORY,
            labels::KEK_SETTINGS,
            labels::VAULT_ACCESS_KEY,
            labels::RECOVERY_KEK,
            labels::DEVICE_ENVELOPE,
            labels::AAD_ENVELOPE,
            labels::AAD_OBJECT,
            labels::AAD_WRAPPED_DEK,
            labels::BACKUP_MANIFEST_KEY,
            labels::BACKUP_MANIFEST,
            labels::DEVICE_LOGIN,
            labels::REQUEST_PROOF,
        ];
        let set: std::collections::HashSet<_> = all.iter().collect();
        assert_eq!(set.len(), all.len());
    }
}
