//! Private owner delegation from an ACTUAL one-use execution PreparedReview.
//! Separate consent/purpose from a job approval. No Bridge or service activation.
use super::*;
mod owner_ledger;
mod schedules;
mod public;
use crate::automation_prepare::PreparedReview;
use crate::service_credential_issuer::OwnerScope;
use crate::session::Unlocked;
use crate::working_set::VaultCredentialMetadata;
use cc_automation_core::target_from_session;
use cc_crypto_core::{
    ServiceAccessMode, ServiceAccessPolicy, ServiceDelegationPolicy, ServicePolicyCredentialRef,
    ServicePolicyHostRef, ServicePolicyKey, ServicePolicyQuotas, ServicePolicyTenant,
    ServiceRouteInputRef, ServiceRouteInputs, ServiceTenantKind, SignedServiceDelegation,
};
use cc_models::ObjectId;
use cc_storage_core::ProfileKind;
use futures::FutureExt;
use sha2::Digest;
use std::time::{Duration, Instant};
use tokio::sync::OwnedSemaphorePermit;

fn invalid() -> AppError {
    AppError::invalid("service_delegation", "service_delegation_unavailable")
}
fn now() -> AppResult<u64> {
    chrono::Utc::now()
        .timestamp()
        .try_into()
        .map_err(|_| invalid())
}

/// Trusted Native policy/enrollment data, not Deserialize/request fields.
/// The service signer/recipient and schedule must already be explicitly selected
/// for this exact offer. Owner epochs/highwaters need the actual persistent
/// owner-ledger adapter before public activation; this private issuer does not
/// accept guest values or claim to implement that missing enrollment adapter.
pub(crate) struct NativeServiceTerms {
    pub service_id: Uuid,
    pub signer: ServicePolicyKey,
    pub recipient: ServicePolicyKey,
    pub owner_epoch: u64,
    pub policy_revision: u64,
    pub revocation_epoch: u64,
    pub signer_not_after: u64,
    pub recipient_not_after: u64,
    pub schedule_id: Uuid,
    pub schedule_revision: u64,
    pub definition_digest: String,
    pub validity_seconds: u32,
    pub access_mode: ServiceAccessMode,
    pub quotas: ServicePolicyQuotas,
}
/// Only the Native selected service identity, never a request boolean/digest.
#[derive(Clone)]
pub(crate) struct ServiceOriginSelection {
    pub(crate) service_id: Uuid,
    signer: String,
    recipient: String,
}
impl ServiceOriginSelection {
    pub(crate) fn from_terms(terms: &NativeServiceTerms) -> AppResult<Self> {
        let key_hash = |text: &str| -> AppResult<String> {
            let bytes = hex::decode(text).map_err(|_| invalid())?;
            if bytes.len() != 32 || text != hex::encode(&bytes) {
                return Err(invalid());
            }
            Ok(hex::encode(sha2::Sha256::digest(bytes)))
        };
        if terms.service_id.is_nil()
            || terms.signer.kind != "ed25519"
            || terms.recipient.kind != "x25519"
        {
            return Err(invalid());
        }
        Ok(Self {
            service_id: terms.service_id,
            signer: key_hash(&terms.signer.public_key)?,
            recipient: key_hash(&terms.recipient.public_key)?,
        })
    }
    pub(crate) fn origin(
        &self,
        runner: &crate::automation_runners::RunnerConnection,
        probe: &cc_automation_core::EnvironmentProbe,
        helper: &str,
        keys: &[cc_models::known_host::KnownHost],
    ) -> AppResult<cc_crypto_core::ServiceExecutionOrigin> {
        let result = cc_crypto_core::ServiceExecutionOrigin {
            version: 1,
            service_id: self.service_id.to_string(),
            runner_revision: runner.revision,
            runner_configuration_digest: hex::encode(sha2::Sha256::digest(
                serde_json::to_vec(&runner.configuration).map_err(AppError::internal)?,
            )),
            signer_key_sha256: self.signer.clone(),
            recipient_key_sha256: self.recipient.clone(),
            environment_digest: probe.environment_digest.clone(),
            helper_digest: helper.into(),
            known_hosts_digest: cc_automation_core::service_known_hosts_digest(keys)
                .map_err(|_| invalid())?,
            backend: "linuxNativeSsh".into(),
            network: "serviceLocal".into(),
        };
        result.validate().map_err(|_| invalid())?;
        Ok(result)
    }
}

#[derive(Serialize)]
pub(crate) struct ConfirmedServiceDelegation {
    pub delegation: SignedServiceDelegation,
    pub initial_status: cc_crypto_core::SignedInitialServiceStatus,
    // Keep this actual remote revision pinned until service enrollment ACK (or
    // original peer shutdown). Neither a serialized policy nor a journal may
    // reconstruct this lease.
    #[serde(skip)]
    pub(crate) template: ServicePreparedTemplate,
}

