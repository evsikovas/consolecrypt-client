//! Shared actual request dispatcher. Transport IDs are never job authority.
use super::*;
use std::sync::{
    RwLock,
    atomic::{AtomicBool, Ordering},
};
#[derive(Clone)]
struct StatusLane {
    peer: Arc<crate::HelperPeer>,
    original: Arc<cc_crypto_core::ServiceOwnerStatusVerifier>,
    stop: tokio_util::sync::CancellationToken,
    policy: Arc<Value>,
}
type PendingGate =
    std::pin::Pin<Box<dyn std::future::Future<Output = tokio::sync::OwnedMutexGuard<()>> + Send>>;
enum AckGate {
    Held(tokio::sync::OwnedMutexGuard<()>),
    Queued(PendingGate),
}
impl AckGate {
    async fn queue(cleanup: Arc<tokio::sync::Mutex<()>>) -> Self {
        // First poll registers custody BEFORE reactive cancellation, without waiting.
        use std::task::Poll;
        let mut waiting: PendingGate = Box::pin(tokio::task::unconstrained(cleanup.lock_owned()));
        // One bounded poll only. Unconstrained prevents an exhausted Tokio coop
        // budget returning Pending BEFORE the actual FIFO queue registration.
        let mut held = None;
        std::future::poll_fn(|cx| {
            if let Poll::Ready(guard) = waiting.as_mut().poll(cx) {
                held = Some(guard);
            }
            Poll::Ready(())
        })
        .await;
        match held {
            Some(guard) => Self::Held(guard),
            None => Self::Queued(waiting),
        }
    }
    async fn hold(self) -> tokio::sync::OwnedMutexGuard<()> {
        match self {
            Self::Held(guard) => guard,
            Self::Queued(waiting) => waiting.await,
        }
    }
}
pub(super) struct StatusDelivery {
    lane: StatusLane,
    delivery: crate::service_owner_status::VerifiedDelivery,
    gate: AckGate,
}
#[derive(Clone)]
struct OfferLane {
    generation: u64,
    public: DevicePublicKeys,
    namespace: Option<crate::ServiceLocalNamespace>,
    state: crate::ServiceState,
    control: Option<Arc<crate::HelperPeer>>,
    origin: Option<ServiceExecutionOrigin>,
}
struct State {
    bootstrapped: bool,
    unlock_observed: bool,
    enrollment_attempted: bool,
    pending_schedule: Option<Uuid>,
    control: Option<Arc<crate::HelperPeer>>,
    stop: Option<tokio_util::sync::CancellationToken>,
    original_origin: Option<ServiceExecutionOrigin>,
    background: Option<tokio::task::JoinHandle<()>>,
}
pub(super) struct Dispatcher {
    launch: HelperLaunch,
    offer: RwLock<OfferLane>,
    owner: Arc<tokio::sync::Mutex<ServiceNode>>,
    state: tokio::sync::Mutex<State>,
    cleanup: Arc<tokio::sync::Mutex<()>>,
    status: RwLock<Option<StatusLane>>,
    closed: AtomicBool,
    pub(super) finished: tokio_util::sync::CancellationToken,
}
impl Dispatcher {
    pub(super) async fn new(launch: HelperLaunch, mut node: ServiceNode) -> Result<Arc<Self>> {
        let public = *node.public_keys();
        let namespace = node.local_namespace()?;
        let restored = match node.restore(&launch).await {
            Ok(mode) => mode,
            Err(error) => {
                node.shutdown().await;
                return Err(error);
            }
        };
        match node.local_namespace() {
            Ok(current) if current == namespace => {}
            _ => {
                node.shutdown().await;
                return Err(denied());
            }
        }
        let owner = Arc::new(tokio::sync::Mutex::new(node));
        let result = Arc::new(Self {
            launch,
            offer: RwLock::new(OfferLane {
                generation: 0,
                public,
                namespace,
                state: if restored.is_some() {
                    crate::ServiceState::Restored
                } else {
                    crate::ServiceState::Unbound
                },
                control: None,
                origin: None,
            }),
            owner,
            state: tokio::sync::Mutex::new(State {
                bootstrapped: restored.is_some(),
                unlock_observed: false,
                enrollment_attempted: restored.is_some(),
                pending_schedule: None,
                control: None,
                stop: None,
                original_origin: None,
                background: None,
            }),
            cleanup: Arc::new(tokio::sync::Mutex::new(())),
            status: RwLock::new(None),
            closed: AtomicBool::new(false),
            finished: tokio_util::sync::CancellationToken::new(),
        });
        if let Some(mode) = restored {
            let mut state = result.state.lock().await;
            {
                let current = result.owner.lock().await;
                state.control = Some(current.control_peer()?);
                state.stop = Some(current.cancellation()?);
                state.original_origin = Some(current.original_origin()?);
            }
            {
                let mut offer = result.offer.write().map_err(|_| denied())?;
                offer.generation = offer.generation.checked_add(1).ok_or_else(denied)?;
                offer.control = state.control.clone();
                offer.origin = state.original_origin.clone();
            }
            result.capture_status(&state).await?;
            eprintln!(
                "{}",
                json!({"service_restoration":"originalOnlyRecovery","previousClientAcknowledgement":"unknown","durability":"observedCompleteRecord"})
            );
            state.background = Some(tokio::spawn(activate_background(
                result.owner.clone(),
                result.cleanup.clone(),
                state.stop.as_ref().ok_or_else(denied)?.clone(),
                mode,
                result.finished.clone(),
            )));
        }
        Ok(result)
    }
    async fn capture_status(&self, state: &State) -> Result<()> {
        let (original, policy) = {
            let held = self.owner.lock().await;
            (
                Arc::new(held.owner_status_verifier()?),
                Arc::new(held.owner_policy()?),
            )
        };
        let lane = StatusLane {
            peer: state.control.as_ref().ok_or_else(denied)?.clone(),
            original,
            policy,
            stop: state.stop.as_ref().ok_or_else(denied)?.clone(),
        };
        *self.status.write().map_err(|_| denied())? = Some(lane);
        Ok(())
    }
    /// Synchronous original signature check/cancel, before ANY long actor/cleanup wait.
    pub(super) async fn admit_status(&self, body: Value) -> Result<StatusDelivery> {
        if self.closed.load(Ordering::Acquire) {
            return Err(denied());
        }
        let lane = self
            .status
            .read()
            .map_err(|_| denied())?
            .clone()
            .ok_or_else(denied)?;
        let gate = AckGate::queue(self.cleanup.clone()).await;
        let delivery =
            crate::service_owner_status::verify_and_cancel(body, &lane.original, &lane.stop)?;
        Ok(StatusDelivery {
            lane,
            delivery,
            gate,
        })
    }

