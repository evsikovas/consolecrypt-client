//! FIXTURE ONLY: existing public known owner11; never part of an Agent package.
//! One stdin object (<=16KiB), one stdout object. Closed exporter statements
//! only, no raw-sign command, arbitrary scope/TTL, private-key input or export.
use cc_agent_identity_policy::*;
use ed25519_dalek::{Signer,SigningKey,VerifyingKey,Signature};
use serde::Deserialize;
use serde_json::json;
use std::io::{Read,Write};
use std::time::{SystemTime,UNIX_EPOCH};

#[derive(Deserialize)]
#[serde(tag="kind",rename_all="camelCase",deny_unknown_fields)]
enum Input {
    Bootstrap{now:u64,node:NodeHello,context:Context,invitation:InvitationId},
    Certificate{now:u64,pin:OwnerPin,offer:Signed<Offer>,claim:Signed<Claim>},
    Grant{now:u64,pin:OwnerPin,certificate:Signed<Certificate>,policy:ManagedExporterPolicy,lease:LeaseId},
    Revoke{now:u64,pin:OwnerPin,certificate:Signed<Certificate>},
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct NodeHello{format:u32,agent_id:AgentId,key_epoch:u64,public_key:PublicKey,key_fingerprint:Hash}
fn owner()->SigningKey{SigningKey::from_bytes(&[11;32])}
fn public()->PublicKey{PublicKey::from_bytes(owner().verifying_key().to_bytes())}
fn now(seconds:u64)->Timestamp {
    let observed=SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    assert!(seconds.abs_diff(observed)<=2,"current fixture wall time required");
    Timestamp::new(seconds).unwrap()
}
fn sign<T:Statement>(body:T)->Signed<T> {
    let signature=SignatureBytes::from_bytes(owner().sign(&body.signing_bytes().unwrap()).to_bytes());
    Signed{body,signature}
}
fn verify<T:Statement>(value:&Signed<T>,key:PublicKey) {
    VerifyingKey::from_bytes(key.as_bytes()).unwrap().verify_strict(
        &value.body.signing_bytes().unwrap(),&Signature::from_bytes(value.signature.as_bytes())).unwrap();
}
fn pin(value:&OwnerPin,now:Timestamp) {
    assert_eq!(value.key,public());assert_eq!(value.epoch,1);
    assert!(value.checkpoint<=3&&now<value.expires_at);
}
fn header(value:&OwnerPin,checkpoint:u64)->Header {
    Header{version:1,context:value.context.clone(),owner_epoch:1,checkpoint}
}
fn cert(value:&Signed<Certificate>,p:&OwnerPin,now:Timestamp) {
    pin(p,now);verify(value,public());
    assert_eq!(value.body.scopes,Scopes(vec![Scope::ExportersManage]));
    assert_eq!(value.body.capabilities,Capabilities{version:1,scopes:value.body.scopes.clone()});
    assert_eq!(value.body.header.context,p.context);assert_eq!(value.body.key_epoch,1);
    assert!(value.body.not_before<=now&&now<value.body.expires_at);
}
fn main() {
    assert_eq!(std::env::args().len(),1,"zero arguments required");
    let mut raw=Vec::new();std::io::stdin().take(MAX_WIRE_BYTES as u64+1).read_to_end(&mut raw).unwrap();
    assert!(raw.len()<=MAX_WIRE_BYTES);
    let input:Input=serde_json::from_slice(&raw).unwrap();
    let output=match input {
        Input::Bootstrap{now:seconds,node,context,invitation}=>{
            let n=now(seconds);assert_eq!(node.format,1);assert_eq!(node.key_epoch,1);
            assert_eq!(node.public_key.fingerprint(),node.key_fingerprint);
            let p=OwnerPin{context,key:public(),epoch:1,checkpoint:0,expires_at:n.checked_add(180).unwrap()};
            let token=InvitationToken::from_bytes([33;32]).unwrap();
            let offer=sign(Offer{header:header(&p,1),invitation_id:invitation,agent_id:node.agent_id,
                agent_key:node.public_key,token_hash:token.digest(),scopes:Scopes(vec![Scope::ExportersManage]),
                issued_at:n,expires_at:n.checked_add(60).unwrap()});
            json!({"pin":p,"offer":offer,"token":"21".repeat(32)})
        },
        Input::Certificate{now:seconds,pin:p,offer,claim}=>{
            let n=now(seconds);pin(&p,n);assert_eq!(p.checkpoint,1);verify(&offer,public());
            verify(&claim,offer.body.agent_key);
            assert_eq!(offer.body.header,header(&p,1));assert!(n<offer.body.expires_at);
            assert_eq!(offer.body.scopes,Scopes(vec![Scope::ExportersManage]));
            assert_eq!(claim.body.header,offer.body.header);
            assert_eq!(claim.body.agent_id,offer.body.agent_id);assert_eq!(claim.body.agent_key,offer.body.agent_key);
            assert_eq!(claim.body.invitation_id,offer.body.invitation_id);assert_eq!(claim.body.offer_hash,offer.body.digest().unwrap());
            assert_eq!(claim.body.capabilities,Capabilities{version:1,scopes:offer.body.scopes.clone()});
            let certificate=sign(Certificate{header:header(&p,2),agent_id:offer.body.agent_id,key_epoch:1,
                agent_key:offer.body.agent_key,scopes:offer.body.scopes,capabilities:claim.body.capabilities.clone(),
                origin:CertificateOrigin::Enrollment{invitation_id:offer.body.invitation_id,
                    claim_hash:claim.body.digest().unwrap(),key_fingerprint:offer.body.agent_key.fingerprint()},
                not_before:n,expires_at:n.checked_add(120).unwrap().min(p.expires_at)});
            json!({"certificateDigest":certificate.body.digest().unwrap(),"certificate":certificate})
        },
        Input::Grant{now:seconds,pin:p,certificate,policy,lease:id}=>{
            let n=now(seconds);cert(&certificate,&p,n);assert_eq!(p.checkpoint,2);
            policy.validate().unwrap();assert_eq!(policy.agent_id,certificate.body.agent_id);
            let expiry=n.checked_add(30).unwrap();assert!(expiry<=certificate.body.expires_at&&expiry<=p.expires_at);
            let operation=policy.operation().unwrap();let h=header(&p,2);
            let lease=sign(Lease{header:h.clone(),id,agent_id:policy.agent_id,key_epoch:1,
                certificate_hash:certificate.body.digest().unwrap(),operation:operation.clone(),issued_at:n,expires_at:expiry});
            let freshness=sign(Freshness{header:h,issued_at:n,expires_at:expiry});
            let binding=NodeRequestBinding{certificate,lease,freshness,operation};policy.check_binding(&binding).unwrap();
            json!({"leaseHash":binding.lease.body.digest().unwrap(),"binding":binding})
        },
        Input::Revoke{now:seconds,pin:p,certificate}=>{
            let n=now(seconds);cert(&certificate,&p,n);assert_eq!(p.checkpoint,2);
            let revocation=sign(Revocation{header:header(&p,3),agent_id:certificate.body.agent_id,key_epoch:1,
                certificate_hash:certificate.body.digest().unwrap(),issued_at:n});
            json!({"revocation":revocation})
        },
    };
    let raw=serde_json::to_vec(&output).unwrap();assert!(raw.len()<=MAX_WIRE_BYTES);
    std::io::stdout().write_all(&raw).unwrap();std::io::stdout().write_all(b"\n").unwrap();
}
