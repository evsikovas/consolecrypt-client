use crate::{
    models::*,
    runtime::cli::{Runtime, validate_id},
};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::Mutex;

pub struct ContainerService<R> {
    pub runtime: R,
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl<R: Runtime> ContainerService<R> {
    pub fn new(runtime: R) -> Self {
        Self {
            runtime,
            locks: Mutex::new(HashMap::new()),
        }
    }

    async fn lock_for(&self, id: &str) -> Arc<Mutex<()>> {
        let mut locks = self.locks.lock().await;
        locks.retain(|_, lock| Arc::strong_count(lock) > 1);
        locks.entry(id.into()).or_default().clone()
    }

    /// Removes a container and confirms it is gone from a fresh list. Never retried.
    pub async fn remove(&self, id: &str, force: bool) -> AppResult<()> {
        validate_id(id)?;
        let lock = self.lock_for(id).await;
        let _guard = lock.lock().await;
        let before = self.runtime.inspect_container(id, false).await?;
        if before.state == ContainerState::Running && !force {
            return Err(AppError::new(
                "invalid_state",
                "remove",
                "Сначала остановите контейнер или подтвердите принудительное удаление",
            ));
        }
        let command = self.runtime.remove(id, force).await;
        let present = self
            .runtime
            .list_containers()
            .await?
            .iter()
            .any(|c| c.id == id);
        match (command, present) {
            (_, false) => Ok(()),
            (Err(e), true) => Err(e),
            (Ok(()), true) => Err(AppError::new(
                "uncertain",
                "remove",
                "Runtime сообщил об удалении, но контейнер всё ещё в списке",
            )),
        }
    }

    pub async fn operate(
        &self,
        id: &str,
        action: ContainerAction,
        confirmed: bool,
    ) -> AppResult<OperationResult> {
        validate_id(id)?;
        let lock = self.lock_for(id).await;
        let _guard = lock.lock().await;
        let before = self.runtime.inspect_container(id, false).await?;
        check_action(&before, action, confirmed)?;
        let budget = before
            .stop_timeout_seconds
            .unwrap_or(45)
            .saturating_add(15)
            .max(60);
        if budget > 300 {
            return Err(AppError::new(
                "stop_budget",
                action.command(),
                "Время штатной остановки превышает проверенный бюджет ожидания",
            ));
        }
        let command = self
            .runtime
            .mutate(id, action, Duration::from_secs(budget))
            .await;
        // Never retry a mutation. Even a non-timeout CLI error can follow a state change.
        let after = self.runtime.inspect_container(id, false).await;
        let expected = if action == ContainerAction::Stop {
            ContainerState::Exited
        } else {
            ContainerState::Running
        };
        let uncertain = command.is_err() || !after.as_ref().is_ok_and(|d| d.state == expected);
        let (current, verification_error) = match after {
            Ok(d) => (Some(d), None),
            Err(e) => (None, Some(e)),
        };
        Ok(OperationResult {
            container_id: id.into(),
            uncertain,
            command_error: command.err(),
            current,
            verification_error,
        })
    }
}

pub fn check_action(
    details: &ContainerDetails,
    action: ContainerAction,
    confirmed: bool,
) -> AppResult<()> {
    let allowed = match action {
        ContainerAction::Start => matches!(
            details.state,
            ContainerState::Created | ContainerState::Exited
        ),
        ContainerAction::Stop | ContainerAction::Restart => {
            details.state == ContainerState::Running
        }
    };
    if !allowed {
        return Err(AppError::new(
            "invalid_state",
            action.command(),
            "Действие недоступно в текущем состоянии контейнера",
        ));
    }
    if action != ContainerAction::Start && !confirmed {
        return Err(AppError::new(
            "confirmation_required",
            action.command(),
            "Подтвердите действие: runtime может удалить контейнер после остановки; флаг auto-remove может быть неизвестен",
        ));
    }
    if action == ContainerAction::Restart && details.auto_remove == Some(true) {
        return Err(AppError::new(
            "auto_remove",
            "restart",
            "Перезапуск контейнера с auto-remove отключён",
        ));
    }
    Ok(())
}
