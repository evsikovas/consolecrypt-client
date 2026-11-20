//! Profile transitions pause only new admission, never an existing worker.
//! The active-session lock is shared with final job admission; a UI list/count
//! check would leave a race. No vault resolver or decrypted metadata is needed.
use crate::prompts::TransitionDecision;
use crate::{session::Session, AppCore, AppError, AppResult};
use cc_storage_core::ProfileId;
use std::collections::BTreeSet;
use std::sync::{atomic::Ordering, Arc};
use std::time::Duration;

#[derive(Clone, Copy)]
pub(crate) enum TransitionAction {
    Switch,
    Close,
    Open(ProfileId),
    Remove(ProfileId),
}

pub(crate) struct ProfileTransitionLease {
    owner: Option<Arc<Session>>,
    _serial: tokio::sync::OwnedMutexGuard<()>,
}

impl Drop for ProfileTransitionLease {
    fn drop(&mut self) {
        if let Some(owner) = &self.owner {
            owner
                .automation
                .admission_paused
                .store(false, Ordering::Release);
        }
    }
}

impl AppCore {
    /// Reserve an approved exit BEFORE launching an installer. No profile is
    /// closed yet; the lease fences new admissions until commit/abort/expiry.
    pub async fn automation_prepare_exit(&self) -> AppResult<String> {
        self.prepare_exit_with_ttl(Duration::from_secs(120)).await
    }

