//! Fixed-input test vectors for every ADR-0002 / ADR-0004 construction
//! (`tests/vectors/*.json`). Other implementations (Dart, Swift, Kotlin, …)
//! must reproduce every `output` from its `input`.
//!
//! * `cargo test -p cc-crypto-core vectors` recomputes every case from the
//!   file's inputs and compares, and checks the file matches the generator.
//! * `CC_UPDATE_VECTORS=1 cargo test -p cc-crypto-core vectors` rewrites the
//!   files (only after an intentional, versioned format change — ADR-0002).
//!
//! All key material here is fixed, public test data (byte sequences), never
//! real secrets.

use crate::device::{x25519, x25519_public, x25519_secret_from, DeviceApproval, DeviceSecretKeys};
use crate::envelope::{
    device_envelope_key, open_device_envelope, open_password_envelope, open_recovery_envelope,
    recovery_kek, seal_device_envelope_with, seal_password_envelope_with,
    seal_recovery_envelope_with, RECIPIENT_CODE_DEVICE, RECIPIENT_CODE_PASSWORD,
    RECIPIENT_CODE_RECOVERY,
};
use crate::kdf::{derive_password_key, normalize_passphrase, Argon2Params};
use crate::keys::{vault_access_key_verifier, Dek, KeyHierarchy, Vrk};
use crate::object::{decrypt_object, encrypt_padded_with, pad_serialized, unpad};
use crate::recovery::RecoveryKey;
use crate::{verification_code, DevicePublicKeys};
use cc_models::KekClass;
use cc_protocol::canonical::{
    device_fingerprint_input, envelope_aad, labels, object_aad, wrapped_dek_aad,
};
use cc_protocol::sync::OBJECT_FORMAT_V1;
use cc_protocol::{DeviceId, DeviceRequestId, ObjectId, VaultId};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::str::FromStr;

const VAULT_ID: &str = "00000000-0000-7000-8000-000000000001";

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn seq<const N: usize>(start: u8) -> [u8; N] {
    let mut a = [0u8; N];
    for (i, b) in a.iter_mut().enumerate() {
        *b = start.wrapping_add(i as u8);
    }
    a
}

fn h(b: &[u8]) -> String {
    hex::encode(b)
}

fn arr<const N: usize>(v: &Value) -> [u8; N] {
    let bytes = hex::decode(v.as_str().expect("hex string")).expect("valid hex");
    bytes.try_into().expect("hex has expected length")
}

fn bytes(v: &Value) -> Vec<u8> {
    hex::decode(v.as_str().expect("hex string")).expect("valid hex")
}

fn id<T: FromStr>(v: &Value) -> T
where
    T::Err: std::fmt::Debug,
{
    T::from_str(v.as_str().expect("id string")).expect("valid id")
}

fn sha(b: &[u8]) -> String {
    h(&Sha256::digest(b))
}

fn utf8(b: &[u8]) -> String {
    String::from_utf8(b.to_vec()).expect("labels are UTF-8")
}

fn class_name(c: KekClass) -> &'static str {
    match c {
        KekClass::Inventory => "inventory",
        KekClass::Secrets => "secrets",
        KekClass::Snippets => "snippets",
        KekClass::History => "history",
        KekClass::Settings => "settings",
    }
}

type Compute = fn(&Value) -> Value;

