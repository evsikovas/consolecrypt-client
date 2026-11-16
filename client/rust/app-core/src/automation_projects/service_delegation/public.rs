//! Fixed public service workflow over the existing review, owner ledger and
//! authenticated ServiceRemote. Caller choices are never Native authority.
use super::*;
mod pending;
mod results;
use crate::automation_prepare::AutomationRunRequest;
use crate::automation_runners::{RunnerConnection, stored_runner};
use crate::session::Session;
use cc_automation_core::{
    ServiceLocalNamespace, ServicePeerObservation, ServiceRemoteConfig, ServiceState,
};
use pending::PendingCleanup;

use cc_storage_core::{ServiceOwnerDelivery, ServiceOwnerRecord};
use futures::FutureExt;
use serde::de::DeserializeOwned;
use std::collections::BTreeSet;

const MAX_REV: u64 = 9_007_199_254_740_991;
fn unavailable() -> AppError {
    AppError::invalid("service", "service_unavailable_or_changed")
}
fn uncertain() -> AppError {
    AppError::invalid("service", "service_enrollment_unknown")
}
fn parse<T: DeserializeOwned>(raw: &str, budget: usize) -> AppResult<T> {
    if raw.len() > budget {
        return Err(unavailable());
    }
    serde_json::from_str(raw).map_err(|_| unavailable())
}
fn encode(value: Value) -> AppResult<String> {
    let raw = serde_json::to_string(&value).map_err(AppError::internal)?;
    if raw.len() > 1024 * 1024 {
        return Err(unavailable());
    }
    Ok(raw)
}
fn stamp(value: u64) -> AppResult<String> {
    chrono::DateTime::from_timestamp(value.try_into().map_err(|_| unavailable())?, 0)
        .map(|v| v.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .ok_or_else(unavailable)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ScheduleChoice {
    zone: String,
    hour: u8,
    minute: u8,
    weekdays: Vec<u8>,
}
impl ScheduleChoice {
    fn definition(&self) -> AppResult<Value> {
        if self.zone.is_empty()
            || self.zone.len() > 128
            || !self.zone.is_ascii()
            || self.zone.split('/').any(|p| {
                p.is_empty()
                    || p == "."
                    || p == ".."
                    || !p
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_+-".contains(&b))
            })
            || self.hour > 23
            || self.minute > 59
            || self.weekdays.is_empty()
            || self.weekdays.len() > 7
            || self.weekdays.iter().any(|d| *d > 6)
            || self.weekdays.iter().collect::<BTreeSet<_>>().len() != self.weekdays.len()
        {
            return Err(unavailable());
        }
        let mut weekdays = self.weekdays.clone();
        weekdays.sort_unstable();
        // Actual existing service ZoneInfo validates availability at bootstrap;
        // this Native syntax check neither infers a zone nor installs tzdata.
        Ok(
            json!({"version":1,"zone":self.zone,"hour":self.hour,"minute":self.minute,"weekdays":weekdays,
            "graceSeconds":60,"gapPolicy":"skip","foldPolicy":"first","missedPolicy":"skip"}),
        )
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Prepare {
    version: u8,
    profile_id: String,
    notification_scope_id: String,
    expected_runner_revision: u64,
    expected_project_revision: u64,
    run: AutomationRunRequest,
    schedule: ScheduleChoice,
    validity_seconds: u32,
    max_jobs: u16,
    max_per_utc_day: u16,
}
impl Prepare {
    fn validate(&self) -> AppResult<Value> {
        self.run.validate()?;
        if self.version != 1
            || !(1..=MAX_REV).contains(&self.expected_runner_revision)
            || !(1..=MAX_REV).contains(&self.expected_project_revision)
            || !(1..=86400).contains(&self.validity_seconds)
            || !(1..=128).contains(&self.max_jobs)
            || !(1..=self.max_jobs).contains(&self.max_per_utc_day)
            || self.run.cancel_on_lock
            || self.run.retry_of.is_some()
            || self.run.vault_credential_id.is_some()
            || self.run.vault_selection.is_some()
            || self.run.parameters.vault.is_some()
        {
            return Err(unavailable());
        }
        self.schedule.definition()
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Confirm {
    version: u8,
    profile_id: String,
    notification_scope_id: String,
    review_id: String,
    policy_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Cancel {
    version: u8,
    profile_id: String,
    notification_scope_id: String,
    review_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Action {
    version: u8,
    profile_id: String,
    notification_scope_id: String,
    runner_id: String,
    expected_runner_revision: u64,
}

struct Frame {
    owner: Arc<Session>,
    unlock: Weak<Unlocked>,
    scope: String,
    generation: u64,
}
impl Frame {
    async fn capture(app: &AppCore, version: u8, profile: &str, scope: &str) -> AppResult<Self> {
        if version != 1 {
            return Err(unavailable());
        }
        canonical_review(profile)?;
        if scope.is_empty() || scope.len() > 256 || scope.chars().any(char::is_control) {
            return Err(unavailable());
        }
        let (owner, u) = app.unlocked().await?;
        let _profile = app.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        if owner.profile_id.to_string() != profile || u.notices.scope_id().as_deref() != Some(scope)
        {
            return Err(unavailable());
        }
        let frame = Self {
            owner: owner.clone(),
            unlock: Arc::downgrade(&u),
            scope: scope.into(),
            generation: owner.automation.unlocked_generation()?,
        };
        frame.check(&u)?;
        Ok(frame)
    }
    fn check(&self, u: &Unlocked) -> AppResult<()> {
        if self.owner.automation.stopped.is_cancelled()
            || *u.sharing_shutdown.borrow()
            || self.owner.automation.unlocked_generation()? != self.generation
            || u.notices.scope_id().as_deref() != Some(self.scope.as_str())
        {
            return Err(unavailable());
        }
        Ok(())
    }
    async fn current(&self, app: &AppCore) -> AppResult<()> {
        let u = self.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = app.hold_session(&self.owner).await?;
        let _unlock = self.owner.hold_unlocked(&u).await?;
        self.check(&u)
    }
}

/// Kept only in the EXISTING PreparedReview consent/template. It is an original
/// source/pending-bootstrap fence, not a grant or another operation registry.
#[derive(Clone)]
pub(super) struct PublicSetup {
    runner: Arc<RunnerConnection>,
    namespace: ServiceLocalNamespace,
    host: cc_models::ObjectId,
    host_revision: i64,
    scope: String,
    generation: u64,
    schedule_id: Uuid,
    working_version: u64,
    pending: Arc<PendingCleanup>,
}
impl PublicSetup {
    pub(super) fn check(&self, owner: &Arc<Session>, u: &Unlocked) -> AppResult<()> {
        self.runner.service_source_current(u)?;
        let same = owner
            .automation
            .connections
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&self.runner.id)
            .is_some_and(|r| Arc::ptr_eq(r, &self.runner));
        if !same
            || self.runner.peer.is_closed()
            || self.runner.remote.as_ref().is_none_or(|r| r.is_closed())
            || owner.automation.unlocked_generation()? != self.generation
            || u.notices.scope_id().as_deref() != Some(self.scope.as_str())
            || *u.sharing_shutdown.borrow()
            || owner.automation.stopped.is_cancelled()
            || u.working().version() != self.working_version
            || u.working().correlation_host_metadata(self.host) != Some((self.host_revision, "ssh"))
            || u.working().vault_of(self.host).is_some()
        {
            return Err(unavailable());
        }
        Ok(())
    }
    async fn cancel_original(&self) -> AppResult<()> {
        // Cleanup uses the captured authenticated peer, never a current service,
        // credential lookup or reconnect. Node atomically refuses once ANY
        // enrollment attempt/uncertainty exists, so late cancel cannot stop it.
        let response = self
            .runner
            .peer
            .request(
                "service_cancel_pending",
                json!({"schedule_id":self.schedule_id}),
                Duration::from_secs(30),
            )
            .await
            .map_err(|_| uncertain())?;
        if response != json!({"cancellation_requested":true}) {
            return Err(uncertain());
        }
        self.pending.disarm();
        Ok(())
    }
}

async fn saved(
    app: &AppCore,
    frame: &Frame,
    id: &str,
    expected: Option<u64>,
) -> AppResult<crate::automation_runners::AutomationRunnerProfile> {
    canonical_review(id)?;
    if expected.is_some_and(|v| !(1..=MAX_REV).contains(&v)) {
        return Err(unavailable());
    }
    let wanted = id.to_owned();
    let row = frame
        .owner
        .storage()?
        .read(move |tx| stored_runner(tx, &wanted))
        .await?;
    frame.current(app).await?;
    if row.read_only || expected.is_some_and(|v| row.revision != Some(v)) {
        return Err(unavailable());
    }
    row.configuration
        .as_ref()
        .ok_or_else(unavailable)?
        .service_remote()?;
    Ok(row)
}

async fn source(
    app: &AppCore,
    frame: &Frame,
    id: &str,
    expected: Option<u64>,
) -> AppResult<(Arc<RunnerConnection>, cc_models::ObjectId, i64)> {
    let result = source_identity(app, frame, id, expected).await?;
    if result.0.peer.is_closed() || result.0.remote.as_ref().is_none_or(|r| r.is_closed()) {
        return Err(unavailable());
    }
    Ok(result)
}
// Only an already validated terminal revocation ACK may use identity without
// continued peer liveness. Original profile/nonce/saved config/mutation and Arc
// identity still fence the result; shutdown of that same peer is the outcome.
async fn source_identity(
    app: &AppCore,
    frame: &Frame,
    id: &str,
    expected: Option<u64>,
) -> AppResult<(Arc<RunnerConnection>, cc_models::ObjectId, i64)> {
    let row = saved(app, frame, id, expected).await?;
    if row.read_only || expected.is_some_and(|v| row.revision != Some(v)) {
        return Err(unavailable());
    }
    let config = row.configuration.ok_or_else(unavailable)?;
    let (host, _) = config.service_remote()?;
    let host = crate::dto::parse_id("host_id", host)?;
    let u = frame.unlock.upgrade().ok_or(AppError::VaultLocked)?;
    let _profile = app.hold_session(&frame.owner).await?;
    let _unlock = frame.owner.hold_unlocked(&u).await?;
    frame.check(&u)?;
    let runner = frame
        .owner
        .automation
        .connections
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(id)
        .cloned()
        .ok_or_else(unavailable)?;
    if row.revision != Some(runner.revision)
        || serde_json::to_value(&config).map_err(AppError::internal)?
            != serde_json::to_value(&runner.configuration).map_err(AppError::internal)?
        || runner.remote.is_none()
        || u.working().vault_of(host).is_some()
    {
        return Err(unavailable());
    }
    runner.service_source_current(&u)?;
    let (revision, kind) = u
        .working()
        .correlation_host_metadata(host)
        .ok_or_else(unavailable)?;
    if kind != "ssh" {
        return Err(unavailable());
    }
    Ok((runner, host, revision))
}
async fn observe_initial(
    frame: &Frame,
    app: &AppCore,
    runner: &Arc<RunnerConnection>,
) -> AppResult<ServicePeerObservation> {
    let (_, identity) = runner.configuration.service_remote()?;
    let probe = runner
        .peer
        .request("probe", json!({}), Duration::from_secs(30))
        .await
        .map_err(|_| unavailable())?;
    let observed = ServiceRemoteConfig {
        identity_id: identity.into(),
    }
    .observe(&runner.peer, probe)
    .await
    .map_err(|_| unavailable())?;
    frame.current(app).await?;
    Ok(observed)
}

fn local_state(row: Option<&ServiceOwnerRecord>) -> &'static str {
    match row.map(|r| r.delivery) {
        None => "absent",
        Some(ServiceOwnerDelivery::PendingEnrollment) => "pendingEnrollment",
        Some(ServiceOwnerDelivery::Acknowledged) => "acknowledged",
        Some(ServiceOwnerDelivery::DeliveryUnknown) => "deliveryUnknown",
        Some(ServiceOwnerDelivery::RevokedLocally) => "localBlocked",
    }
}
fn metadata(
    frame: &Frame,
    runner: &str,
    row: Option<&ServiceOwnerRecord>,
    remote: &str,
    expires: Option<u64>,
) -> AppResult<Value> {
    if row.is_some_and(|r| r.policy_revision > MAX_REV || r.revocation_epoch > MAX_REV) {
        return Err(unavailable());
    }
    Ok(
        json!({"version":1,"profileId":frame.owner.profile_id,"notificationScopeId":frame.scope,
        "runnerId":runner,"serviceId":runner,"localState":local_state(row),"remoteState":remote,
        "policyRevision":row.map(|r|r.policy_revision),"revocationEpoch":row.map(|r|r.revocation_epoch),
        "expiresUtc":expires.map(stamp).transpose()?}),
    )
}

impl AppCore {
    pub async fn automation_service_prepare_json(&self, input: String) -> AppResult<String> {
        let request: Prepare = parse(&input, 128 * 1024)?;
        encode(prepare(self, request).boxed().await?)
    }
    pub async fn automation_service_confirm_json(&self, input: String) -> AppResult<String> {
        let request: Confirm = parse(&input, 4096)?;
        encode(confirm(self, request).boxed().await?)
    }
    pub async fn automation_service_cancel_json(&self, input: String) -> AppResult<String> {
        let request: Cancel = parse(&input, 4096)?;
        encode(cancel(self, request).boxed().await?)
    }
    pub async fn automation_service_status_json(&self, input: String) -> AppResult<String> {
        let request: Action = parse(&input, 4096)?;
        encode(status(self, request, false).boxed().await?)
    }
    pub async fn automation_service_revoke_json(&self, input: String) -> AppResult<String> {
        let request: Action = parse(&input, 4096)?;
        encode(status(self, request, true).boxed().await?)
    }
}

async fn prepare(app: &AppCore, request: Prepare) -> AppResult<Value> {
    let definition = request.validate()?;
    let frame = Frame::capture(
        app,
        request.version,
        &request.profile_id,
        &request.notification_scope_id,
    )
    .await?;
    let context = app
        .automation_project_context_original(
            &request.run.project_id,
            frame.owner.clone(),
            frame.unlock.clone(),
        )
        .await?;
    if context.project.revision != Some(request.expected_project_revision)
        || context.project.read_only
    {
        return Err(unavailable());
    }
    let ledger = Arc::new(
        owner_ledger::OwnerLedger::capture_weak(app, frame.owner.clone(), frame.unlock.clone())
            .await?,
    );
    let snapshot = ledger.read(app).await?;
    let service = canonical_review(&request.run.runner_id)?;
    // Initial enrollment only. Unknown/revoked/existing enrollment is never
    // silently replaced, reset or renewed using local ledger observations.
    if snapshot.records().iter().any(|r| r.service_id == service) {
        return Err(unavailable());
    }
    let saved_runner = saved(
        app,
        &frame,
        &request.run.runner_id,
        Some(request.expected_runner_revision),
    )
    .await?;
    let configuration = saved_runner
        .configuration
        .as_ref()
        .ok_or_else(unavailable)?;
    app.automation_service_connect_original(
        frame.owner.clone(),
        frame.unlock.clone(),
        &request.run.runner_id,
        request.expected_runner_revision,
        configuration,
    )
    .await?;
    let (runner, host, host_revision) = source(
        app,
        &frame,
        &request.run.runner_id,
        Some(request.expected_runner_revision),
    )
    .await?;
    let observed = observe_initial(&frame, app, &runner).await?;
    if observed.service_state != ServiceState::Unbound || observed.original_origin.is_some() {
        return Err(unavailable());
    }
    let schedule_id = Uuid::new_v4();
    let setup = {
        let u = frame.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = app.hold_session(&frame.owner).await?;
        let _unlock = frame.owner.hold_unlocked(&u).await?;
        frame.check(&u)?;
        let setup = PublicSetup {
            runner: runner.clone(),
            namespace: observed.persistent_namespace.clone(),
            host,
            host_revision,
            scope: frame.scope.clone(),
            generation: frame.generation,
            schedule_id,
            working_version: u.working().version(),
            pending: PendingCleanup::new(runner.peer.clone(), schedule_id),
        };
        setup.check(&frame.owner, &u)?;
        setup
    };
    let trust_deadline = now()?.checked_add(86400).ok_or_else(unavailable)?;
    let terms = NativeServiceTerms {
        service_id: service,
        signer: ServicePolicyKey {
            kind: "ed25519".into(),
            public_key: observed.signing.clone(),
            key_epoch: 1,
            certificate_epoch: 1,
        },
        recipient: ServicePolicyKey {
            kind: "x25519".into(),
            public_key: observed.recipient.clone(),
            key_epoch: 1,
            certificate_epoch: 1,
        },
        owner_epoch: 1,
        policy_revision: 1,
        revocation_epoch: 1,
        signer_not_after: trust_deadline,
        recipient_not_after: trust_deadline,
        schedule_id,
        schedule_revision: 1,
        definition_digest: hex::encode(sha2::Sha256::digest(
            serde_json::to_vec(&definition).map_err(AppError::internal)?,
        )),
        validity_seconds: request.validity_seconds,
        access_mode: ServiceAccessMode::BoundedOffline,
        quotas: ServicePolicyQuotas {
            max_jobs: request.max_jobs,
            max_per_utc_day: request.max_per_utc_day,
            max_concurrent: 1,
            max_job_seconds: request.run.timeout_seconds,
            max_forks: request.run.forks,
        },
    };
    let mut created_review = None;
    let work = async {
        frame.current(app).await?;
        app.service_begin_original(
            request.profile_id.clone(),
            &terms,
            definition.clone(),
            frame.owner.clone(),
            frame.unlock.clone(),
            Some(runner.clone()),
        )
        .await?;
        {
            let u = frame.unlock.upgrade().ok_or(AppError::VaultLocked)?;
            let _profile = app.hold_session(&frame.owner).await?;
            let _unlock = frame.owner.hold_unlocked(&u).await?;
            frame.check(&u)?;
            setup.check(&frame.owner, &u)?;
        }
        let review = app
            .automation_prepare_service_original(request.run, &terms, context)
            .await?;
        let review_id = review
            .get("review_id")
            .and_then(Value::as_str)
            .ok_or_else(unavailable)?
            .to_string();
        let id = canonical_review(&review_id)?;
        created_review = Some(id);
        let policy = app
            .service_delegation_preview_original(
                request.profile_id.clone(),
                review_id.clone(),
                terms,
                frame.owner.clone(),
                frame.unlock.clone(),
                Some(trust_deadline),
            )
            .await?;
        let remaining = {
            let u = frame.unlock.upgrade().ok_or(AppError::VaultLocked)?;
            let _profile = app.hold_session(&frame.owner).await?;
            let _unlock = frame.owner.hold_unlocked(&u).await?;
            frame.check(&u)?;
            setup.check(&frame.owner, &u)?;
            let mut reviews = frame
                .owner
                .automation
                .reviews
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let prepared = reviews.get_mut(&id).ok_or_else(unavailable)?;
            if !Weak::ptr_eq(&prepared.context.unlock, &frame.unlock)
                || !Arc::ptr_eq(&prepared.runner, &runner)
                || prepared.context.project_revision != request.expected_project_revision
            {
                return Err(unavailable());
            }
            let consent = prepared
                .service_delegation
                .as_mut()
                .ok_or_else(unavailable)?;
            if consent.working_version != setup.working_version || consent.policy.id != policy.id {
                return Err(unavailable());
            }
            setup.pending.arm(
                consent.deadline,
                frame.owner.automation.stopped.clone(),
                u.sharing_shutdown.subscribe(),
            )?;
            consent.public_setup = Some(setup.clone());
            let left = consent
                .deadline
                .saturating_duration_since(Instant::now())
                .as_millis();
            if left == 0 || left > 60000 {
                return Err(unavailable());
            }
            left as u64
        };
        Ok(
            json!({"version":1,"profileId":request.profile_id,"notificationScopeId":frame.scope,
            "review":review,"policy":policy,"service":{"runnerId":runner.id,"runnerRevision":runner.revision,
                "identityId":setup.namespace.identity_id,"namespace":setup.namespace.namespace,
                "signingFingerprint":hex::encode(sha2::Sha256::digest(hex::decode(&observed.signing).map_err(|_|unavailable())?)),
                "recipientFingerprint":hex::encode(sha2::Sha256::digest(hex::decode(&observed.recipient).map_err(|_|unavailable())?)),
                "keyEpoch":1,"serviceState":"pending"},"schedule":definition,"expiresUtc":stamp(policy.expires_at)?,
                "consentRemainingMs":remaining}),
        )
    };
    let result = work.await;
    if result.is_err() {
        if let Some(id) = created_review {
            frame
                .owner
                .automation
                .reviews
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
        }
        // Even a lost bootstrap/enroll reply cannot cause an enrolled Node to
        // be stopped: only the Node's atomic pre-enrollment predicate permits it.
        if setup.cancel_original().await.is_err() {
            return Err(uncertain());
        }
    }
    result
}

async fn confirm(app: &AppCore, request: Confirm) -> AppResult<Value> {
    let frame = Frame::capture(
        app,
        request.version,
        &request.profile_id,
        &request.notification_scope_id,
    )
    .await?;
    let id = canonical_review(&request.review_id)?;
    canonical_review(&request.policy_id)?;
    let (setup, policy) = {
        let u = frame.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = app.hold_session(&frame.owner).await?;
        let _unlock = frame.owner.hold_unlocked(&u).await?;
        frame.check(&u)?;
        let reviews = frame
            .owner
            .automation
            .reviews
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let prepared = reviews.get(&id).ok_or_else(unavailable)?;
        if !Weak::ptr_eq(&prepared.context.unlock, &frame.unlock) {
            return Err(unavailable());
        }
        let consent = prepared
            .service_delegation
            .as_ref()
            .ok_or_else(unavailable)?;
        if consent.policy.id != request.policy_id {
            return Err(unavailable());
        }
        consent.check_held(&frame.owner, prepared, &u)?;
        (
            consent
                .public_setup
                .as_ref()
                .ok_or_else(unavailable)?
                .clone(),
            consent.policy.clone(),
        )
    };
    let work = async {
        let confirmed = app
            .service_confirm_delegation_original(
                request.profile_id,
                request.review_id,
                request.policy_id,
                frame.owner.clone(),
                frame.unlock.clone(),
            )
            .await?;
        confirmed.activate(app).await?;
        setup.pending.disarm();
        frame.current(app).await.map_err(|_| uncertain())?;
        Ok(
            json!({"version":1,"profileId":frame.owner.profile_id,"notificationScopeId":frame.scope,
            "runnerId":setup.runner.id,"serviceId":policy.service_id,"localState":"acknowledged","remoteState":"enrolled",
            "policyRevision":policy.policy_revision,"revocationEpoch":policy.revocation_epoch,"expiresUtc":stamp(policy.expires_at)?}),
        )
    };
    // A replay may have observed the review before another call consumed it.
    // Only the consuming path can enroll; a losing Confirm must never close the
    // winner's still-pending Node. Explicit Cancel remains its own one-use path.
    // Any consumed/failed confirmation is uncertainty, never rollback/retry.
    work.await
}

async fn cancel(app: &AppCore, request: Cancel) -> AppResult<Value> {
    let frame = Frame::capture(
        app,
        request.version,
        &request.profile_id,
        &request.notification_scope_id,
    )
    .await?;
    let id = canonical_review(&request.review_id)?;
    let setup = {
        let u = frame.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = app.hold_session(&frame.owner).await?;
        let _unlock = frame.owner.hold_unlocked(&u).await?;
        frame.check(&u)?;
        let mut reviews = frame
            .owner
            .automation
            .reviews
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let prepared = reviews.get(&id).ok_or_else(unavailable)?;
        if !Weak::ptr_eq(&prepared.context.unlock, &frame.unlock) {
            return Err(unavailable());
        }
        let setup = prepared
            .service_delegation
            .as_ref()
            .and_then(|c| c.public_setup.as_ref())
            .ok_or_else(unavailable)?
            .clone();
        setup.check(&frame.owner, &u)?;
        reviews.remove(&id);
        setup
    };
    setup.cancel_original().await?;
    frame.current(app).await.map_err(|_| uncertain())?;
    Ok(
        json!({"version":1,"profileId":frame.owner.profile_id,"notificationScopeId":frame.scope,
        "reviewId":request.review_id,"state":"cancelRequested","remoteState":"unknown"}),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OriginalPolicy {
    version: u8,
    delegation: SignedServiceDelegation,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Recorded {
    recorded: bool,
    cancel_requested: u32,
    dispatch: bool,
}

fn policy_metadata_matches(
    row: &ServiceOwnerRecord,
    namespace: &str,
    signing: &str,
    recipient: &str,
    signed: &SignedServiceDelegation,
) -> AppResult<()> {
    let p = signed.body();
    let hash = |bytes: &[u8]| hex::encode(sha2::Sha256::digest(bytes));
    if p.service_id != row.service_id.to_string()
        || row.node_local_profile != namespace
        || row.policy_digest != signed.digest().map_err(|_| unavailable())?
        || row.owner_epoch != p.owner_epoch
        || row.policy_revision < p.policy_revision
        || row.revocation_epoch < p.revocation_epoch
        || p.signer.public_key != signing
        || p.recipient.public_key != recipient
        || row.signer_fingerprint != hash(&hex::decode(&signing).map_err(|_| unavailable())?)
        || row.recipient_fingerprint != hash(&hex::decode(&recipient).map_err(|_| unavailable())?)
        || row.route_trust_digest
            != hash(
                &serde_json::to_vec(&(&p.route_inputs, &p.execution_origin))
                    .map_err(AppError::internal)?,
            )
    {
        return Err(unavailable());
    }
    Ok(())
}

fn policy_matches(
    row: &ServiceOwnerRecord,
    observed: &ServicePeerObservation,
    signed: &SignedServiceDelegation,
) -> AppResult<()> {
    policy_metadata_matches(
        row,
        &observed.persistent_namespace.namespace,
        &observed.signing,
        &observed.recipient,
        signed,
    )?;
    if observed.original_origin.as_ref() != signed.body().execution_origin.as_ref() {
        return Err(unavailable());
    }
    Ok(())
}

async fn check_status_owner(
    app: &AppCore,
    frame: &Frame,
    scope: Option<&Arc<OwnerScope>>,
) -> AppResult<()> {
    let u = frame.unlock.upgrade().ok_or(AppError::VaultLocked)?;
    let _profile = app.hold_session(&frame.owner).await?;
    let _unlock = frame.owner.hold_unlocked(&u).await?;
    frame.check(&u)?;
    if let Some(scope) = scope {
        scope.check(&frame.owner, &u)?;
    }
    Ok(())
}

async fn status(app: &AppCore, request: Action, revoke: bool) -> AppResult<Value> {
    let frame = Frame::capture(
        app,
        request.version,
        &request.profile_id,
        &request.notification_scope_id,
    )
    .await?;
    let service = canonical_review(&request.runner_id)?;
    // Saved intent is checked BEFORE any connection lookup, network or local
    // tombstone. A stale UI click cannot target a newly edited saved Runner.
    let captured_runner = saved(
        app,
        &frame,
        &request.runner_id,
        Some(request.expected_runner_revision),
    )
    .await?;
    let ledger = Arc::new(
        owner_ledger::OwnerLedger::capture_stale(app, frame.owner.clone(), frame.unlock.clone())
            .await?,
    );
    if revoke {
        ledger
            .block_original_runner(app, service, Some(request.expected_runner_revision))
            .await?;
    }
    let snapshot = ledger.read_stale_metadata(app).await?;
    let row = snapshot.records().iter().find(|r| r.service_id == service);
    // The sticky local denial above is durable before any forward. Offline or
    // uncertain remote delivery returns localBlocked/unknown, never revoked.
    let remote = async {
        // Explicit Status is a user action, never an on-open/background poll.
        // Personal metadata needs a current OWNER observation before networking.
        // Explicit Revoke has ALREADY durably blocked this existing record;
        // it can only sign a denial after genuine original-peer policy proof.
        let fresh_status = if !revoke {
            Some(OwnerScope::capture(app, &frame.owner, frame.unlock.clone()).await?)
        } else {
            None
        };
        app.automation_service_connect_original(
            frame.owner.clone(),
            frame.unlock.clone(),
            &request.runner_id,
            request.expected_runner_revision,
            captured_runner
                .configuration
                .as_ref()
                .ok_or_else(unavailable)?,
        )
        .await?;
        let (runner, host, host_revision) = source(
            app,
            &frame,
            &request.runner_id,
            Some(request.expected_runner_revision),
        )
        .await?;
        let observed = observe_initial(&frame, app, &runner).await?;
        if row.is_none() && !revoke {
            let state = match observed.service_state {
                ServiceState::Unbound => "unbound",
                ServiceState::Pending => "pending",
                _ => "unknown",
            };
            let (again, _, _) = source(
                app,
                &frame,
                &request.runner_id,
                Some(request.expected_runner_revision),
            )
            .await?;
            if !Arc::ptr_eq(&again, &runner) {
                return Err(unavailable());
            }
            check_status_owner(app, &frame, fresh_status.as_ref()).await?;
            return metadata(&frame, &request.runner_id, None, state, None);
        }
        let raw = runner
            .peer
            .request("service_owner_policy", json!({}), Duration::from_secs(30))
            .await
            .map_err(|_| unavailable())?;
        let encoded = serde_json::to_vec(&raw).map_err(AppError::internal)?;
        if encoded.len() > 64 * 1024 {
            return Err(unavailable());
        }
        let original: OriginalPolicy = serde_json::from_value(raw).map_err(|_| unavailable())?;
        if original.version != 1 {
            return Err(unavailable());
        }
        let signed = original.delegation;
        let p = signed.body();
        let row = row.ok_or_else(unavailable)?;
        policy_matches(row, &observed, &signed)?;
        SignedServiceDelegation::verify_json(
            &signed.to_json().map_err(|_| unavailable())?,
            &frame.owner.device_id().to_string(),
            &frame.owner.identity().public_keys(),
            &ledger.recorded_tenant(),
            p.issued_at,
        )
        .map_err(|_| unavailable())?;
        let expiry = p.expires_at;
        let check = async {
            let (again, again_host, again_revision) = source(
                app,
                &frame,
                &request.runner_id,
                Some(request.expected_runner_revision),
            )
            .await?;
            if !Arc::ptr_eq(&again, &runner)
                || host != again_host
                || host_revision != again_revision
            {
                return Err(unavailable());
            }
            Ok(())
        };
        check.await?;
        if !revoke {
            let state = match observed.service_state {
                ServiceState::Enrolled => "enrolled",
                ServiceState::Restored => "restored",
                ServiceState::Pending => "pending",
                ServiceState::Unbound => "unbound",
            };
            check_status_owner(app, &frame, fresh_status.as_ref()).await?;
            return metadata(&frame, &request.runner_id, Some(row), state, Some(expiry));
        }
        let statement = ledger.sign_blocked_original(app, &signed).await?;
        // Parse only our own typed output to project the ACTUAL signed floors;
        // no policy/status JSON supplied by a UI reaches this signer.
        let statement: Value =
            serde_json::from_slice(&statement.to_json().map_err(|_| unavailable())?)
                .map_err(AppError::internal)?;
        let policy_revision = statement
            .pointer("/body/policy_revision")
            .and_then(Value::as_u64)
            .ok_or_else(unavailable)?;
        let revocation_epoch = statement
            .pointer("/body/revocation_epoch")
            .and_then(Value::as_u64)
            .ok_or_else(unavailable)?;
        let (again, _, _) = source(
            app,
            &frame,
            &request.runner_id,
            Some(request.expected_runner_revision),
        )
        .await?;
        if !Arc::ptr_eq(&again, &runner) {
            return Err(unavailable());
        }
        let response = runner
            .peer
            .request(
                "service_owner_status",
                json!({"delegation":signed,"statement":statement}),
                Duration::from_secs(30),
            )
            .await
            .map_err(|_| unavailable())?;
        let ack: Recorded = serde_json::from_value(response).map_err(|_| unavailable())?;
        if !ack.recorded || ack.dispatch || ack.cancel_requested > 4096 {
            return Err(unavailable());
        }
        let (again, _, _) = source_identity(
            app,
            &frame,
            &request.runner_id,
            Some(request.expected_runner_revision),
        )
        .await?;
        if !Arc::ptr_eq(&again, &runner) {
            return Err(unavailable());
        }
        let mut actual = row.clone();
        actual.policy_revision = policy_revision;
        actual.revocation_epoch = revocation_epoch;
        metadata(
            &frame,
            &request.runner_id,
            Some(&actual),
            "revocationRecorded",
            Some(expiry),
        )
    }
    .await;
    // Scope loss remains an error even when remote outcome is unknown. An
    // unchanged original scope may honestly display its stale local metadata.
    frame.current(app).await?;
    match remote {
        Ok(value) => Ok(value),
        Err(_) => metadata(&frame, &request.runner_id, row, "unknown", None),
    }
}

#[cfg(test)]
mod tests;
