//! Specific finite service policy, stored ONLY in the existing signed journal.
//! Not a Lease renewal, caller-deserialized Permit, or an independent registry.
use super::*;
#[path = "collector_service_clock.rs"]
mod clock;
pub use clock::CollectorServiceClockObservation;
#[path = "collector_service_access.rs"]
mod access;
pub use access::CollectorServiceAccess;
#[path = "collector_service_observe.rs"]
mod observe;
pub(crate) use observe::{ActiveCollectorServiceObservation, CollectorServiceStoredObservation};

pub(super) const MAX_COLLECTOR_SERVICES: usize = MAX_AGENTS;

/// Reconstructed solely by verified original admission/transition replay.
#[derive(Clone)]
pub(super) struct OriginalCollectorService {
    policy: Signed<CollectorServiceDelegation>,
    claim: Signed<CollectorServiceClaim>,
    hash: Hash,
    clock: CollectorServiceClockStamp,
    starts: u32,
    active: Option<Nonce>,
    revocation_floor: u64,
    revoked: bool,
    lifetime: Option<lifetime::LifetimeAdmission>,
    credential_purpose: Option<CollectorCredentialPurpose>,
}

/// An original run capability cannot be serialized, cloned, or supplied by IPC.
#[derive(Debug)]
pub struct CollectorServicePermit {
    service_id: ResourceId,
    delegation_hash: Hash,
    start: Nonce,
    start_count: u32,
}
impl CollectorServicePermit {
    pub(crate) fn start_id(&self) -> Nonce {
        self.start
    }
    pub fn service_id(&self) -> ResourceId {
        self.service_id
    }
    pub fn delegation_hash(&self) -> Hash {
        self.delegation_hash
    }
}

