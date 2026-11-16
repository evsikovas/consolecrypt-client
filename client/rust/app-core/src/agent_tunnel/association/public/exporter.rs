//! One explicit ExportersOnly grant and its original, finite owned Node child.
use super::*;
use cc_storage_core::NodeAssociationPhase;
use std::sync::Mutex;
use tokio::sync::{oneshot, watch, OwnedSemaphorePermit};
use tokio::task::JoinHandle;

fn unavailable() -> AppError { AppError::invalid("agent_exporter", "exporter_unavailable") }
fn unknown() -> AppError { AppError::invalid("agent_exporter", "exporter_unknown") }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    version: u8, profile_id: String, notification_scope_id: String,
    association_id: String, manifest_sha256: Hash, duration_seconds: u16,
}
impl Request {
    fn validate(&self) -> AppResult<Uuid> {
        let id = Uuid::parse_str(&self.association_id).map_err(|_| unavailable())?;
        if id.is_nil() || id.to_string() != self.association_id || !(30..=300).contains(&self.duration_seconds) {
            return Err(unavailable());
        }
        Ok(id)
    }
}
struct Prepared {
    peer: Option<transport::Peer>, association: Uuid, host: ObjectId, revision: i64,
    route_digest: String, certificate: Signed<Certificate>, pin: OwnerPin,
    probe: protocol::Probe, catalog: protocol::ExporterCatalog,
    lease: Lease, freshness: Freshness, until: Instant, _permit: Arc<OwnedSemaphorePermit>,
}
impl Prepared {
    async fn capture(context: &Context, association: Uuid, manifest: Option<Hash>,
        control: transport::Observation) -> AppResult<Self> {
        let permit = control.retain_permit(review_permit()?)?;
        let ledger = ledger::Ledger::capture_original(context.scope.clone()).await?;
        let rows = ledger.snapshot_original().await?;
        let row = rows.records().iter().find(|r| r.public.association_id == association
            && r.phase == NodeAssociationPhase::Associated && r.revocation.is_none()
            && r.rotation.is_none()).ok_or_else(unavailable)?;
        let key = context.scope.with_current_resources(|_, _, id|
            Ok(PublicKey::from_bytes(id.public_keys().signing)))?;
        let certificate = ledger::current_certificate(row, key)?;
        // A saved Metrics/Tunnel certificate is never upgraded by this entry.
        AssociationPurpose::Exporters.check_scopes(&certificate.body.scopes)?;
        AssociationPurpose::Exporters.check_scopes(&certificate.body.capabilities.scopes)?;
        if certificate.body.key_epoch != 1 { return Err(unavailable()); }
        let trusted = ledger::current_pin(row, key)?;
        context.scope.bind_owner_pin(&trusted)?;
        context.validate_host(row.public.saved_host_id, row.public.saved_host_revision).await?;
        let mut peer = transport::Peer::open_observed_weak(&context.core, context.scope.clone(),
            row.public.saved_host_id, row.public.saved_host_revision, control).await?;
        if peer.route_digest != row.public.route_trust_digest { return Err(unavailable()); }
        let observed: protocol::ExporterProbe = peer.exchange_weak(&context.core,
            "probeExporters", json!({}), None).await?;
        let (probe, catalog) = observed.checked(&context.scope, row.public.saved_host_id, false)?;
        let mut pin = trusted.clone(); pin.checkpoint = probe.owner_pin.checkpoint;
        let minimum = row.public.certificate.as_ref().ok_or_else(unavailable)?.expected_actor_revision;
        if manifest.is_some_and(|expected|catalog.manifest_sha256 != expected) || probe.revoked || probe.node.format != 1
            || probe.owner_pin != pin || pin.checkpoint < trusted.checkpoint || probe.actor_revision < minimum
            || probe.certificate.as_ref() != Some(&certificate.body)
            || probe.node.agent_id != certificate.body.agent_id || probe.node.key_epoch != certificate.body.key_epoch
            || probe.node.public_key != certificate.body.agent_key
            || probe.node.key_fingerprint != probe.node.public_key.fingerprint() { return Err(unavailable()); }
        context.scope.bind_owner_pin(&pin)?;
        let now = context.scope.now()?;
        let remaining = context.scope.deadline().checked_duration_since(Instant::now()).ok_or_else(unavailable)?;
        let expiry = now.0.checked_add(remaining.as_secs().min(catalog.policy.max_seconds as u64))
            .map_err(|_| unavailable())?.min(certificate.body.expires_at).min(pin.expires_at);
        if context.scope.remaining_until(expiry)?.as_secs() < 15 { return Err(unavailable()); }
        let header = Header { version: 1, context: pin.context.clone(), owner_epoch: pin.epoch, checkpoint: pin.checkpoint };
        let lease = Lease { header: header.clone(), id: LeaseId::new(&Uuid::new_v4().to_string()).map_err(|_| unavailable())?,
            agent_id: certificate.body.agent_id, key_epoch: certificate.body.key_epoch,
            certificate_hash: certificate.body.digest().map_err(|_| unavailable())?,
            operation: catalog.policy.operation().map_err(|_| unavailable())?, issued_at: now.0, expires_at: expiry };
        let freshness = Freshness { header, issued_at: now.0, expires_at: expiry };
        PreparedTunnelOwner::exporters_grant(&pin, certificate.clone(), &catalog.policy,
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
        self.catalog.check(self.host, self.probe.node.agent_id)
    }
    async fn revalidate(&self, context: &Context) -> AppResult<()> {
        self.live(context)?;
        let ledger = ledger::Ledger::capture_original(context.scope.clone()).await?;
        let rows = ledger.snapshot_original().await?;
        let row = rows.records().iter().find(|r| r.public.association_id == self.association
            && r.phase == NodeAssociationPhase::Associated && r.revocation.is_none()
            && r.rotation.is_none()).ok_or_else(unavailable)?;
        if row.public.saved_host_id != self.host || row.public.saved_host_revision != self.revision
            || row.public.route_trust_digest != self.route_digest
            || ledger::current_certificate(row, self.pin.key)?.body != self.certificate.body { return Err(unavailable()); }
        context.validate_host(self.host, self.revision).await?;
        self.live(context)
    }
    async fn view(&self, context: &Context) -> AppResult<Value> {
        self.live(context)?; consent_live(self.until)?;
        Ok(json!({"phase":"exporterReview","associationId":self.association,
            "hostId":self.host,"hostRevision":self.revision.to_string(),"owner":context.owner().await?,
            "node":data::node(&self.probe.node),"scope":"exporters.manage","purpose":"exporters",
            "manifestSha256":self.catalog.manifest_sha256,"policy":self.catalog.policy,
            "reviewRemainingMs":self.until.saturating_duration_since(Instant::now()).as_millis().min(60000) as u64,
            "originalExpiresUtc":data::utc(self.lease.expires_at)?,"leaseHash":null,
            "artifactVersion":self.catalog.policy.artifact.version,"runningVersion":null,
            "runtimeObserved":false,"actorAdmission":false,"automaticRenewal":false,
            "installationPerformed":false,"childPrivilege":"nonroot-no-capabilities",
            "cleanupState":"pending","remoteDrainObserved":false}))
    }
    fn close(&mut self) { drop(self.peer.take()); }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Bind { ip: String, port: u16 }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Started {
    format: String, phase: String, manifest_sha256: Hash, lease_hash: Hash,
    expires_at: Timestamp, agent_id: AgentId, bind: Bind, collectors: Vec<String>,
    #[serde(deserialize_with = "required_running_version")]
    running_version: Option<String>, artifact_version: String,
}
fn required_running_version<'de, D: serde::Deserializer<'de>>(d: D)
    -> std::result::Result<Option<String>, D::Error> { Option::<String>::deserialize(d) }
impl Started {
    fn check(&self, ready: &Prepared, lease: Hash) -> AppResult<()> {
        let policy = &ready.catalog.policy;
        if self.format != "cc-agent-managed-exporter-session/v1" || self.phase != "running"
            || self.manifest_sha256 != ready.catalog.manifest_sha256 || self.lease_hash != lease
            || self.expires_at != ready.lease.expires_at || self.agent_id != ready.probe.node.agent_id
            || self.bind.ip != policy.bind_ip || self.bind.port != policy.bind_port
            || self.collectors != policy.collectors || self.running_version.is_some()
            || self.artifact_version != policy.artifact.version { return Err(unknown()); }
        Ok(())
    }
}

async fn run_original(shared: Arc<Shared>, mut ready: Prepared, tx: oneshot::Sender<AppResult<String>>) {
    let mut tx = Some(tx); let mut admitted = false; let mut remote_cleanup = false;
    let mut stopped = false; let mut start_attempted = false;
    let watch_context = shared.context.clone();
    let association = ready.association; let host = ready.host; let revision = ready.revision;
    let route = ready.route_digest.clone(); let certificate = ready.certificate.body.clone();
    let key = ready.pin.key; let expiry = ready.lease.expires_at;
    let action = async {
        ready.revalidate(&shared.context).await?; consent_live(ready.until)?;
        let manifest = ready.catalog.manifest_sha256;
        let prepared = PreparedTunnelOwner::exporters_grant(&ready.pin, ready.certificate.clone(),
            &ready.catalog.policy, ready.lease.clone(), ready.freshness.clone(), shared.context.scope.now()?)
            .map_err(|_| unavailable())?;
        let signed = {
            let app = shared.context.app()?;
            sign_reviewed(&app, &shared.context.scope, ready.until, &ready.pin, prepared).await?
        }; // No Core/session/key survives the actual Node child/SSH wait.
        let TunnelOwnerSigned::Grant(binding) = signed else { return Err(unknown()); };
        let lease_hash = binding.lease.body.digest().map_err(|_| unknown())?;
        start_attempted = true;
        let reply: Started = ready.peer.as_mut().ok_or_else(unknown)?.start_exporter_original(&shared.context.core,
            json!({"expectedRev":ready.probe.actor_revision,"expectedPin":ready.pin,
                "binding":binding,"manifestSha256":manifest}), ready.lease.expires_at).await?;
        reply.check(&ready, lease_hash)?;
        // Preserve a validated original Node-start observation even if the
        // following current-frame/Host check fails. This is evidence, not authority.
        admitted = true;
        ready.revalidate(&shared.context).await?;
        {
            let mut state = shared.state.lock().map_err(|_| unknown())?;
            state.view["phase"] = json!("running"); state.view["actorAdmission"] = json!(true);
            state.view["runtimeObserved"] = json!(true); state.view["leaseHash"] = json!(lease_hash);
            state.view["reviewRemainingMs"] = Value::Null;
        }
        if let Some(tx) = tx.take() { let _ = tx.send(shared.publish()); }
        enum Event { Stop, Terminal(AppResult<transport::ExporterTerminalObservation>) }
        let event = {
            let terminal = ready.peer.as_mut().ok_or_else(unknown)?.observe_exporter_terminal(
                manifest, lease_hash, ready.probe.node.agent_id, ready.lease.expires_at);
            tokio::select! { biased; result = terminal => Event::Terminal(result), _ = shared.stop.cancelled() => Event::Stop }
        };
        let observation = match event {
            Event::Stop => {
                ready.peer.as_mut().ok_or_else(unknown)?.stop_exporter_original(&shared.context.core,
                    manifest, lease_hash, ready.lease.expires_at).await?;
                ready.peer.as_mut().ok_or_else(unknown)?.observe_exporter_terminal(
                    manifest, lease_hash, ready.probe.node.agent_id, ready.lease.expires_at).await?
            },
            Event::Terminal(result) => result?,
        };
        stopped = observation.stopped; remote_cleanup = observation.cleanup_observed;
        Ok::<_, AppError>(())
    };
    // Recheck the same original association/host/credential context throughout.
    // No replacement scope, grant, identity, catalog lookup or lease renewal.
    let watch = async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            watch_context.scope.check_current()?;
            watch_context.scope.remaining_until(expiry)?;
            let ledger = ledger::Ledger::capture_original(watch_context.scope.clone()).await?;
            let rows = ledger.snapshot_original().await?;
            let row = rows.records().iter().find(|r| r.public.association_id == association
                && r.phase == NodeAssociationPhase::Associated && r.revocation.is_none()
                && r.rotation.is_none()).ok_or_else(unavailable)?;
            if row.public.saved_host_id != host || row.public.saved_host_revision != revision
                || row.public.route_trust_digest != route
                || ledger::current_certificate(row, key)?.body != certificate { return Err(unavailable()); }
            watch_context.validate_host(host, revision).await?;
        }
        #[allow(unreachable_code)] Ok::<_, AppError>(())
    };
    let result = tokio::select! { biased; _ = shared.context.scope.cancelled() => Err(unknown()),
        result = watch => result, result = bounded_original(&shared.context.scope, action) => result };
    ready.close();
    let local_closed = shared.control.wait_closed(Instant::now() + Duration::from_secs(3)).await.is_ok();
    shared.finish(if result.is_ok() && stopped { "stopped" } else { "unknown" }, admitted,
        local_closed && (!start_attempted || remote_cleanup));
    if let Some(tx) = tx.take() { let _ = tx.send(Err(unknown())); }
}

