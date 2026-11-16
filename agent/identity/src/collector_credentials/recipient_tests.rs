//! Private codec tests only. No filesystem/custody/installed-service acceptance.
use super::*;
fn resource(id: &str) -> ResourceId { ResourceId::new(id).unwrap() }
fn fixture() -> (Binding, DeviceSecretKeys) {
    (Binding { agent_id: AgentId::new("11111111-1111-4111-8111-111111111111").unwrap(),
        key_epoch: 1, certificate_hash: Hash::from_bytes([2;32]), owner_identity: Hash::from_bytes([3;32]),
        recipient_id: resource("44444444-4444-4444-8444-444444444444") }, DeviceSecretKeys::generate().unwrap())
}
#[test]
fn original_recipient_codec_roundtrips_actual_separate_device_keys() {
    let (binding, keys) = fixture();
    let encoded = encode(&binding, &keys).unwrap();
    assert_eq!(encoded.len(), ENCODED);
    let decoded = decode(&encoded, &binding).unwrap();
    assert_eq!(decoded.public_keys().encryption, keys.public_keys().encryption);
    assert_eq!(decoded.public_keys().signing, keys.public_keys().signing);
    assert!(DeviceSecretKeys::from_secret_bytes(&encoded[184..253]).is_ok());
}
#[test]
fn key_record_cannot_be_adopted_by_another_node_epoch_owner_certificate_or_recipient_id() {
    let (binding, keys) = fixture(); let encoded = encode(&binding, &keys).unwrap();
    let mut variants = Vec::new();
    let mut changed = binding.clone(); changed.key_epoch = 2; variants.push(changed);
    let mut changed = binding.clone(); changed.agent_id = AgentId::new("55555555-5555-4555-8555-555555555555").unwrap(); variants.push(changed);
    let mut changed = binding.clone(); changed.certificate_hash = Hash::from_bytes([6;32]); variants.push(changed);
    let mut changed = binding.clone(); changed.owner_identity = Hash::from_bytes([7;32]); variants.push(changed);
    let mut changed = binding.clone(); changed.recipient_id = resource("88888888-8888-4888-8888-888888888888"); variants.push(changed);
    for changed in variants { assert!(decode(&encoded, &changed).is_err()); }
}
#[test]
fn incomplete_corrupt_or_substituted_private_key_encoding_never_reconstructs_a_holder() {
    let (binding, keys) = fixture(); let encoded = encode(&binding, &keys).unwrap();
    for length in [0, 68, 184, 252, 253, 284] { assert!(decode(&encoded[..length], &binding).is_err()); }
    for offset in [0, 8, 44, 52, 84, 116, 152, 184, 252, 253, 284] {
        let mut changed = Zeroizing::new(encoded.to_vec()); changed[offset] ^= 1;
        assert!(decode(&changed, &binding).is_err());
    }
    let foreign = DeviceSecretKeys::generate().unwrap(); let mut changed = Zeroizing::new(encoded.to_vec());
    changed[184..253].copy_from_slice(&foreign.to_secret_bytes());
    assert!(decode(&changed, &binding).is_err());
}
