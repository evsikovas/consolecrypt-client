//! Actual original-profile/password/review-lifetime causality. The private
//! duplex helper supplies test metadata only; NO target session/job executes.
//! Validation still goes through the real Native verifier, never an opaque
//! witness constructor. A positive deployed-Runner proof remains a later gate.
#[path = "production_owner_tests/happy.rs"]
mod happy;
use super::*;
use crate::automation_prepare::{
    AutomationReviewContext, AutomationRunRequest, CancelOnDrop, PreparedReview, RevisionLease,
};
use crate::automation_production_owner::{FrozenOwnerReview, OwnerPassphrasePause};
use cc_automation_core::{
    EnrolledJobGrant, ManualOwnerGrant, ProjectRoot, RunParameters, UploadedRevision,
    ValidationProof,
};
use cc_models::automation::{ApprovedTarget, JobMode, JobPolicy};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;

const PASSWORD: &str = "owned production confirmation fixture password 42";
struct Fixture {
    _directory: tempfile::TempDir,
    app: Arc<AppCore>,
    owner: Arc<crate::session::Session>,
    id: Uuid,
    peer: Arc<HelperPeer>,
    worker: tokio::task::JoinHandle<()>,
    probes: Arc<AtomicUsize>,
    submits: Arc<AtomicUsize>,
}
fn canonical(value: Value) -> Value {
    match value {
        Value::Object(fields) => {
            Value::Object(fields.into_iter().map(|(k, v)| (k, canonical(v))).collect())
        }
        Value::Array(rows) => Value::Array(rows.into_iter().map(canonical).collect()),
        other => other,
    }
}
fn witness(value: Value) -> Value {
    let raw = serde_json::to_string(&canonical(value)).unwrap();
    json!({"canonical_json":raw,"digest":hex::encode(Sha256::digest(raw.as_bytes()))})
}
impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let app = Arc::new(
            AppCore::new(crate::AppConfig::for_tests(
                directory.path().to_string_lossy().into_owned(),
            ))
            .unwrap(),
        );
        app.create_local_profile("Owned production confirmation".into(), PASSWORD.into())
            .await
            .unwrap();
        let (owner, unlocked) = app.unlocked().await.unwrap();
        let project = app
            .automation_project_create("Owned review fixture".into(), None)
            .await
            .unwrap();
        app.automation_project_save(
            project.id.clone(),
            "site.yml".into(),
            None,
            "---\n- hosts: all\n  tasks: []\n".into(),
        )
        .await
        .unwrap();
        let context = app.automation_project_context(&project.id).await.unwrap();
        let revision = ProjectRoot::open(&context.path)
            .unwrap()
            .revision()
            .unwrap();
        let probes = Arc::new(AtomicUsize::new(0));
        let submits = Arc::new(AtomicUsize::new(0));
        let (client, server) = tokio::io::duplex(8192);
        let (reader, writer) = tokio::io::split(client);
        let peer = HelperPeer::connect(reader, writer);
        let pc = probes.clone();
        let sc = submits.clone();
        let worker = tokio::spawn(async move {
            let (read, mut write) = tokio::io::split(server);
            let mut read = BufReader::new(read);
            while let Ok(Some(raw)) = cc_automation_core::read_frame(&mut read).await {
                let request: Value = serde_json::from_slice(&raw).unwrap();
                let op = request["operation"].as_str().unwrap();
                let mut response = json!({"version":1,"type":"response","request_id":request["request_id"],"result":{},"error":null});
                match op {
                    "configure_manual_owner" => {
                        response["result"] = json!({"configured":true,"persistent":false,"owner_policy":"ownerConfirmationVaultPassphraseV1"})
                    }
                    "probe" => {
                        pc.fetch_add(1, Ordering::SeqCst);
                        response["result"] = Value::Null;
                        response["error"] = json!({"code":"fixture_probe_refused","message":"No execution fixture"});
                    }
                    "accept_job" => {
                        sc.fetch_add(1, Ordering::SeqCst);
                        panic!("no job may execute in password/lifetime fixture");
                    }
                    "revision_release" => {}
                    other => panic!("unexpected fixture operation {other}"),
                }
                let mut raw = serde_json::to_vec(&response).unwrap();
                raw.push(b'\n');
                if write.write_all(&raw).await.is_err() {
                    break;
                }
            }
        });
        let runner_id = Uuid::new_v4();
        let host_id = Uuid::new_v4();
        let now = chrono::Utc::now().timestamp();
        let grant = EnrolledJobGrant {
            signer_id: owner.identity().device_id().to_string().parse().unwrap(),
            public_key: owner.identity().public_keys().signing,
            tenant_id: owner.profile_id.to_string().parse().unwrap(),
            runner_id,
            projects: [project.id.parse().unwrap()].into_iter().collect(),
            hosts: [host_id].into_iter().collect(),
            jobs_execute: true,
            expires_at: now + 3600,
            max_job_seconds: 3600,
        };
        let runner = Arc::new(RunnerConnection {
            id: runner_id.to_string(),
            revision: 1,
            configuration: AutomationRunnerConfig::Remote {
                host_id: host_id.to_string(),
                helper: cc_automation_core::RemoteHelperConfig {
                    python: "/owned/fixture/python".into(),
                    module_dir: "/owned/fixture/module".into(),
                    root: "/owned/fixture/state".into(),
                },
            },
            peer: peer.clone(),
            probe: json!({}),
            remote: None,
            local: None,
            authorization: tokio::sync::Mutex::new(Some(grant.clone())),
        });
        let parameters = RunParameters {
            extra_vars: BTreeMap::new(),
            tags: vec![],
            skip_tags: vec![],
            diff: false,
            vault: None,
        };
        let inventory = json!({"groups":{}});
        let targets = vec![ApprovedTarget {
            host_id,
            alias: "owned".into(),
            address: "127.0.0.1".into(),
            port: 22,
            username: "runner".into(),
            route_digest: "e".repeat(64),
            credential_refs: vec![],
        }];
        let pw = witness(serde_json::to_value(&parameters).unwrap());
        let iw = witness(inventory.clone());
        let tw = witness(serde_json::to_value(&targets).unwrap());
        let checks=["syntax","lint"].map(|name|json!({"name":name,"exit_code":0,"output":"synthetic metadata fixture; no deployment","truncated":false,"cancelled":false,"timed_out":false,"elapsed_ms":1}));
        let validation:ValidationProof=serde_json::from_value(json!({"revision_digest":revision.digest,"environment_digest":"c".repeat(64),"playbook":"site.yml",
            "parameters_digest":pw["digest"],"inventory_digest":iw["digest"],"targets_digest":tw["digest"],"valid":true,"checks":checks,"input_witnesses":{"parameters":pw,"inventory":iw}})).unwrap();
        let validation = validation
            .verify(
                &revision.digest,
                &"c".repeat(64),
                "site.yml",
                &parameters,
                &inventory,
                &targets,
            )
            .unwrap();
        let id = Uuid::new_v4();
        let lease = RevisionLease::new(
            UploadedRevision {
                revision_id: Uuid::new_v4(),
                digest: revision.digest.clone(),
                file_count: 1,
                entry_count: 1,
                bytes: revision.bytes,
            },
            &peer,
        );
        let mut review = PreparedReview {
            id,
            context: AutomationReviewContext {
                unlock: Arc::downgrade(&unlocked),
                generation: owner.automation.unlocked_generation().unwrap(),
                project_id: project.id.clone(),
                project_revision: context.project.revision.unwrap(),
                folder_composition: None,
            },
            request: AutomationRunRequest {
                project_id: project.id,
                runner_id: runner_id.to_string(),
                playbook: "site.yml".into(),
                host_ids: vec![host_id.to_string()],
                mode: JobMode::Apply,
                parameters,
                timeout_seconds: 300,
                forks: 1,
                cancel_on_lock: true,
                retry_of: None,
                vault_credential_id: None,
                vault_selection: None, deployment_environment:None,
            },
            runner,
            revision,
            git_source: None,
            retry_origin: None,
            snapshot: lease,
            validation,
            vault_session: None,
            inventory,
            inventory_ref: None,
            targets,
            sessions: HashMap::new(),
            dependency_revisions: None,
            dependency_kinds: None,
            key_trust_revisions: None,
            service_origin: None,
            service_delegation: None,
            production_owner: None,
            execution_credentials: None,
            cancellation: CancelOnDrop(CancellationToken::new()),
            policy: JobPolicy {
                strict_isolation: true,
                cancel_on_lock: true,
                timeout_seconds: 300,
                forks: 1,
                production: true,
            },
            expires_at: now + 600,
            deadline: Instant::now() + Duration::from_secs(600),
            _review_slot: owner
                .automation
                .review_slots
                .clone()
                .try_acquire_owned()
                .unwrap(),
            _hop_slots: owner
                .automation
                .hop_slots
                .clone()
                .try_acquire_owned()
                .unwrap(),
        };
        let manual = ManualOwnerGrant::configure(&peer, grant.clone())
            .await
            .unwrap();
        review.production_owner = Some(
            FrozenOwnerReview::freeze(
                &review,
                manual,
                &grant,
                unlocked.notices.scope_id().unwrap(),
                now,
            )
            .unwrap(),
        );
        owner.automation.reviews.lock().unwrap().insert(id, review);
        Self {
            _directory: directory,
            app,
            owner,
            id,
            peer,
            worker,
            probes,
            submits,
        }
    }
    fn view(&self) -> Value {
        self.owner.automation.reviews.lock().unwrap()[&self.id]
            .production_owner
            .as_ref()
            .unwrap()
            .view()
    }
    fn pause(&self) -> Arc<OwnerPassphrasePause> {
        self.owner
            .automation
            .reviews
            .lock()
            .unwrap()
            .get_mut(&self.id)
            .unwrap()
            .production_owner
            .as_mut()
            .unwrap()
            .pause_after_passphrase()
    }
    fn start(&self) -> tokio::task::JoinHandle<crate::AppResult<Value>> {
        let app = self.app.clone();
        let id = self.id.to_string();
        tokio::spawn(async move {
            app.automation_confirm_owner_run(id, crate::StepUp::Passphrase(PASSWORD.into()))
                .await
        })
    }
    async fn close(self) {
        assert_eq!(self.submits.load(Ordering::SeqCst), 0);
        self.app.shutdown().await.unwrap();
        self.peer.close().await;
        self.worker.abort();
    }
}