    async fn prepare_exit_with_ttl(&self, ttl: Duration) -> AppResult<String> {
        let lease = self
            .begin_profile_transition(TransitionAction::Close)
            .await?;
        let token = uuid::Uuid::new_v4().to_string();
        *self
            .inner
            .prepared_exit
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some((token.clone(), lease));
        let weak = Arc::downgrade(&self.inner);
        let expiring = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(ttl).await;
            if let Some(inner) = weak.upgrade() {
                let expired = {
                    let mut pending = inner
                        .prepared_exit
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    if pending.as_ref().is_some_and(|(id, _)| id == &expiring) {
                        pending.take()
                    } else {
                        None
                    }
                };
                drop(expired); // Release only this exact lease, outside the map lock.
            }
        });
        Ok(token)
    }

    pub async fn automation_abort_exit(&self, token: String) -> AppResult<()> {
        let removed = {
            let mut pending = self
                .inner
                .prepared_exit
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if pending.as_ref().is_some_and(|(id, _)| id == &token) {
                pending.take()
            } else {
                None
            }
        };
        drop(removed);
        Ok(())
    }

    pub async fn automation_commit_exit(&self, token: String) -> AppResult<()> {
        let lease = {
            let mut pending = self
                .inner
                .prepared_exit
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if pending.as_ref().is_some_and(|(id, _)| id == &token) {
                pending.take().map(|(_, lease)| lease)
            } else {
                None
            }
        }
        .ok_or_else(|| AppError::not_found("exit_approval", "expired or already used"))?;
        self.close_profile_with_transition(lease).await
    }

    pub(crate) async fn begin_profile_transition(
        &self,
        action: TransitionAction,
    ) -> AppResult<ProfileTransitionLease> {
        let serial = self.inner.profile_transition.clone().lock_owned().await;
        let (owner, jobs) = {
            let active = self.inner.active.lock().await;
            let owner = active
                .as_ref()
                .filter(|owner| match action {
                    TransitionAction::Open(id) => id != owner.profile_id,
                    TransitionAction::Remove(id) => id == owner.profile_id,
                    _ => true,
                })
                .cloned();
            let jobs = owner
                .as_ref()
                .map(|owner| {
                    owner
                        .automation
                        .admission_paused
                        .store(true, Ordering::Release);
                    owner
                        .automation
                        .live_jobs
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .iter()
                        .map(|(id, (token, _))| (*id, token.clone()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            (owner, jobs)
        };
        let lease = ProfileTransitionLease {
            owner,
            _serial: serial,
        };
        // Only this session's accepted workers and loaded unknown leases are
        // relevant here. Never unseal/read a locked database merely to exit.
        // Crash recovery is separately performed on explicit jobs/run access.
        let mut displayed_unknown: BTreeSet<_> = lease
            .owner
            .as_ref()
            .map(|owner| {
                owner
                    .automation
                    .coordinator
                    .unknown_jobs()
                    .into_iter()
                    .collect()
            })
            .unwrap_or_default();
        if jobs.is_empty() && displayed_unknown.is_empty() {
            return Ok(lease);
        }
        let operation = match action {
            TransitionAction::Close => "close",
            TransitionAction::Remove(_) => "remove_profile",
            _ => "switch_profile",
        };
        let mut question = self.inner.ctx.prompts.transition(
            jobs.iter()
                .map(|(id, _)| *id)
                .chain(displayed_unknown.iter().copied())
                .collect::<BTreeSet<_>>()
                .len() as u32,
            displayed_unknown.len() as u32,
            operation,
        )?;
        let mut chosen = false;
        let mut acknowledged_unknown = BTreeSet::new();
        let mut tick = tokio::time::interval(Duration::from_millis(200));
        let initial_timeout = tokio::time::sleep(Duration::from_secs(
            self.inner.ctx.config.prompt_timeout_secs.max(1),
        ));
        tokio::pin!(initial_timeout);
        loop {
            tokio::select! {
                biased;
                decision = question.choices.recv() => match decision {
                    Some(TransitionDecision::Wait) => chosen = true,
                    Some(TransitionDecision::CancelJobs) => {
                        for (_, token) in &jobs { token.cancel(); }
                        chosen = true;
                    }
                    Some(TransitionDecision::CancelJobsAcceptUnknown) => {
                        acknowledged_unknown.extend(displayed_unknown.iter().copied());
                        for (_, token) in &jobs { token.cancel(); }
                        chosen = true;
                    }
                    _ => return Err(AppError::Cancelled),
                },
                _ = &mut initial_timeout, if !chosen => return Err(AppError::invalid("automation_transition", "transition choice timed out")),
                _ = tick.tick(), if chosen => {
                    let owner = lease.owner.as_ref().expect("captured jobs have an owner");
                    let (unknown, live) = {
                        // run_admission marks its ticket unknown BEFORE removing
                        // live_jobs. Observe both while holding live_jobs, so a
                        // new unknown cannot fall between the last two checks.
                        let live = owner.automation.live_jobs.lock().unwrap_or_else(|e| e.into_inner());
                        let unknown: BTreeSet<_> = owner.automation.coordinator.unknown_jobs().into_iter().collect();
                        (unknown, live.keys().copied().collect::<BTreeSet<_>>())
                    };
                    if unknown.iter().any(|id| !acknowledged_unknown.contains(id)) {
                        // A worker can lose its channel AFTER Wait was chosen.
                        // Replace that completed question with an explicit new
                        // uncertainty review; never silently treat it as done.
                        drop(question);
                        displayed_unknown = unknown;
                        question = self.inner.ctx.prompts.transition(live.union(&displayed_unknown).count() as u32,
                            displayed_unknown.len() as u32, operation)?;
                        chosen = false;
                        initial_timeout.as_mut().reset(tokio::time::Instant::now() + Duration::from_secs(
                            self.inner.ctx.config.prompt_timeout_secs.max(1)));
                        continue;
                    }
                    let finished = jobs.iter().all(|(id, _)| !live.contains(id));
                    if finished { return Ok(lease); }
                }
            }
        }
    }

    pub async fn automation_answer_transition(
        &self,
        request_id: String,
        decision: String,
    ) -> AppResult<()> {
        let choice = match decision.as_str() {
            "wait" => TransitionDecision::Wait,
            "cancel_jobs" => TransitionDecision::CancelJobs,
            "cancel_jobs_accept_unknown" => TransitionDecision::CancelJobsAcceptUnknown,
            "abort" => TransitionDecision::Abort,
            _ => {
                return Err(AppError::invalid(
                    "transition_decision",
                    "invalid transition decision",
                ))
            }
        };
        self.inner
            .ctx
            .prompts
            .answer_transition(&request_id, choice)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::PromptRequest;
    use tokio_util::sync::CancellationToken;
    use uuid::Uuid;

    const PASSWORD: &str = "correct horse transition test passphrase";

    async fn fixture() -> (tempfile::TempDir, AppCore, Arc<Session>) {
        let directory = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            directory.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile("Transition fixture".into(), PASSWORD.into())
            .await
            .unwrap();
        let owner = app.session().await.unwrap();
        (directory, app, owner)
    }

    fn job(owner: &Session) -> (Uuid, CancellationToken) {
        let id = Uuid::new_v4();
        let token = CancellationToken::new();
        owner
            .automation
            .live_jobs
            .lock()
            .unwrap()
            .insert(id, (token.clone(), false));
        (id, token)
    }

    #[tokio::test]
    async fn no_presenter_never_silently_cancels_or_creates_a_half_profile_even_while_locked() {
        let (_directory, app, _) = fixture().await;
        let first = app.session().await.unwrap().profile_id;
        let inactive = app
            .create_local_profile("Inactive fixture".into(), PASSWORD.into())
            .await
            .unwrap();
        app.open_profile(first.to_string()).await.unwrap();
        let owner = app.session().await.unwrap();
        assert!(owner.unlocked_opt().await.is_none());
        let (id, token) = job(&owner);
        let count = app.list_profiles().await.unwrap().len();
        assert!(app.close_profile().await.is_err());
        assert!(app
            .create_local_profile("Must not exist".into(), PASSWORD.into())
            .await
            .is_err());
        assert_eq!(app.list_profiles().await.unwrap().len(), count);
        assert!(app.remove_profile(first.to_string()).await.is_err());
        app.remove_profile(inactive.profile.id).await.unwrap();
        // Re-selecting the same profile and deleting an inactive one do not
        // affect accepted jobs and do not need an unrelated transition prompt.
        app.open_profile(first.to_string()).await.unwrap();
        assert!(!token.is_cancelled());
        assert!(!owner.automation.admission_paused.load(Ordering::Acquire));
        owner.automation.live_jobs.lock().unwrap().remove(&id);
        app.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn wait_can_be_aborted_without_cancelling_jobs_or_losing_the_current_profile() {
        let (_directory, app, owner) = fixture().await;
        let (id, token) = job(&owner);
        let mut prompts = app.subscribe_prompts();
        let closing = tokio::spawn({
            let app = app.clone();
            async move { app.close_profile().await }
        });
        let PromptRequest::AutomationTransition(prompt) = prompts.recv().await.unwrap() else {
            panic!("transition prompt required")
        };
        assert_eq!(prompt.job_count, 1);
        let waiting = tokio::spawn({
            let app = app.clone();
            let id = prompt.request_id.clone();
            async move { app.automation_answer_transition(id, "wait".into()).await }
        });
        tokio::task::yield_now().await;
        assert!(owner.automation.admission_paused.load(Ordering::Acquire));
        assert!(!closing.is_finished());
        app.automation_answer_transition(prompt.request_id, "abort".into())
            .await
            .unwrap();
        assert!(closing.await.unwrap().is_err());
        waiting.await.unwrap().unwrap();
        assert_eq!(app.session().await.unwrap().profile_id, owner.profile_id);
        assert!(!token.is_cancelled());
        assert!(!owner.automation.admission_paused.load(Ordering::Acquire));
        owner.automation.live_jobs.lock().unwrap().remove(&id);
        app.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn cancel_and_finish_waits_for_worker_cleanup_before_closing() {
        let (_directory, app, owner) = fixture().await;
        let (id, token) = job(&owner);
        let (_, sibling) = job(&owner);
        let mut prompts = app.subscribe_prompts();
        let closing = tokio::spawn({
            let app = app.clone();
            async move { app.shutdown().await }
        });
        let PromptRequest::AutomationTransition(prompt) = prompts.recv().await.unwrap() else {
            panic!("transition prompt required")
        };
        assert_eq!(prompt.job_count, 2);
        let answer = tokio::spawn({
            let app = app.clone();
            async move {
                app.automation_answer_transition(prompt.request_id, "cancel_jobs".into())
                    .await
            }
        });
        tokio::time::timeout(Duration::from_secs(2), token.cancelled())
            .await
            .unwrap();
        assert!(sibling.is_cancelled());
        assert!(!closing.is_finished());
        assert_eq!(app.session().await.unwrap().profile_id, owner.profile_id);
        owner.automation.live_jobs.lock().unwrap().remove(&id);
        assert!(!closing.is_finished());
        owner.automation.live_jobs.lock().unwrap().clear();
        closing.await.unwrap().unwrap();
        answer.await.unwrap().unwrap();
        assert!(app.session().await.is_err());
        assert!(!owner.automation.admission_paused.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn dropping_a_pending_transition_releases_admission_and_invalidates_its_question() {
        let (_directory, app, owner) = fixture().await;
        let (id, token) = job(&owner);
        let mut prompts = app.subscribe_prompts();
        let closing = tokio::spawn({
            let app = app.clone();
            async move { app.close_profile().await }
        });
        let PromptRequest::AutomationTransition(prompt) = prompts.recv().await.unwrap() else {
            panic!("transition prompt required")
        };
        closing.abort();
        assert!(closing.await.unwrap_err().is_cancelled());
        assert!(!owner.automation.admission_paused.load(Ordering::Acquire));
        assert!(!token.is_cancelled());
        assert!(app
            .automation_answer_transition(prompt.request_id, "cancel_jobs".into())
            .await
            .is_err());
        owner.automation.live_jobs.lock().unwrap().remove(&id);
        app.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn newly_unknown_outcome_requires_a_new_specific_choice_and_is_not_acknowledged_as_terminal(
    ) {
        let (_directory, app, owner) = fixture().await;
        let (id, _) = job(&owner);
        let mut prompts = app.subscribe_prompts();
        let closing = tokio::spawn({
            let app = app.clone();
            async move { app.close_profile().await }
        });
        let PromptRequest::AutomationTransition(first) = prompts.recv().await.unwrap() else {
            panic!("transition required")
        };
        let waiting = tokio::spawn({
            let app = app.clone();
            async move {
                app.automation_answer_transition(first.request_id, "wait".into())
                    .await
            }
        });
        owner
            .automation
            .coordinator
            .restore_unknown(id, Uuid::new_v4(), [Uuid::new_v4()])
            .unwrap();
        owner.automation.live_jobs.lock().unwrap().remove(&id);
        let PromptRequest::AutomationTransition(next) =
            tokio::time::timeout(Duration::from_secs(3), prompts.recv())
                .await
                .unwrap()
                .unwrap()
        else {
            panic!("new uncertainty review required")
        };
        waiting.await.unwrap().unwrap();
        assert_eq!(next.unknown_count, 1);
        assert!(!closing.is_finished());
        let answering = tokio::spawn({
            let app = app.clone();
            async move {
                app.automation_answer_transition(
                    next.request_id,
                    "cancel_jobs_accept_unknown".into(),
                )
                .await
            }
        });
        closing.await.unwrap().unwrap();
        answering.await.unwrap().unwrap();
        // A transition acknowledgement only permits closing. It never resolves
        // the job reservation or claims that a detached remote operation ended.
        assert_eq!(owner.automation.coordinator.unknown_jobs(), vec![id]);
    }

    #[tokio::test]
    async fn closing_a_locked_profile_never_opens_or_rewrites_a_future_job_catalog() {
        let (_directory, app, owner) = fixture().await;
        let future = serde_json::json!({"version":2,"records":{},"future":"must-survive"});
        let stored = future.clone();
        owner
            .storage()
            .unwrap()
            .write(move |tx| {
                tx.setting_set("automation.jobs.v1", &stored)?;
                Ok::<_, AppError>(())
            })
            .await
            .unwrap();
        app.lock().await.unwrap();
        assert!(owner.storage_opt().is_none());
        app.shutdown().await.unwrap();
        app.open_profile(owner.profile_id.to_string())
            .await
            .unwrap();
        app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
        let reopened = app.session().await.unwrap();
        let saved: serde_json::Value = reopened
            .storage()
            .unwrap()
            .setting_get("automation.jobs.v1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved, future);
        app.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn prepared_exit_is_one_use_abort_is_specific_and_does_not_close_the_profile() {
        let (_directory, app, owner) = fixture().await;
        let first = app.automation_prepare_exit().await.unwrap();
        assert!(owner.automation.admission_paused.load(Ordering::Acquire));
        assert_eq!(app.session().await.unwrap().profile_id, owner.profile_id);
        app.automation_abort_exit("wrong-token".into())
            .await
            .unwrap();
        assert!(owner.automation.admission_paused.load(Ordering::Acquire));
        assert!(app
            .automation_commit_exit("wrong-token".into())
            .await
            .is_err());
        app.automation_abort_exit(first.clone()).await.unwrap();
        assert!(!owner.automation.admission_paused.load(Ordering::Acquire));
        let second = app.automation_prepare_exit().await.unwrap();
        assert_ne!(first, second);
        app.automation_abort_exit(first.clone()).await.unwrap();
        assert!(app.automation_commit_exit(first).await.is_err());
        assert!(owner.automation.admission_paused.load(Ordering::Acquire));
        app.automation_commit_exit(second.clone()).await.unwrap();
        assert!(!owner.automation.admission_paused.load(Ordering::Acquire));
        assert!(app.session().await.is_err());
        assert!(app.automation_commit_exit(second).await.is_err());
    }

    #[tokio::test]
    async fn an_abandoned_install_preparation_expires_without_closing_or_mutating_the_profile() {
        let (_directory, app, owner) = fixture().await;
        let token = app
            .prepare_exit_with_ttl(Duration::from_millis(50))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while owner.automation.admission_paused.load(Ordering::Acquire) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(app.automation_commit_exit(token).await.is_err());
        assert_eq!(app.session().await.unwrap().profile_id, owner.profile_id);
        app.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn declining_install_preparation_never_closes_or_cancels_existing_jobs() {
        let (_directory, app, owner) = fixture().await;
        let (id, token) = job(&owner);
        let mut prompts = app.subscribe_prompts();
        let preparing = tokio::spawn({
            let app = app.clone();
            async move { app.automation_prepare_exit().await }
        });
        let PromptRequest::AutomationTransition(prompt) = prompts.recv().await.unwrap() else {
            panic!("transition required")
        };
        app.automation_answer_transition(prompt.request_id, "abort".into())
            .await
            .unwrap();
        assert!(preparing.await.unwrap().is_err());
        assert!(!token.is_cancelled());
        assert!(!owner.automation.admission_paused.load(Ordering::Acquire));
        assert!(app.inner.prepared_exit.lock().unwrap().is_none());
        owner.automation.live_jobs.lock().unwrap().remove(&id);
        app.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn prepared_exit_fences_crash_recovery_without_rewriting_the_saved_record() {
        let (_directory, app, owner) = fixture().await;
        let id = Uuid::new_v4();
        let row = serde_json::json!({"version":1,"job_id":id,"runner_id":Uuid::new_v4(),
            "host_ids":[Uuid::new_v4()],"state":"running","manifest_digest":"a".repeat(64)});
        let stored = serde_json::json!({"version":1,"records":{id.to_string():row}});
        let original = stored.clone();
        owner
            .storage()
            .unwrap()
            .write(move |tx| {
                tx.setting_set("automation.jobs.v1", &stored)?;
                Ok::<_, AppError>(())
            })
            .await
            .unwrap();
        let token = app.automation_prepare_exit().await.unwrap();
        assert!(app.automation_jobs().await.is_err());
        let current: serde_json::Value = owner
            .storage()
            .unwrap()
            .setting_get("automation.jobs.v1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(current, original);
        assert!(owner.automation.coordinator.unknown_jobs().is_empty());
        app.automation_abort_exit(token).await.unwrap();
        let recovered = app.automation_jobs().await.unwrap();
        assert_eq!(recovered["jobs"][0]["state"], "unknown");
        assert_eq!(owner.automation.coordinator.unknown_jobs(), vec![id]);
        app.automation_acknowledge_unknown(id.to_string(), true)
            .await
            .unwrap();
        app.shutdown().await.unwrap();
    }
}
