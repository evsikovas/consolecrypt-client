//! Actual existing capsule/signature functions; no OS or installed-service proof.
use super::*;
use crate::{SelectedServiceCredential, seal_service_credential};
use cc_agent_node_wire::{AgentId, Capabilities, CertificateOrigin, CollectorCredentialTarget,
    CollectorServiceProfile, CollectorServiceQuota, Context, Header, ResourceId, Scope, Scopes,
    collector_credential_manifest_sha256, collector_service_resource};
use serde_json::{Value, json};
use zeroize::Zeroizing;

fn time(n: u64) -> Timestamp { Timestamp::new(n).unwrap() }
fn now(n: u64) -> Now { Now(time(n)) }
fn resource(value: &str) -> ResourceId { ResourceId::new(value).unwrap() }
fn sign<T: Statement>(body: T, keys: &DeviceSecretKeys) -> Signed<T> {
    Signed { signature: SignatureBytes::from_bytes(keys.sign_message(&body.signing_bytes().unwrap())), body }
}
struct Fixture {
    owner: DeviceSecretKeys,
    recipient: DeviceSecretKeys,
    pin: OwnerPin,
    cert: Signed<Certificate>,
    base: Signed<CollectorServiceDelegation>,
    proof: Signed<CollectorRecipientAttestation>,
    scope: CollectorCredentialScope,
    manifest: Value,
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
            origin: CertificateOrigin::Enrollment { invitation_id: cc_agent_node_wire::InvitationId::new(
                "44444444-4444-4444-8444-444444444444").unwrap(), claim_hash: Hash::from_bytes([5;32]),
                key_fingerprint: node_key.fingerprint() }, not_before: time(900), expires_at: time(100000) }, &owner);
        let manifest = json!({"schema_version":1,"mode":"remote_write",
            "profile":"maintained-prometheus-disk-512mib-2h-v1","bufferMode":"persistentNativeWal",
            "targets":[{"id":"application","url":"http://127.0.0.1:9187/metrics","kind":"application",
                "sample_limit":1000,"series_limit":256,"response_bytes":65536,"label_limit":32}],
            "labels":{"cc_host_id":"55555555-5555-4555-8555-555555555555","site":"test","environment":"test"},
            "scrape_interval_seconds":15,"receiver":{"url":"http://127.0.0.1:19090/api/v1/write"},
            "buffer":{"max_bytes":536870912u64,"max_age_seconds":7200}});
        let manifest_hash = collector_credential_manifest_sha256(&manifest).unwrap();
        let base = sign(CollectorServiceDelegation { header: header.clone(),
            service_id: resource("66666666-6666-4666-8666-666666666666"), policy_revision: 1, revocation_epoch: 1,
            agent_id: agent, key_epoch: 1, recipient: node_key, certificate_hash: cert.body.digest().unwrap(),
            collector_id: collector_service_resource(manifest_hash).unwrap(), manifest_sha256: manifest_hash,
            profile: CollectorServiceProfile::MaintainedPrometheusDisk512mib2hV1, quota: CollectorServiceQuota::fixed(),
            credential_targets: vec![CollectorCredentialTarget::Scrape { target_id: "application".into() },
                CollectorCredentialTarget::Receiver {}],
            issued_at: time(1000), expires_at: time(87400) }, &owner);
        let recipient_id = resource("77777777-7777-4777-8777-777777777777");
        let proof = sign(CollectorRecipientAttestation { header: header.clone(), agent_id: agent, key_epoch: 1,
            certificate_hash: cert.body.digest().unwrap(), service_id: base.body.service_id,
            base_delegation_hash: base.body.digest().unwrap(), manifest_sha256: manifest_hash,
            original_start_nonce: Nonce::from_bytes([17;16]),
            recipient_id, recipient_public_key: Hash::from_bytes(recipient.public_keys().encryption),
            recipient_key_epoch: 1, recipient_certificate_epoch: 1, challenge: Nonce::from_bytes([8;16]),
            issued_at: time(1000), expires_at: time(1060) }, &node);
        let scope = CollectorCredentialScope { header,
            scope_id: resource("88888888-8888-4888-8888-888888888888"),
            owner_device_id: resource("99999999-9999-4999-8999-999999999999"),
            service_id: base.body.service_id, base_delegation_hash: base.body.digest().unwrap(),
            original_start_nonce: proof.body.original_start_nonce,
            recipient_attestation_digest: proof.body.digest().unwrap(), recipient_challenge: proof.body.challenge,
            policy_revision: 1, revocation_epoch: 1, agent_id: agent, key_epoch: 1,
            certificate_hash: cert.body.digest().unwrap(), collector_id: base.body.collector_id,
            manifest_sha256: manifest_hash, target: CollectorCredentialTarget::Scrape { target_id: "application".into() },
            endpoint_sha256: Hash::from_bytes(Sha256::digest(b"http://127.0.0.1:9187/metrics").into()),
            username: "exporter".into(), credential_id: resource("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"),
            credential_revision: 1, recipient_id,
            recipient_key_sha256: Hash::from_bytes(Sha256::digest(recipient.public_keys().encryption).into()),
            recipient_key_epoch: 1, recipient_certificate_epoch: 1, issued_at: time(1000), expires_at: time(1600) };
        Self { owner, recipient, pin, cert, base, proof, scope, manifest }
    }
    fn approved(&self) -> Signed<CollectorCredentialScope> {
        let prepared = PreparedCollectorCredentialOwner::prepare(&self.pin, &self.cert, &self.base, &self.proof,
            self.proof.body.challenge, self.scope.clone(), &self.manifest, now(1000)).unwrap();
        self.owner.sign_agent_collector_credential_scope_v1(prepared, now(1000)).unwrap()
    }
    fn capsule(&self, scope: &Signed<CollectorCredentialScope>) -> ServiceCredentialCapsule {
        let binding = collector_credential_binding(scope, &self.pin, now(1000)).unwrap();
        let material = SelectedServiceCredential::new(binding, Zeroizing::new(b"synthetic scoped password".to_vec())).unwrap();
        seal_service_credential(material, &self.recipient.public_keys().encryption, &self.owner, 1000, 1600).unwrap()
    }
    fn open(&self, scope: &Signed<CollectorCredentialScope>, capsule: &ServiceCredentialCapsule, at: u64)
        -> Result<OpenedServiceCredential> {
        open_collector_credential(scope, capsule, &self.pin, &self.base.body, self.base.body.digest().unwrap(),
            &self.manifest, self.proof.body.recipient_id, &self.recipient, now(at))
    }
}

