//! Native-owned, one-use execution reviews. Neither the model nor the bridge
//! supplies a manifest, a signature, an SSH endpoint or a credential value.
use crate::automation_projects::{AutomationProjectContext, automation_error, automation_io_slot};
use crate::automation_runners::{AutomationRunnerConfig, RunnerConnection};
use crate::{AppCore, AppError, AppResult};
use cc_automation_core::{
    EnvironmentProbe, HelperLaunch, HelperPeer, ProjectRevision, ProjectRoot, RunParameters,
    UploadedRevision, ValidationProof, VerifiedValidation, target_from_session,
};
use cc_models::{
    ObjectKind,
    automation::{ApprovedTarget, JobMode, JobPolicy},
    host::SshBackend,
};
use cc_ssh_core::SshSession;
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{Arc, Weak},
    time::{Duration, Instant},
};
use tokio::sync::OwnedSemaphorePermit;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Default)]
struct LaunchSecrets {
    vault_password: Option<cc_models::secret::SecretValue>,
    become_password: Option<Arc<cc_automation_core::ScopedBecomePassword>>,
    use_project_vault_password: bool,
}
impl LaunchSecrets {
    fn any(&self) -> bool {
        self.vault_password.is_some()
            || self.become_password.is_some()
            || self.use_project_vault_password
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationRunRequest {
    pub project_id: String,
    pub runner_id: String,
    pub playbook: String,
    pub host_ids: Vec<String>,
    pub mode: JobMode,
    #[serde(deserialize_with = "user_run_parameters")]
    pub parameters: RunParameters,
    pub timeout_seconds: u32,
    pub forks: u8,
    pub cancel_on_lock: bool,
    #[serde(default)]
    pub retry_of: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vault_credential_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vault_selection: Option<crate::automation_projects::vault::selection::VaultSelectionFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment_environment:
        Option<crate::automation_projects::execution_credentials::Selection>,
}

/// Public input remains the legacy four-field shape. In particular even an
/// explicit `vault: null` is rejected, never normalized into a Native binding.
fn user_run_parameters<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<RunParameters, D::Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        extra_vars: BTreeMap<String, Value>,
        tags: Vec<String>,
        skip_tags: Vec<String>,
        diff: bool,
    }
    let value = Input::deserialize(deserializer)?;
    Ok(RunParameters {
        extra_vars: value.extra_vars,
        tags: value.tags,
        skip_tags: value.skip_tags,
        diff: value.diff,
        vault: None,
    })
}

impl std::fmt::Debug for AutomationRunRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AutomationRunRequest")
            .field("project_id", &self.project_id)
            .field("runner_id", &self.runner_id)
            .field("target_count", &self.host_ids.len())
            .field("mode", &self.mode)
            .finish_non_exhaustive()
    }
}

