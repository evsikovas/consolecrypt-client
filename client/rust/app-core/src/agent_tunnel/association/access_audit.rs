//! Observations from the actual Native route, in its original encrypted DB.
//! No payload, TLS key, Relay capability or caller-supplied audit identity.
use super::*;
use cc_storage_core::{NodeAssociationOwner, NodeTunnelAccessEvent, NodeTunnelAccessRecord, Storage};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) enum RouteAuditEnd { StopRequested, Cancelled, Revoked, Unknown }

pub(crate) struct RouteAudit {
    scope: Arc<OriginalScope>,
    storage: Storage,
    owner: NodeAssociationOwner,
    base: NodeTunnelAccessRecord,
    opening: AtomicBool,
    opened: AtomicBool,
    finished: AtomicBool,
    last_observed: AtomicU64,
}
impl std::fmt::Debug for RouteAudit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RouteAudit([redacted])")
    }
}
fn id(value: &impl serde::Serialize) -> AppResult<Uuid> {
    let value = serde_json::to_value(value).map_err(|_| unknown())?;
    let raw = value.as_str().ok_or_else(unknown)?;
    let id = Uuid::parse_str(raw).map_err(|_| unknown())?;
    if id.is_nil() || id.to_string() != raw { return Err(unknown()); }
    Ok(id)
}
fn observed_after(start: u64) -> AppResult<u64> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| unknown())?.as_secs();
    if now < start || now > 253_402_300_799 { return Err(unknown()); }
    Ok(now)
}
impl RouteAudit {
    pub(crate) async fn open(
        scope: &Arc<OriginalScope>, host: ObjectId, host_revision: i64,
        association_id: Uuid, binding: &NodeRequestBinding, policy: &TunnelRoutePolicyV2,
    ) -> AppResult<Self> {
        policy.validate().map_err(|_| unavailable())?;
        scope.check_current()?;
        let ledger = ledger::Ledger::capture_original(scope.clone()).await?;
        let owner = ledger.audit_owner()?;
        let storage = scope.with_current_resources(|session, _, _| {
            let storage = session.storage()?;
            if storage.path().is_none() { return Err(unavailable()); }
            Ok(storage)
        })?;
        let base = NodeTunnelAccessRecord {
            operation_id: Uuid::new_v4(), association_id, host_id: host, host_revision,
            grant_id: id(&binding.lease.body.id)?,
            grant_digest: hex::encode(binding.lease.body.digest().map_err(|_| unknown())?.as_bytes()),
            policy_digest: hex::encode(policy.digest().map_err(|_| unknown())?.as_bytes()),
            agent_id: id(&policy.route.agent_id)?.to_string(),
            key_epoch: policy.route.key_epoch, protocol: policy.route.destination.protocol.clone(),
            destination_ip: policy.route.destination.ip.clone(),
            destination_port: policy.route.destination.port,
            expires_at: binding.lease.body.expires_at.min(binding.freshness.body.expires_at)
                .min(binding.certificate.body.expires_at).seconds(),
            observed_at: scope.now()?.0.seconds(), event: NodeTunnelAccessEvent::OpenRequested,
            sent: 0, received: 0, counters_exact: true, native_control_finished: false,
        };
        let last_observed = AtomicU64::new(base.observed_at);
        let value = Self { scope: scope.clone(), storage, owner, base,
            opening: AtomicBool::new(false), opened: AtomicBool::new(false), finished: AtomicBool::new(false), last_observed };
        let owner = value.owner.clone();
        let record = value.base.clone();
        let original = scope.clone();
        tokio::time::timeout(Duration::from_secs(2), value.storage.write_node_association(move |tx| {
            original.with_io_current(|_| tx.node_tunnel_access_append(&owner, record).map_err(AppError::from))
        })).await.map_err(|_| unknown())?.map_err(|_| unknown())?; // Durable BEFORE bind/dispatch.
        scope.check_current()?;
        Ok(value)
    }
    async fn write(&self, record: NodeTunnelAccessRecord) -> AppResult<()> {
        let owner = self.owner.clone();
        // This is the already-captured original database. Terminal observation
        // may finish after grant expiry; it never resolves a new Core/session,
        // obtains a key, signs, or permits another network operation.
        tokio::time::timeout(Duration::from_secs(2), self.storage.write_node_association(move |tx| {
            tx.node_tunnel_access_append(&owner, record).map_err(AppError::from)
        })).await.map_err(|_| unknown())?.map_err(|_| unknown())
    }
    pub(crate) fn require_binding(
        &self, scope: &Arc<OriginalScope>, binding: &NodeRequestBinding, policy: &TunnelRoutePolicyV2,
    ) -> AppResult<()> {
        self.scope.check_current()?;
        if !Arc::ptr_eq(scope, &self.scope) || self.finished.load(Ordering::SeqCst)
            || self.base.grant_id != id(&binding.lease.body.id)?
            || self.base.grant_digest != hex::encode(binding.lease.body.digest().map_err(|_| unknown())?.as_bytes())
            || self.base.policy_digest != hex::encode(policy.digest().map_err(|_| unknown())?.as_bytes())
        { return Err(unknown()); }
        Ok(())
    }
    /// Called only by the maintained pump after the SAME TLS stream's Node ACK
    /// has verified, before the first application payload is read or copied.
    pub(crate) async fn observe_opened(&self) -> AppResult<()> {
        self.scope.check_current()?;
        if self.finished.load(Ordering::SeqCst) || self.opening.swap(true, Ordering::SeqCst) {
            return Err(unknown());
        }
        let mut record = self.base.clone();
        record.event = NodeTunnelAccessEvent::Opened;
        record.observed_at = self.scope.now()?.0.seconds();
        self.last_observed.store(record.observed_at, Ordering::SeqCst);
        let owner = self.owner.clone();
        let original = self.scope.clone();
        // Capture only weak provenance plus this original DB. No AppCore or
        // upgraded Session is retained through the storage queue/await.
        tokio::time::timeout(Duration::from_secs(2), self.storage.write_node_association(move |tx| {
            original.with_io_current(|_| {
                tx.node_tunnel_access_append(&owner, record).map_err(AppError::from)
            })
        })).await.map_err(|_| unknown())?.map_err(|_| unknown())?;
        self.scope.check_current()?;
        self.opened.store(true, Ordering::SeqCst);
        Ok(())
    }
    /// Observed ACK and durable Opened only; this flag grants no authority.
    pub(crate) fn opened_observed(&self) -> bool {
        self.opened.load(Ordering::SeqCst)
    }
    /// The private parent invokes this only after dropping/joining its owned
    /// stream/listener/worker. Native counts and closure are not remote delivery.
    pub(crate) async fn finish(
        &self, end: RouteAuditEnd, sent: u64, received: u64,
    ) -> AppResult<()> {
        if self.finished.swap(true, Ordering::SeqCst) { return Err(unknown()); }
        let mut record = self.base.clone();
        record.event = match end {
            RouteAuditEnd::StopRequested => NodeTunnelAccessEvent::StopRequested,
            RouteAuditEnd::Cancelled => NodeTunnelAccessEvent::Cancelled,
            RouteAuditEnd::Revoked => NodeTunnelAccessEvent::Revoked,
            RouteAuditEnd::Unknown => NodeTunnelAccessEvent::Unknown,
        };
        record.sent = sent;
        record.received = received;
        // An interrupted write can have emitted an unmeasured prefix. These
        // remain Native lower bounds until a separate trusted drain proves more.
        record.counters_exact = false;
        record.native_control_finished = true;
        record.observed_at = observed_after(self.last_observed.load(Ordering::SeqCst))?;
        self.write(record).await
    }
    pub(crate) async fn list(scope: &Arc<OriginalScope>) -> AppResult<Value> {
        let ledger = ledger::Ledger::capture_original(scope.clone()).await?;
        let owner = ledger.audit_owner()?;
        let storage = scope.with_current_resources(|session, _, _| Ok(session.storage()?))?;
        let original = scope.clone();
        let rows = storage.read(move |tx| original.with_io_current(|_| {
            tx.node_tunnel_access_records(&owner).map_err(AppError::from)
        })).await?;
        scope.check_current()?;
        Ok(serde_json::json!({"version":1,"boundedLocalObservations":true,
            "immutableLog":false,"remoteDeliveryGuarantee":false,"rows":rows}))
    }
}
