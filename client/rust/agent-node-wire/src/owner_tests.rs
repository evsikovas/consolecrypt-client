//! Draft public-crypto tests, no Native UI or Actor admission implied.
use super::*;
use ed25519_dalek::{Signer, SigningKey};
use serde_json::json;
fn sign<T: Statement>(body: T, key: &SigningKey) -> Signed<T> {
    let signature = SignatureBytes::from_bytes(key.sign(&body.signing_bytes().unwrap()).to_bytes());
    Signed { body, signature }
}
fn finish(input: PreparedTunnelOwner, key: &SigningKey) -> Result<TunnelOwnerSigned> {
    let sig = input
        .signing_transcripts()?
        .iter()
        .map(|v| key.sign(v).to_bytes())
        .collect();
    input.finish(sig)
}
fn policy() -> TunnelRoutePolicy {
    serde_json::from_str(include_str!("tunnel_policy_vector.json")).unwrap()
}
fn time(v: u64) -> Timestamp {
    Timestamp::new(v).unwrap()
}
fn header(p: &TunnelRoutePolicy, checkpoint: u64) -> Header {
    Header {
        version: 1,
        context: p.context.clone(),
        owner_epoch: 1,
        checkpoint,
    }
}
fn fixture() -> (
    SigningKey,
    SigningKey,
    OwnerPin,
    TunnelRoutePolicy,
    NodeRequestBinding,
) {
    let owner = SigningKey::from_bytes(&[11; 32]);
    let node = SigningKey::from_bytes(&[44; 32]);
    let mut p = policy();
    let mut pin = OwnerPin {
        context: p.context.clone(),
        key: PublicKey::from_bytes(owner.verifying_key().to_bytes()),
        epoch: 1,
        checkpoint: 0,
        expires_at: time(2000),
    };
    let offer = Offer {
        header: header(&p, 1),
        invitation_id: InvitationId::new("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap(),
        agent_id: p.agent_id,
        agent_key: PublicKey::from_bytes(node.verifying_key().to_bytes()),
        token_hash: Hash::from_bytes([5; 32]),
        scopes: Scopes(vec![Scope::TunnelOpen]),
        issued_at: time(1000),
        expires_at: time(1400),
    };
    let TunnelOwnerSigned::Offer(offer) = finish(
        PreparedTunnelOwner::offer(&pin, offer, Now(time(1000))).unwrap(),
        &owner,
    )
    .unwrap() else {
        panic!()
    };
    pin.checkpoint = 1;
    let claim = sign(
        Claim {
            header: offer.body.header.clone(),
            invitation_id: offer.body.invitation_id,
            offer_hash: offer.body.digest().unwrap(),
            agent_id: p.agent_id,
            agent_key: offer.body.agent_key,
            capabilities: Capabilities {
                version: 1,
                scopes: Scopes(vec![Scope::TunnelOpen]),
            },
            nonce: Nonce::from_bytes([1; 16]),
            issued_at: time(1000),
        },
        &node,
    );
    let cert = Certificate {
        header: header(&p, 2),
        agent_id: p.agent_id,
        key_epoch: 1,
        agent_key: offer.body.agent_key,
        scopes: offer.body.scopes.clone(),
        capabilities: claim.body.capabilities.clone(),
        origin: CertificateOrigin::Enrollment {
            invitation_id: offer.body.invitation_id,
            claim_hash: claim.body.digest().unwrap(),
            key_fingerprint: claim.body.agent_key.fingerprint(),
        },
        not_before: time(1000),
        expires_at: time(1500),
    };
    let TunnelOwnerSigned::Certificate(cert) = finish(
        PreparedTunnelOwner::certificate(&pin, offer, claim, cert, Now(time(1000))).unwrap(),
        &owner,
    )
    .unwrap() else {
        panic!()
    };
    pin.checkpoint = 2;
    p.agent_certificate_hash = cert.body.digest().unwrap();
    p.key_epoch = 1;
    let lease = Lease {
        header: header(&p, 2),
        id: LeaseId::new("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb").unwrap(),
        agent_id: p.agent_id,
        key_epoch: 1,
        certificate_hash: p.agent_certificate_hash,
        operation: p.operation().unwrap(),
        issued_at: time(1000),
        expires_at: time(1060),
    };
    let fresh = Freshness {
        header: header(&p, 2),
        issued_at: time(1000),
        expires_at: time(1060),
    };
    let TunnelOwnerSigned::Grant(binding) = finish(
        PreparedTunnelOwner::grant(
            &pin,
            cert,
            &p,
            lease,
            fresh,
            &p.client_device_id,
            Now(time(1000)),
        )
        .unwrap(),
        &owner,
    )
    .unwrap() else {
        panic!()
    };
    (owner, node, pin, p, binding)
}
#[test]
fn exact_node_owner_purposes_and_node_ack_roundtrip() {
    let (_, node, pin, p, b) = fixture();
    verify_binding(&pin, &b, &p, Now(time(1001))).unwrap();
    let proof = sign(
        AgentRequest {
            header: b.lease.body.header.clone(),
            agent_id: p.agent_id,
            key_epoch: 1,
            lease_hash: b.lease.body.digest().unwrap(),
            nonce: Nonce::from_bytes([2; 16]),
            issued_at: time(1001),
        },
        &node,
    );
    let ack = serde_json::json!({"format":"cc-agent-tunnel-open-ack/v1","agentId":p.agent_id,"route":p.route_id,"policyHash":p.digest().unwrap(),"expiresAt":1060,"proof":proof});
    let mut raw = Vec::new();
    wire::canonical_json(&ack, &mut raw).unwrap();
    raw.push(b'\n');
    verify_ack(&raw, &pin, &b, &p, Now(time(1001))).unwrap();
    assert!(verify_ack(&raw, &pin, &b, &p, Now(time(1060))).is_err());
}
#[test]
fn changed_policy_or_owner_never_becomes_valid_binding() {
    let (_, _, mut pin, mut p, b) = fixture();
    p.destination.port = 12345;
    assert!(verify_binding(&pin, &b, &p, Now(time(1001))).is_err());
    p = policy();
    pin.key = PublicKey::from_bytes(SigningKey::from_bytes(&[77; 32]).verifying_key().to_bytes());
    assert!(verify_binding(&pin, &b, &p, Now(time(1001))).is_err());
}
#[test]
fn unbound_legacy_operation_and_foreign_client_device_are_denied() {
    let (_, _, pin, p, mut b) = fixture();
    b.operation = Operation::TunnelOpen { route: p.route_id };
    assert!(verify_binding(&pin, &b, &p, Now(time(1001))).is_err());
    let (_, _, pin, p, b) = fixture();
    assert!(PreparedTunnelOwner::grant(
        &pin,
        b.certificate,
        &p,
        b.lease.body,
        b.freshness.body,
        "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
        Now(time(1001))
    )
    .is_err());
}
#[test]
fn owner_signature_cannot_be_replaced_by_device_or_reader_key() {
    let (_owner, node, mut pin, p, _) = fixture();
    pin.checkpoint = 0;
    let offer = Offer {
        header: header(&p, 1),
        invitation_id: InvitationId::new("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap(),
        agent_id: p.agent_id,
        agent_key: PublicKey::from_bytes(node.verifying_key().to_bytes()),
        token_hash: Hash::from_bytes([5; 32]),
        scopes: Scopes(vec![Scope::TunnelOpen]),
        issued_at: time(1000),
        expires_at: time(1400),
    };
    let prepared = PreparedTunnelOwner::offer(&pin, offer, Now(time(1000))).unwrap();
    assert!(finish(prepared, &SigningKey::from_bytes(&[99; 32])).is_err());
}

#[test]
fn revoke_current_expired_certificate_uses_existing_domain_and_owner_key() {
    let (owner, _, pin, _, binding) = fixture();
    let body = Revocation { header: Header {version:1,context:pin.context.clone(),owner_epoch:pin.epoch,
        checkpoint:pin.checkpoint+1},agent_id:binding.certificate.body.agent_id,
        key_epoch:binding.certificate.body.key_epoch,
        certificate_hash:binding.certificate.body.digest().unwrap(),issued_at:time(1600) };
    assert!(binding.certificate.body.expires_at < time(1600));
    let prepared = PreparedTunnelOwner::revoke(&pin,binding.certificate,body.clone(),Now(time(1600))).unwrap();
    let transcripts = prepared.signing_transcripts().unwrap();
    assert_eq!(transcripts,vec![body.signing_bytes().unwrap()]);
    let TunnelOwnerSigned::Revocation(signed) = finish(prepared,&owner).unwrap() else {panic!()};
    signed.verify(pin.key).unwrap();
    assert_eq!(signed.body,body);
}
#[test]
fn revoke_rejects_wrong_current_certificate_context_epoch_and_capability() {
    let (owner, _, pin, _, binding) = fixture();
    let body = Revocation { header: Header {version:1,context:pin.context.clone(),owner_epoch:pin.epoch,
        checkpoint:pin.checkpoint+1},agent_id:binding.certificate.body.agent_id,
        key_epoch:1,certificate_hash:binding.certificate.body.digest().unwrap(),issued_at:time(1001) };
    let mut wrong = body.clone(); wrong.certificate_hash = Hash::from_bytes([8;32]);
    assert!(PreparedTunnelOwner::revoke(&pin,binding.certificate.clone(),wrong,Now(time(1001))).is_err());
    let mut wrong = body.clone(); wrong.header.owner_epoch += 1;
    assert!(PreparedTunnelOwner::revoke(&pin,binding.certificate.clone(),wrong,Now(time(1001))).is_err());
    let mut foreign = pin.clone(); foreign.context.deployment_id = DeploymentId::new("cccccccc-cccc-4ccc-8ccc-cccccccccccc").unwrap();
    assert!(PreparedTunnelOwner::revoke(&foreign,binding.certificate.clone(),body.clone(),Now(time(1001))).is_err());
    let mut cert = binding.certificate.body.clone(); cert.scopes=Scopes(vec![Scope::TelemetryRead]);
    cert.capabilities.scopes=cert.scopes.clone(); let cert=sign(cert,&owner);
    let mut adjusted=body;adjusted.certificate_hash=cert.body.digest().unwrap();
    assert!(PreparedTunnelOwner::revoke(&pin,cert,adjusted,Now(time(1001))).is_err());
}
#[test]
fn revoke_signing_window_is_original_proof_ttl_and_live_owner_pin() {
    let (_, _, pin, _, binding) = fixture();
    let body = Revocation { header: Header {version:1,context:pin.context.clone(),owner_epoch:pin.epoch,
        checkpoint:pin.checkpoint+1},agent_id:binding.certificate.body.agent_id,
        key_epoch:1,certificate_hash:binding.certificate.body.digest().unwrap(),issued_at:time(1600) };
    let prepared=PreparedTunnelOwner::revoke(&pin,binding.certificate.clone(),body.clone(),Now(time(1600))).unwrap();
    assert!(prepared.recheck(Now(time(1659))).is_ok());
    assert!(prepared.recheck(Now(time(1660))).is_err());
    let mut expired=pin;expired.expires_at=time(1600);
    assert!(PreparedTunnelOwner::revoke(&expired,binding.certificate,body,Now(time(1600))).is_err());
}

#[test]
fn tunnel_rotation_certificate_is_same_closed_purpose_with_no_cap_or_expiry_increase() {
    let(owner,_,pin,_,binding)=fixture();let previous=binding.certificate;
    let new=SigningKey::from_bytes(&[55;32]);let mut cert=previous.body.clone();
    cert.header.checkpoint=3;cert.key_epoch=2;cert.agent_key=PublicKey::from_bytes(new.verifying_key().to_bytes());
    cert.origin=CertificateOrigin::Rotation{previous_certificate:previous.body.digest().unwrap(),previous_key_epoch:1};
    cert.not_before=time(1001);
    let prepared=PreparedTunnelOwner::rotation_certificate(&pin,previous.clone(),cert.clone(),Now(time(1001))).unwrap();
    assert_eq!(prepared.signing_transcripts().unwrap(),vec![cert.signing_bytes().unwrap()]);
    assert!(prepared.recheck(Now(time(1061))).is_err());
    let TunnelOwnerSigned::Certificate(signed)=finish(prepared,&owner).unwrap() else {panic!()};
    signed.verify(pin.key).unwrap();assert_eq!(signed.body.scopes,previous.body.scopes);
    assert_eq!(signed.body.capabilities,previous.body.capabilities);assert_eq!(signed.body.expires_at,previous.body.expires_at);
    let mut longer=cert.clone();longer.expires_at=time(1501);
    assert!(PreparedTunnelOwner::rotation_certificate(&pin,previous.clone(),longer,Now(time(1001))).is_err());
    let mut broader=cert.clone();broader.capabilities.scopes=Scopes(vec![Scope::TelemetryRead]);
    assert!(PreparedTunnelOwner::rotation_certificate(&pin,previous.clone(),broader,Now(time(1001))).is_err());
    let mut wrong=cert;wrong.key_epoch=3;
    assert!(PreparedTunnelOwner::rotation_certificate(&pin,previous.clone(),wrong,Now(time(1001))).is_err());
    assert!(PreparedTunnelOwner::rotation_certificate(&pin,previous.clone(),signed.body,Now(time(1500))).is_err());
}


// DRAFT_UNCOMPILED_UNRUN: existing Node domains, explicit MetricsOnly issuer.
fn metrics_fixture()->(SigningKey,OwnerPin,Signed<Certificate>,ResourceId,Lease,Freshness) {
    let(owner,node,mut pin,p,_)=fixture();pin.checkpoint=0;
    let offer=Offer {header:header(&p,1),invitation_id:InvitationId::new("cccccccc-cccc-4ccc-8ccc-cccccccccccc").unwrap(),
        agent_id:p.agent_id,agent_key:PublicKey::from_bytes(node.verifying_key().to_bytes()),token_hash:Hash::from_bytes([33;32]),
        scopes:Scopes(vec![Scope::MetricsCollect]),issued_at:time(1000),expires_at:time(1300)};
    assert!(PreparedTunnelOwner::offer(&pin,offer.clone(),Now(time(1000))).is_err());
    let TunnelOwnerSigned::Offer(offer)=finish(PreparedTunnelOwner::metrics_offer(&pin,offer,Now(time(1000))).unwrap(),&owner).unwrap()else{panic!()};
    pin.checkpoint=1;
    let claim=sign(Claim {header:offer.body.header.clone(),invitation_id:offer.body.invitation_id,offer_hash:offer.body.digest().unwrap(),
        agent_id:p.agent_id,agent_key:offer.body.agent_key,capabilities:Capabilities{version:1,scopes:Scopes(vec![Scope::MetricsCollect])},
        nonce:Nonce::from_bytes([3;16]),issued_at:time(1000)},&node);
    let cert=Certificate {header:header(&p,2),agent_id:p.agent_id,key_epoch:1,agent_key:claim.body.agent_key,
        scopes:Scopes(vec![Scope::MetricsCollect]),capabilities:claim.body.capabilities.clone(),
        origin:CertificateOrigin::Enrollment{invitation_id:offer.body.invitation_id,claim_hash:claim.body.digest().unwrap(),key_fingerprint:claim.body.agent_key.fingerprint()},
        not_before:time(1000),expires_at:time(1500)};
    let TunnelOwnerSigned::Certificate(cert)=finish(PreparedTunnelOwner::metrics_certificate(&pin,offer,claim,cert,Now(time(1000))).unwrap(),&owner).unwrap()else{panic!()};
    pin.checkpoint=2;
    let collector=ResourceId::new("dddddddd-dddd-4ddd-8ddd-dddddddddddd").unwrap();
    let lease=Lease{header:header(&p,2),id:LeaseId::new("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee").unwrap(),agent_id:p.agent_id,key_epoch:1,
        certificate_hash:cert.body.digest().unwrap(),operation:Operation::MetricsCollect{collector},issued_at:time(1000),expires_at:time(1300)};
    let fresh=Freshness{header:header(&p,2),issued_at:time(1000),expires_at:time(1300)};
    (owner,pin,cert,collector,lease,fresh)
}
#[test]
fn metrics_only_owner_roundtrip_signs_existing_lease_and_freshness_domains() {
    let(owner,pin,cert,id,lease,fresh)=metrics_fixture();
    let p=PreparedTunnelOwner::metrics_grant(&pin,cert,id,lease.clone(),fresh.clone(),Now(time(1000))).unwrap();
    let TunnelOwnerSigned::Grant(b)=finish(p,&owner).unwrap()else{panic!()};
    b.lease.verify(pin.key).unwrap();b.freshness.verify(pin.key).unwrap();
    assert_eq!(b.lease.body,lease);assert_eq!(b.freshness.body,fresh);
    assert_eq!(b.operation,Operation::MetricsCollect{collector:id});
    assert!(b.lease.body.signing_bytes().unwrap().starts_with(b"consolecrypt.agent.identity.personal-prototype\0v1\0operation-lease\0"));
}
#[test]
fn metrics_owner_rejects_wrong_resource_scope_and_extended_signing_window() {
    let(_,pin,cert,id,lease,fresh)=metrics_fixture();
    let other=ResourceId::new("ffffffff-ffff-4fff-8fff-ffffffffffff").unwrap();
    assert!(PreparedTunnelOwner::metrics_grant(&pin,cert.clone(),other,lease.clone(),fresh.clone(),Now(time(1000))).is_err());
    let p=PreparedTunnelOwner::metrics_grant(&pin,cert.clone(),id,lease.clone(),fresh.clone(),Now(time(1000))).unwrap();
    assert!(p.recheck(Now(time(1060))).is_err());
    let(_,_,tunnel_pin,_,tunnel)=fixture();
    assert!(PreparedTunnelOwner::metrics_grant(&tunnel_pin,tunnel.certificate,id,lease.clone(),fresh.clone(),Now(time(1000))).is_err());
    let mut too_long=lease;too_long.expires_at=time(1301);
    assert!(PreparedTunnelOwner::metrics_grant(&pin,cert,id,too_long,fresh,Now(time(1000))).is_err());
}
#[test]
fn metrics_current_expired_certificate_can_be_revoked_under_live_owner_pin() {
    let(owner,pin,cert,_,_,_)=metrics_fixture();
    let revoke=Revocation{header:Header{checkpoint:3,..cert.body.header.clone()},agent_id:cert.body.agent_id,
        key_epoch:cert.body.key_epoch,certificate_hash:cert.body.digest().unwrap(),issued_at:time(1600)};
    assert!(PreparedTunnelOwner::revoke(&pin,cert.clone(),revoke.clone(),Now(time(1600))).is_err());
    let prepared=PreparedTunnelOwner::metrics_revoke(&pin,cert,revoke,Now(time(1600))).unwrap();
    let TunnelOwnerSigned::Revocation(signed)=finish(prepared,&owner).unwrap()else{panic!()};signed.verify(pin.key).unwrap();
}
fn metrics_manifest()->serde_json::Value {serde_json::json!({"schema_version":1,"mode":"remote_write",
    "targets":[{"id":"app","url":"http://127.0.0.1:9100/metrics","kind":"application","sample_limit":1000,
        "series_limit":1000,"response_bytes":4096,"label_limit":32}],
    "labels":{"cc_host_id":"11111111-1111-4111-8111-111111111111","site":"local","environment":"test"},
    "scrape_interval_seconds":1,"receiver":{"url":"http://127.0.0.1:9090/api/v1/write"},
    "buffer":{"max_bytes":33554432,"max_age_seconds":300}})}
#[test]
fn metrics_manifest_is_closed_host_bound_and_hashes_canonical_without_lf() {
    use sha2::{Digest,Sha256};
    let host="11111111-1111-4111-8111-111111111111";let m=metrics_manifest();
    let actual=validate_metrics_manifest(&m,host).unwrap();let mut raw=Vec::new();canonical_json(&m,&mut raw).unwrap();
    let expected:[u8;32]=Sha256::digest(&raw).into();assert_eq!(*actual.as_bytes(),expected);
    let id=metrics_collector_id(actual).unwrap();let mut address=[0u8;16];address.copy_from_slice(&expected[..16]);
    address[6]=(address[6]&0x0f)|0x80;address[8]=(address[8]&0x3f)|0x80;
    assert_eq!(id,ResourceId::new(&uuid::Uuid::from_bytes(address).to_string()).unwrap());
    assert!(validate_metrics_manifest(&m,"22222222-2222-4222-8222-222222222222").is_err());
    let mut wrong=m.clone();wrong["buffer"]["max_bytes"]=serde_json::json!(536870912);assert!(validate_metrics_manifest(&wrong,host).is_err());
    let mut wrong=m.clone();wrong["receiver"]["follow_redirects"]=serde_json::json!(true);assert!(validate_metrics_manifest(&wrong,host).is_err());
    let mut wrong=m;wrong["receiver"]["url"]=serde_json::json!("http://127.0.0.2:9090/api/v1/write");assert!(validate_metrics_manifest(&wrong,host).is_err());
}

// Authored for the next coherent Native gate; not executed by source staging.
fn runtime_manifest(arm:bool)->serde_json::Value {
    let mut m=metrics_manifest();m["schema_version"]=serde_json::json!(2);
    m["runtime"]=if arm {serde_json::json!({"kind":"vmagent","upstreamTag":"v1.156.0","platform":"linux-aarch64",
        "binarySha256":"564346d8acd0979b62c2671ad586d63b243a925307bfa54d0d5f94bd50dda4f5","binaryBytes":21299768})}
        else {serde_json::json!({"kind":"vmagent","upstreamTag":"v1.156.0","platform":"linux-x86_64",
        "binarySha256":"2039bc05b6ad5c96b34b72801908896bf8a6a7bcab8ae32120cbb8e6217ef6f6","binaryBytes":23209584})};m
}
fn disk_runtime_manifest(platform:&str)->serde_json::Value {
    let mut m=metrics_manifest();m["schema_version"]=serde_json::json!(2);
    m["profile"]=serde_json::json!("maintained-prometheus-disk-512mib-2h-v1");
    m["bufferMode"]=serde_json::json!("persistentNativeWal");
    m["buffer"]=serde_json::json!({"max_bytes":536870912u64,"max_age_seconds":7200});
    for target in m["targets"].as_array_mut().unwrap(){target["series_limit"]=serde_json::json!(256);}
    m["runtime"]=serde_json::json!({"kind":"prometheusHardmode","upstreamTag":"3.16.0",
        "platform":platform,"binarySha256":"a".repeat(64),"binaryBytes":64000});m
}
#[test]
fn disk_v2_full_runtime_identity_is_hashed_and_cannot_alias_a_signed_grant() {
    let host="11111111-1111-4111-8111-111111111111";
    let amd=disk_runtime_manifest("linux-x86_64");let arm=disk_runtime_manifest("linux-aarch64");
    let a=validate_metrics_manifest(&amd,host).unwrap();let b=validate_metrics_manifest(&arm,host).unwrap();
    assert_ne!(a,b);let old=metrics_collector_id(a).unwrap();let next=metrics_collector_id(b).unwrap();
    assert_ne!(old,next);
    let(owner,pin,cert,_,mut lease,fresh)=metrics_fixture();lease.operation=Operation::MetricsCollect{collector:old};
    assert!(PreparedTunnelOwner::metrics_grant(&pin,cert.clone(),next,lease.clone(),fresh.clone(),Now(time(1000))).is_err());
    let TunnelOwnerSigned::Grant(binding)=finish(PreparedTunnelOwner::metrics_grant(&pin,cert,old,lease,fresh,Now(time(1000))).unwrap(),&owner).unwrap()else{panic!()};
    let mut changed=binding.lease;changed.body.operation=Operation::MetricsCollect{collector:next};
    assert!(changed.verify(pin.key).is_err());
    use sha2::{Digest,Sha256};let mut raw=Vec::new();canonical_json(&arm,&mut raw).unwrap();
    assert_eq!(b,Hash::from_bytes(Sha256::digest(raw).into()));
}
#[test]
fn disk_v2_rejects_runtime_substitution_unknown_fields_and_budget_widening() {
    let host="11111111-1111-4111-8111-111111111111";let original=disk_runtime_manifest("linux-aarch64");
    for bytes in [64u64,144399412,192*1024*1024] {
        let mut valid=original.clone();valid["runtime"]["binaryBytes"]=json!(bytes);
        assert!(validate_metrics_manifest(&valid,host).is_ok());
    }
    for(field,value)in [("kind",json!("vmagent")),("upstreamTag",json!("latest")),("platform",json!("linux-arm64")),
        ("binaryBytes",json!(201326593)),("binaryBytes",json!(true)),("binarySha256",json!("unverified")),
        ("ready",json!(true))] {
        let mut bad=original.clone();bad["runtime"][field]=value;
        assert!(validate_metrics_manifest(&bad,host).is_err(),"{field}");
    }
    for(field,value)in [("profile",json!("other")),("bufferMode",json!("ramOnly")),("schema_version",json!(1))] {
        let mut bad=original.clone();bad[field]=value;assert!(validate_metrics_manifest(&bad,host).is_err());
    }
    let mut bad=original;bad["buffer"]["max_age_seconds"]=json!(7201);
    assert!(validate_metrics_manifest(&bad,host).is_err());
}
#[test]
fn runtime_v2_uses_full_canonical_hash_and_distinct_resource_without_v1_alias() {
    let host="11111111-1111-4111-8111-111111111111";
    let old=validate_metrics_manifest(&metrics_manifest(),host).unwrap();
    let amd=validate_metrics_manifest(&runtime_manifest(false),host).unwrap();
    let arm_manifest=runtime_manifest(true);let arm=validate_metrics_manifest(&arm_manifest,host).unwrap();
    assert_ne!(old,amd);assert_ne!(old,arm);assert_ne!(amd,arm);
    assert_ne!(metrics_collector_id(old).unwrap(),metrics_collector_id(arm).unwrap());
    assert_ne!(metrics_collector_id(amd).unwrap(),metrics_collector_id(arm).unwrap());
    use sha2::{Digest,Sha256};let mut raw=Vec::new();canonical_json(&arm_manifest,&mut raw).unwrap();
    assert_eq!(arm,Hash::from_bytes(Sha256::digest(raw).into()));
}
#[test]
fn runtime_v2_rejects_unknown_cross_component_scope_and_caller_architecture() {
    let host="11111111-1111-4111-8111-111111111111";let original=runtime_manifest(true);
    for(field,value)in [("platform",serde_json::json!("linux-x86_64")),("binaryBytes",serde_json::json!(23209584)),
        ("binarySha256",serde_json::json!("2039bc05b6ad5c96b34b72801908896bf8a6a7bcab8ae32120cbb8e6217ef6f6")),
        ("upstreamTag",serde_json::json!("latest")),("callerArchitecture",serde_json::json!("arm64"))] {
        let mut bad=original.clone();bad["runtime"][field]=value;assert!(validate_metrics_manifest(&bad,host).is_err());
    }
    let mut old=original.clone();old["schema_version"]=serde_json::json!(1);assert!(validate_metrics_manifest(&old,host).is_err());
    let mut bad=original.clone();bad["receiver"]["url"]=serde_json::json!("http://127.0.0.2:9090/api/v1/write");assert!(validate_metrics_manifest(&bad,host).is_err());
    let mut bad=original;bad["buffer"]["max_bytes"]=serde_json::json!(33554433);assert!(validate_metrics_manifest(&bad,host).is_err());
}
#[test]
fn old_signed_metrics_lease_cannot_authorize_runtime_v2_resource() {
    let(owner,pin,cert,_,mut lease,fresh)=metrics_fixture();let host="11111111-1111-4111-8111-111111111111";
    let old=metrics_collector_id(validate_metrics_manifest(&metrics_manifest(),host).unwrap()).unwrap();
    let arm=metrics_collector_id(validate_metrics_manifest(&runtime_manifest(true),host).unwrap()).unwrap();
    lease.operation=Operation::MetricsCollect{collector:old};
    assert!(PreparedTunnelOwner::metrics_grant(&pin,cert.clone(),arm,lease.clone(),fresh.clone(),Now(time(1000))).is_err());
    let TunnelOwnerSigned::Grant(grant)=finish(PreparedTunnelOwner::metrics_grant(&pin,cert,old,lease,fresh,Now(time(1000))).unwrap(),&owner).unwrap()else{panic!()};
    grant.lease.verify(pin.key).unwrap();
    let mut changed=grant.lease;changed.body.operation=Operation::MetricsCollect{collector:arm};
    assert!(changed.verify(pin.key).is_err());
}
