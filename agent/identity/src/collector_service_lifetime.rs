//! Original Actor journal lifecycle. Data here never constructs a Permit/end.
use super::*;
use crate::CollectorServiceCredentialAuthorization;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum CollectorCredentialPurpose { SingleStart, ServiceLifetime }

/// Ciphertext/public metadata only; admitted atomically by the original CAS.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StoredCollectorServiceCredentialSet {
    pub(crate) authorization: Signed<CollectorServiceCredentialAuthorization>,
    pub(crate) capsules: Vec<serde_json::Value>,
}
#[derive(Clone)]
pub(super) struct LifetimeAdmission { pub(super) set: StoredCollectorServiceCredentialSet, pub(super) revision: u64 }

impl StoredCollectorServiceCredentialSet {
    pub(crate) fn decoded(&self) -> Result<Vec<cc_crypto_core::ServiceCredentialCapsule>> {
        if self.capsules.is_empty() || self.capsules.len() > 2
            || serde_json::to_vec(self).map_err(|_| Error::InvalidWire)?.len() > 32768
        { return Err(Error::Quota); }
        self.capsules.iter().map(|value| {
            let raw = serde_json::to_vec(value).map_err(|_| Error::InvalidWire)?;
            if raw.len() > 8192 { return Err(Error::Quota); }
            cc_crypto_core::ServiceCredentialCapsule::from_json(&raw).map_err(|_| Error::InvalidWire)
        }).collect()
    }
    pub(crate) fn capsule_hashes(&self) -> Result<Vec<Hash>> {
        use sha2::Digest;
        self.decoded()?.iter().map(|capsule| {
            let raw = capsule.to_json().map_err(|_| Error::InvalidWire)?;
            Ok(Hash::from_bytes(sha2::Sha256::digest(raw).into()))
        }).collect()
    }
    pub(crate) fn check(&self, pin: &OwnerPin, base: &CollectorServiceDelegation,
        original_clock: &CollectorServiceClockStamp, manifest: &serde_json::Value, now: Now) -> Result<()> {
        use cc_agent_node_wire::Statement as NativeStatement;
        self.authorization.verify(pin.key)?;
        self.authorization.body.check_original(base, original_clock, manifest, now)?;
        let native: cc_agent_node_wire::Signed<cc_agent_node_wire::CollectorServiceCredentialAuthorization> =
            serde_json::from_slice(&serde_json::to_vec(&self.authorization).map_err(|_| Error::InvalidWire)?)
                .map_err(|_| Error::InvalidWire)?;
        if native.body.signing_bytes().map_err(|_| Error::InvalidWire)? != self.authorization.body.signing_bytes()?
            || native.signature.as_bytes() != self.authorization.signature.as_bytes() { return Err(Error::WrongClaim); }
        let native_pin: cc_agent_node_wire::OwnerPin = serde_json::from_value(
            serde_json::to_value(pin).map_err(|_| Error::InvalidWire)?).map_err(|_| Error::InvalidWire)?;
        let native_base: cc_agent_node_wire::CollectorServiceDelegation = serde_json::from_value(
            serde_json::to_value(base).map_err(|_| Error::InvalidWire)?).map_err(|_| Error::InvalidWire)?;
        if native_base.digest().map_err(|_| Error::InvalidWire)?.as_bytes() != base.digest()?.as_bytes()
            || serde_json::to_value(&native_pin).map_err(|_| Error::InvalidWire)? != serde_json::to_value(pin).map_err(|_| Error::InvalidWire)?
        { return Err(Error::WrongClaim); }
        let capsules = self.decoded()?;
        let now = cc_agent_node_wire::Now(cc_agent_node_wire::Timestamp::new(now.0.seconds()).map_err(|_| Error::InvalidTime)?);
        let expected = cc_crypto_core::preflight_collector_service_credential_set(&native, &capsules,
            &native_pin, &native_base, &native.body.admission.body.clock, manifest, now).map_err(|_| Error::WrongClaim)?;
        for (capsule, binding) in capsules.iter().zip(expected) {
            // Authenticate ciphertext BEFORE persistent admission. This uses
            // the existing capsule transcript and never decrypts any material.
            capsule.verify_owner_metadata(&binding, pin.key.as_bytes(), now.0.seconds()).map_err(|_| Error::WrongClaim)?;
        }
        Ok(())
    }
}