#[test]
fn exact_signed_scope_roundtrips_real_existing_capsule_past_interactive_300_seconds() {
    let f = Fixture::new(); let scope = f.approved(); let capsule = f.capsule(&scope);
    let opened = f.open(&scope, &capsule, 1400).unwrap();
    let binding = collector_credential_binding(&scope, &f.pin, now(1400)).unwrap();
    assert!(opened.with_secret(&binding, 1400, |value| value == b"synthetic scoped password").unwrap());
    assert!(f.open(&scope, &capsule, 1600).is_err());
    assert_eq!(cc_agent_node_wire::LEASE_TTL, 300);
}
#[test]
fn wrong_challenge_or_recipient_cannot_prepare_the_owner_signature() {
    let f = Fixture::new();
    assert!(PreparedCollectorCredentialOwner::prepare(&f.pin, &f.cert, &f.base, &f.proof,
        Nonce::from_bytes([9;16]), f.scope.clone(), &f.manifest, now(1000)).is_err());
    let mut scope = f.scope.clone(); scope.recipient_key_sha256 = Hash::from_bytes([9;32]);
    assert!(PreparedCollectorCredentialOwner::prepare(&f.pin, &f.cert, &f.base, &f.proof,
        f.proof.body.challenge, scope, &f.manifest, now(1000)).is_err());
}
#[test]
fn a_separately_valid_receiver_scope_cannot_reuse_the_exporter_capsule() {
    let mut f = Fixture::new(); let first = f.approved(); let capsule = f.capsule(&first);
    f.scope.target = CollectorCredentialTarget::Receiver {};
    f.scope.endpoint_sha256 = Hash::from_bytes(Sha256::digest(b"http://127.0.0.1:19090/api/v1/write").into());
    let second = f.approved(); assert!(f.open(&second, &capsule, 1000).is_err());
}
#[test]
fn actual_capsule_rejects_changed_base_private_recipient_and_current_pin() {
    let mut f = Fixture::new(); let scope = f.approved(); let capsule = f.capsule(&scope);
    let original = f.base.body.digest().unwrap();
    assert!(open_collector_credential(&scope, &capsule, &f.pin, &f.base.body, Hash::from_bytes([1;32]),
        &f.manifest, f.proof.body.recipient_id, &f.recipient, now(1000)).is_err());
    let foreign = DeviceSecretKeys::generate().unwrap();
    assert!(open_collector_credential(&scope, &capsule, &f.pin, &f.base.body, original,
        &f.manifest, f.proof.body.recipient_id, &foreign, now(1000)).is_err());
    f.pin.checkpoint += 1; assert!(f.open(&scope, &capsule, 1000).is_err());
}
#[test]
fn target_manifest_and_credential_revision_substitution_never_open_the_capsule() {
    let mut f = Fixture::new(); let scope = f.approved(); let capsule = f.capsule(&scope);
    f.manifest["targets"][0]["url"] = json!("http://127.0.0.1:9188/metrics");
    assert!(f.open(&scope, &capsule, 1000).is_err());
    f.manifest["targets"][0]["url"] = json!("http://127.0.0.1:9187/metrics");
    f.scope.credential_revision += 1; let changed = f.approved();
    assert!(f.open(&changed, &capsule, 1000).is_err());
    let mut wire = serde_json::to_value(&f.scope).unwrap(); wire["url"] = json!("http://other.invalid/");
    assert!(serde_json::from_value::<CollectorCredentialScope>(wire).is_err());
    assert!(serde_json::from_str::<CollectorCredentialTarget>(r#"{"kind":"receiver","extra":true}"#).is_err());
}

#[test]
fn exact_pinned_go_trim_edges_are_denied_without_silently_changing_a_password() {
    let whitespace = ['\t', '\n', '\u{000b}', '\u{000c}', '\r', ' ', '\u{0085}', '\u{00a0}', '\u{1680}',
        '\u{2000}', '\u{2001}', '\u{2002}', '\u{2003}', '\u{2004}', '\u{2005}', '\u{2006}', '\u{2007}',
        '\u{2008}', '\u{2009}', '\u{200a}', '\u{2028}', '\u{2029}', '\u{202f}', '\u{205f}', '\u{3000}'];
    for c in whitespace {
        assert!(!collector_basic_password_supported(format!("{c}synthetic").as_bytes()));
        assert!(!collector_basic_password_supported(format!("synthetic{c}").as_bytes()));
    }
    for bad in [b"".as_slice(), b"a\nb", b"a\rb", b"a\0b", &[0xff]] {
        assert!(!collector_basic_password_supported(bad));
    }
    assert!(!collector_basic_password_supported(&vec![b'a';4097]));
    assert!(collector_basic_password_supported("synthetic \u{00a0} internal".as_bytes()));
    assert!(collector_basic_password_supported("\u{200b}synthetic\u{200b}".as_bytes()));
    let f = Fixture::new(); let original = f.approved(); let capsule = f.capsule(&original);
    let mut changed = f.scope.clone(); changed.username = "different-saved-user".into();
    let changed = sign(changed, &f.owner);
    assert!(f.open(&changed, &capsule, 1000).is_err());
    assert!(!format!("{:?}", original.body).contains("exporter"));
}

#[test]
fn encrypted_envelope_roundtrips_real_capsule_and_requires_original_fresh_proof() {
    let f = Fixture::new(); let scope = f.approved();
    let envelope = CollectorCredentialEnvelope::new(scope.clone(), f.capsule(&scope)).unwrap();
    let raw = envelope.to_json().unwrap();
    assert!(raw.len() <= 12 * 1024);
    assert!(!String::from_utf8_lossy(&raw).contains("synthetic scoped password"));
    let envelope = CollectorCredentialEnvelope::from_json(&raw).unwrap();
    envelope.check_metadata_only(&f.pin, &f.cert, &f.base, &f.proof,
        f.proof.body.challenge, &f.manifest, now(1000)).unwrap();
    assert!(envelope.check_metadata_only(&f.pin, &f.cert, &f.base, &f.proof,
        Nonce::from_bytes([19;16]), &f.manifest, now(1000)).is_err());
    assert!(envelope.check_metadata_only(&f.pin, &f.cert, &f.base, &f.proof,
        f.proof.body.challenge, &f.manifest, now(1060)).is_err());
    let (scope, capsule) = envelope.into_parts();
    assert!(f.open(&scope, &capsule, 1400).is_ok());
}

#[test]
fn encrypted_envelope_rejects_unknown_duplicate_fields_and_oversized_capsule() {
    let f = Fixture::new(); let scope = f.approved();
    let envelope = CollectorCredentialEnvelope::new(scope.clone(), f.capsule(&scope)).unwrap();
    let raw = envelope.to_json().unwrap();
    let mut value: Value = serde_json::from_slice(&raw).unwrap();
    value["approved"] = json!(true);
    assert!(CollectorCredentialEnvelope::from_json(&serde_json::to_vec(&value).unwrap()).is_err());
    let duplicate = format!("{{\"version\":1,{}", &String::from_utf8(raw).unwrap()[1..]);
    assert!(CollectorCredentialEnvelope::from_json(duplicate.as_bytes()).is_err());
    assert!(CollectorCredentialEnvelope::from_json(&vec![b' ';12*1024+1]).is_err());
    let binding = collector_credential_binding(&scope, &f.pin, now(1000)).unwrap();
    let selected = SelectedServiceCredential::new(binding, Zeroizing::new(vec![b'a';6000])).unwrap();
    let large = seal_service_credential(selected, &f.recipient.public_keys().encryption, &f.owner, 1000, 1600).unwrap();
    assert!(large.to_json().unwrap().len() > 8192);
    assert!(CollectorCredentialEnvelope::new(scope, large).is_err());
}

#[test]
fn encrypted_pair_requires_one_exporter_one_receiver_and_exact_capsule_scope() {
    let mut f = Fixture::new(); let exporter = f.approved();
    let same = || CollectorCredentialEnvelope::new(exporter.clone(), f.capsule(&exporter)).unwrap();
    assert!(CollectorCredentialBundle::from_envelopes(vec![same(), same()]).is_err());
    let exporter_envelope = same();
    let exporter_capsule = f.capsule(&exporter);
    f.scope.target = CollectorCredentialTarget::Receiver {};
    f.scope.endpoint_sha256 = Hash::from_bytes(Sha256::digest(b"http://127.0.0.1:19090/api/v1/write").into());
    let receiver = f.approved();
    let wrong = CollectorCredentialEnvelope::new(receiver.clone(), exporter_capsule).unwrap();
    assert!(wrong.check_metadata_only(&f.pin, &f.cert, &f.base, &f.proof,
        f.proof.body.challenge, &f.manifest, now(1000)).is_err());
    let receiver_envelope = CollectorCredentialEnvelope::new(receiver.clone(), f.capsule(&receiver)).unwrap();
    let batch = CollectorCredentialBundle::from_envelopes(vec![exporter_envelope, receiver_envelope]).unwrap();
    batch.check_metadata_only(&f.pin, &f.cert, &f.base, &f.proof,
        f.proof.body.challenge, &f.manifest, now(1000)).unwrap();
    assert_eq!(batch.into_envelopes().len(), 2);
    assert!(CollectorCredentialBundle::from_envelopes(Vec::new()).is_err());
}

#[test]
fn metadata_preflight_never_substitutes_for_actual_capsule_signature_and_aead() {
    let f = Fixture::new(); let scope = f.approved(); let capsule = f.capsule(&scope);
    let mut value: Value = serde_json::from_slice(&capsule.to_json().unwrap()).unwrap();
    value["signature"] = serde_json::to_value(cc_protocol::Bytes::new(vec![0;64])).unwrap();
    let tampered = ServiceCredentialCapsule::from_json(&serde_json::to_vec(&value).unwrap()).unwrap();
    let envelope = CollectorCredentialEnvelope::new(scope, tampered).unwrap();
    envelope.check_metadata_only(&f.pin, &f.cert, &f.base, &f.proof,
        f.proof.body.challenge, &f.manifest, now(1000)).unwrap();
    let (scope, capsule) = envelope.into_parts();
    assert!(f.open(&scope, &capsule, 1000).is_err());
}

#[test]
fn owner_scope_binds_exact_node_attestation_challenge_and_original_reserved_start() {
    let f = Fixture::new();
    for cause in 0..3 {
        let mut changed = f.scope.clone();
        match cause {
            0 => changed.original_start_nonce = Nonce::from_bytes([18;16]),
            1 => changed.recipient_challenge = Nonce::from_bytes([19;16]),
            _ => changed.recipient_attestation_digest = Hash::from_bytes([20;32]),
        }
        assert!(PreparedCollectorCredentialOwner::prepare(&f.pin, &f.cert, &f.base, &f.proof,
            f.proof.body.challenge, changed.clone(), &f.manifest, now(1000)).is_err());
        let signed = sign(changed, &f.owner);
        let capsule = f.capsule(&signed);
        let envelope = CollectorCredentialEnvelope::new(signed, capsule).unwrap();
        assert!(envelope.check_metadata_only(&f.pin, &f.cert, &f.base, &f.proof,
            f.proof.body.challenge, &f.manifest, now(1000)).is_err());
    }
    let mut wire = serde_json::to_value(&f.scope).unwrap();
    wire.as_object_mut().unwrap().remove("originalStartNonce");
    assert!(serde_json::from_value::<CollectorCredentialScope>(wire).is_err());
    let mut wire = serde_json::to_value(&f.proof.body).unwrap();
    wire.as_object_mut().unwrap().remove("originalStartNonce");
    assert!(serde_json::from_value::<CollectorRecipientAttestation>(wire).is_err());
}

#[test]
fn signed_required_target_set_rejects_partial_bundle_duplicates_and_implicit_public_mode() {
    use cc_agent_node_wire::check_collector_credential_target_set as check_set;
    let f = Fixture::new();
    let scrape = CollectorCredentialTarget::Scrape { target_id: "application".into() };
    let receiver = CollectorCredentialTarget::Receiver {};
    check_set(&f.base.body, &[receiver.clone(), scrape.clone()], &f.manifest).unwrap();
    for actual in [vec![], vec![scrape.clone()], vec![scrape.clone(), scrape.clone()],
        vec![scrape.clone(), receiver.clone(), receiver.clone()]] {
        assert!(check_set(&f.base.body, &actual, &f.manifest).is_err());
    }
    let scope = f.approved();
    let partial = CollectorCredentialBundle::from_envelopes(vec![
        CollectorCredentialEnvelope::new(scope.clone(), f.capsule(&scope)).unwrap()]).unwrap();
    assert!(partial.check_metadata_only(&f.pin, &f.cert, &f.base, &f.proof,
        f.proof.body.challenge, &f.manifest, now(1000)).is_err());
    let mut public = f.base.body.clone(); public.credential_targets.clear();
    assert_ne!(public.digest().unwrap(), f.base.body.digest().unwrap());
    check_set(&public, &[], &f.manifest).unwrap();
    assert!(check_set(&public, &[scrape], &f.manifest).is_err());
    let mut missing = serde_json::to_value(&public).unwrap();
    missing.as_object_mut().unwrap().remove("credentialTargets");
    assert!(serde_json::from_value::<CollectorServiceDelegation>(missing).is_err());
}

#[test]
fn even_a_valid_owner_signed_scope_and_capsule_cannot_exceed_base_target_intent() {
    let mut f = Fixture::new();
    f.base.body.credential_targets = vec![CollectorCredentialTarget::Scrape { target_id: "application".into() }];
    f.base = sign(f.base.body.clone(), &f.owner);
    f.scope.base_delegation_hash = f.base.body.digest().unwrap();
    f.scope.target = CollectorCredentialTarget::Receiver {};
    f.scope.endpoint_sha256 = Hash::from_bytes(Sha256::digest(b"http://127.0.0.1:19090/api/v1/write").into());
    // Actual valid owner signature and existing v1 capsule for the same private
    // recipient; rejection must come from base intent, not bad ciphertext.
    let scope = sign(f.scope.clone(), &f.owner); let capsule = f.capsule(&scope);
    let binding = collector_credential_binding(&scope, &f.pin, now(1000)).unwrap();
    assert!(crate::open_service_credential(&capsule, &binding, f.pin.key.as_bytes(), &f.recipient, 1000).is_ok());
    assert!(f.open(&scope, &capsule, 1000).is_err());
}
