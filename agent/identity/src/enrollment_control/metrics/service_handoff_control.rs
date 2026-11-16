//! SAME original configure SSH worker, public/encrypted handoff ONLY. This
//! process never owns a service Permit, decrypts material or cleans a leaf.
use super::service_credential_handoff::{
    open_physical_namespace, read_original_reference_physical, write_encrypted_frame_physical,
    EncryptedHandoffFrame, RecipientReference,
};
use super::*;

pub(super) fn active(
    session: &Session<SqliteJournalStore>,
    policy: &CollectorServiceDelegation,
    config_raw: &[u8],
    pin_path: &Path,
    pin_raw: &[u8],
    clock: &mut Clock,
) -> Result<ActiveCollectorServiceObservation> {
    original_config(config_raw, pin_path, pin_raw)?;
    let catalog = session.catalog.as_ref().ok_or(Error::CapabilityDenied)?;
    catalog.check()?;
    if inspect_existing_node_identity(&session.node_dir, session.agent)? != session.node {
        return Err(Error::WrongClaim);
    }
    let config: Config = serde_json::from_slice(config_raw).map_err(|_| Error::InvalidWire)?;
    let genesis: OwnerPin = serde_json::from_slice(pin_raw).map_err(|_| Error::InvalidWire)?;
    let store = SqliteJournalStore::open(
        &config.db_dir,
        &config.anchor_dir,
        genesis,
        SqliteStorageBudget::default(),
    )?;
    let mut actor = Actor::new(store)?;
    let observed = actor.observe_active_collector_service(
        policy.service_id,
        policy.digest()?,
        clock.now()?,
        &mut CollectorServiceClockObservation::observe_local,
    )?;
    if observed.stored.policy.body != *policy
        || observed.stored.owner != session.expected_pin
        || catalog.hash != policy.manifest_sha256
        || catalog.id != policy.collector_id
        || policy.agent_id != session.agent
        || policy.key_epoch != session.node.key_epoch
        || policy.recipient != session.node.public_key
    {
        return Err(Error::WrongClaim);
    }
    Ok(observed)
}
fn reference(
    session: &Session<SqliteJournalStore>,
    policy: &CollectorServiceDelegation,
    config_raw: &[u8],
    pin_path: &Path,
    pin_raw: &[u8],
    clock: &mut Clock,
) -> Result<RecipientReference> {
    let before = active(session, policy, config_raw, pin_path, pin_raw, clock)?;
    let namespace = hex::encode(before.original_start_nonce.as_bytes());
    let directory = open_physical_namespace(&namespace)?;
    let held = read_original_reference_physical(&directory, &namespace)?;
    let reference = held.recipient_reference()?;
    reference.check_metadata_against(policy, clock.now()?)?;
    if reference.attestation().body.original_start_nonce != before.original_start_nonce
        || reference.namespace_id() != namespace
    {
        return Err(Error::WrongClaim);
    }
    held.recheck_physical(&directory, &namespace)?;
    let after = active(session, policy, config_raw, pin_path, pin_raw, clock)?;
    if after.original_start_nonce != before.original_start_nonce {
        return Err(Error::Conflict);
    }
    held.recheck_physical(&directory, &namespace)?;
    Ok(reference)
}
pub(super) fn serve(
    session: &mut Session<SqliteJournalStore>,
    pipe: &mut Pipe,
    clock: &mut Clock,
    policy: &CollectorServiceDelegation,
    config_raw: &[u8],
    pin_path: &Path,
    pin_raw: &[u8],
    lifetime_protocol: bool,
) -> Result<()> {
    if policy.credential_targets.is_empty() || policy.credential_targets.len() > 2 {
        return Err(Error::WrongState);
    }
    let request = parse(&pipe.read(clock)?)?;
    if request.kind != "collectorServiceRecipient"
        || request.payload != json!({})
        || !session.seen.insert(request.request_id.clone())
    {
        return Err(Error::WrongState);
    }
    // Read-only wait shares ONE fixed end. No fresh Actor grant/Permit/start is
    // minted here and the supervisor's signed BOOT/policy end is unchanged.
    let end = Instant::now() + Duration::from_secs(3);
    let original = loop {
        clock.now()?;
        pipe.check()?;
        original_config(config_raw, pin_path, pin_raw)?;
        match reference(session, policy, config_raw, pin_path, pin_raw, clock) {
            Ok(value) => break value,
            Err(Error::WrongState | Error::Storage) if Instant::now() < end => {
                std::thread::sleep(Duration::from_millis(20))
            }
            Err(error) => return Err(error),
        }
    };
    reply_for(
        pipe,
        clock,
        &request,
        serde_json::to_value(&original).map_err(|_| Error::InvalidWire)?,
    )?;
    let first = parse(&pipe.read(clock)?)?;
    if lifetime_protocol != (first.kind=="stageCollectorServiceLifetimeFrame") {return Err(Error::WrongState);}
    if first.kind == "stageCollectorServiceLifetimeFrame" {
        return super::service_lifetime_control::serve(session,pipe,clock,policy,config_raw,pin_path,pin_raw,&original,first);
    }
    let mut first = Some(first);
    for slot in 0..policy.credential_targets.len() {
        let request = match first.take() {Some(request)=>request,None=>parse(&pipe.read(clock)?)?};
        if request.kind != "deliverCollectorServiceCredential"
            || !session.seen.insert(request.request_id.clone())
        {
            return Err(Error::WrongState);
        }
        let before = active(session, policy, config_raw, pin_path, pin_raw, clock)?;
        if before.original_start_nonce != original.attestation().body.original_start_nonce {
            return Err(Error::Conflict);
        }
        let frame = EncryptedHandoffFrame::from_json(&json_body(&request.payload)?)?;
        if serde_json::to_value(&frame)
            .map_err(|_| Error::InvalidWire)?
            .get("slot")
            .and_then(Value::as_u64)
            != Some(slot as u64)
        {
            return Err(Error::WrongClaim);
        }
        let namespace = hex::encode(before.original_start_nonce.as_bytes());
        let directory = open_physical_namespace(&namespace)?;
        frame.check_owner_metadata(
            &original,
            &before.stored.owner,
            policy,
            &session
                .catalog
                .as_ref()
                .ok_or(Error::CapabilityDenied)?
                .manifest,
            clock.now()?,
        )?;
        // Commit v1 selection BEFORE any legacy physical handoff. A concurrent
        // lifetime commit cannot then switch this original active start.
        let config:Config=serde_json::from_slice(config_raw).map_err(|_|Error::InvalidWire)?;
        let genesis:OwnerPin=serde_json::from_slice(pin_raw).map_err(|_|Error::InvalidWire)?;
        let store=SqliteJournalStore::open(&config.db_dir,&config.anchor_dir,genesis,SqliteStorageBudget::default())?;
        let mut actor=Actor::new(store)?;
        actor.select_collector_single_start(policy.service_id,policy.digest()?,before.original_start_nonce,
            clock.now()?,&mut CollectorServiceClockObservation::observe_local)?;
        let held = write_encrypted_frame_physical(
            &directory,
            &frame,
            &original,
            &before.stored.owner,
            policy,
            &session
                .catalog
                .as_ref()
                .ok_or(Error::CapabilityDenied)?
                .manifest,
            clock.now()?,
        )?;
        held.recheck_physical(&directory, &namespace)?;
        let after = active(session, policy, config_raw, pin_path, pin_raw, clock)?;
        if after.original_start_nonce != before.original_start_nonce {
            return Err(Error::Conflict);
        }
        held.recheck_physical(&directory, &namespace)?;
        // Physical encrypted acceptance is NEVER material/launch/delivery ACK.
        reply_for(
            pipe,
            clock,
            &request,
            json!({"kind":"collectorCredentialHandoff","slot":slot,
            "serviceId":policy.service_id,"delegationHash":policy.digest()?,"encryptedAccepted":true,
            "materialState":"notObserved","runtimeState":"notObserved"}),
        )?;
    }
    Ok(())
}
