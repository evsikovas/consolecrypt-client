//! Durable interactive admissions and conservative, profile-wide target leases.
//! Persistent server-side scheduling is a separate service and authority.
use crate::automation_prepare::{AutomationRunRequest, PreparedReview, invalid};
use crate::automation_production_owner::{OwnerPassphraseWitness, manifest_for_review};
use crate::automation_projects::{automation_error, automation_io_slot};
use crate::automation_runners::RunnerConnection;
use crate::dto::AutomationJobStateDto;
use crate::session::Session;
use crate::{AppCore, AppError, AppEvent, AppResult};
use base64::{Engine, engine::general_purpose::STANDARD};
use cc_automation_core::{
    EnrolledJobGrant, EnvironmentProbe, JobTargetScope, JobTicket, SignedJobApproval,
    TargetHandler, VerifiedJob,
};
use cc_models::automation::JobManifest;
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Weak},
    time::Duration,
};
use uuid::Uuid;

const JOBS: &str = "automation.jobs.v1";
mod admission_audit;
mod provenance;
#[cfg(all(feature = "test-harness", not(test)))]
pub(crate) use admission_audit::tests as __focused_admission_audit;
mod result_projection;
#[derive(Serialize, Deserialize)]
struct Catalog {
    version: u32,
    records: BTreeMap<String, Value>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}
impl Default for Catalog {
    fn default() -> Self {
        Self {
            version: 1,
            records: BTreeMap::new(),
            extra: BTreeMap::new(),
        }
    }
}

