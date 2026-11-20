//! Current Host observations only. No source catalog, agents, exporters, metrics
//! database, historical queries or background installation capability.
use crate::{AppCore, AppError, AppResult};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
mod live;
fn invalid(reason: &str) -> AppError {
    AppError::invalid("host_metrics", reason)
}
struct Operation {
    cancel: CancellationToken,
    started: bool,
    expires: Instant,
}
pub(crate) struct HostMetricsRuntime {
    operations: Mutex<HashMap<Uuid, Operation>>,
    permits: Arc<tokio::sync::Semaphore>,
}
impl Default for HostMetricsRuntime {
    fn default() -> Self {
        Self {
            operations: Mutex::new(HashMap::new()),
            permits: Arc::new(tokio::sync::Semaphore::new(8)),
        }
    }
}
struct OwnedHostMetricsRegistration {
    unlock: Weak<crate::session::Unlocked>,
    id: Uuid,
    _permit: tokio::sync::OwnedSemaphorePermit,
}
impl Drop for OwnedHostMetricsRegistration {
    fn drop(&mut self) {
        if let Some(u) = self.unlock.upgrade() {
            let operation = u
                .host_metrics
                .operations
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&self.id);
            if let Some(operation) = operation {
                operation.cancel.cancel();
            }
        }
    }
}
impl HostMetricsRuntime {
    fn register(&self, id: Uuid) -> AppResult<CancellationToken> {
        if id.is_nil() {
            return Err(invalid("invalid operation identifier"));
        }
        let mut operations = self.operations.lock().unwrap_or_else(|e| e.into_inner());
        operations.retain(|_, op| op.started || op.expires > Instant::now());
        if operations.contains_key(&id) {
            return Err(invalid("operation already active or cancelled"));
        }
        if operations.len() >= 32 {
            return Err(invalid("host metrics concurrency budget reached"));
        }
        let cancel = CancellationToken::new();
        operations.insert(
            id,
            Operation {
                cancel: cancel.clone(),
                started: true,
                expires: Instant::now() + Duration::from_secs(45),
            },
        );
        Ok(cancel)
    }
    fn cancel(&self, id: Uuid) -> AppResult<()> {
        if id.is_nil() {
            return Err(invalid("invalid operation identifier"));
        }
        let mut operations = self.operations.lock().unwrap_or_else(|e| e.into_inner());
        operations.retain(|_, op| op.started || op.expires > Instant::now());
        if let Some(op) = operations.get(&id) {
            op.cancel.cancel();
            return Ok(());
        }
        if operations.len() >= 32 {
            return Err(invalid("host metrics concurrency budget reached"));
        }
        let cancel = CancellationToken::new();
        cancel.cancel();
        operations.insert(
            id,
            Operation {
                cancel,
                started: false,
                expires: Instant::now() + Duration::from_secs(45),
            },
        );
        Ok(())
    }
    pub(crate) fn close(&self) {
        for op in self
            .operations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
        {
            op.cancel.cancel();
        }
    }
}
impl AppCore {
    pub async fn host_metrics_cancel(&self, operation_id: String) -> AppResult<()> {
        let id =
            Uuid::parse_str(&operation_id).map_err(|_| invalid("invalid operation identifier"))?;
        let (owner, u) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        u.host_metrics.cancel(id)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancel_before_registration_cannot_restart_a_late_watch() {
        let runtime = HostMetricsRuntime::default();
        let id = Uuid::new_v4();
        runtime.cancel(id).unwrap();
        assert!(runtime.register(id).is_err());
    }
    #[test]
    fn cancellation_and_close_retire_existing_tokens() {
        let runtime = HostMetricsRuntime::default();
        let id = Uuid::new_v4();
        let first = runtime.register(id).unwrap();
        runtime.cancel(id).unwrap();
        assert!(first.is_cancelled());
        let second = runtime.register(Uuid::new_v4()).unwrap();
        runtime.close();
        assert!(second.is_cancelled());
    }
    #[test]
    fn identifiers_and_registry_are_bounded() {
        let runtime = HostMetricsRuntime::default();
        assert!(runtime.register(Uuid::nil()).is_err());
        for _ in 0..32 {
            runtime.cancel(Uuid::new_v4()).unwrap();
        }
        assert!(runtime.register(Uuid::new_v4()).is_err());
    }
}
