//! Legacy encrypted-settings admission review lifetime only. Kept because
//! Automation metadata synchronization shares the original guarded writer.
//! No metrics client, saved-source transport or Agent execution capability.
use crate::{AppError, AppResult};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
pub(crate) struct SettingsReviewRuntime {
    operations: Mutex<HashMap<Uuid, CancellationToken>>,
    permits: Arc<tokio::sync::Semaphore>,
}
impl Default for SettingsReviewRuntime {
    fn default() -> Self {
        Self {
            operations: Mutex::new(HashMap::new()),
            permits: Arc::new(tokio::sync::Semaphore::new(8)),
        }
    }
}
pub(crate) struct OwnedSettingsReviewRegistration {
    unlock: Weak<crate::session::Unlocked>,
    id: Uuid,
    _permit: tokio::sync::OwnedSemaphorePermit,
}
impl Drop for OwnedSettingsReviewRegistration {
    fn drop(&mut self) {
        if let Some(u) = self.unlock.upgrade() {
            let cancelled = u
                .settings_reviews
                .operations
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&self.id);
            if let Some(cancel) = cancelled {
                cancel.cancel();
            }
        }
    }
}
impl SettingsReviewRuntime {
    pub(crate) fn begin_owned(
        unlock: Weak<crate::session::Unlocked>,
    ) -> AppResult<(OwnedSettingsReviewRegistration, CancellationToken)> {
        let u = unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let permit = u
            .settings_reviews
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| AppError::invalid("settings_review", "concurrency_budget"))?;
        let id = Uuid::new_v4();
        let cancel = CancellationToken::new();
        let mut operations = u
            .settings_reviews
            .operations
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if operations.len() >= 32 {
            return Err(AppError::invalid("settings_review", "concurrency_budget"));
        }
        operations.insert(id, cancel.clone());
        drop(operations);
        drop(u);
        Ok((
            OwnedSettingsReviewRegistration {
                unlock,
                id,
                _permit: permit,
            },
            cancel,
        ))
    }
    pub(crate) fn close(&self) {
        for token in self
            .operations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
        {
            token.cancel();
        }
    }
}