    pub(super) async fn persist_status(&self, prepared: StatusDelivery) -> Result<Value> {
        let _ack = prepared.gate.hold().await;
        crate::service_owner_status::persist(&prepared.lane.peer, prepared.delivery).await
    }
    pub(super) fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
    pub(super) async fn dispatch(&self, request: &Request) -> Result<Value> {
        if self.closed() {
            return Err(denied());
        }
        if request.operation == "service_owner_status" {
            return self
                .persist_status(self.admit_status(request.body.clone()).await?)
                .await;
        }
        if request.operation == "service_owner_policy" {
            if request.body != json!({}) {
                return Err(denied());
            }
            let lane = self
                .status
                .read()
                .map_err(|_| denied())?
                .clone()
                .ok_or_else(denied)?;
            return Ok(json!({"version":1,"delegation":lane.policy.as_ref()}));
        }
        if request.operation == "service_close" {
            if request.body != json!({}) {
                return Err(denied());
            }
            if let Some(lane) = self.status.read().map_err(|_| denied())?.as_ref() {
                lane.stop.cancel();
            }
            self.closed.store(true, Ordering::Release);
            self.finished.cancel();
            return Ok(json!({"cancellation_requested":true}));
        }
        if request.operation == "probe" || request.operation == "service_offer" {
            if request.body != json!({}) {
                return Err(denied());
            }
            let snapshot = self.offer.read().map_err(|_| denied())?.clone();
            let mut actual = if let Some(peer) = snapshot.control {
                peer.request("probe", json!({}), Duration::from_secs(30))
                    .await?
            } else {
                probe(&self.launch).await?
            };
            if self.closed()
                || self.finished.is_cancelled()
                || self.offer.read().map_err(|_| denied())?.generation != snapshot.generation
            {
                return Err(denied());
            }
            if request.operation == "probe" {
                actual
                    .get_mut("capabilities")
                    .and_then(Value::as_object_mut)
                    .ok_or_else(denied)?
                    .insert("service_local_executor_v1".into(), Value::Bool(true));
                if let Some(origin) = snapshot.origin {
                    actual.as_object_mut().ok_or_else(denied)?.insert(
                        "service_origin".into(),
                        serde_json::to_value(origin).map_err(|_| denied())?,
                    );
                }
                if snapshot.state == crate::ServiceState::Restored {
                    actual
                        .as_object_mut()
                        .ok_or_else(denied)?
                        .insert("service_restoration".into(), json!("originalOnlyRecovery"));
                }
                return Ok(actual);
            }
            let mut value = json!({"signing":hex::encode(snapshot.public.signing),"recipient":hex::encode(snapshot.public.encryption),"probe":actual,"serviceState":snapshot.state});
            if let Some(namespace) = snapshot.namespace {
                value.as_object_mut().ok_or_else(denied)?.insert(
                    "persistentNamespace".into(),
                    serde_json::to_value(namespace).map_err(|_| denied())?,
                );
            }
            return Ok(value);
        }
        let mut state = self.state.lock().await;
        if self.closed() {
            return Err(denied());
        }
        match request.operation.as_str() {
            "vault_state" if !state.bootstrapped && !state.unlock_observed => {
                let observed = observe_initial_unlock(request.body.clone())?;
                state.unlock_observed = true;
                Ok(observed)
            }
            "service_bootstrap" if !state.bootstrapped => {
                let b: Bootstrap =
                    serde_json::from_value(request.body.clone()).map_err(|_| denied())?;
                let schedule = b
                    .schedule
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(denied)?;
                let schedule_id = Uuid::parse_str(schedule).map_err(|_| denied())?;
                if schedule_id.is_nil() || schedule_id.to_string() != schedule {
                    return Err(denied());
                }
                let key = |text: &str| -> Result<[u8; 32]> {
                    hex::decode(text)
                        .map_err(|_| denied())?
                        .try_into()
                        .map_err(|_| denied())
                };
                let public = DevicePublicKeys {
                    signing: key(&b.owner.signing)?,
                    encryption: key(&b.owner.encryption)?,
                };
                state.original_origin = Some(b.origin.clone());
                let started = self
                    .owner
                    .lock()
                    .await
                    .start(
                        &self.launch,
                        b.origin,
                        b.owner.owner_id,
                        public,
                        b.owner.tenant,
                        b.owner.owner_epoch,
                        b.owner.not_after,
                        b.schedule,
                    )
                    .await;
                if let Err(error) = started {
                    // No enrollment has been attempted. A partial bootstrap is
                    // never presented as an adoptable pending/unbound service.
                    self.closed.store(true, Ordering::Release);
                    self.finished.cancel();
                    return Err(error);
                }
                state.pending_schedule = Some(schedule_id);
                state.control = Some(self.owner.lock().await.control_peer()?);
                state.bootstrapped = true;
                {
                    let mut offer = self.offer.write().map_err(|_| denied())?;
                    offer.generation = offer.generation.checked_add(1).ok_or_else(denied)?;
                    offer.state = crate::ServiceState::Pending;
                    offer.control = state.control.clone();
                    offer.origin = state.original_origin.clone();
                }
                Ok(json!({"bootstrapped":true}))
            }
            "service_enroll" if state.bootstrapped && state.background.is_none() => {
                let b: Enrollment =
                    serde_json::from_value(request.body.clone()).map_err(|_| denied())?;
                let modes = b
                    .template
                    .get("delegation")
                    .and_then(|v| v.get("body"))
                    .and_then(|v| v.get("modes"))
                    .and_then(Value::as_array)
                    .ok_or_else(denied)?;
                if modes.len() != 1 {
                    return Err(denied());
                }
                let mode = match modes[0].as_str() {
                    Some("check") => JobMode::Check,
                    Some("apply") => JobMode::Apply,
                    _ => return Err(denied()),
                };
                state.enrollment_attempted = true; // Sticky BEFORE any verify/send/commit uncertainty.
                let value = self
                    .owner
                    .lock()
                    .await
                    .enroll(b.template, b.plans, b.known_hosts, b.capsules)
                    .await?;
                {
                    let mut offer = self.offer.write().map_err(|_| denied())?;
                    offer.generation = offer.generation.checked_add(1).ok_or_else(denied)?;
                    offer.state = crate::ServiceState::Enrolled;
                }
                state.stop = Some(self.owner.lock().await.cancellation()?);
                self.capture_status(&state).await?;
                state.background = Some(tokio::spawn(activate_background(
                    self.owner.clone(),
                    self.cleanup.clone(),
                    state.stop.as_ref().ok_or_else(denied)?.clone(),
                    mode,
                    self.finished.clone(),
                )));
                Ok(value)
            }
            "service_cancel_pending" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Cancel {
                    schedule_id: String,
                }
                let input: Cancel =
                    serde_json::from_value(request.body.clone()).map_err(|_| denied())?;
                let id = Uuid::parse_str(&input.schedule_id).map_err(|_| denied())?;
                if id.is_nil() || id.to_string() != input.schedule_id {
                    return Err(denied());
                }
                self.cancel_pending_held(&state, id)?;
                Ok(json!({"cancellation_requested":true}))
            }
            "service_job_status" | "job_get" | "job_events" | "job_cancel"
                if state.background.is_some() =>
            {
                state
                    .control
                    .as_ref()
                    .ok_or_else(denied)?
                    .request(
                        &request.operation,
                        request.body.clone(),
                        Duration::from_secs(30),
                    )
                    .await
            }
            _ if state.bootstrapped && state.background.is_none() => {
                self.owner
                    .lock()
                    .await
                    .request(
                        &request.operation,
                        request.body.clone(),
                        Duration::from_secs(120),
                    )
                    .await
            }
            _ => Err(denied()),
        }
    }
    // The RPC and its original connection's EOF backstop share one atomic gate.
    // Never roll back attempted enrollment: persistence/send may already have won.
    fn cancel_pending_held(&self, state: &State, id: Uuid) -> Result<()> {
        if self.closed()
            || state.pending_schedule != Some(id)
            || !state.bootstrapped
            || state.enrollment_attempted
            || state.background.is_some()
        {
            return Err(denied());
        }
        self.closed.store(true, Ordering::Release);
        self.finished.cancel();
        Ok(())
    }
    pub(super) async fn cancel_connection_pending(&self, id: Uuid) {
        let state = self.state.lock().await;
        // Duplicate expiry/EOF cleanup and an enrollment that won the same lock
        // are deliberately harmless. No state reset, receipt or grant is created.
        let _ = self.cancel_pending_held(&state, id);
    }
    pub(super) async fn finish(&self, cancel: bool) -> Result<()> {
        if cancel {
            if let Some(lane) = self.status.read().map_err(|_| denied())?.as_ref() {
                lane.stop.cancel();
            }
        }
        let task = self.state.lock().await.background.take();
        if let Some(task) = task {
            task.await.map_err(|_| denied())?;
        }
        self.owner.lock().await.shutdown().await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    #[cfg(target_os = "linux")]
    async fn empty_dispatcher() -> Arc<Dispatcher> {
        // Lifecycle metadata unit only: no helper bootstrap, policy or witness.
        Dispatcher::new(
            HelperLaunch::Native {
                python: "/unopened-python".into(),
                module_dir: "/unopened-modules".into(),
                root: "/unopened-runtime".into(),
            },
            ServiceNode::new().unwrap(),
        )
        .await
        .unwrap()
    }
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn invalid_bootstrap_cannot_publish_pending_and_start_error_closes_original() {
        let dispatch = empty_dispatcher().await;
        let mut request = Request {
            version: 1,
            request_id: Uuid::new_v4(),
            operation: "service_bootstrap".into(),
            body: json!({
                "origin":{"version":1,"service_id":Uuid::new_v4(),"runner_revision":1,
                    "runner_configuration_digest":"1".repeat(64),"signer_key_sha256":"2".repeat(64),
                    "recipient_key_sha256":"3".repeat(64),"environment_digest":"4".repeat(64),
                    "helper_digest":"5".repeat(64),"known_hosts_digest":"6".repeat(64),
                    "backend":"linuxNativeSsh","network":"serviceLocal"},
                "owner":{"owner_id":Uuid::new_v4(),"owner_epoch":1,"not_after":1,
                    "tenant":cc_crypto_core::ServicePolicyTenant {kind:cc_crypto_core::ServiceTenantKind::LocalProfile,id:Uuid::new_v4().to_string()},
                    "signing":"invalid","encryption":"1".repeat(64)},
                "schedule":{"id":Uuid::new_v4()}
            }),
        };
        assert!(dispatch.dispatch(&request).await.is_err());
        assert!(!dispatch.closed());
        assert!(matches!(
            dispatch.offer.read().unwrap().state,
            crate::ServiceState::Unbound
        ));
        assert!(dispatch.state.lock().await.pending_schedule.is_none());
        request.body["owner"]["signing"] = json!("1".repeat(64));
        // Now the actual Node.start is reached, and rejects mismatched original
        // identity hashes before opening any helper. This closes, never resets.
        assert!(dispatch.dispatch(&request).await.is_err());
        assert!(dispatch.closed() && dispatch.finished.is_cancelled());
        assert!(dispatch.state.lock().await.pending_schedule.is_none());
        assert!(!matches!(
            dispatch.offer.read().unwrap().state,
            crate::ServiceState::Pending
        ));
        dispatch.finish(false).await.unwrap();
    }
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn original_pending_eof_gate_cancels_only_matching_unattempted_metadata() {
        let dispatch = empty_dispatcher().await;
        let schedule = Uuid::new_v4();
        {
            let mut state = dispatch.state.lock().await;
            state.bootstrapped = true;
            state.pending_schedule = Some(schedule);
        }
        dispatch.cancel_connection_pending(Uuid::new_v4()).await;
        assert!(!dispatch.closed() && !dispatch.finished.is_cancelled());
        dispatch.cancel_connection_pending(schedule).await;
        assert!(dispatch.closed() && dispatch.finished.is_cancelled());
        dispatch.cancel_connection_pending(schedule).await; // Duplicate cleanup is inert.
        assert!(dispatch.closed());
        dispatch.finish(false).await.unwrap();
    }
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn queued_eof_cleanup_cannot_overtake_attempted_enrollment_or_restored_state() {
        use std::task::Poll;
        let dispatch = empty_dispatcher().await;
        let schedule = Uuid::new_v4();
        let mut state = dispatch.state.lock().await;
        state.bootstrapped = true;
        state.pending_schedule = Some(schedule);
        let cleanup = dispatch.cancel_connection_pending(schedule);
        tokio::pin!(cleanup);
        std::future::poll_fn(|cx| {
            assert!(matches!(cleanup.as_mut().poll(cx), Poll::Pending));
            Poll::Ready(())
        })
        .await;
        // Same gate as real enroll sets this BEFORE any asynchronous send/commit.
        state.enrollment_attempted = true;
        drop(state);
        cleanup.await;
        assert!(!dispatch.closed() && !dispatch.finished.is_cancelled());
        {
            let mut state = dispatch.state.lock().await;
            state.pending_schedule = None; // Restored state has no connection owner.
        }
        dispatch.cancel_connection_pending(schedule).await;
        assert!(!dispatch.closed() && !dispatch.finished.is_cancelled());
        dispatch.finish(false).await.unwrap();
    }
    #[tokio::test]
    async fn queued_ack_custody_is_registered_before_later_retirement_without_waiting() {
        use std::task::Poll;
        let cleanup = Arc::new(tokio::sync::Mutex::new(()));
        let original = cleanup.clone().lock_owned().await;
        let delivery = AckGate::queue(cleanup.clone()).await;
        let mut retire: PendingGate = Box::pin(cleanup.lock_owned());
        std::future::poll_fn(|cx| {
            assert!(matches!(retire.as_mut().poll(cx), Poll::Pending));
            Poll::Ready(())
        })
        .await;
        drop(original);
        let actual_ack = delivery.hold().await;
        std::future::poll_fn(|cx| {
            assert!(matches!(retire.as_mut().poll(cx), Poll::Pending));
            Poll::Ready(())
        })
        .await;
        drop(actual_ack);
        drop(retire.await);
    }
}