/// Build the file from `cases`, then either rewrite it (CC_UPDATE_VECTORS)
/// or verify it: every stored output must be reproducible from its stored
/// input, and the file must equal what the generator produces now.
fn run(file: &str, description: &str, cases: Vec<(&str, Value)>, compute: Compute) {
    let generated = json!({
        "description": description,
        "cases": cases
            .into_iter()
            .map(|(name, input)| {
                let output = compute(&input);
                json!({ "name": name, "input": input, "output": output })
            })
            .collect::<Vec<_>>(),
    });
    let path = format!("{}/tests/vectors/{file}", env!("CARGO_MANIFEST_DIR"));
    if std::env::var_os("CC_UPDATE_VECTORS").is_some() {
        let mut text = serde_json::to_string_pretty(&generated).expect("serialize vectors");
        text.push('\n');
        std::fs::write(&path, text).expect("write vector file");
    }
    let stored: Value = serde_json::from_str(
        &std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("{path} missing — run with CC_UPDATE_VECTORS=1")),
    )
    .expect("vector file is JSON");
    for case in stored["cases"].as_array().expect("cases array") {
        assert_eq!(
            compute(&case["input"]),
            case["output"],
            "{file}: case {} does not reproduce",
            case["name"]
        );
    }
    assert_eq!(
        stored, generated,
        "{file} is stale (inputs/outputs changed)"
    );
}

// ---------------------------------------------------------------------------
// key hierarchy
// ---------------------------------------------------------------------------

fn compute_hierarchy(input: &Value) -> Value {
    let vault_id: VaultId = id(&input["vault_id"]);
    let vrk = Vrk::from_bytes(&arr(&input["vrk"]));
    let keys = KeyHierarchy::derive(&vrk, vault_id);
    let mut keks = serde_json::Map::new();
    for class in KekClass::ALL {
        keks.insert(
            class_name(class).into(),
            json!({
                "hkdf_info_utf8": utf8(class.hkdf_label()),
                "okm": h(keys.kek(class).expose()),
            }),
        );
    }
    let vak = keys.vault_access_key().expose();
    json!({
        "hkdf_salt": h(vault_id.as_bytes()),
        "kek": keks,
        "vault_access_key": {
            "hkdf_info_utf8": utf8(labels::VAULT_ACCESS_KEY),
            "okm": h(vak),
        },
        "vault_access_key_sha256": h(&vault_access_key_verifier(vak)),
    })
}

#[test]
fn key_hierarchy_vectors() {
    run(
        "key_hierarchy.json",
        "KEKs and VAK: HKDF-SHA256(ikm = VRK, salt = vault_id (16 raw bytes), info = label), 32-byte output; verifier = SHA-256(VAK)",
        vec![
            ("sequential", json!({ "vault_id": VAULT_ID, "vrk": h(&seq::<32>(0x00)) })),
            (
                "other_vault",
                json!({ "vault_id": "0192f4a0-1b2c-7d3e-8f40-5a6b7c8d9e0f", "vrk": h(&seq::<32>(0x00)) }),
            ),
        ],
        compute_hierarchy,
    );
}

// ---------------------------------------------------------------------------
// envelopes
// ---------------------------------------------------------------------------

fn compute_password(input: &Value) -> Value {
    let vault_id: VaultId = id(&input["vault_id"]);
    let vrk = Vrk::from_bytes(&arr(&input["vrk"]));
    let pass = SecretString::from(
        String::from_utf8(bytes(&input["passphrase_utf8"])).expect("passphrase UTF-8"),
    );
    let params = Argon2Params::new(
        input["memory_kib"].as_u64().expect("m") as u32,
        input["iterations"].as_u64().expect("t") as u32,
        input["parallelism"].as_u64().expect("p") as u32,
    )
    .expect("valid params");
    let salt = arr::<16>(&input["salt"]);
    let nonce = arr::<24>(&input["nonce"]);
    let env = seal_password_envelope_with(&vrk, vault_id, &pass, params, &salt, &nonce)
        .expect("seal password envelope");
    let pek = derive_password_key(&pass, &salt, params).expect("argon2");
    let opened = open_password_envelope(&env, vault_id, &pass).expect("open");
    assert_eq!(opened.expose(), vrk.expose());
    json!({
        "passphrase_nfc_utf8": h(normalize_passphrase(&pass).as_bytes()),
        "password_key": h(pek.expose_secret()),
        "aad": h(&envelope_aad(vault_id, RECIPIENT_CODE_PASSWORD, None)),
        "ciphertext": h(env.ciphertext.as_slice()),
        "envelope": serde_json::to_value(&env).expect("envelope JSON"),
    })
}

