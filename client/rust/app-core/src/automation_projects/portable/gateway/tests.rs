use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const PASSWORD: &str = "owned gateway synthetic profile password 42";
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
        app.create_local_profile("Owned portable gateway".into(), PASSWORD.into())
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
    async fn probe(&self, dropped: Arc<AtomicUsize>, ttl: Duration) -> PortableOperation {
        self.app
            .portable_register(
                self.scope.clone(),
                Body::Probe(PayloadProbe {
                    secret: Some(SecretString::from(
                        "synthetic package password, never emitted",
                    )),
                    dropped,
                    owner: Arc::downgrade(&self.owner),
                }),
                ttl,
            )
            .await
            .unwrap()
    }
}
/// Observes ordering, without attempting to inspect zeroized allocator memory.
/// SecretString's own zeroize-on-drop is the existing crypto primitive contract.
pub(super) struct PayloadProbe {
    secret: Option<SecretString>,
    dropped: Arc<AtomicUsize>,
    owner: Weak<Session>,
}
impl Drop for PayloadProbe {
    fn drop(&mut self) {
        drop(self.secret.take());
        if let Some(owner) = self.owner.upgrade() {
            assert!(
                owned_setup::test_map_unlocked(&owner),
                "callback/body destructor must run outside setup-map mutex"
            );
            assert_eq!(
                owned_setup::test_count(&owner),
                1,
                "secret drop precedes reservation removal"
            );
            assert_eq!(
                owner.automation.prepare_slots.available_permits(),
                1,
                "secret drop precedes admission release"
            );
        }
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}
async fn joined(op: &PortableOperation) {
    tokio::time::timeout(Duration::from_secs(10), op.joined())
        .await
        .unwrap();
}
#[tokio::test]
async fn idle_cancel_wipes_body_synchronously_before_join_and_capacity_release() {
    let f = Fixture::new().await;
    let dropped = Arc::new(AtomicUsize::new(0));
    let op = f.probe(dropped.clone(), Duration::from_secs(300)).await;
    assert_eq!(owned_setup::test_count(&f.owner), 1);
    op.cancel();
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert!(op.control.joined.is_cancelled());
    assert_eq!(owned_setup::test_count(&f.owner), 0);
    assert_eq!(f.owner.automation.prepare_slots.available_permits(), 2);
    assert!(op.start().await.is_err());
    op.cancel();
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    drop(op);
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn idle_handle_drop_wipes_body_and_releases_existing_slot() {
    let f = Fixture::new().await;
    let dropped = Arc::new(AtomicUsize::new(0));
    let op = f.probe(dropped.clone(), Duration::from_secs(300)).await;
    let observed = op.control.joined.clone();
    drop(op);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert!(observed.is_cancelled());
    assert_eq!(owned_setup::test_count(&f.owner), 0);
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn idle_expiry_retires_body_without_start_or_tombstone() {
    let f = Fixture::new().await;
    let dropped = Arc::new(AtomicUsize::new(0));
    let op = f.probe(dropped.clone(), Duration::from_secs(1)).await;
    joined(&op).await;
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert_eq!(owned_setup::test_count(&f.owner), 0);
    assert!(op.start().await.is_err());
    drop(op);
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn actual_lock_wipes_idle_and_same_profile_unlock_never_revives_issued_owner() {
    let f = Fixture::new().await;
    let dropped = Arc::new(AtomicUsize::new(0));
    let op = f.probe(dropped.clone(), Duration::from_secs(300)).await;
    f.app.lock().await.unwrap();
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert!(op.control.joined.is_cancelled());
    f.app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    assert!(op.start().await.is_err());
    let next = f.app.automation_portable_scope().await.unwrap();
    assert_ne!(next.notification_scope_id, f.scope.notification_scope_id);
    assert_eq!(owned_setup::test_count(&f.owner), 0);
    drop(op);
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn legacy_id_cancel_denies_owned_operation_but_keeps_borrowed_tombstone_behavior() {
    let f = Fixture::new().await;
    let dropped = Arc::new(AtomicUsize::new(0));
    let op = f.probe(dropped.clone(), Duration::from_secs(300)).await;
    let id = op.control.slot.lock().unwrap().as_ref().unwrap().id();
    assert!(f
        .app
        .automation_runner_cancel_setup(id.to_string())
        .await
        .is_err());
    assert!(!op.control.cancel.is_cancelled());
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    op.cancel();
    let early = Uuid::new_v4().to_string();
    f.app
        .automation_runner_cancel_setup(early.clone())
        .await
        .unwrap();
    assert!(matches!(
        f.app
            .automation_runner_setup(early, cc_automation_core::SetupTarget::Native, true)
            .await,
        Err(AppError::Cancelled)
    ));
    assert_eq!(owned_setup::test_count(&f.owner), 0);
    drop(op);
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn issued_cancel_uses_original_core_after_current_core_holder_is_replaced() {
    let f = Fixture::new().await;
    let other = Fixture::new().await;
    let dropped = Arc::new(AtomicUsize::new(0));
    let op = f.probe(dropped.clone(), Duration::from_secs(300)).await;
    let current = std::sync::Mutex::new(f.app.clone());
    *current.lock().unwrap() = other.app.clone();
    let other_scope = current
        .lock()
        .unwrap()
        .clone()
        .automation_portable_scope()
        .await
        .unwrap();
    assert_ne!(other_scope.profile_id, f.scope.profile_id);
    op.cancel();
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert_eq!(owned_setup::test_count(&f.owner), 0);
    assert_eq!(owned_setup::test_count(&other.owner), 0);
    assert!(other.app.is_unlocked().await);
    drop(op);
    f.app.shutdown().await.unwrap();
    other.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn actual_context_export_inspect_use_existing_operations_and_one_use_owners() {
    let f = Fixture::new().await;
    let project = f
        .app
        .automation_project_create("owned context".into(), None)
        .await
        .unwrap();
    f.app
        .automation_project_save(
            project.id.clone(),
            "selected.yml".into(),
            None,
            "---\n- hosts: all\n  tasks: []\n".into(),
        )
        .await
        .unwrap();
    let catalog = f.app.automation_projects().await.unwrap();
    let revision = catalog
        .projects
        .iter()
        .find(|p| p.id == project.id)
        .unwrap()
        .revision
        .unwrap();
    let before = f
        .app
        .automation_project_files(project.id.clone())
        .await
        .unwrap();
    let op = f
        .app
        .portable_register_context(f.scope.clone(), project.id.clone(), revision)
        .await
        .unwrap();
    let result: Value = serde_json::from_str(&op.start().await.unwrap()).unwrap();
    assert_eq!(result["projectId"], project.id);
    joined(&op).await;
    assert!(op.start().await.is_err());
    let destination = f.directory.path().join("owned-gateway-export.ccproject");
    let request=json!({"scope":result["scope"],"projectId":project.id,"projectRevision":revision,"revisionDigest":result["revisionDigest"],"projectStamp":result["projectStamp"],"selectedPaths":["selected.yml"],"playbook":"selected.yml"}).to_string();
    let export = f
        .app
        .portable_register_export(
            request,
            SecretString::from("synthetic separate transfer password"),
            destination.clone(),
        )
        .await
        .unwrap();
    assert!(
        !destination.exists(),
        "registration ACK is not a file write/start"
    );
    let exported: Value = serde_json::from_str(&export.start().await.unwrap()).unwrap();
    joined(&export).await;
    assert_eq!(exported["version"], 1);
    assert!(destination.exists());
    assert!(export.start().await.is_err());
    let inspect = f
        .app
        .portable_register_inspect(
            f.scope.clone(),
            destination,
            SecretString::from("synthetic separate transfer password"),
        )
        .await
        .unwrap();
    let inspected: Value = serde_json::from_str(&inspect.start().await.unwrap()).unwrap();
    joined(&inspect).await;
    assert_eq!(inspected["encryptedSha256"], exported["encryptedSha256"]);
    assert_eq!(inspected["summary"]["files"], 1);
    assert_eq!(inspected["publicationSupported"], false);
    assert_eq!(inspected["reason"], "managed_publication_required");
    assert_eq!(
        f.app
            .automation_project_files(project.id.clone())
            .await
            .unwrap(),
        before
    );
    assert_eq!(
        serde_json::to_value(f.app.automation_projects().await.unwrap()).unwrap(),
        serde_json::to_value(catalog).unwrap()
    );
    assert_eq!(owned_setup::test_count(&f.owner), 0);
    assert_eq!(f.owner.automation.prepare_slots.available_permits(), 2);
    drop((op, export, inspect));
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn idle_registration_is_bounded_by_existing_prepare_admission() {
    let f = Fixture::new().await;
    let first = f
        .probe(Arc::new(AtomicUsize::new(0)), Duration::from_secs(300))
        .await;
    // Do not use the probe ordering assertion when TWO reservations are present.
    let second = f
        .app
        .portable_register_context(f.scope.clone(), Uuid::new_v4().to_string(), 1)
        .await
        .unwrap();
    assert!(f
        .app
        .portable_register_context(f.scope.clone(), Uuid::new_v4().to_string(), 1)
        .await
        .is_err());
    assert_eq!(owned_setup::test_count(&f.owner), 2);
    second.cancel();
    first.cancel();
    drop((first, second));
    f.app.shutdown().await.unwrap();
}
#[test]
fn queued_actual_worker_observes_owned_cancel_before_body_entry_and_retains_registration() {
    std::thread::spawn(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()
            .unwrap()
            .block_on(async {
                let f = Fixture::new().await;
                let dropped = Arc::new(AtomicUsize::new(0));
                let op = f.probe(dropped.clone(), Duration::from_secs(300)).await;
                let (body, registration) = op.control.take().unwrap();
                let binding = OwnedBinding {
                    registration: registration.clone(),
                };
                let subject = binding
                    .subject(&f.app, &f.scope, &op.control.cancel)
                    .await
                    .unwrap();
                drop(binding);
                let (entered_tx, entered_rx) = oneshot::channel();
                let (release_tx, release_rx) = std::sync::mpsc::channel();
                let blocking = tokio::task::spawn_blocking(move || {
                    entered_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                });
                entered_rx.await.unwrap();
                let entered = Arc::new(AtomicBool::new(false));
                let observed = entered.clone();
                let marker = f
                    .directory
                    .path()
                    .join("must-not-create-after-owned-cancel");
                let target = marker.clone();
                let mut worker = Box::pin(work(&subject, move |_| {
                    observed.store(true, Ordering::SeqCst);
                    std::fs::write(target, b"owned only").unwrap();
                    Ok(())
                }));
                std::future::poll_fn(|cx| match worker.as_mut().poll(cx) {
                    Poll::Pending => Poll::Ready(()),
                    Poll::Ready(_) => {
                        panic!("single occupied blocking slot must queue real worker")
                    }
                })
                .await;
                assert_eq!(f.owner.automation.io_slots.available_permits(), 3);
                op.cancel();
                drop(body);
                drop(registration);
                assert!(!op.control.joined.is_cancelled());
                assert_eq!(owned_setup::test_count(&f.owner), 1);
                release_tx.send(()).unwrap();
                blocking.await.unwrap();
                assert!(tokio::time::timeout(Duration::from_secs(10), &mut worker)
                    .await
                    .unwrap()
                    .is_err());
                drop(worker);
                drop(subject);
                joined(&op).await;
                assert!(!entered.load(Ordering::SeqCst));
                assert!(!marker.exists());
                assert_eq!(dropped.load(Ordering::SeqCst), 1);
                assert_eq!(owned_setup::test_count(&f.owner), 0);
                drop(op);
                f.app.shutdown().await.unwrap();
            });
    })
    .join()
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_actual_blocking_worker_fences_hold_slot_after_real_caller_task_abort() {
    let f = Fixture::new().await;
    let dropped = Arc::new(AtomicUsize::new(0));
    let op = f.probe(dropped.clone(), Duration::from_secs(300)).await;
    let (body, registration) = op.control.take().unwrap();
    let binding = OwnedBinding {
        registration: registration.clone(),
    };
    let subject = binding
        .subject(&f.app, &f.scope, &op.control.cancel)
        .await
        .unwrap();
    drop(binding);
    let (entered1_tx, entered1_rx) = oneshot::channel();
    let (entered2_tx, entered2_rx) = oneshot::channel();
    let (release1_tx, release1_rx) = std::sync::mpsc::channel();
    let (release2_tx, release2_rx) = std::sync::mpsc::channel();
    let caller = tokio::spawn(Invocation {
        future: Some(Box::pin(async move {
            let _body = body;
            let first = work(&subject, move |_| {
                entered1_tx.send(()).unwrap();
                release1_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                Ok(())
            });
            let second = work(&subject, move |_| {
                entered2_tx.send(()).unwrap();
                release2_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                Ok(())
            });
            let _ = tokio::join!(first, second);
            Ok(String::new())
        })),
        registration: Some(registration),
    });
    tokio::time::timeout(Duration::from_secs(10), entered1_rx)
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), entered2_rx)
        .await
        .unwrap()
        .unwrap();
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert!(!op.control.joined.is_cancelled());
    assert_eq!(owned_setup::test_count(&f.owner), 1);
    assert_eq!(f.owner.automation.prepare_slots.available_permits(), 1);
    release1_tx.send(()).unwrap();
    // Actual IO permit Drop proves first worker finished; the second real fence
    // still owns the shared registration and cannot be reported Joined.
    let three = tokio::time::timeout(
        Duration::from_secs(10),
        f.owner.automation.io_slots.clone().acquire_many_owned(3),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!op.control.joined.is_cancelled());
    assert_eq!(owned_setup::test_count(&f.owner), 1);
    release2_tx.send(()).unwrap();
    joined(&op).await;
    drop(three);
    assert_eq!(owned_setup::test_count(&f.owner), 0);
    assert_eq!(f.owner.automation.prepare_slots.available_permits(), 2);
    drop(op);
    f.app.shutdown().await.unwrap();
}

struct ReturnedRootDrop {
    root: Option<ProjectRoot>,
    dropping: Option<oneshot::Sender<()>>,
    release: std::sync::mpsc::Receiver<()>,
}
impl Drop for ReturnedRootDrop {
    fn drop(&mut self) {
        self.dropping.take().unwrap().send(()).unwrap();
        self.release.recv_timeout(Duration::from_secs(10)).unwrap();
        drop(self.root.take());
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn abandoned_real_worker_result_drops_its_owned_root_before_last_registration_joins() {
    let f = Fixture::new().await;
    let dropped = Arc::new(AtomicUsize::new(0));
    let op = f.probe(dropped.clone(), Duration::from_secs(300)).await;
    let (body, registration) = op.control.take().unwrap();
    let binding = OwnedBinding {
        registration: registration.clone(),
    };
    let subject = binding
        .subject(&f.app, &f.scope, &op.control.cancel)
        .await
        .unwrap();
    drop(binding);
    let (entered_tx, entered_rx) = oneshot::channel();
    let (return_tx, return_rx) = std::sync::mpsc::channel();
    let (dropping_tx, dropping_rx) = oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let own_root = f.directory.path().join("owned-returned-root");
    std::fs::create_dir(&own_root).unwrap();
    let mut worker = Box::pin(work(&subject, move |_| {
        let root = ProjectRoot::open(&own_root).map_err(|_| invalid())?;
        entered_tx.send(()).unwrap();
        return_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        Ok(ReturnedRootDrop {
            root: Some(root),
            dropping: Some(dropping_tx),
            release: release_rx,
        })
    }));
    std::future::poll_fn(|cx| match worker.as_mut().poll(cx) {
        Poll::Pending => Poll::Ready(()),
        Poll::Ready(_) => panic!("worker cannot finish before explicit return gate"),
    })
    .await;
    tokio::time::timeout(Duration::from_secs(10), entered_rx)
        .await
        .unwrap()
        .unwrap();
    // Receiver disappears, but keep external Subject alive until the worker has
    // passed its post-check. This isolates the failed-send result Drop seam,
    // rather than letting Subject.cancel reject T before send is attempted.
    drop(worker);
    return_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(10), dropping_rx)
        .await
        .unwrap()
        .unwrap();
    drop(body);
    drop(subject);
    drop(registration);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert!(
        !op.control.joined.is_cancelled(),
        "failed-send payload still holds a real root in its blocking destructor"
    );
    assert_eq!(owned_setup::test_count(&f.owner), 1);
    assert_eq!(f.owner.automation.prepare_slots.available_permits(), 1);
    release_tx.send(()).unwrap();
    joined(&op).await;
    assert_eq!(owned_setup::test_count(&f.owner), 0);
    assert_eq!(f.owner.automation.prepare_slots.available_permits(), 2);
    drop(op);
    f.app.shutdown().await.unwrap();
}

#[test]
fn invocation_drop_destroys_successfully_queued_real_root_result_before_its_root_registration() {
    std::thread::spawn(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()
            .unwrap()
            .block_on(async {
                let f = Fixture::new().await;
                let dropped = Arc::new(AtomicUsize::new(0));
                let op = f.probe(dropped.clone(), Duration::from_secs(300)).await;
                let (body, registration) = op.control.take().unwrap();
                let binding = OwnedBinding {
                    registration: registration.clone(),
                };
                let subject = binding
                    .subject(&f.app, &f.scope, &op.control.cancel)
                    .await
                    .unwrap();
                drop(binding);
                let (entered_tx, entered_rx) = oneshot::channel();
                let (return_tx, return_rx) = std::sync::mpsc::channel();
                let (dropping_tx, dropping_rx) = oneshot::channel();
                let (release_tx, release_rx) = std::sync::mpsc::channel();
                let own_root = f.directory.path().join("owned-successfully-queued-root");
                std::fs::create_dir(&own_root).unwrap();
                let mut invocation = Box::pin(Invocation {
                    future: Some(Box::pin(async move {
                        let _body = body;
                        let _result = work(&subject, move |_| {
                            let root = ProjectRoot::open(&own_root).map_err(|_| invalid())?;
                            entered_tx.send(()).unwrap();
                            return_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                            Ok(ReturnedRootDrop {
                                root: Some(root),
                                dropping: Some(dropping_tx),
                                release: release_rx,
                            })
                        })
                        .await?;
                        Ok(String::new())
                    })),
                    registration: Some(registration),
                });
                std::future::poll_fn(|cx| match invocation.as_mut().poll(cx) {
                    Poll::Pending => Poll::Ready(()),
                    Poll::Ready(_) => panic!("worker return gate must keep receiver pending"),
                })
                .await;
                tokio::time::timeout(Duration::from_secs(10), entered_rx)
                    .await
                    .unwrap()
                    .unwrap();
                // This sentinel cannot run until the real work closure has sent its
                // result and released its final worker fence: pool limit is ONE.
                let sentinel = tokio::task::spawn_blocking(|| ());
                return_tx.send(()).unwrap();
                tokio::time::timeout(Duration::from_secs(10), sentinel)
                    .await
                    .unwrap()
                    .unwrap();
                // Invocation is deliberately not polled again. The SUCCESSFUL send
                // result is still inside its actual oneshot receive future.
                assert!(!op.control.joined.is_cancelled());
                let dropper = tokio::task::spawn_blocking(move || drop(invocation));
                tokio::time::timeout(Duration::from_secs(10), dropping_rx)
                    .await
                    .unwrap()
                    .unwrap();
                assert!(
                    !op.control.joined.is_cancelled(),
                    "Invocation's root registration must outlive future/queued result destruction"
                );
                assert_eq!(owned_setup::test_count(&f.owner), 1);
                assert_eq!(f.owner.automation.prepare_slots.available_permits(), 1);
                release_tx.send(()).unwrap();
                tokio::time::timeout(Duration::from_secs(10), dropper)
                    .await
                    .unwrap()
                    .unwrap();
                joined(&op).await;
                assert_eq!(dropped.load(Ordering::SeqCst), 1);
                assert_eq!(owned_setup::test_count(&f.owner), 0);
                assert_eq!(f.owner.automation.prepare_slots.available_permits(), 2);
                drop(op);
                f.app.shutdown().await.unwrap();
            });
    })
    .join()
    .unwrap();
}
