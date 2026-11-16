// Synthetic-key authority policy regressions, not provisioning/import APIs.
fn rotate_owner_at(h: &mut Harness, replacement: &SigningKey, observed: Now) -> Result<()> {
    let state = h.actor.snapshot()?;
    let change = OwnerRotation {
        header: Header { version: 1, context: state.owner().context.clone(),
            owner_epoch: state.owner().epoch, checkpoint: state.owner().checkpoint + 1 },
        new_epoch: state.owner().epoch + 1, new_key: public(replacement),
        issued_at: observed.0, expires_at: time(4600),
    };
    h.actor.rotate_owner(sign(change.clone(), &h.owner), sign(NewOwnerProof(change), replacement), observed)?;
    h.owner = SigningKey::from_bytes(&replacement.to_bytes());
    Ok(())
}
fn rotate_owner(h: &mut Harness, replacement: &SigningKey) -> Result<()> {
    rotate_owner_at(h, replacement, now())
}
fn agent_offer(h: &Harness, key: PublicKey) -> Offer {
    Offer { header: Header { checkpoint: h.current_header().checkpoint + 1, ..h.current_header() },
        invitation_id: InvitationId::new(&uuid(90)).unwrap(), agent_id: AgentId::new(&uuid(91)).unwrap(),
        agent_key: key, token_hash: InvitationToken::from_bytes([77; 32]).unwrap().digest(),
        scopes: Scopes(vec![Scope::TelemetryRead]), issued_at: time(1000), expires_at: time(1600) }
}
fn unchanged(before: &State, after: &State) {
    assert_eq!(before.revision, after.revision);
    assert_eq!(before.pin, after.pin);
    assert_eq!(before.owner_keys, after.owner_keys);
    assert_eq!(before.agent_keys, after.agent_keys);
    assert_eq!(before.nonces, after.nonces);
    assert_eq!(before.leases, after.leases);
    assert_eq!(serde_json::to_value(before.capacity()).unwrap(), serde_json::to_value(after.capacity()).unwrap());
}
#[test]
fn owner_role_history_denies_agent_promotions_and_retains_failed_transition_state() {
    for stage in ["offered", "claimed", "registered", "revoked", "retired"] {
        let mut h = match stage { "offered" => Harness::offered(Scopes(vec![Scope::TelemetryRead]), scopes()),
            "claimed" => Harness::claimed(), _ => Harness::enrolled() };
        if stage == "revoked" {
            h.actor.revoke(sign(Revocation { header: header(3), agent_id: h.cert.agent_id, key_epoch: 1,
                certificate_hash: h.cert.digest().unwrap(), issued_at: time(1000) }, &h.owner), now()).unwrap();
        }
        if stage == "retired" {
            let next = key(66);
            let (cert, proof) = rotated(&h, &next);
            h.actor.rotate(sign(cert.clone(), &h.owner), sign(proof.clone(), &h.agent), sign(NewKeyProof(proof), &next), now()).unwrap();
            h.cert = cert;
            h.agent = next;
            let next = key(77);
            let mut cert = h.cert.clone();
            cert.header = header(4); cert.key_epoch = 3; cert.agent_key = public(&next);
            cert.origin = CertificateOrigin::Rotation { previous_certificate: h.cert.digest().unwrap(), previous_key_epoch: 2 };
            let proof = RotationProof { header: header(4), agent_id: cert.agent_id, certificate_hash: cert.digest().unwrap(),
                nonce: Nonce::from_bytes([10;16]), issued_at: time(1000) };
            h.actor.rotate(sign(cert.clone(), &h.owner), sign(proof.clone(), &h.agent), sign(NewKeyProof(proof), &next), now()).unwrap();
            h.cert = cert; h.agent = next;
        }
        let before = h.actor.snapshot().unwrap();
        // Intermediate retired key66 has no invitation record; only retained
        // agent-key history can preserve its dedicated role.
        assert_eq!(rotate_owner(&mut h, &key(if stage=="retired" {66} else {22})), Err(Error::KeyReuse), "{stage}");
        unchanged(&before, &h.actor.snapshot().unwrap());
        rotate_owner(&mut h, &key(55)).unwrap();
        assert_eq!(h.actor.snapshot().unwrap().capacity().retained_owner_key_tombstones, 2);
    }
}
#[test]
fn owner_role_history_reserves_all_former_owners_for_agent_offer_and_rotation() {
    let mut h = Harness::enrolled();
    rotate_owner(&mut h, &key(55)).unwrap();
    rotate_owner(&mut h, &key(66)).unwrap();
    for seed in [11,55,66] {
        let before = h.actor.snapshot().unwrap();
        let offered = agent_offer(&h, public(&key(seed)));
        assert_eq!(h.actor.offer(sign(offered, &h.owner), now()), Err(Error::KeyReuse));
        unchanged(&before, &h.actor.snapshot().unwrap());
    }
    let before = h.actor.snapshot().unwrap();
    assert_eq!(rotate_owner(&mut h, &key(11)), Err(Error::KeyReuse));
    unchanged(&before, &h.actor.snapshot().unwrap());
    for seed in [11,55,66] {
        let target = key(seed);
        let (mut cert, mut proof) = rotated(&h, &target);
        cert.header = Header { checkpoint: h.current_header().checkpoint + 1, ..h.current_header() };
        proof.header = cert.header.clone();
        proof.certificate_hash = cert.digest().unwrap();
        assert_eq!(h.actor.rotate(sign(cert.clone(), &h.owner), sign(proof.clone(), &key(99)), sign(NewKeyProof(proof.clone()), &target), now()), Err(Error::InvalidSignature));
        assert_eq!(h.actor.rotate(sign(cert.clone(), &h.owner), sign(proof.clone(), &h.agent), sign(NewKeyProof(proof.clone()), &key(99)), now()), Err(Error::InvalidSignature));
        assert_eq!(h.actor.rotate(sign(cert, &h.owner), sign(proof.clone(), &h.agent), sign(NewKeyProof(proof), &target), now()), Err(Error::KeyReuse));
        unchanged(&before, &h.actor.snapshot().unwrap());
    }
    // Failed role checks/signatures have not consumed rotation nonce9.
    let next = key(77);
    let (mut cert, mut proof) = rotated(&h, &next);
    cert.header = Header { checkpoint: h.current_header().checkpoint + 1, ..h.current_header() };
    proof.header = cert.header.clone(); proof.certificate_hash = cert.digest().unwrap();
    h.actor.rotate(sign(cert, &h.owner), sign(proof.clone(), &h.agent), sign(NewKeyProof(proof), &next), now()).unwrap();
}
#[test]
fn owner_role_history_terminal_invitation_exclusion_does_not_break_stable_node_ttl_retry() {
    for expired in [false,true] {
        let mut h = Harness::claimed();
        let observed = if expired { Now(time(1600)) } else { now() };
        if expired { h.actor.expire(h.offer.invitation_id, observed).unwrap(); }
        else { h.actor.reject(sign(Rejection { header: header(2), invitation_id: h.offer.invitation_id,
            claim_hash: h.claim.digest().unwrap(), issued_at: observed.0 }, &h.owner), observed).unwrap(); }
        let before = h.actor.snapshot().unwrap();
        assert_eq!(rotate_owner_at(&mut h, &key(22), observed), Err(Error::KeyReuse));
        unchanged(&before, &h.actor.snapshot().unwrap());
        let mut retry = h.offer.clone();
        retry.header = Header { checkpoint: h.current_header().checkpoint + 1, ..h.current_header() };
        retry.invitation_id = InvitationId::new(&uuid(92)).unwrap();
        retry.token_hash = InvitationToken::from_bytes([78;32]).unwrap().digest();
        retry.issued_at = observed.0; retry.expires_at = observed.0.checked_add(600).unwrap();
        h.actor.offer(sign(retry, &h.owner), observed).unwrap();
    }
}
#[test]
fn owner_role_history_checks_signatures_and_cas_before_publishing_new_key() {
    let mut h = Harness::enrolled();
    let change = OwnerRotation { header: header(3), new_epoch: 2, new_key: public(&h.agent),
        issued_at: time(1000), expires_at: time(4600) };
    let before = h.actor.snapshot().unwrap();
    assert_eq!(h.actor.rotate_owner(sign(change.clone(), &key(99)), sign(NewOwnerProof(change.clone()), &h.agent), now()), Err(Error::InvalidSignature));
    assert_eq!(h.actor.rotate_owner(sign(change.clone(), &h.owner), sign(NewOwnerProof(change), &key(99)), now()), Err(Error::InvalidSignature));
    unchanged(&before, &h.actor.snapshot().unwrap());
    h.memory.0.lock().unwrap().1 = true;
    assert_eq!(rotate_owner(&mut h, &key(55)), Err(Error::Conflict));
    unchanged(&before, &h.actor.snapshot().unwrap());
    h.memory.0.lock().unwrap().1 = false;
    rotate_owner(&mut h, &key(55)).unwrap();
}
#[test]
fn owner_role_history_has_explicit_lifetime_quota_without_eviction_or_admission_reset() {
    let mut h = Harness::enrolled();
    assert_eq!(h.actor.snapshot().unwrap().capacity().retained_owner_key_tombstones, 1);
    let mut seeds = (1u8..=255).filter(|n| ![11,22].contains(n));
    for _ in 1..MAX_OWNER_KEYS { rotate_owner(&mut h, &key(seeds.next().unwrap())).unwrap(); }
    let before = h.actor.snapshot().unwrap();
    assert_eq!(before.capacity().retained_owner_key_tombstones, MAX_OWNER_KEYS);
    assert_eq!(before.capacity().max_owner_key_tombstones, MAX_OWNER_KEYS);
    assert_eq!(rotate_owner(&mut h, &key(seeds.next().unwrap())), Err(Error::Quota));
    unchanged(&before, &h.actor.snapshot().unwrap());
    assert_eq!(rotate_owner(&mut h, &key(11)), Err(Error::KeyReuse));
    unchanged(&before, &h.actor.snapshot().unwrap());
    h.admit(Operation::OwnHeartbeat, 42).unwrap();
    assert_eq!(h.actor.snapshot().unwrap().capacity().retained_owner_key_tombstones, MAX_OWNER_KEYS);
}

