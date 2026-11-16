//! Actual private Node/SQLite components; not Native SSH or portable ACK proof.
use super::*;
use ed25519_dalek::{Signer,SigningKey};
use std::os::unix::fs::DirBuilderExt;
struct Root(PathBuf);
impl Root {fn new()->Self{let mut nonce=[0;16];getrandom::fill(&mut nonce).unwrap();
    let path=std::env::temp_dir().join(format!("cc-tunnel-rotation-{}",hex::encode(nonce)));
    fs::DirBuilder::new().mode(0o700).create(&path).unwrap();Self(path)}}
impl Drop for Root {fn drop(&mut self){assert_eq!(self.0.parent(),Some(std::env::temp_dir().as_path()));
    assert!(self.0.file_name().unwrap().to_string_lossy().starts_with("cc-tunnel-rotation-"));
    assert!(!fs::symlink_metadata(&self.0).unwrap().file_type().is_symlink());fs::remove_dir_all(&self.0).unwrap();}}
fn sign<T:Statement>(body:T,key:&SigningKey)->Signed<T>{let signature=SignatureBytes::from_bytes(key.sign(&body.signing_bytes().unwrap()).to_bytes());Signed{body,signature}}
fn frame(n:u8,kind:&str,payload:Value)->Envelope{parse(&canonical(&json!({"format":FORMAT,
    "requestId":format!("55555555-5555-4555-8555-{n:012x}"),"kind":kind,"payload":payload})).unwrap()).unwrap()}
struct Fixture {root:Root,owner:SigningKey,genesis:OwnerPin,previous:Signed<Certificate>,session:Session,clock:Clock}
fn fixture()->Fixture {
    let root=Root::new();let owner=SigningKey::from_bytes(&[11;32]);let key=PublicKey::from_bytes(owner.verifying_key().to_bytes());
    let agent=AgentId::new("44444444-4444-4444-8444-444444444444").unwrap();let dir=root.0.join("node");
    let node=provision_node(&dir,agent).unwrap();let now=Clock::observed().unwrap();
    let context=Context{tenant:Tenant::LocalProfile{id:LocalProfileId::new("11111111-1111-4111-8111-111111111111").unwrap()},
        deployment_id:DeploymentId::new("22222222-2222-4222-8222-222222222222").unwrap()};
    let genesis=OwnerPin{context:context.clone(),key,epoch:1,checkpoint:0,expires_at:now.checked_add(300).unwrap()};
    let header=|cp|Header{version:1,context:context.clone(),owner_epoch:1,checkpoint:cp};
    let store=SqliteJournalStore::create(root.0.join("db"),root.0.join("anchor"),genesis.clone(),Now(now),SqliteStorageBudget::default()).unwrap();
    let token=InvitationToken::from_bytes([33;32]).unwrap();let offer=sign(Offer{header:header(1),
        invitation_id:InvitationId::new("33333333-3333-4333-8333-333333333333").unwrap(),agent_id:agent,agent_key:node.public_key,
        scopes:Scopes(vec![Scope::TunnelOpen]),token_hash:token.digest(),issued_at:now,expires_at:now.checked_add(120).unwrap()},&owner);
    let mut actor=Actor::new(store).unwrap();actor.offer(offer.clone(),Now(now)).unwrap();
    let claim=create_tunnel_node_claim(&dir,agent,&genesis,offer,Now(now)).unwrap();
    actor.consume(&token,claim.clone(),Now(now)).unwrap();
    let claim_hash=claim.body.digest().unwrap();
    let previous=sign(Certificate{header:header(2),agent_id:agent,key_epoch:1,agent_key:node.public_key,
        scopes:Scopes(vec![Scope::TunnelOpen]),capabilities:claim.body.capabilities,
        origin:CertificateOrigin::Enrollment{invitation_id:claim.body.invitation_id,claim_hash,
            key_fingerprint:node.key_fingerprint},not_before:now,expires_at:now.checked_add(100).unwrap()},&owner);
    actor.approve(previous.clone(),Now(now)).unwrap();let store=actor.into_store();let state=store.load().unwrap();
    let original=local(&dir,agent).unwrap();let mut clock=Clock::start().unwrap();clock.cap(genesis.expires_at).unwrap();clock.cap(previous.body.expires_at).unwrap();
    let session=Session{store:Some(store),node_dir:dir,agent,pin:state.owner().clone(),revision:3,original,current:previous.body.clone(),
        prepared:None,previous:None,proofs:None,mode_reconcile:false,stage:0,seen:BTreeSet::new(),effects:false};
    Fixture{root,owner,genesis,previous,session,clock}
}
fn proposed(f:&Fixture)->Signed<Certificate>{let candidate=f.session.prepared.as_ref().unwrap();let mut cert=f.previous.body.clone();
    cert.header.checkpoint=3;cert.key_epoch=2;cert.agent_key=candidate.new_public_key;
    cert.origin=CertificateOrigin::Rotation{previous_certificate:f.previous.body.digest().unwrap(),previous_key_epoch:1};
    cert.not_before=Clock::observed().unwrap();sign(cert,&f.owner)}