pub(crate) struct LifetimeReadback {
    pub(crate) set: StoredCollectorServiceCredentialSet,
    pub(crate) policy: Signed<CollectorServiceDelegation>,
    pub(crate) pin: OwnerPin,
    pub(crate) certificate: Certificate,
    pub(crate) original_start: Nonce,
    pub(crate) admitted_revision: u64,
    pub(crate) observed_revision: u64,
}
impl<S: CompareAndSwap> Actor<S> {
    pub(crate) fn select_collector_single_start(&mut self, service: ResourceId, hash: Hash, start: Nonce,
        now: Now, clock: &mut impl FnMut() -> Result<CollectorServiceClockObservation>) -> Result<()> {
        let observed = clock()?;
        if observed.wall_time < now.0 || observed.wall_time < self.last_observed_time { return Err(Error::InvalidTime); }
        let now = Now(observed.wall_time);
        let mut state = self.read(now)?;
        let record = state.collector_services.get(&service).ok_or(Error::NotFound)?;
        current(&state, record, now, &observed)?;
        if record.hash != hash || record.active != Some(start) || record.lifetime.is_some()
            || record.credential_purpose == Some(CollectorCredentialPurpose::ServiceLifetime) { return Err(Error::WrongState); }
        if record.credential_purpose == Some(CollectorCredentialPurpose::SingleStart) { return Ok(()); }
        state.collector_services.get_mut(&service).ok_or(Error::NotFound)?.credential_purpose = Some(CollectorCredentialPurpose::SingleStart);
        self.commit(state, now, Transition::CollectorServiceSingleStartSelected {
            service_id: service, delegation_hash: hash, start, observed_boot: observed.boot_id, observed_boottime_ns: observed.boottime_ns })
    }
    pub(crate) fn admit_collector_service_credential_set(&mut self, set: StoredCollectorServiceCredentialSet,
        manifest: &serde_json::Value, now: Now,
        clock: &mut impl FnMut() -> Result<CollectorServiceClockObservation>) -> Result<LifetimeReadback> {
        let observed = clock()?;
        if observed.wall_time < now.0 || observed.wall_time < self.last_observed_time { return Err(Error::InvalidTime); }
        let now = Now(observed.wall_time);
        let mut state = self.read(now)?;
        let service = set.authorization.body.base.body.service_id;
        let record = state.collector_services.get(&service).ok_or(Error::NotFound)?;
        current(&state, record, now, &observed)?;
        let a = &set.authorization.body;
        if record.lifetime.is_some() || record.credential_purpose.is_some()
            || record.active != Some(a.recipient.body.original_start_nonce)
            || a.base.body != record.policy.body || a.base.signature.as_bytes() != record.policy.signature.as_bytes()
            || a.admission.body != record.claim.body || a.admission.signature.as_bytes() != record.claim.signature.as_bytes()
            || now.0 >= a.recipient.body.expires_at { return Err(Error::WrongState); }
        set.check(&state.pin, &record.policy.body, &record.clock, manifest, now)?;
        let revision = state.revision.checked_add(1).ok_or(Error::Quota)?;
        let record = state.collector_services.get_mut(&service).ok_or(Error::NotFound)?;
        record.lifetime = Some(LifetimeAdmission { set: set.clone(), revision });
        record.credential_purpose = Some(CollectorCredentialPurpose::ServiceLifetime);
        let hash = record.hash;
        self.commit(state, now, Transition::CollectorServiceCredentialSetAdmit { set,
            manifest: manifest.clone(), observed_boot: observed.boot_id, observed_boottime_ns: observed.boottime_ns })?;
        // CAS failure/unknown/lost ACK never constructs an admission receipt.
        self.readback_collector_service_lifetime(service, hash, now, clock)
    }
    pub(crate) fn readback_collector_service_lifetime(&mut self, service: ResourceId, hash: Hash,
        now: Now, clock: &mut impl FnMut() -> Result<CollectorServiceClockObservation>) -> Result<LifetimeReadback> {
        let observed = clock()?;
        if observed.wall_time < now.0 || observed.wall_time < self.last_observed_time { return Err(Error::InvalidTime); }
        let state = self.read(Now(observed.wall_time))?;
        let record = state.collector_services.get(&service).ok_or(Error::NotFound)?;
        current(&state, record, Now(observed.wall_time), &observed)?;
        if record.hash != hash || record.credential_purpose != Some(CollectorCredentialPurpose::ServiceLifetime) { return Err(Error::WrongState); }
        let admission = record.lifetime.as_ref().ok_or(Error::WrongState)?;
        if state.revision != admission.revision { return Err(Error::Conflict); }
        match state.latest.as_ref().map(|e| &e.transition) {
            Some(Transition::CollectorServiceCredentialSetAdmit { set, .. })
                if set.authorization.body.digest()? == admission.set.authorization.body.digest()?
                    && set.capsule_hashes()? == admission.set.capsule_hashes()? => (),
            _ => return Err(Error::UncertainCommit),
        }
        Ok(LifetimeReadback { set: admission.set.clone(), policy: record.policy.clone(), pin: state.pin.clone(),
            certificate: state.certificate(record.policy.body.agent_id).ok_or(Error::NotFound)?.clone(),
            original_start: record.active.ok_or(Error::WrongState)?, admitted_revision: admission.revision,
            observed_revision: state.revision })
    }
    pub(crate) fn collector_service_credential_set_original(&mut self, permit: &CollectorServicePermit,
        now: Now, clock: &mut impl FnMut() -> Result<CollectorServiceClockObservation>)
        -> Result<Option<StoredCollectorServiceCredentialSet>> {
        self.check_collector_service(permit, now, clock)?;
        let observed = clock()?;
        if observed.wall_time < now.0 || observed.wall_time < self.last_observed_time { return Err(Error::InvalidTime); }
        let state = self.read(Now(observed.wall_time))?;
        let record = state.collector_services.get(&permit.service_id).ok_or(Error::NotFound)?;
        current(&state, record, Now(observed.wall_time), &observed)?;
        if record.hash != permit.delegation_hash || record.active != Some(permit.start) || record.starts != permit.start_count {
            return Err(Error::WrongState);
        }
        Ok(record.lifetime.as_ref().map(|a| a.set.clone()))
    }
    pub(crate) fn replay_collector_service_lifetime(&mut self, set: StoredCollectorServiceCredentialSet,
        manifest: serde_json::Value, boot: ResourceId, at: u64, now: Now) -> Result<()> {
        self.admit_collector_service_credential_set(set, &manifest, now,
            &mut || CollectorServiceClockObservation::journal(boot, at, now.0)).map(|_| ())
    }
    pub(crate) fn replay_collector_single_start(&mut self, service: ResourceId, hash: Hash, start: Nonce,
        boot: ResourceId, at: u64, now: Now) -> Result<()> {
        self.select_collector_single_start(service, hash, start, now,
            &mut || CollectorServiceClockObservation::journal(boot, at, now.0))
    }
}
