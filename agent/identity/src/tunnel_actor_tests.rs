// DRAFT actual signed Actor tests, using the existing parent test harness.
use super::*;

fn enrolled() -> Harness {
    let mut h = Harness::offered(
        Scopes(vec![Scope::TunnelOpen]),
        Scopes(vec![Scope::TunnelOpen]),
    );
    h.actor
        .consume(&h.token, sign(h.claim.clone(), &h.agent), now())
        .unwrap();
    h.actor
        .approve(sign(h.cert.clone(), &h.owner), now())
        .unwrap();
    h
}
fn bound(h: &Harness) -> TunnelRoutePolicy {
    let mut p: TunnelRoutePolicy =
        serde_json::from_str(include_str!("tunnel_policy_vector.json")).unwrap();
    p.context = context();
    p.agent_id = h.cert.agent_id;
    p.agent_certificate_hash = h.cert.digest().unwrap();
    p.limits.max_ttl_seconds = 120;
    p
}

#[test]
fn actual_tunnel_bound_admission_replay_and_same_store_revoke() {
    let mut h = enrolled();
    let policy = bound(&h);
    let op = policy.operation().unwrap();
    let permit = h.admit(op.clone(), 80).unwrap();
    h.actor.check_permit(&permit, now()).unwrap();
    assert!(matches!(h.admit(op, 80), Err(Error::Replay)));
    h.actor
        .revoke(
            sign(
                Revocation {
                    header: header(3),
                    agent_id: h.cert.agent_id,
                    key_epoch: 1,
                    certificate_hash: h.cert.digest().unwrap(),
                    issued_at: time(1000),
                },
                &h.owner,
            ),
            now(),
        )
        .unwrap();
    assert_eq!(
        h.actor.check_permit(&permit, now()),
        Err(Error::WrongCheckpoint)
    );
}

#[test]
fn actual_tunnel_scope_never_implies_jobs_or_telemetry_and_expiry_is_live() {
    let mut h = enrolled();
    let policy = bound(&h);
    assert!(matches!(
        h.admit(
            Operation::JobsExecute {
                manifest: Hash::from_bytes([9; 32])
            },
            81
        ),
        Err(Error::ScopeDenied)
    ));
    assert!(matches!(
        h.admit(
            Operation::TelemetryRead {
                source: policy.route_id
            },
            82
        ),
        Err(Error::ScopeDenied)
    ));
    let permit = h.admit(policy.operation().unwrap(), 83).unwrap();
    assert_eq!(
        h.actor.check_permit(&permit, Now(time(1120))),
        Err(Error::Expired)
    );
}

#[test]
fn same_tunnel_lease_fresh_nonce_after_actor_restart_cannot_reset_budget() {
    let mut h = enrolled();
    let policy = bound(&h);
    let operation = policy.operation().unwrap();
    h.admit(operation.clone(), 86).unwrap();
    let lease = h.lease(operation.clone(), 86);
    let new_proof = h.proof(&lease, 87); // DIFFERENT nonce, SAME already consumed LeaseId.
    let mut restarted = Actor::new(h.memory.clone()).unwrap();
    let before = restarted.snapshot().unwrap().revision();
    assert!(matches!(
        restarted.admit(
            sign(lease, &h.owner),
            new_proof,
            Some(&h.fresh()),
            &operation,
            now()
        ),
        Err(Error::Replay)
    ));
    assert_eq!(restarted.snapshot().unwrap().revision(), before);
}

#[test]
fn policy_refuses_legacy_unbound_and_mutated_policy_before_actor_mutation() {
    let h = enrolled();
    let policy = bound(&h);
    let original = h.actor.snapshot().unwrap().revision();
    let op = Operation::TunnelOpen {
        route: policy.route_id,
    };
    let binding = NodeRequestBinding {
        certificate: sign(h.cert.clone(), &h.owner),
        lease: sign(h.lease(op.clone(), 84), &h.owner),
        freshness: h.fresh(),
        operation: op,
    };
    assert_eq!(policy.check_binding(&binding), Err(Error::ScopeDenied));
    assert_eq!(h.actor.snapshot().unwrap().revision(), original);
    let mut mutated = policy.clone();
    mutated.destination.port += 1;
    let op = policy.operation().unwrap();
    let binding = NodeRequestBinding {
        certificate: sign(h.cert.clone(), &h.owner),
        lease: sign(h.lease(op.clone(), 85), &h.owner),
        freshness: h.fresh(),
        operation: op,
    };
    policy.check_binding(&binding).unwrap();
    assert_eq!(mutated.check_binding(&binding), Err(Error::ScopeDenied));
    assert_eq!(h.actor.snapshot().unwrap().revision(), original);
}

