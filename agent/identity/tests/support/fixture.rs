// Synthetic Ed25519 keys belong only to examples/tests. Never enroll a host
// using this fixture. It opens no network and runs no service.
use cc_agent_identity_policy::*;
use ed25519_dalek::{Signer, SigningKey};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;

fn sign<T: Statement>(body: T, key: &SigningKey) -> Signed<T> {
    let signature = SignatureBytes::from_bytes(key.sign(&body.signing_bytes().unwrap()).to_bytes());
    Signed { body, signature }
}
fn public(key: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(key.verifying_key().to_bytes())
}
fn write(path: &Path, value: &impl Serialize) {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).unwrap();
    file.write_all(&serde_json::to_vec_pretty(value).unwrap())
        .unwrap();
    file.sync_all().unwrap();
}
pub fn create(
    out: &Path,
    seconds: u64,
    agent_id: AgentId,
    heartbeat: ResourceId,
    history: ResourceId,
) -> Value {
    assert!(
        out.is_absolute() && out.is_dir(),
        "existing absolute private output directory required"
    );
    let owner = SigningKey::from_bytes(&[11; 32]);
    let agent = SigningKey::from_bytes(&[22; 32]);
    let now = Timestamp::new(seconds).unwrap();
    let context = Context {
        tenant: Tenant::LocalProfile {
            id: LocalProfileId::new("11111111-1111-4111-8111-111111111111").unwrap(),
        },
        deployment_id: DeploymentId::new("22222222-2222-4222-8222-222222222222").unwrap(),
    };
    let header = |checkpoint| Header {
        version: 1,
        context: context.clone(),
        owner_epoch: 1,
        checkpoint,
    };
    let pin = OwnerPin {
        context: context.clone(),
        key: public(&owner),
        epoch: 1,
        checkpoint: 0,
        expires_at: now.checked_add(3600).unwrap(),
    };
    write(&out.join("owner-pin.json"), &pin);
    let mut actor = Actor::new(
        FileStore::create(out.join("store"), out.join("anchor"), pin, Now(now)).unwrap(),
    )
    .unwrap();
    let token = InvitationToken::from_bytes([33; 32]).unwrap();
    let scopes = Scopes(vec![Scope::TelemetryRead]);
    let offer = Offer {
        header: header(1),
        invitation_id: InvitationId::new("33333333-3333-4333-8333-333333333333").unwrap(),
        agent_id,
        agent_key: public(&agent),
        token_hash: token.digest(),
        scopes: scopes.clone(),
        issued_at: now,
        expires_at: now.checked_add(600).unwrap(),
    };
    actor.offer(sign(offer.clone(), &owner), Now(now)).unwrap();
    let claim = Claim {
        header: header(1),
        invitation_id: offer.invitation_id,
        offer_hash: offer.digest().unwrap(),
        agent_id,
        agent_key: public(&agent),
        capabilities: Capabilities {
            version: 1,
            scopes: scopes.clone(),
        },
        nonce: Nonce::from_bytes([1; 16]),
        issued_at: now,
    };
    actor
        .consume(&token, sign(claim.clone(), &agent), Now(now))
        .unwrap();
    let certificate = Certificate {
        header: header(2),
        agent_id,
        key_epoch: 1,
        agent_key: public(&agent),
        scopes,
        capabilities: claim.capabilities.clone(),
        origin: CertificateOrigin::Enrollment {
            invitation_id: offer.invitation_id,
            claim_hash: claim.digest().unwrap(),
            key_fingerprint: public(&agent).fingerprint(),
        },
        not_before: now,
        expires_at: now.checked_add(3600).unwrap(),
    };
    actor
        .approve(sign(certificate.clone(), &owner), Now(now))
        .unwrap();
    let fresh = sign(
        Freshness {
            header: header(2),
            issued_at: now,
            expires_at: now.checked_add(300).unwrap(),
        },
        &owner,
    );
    let mut bundles = Vec::new();
    for (name, source, n) in [
        ("heartbeat-request.json", heartbeat, 2u8),
        ("history-request.json", history, 3u8),
        ("heartbeat-after-revoke-request.json", heartbeat, 4u8),
        ("heartbeat-after-rollback-request.json", heartbeat, 5u8),
    ] {
        let operation = Operation::TelemetryRead { source };
        let lease = Lease {
            header: header(2),
            id: LeaseId::new(&format!("55555555-5555-4555-8555-{n:012x}")).unwrap(),
            agent_id,
            key_epoch: 1,
            certificate_hash: certificate.digest().unwrap(),
            operation: operation.clone(),
            issued_at: now,
            expires_at: now.checked_add(300).unwrap(),
        };
        let proof = sign(
            AgentRequest {
                header: header(2),
                agent_id,
                key_epoch: 1,
                lease_hash: lease.digest().unwrap(),
                nonce: Nonce::from_bytes([n; 16]),
                issued_at: now,
            },
            &agent,
        );
        let bundle = json!({"now": now, "lease": sign(lease, &owner), "proof": proof, "freshness": fresh, "operation": operation});
        write(&out.join(name), &bundle);
        bundles.push(name);
    }
    let revoke = sign(
        Revocation {
            header: header(3),
            agent_id,
            key_epoch: 1,
            certificate_hash: certificate.digest().unwrap(),
            issued_at: now,
        },
        &owner,
    );
    write(
        &out.join("revoke-request.json"),
        &json!({"now": now, "mutation": {"kind":"Revoke", "value":revoke}}),
    );
    let summary = json!({"testOnly":true,"agentId":agent_id,"now":now,"store":out.join("store"),"anchor":out.join("anchor"),"ownerPin":out.join("owner-pin.json"),"bundles":bundles});
    write(&out.join("fixture-manifest.json"), &summary);
    summary
}