#[tokio::test]
async fn wrong_passphrase_retries_same_frozen_manifest_deadline_and_never_submits() {
    let fixture = Fixture::new().await;
    let before = fixture.view();
    let error = fixture
        .app
        .automation_confirm_owner_run(
            fixture.id.to_string(),
            crate::StepUp::Passphrase("incorrect owned fixture password".into()),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, crate::AppError::WrongPassphrase));
    assert_eq!(before, fixture.view());
    assert_eq!(fixture.probes.load(Ordering::SeqCst), 0);
    let pause = fixture.pause();
    let task = fixture.start();
    tokio::time::timeout(Duration::from_secs(10), pause.entered.notified())
        .await
        .unwrap();
    assert_eq!(before, fixture.view());
    pause.release.notify_one();
    let error = task.await.unwrap().unwrap_err();
    assert!(!matches!(error, crate::AppError::WrongPassphrase));
    assert_eq!(fixture.probes.load(Ordering::SeqCst), 1);
    assert!(!fixture
        .owner
        .automation
        .reviews
        .lock()
        .unwrap()
        .contains_key(&fixture.id));
    fixture.close().await;
}

#[tokio::test]
async fn lock_during_actual_passphrase_completion_denies_before_probe_or_submit() {
    let fixture = Fixture::new().await;
    let pause = fixture.pause();
    let task = fixture.start();
    tokio::time::timeout(Duration::from_secs(10), pause.entered.notified())
        .await
        .unwrap();
    fixture.app.lock().await.unwrap();
    pause.release.notify_one();
    assert!(task.await.unwrap().is_err());
    assert_eq!(fixture.probes.load(Ordering::SeqCst), 0);
    fixture.close().await;
}