struct State {
    ready: Option<Prepared>, task: Option<JoinHandle<()>>, view: Value,
    consumed: bool, busy: bool, joined: bool, orphaned: bool,
}
struct Shared {
    context: Context, control: transport::Observation, stop: CancellationToken,
    state: Mutex<State>, done: watch::Sender<bool>,
}
pub struct AgentExporterOperation { shared: Arc<Shared> }
impl std::fmt::Debug for AgentExporterOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("AgentExporterOperation([redacted])") }
}
impl Drop for AgentExporterOperation { fn drop(&mut self) { self.cancel(); } }
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
            let runtime = state.view["runtimeObserved"].as_bool().unwrap_or(false);
            state.view["runtimeObserved"] = json!(runtime || admitted);
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
impl AgentExporterOperation {
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
            "actorAdmission":state.view["actorAdmission"],"runtimeObserved":state.view["runtimeObserved"],"remoteDrainObserved":false,
            "cleanupState":if joined && !state.orphaned && !state.busy
                && (state.view["cleanupState"] == "closedObserved" || !state.consumed) { "closedObserved" } else { "unknown" }}))
            .map_err(|_| unknown())
    }
}

impl AppCore {
    pub async fn agent_exporter_prepare_json(self, raw: &str) -> AppResult<AgentExporterOperation> {
        let request: Request = parse(raw)?;
        let association = request.validate()?;
        let context = Context::capture_collection(&self, Frame { version: request.version,
            profile_id: request.profile_id, notification_scope_id: request.notification_scope_id },
            Duration::from_secs(request.duration_seconds as u64)).await?;
        drop(self);
        let control = transport::Observation::new();
        let ready = bounded_original(&context.scope, Prepared::capture(&context, association, Some(request.manifest_sha256), control.clone())).await?;
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
        Ok(AgentExporterOperation { shared })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CatalogRequest {
    version: u8, profile_id: String, notification_scope_id: String, association_id: String,
}
impl AppCore {
    /// Explicit read of the original Node's fixed advertised artifact/policy.
    /// No owner signature, Actor admission, child spawn or returned operation.
    pub async fn agent_exporter_catalog_json(self, raw:&str) -> AppResult<String> {
        let request:CatalogRequest=parse(raw)?;
        let association=Uuid::parse_str(&request.association_id).map_err(|_|unavailable())?;
        if association.is_nil() || association.to_string()!=request.association_id {return Err(unavailable());}
        let context=Context::capture(&self,Frame {version:request.version,profile_id:request.profile_id,
            notification_scope_id:request.notification_scope_id}).await?;
        drop(self);
        bounded_original(&context.scope,async {
            let control=transport::Observation::new();
            let mut ready=Prepared::capture(&context,association,None,control.clone()).await?;
            ready.revalidate(&context).await?;
            let view=json!({"phase":"exporterCatalog","associationId":association,
                "hostId":ready.host,"hostRevision":ready.revision.to_string(),"node":data::node(&ready.probe.node),
                "purpose":"exporters","scope":"exporters.manage","manifestSha256":ready.catalog.manifest_sha256,
                "policy":ready.catalog.policy,"runtimeObserved":false,"actorAdmission":false,
                "installationPerformed":false,"automaticRenewal":false,"cleanupState":"closedObserved"});
            ready.close();
            control.wait_closed(Instant::now()+Duration::from_secs(3)).await?;
            context.check().await?;
            context.publish(view,8192)
        }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exporter_intent_rejects_paths_policy_injection_unbounded_ttl_and_aliases() {
        let original=json!({"version":1,"profileId":"original","notificationScopeId":"same-frame",
            "associationId":"00000000-0000-4000-8000-000000000001",
            "manifestSha256":Hash::from_bytes([1;32]),"durationSeconds":60});
        parse::<Request>(&original.to_string()).unwrap().validate().unwrap();
        for seconds in [0,29,301,65535] {
            let mut changed=original.clone();changed["durationSeconds"]=json!(seconds);
            assert!(parse::<Request>(&changed.to_string()).and_then(|r|r.validate()).is_err());
        }
        for key in ["binaryPath","policy","approved","scope","lease"] {
            let mut changed=original.clone();changed[key]=json!("untrusted");
            assert!(parse::<Request>(&changed.to_string()).is_err());
        }
        let mut missing=original;missing.as_object_mut().unwrap().remove("manifestSha256");
        assert!(parse::<Request>(&missing.to_string()).is_err());
    }
    #[test]
    fn exporter_start_receipt_requires_explicit_unknown_running_version_field() {
        let value=json!({"format":"cc-agent-managed-exporter-session/v1","phase":"running",
            "manifestSha256":Hash::from_bytes([1;32]),"leaseHash":Hash::from_bytes([2;32]),
            "expiresAt":Timestamp::new(1000).unwrap(),
            "agentId":AgentId::new("00000000-0000-4000-8000-000000000001").unwrap(),
            "bind":{"ip":"127.0.0.1","port":19100},"collectors":["cpu","meminfo"],
            "runningVersion":null,"artifactVersion":"v1.12.1"});
        let observed:Started=serde_json::from_value(value.clone()).unwrap();
        assert!(observed.running_version.is_none());
        let mut missing=value;missing.as_object_mut().unwrap().remove("runningVersion");
        assert!(serde_json::from_value::<Started>(missing).is_err());
        // The real Started::check also requires original policy/hash/lease/Node/expiry.
    }
}