fn compute_recovery(input: &Value) -> Value {
    let vault_id: VaultId = id(&input["vault_id"]);
    let vrk = Vrk::from_bytes(&arr(&input["vrk"]));
    let rk = RecoveryKey::from_bytes(&arr(&input["recovery_key"]));
    let nonce = arr::<24>(&input["nonce"]);
    let env = seal_recovery_envelope_with(&vrk, vault_id, &rk, &nonce).expect("seal recovery");
    let opened = open_recovery_envelope(&env, vault_id, &rk).expect("open");
    assert_eq!(opened.expose(), vrk.expose());
    json!({
        "recovery_kek": h(recovery_kek(&rk, vault_id).expose_secret()),
        "hkdf_info_utf8": utf8(labels::RECOVERY_KEK),
        "aad": h(&envelope_aad(vault_id, RECIPIENT_CODE_RECOVERY, None)),
        "ciphertext": h(env.ciphertext.as_slice()),
        "envelope": serde_json::to_value(&env).expect("envelope JSON"),
    })
}

fn compute_device(input: &Value) -> Value {
    let vault_id: VaultId = id(&input["vault_id"]);
    let device_id: DeviceId = id(&input["device_id"]);
    let vrk = Vrk::from_bytes(&arr(&input["vrk"]));
    let device = DeviceSecretKeys::from_test_seeds(
        &arr(&input["device_x25519_secret"]),
        &arr(&input["device_ed25519_seed"]),
    );
    let d_pub = device.public_keys().encryption;
    let eph = x25519_secret_from(&arr(&input["ephemeral_x25519_secret"]));
    let e_pub = x25519_public(&eph);
    let nonce = arr::<24>(&input["nonce"]);
    let env = seal_device_envelope_with(&vrk, vault_id, device_id, &d_pub, &eph, &nonce)
        .expect("seal device");
    let shared = x25519(&eph, &d_pub).expect("dh");
    let key = device_envelope_key(&shared, &e_pub, &d_pub, vault_id);
    let opened = open_device_envelope(&env, vault_id, device_id, &device).expect("open");
    assert_eq!(opened.expose(), vrk.expose());
    let mut salt = e_pub.to_vec();
    salt.extend_from_slice(&d_pub);
    let mut info = labels::DEVICE_ENVELOPE.to_vec();
    info.extend_from_slice(vault_id.as_bytes());
    json!({
        "device_x25519_public": h(&d_pub),
        "ephemeral_x25519_public": h(&e_pub),
        "shared_secret": h(shared.expose_secret()),
        "hkdf_salt": h(&salt),
        "hkdf_info": h(&info),
        "envelope_key": h(key.expose_secret()),
        "aad": h(&envelope_aad(vault_id, RECIPIENT_CODE_DEVICE, Some(device_id.as_bytes()))),
        "ciphertext": h(env.ciphertext.as_slice()),
        "envelope": serde_json::to_value(&env).expect("envelope JSON"),
    })
}

fn compute_envelope(input: &Value) -> Value {
    match input["type"].as_str() {
        Some("password") => compute_password(input),
        Some("recovery") => compute_recovery(input),
        Some("device") => compute_device(input),
        other => panic!("unknown envelope type {other:?}"),
    }
}

