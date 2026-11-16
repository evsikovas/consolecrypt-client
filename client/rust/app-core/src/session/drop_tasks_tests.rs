//! Actual tracked tasks, SQLCipher senders and original unlock destruction.
//! The existing managedTLS WAL/SHM5s oracle is intentionally unchanged.
use super::*;
use crate::AppCore;
const PASSWORD: &str = "owned unlock task retirement password42";
async fn fixture() -> (tempfile::TempDir, AppCore, Arc<Session>, Arc<Unlocked>) {
    let root = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        root.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("Owned unlock background lifecycle".into(), PASSWORD.into())
        .await
        .unwrap();
    let (owner, u) = app.unlocked().await.unwrap();
    (root, app, owner, u)
}
struct HeldSql {
    _storage: Storage,
    dropped: Option<tokio::sync::oneshot::Sender<()>>,
}
impl Drop for HeldSql {
    fn drop(&mut self) {
        if let Some(sender) = self.dropped.take() {
            let _ = sender.send(());
        }
    }
}
fn held_job(
    storage: Storage,
) -> (
    JoinHandle<()>,
    tokio::sync::oneshot::Receiver<()>,
    tokio::sync::oneshot::Receiver<()>,
) {
    let (entered, ready) = tokio::sync::oneshot::channel();
    let (dropped, observed) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let held = HeldSql {
            _storage: storage,
            dropped: Some(dropped),
        };
        let _ = entered.send(());
        std::future::pending::<()>().await;
        drop(held);
    });
    (task, ready, observed)
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bare_last_core_drop_aborts_only_actual_unlock_owned_tasks_and_retires_ai_writer() {
    let (_root, app, owner, u) = fixture().await;
    let control = owner.storage().unwrap();
    let profile = owner.profile_id;
    let (task1, ready1, dropped1) = held_job(control.clone());
    let (task2, ready2, dropped2) = held_job(control.clone());
    u.push_task(task1);
    lock_mutex(&u.engine_tasks).push(task2);
    ready1.await.unwrap();
    ready2.await.unwrap();
    let foreign = tokio::spawn(std::future::pending::<()>());
    let weak_core = Arc::downgrade(&app.inner);
    let weak_owner = Arc::downgrade(&owner);
    let weak_unlock = Arc::downgrade(&u);
    let weak_ai = Arc::downgrade(&u.ai);
    assert!(!u.ai.is_closed());
    assert!(!foreign.is_finished());
    drop(u);
    drop(owner);
    drop(app); // NO shutdown/lock/close.
    assert!(weak_core.upgrade().is_none());
    assert!(weak_owner.upgrade().is_none());
    assert!(weak_unlock.upgrade().is_none());
    tokio::time::timeout(Duration::from_secs(5), async {
        dropped1.await.unwrap();
        dropped2.await.unwrap();
        while weak_ai.upgrade().is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("owned original tasks/AI must actually release");
    assert_eq!(
        control.get_profile().await.unwrap().unwrap().profile_id,
        profile,
        "explicit control sender still owns live SQL worker"
    );
    assert!(
        !foreign.is_finished(),
        "no global or delegated-work cancellation"
    );
    foreign.abort();
    assert!(foreign.await.unwrap_err().is_cancelled());
    drop(control);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_lock_keeps_existing_shutdown_join_and_later_drop_is_idempotent() {
    let (_root, app, owner, u) = fixture().await;
    let (task, ready, dropped) = held_job(owner.storage().unwrap());
    u.push_task(task);
    ready.await.unwrap();
    let foreign = tokio::spawn(std::future::pending::<()>());
    app.lock().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), dropped)
        .await
        .unwrap()
        .unwrap();
    assert!(u.ai.is_closed());
    assert!(lock_mutex(&u.tasks).is_empty());
    assert!(lock_mutex(&u.engine_tasks).is_empty());
    drop(u);
    assert!(!foreign.is_finished());
    foreign.abort();
    assert!(foreign.await.unwrap_err().is_cancelled());
    drop(owner);
    app.shutdown().await.unwrap();
}
