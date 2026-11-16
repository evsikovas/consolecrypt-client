//! One enrolled service process: scheduler в†’ capsule sessions в†’ SAME Node job.
//! Existing JobScopeRegistry is the only target authority registry.
use crate::{
    AutomationError, EnrolledJobGrant, EnvironmentProbe, HelperPeer, JobScopeRegistry,
    JobTargetScope, Result, ServiceCapsuleInput, SignedJobApproval, TargetHandler, VerifiedJob,
};
use cc_crypto_core::{
    DeviceSecretKeys, ServiceExecutionOrigin, ServiceScheduledSlot, VerifiedServiceDelegation,
};
use cc_models::{
    automation::{ApprovedTarget, JobMode},
    known_host::KnownHost,
};
use cc_ssh_core::ConnectionPlan;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

fn denied() -> AutomationError {
    AutomationError::Invalid("service activation unavailable")
}

#[cfg(test)]
mod cancellation_order_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn original_job_cancel_response_precedes_close_even_when_cancel_fails() {
        // Actual HelperPeer framing/backpressure, not a fabricated job grant.
        // The private helper only retires the already-captured job identity.
        for remote_error in [false, true] {
            let (client, server) = tokio::io::duplex(4096);
            let (read, write) = tokio::io::split(client);
            let peer = HelperPeer::connect(tokio::io::BufReader::new(read), write);
            let (read, mut write) = tokio::io::split(server);
            let mut read = tokio::io::BufReader::new(read);
            let job = Uuid::new_v4();
            let stopped = CancellationToken::new();
            let unrelated = CancellationToken::new();
            let close_polled = Arc::new(AtomicBool::new(false));
            let seen = close_polled.clone();
            let close = async move {
                seen.store(true, Ordering::SeqCst);
                false // An uncertain close is never upgraded into success.
            };
            let task = tokio::spawn(cancel_before_close(
                peer.clone(),
                stopped.clone(),
                job,
                close,
            ));
            let mut raw = Vec::new();
            tokio::time::timeout(Duration::from_secs(2), read.read_until(b'\n', &mut raw))
                .await
                .unwrap()
                .unwrap();
            let request: Value = serde_json::from_slice(&raw).unwrap();
            assert_eq!(request["operation"], "job_cancel");
            assert_eq!(request["body"], json!({"job_id":job}));
            assert!(stopped.is_cancelled() && !unrelated.is_cancelled());
            assert!(
                !close_polled.load(Ordering::SeqCst) && !task.is_finished(),
                "original close must not begin while cancellation response is held"
            );
            let error = if remote_error {
                json!({"code":"job_denied","message":"denied"})
            } else {
                Value::Null
            };
            let mut response = serde_json::to_vec(&json!({"version":1,"type":"response",
                "request_id":request["request_id"],"result":{"state":"cancelling"},"error":error}))
            .unwrap();
            response.push(b'\n');
            write.write_all(&response).await.unwrap();
            assert!(!tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap());
            assert!(close_polled.load(Ordering::SeqCst) && stopped.is_cancelled());
            peer.close().await;
        }
    }
}
// Only the captured original active job reaches here. Cancellation must be
// delivered while its identity is retained, before close_active takes it.
async fn cancel_before_close(
    peer: Arc<HelperPeer>,
    stopped: CancellationToken,
    job: Uuid,
    close: impl std::future::Future<Output = bool>,
) -> bool {
    stopped.cancel();
    let _ = peer
        .request("job_cancel", json!({"job_id":job}), Duration::from_secs(5))
        .await;
    // Failure/timeout is not a cancellation acknowledgement. Still close the
    // original channels; caller retains its original error/unknown outcome.
    close.await
}
fn now() -> Result<u64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| denied())?
        .as_secs())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Due {
    status: String,
    slot: Option<ServiceScheduledSlot>,
    targets: Option<Vec<ApprovedTarget>>,
}
#[derive(Serialize)]
pub struct ServiceExecutionObservation {
    pub job_id: Uuid,
    pub state: Value,
    pub schedule_resolved: bool,
    pub local_sessions_closed: bool,
}

