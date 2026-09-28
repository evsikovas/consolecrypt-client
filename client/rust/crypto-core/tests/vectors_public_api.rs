//! Consume the committed test vectors through the *public* API only — the
//! way another component (or another language) would: recover the VRK from
//! the recovery/password envelopes, derive the hierarchy, decrypt the object
//! vectors, recompute verification codes and verify the approval signature.

use cc_crypto_core::{
    decrypt_object, open_password_envelope, open_recovery_envelope, verification_code,
    verify_device_approval, DeviceApproval, DevicePublicKeys, KeyHierarchy, RecoveryKey,
    SecretString,
};
use cc_protocol::envelopes::NewEnvelope;
use cc_protocol::sync::EncryptedBody;
use cc_protocol::{DeviceId, ObjectId, VaultId};
use serde_json::Value;
use std::str::FromStr;

fn load(name: &str) -> Value {
    let path = format!("{}/tests/vectors/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn case<'a>(file: &'a Value, name: &str) -> &'a Value {
    file["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap()
}

fn arr32(v: &Value) -> [u8; 32] {
    hex::decode(v.as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap()
}

#[test]
fn recover_vrk_from_vectors_and_decrypt_objects() {
    let hierarchy = load("key_hierarchy.json");
    let envelopes = load("envelopes.json");
    let recovery = load("recovery_key.json");
    let objects = load("objects.json");

    let rec_case = case(&envelopes, "recovery");
    let vault = VaultId::from_str(rec_case["input"]["vault_id"].as_str().unwrap()).unwrap();
    let expected_vak = case(&hierarchy, "sequential")["output"]["vault_access_key"]["okm"]
        .as_str()
        .unwrap()
        .to_string();

    // Recovery Key via its mnemonic, as a user would type it.
    let mnemonic = case(&recovery, "sequential")["output"]["mnemonic"]
        .as_str()
        .unwrap();
    let rk = RecoveryKey::from_mnemonic(mnemonic).unwrap();
    let env: NewEnvelope = serde_json::from_value(rec_case["output"]["envelope"].clone()).unwrap();
    let vrk = open_recovery_envelope(&env, vault, &rk).unwrap();
    let keys = KeyHierarchy::derive(&vrk, vault);
    assert_eq!(
        hex::encode(keys.vault_access_key().to_protocol_bytes().as_slice()),
        expected_vak
    );

    // Password envelope with the (NFD) passphrase bytes from the vector.
    let pw = case(&envelopes, "password");
    let passphrase =
        String::from_utf8(hex::decode(pw["input"]["passphrase_utf8"].as_str().unwrap()).unwrap())
            .unwrap();
    let env: NewEnvelope = serde_json::from_value(pw["output"]["envelope"].clone()).unwrap();
    let vrk2 = open_password_envelope(&env, vault, &SecretString::from(passphrase)).unwrap();
    assert_eq!(
        KeyHierarchy::derive(&vrk2, vault)
            .vault_access_key()
            .verifier(),
        keys.vault_access_key().verifier()
    );

    // Every object vector decrypts with the recovered hierarchy.
    for c in objects["cases"].as_array().unwrap() {
        let input = &c["input"];
        let object_id = ObjectId::from_str(input["object_id"].as_str().unwrap()).unwrap();
        let revision = input["revision"].as_i64().unwrap();
        let body: EncryptedBody = serde_json::from_value(c["output"]["body"].clone()).unwrap();
        let (payload, class) =
            decrypt_object(&keys, vault, object_id, revision, &body, None).unwrap();
        let expected: cc_models::ObjectPayload =
            serde_json::from_str(input["payload_json"].as_str().unwrap()).unwrap();
        assert_eq!(payload, expected);
        assert_eq!(
            serde_json::to_value(class).unwrap(),
            c["output"]["kek_class"]
        );
        // Wrong revision fails.
        assert!(decrypt_object(&keys, vault, object_id, revision + 1, &body, None).is_err());
    }
}

#[test]
fn verification_codes_and_approval_signature() {
    let device = load("device.json");
    for c in device["cases"].as_array().unwrap() {
        let (i, o) = (&c["input"], &c["output"]);
        match i["type"].as_str().unwrap() {
            "verification_code" => {
                let id = DeviceId::from_str(i["device_id"].as_str().unwrap()).unwrap();
                let code = verification_code(
                    id,
                    &arr32(&o["encryption_public_key"]),
                    &arr32(&o["signing_public_key"]),
                );
                assert_eq!(code.to_string(), o["verification_code"].as_str().unwrap());
            }
            "approval" => {
                let keys = DevicePublicKeys::from_slices(
                    &arr32(&i["new_encryption_public_key"]),
                    &arr32(&i["new_signing_public_key"]),
                )
                .unwrap();
                let vaults: Vec<VaultId> = i["vault_ids"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| VaultId::from_str(v.as_str().unwrap()).unwrap())
                    .collect();
                let approval = DeviceApproval {
                    request_id: i["request_id"].as_str().unwrap().parse().unwrap(),
                    approver_device_id: i["approver_device_id"].as_str().unwrap().parse().unwrap(),
                    new_device_id: i["new_device_id"].as_str().unwrap().parse().unwrap(),
                    new_device_keys: &keys,
                    issued_at: i["issued_at"].as_i64().unwrap(),
                    vault_ids: &vaults,
                };
                assert_eq!(
                    hex::encode(approval.message()),
                    o["message"].as_str().unwrap()
                );
                let sig = hex::decode(o["signature"].as_str().unwrap()).unwrap();
                verify_device_approval(&arr32(&o["approver_signing_public_key"]), &approval, &sig)
                    .unwrap();
            }
            other => panic!("unexpected case type {other}"),
        }
    }
}