pub(crate) struct ServicePreparedTemplate {
    public_setup: Option<public::PublicSetup>,
    owner_scope: Arc<OwnerScope>,
    namespace: Option<cc_automation_core::ServiceLocalNamespace>,
    pub snapshot: crate::automation_prepare::RevisionLease,
    pub inventory: Value,
    pub parameters: cc_automation_core::RunParameters,
    pub targets: Vec<cc_models::automation::ApprovedTarget>,
    pub plans: Vec<cc_ssh_core::ConnectionPlan>,
    pub known_hosts: Vec<cc_models::known_host::KnownHost>,
    owner: Arc<crate::session::Session>,
    unlock: Weak<Unlocked>,
    generation: u64,
    scope: String,
    deadline: Instant,
    credential_metadata: BTreeMap<ObjectId, VaultCredentialMetadata>,
    runner: Arc<crate::automation_runners::RunnerConnection>,
    revision: cc_automation_core::ProjectRevision,
    _cancellation: crate::automation_prepare::CancelOnDrop,
}
impl ConfirmedServiceDelegation {
    pub(crate) fn enrollment(&self) -> AppResult<Value> {
        Ok(
            serde_json::json!({"delegation":self.delegation,"initial_status":self.initial_status,
            "revision_id":self.template.snapshot.uploaded.revision_id,"inventory":self.template.inventory,
            "parameters":self.template.parameters,"targets":self.template.targets}),
        )
    }

    fn original_held(&self, u: &Unlocked) -> AppResult<()> {
        let t = &self.template;
        let p = self.delegation.body();
        self.template.owner_scope.check(&t.owner, u)?;
        if let Some(setup) = &t.public_setup {
            setup.check(&t.owner, u)?;
        }
        if Instant::now() >= t.deadline
            || t._cancellation.0.is_cancelled()
            || t.owner.automation.stopped.is_cancelled()
            || *u.sharing_shutdown.borrow()
            || t.owner.automation.unlocked_generation()? != t.generation
            || u.notices.scope_id().as_deref() != Some(t.scope.as_str())
            || t.owner.device_id().to_string() != p.owner_id
            || t.runner.peer.is_closed()
            || p.execution_origin
                .as_ref()
                .is_none_or(|origin| origin.runner_revision != t.runner.revision)
            || t.credential_metadata.iter().any(|(id, expected)| {
                u.working().vault_credential_metadata(*id).as_ref() != Some(expected)
            })
        {
            return Err(invalid());
        }
        let keys = u
            .working()
            .service_key_trust_records()
            .ok_or_else(invalid)?;
        if cc_automation_core::service_known_hosts_digest(&keys).map_err(|_| invalid())?
            != p.execution_origin
                .as_ref()
                .ok_or_else(invalid)?
                .known_hosts_digest
        {
            return Err(invalid());
        }
        Ok(())
    }

    /// The explicit service consent is consumed once. Passwords are sealed by
    /// the existing issuer; no secret is serialized or sent in ordinary JSON.
    pub(crate) async fn activate(self, app: &AppCore) -> AppResult<Value> {
        let t = &self.template;
        let owner = t.owner.clone();
        let weak = t.unlock.clone();
        let u = weak.upgrade().ok_or(AppError::VaultLocked)?;
        {
            let _profile = app.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&u).await?;
            self.original_held(&u)?;
        }
        drop(u);
        t.owner_scope.verify(&owner, weak.clone()).await?;
        let policy = self.delegation.body();
        let digest = self.delegation.digest().map_err(|_| invalid())?;
        let recipient: [u8; 32] = hex::decode(&policy.recipient.public_key)
            .map_err(|_| invalid())?
            .try_into()
            .map_err(|_| invalid())?;
        let mut capsules = Vec::new();
        for selected in &policy.credentials {
            let review = app
                .service_credential_review_original(
                    owner.profile_id.to_string(),
                    selected.id.clone(),
                    crate::service_credential_issuer::ServiceRecipientSelection {
                        service_id: policy.service_id.parse().map_err(|_| invalid())?,
                        recipient_public_key: recipient,
                        recipient_key_epoch: policy.recipient.key_epoch,
                        recipient_certificate_epoch: policy.recipient.certificate_epoch,
                        delegation_digest: digest.clone(),
                        policy_revision: policy.policy_revision,
                        revocation_epoch: policy.revocation_epoch,
                        delegation_expires_at: policy.expires_at,
                    },
                    &t._cancellation.0,
                    owner.clone(),
                    weak.clone(),
                )
                .await?;
            let binding = review.binding().clone();
            if binding.credential_revision != selected.revision {
                return Err(invalid());
            }
            let capsule = review.approve_and_seal(app).await?;
            capsules.push(cc_automation_core::ServiceCapsuleInput { binding, capsule });
            let u = weak.upgrade().ok_or(AppError::VaultLocked)?;
            let _profile = app.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&u).await?;
            self.original_held(&u)?;
        }
        let context = app
            .automation_project_context_original(&policy.project_id, owner.clone(), weak.clone())
            .await?;
        if context.project.revision != Some(policy.project_revision) {
            return Err(invalid());
        }
        let io = managed::admit_io(app, &context).await?;
        let selection = context.root_selection.clone();
        let path = context.path.clone();
        let expected = t.revision.clone();
        let fence = ServiceFsFence {
            registry_epoch: owner.automation.dependencies.local_filesystem_epoch()?,
            shutdown: weak
                .upgrade()
                .ok_or(AppError::VaultLocked)?
                .sharing_shutdown
                .subscribe(),
            caller: t._cancellation.0.clone(),
            deadline: t.deadline,
        };
        let pinned = tracked_source(&owner, fence, move |fence| {
            let root = selection.open()?;
            root.recheck()?;
            root.reopen_same_directory(&path).map_err(|_| invalid())?;
            let actual = root.revision().map_err(|_| invalid())?;
            root.recheck()?;
            root.reopen_same_directory(&path).map_err(|_| invalid())?;
            fence.check()?;
            if actual != expected {
                return Err(invalid());
            }
            Ok(SourcePin { root, _io: io })
        })
        .await?;
        context.check_after_io(app).await?;
        let u = weak.upgrade().ok_or(AppError::VaultLocked)?;
        {
            let _profile = app.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&u).await?;
            self.original_held(&u)?;
        }
        drop(u);
        let body = serde_json::json!({"template":self.enrollment()?,"plans":t.plans,"known_hosts":t.known_hosts,"capsules":capsules});
        // The original service Rust process owns its keys, same-root snapshot
        // pin and JobScopeRegistry. Default job helpers reject this endpoint.
        // Once this send begins, a lost ACK is uncertain export/enrollment;
        // cancellation cannot honestly claim that no durable mutation occurred.
        let pending = if let Some(original) = &t.namespace {
            let current = observed_namespace(
                &t.runner,
                &policy.signer.public_key,
                &policy.recipient.public_key,
            )
            .await?;
            if current.as_ref() != Some(original) {
                return Err(invalid());
            }
            Some(owner_ledger::PendingDelivery::before_send(app, &self, original).await?)
        } else {
            None
        }; // Existing ephemeral PRIVATE proof remains unchanged.
        {
            let u = weak.upgrade().ok_or(AppError::VaultLocked)?;
            let _profile = app.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&u).await?;
            self.original_held(&u)?;
        }
        let result = t
            .runner
            .peer
            .request("service_enroll", body, Duration::from_secs(30))
            .await;
        drop(pinned);
        let unknown = || AppError::invalid("service_activation", "service_enrollment_unknown");
        let result = result.map_err(|_| unknown())?;
        if result.get("enrolled") != Some(&serde_json::json!(true))
            || result.get("delegationDigest") != Some(&serde_json::json!(digest))
            || result.get("revisionId") != Some(&serde_json::json!(t.snapshot.uploaded.revision_id))
        {
            return Err(unknown());
        }
        {
            let u = weak.upgrade().ok_or_else(unknown)?;
            let _profile = app.hold_session(&owner).await.map_err(|_| unknown())?;
            let _unlock = owner.hold_unlocked(&u).await.map_err(|_| unknown())?;
            self.original_held(&u).map_err(|_| unknown())?;
        }
        if let Some(pending) = pending {
            pending.acknowledge(app, &result).await?;
        }
        Ok(result)
    }
}

