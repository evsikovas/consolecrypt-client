// DRAFT_UNCOMPILED_UNRUN. Synthetic test keys/hashes never reach runtime Catalog.
// These use actual Node custody and Actor; no child/systemd/SSH proof is claimed.
use super::*;
use ed25519_dalek::{Signer,SigningKey};
use std::sync::{Arc,Mutex};
use std::os::unix::fs::DirBuilderExt;
struct Directory(PathBuf);
impl Directory {fn new()->Self{let mut b=[0u8;16];getrandom::fill(&mut b).unwrap();
    let p=std::env::temp_dir().join(format!("cc-exporter-{}",hex::encode(b)));
    fs::DirBuilder::new().mode(0o700).create(&p).unwrap();Self(p)}}
impl Drop for Directory {fn drop(&mut self){if self.0.parent()==Some(std::env::temp_dir().as_path())
    &&self.0.file_name().unwrap().to_string_lossy().starts_with("cc-exporter-")
    &&!fs::symlink_metadata(&self.0).unwrap().file_type().is_symlink(){fs::remove_dir_all(&self.0).unwrap();}}}
#[derive(Clone)]struct Memory(Arc<Mutex<State>>);
impl CompareAndSwap for Memory {
    fn load(&self)->Result<State>{Ok(self.0.lock().unwrap().clone())}
    fn compare_and_swap(&mut self,r:u64,n:State)->Result<()>{let mut s=self.0.lock().unwrap();if s.revision()!=r{return Err(Error::Conflict)}*s=n;Ok(())}
}
fn signed<T:Statement>(body:T,key:&SigningKey)->Signed<T>{let signature=SignatureBytes::from_bytes(key.sign(&body.signing_bytes().unwrap()).to_bytes());Signed{body,signature}}
fn stamp(n:u64)->Timestamp{Timestamp::new(n).unwrap()}
struct Fixture {directory:Directory,owner:SigningKey,node:PublicNodeIdentity,actor:Actor<Memory>,cert:Signed<Certificate>,now:u64,policy:ManagedExporterPolicy}
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
        token_hash:token.digest(),scopes:Scopes(vec![Scope::ExportersManage]),issued_at:stamp(now),expires_at:stamp(now+120)},&owner);
    assert!(create_tunnel_node_claim(&directory.0.join("node"),agent,&pin,offer.clone(),Now(stamp(now))).is_err());
    actor.offer(offer.clone(),Now(stamp(now))).unwrap();
    let claim=create_exporter_node_claim(&directory.0.join("node"),agent,&pin,offer.clone(),Now(stamp(now))).unwrap();
    claim.verify(node.public_key).unwrap();assert_eq!(claim.body.capabilities.scopes,Scopes(vec![Scope::ExportersManage]));
    let claim_hash=claim.body.digest().unwrap();actor.consume(&token,claim,Now(stamp(now))).unwrap();
    let cert=signed(Certificate {header:Header{version:1,context,owner_epoch:1,checkpoint:2},agent_id:agent,key_epoch:1,agent_key:node.public_key,
        scopes:Scopes(vec![Scope::ExportersManage]),capabilities:Capabilities{version:1,scopes:Scopes(vec![Scope::ExportersManage])},
        origin:CertificateOrigin::Enrollment{invitation_id:offer.body.invitation_id,claim_hash,key_fingerprint:node.key_fingerprint},
        not_before:stamp(now),expires_at:stamp(now+600)},&owner);
    actor.approve(cert.clone(),Now(stamp(now))).unwrap();
    assert_eq!(actor.snapshot().unwrap().revision(),3);assert_eq!(actor.snapshot().unwrap().owner().checkpoint,2);
    Self {directory,owner,node,actor,cert,now,policy:test_policy(agent)}
 }
 fn binding(&self)->NodeRequestBinding {let pin=self.actor.snapshot().unwrap().owner().clone();let operation=self.policy.operation().unwrap();
    let lease=signed(Lease{header:Header{version:1,context:pin.context.clone(),owner_epoch:pin.epoch,checkpoint:pin.checkpoint},
        id:LeaseId::new("77777777-7777-4777-8777-777777777777").unwrap(),agent_id:self.node.agent_id,key_epoch:self.node.key_epoch,
        certificate_hash:self.cert.body.digest().unwrap(),operation:operation.clone(),issued_at:stamp(self.now),expires_at:stamp(self.now+30)},&self.owner);
    let freshness=signed(Freshness{header:lease.body.header.clone(),issued_at:stamp(self.now),expires_at:stamp(self.now+30)},&self.owner);
    NodeRequestBinding {certificate:self.cert.clone(),lease,freshness,operation}
 }
}
#[test]
fn exporter_real_node_claim_and_actor_admission_are_manifest_bound_one_use() {
    let mut f=Fixture::new();let b=f.binding();let pin=f.actor.snapshot().unwrap().owner().clone();
    let mut wrong=f.policy.clone();wrong.max_seconds=299;
    assert!(create_exporter_node_request(&f.directory.0.join("node"),f.node.agent_id,&pin,&wrong,b.clone(),Now(stamp(f.now))).is_err());
    let proof=create_exporter_node_request(&f.directory.0.join("node"),f.node.agent_id,&pin,&f.policy,b.clone(),Now(stamp(f.now))).unwrap();
    let permit=f.actor.admit(b.lease.clone(),proof,Some(&b.freshness),&b.operation,Now(stamp(f.now))).unwrap();
    assert_eq!(permit.operation(),&b.operation);f.actor.check_permit(&permit,Now(stamp(f.now))).unwrap();
    assert_eq!(f.actor.snapshot().unwrap().revision(),4);
    let fresh_proof=create_exporter_node_request(&f.directory.0.join("node"),f.node.agent_id,&pin,&f.policy,b.clone(),Now(stamp(f.now))).unwrap();
    assert_eq!(f.actor.admit(b.lease,fresh_proof,Some(&b.freshness),&b.operation,Now(stamp(f.now))).unwrap_err(),Error::Replay);
}
#[test]
fn exporter_actual_revoke_invalidates_original_permit() {
    let mut f=Fixture::new();let b=f.binding();let pin=f.actor.snapshot().unwrap().owner().clone();
    let proof=create_exporter_node_request(&f.directory.0.join("node"),f.node.agent_id,&pin,&f.policy,b.clone(),Now(stamp(f.now))).unwrap();
    let permit=f.actor.admit(b.lease,proof,Some(&b.freshness),&b.operation,Now(stamp(f.now))).unwrap();
    let rev=signed(Revocation{header:Header{checkpoint:3,..f.cert.body.header.clone()},agent_id:f.node.agent_id,
        key_epoch:1,certificate_hash:f.cert.body.digest().unwrap(),issued_at:stamp(f.now)},&f.owner);
    f.actor.revoke(rev,Now(stamp(f.now))).unwrap();
    assert!(f.actor.check_permit(&permit,Now(stamp(f.now))).is_err());assert!(f.actor.snapshot().unwrap().revoked(f.node.agent_id));
}