#[test]
fn envelope_vectors() {
    // "correct horse battery staple Cafe" + U+0301 (NFD é): NFC must turn it
    // into U+00E9 before Argon2.
    let passphrase = "correct horse battery staple Cafe\u{301}";
    run(
        "envelopes.json",
        "VRK envelopes (password: Argon2id v0x13 → XChaCha20-Poly1305; recovery: HKDF(RK) → XChaCha20-Poly1305; device: X25519 + HKDF → XChaCha20-Poly1305). AAD = envelope_aad(vault_id, recipient_code, recipient_id). Password case uses the server-floor Argon2 params to keep tests fast; production default is m=65536, t=3, p=1.",
        vec![
            (
                "password",
                json!({
                    "type": "password",
                    "vault_id": VAULT_ID,
                    "vrk": h(&seq::<32>(0x00)),
                    "passphrase_utf8": h(passphrase.as_bytes()),
                    "salt": h(&seq::<16>(0x10)),
                    "memory_kib": Argon2Params::FLOOR.memory_kib(),
                    "iterations": Argon2Params::FLOOR.iterations(),
                    "parallelism": Argon2Params::FLOOR.parallelism(),
                    "nonce": h(&seq::<24>(0x20)),
                }),
            ),
            (
                "recovery",
                json!({
                    "type": "recovery",
                    "vault_id": VAULT_ID,
                    "vrk": h(&seq::<32>(0x00)),
                    "recovery_key": h(&seq::<32>(0x40)),
                    "nonce": h(&seq::<24>(0x60)),
                }),
            ),
            (
                "device",
                json!({
                    "type": "device",
                    "vault_id": VAULT_ID,
                    "vrk": h(&seq::<32>(0x00)),
                    "device_id": "00000000-0000-7000-8000-0000000000d1",
                    "device_x25519_secret": h(&seq::<32>(0x80)),
                    "device_ed25519_seed": h(&seq::<32>(0xa0)),
                    "ephemeral_x25519_secret": h(&seq::<32>(0xc0)),
                    "nonce": h(&seq::<24>(0xe0)),
                }),
            ),
        ],
        compute_envelope,
    );
}

// ---------------------------------------------------------------------------
// recovery key encodings
// ---------------------------------------------------------------------------

fn compute_recovery_key(input: &Value) -> Value {
    let vault_id: VaultId = id(&input["vault_id"]);
    let rk = RecoveryKey::from_bytes(&arr(&input["recovery_key"]));
    let mnemonic = rk.to_mnemonic();
    let back = RecoveryKey::from_mnemonic(mnemonic.expose_phrase()).expect("decode mnemonic");
    assert_eq!(back.expose(), rk.expose());
    let qr = rk.to_qr_payload(vault_id);
    let (v, back) = RecoveryKey::from_qr_payload(&qr).expect("parse qr");
    assert_eq!((v, back.expose()), (vault_id, rk.expose()));
    json!({
        "mnemonic": mnemonic.expose_phrase(),
        "qr_payload": qr.as_str(),
    })
}

#[test]
fn recovery_key_vectors() {
    run(
        "recovery_key.json",
        "Recovery Key encodings: BIP-39 English 24-word mnemonic of the 32 bytes (entropy encoding only, no seed derivation) and QR payload consolecrypt-recovery:v1:<vault_id>:<base64url(RK), no padding>",
        vec![
            ("zero", json!({ "vault_id": VAULT_ID, "recovery_key": h(&[0u8; 32]) })),
            ("sequential", json!({ "vault_id": VAULT_ID, "recovery_key": h(&seq::<32>(0x40)) })),
            ("ff", json!({ "vault_id": VAULT_ID, "recovery_key": h(&[0xffu8; 32]) })),
        ],
        compute_recovery_key,
    );
}

// ---------------------------------------------------------------------------
// objects
// ---------------------------------------------------------------------------

