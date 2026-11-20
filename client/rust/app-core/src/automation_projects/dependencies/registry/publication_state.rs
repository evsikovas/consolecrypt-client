//! One-use custody on the EXISTING operation. No second owner map/approval.
use super::*;
use futures::future::BoxFuture;

#[derive(Default)]
pub(in crate::automation_projects::dependencies) struct JoinedCleanup {
    complete: AtomicU8,
    notify: tokio::sync::Notify,
}
impl JoinedCleanup {
    pub(super) fn finish(&self, success: bool) {
        self.complete
            .store(if success { 1 } else { 2 }, Ordering::Release);
        self.notify.notify_waiters();
    }
    pub(super) async fn wait(&self) -> bool {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let state = self.complete.load(Ordering::Acquire);
            if state != 0 {
                return state == 1;
            }
            notified.await;
        }
    }
}
pub(super) struct Publishing {
    nonce: Uuid,
    commit_pending: bool,
}
fn claim_state<T>(state: &mut State<T>, digest: &str, nonce: Uuid) -> AppResult<Arc<T>> {
    let State::Accepted(view, ready, acceptance) = state else {
        return Err(invalid("dependency_review_changed"));
    };
    if !acceptance.registry_owned || view.review_digest.as_deref() != Some(digest) {
        return Err(invalid("dependency_review_changed"));
    }
    let ready = ready.clone();
    let mut view = view.clone();
    view.phase = Phase::Publishing;
    *state = State::Publishing(
        view,
        ready.clone(),
        Publishing {
            nonce,
            commit_pending: false,
        },
    );
    Ok(ready)
}
fn mark_sql_commit<T>(
    state: &mut State<T>,
    epoch: &AtomicU64,
    nonce: Uuid,
    live: bool,
) -> AppResult<()> {
    let State::Publishing(_, _, claim) = state else {
        return Err(invalid("dependency_publication_context_changed"));
    };
    if claim.nonce != nonce || claim.commit_pending || !live {
        return Err(invalid("dependency_publication_context_changed"));
    }
    claim.commit_pending = true;
    epoch.fetch_add(1, Ordering::AcqRel);
    Ok(())
}
fn watched_retirement<T>(
    state: &mut State<T>,
    epoch: &AtomicU64,
    original: u64,
) -> Option<State<T>> {
    if epoch.load(Ordering::Acquire) != original
        || matches!(
            state,
            State::Terminal(..)
                | State::Completing(..)
                | State::Publishing(
                    _,
                    _,
                    Publishing {
                        commit_pending: true,
                        ..
                    }
                )
        )
    {
        return None;
    }
    let old = std::mem::replace(
        state,
        State::Terminal(Phase::ContextChanged, "dependency_context_changed"),
    );
    epoch.fetch_add(1, Ordering::AcqRel);
    Some(old)
}

fn abandon_state<T>(state: &mut State<T>, nonce: Uuid) -> Option<State<T>> {
    let belongs = matches!(&*state,State::Publishing(_,_,claim) if claim.nonce==nonce)
        || matches!(&*state,State::Completing(_,current) if *current==nonce);
    if !belongs {
        return None;
    }
    Some(std::mem::replace(
        state,
        State::Terminal(
            Phase::RecoveryRequired,
            "dependency_publication_recovery_required",
        ),
    ))
}
impl Operation {
    pub(in crate::automation_projects::dependencies) fn is_published_metadata(&self) -> bool {
        matches!(
            &*self.state.lock().unwrap_or_else(|e| e.into_inner()),
            State::Terminal(Phase::Published, "dependency_published")
        )
    }

