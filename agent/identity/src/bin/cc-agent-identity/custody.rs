//! Private process-local custody. Only an actual admitted Permit is retained.
//! No token/Permit Debug, serialization, cloning, persistence or reconstruction.
use cc_agent_identity_policy::*;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use zeroize::{Zeroize, Zeroizing};

const CAPACITY: usize = 8;
const LIFETIME: Duration = Duration::from_secs(5);

pub(super) struct Handle([u8; 32]);
impl Handle {
    pub(super) fn parse(value: &str) -> Result<Self> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::InvalidToken);
        }
        let mut bytes = [0; 32];
        hex::decode_to_slice(value, &mut bytes).map_err(|_| Error::InvalidToken)?;
        if bytes == [0; 32] {
            return Err(Error::InvalidToken);
        }
        Ok(Self(bytes))
    }
    pub(super) fn encoded(&self) -> Zeroizing<String> {
        Zeroizing::new(hex::encode(self.0))
    }
    fn matches(&self, other: &Self) -> bool {
        use subtle::ConstantTimeEq;
        bool::from(self.0.ct_eq(&other.0))
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Clone, Copy)]
struct Sample {
    wall: Duration,
    monotonic: Duration,
}
impl Sample {
    fn now(self) -> Result<Now> {
        if self.wall > Duration::from_secs(253_402_300_799) {
            return Err(Error::InvalidTime);
        }
        Ok(Now(Timestamp::new(self.wall.as_secs())?))
    }
}
struct Clock {
    origin: Instant,
    #[cfg(test)]
    scripted: std::collections::VecDeque<Result<Sample>>,
}
impl Clock {
    fn new() -> Self {
        Self {
            origin: Instant::now(),
            #[cfg(test)]
            scripted: std::collections::VecDeque::new(),
        }
    }
    fn sample(&mut self) -> Result<Sample> {
        #[cfg(test)]
        if let Some(value) = self.scripted.pop_front() {
            return value;
        }
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::InvalidTime)?;
        let value = Sample {
            wall,
            monotonic: self.origin.elapsed(),
        };
        value.now()?;
        Ok(value)
    }
}

enum Phase {
    Begun,
    Checked,
}
struct Entry {
    handle: Handle,
    permit: Permit,
    started: Duration,
    phase: Phase,
}
pub(super) struct Begun {
    pub(super) handle: Handle,
    pub(super) agent_id: AgentId,
    pub(super) operation: Operation,
    pub(super) expires_at: Timestamp,
}