#[tokio::test]
async fn profile_switch_during_actual_passphrase_completion_cannot_adopt_new_unlock() {
    let fixture = Fixture::new().await;
    let pause = fixture.pause();
    let task = fixture.start();
    tokio::time::timeout(Duration::from_secs(10), pause.entered.notified())
        .await
        .unwrap();
    fixture
        .app
        .create_local_profile(
            "Different owned profile".into(),
            "unrelated owned profile password42".into(),
        )
        .await
        .unwrap();
    let (other, _) = fixture.app.unlocked().await.unwrap();
    assert_ne!(fixture.owner.profile_id, other.profile_id);
    pause.release.notify_one();
    assert!(task.await.unwrap().is_err());
    assert_eq!(fixture.probes.load(Ordering::SeqCst), 0);
    fixture.close().await;
}

#[tokio::test]
async fn explicit_review_removal_during_actual_passphrase_completion_is_sticky() {
    let fixture = Fixture::new().await;
    let pause = fixture.pause();
    let task = fixture.start();
    tokio::time::timeout(Duration::from_secs(10), pause.entered.notified())
        .await
        .unwrap();
    fixture
        .app
        .automation_discard_review(fixture.id.to_string())
        .await
        .unwrap();
    pause.release.notify_one();
    assert!(task.await.unwrap().is_err());
    assert!(!fixture
        .owner
        .automation
        .reviews
        .lock()
        .unwrap()
        .contains_key(&fixture.id));
    assert_eq!(fixture.probes.load(Ordering::SeqCst), 0);
    fixture.close().await;
}