impl AutomationRunRequest {
    pub(crate) fn validate(&self) -> AppResult<()> {
        for id in [&self.project_id, &self.runner_id] {
            let parsed = Uuid::parse_str(id).map_err(|_| invalid("invalid identifier"))?;
            if parsed.is_nil() || parsed.to_string() != *id {
                return Err(invalid("invalid identifier"));
            }
        }
        if let Some(selection) = &self.deployment_environment {
            selection.validate()?;
            if selection.frame.project_id != self.project_id {
                return Err(invalid("deployment environment belongs to another project"));
            }
        }
        cc_automation_core::validate_project_path(&self.playbook).map_err(automation_error)?;
        // The caller may select a credential, never inject Native purpose or
        // sidecar facts into the subsequently signed parameters digest.
        if self.parameters.vault.is_some() {
            return Err(invalid("reserved Native Vault parameters"));
        }
        if self.vault_credential_id.is_some() != self.vault_selection.is_some()
            || self.vault_selection.as_ref().is_some_and(|v| !v.valid())
        {
            return Err(invalid("original Vault selection context required"));
        }
        if let Some(id) = &self.vault_credential_id {
            let parsed =
                Uuid::parse_str(id).map_err(|_| invalid("invalid Vault credential selection"))?;
            if parsed.is_nil() || parsed.to_string() != *id || self.parameters.diff {
                return Err(invalid("invalid Vault credential selection or diff policy"));
            }
        }
        self.parameters.validate().map_err(automation_error)?;
        if !(self.playbook.ends_with(".yml") || self.playbook.ends_with(".yaml"))
            || !(1..=3600).contains(&self.timeout_seconds)
            || !(1..=5).contains(&self.forks)
            || self.host_ids.is_empty()
            || self.host_ids.len() > 32
            || self.host_ids.iter().collect::<HashSet<_>>().len() != self.host_ids.len()
        {
            return Err(invalid(
                "invalid playbook, target selection or execution limits",
            ));
        }
        for id in &self.host_ids {
            crate::dto::parse_id("host_id", id)?;
        }
        if let Some(id) = &self.retry_of {
            let parsed = Uuid::parse_str(id).map_err(|_| invalid("invalid retry identifier"))?;
            if parsed.is_nil() || parsed.to_string() != *id {
                return Err(invalid("invalid retry identifier"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn preparation_state_machine_does_not_exhaust_a_desktop_thread_stack() {
        let directory = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            directory.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        let request = AutomationRunRequest {
            project_id: Uuid::new_v4().to_string(),
            runner_id: Uuid::new_v4().to_string(),
            playbook: "playbook.yml".into(),
            host_ids: vec![Uuid::new_v4().to_string()],
            mode: JobMode::Check,
            parameters: RunParameters {
                extra_vars: BTreeMap::new(),
                tags: vec![],
                skip_tags: vec![],
                diff: false,
                vault: None,
            },
            timeout_seconds: 60,
            forks: 1,
            cancel_on_lock: true,
            retry_of: None,
            vault_credential_id: None,
            vault_selection: None,
            deployment_environment: None,
        };
        let future = app.automation_prepare_run(request);
        let bytes = std::mem::size_of_val(&future);
        assert!(
            bytes <= 64 * 1024,
            "native preparation future is {bytes} bytes; nested operations must be boxed"
        );
    }
}

pub(crate) fn invalid(rule: &str) -> AppError {
    AppError::invalid("automation_run", rule)
}

pub(crate) struct CancelOnDrop(pub CancellationToken);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// Releases only this captured snapshot. An accepted job's retained reference
/// makes the helper refuse deletion. No arbitrary path is ever removed here.
pub(crate) struct RevisionLease {
    pub uploaded: UploadedRevision,
    peer: Weak<HelperPeer>,
}
impl RevisionLease {
    pub(crate) fn new(uploaded: UploadedRevision, peer: &Arc<HelperPeer>) -> Self {
        Self {
            uploaded,
            peer: Arc::downgrade(peer),
        }
    }
}
impl Drop for RevisionLease {
    fn drop(&mut self) {
        if let (Some(peer), Ok(runtime)) =
            (self.peer.upgrade(), tokio::runtime::Handle::try_current())
        {
            let id = self.uploaded.revision_id;
            runtime.spawn(async move {
                let _ = peer
                    .request(
                        "revision_release",
                        json!({"revision_id":id}),
                        Duration::from_secs(5),
                    )
                    .await;
            });
        }
    }
}

pub(crate) struct PreparedReview {
    pub id: Uuid,
    pub context: AutomationReviewContext,
    pub request: AutomationRunRequest,
    pub runner: Arc<RunnerConnection>,
    pub revision: ProjectRevision,
    pub git_source: Option<Value>,
    pub retry_origin: Option<crate::automation_jobs::RetryOrigin>,
    pub snapshot: RevisionLease,
    pub validation: VerifiedValidation,
    pub vault_session: Option<crate::automation_projects::vault::VaultJobSession>,
    pub become_password: Option<Arc<cc_automation_core::ScopedBecomePassword>>,
    pub inventory: Value,
    pub inventory_ref: Option<cc_automation_core::InventoryReference>,
    pub targets: Vec<ApprovedTarget>,
    pub sessions: HashMap<Uuid, Arc<SshSession>>,
    // Bounded original metadata for separate explicit service consent only.
    pub dependency_revisions: Option<BTreeMap<cc_models::ObjectId, i64>>,
    pub dependency_kinds: Option<BTreeMap<cc_models::ObjectId, cc_models::ObjectKind>>,
    pub key_trust_revisions: Option<BTreeMap<cc_models::ObjectId, i64>>,
    pub production_owner: Option<crate::automation_production_owner::FrozenOwnerReview>,
    pub execution_credentials: Option<crate::automation_projects::execution_credentials::Frozen>,
    pub service_origin: Option<cc_crypto_core::ServiceExecutionOrigin>,
    pub service_delegation:
        Option<crate::automation_projects::service_delegation::ServiceDelegationConsent>,
    pub cancellation: CancelOnDrop,
    pub policy: JobPolicy,
    pub expires_at: i64,
    pub deadline: Instant,
    pub _review_slot: OwnedSemaphorePermit,
    pub _hop_slots: OwnedSemaphorePermit,
}

/// A weak unlock epoch is sufficient to invalidate a review. Keeping a review
/// must not keep the vault key or a general credential resolver alive.
pub(crate) struct AutomationReviewContext {
    pub unlock: Weak<crate::session::Unlocked>,
    pub generation: u64,
    pub project_id: String,
    pub project_revision: u64,
    pub folder_composition: Option<crate::automation_projects::folder_bindings::Composition>,
}

impl PreparedReview {
    pub(crate) fn inventory_groups_summary(&self) -> Value {
        if self.inventory_ref.is_none() {
            return self.inventory["groups"].clone();
        }
        let selected = self
            .targets
            .iter()
            .map(|t| t.alias.as_str())
            .collect::<HashSet<_>>();
        self.inventory["groups"].as_object().map(|groups|Value::Object(groups.iter().map(|(name,group)|{
            let hosts=group["hosts"].as_array();
            (name.clone(),json!({"host_count":hosts.map_or(0,Vec::len),"selected_count":hosts.map_or(0,|h|h.iter().filter(|a|a.as_str().is_some_and(|a|selected.contains(a))).count())}))
        }).collect())).unwrap_or_else(||json!({}))
    }
    pub fn usable(&self, generation: u64) -> bool {
        generation == self.context.generation
            && self.context.unlock.strong_count() > 0
            && self.execution_credentials.as_ref().is_none_or(|selected| {
                self.context
                    .unlock
                    .upgrade()
                    .is_some_and(|u| selected.current(&u))
            })
            && !self.cancellation.0.is_cancelled()
            && !self.runner.peer.is_closed()
            && Instant::now() < self.deadline
            && chrono::Utc::now().timestamp() < self.expires_at
    }
    pub fn dto(&self) -> Value {
        // Display only the frozen, authenticated route. Never serialize the
        // ConnectionPlan itself: credentials/proxy secret references belong
        // to native authorization, not to the review presentation.
        let route = |session: &SshSession| {
            let plan = session.connection_plan();
            json!({"host_id":plan.host_id,"name":plan.name,
                "hops":plan.all_hops().iter().map(|hop|json!({"name":hop.name,
                    "endpoint":format!("{}@{}",hop.username,hop.endpoint)})).collect::<Vec<_>>(),
                "proxy":plan.proxy.as_ref().map(|proxy|json!({"name":proxy.name,
                    "kind":proxy.kind,"address":proxy.address,"port":proxy.port}))})
        };
        let origin = match &self.runner.configuration {
            AutomationRunnerConfig::Local {
                launch:
                    HelperLaunch::Wsl {
                        distribution, user, ..
                    },
            } => json!({"kind":"wsl","distribution":distribution,"user":user}),
            AutomationRunnerConfig::Local { .. } => json!({"kind":"local"}),
            AutomationRunnerConfig::Remote { .. } => json!({"kind":"remote",
                "route":self.runner.remote.as_ref().map(|session|route(session))}),
            AutomationRunnerConfig::ServiceRemote { .. } => json!({"kind":"service_remote",
                "route":self.runner.remote.as_ref().map(|session|route(session))}),
        };
        let mut value = json!({"review_id":self.id,"project_id":self.request.project_id,"runner_id":self.request.runner_id,
            "project_revision":self.context.project_revision,
            "inventory_digest":self.validation.inventory_digest(),
            "privilege_review":{"escalation":"not_verified",
                "declarations":self.validation.declared_privileges(),
                "accounts":self.targets.iter().map(|target|json!({"host_id":target.host_id,"username":target.username})).collect::<Vec<_>>()},
            "playbook":self.request.playbook,"mode":self.request.mode,"parameters":self.request.parameters,
            "policy":self.policy,"targets":self.targets,"revision":self.revision,
            "validation":self.validation,"expires_at":self.expires_at,
            "service_origin":self.service_origin,"folder_composition":self.context.folder_composition.as_ref().map(|f|&f.view),"git_source":self.git_source,"retry_origin":self.retry_origin,"inventory_groups":self.inventory_groups_summary(),"origin":origin,"routes":self.targets.iter().filter_map(|target|
                self.sessions.get(&target.host_id).map(|session|route(session))).collect::<Vec<_>>()});
        if let Some(owner) = &self.production_owner {
            value["owner_confirmation"] = owner.view();
        }
        if let Some(selected) = &self.execution_credentials {
            value["deployment_environment"] = json!({"id":selected.environment.id,"name":selected.environment.name,
                "production":self.policy.production,"policy_sha256":selected.selection.frame.policy_sha256,
                "credential_overrides":selected.environment.overrides});
        }
        value["one_shot_become"] = json!(self.become_password.is_some());
        value
    }
}

fn digest(value: &Value) -> AppResult<String> {
    // serde_json maps use sorted keys in this workspace. There are no floating
    // values in this provenance record; parameter witnesses are checked separately.
    Ok(
        Sha256::digest(serde_json::to_vec(value).map_err(AppError::internal)?)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    )
}

impl AppCore {
    /// Performs validation only. A successful response is not permission to run.
    pub async fn automation_prepare_run(&self, request: AutomationRunRequest) -> AppResult<Value> {
        self.automation_prepare_purpose(request, None).await
    }
    /// Secret arguments are separate typed FFI input, never part of launch JSON.
    pub async fn automation_prepare_run_with_inputs(
        &self,
        request: AutomationRunRequest,
        profile_id: String,
        notification_scope_id: String,
        vault_password: Option<String>,
        become_password: Option<String>,
        use_project_vault_password: bool,
    ) -> AppResult<Value> {
        use cc_models::secret::SecretValue;
        let vault_password = vault_password.map(SecretValue::new);
        let become_password = become_password.map(SecretValue::new);
        for value in [vault_password.as_ref(), become_password.as_ref()]
            .into_iter()
            .flatten()
        {
            if value.is_empty()
                || value.len() > 4096
                || value.expose_secret().contains(['\0', '\r', '\n'])
            {
                return Err(invalid("invalid one-shot password"));
            }
        }
        if use_project_vault_password && vault_password.is_some() {
            return Err(invalid("select one Vault password source"));
        }
        if (use_project_vault_password || vault_password.is_some()) && request.parameters.diff {
            return Err(invalid("one-shot Vault jobs prohibit diff"));
        }
        let secrets = LaunchSecrets {
            vault_password,
            become_password: become_password
                .map(|v| cc_automation_core::ScopedBecomePassword::new(v).map(Arc::new))
                .transpose()
                .map_err(automation_error)?,
            use_project_vault_password,
        };
        if secrets.any() && !request.cancel_on_lock {
            return Err(invalid("one-shot passwords require cancellation on lock"));
        }
        if (secrets.vault_password.is_some() || secrets.use_project_vault_password)
            && request.vault_credential_id.is_some()
        {
            return Err(invalid("select one Vault password source"));
        }
        request.validate()?;
        let context = self
            .automation_project_context(&request.project_id)
            .boxed()
            .await?;
        let unlocked = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        if context.owner.profile_id.to_string() != profile_id
            || unlocked.notices.scope_id().as_deref() != Some(notification_scope_id.as_str())
        {
            return Err(invalid("launch password context changed"));
        }
        drop(unlocked);
        context.check_after_io(self).await?;
        self.automation_prepare_context_purpose(context, request, None, secrets)
            .boxed()
            .await
    }

    pub(crate) async fn automation_prepare_service(
        &self,
        request: AutomationRunRequest,
        terms: &crate::automation_projects::service_delegation::NativeServiceTerms,
    ) -> AppResult<Value> {
        let selection =
            crate::automation_projects::service_delegation::ServiceOriginSelection::from_terms(
                terms,
            )?;
        if terms.access_mode != cc_crypto_core::ServiceAccessMode::BoundedOffline
            || request.runner_id != terms.service_id.to_string()
            || request.retry_of.is_some()
            || request.vault_credential_id.is_some()
            || request.vault_selection.is_some()
        {
            return Err(invalid("service preparation unavailable"));
        }
        self.automation_prepare_purpose(request, Some(selection))
            .await
    }

    async fn automation_prepare_purpose(
        &self,
        request: AutomationRunRequest,
        service: Option<crate::automation_projects::service_delegation::ServiceOriginSelection>,
    ) -> AppResult<Value> {
        request.validate()?;
        let context = self
            .automation_project_context(&request.project_id)
            .boxed()
            .await?;
        self.automation_prepare_context_purpose(context, request, service, LaunchSecrets::default())
            .boxed()
            .await
    }

    /// Original-context service path. Never rediscover a current profile after
    /// the explicit public service operation has captured its original owner.
    pub(crate) async fn automation_prepare_service_original(
        &self,
        request: AutomationRunRequest,
        terms: &crate::automation_projects::service_delegation::NativeServiceTerms,
        context: AutomationProjectContext,
    ) -> AppResult<Value> {
        request.validate()?;
        let selection =
            crate::automation_projects::service_delegation::ServiceOriginSelection::from_terms(
                terms,
            )?;
        if request.project_id != context.project.id
            || terms.access_mode != cc_crypto_core::ServiceAccessMode::BoundedOffline
            || request.runner_id != terms.service_id.to_string()
            || request.retry_of.is_some()
            || request.vault_credential_id.is_some()
            || request.vault_selection.is_some()
        {
            return Err(invalid("service preparation unavailable"));
        }
        context.check_after_io(self).await?;
        self.automation_prepare_context_purpose(
            context,
            request,
            Some(selection),
            LaunchSecrets::default(),
        )
        .boxed()
        .await
    }

    async fn automation_prepare_context_purpose(
        &self,
        context: AutomationProjectContext,
        request: AutomationRunRequest,
        service: Option<crate::automation_projects::service_delegation::ServiceOriginSelection>,
        secrets: LaunchSecrets,
    ) -> AppResult<Value> {
        if let (Some(frame), Some(credential)) =
            (&request.vault_selection, &request.vault_credential_id)
        {
            crate::automation_projects::vault::selection::check_run_frame(
                self, &context, frame, credential,
            )
            .boxed()
            .await?;
        }
        let runtime = context.owner.automation.clone();
        let current = runtime.unlocked_generation()?;
        runtime
            .reviews
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|_, review| review.usable(current));
        let _prepare_slot = runtime
            .prepare_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| invalid("preparation limit reached"))?;
        let review_slot = runtime
            .review_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| invalid("close an existing review first"))?;
        let cancellation = CancelOnDrop(CancellationToken::new());
        let cancel = cancellation.0.clone();
        let generation = runtime.unlocked_generation()?;
        let unlocked = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let mut shutdown = unlocked.sharing_shutdown.subscribe();
        drop(unlocked);
        // SSH/auth/sharing each contain sizeable state machines in debug
        // builds. Keep nested operations on the heap instead of embedding all
        // of them in every enclosing select/timeout frame on a 2MiB thread.
        let work = self
            .automation_prepare_inner(
                context,
                request,
                generation,
                cancellation,
                review_slot,
                service,
                secrets,
            )
            .boxed();
        let review = tokio::select! {
            biased;
            _ = shutdown.changed() => { cancel.cancel(); return Err(AppError::VaultLocked); },
            result = tokio::time::timeout(Duration::from_secs(600),work) => result.map_err(|_|invalid("preparation timed out"))??,
        };
        let dto = review.dto();
        let mut reviews = runtime.reviews.lock().unwrap_or_else(|e| e.into_inner());
        let current = runtime.unlocked_generation()?;
        reviews.retain(|_, review| review.usable(current));
        if !review.usable(current) {
            return Err(invalid("review changed during preparation"));
        }
        reviews.insert(review.id, review);
        let expiry_id = dto["review_id"]
            .as_str()
            .and_then(|id| Uuid::parse_str(id).ok())
            .ok_or_else(|| invalid("invalid native review identifier"))?;
        let expiry_owner = Arc::downgrade(&runtime);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(600)).await;
            if let Some(runtime) = expiry_owner.upgrade() {
                runtime
                    .reviews
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&expiry_id);
            }
        });
        Ok(dto)
    }

    async fn automation_prepare_inner(
        &self,
        mut context: AutomationProjectContext,
        mut request: AutomationRunRequest,
        generation: u64,
        cancellation: CancelOnDrop,
        review_slot: OwnedSemaphorePermit,
        service: Option<crate::automation_projects::service_delegation::ServiceOriginSelection>,
        mut secrets: LaunchSecrets,
    ) -> AppResult<PreparedReview> {
        let execution_credentials = crate::automation_projects::execution_credentials::freeze(
            self,
            &context,
            request.deployment_environment.as_ref(),
            &request.host_ids,
        )
        .boxed()
        .await?;
        let mut folder_composition =
            crate::automation_projects::folder_bindings::capture(self, &context)
                .boxed()
                .await?;
        if let Some(folders) = &folder_composition {
            context.inventory = folders.inventory.clone();
        }
        let runtime = context.owner.automation.clone();
        for id in &request.host_ids {
            if !context.inventory.permits(
                Uuid::parse_str(id).map_err(|_| invalid("invalid target"))?,
                context.external_inventory.as_ref(),
            ) {
                return Err(invalid("target is not selected in project inventory"));
            }
        }
        self.automation_initialize_jobs(&context.owner).await?;
        let retry_origin = self
            .automation_retry_origin(&context.owner, &request)
            .await?;
        let runner = runtime
            .connections
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&request.runner_id)
            .cloned()
            .filter(|runner| !runner.peer.is_closed())
            .ok_or_else(|| invalid("connect the selected Runner first"))?;
        if service.is_none()
            && matches!(
                runner.configuration,
                AutomationRunnerConfig::ServiceRemote { .. }
            )
        {
            return Err(invalid(
                "service Runner requires separate delegation review",
            ));
        }
        let cancel = &cancellation.0;
        let raw_probe = runner
            .peer
            .request("probe", json!({}), Duration::from_secs(60))
            .await
            .map_err(automation_error)?;
        if service.is_some()
            && raw_probe["capabilities"]["service_local_executor_v1"] != serde_json::json!(true)
        {
            return Err(invalid(
                "selected Runner is not an enrolled service-local executor",
            ));
        }
        let observed_service_origin = if service.is_some() {
            Some(
                serde_json::from_value::<cc_crypto_core::ServiceExecutionOrigin>(
                    raw_probe["service_origin"].clone(),
                )
                .map_err(|_| invalid("original service origin is unavailable"))?,
            )
        } else {
            None
        };
        let service_helper = raw_probe
            .get("helper_digest")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let probe = EnvironmentProbe::parse(raw_probe).map_err(automation_error)?;
        if service.is_some() && secrets.any() {
            return Err(invalid(
                "one-shot passwords are unavailable for service delegation",
            ));
        }
        if secrets.become_password.is_some()
            && probe.capabilities.get("become_native_v1") != Some(&true)
        {
            return Err(AppError::invalid(
                "automation_runner_update",
                "update the selected Runner for one-shot sudo",
            ));
        }
        probe.require_trusted_worker_bootstrap().map_err(|_| {
            AppError::invalid(
                "automation_runner_update",
                "update the selected Runner to use trusted worker bootstrap",
            )
        })?;
        let local_macos_sandbox = cfg!(target_os = "macos")
            && matches!(&runner.configuration, AutomationRunnerConfig::Local {
                launch: cc_automation_core::HelperLaunch::Native { .. }
            })
            && probe.system == "Darwin"
            && probe.capabilities.get("macos_worker_sandbox_v1") == Some(&true);
        if !probe.dependencies_ready
            || !probe.non_root
            || !probe.sandbox.filesystem
            || !(probe.sandbox.pid || local_macos_sandbox)
            || !probe.sandbox.network
        {
            return Err(invalid("isolated execution environment is unavailable"));
        }
        let unlocked = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let service_origin = if let Some(service) = &service {
            if !matches!(
                runner.configuration,
                AutomationRunnerConfig::Remote { .. }
                    | AutomationRunnerConfig::ServiceRemote { .. }
            ) || probe.system != "Linux"
                || request.runner_id != service.service_id.to_string()
            {
                return Err(invalid("service-local Linux Runner required"));
            }
            let records = unlocked
                .working()
                .service_key_trust_records()
                .ok_or_else(|| invalid("service key trust unavailable"))?;
            let selected = service.origin(
                &runner,
                &probe,
                service_helper
                    .as_deref()
                    .ok_or_else(|| invalid("service helper identity unavailable"))?,
                &records,
            )?;
            if observed_service_origin.as_ref() != Some(&selected) {
                return Err(invalid("service origin differs from Native selection"));
            }
            Some(selected)
        } else {
            None
        };
        let mut watched = BTreeMap::new();
        if let Some(folders) = &mut folder_composition {
            folders.watch(&unlocked, cancel.clone())?;
        }
        let snapshot = unlocked.working().snapshot();
        if !context.external_inventory.as_ref().is_some_and(|config| {
            config.mode == cc_automation_core::InventoryMode::ExternalOnly
        }) {
            crate::automation_projects::folder_bindings::add_host_groups(
                &mut context.inventory, &snapshot, &request.host_ids,
            )?;
        }
        let mut dependency_roots = if execution_credentials.is_some() {
            Vec::new()
        } else {
            request.host_ids.clone()
        };
        if let AutomationRunnerConfig::Remote { host_id, .. }
        | AutomationRunnerConfig::ServiceRemote { host_id, .. } = &runner.configuration
        {
            dependency_roots.push(host_id.clone());
        }
        for id in dependency_roots {
            watched.extend(crate::kubernetes_runtime::dependencies(
                &snapshot,
                crate::dto::parse_id("host_id", &id)?,
            )?);
        }
        if let Some(selected) = &execution_credentials {
            if !selected.current(&unlocked) {
                return Err(invalid("project environment credentials changed"));
            }
            watched.extend(selected.revisions.clone());
        }
        // Classification comes from native inventory, never a client-supplied
        // production=false. Fixed OWNER policy is always required for production.
        let production = crate::automation_projects::execution_credentials::production(
            execution_credentials.as_ref(),
            &snapshot,
            &watched,
        );
        if production && service_origin.is_some() {
            return Err(invalid("production service execution remains unsupported"));
        }
        let dependency_revisions = (watched.len() <= 512).then(|| watched.clone());
        let dependency_kinds = dependency_revisions
            .as_ref()
            .map(|original| {
                original
                    .keys()
                    .map(|id| {
                        snapshot
                            .get(id)
                            .map(|entry| (*id, entry.kind()))
                            .ok_or_else(|| invalid("connection dependency changed"))
                    })
                    .collect::<AppResult<BTreeMap<_, _>>>()
            })
            .transpose()?;
        drop(snapshot);
        if !unlocked.working().watch_revisions(watched, cancel.clone()) {
            return Err(AppError::VaultLocked);
        }
        drop(unlocked);
        request.host_ids.sort();
        let mut targets = Vec::new();
        let mut sessions = HashMap::new();
        let mut plans = Vec::new();
        let mut hops = 0usize;
        for id in &request.host_ids {
            let mut prepared = if let Some(selected) = &execution_credentials {
                let original = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
                if !selected.current(&original) {
                    return Err(invalid("project environment credentials changed"));
                }
                let host = crate::dto::parse_id("host_id", id)?;
                let plan = selected
                    .plans
                    .get(&host)
                    .cloned()
                    .ok_or_else(|| invalid("project environment route missing"))?;
                crate::sharing_bindings::PreparedSharingConnection {
                    unlocked: original,
                    plan,
                }
            } else if service_origin.is_some() {
                // Service purpose admits only original personal/local routes.
                // The generic sharing refresh helper discovers CURRENT scope
                // and may perform online I/O, so it cannot be used here.
                let original = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
                {
                    let _profile = self.hold_session(&context.owner).await?;
                    let _epoch = context.owner.hold_unlocked(&original).await?;
                }
                let host_id = crate::dto::parse_id("host_id", id)?;
                let target = crate::inventory::host_of(&original, host_id)?;
                if original.working().vault_of(host_id).is_some()
                    || crate::sharing_bindings::host_sharing_binding(&target)?.is_some()
                {
                    return Err(invalid("service requires personal target routes"));
                }
                // Planning reads metadata from this captured working set only.
                let plan = original.ssh.plan(host_id).await?;
                for hop in plan.all_hops() {
                    let host = crate::inventory::host_of(&original, hop.host_id)?;
                    if original.working().vault_of(hop.host_id).is_some()
                        || crate::sharing_bindings::host_sharing_binding(&host)?.is_some()
                        || hop.credential.as_ref().is_some_and(|credential| {
                            original.working().vault_of(credential.id).is_some()
                        })
                    {
                        return Err(invalid("service requires personal target routes"));
                    }
                }
                {
                    let _profile = self.hold_session(&context.owner).await?;
                    let _epoch = context.owner.hold_unlocked(&original).await?;
                }
                crate::sharing_bindings::PreparedSharingConnection {
                    unlocked: original,
                    plan,
                }
            } else {
                self.sharing_prepare_connection(id).boxed().await?
            };
            let original = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
            if !Arc::ptr_eq(&original, &prepared.unlocked) || cancel.is_cancelled() {
                return Err(AppError::VaultLocked);
            }
            if execution_credentials
                .as_ref()
                .is_some_and(|selected| !selected.current(&original))
            {
                return Err(invalid("project environment credentials changed"));
            }
            if prepared.plan.backend != SshBackend::Native {
                return Err(invalid("target requires native SSH"));
            }
            for hop in prepared.plan.all_hops() {
                if crate::inventory::host_of(&original, hop.host_id)?.backend != SshBackend::Native
                {
                    return Err(invalid("jump requires native SSH"));
                }
            }
            // A dedicated copy grants exec/SFTP only. Saved terminal forwards
            // and agent forwarding are not started or changed in the vault.
            if service_origin.is_some()
                && (prepared.plan.proxy.is_some()
                    || prepared.plan.all_hops().iter().any(|hop| {
                        hop.host_key_policy != cc_models::host::HostKeyPolicy::Strict
                            || hop.credential.as_ref().is_none_or(|c| {
                                c.kind != cc_models::credential::CredentialKind::Password
                            })
                    }))
            {
                return Err(invalid(
                    "service requires pinned host keys and explicit password capsules",
                ));
            }
            prepared.plan.forwards.clear();
            prepared.plan.agent_forwarding = false;
            hops = hops
                .checked_add(prepared.plan.all_hops().len())
                .ok_or_else(|| invalid("route budget exceeded"))?;
            plans.push(prepared);
        }
        let hop_slots = runtime
            .hop_slots
            .clone()
            .try_acquire_many_owned(
                hops.try_into()
                    .map_err(|_| invalid("route budget exceeded"))?,
            )
            .map_err(|_| invalid("route budget exceeded"))?;
        let diagnostic_capture = Arc::new(cc_ssh_core::DiagnosticCapture::new());
        for prepared in plans {
            if let Some(selected) = &execution_credentials {
                if !selected.current(&prepared.unlocked) {
                    return Err(invalid("project environment credentials changed"));
                }
            }
            let first = prepared
                .plan
                .all_hops()
                .into_iter()
                .next()
                .ok_or_else(|| invalid("empty target route"))?;
            let connector = prepared
                .unlocked
                .ssh
                .connector
                .clone()
                .with_diagnostic_capture(diagnostic_capture.clone());
            let connect = async {
                let session = match &runner.configuration {
                    AutomationRunnerConfig::Local {
                        launch: HelperLaunch::Wsl { .. },
                    } => {
                        if prepared.plan.proxy.is_some() {
                            return Err(invalid("proxy over WSL transport is not supported"));
                        }
                        let AutomationRunnerConfig::Local { launch } = &runner.configuration else {
                            unreachable!()
                        };
                        let stream = launch
                            .wsl_origin_stream(
                                &first.endpoint.host,
                                first.endpoint.port,
                                &probe.environment_digest,
                            )
                            .await
                            .map_err(automation_error)?;
                        let origin = stream.scope_digest();
                        connector
                            .connect_over_stream(&prepared.plan, stream, origin).boxed()
                            .await?
                    }
                    AutomationRunnerConfig::Local { .. } => {
                        connector.connect(&prepared.plan).boxed().await?
                    }
                    AutomationRunnerConfig::Remote { .. } | AutomationRunnerConfig::ServiceRemote { .. } => {
                        if prepared.plan.proxy.is_some() {
                            return Err(invalid(
                                "proxy over remote Runner transport is not supported",
                            ));
                        }
                        let remote = runner
                            .remote
                            .as_ref()
                            .filter(|remote| !remote.is_closed())
                            .ok_or_else(|| invalid("Runner SSH channel closed"))?;
                        let origin = if let Some(origin) = &service_origin {
                            origin.stream_scope(&first.endpoint.host, first.endpoint.port).map_err(|_|invalid("service origin unavailable"))?
                        } else {
                            Sha256::digest(serde_json::to_vec(&json!({"runner":runner.id,"route":remote.connection_plan(),
                            "origin":remote.network_origin(),"environment":probe.environment_digest,"target":first.endpoint})).map_err(AppError::internal)?).into()
                        };
                        let stream = remote
                            .open_direct_tcpip(&first.endpoint.host, first.endpoint.port, None)
                            .await?;
                        connector
                            .connect_over_stream(&prepared.plan, stream, origin).boxed()
                            .await?
                    }
                };
                Ok::<_, AppError>(Arc::new(session))
            }.boxed();
            let session = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(invalid("target configuration changed")),
                result = tokio::time::timeout(Duration::from_secs(120),prepared.run(connect)) => result.map_err(|_|invalid("target connection timed out"))??,
            };
            let owned_session = session.clone();
            let revoked = cancel.clone();
            let stopped = runtime.stopped.clone();
            tokio::spawn(async move {
                tokio::select! { _=revoked.cancelled()=>{}, _=stopped.cancelled()=>{} }
                let _ = owned_session.disconnect().await;
            });
            let alias = format!("cc_{}", prepared.plan.host_id.to_string().replace('-', "_"));
            let target = target_from_session(alias, &session).map_err(automation_error)?;
            sessions.insert(target.host_id, session);
            targets.push(target);
        }
        let unlocked = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let trust: BTreeMap<cc_models::ObjectId, i64> = unlocked
            .working()
            .snapshot()
            .into_iter()
            .filter(|(_, e)| e.kind() == ObjectKind::KnownHost)
            .map(|(id, e)| (id, e.revision))
            .collect();
        let key_trust_revisions = dependency_revisions
            .as_ref()
            .filter(|deps| !trust.is_empty() && deps.len() + trust.len() <= 512)
            .map(|_| trust.clone());
        if !unlocked.working().watch_revisions(trust, cancel.clone()) {
            return Err(AppError::VaultLocked);
        }
        let _profile = self.hold_session(&context.owner).await?;
        let _epoch = context.owner.hold_unlocked(&unlocked).await?;
        drop(_epoch);
        drop(_profile);
        drop(unlocked);
        let source = context.root_selection.clone();
        let git_source = context.project.git_source.clone();
        let expected_git_metadata = context.git_metadata_sha256.clone();
        let retained_io = crate::automation_projects::managed::admit_io(self, &context).await?;
        let io_slot = retained_io.clone();
        let (root, revision) = tokio::task::spawn_blocking(move || -> AppResult<_> {
            let _io_slot = io_slot;
            let root = Arc::new(source.open()?);
            if let Some(expected) = expected_git_metadata {
                if root
                    .git_source()
                    .map_err(automation_error)?
                    .map(|(_, stamp)| stamp)
                    != Some(expected)
                {
                    return Err(automation_error(
                        cc_automation_core::AutomationError::Invalid("Git source metadata changed"),
                    ));
                }
            }
            let revision = root.revision().map_err(automation_error)?;
            root.recheck()?;
            Ok((root, revision))
        })
        .await
        .map_err(|_| invalid("source scan stopped"))??;
        context.check_after_io(self).await?;
        drop(retained_io);
        probe.require_project_capabilities(&revision).map_err(|_| {
            AppError::invalid(
                "automation_runner_update",
                "selected Runner must validate declared project inputs",
            )
        })?;
        probe.require_target_match().map_err(|_| {
            AppError::invalid(
                "automation_runner_update",
                "selected Runner must validate playbook target matching",
            )
        })?;
        if !revision
            .entries
            .iter()
            .any(|entry| entry.path() == request.playbook)
        {
            return Err(invalid("selected playbook does not exist"));
        }
        if let Some(frame) = &request.vault_selection {
            frame.matches_tree(&root, &revision)?;
        }
        let has_vault_reference = revision
            .entries
            .iter()
            .any(|entry| entry.path() == cc_automation_core::VAULT_INPUT_FILE);
        let ephemeral_vault =
            secrets.vault_password.is_some() || secrets.use_project_vault_password;
        if (has_vault_reference != request.vault_credential_id.is_some())
            || (ephemeral_vault && has_vault_reference)
        {
            return Err(invalid(
                "explicit Ansible Vault selection must match the project reference",
            ));
        }
        let vault_session = if ephemeral_vault {
            if probe.capabilities.get("vault_ephemeral_v1") != Some(&true) {
                return Err(AppError::invalid(
                    "automation_runner_update",
                    "update the selected Runner for one-shot Vault",
                ));
            }
            let password = if secrets.use_project_vault_password {
                self.automation_project_vault_file_password(
                    context.clone(),
                    root.clone(),
                    revision.clone(),
                    cancel,
                )
                .await?
            } else {
                secrets
                    .vault_password
                    .take()
                    .ok_or_else(|| invalid("one-shot Vault input unavailable"))?
            };
            let session = self
                .automation_open_ephemeral_vault_job_session(
                    context.clone(),
                    revision.digest.clone(),
                    password,
                    runner.id.clone(),
                    runner.revision,
                    request.playbook.clone(),
                    root.clone(),
                    revision.clone(),
                    cancel,
                )
                .await?;
            request.parameters.vault = Some(session.binding()?);
            Some(session)
        } else if let Some(credential) = request.vault_credential_id.clone() {
            if probe.capabilities.get("vault_job_v1") != Some(&true) {
                return Err(AppError::invalid(
                    "automation_runner_update",
                    "update the selected Runner for Ansible Vault jobs",
                ));
            }
            let session = self
                .automation_open_vault_job_session(
                    request.project_id.clone(),
                    context.project.revision.unwrap_or_default(),
                    revision.digest.clone(),
                    credential,
                    runner.id.clone(),
                    runner.revision,
                    request.playbook.clone(),
                    root.clone(),
                    revision.clone(),
                    cancel,
                )
                .await?;
            request.parameters.vault = Some(session.binding()?);
            Some(session)
        } else {
            None
        };
        let peer = runner.peer.clone();
        let upload_root = root.clone();
        let upload_revision = revision.clone();
        let upload_cancel = cancel.clone();
        let handle = tokio::runtime::Handle::current();
        let io_slot = automation_io_slot(&context.owner)?;
        let uploaded=tokio::task::spawn_blocking(move || {let _io_slot=io_slot;handle.block_on(async move {
            upload_root.recheck()?;
            let uploaded=tokio::select!{biased;_=upload_cancel.cancelled()=>Err(cc_automation_core::AutomationError::Closed),
                result=upload_root.upload(&upload_revision,&peer)=>result}.map_err(automation_error)?;
            upload_root.recheck()?;
            Ok::<_,AppError>(uploaded)
        })}).await.map_err(|_|invalid("snapshot transfer stopped"))??;
        let lease = RevisionLease {
            uploaded,
            peer: Arc::downgrade(&runner.peer),
        };
        let (inventory, artifact) = if let Some(config) = context.external_inventory {
            let io_slot = automation_io_slot(&context.owner)?;
            let handle = tokio::runtime::Handle::current();
            let peer = runner.peer.clone();
            let captured = lease.uploaded.clone();
            let source_revision = revision.clone();
            let cancellation = cancel.clone();
            let mut approved = targets.clone();
            let groups = context.inventory;
            let result = tokio::task::spawn_blocking(move || {
                let _io_slot = io_slot;
                handle.block_on(async move {
                    let source = cc_automation_core::CapturedInventory::fetch(
                        &peer,
                        &captured,
                        &source_revision,
                        &config.sources,
                        &cancellation,
                    )
                    .await?;
                    let inventory = serde_json::to_value(config.runtime_snapshot(
                        &source.inventory,
                        &groups,
                        &mut approved,
                    )?)
                    .map_err(|_| {
                        cc_automation_core::AutomationError::Invalid("invalid merged inventory")
                    })?;
                    let artifact = cc_automation_core::InventoryArtifact::store(
                        &peer,
                        &captured,
                        &inventory,
                        &cancellation,
                    )
                    .await?;
                    Ok::<_, cc_automation_core::AutomationError>((inventory, artifact, approved))
                })
            })
            .await
            .map_err(|_| invalid("inventory worker stopped"))?
            .map_err(automation_error)?;
            targets = result.2;
            (result.0, Some(result.1))
        } else {
            let inventory_groups = context
                .inventory
                .runtime_groups(&targets)
                .map_err(automation_error)?;
            (
                json!({"version":1,"hosts":targets.iter().map(|target|json!({"alias":target.alias,"host_id":target.host_id,
                "address":target.address,"port":target.port,"username":target.username,"variables":{},"sources":["saved_hosts"],
                "credential_refs":target.credential_refs})).collect::<Vec<_>>(),"groups":inventory_groups,"source_digests":{"saved_hosts":digest(&json!(targets))?,"project_groups":digest(&json!(context.inventory))?}}),
                None,
            )
        };
        let inventory_ref = artifact.as_ref().map(|artifact| artifact.reference.clone());
        let mut body = json!({"revision_id":lease.uploaded.revision_id,"playbook":request.playbook,
            "parameters":request.parameters,"targets":targets});
        if let Some(reference) = &inventory_ref {
            body["inventory_ref"] = json!(reference);
        } else {
            body["inventory"] = inventory.clone();
        }
        // Uploaded/merged artifact still represents this exact folder proposal.
        // Repeat metadata/composition before allowing the validation RPC itself.
        {
            let u = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
            let _profile = self.hold_session(&context.owner).await?;
            let _epoch = context.owner.hold_unlocked(&u).await?;
            if let Some(folders) = &folder_composition {
                folders.check(&u)?;
            }
            crate::automation_projects::folder_bindings::confirm_metadata(
                &context.owner,
                &context.project.id,
                context.project.revision.unwrap_or_default(),
                folder_composition.as_ref(),
            )
            .boxed()
            .await?;
        }
        let mut raw = tokio::select! { biased; _=cancel.cancelled()=>return Err(invalid("target configuration changed")),
        result=runner.peer.request("prepare_run",body,Duration::from_secs(240))=>result.map_err(automation_error)?, };
        if let Some(artifact) = artifact {
            if raw.pointer("/input_witnesses/inventory")
                != Some(&json!({"digest":artifact.reference.digest,"reference":artifact.reference}))
            {
                return Err(invalid("validation inventory reference changed"));
            }
            // Only native receives these private canonical bytes. The bridge,
            // persisted history and signed manifest contain no full variables.
            raw["input_witnesses"]["inventory"] =
                serde_json::to_value(artifact.witness).map_err(AppError::internal)?;
        }
        let proof: ValidationProof =
            serde_json::from_value(raw).map_err(|_| invalid("invalid validation proof"))?;
        let validation = proof
            .verify(
                &revision.digest,
                &probe.environment_digest,
                &request.playbook,
                &request.parameters,
                &inventory,
                &targets,
            )
            .map_err(automation_error)?;
        let validation = if let Some(session) = &vault_session {
            session.validated(validation).await?
        } else {
            validation
        };
        let reread = root.clone();
        let io_slot = automation_io_slot(&context.owner)?;
        let current = tokio::task::spawn_blocking(move || {
            let _io_slot = io_slot;
            let revision = reread.revision().map_err(automation_error)?;
            reread.recheck()?;
            Ok::<_, AppError>(revision)
        })
        .await
        .map_err(|_| invalid("source scan stopped"))??;
        if current.digest != revision.digest || cancel.is_cancelled() {
            return Err(invalid("source or targets changed during validation"));
        }
        {
            let u = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
            let _profile = self.hold_session(&context.owner).await?;
            let _epoch = context.owner.hold_unlocked(&u).await?;
            if let Some(folders) = &folder_composition {
                folders.check(&u)?;
            }
            crate::automation_projects::folder_bindings::confirm_metadata(
                &context.owner,
                &context.project.id,
                context.project.revision.unwrap_or_default(),
                folder_composition.as_ref(),
            )
            .boxed()
            .await?;
        }
        let now = chrono::Utc::now().timestamp();
        let mut review = PreparedReview {
            id: Uuid::new_v4(),
            context: AutomationReviewContext {
                unlock: context.unlock,
                generation,
                project_id: request.project_id.clone(),
                project_revision: context.project.revision.unwrap_or_default(),
                folder_composition,
            },
            policy: JobPolicy {
                strict_isolation: true,
                cancel_on_lock: request.cancel_on_lock,
                timeout_seconds: request.timeout_seconds,
                forks: request.forks,
                production,
            },
            request,
            runner,
            revision,
            git_source,
            retry_origin,
            snapshot: lease,
            validation,
            vault_session,
            become_password: secrets.become_password,
            inventory,
            inventory_ref,
            targets,
            sessions,
            dependency_revisions,
            dependency_kinds,
            key_trust_revisions,
            service_origin,
            service_delegation: None,
            production_owner: None,
            execution_credentials,
            cancellation,
            expires_at: now + 600,
            deadline: Instant::now() + Duration::from_secs(600),
            _review_slot: review_slot,
            _hop_slots: hop_slots,
        };
        if production && review.validation.is_valid() {
            let grant = self
                .automation_authorize_connection(&context.owner, &review)
                .await?;
            let identity = context.owner.identity();
            if identity.public_keys().signing != grant.public_key
                || identity.device_id().to_string() != grant.signer_id.to_string()
            {
                return Err(invalid("original manual owner key changed"));
            }
            let manual =
                cc_automation_core::ManualOwnerGrant::configure(&review.runner.peer, grant.clone())
                    .await
                    .map_err(automation_error)?;
            let original = review
                .context
                .unlock
                .upgrade()
                .ok_or(AppError::VaultLocked)?;
            let _profile = self.hold_session(&context.owner).await?;
            let _epoch = context.owner.hold_unlocked(&original).await?;
            if !review.usable(context.owner.automation.unlocked_generation()?) {
                return Err(invalid("review changed before production manifest freeze"));
            }
            let scope = original.notices.scope_id().ok_or(AppError::VaultLocked)?;
            review.production_owner = Some(
                crate::automation_production_owner::FrozenOwnerReview::freeze(
                    &review,
                    manual,
                    &grant,
                    scope,
                    chrono::Utc::now().timestamp(),
                )?,
            );
        }
        Ok(review)
    }

    pub async fn automation_discard_review(&self, review_id: String) -> AppResult<()> {
        let id = Uuid::parse_str(&review_id).map_err(|_| invalid("invalid review identifier"))?;
        let (owner, unlocked) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _epoch = owner.hold_unlocked(&unlocked).await?;
        owner
            .automation
            .reviews
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
        Ok(())
    }
}