fn run_to_apply(f:&mut Fixture)->PreparedRotationProofs {
    f.session.request(frame(1,"probeRotation",json!({})),&mut f.clock).unwrap();
    f.session.request(frame(2,"prepareRotationCandidate",json!({"expectedRev":3,"expectedPin":f.session.pin,
        "certificateDigest":f.previous.body.digest().unwrap()})),&mut f.clock).unwrap();
    let cert=proposed(f);let rotation=f.session.prepared.as_ref().unwrap().rotation_id.clone();
    // Existing Telemetry API remains closed: it cannot sign this Tunnel cert.
    assert!(matches!(sign_prepared_node_rotation(&f.session.node_dir,f.session.agent,&rotation,&f.session.pin,
        PreparedRotationInput{previous_certificate:f.previous.clone(),certificate:cert.clone()},Now(Clock::observed().unwrap())),Err(Error::CapabilityDenied)));
    let mut longer=cert.body.clone();longer.expires_at=longer.expires_at.checked_add(1).unwrap();
    assert!(matches!(sign_prepared_tunnel_node_rotation(&f.session.node_dir,f.session.agent,&rotation,&f.session.pin,
        PreparedRotationInput{previous_certificate:f.previous.clone(),certificate:sign(longer,&f.owner)},Now(Clock::observed().unwrap())),Err(Error::Expired)));
    let value=f.session.request(frame(3,"proveRotation",json!({"expectedRev":3,"expectedPin":f.session.pin,"rotationId":rotation,
        "previousCertificate":f.previous,"certificate":cert})),&mut f.clock).unwrap();
    let proofs:PreparedRotationProofs=serde_json::from_value(value["payload"].clone()).unwrap();
    f.session.request(frame(4,"applyRotation",json!({"expectedRev":3,"expectedPin":f.session.pin,"rotationId":rotation,
        "certificateDigest":proofs.certificate.body.digest().unwrap()})),&mut f.clock).unwrap();proofs
}
#[test]
fn tunnel_rotation_actual_dual_proof_cas_selector_and_selected_private_signer() {
    let mut f=fixture();let old_bytes=fs::read(f.session.node_dir.join("node-identity.key")).unwrap();
    let proofs=run_to_apply(&mut f);let candidate=f.session.prepared.as_ref().unwrap().clone();
    let observed=f.session.request(frame(5,"observeRotation",json!({"expectedRev":4,"expectedPin":f.session.pin,
        "rotationId":candidate.rotation_id,"certificateDigest":proofs.certificate.body.digest().unwrap()})),&mut f.clock).unwrap();
    assert_eq!(observed["payload"]["actorRevision"],4);assert_eq!(observed["payload"]["ownerPin"]["checkpoint"],3);
    assert_eq!(observed["payload"]["node"]["keyEpoch"],2);assert_eq!(f.session.stage,5);
    assert_eq!(proofs.certificate.body.scopes,f.previous.body.scopes);assert_eq!(proofs.certificate.body.capabilities,f.previous.body.capabilities);
    assert!(proofs.certificate.body.expires_at<=f.previous.body.expires_at);
    assert_eq!(fs::read(f.session.node_dir.join("node-identity.key")).unwrap(),old_bytes);
    let guard=crate::node_prepare::existing_read_lock(&f.session.node_dir).unwrap();
    let request=AgentRequest{header:Header{version:1,context:f.genesis.context.clone(),owner_epoch:1,checkpoint:3},
        agent_id:f.session.agent,key_epoch:2,lease_hash:Hash::from_bytes([7;32]),nonce:Nonce::from_bytes([8;16]),issued_at:Clock::observed().unwrap()};
    let signed=crate::node_prepare::sign_selected_request(&guard,&f.session.original.old,request).unwrap();
    signed.verify(candidate.new_public_key).unwrap();assert!(signed.verify(f.session.original.old.public_key).is_err());
    // This is a closed-domain possession proof, not a fabricated Lease/Permit.
}
#[test]
fn tunnel_rotation_lost_reply_exact_reconcile_does_not_cas_sign_or_renew() {
    let mut f=fixture();let proofs=run_to_apply(&mut f);let bytes=serde_json::to_vec(&proofs).unwrap();
    let cert=proofs.certificate.body.clone();let candidate=f.session.prepared.as_ref().unwrap().clone();
    let store=f.session.store.take().unwrap();drop(store);
    let store=SqliteJournalStore::open(f.root.0.join("db"),f.root.0.join("anchor"),f.genesis.clone(),SqliteStorageBudget::default()).unwrap();
    let state=store.load().unwrap();assert_eq!(state.revision(),4);
    let original=local(&f.session.node_dir,f.session.agent).unwrap();
    let mut session=Session{store:Some(store),node_dir:f.session.node_dir.clone(),agent:f.session.agent,pin:state.owner().clone(),
        revision:4,original,current:cert.clone(),prepared:Some(candidate.clone()),previous:None,proofs:None,
        mode_reconcile:true,stage:0,seen:BTreeSet::new(),effects:false};
    session.request(frame(1,"probeRotationReconcile",json!({})),&mut f.clock).unwrap();
    session.request(frame(2,"reconcileRotation",json!({"expectedRev":4,"expectedPin":session.pin,"rotationId":candidate.rotation_id,
        "previousCertificate":f.previous,"proofs":proofs})),&mut f.clock).unwrap();
    session.request(frame(3,"observeRotation",json!({"expectedRev":4,"expectedPin":session.pin,"rotationId":candidate.rotation_id,
        "certificateDigest":cert.digest().unwrap()})),&mut f.clock).unwrap();
    assert_eq!(session.store.as_ref().unwrap().load().unwrap().revision(),4);assert_eq!(session.pin.checkpoint,3);
    assert_eq!(session.current,cert);assert_eq!(serde_json::to_vec(&proofs).unwrap(),bytes);
    assert_eq!(session.prepared.unwrap(),candidate);
}