#[tokio::test]
async fn changed_current_password_envelope_denies_old_verified_passphrase() {
    let fixture = Fixture::new().await;
    let pause = fixture.pause();
    let task = fixture.start();
    tokio::time::timeout(Duration::from_secs(10), pause.entered.notified())
        .await
        .unwrap();
    fixture
        .app
        .change_passphrase(
            PASSWORD.into(),
            "replacement owned fixture password42".into(),
        )
        .await
        .unwrap();
    pause.release.notify_one();
    assert!(task.await.unwrap().is_err());
    assert_eq!(fixture.probes.load(Ordering::SeqCst), 0);
    fixture.close().await;
}

#[tokio::test]
async fn generic_presence_token_cannot_substitute_owner_vault_passphrase() {
    let fixture = Fixture::new().await;
    let before = fixture.view();
    let error = fixture
        .app
        .automation_confirm_owner_run(
            fixture.id.to_string(),
            crate::StepUp::Presence("arbitrary caller presence token".into()),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, crate::AppError::PassphraseRequired));
    assert_eq!(before, fixture.view());
    assert_eq!(fixture.probes.load(Ordering::SeqCst), 0);
    fixture.close().await;
}

#[tokio::test]
async fn default_confirm_cannot_consume_challenge_metadata_as_owner_authority() {
    let fixture = Fixture::new().await;
    assert!(fixture
        .app
        .automation_confirm_run(fixture.id.to_string())
        .await
        .is_err());
    assert_eq!(fixture.probes.load(Ordering::SeqCst), 0);
    fixture.close().await;
}

#[tokio::test]
async fn original_review_expiry_during_passphrase_does_not_renew_manifest_or_deadline() {
    let fixture = Fixture::new().await;
    let before = fixture.view();
    let pause = fixture.pause();
    let task = fixture.start();
    tokio::time::timeout(Duration::from_secs(10), pause.entered.notified())
        .await
        .unwrap();
    fixture
        .owner
        .automation
        .reviews
        .lock()
        .unwrap()
        .get_mut(&fixture.id)
        .unwrap()
        .deadline = Instant::now();
    assert_eq!(before, fixture.view());
    pause.release.notify_one();
    assert!(task.await.unwrap().is_err());
    assert_eq!(fixture.probes.load(Ordering::SeqCst), 0);
    fixture.close().await;
}

#[tokio::test]
async fn a_pending_owner_attempt_does_not_start_another_passphrase_worker() {
    let fixture = Fixture::new().await;
    let pause = fixture.pause();
    let task = fixture.start();
    tokio::time::timeout(Duration::from_secs(10), pause.entered.notified())
        .await
        .unwrap();
    let error = fixture
        .app
        .automation_confirm_owner_run(
            fixture.id.to_string(),
            crate::StepUp::Passphrase("not checked while first is pending".into()),
        )
        .await
        .unwrap_err();
    assert!(!matches!(error, crate::AppError::WrongPassphrase));
    fixture
        .app
        .automation_discard_review(fixture.id.to_string())
        .await
        .unwrap();
    pause.release.notify_one();
    assert!(task.await.unwrap().is_err());
    assert_eq!(fixture.probes.load(Ordering::SeqCst), 0);
    fixture.close().await;
}