/// Constructed only by the real installed supervisor after positive original
/// unit/cgroup/process-end observations. Timeout/EOF does not construct this.
/// Private fields deliberately prohibit a caller JSON "joined":true witness.
pub(super) struct OriginalServiceEnd {
    service_id: ResourceId,
    delegation_hash: Hash,
    start: Nonce,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CollectorServiceEndReason {
    JoinedOriginalUnit,
    NoChildCreated,
}

fn current(
    state: &State,
    record: &OriginalCollectorService,
    now: Now,
    observed: &CollectorServiceClockObservation,
) -> Result<()> {
    observed.check(&record.clock)?;
    let p = &record.policy.body;
    current_header(state, &p.header, now)?;
    record.policy.verify(state.pin.key)?;
    live(p.issued_at, p.expires_at, now)?;
    if record.revoked || record.revocation_floor != p.revocation_epoch {
        return Err(Error::Revoked);
    }
    let agent = state.agents.get(&p.agent_id).ok_or(Error::NotFound)?;
    if agent.revoked {
        return Err(Error::Revoked);
    }
    let cert = &agent.certificate;
    live(cert.not_before, cert.expires_at, now)?;
    if cert.scopes.0.as_slice() != [Scope::MetricsCollect]
        || cert.capabilities.scopes != cert.scopes
    {
        return Err(Error::CapabilityDenied);
    }
    if cert.key_epoch != p.key_epoch
        || cert.agent_key != p.recipient
        || cert.digest()? != p.certificate_hash
    {
        return Err(Error::WrongEpoch);
    }
    if p.expires_at > cert.expires_at || p.expires_at > state.pin.expires_at {
        return Err(Error::Expired);
    }
    p.validate()
}

impl<S: CompareAndSwap> Actor<S> {
    #[cfg(target_os = "linux")]
    pub(crate) fn finish_unlaunched_collector_service(
        &mut self,
        permit: &CollectorServicePermit,
        ended: crate::enrollment_control::metrics::service_supervisor::UnlaunchedCollectorService<
            '_,
        >,
        now: Now,
    ) -> Result<()> {
        if !ended.matches_original(permit) {
            return Err(Error::WrongClaim);
        }
        self.replay_collector_service_end(
            permit.service_id,
            permit.delegation_hash,
            permit.start,
            CollectorServiceEndReason::NoChildCreated,
            now,
        )
    }
    #[cfg(target_os = "linux")]
    pub(crate) fn finish_joined_collector_service(
        &mut self,
        permit: &CollectorServicePermit,
        ended: crate::enrollment_control::metrics::service_supervisor::JoinedCollectorService<'_>,
        now: Now,
    ) -> Result<()> {
        if !ended.matches_original(permit) {
            return Err(Error::WrongClaim);
        }
        self.finish_collector_service(
            OriginalServiceEnd {
                service_id: permit.service_id,
                delegation_hash: permit.delegation_hash,
                start: permit.start,
            },
            now,
        )
    }
    /// The authenticated Node catalog must additionally match the FULL policy
    /// digest/profile before any actual collector resource is touched.
    pub fn admit_collector_service(
        &mut self,
        policy: Signed<CollectorServiceDelegation>,
        proof: Signed<CollectorServiceClaim>,
        now: Now,
        clock: &mut impl FnMut() -> Result<CollectorServiceClockObservation>,
    ) -> Result<()> {
        let mut state = self.read(now)?;
        let observed = clock()?;
        if observed.wall_time < now.0 || observed.wall_time < self.last_observed_time {
            return Err(Error::InvalidTime);
        }
        self.last_observed_time = observed.wall_time;
        let now = Now(observed.wall_time);
        current_header(&state, &policy.body.header, now)?;
        policy.verify(state.pin.key)?;
        recent(policy.body.issued_at, now)?;
        if state
            .collector_services
            .contains_key(&policy.body.service_id)
        {
            return Err(Error::Replay);
        }
        if state.collector_services.len() >= MAX_COLLECTOR_SERVICES {
            return Err(Error::Quota);
        }
        let record = OriginalCollectorService {
            hash: policy.body.digest()?,
            clock: proof.body.clock.clone(),
            starts: 0,
            active: None,
            revocation_floor: policy.body.revocation_epoch,
            revoked: false,
            lifetime: None,
            credential_purpose: None,
            policy: policy.clone(),
            claim: proof.clone(),
        };
        current(&state, &record, now, &observed)?;
        if state.collector_services.values().any(|old| {
            old.policy.body.agent_id == policy.body.agent_id
                && (old.active.is_some() || (!old.revoked && old.policy.body.expires_at > now.0))
        }) {
            return Err(Error::Quota);
        }
        let p = &proof.body;
        if p.header != policy.body.header
            || p.agent_id != policy.body.agent_id
            || p.key_epoch != policy.body.key_epoch
            || p.delegation_hash != record.hash
            || p.issued_at < policy.body.issued_at
        {
            return Err(Error::WrongClaim);
        }
        recent(p.issued_at, now)?;
        let remaining = policy
            .body
            .expires_at
            .seconds()
            .checked_sub(p.issued_at.seconds())
            .and_then(|n| n.checked_mul(1_000_000_000))
            .ok_or(Error::Expired)?;
        if p.clock.deadline_boottime_ns - p.clock.created_boottime_ns > remaining {
            return Err(Error::Expired);
        }
        proof.verify(policy.body.recipient)?;
        nonce(&mut state, p.nonce)?;
        let original_end = record.clock.clone();
        let expiry = policy.body.expires_at;
        state
            .collector_services
            .insert(policy.body.service_id, record);
        // CAS failure, pending anchor, quota or lost ACK supplies no Permit.
        self.commit(
            state,
            now,
            Transition::CollectorServiceAdmit { policy, proof },
        )?;
        let after = clock()?;
        after.check(&original_end)?;
        if after.wall_time < now.0 || after.wall_time >= expiry {
            return Err(Error::Expired);
        }
        self.last_observed_time = after.wall_time;
        Ok(())
    }

