//! SAME supervisor/Permit only. One public recipient proof, then at most two
//! separately bounded encrypted handoffs. This is not a metrics payload queue.
use super::service_credential_handoff::{
    read_registered_encrypted_frame_physical, write_registered_reference_physical,
    RecipientReference,
};
use super::*;
use crate::collector_credentials::{sign_original_recipient, CollectorCredentialRecipient};

pub(super) struct Material {
    pub(super) scrape: Vec<u8>,
    pub(super) expires: Timestamp,
    pub(super) deadline: u64,
}
pub(super) fn prepare(
    actor: &mut Actor<SqliteJournalStore>,
    permit: &CollectorServicePermit,
    files: &mut Files,
    catalog: &Catalog,
    node: &Path,
    input: &mut File,
    pending: &mut Vec<u8>,
    checks: &super::service_supervisor::ReadChecks<'_>,
) -> Result<Material> {
    let now = super::service_supervisor::actual_now()?;
    let policy = actor.check_collector_service_waiting(permit, now, Instant::now() + Duration::from_secs(3), &mut || checks.clock(input, pending))?;
    policy.check_credential_targets(&catalog.manifest)?;
    if policy.credential_targets.is_empty() {
        let access = actor.borrow_collector_service_waiting(permit, now, Instant::now() + Duration::from_secs(3), &mut || checks.clock(input, pending))?;
        return Ok(Material {
            scrape: catalog.scrape()?,
            expires: access.expires_at(),
            deadline: access.deadline_boottime_ns(),
        });
    }
    if let Some(stored) = actor.collector_service_credential_set_original_waiting(permit, now, Instant::now() + Duration::from_secs(3), &mut || checks.clock(input, pending))? {
        // On reopen a missing/corrupt key FAILS; never generate a replacement.
        let mut recipient = CollectorCredentialRecipient::open_existing_original_waiting(actor, permit,
            stored.authorization.body.recipient.body.recipient_id, Instant::now() + Duration::from_secs(3), &mut || checks.clock(input, pending).map(|v| v.now()))?;
        return super::service_lifetime_material::prepare(actor, permit, files, catalog, &mut recipient, stored, input, pending, checks);
    }
    files.register_service_handoff()?;
    // This separate X25519 identity is scoped to the actual certified Node and
    // owner. A missing/corrupt existing key never falls back to regeneration.
    let key = Path::new("/var/lib/consolecrypt-agent/collector-credentials/recipient.key");
    let mut recipient = match fs::symlink_metadata(key) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            CollectorCredentialRecipient::create_original_waiting(
                actor,
                permit,
                policy.service_id,
                Instant::now() + Duration::from_secs(3), &mut || checks.clock(input, pending).map(|v| v.now()),
            )?
        }
        Ok(_) => CollectorCredentialRecipient::open_existing_original_waiting(
            actor,
            permit,
            policy.service_id,
            Instant::now() + Duration::from_secs(3), &mut || checks.clock(input, pending).map(|v| v.now()),
        )?,
        Err(_) => return Err(Error::Storage),
    };
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| Error::Entropy)?;
    let prepared = recipient.prepare_attestation_waiting(
        actor,
        permit,
        Nonce::from_bytes(bytes),
        Instant::now() + Duration::from_secs(3), &mut || checks.clock(input, pending).map(|v| v.now()),
    )?;
    let proof =
        sign_original_recipient(node, prepared, &mut || checks.clock(input, pending).map(|v| v.now()))?;
    let reference = RecipientReference::new(files.session.clone(), proof.clone())?;
    {
        let now = super::service_supervisor::actual_now()?;
        let mut access = actor.borrow_collector_service_waiting(permit, now, Instant::now() + Duration::from_secs(3), &mut || checks.clock(input, pending))?;
        files.with_original_credential_session(
            catalog,
            &mut access,
            now,
            &mut || checks.clock(input, pending),
            |writer| {
                let writer_now = writer.recheck()?;
                let held = {
                    let namespace = writer.handoff_namespace()?;
                    write_registered_reference_physical(
                        &namespace, &reference, permit, &policy, writer_now,
                    )?
                };
                writer.retain_original_reference(permit, &held)?;
                Ok(())
            },
        )?;
    }
    // ONE fixed wait end for the original challenge, never per-frame renewal.
    let end = Instant::now() + Duration::from_secs(60);
    let mut envelopes = Vec::new();
    while envelopes.len() < policy.credential_targets.len() {
        if let Some(stored) = actor.collector_service_credential_set_original_waiting(permit,
            super::service_supervisor::actual_now()?, end.min(Instant::now() + Duration::from_secs(3)), &mut || checks.clock_before(input, pending, end, proof.body.expires_at))? {
            if !envelopes.is_empty() { return Err(Error::WrongState); }
            return super::service_lifetime_material::prepare(actor,permit,files,catalog,&mut recipient,stored,input,pending,checks);
        }
        catalog.check()?;
        let now = super::service_supervisor::actual_now()?;
        actor.check_collector_service_waiting(permit, now, end.min(Instant::now() + Duration::from_secs(3)), &mut || checks.clock_before(input, pending, end, proof.body.expires_at))?;
        if now.0 >= proof.body.expires_at
            || Instant::now() >= end
            || !super::service_supervisor::launcher_alive(input, pending)?
        {
            return Err(Error::Expired);
        }
        let slot = envelopes.len() as u8;
        let owner = actor.snapshot_collector_service_waiting(super::service_supervisor::actual_now()?,
            end.min(Instant::now() + Duration::from_secs(3)), &mut || checks.clock_before(input, pending, end, proof.body.expires_at))?.owner().clone();
        let mut access = actor.borrow_collector_service_waiting(permit, now, end.min(Instant::now() + Duration::from_secs(3)), &mut || checks.clock_before(input, pending, end, proof.body.expires_at))?;
        let frame = files.with_original_credential_session(
            catalog,
            &mut access,
            now,
            &mut || checks.clock_before(input, pending, end, proof.body.expires_at),
            |writer| {
                let checked_now = writer.recheck()?;
                let held = {
                    let namespace = writer.handoff_namespace()?;
                    let path = namespace.registered_leaf(slot + 1)?;
                    match fs::symlink_metadata(path) {
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                        Err(_) => return Err(Error::Storage),
                        Ok(_) => {}
                    }
                    read_registered_encrypted_frame_physical(
                        &namespace,
                        permit,
                        &reference,
                        slot,
                        &owner,
                        &policy,
                        &catalog.manifest,
                        checked_now,
                    )?
                };
                Ok(Some(
                    writer.retain_encrypted_frame(permit, &reference, slot, &held)?,
                ))
            },
        )?;
        if let Some(frame) = frame {
            envelopes.push(frame.into_envelope());
        } else {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    if actor.collector_service_credential_set_original_waiting(permit, super::service_supervisor::actual_now()?, Instant::now() + Duration::from_secs(3), &mut || checks.clock(input, pending))?.is_some() {
        return Err(Error::WrongState); // Never select v1 after lifetime admission.
    }
    let mut opened = Vec::new();
    for envelope in envelopes {
        let (native_scope, capsule) = envelope.into_parts();
        let bytes = serde_json::to_vec(&native_scope).map_err(|_| Error::InvalidWire)?;
        let scope: Signed<CollectorCredentialScope> =
            serde_json::from_slice(&bytes).map_err(|_| Error::InvalidWire)?;
        // Data codec only; no Actor/Permit/Access is ever deserialized.
        if scope.body.signing_bytes()?
            != cc_agent_node_wire::Statement::signing_bytes(&native_scope.body)
                .map_err(|_| Error::InvalidWire)?
            || scope.signature.as_bytes() != native_scope.signature.as_bytes()
        {
            return Err(Error::WrongClaim);
        }
        opened.push(recipient.open_original_waiting(
            actor,
            permit,
            &scope,
            &proof,
            &capsule,
            &catalog.manifest,
            Instant::now() + Duration::from_secs(3), &mut || checks.clock(input, pending).map(|v| v.now()),
        )?);
    }
    let now = super::service_supervisor::actual_now()?;
    let mut access = actor.borrow_collector_service_waiting(permit, now, Instant::now() + Duration::from_secs(3), &mut || checks.clock(input, pending))?;
    let cfg = super::credential_capsules::build_registered_scrape(
        files,
        catalog,
        &mut access,
        opened,
        now,
        &mut || checks.clock(input, pending),
    )?;
    Ok(Material {
        scrape: cfg.raw,
        expires: cfg.expires_at,
        deadline: cfg.deadline_boottime_ns,
    })
}
