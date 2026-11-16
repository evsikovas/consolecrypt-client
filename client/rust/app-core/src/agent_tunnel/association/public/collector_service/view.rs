//! Closed real producer. No raw policy/OwnerPin/credential/key/path is exposed.
use super::*;

pub(super) fn value(
    facts: &ServiceReviewFacts,
    phase: ServiceViewPhase,
    owner: Value,
    acknowledged: Option<&VerifiedServiceConfiguration>,
    cleanup: &str,
) -> AppResult<Value> {
    if !matches!(
        cleanup,
        "notStarted" | "pending" | "closedObserved" | "unknown"
    ) {
        return Err(unavailable());
    }
    if matches!(phase, ServiceViewPhase::ConfigurationRecorded) != acknowledged.is_some() {
        return Err(unavailable());
    }
    let policy = &facts.policy;
    let hash = policy.digest().map_err(|_| unavailable())?;
    let boot = acknowledged.map(|a| {
        json!({
        "bootId":a.claim.body.clock.boot_id,
        "createdBoottimeNs":a.claim.body.clock.created_boottime_ns.to_string(),
        "deadlineBoottimeNs":a.claim.body.clock.deadline_boottime_ns.to_string()})
    });
    let ack = acknowledged
        .map(|a| -> AppResult<Value> {
            Ok(json!({
        "actorRevision":a.receipt.body.actor_revision.to_string(),
        "recordedAtUtc":data::utc(a.receipt.body.issued_at)?}))
        })
        .transpose()?;
    let mut value = json!({
        "kind":"finiteCollectorService","phase":phase.label(),
        "associationId":facts.association,"hostId":facts.host,"hostRevision":facts.host_revision.to_string(),
        "owner":owner,"node":data::node(&facts.node),"scope":"metrics.collect",
        "serviceId":policy.service_id,"serviceHash":hash,
        "collectorId":policy.collector_id,"manifestSha256":policy.manifest_sha256,
        "manifest":facts.catalog.manifest,
        "ownerEpoch":policy.header.owner_epoch.to_string(),"checkpoint":policy.header.checkpoint.to_string(),
        "actorRevision":acknowledged.map(|a|a.receipt.body.actor_revision).unwrap_or(facts.actor_revision).to_string(),
        "policyRevision":policy.policy_revision.to_string(),"revocationEpoch":policy.revocation_epoch.to_string(),
        "profile":"maintained-prometheus-disk-512mib-2h-v1","bufferMode":"persistentNativeWal",
        "remainingMs":0,"issuedAtUtc":data::utc(policy.issued_at)?,"expiresAtUtc":data::utc(policy.expires_at)?,
        "effectiveAuthoritySeconds":policy.expires_at.seconds().checked_sub(policy.issued_at.seconds()).ok_or_else(unavailable)?,
        "maxAuthoritySeconds":86400,"maxStarts":32,"maxConcurrent":1,
        "maxBufferBytes":536870912u64,"maxResidenceSeconds":7200,"maxMemoryBytes":536870912u64,
        "continuesWhenClientLocks":true,"automaticRenewal":false,
        "osBootPolicy":"sameBootOnly","osRebootRequiresApproval":true,
        "bootStamp":boot,"configurationAck":ack,
        "runtimeState":"notObserved","deliveryGuaranteed":false,"lossUnknown":true,
        "cleanupState":cleanup
    });
    if !facts.credentials.is_empty() {
        value["kind"] = json!("finiteCollectorServiceCredentials");
        value["credentialSelections"] = json!(facts
            .credentials
            .iter()
            .map(|item| item.view())
            .collect::<Vec<_>>());
    }
    Ok(value)
}

pub(super) async fn publish(
    context: &Context,
    facts: &ServiceReviewFacts,
    phase: ServiceViewPhase,
    acknowledged: Option<&VerifiedServiceConfiguration>,
    control: &transport::Observation,
) -> AppResult<String> {
    facts.live(context)?;
    let owner = tokio::select! {biased;
        _=context.scope.cancelled()=>return Err(unavailable()),
        _=tokio::time::sleep_until(tokio::time::Instant::from_std(context.scope.deadline()))=>return Err(unavailable()),
        owner=context.owner()=>owner?,
    };
    facts.live(context)?;
    context.publish(
        value(facts, phase, owner, acknowledged, control.cleanup_state())?,
        16384,
    )
}
