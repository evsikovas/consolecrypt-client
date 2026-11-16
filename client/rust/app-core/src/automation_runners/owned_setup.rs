//! One bounded entry in the EXISTING setup map. No public ID lookup/authority.
//! Weak cancellation projection only; actual one-use owner remains issued Rust.
use super::*;
use crate::session::Session;
use std::sync::{atomic::Ordering, Weak};
use tokio_util::sync::CancellationToken;

/// Internal native lifecycle cancellation only. Never a generic resolver or
/// serializable token. Runtime invokes it AFTER releasing setup_operations.
pub(crate) trait OwnedSetupCancellation: Send + Sync {
    fn cancel_owned(&self);
    fn joined_token(&self) -> CancellationToken;
}
/// Native-chosen reservation. It is retained by the issued control until idle
/// payload wipe, or until the last real worker registration has retired.
pub(crate) struct OwnedSetupSlot {
    owner: Arc<Session>,
    id: Uuid,
    identity: Weak<dyn OwnedSetupCancellation>,
}
impl OwnedSetupSlot {
    #[cfg(test)]
    pub(crate) fn id(&self) -> Uuid {
        self.id
    }
}
impl Drop for OwnedSetupSlot {
    fn drop(&mut self) {
        let mut map = self
            .owner
            .automation
            .setup_operations
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // An old release cannot erase a different borrowed tombstone/operation.
        let exact = map
            .get(&self.id)
            .and_then(|entry| entry.owned.as_ref())
            .is_some_and(|current| Weak::ptr_eq(current, &self.identity));
        if exact {
            map.remove(&self.id);
        }
    }
}
pub(crate) fn register(
    owner: &Arc<Session>,
    identity: Weak<dyn OwnedSetupCancellation>,
    cancel: CancellationToken,
    expires: std::time::Instant,
) -> AppResult<OwnedSetupSlot> {
    let runtime = &owner.automation;
    let mut map = runtime
        .setup_operations
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    map.retain(|_, op| op.owned.is_some() || op.started || op.expires > std::time::Instant::now());
    if runtime.setup_owned_state.load(Ordering::Acquire) != 0
        || runtime.stopped.is_cancelled()
        || expires <= std::time::Instant::now()
        || cancel.is_cancelled()
        || identity.strong_count() == 0
        || map.len() >= 16
    {
        return Err(AppError::invalid(
            "portable_project",
            "portable_operation_unavailable",
        ));
    }
    let id = (0..4)
        .map(|_| Uuid::new_v4())
        .find(|id| !map.contains_key(id))
        .ok_or_else(|| AppError::invalid("portable_project", "portable_operation_unavailable"))?;
    map.insert(
        id,
        SetupOperation {
            cancel,
            started: true,
            expires,
            owned: Some(identity.clone()),
        },
    );
    Ok(OwnedSetupSlot {
        owner: owner.clone(),
        id,
        identity,
    })
}
impl AutomationRuntime {
    pub(super) fn cancel_owned_setup(&self, permanently: bool) -> Vec<CancellationToken> {
        // Admission closes BEFORE the same-map snapshot: either a concurrent
        // registration fails, or the snapshot includes it. Reopen cannot revive
        // an issued control's terminal phase or its removed secret body.
        self.setup_owned_state
            .fetch_max(if permanently { 2 } else { 1 }, Ordering::AcqRel);
        let controls = {
            let map = self
                .setup_operations
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            map.values()
                .filter_map(|entry| entry.owned.as_ref().and_then(Weak::upgrade))
                .collect::<Vec<_>>()
        };
        let mut joined = Vec::with_capacity(controls.len());
        for control in controls {
            joined.push(control.joined_token());
            control.cancel_owned();
        }
        joined
    }
}

#[cfg(test)]
pub(crate) fn test_count(owner: &Session) -> usize {
    owner.automation.setup_operations.lock().unwrap().len()
}
#[cfg(test)]
pub(crate) fn test_map_unlocked(owner: &Session) -> bool {
    owner.automation.setup_operations.try_lock().is_ok()
}