struct SourcePin {
    root: managed::OpenedProjectRoot,
    _io: Arc<OwnedSemaphorePermit>, // Dropped AFTER the actual root handle.
}

/// Metadata-only consent in the EXISTING review registry. Lock/discard drops
/// it with the original prepared review. No separate root/IO/holder is idle.
pub(crate) struct ServiceDelegationConsent {
    public_setup: Option<public::PublicSetup>,
    working_version: u64,
    owner_scope: Arc<OwnerScope>,
    namespace: Option<cc_automation_core::ServiceLocalNamespace>,
    policy: ServiceDelegationPolicy,
    scope: String,
    owner_signing: [u8; 32],
    credential_metadata: BTreeMap<ObjectId, VaultCredentialMetadata>,
    deadline: Instant,
}
#[derive(Clone)]
struct ServiceFsFence {
    registry_epoch: tokio_util::sync::CancellationToken,
    shutdown: tokio::sync::watch::Receiver<bool>,
    caller: tokio_util::sync::CancellationToken,
    deadline: Instant,
}
impl ServiceFsFence {
    fn check(&self) -> AppResult<()> {
        if self.registry_epoch.is_cancelled()
            || *self.shutdown.borrow()
            || self.caller.is_cancelled()
            || Instant::now() >= self.deadline
        {
            Err(AppError::Cancelled)
        } else {
            Ok(())
        }
    }
}
async fn tracked_source<T: Send + 'static>(
    owner: &Arc<crate::session::Session>,
    fence: ServiceFsFence,
    task: impl FnOnce(&ServiceFsFence) -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    fence.check()?;
    let (send, receive) = tokio::sync::oneshot::channel();
    owner.automation.dependencies.launch_worker(move || {
        let result = fence.check().and_then(|_| task(&fence)).and_then(|value| {
            fence.check()?;
            Ok(value)
        });
        // Failed receive destroys its actual root/IO payload before retirement.
        drop(send.send(result));
    })?;
    receive.await.map_err(|_| invalid())?
}
async fn observed_namespace(
    runner: &crate::automation_runners::RunnerConnection,
    signing: &str,
    recipient: &str,
) -> AppResult<Option<cc_automation_core::ServiceLocalNamespace>> {
    let offer = runner
        .peer
        .request(
            "service_offer",
            serde_json::json!({}),
            Duration::from_secs(30),
        )
        .await
        .map_err(|_| invalid())?;
    if offer.get("signing").and_then(Value::as_str) != Some(signing)
        || offer.get("recipient").and_then(Value::as_str) != Some(recipient)
    {
        return Err(invalid());
    }
    let Some(raw) = offer.get("persistentNamespace") else {
        return Ok(None);
    };
    let namespace: cc_automation_core::ServiceLocalNamespace =
        serde_json::from_value(raw.clone()).map_err(|_| invalid())?;
    let key = |text: &str| -> AppResult<[u8; 32]> {
        hex::decode(text)
            .map_err(|_| invalid())?
            .try_into()
            .map_err(|_| invalid())
    };
    namespace
        .validate_public(&cc_crypto_core::DevicePublicKeys {
            signing: key(signing)?,
            encryption: key(recipient)?,
        })
        .map_err(|_| invalid())?;
    Ok(Some(namespace))
}
fn namespace_epochs(
    namespace: &Option<cc_automation_core::ServiceLocalNamespace>,
    signer: u64,
    recipient: u64,
) -> AppResult<()> {
    if namespace
        .as_ref()
        .is_some_and(|n| n.key_epoch != signer || n.key_epoch != recipient)
    {
        return Err(invalid());
    }
    Ok(())
}
fn canonical_review(value: &str) -> AppResult<Uuid> {
    let id = Uuid::parse_str(value).map_err(|_| invalid())?;
    if id.is_nil() || id.to_string() != value {
        return Err(invalid());
    }
    Ok(id)
}
fn initial_review(
    owner: &Arc<crate::session::Session>,
    u: &Arc<Unlocked>,
    prepared: &PreparedReview,
    terms: &NativeServiceTerms,
) -> AppResult<()> {
    if !Weak::ptr_eq(&prepared.context.unlock, &Arc::downgrade(u))
        || !prepared.usable(owner.automation.unlocked_generation()?)
        || !prepared.validation.is_valid()
        || prepared.retry_origin.is_some()
        || prepared.service_delegation.is_some()
        || prepared.service_origin.as_ref().is_none_or(|origin| {
            ServiceOriginSelection::from_terms(terms).map_or(true, |selected|
                origin.service_id != selected.service_id.to_string()
                || origin.signer_key_sha256 != selected.signer
                || origin.recipient_key_sha256 != selected.recipient)
        })
        // Short-lived per-job Vault purpose is not a service resolver.
        || prepared.vault_session.is_some()
        || prepared.request.parameters.vault.is_some()
        || prepared.request.runner_id != terms.service_id.to_string()
        || !prepared.policy.strict_isolation
        || prepared.policy.production
    {
        return Err(invalid());
    }
    Ok(())
}
fn current_references(
    u: &Unlocked,
    prepared: &PreparedReview,
) -> AppResult<(
    Vec<ServicePolicyHostRef>,
    Vec<ServicePolicyCredentialRef>,
    BTreeMap<ObjectId, VaultCredentialMetadata>,
)> {
    let mut hosts = Vec::new();
    let mut credentials = BTreeMap::new();
    if prepared.targets.is_empty() || prepared.targets.len() > 32 {
        return Err(invalid());
    }
    for target in &prepared.targets {
        let id = crate::dto::parse_id("host_id", &target.host_id.to_string())?;
        let (revision, kind) = u
            .working()
            .correlation_host_metadata(id)
            .ok_or_else(invalid)?;
        if kind != "ssh" || u.working().vault_of(id).is_some() {
            return Err(invalid());
        }
        let session = prepared.sessions.get(&target.host_id).ok_or_else(invalid)?;
        let actual = target_from_session(target.alias.clone(), session).map_err(|_| invalid())?;
        if serde_json::to_value(&actual).map_err(|_| invalid())?
            != serde_json::to_value(target).map_err(|_| invalid())?
        {
            return Err(invalid());
        }
        hosts.push(
            ServicePolicyHostRef::from_target(&actual, revision.try_into().map_err(|_| invalid())?)
                .map_err(|_| invalid())?,
        );
        for credential in &target.credential_refs {
            let id = crate::dto::parse_id("credential_id", &credential.to_string())?;
            let metadata = u
                .working()
                .vault_credential_metadata(id)
                .ok_or_else(invalid)?;
            credentials.insert(id, metadata);
        }
    }
    hosts.sort_by(|a, b| a.id.cmp(&b.id));
    let mut refs = Vec::new();
    for (id, metadata) in &credentials {
        let revision = *metadata.fences().get(id).ok_or_else(invalid)?;
        refs.push(ServicePolicyCredentialRef {
            id: id.to_string(),
            revision: revision.try_into().map_err(|_| invalid())?,
        });
    }
    refs.sort_by(|a, b| a.id.cmp(&b.id));
    Ok((hosts, refs, credentials))
}
fn same<T: Serialize>(a: &T, b: &T) -> AppResult<bool> {
    Ok(serde_json::to_value(a).map_err(|_| invalid())?
        == serde_json::to_value(b).map_err(|_| invalid())?)
}
fn original_route_inputs(
    u: &Unlocked,
    dependencies: &BTreeMap<ObjectId, i64>,
    kinds: &BTreeMap<ObjectId, cc_models::ObjectKind>,
    trust: &BTreeMap<ObjectId, i64>,
) -> AppResult<ServiceRouteInputs> {
    if dependencies.is_empty()
        || trust.is_empty()
        || dependencies.len() + trust.len() > 512
        || dependencies.len() != kinds.len()
        || u.working().service_key_trust_revisions().as_ref() != Some(trust)
    {
        return Err(invalid());
    }
    let mut dependency_rows = Vec::new();
    let mut trust_rows = Vec::new();
    for (expected, is_trust, rows) in [
        (dependencies, false, &mut dependency_rows),
        (trust, true, &mut trust_rows),
    ] {
        for (id, revision) in expected {
            let kind = if is_trust {
                cc_models::ObjectKind::KnownHost
            } else {
                *kinds.get(id).ok_or_else(invalid)?
            };
            if u.working().revision_of(*id) != Some(*revision)
                || u.working().kind_of(*id) != Some(kind)
                || u.working().vault_of(*id).is_some()
            {
                return Err(invalid());
            }
            rows.push(ServiceRouteInputRef {
                id: id.to_string(),
                revision: (*revision).try_into().map_err(|_| invalid())?,
                kind,
            });
        }
        rows.sort_by(|a, b| a.id.cmp(&b.id));
    }
    Ok(ServiceRouteInputs {
        dependencies: dependency_rows,
        key_trust: trust_rows,
    })
}

