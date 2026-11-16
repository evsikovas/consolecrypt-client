// DRAFT_UNCOMPILED_UNRUN. Real Node custody and Actor, not a service/SSH proof.
use super::*;
include!("disk_cases.rs");
use ed25519_dalek::{Signer,SigningKey};
use std::sync::{Arc,Mutex};
struct Directory(PathBuf);
impl Directory {fn new()->Self{let mut b=[0u8;16];getrandom::fill(&mut b).unwrap();
    let p=std::env::temp_dir().join(format!("cc-metrics-{}",hex::encode(b)));
    fs::DirBuilder::new().mode(0o700).create(&p).unwrap();Self(p)}}
impl Drop for Directory {fn drop(&mut self){if self.0.parent()==Some(std::env::temp_dir().as_path())
    &&self.0.file_name().unwrap().to_string_lossy().starts_with("cc-metrics-")
    &&!fs::symlink_metadata(&self.0).unwrap().file_type().is_symlink(){fs::remove_dir_all(&self.0).unwrap();}}}
#[derive(Clone)]struct Memory(Arc<Mutex<State>>);
impl CompareAndSwap for Memory {
    fn load(&self)->Result<State>{Ok(self.0.lock().unwrap().clone())}
    fn compare_and_swap(&mut self,r:u64,n:State)->Result<()>{let mut s=self.0.lock().unwrap();if s.revision()!=r{return Err(Error::Conflict)}*s=n;Ok(())}
}
fn signed<T:Statement>(body:T,key:&SigningKey)->Signed<T>{let signature=SignatureBytes::from_bytes(key.sign(&body.signing_bytes().unwrap()).to_bytes());Signed{body,signature}}
fn stamp(n:u64)->Timestamp{Timestamp::new(n).unwrap()}
fn manifest()->Value{json!({"schema_version":1,"mode":"remote_write","targets":[{"id":"app","url":"http://127.0.0.1:9100/metrics",
    "kind":"application","sample_limit":1000,"series_limit":1000,"response_bytes":4096,"label_limit":32}],
    "labels":{"cc_host_id":"11111111-1111-4111-8111-111111111111","site":"local","environment":"test"},
    "scrape_interval_seconds":1,"receiver":{"url":"http://127.0.0.1:9090/api/v1/write"},"buffer":{"max_bytes":33554432,"max_age_seconds":300}})}
