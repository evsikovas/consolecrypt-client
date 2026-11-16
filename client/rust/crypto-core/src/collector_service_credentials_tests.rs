//! Authored, UNRUN. Real existing signature/capsule functions, synthetic values.
//! These are NOT Actor durable-counter, process-loss, installed-service tests.
use super::*;
use crate::{SelectedServiceCredential, seal_service_credential};
use cc_agent_node_wire::{AgentId, Capabilities, CertificateOrigin, CollectorCredentialScope,
    CollectorCredentialTarget, CollectorRecipientAttestation, CollectorServiceCredentialRef,
    CollectorServiceProfile, CollectorServiceQuota, Context, Header, InvitationId,
    Scope, Scopes, collector_credential_manifest_sha256, collector_service_resource};
use serde_json::{Value, json};
use zeroize::Zeroizing;

fn time(n: u64) -> Timestamp { Timestamp::new(n).unwrap() }
fn at(n: u64) -> Now { Now(time(n)) }
fn id(s: &str) -> ResourceId { ResourceId::new(s).unwrap() }
fn sign<T: Statement>(body: T, keys: &DeviceSecretKeys) -> Signed<T> {
    Signed { signature: SignatureBytes::from_bytes(keys.sign_message(&body.signing_bytes().unwrap())), body }
}
struct Fixture {
    owner: DeviceSecretKeys, recipient: DeviceSecretKeys, pin: OwnerPin,
    cert: Signed<Certificate>, authorization: CollectorServiceCredentialAuthorization, catalog: Value,
}
impl Fixture {
    fn new() -> Self {
        let owner = DeviceSecretKeys::generate().unwrap();
        let node = DeviceSecretKeys::generate().unwrap();
        let recipient = DeviceSecretKeys::generate().unwrap();
        let context: Context = serde_json::from_value(json!({"tenant":{"kind":"localProfile",
            "id":"11111111-1111-4111-8111-111111111111"},
            "deploymentId":"22222222-2222-4222-8222-222222222222"})).unwrap();
        let pin = OwnerPin { context: context.clone(), key: PublicKey::from_bytes(owner.public_keys().signing),
            epoch: 1, checkpoint: 2, expires_at: time(100000) };
        let header = Header { version: 1, context, owner_epoch: 1, checkpoint: 2 };
        let agent = AgentId::new("33333333-3333-4333-8333-333333333333").unwrap();
        let node_key = PublicKey::from_bytes(node.public_keys().signing);
        let cert = sign(Certificate { header: header.clone(), agent_id: agent, key_epoch: 1,
            agent_key: node_key, scopes: Scopes(vec![Scope::MetricsCollect]),
            capabilities: Capabilities { version: 1, scopes: Scopes(vec![Scope::MetricsCollect]) },
            origin: CertificateOrigin::Enrollment {
                invitation_id: InvitationId::new("44444444-4444-4444-8444-444444444444").unwrap(),
                claim_hash: Hash::from_bytes([5;32]), key_fingerprint: node_key.fingerprint() },
            not_before: time(900), expires_at: time(100000) }, &owner);
        let catalog = json!({"targets":[{"id":"application","url":"http://127.0.0.1:9187/metrics"}],
            "receiver":{"url":"http://127.0.0.1:19090/api/v1/write"}});
        let manifest_sha256 = collector_credential_manifest_sha256(&catalog).unwrap();
        let base = sign(CollectorServiceDelegation { header: header.clone(),
            service_id: id("66666666-6666-4666-8666-666666666666"), policy_revision: 1, revocation_epoch: 1,
            agent_id: agent, key_epoch: 1, recipient: node_key, certificate_hash: cert.body.digest().unwrap(),
            collector_id: collector_service_resource(manifest_sha256).unwrap(), manifest_sha256,
            profile: CollectorServiceProfile::MaintainedPrometheusDisk512mib2hV1,
            quota: CollectorServiceQuota::fixed(), credential_targets: vec![
                CollectorCredentialTarget::Scrape { target_id: "application".into() },
                CollectorCredentialTarget::Receiver {}], issued_at: time(1000), expires_at: time(87400) }, &owner);
        let admission = sign(CollectorServiceClaim { header: header.clone(), agent_id: agent, key_epoch: 1,
            delegation_hash: base.body.digest().unwrap(), clock: CollectorServiceClockStamp {
                boot_id: id("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"), created_boottime_ns: 10_000_000_000,
                deadline_boottime_ns: 86_410_000_000_000 }, nonce: Nonce::from_bytes([6;16]), issued_at: time(1000) }, &node);
        let proof = sign(CollectorRecipientAttestation { header, agent_id: agent, key_epoch: 1,
            certificate_hash: cert.body.digest().unwrap(), service_id: base.body.service_id,
            base_delegation_hash: base.body.digest().unwrap(), manifest_sha256,
            original_start_nonce: Nonce::from_bytes([7;16]),
            recipient_id: id("77777777-7777-4777-8777-777777777777"),
            recipient_public_key: Hash::from_bytes(recipient.public_keys().encryption),
            recipient_key_epoch: 1, recipient_certificate_epoch: 1, challenge: Nonce::from_bytes([8;16]),
            issued_at: time(1000), expires_at: time(1060) }, &node);
        let credentials = vec![CollectorServiceCredentialRef {
            target: CollectorCredentialTarget::Scrape { target_id: "application".into() },
            endpoint_sha256: Hash::from_bytes(Sha256::digest(b"http://127.0.0.1:9187/metrics").into()),
            username: "exporter".into(), credential_id: id("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"), credential_revision: 1,
        }, CollectorServiceCredentialRef { target: CollectorCredentialTarget::Receiver {},
            endpoint_sha256: Hash::from_bytes(Sha256::digest(b"http://127.0.0.1:19090/api/v1/write").into()),
            username: "receiver".into(), credential_id: id("cccccccc-cccc-4ccc-8ccc-cccccccccccc"), credential_revision: 2 }];
        let authorization = CollectorServiceCredentialAuthorization {
            authorization_id: id("dddddddd-dddd-4ddd-8ddd-dddddddddddd"),
            owner_device_id: id("99999999-9999-4999-8999-999999999999"),
            base, admission, recipient: proof, credentials, issued_at: time(1000), expires_at: time(1600) };
        Self { owner, recipient, pin, cert, authorization, catalog }
    }
    fn approved(&self) -> Signed<CollectorServiceCredentialAuthorization> {
        let a = &self.authorization;
        let prepared = PreparedCollectorServiceCredentialOwner::prepare(&self.pin, &self.cert,
            &a.base, &a.admission, a.clone(), a.recipient.body.challenge, &self.catalog, at(1000)).unwrap();
        self.owner.sign_collector_service_credential_authorization_v1(prepared, at(1000)).unwrap()
    }
    fn capsules(&self, a: &Signed<CollectorServiceCredentialAuthorization>) -> Vec<ServiceCredentialCapsule> {
        (0..2).map(|slot| {
            let binding = collector_service_credential_binding(a, &self.pin, slot, at(1000)).unwrap();
            let selected = SelectedServiceCredential::new(binding, Zeroizing::new(b"synthetic scoped password".to_vec())).unwrap();
            seal_service_credential(selected, &self.recipient.public_keys().encryption, &self.owner, 1000, 1600).unwrap()
        }).collect()
    }
    fn open(&self, a: &Signed<CollectorServiceCredentialAuthorization>, c: &[ServiceCredentialCapsule], n: u64)
        -> Result<Vec<OpenedServiceCredential>> {
        open_collector_service_credential_set(a, c, &self.pin, &self.authorization.base.body,
            &self.authorization.admission.body.clock, &self.catalog,
            self.authorization.recipient.body.recipient_id, &self.recipient, at(n))
    }
}