fn terminal(state: &str) -> bool {
    matches!(
        state,
        "succeeded" | "failed" | "cancelled" | "interrupted" | "acknowledged"
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RetryOrigin {
    pub job_id: Uuid,
    pub manifest_digest: String,
}

/// A retry is a fresh, explicitly reviewed operation. This relationship grants
/// no credentials and cannot turn an uncertain/active job into a repeat.
fn resolve_retry_origin(
    catalog: &Catalog,
    request: &AutomationRunRequest,
) -> AppResult<Option<RetryOrigin>> {
    let Some(id) = request.retry_of.as_deref() else {
        return Ok(None);
    };
    if catalog.version != 1 {
        return Err(invalid("newer job history is read-only"));
    }
    let row = catalog
        .records
        .get(id)
        .ok_or_else(|| invalid("previous job is missing from this profile"))?;
    let (job, _, hosts) = row_identity(row)?;
    let digest = row["manifest_digest"]
        .as_str()
        .filter(|value| cc_models::automation::is_digest(value))
        .ok_or_else(|| invalid("invalid previous job digest"))?;
    if job.to_string() != id
        || !terminal(row["state"].as_str().unwrap_or_default())
        || row["project_id"] != json!(request.project_id)
        || row["manifest"]["playbook"] != json!(request.playbook)
        || request
            .host_ids
            .iter()
            .any(|id| !hosts.iter().any(|host| host.to_string() == *id))
    {
        return Err(invalid(
            "retry requires a finished job, the same project/playbook and explicitly selected original targets",
        ));
    }
    Ok(Some(RetryOrigin {
        job_id: job,
        manifest_digest: digest.into(),
    }))
}
fn row_identity(row: &Value) -> AppResult<(Uuid, Uuid, Vec<Uuid>)> {
    if row["version"] != 1 {
        return Err(invalid("newer job history is read-only"));
    }
    let job = serde_json::from_value(row["job_id"].clone())
        .map_err(|_| invalid("invalid saved job identifier"))?;
    let runner = serde_json::from_value(row["runner_id"].clone())
        .map_err(|_| invalid("invalid saved Runner identifier"))?;
    let hosts: Vec<Uuid> = serde_json::from_value(row["host_ids"].clone())
        .map_err(|_| invalid("invalid saved job targets"))?;
    if hosts.is_empty() || hosts.len() > 10000 {
        return Err(invalid("invalid saved job targets"));
    }
    Ok((job, runner, hosts))
}

const MAX_NOTIFICATION_REVISION: u64 = 9_007_199_254_740_991;

fn increment_notification_revision(row: &mut Value) -> AppResult<u64> {
    let previous = match row.get("notification_revision") {
        None => 0,
        Some(value) => value
            .as_u64()
            .ok_or_else(|| invalid("invalid job notification revision"))?,
    };
    let next = previous
        .checked_add(1)
        .filter(|value| *value <= MAX_NOTIFICATION_REVISION)
        .ok_or_else(|| invalid("job notification revision exhausted"))?;
    row["notification_revision"] = json!(next);
    Ok(next)
}

fn emit_job_change(owner: &Session, job: Uuid, state: AutomationJobStateDto, revision: u64) {
    owner.ctx.emit(AppEvent::AutomationJobChanged {
        profile_id: owner.profile_id.to_string(),
        job_id: job.to_string(),
        revision,
        state,
    });
}

// Only the two real control-evidence shapes are projected; no recursive
// authority/result interpretation and no change to outer outcome/intervals.
fn project_saved_summary(row: &mut Value) -> AppResult<()> {
    let original = row.get("runner_result").cloned();
    admission_audit::project_row(row);
    let Some(before) = original else {
        return Ok(());
    };
    let aliases: std::collections::BTreeSet<String> = row["manifest"]["targets"]
        .as_array()
        .into_iter()
        .flatten()
        .take(10_000)
        .filter_map(|target| {
            target["alias"]
                .as_str()
                .filter(|a| !a.is_empty() && a.len() <= 256)
                .map(str::to_owned)
        })
        .collect();
    let projected = result_projection::project(&row["runner_result"], &aliases);
    if projected != before {
        if !row["runner_result_original_sha256"]
            .as_str()
            .is_some_and(cc_models::automation::is_digest)
        {
            use sha2::{Digest, Sha256};
            let bytes =
                serde_json::to_vec(&before).map_err(|_| invalid("invalid saved job result"))?;
            row["runner_result_original_sha256"] = json!(
                Sha256::digest(bytes)
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            );
        }
        row["runner_result_redacted"] = json!(true);
    }
    row["runner_result"] = projected;
    Ok(())
}

async fn change_state(
    owner: &Session,
    id: Uuid,
    state: &str,
    evidence: Option<Value>,
) -> AppResult<()> {
    let state = AutomationJobStateDto::from_state(state)
        .ok_or_else(|| invalid("invalid job notification state"))?;
    let changed = owner
        .storage()?
        .write(move |transaction| {
            let mut catalog: Catalog = transaction.setting_get(JOBS)?.unwrap_or_default();
            if catalog.version != 1 {
                return Err(invalid("newer job history is read-only"));
            }
            let row = catalog
                .records
                .get_mut(&id.to_string())
                .ok_or_else(|| invalid("job admission is missing"))?;
            if terminal(row["state"].as_str().unwrap_or_default()) {
                return Err(invalid("job already finished"));
            }
            let previous = row["state"]
                .as_str()
                .ok_or_else(|| invalid("invalid saved job state"))?;
            AutomationJobStateDto::from_state(previous)
                .ok_or_else(|| invalid("invalid saved job state"))?;
            let changed = previous != state.as_state();
            let revision = if changed {
                Some(increment_notification_revision(row)?)
            } else {
                None
            };
            row["state"] = json!(state);
            row["updated_at"] = json!(chrono::Utc::now().timestamp());
            if let Some(evidence) = evidence {
                row.as_object_mut()
                    .ok_or_else(|| invalid("invalid saved job row"))?
                    .remove("runner_result_original_sha256");
                row.as_object_mut()
                    .ok_or_else(|| invalid("invalid saved job row"))?
                    .remove("runner_result_redacted");
                row["runner_result"] = evidence;
                project_saved_summary(row)?;
            }
            transaction.setting_set(JOBS, &catalog)?;
            Ok::<_, AppError>(revision)
        })
        .await?;
    // storage.write resolves only after the SQL transaction commits. Concurrent
    // send order may differ from commit order; consumers compare row revision.
    if let Some(revision) = changed {
        emit_job_change(owner, id, state, revision);
    }
    Ok(())
}

impl AppCore {
    pub(crate) async fn automation_retry_origin(
        &self,
        owner: &Session,
        request: &AutomationRunRequest,
    ) -> AppResult<Option<RetryOrigin>> {
        if request.retry_of.is_none() {
            return Ok(None);
        }
        let catalog = owner
            .storage()?
            .setting_get::<Catalog>(JOBS)
            .await?
            .unwrap_or_default();
        resolve_retry_origin(&catalog, request)
    }

    pub(crate) async fn automation_initialize_jobs(&self, owner: &Arc<Session>) -> AppResult<()> {
        let mut initialized = owner.automation.jobs_initialized.lock().await;
        if *initialized {
            return Ok(());
        }
        // Recovery can introduce unknown reservations without a live worker.
        // Serialize it with the same active-session fence as admission, so an
        // approved/prepared exit cannot gain a new unreviewed unknown lease.
        let _profile = self.hold_session(owner).await?;
        if owner
            .automation
            .admission_paused
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(AppError::invalid(
                "automation_transition",
                "profile transition is waiting; job recovery is paused",
            ));
        }
        let (unknown, changes) = owner
            .storage()?
            .write(|transaction| {
                let mut catalog: Catalog = transaction.setting_get(JOBS)?.unwrap_or_default();
                if catalog.version != 1 {
                    return Err(invalid("newer job history is read-only"));
                }
                let mut recovered = Vec::new();
                let mut changes = Vec::new();
                for row in catalog.records.values_mut() {
                    let identity = row_identity(row)?;
                    // Include terminal/pinned legacy rows before recovery's
                    // early continue; SQL commit precedes history publication.
                    project_saved_summary(row)?;
                    let state = row["state"]
                        .as_str()
                        .ok_or_else(|| invalid("invalid saved job state"))?;
                    if terminal(state) {
                        continue;
                    }
                    // Preserve the existing conservative recovery policy. A
                    // repeated unknown row restores its lease but emits no
                    // invented new state transition.
                    let next = if state == "queued" {
                        AutomationJobStateDto::Interrupted
                    } else {
                        AutomationJobStateDto::Unknown
                    };
                    if state != next.as_state() {
                        let revision = increment_notification_revision(row)?;
                        row["state"] = json!(next);
                        row["updated_at"] = json!(chrono::Utc::now().timestamp());
                        if next == AutomationJobStateDto::Interrupted {
                            row["reason"] = json!("client_restarted_before_dispatch");
                        }
                        changes.push((identity.0, next, revision));
                    }
                    if next == AutomationJobStateDto::Unknown {
                        recovered.push(identity);
                    }
                }
                transaction.setting_set(JOBS, &catalog)?;
                Ok::<_, AppError>((recovered, changes))
            })
            .await?;
        for (job, state, revision) in changes {
            emit_job_change(owner, job, state, revision);
        }
        for (job, runner, hosts) in unknown {
            owner
                .automation
                .coordinator
                .restore_unknown(job, runner, hosts)
                .map_err(automation_error)?;
        }
        *initialized = true;
        Ok(())
    }

    /// Accepts only a one-use review ID. No externally supplied manifest or
    /// signature can reach the signing key through this API.
    pub async fn automation_confirm_run(&self, review_id: String) -> AppResult<Value> {
        let id = Uuid::parse_str(&review_id).map_err(|_| invalid("invalid review identifier"))?;
        let (owner, unlocked) = self.unlocked().await?;
        let original = Arc::downgrade(&unlocked);
        drop(unlocked);
        self.automation_confirm_original(owner, original, id, None)
            .await
    }
    pub(crate) async fn automation_confirm_original(
        &self,
        owner: Arc<Session>,
        captured: Weak<crate::session::Unlocked>,
        id: Uuid,
        owner_witness: Option<OwnerPassphraseWitness>,
    ) -> AppResult<Value> {
        self.automation_initialize_jobs(&owner).await?;
        let unlocked = captured.upgrade().ok_or(AppError::VaultLocked)?;
        let review = owner
            .automation
            .reviews
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id)
            .ok_or_else(|| invalid("review expired, was cancelled or was already consumed"))?;
        let generation = owner.automation.unlocked_generation()?;
        let original = review
            .context
            .unlock
            .upgrade()
            .ok_or(AppError::VaultLocked)?;
        if !Arc::ptr_eq(&original, &unlocked)
            || !review.usable(generation)
            || !Weak::ptr_eq(&captured, &review.context.unlock)
            || !review.validation.is_valid()
            || (review.policy.production && owner_witness.is_none())
            || (!review.policy.production && owner_witness.is_some())
            || review.service_delegation.is_some()
            || review.service_origin.is_some()
            || matches!(
                review.runner.configuration,
                crate::automation_runners::AutomationRunnerConfig::ServiceRemote { .. }
            )
        {
            return Err(invalid("review no longer permits execution"));
        }
        drop(original);
        let mut shutdown = unlocked.sharing_shutdown.subscribe();
        drop(unlocked);
        let prepare = async {
            let current = self
                .automation_project_context(&review.context.project_id)
                .await?;
            if !Arc::ptr_eq(&current.owner, &owner)
                || !Weak::ptr_eq(&current.unlock, &review.context.unlock)
            {
                return Err(AppError::VaultLocked);
            }
            if current.project.revision != Some(review.context.project_revision) {
                return Err(invalid("project metadata changed"));
            }
            if let Some(folders) = &review.context.folder_composition {
                let u = current.unlock.upgrade().ok_or(AppError::VaultLocked)?;
                let _profile = self.hold_session(&owner).await?;
                let _epoch = owner.hold_unlocked(&u).await?;
                folders.check(&u)?;
            }
            // An editor may atomically replace the complete project directory.
            // A retained old directory handle must not hide the new current tree.
            let source = current.path;
            let io_slot = automation_io_slot(&owner)?;
            let revision = tokio::task::spawn_blocking(move || {
                let _io_slot = io_slot;
                cc_automation_core::ProjectRoot::open(&source)?.revision()
            })
            .await
            .map_err(|_| invalid("source scan stopped"))?
            .map_err(automation_error)?;
            if revision.digest != review.revision.digest {
                return Err(invalid("source changed after review"));
            }
            let environment = EnvironmentProbe::parse(
                review
                    .runner
                    .peer
                    .request("probe", json!({}), Duration::from_secs(60))
                    .await
                    .map_err(automation_error)?,
            )
            .map_err(automation_error)?;
            environment
                .require_trusted_worker_bootstrap()
                .map_err(|_| {
                    AppError::invalid(
                        "automation_runner_update",
                        "update the selected Runner to use trusted worker bootstrap",
                    )
                })?;
            if environment.environment_digest != review.validation.environment_digest() {
                return Err(invalid("Runner changed after review"));
            }
            self.automation_authorize_connection(&owner, &review).await
        };
        let grant = tokio::select! { biased; _=shutdown.changed()=>return Err(AppError::VaultLocked), result=prepare=>result?, };
        let original = review
            .context
            .unlock
            .upgrade()
            .ok_or(AppError::VaultLocked)?;
        let _profile = self.hold_session(&owner).await?;
        let _epoch = owner.hold_unlocked(&original).await?;
        if owner
            .automation
            .admission_paused
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(AppError::invalid(
                "automation_transition",
                "profile transition is waiting; no new jobs admitted",
            ));
        }
        if !review.usable(owner.automation.unlocked_generation()?) {
            return Err(invalid("review changed before confirmation"));
        }
        crate::automation_projects::folder_bindings::confirm_metadata(
            &owner,
            &review.context.project_id,
            review.context.project_revision,
            review.context.folder_composition.as_ref(),
        )
        .boxed()
        .await?;
        if let Some(folders) = &review.context.folder_composition {
            folders.check(&original)?;
        }
        if let Some(witness) = &owner_witness {
            witness.check_password_envelope(&owner).await?;
        }
        let now = chrono::Utc::now().timestamp();
        let manifest = if let Some(frozen) = &review.production_owner {
            let witness = owner_witness
                .as_ref()
                .ok_or_else(|| invalid("production owner passphrase confirmation required"))?;
            frozen.authorize(witness, &owner, &review, &grant)?;
            frozen.manifest().clone()
        } else {
            if review.policy.production {
                return Err(invalid("production manifest unavailable"));
            }
            manifest_for_review(&review, &grant, now)?
        };
        let identity = owner.identity();
        if identity.public_keys().signing != grant.public_key
            || identity.device_id().to_string() != grant.signer_id.to_string()
        {
            return Err(invalid("device identity changed; reconnect Runner"));
        }
        if let Some(folders) = &review.context.folder_composition {
            folders.finish(&original)?;
        }
        let signature = identity
            .sign_automation_manifest(&manifest)
            .map_err(|_| invalid("job signature failed"))?;
        let approval = SignedJobApproval {
            signer_id: grant.signer_id,
            manifest,
            signature: STANDARD.encode(signature),
        };
        let verified = if let Some(frozen) = &review.production_owner {
            let confirmation = identity
                .sign_production_owner_confirmation(&approval.manifest)
                .map_err(|_| invalid("production owner confirmation signature failed"))?;
            VerifiedJob::verify_owner_confirmed(
                &approval,
                frozen.manual(),
                &confirmation,
                review.validation.environment_digest(),
                now,
            )
        } else {
            VerifiedJob::verify(
                &approval,
                &grant,
                review.validation.environment_digest(),
                now,
            )
        }
        .map_err(automation_error)?;
        let job = approval.manifest.job_id;
        let digest = verified.digest().to_owned();
        // These facts come only from the verified original Native admission;
        // UI callers cannot provide a signer or substitute an uploaded revision.
        let provenance = provenance::JobProvenance::admitted(
            &approval.manifest,
            Uuid::parse_str(&owner.profile_id.to_string())
                .map_err(|_| invalid("invalid profile"))?,
            Uuid::parse_str(&review.context.project_id).map_err(|_| invalid("invalid project"))?,
            approval.signer_id,
            review.context.project_revision,
            review.snapshot.uploaded.revision_id,
        )?;
        let mut row = json!({"version":1,"job_id":job,"runner_id":grant.runner_id,"project_id":approval.manifest.project_id,
            "host_ids":review.targets.iter().map(|target|target.host_id).collect::<Vec<_>>(),"manifest_digest":digest,
            "manifest":approval.manifest,"correlation_provenance":provenance,"git_source":review.git_source,"inventory_groups":review.inventory_groups_summary(),"retry_origin":review.retry_origin,"state":"queued","notification_revision":1,"accepted_at":now,"updated_at":now,"review_id":id});
        if let Some(confirmation) = verified.owner_confirmation() {
            row["owner_confirmation"] = json!(confirmation);
        }
        admission_audit::project_row(&mut row);
        let persisted = row.clone();
        let project_id = review.context.project_id.clone();
        let project_revision = review.context.project_revision;
        let retry_request = review.request.clone();
        let expected_retry = review.retry_origin.clone();
        // Reserve before the database write, but dispatch only after it. A
        // failed write drops a waiting ticket and cannot leave an orphan job.
        let ticket = owner
            .automation
            .coordinator
            .enqueue(
                job,
                grant.runner_id,
                review.targets.iter().map(|target| target.host_id),
            )
            .map_err(automation_error)?;
        owner
            .storage()?
            .write(move |transaction| {
                let projects: Value = transaction
                    .setting_get("automation.projects.v1")?
                    .ok_or_else(|| invalid("project metadata disappeared"))?;
                if projects["version"] != 1
                    || projects["records"][&project_id]["revision"] != json!(project_revision)
                {
                    return Err(invalid("project changed before admission"));
                }
                let mut catalog: Catalog = transaction.setting_get(JOBS)?.unwrap_or_default();
                if resolve_retry_origin(&catalog, &retry_request)? != expected_retry {
                    return Err(invalid("previous job changed after review"));
                }
                if catalog.version != 1 || catalog.records.len() >= 2048 {
                    return Err(invalid("job history limit reached"));
                }
                if catalog.records.insert(job.to_string(), persisted).is_some() {
                    return Err(invalid("duplicate job admission"));
                }
                transaction.setting_set(JOBS, &catalog)?;
                Ok::<_, AppError>(())
            })
            .await?;
        emit_job_change(&owner, job, AutomationJobStateDto::Queued, 1);
        owner
            .automation
            .live_jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                job,
                (review.cancellation.0.clone(), review.policy.cancel_on_lock),
            );
        drop(_epoch);
        drop(_profile);
        drop(original);
        if let Some(session) = &review.vault_session {
            // Durable normal job admission remains unchanged. Resolve only
            // while the ORIGINAL unlock is still live; a lock winning here
            // interrupts this job instead of doing a later general lookup.
            let material: AppResult<()> = async {
                let pending = session
                    .resolve_pending_job(self, &verified, &review.cancellation.0)
                    .await?;
                let original = review
                    .context
                    .unlock
                    .upgrade()
                    .ok_or(AppError::VaultLocked)?;
                let _profile = self.hold_session(&owner).await?;
                let _epoch = owner.hold_unlocked(&original).await?;
                if !review.usable(owner.automation.unlocked_generation()?) {
                    return Err(invalid("Vault job context changed"));
                }
                session.check_before_material(&original).await?;
                let diagnostic = review
                    .sessions
                    .values()
                    .next()
                    .and_then(|session| session.diagnostic_capture())
                    .ok_or_else(|| invalid("job diagnostic custody unavailable"))?;
                let opened =
                    session.commit_material(&verified, pending, &original, diagnostic.as_ref())?;
                drop(_epoch);
                drop(_profile);
                drop(original);
                opened
                    .await
                    .map_err(|_| invalid("Vault material owner retired"))??;
                Ok(())
            }
            .await;
            if material.is_err() {
                review.cancellation.0.cancel();
                session.close_and_join().await;
                let _=change_state(&owner,job,"interrupted",Some(json!({"reason":"vault_material_unavailable_before_dispatch","remote_changes_may_be_partial":false}))).await;
                owner
                    .automation
                    .live_jobs
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&job);
                return Err(AppError::invalid(
                    "automation_vault",
                    "job_admitted_material_unavailable",
                ));
            }
        }
        let task_owner = owner.clone();
        tokio::spawn(async move {
            run_admission(task_owner, review, approval, verified, ticket).await;
        });
        Ok(row)
    }

    pub(crate) async fn automation_authorize_connection(
        &self,
        owner: &Arc<Session>,
        review: &PreparedReview,
    ) -> AppResult<EnrolledJobGrant> {
        let grant = self
            .automation_runner_authorization(owner, &review.runner, review.context.unlock.clone())
            .await?;
        if grant.expires_at
            <= chrono::Utc::now().timestamp() + i64::from(review.policy.timeout_seconds)
            || !grant.projects.contains(
                &Uuid::parse_str(&review.request.project_id)
                    .map_err(|_| invalid("invalid project"))?,
            )
            || review
                .targets
                .iter()
                .any(|target| !grant.hosts.contains(&target.host_id))
        {
            return Err(invalid(
                "Runner grant expired or inventory changed; disconnect and reconnect Runner",
            ));
        }
        Ok(grant)
    }

    async fn automation_runner_authorization(
        &self,
        owner: &Arc<Session>,
        runner: &Arc<RunnerConnection>,
        unlock: Weak<crate::session::Unlocked>,
    ) -> AppResult<EnrolledJobGrant> {
        if matches!(
            runner.configuration,
            crate::automation_runners::AutomationRunnerConfig::ServiceRemote { .. }
        ) {
            return Err(invalid(
                "service Runner cannot issue an interactive job grant",
            ));
        }
        let mut existing = runner.authorization.lock().await;
        if let Some(grant) = existing.as_ref() {
            return Ok(grant.clone());
        }
        let unlocked = unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let mut shutdown = unlocked.sharing_shutdown.subscribe();
        let hosts = unlocked
            .working()
            .hosts()
            .into_iter()
            .map(|host| Uuid::parse_str(&host.id.to_string()).map_err(|_| invalid("invalid host")))
            .collect::<AppResult<_>>()?;
        drop(unlocked);
        let projects = self
            .automation_projects()
            .await?
            .projects
            .into_iter()
            .filter(|project| !project.read_only)
            .map(|project| Uuid::parse_str(&project.id).map_err(|_| invalid("invalid project")))
            .collect::<AppResult<_>>()?;
        let identity = owner.identity();
        let now = chrono::Utc::now().timestamp();
        let grant = EnrolledJobGrant {
            signer_id: Uuid::parse_str(&identity.device_id().to_string())
                .map_err(|_| invalid("invalid device"))?,
            public_key: identity.public_keys().signing,
            tenant_id: Uuid::parse_str(&owner.profile_id.to_string())
                .map_err(|_| invalid("invalid profile"))?,
            runner_id: Uuid::parse_str(&runner.id).map_err(|_| invalid("invalid Runner"))?,
            projects,
            hosts,
            jobs_execute: true,
            expires_at: now + 3600,
            max_job_seconds: 3600,
        };
        let original = unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let profile = self.hold_session(owner).await?;
        let epoch = owner.hold_unlocked(&original).await?;
        drop(epoch);
        drop(profile);
        drop(original);
        if *shutdown.borrow() {
            return Err(AppError::VaultLocked);
        }
        let configure=runner.peer.request("configure_interactive",json!({"tenant_id":grant.tenant_id,"runner_id":grant.runner_id,
            "signer_id":grant.signer_id,"public_key":STANDARD.encode(grant.public_key),"hosts":grant.hosts,"projects":grant.projects,
            "expires_at":grant.expires_at,"max_job_seconds":grant.max_job_seconds,"transport":"native_scoped_rpc_v1"}),Duration::from_secs(10));
        tokio::select! {biased;_=shutdown.changed()=>return Err(AppError::VaultLocked),result=configure=>result.map_err(automation_error)?,};
        let original = unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = self.hold_session(owner).await?;
        let _epoch = owner.hold_unlocked(&original).await?;
        *existing = Some(grant.clone());
        Ok(grant)
    }

    pub async fn automation_job_events(
        &self,
        id: String,
        after: u64,
        limit: u16,
    ) -> AppResult<cc_automation_core::JobEventPage> {
        if after > i64::MAX as u64 || !(1..=500).contains(&limit) {
            return Err(invalid("invalid event page"));
        }
        let job = Uuid::parse_str(&id).map_err(|_| invalid("invalid job identifier"))?;
        let (owner, unlocked) = self.unlocked().await?;
        let unlock = Arc::downgrade(&unlocked);
        let mut shutdown = unlocked.sharing_shutdown.subscribe();
        drop(unlocked);
        let catalog: Catalog = owner
            .storage()?
            .setting_get(JOBS)
            .await?
            .unwrap_or_default();
        if catalog.version != 1 {
            return Err(invalid("newer job history is read-only"));
        }
        let row = catalog
            .records
            .get(&job.to_string())
            .ok_or_else(|| invalid("job admission is missing"))?;
        let (stored_job, runner_id, _) = row_identity(row)?;
        if stored_job != job {
            return Err(invalid("job admission identifier mismatch"));
        }
        let manifest_digest = row["manifest_digest"]
            .as_str()
            .ok_or_else(|| invalid("job digest is missing"))?;
        let runner = owner
            .automation
            .connections
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&runner_id.to_string())
            .cloned()
            .filter(|runner| !runner.peer.is_closed())
            .ok_or_else(|| invalid("connect the job Runner to read its events"))?;
        let request = async {
            self.automation_runner_authorization(&owner, &runner, unlock.clone())
                .await?;
            runner
                .peer
                .request(
                    "job_events",
                    json!({"job_id":job,"after":after,"limit":limit}),
                    Duration::from_secs(15),
                )
                .await
                .map_err(automation_error)
        };
        let raw = tokio::select! {biased;_=shutdown.changed()=>return Err(AppError::VaultLocked),result=request=>result?,};
        let page = cc_automation_core::JobEventPage::parse(raw, job, manifest_digest, after, limit)
            .map_err(automation_error)?;
        let original = unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = self.hold_session(&owner).await?;
        let _epoch = owner.hold_unlocked(&original).await?;
        Ok(page)
    }

    pub async fn automation_jobs(&self) -> AppResult<Value> {
        let (owner, unlocked) = self.unlocked().await?;
        self.automation_initialize_jobs(&owner).await?;
        let catalog: Catalog = owner
            .storage()?
            .setting_get(JOBS)
            .await?
            .unwrap_or_default();
        let _profile = self.hold_session(&owner).await?;
        let _epoch = owner.hold_unlocked(&unlocked).await?;
        let mut catalog = catalog;
        for row in catalog.records.values_mut() {
            project_saved_summary(row)?;
        }
        Ok(
            json!({"schema_version":catalog.version,"read_only":catalog.version!=1,"jobs":catalog.records.into_values().collect::<Vec<_>>()}),
        )
    }

    pub async fn automation_cancel_job(&self, id: String) -> AppResult<()> {
        let job = Uuid::parse_str(&id).map_err(|_| invalid("invalid job identifier"))?;
        let (owner, unlocked) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _epoch = owner.hold_unlocked(&unlocked).await?;
        let (cancel, _) = owner
            .automation
            .live_jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&job)
            .cloned()
            .ok_or_else(|| invalid("job is not active"))?;
        cancel.cancel();
        Ok(())
    }

    /// Explicit acknowledgement is deliberately separate from Cancel. The UI
    /// must show that commands may still be running and changes may be partial.
    pub async fn automation_acknowledge_unknown(
        &self,
        id: String,
        accept_partial_changes: bool,
    ) -> AppResult<()> {
        if !accept_partial_changes {
            return Err(invalid("explicit acknowledgement is required"));
        }
        let job = Uuid::parse_str(&id).map_err(|_| invalid("invalid job identifier"))?;
        let (owner, unlocked) = self.unlocked().await?;
        self.automation_initialize_jobs(&owner).await?;
        let _profile = self.hold_session(&owner).await?;
        let _epoch = owner.hold_unlocked(&unlocked).await?;
        if !owner.automation.coordinator.unknown_jobs().contains(&job) {
            return Err(invalid("job is not awaiting reconciliation"));
        }
        change_state(
            &owner,
            job,
            "acknowledged",
            Some(json!({"user_acknowledged_partial_changes":true})),
        )
        .await?;
        owner
            .automation
            .coordinator
            .resolve_unknown(job)
            .map_err(automation_error)
    }
}