fn test_policy(agent:AgentId)->ManagedExporterPolicy {
    serde_json::from_value(json!({"format":"cc-agent-managed-node-exporter/v1",
      "hostId":"11111111-1111-4111-8111-111111111111","agentId":agent,
      "artifact":{"name":"node_exporter","version":"v1.12.1","platform":"linux-x86_64",
        "archiveSha256":"b51d8a76aa2a9156a55d501aca6276fae09e262259a5e4e831d2c2222f084e63",
        "binarySha256":"01".repeat(32),"binaryBytes":1024,"noticeInventorySha256":"02".repeat(32)},
      "bindIp":"127.0.0.1","bindPort":19100,"collectors":["cpu","meminfo"],"maxSeconds":300})).unwrap()
}
#[test]
fn exporter_proof_rejects_changed_host_artifact_collectors_agent_and_scope() {
    let f=Fixture::new();let b=f.binding();let pin=f.actor.snapshot().unwrap().owner().clone();
    let mut policies=Vec::new();
    let mut p=f.policy.clone();p.host_id="88888888-8888-4888-8888-888888888888".into();policies.push(p);
    let mut p=f.policy.clone();p.agent_id=AgentId::new("88888888-8888-4888-8888-888888888888").unwrap();policies.push(p);
    let mut p=f.policy.clone();p.artifact.binary_sha256=Hash::from_bytes([3;32]);policies.push(p);
    let mut p=f.policy.clone();p.artifact.notice_inventory_sha256=Hash::from_bytes([4;32]);policies.push(p);
    let mut p=f.policy.clone();p.collectors.push("filesystem".into());policies.push(p);
    let mut p=f.policy.clone();p.bind_ip="0.0.0.0".into();policies.push(p);
    for policy in policies {
      assert!(create_exporter_node_request(&f.directory.0.join("node"),f.node.agent_id,&pin,&policy,b.clone(),Now(stamp(f.now))).is_err());
    }
    let mut metrics=b.clone();metrics.certificate.body.scopes=Scopes(vec![Scope::MetricsCollect]);
    metrics.certificate.body.capabilities.scopes=metrics.certificate.body.scopes.clone();
    assert!(f.policy.check_binding(&metrics).is_err());
    assert!(create_metrics_node_request(&f.directory.0.join("node"),f.node.agent_id,&pin,
      ResourceId::new("66666666-6666-4666-8666-666666666666").unwrap(),b,Now(stamp(f.now))).is_err());
}
#[test]
fn exporter_original_permit_expires_and_bad_owner_signature_never_admits() {
    let mut f=Fixture::new();let b=f.binding();let pin=f.actor.snapshot().unwrap().owner().clone();
    let mut tampered=b.clone();tampered.lease.signature=SignatureBytes::from_bytes([0;64]);
    assert!(create_exporter_node_request(&f.directory.0.join("node"),f.node.agent_id,&pin,&f.policy,tampered,Now(stamp(f.now))).is_err());
    let proof=create_exporter_node_request(&f.directory.0.join("node"),f.node.agent_id,&pin,&f.policy,b.clone(),Now(stamp(f.now))).unwrap();
    let permit=f.actor.admit(b.lease,proof,Some(&b.freshness),&b.operation,Now(stamp(f.now))).unwrap();
    assert!(f.actor.check_permit(&permit,Now(stamp(f.now+30))).is_err());
}
#[test]
fn exporter_start_and_controls_accept_no_path_clock_pid_or_success_boolean() {
    assert!(decode::<Start>(json!({"approved":true,"path":"/tmp/a","pid":1,"now":1})).is_err());
    assert!(decode::<Control>(json!({"manifestSha256":"01".repeat(32),"leaseHash":"02".repeat(32),"joined":true})).is_err());
    assert!(parse(b"{\"a\":1,\"a\":1}\n").is_err());
}