fn compute_object(input: &Value) -> Value {
    let vault_id: VaultId = id(&input["vault_id"]);
    let object_id: ObjectId = id(&input["object_id"]);
    let revision = input["revision"].as_i64().expect("revision");
    let vrk = Vrk::from_bytes(&arr(&input["vrk"]));
    let keys = KeyHierarchy::derive(&vrk, vault_id);
    let json_text = input["payload_json"].as_str().expect("payload_json");
    let payload: cc_models::ObjectPayload =
        serde_json::from_str(json_text).expect("payload_json parses as ObjectPayload");
    let class = payload.object.kind().kek_class();
    let plain = pad_serialized(json_text.as_bytes()).expect("pad");
    assert_eq!(unpad(&plain).expect("unpad"), json_text.as_bytes());
    let dek = Dek::from_bytes(&arr(&input["dek"]));
    let body = encrypt_padded_with(
        &keys,
        object_id,
        revision,
        class,
        &plain,
        &dek,
        &arr(&input["nonce"]),
        &arr(&input["wrapped_dek_nonce"]),
    )
    .expect("encrypt");
    let (decrypted, got_class) =
        decrypt_object(&keys, vault_id, object_id, revision, &body, None).expect("decrypt");
    assert_eq!(got_class, class);
    assert_eq!(decrypted, payload);
    json!({
        "kek_class": class_name(class),
        "kek": h(keys.kek(class).expose()),
        "padded_plaintext_len": plain.len(),
        "padded_plaintext_sha256": sha(&plain),
        "object_aad": h(&object_aad(vault_id, object_id, revision, OBJECT_FORMAT_V1)),
        "wrapped_dek_aad": h(&wrapped_dek_aad(vault_id, object_id, revision)),
        "ciphertext_sha256": sha(body.ciphertext.as_slice()),
        "wrapped_dek": h(body.wrapped_dek.as_slice()),
        "body": serde_json::to_value(&body).expect("body JSON"),
    })
}

#[test]
fn object_vectors() {
    const TS: &str = "2026-01-01T00:00:00Z";
    let host = format!(
        r#"{{"schema":1,"kind":"host","data":{{"id":"00000000-0000-7000-8000-000000000002","name":"prod-db","address":"10.10.10.20","port":22,"username":"deploy","created_at":"{TS}","updated_at":"{TS}"}}}}"#
    );
    // > 16 KiB unpadded → 4 KiB padding bucket.
    let big_host = format!(
        r#"{{"schema":1,"kind":"host","data":{{"id":"00000000-0000-7000-8000-000000000003","name":"big","address":"192.0.2.7","notes":"{}","created_at":"{TS}","updated_at":"{TS}"}}}}"#,
        "n".repeat(17_000)
    );
    let note = format!(
        r#"{{"schema":1,"kind":"note","data":{{"id":"00000000-0000-7000-8000-000000000004","title":"Runbook","body":"Restart with systemctl restart app","tags":["ops"],"created_at":"{TS}","updated_at":"{TS}"}}}}"#
    );
    let case = |object_id: &str, revision: i64, payload: &str| {
        json!({
            "vault_id": VAULT_ID,
            "vrk": h(&seq::<32>(0x00)),
            "object_id": object_id,
            "revision": revision,
            "payload_json": payload,
            "dek": h(&seq::<32>(0xd0)),
            "nonce": h(&seq::<24>(0x30)),
            "wrapped_dek_nonce": h(&seq::<24>(0x50)),
        })
    };
    run(
        "objects.json",
        "Object encryption: plain = u32_be(len(json)) || json || zero padding (256 B buckets below 16 KiB, 4 KiB from 16 KiB); ct = XChaCha20-Poly1305(DEK, nonce, plain, object_aad(vault, object, revision, format=1)); wrapped_dek = XChaCha20-Poly1305(KEK_class, wrapped_dek_nonce, DEK, wrapped_dek_aad(vault, object, revision)). Inputs carry the exact JSON bytes.",
        vec![
            ("host_inventory", case("00000000-0000-7000-8000-000000000002", 3, &host)),
            ("large_host_4k_bucket", case("00000000-0000-7000-8000-000000000003", 1, &big_host)),
            ("note_snippets", case("00000000-0000-7000-8000-000000000004", 7, &note)),
        ],
        compute_object,
    );
}

// ---------------------------------------------------------------------------
// device verification code and approval signature
// ---------------------------------------------------------------------------

