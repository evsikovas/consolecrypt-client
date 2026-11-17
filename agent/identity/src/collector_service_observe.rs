//! Original signed-record readback. This object is DATA, never a service Permit.
use super::*;

pub(crate) struct CollectorServiceStoredObservation {
    pub(crate) policy: Signed<CollectorServiceDelegation>,
    pub(crate) claim: Signed<CollectorServiceClaim>,
    pub(crate) owner: OwnerPin,
    pub(crate) actor_revision: u64,
    pub(crate) revocation_floor: u64,
}
/// Read-only original signed-journal data; cannot be passed to material-use or
/// execution APIs in place of the non-Serde, privately minted actual Permit.
pub(crate) struct ActiveCollectorServiceObservation {
    pub(crate) stored:CollectorServiceStoredObservation,
    pub(crate) original_start_nonce:Nonce,
}
impl<S: CompareAndSwap> Actor<S> {
    pub(crate) fn observe_active_collector_service(&mut self,service:ResourceId,hash:Hash,now:Now,
        clock:&mut impl FnMut()->Result<CollectorServiceClockObservation>)->Result<ActiveCollectorServiceObservation>{
        let observed=clock()?;let state=self.read(Now(observed.wall_time))?;
        self.observe_active_collector_service_from_read(service,hash,now,state,observed)
    }
    pub(crate) fn observe_active_collector_service_waiting(
        &mut self, service:ResourceId, hash:Hash, now:Now, end:std::time::Instant,
        clock:&mut impl FnMut()->Result<CollectorServiceClockObservation>,
    )->Result<ActiveCollectorServiceObservation>{
        let (state,observed)=self.read_collector_service_waiting(now,end,clock)?;
        self.observe_active_collector_service_from_read(service,hash,now,state,observed)
    }
    fn observe_active_collector_service_from_read(
        &mut self, service:ResourceId, hash:Hash, now:Now, state:State,
        observed:CollectorServiceClockObservation,
    )->Result<ActiveCollectorServiceObservation>{
        if observed.wall_time<now.0||observed.wall_time<self.last_observed_time{return Err(Error::InvalidTime);}
        let record=state.collector_services.get(&service).ok_or(Error::NotFound)?;
        current(&state,record,Now(observed.wall_time),&observed)?;
        if record.hash!=hash{return Err(Error::WrongClaim);}
        let start=record.active.ok_or(Error::WrongState)?;
        self.last_observed_time=observed.wall_time;
        Ok(ActiveCollectorServiceObservation{original_start_nonce:start,
            stored:CollectorServiceStoredObservation{policy:record.policy.clone(),claim:record.claim.clone(),
                owner:state.pin.clone(),actor_revision:state.revision,revocation_floor:record.revocation_floor}})
    }
    /// Read-only signed-history observation. Ended/boot-changed records can be
    /// inspected or targeted for revocation, but this never creates a Permit.
    pub(crate) fn observe_stored_collector_service(
        &mut self,
        service_id: ResourceId,
        expected: Hash,
        now: Now,
    ) -> Result<CollectorServiceStoredObservation> {
        let state = self.read(now)?;
        let record = state
            .collector_services
            .get(&service_id)
            .ok_or(Error::NotFound)?;
        let p = &record.policy.body;
        record.policy.verify(state.pin.key)?;
        if record.hash != expected
            || p.header.context != state.pin.context
            || p.header.owner_epoch != state.pin.epoch
            || p.header.checkpoint > state.pin.checkpoint
        {
            return Err(Error::WrongContext);
        }
        let cert = state.certificate(p.agent_id).ok_or(Error::NotFound)?;
        if cert.agent_key != p.recipient
            || cert.key_epoch != p.key_epoch
            || cert.digest()? != p.certificate_hash
            || cert.scopes.0.as_slice() != [Scope::MetricsCollect]
            || cert.capabilities.scopes != cert.scopes
        {
            return Err(Error::WrongClaim);
        }
        record.claim.verify(cert.agent_key)?;
        Ok(CollectorServiceStoredObservation {
            policy: record.policy.clone(),
            claim: record.claim.clone(),
            owner: state.pin.clone(),
            actor_revision: state.revision,
            revocation_floor: record.revocation_floor,
        })
    }
    pub(crate) fn next_collector_service(
        &mut self,
        agent: AgentId,
        manifest: Hash,
        now: Now,
        clock: &mut impl FnMut() -> Result<CollectorServiceClockObservation>,
    ) -> Result<Option<CollectorServiceStoredObservation>> {
        let observed = clock()?;
        let state = self.read(Now(observed.wall_time))?;
        self.next_collector_service_from_read(agent, manifest, now, state, observed)
    }
    pub(crate) fn next_collector_service_waiting(
        &mut self, agent: AgentId, manifest: Hash, now: Now,
        end: std::time::Instant,
        clock: &mut impl FnMut() -> Result<CollectorServiceClockObservation>,
    ) -> Result<Option<CollectorServiceStoredObservation>> {
        let (state, observed) = self.read_collector_service_waiting(now, end, clock)?;
        self.next_collector_service_from_read(agent, manifest, now, state, observed)
    }
    fn next_collector_service_from_read(
        &mut self, agent: AgentId, manifest: Hash, now: Now, state: State,
        observed: CollectorServiceClockObservation,
    ) -> Result<Option<CollectorServiceStoredObservation>> {
        if observed.wall_time < now.0 || observed.wall_time < self.last_observed_time {
            return Err(Error::InvalidTime);
        }
        let mut found = None;
        for record in state
            .collector_services
            .values()
            .filter(|r| r.policy.body.agent_id == agent)
        {
            // A persisted unknown original slot is never assumed ended merely
            // because this daemon is new or a private socket is absent.
            if record.active.is_some() {
                return Err(Error::UncertainCommit);
            }
            if record.revoked || observed.wall_time >= record.policy.body.expires_at {
                continue;
            }
            current(&state, record, Now(observed.wall_time), &observed)?;
            if record.policy.body.manifest_sha256 != manifest || found.is_some() {
                return Err(Error::WrongClaim);
            }
            found = Some(CollectorServiceStoredObservation {
                policy: record.policy.clone(),
                claim: record.claim.clone(),
                owner: state.pin.clone(),
                actor_revision: state.revision,
                revocation_floor: record.revocation_floor,
            });
        }
        self.last_observed_time = observed.wall_time;
        Ok(found)
    }
    pub(crate) fn observe_current_collector_service(
        &mut self,
        service_id: ResourceId,
        expected: Hash,
        now: Now,
        clock: &mut impl FnMut() -> Result<CollectorServiceClockObservation>,
    ) -> Result<CollectorServiceStoredObservation> {
        let state = self.read(now)?;
        let observed = clock()?;
        if observed.wall_time < now.0 || observed.wall_time < self.last_observed_time {
            return Err(Error::InvalidTime);
        }
        self.last_observed_time = observed.wall_time;
        let record = state
            .collector_services
            .get(&service_id)
            .ok_or(Error::NotFound)?;
        current(&state, record, Now(observed.wall_time), &observed)?;
        if record.hash != expected {
            return Err(Error::WrongClaim);
        }
        Ok(CollectorServiceStoredObservation {
            policy: record.policy.clone(),
            claim: record.claim.clone(),
            owner: state.pin.clone(),
            actor_revision: state.revision,
            revocation_floor: record.revocation_floor,
        })
    }
    pub(crate) fn observe_revoked_collector_service(
        &mut self,
        service_id: ResourceId,
        expected: Hash,
        now: Now,
    ) -> Result<CollectorServiceStoredObservation> {
        let state = self.read(now)?;
        let record = state
            .collector_services
            .get(&service_id)
            .ok_or(Error::NotFound)?;
        if record.hash != expected
            || !record.revoked
            || record.revocation_floor <= record.policy.body.revocation_epoch
        {
            return Err(Error::WrongState);
        }
        // No policy-live predicate: current-owner revocation metadata remains
        // observable after the service ended, and cannot dispatch anything.
        record.policy.verify(state.pin.key)?;
        if record.policy.body.header.context != state.pin.context
            || record.policy.body.header.owner_epoch != state.pin.epoch
        {
            return Err(Error::WrongContext);
        }
        Ok(CollectorServiceStoredObservation {
            policy: record.policy.clone(),
            claim: record.claim.clone(),
            owner: state.pin.clone(),
            actor_revision: state.revision,
            revocation_floor: record.revocation_floor,
        })
    }
}