pub(super) struct Custody<S: CompareAndSwap> {
    actor: Actor<S>,
    entries: Vec<Entry>,
    clock: Clock,
    last: Option<Sample>,
    clock_failed: bool,
    #[cfg(test)]
    entropy: Option<Result<[u8; 32]>>,
}
impl<S: CompareAndSwap> Custody<S> {
    pub(super) fn new(actor: Actor<S>) -> Self {
        Self {
            actor,
            entries: Vec::new(),
            clock: Clock::new(),
            last: None,
            clock_failed: false,
            #[cfg(test)]
            entropy: None,
        }
    }
    fn sample(&mut self) -> Result<Sample> {
        if self.clock_failed {
            return Err(Error::InvalidTime);
        }
        // Only this fixed local clock path can poison the process clock.
        // Actor policy also returns InvalidTime for untrusted signed fields;
        // that per-request denial must not destroy other retained Permits.
        let observed = (|| {
            let value = self.clock.sample()?;
            value.now()?;
            if self
                .last
                .is_some_and(|last| value.wall < last.wall || value.monotonic < last.monotonic)
            {
                return Err(Error::InvalidTime);
            }
            Ok(value)
        })();
        let value = match observed {
            Ok(value) => value,
            Err(error) => {
                self.clock_failed = true;
                self.entries.clear();
                return Err(error);
            }
        };
        self.last = Some(value);
        self.entries
            .retain(|entry| Self::live(entry, value).is_ok());
        Ok(value)
    }
    pub(super) fn clock_failed(&self) -> bool {
        self.clock_failed
    }
    fn live(entry: &Entry, value: Sample) -> Result<()> {
        if value.wall >= Duration::from_secs(entry.permit.expires_at().seconds())
            || value
                .monotonic
                .checked_sub(entry.started)
                .ok_or(Error::InvalidTime)?
                >= LIFETIME
        {
            return Err(Error::Expired);
        }
        Ok(())
    }
    fn random(&mut self) -> Result<Handle> {
        let mut bytes = Zeroizing::new([0; 32]);
        #[cfg(test)]
        if let Some(value) = self.entropy.take() {
            *bytes = value?;
        } else {
            getrandom::fill(bytes.as_mut()).map_err(|_| Error::Entropy)?;
        }
        #[cfg(not(test))]
        getrandom::fill(bytes.as_mut()).map_err(|_| Error::Entropy)?;
        let handle = Handle(*bytes);
        if handle.0 == [0; 32]
            || self
                .entries
                .iter()
                .any(|entry| entry.handle.matches(&handle))
        {
            return Err(Error::Entropy);
        }
        Ok(handle)
    }
    pub(super) fn begin(
        &mut self,
        expected_agent: AgentId,
        operation: Operation,
        lease: Signed<Lease>,
        proof: Signed<AgentRequest>,
        freshness: Signed<Freshness>,
    ) -> Result<Begun> {
        if !matches!(operation, Operation::TelemetryRead { .. }) {
            return Err(Error::ScopeDenied);
        }
        if lease.body.agent_id != expected_agent
            || proof.body.agent_id != expected_agent
            || lease.body.operation != operation
        {
            return Err(Error::WrongClaim);
        }
        let before = self.sample()?;
        if self.entries.len() >= CAPACITY {
            return Err(Error::Quota);
        }
        // Reserve entropy/capacity before consume; never redraw on failure.
        let handle = self.random()?;
        let permit = self
            .actor
            .admit(lease, proof, Some(&freshness), &operation, before.now()?)?;
        let entry = Entry {
            handle,
            permit,
            started: before.monotonic,
            phase: Phase::Begun,
        };
        let check = self.sample()?;
        // A committed admission may itself use the whole custody window.
        // Do not start another store read after observing it already expired.
        Self::live(&entry, check)?;
        self.actor.check_permit(&entry.permit, check.now()?)?;
        let after = self.sample()?;
        Self::live(&entry, after)?;
        let response = Begun {
            handle: Handle(entry.handle.0),
            agent_id: entry.permit.agent_id(),
            operation: entry.permit.operation().clone(),
            expires_at: entry.permit.expires_at(),
        };
        self.entries.push(entry);
        Ok(response)
    }
    fn take(&mut self, handle: &Handle) -> Result<Entry> {
        let index = self
            .entries
            .iter()
            .position(|entry| entry.handle.matches(handle))
            .ok_or(Error::NotFound)?;
        Ok(self.entries.swap_remove(index))
    }
    fn context(entry: &Entry, agent: AgentId, operation: &Operation) -> Result<()> {
        if entry.permit.agent_id() != agent || entry.permit.operation() != operation {
            return Err(Error::WrongClaim);
        }
        Ok(())
    }
    pub(super) fn check(
        &mut self,
        handle: &Handle,
        agent: AgentId,
        operation: &Operation,
    ) -> Result<()> {
        let before = self.sample()?;
        let mut entry = self.take(handle)?;
        Self::context(&entry, agent, operation)?;
        if !matches!(entry.phase, Phase::Begun) {
            return Err(Error::WrongState);
        }
        Self::live(&entry, before)?;
        self.actor.check_permit(&entry.permit, before.now()?)?;
        let after = self.sample()?;
        Self::live(&entry, after)?;
        entry.phase = Phase::Checked;
        self.entries.push(entry);
        Ok(())
    }
    pub(super) fn finalize(
        &mut self,
        handle: &Handle,
        agent: AgentId,
        operation: &Operation,
    ) -> Result<()> {
        // Remove first: a failed/uncertain finalize can never be retried.
        let entry = self.take(handle)?;
        Self::context(&entry, agent, operation)?;
        if !matches!(entry.phase, Phase::Checked) {
            return Err(Error::WrongState);
        }
        let before = self.sample()?;
        Self::live(&entry, before)?;
        self.actor.check_permit(&entry.permit, before.now()?)?;
        let after = self.sample()?;
        Self::live(&entry, after)
    }
    pub(super) fn retire(
        &mut self,
        handle: &Handle,
        agent: AgentId,
        operation: &Operation,
    ) -> Result<()> {
        let _ = self.sample()?;
        match self.take(handle) {
            Ok(entry) => Self::context(&entry, agent, operation),
            Err(Error::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
#[path = "custody_tests.rs"]
mod tests;