#[cfg(target_os = "linux")]
#[test]
fn actual_os_node_tunnel_claim_and_signer_are_explicit_and_legacy_stays_telemetry_only() {
    let _owned = DurableTemp::new();
    let directory = &_owned.0;
    let node = directory.join("node");
    let agent_id = AgentId::new(AGENT).unwrap();
    let hello = provision_node(&node, agent_id).unwrap();
    let owner = key(11);
    let pin = OwnerPin {
        context: context(),
        key: public(&owner),
        epoch: 1,
        checkpoint: 0,
        expires_at: time(4600),
    };
    let memory = Memory(Arc::new(Mutex::new((
        State::bootstrap(pin.clone(), now()).unwrap(),
        false,
    ))));
    let mut actor = Actor::new(memory).unwrap();
    let token = InvitationToken::from_bytes([33; 32]).unwrap();
    let offer = Offer {
        header: header(1),
        invitation_id: InvitationId::new(INVITE).unwrap(),
        agent_id,
        agent_key: hello.public_key,
        token_hash: token.digest(),
        scopes: Scopes(vec![Scope::TunnelOpen]),
        issued_at: time(1000),
        expires_at: time(1600),
    };
    actor.offer(sign(offer.clone(), &owner), now()).unwrap();
    assert!(matches!(
        create_node_claim(&node, agent_id, &pin, sign(offer.clone(), &owner), now()),
        Err(Error::CapabilityDenied)
    ));
    let claim = create_tunnel_node_claim(&node, agent_id, &pin, sign(offer.clone(), &owner), now())
        .unwrap();
    assert_eq!(
        claim.body.capabilities.scopes,
        Scopes(vec![Scope::TunnelOpen])
    );
    let claim_hash = actor.consume(&token, claim, now()).unwrap();
    let cert = Certificate {
        header: header(2),
        agent_id,
        key_epoch: 1,
        agent_key: hello.public_key,
        scopes: Scopes(vec![Scope::TunnelOpen]),
        capabilities: Capabilities {
            version: 1,
            scopes: Scopes(vec![Scope::TunnelOpen]),
        },
        origin: CertificateOrigin::Enrollment {
            invitation_id: offer.invitation_id,
            claim_hash,
            key_fingerprint: hello.key_fingerprint,
        },
        not_before: time(1000),
        expires_at: time(4600),
    };
    actor.approve(sign(cert.clone(), &owner), now()).unwrap();
    let pin = actor.snapshot().unwrap().owner().clone();
    let mut policy: TunnelRoutePolicy =
        serde_json::from_str(include_str!("tunnel_policy_vector.json")).unwrap();
    policy.context = context();
    policy.agent_id = agent_id;
    policy.agent_certificate_hash = cert.digest().unwrap();
    let operation = policy.operation().unwrap();
    let lease = sign(
        Lease {
            header: header(2),
            id: LeaseId::new(&uuid(90)).unwrap(),
            agent_id,
            key_epoch: 1,
            certificate_hash: cert.digest().unwrap(),
            operation: operation.clone(),
            issued_at: time(1000),
            expires_at: time(1060),
        },
        &owner,
    );
    let freshness = sign(
        Freshness {
            header: header(2),
            issued_at: time(1000),
            expires_at: time(1300),
        },
        &owner,
    );
    let binding = NodeRequestBinding {
        certificate: sign(cert, &owner),
        lease: lease.clone(),
        freshness: freshness.clone(),
        operation: operation.clone(),
    };
    assert!(matches!(
        create_node_request(&node, agent_id, &pin, binding.clone(), now()),
        Err(Error::CapabilityDenied)
    ));
    let proof = create_tunnel_node_request(&node, agent_id, &pin, binding, &policy, now()).unwrap();
    let permit = actor
        .admit(lease, proof, Some(&freshness), &operation, now())
        .unwrap();
    actor.check_permit(&permit, now()).unwrap();
}
