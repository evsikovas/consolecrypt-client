use super::*;
use crate::automation_runners::AutomationRunnerConfig;
use cc_automation_core::{DependencyArtifactSource, DependencyKind, HelperLaunch};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    OnceLock,
};

// Process-wide admission tests must not consume each other's two slots.
static SERIAL: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
pub(super) fn serial() -> &'static tokio::sync::Mutex<()> {
    SERIAL.get_or_init(Default::default)
}
fn selected() -> DependencySourceSelection {
    DependencySourceSelection::new(
        DependencyKind::Role,
        "owned_fixture".into(),
        "1.2.3".into(),
        DependencyArtifactSource {
            url: "https://source.example.invalid/role.tar.gz".into(),
            sha256: "a".repeat(64),
        },
        0,
    )
    .unwrap()
}
pub(super) struct Fixture {
    _directory: tempfile::TempDir,
    pub(super) app: AppCore,
    pub(super) project: String,
    pub(super) runner: String,
}
impl Fixture {
    pub(super) async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            directory.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile(
            "Owned dependency controller fixture".into(),
            "owned dependency fixture passphrase 42".into(),
        )
        .await
        .unwrap();
        let project = app
            .automation_project_create("Dependency base".into(), None)
            .await
            .unwrap();
        // Deliberately unavailable launch on either supported platform.
        // Windows must use the real explicit non-root WSL configuration rule;
        // no distribution is created, inspected or started by these tests.
        // Successful review cannot have launched a Helper; start tests cancel
        // before this factory boundary.
        let launch = AutomationRunnerConfig::Local {
            launch: if cfg!(target_os = "windows") {
                HelperLaunch::Wsl {
                    distribution: format!("cc-uninstalled-fixture-{}", Uuid::new_v4()),
                    user: "cc_fixture_nonroot".into(),
                    python: "/nonexistent-consolecrypt-fixture/bin/python".into(),
                    module_dir: "/nonexistent-consolecrypt-fixture/runner/src".into(),
                    root: "/nonexistent-consolecrypt-fixture/runner/jobs".into(),
                }
            } else {
                HelperLaunch::Native {
                    python: directory.path().join("not-a-python-executable"),
                    module_dir: directory.path().join("not-a-runner-module"),
                    root: directory.path().join("runner-jobs"),
                }
            },
        };
        let runner = app
            .automation_runner_create("Dedicated dependency fixture".into(), launch)
            .await
            .unwrap();
        Self {
            _directory: directory,
            app,
            project: project.id,
            runner: runner.id,
        }
    }
    async fn review(&self) -> SourceReviewView {
        self.app
            .automation_dependency_review(
                self.project.clone(),
                1,
                self.runner.clone(),
                1,
                selected(),
            )
            .await
            .unwrap()
    }
    pub(super) async fn ticket(&self) -> (Arc<DependencyRegistry>, Ticket) {
        let view = self.review().await;
        let (owner, u) = self.app.unlocked().await.unwrap();
        drop(u);
        let registry = owner.automation.dependencies.clone();
        let ticket = registry.consume(view.review_id).unwrap();
        (registry, ticket)
    }
}
fn value(view: &DependencyView) -> Value {
    serde_json::to_value(view).unwrap()
}