    /// None means there is no OLD-row check to start. Committing is a private
    /// final SQL phase, never permission to run/fetch/perform new FS operations.
    pub(in crate::automation_projects::dependencies) fn watch_generation(&self) -> Option<u64> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(
            &*state,
            State::Terminal(..)
                | State::Completing(..)
                | State::Publishing(
                    _,
                    _,
                    Publishing {
                        commit_pending: true,
                        ..
                    }
                )
        ) {
            None
        } else {
            Some(self.watch_epoch.load(Ordering::Acquire))
        }
    }
    pub(in crate::automation_projects::dependencies) fn retire_watched(&self, generation: u64) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let Some(old) = watched_retirement(&mut *state, &self.watch_epoch, generation) else {
            return;
        };
        self.cancel.cancel();
        drop(state);
        if let State::Ready(_, ready)
        | State::Accepted(_, ready, _)
        | State::Publishing(_, ready, _) = old
        {
            dispose(ready);
        }
    }
    /// Called synchronously INSIDE the actual SQL transaction, after reading
    /// both captured rows and final scope/cancel/TTL/WorkingSet fences. A later
    /// SQL error can only become RecoveryRequired, never Accepted again.
    pub(in crate::automation_projects::dependencies) fn begin_sql_commit(
        &self,
        nonce: Uuid,
    ) -> AppResult<()> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        mark_sql_commit(
            &mut *state,
            &self.watch_epoch,
            nonce,
            !self.cancel.is_cancelled() && Instant::now() < self.expires,
        )
    }
    pub(in crate::automation_projects::dependencies) fn finish_publication(
        &self,
        nonce: Uuid,
        acknowledged: bool,
    ) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let State::Publishing(view, _, claim) = &*state else {
            return false;
        };
        if claim.nonce != nonce {
            return false;
        }
        let provisional = acknowledged
            && claim.commit_pending
            && !self.cancel.is_cancelled()
            && Instant::now() < self.expires;
        let next = if provisional {
            State::Completing(view.clone(), nonce)
        } else {
            State::Terminal(
                Phase::RecoveryRequired,
                "dependency_publication_recovery_required",
            )
        };
        let old = std::mem::replace(&mut *state, next);
        self.watch_epoch.fetch_add(1, Ordering::AcqRel);
        if !provisional {
            self.cancel.cancel();
        }
        drop(state);
        if let State::Publishing(_, ready, _) = old {
            dispose(ready);
        }
        provisional
    }
    pub(in crate::automation_projects::dependencies) fn complete_publication_cleanup(
        &self,
        nonce: Uuid,
        original_scope_and_joined: bool,
    ) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !matches!(&*state,State::Completing(_,current) if *current==nonce) {
            return false;
        }
        let success = original_scope_and_joined
            && !self.cancel.is_cancelled()
            && Instant::now() < self.expires;
        *state = State::Terminal(
            if success {
                Phase::Published
            } else {
                Phase::RecoveryRequired
            },
            if success {
                "dependency_published"
            } else {
                "dependency_publication_recovery_required"
            },
        );
        self.watch_epoch.fetch_add(1, Ordering::AcqRel);
        self.cancel.cancel();
        success
    }
    pub(in crate::automation_projects::dependencies) fn abandon_publication(&self, nonce: Uuid) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let Some(old) = abandon_state(&mut *state, nonce) else {
            return;
        };
        self.watch_epoch.fetch_add(1, Ordering::AcqRel);
        self.cancel.cancel();
        drop(state);
        if let State::Publishing(_, ready, _) = old {
            dispose(ready);
        }
    }
    pub(in crate::automation_projects::dependencies) async fn joined_cleanup(&self) -> bool {
        self.joined.wait().await
    }
}

impl DependencyRegistry {
    pub(in crate::automation_projects::dependencies) fn publish_work<T: Send + 'static>(
        &self,
        op: &Arc<Operation>,
        digest: &str,
        io: OwnedSemaphorePermit,
        worker: impl FnOnce(Arc<Ready>, Uuid) -> BoxFuture<'static, AppResult<T>> + Send + 'static,
    ) -> AppResult<tokio::sync::oneshot::Receiver<AppResult<T>>> {
        let maps = self.maps.lock().unwrap_or_else(|e| e.into_inner());
        if maps.closed
            || op.cancel.is_cancelled()
            || Instant::now() >= op.expires
            || !maps
                .operations
                .get(&op.id)
                .is_some_and(|current| Arc::ptr_eq(current, op))
        {
            return Err(invalid("dependency_publication_context_changed"));
        }
        let mut state = op.state.lock().unwrap_or_else(|e| e.into_inner());
        let State::Accepted(view, ready, acceptance) = &*state else {
            return Err(invalid("dependency_review_changed"));
        };
        if !acceptance.registry_owned || view.review_digest.as_deref() != Some(digest) {
            return Err(invalid("dependency_review_changed"));
        }
        let mut custody = ready
            ._resources
            .publication_io
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if custody.is_some() {
            return Err(invalid("dependency_publication_busy"));
        }
        *custody = Some(io);
        drop(custody);
        let nonce = Uuid::new_v4();
        let ready = claim_state(&mut *state, digest, nonce)?;
        op.watch_epoch.fetch_add(1, Ordering::AcqRel);
        let (send, receive) = tokio::sync::oneshot::channel();
        let mut tasks = self.workers.lock().unwrap_or_else(|e| e.into_inner());
        tasks.retain(|task| !task.is_finished());
        tasks.push(tokio::spawn(async move {
            let _ = send.send(worker(ready, nonce).await);
        }));
        Ok(receive)
    }
}

#[cfg(test)]
mod tests;