async fn run_admission(
    owner: Arc<Session>,
    review: PreparedReview,
    approval: SignedJobApproval,
    verified: VerifiedJob,
    ticket: JobTicket,
) {
    let job = approval.manifest.job_id;
    let outcome = dispatch(&owner, &review, &approval, verified, &ticket).await;
    if let Some(session) = &review.vault_session {
        session.close_and_join().await;
    }
    // Revocation must not wait for the vault to reopen its history database.
    review.cancellation.0.cancel();
    let known = match outcome {
        Ok((state, evidence)) => {
            // Lock can seal SQLCipher while an approved job continues. Keep the
            // lease until its result is durably recorded after unlock, or leave
            // dispatching/unknown for explicit recovery after a profile close.
            loop {
                if change_state(&owner, job, &state, evidence.clone())
                    .await
                    .is_ok()
                {
                    break terminal(&state);
                }
                tokio::select! { _=owner.automation.stopped.cancelled()=>break false, _=tokio::time::sleep(Duration::from_millis(500))=>{} }
            }
        }
        Err(_) => {
            let _=change_state(&owner,job,"unknown",Some(json!({"reason":"control_channel_or_storage_lost","remote_changes_may_be_partial":true}))).await;
            false
        }
    };
    if known {
        ticket.finish();
    } else {
        drop(ticket);
    }
    owner
        .automation
        .live_jobs
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&job);
}