impl ServiceDelegationConsent {
    fn check_held(
        &self,
        owner: &Arc<crate::session::Session>,
        prepared: &PreparedReview,
        u: &Unlocked,
    ) -> AppResult<()> {
        if let Some(setup) = &self.public_setup {
            setup.check(owner, u)?;
            if u.working().version() != self.working_version {
                return Err(invalid());
            }
        }
        if Instant::now() >= self.deadline
            || *u.sharing_shutdown.borrow()
            || owner.automation.stopped.is_cancelled()
            || !prepared.usable(owner.automation.unlocked_generation()?)
            || u.notices.scope_id().as_deref() != Some(self.scope.as_str())
            || owner.identity().public_keys().signing != self.owner_signing
            || owner.device_id().to_string() != self.policy.owner_id
            || self.owner_scope.check(owner, u).is_err()
        {
            return Err(invalid());
        }
        if let Some(folders) = &prepared.context.folder_composition {
            folders.check(u)?;
        }
        let origin = prepared.service_origin.as_ref().ok_or_else(invalid)?;
        let keys = u
            .working()
            .service_key_trust_records()
            .ok_or_else(invalid)?;
        if self.policy.execution_origin.as_ref() != Some(origin)
            || origin.known_hosts_digest
                != cc_automation_core::service_known_hosts_digest(&keys).map_err(|_| invalid())?
        {
            return Err(invalid());
        }
        let (hosts, credentials, metadata) = current_references(u, prepared)?;
        let route_inputs = original_route_inputs(
            u,
            prepared.dependency_revisions.as_ref().ok_or_else(invalid)?,
            prepared.dependency_kinds.as_ref().ok_or_else(invalid)?,
            prepared.key_trust_revisions.as_ref().ok_or_else(invalid)?,
        )?;
        if !same(&hosts, &self.policy.hosts)?
            || !same(&credentials, &self.policy.credentials)?
            || route_inputs != self.policy.route_inputs
            || metadata != self.credential_metadata
        {
            return Err(invalid());
        }
        Ok(())
    }
}
impl AppCore {
    /// Original-scope service enrollment observation. No project, job, capsule
    /// or secret is exported here. Terms and definition are Native selections;
    /// this is not a public JSON/Bridge function.
    pub(crate) async fn service_begin(
        &self,
        expected_profile: String,
        terms: &NativeServiceTerms,
        definition: Value,
    ) -> AppResult<Value> {
        let (owner, u) = self.unlocked().await?;
        let weak = Arc::downgrade(&u);
        drop(u);
        self.service_begin_original(expected_profile, terms, definition, owner, weak, None)
            .boxed()
            .await
    }
    pub(super) async fn service_begin_original(
        &self,
        expected_profile: String,
        terms: &NativeServiceTerms,
        definition: Value,
        owner: Arc<crate::session::Session>,
        weak: Weak<Unlocked>,
        expected_runner: Option<Arc<crate::automation_runners::RunnerConnection>>,
    ) -> AppResult<Value> {
        let owner_scope = OwnerScope::capture(self, &owner, weak.clone()).await?;
        let u = weak.upgrade().ok_or(AppError::VaultLocked)?;
        let (runner, origin, scope, generation, public, tenant) = {
            let _profile = self.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&u).await?;
            let profile = owner.profile().await?;
            if terms.access_mode != ServiceAccessMode::BoundedOffline
                || owner.profile_id.to_string() != expected_profile
                || owner_scope.check_profile(&profile).is_err()
                || owner_scope.check(&owner, &u).is_err()
            {
                return Err(invalid());
            }
            let runner = owner
                .automation
                .connections
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&terms.service_id.to_string())
                .cloned()
                .filter(|r| !r.peer.is_closed())
                .ok_or_else(invalid)?;
            if !matches!(
                runner.configuration,
                crate::automation_runners::AutomationRunnerConfig::Remote { .. }
                    | crate::automation_runners::AutomationRunnerConfig::ServiceRemote { .. }
            ) {
                return Err(invalid());
            }
            if expected_runner
                .as_ref()
                .is_some_and(|expected| !Arc::ptr_eq(expected, &runner))
            {
                return Err(invalid());
            }
            let selected = ServiceOriginSelection::from_terms(terms)?;
            (
                runner,
                selected,
                u.notices.scope_id().ok_or_else(invalid)?,
                owner.automation.unlocked_generation()?,
                owner.identity().public_keys(),
                owner_scope.tenant().clone(),
            )
        };
        let weak = Arc::downgrade(&u);
        drop(u);
        let offer = runner
            .peer
            .request(
                "service_offer",
                serde_json::json!({}),
                Duration::from_secs(60),
            )
            .await
            .map_err(|_| invalid())?;
        if offer.get("signing").and_then(Value::as_str) != Some(terms.signer.public_key.as_str())
            || offer.get("recipient").and_then(Value::as_str)
                != Some(terms.recipient.public_key.as_str())
        {
            return Err(invalid());
        }
        let raw = offer.get("probe").ok_or_else(invalid)?;
        let helper = raw
            .get("helper_digest")
            .and_then(Value::as_str)
            .ok_or_else(invalid)?;
        let probe =
            cc_automation_core::EnvironmentProbe::parse(raw.clone()).map_err(|_| invalid())?;
        if probe.system != "Linux"
            || !probe.non_root
            || !probe.dependencies_ready
            || probe.require_trusted_worker_bootstrap().is_err()
        {
            return Err(invalid());
        }
        if definition.get("graceSeconds").and_then(Value::as_u64) != Some(60) {
            return Err(invalid());
        }
        if hex::encode(sha2::Sha256::digest(
            serde_json::to_vec(&definition).map_err(AppError::internal)?,
        )) != terms.definition_digest
            || serde_json::to_vec(&definition)
                .map_err(AppError::internal)?
                .len()
                > 4096
        {
            return Err(invalid());
        }
        let u = weak.upgrade().ok_or(AppError::VaultLocked)?;
        let captured = {
            let _profile = self.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&u).await?;
            if owner.automation.unlocked_generation()? != generation
                || u.notices.scope_id().as_deref() != Some(scope.as_str())
            {
                return Err(invalid());
            }
            owner_scope.check(&owner, &u)?;
            if expected_runner.as_ref().is_some_and(|expected| {
                !owner
                    .automation
                    .connections
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&runner.id)
                    .is_some_and(|current| Arc::ptr_eq(expected, current))
            }) {
                return Err(invalid());
            }
            let keys = u
                .working()
                .service_key_trust_records()
                .ok_or_else(invalid)?;
            origin.origin(&runner, &probe, helper, &keys)?
        };
        drop(u);
        let result=runner.peer.request("service_bootstrap",serde_json::json!({"origin":captured,"schedule":{
            "id":terms.schedule_id,"revision":terms.schedule_revision,"definition":definition},"owner":{
            "owner_id":owner.device_id(),"owner_epoch":terms.owner_epoch,"tenant":tenant,
            "signing":hex::encode(public.signing),"encryption":hex::encode(public.encryption),
            "not_after":terms.signer_not_after.min(terms.recipient_not_after)}}),Duration::from_secs(60)).await.map_err(|_|invalid())?;
        let u = weak.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        if owner.automation.unlocked_generation()? != generation
            || u.notices.scope_id().as_deref() != Some(scope.as_str())
            || result.get("bootstrapped") != Some(&serde_json::json!(true))
        {
            return Err(invalid());
        }
        owner_scope.check(&owner, &u)?;
        Ok(
            serde_json::json!({"profile_id":expected_profile,"notification_scope_id":scope,"origin":captured}),
        )
    }

    pub(crate) async fn service_delegation_preview(
        &self,
        expected_profile: String,
        review_id: String,
        terms: NativeServiceTerms,
    ) -> AppResult<ServiceDelegationPolicy> {
        let (owner, u) = self.unlocked().await?;
        let weak = Arc::downgrade(&u);
        drop(u);
        self.service_delegation_preview_original(
            expected_profile,
            review_id,
            terms,
            owner,
            weak,
            None,
        )
        .boxed()
        .await
    }
    pub(super) async fn service_delegation_preview_original(
        &self,
        expected_profile: String,
        review_id: String,
        terms: NativeServiceTerms,
        owner: Arc<crate::session::Session>,
        weak: Weak<Unlocked>,
        expiry_cap: Option<u64>,
    ) -> AppResult<ServiceDelegationPolicy> {
        let id = canonical_review(&review_id)?;
        let u = weak.upgrade().ok_or(AppError::VaultLocked)?;
        let (project_id, project_revision, uploaded, digest, runner) = {
            let _profile = self.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&u).await?;
            if owner.profile_id.to_string() != expected_profile {
                return Err(invalid());
            }
            let reviews = owner
                .automation
                .reviews
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let prepared = reviews.get(&id).ok_or_else(invalid)?;
            initial_review(&owner, &u, prepared, &terms)?;
            (
                prepared.context.project_id.clone(),
                prepared.context.project_revision,
                prepared.snapshot.uploaded.revision_id,
                prepared.revision.digest.clone(),
                prepared.runner.clone(),
            )
        };
        let original = Arc::downgrade(&u);
        drop(u);
        let owner_scope = OwnerScope::capture(self, &owner, original.clone()).await?;
        let namespace = observed_namespace(
            &runner,
            &terms.signer.public_key,
            &terms.recipient.public_key,
        )
        .await?;
        namespace_epochs(
            &namespace,
            terms.signer.key_epoch,
            terms.recipient.key_epoch,
        )?;
        let context = self
            .automation_project_context_original(&project_id, owner.clone(), original.clone())
            .await?;
        let u = original.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        let profile = owner.profile().await?;
        context.check_catalog_held().await?;
        if owner_scope.check_profile(&profile).is_err()
            || owner_scope.check(&owner, &u).is_err()
            || context.project.read_only
            || context.project.revision != Some(project_revision)
        {
            return Err(invalid());
        }
        let mut reviews = owner
            .automation
            .reviews
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let prepared = reviews.get_mut(&id).ok_or_else(invalid)?;
        initial_review(&owner, &u, prepared, &terms)?;
        if prepared.snapshot.uploaded.revision_id != uploaded || prepared.revision.digest != digest
        {
            return Err(invalid());
        }
        let version = u.working().version();
        let issued = now()?;
        let (hosts, credentials, metadata) = current_references(&u, &prepared)?;
        let route_inputs = original_route_inputs(
            &u,
            prepared.dependency_revisions.as_ref().ok_or_else(invalid)?,
            prepared.dependency_kinds.as_ref().ok_or_else(invalid)?,
            prepared.key_trust_revisions.as_ref().ok_or_else(invalid)?,
        )?;
        let tenant = owner_scope.tenant().clone();
        let requested_expires = issued
            .checked_add(u64::from(terms.validity_seconds))
            .ok_or_else(invalid)?;
        let expires = expiry_cap.map_or(requested_expires, |cap| requested_expires.min(cap));
        if expires <= issued
            || terms.validity_seconds == 0
            || terms.validity_seconds > 86400
            || expires > terms.signer_not_after
            || expires > terms.recipient_not_after
        {
            return Err(invalid());
        }
        let mode = match prepared.request.mode {
            cc_models::automation::JobMode::Check => "check",
            cc_models::automation::JobMode::Apply => "apply",
        };
        let policy = ServiceDelegationPolicy {
            version: 4,
            id: Uuid::new_v4().to_string(),
            owner_id: owner.device_id().to_string(),
            owner_epoch: terms.owner_epoch,
            tenant,
            service_id: terms.service_id.to_string(),
            signer: terms.signer,
            recipient: terms.recipient,
            project_id: prepared.context.project_id.clone(),
            project_revision: prepared.context.project_revision,
            template_digest: prepared.revision.digest.clone(),
            playbook: prepared.request.playbook.clone(),
            schedule_id: terms.schedule_id.to_string(),
            schedule_revision: terms.schedule_revision,
            definition_digest: terms.definition_digest,
            inventory_digest: prepared.validation.inventory_digest().into(),
            parameters_digest: prepared.validation.parameters_digest().into(),
            environment_digest: prepared.validation.environment_digest().into(),
            hosts,
            credentials,
            route_inputs,
            modes: vec![mode.into()],
            quotas: terms.quotas,
            policy_revision: terms.policy_revision,
            revocation_epoch: terms.revocation_epoch,
            issued_at: issued,
            expires_at: expires,
            desktop_lock_policy: "independentDelegatedService".into(),
            execution_origin: prepared.service_origin.clone(),
            access_policy: ServiceAccessPolicy {
                mode: terms.access_mode,
                offline_not_after: match terms.access_mode {
                    ServiceAccessMode::FreshOwnerHead => None,
                    ServiceAccessMode::BoundedOffline => Some(expires),
                },
            },
        };
        if policy.quotas.max_forks > prepared.policy.forks
            || policy.quotas.max_job_seconds > prepared.policy.timeout_seconds
        {
            return Err(invalid());
        }
        policy
            .validate(
                &policy.owner_id,
                &owner.identity().public_keys(),
                &policy.tenant,
                issued,
            )
            .map_err(|_| invalid())?;

        let deadline = prepared
            .deadline
            .min(Instant::now() + Duration::from_secs(60));
        let consent = ServiceDelegationConsent {
            namespace,
            owner_scope,
            public_setup: None,
            working_version: version,
            policy,
            scope: u.notices.scope_id().ok_or_else(invalid)?,
            owner_signing: owner.identity().public_keys().signing,
            credential_metadata: metadata,
            deadline,
        };
        consent.check_held(&owner, prepared, &u)?;
        if version != u.working().version() {
            return Err(invalid());
        }
        let result = consent.policy.clone();
        prepared.service_delegation = Some(consent);
        Ok(result)
    }

    /// Separate explicit owner action. No caller policy/terms/manifest can
    /// replace the exact metadata frozen inside the original PreparedReview.
    pub(crate) async fn service_confirm_delegation(
        &self,
        expected_profile: String,
        review_id: String,
        expected_policy_id: String,
    ) -> AppResult<ConfirmedServiceDelegation> {
        let (owner, u) = self.unlocked().await?;
        let weak = Arc::downgrade(&u);
        drop(u);
        self.service_confirm_delegation_original(
            expected_profile,
            review_id,
            expected_policy_id,
            owner,
            weak,
        )
        .boxed()
        .await
    }
    pub(super) async fn service_confirm_delegation_original(
        &self,
        expected_profile: String,
        review_id: String,
        expected_policy_id: String,
        owner: Arc<crate::session::Session>,
        weak: Weak<Unlocked>,
    ) -> AppResult<ConfirmedServiceDelegation> {
        let id = canonical_review(&review_id)?;
        canonical_review(&expected_policy_id)?;
        let u = weak.upgrade().ok_or(AppError::VaultLocked)?;
        let (prepared, consent, fence) = {
            let _profile = self.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&u).await?;
            if owner.profile_id.to_string() != expected_profile {
                return Err(invalid());
            }
            let mut prepared = owner
                .automation
                .reviews
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id)
                .ok_or_else(invalid)?;
            let consent = prepared.service_delegation.take().ok_or_else(invalid)?;
            if consent.policy.id != expected_policy_id {
                return Err(invalid());
            }
            consent.check_held(&owner, &prepared, &u)?;
            let fence = ServiceFsFence {
                registry_epoch: owner.automation.dependencies.local_filesystem_epoch()?,
                shutdown: u.sharing_shutdown.subscribe(),
                caller: prepared.cancellation.0.clone(),
                deadline: consent.deadline,
            };
            (prepared, consent, fence)
        };
        drop(u);
        let mut shutdown = fence.shutdown.clone();
        let stopped = fence.registry_epoch.clone();
        let cancel = fence.caller.clone();
        let deadline = tokio::time::Instant::from_std(fence.deadline);
        let work = async {
            consent
                .owner_scope
                .verify(&owner, prepared.context.unlock.clone())
                .await?;
            let context = self
                .automation_project_context_original(
                    &prepared.context.project_id,
                    owner.clone(),
                    prepared.context.unlock.clone(),
                )
                .await?;
            if context.project.read_only
                || context.project.revision != Some(prepared.context.project_revision)
            {
                return Err(invalid());
            }
            let io = managed::admit_io(self, &context).await?;
            let source = context.root_selection.clone();
            let path = context.path.clone();
            let expected = prepared.revision.clone();
            let source = tracked_source(&owner, fence.clone(), move |fence| {
                fence.check()?;
                let root = source.open()?;
                root.recheck()?;
                root.reopen_same_directory(&path).map_err(|_| invalid())?;
                let revision = root.revision().map_err(|_| invalid())?;
                root.recheck()?;
                root.reopen_same_directory(&path).map_err(|_| invalid())?;
                fence.check()?;
                if revision != expected {
                    return Err(invalid());
                }
                Ok(Arc::new(SourcePin { root, _io: io }))
            })
            .await?;
            context.check_after_io(self).await?;
            let u = prepared
                .context
                .unlock
                .upgrade()
                .ok_or(AppError::VaultLocked)?;
            let _profile = self.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&u).await?;
            let profile = owner.profile().await?;
            context.check_catalog_held().await?;
            consent.owner_scope.check_profile(&profile)?;
            let version = u.working().version();
            fence.check()?;
            consent.check_held(&owner, &prepared, &u)?;
            let (signed, initial_status) = consent.owner_scope.with_current(&owner, || {
                let identity = owner.identity();
                let signed = identity
                    .sign_service_delegation(consent.policy.clone(), now()?)
                    .map_err(|_| invalid())?;
                let status = identity
                    .sign_initial_service_status(&signed, now()?)
                    .map_err(|_| invalid())?;
                Ok((signed, status))
            })?;
            consent.check_held(&owner, &prepared, &u)?;
            fence.check()?;
            if version != u.working().version() {
                return Err(invalid());
            }
            // Keep source identity + IO through final guarded SQL/signature.
            drop(source);
            Ok(ConfirmedServiceDelegation {
                delegation: signed,
                initial_status,
                template: ServicePreparedTemplate {
                    namespace: consent.namespace,
                    owner_scope: consent.owner_scope.clone(),
                    public_setup: consent.public_setup,
                    plans: prepared
                        .targets
                        .iter()
                        .map(|target| {
                            prepared
                                .sessions
                                .get(&target.host_id)
                                .map(|session| session.connection_plan().clone())
                                .ok_or_else(invalid)
                        })
                        .collect::<AppResult<Vec<_>>>()?,
                    known_hosts: u
                        .working()
                        .service_key_trust_records()
                        .ok_or_else(invalid)?,
                    owner: owner.clone(),
                    unlock: prepared.context.unlock.clone(),
                    generation: prepared.context.generation,
                    scope: consent.scope.clone(),
                    deadline: consent.deadline,
                    credential_metadata: consent.credential_metadata,
                    runner: prepared.runner,
                    revision: prepared.revision,
                    _cancellation: prepared.cancellation,
                    snapshot: prepared.snapshot,
                    inventory: prepared.inventory,
                    parameters: prepared.request.parameters,
                    targets: prepared.targets,
                },
            })
        };
        tokio::select! {biased;
            _=shutdown.changed()=>Err(AppError::VaultLocked),
            _=stopped.cancelled()=>Err(AppError::VaultLocked),
            _=cancel.cancelled()=>Err(AppError::Cancelled),
            _=tokio::time::sleep_until(deadline)=>Err(AppError::Cancelled),
            result=work=>result,
        }
    }
}

#[cfg(test)]
mod tests;