/// The caller supplies ORIGINAL privately enrolled keys/owner evidence and the
/// existing helper/target registry, never a user vault or general resolver.
/// Keep this owner alive through shutdown; dropping it cancels, not joins.
pub struct ServiceExecutionSupervisor {
    peer: Arc<HelperPeer>,
    registry: Arc<JobScopeRegistry>,
    delegation: VerifiedServiceDelegation,
    keys: DeviceSecretKeys,
    origin: ServiceExecutionOrigin,
    plans: Vec<ConnectionPlan>,
    known_hosts: Vec<KnownHost>,
    capsules: Vec<ServiceCapsuleInput>,
    stopped: CancellationToken,
    active: Option<(Uuid, String, Arc<JobTargetScope>)>,
    inflight: bool,
    access_deadline: u64,
}
impl Drop for ServiceExecutionSupervisor {
    fn drop(&mut self) {
        self.stopped.cancel();
        if let Some((id, digest, scope)) = &self.active {
            scope.cancel();
            self.registry.revoke(*id, digest);
        }
    }
}
impl ServiceExecutionSupervisor {
    pub fn new(
        peer: Arc<HelperPeer>,
        registry: Arc<JobScopeRegistry>,
        delegation: VerifiedServiceDelegation,
        keys: DeviceSecretKeys,
        origin: ServiceExecutionOrigin,
        plans: Vec<ConnectionPlan>,
        known_hosts: Vec<KnownHost>,
        capsules: Vec<ServiceCapsuleInput>,
    ) -> Result<Self> {
        if !cfg!(target_os = "linux")
            || peer.is_closed()
            || plans.is_empty()
            || plans.len() > 32
            || capsules.len() > 64
            || delegation.body().execution_origin.as_ref() != Some(&origin)
            || now()? >= delegation.body().expires_at
        {
            return Err(denied());
        }
        let access_deadline =
            capsules
                .iter()
                .try_fold(delegation.body().expires_at, |deadline, input| {
                    input
                        .capsule
                        .access_expires_at()
                        .map(|end| deadline.min(end))
                        .map_err(|_| denied())
                })?;
        if now()? >= access_deadline {
            return Err(denied());
        }
        Ok(Self {
            peer,
            registry,
            delegation,
            keys,
            origin,
            plans,
            known_hosts,
            capsules,
            stopped: CancellationToken::new(),
            active: None,
            inflight: false,
            access_deadline,
        })
    }
    pub(crate) fn owner_status_verifier(
        &self,
    ) -> Result<cc_crypto_core::ServiceOwnerStatusVerifier> {
        self.delegation
            .owner_status_verifier()
            .map_err(|_| denied())
    }
    pub fn cancellation(&self) -> CancellationToken {
        self.stopped.clone()
    }
    pub(crate) fn signed(&self) -> Result<Value> {
        serde_json::from_slice(&self.delegation.signed_json().map_err(|_| denied())?)
            .map_err(|_| denied())
    }