#[tokio::test]
async fn review_issues_exact_native_source_and_destination_without_starting_helper() {
    let _serial = serial().lock().await;
    let f = Fixture::new().await;
    let review = f.review().await;
    assert_eq!(review.project_revision, 1);
    assert_eq!(review.runner_revision, 1);
    assert_eq!(review.destination, "roles/owned_fixture");
    assert_eq!(review.expires_in_seconds, 300);
    assert_eq!(review.source, selected());
    assert!(!review.operation_id.is_nil());
    assert!(!review.review_id.is_nil());
    assert!(review.old_lock_sha256.is_none());
    assert_eq!(review.base_digest.len(), 64);
    let bytes = serde_json::to_vec(&review).unwrap();
    assert!(bytes.len() < 4096);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn review_ticket_budget_one_use_expiry_and_cancel_before_start() {
    let _serial = serial().lock().await;
    let f = Fixture::new().await;
    let first = f.review().await;
    for _ in 1..8 {
        f.review().await;
    }
    assert!(f
        .app
        .automation_dependency_review(f.project.clone(), 1, f.runner.clone(), 1, selected())
        .await
        .is_err());
    f.app
        .automation_dependency_cancel(first.operation_id.to_string())
        .await
        .unwrap();
    assert!(f
        .app
        .automation_dependency_start(first.review_id.to_string())
        .await
        .is_err());
    assert!(f
        .app
        .automation_dependency_start(first.review_id.to_string())
        .await
        .is_err());
    let (registry, mut ticket) = f.ticket().await;
    ticket.expires = Instant::now() - Duration::from_secs(1);
    let id = ticket.review;
    registry.issue(ticket).unwrap();
    assert!(registry.consume(id).is_err());
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn unsupported_receipt_and_future_lock_reject_before_network_and_preserve_bytes() {
    let _serial = serial().lock().await;
    let f = Fixture::new().await;
    let context = f.app.automation_project_context(&f.project).await.unwrap();
    for (path, text) in [
        (
            ".consolecrypt-dependency-receipts.json",
            "{\"version\":1,\"receipts\":[]}",
        ),
        (
            ".consolecrypt-dependencies.json",
            "{\"version\":987,\"dependencies\":[]}",
        ),
    ] {
        std::fs::write(context.path.join(path), text).unwrap();
        assert!(f
            .app
            .automation_dependency_review(f.project.clone(), 1, f.runner.clone(), 1, selected())
            .await
            .is_err());
        assert_eq!(
            std::fs::read_to_string(context.path.join(path)).unwrap(),
            text
        );
        std::fs::remove_file(context.path.join(path)).unwrap();
    }
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn real_sql_only_project_mutation_invalidates_captured_ticket() {
    let _serial = serial().lock().await;
    let f = Fixture::new().await;
    let (_, ticket) = f.ticket().await;
    let (_, u) = f.app.unlocked().await.unwrap();
    let version = u.working().version();
    drop(u);
    f.app
        .automation_project_save_inventory(
            f.project.clone(),
            1,
            json!({"version":2,"selected_hosts":[],"groups":{}}).to_string(),
        )
        .await
        .unwrap();
    let (_, u) = f.app.unlocked().await.unwrap();
    assert_eq!(version, u.working().version());
    drop(u);
    assert!(ticket.context.current(&f.app).await.is_err());
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn actual_file_mutation_fails_same_core_preflight_without_copying_base() {
    let _serial = serial().lock().await;
    let f = Fixture::new().await;
    let (_, ticket) = f.ticket().await;
    std::fs::write(
        ticket.context.project.path.join("playbook.yml"),
        "changed after explicit source review\n",
    )
    .unwrap();
    assert!(dependency_preflight(
        &ticket.context.base,
        &ticket.context.revision,
        &ticket.context.selection
    )
    .is_err());
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn runner_successful_edit_forget_disconnect_revoke_and_never_resurrect_phase() {
    let _serial = serial().lock().await;
    let f = Fixture::new().await;
    let (registry, ticket) = f.ticket().await;
    let (op, reservation) = registry.admit(ticket).unwrap();
    let config = op.context.runner.configuration.clone();
    f.app
        .automation_runner_update(f.runner.clone(), 1, "Edited runner".into(), config)
        .await
        .unwrap();
    assert!(op.cancel.is_cancelled());
    op.phase(Phase::CandidatePreview);
    assert_eq!(value(&op.view())["phase"], "contextChanged");
    drop(reservation);
    // Another operation on the new exact revision is revoked by disconnect
    // even though this feature never inserted a normal RunnerConnection.
    let review = f
        .app
        .automation_dependency_review(f.project.clone(), 1, f.runner.clone(), 2, selected())
        .await
        .unwrap();
    let ticket = registry.consume(review.review_id).unwrap();
    let (op, reservation) = registry.admit(ticket).unwrap();
    f.app
        .automation_runner_disconnect(f.runner.clone())
        .await
        .unwrap();
    assert!(op.cancel.is_cancelled());
    drop(reservation);
    let review = f
        .app
        .automation_dependency_review(f.project.clone(), 1, f.runner.clone(), 2, selected())
        .await
        .unwrap();
    f.app
        .automation_runner_forget(f.runner.clone(), 2)
        .await
        .unwrap();
    assert!(registry.consume(review.review_id).is_err());
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn lock_and_profile_switch_revoke_pending_source_reviews() {
    let _serial = serial().lock().await;
    let f = Fixture::new().await;
    let (registry, ticket) = f.ticket().await;
    let context = ticket.context.clone();
    let id = ticket.review;
    registry.issue(ticket).unwrap();
    f.app.lock().await.unwrap();
    assert!(registry.consume(id).is_err());
    assert!(context.current(&f.app).await.is_err());
    f.app
        .create_local_profile(
            "Different owned dependency profile".into(),
            "other owned dependency fixture passphrase 42".into(),
        )
        .await
        .unwrap();
    assert!(context.current(&f.app).await.is_err());
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_blocking_worker_retains_project_and_global_admission_until_actual_exit() {
    let _serial = serial().lock().await;
    let f = Fixture::new().await;
    let (registry, ticket) = f.ticket().await;
    let (op, reservation) = registry.admit(ticket).unwrap();
    let entered = Arc::new(AtomicBool::new(false));
    let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    registry
        .launch_worker({
            let release = release.clone();
            let entered = entered.clone();
            move || {
                let _reservation = reservation;
                entered.store(true, Ordering::Release);
                let (lock, cv) = &*release;
                let mut released = lock.lock().unwrap();
                while !*released {
                    released = cv.wait(released).unwrap();
                }
            }
        })
        .unwrap();
    while !entered.load(Ordering::Acquire) {
        tokio::task::yield_now().await;
    }
    registry.cancel(op.id).unwrap();
    let review = f.review().await;
    let competing = registry.consume(review.review_id).unwrap();
    assert!(
        registry.admit(competing).is_err(),
        "cancelled CPU task still owns its project's slot"
    );
    let joined = tokio::spawn({
        let registry = registry.clone();
        async move { registry.close_and_wait().await }
    });
    tokio::task::yield_now().await;
    assert!(!joined.is_finished());
    *release.0.lock().unwrap() = true;
    release.1.notify_all();
    joined.await.unwrap();
    assert!(
        registry.launch_worker(|| {}).is_err(),
        "closed registry rejects late worker registration"
    );
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn process_two_project_one_admission_is_nonqueued_and_one_use() {
    let _serial = serial().lock().await;
    let f = Fixture::new().await;
    let (registry, first) = f.ticket().await;
    let (_, a) = registry.admit(first).unwrap();
    let review = f.review().await;
    assert!(registry
        .admit(registry.consume(review.review_id).unwrap())
        .is_err());
    assert!(registry.consume(review.review_id).is_err());
    let project2 = f
        .app
        .automation_project_create("Second base".into(), None)
        .await
        .unwrap();
    let review = f
        .app
        .automation_dependency_review(project2.id, 1, f.runner.clone(), 1, selected())
        .await
        .unwrap();
    let (_, b) = registry
        .admit(registry.consume(review.review_id).unwrap())
        .unwrap();
    let project3 = f
        .app
        .automation_project_create("Third base".into(), None)
        .await
        .unwrap();
    let review = f
        .app
        .automation_dependency_review(project3.id, 1, f.runner.clone(), 1, selected())
        .await
        .unwrap();
    assert!(registry
        .admit(registry.consume(review.review_id).unwrap())
        .is_err());
    drop(a);
    drop(b);
    f.app.shutdown().await.unwrap();
}

#[test]
fn only_canonical_native_identifiers_are_accepted() {
    let id = Uuid::new_v4().to_string();
    assert!(canonical(&id).is_ok());
    for invalid_id in [
        Uuid::nil().to_string(),
        id.replace('-', ""),
        format!("{{{id}}}"),
        id.to_uppercase(),
        "https://arbitrary.example/".into(),
    ] {
        assert!(canonical(&invalid_id).is_err());
    }
}

#[tokio::test]
async fn saturated_early_cancel_tombstones_never_block_active_operation_cancel() {
    let _serial = serial().lock().await;
    let f = Fixture::new().await;
    let (registry, ticket) = f.ticket().await;
    let (op, reservation) = registry.admit(ticket).unwrap();
    for _ in 0..16 {
        registry.cancel(Uuid::new_v4()).unwrap();
    }
    assert!(registry.cancel(Uuid::new_v4()).is_err());
    registry.cancel(op.id).unwrap();
    assert!(op.cancel.is_cancelled());
    assert_eq!(value(&op.view())["phase"], "cancelled");
    drop(reservation);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn private_entry_futures_fit_desktop_stack() {
    let _serial = serial().lock().await;
    let f = Fixture::new().await;
    let review =
        f.app
            .automation_dependency_review(f.project.clone(), 1, f.runner.clone(), 1, selected());
    assert!(std::mem::size_of_val(&review) < 64 * 1024);
    drop(review);
    let start = f
        .app
        .automation_dependency_start(Uuid::new_v4().to_string());
    assert!(std::mem::size_of_val(&start) < 64 * 1024);
    drop(start);
    let digest = "a".repeat(64);
    let operation = Uuid::new_v4().to_string();
    let changes = f
        .app
        .automation_dependency_changes(operation.clone(), digest.clone(), 0, 100);
    assert!(std::mem::size_of_val(&changes) < 64 * 1024);
    drop(changes);
    let file =
        f.app
            .automation_dependency_file(operation.clone(), digest.clone(), "playbook.yml".into());
    assert!(std::mem::size_of_val(&file) < 64 * 1024);
    drop(file);
    let accept = f
        .app
        .automation_dependency_accept(operation.clone(), digest.clone());
    assert!(std::mem::size_of_val(&accept) < 64 * 1024);
    drop(accept);
    let reject = f.app.automation_dependency_reject(operation, digest);
    assert!(std::mem::size_of_val(&reject) < 64 * 1024);
    drop(reject);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn candidate_review_actions_cannot_turn_unprepared_operation_into_accepted_or_touch_source() {
    let _serial = serial().lock().await;
    let f = Fixture::new().await;
    let (registry, ticket) = f.ticket().await;
    let before = ticket.context.base.revision().unwrap();
    let (op, reservation) = registry.admit(ticket).unwrap();
    let id = op.id.to_string();
    let digest = "a".repeat(64);
    assert!(f
        .app
        .automation_dependency_changes(id.clone(), digest.clone(), 0, 100)
        .await
        .is_err());
    assert!(f
        .app
        .automation_dependency_changes(id.clone(), digest.clone(), 0, 101)
        .await
        .is_err());
    assert!(f
        .app
        .automation_dependency_file(id.clone(), digest.clone(), "../outside".into())
        .await
        .is_err());
    assert!(f
        .app
        .automation_dependency_file(id.clone(), digest.clone(), "playbook.yml".into())
        .await
        .is_err());
    assert!(f
        .app
        .automation_dependency_accept(id.clone(), digest.clone())
        .await
        .is_err());
    assert!(f
        .app
        .automation_dependency_reject(id, digest)
        .await
        .is_err());
    assert_eq!(value(&op.view())["phase"], "opening");
    assert!(!op.cancel.is_cancelled());
    assert_eq!(op.context.base.revision().unwrap(), before);
    drop(reservation);
    f.app.shutdown().await.unwrap();
}
