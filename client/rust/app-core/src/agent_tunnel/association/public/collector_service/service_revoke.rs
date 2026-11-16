//! Explicit targeted revoke of the original saved service. Caller cancellation
//! retires ONLY this review/carrier; remote service stop is never inferred.
use super::management::{verify_observed, Observed, Request, Saved};
use super::*;
use cc_agent_node_wire::{CollectorServiceOwnerSigned, CollectorServiceRevocation};
use cc_storage_core::NodeCollectorServiceRevocation;
use std::sync::Mutex;
use tokio::{
    sync::{oneshot, watch},
    task::JoinHandle,
};

struct State {
    peer: Option<transport::Peer>,
    phase: &'static str,
    consumed: bool,
    busy: bool,
    local_blocked: bool,
    remote_revoked: bool,
    task: Option<JoinHandle<()>>,
    joined: bool,
    orphaned: bool,
}
struct Shared {
    context: Context,
    saved: Saved,
    probe: protocol::Probe,
    control: transport::Observation,
    _permit: Arc<tokio::sync::OwnedSemaphorePermit>,
    state: Mutex<State>,
    done: watch::Sender<bool>,
}
pub struct AgentCollectorServiceRevocationReview {
    shared: Arc<Shared>,
}
impl std::fmt::Debug for AgentCollectorServiceRevocationReview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AgentCollectorServiceRevocationReview([redacted])")
    }
}
impl Drop for AgentCollectorServiceRevocationReview {
    fn drop(&mut self) {
        self.cancel();
    }
}
impl Shared {
    fn cancel(&self) {
        self.context.cancel.cancel();
        self.control.abort();
        if let Ok(mut s) = self.state.lock() {
            if !s.busy && !s.consumed {
                s.peer.take();
                s.phase = "cancelled";
            }
        }
    }
    async fn publish(&self) -> AppResult<String> {
        let owner = self.context.owner().await?;
        let s = self.state.lock().map_err(|_| unknown())?;
        let p = &self.saved.delegation.body;
        self.context.publish(json!({"kind":"finiteCollectorServiceRevocation","phase":s.phase,
            "associationId":self.saved.row.public.association_id,"serviceId":p.service_id,
            "serviceHash":p.digest().map_err(|_|unavailable())?,"hostId":self.saved.row.public.saved_host_id,
            "hostRevision":self.saved.row.public.saved_host_revision.to_string(),"owner":owner,
            "node":data::node(&self.probe.node),"remainingMs":0,"localBlocked":s.local_blocked,
            "remoteRevoked":s.remote_revoked,"runtimeState":"notObserved","lossUnknown":true,
            "deliveryGuaranteed":false,"cleanupState":self.control.cleanup_state()}),16384)
    }
    fn finish(&self, remote: bool) {
        if let Ok(mut s) = self.state.lock() {
            s.peer.take();
            s.busy = false;
            s.remote_revoked = remote;
            s.phase = if remote { "revoked" } else { "unknown" };
        }
        self.done.send_replace(true);
    }
}
struct WorkGuard {
    shared: Arc<Shared>,
    finished: bool,
}
impl Drop for WorkGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.shared.control.abort();
            if let Ok(mut s) = self.shared.state.lock() {
                s.orphaned = true;
            }
            self.shared.finish(false);
        }
    }
}
impl AgentCollectorServiceRevocationReview {
    pub async fn details_json(&self) -> AppResult<String> {
        bounded_original(&self.shared.context.scope, self.shared.publish()).await
    }
    pub fn cancel(&self) {
        self.shared.cancel();
    }
    pub async fn confirm_revocation(&self) -> AppResult<String> {
        bounded_original(&self.shared.context.scope, self.shared.context.check()).await?;
        let peer = {
            let mut s = self.shared.state.lock().map_err(|_| unknown())?;
            if s.consumed || s.busy {
                return Err(unavailable());
            }
            consent_live(self.shared.context.scope.deadline())?;
            let peer = s.peer.take().ok_or_else(unavailable)?;
            s.consumed = true;
            s.busy = true;
            s.phase = "revoking";
            self.shared.done.send_replace(false);
            peer
        };
        let shared = self.shared.clone();
        let (tx, rx) = oneshot::channel();
        let task = self.shared.context.runtime.spawn(async move {
            let mut guard = WorkGuard {
                shared: shared.clone(),
                finished: false,
            };
            revoke(shared, peer, tx).await;
            guard.finished = true;
        });
        self.shared.state.lock().map_err(|_| unknown())?.task = Some(task);
        rx.await.map_err(|_| unknown())?
    }
    pub async fn joined_json(&self) -> AppResult<String> {
        let end = Instant::now() + Duration::from_secs(5);
        let mut done = self.shared.done.subscribe();
        let result = tokio::time::timeout_at(tokio::time::Instant::from_std(end), async {
            while !*done.borrow() {
                done.changed().await.map_err(|_| unknown())?;
            }
            self.shared.control.wait_closed(end).await
        })
        .await;
        let handle = if matches!(result, Ok(Ok(()))) {
            self.shared.state.lock().map_err(|_| unknown())?.task.take()
        } else {
            None
        };
        if let Some(mut handle) = handle {
            match tokio::time::timeout_at(tokio::time::Instant::from_std(end), &mut handle).await {
                Ok(Ok(())) => self.shared.state.lock().map_err(|_| unknown())?.joined = true,
                Ok(Err(_)) => self.shared.state.lock().map_err(|_| unknown())?.orphaned = true,
                Err(_) => self.shared.state.lock().map_err(|_| unknown())?.task = Some(handle),
            }
        }
        let s = self.shared.state.lock().map_err(|_| unknown())?;
        let joined =
            matches!(result, Ok(Ok(()))) && !s.busy && !s.orphaned && (s.joined || !s.consumed);
        serde_json::to_string(
            &json!({"version":1,"phase":s.phase,"localBlocked":s.local_blocked,
            "remoteRevoked":s.remote_revoked,"runtimeState":"notObserved","lossUnknown":true,
            "deliveryGuaranteed":false,"cleanupState":if joined{"closedObserved"}else{"unknown"}}),
        )
        .map_err(|_| unknown())
    }
}
async fn revoke(
    shared: Arc<Shared>,
    mut peer: transport::Peer,
    tx: oneshot::Sender<AppResult<String>>,
) {
    let work = async {
        let context = &shared.context;
        let saved = &shared.saved;
        let p = &saved.delegation.body;
        saved.current(context).await?;
        let body = CollectorServiceRevocation {
            header: Header {
                version: 1,
                context: shared.probe.owner_pin.context.clone(),
                owner_epoch: shared.probe.owner_pin.epoch,
                checkpoint: shared.probe.owner_pin.checkpoint,
            },
            service_id: p.service_id,
            delegation_hash: p.digest().map_err(|_| unavailable())?,
            policy_revision: p.policy_revision,
            revocation_epoch: p.revocation_epoch.checked_add(1).ok_or_else(unavailable)?,
            issued_at: context.scope.now()?.0,
        };
        let prepared = PreparedCollectorServiceOwner::revocation(
            &shared.probe.owner_pin,
            &saved.delegation,
            body,
            context.scope.now()?,
        )
        .map_err(|_| unavailable())?;
        let signed = {
            let app = context.app()?;
            under_review(
                context.scope.deadline(),
                context.scope.with_loaded(&app, |id| {
                    consent_live(context.scope.deadline())?;
                    context.scope.bind_owner_pin(&shared.probe.owner_pin)?;
                    let signed = id
                        .sign_collector_service_owner_v1(prepared, context.scope.now()?)
                        .map_err(|_| unknown())?;
                    consent_live(context.scope.deadline())?;
                    context.scope.bind_owner_pin(&shared.probe.owner_pin)?;
                    Ok(signed)
                }),
            )
            .await?
        };
        let CollectorServiceOwnerSigned::Revocation(signed) = signed else {
            return Err(unknown());
        };
        saved.current(context).await?;
        let mut ledger = ledger::Ledger::capture_original(context.scope.clone()).await?;
        ledger.cap_review(context.scope.deadline())?;
        let intent = NodeCollectorServiceRevocation {
            signed_revocation_json: serde_json::to_vec(&signed).map_err(|_| unknown())?,
            expected_actor_revision: shared
                .probe
                .actor_revision
                .checked_add(1)
                .ok_or_else(unknown)?,
            signed_receipt_json: None,
        };
        ledger
            .begin_service_revocation_original(
                saved.row.public.association_id,
                saved.service.clone(),
                intent.clone(),
            )
            .await?;
        shared.state.lock().map_err(|_| unknown())?.local_blocked = true;
        let mut expected = saved.service.clone();
        expected.revocation = Some(intent.clone());
        saved.current_with_service(context, &expected).await?;
        let response:Observed=peer.exchange_weak(&context.core,"revokeCollectorService",
            json!({"expectedRev":shared.probe.actor_revision,"expectedPin":shared.probe.owner_pin,"revocation":signed}),None).await?;
        verify_observed(saved, &shared.probe, &response, context)?;
        if response.receipt.body.effect != CollectorServiceEffect::Revoked
            || response.receipt.body.revocation_epoch != signed.body.revocation_epoch
            || response.receipt.body.actor_revision < intent.expected_actor_revision
        {
            return Err(unknown());
        }
        saved.current_with_service(context, &expected).await?;
        ledger
            .complete_service_revocation_original(
                saved.row.public.association_id,
                intent.clone(),
                serde_json::to_vec(&response.receipt).map_err(|_| unknown())?,
            )
            .await?;
        expected
            .revocation
            .as_mut()
            .ok_or_else(unknown)?
            .signed_receipt_json =
            Some(serde_json::to_vec(&response.receipt).map_err(|_| unknown())?);
        saved.current_with_service(context, &expected).await?;
        Ok::<_, AppError>(())
    };
    let remote = bounded_original(&shared.context.scope, work).await.is_ok();
    drop(peer);
    shared.finish(remote);
    let response = bounded_original(&shared.context.scope, shared.publish()).await;
    let _ = tx.send(response);
}
impl AppCore {
    pub async fn agent_collector_service_revoke_prepare_json(
        self,
        raw: &str,
    ) -> AppResult<AgentCollectorServiceRevocationReview> {
        let request: Request = parse(raw)?;
        let id = Uuid::parse_str(&request.association_id).map_err(|_| unavailable())?;
        if id.is_nil() || id.to_string() != request.association_id {
            return Err(unavailable());
        }
        let context = Context::capture(
            &self,
            Frame {
                version: request.version,
                profile_id: request.profile_id,
                notification_scope_id: request.notification_scope_id,
            },
        )
        .await?;
        drop(self);
        let control = transport::Observation::new();
        let permit = control.retain_permit(review_permit()?)?;
        let (saved, peer, probe) = bounded_original(&context.scope, async {
            let saved = Saved::capture(&context, id).await?;
            if saved.service.revocation.is_some() {
                return Err(unavailable());
            }
            let (peer, probe) = saved.probe(&context, control.clone()).await?;
            Ok((saved, peer, probe))
        })
        .await?;
        let (done, _) = watch::channel(true);
        let shared = Arc::new(Shared {
            context,
            saved,
            probe,
            control,
            _permit: permit,
            done,
            state: Mutex::new(State {
                peer: Some(peer),
                phase: "serviceRevokeReview",
                consumed: false,
                busy: false,
                local_blocked: false,
                remote_revoked: false,
                task: None,
                joined: false,
                orphaned: false,
            }),
        });
        let weak = Arc::downgrade(&shared);
        let scope = shared.context.scope.clone();
        shared.context.runtime.spawn(async move{
            tokio::select!{_=scope.cancelled()=>{},_=tokio::time::sleep_until(tokio::time::Instant::from_std(scope.deadline()))=>{}}
            if let Some(shared)=weak.upgrade(){shared.cancel();}
        });
        bounded_original(&shared.context.scope, shared.publish()).await?;
        Ok(AgentCollectorServiceRevocationReview { shared })
    }
}