fn compute_device_case(input: &Value) -> Value {
    match input["type"].as_str() {
        Some("verification_code") => {
            let device_id: DeviceId = id(&input["device_id"]);
            let keys = DeviceSecretKeys::from_test_seeds(
                &arr(&input["x25519_secret"]),
                &arr(&input["ed25519_seed"]),
            )
            .public_keys();
            let fp = device_fingerprint_input(device_id, &keys.encryption, &keys.signing);
            json!({
                "encryption_public_key": h(&keys.encryption),
                "signing_public_key": h(&keys.signing),
                "fingerprint_input": h(&fp),
                "fingerprint_sha256": sha(&fp),
                "verification_code": verification_code(device_id, &keys.encryption, &keys.signing).to_string(),
            })
        }
        Some("approval") => {
            let approver = DeviceSecretKeys::from_test_seeds(
                &arr(&input["approver_x25519_secret"]),
                &arr(&input["approver_ed25519_seed"]),
            );
            let new_keys = DevicePublicKeys {
                encryption: arr(&input["new_encryption_public_key"]),
                signing: arr(&input["new_signing_public_key"]),
            };
            let vaults: Vec<VaultId> = input["vault_ids"]
                .as_array()
                .expect("vault_ids")
                .iter()
                .map(id)
                .collect();
            let approval = DeviceApproval {
                request_id: id::<DeviceRequestId>(&input["request_id"]),
                approver_device_id: id(&input["approver_device_id"]),
                new_device_id: id(&input["new_device_id"]),
                new_device_keys: &new_keys,
                issued_at: input["issued_at"].as_i64().expect("issued_at"),
                vault_ids: &vaults,
            };
            let sig = approver.sign_device_approval(&approval);
            let pk = approver.public_keys().signing;
            crate::verify_device_approval(&pk, &approval, &sig).expect("verify");
            json!({
                "approver_signing_public_key": h(&pk),
                "message": h(&approval.message()),
                "signature": h(&sig),
            })
        }
        other => panic!("unknown device case {other:?}"),
    }
}

#[test]
fn device_vectors() {
    let new_dev =
        DeviceSecretKeys::from_test_seeds(&seq::<32>(0x11), &seq::<32>(0x22)).public_keys();
    run(
        "device.json",
        "Device verification code: SHA-256(device_fingerprint_input(device_id, X25519_pub, Ed25519_pub)) → 6 groups of 5 digits, group i = big-endian u40 of bytes [5i, 5i+5) mod 100000. Approval: Ed25519 over device_approval_message (vault ids sorted + deduplicated).",
        vec![
            (
                "verification_code_1",
                json!({
                    "type": "verification_code",
                    "device_id": "00000000-0000-7000-8000-0000000000d1",
                    "x25519_secret": h(&seq::<32>(0x80)),
                    "ed25519_seed": h(&seq::<32>(0xa0)),
                }),
            ),
            (
                "verification_code_2",
                json!({
                    "type": "verification_code",
                    "device_id": "00000000-0000-7000-8000-0000000000d2",
                    "x25519_secret": h(&seq::<32>(0x11)),
                    "ed25519_seed": h(&seq::<32>(0x22)),
                }),
            ),
            (
                "approval",
                json!({
                    "type": "approval",
                    "approver_x25519_secret": h(&seq::<32>(0x80)),
                    "approver_ed25519_seed": h(&seq::<32>(0xa0)),
                    "request_id": "00000000-0000-7000-8000-0000000000a1",
                    "approver_device_id": "00000000-0000-7000-8000-0000000000d1",
                    "new_device_id": "00000000-0000-7000-8000-0000000000d2",
                    "new_encryption_public_key": h(&new_dev.encryption),
                    "new_signing_public_key": h(&new_dev.signing),
                    "issued_at": 1_800_000_000i64,
                    "vault_ids": [
                        "00000000-0000-7000-8000-000000000009",
                        VAULT_ID,
                        "00000000-0000-7000-8000-000000000009",
                    ],
                }),
            ),
        ],
        compute_device_case,
    );
}

