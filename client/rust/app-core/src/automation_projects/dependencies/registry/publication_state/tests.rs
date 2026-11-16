//! State-transition payload () is DATA, not a forged Core archive/Ready witness.
use super::*;
#[test]
fn stale_retirement_never_clobbers_closed_terminal_or_a_different_one_use_claim() {
    let nonce = Uuid::new_v4();
    let mut state: State<()> = State::Terminal(Phase::ContextChanged, "original_closed");
    assert!(abandon_state(&mut state, nonce).is_none());
    assert!(matches!(
        state,
        State::Terminal(Phase::ContextChanged, "original_closed")
    ));
    state = accepted();
    drop(claim_state(&mut state, &"c".repeat(64), nonce).unwrap());
    assert!(abandon_state(&mut state, Uuid::new_v4()).is_none());
    assert!(matches!(state, State::Publishing(..)));
    assert!(abandon_state(&mut state, nonce).is_some());
    assert!(matches!(state, State::Terminal(Phase::RecoveryRequired, _)));
}
fn accepted() -> State<()> {
    State::Accepted(
        DependencyView {
            version: 1,
            operation_id: Uuid::new_v4(),
            phase: Phase::Accepted,
            error: None,
            cleanup: Cleanup::NotStarted,
            archive_verified: true,
            base_digest: Some("a".repeat(64)),
            candidate_digest: Some("b".repeat(64)),
            review_digest: Some("c".repeat(64)),
            change_count: 0,
            changes: vec![],
            changes_truncated: false,
        },
        Arc::new(()),
        Acceptance {
            nonce: Uuid::new_v4(),
            registry_owned: true,
        },
    )
}
#[test]
fn publication_claim_is_one_use_exact_digest_and_actual_custody_required() {
    let mut state = accepted();
    let nonce = Uuid::new_v4();
    assert!(claim_state(&mut state, &"d".repeat(64), nonce).is_err());
    if let State::Accepted(_, _, a) = &mut state {
        a.registry_owned = false;
    }
    assert!(claim_state(&mut state, &"c".repeat(64), nonce).is_err());
    if let State::Accepted(_, _, a) = &mut state {
        a.registry_owned = true;
    }
    drop(claim_state(&mut state, &"c".repeat(64), nonce).unwrap());
    assert!(claim_state(&mut state, &"c".repeat(64), nonce).is_err());
    let epoch = AtomicU64::new(1);
    assert!(mark_sql_commit(&mut state, &epoch, Uuid::new_v4(), true).is_err());
    assert!(mark_sql_commit(&mut state, &epoch, nonce, false).is_err());
    mark_sql_commit(&mut state, &epoch, nonce, true).unwrap();
    assert!(mark_sql_commit(&mut state, &epoch, nonce, true).is_err());
    assert_eq!(epoch.load(Ordering::Acquire), 2);
}
#[tokio::test]
async fn actual_sql_old_context_error_after_internal_commit_transition_cannot_retire_own_claim() {
    let root = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        root.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile(
        "Own commit state fixture".into(),
        "own commit state password 42".into(),
    )
    .await
    .unwrap();
    let id = app
        .automation_project_create("Original".into(), None)
        .await
        .unwrap()
        .id;
    let original = app.automation_project_context(&id).await.unwrap();
    let state = Arc::new(Mutex::new(accepted()));
    let epoch = Arc::new(AtomicU64::new(0));
    let nonce = Uuid::new_v4();
    drop(claim_state(&mut *state.lock().unwrap(), &"c".repeat(64), nonce).unwrap());
    let before_watch = epoch.load(Ordering::Acquire);
    let own_state = state.clone();
    let own_epoch = epoch.clone();
    original
        .owner
        .storage()
        .unwrap()
        .write(move |tx| -> AppResult<()> {
            let mut catalog: Catalog = tx.setting_get(PROJECTS)?.unwrap();
            let row = catalog.records.get_mut(&id).unwrap();
            row["revision"] = json!(2);
            row["name"] = json!("Actual SQL change");
            mark_sql_commit(&mut *own_state.lock().unwrap(), &own_epoch, nonce, true)?;
            tx.setting_set(PROJECTS, &catalog)?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(
        original.check_after_io(&app).await.is_err(),
        "the actual OLD-row asynchronous read really observes a changed SQL row"
    );
    assert!(watched_retirement(&mut *state.lock().unwrap(), &epoch, before_watch).is_none());
    assert!(matches!(
        &*state.lock().unwrap(),
        State::Publishing(
            _,
            _,
            Publishing {
                commit_pending: true,
                ..
            }
        )
    ));
    // Without an internal successful-transaction transition, the SAME old-row
    // observation still revokes; Publishing is not a blanket watcher bypass.
    let mut external = accepted();
    drop(claim_state(&mut external, &"c".repeat(64), nonce).unwrap());
    let external_epoch = AtomicU64::new(0);
    assert!(watched_retirement(&mut external, &external_epoch, 0).is_some());
    assert!(matches!(
        external,
        State::Terminal(Phase::ContextChanged, _)
    ));
    app.shutdown().await.unwrap();
}
#[test]
fn all_four_io_permits_survive_resources_drop_until_actual_staging_cleanup_joins() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    runtime.block_on(async {
        use std::{future::Future, task::Poll, time::Duration};
        let registry = Arc::new(DependencyRegistry::default());
        let project = Uuid::new_v4().to_string();
        let operation = Uuid::new_v4();
        registry
            .maps
            .lock()
            .unwrap()
            .projects
            .insert(project.clone(), operation);
        let io = Arc::new(Semaphore::new(4));
        let slot = io.clone().try_acquire_many_owned(4).unwrap();
        let global = Arc::new(Semaphore::new(1));
        let staging = tempfile::Builder::new()
            .prefix("owned-publishing-cleanup-")
            .tempdir()
            .unwrap();
        let path = staging.path().to_owned();
        let joined = Arc::new(JoinedCleanup::default());
        let resources = Resources {
            runtime: runtime.handle().clone(),
            connection: None,
            staging: Some(staging),
            reservation: Some(Reservation {
                registry: Arc::downgrade(&registry),
                project,
                operation,
                _global: global.clone().try_acquire_owned().unwrap(),
            }),
            cleanup: Arc::new(AtomicU8::new(0)),
            remote: false,
            publication_io: Mutex::new(Some(slot)),
            joined: joined.clone(),
        };
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let blocker = tokio::task::spawn_blocking(move || {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        });
        tokio::time::timeout(Duration::from_secs(5), entered_rx)
            .await
            .unwrap()
            .unwrap();
        drop(resources);
        let mut waiting = Box::pin(joined.wait());
        std::future::poll_fn(|cx| match waiting.as_mut().poll(cx) {
            Poll::Pending => Poll::Ready(()),
            Poll::Ready(_) => panic!("cleanup cannot finish behind occupied blocking worker"),
        })
        .await;
        assert_eq!(io.available_permits(), 0);
        assert_eq!(global.available_permits(), 0);
        assert!(path.is_dir());
        release_tx.send(()).unwrap();
        blocker.await.unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(5), &mut waiting)
            .await
            .unwrap());
        assert_eq!(io.available_permits(), 4);
        assert_eq!(global.available_permits(), 1);
        assert!(!path.exists());
        registry.close_and_wait().await;
    });
}