async fn dispatch(
    owner: &Session,
    review: &PreparedReview,
    approval: &SignedJobApproval,
    verified: VerifiedJob,
    ticket: &JobTicket,
) -> AppResult<(String, Option<Value>)> {
    let job = approval.manifest.job_id;
    let cancel = &review.cancellation.0;
    tokio::select! { biased;
        _=owner.automation.stopped.cancelled()=>return Ok(("interrupted".into(),None)),
        _=tokio::time::sleep(verified.dispatch_remaining())=>return Ok(("interrupted".into(),Some(json!({"reason":"approval_expired_in_queue"})))),
        result=ticket.wait(cancel)=>if result.is_err() { return Ok(("cancelled".into(),None)); },
    }
    // Queue waiting never extends either the signed wall-clock deadline or its
    // native monotonic deadline. No fresh signature is produced automatically.
    if cancel.is_cancelled()
        || verified.is_access_expired()
        || verified.is_submission_expired()
        || chrono::Utc::now().timestamp() >= approval.manifest.expires_at
    {
        return Ok((
            "interrupted".into(),
            Some(json!({"reason":"approval_expired_in_queue"})),
        ));
    }
    // A queued job may wait through lock, but it cannot be newly dispatched from
    // a locked vault. The already signed capability is never silently renewed.
    loop {
        if cancel.is_cancelled() || verified.is_access_expired() || verified.is_submission_expired()
        {
            return Ok(("interrupted".into(), None));
        }
        if owner.automation.unlocked_generation().is_ok() {
            break;
        }
        tokio::select! { _=owner.automation.stopped.cancelled()=>return Ok(("interrupted".into(),None)), _=tokio::time::sleep(Duration::from_millis(250))=>{} }
    }
    let vault_generation = owner.automation.unlocked_generation()?;
    if review.become_password.is_some() && vault_generation != review.context.generation {
        if let Some(password) = &review.become_password {
            password.revoke();
        }
        return Ok((
            "interrupted".into(),
            Some(json!({"reason":"sudo_input_context_retired",
            "remote_changes_may_be_partial":false})),
        ));
    }
    if review
        .vault_session
        .as_ref()
        .is_some_and(|session| !session.material_live())
    {
        return Ok((
            "interrupted".into(),
            Some(
                json!({"reason":"vault_material_expired_before_dispatch","remote_changes_may_be_partial":false}),
            ),
        ));
    }
    change_state(owner, job, "dispatching", None).await?;
    let owner_confirmation = verified.owner_confirmation().cloned();
    let scope = JobTargetScope::bind_with_become(
        verified,
        review.sessions.clone(),
        cancel.clone(),
        review.become_password.clone(),
    )
    .map_err(automation_error)?;
    owner
        .automation
        .scopes
        .register(scope.clone())
        .map_err(automation_error)?;
    let peer = &review.runner.peer;
    let mut submission = json!({"approval":approval,"revision_id":review.snapshot.uploaded.revision_id,
        "parameters":review.request.parameters,"vault_generation":vault_generation});
    if review.become_password.is_some() {
        submission["become_native"] = json!(true);
    }
    if let Some(confirmation) = owner_confirmation {
        submission["approval"]["owner_confirmation"] = json!(confirmation);
    }
    if let Some(reference) = &review.inventory_ref {
        submission["inventory_ref"] = json!(reference);
    } else {
        submission["inventory"] = review.inventory.clone();
    }
    let submitted = peer
        .request("accept_job", submission, Duration::from_secs(30))
        .await;
    // Lost acknowledgement is queried using the same ID, never resubmitted.
    if submitted.is_ok() {
        let _ = change_state(owner, job, "running", None).await;
    }
    let deadline = tokio::time::Instant::now()
        + Duration::from_secs(u64::from(review.policy.timeout_seconds) + 45);
    let mut sent_cancel = false;
    let outcome = loop {
        if owner.automation.stopped.is_cancelled() || tokio::time::Instant::now() >= deadline {
            break Ok(unknown_control(
                if owner.automation.stopped.is_cancelled() {
                    "owner_closed"
                } else {
                    "observer_deadline"
                },
            ));
        }
        if cancel.is_cancelled() && !sent_cancel {
            let _ = peer
                .request("job_cancel", json!({"job_id":job}), Duration::from_secs(5))
                .await;
            sent_cancel = true;
        }
        let state = peer
            .request("job_get", json!({"job_id":job}), Duration::from_secs(10))
            .await;
        match state {
            Ok(state)
                if state["job_id"] == json!(job)
                    && state["manifest_digest"] == json!(approval_digest(approval)?) =>
            {
                let phase = state["state"].as_str().unwrap_or_default();
                if phase == "unknown" {
                    break Ok(("unknown".into(), Some(state)));
                }
                if terminal(phase) && phase != "acknowledged" {
                    if scope.has_uncertain_operations() {
                        break Ok((
                            "unknown".into(),
                            Some(
                                json!({"reason":"native_operation_not_acknowledged","runner_result":state}),
                            ),
                        ));
                    }
                    break Ok((phase.into(), Some(state)));
                }
                if !matches!(phase, "queued" | "running" | "cancelling") {
                    break Ok(unknown_control("unexpected_runner_phase"));
                }
            }
            Ok(_) => break Ok(unknown_control("runner_binding_mismatch")),
            Err(error) => break Ok(unknown_control(control_failure_kind(&error))),
        }
        tokio::time::sleep(Duration::from_millis(350)).await;
    };
    owner
        .automation
        .scopes
        .revoke(job, &approval_digest(approval)?);
    outcome
}

