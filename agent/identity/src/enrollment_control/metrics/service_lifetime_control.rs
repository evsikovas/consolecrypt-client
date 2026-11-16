//! SAME configure SSH worker. Ciphertext stage in bounded memory, one journal CAS.
use super::*;
use crate::{StoredCollectorServiceCredentialSet, CollectorServiceCredentialSetReceipt};
use super::service_credential_handoff::RecipientReference;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Stage { version: u8, sequence: usize, frame: cc_crypto_core::CollectorServiceCredentialFrame }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Commit { version: u8, authorization_digest: Hash }

pub(crate) struct PreparedLifetimeReceipt<'a> {
    actor: &'a mut Actor<SqliteJournalStore>,
    manifest: &'a Value,
    readback: crate::LifetimeReadback,
    body: CollectorServiceCredentialSetReceipt,
}
impl PreparedLifetimeReceipt<'_> {
    pub(crate) fn original(&self) -> (&CollectorServiceCredentialSetReceipt, &crate::LifetimeReadback) { (&self.body, &self.readback) }
    pub(crate) fn recheck(&mut self) -> Result<Now> {
        let now = super::service_supervisor::actual_now()?;
        let current = self.actor.readback_collector_service_lifetime(self.body.service_id,
            self.body.base_delegation_hash, now, &mut CollectorServiceClockObservation::observe_local)?;
        current.set.check(&current.pin, &current.policy.body, &current.set.authorization.body.admission.body.clock, self.manifest, now)?;
        if current.observed_revision != self.body.actor_revision || current.admitted_revision != self.body.admitted_revision
            || current.original_start != self.body.original_start_nonce
            || current.set.authorization.body.digest()? != self.body.authorization_digest
            || current.set.capsule_hashes()? != self.body.ordered_capsule_hashes
            || current.pin != self.readback.pin || current.certificate != self.readback.certificate
            || now.0 < self.body.issued_at || now.0 >= self.body.expires_at { return Err(Error::Conflict); }
        Ok(now)
    }
    pub(crate) fn finish(mut self, signed: Signed<CollectorServiceCredentialSetReceipt>)
        -> Result<Signed<CollectorServiceCredentialSetReceipt>> {
        if signed.body != self.body { return Err(Error::WrongClaim); }
        signed.verify(self.readback.policy.body.recipient)?;
        self.recheck()?;
        Ok(signed)
    }
}
pub(super) fn serve(
    session: &mut Session<SqliteJournalStore>, pipe: &mut Pipe, clock: &mut Clock,
    policy: &CollectorServiceDelegation, config_raw: &[u8], pin_path: &Path, pin_raw: &[u8],
    original: &RecipientReference, first: Envelope,
) -> Result<()> {
    let catalog = session.catalog.as_ref().ok_or(Error::CapabilityDenied)?;
    let mut frames = Vec::new();
    let expected_count = 1 + policy.credential_targets.len();
    let mut next = Some(first);
    let mut authorization_digest = None;
    for sequence in 0..expected_count {
        let request = match next.take() { Some(request) => request, None => parse(&pipe.read(clock)?)? };
        if request.kind != "stageCollectorServiceLifetimeFrame" || !session.seen.insert(request.request_id.clone()) { return Err(Error::WrongState); }
        let active = super::service_handoff_control::active(session, policy, config_raw, pin_path, pin_raw, clock)?;
        if active.original_start_nonce != original.attestation().body.original_start_nonce { return Err(Error::Conflict); }
        let stage: Stage = decode(request.payload.clone())?;
        if stage.version != 1 || stage.sequence != sequence { return Err(Error::WrongClaim); }
        let raw = stage.frame.to_json().map_err(|_| Error::InvalidWire)?;
        if raw.len() > 12288 { return Err(Error::Quota); }
        // The first frame MUST be a signed new authorization bound to the
        // actual reference published by the original supervisor, not caller ID.
        if sequence == 0 {
            let value: Value = serde_json::from_slice(&raw).map_err(|_| Error::InvalidWire)?;
            let a: Signed<crate::CollectorServiceCredentialAuthorization> = serde_json::from_value(
                value.get("payload").and_then(|p|p.get("authorization")).ok_or(Error::WrongClaim)?.clone())
                .map_err(|_| Error::InvalidWire)?;
            a.verify(active.stored.owner.key)?;
            a.body.check_original(policy, &active.stored.claim.body.clock, &catalog.manifest, clock.now()?)?;
            if a.body.recipient.body != original.attestation().body
                || a.body.recipient.signature.as_bytes() != original.attestation().signature.as_bytes() { return Err(Error::WrongClaim); }
            authorization_digest = Some(a.body.digest()?);
        }
        frames.push(stage.frame);
        let digest = authorization_digest.ok_or(Error::WrongState)?;
        reply_for(pipe, clock, &request, json!({"kind":"collectorServiceLifetimeFrameStaged", "sequence":sequence,
            "authorizationDigest":digest}))?;
    }
    let (native, capsules) = cc_crypto_core::CollectorServiceCredentialFrame::into_complete_set(frames).map_err(|_| Error::WrongClaim)?;
    let a: Signed<crate::CollectorServiceCredentialAuthorization> = serde_json::from_slice(
        &serde_json::to_vec(&native).map_err(|_| Error::InvalidWire)?).map_err(|_| Error::InvalidWire)?;
    if a.body.signing_bytes()? != cc_agent_node_wire::Statement::signing_bytes(&native.body).map_err(|_| Error::InvalidWire)?
        || a.signature.as_bytes() != native.signature.as_bytes() { return Err(Error::WrongClaim); }
    let mut values = Vec::new();
    for capsule in capsules { values.push(serde_json::from_slice(&capsule.to_json().map_err(|_| Error::InvalidWire)?).map_err(|_| Error::InvalidWire)?); }
    let set = StoredCollectorServiceCredentialSet { authorization: a, capsules: values };
    let request = parse(&pipe.read(clock)?)?;
    if request.kind != "admitCollectorServiceLifetimeSet" || !session.seen.insert(request.request_id.clone()) { return Err(Error::WrongState); }
    let commit: Commit = decode(request.payload.clone())?;
    if commit.version != 1 || commit.authorization_digest != set.authorization.body.digest()? { return Err(Error::WrongClaim); }
    let active = super::service_handoff_control::active(session, policy, config_raw, pin_path, pin_raw, clock)?;
    if active.original_start_nonce != original.attestation().body.original_start_nonce { return Err(Error::Conflict); }
    let config: Config = serde_json::from_slice(config_raw).map_err(|_| Error::InvalidWire)?;
    let genesis: OwnerPin = serde_json::from_slice(pin_raw).map_err(|_| Error::InvalidWire)?;
    let store = SqliteJournalStore::open(&config.db_dir, &config.anchor_dir, genesis, SqliteStorageBudget::default())?;
    let mut actor = Actor::new(store)?;
    let now = super::service_supervisor::actual_now()?;
    let readback = actor.admit_collector_service_credential_set(set, &catalog.manifest, now,
        &mut CollectorServiceClockObservation::observe_local)?;
    let body = CollectorServiceCredentialSetReceipt { header: policy.header.clone(), agent_id: policy.agent_id,
        key_epoch: policy.key_epoch, service_id: policy.service_id, base_delegation_hash: policy.digest()?,
        authorization_digest: readback.set.authorization.body.digest()?, ordered_capsule_hashes: readback.set.capsule_hashes()?,
        original_start_nonce: readback.original_start, admitted_revision: readback.admitted_revision,
        actor_revision: readback.observed_revision, issued_at: now.0,
        expires_at: now.0.checked_add(crate::PROOF_TTL)?.min(policy.expires_at) };
    body.validate()?;
    let prepared = PreparedLifetimeReceipt { actor: &mut actor, manifest: &catalog.manifest, readback, body };
    let receipt = crate::provision::sign_collector_service_lifetime_receipt(&session.node_dir, prepared)?;
    original_config(config_raw, pin_path, pin_raw)?;
    catalog.check()?;
    if inspect_existing_node_identity(&session.node_dir, session.agent)? != session.node { return Err(Error::WrongClaim); }
    // Only exact Node-signed, fresh SQL/journal readback leaves the worker.
    reply_for(pipe, clock, &request, serde_json::to_value(receipt).map_err(|_| Error::InvalidWire)?)
}
