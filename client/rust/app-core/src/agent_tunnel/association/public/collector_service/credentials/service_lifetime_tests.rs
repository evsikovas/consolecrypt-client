//! DRAFT/UNRUN actual AppCore Context/scope causals. No fake owner/Actor tuple.
//! Guard-causal coverage, not complete crypto/Node journal/runtime acceptance.
use super::*;
use cc_platform_core::{InMemorySecureStore, UnsupportedDeviceKeys};
const PASSWORD: &str = "owned original lifetime confirmation passphrase42";
struct Fixture { _root: tempfile::TempDir, app: AppCore, context: Context, credential: String }
impl Fixture {
    async fn new(ttl: Duration) -> Self {
        let root = tempfile::tempdir().unwrap();
        let app = AppCore::with_platform(crate::AppConfig::for_tests(root.path().to_string_lossy().into_owned()),
            Arc::new(InMemorySecureStore::new()), Arc::new(UnsupportedDeviceKeys)).unwrap();
        app.create_local_profile("Original lifetime owner A".into(), PASSWORD.into()).await.unwrap();
        let mut events = app.subscribe_events();
        let credential = app.add_password_credential("Explicit service ref".into(), Some("exporter".into()),
            "synthetic selected password42".into()).await.unwrap().id;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let crate::AppEvent::ObjectsChanged { kind: cc_models::ObjectKind::Credential,
                    ids, origin: crate::ChangeOriginDto::Local } = events.recv().await.unwrap() {
                    if ids.iter().any(|id| id == &credential) { break; }
                }
            }
        }).await.expect("actual selected-ref FIFO readback completes before capture");
        drop(events);
        let (owner, unlock) = app.unlocked().await.unwrap();
        let frame = Frame { version: 1, profile_id: owner.profile_id.to_string(),
            notification_scope_id: unlock.notices.scope_id().unwrap() };
        drop(unlock); drop(owner);
        // Single original fixture capture. Runtime helper NEVER calls capture.
        let cancel = CancellationToken::new();
        let scope = Arc::new(OriginalScope::capture_expected(&app, &cancel, ttl,
            &frame.profile_id, &frame.notification_scope_id).await.unwrap());
        let context = Context { core: Arc::downgrade(&app.inner), scope, frame, cancel,
            runtime: tokio::runtime::Handle::current() };
        Self { _root: root, app, context, credential }
    }
    fn publication(&self) -> AppResult<String> {
        OriginalLifetimeFrame::from_context(&self.context)?.publish(json!({"kind":"originalLifetimeGuardCausal"}))
    }
    async fn fresh_control(&self) -> Context {
        let (owner, unlock) = self.app.unlocked().await.unwrap();
        let frame = Frame { version: 1, profile_id: owner.profile_id.to_string(),
            notification_scope_id: unlock.notices.scope_id().unwrap() };
        drop(unlock); drop(owner);
        Context::capture(&self.app, frame).await.unwrap()
    }
}

#[tokio::test]
async fn original_lock_denies_loaded_operation_and_final_publication() {
    let f = Fixture::new(Duration::from_secs(60)).await;
    let original = OriginalLifetimeFrame::from_context(&f.context).unwrap();
    assert!(f.publication().is_ok());
    f.app.lock().await.unwrap();
    assert!(original.run(f.context.check()).await.is_err());
    assert!(original.publish(json!({"kind":"mustNotPublish"})).is_err());
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn original_unlock_generation_cannot_be_replaced_after_reunlock() {
    let f = Fixture::new(Duration::from_secs(60)).await;
    let original = OriginalLifetimeFrame::from_context(&f.context).unwrap();
    f.app.lock().await.unwrap();
    f.app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    assert!(original.run(f.context.check()).await.is_err());
    assert!(original.publish(json!({"kind":"mustNotPublish"})).is_err());
    let fresh = f.fresh_control().await;
    assert!(OriginalLifetimeFrame::from_context(&fresh).unwrap().publish(json!({"kind":"freshConsentControl"})).is_ok());
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn profile_a_to_b_to_a_does_not_recapture_auth_or_provenance() {
    let f = Fixture::new(Duration::from_secs(60)).await;
    let original = OriginalLifetimeFrame::from_context(&f.context).unwrap();
    let profile_a = f.context.frame.profile_id.clone();
    f.app.create_local_profile("Different actual owner B".into(), PASSWORD.into()).await.unwrap();
    assert_ne!(f.app.session().await.unwrap().profile_id.to_string(), profile_a);
    f.app.open_profile(profile_a.clone()).await.unwrap();
    f.app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    assert_eq!(f.app.session().await.unwrap().profile_id.to_string(), profile_a);
    assert!(original.run(f.context.check()).await.is_err());
    assert!(original.publish(json!({"kind":"mustNotPublish"})).is_err());
    let fresh = f.fresh_control().await;
    assert!(OriginalLifetimeFrame::from_context(&fresh).unwrap().publish(json!({"kind":"freshSeparateControl"})).is_ok());
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn actual_selected_credential_revision_invalidates_original_frame() {
    let f = Fixture::new(Duration::from_secs(60)).await;
    let original = OriginalLifetimeFrame::from_context(&f.context).unwrap();
    assert!(f.publication().is_ok());
    f.app.set_credential_password(f.credential.clone(), "synthetic replacement44".into()).await.unwrap();
    assert!(original.run(f.context.check()).await.is_err());
    assert!(original.publish(json!({"kind":"mustNotPublish"})).is_err());
    f.app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn held_actual_session_guard_cannot_extend_the_original_deadline() {
    let f = Fixture::new(Duration::from_millis(500)).await;
    let original = OriginalLifetimeFrame::from_context(&f.context).unwrap();
    let until = original.until;
    let held = f.app.inner.active.lock().await;
    assert!(tokio::time::timeout(Duration::from_secs(2), original.run(f.context.check())).await.unwrap().is_err());
    assert_eq!(original.until, until);
    drop(held);
    assert!(original.publish(json!({"kind":"mustNotPublish"})).is_err());
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn original_cancel_ends_pending_wait_without_claiming_worker_join() {
    let f = Fixture::new(Duration::from_secs(60)).await;
    let original = OriginalLifetimeFrame::from_context(&f.context).unwrap();
    let (_sender, receiver) = tokio::sync::oneshot::channel::<()>();
    let mut pending = Box::pin(original.run(async { receiver.await.map_err(|_| unavailable()) }));
    tokio::select! { result = &mut pending => panic!("wait ended before original cancel: {result:?}"),
        _ = tokio::task::yield_now() => {} }
    f.context.cancel.cancel();
    assert!(tokio::time::timeout(Duration::from_secs(1), pending).await.unwrap().is_err());
    assert!(original.publish(json!({"kind":"mustNotPublish"})).is_err());
    f.app.shutdown().await.unwrap();
}
