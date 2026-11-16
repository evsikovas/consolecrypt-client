//! DRAFT/UNRUN. Actual Actor/CAS/journal and existing capsule crypto, synthetic
//! credentials. Actor-layer original-end inputs do NOT attest an OS join.
use super::*;
use crate::{StoredCollectorServiceCredentialSet, CollectorServiceCredentialAuthorization,
    CollectorServiceCredentialRef, CollectorCredentialTarget, CollectorRecipientAttestation};
use sha2::Digest;
use zeroize::Zeroizing;

fn lifetime_fixture() -> (Fixture, CollectorServicePermit, StoredCollectorServiceCredentialSet, serde_json::Value) {
    let mut f = Fixture::new();
    let catalog = serde_json::json!({"targets":[{"id":"application","url":"http://127.0.0.1:9187/metrics"}],
        "receiver":{"url":"http://127.0.0.1:19090/api/v1/write"}});
    f.policy.body.manifest_sha256 = crate::collector_credential_manifest_sha256(&catalog).unwrap();
    f.policy.body.collector_id = collector_service_resource(f.policy.body.manifest_sha256).unwrap();
    f.policy.body.credential_targets = vec![CollectorCredentialTarget::Scrape {target_id:"application".into()}, CollectorCredentialTarget::Receiver {}];
    f.policy = sign(f.policy.body.clone(), &f.owner);
    f.admit();
    let permit = f.start(3,1000).unwrap();
    let recipient = cc_crypto_core::DeviceSecretKeys::generate().unwrap();
    let mut encoded = Zeroizing::new(b"CCDK\x01".to_vec());
    encoded.extend_from_slice(&[44;32]); encoded.extend_from_slice(&f.owner.to_bytes());
    let owner = cc_crypto_core::DeviceSecretKeys::from_secret_bytes(&encoded).unwrap();
    let proof = sign(CollectorRecipientAttestation {
        header:f.policy.body.header.clone(),agent_id:f.policy.body.agent_id,key_epoch:1,
        certificate_hash:f.policy.body.certificate_hash,service_id:f.policy.body.service_id,
        base_delegation_hash:f.policy.body.digest().unwrap(),manifest_sha256:f.policy.body.manifest_sha256,
        original_start_nonce:permit.start,recipient_id:f.policy.body.service_id,
        recipient_public_key:Hash::from_bytes(recipient.public_keys().encryption),recipient_key_epoch:1,
        recipient_certificate_epoch:1,challenge:Nonce::from_bytes([8;16]),issued_at:time(1000),expires_at:time(1060)},&f.node);
    let refs=vec![CollectorServiceCredentialRef {target:CollectorCredentialTarget::Scrape {target_id:"application".into()},
        endpoint_sha256:Hash::from_bytes(sha2::Sha256::digest(b"http://127.0.0.1:9187/metrics").into()),username:"exporter".into(),
        credential_id:ResourceId::new("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap(),credential_revision:1},
        CollectorServiceCredentialRef {target:CollectorCredentialTarget::Receiver {},
        endpoint_sha256:Hash::from_bytes(sha2::Sha256::digest(b"http://127.0.0.1:19090/api/v1/write").into()),username:"receiver".into(),
        credential_id:ResourceId::new("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb").unwrap(),credential_revision:2}];
    let authorization=sign(CollectorServiceCredentialAuthorization {
        authorization_id:ResourceId::new("cccccccc-cccc-4ccc-8ccc-cccccccccccc").unwrap(),
        owner_device_id:ResourceId::new("dddddddd-dddd-4ddd-8ddd-dddddddddddd").unwrap(),base:f.policy.clone(),
        admission:f.proof(),recipient:proof,credentials:refs,issued_at:time(1000),expires_at:time(87400)},&f.owner);
    let native:cc_agent_node_wire::Signed<cc_agent_node_wire::CollectorServiceCredentialAuthorization>=
        serde_json::from_value(serde_json::to_value(&authorization).unwrap()).unwrap();
    let pin:cc_agent_node_wire::OwnerPin=serde_json::from_value(serde_json::to_value(f.actor.snapshot().unwrap().owner()).unwrap()).unwrap();
    let mut capsules=Vec::new();
    for slot in 0..2 {
        let binding=cc_crypto_core::collector_service_credential_binding(&native,&pin,slot,
            cc_agent_node_wire::Now(cc_agent_node_wire::Timestamp::new(1000).unwrap())).unwrap();
        let material=cc_crypto_core::SelectedServiceCredential::new(binding,Zeroizing::new(b"synthetic original scoped password".to_vec())).unwrap();
        let capsule=cc_crypto_core::seal_service_credential(material,&recipient.public_keys().encryption,&owner,1000,87400).unwrap();
        capsules.push(serde_json::from_slice(&capsule.to_json().unwrap()).unwrap());
    }
    (f,permit,StoredCollectorServiceCredentialSet {authorization,capsules},catalog)
}
#[test]
fn lifetime_whole_pair_commits_once_and_replays_exact_hashes() {
    let (mut f,permit,set,catalog)=lifetime_fixture();
    let expected=set.capsule_hashes().unwrap();
    let receipt=f.actor.admit_collector_service_credential_set(set.clone(),&catalog,now(1000),&mut||sample(1000)).unwrap();
    assert_eq!(receipt.set.capsule_hashes().unwrap(),expected);
    assert_eq!(receipt.admitted_revision,receipt.observed_revision);
    assert!(f.actor.admit_collector_service_credential_set(set,&catalog,now(1000),&mut||sample(1000)).is_err());
    let mut replay=f.restored();
    assert_eq!(replay.collector_service_credential_set_original(&permit,now(1000),&mut||sample(1000)).unwrap().unwrap().capsule_hashes().unwrap(),expected);
    let last=f.memory.0.lock().unwrap().1.last().unwrap().clone();
    assert!(serde_json::to_vec(&last).unwrap().len()<=65536);
}
#[test]
fn partial_tampered_or_foreign_pair_is_refused_before_any_journal_mutation() {
    let (mut f,_permit,set,catalog)=lifetime_fixture();
    let before=f.actor.snapshot().unwrap().revision();
    let mut partial=set.clone();partial.capsules.pop();
    assert!(f.actor.admit_collector_service_credential_set(partial,&catalog,now(1000),&mut||sample(1000)).is_err());
    let mut bad=set.clone();
    let mut signature=bad.capsules[0]["signature"].as_str().expect("existing capsule Bytes encoding").to_owned();
    let replacement=if signature.starts_with('A'){"B"}else{"A"};signature.replace_range(0..1,replacement);
    bad.capsules[0]["signature"]=serde_json::Value::String(signature);
    assert!(f.actor.admit_collector_service_credential_set(bad,&catalog,now(1000),&mut||sample(1000)).is_err());
    let mut swapped=set;swapped.capsules.swap(0,1);
    assert!(f.actor.admit_collector_service_credential_set(swapped,&catalog,now(1000),&mut||sample(1000)).is_err());
    assert_eq!(f.actor.snapshot().unwrap().revision(),before);
}
#[test]
fn durable_choice_refuses_both_directions_of_v1_lifetime_race() {
    let (mut a,permit,set,catalog)=lifetime_fixture();
    a.actor.select_collector_single_start(permit.service_id,permit.delegation_hash,permit.start,now(1000),&mut||sample(1000)).unwrap();
    assert!(a.actor.admit_collector_service_credential_set(set,&catalog,now(1000),&mut||sample(1000)).is_err());
    let (mut b,permit,set,catalog)=lifetime_fixture();
    b.actor.admit_collector_service_credential_set(set,&catalog,now(1000),&mut||sample(1000)).unwrap();
    assert!(b.actor.select_collector_single_start(permit.service_id,permit.delegation_hash,permit.start,now(1000),&mut||sample(1000)).is_err());
}
#[test]
fn controlled_actor_end_preserves_whole_set_count32_and_original_end() {
    let (mut f,permit,set,catalog)=lifetime_fixture();
    f.actor.admit_collector_service_credential_set(set,&catalog,now(1000),&mut||sample(1000)).unwrap();
    let original_clock=f.proof().body.clock;
    f.ended(&permit,1000); // Existing Actor-test end input, NOT actual OS cleanup.
    f.actor=Actor::new(f.memory.clone()).unwrap();
    for n in 4..=34 {
        let permit=f.start(n,1001).unwrap();
        let stored=f.actor.collector_service_credential_set_original(&permit,now(1001),&mut||sample(1001)).unwrap().unwrap();
        assert_eq!(stored.authorization.body.admission.body.clock,original_clock);
        assert_eq!(stored.authorization.body.expires_at,time(87400));
        f.ended(&permit,1001);
    }
    assert!(matches!(f.start(35,1001),Err(Error::Quota)));
    let state=f.actor.snapshot().unwrap();
    let record=state.collector_services.get(&f.policy.body.service_id).unwrap();
    assert_eq!(record.starts,32);assert_eq!(record.clock,original_clock);
}
#[test]
fn lost_original_owner_remains_unknown_despite_persisted_valid_material() {
    let (mut f,_permit,set,catalog)=lifetime_fixture();
    f.actor.admit_collector_service_credential_set(set,&catalog,now(1000),&mut||sample(1000)).unwrap();
    let mut replay=f.restored();
    assert!(matches!(replay.next_collector_service(f.policy.body.agent_id,f.policy.body.manifest_sha256,
        now(1001),&mut||sample(1001)),Err(Error::UncertainCommit)));
    assert!(replay.reserve_collector_service_start(f.policy.body.service_id,Nonce::from_bytes([42;16]),
        now(1001),&mut||sample(1001)).is_err());
}
#[derive(Clone)]
struct LostAck(Memory);
impl CompareAndSwap for LostAck {
    fn load(&self)->Result<State>{self.0.load()}
    fn compare_and_swap(&mut self,expected:u64,next:State)->Result<()>{
        self.0.compare_and_swap(expected,next)?;Err(Error::UncertainCommit)
    }
}
#[test]
fn cas_failure_or_lost_ack_never_constructs_admission_readback() {
    let (mut f,permit,set,catalog)=lifetime_fixture();
    f.memory.0.lock().unwrap().2=true;
    assert!(f.actor.admit_collector_service_credential_set(set.clone(),&catalog,now(1000),&mut||sample(1000)).is_err());
    f.memory.0.lock().unwrap().2=false;
    assert!(f.actor.collector_service_credential_set_original(&permit,now(1000),&mut||sample(1000)).unwrap().is_none());
    let mut writer=Actor::new(LostAck(f.memory.clone())).unwrap();
    assert!(matches!(writer.admit_collector_service_credential_set(set,&catalog,now(1000),&mut||sample(1000)),Err(Error::UncertainCommit)));
    let mut replay=f.restored();
    assert!(replay.collector_service_credential_set_original(&permit,now(1000),&mut||sample(1000)).unwrap().is_some());
}
#[test]
fn changed_os_boot_and_original_expiry_refuse_reopen() {
    let (mut f,permit,set,catalog)=lifetime_fixture();
    f.actor.admit_collector_service_credential_set(set,&catalog,now(1000),&mut||sample(1000)).unwrap();
    assert!(f.actor.collector_service_credential_set_original(&permit,now(87400),&mut||sample(87400)).is_err());
    let mut replay=f.restored();
    assert!(replay.collector_service_credential_set_original(&permit,now(1001),&mut||
        CollectorServiceClockObservation::journal(ResourceId::new("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee").unwrap(),2_000_000_000,time(1001))).is_err());
}

#[test]
fn actual_signed_readback_receipt_binds_order_authorization_and_revision() {
    let (mut f,_permit,set,catalog)=lifetime_fixture();
    let readback=f.actor.admit_collector_service_credential_set(set.clone(),&catalog,now(1000),&mut||sample(1000)).unwrap();
    let certificate=sign(readback.certificate.clone(),&f.owner);
    let hashes=set.capsule_hashes().unwrap();
    let body=crate::CollectorServiceCredentialSetReceipt {
        header:f.policy.body.header.clone(),agent_id:f.policy.body.agent_id,key_epoch:1,service_id:f.policy.body.service_id,
        base_delegation_hash:f.policy.body.digest().unwrap(),authorization_digest:set.authorization.body.digest().unwrap(),
        ordered_capsule_hashes:hashes.clone(),original_start_nonce:readback.original_start,
        admitted_revision:readback.admitted_revision,actor_revision:readback.observed_revision,issued_at:time(1000),expires_at:time(1060)};
    let receipt=sign(body.clone(),&f.node);
    assert!(crate::verify_collector_service_set_receipt(&readback.pin,&certificate,&set.authorization,&receipt,&hashes,readback.admitted_revision,now(1000)).is_ok());
    // A correctly Node-signed but WRONG ordered set still cannot complete intent.
    let mut bad=body.clone();bad.ordered_capsule_hashes.swap(0,1);
    assert!(crate::verify_collector_service_set_receipt(&readback.pin,&certificate,&set.authorization,&sign(bad,&f.node),&hashes,readback.admitted_revision,now(1000)).is_err());
    let mut bad=body;bad.authorization_digest=Hash::from_bytes([42;32]);
    assert!(crate::verify_collector_service_set_receipt(&readback.pin,&certificate,&set.authorization,&sign(bad,&f.node),&hashes,readback.admitted_revision,now(1000)).is_err());
    assert!(crate::verify_collector_service_set_receipt(&readback.pin,&certificate,&set.authorization,&receipt,&hashes,readback.admitted_revision+1,now(1000)).is_err());
}