    /// Each actual start (including restart) commits BEFORE touching the unit.
    /// Original absolute expiry and consumed-start counter survive process loss.
    pub(crate) fn reserve_collector_service_start(
        &mut self,
        service_id: ResourceId,
        start: Nonce,
        now: Now,
        clock: &mut impl FnMut() -> Result<CollectorServiceClockObservation>,
    ) -> Result<CollectorServicePermit> {
        let mut state = self.read(now)?;
        let observed = clock()?;
        if observed.wall_time < now.0 || observed.wall_time < self.last_observed_time {
            return Err(Error::InvalidTime);
        }
        self.last_observed_time = observed.wall_time;
        let now = Now(observed.wall_time);
        let record = state
            .collector_services
            .get(&service_id)
            .ok_or(Error::NotFound)?;
        current(&state, record, now, &observed)?;
        if record.active.is_some() {
            return Err(Error::WrongState);
        }
        if record.starts >= record.policy.body.quota.max_starts {
            return Err(Error::Quota);
        }
        if state.collector_services.values().any(|other| {
            other.policy.body.agent_id == record.policy.body.agent_id && other.active.is_some()
        }) {
            return Err(Error::Quota);
        }
        let prior_starts = record.starts;
        let hash = record.hash;
        let original_end = record.clock.clone();
        let expiry = record.policy.body.expires_at;
        nonce(&mut state, start)?;
        let record = state
            .collector_services
            .get_mut(&service_id)
            .ok_or(Error::NotFound)?;
        record.starts = prior_starts.checked_add(1).ok_or(Error::Quota)?;
        record.active = Some(start);
        record.credential_purpose = record.lifetime.as_ref().map(|_| CollectorCredentialPurpose::ServiceLifetime);
        let permit = CollectorServicePermit {
            service_id,
            delegation_hash: hash,
            start,
            start_count: record.starts,
        };
        self.commit(
            state,
            now,
            Transition::CollectorServiceStart {
                service_id,
                delegation_hash: hash,
                prior_starts,
                start,
                observed_boot: observed.boot_id,
                observed_boottime_ns: observed.boottime_ns,
            },
        )?;
        let after = clock()?;
        after.check(&original_end)?;
        if after.wall_time < now.0 || after.wall_time >= expiry {
            return Err(Error::Expired);
        }
        self.last_observed_time = after.wall_time;
        Ok(permit)
    }

    pub fn check_collector_service(
        &mut self, permit: &CollectorServicePermit, now: Now,
        clock: &mut impl FnMut() -> Result<CollectorServiceClockObservation>,
    ) -> Result<CollectorServiceDelegation> {
        let state = self.read(now)?;
        let observed = clock()?;
        self.check_collector_service_loaded(permit, now, state, observed)
    }
    pub(crate) fn check_collector_service_waiting(
        &mut self, permit: &CollectorServicePermit, now: Now, end: std::time::Instant,
        clock: &mut impl FnMut() -> Result<CollectorServiceClockObservation>,
    ) -> Result<CollectorServiceDelegation> {
        let (state, observed) = self.read_collector_service_waiting(now, end, clock)?;
        self.check_collector_service_loaded(permit, now, state, observed)
    }
    fn check_collector_service_loaded(
        &mut self, permit: &CollectorServicePermit, now: Now, state: State,
        observed: CollectorServiceClockObservation,
    ) -> Result<CollectorServiceDelegation> {
        if observed.wall_time < now.0 || observed.wall_time < self.last_observed_time {
            return Err(Error::InvalidTime);
        }
        self.last_observed_time = observed.wall_time;
        let now = Now(observed.wall_time);
        let record = state
            .collector_services
            .get(&permit.service_id)
            .ok_or(Error::NotFound)?;
        current(&state, record, now, &observed)?;
        if record.hash != permit.delegation_hash
            || record.active != Some(permit.start)
            || record.starts != permit.start_count
        {
            return Err(Error::WrongState);
        }
        Ok(record.policy.body.clone())
    }
    pub(crate) fn snapshot_collector_service_waiting(
        &mut self, now: Now, end: std::time::Instant,
        clock: &mut impl FnMut() -> Result<CollectorServiceClockObservation>,
    ) -> Result<State> {
        self.read_collector_service_waiting(now, end, clock).map(|(state, _)| state)
    }

    pub fn revoke_collector_service(
        &mut self,
        signed: Signed<CollectorServiceRevocation>,
        now: Now,
    ) -> Result<()> {
        let mut state = self.read(now)?;
        current_header(&state, &signed.body.header, now)?;
        signed.verify(state.pin.key)?;
        recent(signed.body.issued_at, now)?;
        let revoke = &signed.body;
        let record = state
            .collector_services
            .get_mut(&revoke.service_id)
            .ok_or(Error::NotFound)?;
        if record.hash != revoke.delegation_hash
            || record.policy.body.policy_revision != revoke.policy_revision
        {
            return Err(Error::WrongClaim);
        }
        if revoke.revocation_epoch <= record.revocation_floor {
            return Err(Error::Replay);
        }
        record.revocation_floor = revoke.revocation_epoch;
        record.revoked = true;
        // The original active slot stays occupied until ACTUAL owned teardown.
        self.commit(state, now, Transition::CollectorServiceRevoke(signed))
    }

