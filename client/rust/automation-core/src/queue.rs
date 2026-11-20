//! Process-local leases complement durable AppCore admissions. Revoking a
//! credential channel never proves the target command has finished.
use crate::{AutomationError, Result};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Waiting,
    Active,
    Unknown,
}
#[derive(Debug)]
struct Reservation {
    order: u64,
    runner: Uuid,
    hosts: BTreeSet<Uuid>,
    phase: Phase,
}
#[derive(Debug, Default)]
struct State {
    next: u64,
    reservations: BTreeMap<Uuid, Reservation>,
}
#[derive(Debug, Default)]
pub struct JobCoordinator {
    state: Mutex<State>,
    changed: Notify,
}

fn overlaps(a: &Reservation, b: &Reservation) -> bool {
    a.runner == b.runner || !a.hosts.is_disjoint(&b.hosts)
}

impl JobCoordinator {
    pub fn enqueue(
        self: &Arc<Self>,
        job: Uuid,
        runner: Uuid,
        hosts: impl IntoIterator<Item = Uuid>,
    ) -> Result<JobTicket> {
        let hosts: BTreeSet<_> = hosts.into_iter().collect();
        if hosts.is_empty() || hosts.len() > 10000 {
            return Err(AutomationError::Invalid("invalid queue targets"));
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.reservations.len() >= 4096 || state.reservations.contains_key(&job) {
            return Err(AutomationError::QueueFull);
        }
        state.next = state
            .next
            .checked_add(1)
            .ok_or(AutomationError::QueueFull)?;
        let order = state.next;
        state.reservations.insert(
            job,
            Reservation {
                order,
                runner,
                hosts,
                phase: Phase::Waiting,
            },
        );
        Ok(JobTicket {
            owner: self.clone(),
            job,
            finished: false,
        })
    }
    /// Only records persisted before a crash are restored; never dispatch them.
    pub fn restore_unknown(
        &self,
        job: Uuid,
        runner: Uuid,
        hosts: impl IntoIterator<Item = Uuid>,
    ) -> Result<()> {
        let hosts: BTreeSet<_> = hosts.into_iter().collect();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if hosts.is_empty()
            || state.reservations.len() >= 4096
            || state.reservations.contains_key(&job)
        {
            return Err(AutomationError::Invalid(
                "invalid recovered queue reservation",
            ));
        }
        state.reservations.insert(
            job,
            Reservation {
                order: 0,
                runner,
                hosts,
                phase: Phase::Unknown,
            },
        );
        Ok(())
    }
    pub fn holds_runner(&self, runner: Uuid) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .reservations
            .values()
            .any(|entry| entry.runner == runner)
    }
    pub fn unknown_jobs(&self) -> Vec<Uuid> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .reservations
            .iter()
            .filter(|(_, v)| v.phase == Phase::Unknown)
            .map(|(id, _)| *id)
            .collect()
    }
    /// Caller must first persist verified terminal evidence or an explicit
    /// user acknowledgement of possible partial changes. Never an automatic TTL.
    pub fn resolve_unknown(&self, job: Uuid) -> Result<()> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !state
            .reservations
            .get(&job)
            .is_some_and(|entry| entry.phase == Phase::Unknown)
        {
            return Err(AutomationError::Invalid(
                "job is not awaiting reconciliation",
            ));
        }
        state.reservations.remove(&job);
        drop(state);
        self.changed.notify_waiters();
        Ok(())
    }
}

