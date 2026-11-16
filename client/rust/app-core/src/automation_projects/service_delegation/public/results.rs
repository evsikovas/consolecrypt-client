//! Closed public service history; persisted observations never become a grant.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadReply {
    version: u8,
    namespace: String,
    signing: String,
    recipient: String,
    delegation: SignedServiceDelegation,
    jobs: Vec<Job>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum State {
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
    Unknown,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Target {
    Known,
    Pending,
    Uncertain,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Job {
    job_id: String,
    manifest_digest: String,
    schedule_id: String,
    local_slot: String,
    due_utc: u64,
    state: State,
    runner_state: State,
    target_outcome: Target,
    remote_changes_may_be_partial: bool,
    cancel_requested: Option<bool>,
    timed_out: Option<bool>,
    exit_code: Option<i32>,
    accepted_utc: String,
    updated_utc: String,
}
fn check_job(job: &Job, policy: &SignedServiceDelegation) -> AppResult<()> {
    canonical_review(&job.job_id)?;
    canonical_review(&job.schedule_id)?;
    let body = policy.body();
    if job.schedule_id != body.schedule_id
        || !cc_models::automation::is_digest(&job.manifest_digest)
        || job.due_utc == 0
        || job.due_utc >= body.expires_at
        || job.local_slot.len() != 16
        || chrono::NaiveDateTime::parse_from_str(&job.local_slot, "%Y-%m-%dT%H:%M")
            .map_err(|_| unavailable())?
            .format("%Y-%m-%dT%H:%M")
            .to_string()
            != job.local_slot
    {
        return Err(unavailable());
    }
    for value in [&job.accepted_utc, &job.updated_utc] {
        if value.len() > 64
            || !value.ends_with('Z')
            || chrono::DateTime::parse_from_rfc3339(value).is_err()
        {
            return Err(unavailable());
        }
    }
    let uncertain = job.remote_changes_may_be_partial
        || job.target_outcome != Target::Known
        || job.runner_state == State::Unknown;
    if uncertain != job.remote_changes_may_be_partial
        || (uncertain && job.state != State::Unknown)
        || (!uncertain && job.state != job.runner_state)
        || (job.state == State::Succeeded
            && (job.exit_code != Some(0)
                || job.timed_out != Some(false)
                || job.cancel_requested != Some(false)))
    {
        return Err(unavailable());
    }
    Ok(())
}

impl AppCore {
    pub async fn automation_service_results_json(&self, input: String) -> AppResult<String> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        let original_auth = crate::agent_reader::OriginalSessionAuth::capture(self)?;
        let work = async {
            let request: Action = parse(&input, 4096)?;
            let frame = Frame::capture(
                self,
                request.version,
                &request.profile_id,
                &request.notification_scope_id,
            )
            .await?;
            let original_mutation = frame
                .unlock
                .upgrade()
                .ok_or(AppError::VaultLocked)?
                .working()
                .mutation_stamp();
            let saved = saved(
                self,
                &frame,
                &request.runner_id,
                Some(request.expected_runner_revision),
            )
            .await?;
            let config = saved.configuration.as_ref().ok_or_else(unavailable)?;
            let (_, identity) = config.service_remote()?;
            let ledger = Arc::new(
                owner_ledger::OwnerLedger::capture_weak(
                    self,
                    frame.owner.clone(),
                    frame.unlock.clone(),
                )
                .await?,
            );
            let owner_scope = ledger.original_owner_scope()?;
            let u = frame.unlock.upgrade().ok_or(AppError::VaultLocked)?;
            let mut shutdown = u.sharing_shutdown.subscribe();
            drop(u);
            let observed = async {
                let snapshot = ledger.read(self).await?;
                let service = canonical_review(&request.runner_id)?;
                let row = snapshot
                    .records()
                    .iter()
                    .find(|r| r.service_id == service)
                    .cloned()
                    .ok_or_else(unavailable)?;
                let hash = |bytes: &[u8]| hex::encode(sha2::Sha256::digest(bytes));
                let filter = json!({"namespace":row.node_local_profile,"service_id":row.service_id,"tenant":ledger.recorded_tenant(),
            "owner_id":frame.owner.device_id(),"owner_epoch":row.owner_epoch,"owner_key_sha256":hash(&frame.owner.identity().public_keys().signing),
            "signer_sha256":row.signer_fingerprint,"recipient_sha256":row.recipient_fingerprint,"policy_digest":row.policy_digest});
                let raw = self
                    .automation_service_results_original(
                        frame.owner.clone(),
                        frame.unlock.clone(),
                        &request.runner_id,
                        request.expected_runner_revision,
                        config,
                        filter,
                        owner_scope.clone(),
                        deadline,
                    )
                    .await?;
                let reply: ReadReply = serde_json::from_value(raw).map_err(|_| unavailable())?;
                if reply.version != 1 || reply.jobs.len() > 32 {
                    return Err(unavailable());
                }
                let namespace = ServiceLocalNamespace {
                    version: 1,
                    identity_id: canonical_review(identity)?,
                    key_epoch: 1,
                    namespace: reply.namespace.clone(),
                };
                let public = cc_crypto_core::DevicePublicKeys {
                    signing: hex::decode(&reply.signing)
                        .map_err(|_| unavailable())?
                        .try_into()
                        .map_err(|_| unavailable())?,
                    encryption: hex::decode(&reply.recipient)
                        .map_err(|_| unavailable())?
                        .try_into()
                        .map_err(|_| unavailable())?,
                };
                namespace
                    .validate_public(&public)
                    .map_err(|_| unavailable())?;
                let policy = &reply.delegation;
                SignedServiceDelegation::verify_json(
                    &policy.to_json().map_err(|_| unavailable())?,
                    &frame.owner.device_id().to_string(),
                    &frame.owner.identity().public_keys(),
                    &ledger.recorded_tenant(),
                    policy.body().issued_at,
                )
                .map_err(|_| unavailable())?;
                policy_metadata_matches(
                    &row,
                    &reply.namespace,
                    &reply.signing,
                    &reply.recipient,
                    policy,
                )?;
                let mut seen = BTreeSet::new();
                for job in &reply.jobs {
                    if !seen.insert(&job.job_id) {
                        return Err(unavailable());
                    }
                    check_job(job, policy)?;
                }
                let final_snapshot = ledger.read(self).await?;
                if final_snapshot
                    .records()
                    .iter()
                    .find(|r| r.service_id == service)
                    != Some(&row)
                {
                    return Err(unavailable());
                }
                frame.current(self).await?;
                let u = frame.unlock.upgrade().ok_or(AppError::VaultLocked)?;
                let _profile = self.hold_session(&frame.owner).await?;
                let _unlock = frame.owner.hold_unlocked(&u).await?;
                frame.check(&u)?;
                if u.working().mutation_stamp() != original_mutation {
                    return Err(unavailable());
                }
                owner_scope.check(&frame.owner, &u)?;
                if tokio::time::Instant::now() >= deadline {
                    return Err(unavailable());
                }
                owner_scope.with_current(&frame.owner, || {
                    u.working().tunnel_io_at_version(original_mutation, || {
                        // Recheck AFTER auth/version guard acquisition; a synchronous
                        // mutex wait can outlive the original timer in this poll.
                        frame.check(&u)?;
                        owner_scope.check(&frame.owner,&u)?;
                        if tokio::time::Instant::now()>=deadline{return Err(unavailable());}
                        let output=encode(json!({"version":1,"profileId":frame.owner.profile_id,"notificationScopeId":frame.scope,
                            "runnerId":request.runner_id,"serviceId":service,"policyDigest":row.policy_digest,"resultsAvailable":true,"jobs":reply.jobs}))?;
                        if output.len()>65536{return Err(unavailable());}
                        Ok(output)
                    }).ok_or_else(unavailable)?
                })
            };
            tokio::select! {biased;
                _=owner_scope.cancelled()=>Err(unavailable()),
                _=frame.owner.automation.stopped.cancelled()=>Err(unavailable()),
                _=shutdown.changed()=>Err(AppError::VaultLocked),
                _=tokio::time::sleep_until(deadline)=>Err(unavailable()),
                result=observed=>result,
            }
        };
        tokio::select! {biased;
            _=original_auth.cancelled()=>Err(unavailable()),
            _=tokio::time::sleep_until(deadline)=>Err(unavailable()),
            result=work=>result,
        }
    }
}
