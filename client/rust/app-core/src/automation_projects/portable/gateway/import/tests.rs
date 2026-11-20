//! Genuine existing Native export/decrypt and original-owner SQL causality.
//! No fabricated package witness, public endpoint, ACL repair or user profile.
use super::*;
use std::sync::OnceLock;

const PROFILE_PASSWORD: &str = "owned import profile synthetic password42";
const PACKAGE_PASSWORD: &str = "separate owned synthetic package password";
fn serial() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(Default::default)
}
struct Fixture {
    directory: tempfile::TempDir,
    app: AppCore,
    owner: Arc<Session>,
    scope: PortableScope,
}
impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            directory.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile("Owned import fixture".into(), PROFILE_PASSWORD.into())
            .await
            .unwrap();
        let (owner, _) = app.unlocked().await.unwrap();
        let scope = app.automation_portable_scope().await.unwrap();
        Self {
            directory,
            app,
            owner,
            scope,
        }
    }
    async fn exported(&self) -> PathBuf {
        let project = self
            .app
            .automation_project_create("Original untouched".into(), None)
            .await
            .unwrap();
        self.app
            .automation_project_save(
                project.id.clone(),
                "selected.yml".into(),
                None,
                "---\n- hosts: all\n  tasks: []\n".into(),
            )
            .await
            .unwrap();
        let row = self
            .app
            .automation_projects()
            .await
            .unwrap()
            .projects
            .into_iter()
            .find(|p| p.id == project.id)
            .unwrap();
        let context = self
            .app
            .automation_portable_context(
                self.scope.clone(),
                project.id.clone(),
                row.revision.unwrap(),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        let request = json!({"scope":context.scope,"projectId":project.id,"projectRevision":context.project_revision,
            "revisionDigest":context.revision_digest,"projectStamp":context.project_stamp,"selectedPaths":["selected.yml"],"playbook":"selected.yml"}).to_string();
        let path = self.directory.path().join("owned-import.ccproject");
        self.app
            .automation_portable_export(
                request,
                SecretString::from(PACKAGE_PASSWORD),
                path.clone(),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        path
    }
    async fn preview(&self, path: &Path) -> PortableOperation {
        self.app
            .automation_portable_register_import_json(
                serde_json::to_string(&self.scope).unwrap(),
                path.to_string_lossy().into_owned(),
                "  New owned project  ".into(),
                PortablePasswordTransfer::from_bytes(PACKAGE_PASSWORD.as_bytes().to_vec()).unwrap(),
            )
            .await
            .unwrap()
    }
    async fn raw_catalog(&self) -> Option<Value> {
        self.owner
            .storage()
            .unwrap()
            .read(|tx| tx.setting_get(PROJECTS))
            .await
            .unwrap()
    }
}
async fn joined(op: &PortableOperation) {
    tokio::time::timeout(Duration::from_secs(10), op.joined())
        .await
        .unwrap();
}

#[tokio::test]
async fn genuine_ready_is_not_joined_or_catalog_project_and_cancel_releases_retained_owner() {
    let _serial = serial().lock().await;
    let f = Fixture::new().await;
    let path = f.exported().await;
    let before = f.raw_catalog().await;
    let bytes = std::fs::read(&path).unwrap();
    let op = f.preview(&path).await;
    let view: Value = serde_json::from_str(&op.start().await.unwrap()).unwrap();
    assert_eq!(view["requestedName"], "  New owned project  ");
    assert_eq!(view["name"], "New owned project");
    assert_eq!(view["encryptedSha256"], digest(&bytes));
    assert_eq!(view["summary"]["files"], 1);
    assert!(view["remainingMillis"]
        .as_u64()
        .is_some_and(|v| v > 0 && v <= 300_000));
    assert!(
        !op.control.joined.is_cancelled(),
        "preview does not retire its real witness"
    );
    assert_eq!(owned_setup::test_count(&f.owner), 1);
    assert_eq!(f.owner.automation.prepare_slots.available_permits(), 1);
    assert!(matches!(
        *op.control.phase.lock().unwrap(),
        Phase::ReadyImport(_)
    ));
    assert!(op.start().await.is_err());
    // A caller-edited review digest neither starts publication nor consumes Ready.
    assert!(op
        .accept_import(view["reviewId"].as_str().unwrap().into(), "0".repeat(64))
        .await
        .is_err());
    assert!(matches!(
        *op.control.phase.lock().unwrap(),
        Phase::ReadyImport(_)
    ));
    assert_eq!(f.raw_catalog().await, before);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    op.cancel();
    joined(&op).await;
    assert_eq!(owned_setup::test_count(&f.owner), 0);
    assert_eq!(f.owner.automation.prepare_slots.available_permits(), 2);
    assert_eq!(f.raw_catalog().await, before);
    drop(op);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn actual_lock_retires_genuine_ready_and_old_scope_cannot_accept_after_unlock() {
    let _serial = serial().lock().await;
    let f = Fixture::new().await;
    let path = f.exported().await;
    let before = f.raw_catalog().await;
    let op = f.preview(&path).await;
    let view: Value = serde_json::from_str(&op.start().await.unwrap()).unwrap();
    f.app.lock().await.unwrap();
    joined(&op).await;
    assert_eq!(owned_setup::test_count(&f.owner), 0);
    f.app
        .unlock_with_passphrase(PROFILE_PASSWORD.into())
        .await
        .unwrap();
    assert_ne!(
        f.app
            .automation_portable_scope()
            .await
            .unwrap()
            .notification_scope_id,
        f.scope.notification_scope_id
    );
    assert!(op
        .accept_import(
            view["reviewId"].as_str().unwrap().into(),
            view["reviewDigest"].as_str().unwrap().into()
        )
        .await
        .is_err());
    assert_eq!(f.raw_catalog().await, before);
    drop(op);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn queued_actual_sql_lock_does_not_hold_unlock_or_release_worker_registration_early() {
    let _serial = serial().lock().await;
    let f = Fixture::new().await;
    let storage = f.owner.storage().unwrap();
    let before = f.raw_catalog().await;
    let (entered, entered_rx) = tokio::sync::oneshot::channel();
    let (release, release_rx) = std::sync::mpsc::channel();
    let blocked = storage.clone();
    let blocker = tokio::spawn(async move {
        blocked
            .call(move |_| {
                entered.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(15)).unwrap();
                Ok::<_, cc_storage_core::StorageError>(())
            })
            .await
            .unwrap()
    });
    entered_rx.await.unwrap();
    let op = f
        .app
        .portable_register_context(f.scope.clone(), Uuid::new_v4().to_string(), 1)
        .await
        .unwrap();
    let (body, registration) = op.control.take().unwrap();
    drop(body);
    let binding = OwnedBinding {
        registration: registration.clone(),
    };
    let subject = binding
        .subject(&f.app, &f.scope, &op.control.cancel)
        .await
        .unwrap();
    drop(binding);
    let current = op.control.clone();
    let runtime = tokio::runtime::Handle::current();
    let (queued, queued_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        // Poll exactly once: on this actual storage worker queue, the SQL job
        // is enqueued before its oneshot await becomes Pending.
        work(&subject, move |fence| {
            runtime.block_on(async move {
                let mut future = Box::pin(sql(current, |_| {
                    Ok(SqlAck {
                        revision: 1,
                        row_sha: "0".repeat(64),
                    })
                }));
                std::future::poll_fn(|cx| {
                    assert!(future.as_mut().poll(cx).is_pending());
                    Poll::Ready(())
                })
                .await;
                queued.send(()).unwrap();
                let result = future.await;
                drop(fence);
                result
            })
        })
        .await
    });
    drop(registration);
    queued_rx.await.unwrap();
    // Lock takes unlocked.write and synchronously cancels this exact owner.
    // Its later shutdown/reseal legitimately waits for the blocked SQLite
    // worker. A retained hold_unlocked read guard would block THIS fence.
    let locking_app = f.app.clone();
    let locking = tokio::spawn(async move { locking_app.lock().await });
    tokio::time::timeout(Duration::from_secs(5), op.control.cancel.cancelled()).await.unwrap();
    assert!(f.owner.unlocked_opt().await.is_none());
    assert!(!op.control.joined.is_cancelled());
    assert_eq!(owned_setup::test_count(&f.owner), 1);
    assert_eq!(f.owner.automation.prepare_slots.available_permits(), 1);
    release.send(()).unwrap();
    blocker.await.unwrap();
    assert!(task.await.unwrap().is_err());
    tokio::time::timeout(Duration::from_secs(5), locking).await.unwrap().unwrap().unwrap();
    joined(&op).await;
    assert!(!op
        .control
        .import_durable
        .load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(owned_setup::test_count(&f.owner), 0);
    f.app
        .unlock_with_passphrase(PROFILE_PASSWORD.into())
        .await
        .unwrap();
    assert_eq!(f.raw_catalog().await, before);
    drop(op);
    f.app.shutdown().await.unwrap();
}
