//! One explicit original SSH configuration transaction. Worker still <=300s;
//! actual service execution is owned separately by the installed daemon.
use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Configure {
    expected_rev: u64,
    expected_pin: OwnerPin,
    delegation: Signed<CollectorServiceDelegation>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Observe {
    expected_rev: u64,
    expected_pin: OwnerPin,
    service_id: ResourceId,
    delegation_hash: Hash,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Revoke {
    expected_rev: u64,
    expected_pin: OwnerPin,
    revocation: Signed<CollectorServiceRevocation>,
}

pub(super) fn manage(
    mut session: Session<SqliteJournalStore>,
    mut pipe: Pipe,
    mut clock: Clock,
    frame: Envelope,
    config_raw: Vec<u8>,
    pin_path: PathBuf,
    pin_raw: Vec<u8>,
) -> Result<()> {
    if session.phase != Phase::RevocationProbed || !session.seen.insert(frame.request_id.clone()) {
        return Err(Error::WrongState);
    }
    let action = (|| -> Result<Value> {
        original_config(&config_raw, &pin_path, &pin_raw)?;
        let (id, hash, minimum) = match frame.kind.as_str() {
            "observeCollectorService" => {
                let input: Observe = decode(frame.payload.clone())?;
                session.expect_request(input.expected_rev, &input.expected_pin)?;
                session.checked(clock.now()?)?;
                (input.service_id, input.delegation_hash, input.expected_rev)
            }
            "revokeCollectorService" => {
                let input: Revoke = decode(frame.payload.clone())?;
                session.expect_request(input.expected_rev, &input.expected_pin)?;
                session.checked(clock.now()?)?;
                let id = input.revocation.body.service_id;
                let hash = input.revocation.body.delegation_hash;
                // Exactly ONE original CAS. The running daemon's later end
                // observation is separate and cannot be forged by this ACK.
                session.mutate(input.expected_rev, |actor| {
                    actor.revoke_collector_service(input.revocation, clock.now()?)
                })?;
                (
                    id,
                    hash,
                    input.expected_rev.checked_add(1).ok_or(Error::Quota)?,
                )
            }
            _ => return Err(Error::InvalidWire),
        };
        let store = session.store.take().ok_or(Error::WrongState)?;
        let mut actor = Actor::new(store)?;
        let actual = actor.observe_stored_collector_service(id, hash, clock.now()?)?;
        if actual.owner != session.expected_pin
            || actual.actor_revision < minimum
            || inspect_existing_node_identity(&session.node_dir, session.agent)? != session.node
        {
            return Err(Error::Conflict);
        }
        let p = &actual.policy.body;
        if p.agent_id != session.agent
            || p.recipient != session.node.public_key
            || p.key_epoch != session.node.key_epoch
        {
            return Err(Error::WrongClaim);
        }
        let revoked = actual.revocation_floor > p.revocation_epoch;
        if frame.kind == "revokeCollectorService" && !revoked {
            return Err(Error::WrongState);
        }
        let receipt = CollectorServiceReceipt {
            header: Header {
                version: 1,
                context: actual.owner.context.clone(),
                owner_epoch: actual.owner.epoch,
                checkpoint: actual.owner.checkpoint,
            },
            service_id: id,
            delegation_hash: hash,
            policy_revision: p.policy_revision,
            revocation_epoch: actual.revocation_floor,
            agent_id: session.agent,
            key_epoch: session.node.key_epoch,
            actor_revision: actual.actor_revision,
            effect: if revoked {
                CollectorServiceEffect::Revoked
            } else {
                CollectorServiceEffect::Configured
            },
            issued_at: clock.now()?.0,
        };
        let receipt = crate::provision::create_collector_service_receipt(
            &session.node_dir,
            session.agent,
            &actual.owner,
            &actual.policy,
            receipt,
            clock.now()?,
        )?;
        original_config(&config_raw, &pin_path, &pin_raw)?;
        let final_state = actor.observe_stored_collector_service(id, hash, clock.now()?)?;
        if final_state.owner != actual.owner
            || final_state.revocation_floor != actual.revocation_floor
            || final_state.policy.body != actual.policy.body
            || final_state.policy.signature != actual.policy.signature
        {
            return Err(Error::Conflict);
        }
        Ok(json!({"receipt":receipt,"claim":actual.claim,"delegation":actual.policy}))
    })();
    match action {
        Ok(payload) => reply_for(&mut pipe, &mut clock, &frame, payload),
        Err(error) => {
            let _=pipe.write(&json!({"format":FORMAT,"requestId":frame.request_id,"kind":"error",
                "payload":{"code":"serviceStateUnknown","effect":if session.effects{"unknown"}else{"notCommitted"}}}),&mut clock);
            Err(error)
        }
    }
}

pub(super) fn configure(
    mut session: Session<SqliteJournalStore>,
    mut pipe: Pipe,
    mut clock: Clock,
    frame: Envelope,
    config_raw: Vec<u8>,
    pin_path: PathBuf,
    pin_raw: Vec<u8>,
    lifetime_protocol: bool,
) -> Result<()> {
    if frame.kind != "configureCollectorService"
        || session.phase != Phase::Probed
        || !session.seen.insert(frame.request_id.clone())
    {
        return Err(Error::WrongState);
    }
    let action = (|| -> Result<(Value, CollectorServiceDelegation)> {
        let input: Configure = decode(frame.payload.clone())?;
        session.expect_request(input.expected_rev, &input.expected_pin)?;
        let state = session.checked(clock.now()?)?;
        let cert = state.certificate(session.agent).ok_or(Error::NotFound)?;
        if state.revoked(session.agent) {
            return Err(Error::Revoked);
        }
        let catalog = session.catalog.as_ref().ok_or(Error::CapabilityDenied)?;
        catalog.check()?;
        let p = &input.delegation.body;
        p.validate()?;
        p.check_credential_targets(&catalog.manifest)?;
        if catalog.disk.is_none()
            || p.collector_id != catalog.id
            || p.manifest_sha256 != catalog.hash
            || p.agent_id != session.agent
            || p.recipient != session.node.public_key
            || p.key_epoch != session.node.key_epoch
            || p.certificate_hash != cert.digest()?
        {
            return Err(Error::WrongClaim);
        }
        let proof = crate::provision::create_collector_service_claim(
            &session.node_dir,
            session.agent,
            &session.expected_pin,
            cert,
            &input.delegation,
            clock.now()?,
        )?;
        let hash = p.digest()?;
        let service = p.service_id;
        let revision = input.expected_rev.checked_add(1).ok_or(Error::Quota)?;
        original_config(&config_raw, &pin_path, &pin_raw)?;
        session.checked(clock.now()?)?;
        let policy = input.delegation;
        // Exactly ONE original expected-R CAS. A later daemon start observes
        // its own current revision; it never reuses this fixed RevisionFence.
        session.mutate(input.expected_rev, |actor| {
            actor.admit_collector_service(
                policy,
                proof,
                clock.now()?,
                &mut CollectorServiceClockObservation::observe_local,
            )
        })?;
        session.expected_rev = revision;
        let store = session.store.take().ok_or(Error::WrongState)?;
        let mut actor = Actor::new(store)?;
        let actual = actor.observe_current_collector_service(
            service,
            hash,
            clock.now()?,
            &mut CollectorServiceClockObservation::observe_local,
        )?;
        if actual.actor_revision < revision
            || actual.owner != session.expected_pin
            || inspect_existing_node_identity(&session.node_dir, session.agent)? != session.node
        {
            return Err(Error::Conflict);
        }
        // Readback may include a strictly later SAME-policy daemon quota
        // debit. It cannot substitute an owner/header/cert/policy replacement.
        original_config(&config_raw, &pin_path, &pin_raw)?;
        let catalog = session.catalog.as_ref().ok_or(Error::CapabilityDenied)?;
        catalog.check()?;
        let receipt = CollectorServiceReceipt {
            header: actual.policy.body.header.clone(),
            service_id: service,
            delegation_hash: hash,
            policy_revision: actual.policy.body.policy_revision,
            revocation_epoch: actual.revocation_floor,
            agent_id: session.agent,
            key_epoch: session.node.key_epoch,
            actor_revision: actual.actor_revision,
            effect: CollectorServiceEffect::Configured,
            issued_at: clock.now()?.0,
        };
        let receipt = crate::provision::create_collector_service_receipt(
            &session.node_dir,
            session.agent,
            &actual.owner,
            &actual.policy,
            receipt,
            clock.now()?,
        )?;
        original_config(&config_raw, &pin_path, &pin_raw)?;
        catalog.check()?;
        actor.observe_current_collector_service(
            service,
            hash,
            clock.now()?,
            &mut CollectorServiceClockObservation::observe_local,
        )?;
        Ok((
            json!({"receipt":receipt,"claim":actual.claim}),
            actual.policy.body.clone(),
        ))
    })();
    match action {
        Ok((payload, policy)) => {
            reply_for(&mut pipe, &mut clock, &frame, payload)?;
            if !policy.credential_targets.is_empty() {
                super::service_handoff_control::serve(
                    &mut session,
                    &mut pipe,
                    &mut clock,
                    &policy,
                    &config_raw,
                    &pin_path,
                    &pin_raw,
                    lifetime_protocol,
                )?;
            }
            Ok(())
        }
        Err(error) => {
            let code = match error {
                Error::Expired | Error::InvalidTime => "expired",
                Error::Conflict => "contextChanged",
                Error::Storage => "storageUnavailable",
                Error::Quota => "capacity",
                Error::WrongState => "stateUnknown",
                _ => "denied",
            };
            let _=pipe.write(&json!({"format":FORMAT,"requestId":frame.request_id,"kind":"error",
                "payload":{"code":code,"effect":if session.effects{"unknown"}else{"notCommitted"}}}),&mut clock);
            Err(error)
        }
    }
}
