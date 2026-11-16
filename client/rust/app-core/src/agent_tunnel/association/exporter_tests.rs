//! Real Ed25519 original-purpose transcripts. No artifact/install/child claim.
use super::*;

fn transcript() -> (Key, Probe, Signed<Offer>, Claimed, Signed<Certificate>, ManagedExporterPolicy) {
    let owner=Key::new();let node=Key::new();let(mut probe,_,_,_,_)=fixture();
    probe.owner_pin.key=owner.public();probe.node.public_key=node.public();
    probe.node.key_fingerprint=node.public().fingerprint();
    let pin=&probe.owner_pin;
    let offer=owner.sign(Offer {header:Header {version:1,context:pin.context.clone(),owner_epoch:pin.epoch,checkpoint:pin.checkpoint+1},
        invitation_id:InvitationId::new(&Uuid::new_v4().to_string()).unwrap(),agent_id:probe.node.agent_id,
        agent_key:node.public(),token_hash:Hash::from_bytes([9;32]),scopes:Scopes(vec![Scope::ExportersManage]),
        issued_at:time(1000),expires_at:time(1060)});
    let signed_claim=node.sign(Claim {header:offer.body.header.clone(),invitation_id:offer.body.invitation_id,
        offer_hash:offer.body.digest().unwrap(),agent_id:probe.node.agent_id,agent_key:node.public(),
        capabilities:Capabilities {version:1,scopes:offer.body.scopes.clone()},nonce:Nonce::from_bytes([17;16]),issued_at:time(1001)});
    let claim=Claimed {claim_digest:signed_claim.body.digest().unwrap(),claim:signed_claim,
        owner_pin:protocol::advance(pin,1).unwrap(),actor_revision:probe.actor_revision+2};
    let cert=owner.sign(Certificate {header:Header {version:1,context:pin.context.clone(),owner_epoch:pin.epoch,checkpoint:pin.checkpoint+2},
        agent_id:probe.node.agent_id,key_epoch:1,agent_key:node.public(),scopes:offer.body.scopes.clone(),
        capabilities:claim.claim.body.capabilities.clone(),origin:CertificateOrigin::Enrollment {
            invitation_id:offer.body.invitation_id,claim_hash:claim.claim_digest,key_fingerprint:node.public().fingerprint()},
        not_before:time(1001),expires_at:time(1500)});
    let policy=ManagedExporterPolicy {format:"cc-agent-managed-node-exporter/v1".into(),
        host_id:"00000000-0000-4000-8000-000000000001".into(),agent_id:probe.node.agent_id,
        artifact:ExporterArtifact {name:"node_exporter".into(),version:"v1.12.1".into(),platform:"linux-x86_64".into(),
            archive_sha256:Hash::from_bytes(hex::decode("b51d8a76aa2a9156a55d501aca6276fae09e262259a5e4e831d2c2222f084e63").unwrap().try_into().unwrap()),
            binary_sha256:Hash::from_bytes([21;32]),binary_bytes:4096,notice_inventory_sha256:Hash::from_bytes([22;32])},
        bind_ip:"127.0.0.1".into(),bind_port:19100,collectors:vec!["cpu".into(),"meminfo".into()],max_seconds:300};
    (owner,probe,offer,claim,cert,policy)
}
fn grant(pin:&OwnerPin,cert:&Signed<Certificate>,policy:&ManagedExporterPolicy)->(Lease,Freshness) {
    let header=Header {version:1,context:pin.context.clone(),owner_epoch:pin.epoch,checkpoint:pin.checkpoint};
    (Lease {header:header.clone(),id:LeaseId::new(&Uuid::new_v4().to_string()).unwrap(),agent_id:cert.body.agent_id,
        key_epoch:cert.body.key_epoch,certificate_hash:cert.body.digest().unwrap(),operation:policy.operation().unwrap(),
        issued_at:time(1010),expires_at:time(1250)},Freshness {header,issued_at:time(1010),expires_at:time(1250)})
}
#[test]
fn exporters_signed_enrollment_is_not_metrics_or_tunnel_enrollment() {
    let(_,probe,offer,claim,cert,_)=transcript();let now=Now(time(1002));
    protocol::claimed_for(AssociationPurpose::Exporters,&probe,&offer,&claim,now).unwrap();
    assert!(protocol::claimed_for(AssociationPurpose::Metrics,&probe,&offer,&claim,now).is_err());
    assert!(protocol::claimed_for(AssociationPurpose::Tunnel,&probe,&offer,&claim,now).is_err());
    PreparedTunnelOwner::exporters_certificate(&claim.owner_pin,offer.clone(),claim.claim.clone(),cert.body.clone(),now).unwrap();
    assert!(PreparedTunnelOwner::metrics_certificate(&claim.owner_pin,offer,claim.claim,cert.body,now).is_err());
}
#[test]
fn exporter_full_artifact_notice_and_budget_cannot_retarget_an_original_lease() {
    let(_,probe,_,_,cert,policy)=transcript();let pin=protocol::advance(&probe.owner_pin,2).unwrap();
    let(lease,freshness)=grant(&pin,&cert,&policy);let now=Now(time(1011));
    PreparedTunnelOwner::exporters_grant(&pin,cert.clone(),&policy,lease.clone(),freshness.clone(),now).unwrap();
    for mutated in [
        {let mut p=policy.clone();p.artifact.binary_sha256=Hash::from_bytes([23;32]);p},
        {let mut p=policy.clone();p.artifact.notice_inventory_sha256=Hash::from_bytes([24;32]);p},
        {let mut p=policy.clone();p.max_seconds=250;p},
    ] {
        assert_ne!(policy.digest().unwrap(),mutated.digest().unwrap());
        assert!(PreparedTunnelOwner::exporters_grant(&pin,cert.clone(),&mutated,lease.clone(),freshness.clone(),now).is_err());
    }
    let mut too_long=lease.clone();too_long.expires_at=time(1311);
    let mut fresh=freshness.clone();fresh.expires_at=too_long.expires_at;
    assert!(PreparedTunnelOwner::exporters_grant(&pin,cert,&policy,too_long,fresh,now).is_err());
}
#[test]
fn exporter_original_owner_signature_and_metrics_certificate_cannot_be_substituted() {
    let(owner,probe,_,_,cert,policy)=transcript();let pin=protocol::advance(&probe.owner_pin,2).unwrap();
    let(lease,freshness)=grant(&pin,&cert,&policy);let now=Now(time(1011));
    let mut foreign=cert.clone();foreign.signature=SignatureBytes::from_bytes([4;64]);
    assert!(PreparedTunnelOwner::exporters_grant(&pin,foreign,&policy,lease.clone(),freshness.clone(),now).is_err());
    let mut body=cert.body;body.scopes=Scopes(vec![Scope::MetricsCollect]);body.capabilities.scopes=body.scopes.clone();
    let metrics=owner.sign(body);
    assert!(PreparedTunnelOwner::exporters_grant(&pin,metrics,&policy,lease,freshness,now).is_err());
}
#[test]
fn exporter_catalog_is_exactly_bound_to_saved_host_node_and_full_policy_hash() {
    let(_,probe,_,_,_,policy)=transcript();let host=policy.host_id.parse::<ObjectId>().unwrap();
    let mut catalog=protocol::ExporterCatalog {manifest_sha256:policy.digest().unwrap(),policy};
    catalog.check(host,probe.node.agent_id).unwrap();
    let other="00000000-0000-4000-8000-000000000002".parse::<ObjectId>().unwrap();
    assert!(catalog.check(other,probe.node.agent_id).is_err());
    catalog.policy.artifact.notice_inventory_sha256=Hash::from_bytes([31;32]);
    assert!(catalog.check(host,probe.node.agent_id).is_err());
}