#[test]
fn owner_role_history_filestore_restores_original_pin_and_denies_validly_signed_legacy_reuse() {
    use crate::durable::{chained, Entry, Transition};
    use std::io::Write;
    let h = Harness::enrolled();
    let temp = DurableTemp::new();
    let mut actor = temp.create(&h);
    actor.offer(sign(h.offer.clone(), &h.owner), now()).unwrap();
    actor.consume(&h.token, sign(h.claim.clone(), &h.agent), now()).unwrap();
    actor.approve(sign(h.cert.clone(), &h.owner), now()).unwrap();
    let change = OwnerRotation { header: header(3), new_epoch: 2, new_key: public(&key(55)),
        issued_at: time(1000), expires_at: time(4600) };
    actor.rotate_owner(sign(change.clone(), &h.owner), sign(NewOwnerProof(change), &key(55)), now()).unwrap();
    drop(actor);
    let mut actor = temp.open(&h);
    let before = actor.snapshot().unwrap();
    assert_eq!(before.capacity().retained_owner_key_tombstones, 2);
    let mut offered = agent_offer(&h, public(&key(11)));
    offered.header = Header { owner_epoch: 2, checkpoint: 4, ..header(4) };
    assert_eq!(actor.offer(sign(offered, &key(55)), now()), Err(Error::KeyReuse));
    unchanged(&before, &actor.snapshot().unwrap());
    drop(actor);
    // Construct one authentic pre-strengthening journal transition, not a
    // state snapshot. Both signatures and the public hash chain are correct.
    let back = OwnerRotation { header: Header { owner_epoch: 2, checkpoint: 4, ..header(4) },
        new_epoch: 3, new_key: public(&key(11)), issued_at: time(1000), expires_at: time(4600) };
    let old = sign(back.clone(), &key(55));
    let new = sign(NewOwnerProof(back), &key(11));
    old.verify(public(&key(55))).unwrap();
    new.verify(public(&key(11))).unwrap();
    let entry = serde_json::to_vec(&Entry { at: time(1000), transition: Transition::RotateOwner { old, new } }).unwrap();
    let anchor: serde_json::Value = serde_json::from_slice(&std::fs::read(temp.0.join("anchor/00000000000000000004.json")).unwrap()).unwrap();
    let previous: Hash = serde_json::from_value(anchor["digest"].clone()).unwrap();
    let digest = chained(previous, &entry);
    for (path, bytes) in [
        (temp.0.join("data/00000000000000000005.json"), entry),
        (temp.0.join("anchor/00000000000000000005.json"), serde_json::to_vec(&json!({"revision":5,"digest":digest})).unwrap()),
    ] {
        let mut options = std::fs::OpenOptions::new(); options.create_new(true).write(true);
        #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
        let mut file = options.open(path).unwrap(); file.write_all(&bytes).unwrap(); file.sync_all().unwrap();
    }
    assert!(matches!(FileStore::open(temp.0.join("data"), temp.0.join("anchor"), DurableTemp::pin(&h)), Err(Error::KeyReuse)));
}