struct Fixture {directory:Directory,owner:SigningKey,node:PublicNodeIdentity,actor:Actor<Memory>,cert:Signed<Certificate>,now:u64,id:ResourceId}
impl Fixture {
 fn new()->Self {
    let directory=Directory::new();let owner=SigningKey::from_bytes(&[11;32]);let now=Clock::observed().unwrap().seconds();
    let context:Context=serde_json::from_value(json!({"tenant":{"kind":"localProfile","id":"11111111-1111-4111-8111-111111111111"},
        "deploymentId":"22222222-2222-4222-8222-222222222222"})).unwrap();
    let pin=OwnerPin {context:context.clone(),key:PublicKey::from_bytes(owner.verifying_key().to_bytes()),epoch:1,checkpoint:0,expires_at:stamp(now+1000)};
    let agent=AgentId::new("44444444-4444-4444-8444-444444444444").unwrap();let node=provision_node(&directory.0.join("node"),agent).unwrap();
    let mut actor=Actor::new(Memory(Arc::new(Mutex::new(State::bootstrap(pin.clone(),Now(stamp(now))).unwrap())))).unwrap();
    let token=InvitationToken::from_bytes([33;32]).unwrap();let offer=signed(Offer {header:Header{version:1,context:context.clone(),owner_epoch:1,checkpoint:1},
        invitation_id:InvitationId::new("33333333-3333-4333-8333-333333333333").unwrap(),agent_id:agent,agent_key:node.public_key,
        token_hash:token.digest(),scopes:Scopes(vec![Scope::MetricsCollect]),issued_at:stamp(now),expires_at:stamp(now+120)},&owner);
    assert!(create_tunnel_node_claim(&directory.0.join("node"),agent,&pin,offer.clone(),Now(stamp(now))).is_err());
    actor.offer(offer.clone(),Now(stamp(now))).unwrap();
    let claim=create_metrics_node_claim(&directory.0.join("node"),agent,&pin,offer.clone(),Now(stamp(now))).unwrap();
    claim.verify(node.public_key).unwrap();assert_eq!(claim.body.capabilities.scopes,Scopes(vec![Scope::MetricsCollect]));
    let claim_hash=claim.body.digest().unwrap();actor.consume(&token,claim,Now(stamp(now))).unwrap();
    let cert=signed(Certificate {header:Header{version:1,context,owner_epoch:1,checkpoint:2},agent_id:agent,key_epoch:1,agent_key:node.public_key,
        scopes:Scopes(vec![Scope::MetricsCollect]),capabilities:Capabilities{version:1,scopes:Scopes(vec![Scope::MetricsCollect])},
        origin:CertificateOrigin::Enrollment{invitation_id:offer.body.invitation_id,claim_hash,key_fingerprint:node.key_fingerprint},
        not_before:stamp(now),expires_at:stamp(now+600)},&owner);
    actor.approve(cert.clone(),Now(stamp(now))).unwrap();
    assert_eq!(actor.snapshot().unwrap().revision(),3);assert_eq!(actor.snapshot().unwrap().owner().checkpoint,2);
    Self {directory,owner,node,actor,cert,now,id:ResourceId::new("66666666-6666-4666-8666-666666666666").unwrap()}
 }
 fn binding(&self)->NodeRequestBinding {let pin=self.actor.snapshot().unwrap().owner().clone();let operation=Operation::MetricsCollect{collector:self.id};
    let lease=signed(Lease{header:Header{version:1,context:pin.context.clone(),owner_epoch:pin.epoch,checkpoint:pin.checkpoint},
        id:LeaseId::new("77777777-7777-4777-8777-777777777777").unwrap(),agent_id:self.node.agent_id,key_epoch:self.node.key_epoch,
        certificate_hash:self.cert.body.digest().unwrap(),operation:operation.clone(),issued_at:stamp(self.now),expires_at:stamp(self.now+30)},&self.owner);
    let freshness=signed(Freshness{header:lease.body.header.clone(),issued_at:stamp(self.now),expires_at:stamp(self.now+30)},&self.owner);
    NodeRequestBinding {certificate:self.cert.clone(),lease,freshness,operation}
 }
}
#[test]
fn metrics_real_node_claim_and_actor_admission_are_exact_resource_one_use() {
    let mut f=Fixture::new();let b=f.binding();let pin=f.actor.snapshot().unwrap().owner().clone();
    let wrong=ResourceId::new("88888888-8888-4888-8888-888888888888").unwrap();
    assert!(create_metrics_node_request(&f.directory.0.join("node"),f.node.agent_id,&pin,wrong,b.clone(),Now(stamp(f.now))).is_err());
    let proof=create_metrics_node_request(&f.directory.0.join("node"),f.node.agent_id,&pin,f.id,b.clone(),Now(stamp(f.now))).unwrap();
    let permit=f.actor.admit(b.lease.clone(),proof,Some(&b.freshness),&b.operation,Now(stamp(f.now))).unwrap();
    assert_eq!(permit.operation(),&b.operation);f.actor.check_permit(&permit,Now(stamp(f.now))).unwrap();
    assert_eq!(f.actor.snapshot().unwrap().revision(),4);
    let fresh_proof=create_metrics_node_request(&f.directory.0.join("node"),f.node.agent_id,&pin,f.id,b.clone(),Now(stamp(f.now))).unwrap();
    assert_eq!(f.actor.admit(b.lease,fresh_proof,Some(&b.freshness),&b.operation,Now(stamp(f.now))).unwrap_err(),Error::Replay);
}
#[test]
fn metrics_actual_revoke_invalidates_retained_collection_permit() {
    let mut f=Fixture::new();let b=f.binding();let pin=f.actor.snapshot().unwrap().owner().clone();
    let proof=create_metrics_node_request(&f.directory.0.join("node"),f.node.agent_id,&pin,f.id,b.clone(),Now(stamp(f.now))).unwrap();
    let permit=f.actor.admit(b.lease,proof,Some(&b.freshness),&b.operation,Now(stamp(f.now))).unwrap();
    let rev=signed(Revocation{header:Header{checkpoint:3,..f.cert.body.header.clone()},agent_id:f.node.agent_id,
        key_epoch:1,certificate_hash:f.cert.body.digest().unwrap(),issued_at:stamp(f.now)},&f.owner);
    f.actor.revoke(rev,Now(stamp(f.now))).unwrap();
    assert!(f.actor.check_permit(&permit,Now(stamp(f.now))).is_err());assert!(f.actor.snapshot().unwrap().revoked(f.node.agent_id));
}
#[test]
fn metrics_manifest_bounds_and_start_schema_never_supply_authority_paths_clock_or_bool() {
    let good=manifest();validate_manifest(&good).unwrap();
    let mut wrong=good.clone();wrong["buffer"]["max_bytes"]=json!(536870912);assert!(validate_manifest(&wrong).is_err());
    let mut wrong=good;wrong["receiver"]["url"]=json!("http://127.0.0.1:9090/api/v1/write?token=x");assert!(validate_manifest(&wrong).is_err());
    assert!(decode::<StartCollector>(json!({"now":1,"path":"/tmp/vmagent","authorized":true})).is_err());
    assert!(decode::<StopCollector>(json!({"collectorId":"66666666-6666-4666-8666-666666666666","leaseHash":"00".repeat(32),"drained":true})).is_err());
    assert!(parse(b"{\"a\":1,\"a\":1}\n").is_err());
}

