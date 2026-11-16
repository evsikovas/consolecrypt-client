//! DRAFT: real Local AppCore/SQLCipher/Host/source/binding, not a fake context.
//! These cases exercise fences only; they do not approve an Agent or open SSH.
use super::*;
use cc_monitoring_core::{AuthRef, Route, SourceConfig, TlsConfig};

const PASSWORD: &str = "reader original context ephemeral password 42";
async fn fixture() -> (tempfile::TempDir, AppCore, ReaderSourceSelection) {
    let root = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        root.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("Reader context fixture".into(), PASSWORD.into())
        .await
        .unwrap();
    let host = app
        .save_host(crate::HostDto::new("Reader fixture", "127.0.0.1"))
        .await
        .unwrap();
    let view = app
        .monitoring_source_create(
            0,
            SourceConfig {
                id: String::new(),
                name: "Reader metadata only".into(),
                endpoint: "https://reader-fixture.example.invalid/prometheus".into(),
                route: Route::Direct,
                tls: TlsConfig::default(),
                auth: AuthRef::None,
            },
        )
        .await
        .unwrap();
    let source = view.sources[0].id.clone();
    let view = app
        .monitoring_binding_create(
            view.revision.unwrap(),
            "Reader explicit metadata binding".into(),
            host.id.clone(),
            source.clone(),
            1,
            BTreeMap::from([("job".into(), "reader-fixture".into())]),
        )
        .await
        .unwrap();
    // The real object-event worker processes store events FIFO. Subscribe
    // before a fresh sentinel write and wait for its post-refresh event, so
    // all earlier fixture writes/readbacks finish BEFORE the Reader baseline.
    // save_dto/put returns only after its own synchronous refresh, too.
    let mut events = app.subscribe_events();
    let sentinel = app
        .save_host(crate::HostDto::new("Reader fixture writer barrier", "127.0.0.2"))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let crate::dto::AppEvent::ObjectsChanged {
                kind: cc_models::ObjectKind::Host,
                origin: crate::dto::ChangeOriginDto::Local,
                ids,
            } = events.recv().await.unwrap()
            {
                if ids.len() == 1 && ids[0] == sentinel.id {
                    break;
                }
            }
        }
    })
    .await
    .expect("fixture writer did not cross its real object-event barrier");
    let selection = ReaderSourceSelection {
        profile_id: app.session().await.unwrap().profile_id.to_string(),
        host_id: host.id,
        source_id: source,
        source_revision: 1,
        binding_id: view.bindings[0].id.clone(),
        binding_revision: view.bindings[0].revision,
    };
    (root, app, selection)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn same_unlock_identical_snapshot_does_not_rebind_original_attempt() {
    let (_root, app, selection) = fixture().await;
    let a = ReaderOriginalContext::capture(&app, selection.clone(), &CancellationToken::new())
        .await
        .unwrap();
    let b = ReaderOriginalContext::capture(&app, selection, &CancellationToken::new())
        .await
        .unwrap();
    assert!(a.snapshot() == b.snapshot());
    let fence = a.scope_identity();
    let clone = fence.clone();
    assert!(fence.check(&a).is_ok());
    assert!(fence.check(&b).is_err());
    assert!(clone.check(&b).is_err());
    drop(a);
    assert!(!fence.is_live());
    assert!(!clone.is_live());
    assert!(b.scope_identity().check(&b).is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn original_cancel_lock_reunlock_and_private_deadline_invalidate_fence() {
    let (_root, app, selection) = fixture().await;
    let cancel = CancellationToken::new();
    let a = ReaderOriginalContext::capture(&app, selection.clone(), &cancel)
        .await
        .unwrap();
    let fence = a.scope_identity();
    cancel.cancel();
    assert!(!fence.is_live());
    assert!(fence.check(&a).is_err());
    tokio::time::timeout(Duration::from_millis(200), fence.cancelled())
        .await
        .unwrap();
    let b = ReaderOriginalContext::capture(&app, selection.clone(), &CancellationToken::new())
        .await
        .unwrap();
    let old = b.scope_identity();
    app.lock().await.unwrap();
    assert!(!old.is_live());
    app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    let mut fresh = ReaderOriginalContext::capture(&app, selection, &CancellationToken::new())
        .await
        .unwrap();
    assert!(old.check(&fresh).is_err());
    assert!(fresh.scope_identity().is_live());
    // Test-private monotonic expiry injection; there is no public clock input.
    fresh.deadline = Instant::now() - Duration::from_secs(1);
    let expired = fresh.scope_identity();
    assert!(!expired.is_live());
    assert!(expired.check(&fresh).is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocked_original_guard_acquisition_cancels_without_running_held_action() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let (_root, app, selection) = fixture().await;
    let cancel = CancellationToken::new();
    let ctx = ReaderOriginalContext::capture(&app, selection, &cancel)
        .await
        .unwrap();
    let owner = app.session().await.unwrap();
    let _block = app.hold_session(&owner).await.unwrap();
    let called = AtomicBool::new(false);
    let work = ctx.with_held(&app, |_| {
        called.store(true, Ordering::SeqCst);
        Ok(())
    });
    tokio::pin!(work);
    tokio::select! { biased;
        _ = &mut work => panic!("held action escaped original guard"),
        _ = tokio::time::sleep(Duration::from_millis(20)) => {},
    }
    cancel.cancel();
    assert!(tokio::time::timeout(Duration::from_millis(200), &mut work)
        .await
        .unwrap()
        .is_err());
    assert!(!called.load(Ordering::SeqCst));
}
