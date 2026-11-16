//! One explicitly reviewed original-owner volatile collection session.
//! Catalog observations never replace owner signatures or the actual Actor permit.
use super::*;
use cc_storage_core::NodeAssociationPhase;
use std::sync::Mutex;
use tokio::{sync::{oneshot, watch}, task::JoinHandle};
mod manifest;
#[cfg(test)]
mod tests;
fn unavailable() -> AppError { AppError::invalid("agent_collector", "collector_unavailable") }
fn unknown() -> AppError { AppError::invalid("agent_collector", "collection_unknown") }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    version: u8, profile_id: String, notification_scope_id: String,
    association_id: String, duration_seconds: u32,
}
impl Request {
    fn validate(&self) -> AppResult<Uuid> {
        if self.version != 1 || !(30..=300).contains(&self.duration_seconds) { return Err(unavailable()); }
        let id = Uuid::parse_str(&self.association_id).map_err(|_| unavailable())?;
        if id.is_nil() || id.to_string() != self.association_id { return Err(unavailable()); }
        Ok(id)
    }
}
struct Prepared {
    peer: Option<transport::Peer>, association: Uuid, host: ObjectId, revision: i64,
    route_digest: String, certificate: Signed<Certificate>, pin: OwnerPin,
    probe: protocol::Probe, catalog: protocol::MetricsCollector, lease: Lease, freshness: Freshness, until: Instant,
    _permit: Arc<tokio::sync::OwnedSemaphorePermit>,
}
impl Prepared {
    fn collector(&self) -> AppResult<&protocol::MetricsCollector> {
        Ok(&self.catalog)
    }
    async fn capture(context: &Context, association: Uuid, control: transport::Observation) -> AppResult<Self> {
        let permit = control.retain_permit(review_permit()?)?;
        let ledger = ledger::Ledger::capture_original(context.scope.clone()).await?;
        let rows = ledger.snapshot_original().await?;
        let row = rows.records().iter().find(|row| row.public.association_id == association
            && row.phase == NodeAssociationPhase::Associated && row.revocation.is_none()
            && row.rotation.is_none()).ok_or_else(unavailable)?;
        let key = context.scope.with_current_resources(|_, _, id| {
            Ok(PublicKey::from_bytes(id.public_keys().signing))
        })?;
        let certificate = ledger::current_certificate(row, key)?;
        // Existing Tunnel/Reader certificates are not collection authority.
        if certificate.body.scopes.0.as_slice() != [Scope::MetricsCollect]
            || certificate.body.capabilities.scopes != certificate.body.scopes
            || certificate.body.key_epoch != 1
        { return Err(AppError::invalid("agent_collector", "metrics_enrollment_required")); }
        let trusted = ledger::current_pin(row, key)?;
        context.scope.bind_owner_pin(&trusted)?;
        context.validate_host(row.public.saved_host_id, row.public.saved_host_revision).await?;
        let mut peer = transport::Peer::open_observed_weak(&context.core, context.scope.clone(),
            row.public.saved_host_id, row.public.saved_host_revision, control).await?;
        if peer.route_digest != row.public.route_trust_digest { return Err(unavailable()); }
        let observed: protocol::MetricsProbe = peer.exchange_weak(&context.core,
            "probeMetrics", json!({}), None).await?;
        let (probe, catalog) = observed.checked(&context.scope, row.public.saved_host_id, false)?;
        let mut pin = trusted.clone(); pin.checkpoint = probe.owner_pin.checkpoint;
        let minimum = row.public.certificate.as_ref().ok_or_else(unavailable)?.expected_actor_revision;
        if probe.revoked || probe.node.format != 1 || probe.owner_pin != pin || pin.checkpoint < trusted.checkpoint || probe.actor_revision < minimum
            || probe.certificate.as_ref() != Some(&certificate.body)
            || probe.node.agent_id != certificate.body.agent_id || probe.node.key_epoch != certificate.body.key_epoch
            || probe.node.public_key != certificate.body.agent_key
            || probe.node.key_fingerprint != probe.node.public_key.fingerprint()
        { return Err(unavailable()); }
        context.scope.bind_owner_pin(&pin)?;
        let collector = &catalog;
        collector.check(row.public.saved_host_id)?;
        let now = context.scope.now()?;
        let remaining = context.scope.deadline().checked_duration_since(Instant::now()).ok_or_else(unavailable)?;
        let age = collector.manifest.pointer("/buffer/max_age_seconds").and_then(Value::as_u64).unwrap_or(300).min(300);
        let expiry = now.0.checked_add(remaining.as_secs().min(age)).map_err(|_| unavailable())?
            .min(certificate.body.expires_at).min(pin.expires_at);
        if context.scope.remaining_until(expiry)?.as_secs() < 15 { return Err(unavailable()); }
        let header = Header { version: 1, context: pin.context.clone(), owner_epoch: pin.epoch, checkpoint: pin.checkpoint };
        let operation = Operation::MetricsCollect { collector: collector.collector_id };
        let lease = Lease { header: header.clone(), id: LeaseId::new(&Uuid::new_v4().to_string()).map_err(|_| unavailable())?,
            agent_id: certificate.body.agent_id, key_epoch: certificate.body.key_epoch,
            certificate_hash: certificate.body.digest().map_err(|_| unavailable())?, operation,
            issued_at: now.0, expires_at: expiry };
        let freshness = Freshness { header, issued_at: now.0, expires_at: expiry };
        PreparedTunnelOwner::metrics_grant(&pin, certificate.clone(), collector.collector_id,
            lease.clone(), freshness.clone(), now).map_err(|_| unavailable())?;
        Ok(Self { route_digest: peer.route_digest.clone(), peer: Some(peer), association,
            host: row.public.saved_host_id, revision: row.public.saved_host_revision,
            certificate, pin, probe, catalog, lease, freshness,
            until: context.scope.deadline().min(Instant::now() + Duration::from_secs(60)), _permit: permit })
    }
    fn live(&self, context: &Context) -> AppResult<()> {
        context.scope.check_current()?;
        context.scope.bind_owner_pin(&self.pin)?;
        context.scope.remaining_until(self.lease.expires_at)?;
        check_live(self.certificate.body.not_before, self.certificate.body.expires_at,
            context.scope.now()?).map_err(|_| unavailable())?;
        self.collector()?.check(self.host)
    }
    async fn revalidate(&self, context: &Context) -> AppResult<()> {
        self.live(context)?;
        let ledger = ledger::Ledger::capture_original(context.scope.clone()).await?;
        let rows = ledger.snapshot_original().await?;
        let row = rows.records().iter().find(|row| row.public.association_id == self.association
            && row.phase == NodeAssociationPhase::Associated && row.revocation.is_none()
            && row.rotation.is_none()).ok_or_else(unavailable)?;
        if row.public.saved_host_id != self.host || row.public.saved_host_revision != self.revision
            || row.public.route_trust_digest != self.route_digest
            || ledger::current_certificate(row, self.pin.key)?.body != self.certificate.body
        { return Err(unavailable()); }
        context.validate_host(self.host, self.revision).await?;
        self.live(context)
    }
    async fn view(&self, context: &Context) -> AppResult<Value> {
        self.live(context)?; consent_live(self.until)?;
        let collector = self.collector()?;
        let manifest = manifest::Manifest(collector.manifest.clone());
        if manifest.validate(self.host)? != collector.manifest_sha256 { return Err(unavailable()); }
        let mut view=json!({"phase":"collectorReview","associationId":self.association,
            "hostId":self.host,"hostRevision":self.revision.to_string(),"owner":context.owner().await?,
            "node":data::node(&self.probe.node),"scope":"metrics.collect",
            "collectorId":collector.collector_id,"manifestSha256":collector.manifest_sha256,
            "manifest":manifest.review(),"reviewRemainingMs":self.until.saturating_duration_since(Instant::now()).as_millis().min(60000) as u64,
            "originalExpiresUtc":data::utc(self.lease.expires_at)?,"profile":"volatile-collection-session-v1",
            "maxMemoryBytes":536870912u64,"maxTemporaryStateBytes":33554432u64,
            "lossUnknown":true,"unacknowledgedDataCanBeLost":true,"crashReplay":false,
            "automaticRenewal":false,"strictRemoteWriteAvailable":false,"deliveryGuaranteed":false,
            "receiverPolicy":"operator-controlled-nonredirecting-loopback","arbitraryReceiversSupported":false,
            "actorAdmission":false,"cleanupState":"pending"});
        manifest.project_profile(&mut view)?;
        Ok(view)
    }
    fn close(&mut self) { drop(self.peer.take()); }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Started {
    node: protocol::NodeIdentity, actor_revision: u64, collector_id: ResourceId,
    manifest_sha256: Hash, lease_hash: Hash, expires_at: Timestamp, state: String, loss_unknown: bool,
}
struct State {
    ready: Option<Prepared>, task: Option<JoinHandle<()>>, view: Value,
    consumed: bool, busy: bool, joined: bool, orphaned: bool,
}
struct Shared {
    context: Context, control: transport::Observation, stop: CancellationToken,
    state: Mutex<State>, done: watch::Sender<bool>,
}
pub struct AgentCollectorOperation { shared: Arc<Shared> }
impl std::fmt::Debug for AgentCollectorOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("AgentCollectorOperation([redacted])") }
}
impl Drop for AgentCollectorOperation { fn drop(&mut self) { self.cancel(); } }
impl Shared {
    fn cancel(&self) {
        self.context.cancel.cancel(); self.control.abort();
        if let Ok(mut state) = self.state.lock() {
            if !state.busy && !state.consumed { state.ready.take(); state.view["phase"] = json!("cancelled"); }
        }
    }
    fn publish(&self) -> AppResult<String> {
        let state = self.state.lock().map_err(|_| unknown())?;
        self.context.publish(state.view.clone(), 16384)
    }
    fn finish(&self, phase: &str, admitted: bool, cleanup: bool) {
        if let Ok(mut state) = self.state.lock() {
            state.ready.take(); state.busy = false;
            state.view["phase"] = json!(phase);
            let observed = state.view["actorAdmission"].as_bool().unwrap_or(false);
            state.view["actorAdmission"] = json!(observed || admitted);
            state.view["lossUnknown"] = json!(true);
            state.view["cleanupState"] = json!(if cleanup && !state.orphaned { "closedObserved" } else { "unknown" });
        }
        self.done.send_replace(true);
    }
}
struct WorkGuard { shared: Arc<Shared>, finished: bool }
impl Drop for WorkGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.shared.control.abort();
            if let Ok(mut state) = self.shared.state.lock() { state.orphaned = true; }
            self.shared.finish("unknown", false, false);
        }
    }
}
impl AgentCollectorOperation {
    pub async fn details_json(&self) -> AppResult<String> {
        self.shared.context.check().await?;
        {
            let mut state = self.shared.state.lock().map_err(|_| unknown())?;
            if let Some(ready) = &state.ready {
                ready.live(&self.shared.context)?; consent_live(ready.until)?;
                let ms = ready.until.saturating_duration_since(Instant::now()).as_millis().min(60000) as u64;
                state.view["reviewRemainingMs"] = json!(ms);
            }
        }
        self.shared.publish()
    }
    pub async fn approve_once(&self) -> AppResult<String> {
        self.shared.context.check().await?;
        let ready = {
            let mut state = self.shared.state.lock().map_err(|_| unknown())?;
            if state.consumed || state.busy { return Err(unavailable()); }
            let ready = state.ready.take().ok_or_else(unavailable)?;
            ready.live(&self.shared.context)?; consent_live(ready.until)?;
            state.consumed = true; state.busy = true; state.view["phase"] = json!("starting");
            self.shared.done.send_replace(false); ready
        };
        let shared = self.shared.clone(); let (tx, rx) = oneshot::channel();
        let handle = self.shared.context.runtime.spawn(async move {
            let mut guard = WorkGuard { shared: shared.clone(), finished: false };
            run_original(shared, ready, tx).await; guard.finished = true;
        });
        self.shared.state.lock().map_err(|_| unknown())?.task = Some(handle);
        rx.await.map_err(|_| unknown())?
    }
    pub async fn stop(&self) -> AppResult<String> {
        self.shared.context.check().await?;
        {
            let state = self.shared.state.lock().map_err(|_| unknown())?;
            if !state.consumed || !state.busy || state.view["phase"] != "running" { return Err(unavailable()); }
        }
        self.shared.stop.cancel();
        self.shared.publish()
    }
    pub fn cancel(&self) { self.shared.cancel(); }
    pub async fn joined_json(&self) -> AppResult<String> {
        let mut done = self.shared.done.subscribe(); let end = Instant::now() + Duration::from_secs(5);
        let observed = tokio::time::timeout_at(tokio::time::Instant::from_std(end), async {
            while !*done.borrow() { done.changed().await.map_err(|_| unknown())?; }
            self.shared.control.wait_closed(end).await
        }).await;
        let handle = if matches!(observed, Ok(Ok(()))) {
            self.shared.state.lock().map_err(|_| unknown())?.task.take()
        } else { None };
        let joined = match handle {
            Some(mut handle) => match tokio::time::timeout_at(tokio::time::Instant::from_std(end), &mut handle).await {
                Ok(Ok(())) => { self.shared.state.lock().map_err(|_| unknown())?.joined = true; true },
                Ok(Err(_)) => false,
                Err(_) => { self.shared.state.lock().map_err(|_| unknown())?.task = Some(handle); false },
            },
            None => {
                let state = self.shared.state.lock().map_err(|_| unknown())?;
                matches!(observed, Ok(Ok(()))) && (state.joined || !state.consumed && !state.busy)
            },
        };
        let state = self.shared.state.lock().map_err(|_| unknown())?;
        serde_json::to_string(&json!({"version":1,"phase":state.view["phase"],
            "actorAdmission":state.view["actorAdmission"],"lossUnknown":true,"deliveryGuaranteed":false,
            "cleanupState":if joined && !state.orphaned && !state.busy
                && (state.view["cleanupState"] == "closedObserved" || !state.consumed) { "closedObserved" } else { "unknown" }}))
            .map_err(|_| unknown())
    }
}
async fn run_original(shared: Arc<Shared>, mut ready: Prepared, tx: oneshot::Sender<AppResult<String>>) {
    let mut tx = Some(tx); let mut admitted = false; let mut remote_cleanup = false;
    let mut stopped = false; let mut start_attempted = false;
    let watch_scope = shared.context.clone();
    let watch_association = ready.association; let watch_host = ready.host; let watch_revision = ready.revision;
    let watch_route = ready.route_digest.clone(); let watch_certificate = ready.certificate.body.clone();
    let watch_key = ready.pin.key; let watch_expiry = ready.lease.expires_at;
    let action = async {
        ready.revalidate(&shared.context).await?; consent_live(ready.until)?;
        let collector = ready.collector()?.collector_id;
        let manifest_hash = ready.collector()?.manifest_sha256;
        let prepared = PreparedTunnelOwner::metrics_grant(&ready.pin, ready.certificate.clone(), collector,
            ready.lease.clone(), ready.freshness.clone(), shared.context.scope.now()?).map_err(|_| unavailable())?;
        let signed = {
            let app = shared.context.app()?;
            sign_reviewed(&app, &shared.context.scope, ready.until, &ready.pin, prepared).await?
        }; // No Core survives the actual SSH/Actor/unit waits.
        let TunnelOwnerSigned::Grant(binding) = signed else { return Err(unknown()); };
        let lease_hash = binding.lease.body.digest().map_err(|_| unknown())?;
        start_attempted = true;
        let reply: Started = ready.peer.as_mut().ok_or_else(unknown)?.start_collector_original(&shared.context.core,
            json!({"expectedRev":ready.probe.actor_revision,"expectedPin":ready.pin,
                "binding":binding,"collectorId":collector,"manifestSha256":manifest_hash}),
            ready.lease.expires_at).await?;
        if reply.node != ready.probe.node || reply.actor_revision != ready.probe.actor_revision.checked_add(1).ok_or_else(unknown)?
            || reply.collector_id != collector || reply.manifest_sha256 != manifest_hash || reply.lease_hash != lease_hash
            || reply.expires_at != ready.lease.expires_at || reply.state != "running" || !reply.loss_unknown
        { return Err(unknown()); }
        ready.revalidate(&shared.context).await?; admitted = true;
        {
            let mut state = shared.state.lock().map_err(|_| unknown())?;
            state.view["phase"] = json!("running"); state.view["actorAdmission"] = json!(true);
            state.view["leaseHash"] = json!(lease_hash); state.view["reviewRemainingMs"] = Value::Null;
        }
        if let Some(tx) = tx.take() { let _ = tx.send(shared.publish()); }
        enum Event { Stop, Terminal(AppResult<transport::CollectorTerminalObservation>) }
        let event = {
            let terminal = ready.peer.as_mut().ok_or_else(unknown)?.observe_collector_terminal(collector, lease_hash, ready.lease.expires_at);
            tokio::select! { biased; result = terminal => Event::Terminal(result), _ = shared.stop.cancelled() => Event::Stop }
        };
        match event {
            Event::Stop => {
                ready.peer.as_mut().ok_or_else(unknown)?.stop_collector_original(&shared.context.core,
                    collector, lease_hash, ready.lease.expires_at).await?;
                let observation = ready.peer.as_mut().ok_or_else(unknown)?.observe_collector_terminal(collector, lease_hash,
                    ready.lease.expires_at).await?;
                stopped = observation.stopped; remote_cleanup = observation.cleanup_observed;
            },
            Event::Terminal(result) => { let observation = result?; stopped = observation.stopped; remote_cleanup = observation.cleanup_observed; },
        }
        Ok::<_, AppError>(())
    };
    // Original local ledger/Host changes abort this SAME operation; no grant,
    // source, session, certificate, credential or endpoint is refreshed.
    let watch = async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            watch_scope.scope.check_current()?;
            watch_scope.scope.remaining_until(watch_expiry)?;
            let ledger = ledger::Ledger::capture_original(watch_scope.scope.clone()).await?;
            let rows = ledger.snapshot_original().await?;
            let row = rows.records().iter().find(|row| row.public.association_id == watch_association
                && row.phase == NodeAssociationPhase::Associated && row.revocation.is_none()
                && row.rotation.is_none()).ok_or_else(unavailable)?;
            if row.public.saved_host_id != watch_host || row.public.saved_host_revision != watch_revision
                || row.public.route_trust_digest != watch_route
                || ledger::current_certificate(row, watch_key)?.body != watch_certificate
            { return Err(unavailable()); }
            watch_scope.validate_host(watch_host, watch_revision).await?;
        }
        #[allow(unreachable_code)] Ok::<_, AppError>(())
    };
    let result = tokio::select! { biased; _ = shared.context.scope.cancelled() => Err(unknown()),
        result = watch => result, result = bounded_original(&shared.context.scope, action) => result };
    ready.close();
    let local_closed = shared.control.wait_closed(Instant::now() + Duration::from_secs(3)).await.is_ok();
    let closed = local_closed && (!start_attempted || remote_cleanup);
    shared.finish(if result.is_ok() && stopped { "stopped" } else { "unknown" }, admitted, closed);
    if let Some(tx) = tx.take() { let _ = tx.send(Err(unknown())); }
}
impl AppCore {
    pub async fn agent_collector_prepare_json(self, raw: &str) -> AppResult<AgentCollectorOperation> {
        let request: Request = parse(raw)?;
        let association = request.validate()?;
        let context = Context::capture_collection(&self, Frame { version: request.version,
            profile_id: request.profile_id, notification_scope_id: request.notification_scope_id },
            Duration::from_secs(request.duration_seconds as u64)).await?;
        drop(self);
        let control = transport::Observation::new();
        let ready = bounded_original(&context.scope, Prepared::capture(&context, association, control.clone())).await?;
        let until = ready.until; let view = ready.view(&context).await?; let (done, _) = watch::channel(true);
        let shared = Arc::new(Shared { context, control, stop: CancellationToken::new(),
            state: Mutex::new(State { ready: Some(ready), task: None, view, consumed: false,
                busy: false, joined: false, orphaned: false }), done });
        let weak = Arc::downgrade(&shared); let scope = shared.context.scope.clone();
        shared.context.runtime.spawn(async move {
            tokio::select! { _ = scope.cancelled() => {}, _ = tokio::time::sleep_until(tokio::time::Instant::from_std(until)) => {} }
            if let Some(shared) = weak.upgrade() {
                let cancel = shared.state.lock().map(|state| !state.consumed && !state.busy).unwrap_or(true);
                if cancel { shared.cancel(); }
            }
        });
        shared.context.check().await?;
        Ok(AgentCollectorOperation { shared })
    }
}