// ---------------------------------------------------------------------------
// backup manifest MAC
// ---------------------------------------------------------------------------

fn compute_backup(input: &Value) -> Value {
    use crate::backup::{manifest_key_for_tests, BackupObject, VaultBackup};
    let vault_id: VaultId = id(&input["vault_id"]);
    let vrk = Vrk::from_bytes(&arr(&input["vrk"]));
    let objects: Vec<BackupObject> =
        serde_json::from_value(input["objects"].clone()).expect("objects");
    let backup = VaultBackup::seal(
        &vrk,
        vault_id,
        serde_json::from_value(input["password_envelope"].clone()).expect("password envelope"),
        serde_json::from_value(input["recovery_envelope"].clone()).expect("recovery envelope"),
        objects,
        serde_json::from_value(input["created_at"].clone()).expect("created_at"),
        input["app_version"]
            .as_str()
            .expect("app_version")
            .to_owned(),
    )
    .expect("seal backup");
    backup.verify_manifest(&vrk).expect("verify");
    json!({
        "manifest_key_hkdf_info_utf8": utf8(crate::backup::BACKUP_MANIFEST_KEY_LABEL),
        "manifest_key": h(&manifest_key_for_tests(&vrk, vault_id)),
        "manifest": h(&backup.manifest_bytes()),
        "manifest_mac": h(backup.manifest_mac.as_slice()),
    })
}

#[test]
fn backup_vectors() {
    let vault_id = VaultId::from_str(VAULT_ID).unwrap();
    let vrk = Vrk::from_bytes(&seq::<32>(0x00));
    let keys = KeyHierarchy::derive(&vrk, vault_id);
    let pw = seal_password_envelope_with(
        &vrk,
        vault_id,
        &SecretString::from("correct horse battery staple"),
        Argon2Params::FLOOR,
        &seq::<16>(0x10),
        &seq::<24>(0x20),
    )
    .unwrap();
    let rec = seal_recovery_envelope_with(
        &vrk,
        vault_id,
        &RecoveryKey::from_bytes(&seq::<32>(0x40)),
        &seq::<24>(0x60),
    )
    .unwrap();
    let object_id = ObjectId::from_str("00000000-0000-7000-8000-000000000002").unwrap();
    let json_text = r#"{"schema":1,"kind":"host","data":{"id":"00000000-0000-7000-8000-000000000002","name":"prod-db","address":"10.10.10.20","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}}"#;
    let body = encrypt_padded_with(
        &keys,
        object_id,
        2,
        KekClass::Inventory,
        &pad_serialized(json_text.as_bytes()).unwrap(),
        &Dek::from_bytes(&seq::<32>(0xd0)),
        &seq::<24>(0x30),
        &seq::<24>(0x50),
    )
    .unwrap();
    run(
        "backup.json",
        "Backup manifest MAC (.ccbackup v1, ADR-0106/ADR-0102): key = HKDF-SHA256(VRK, salt = vault_id, info = \"consolecrypt/v1/backup-manifest-key\"); mac = HMAC-SHA256(key, manifest) with the canonical manifest layout of VaultBackup::manifest_bytes (objects sorted by id).",
        vec![(
            "two_objects",
            json!({
                "vault_id": VAULT_ID,
                "vrk": h(&seq::<32>(0x00)),
                "created_at": "2026-09-26T12:00:00.123456789Z",
                "app_version": "0.1.0",
                "password_envelope": serde_json::to_value(&pw).unwrap(),
                "recovery_envelope": serde_json::to_value(&rec).unwrap(),
                "objects": [
                    { "object_id": "00000000-0000-7000-8000-00000000000f", "revision": 5, "deleted": true },
                    { "object_id": object_id.to_string(), "revision": 2, "deleted": false, "body": serde_json::to_value(&body).unwrap() },
                ],
            }),
        )],
        compute_backup,
    );
}