#[test]
fn metrics_revocation_session_needs_no_catalog_adapter_or_tls_prerequisite() {
    let f=Fixture::new();let store=f.actor.into_store();let state=store.load().unwrap();
    let mut session=Session {store:Some(store),node_dir:f.directory.0.join("node"),agent:f.node.agent_id,node:f.node.clone(),
        phase:Phase::New,seen:BTreeSet::new(),expected_pin:state.owner().clone(),expected_rev:state.revision(),
        claim:None,certificate:None,offer_expires:None,effects:false,catalog:None};
    let mut clock=Clock::start().unwrap();
    let make=|id:u8,kind:&str,payload:Value|parse(&canonical(&json!({"format":FORMAT,
        "requestId":format!("99999999-9999-4999-8999-{id:012x}"),"kind":kind,"payload":payload})).unwrap()).unwrap();
    let probe=session.request(make(1,"probeMetricsRevocation",json!({})),&mut clock).unwrap();
    assert!(probe["payload"]["collector"].is_null());
    assert_eq!(probe["payload"]["advertisedScopes"],json!([Scope::MetricsCollect]));
    let revoke=signed(Revocation {header:Header{checkpoint:3,..f.cert.body.header.clone()},agent_id:f.node.agent_id,
        key_epoch:f.node.key_epoch,certificate_hash:f.cert.body.digest().unwrap(),issued_at:stamp(f.now)},&f.owner);
    let reply=session.request(make(2,"revokeCertificate",json!({"expectedRev":3,"expectedPin":state.owner(),
        "revocation":revoke})),&mut clock).unwrap();
    assert_eq!(reply["payload"]["revoked"],json!(true));assert_eq!(reply["payload"]["actorRevision"],json!(4));
}

#[test]
fn metrics_changed_manifest_cannot_retarget_an_existing_collector_resource() {
    let first=manifest();validate_manifest(&first).unwrap();
    let first_hash=digest(&json_body(&first).unwrap());let old=collector_id(first_hash).unwrap();
    check_collector_id(old,first_hash).unwrap();
    let mut second=first;second["receiver"]["url"]=json!("http://127.0.0.1:9091/api/v1/write");
    validate_manifest(&second).unwrap();let second_hash=digest(&json_body(&second).unwrap());
    assert_ne!(first_hash,second_hash);assert_eq!(check_collector_id(old,second_hash),Err(Error::WrongClaim));
    let new=collector_id(second_hash).unwrap();assert_ne!(old,new);check_collector_id(new,second_hash).unwrap();
}