fn unknown_control(kind: &'static str) -> (String, Option<serde_json::Value>) {
    (
        "unknown".into(),
        Some(json!({
            "reason":"control_channel_or_storage_lost",
            "control_failure":kind,
            "remote_changes_may_be_partial":true
        })),
    )
}

/// Do not persist free-form remote errors: they may contain paths, credentials
/// or output. This evidence classifies observation loss, never the remote job's
/// result, and does not authorize a retry or a longer target-access lease.
fn control_failure_kind(error: &cc_automation_core::AutomationError) -> &'static str {
    use cc_automation_core::AutomationError as E;
    match error {
        E::Timeout => "rpc_timeout",
        E::Closed => "rpc_closed",
        E::QueueFull => "rpc_queue_full",
        E::Protocol(_) => "rpc_protocol",
        E::Remote(_) => "rpc_remote",
        E::Io(_) => "rpc_io",
        _ => "rpc_invalid",
    }
}

fn approval_digest(approval: &SignedJobApproval) -> AppResult<String> {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(cc_models::automation::JOB_SIGNATURE_DOMAIN);
    hash.update(approval.manifest.canonical_bytes().map_err(invalid)?);
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    const PASSWORD: &str = "correct horse automation fixture 42";
    #[test]
    fn lost_control_evidence_redacts_remote_and_io_material_and_remains_unknown() {
        use cc_automation_core::AutomationError as E;
        for error in [
            E::Remote("synthetic-private-key-and-token".into()),
            E::Io(std::io::Error::other("synthetic-private-key-and-token")),
            E::Protocol("synthetic-private-key-and-token"),
        ] {
            let (state, evidence) = unknown_control(control_failure_kind(&error));
            assert_eq!(state, "unknown");
            assert!(!terminal(&state));
            let evidence = evidence.unwrap();
            assert_eq!(evidence["remote_changes_may_be_partial"], true);
            assert_eq!(evidence["reason"], "control_channel_or_storage_lost");
            assert!(!evidence.to_string().contains("synthetic-private"));
            assert!(evidence.get("exit_code").is_none());
            assert!(evidence.get("cancelled").is_none());
        }
    }

    fn legacy_result_fixture() -> Value {
        json!({"job_id":Uuid::new_v4(),"manifest_digest":"a".repeat(64),"state":"unknown","updated_at":123,"cancel_requested":true,"result":{"exit_code":7,"partial":true,"elapsed":1.25,"runner_execution":{"version":1,"exit_observed":true},"summary":{"status":"failed","exit_code":7,"stdout":"knownlegacysecret","token":"knownlegacysecret","stats":{"failures":{"cc_owned":1,"knownlegacysecret":2},"arbitrary":"knownlegacysecret"}}}})
    }
    #[test]
    fn saved_summary_projection_is_closed_idempotent_and_keeps_outer_outcome() {
        for nested in [false, true] {
            let original = legacy_result_fixture();
            let evidence = if nested {
                json!({"reason":"native_operation_not_acknowledged","runner_result":original})
            } else {
                original
            };
            let mut row =
                json!({"manifest":{"targets":[{"alias":"cc_owned"}]},"runner_result":evidence});
            use sha2::{Digest, Sha256};
            let digest = Sha256::digest(serde_json::to_vec(&row["runner_result"]).unwrap())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            project_saved_summary(&mut row).unwrap();
            assert!(!row.to_string().contains("knownlegacysecret"));
            assert_eq!(row["runner_result_original_sha256"], digest);
            let actual = if nested {
                &row["runner_result"]["runner_result"]
            } else {
                &row["runner_result"]
            };
            let mut expected = legacy_result_fixture();
            expected["job_id"] = actual["job_id"].clone();
            expected["result"]["summary"] =
                json!({"status":"failed","exit_code":7,"stats":{"failures":{"cc_owned":1}}});
            expected["result"]["runner_execution"] = json!({"redacted":true});
            assert_eq!(actual, &expected);
            let stable = row.clone();
            project_saved_summary(&mut row).unwrap();
            assert_eq!(row, stable);
        }
    }
    #[tokio::test]
    async fn actual_sql_legacy_terminal_summary_scrub_and_current_save_share_projection() {
        let directory = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            directory.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile("Summary privacy".into(), PASSWORD.into())
            .await
            .unwrap();
        let (owner, _) = app.unlocked().await.unwrap();
        let mut catalog = Catalog::default();
        let mut ids = Vec::new();
        for phase in ["failed", "acknowledged", "queued"] {
            let id = Uuid::new_v4();
            ids.push(id);
            catalog.records.insert(id.to_string(),json!({"version":1,"job_id":id,"runner_id":Uuid::new_v4(),"host_ids":[Uuid::new_v4()],"state":phase,"updated_at":123,"notification_revision":4,"pinned":true,"future_field":{"preserve":true},"manifest":{"targets":[{"alias":"cc_owned"}]},"runner_result":legacy_result_fixture()}));
        }
        owner
            .storage()
            .unwrap()
            .write(move |tx| tx.setting_set(JOBS, &catalog))
            .await
            .unwrap();
        let visible = app.automation_jobs().await.unwrap();
        assert!(!visible.to_string().contains("knownlegacysecret"));
        let saved: Catalog = owner
            .storage()
            .unwrap()
            .setting_get(JOBS)
            .await
            .unwrap()
            .unwrap();
        for id in &ids {
            let row = &saved.records[&id.to_string()];
            assert_eq!(row["pinned"], true);
            assert_eq!(row["future_field"]["preserve"], true);
            assert!(row["runner_result_original_sha256"].as_str().is_some());
        }
        for id in &ids[..2] {
            let row = &saved.records[&id.to_string()];
            assert_eq!(row["updated_at"], 123);
            assert_eq!(row["notification_revision"], 4);
        }
        let queued = ids[2];
        assert_eq!(saved.records[&queued.to_string()]["state"], "interrupted");
        let current = Uuid::new_v4();
        owner.storage().unwrap().write(move|tx|{let mut c:Catalog=tx.setting_get(JOBS)?.unwrap();c.records.insert(current.to_string(),json!({"version":1,"job_id":current,"runner_id":Uuid::new_v4(),"host_ids":[Uuid::new_v4()],"state":"running","manifest":{"targets":[{"alias":"cc_owned"}]}}));tx.setting_set(JOBS,&c)}).await.unwrap();
        change_state(&owner, current, "failed", Some(legacy_result_fixture()))
            .await
            .unwrap();
        let saved: Catalog = owner
            .storage()
            .unwrap()
            .setting_get(JOBS)
            .await
            .unwrap()
            .unwrap();
        let row = &saved.records[&current.to_string()];
        assert!(!row.to_string().contains("knownlegacysecret"));
        assert_eq!(row["runner_result"]["result"]["exit_code"], 7);
        assert_eq!(row["runner_result"]["result"]["partial"], true);
        app.shutdown().await.unwrap();
    }

    #[test]
    fn observation_timeout_and_closed_channel_are_distinguished_without_retries() {
        use cc_automation_core::AutomationError as E;
        let (_, timeout) = unknown_control(control_failure_kind(&E::Timeout));
        let (_, closed) = unknown_control(control_failure_kind(&E::Closed));
        assert_eq!(timeout.unwrap()["control_failure"], "rpc_timeout");
        assert_eq!(closed.unwrap()["control_failure"], "rpc_closed");
        assert!(!terminal("unknown"));
    }
    #[test]
    fn retry_links_only_a_finished_original_job_and_never_expands_targets() {
        let job = Uuid::new_v4();
        let host = Uuid::new_v4();
        let project = Uuid::new_v4();
        let row = json!({"version":1,"job_id":job,"runner_id":Uuid::new_v4(),"project_id":project,
            "host_ids":[host],"state":"failed","manifest_digest":"a".repeat(64),"manifest":{"playbook":"playbook.yml"}});
        let mut catalog = Catalog::default();
        catalog.records.insert(job.to_string(), row);
        let request:AutomationRunRequest=serde_json::from_value(json!({"project_id":project,"runner_id":Uuid::new_v4(),
            "playbook":"playbook.yml","host_ids":[host],"mode":"check","timeout_seconds":60,"forks":1,
            "cancel_on_lock":true,"retry_of":job,"parameters":{"extra_vars":{},"tags":[],"skip_tags":[],"diff":false}})).unwrap();
        let frozen = resolve_retry_origin(&catalog, &request).unwrap().unwrap();
        assert_eq!(frozen.job_id, job);
        for state in ["running", "dispatching", "queued", "unknown", "cancelling"] {
            catalog.records.get_mut(&job.to_string()).unwrap()["state"] = json!(state);
            assert!(resolve_retry_origin(&catalog, &request).is_err());
        }
        catalog.records.get_mut(&job.to_string()).unwrap()["state"] = json!("failed");
        for field in ["project_id", "playbook", "host_ids"] {
            let mut changed = request.clone();
            match field {
                "project_id" => changed.project_id = Uuid::new_v4().to_string(),
                "playbook" => changed.playbook = "other.yml".into(),
                _ => changed.host_ids.push(Uuid::new_v4().to_string()),
            }
            assert!(resolve_retry_origin(&catalog, &changed).is_err());
        }
        assert!(resolve_retry_origin(&Catalog::default(), &request).is_err());
        catalog.records.get_mut(&job.to_string()).unwrap()["manifest_digest"] =
            json!("b".repeat(64));
        assert_ne!(
            resolve_retry_origin(&catalog, &request).unwrap().unwrap(),
            frozen
        );
        catalog.version = 2;
        assert!(resolve_retry_origin(&catalog, &request).is_err());
    }
    #[tokio::test]
    async fn restart_recovery_does_not_replay_jobs_and_keeps_unknown_targets_fenced() {
        let directory = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            directory.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile("Recovery test".into(), PASSWORD.into())
            .await
            .unwrap();
        let (owner, _) = app.unlocked().await.unwrap();
        let runner = Uuid::new_v4();
        let host = Uuid::new_v4();
        let queued = Uuid::new_v4();
        let active = Uuid::new_v4();
        let catalog=Catalog {version:1,extra:BTreeMap::new(),records:[(queued,"queued"),(active,"dispatching")].into_iter().map(|(id,state)|
            (id.to_string(),json!({"version":1,"job_id":id,"runner_id":runner,"project_id":Uuid::new_v4(),"host_ids":[host],"state":state,
                "future_field":{"preserve":true}}))).collect()};
        owner
            .storage()
            .unwrap()
            .write(move |tx| tx.setting_set(JOBS, &catalog))
            .await
            .unwrap();
        let rows = app.automation_jobs().await.unwrap();
        let rows = rows["jobs"].as_array().unwrap();
        assert_eq!(
            rows.iter()
                .find(|row| row["job_id"] == json!(queued))
                .unwrap()["state"],
            "interrupted"
        );
        assert_eq!(
            rows.iter()
                .find(|row| row["job_id"] == json!(active))
                .unwrap()["state"],
            "unknown"
        );
        assert!(
            rows.iter()
                .all(|row| row["future_field"]["preserve"] == true)
        );
        assert_eq!(owner.automation.coordinator.unknown_jobs(), vec![active]);
        assert!(
            app.automation_acknowledge_unknown(active.to_string(), false)
                .await
                .is_err()
        );
        assert!(owner.automation.coordinator.holds_runner(runner));
        app.automation_acknowledge_unknown(active.to_string(), true)
            .await
            .unwrap();
        assert!(!owner.automation.coordinator.holds_runner(runner));
        assert!(
            app.automation_acknowledge_unknown(active.to_string(), true)
                .await
                .is_err()
        );
        app.shutdown().await.unwrap();
    }
    #[tokio::test]
    async fn lock_revokes_only_jobs_whose_frozen_policy_requests_it() {
        let directory = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            directory.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile("Lock test".into(), PASSWORD.into())
            .await
            .unwrap();
        let (owner, unlocked) = app.unlocked().await.unwrap();
        let cancel = tokio_util::sync::CancellationToken::new();
        let keep = tokio_util::sync::CancellationToken::new();
        owner
            .automation
            .live_jobs
            .lock()
            .unwrap()
            .insert(Uuid::new_v4(), (cancel.clone(), true));
        owner
            .automation
            .live_jobs
            .lock()
            .unwrap()
            .insert(Uuid::new_v4(), (keep.clone(), false));
        drop(unlocked);
        app.lock().await.unwrap();
        assert!(cancel.is_cancelled());
        assert!(!keep.is_cancelled());
        assert!(app.automation_jobs().await.is_err());
        assert!(app.shutdown().await.is_err());
        assert!(!keep.is_cancelled());
        // No workers were dispatched in this policy-only unit fixture. Model
        // their completion explicitly; shutdown must not silently cancel them.
        owner.automation.live_jobs.lock().unwrap().clear();
        app.shutdown().await.unwrap();
    }
    #[tokio::test]
    async fn newer_history_is_not_rewritten_during_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            directory.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile("Compatibility test".into(), PASSWORD.into())
            .await
            .unwrap();
        let (owner, _) = app.unlocked().await.unwrap();
        let future =
            json!({"version":999,"records":{"future":{"opaque":"preserved"}},"new_key":[1,2,3]});
        let saved = future.clone();
        owner
            .storage()
            .unwrap()
            .write(move |tx| tx.setting_set(JOBS, &saved))
            .await
            .unwrap();
        assert!(app.automation_jobs().await.is_err());
        assert_eq!(
            owner
                .storage()
                .unwrap()
                .setting_get::<Value>(JOBS)
                .await
                .unwrap(),
            Some(future)
        );
        app.shutdown().await.unwrap();
    }
}

#[cfg(test)]
#[path = "automation_jobs/notification_tests.rs"]
mod notification_tests;
