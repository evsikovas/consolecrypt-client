//! Narrow material-use borrow of the SAME live service Permit. No JSON grant.
use super::*;

pub struct CollectorServiceAccess<'a, S: CompareAndSwap> {
    actor: &'a mut Actor<S>,
    permit: &'a CollectorServicePermit,
    policy: CollectorServiceDelegation,
    owner:OwnerPin,
    original_clock: CollectorServiceClockStamp,
    expires_at: Timestamp,
    deadline_boottime_ns: u64,
    read_wait_end: Option<std::time::Instant>,
}
impl<S: CompareAndSwap> CollectorServiceAccess<'_, S> {
    pub fn service_id(&self) -> ResourceId {
        self.permit.service_id
    }
    pub fn delegation_hash(&self) -> Hash {
        self.permit.delegation_hash
    }
    pub fn matches_original_permit(&self, permit: &CollectorServicePermit) -> Result<()> {
        if !std::ptr::eq(self.permit, permit) {
            return Err(Error::WrongClaim);
        }
        Ok(())
    }
    pub fn policy(&self) -> &CollectorServiceDelegation {
        &self.policy
    }
    pub(crate) fn owner_pin(&self)->&OwnerPin {&self.owner}
    pub fn expires_at(&self) -> Timestamp {
        self.expires_at
    }
    pub fn deadline_boottime_ns(&self) -> u64 {
        self.deadline_boottime_ns
    }

    /// Caller has separately verified its signed credential-scope capsule.
    /// No new nowWall-derived window: subtract ONLY from ORIGINAL BOOTTIME end.
    /// Repeated narrowing can never increase either previously held end.
    pub fn narrow_expiry(&mut self, expires: Timestamp) -> Result<()> {
        if expires > self.expires_at {
            return Err(Error::Expired);
        }
        let delta = self
            .policy
            .expires_at
            .seconds()
            .checked_sub(expires.seconds())
            .and_then(|n| n.checked_mul(1_000_000_000))
            .ok_or(Error::InvalidTime)?;
        let deadline = self
            .original_clock
            .deadline_boottime_ns
            .checked_sub(delta)
            .filter(|n| *n > self.original_clock.created_boottime_ns)
            .ok_or(Error::Expired)?;
        if deadline > self.deadline_boottime_ns {
            return Err(Error::Expired);
        }
        self.expires_at = expires;
        self.deadline_boottime_ns = deadline;
        Ok(())
    }
    pub fn recheck(
        &mut self, now: Now,
        clock: &mut impl FnMut() -> Result<CollectorServiceClockObservation>,
    ) -> Result<Now> {
        self.recheck_read_phase(now, self.read_wait_end, clock)
    }
    pub(crate) fn recheck_waiting(
        &mut self, now: Now, end: std::time::Instant,
        clock: &mut impl FnMut() -> Result<CollectorServiceClockObservation>,
    ) -> Result<Now> {
        self.recheck_read_phase(now, Some(end), clock)
    }
    fn recheck_read_phase(
        &mut self, now: Now, read_end: Option<std::time::Instant>,
        clock: &mut impl FnMut() -> Result<CollectorServiceClockObservation>,
    ) -> Result<Now> {
        // `now` is the original caller's lower bound, never a frozen clock
        // used for a later Actor::read. Sample BEFORE read/highwater checks.
        let before = clock()?;
        before.check(&self.original_clock)?;
        if before.wall_time < now.0
            || before.wall_time < self.actor.last_observed_time
            || before.wall_time >= self.expires_at
            || before.boottime_ns >= self.deadline_boottime_ns
        {
            return Err(Error::Expired);
        }
        match read_end {
            Some(end) => { self.actor.check_collector_service_waiting(self.permit, Now(before.wall_time), end, clock)?; }
            None => { self.actor.check_collector_service(self.permit, Now(before.wall_time), clock)?; }
        }
        let observed = clock()?;
        observed.check(&self.original_clock)?;
        if observed.wall_time < now.0
            || observed.wall_time < self.actor.last_observed_time
            || observed.wall_time >= self.expires_at
            || observed.boottime_ns >= self.deadline_boottime_ns
        {
            return Err(Error::Expired);
        }
        self.actor.last_observed_time = observed.wall_time;
        Ok(Now(observed.wall_time))
    }
}
impl<S: CompareAndSwap> Actor<S> {
    pub fn borrow_collector_service<'a>(
        &'a mut self, permit: &'a CollectorServicePermit, now: Now,
        clock: &mut impl FnMut() -> Result<CollectorServiceClockObservation>,
    ) -> Result<CollectorServiceAccess<'a, S>> {
        let observed = clock()?;
        if observed.wall_time < now.0 || observed.wall_time < self.last_observed_time {
            return Err(Error::InvalidTime);
        }
        let state = self.read(Now(observed.wall_time))?;
        self.borrow_collector_service_loaded(permit, state, observed, None)
    }
    pub(crate) fn borrow_collector_service_waiting<'a>(
        &'a mut self, permit: &'a CollectorServicePermit, now: Now, end: std::time::Instant,
        clock: &mut impl FnMut() -> Result<CollectorServiceClockObservation>,
    ) -> Result<CollectorServiceAccess<'a, S>> {
        let (state, observed) = self.read_collector_service_waiting(now, end, clock)?;
        self.borrow_collector_service_loaded(permit, state, observed, Some(end))
    }
    fn borrow_collector_service_loaded<'a>(
        &'a mut self, permit: &'a CollectorServicePermit, state: State,
        observed: CollectorServiceClockObservation, read_wait_end: Option<std::time::Instant>,
    ) -> Result<CollectorServiceAccess<'a, S>> {
        self.last_observed_time = observed.wall_time;
        let record = state
            .collector_services
            .get(&permit.service_id)
            .ok_or(Error::NotFound)?;
        current(&state, record, Now(observed.wall_time), &observed)?;
        if record.hash != permit.delegation_hash
            || record.active != Some(permit.start)
            || record.starts != permit.start_count
        {
            return Err(Error::WrongState);
        }
        Ok(CollectorServiceAccess {
            actor: self,
            read_wait_end,
            permit,
            policy: record.policy.body.clone(),
            owner:state.pin.clone(),
            original_clock: record.clock.clone(),
            expires_at: record.policy.body.expires_at,
            deadline_boottime_ns: record.clock.deadline_boottime_ns,
        })
    }
}

include!("collector_service_lifetime_access.rs");
