//! Draft tests: existing public keys/transcripts only, no UI/Actor claim.
use super::*;
use ed25519_dalek::{Signer, SigningKey};

fn time(seconds: u64) -> Timestamp { Timestamp::new(seconds).unwrap() }
fn sign<T: Statement>(body: T, key: &SigningKey) -> Signed<T> {
    let signature = SignatureBytes::from_bytes(key.sign(&body.signing_bytes().unwrap()).to_bytes());
    Signed { body, signature }
}
fn finish(input: PreparedReaderNodeOwner, key: &SigningKey) -> Result<ReaderNodeOwnerSigned> {
    let signature = key.sign(&input.signing_transcript()?).to_bytes();
    input.finish(signature)
}
fn fixture() -> (SigningKey, SigningKey, OwnerPin, Offer) {
    let owner = SigningKey::from_bytes(&[11; 32]);
    let node = SigningKey::from_bytes(&[44; 32]);
    let pin = OwnerPin {
        context: Context {
            tenant: Tenant::LocalProfile { id: LocalProfileId::new("11111111-1111-4111-8111-111111111111").unwrap() },
            deployment_id: DeploymentId::new("22222222-2222-4222-8222-222222222222").unwrap(),
        },
        key: PublicKey::from_bytes(owner.verifying_key().to_bytes()),
        epoch: 1, checkpoint: 0, expires_at: time(10000),
    };
    let offer = Offer {
        header: Header { version: 1, context: pin.context.clone(), owner_epoch: 1, checkpoint: 1 },
        invitation_id: InvitationId::new("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap(),
        agent_id: AgentId::new("33333333-3333-4333-8333-333333333333").unwrap(),
        agent_key: PublicKey::from_bytes(node.verifying_key().to_bytes()),
        token_hash: Hash::from_bytes([5; 32]),
        scopes: Scopes(vec![Scope::TelemetryRead]),
        issued_at: time(1000), expires_at: time(1400),
    };
    (owner, node, pin, offer)
}
fn chain() -> (SigningKey, SigningKey, OwnerPin, Signed<Offer>, Signed<Claim>, Certificate) {
    let (owner, node, mut pin, offer) = fixture();
    let offer = sign(offer, &owner);
    pin.checkpoint = 1; // Caller only supplies this AFTER real Actor Offer ACK.
    let claim = sign(Claim {
        header: offer.body.header.clone(), invitation_id: offer.body.invitation_id,
        offer_hash: offer.body.digest().unwrap(), agent_id: offer.body.agent_id,
        agent_key: offer.body.agent_key,
        capabilities: Capabilities { version: 1, scopes: offer.body.scopes.clone() },
        nonce: Nonce::from_bytes([1; 16]), issued_at: time(1000),
    }, &node);
    let cert = Certificate {
        header: Header { version: 1, context: pin.context.clone(), owner_epoch: 1, checkpoint: 2 },
        agent_id: offer.body.agent_id, key_epoch: 1, agent_key: offer.body.agent_key,
        scopes: offer.body.scopes.clone(), capabilities: claim.body.capabilities.clone(),
        origin: CertificateOrigin::Enrollment {
            invitation_id: offer.body.invitation_id,
            claim_hash: claim.body.digest().unwrap(), key_fingerprint: claim.body.agent_key.fingerprint(),
        },
        not_before: time(1000), expires_at: time(9000),
    };
    (owner, node, pin, offer, claim, cert)
}

#[test]
fn reader_node_owner_offer_preserves_existing_domain_and_short_window() {
    let (owner, _, pin, offer) = fixture();
    let exact = offer.signing_bytes().unwrap();
    let prepared = PreparedReaderNodeOwner::offer(&pin, offer.clone(), Now(time(1000))).unwrap();
    assert_eq!(prepared.signing_transcript().unwrap(), exact);
    assert_eq!(prepared.details().owner_pin, pin);
    assert_eq!(prepared.details().signing_expires_at, time(1060));
    let ReaderNodeOwnerSigned::Offer(signed) = finish(prepared, &owner).unwrap() else { panic!() };
    assert_eq!(signed.body, offer);
    verify_statement(&signed, pin.key).unwrap();
}

#[test]
fn reader_node_owner_certificate_verifies_real_pop_and_existing_domain() {
    let (owner, _, pin, offer, claim, cert) = chain();
    let exact = cert.signing_bytes().unwrap();
    let prepared = PreparedReaderNodeOwner::certificate(&pin, offer, claim, cert.clone(), Now(time(1000))).unwrap();
    assert_eq!(prepared.signing_transcript().unwrap(), exact);
    assert_eq!(prepared.details().signing_expires_at, time(1060));
    let ReaderNodeOwnerSigned::Certificate(signed) = finish(prepared, &owner).unwrap() else { panic!() };
    assert_eq!(signed.body, cert);
    verify_statement(&signed, pin.key).unwrap();
}

#[test]
fn reader_node_owner_scope_never_relabels_tunnel_or_mixed_capabilities() {
    let (_, _, pin, mut offer) = fixture();
    assert!(matches!(PreparedTunnelOwner::offer(&pin, offer.clone(), Now(time(1000))), Err(Error::CapabilityDenied)));
    offer.scopes = Scopes(vec![Scope::TunnelOpen]);
    assert!(PreparedTunnelOwner::offer(&pin, offer.clone(), Now(time(1000))).is_ok());
    assert!(matches!(PreparedReaderNodeOwner::offer(&pin, offer.clone(), Now(time(1000))), Err(Error::CapabilityDenied)));
    offer.scopes = Scopes(vec![Scope::TelemetryRead, Scope::TunnelOpen]);
    assert!(matches!(PreparedReaderNodeOwner::offer(&pin, offer, Now(time(1000))), Err(Error::CapabilityDenied)));
    let (_, _, pin, offer, claim, mut cert) = chain();
    cert.capabilities.scopes = Scopes(vec![Scope::TunnelOpen]);
    assert!(matches!(PreparedReaderNodeOwner::certificate(&pin, offer, claim, cert, Now(time(1000))), Err(Error::CapabilityDenied)));
}

#[test]
fn reader_node_owner_requires_exact_applied_offer_checkpoint_and_context() {
    let (_, _, mut pin, offer, claim, mut cert) = chain();
    pin.checkpoint = 2;
    cert.header.checkpoint = 3;
    assert!(matches!(PreparedReaderNodeOwner::certificate(&pin, offer, claim, cert, Now(time(1000))), Err(Error::WrongClaim)));
    let (_, _, pin, offer, claim, mut cert) = chain();
    cert.header.context.tenant = Tenant::PersonalAccount { id: PersonalAccountId::new("11111111-1111-4111-8111-111111111111").unwrap() };
    assert!(matches!(PreparedReaderNodeOwner::certificate(&pin, offer, claim, cert, Now(time(1000))), Err(Error::WrongCheckpoint)));
    let (_, _, pin, mut offer) = fixture();
    offer.header.owner_epoch = 2;
    assert!(matches!(PreparedReaderNodeOwner::offer(&pin, offer, Now(time(1000))), Err(Error::WrongCheckpoint)));
}

#[test]
fn reader_node_owner_rejects_forged_claim_origin_and_owner_key_reuse() {
    let (_, _, pin, offer, mut claim, cert) = chain();
    claim.signature = SignatureBytes::from_bytes([0; 64]);
    assert!(matches!(PreparedReaderNodeOwner::certificate(&pin, offer, claim, cert, Now(time(1000))), Err(Error::InvalidSignature)));
    let (_, _, pin, offer, claim, mut cert) = chain();
    cert.origin = CertificateOrigin::Enrollment { invitation_id: offer.body.invitation_id, claim_hash: Hash::from_bytes([9; 32]), key_fingerprint: claim.body.agent_key.fingerprint() };
    assert!(matches!(PreparedReaderNodeOwner::certificate(&pin, offer, claim, cert, Now(time(1000))), Err(Error::WrongClaim)));
    let (_, _, pin, mut offer) = fixture();
    offer.agent_key = pin.key;
    assert!(matches!(PreparedReaderNodeOwner::offer(&pin, offer, Now(time(1000))), Err(Error::KeyReuse)));
}

#[test]
fn reader_node_owner_review_expires_without_certificate_renewal() {
    let (_, _, pin, offer, claim, cert) = chain();
    let prepared = PreparedReaderNodeOwner::certificate(&pin, offer, claim, cert, Now(time(1000))).unwrap();
    prepared.recheck(Now(time(1059))).unwrap();
    assert!(matches!(prepared.recheck(Now(time(1060))), Err(Error::Expired)));
    assert!(matches!(prepared.recheck(Now(time(999))), Err(Error::InvalidTime)));
    let (_, _, pin, offer, claim, cert) = chain();
    assert!(matches!(PreparedReaderNodeOwner::certificate(&pin, offer, claim, cert, Now(time(1061))), Err(Error::InvalidTime)));
}

#[test]
fn reader_node_owner_team_denied_and_wrong_device_signature_rejected() {
    let (_, node, mut pin, mut offer) = fixture();
    pin.context.tenant = Tenant::Team { id: TeamId::new("11111111-1111-4111-8111-111111111111").unwrap() };
    offer.header.context = pin.context.clone();
    assert!(matches!(PreparedReaderNodeOwner::offer(&pin, offer, Now(time(1000))), Err(Error::UnsupportedTeam)));
    let (_, _, pin, offer) = fixture();
    let prepared = PreparedReaderNodeOwner::offer(&pin, offer, Now(time(1000))).unwrap();
    assert!(matches!(finish(prepared, &node), Err(Error::InvalidSignature)));
}