    pub(super) fn finish_collector_service(
        &mut self,
        ended: OriginalServiceEnd,
        now: Now,
    ) -> Result<()> {
        self.replay_collector_service_end(
            ended.service_id,
            ended.delegation_hash,
            ended.start,
            CollectorServiceEndReason::JoinedOriginalUnit,
            now,
        )
    }
    pub(crate) fn replay_collector_service_end(
        &mut self,
        service_id: ResourceId,
        delegation_hash: Hash,
        start: Nonce,
        reason: CollectorServiceEndReason,
        now: Now,
    ) -> Result<()> {
        let mut state = self.read(now)?;
        let record = state
            .collector_services
            .get_mut(&service_id)
            .ok_or(Error::NotFound)?;
        if record.hash != delegation_hash || record.active != Some(start) {
            return Err(Error::WrongState);
        }
        record.active = None; // Narrows execution only; never resets starts/expiry/revocation.
        self.commit(
            state,
            now,
            Transition::CollectorServiceEnd {
                service_id,
                delegation_hash,
                start,
                reason,
            },
        )
    }
    pub(crate) fn replay_collector_service_start(
        &mut self,
        service_id: ResourceId,
        delegation_hash: Hash,
        prior_starts: u32,
        start: Nonce,
        observed_boot: ResourceId,
        observed_boottime_ns: u64,
        now: Now,
    ) -> Result<()> {
        let record = self
            .read(now)?
            .collector_services
            .get(&service_id)
            .cloned()
            .ok_or(Error::NotFound)?;
        if record.hash != delegation_hash || record.starts != prior_starts {
            return Err(Error::WrongState);
        }
        self.reserve_collector_service_start(service_id, start, now, &mut || {
            CollectorServiceClockObservation::journal(observed_boot, observed_boottime_ns, now.0)
        })
        .map(|_| ())
    }
    pub(crate) fn replay_collector_service_admit(
        &mut self,
        policy: Signed<CollectorServiceDelegation>,
        proof: Signed<CollectorServiceClaim>,
        now: Now,
    ) -> Result<()> {
        let boot = proof.body.clock.boot_id;
        let at = proof.body.clock.created_boottime_ns;
        self.admit_collector_service(policy, proof, now, &mut || {
            CollectorServiceClockObservation::journal(boot, at, now.0)
        })
    }
}

#[cfg(test)]
#[path = "collector_service_tests.rs"]
mod tests;

#[path="collector_service_lifetime.rs"]
mod lifetime;
pub(crate) use lifetime::{CollectorCredentialPurpose, StoredCollectorServiceCredentialSet, LifetimeReadback};

/// A read-only provider boundary, not an operation retry. Outer guards and
/// clock failures are never retried; only the supplied load's pending or snapshot-conflict error is.
pub(crate) fn wait_collector_service_read<T>(
    end: std::time::Instant, lower: Now,
    clock: &mut impl FnMut() -> Result<CollectorServiceClockObservation>,
    mut load: impl FnMut() -> Result<T>,
) -> Result<(T, CollectorServiceClockObservation)> {
    lower.0.valid()?;
    let mut previous = lower.0;
    let mut previous_boot = None;
    let mut previous_boottime = 0;
    loop {
        let before = clock()?;
        before.wall_time.valid()?;
        if before.wall_time < previous || previous_boot.as_ref().is_some_and(|boot| *boot != before.boot_id)
            || before.boottime_ns < previous_boottime { return Err(Error::InvalidTime); }
        previous = before.wall_time;
        previous_boot = Some(before.boot_id);
        previous_boottime = before.boottime_ns;
        match load() {
            Ok(value) => {
                let after = clock()?;
                after.wall_time.valid()?;
                if after.wall_time < previous || after.boot_id != before.boot_id
                    || after.boottime_ns < before.boottime_ns {
                    return Err(Error::InvalidTime);
                }
                return Ok((value, after));
            }
            Err(Error::UncertainCommit | Error::Conflict) if std::time::Instant::now() < end => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(error) => return Err(error),
        }
    }
}
