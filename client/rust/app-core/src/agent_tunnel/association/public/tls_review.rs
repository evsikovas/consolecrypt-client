//! Explicit fixed client identity creation. The original owned OS job is joined,
//! even when its caller cancels; a pending/unknown job is never replaced.
use super::*;
use std::sync::Mutex;
use tokio::sync::{OwnedSemaphorePermit, oneshot, watch};
struct State {
    view: Value,
    busy: bool,
    spent: bool,
    orphaned: bool,
    permit: Option<OwnedSemaphorePermit>,
}
struct Shared {
    context: Context,
    custody: ManagedTlsCustody,
    state: Mutex<State>,
    done: watch::Sender<bool>,
}
pub struct NodeClientIdentityReview {
    shared: Arc<Shared>,
}
impl std::fmt::Debug for NodeClientIdentityReview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NodeClientIdentityReview([redacted])")
    }
}
impl Drop for NodeClientIdentityReview {
    fn drop(&mut self) {
        self.cancel()
    }
}
impl Shared {
    fn retire(&self) {
        self.context.cancel.cancel();
        if let Ok(mut s) = self.state.lock() {
            if !s.busy && !s.spent {
                s.spent = true;
                s.permit.take();
                s.view["phase"] = json!(if Instant::now() >= self.context.scope.deadline() {
                    "expired"
                } else {
                    "cancelled"
                });
            }
        }
    }
}
struct WorkGuard {
    shared: Arc<Shared>,
    finished: bool,
}
impl Drop for WorkGuard {
    fn drop(&mut self) {
        if !self.finished {
            if let Ok(mut s) = self.shared.state.lock() {
                s.busy = false;
                s.orphaned = true;
                s.view["phase"] = json!("unknown");
                s.permit.take();
            }
            self.shared.done.send_replace(true);
        }
    }
}
impl NodeClientIdentityReview {
    pub(super) fn new(
        context: Context,
        custody: ManagedTlsCustody,
        permit: OwnedSemaphorePermit,
        owner: Value,
    ) -> AppResult<Self> {
        context.scope.check_current()?;
        let view = json!({"version":1,"profileId":context.frame.profile_id,"notificationScopeId":context.frame.notification_scope_id,
            "phase":"createReview","owner":owner,"algorithm":"ecdsaP256Sha256","validityDays":30,"remainingMs":0,
            "identityId":null,"certificateSha256":null,"cleanupState":"notStarted"});
        let (done, _) = watch::channel(true);
        let shared = Arc::new(Shared {
            context,
            custody,
            state: Mutex::new(State {
                view,
                busy: false,
                spent: false,
                orphaned: false,
                permit: Some(permit),
            }),
            done,
        });
        let weak = Arc::downgrade(&shared);
        let scope = shared.context.scope.clone();
        shared.context.runtime.spawn(async move {
            scope.cancelled().await;
            if let Some(shared) = weak.upgrade() {
                shared.retire()
            }
        });
        Ok(Self { shared })
    }
    pub fn cancel(&self) {
        self.shared.retire()
    }
    pub async fn details_json(&self) -> AppResult<String> {
        self.shared.context.check().await?;
        let s = self.shared.state.lock().map_err(|_| unknown())?;
        let mut view = s.view.clone();
        view["cleanupState"] = json!(if s.orphaned {
            "unknown"
        } else if s.busy {
            "pending"
        } else if s.spent {
            "closedObserved"
        } else {
            "notStarted"
        });
        drop(s);
        self.shared.context.publish(view, 16384)
    }
    pub async fn confirm_creation(&self) -> AppResult<String> {
        self.shared.context.scope.check_current()?;
        {
            let mut s = self.shared.state.lock().map_err(|_| unknown())?;
            if s.spent || s.busy {
                return Err(unavailable());
            }
            s.spent = true;
            s.busy = true;
            s.view["phase"] = json!("creating");
            s.view["cleanupState"] = json!("pending");
            self.shared.done.send_replace(false);
        }
        let shared = self.shared.clone();
        let (tx, rx) = oneshot::channel();
        self.shared.context.runtime.spawn(async move {
            let mut guard = WorkGuard {
                shared: shared.clone(),
                finished: false,
            };
            // This future is intentionally NOT selected against cancel/deadline:
            // the existing spawn_blocking credential job owns its join. Each
            // following signing/SQL admission independently rechecks the SAME
            // original scope, so cancelled work cannot publish stored success.
            let result = shared.custody.create_original(&shared.context.core).await;
            let live = shared.context.scope.check_current().is_ok()
                && !shared.context.cancel.is_cancelled();
            let mut s = match shared.state.lock() {
                Ok(s) => s,
                Err(_) => return,
            };
            let success = match result {
                Ok(public) if live => {
                    s.view["phase"] = json!("stored");
                    s.view["identityId"] = json!(public.identity_id);
                    s.view["certificateSha256"] = json!(public.certificate_sha256);
                    true
                }
                _ => {
                    s.view["phase"] = json!("unknown");
                    false
                }
            };
            s.busy = false;
            s.view["cleanupState"] = json!("closedObserved");
            s.permit.take();
            let value = s.view.clone();
            drop(s);
            shared.done.send_replace(true);
            let result = if success {
                shared.context.publish(value, 16384)
            } else {
                Err(unknown())
            };
            guard.finished = true;
            let _ = tx.send(result);
        });
        rx.await.map_err(|_| unknown())?
    }
    pub async fn joined_json(&self) -> AppResult<String> {
        let mut done = self.shared.done.subscribe();
        let end = tokio::time::Instant::now() + Duration::from_secs(5);
        let observed = tokio::time::timeout_at(end, async {
            loop {
                if *done.borrow() {
                    return Ok::<_, AppError>(());
                }
                done.changed().await.map_err(|_| unknown())?;
            }
        })
        .await;
        let s = self.shared.state.lock().map_err(|_| unknown())?;
        let cleanup = if matches!(observed, Ok(Ok(()))) && !s.busy && !s.orphaned {
            "closedObserved"
        } else {
            "unknown"
        };
        serde_json::to_string(&json!({"version":1,"phase":s.view["phase"],"cleanupState":cleanup}))
            .map_err(|_| unknown())
    }
}