#[test]
fn service_material_reopens_after_initial_proof_expiry_without_refresh() {
    let f = Fixture::new(); let a = f.approved(); let c = f.capsules(&a);
    let original = a.body.admission.body.clock.clone();
    assert_eq!(f.open(&a, &c, 1000).unwrap().len(), 2);
    assert_eq!(f.open(&a, &c, 1200).unwrap().len(), 2);
    assert_eq!(a.body.admission.body.clock, original);
    assert!(f.open(&a, &c, 1600).is_err());
}
#[test]
fn changed_boot_or_even_one_ns_deadline_change_is_denied() {
    let f = Fixture::new(); let a = f.approved();
    let mut clock = a.body.admission.body.clock.clone();
    clock.boot_id = id("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee");
    assert!(a.body.check_original(&a.body.base.body, &clock, &f.catalog, at(1200)).is_err());
    clock = a.body.admission.body.clock.clone(); clock.deadline_boottime_ns += 1;
    assert!(a.body.check_original(&a.body.base.body, &clock, &f.catalog, at(1200)).is_err());
}
#[test]
fn incomplete_duplicate_public_or_reordered_capsules_are_denied() {
    let f = Fixture::new(); let a = f.approved(); let mut c = f.capsules(&a);
    assert!(f.open(&a, &c[..1], 1200).is_err());
    c.swap(0, 1); assert!(f.open(&a, &c, 1200).is_err());
    let mut bad = a.body.clone(); bad.credentials[1] = bad.credentials[0].clone(); assert!(bad.validate().is_err());
    bad.credentials.clear(); assert!(bad.validate().is_err());
}
#[test]
fn changing_any_reference_changes_all_capsule_bindings() {
    let f = Fixture::new(); let a = f.approved();
    let original = a.body.capsule_delegation_digest(0).unwrap();
    let mut altered = a.body.clone(); altered.credentials[1].credential_revision += 1;
    assert_ne!(original, altered.capsule_delegation_digest(0).unwrap());
    let changed = sign(altered, &f.owner);
    assert!(f.open(&changed, &f.capsules(&a), 1200).is_err());
}
#[test]
fn tampered_ciphertext_passes_only_metadata_then_fails_actual_open() {
    let f = Fixture::new(); let a = f.approved(); let mut c = f.capsules(&a);
    let mut value: Value = serde_json::from_slice(&c[0].to_json().unwrap()).unwrap();
    value["signature"] = serde_json::to_value(cc_protocol::Bytes::new(vec![0;64])).unwrap();
    c[0] = ServiceCredentialCapsule::from_json(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(preflight_collector_service_credential_set(&a, &c, &f.pin, &a.body.base.body,
        &a.body.admission.body.clock, &f.catalog, at(1200)).is_ok());
    assert!(f.open(&a, &c, 1200).is_err());
}
#[test]
fn real_single_start_capsule_is_not_a_lifetime_capsule() {
    let f = Fixture::new(); let a = f.approved(); let mut c = f.capsules(&a);
    let b = &a.body.base.body; let p = &a.body.recipient.body; let reference = &a.body.credentials[0];
    let scope = CollectorCredentialScope { header: b.header.clone(), scope_id: a.body.authorization_id,
        owner_device_id: a.body.owner_device_id, service_id: b.service_id, base_delegation_hash: b.digest().unwrap(),
        original_start_nonce: p.original_start_nonce, recipient_attestation_digest: p.digest().unwrap(),
        recipient_challenge: p.challenge, policy_revision: b.policy_revision, revocation_epoch: b.revocation_epoch,
        agent_id: b.agent_id, key_epoch: b.key_epoch, certificate_hash: b.certificate_hash,
        collector_id: b.collector_id, manifest_sha256: b.manifest_sha256, target: reference.target.clone(),
        endpoint_sha256: reference.endpoint_sha256, username: reference.username.clone(),
        credential_id: reference.credential_id, credential_revision: reference.credential_revision,
        recipient_id: p.recipient_id,
        recipient_key_sha256: Hash::from_bytes(Sha256::digest(p.recipient_public_key.as_bytes()).into()),
        recipient_key_epoch: 1, recipient_certificate_epoch: 1, issued_at: time(1000), expires_at: time(1600) };
    let old = sign(scope, &f.owner);
    let old_binding = crate::collector_credential_binding(&old, &f.pin, at(1000)).unwrap();
    let selected = SelectedServiceCredential::new(old_binding, Zeroizing::new(b"synthetic old purpose".to_vec())).unwrap();
    c[0] = seal_service_credential(selected, &f.recipient.public_keys().encryption, &f.owner, 1000, 1600).unwrap();
    assert!(crate::open_collector_credential(&old, &c[0], &f.pin, b, b.digest().unwrap(), &f.catalog,
        p.recipient_id, &f.recipient, at(1000)).is_ok());
    assert!(f.open(&a, &c, 1000).is_err());
    assert_ne!(CollectorCredentialScope::DOMAIN, CollectorServiceCredentialAuthorization::DOMAIN);
}
#[test]
fn recipient_change_or_current_policy_change_is_denied() {
    let f = Fixture::new(); let a = f.approved(); let c = f.capsules(&a);
    let wrong = DeviceSecretKeys::generate().unwrap();
    assert!(open_collector_service_credential_set(&a, &c, &f.pin, &a.body.base.body,
        &a.body.admission.body.clock, &f.catalog, a.body.recipient.body.recipient_id, &wrong, at(1200)).is_err());
    let mut base = a.body.base.body.clone(); base.revocation_epoch += 1;
    assert!(a.body.check_original(&base, &a.body.admission.body.clock, &f.catalog, at(1200)).is_err());
}
#[test]
fn fixed_quotas_and_strict_new_version_are_not_optional() {
    let f = Fixture::new(); let mut a = f.authorization.clone();
    assert_eq!(a.base.body.quota.max_starts, 32);
    a.base.body.quota.max_starts = 33; assert!(a.validate().is_err());
    let mut value = serde_json::to_value(&f.authorization).unwrap();
    value["mode"] = json!("singleStartFallback");
    assert!(serde_json::from_value::<CollectorServiceCredentialAuthorization>(value).is_err());
}

#[test]
fn fresh_owner_preparation_cannot_reuse_an_expired_initial_challenge() {
    let f = Fixture::new(); let a = &f.authorization;
    assert!(PreparedCollectorServiceCredentialOwner::prepare(&f.pin, &f.cert, &a.base, &a.admission,
        a.clone(), a.recipient.body.challenge, &f.catalog, at(1060)).is_err());
    assert!(PreparedCollectorServiceCredentialOwner::prepare(&f.pin, &f.cert, &a.base, &a.admission,
        a.clone(), Nonce::from_bytes([9;16]), &f.catalog, at(1000)).is_err());
}
fn frames(f: &Fixture, authorization: &Signed<CollectorServiceCredentialAuthorization>) -> Vec<CollectorServiceCredentialFrame> {
    let mut result = vec![CollectorServiceCredentialFrame::authorization(authorization.clone()).unwrap()];
    for (slot, capsule) in f.capsules(authorization).into_iter().enumerate() {
        let frame = CollectorServiceCredentialFrame::capsule(authorization.body.digest().unwrap(), slot as u8, capsule).unwrap();
        let encoded = frame.to_json().unwrap();
        assert!(encoded.len() <= 12288);
        result.push(CollectorServiceCredentialFrame::from_json(&encoded).unwrap());
    }
    result
}
#[test]
fn frame_assembly_requires_the_whole_set_and_exact_slot_digest() {
    let f = Fixture::new(); let a = f.approved();
    let (roundtrip, c) = CollectorServiceCredentialFrame::into_complete_set(frames(&f, &a)).unwrap();
    assert!(f.open(&roundtrip, &c, 1200).is_ok());
    let mut incomplete = frames(&f, &a); incomplete.pop();
    assert!(CollectorServiceCredentialFrame::into_complete_set(incomplete).is_err());
    let mut foreign = frames(&f, &a); foreign.pop();
    foreign.push(CollectorServiceCredentialFrame::capsule(Hash::from_bytes([42;32]), 1,
        f.capsules(&a).pop().unwrap()).unwrap());
    assert!(CollectorServiceCredentialFrame::into_complete_set(foreign).is_err());
    let mut duplicate = frames(&f, &a); duplicate.pop();
    duplicate.push(CollectorServiceCredentialFrame::capsule(a.body.digest().unwrap(), 0,
        f.capsules(&a).pop().unwrap()).unwrap());
    assert!(CollectorServiceCredentialFrame::into_complete_set(duplicate).is_err());
}
#[test]
fn frame_version_unknown_fields_and_size_fail_closed() {
    let f = Fixture::new(); let a = f.approved();
    let frame = CollectorServiceCredentialFrame::authorization(a).unwrap();
    let mut value: Value = serde_json::from_slice(&frame.to_json().unwrap()).unwrap();
    value["version"] = json!(2);
    assert!(CollectorServiceCredentialFrame::from_json(&serde_json::to_vec(&value).unwrap()).is_err());
    value["version"] = json!(1); value["approved"] = json!(true);
    assert!(CollectorServiceCredentialFrame::from_json(&serde_json::to_vec(&value).unwrap()).is_err());
    assert!(CollectorServiceCredentialFrame::from_json(&vec![b' ';12289]).is_err());
}
