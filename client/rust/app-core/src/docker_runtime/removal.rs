use super::{docker_error, invalid, AppCore, AppResult};
use cc_docker_core::{DockerRemovalPlan, DockerRemovalResult, DockerRemovalSelection};
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

const PREVIEW_LIFETIME: Duration = Duration::from_secs(10 * 60);
const MAX_PREVIEWS: usize = 64;

struct Preview {
    session_id: String,
    created_at: Instant,
    plan: DockerRemovalPlan,
}

#[derive(Default)]
pub(super) struct PendingRemovals(Mutex<HashMap<String, Preview>>);

impl PendingRemovals {
    fn register(
        &self,
        session_id: String,
        mut plan: DockerRemovalPlan,
    ) -> AppResult<DockerRemovalPlan> {
        let mut pending = self.0.lock().unwrap_or_else(|e| e.into_inner());
        pending.retain(|_, p| {
            p.created_at.elapsed() < PREVIEW_LIFETIME
                && !(p.session_id == session_id && p.plan.container_id == plan.container_id)
        });
        if pending.len() >= MAX_PREVIEWS {
            return Err(invalid("too_many_previews"));
        }
        plan.plan_id = uuid::Uuid::new_v4().to_string();
        pending.insert(
            plan.plan_id.clone(),
            Preview {
                session_id,
                created_at: Instant::now(),
                plan: plan.clone(),
            },
        );
        Ok(plan)
    }

    fn take(&self, session_id: &str, container: &str, token: &str) -> AppResult<DockerRemovalPlan> {
        let mut pending = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if !pending.get(token).is_some_and(|p| {
            p.session_id == session_id
                && p.plan.container_id == container
                && p.created_at.elapsed() < PREVIEW_LIFETIME
        }) {
            return Err(invalid("changed"));
        }
        Ok(pending.remove(token).expect("checked under lock").plan)
    }

    pub(super) fn clear(&self) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    pub(super) fn forget_session(&self, session_id: &str) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|_, p| p.session_id != session_id);
    }
}

impl AppCore {
    pub async fn docker_removal_plan(
        &self,
        session_id: String,
        container: String,
    ) -> AppResult<DockerRemovalPlan> {
        let (_, unlocked) = self.unlocked().await?;
        let live = unlocked.docker.session(&session_id)?;
        let plan = tokio::select! {
            biased;
            _ = live.cancel.cancelled() => return Err(invalid("session_closed")),
            plan = live.client.removal_plan(&container) => plan.map_err(docker_error)?,
        };
        unlocked.docker.removals.register(session_id, plan)
    }

    pub async fn docker_remove_with_cleanup(
        &self,
        session_id: String,
        container: String,
        selection: DockerRemovalSelection,
    ) -> AppResult<DockerRemovalResult> {
        let (_, unlocked) = self.unlocked().await?;
        let live = unlocked.docker.session(&session_id)?;
        let plan = unlocked
            .docker
            .removals
            .take(&session_id, &container, &selection.plan_id)?;
        live.client
            .remove_with_cleanup(&plan, &selection, &live.cancel)
            .await
            .map_err(docker_error)
    }
}