#[derive(Debug)]
pub struct JobTicket {
    owner: Arc<JobCoordinator>,
    job: Uuid,
    finished: bool,
}
impl JobTicket {
    pub async fn wait(&self, cancel: &CancellationToken) -> Result<()> {
        loop {
            let changed = self.owner.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if cancel.is_cancelled() {
                return Err(AutomationError::Closed);
            }
            {
                let mut state = self.owner.state.lock().unwrap_or_else(|e| e.into_inner());
                let own = state
                    .reservations
                    .get(&self.job)
                    .ok_or(AutomationError::Closed)?;
                if own.phase != Phase::Waiting {
                    return Err(AutomationError::Invalid("job already admitted"));
                }
                let blocked = state.reservations.iter().any(|(id, other)| {
                    *id != self.job
                        && overlaps(own, other)
                        && (other.phase != Phase::Waiting || other.order < own.order)
                });
                if !blocked {
                    state.reservations.get_mut(&self.job).unwrap().phase = Phase::Active;
                    return Ok(());
                }
            }
            tokio::select! { biased; _=cancel.cancelled()=>return Err(AutomationError::Closed), _=&mut changed=>{} }
        }
    }
    /// Use only before dispatch or after a known terminal outcome was persisted.
    pub fn finish(mut self) {
        self.owner
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .reservations
            .remove(&self.job);
        self.finished = true;
        self.owner.changed.notify_waiters();
    }
}
impl Drop for JobTicket {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let mut state = self.owner.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = state.reservations.get_mut(&self.job) {
            if entry.phase == Phase::Waiting {
                state.reservations.remove(&self.job);
            } else {
                entry.phase = Phase::Unknown;
            }
        }
        drop(state);
        self.owner.changed.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[tokio::test]
    async fn same_host_is_serialized_across_runners_but_independent_jobs_can_run() {
        let queue = Arc::new(JobCoordinator::default());
        let host = Uuid::new_v4();
        let runner = Uuid::new_v4();
        let a = queue.enqueue(Uuid::new_v4(), runner, [host]).unwrap();
        a.wait(&CancellationToken::new()).await.unwrap();
        let b = queue
            .enqueue(Uuid::new_v4(), Uuid::new_v4(), [host])
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(20), b.wait(&CancellationToken::new()))
                .await
                .is_err()
        );
        let c = queue
            .enqueue(Uuid::new_v4(), Uuid::new_v4(), [Uuid::new_v4()])
            .unwrap();
        c.wait(&CancellationToken::new()).await.unwrap();
        c.finish();
        a.finish();
        b.wait(&CancellationToken::new()).await.unwrap();
        b.finish();
    }
    #[tokio::test]
    async fn abandoned_active_job_blocks_until_explicit_reconciliation_without_expiry() {
        let queue = Arc::new(JobCoordinator::default());
        let job = Uuid::new_v4();
        let host = Uuid::new_v4();
        let a = queue.enqueue(job, Uuid::new_v4(), [host]).unwrap();
        a.wait(&CancellationToken::new()).await.unwrap();
        drop(a);
        assert_eq!(queue.unknown_jobs(), vec![job]);
        let b = queue
            .enqueue(Uuid::new_v4(), Uuid::new_v4(), [host])
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(20), b.wait(&CancellationToken::new()))
                .await
                .is_err()
        );
        queue.resolve_unknown(job).unwrap();
        b.wait(&CancellationToken::new()).await.unwrap();
        b.finish();
    }
    #[tokio::test]
    async fn cancelling_waiter_does_not_release_another_jobs_target() {
        let queue = Arc::new(JobCoordinator::default());
        let host = Uuid::new_v4();
        let runner = Uuid::new_v4();
        let a = queue.enqueue(Uuid::new_v4(), runner, [host]).unwrap();
        a.wait(&CancellationToken::new()).await.unwrap();
        let b = queue.enqueue(Uuid::new_v4(), runner, [host]).unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(b.wait(&cancel).await.is_err());
        drop(b);
        assert!(queue.holds_runner(runner));
        a.finish();
        assert!(!queue.holds_runner(runner));
    }
    #[tokio::test]
    async fn recovered_unknown_is_never_automatically_dispatched() {
        let queue = Arc::new(JobCoordinator::default());
        let host = Uuid::new_v4();
        let id = Uuid::new_v4();
        let runner = Uuid::new_v4();
        queue.restore_unknown(id, runner, [host]).unwrap();
        assert!(queue.enqueue(id, runner, [host]).is_err());
        let other = queue
            .enqueue(Uuid::new_v4(), Uuid::new_v4(), [host])
            .unwrap();
        assert!(tokio::time::timeout(
            Duration::from_millis(20),
            other.wait(&CancellationToken::new())
        )
        .await
        .is_err());
        queue.resolve_unknown(id).unwrap();
        other.wait(&CancellationToken::new()).await.unwrap();
        other.finish();
    }
}