    /// Polls the REAL scheduler. A not-due response creates no signed job, secret
    /// resolution or target connection. Unknown slots are never automatically
    /// reconstructed or retried after restart.
    pub async fn activate_due(
        &mut self,
        mode: JobMode,
    ) -> Result<Option<ServiceExecutionObservation>> {
        if self.active.is_some() || self.stopped.is_cancelled() || now()? >= self.access_deadline {
            return Err(denied());
        }
        self.inflight = true;
        let signed = self.signed()?;
        let due: Due = serde_json::from_value(
            self.peer
                .request(
                    "service_due",
                    json!({"delegation":signed}),
                    Duration::from_secs(10),
                )
                .await?,
        )
        .map_err(|_| denied())?;
        if due.status != "reserved" {
            if due.slot.is_some()
                || due.targets.is_some()
                || ![
                    "not_due",
                    "already_seen",
                    "skipped_gap",
                    "skipped_missed",
                    "skipped_overlap",
                    "skipped_before_activation",
                    "clock_restart_baseline",
                    "clock_forward_skip",
                    "clock_rollback_blocked",
                ]
                .contains(&due.status.as_str())
            {
                return Err(denied());
            }
            self.inflight = false;
            return Ok(None);
        }
        let slot = due.slot.ok_or_else(denied)?;
        let targets = due.targets.ok_or_else(denied)?;
        let raw = self
            .peer
            .request("probe", json!({}), Duration::from_secs(30))
            .await?;
        let helper = raw
            .get("helper_digest")
            .and_then(Value::as_str)
            .ok_or_else(denied)?
            .to_owned();
        let probe = EnvironmentProbe::parse(raw)?;
        let current = now()?;
        let deadline =
            self.capsules
                .iter()
                .try_fold(self.delegation.body().expires_at, |min, input| {
                    input
                        .capsule
                        .access_expires_at()
                        .map(|time| min.min(time))
                        .map_err(|_| denied())
                })?;
        let job = self
            .keys
            .sign_service_scheduled_job(&self.delegation, &slot, targets, mode, deadline, current)
            .map_err(|_| denied())?;
        let approval: SignedJobApproval =
            serde_json::from_value(serde_json::to_value(&job.approval).map_err(|_| denied())?)
                .map_err(|_| denied())?;
        let policy = self.delegation.body();
        let grant = EnrolledJobGrant {
            signer_id: approval.signer_id,
            public_key: self.keys.public_keys().signing,
            runner_id: policy.service_id.parse().map_err(|_| denied())?,
            tenant_id: policy.tenant.id.parse().map_err(|_| denied())?,
            projects: [policy.project_id.parse().map_err(|_| denied())?]
                .into_iter()
                .collect(),
            hosts: policy
                .hosts
                .iter()
                .map(|h| h.id.parse().map_err(|_| denied()))
                .collect::<Result<_>>()?,
            jobs_execute: true,
            expires_at: deadline.try_into().map_err(|_| denied())?,
            max_job_seconds: policy.quotas.max_job_seconds,
        };
        let verified = VerifiedJob::verify(
            &approval,
            &grant,
            &probe.environment_digest,
            current.try_into().map_err(|_| denied())?,
        )?;
        let id = verified.manifest().job_id;
        let digest = verified.digest().to_owned();
        let scope = crate::bind_service_job_scope(
            &self.delegation,
            &self.origin,
            &probe,
            &helper,
            verified,
            self.plans.clone(),
            self.known_hosts.clone(),
            &self.capsules,
            &self.keys,
            self.stopped.child_token(),
        )
        .await?;
        if let Err(error) = self.registry.register(scope.clone()) {
            let _ = scope.close_sessions().await;
            return Err(error);
        }
        self.active = Some((id, digest, scope));
        let work = async {
            let policy = self.delegation.body();
            let refs = json!({"service_id":policy.service_id,"signer_epoch":policy.signer.key_epoch,
                "recipient_epoch":policy.recipient.key_epoch,"certificate_epoch":policy.signer.certificate_epoch,
                "recipient_certificate_epoch":policy.recipient.certificate_epoch,"project_id":policy.project_id,
                "project_revision":policy.project_revision,"template_digest":policy.template_digest,
                "hosts":policy.hosts,"credentials":policy.credentials,"environment_digest":policy.environment_digest,
                "inventory_digest":policy.inventory_digest,"parameters_digest":policy.parameters_digest,
                "playbook":policy.playbook,"route_inputs":policy.route_inputs});
            // Bounded offline has no owner-head renewal. Fresh-head mode needs
            // a separate original authenticated observation; v1 activates ONLY
            // explicitly approved boundedOffline delegations.
            if policy.access_policy.mode != cc_crypto_core::ServiceAccessMode::BoundedOffline {
                return Err(denied());
            }
            let ack = self
                .peer
                .request(
                    "service_start",
                    json!({"delegation":self.signed()?,"head":null,
                "slot":job.slot,"approval":job.approval,"current_refs":refs}),
                    Duration::from_secs(30),
                )
                .await?;
            if ack.get("started") != Some(&json!(true)) || ack.get("jobId") != Some(&json!(id)) {
                return Err(denied());
            }
            loop {
                let state = self
                    .peer
                    .request("job_get", json!({"job_id":id}), Duration::from_secs(10))
                    .await?;
                if state.get("job_id") != Some(&json!(id)) {
                    return Err(denied());
                }
                match state.get("state").and_then(Value::as_str) {
                    Some("queued" | "running" | "cancelling") => {
                        tokio::time::sleep(Duration::from_millis(200)).await
                    }
                    Some("succeeded" | "failed" | "cancelled" | "interrupted" | "unknown") => {
                        return Ok(state);
                    }
                    _ => return Err(denied()),
                }
            }
        };
        let result = tokio::select! {biased;
            _=self.stopped.cancelled()=>Err(AutomationError::Closed),
            result=tokio::time::timeout(Duration::from_secs(330),work)=>result.map_err(|_|AutomationError::Timeout).and_then(|v|v),
        };
        let uncertain = self
            .active
            .as_ref()
            .is_none_or(|(_, _, scope)| scope.has_uncertain_operations());
        let manifest_digest = self
            .active
            .as_ref()
            .map(|(_, digest, _)| digest.clone())
            .ok_or_else(denied)?;
        let closed = if result.is_err() {
            cancel_before_close(
                self.peer.clone(),
                self.stopped.clone(),
                id,
                self.close_active(),
            )
            .await
        } else {
            self.close_active().await
        };
        self.inflight = false;
        let _runner_state = result?;
        let outcome = if uncertain || !closed {
            "uncertain"
        } else {
            "known"
        };
        let observation = self
            .peer
            .request(
                "service_target_end",
                json!({"job_id":id,"manifest_digest":manifest_digest,
            "outcome":outcome}),
                Duration::from_secs(10),
            )
            .await?;
        if observation.get("recorded") != Some(&json!(true)) {
            return Err(denied());
        }
        let resolved = if outcome == "known" {
            self.peer
                .request(
                    "service_finish_slot",
                    json!({"job_id":id}),
                    Duration::from_secs(10),
                )
                .await
                .is_ok_and(|v| v.get("resolved") == Some(&json!(true)))
        } else {
            false
        };
        let state = self
            .peer
            .request(
                "service_job_status",
                json!({"job_id":id}),
                Duration::from_secs(10),
            )
            .await?;
        if !closed {
            return Err(AutomationError::Closed);
        }
        Ok(Some(ServiceExecutionObservation {
            job_id: id,
            state,
            schedule_resolved: resolved,
            local_sessions_closed: true,
        }))
    }
    async fn close_active(&mut self) -> bool {
        if let Some((id, digest, scope)) = self.active.take() {
            self.registry.revoke(id, &digest);
            scope.close_sessions().await.is_ok()
        } else {
            true
        }
    }
    pub async fn shutdown(&mut self) -> bool {
        self.stopped.cancel();
        if let Some((id, _, _)) = &self.active {
            let _ = self
                .peer
                .request("job_cancel", json!({"job_id":id}), Duration::from_secs(5))
                .await;
        }
        let abandoned = self.inflight;
        let closed = self.close_active().await;
        self.inflight = false;
        closed && !abandoned
    }
}