#[test]
fn exporter_frozen_wall_after_real_admission_cannot_restart_original_deadline() {
    let mut f=Fixture::new();let b=f.binding();let pin=f.actor.snapshot().unwrap().owner().clone();
    let captured=Instant::now();let wall=Duration::from_secs(f.now)+Duration::from_millis(750);
    let mut clock=Clock{end:captured+Duration::from_secs(300),last:stamp(f.now),expiry:None};
    let deadline=clock.cap_exporter_at(b.lease.body.expires_at,captured,wall).unwrap();
    assert_eq!(deadline,captured+Duration::from_millis(29250));
    let proof=create_exporter_node_request(&f.directory.0.join("node"),f.node.agent_id,&pin,&f.policy,b.clone(),Now(stamp(f.now))).unwrap();
    let permit=f.actor.admit(b.lease,proof,Some(&b.freshness),&b.operation,Now(stamp(f.now))).unwrap();
    // Real Actor admission/check can succeed at the same wall second despite
    // admission I/O having consumed the entire original monotonic window.
    f.actor.check_permit(&permit,Now(stamp(f.now))).unwrap();
    let after_admission=captured+Duration::from_secs(30);
    assert_eq!(runtime::original_remaining(deadline,after_admission),Err(Error::Expired));
    assert_eq!(clock.end,deadline);
    // Even another frozen-wall observation cannot replenish Clock or spawn.
    assert_eq!(clock.cap_exporter_at(stamp(f.now+30),after_admission,wall),Err(Error::Expired));
    assert_eq!(clock.end,deadline);
}

#[test]
fn exporter_wall_rollback_after_actual_permit_check_cannot_extend_pidfd_budget() {
    let mut f=Fixture::new();let b=f.binding();let pin=f.actor.snapshot().unwrap().owner().clone();
    let captured=Instant::now();let mut clock=Clock{end:captured+Duration::from_secs(300),last:stamp(f.now),expiry:None};
    let deadline=clock.cap_exporter_at(b.lease.body.expires_at,captured,Duration::from_secs(f.now)).unwrap();
    let proof=create_exporter_node_request(&f.directory.0.join("node"),f.node.agent_id,&pin,&f.policy,b.clone(),Now(stamp(f.now))).unwrap();
    let permit=f.actor.admit(b.lease,proof,Some(&b.freshness),&b.operation,Now(stamp(f.now))).unwrap();
    f.actor.check_permit(&permit,Now(stamp(f.now))).unwrap();
    let after_check=captured+Duration::from_secs(20);
    assert_eq!(clock.cap_exporter_at(stamp(f.now+30),after_check,Duration::from_secs(f.now-10)),Err(Error::Expired));
    assert_eq!(clock.end,deadline);
    assert_eq!(runtime::original_remaining(deadline,after_check).unwrap(),Duration::from_secs(10));
    assert_eq!(runtime::original_remaining(deadline,captured+Duration::from_secs(30)),Err(Error::Expired));
}

#[test]
fn exporter_deadline_preserves_earlier_original_clock_and_exact_expiry_boundary() {
    let captured=Instant::now();let earlier=captured+Duration::from_secs(2);
    let mut clock=Clock{end:earlier,last:stamp(1000),expiry:Some(stamp(1010))};
    let deadline=clock.cap_exporter_at(stamp(1030),captured,Duration::from_millis(1000250)).unwrap();
    assert_eq!(deadline,earlier);assert_eq!(clock.expiry,Some(stamp(1010)));
    assert_eq!(runtime::original_remaining(deadline,earlier),Err(Error::Expired));
    let mut expired=Clock{end:captured+Duration::from_secs(300),last:stamp(1000),expiry:None};
    assert_eq!(expired.cap_exporter_at(stamp(1000),captured,Duration::from_secs(1000)),Err(Error::Expired));
}
