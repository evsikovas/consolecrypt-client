//! One issued operation, one original deadline, no authority reconstructed from UI.
use super::*;
use std::sync::Mutex;
use tokio::sync::{OwnedSemaphorePermit, oneshot, watch};
enum Payload {
    Idle(OwnedSemaphorePermit),
    Offer(NodeAssociationReview),
    Claim(NodeClaimReview),
    Empty,
}
struct State {
    payload: Payload,
    view: Value,
    busy: bool,
    mutated: bool,
    orphaned: bool,
}
struct Shared {
    purpose:AssociationPurpose,
    context: Context,
    host: ObjectId,
    revision: i64,
    state: Mutex<State>,
    done: watch::Sender<bool>,
    carrier: transport::Observation,
}
pub struct NodeAssociationOperation {
    shared: Arc<Shared>,
}
impl std::fmt::Debug for NodeAssociationOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NodeAssociationOperation([redacted])")
    }
}
impl Drop for NodeAssociationOperation {
    fn drop(&mut self) {
        self.cancel()
    }
}
impl Shared {
    fn retire(&self) {
        self.context.cancel.cancel();
        self.carrier.abort();
        if let Ok(mut s) = self.state.lock() {
            if !s.busy {
                let old = std::mem::replace(&mut s.payload, Payload::Empty);
                if !matches!(s.view["phase"].as_str(), Some("associated" | "unknown")) {
                    s.view["phase"] = json!(if s.mutated {
                        "unknown"
                    } else if Instant::now() >= self.context.scope.deadline() {
                        "expired"
                    } else {
                        "cancelled"
                    });
                }
                drop(s);
                drop(old);
            }
        }
    }
    fn finish(
        &self,
        result: AppResult<Payload>,
        project: impl FnOnce(&mut Value, &Payload) -> AppResult<()>,
    ) -> AppResult<String> {
        let current =
            self.context.scope.check_current().is_ok() && !self.context.cancel.is_cancelled();
        let mut s = self.state.lock().map_err(|_| unknown())?;
        let result = result.and_then(|payload| {
            if !current {
                return Err(unknown());
            }
            project(&mut s.view, &payload)?;
            Ok(payload)
        });
        let success = result.is_ok();
        match result {
            Ok(payload) => s.payload = payload,
            Err(_) => {
                s.payload = Payload::Empty;
                s.view["maxCertificateLifetimeSeconds"] = json!(0);
                s.view["phase"] = json!(if s.mutated {
                    "unknown"
                } else if Instant::now() >= self.context.scope.deadline() {
                    "expired"
                } else if self.context.cancel.is_cancelled() {
                    "cancelled"
                } else {
                    "unknown"
                });
            }
        }
        s.busy = false;
        self.carrier.release_closed();
        s.view["cleanupState"] = json!(if s.orphaned {
            "unknown"
        } else {
            self.carrier.cleanup_state()
        });
        let value = s.view.clone();
        drop(s);
        self.done.send_replace(true);
        if success {
            self.context.publish(value, 16384)
        } else {
            Err(unknown())
        }
    }
}
/// A panic/aborted task is NOT observed join. Its owned resources still drop
/// through the original runtime/carrier, but the UI receives Unknown.
struct WorkGuard {
    shared: Arc<Shared>,
    finished: bool,
}
impl Drop for WorkGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.shared.carrier.abort();
            if let Ok(mut s) = self.shared.state.lock() {
                s.orphaned = true;
                s.busy = false;
                s.view["phase"] = json!("unknown");
                s.payload = Payload::Empty;
            }
            self.shared.done.send_replace(true);
        }
    }
}
impl NodeAssociationOperation {
    pub(super) fn new(
        context: Context,
        host: ObjectId,
        revision: i64,
        permit: OwnedSemaphorePermit,
        owner: Value,
    ) -> AppResult<Self> {
        Self::new_for(context,host,revision,permit,owner,AssociationPurpose::Tunnel)
    }
    pub(super) fn new_metrics(context:Context,host:ObjectId,revision:i64,permit:OwnedSemaphorePermit,owner:Value)->AppResult<Self> {
        Self::new_for(context,host,revision,permit,owner,AssociationPurpose::Metrics)
    }
    pub(super) fn new_exporters(context:Context,host:ObjectId,revision:i64,permit:OwnedSemaphorePermit,owner:Value)->AppResult<Self> {
        Self::new_for(context,host,revision,permit,owner,AssociationPurpose::Exporters)
    }
    fn new_for(context:Context,host:ObjectId,revision:i64,permit:OwnedSemaphorePermit,owner:Value,purpose:AssociationPurpose)->AppResult<Self> {
        context.scope.check_current()?;
        let (done, _) = watch::channel(true);
        let shared = Arc::new(Shared {
            purpose,
            state: Mutex::new(State {
                payload: Payload::Idle(permit),
                view: data::view_for(&context, host, revision, owner,purpose),
                busy: false,
                mutated: false,
                orphaned: false,
            }),
            context,
            host,
            revision,
            done,
            carrier: transport::Observation::new(),
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
        let mut s = self.shared.state.lock().map_err(|_| unknown())?;
        let maximum = match &s.payload {
            Payload::Claim(claim) => Some(data::max_lifetime(claim)?),
            _ => None,
        };
        s.view["maxCertificateLifetimeSeconds"] = json!(maximum.unwrap_or(0));
        let mut value = s.view.clone();
        value["cleanupState"] = json!(if s.orphaned {
            "unknown"
        } else {
            self.shared.carrier.cleanup_state()
        });
        drop(s);
        self.shared.context.publish(value, 16384)
    }
    fn take(&self, expected: &str, phase: &str, mutated: bool) -> AppResult<Payload> {
        self.shared.context.scope.check_current()?;
        let mut s = self.shared.state.lock().map_err(|_| unknown())?;
        if s.busy || s.view["phase"] != expected {
            return Err(unavailable());
        }
        // A replay loser returns without touching the winning task/token.
        let payload = std::mem::replace(&mut s.payload, Payload::Empty);
        s.busy = true;
        s.mutated |= mutated;
        s.view["phase"] = json!(phase);
        s.view["maxCertificateLifetimeSeconds"] = json!(0);
        self.shared.done.send_replace(false);
        Ok(payload)
    }
    pub async fn probe(&self) -> AppResult<String> {
        let Payload::Idle(permit) = self.take("idle", "probing", false)? else {
            return Err(unavailable());
        };
        let shared = self.shared.clone();
        let (tx, rx) = oneshot::channel();
        self.shared.context.runtime.spawn(async move {
            let mut guard = WorkGuard {
                shared: shared.clone(),
                finished: false,
            };
            let result = if shared.purpose==AssociationPurpose::Metrics {
                bounded_original(&shared.context.scope,
                    NodeAssociationReview::capture_metrics_original_weak(&shared.context.core,shared.context.scope.clone(),
                        shared.host,shared.revision,permit,shared.carrier.clone())).await
            }else if shared.purpose==AssociationPurpose::Exporters {
                bounded_original(&shared.context.scope,
                    NodeAssociationReview::capture_exporters_original_weak(&shared.context.core,shared.context.scope.clone(),
                        shared.host,shared.revision,permit,shared.carrier.clone())).await
            }else{
                NodeAssociationReview::capture_original_weak(&shared.context.core,shared.context.scope.clone(),
                    shared.host,shared.revision,permit,shared.carrier.clone()).await
            }.map(Payload::Offer);
            let result = shared.finish(result, |v, p| {
                let Payload::Offer(r) = p else {
                    return Err(unknown());
                };
                data::offer(v, r)
            });
            guard.finished = true;
            let _ = tx.send(result);
        });
        rx.await.map_err(|_| unknown())?
    }
    pub async fn approve_offer(&self) -> AppResult<String> {
        let Payload::Offer(review) = self.take("offerReview", "claiming", true)? else {
            return Err(unavailable());
        };
        let shared = self.shared.clone();
        let (tx, rx) = oneshot::channel();
        self.shared.context.runtime.spawn(async move {
            let mut guard = WorkGuard {
                shared: shared.clone(),
                finished: false,
            };
            let result = if shared.purpose==AssociationPurpose::Metrics {
                let until=review.until;
                bounded_original(&shared.context.scope,under_review(until,
                    review.approve_metrics_offer_once(&shared.context.core))).await.map(Payload::Claim)
            }else if shared.purpose==AssociationPurpose::Exporters {
                let until=review.until;
                bounded_original(&shared.context.scope,under_review(until,
                    review.approve_exporters_offer_once(&shared.context.core))).await.map(Payload::Claim)
            }else{
                async {
                    let app=shared.context.app()?;
                    bounded_original(&shared.context.scope,review.approve_offer_once(&app)).await.map(Payload::Claim)
                }.await
            };
            let result = shared.finish(result, |v, p| {
                let Payload::Claim(r) = p else {
                    return Err(unknown());
                };
                data::claim(v, r)
            });
            guard.finished = true;
            let _ = tx.send(result);
        });
        rx.await.map_err(|_| unknown())?
    }
    pub async fn approve_certificate(&self, lifetime_seconds: u32) -> AppResult<String> {
        // Validate choice BEFORE consuming the current review; never sign a zero
        // or extended lifetime and never recreate a consumed review.
        {
            let s = self.shared.state.lock().map_err(|_| unknown())?;
            let Payload::Claim(r) = &s.payload else {
                return Err(unavailable());
            };
            if lifetime_seconds == 0 || lifetime_seconds > data::max_lifetime(r)? {
                return Err(unavailable());
            }
        }
        let Payload::Claim(review) = self.take("claimReview", "applying", true)? else {
            return Err(unavailable());
        };
        let shared = self.shared.clone();
        let (tx, rx) = oneshot::channel();
        self.shared.context.runtime.spawn(async move {
            let mut guard = WorkGuard {
                shared: shared.clone(),
                finished: false,
            };
            let result = if shared.purpose==AssociationPurpose::Metrics {
                let until=review.original.until;
                bounded_original(&shared.context.scope,under_review(until,
                    review.approve_metrics_certificate_once(&shared.context.core,Duration::from_secs(lifetime_seconds.into())))).await
            }else if shared.purpose==AssociationPurpose::Exporters {
                let until=review.original.until;
                bounded_original(&shared.context.scope,under_review(until,
                    review.approve_exporters_certificate_once(&shared.context.core,Duration::from_secs(lifetime_seconds.into())))).await
            }else{
                async {
                    let app=shared.context.app()?;
                    bounded_original(&shared.context.scope,review.approve_certificate_once(&app,Duration::from_secs(lifetime_seconds.into()))).await
                }.await
            };
            let result = match result {
                Ok(result) => {
                    shared.finish(Ok(Payload::Empty), |v, _| data::associated(v, &result))
                }
                Err(error) => shared.finish(Err(error), |_, _| Ok(())),
            };
            guard.finished = true;
            let _ = tx.send(result);
        });
        rx.await.map_err(|_| unknown())?
    }
    pub async fn joined_json(&self) -> AppResult<String> {
        let end = Instant::now() + Duration::from_secs(5);
        let mut done = self.shared.done.subscribe();
        let observed = tokio::time::timeout_at(tokio::time::Instant::from_std(end), async {
            loop {
                if *done.borrow() {
                    break;
                }
                done.changed().await.map_err(|_| unknown())?;
            }
            self.shared.carrier.wait_closed(end).await
        })
        .await;
        // The worker wait shares ONE deadline with carrier destruction.
        let cleanup = if matches!(observed, Ok(Ok(()))) {
            let s = self.shared.state.lock().map_err(|_| unknown())?;
            if s.busy || s.orphaned {
                "unknown"
            } else {
                "closedObserved"
            }
        } else {
            "unknown"
        };
        let s = self.shared.state.lock().map_err(|_| unknown())?;
        serde_json::to_string(&json!({"version":1,"phase":s.view["phase"],"cleanupState":cleanup}))
            .map_err(|_| unknown())
    }
}
