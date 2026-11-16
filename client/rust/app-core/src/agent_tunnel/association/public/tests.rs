//! Actual original Local profile/SQL/OS-call causals. No fake Agent ACK.
use super::*;
use cc_platform_core::{InMemorySecureStore, SecureStore, SecureStoreError, UnsupportedDeviceKeys};
const PASSWORD: &str = "owned association public fixture passphrase42";
async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    static SERIAL: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    SERIAL
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}
async fn fixture(secure: Arc<dyn SecureStore>) -> (tempfile::TempDir, AppCore, Value) {
    let root = tempfile::tempdir().unwrap();
    let app = AppCore::with_platform(
        crate::AppConfig::for_tests(root.path().to_string_lossy().into_owned()),
        secure,
        Arc::new(UnsupportedDeviceKeys),
    )
    .unwrap();
    app.create_local_profile("Owned association UI".into(), PASSWORD.into())
        .await
        .unwrap();
    let (owner, u) = app.unlocked().await.unwrap();
    let frame = json!({"version":1,"profileId":owner.profile_id.to_string(),"notificationScopeId":u.notices.scope_id().unwrap()});
    drop(u);
    drop(owner);
    (root, app, frame)
}
async fn host(app: &AppCore) -> (ObjectId, i64) {
    let mut h = crate::HostDto::new("Owned saved Native SSH", "127.0.0.1");
    h.host_key_policy = cc_ssh_core::HostKeyPolicy::Strict;
    save_host_fixture(app, h).await
}
async fn save_host_fixture(app: &AppCore, h: crate::HostDto) -> (ObjectId, i64) {
    // The writer's own awaited readback does not join the original FIFO event
    // task's second readback. Quiesce this exact fixture write before capturing
    // any original scope or reading the saved revision used by its handles.
    let mut events = app.subscribe_events();
    let h = app.save_host(h).await.unwrap();
    let id = crate::dto::parse_id("host", &h.id).unwrap();
    let saved_id = id.to_string();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let crate::AppEvent::ObjectsChanged {
                kind: cc_models::ObjectKind::Host,
                ids,
                origin: crate::ChangeOriginDto::Local,
            } = events.recv().await.expect("original object-event stream")
            {
                if ids.iter().any(|saved| saved == &saved_id) {
                    break;
                }
            }
        }
    })
    .await
    .expect("saved host FIFO readback completed before original scope capture");
    drop(events);
    let rev = app
        .unlocked()
        .await
        .unwrap()
        .1
        .working()
        .revision_of(id)
        .unwrap();
    (id, rev)
}
fn request(frame: &Value, host: ObjectId, rev: i64) -> String {
    let mut r = frame.clone();
    r["hostId"] = json!(host);
    r["expectedHostRevision"] = json!(rev.to_string());
    r.to_string()
}
fn value(raw: String) -> Value {
    serde_json::from_str(&raw).unwrap()
}
#[test]
fn closed_frame_and_revision_domains_do_not_coerce_caller_facts() {
    for raw in ["0", "01", "+1", "-1", "1.0", "9223372036854775808"] {
        assert!(revision(raw).is_err())
    }
    assert_eq!(revision("9223372036854775807").unwrap(), i64::MAX);
    assert!(
        parse::<Frame>(r#"{"version":1,"profileId":"x","notificationScopeId":"n","ownerPin":{}}"#)
            .is_err()
    );
    assert!(parse::<ProbeRequest>(r#"{"version":1,"profileId":"x","notificationScopeId":"n","hostId":"h","expectedHostRevision":1}"#).is_err());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_status_idle_registration_and_tls_review_serialize_metadata_without_key_creation() {
    let _serial = serial().await;
    let (_root, app, frame) = fixture(Arc::new(InMemorySecureStore::new())).await;
    let (id, rev) = host(&app).await;
    let status = app
        .agent_node_association_status_json(&frame.to_string())
        .await
        .unwrap();
    let parsed: Value = serde_json::from_str(&status).unwrap();
    assert_eq!(parsed["hosts"][0]["hostRevision"], rev.to_string());
    assert_eq!(parsed["clientIdentity"]["state"], "missing");
    assert_eq!(parsed["associations"], json!([]));
    let operation = app
        .agent_node_association_register_json(&request(&frame, id, rev))
        .await
        .unwrap();
    let idle = operation.details_json().await.unwrap();
    let v: Value = serde_json::from_str(&idle).unwrap();
    assert_eq!(v["phase"], "idle");
    assert_eq!(v["actorAdmission"], false);
    assert!(v["node"].is_null());
    assert!(
        v["remainingMs"]
            .as_u64()
            .is_some_and(|v| v > 0 && v <= 60000)
    );
    let identity = app
        .agent_node_client_identity_prepare_json(&frame.to_string())
        .await
        .unwrap();
    let review = identity.details_json().await.unwrap();
    assert_eq!(value(review.clone())["phase"], "createReview");
    assert!(value(review.clone())["identityId"].is_null());
    assert_eq!(
        value(
            app.agent_node_association_status_json(&frame.to_string())
                .await
                .unwrap()
        )["clientIdentity"]["state"],
        "missing",
        "prepare never generates a key or SQL reservation"
    );
    println!("CC_ASSOCIATION_SYNTHETIC_STATUS={status}");
    println!("CC_ASSOCIATION_SYNTHETIC_IDLE={idle}");
    println!("CC_ASSOCIATION_SYNTHETIC_TLS_REVIEW={review}");
    operation.cancel();
    identity.cancel();
    assert_eq!(
        value(operation.joined_json().await.unwrap())["cleanupState"],
        "closedObserved"
    );
    assert_eq!(
        value(identity.joined_json().await.unwrap())["cleanupState"],
        "closedObserved"
    );
    app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn original_nonce_and_actual_saved_revision_deny_registration_before_probe() {
    let _serial = serial().await;
    let (_root, app, frame) = fixture(Arc::new(InMemorySecureStore::new())).await;
    let (id, rev) = host(&app).await;
    let mut stale = frame.clone();
    stale["notificationScopeId"] = json!("old opaque nonce");
    assert!(
        app.agent_node_association_register_json(&request(&stale, id, rev))
            .await
            .is_err()
    );
    assert!(
        app.agent_node_association_register_json(&request(&frame, id, rev + 1))
            .await
            .is_err()
    );
    assert!(
        app.agent_node_client_identity_prepare_json(&stale.to_string())
            .await
            .is_err()
    );
    app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_tls_probe_fails_before_any_saved_endpoint_tcp() {
    let _serial = serial().await;
    let (_root, app, frame) = fixture(Arc::new(InMemorySecureStore::new())).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut h = crate::HostDto::new("Owned no TLS", "127.0.0.1");
    h.port = Some(listener.local_addr().unwrap().port());
    h.host_key_policy = cc_ssh_core::HostKeyPolicy::Strict;
    let (id, rev) = save_host_fixture(&app, h).await;
    let operation = app
        .agent_node_association_register_json(&request(&frame, id, rev))
        .await
        .unwrap();
    assert!(operation.probe().await.is_err());
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    );
    assert_eq!(
        value(operation.joined_json().await.unwrap())["cleanupState"],
        "closedObserved"
    );
    app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn idle_handles_do_not_retain_last_originating_core_or_unlock() {
    let _serial = serial().await;
    let (_root, app, frame) = fixture(Arc::new(InMemorySecureStore::new())).await;
    let (id, rev) = host(&app).await;
    let operation = app
        .agent_node_association_register_json(&request(&frame, id, rev))
        .await
        .unwrap();
    let identity = app
        .agent_node_client_identity_prepare_json(&frame.to_string())
        .await
        .unwrap();
    let weak = Arc::downgrade(&app.inner);
    let (owner, u) = app.unlocked().await.unwrap();
    let weak_owner = Arc::downgrade(&owner);
    let unlock = Arc::downgrade(&u);
    drop(u);
    drop(owner);
    drop(app);
    assert!(weak.upgrade().is_none());
    assert!(weak_owner.upgrade().is_none());
    assert!(unlock.upgrade().is_none());
    assert!(operation.details_json().await.is_err());
    assert!(identity.confirm_creation().await.is_err());
    operation.cancel();
    identity.cancel();
    assert_eq!(
        value(identity.joined_json().await.unwrap())["cleanupState"],
        "closedObserved"
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lock_reunlock_does_not_refresh_issued_handle_or_create_identity() {
    let _serial = serial().await;
    let (_root, app, frame) = fixture(Arc::new(InMemorySecureStore::new())).await;
    let identity = app
        .agent_node_client_identity_prepare_json(&frame.to_string())
        .await
        .unwrap();
    app.lock().await.unwrap();
    app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    assert!(identity.confirm_creation().await.is_err());
    assert!(identity.details_json().await.is_err());
    app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_creation_is_one_use_and_status_remains_stored_metadata_not_key_ready() {
    let _serial = serial().await;
    let (_root, app, frame) = fixture(Arc::new(InMemorySecureStore::new())).await;
    let identity = app
        .agent_node_client_identity_prepare_json(&frame.to_string())
        .await
        .unwrap();
    let stored = identity.confirm_creation().await.unwrap();
    let result = value(stored.clone());
    assert_eq!(result["phase"], "stored");
    assert!(result["identityId"].as_str().is_some());
    assert_eq!(
        value(identity.joined_json().await.unwrap())["cleanupState"],
        "closedObserved"
    );
    assert!(identity.confirm_creation().await.is_err());
    let status = value(
        app.agent_node_association_status_json(&frame.to_string())
            .await
            .unwrap(),
    );
    assert_eq!(status["clientIdentity"]["state"], "storedMetadata");
    assert_eq!(status["clientIdentity"]["identityId"], result["identityId"]);
    assert!(
        app.agent_node_client_identity_prepare_json(&frame.to_string())
            .await
            .is_err()
    );
    println!("CC_ASSOCIATION_SYNTHETIC_TLS_STORED={stored}");
    app.shutdown().await.unwrap();
}
#[derive(Debug)]
struct HeldStore {
    inner: InMemorySecureStore,
    entered: std::sync::mpsc::SyncSender<()>,
    release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
}
impl SecureStore for HeldStore {
    fn get(
        &self,
        name: &str,
    ) -> std::result::Result<Option<secrecy::SecretSlice<u8>>, SecureStoreError> {
        self.inner.get(name)
    }
    fn set(&self, name: &str, secret: &[u8]) -> std::result::Result<(), SecureStoreError> {
        if name.contains("/tunnel-tls/") {
            self.entered
                .send(())
                .map_err(|_| SecureStoreError::Corrupted)?;
            self.release
                .lock()
                .map_err(|_| SecureStoreError::Corrupted)?
                .recv_timeout(Duration::from_secs(12))
                .map_err(|_| SecureStoreError::Corrupted)?;
        }
        self.inner.set(name, secret)
    }
    fn delete(&self, name: &str) -> std::result::Result<bool, SecureStoreError> {
        self.inner.delete(name)
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_and_duplicate_confirm_preserve_actual_pending_os_job_until_observed_return() {
    let _serial = serial().await;
    let (entered, observed) = std::sync::mpsc::sync_channel(1);
    let (release, held) = std::sync::mpsc::sync_channel(1);
    let secure = Arc::new(HeldStore {
        inner: InMemorySecureStore::new(),
        entered,
        release: std::sync::Mutex::new(held),
    });
    let (_root, app, frame) = fixture(secure).await;
    let identity = Arc::new(
        app.agent_node_client_identity_prepare_json(&frame.to_string())
            .await
            .unwrap(),
    );
    let caller = {
        let identity = identity.clone();
        tokio::spawn(async move { identity.confirm_creation().await })
    };
    tokio::task::spawn_blocking(move || observed.recv_timeout(Duration::from_secs(10)).unwrap())
        .await
        .unwrap();
    assert!(
        identity.confirm_creation().await.is_err(),
        "replay loser cannot cancel winning creation"
    );
    assert!(!caller.is_finished());
    identity.cancel();
    assert!(!caller.is_finished(), "cancel is a request, not an OS join");
    assert_eq!(
        value(identity.joined_json().await.unwrap())["cleanupState"],
        "unknown"
    );
    assert!(!caller.is_finished());
    assert!(
        app.agent_node_client_identity_prepare_json(&frame.to_string())
            .await
            .is_err(),
        "durable pending reservation blocks replacement"
    );
    release.send(()).unwrap();
    assert!(caller.await.unwrap().is_err());
    let joined = value(identity.joined_json().await.unwrap());
    assert_eq!(joined["cleanupState"], "closedObserved");
    assert_eq!(joined["phase"], "unknown");
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn same_original_deadline_wins_held_guard_and_releases_idle_operation_permit() {
    let _serial = serial().await;
    let (_root, app, frame) = fixture(Arc::new(InMemorySecureStore::new())).await;
    let (id, rev) = host(&app).await;
    let cancel = CancellationToken::new();
    let scope = Arc::new(
        OriginalScope::capture_expected(
            &app,
            &cancel,
            Duration::from_millis(1000),
            frame["profileId"].as_str().unwrap(),
            frame["notificationScopeId"].as_str().unwrap(),
        )
        .await
        .unwrap(),
    );
    let context = Context {
        core: Arc::downgrade(&app.inner),
        scope,
        frame: parse(&frame.to_string()).unwrap(),
        cancel,
        runtime: tokio::runtime::Handle::current(),
    };
    let owner = context.owner().await.unwrap();
    let operation =
        NodeAssociationOperation::new(context, id, rev, review_permit().unwrap(), owner).unwrap();
    let held = app.inner.active.lock().await;
    assert!(
        tokio::time::timeout(Duration::from_secs(2), operation.details_json())
            .await
            .unwrap()
            .is_err(),
        "guard cannot extend original review lifetime"
    );
    drop(held);
    assert!(operation.probe().await.is_err());
    operation.cancel();
    assert_eq!(
        value(operation.joined_json().await.unwrap())["phase"],
        "expired"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_real_sql_read_cancels_inside_original_bound_without_waiting_for_worker() {
    let _serial = serial().await;
    let (_root, app, frame) = fixture(Arc::new(InMemorySecureStore::new())).await;
    let context = Context::capture(&app, parse(&frame.to_string()).unwrap())
        .await
        .unwrap();
    let custody = ManagedTlsCustody::capture(&app, context.scope.clone())
        .await
        .unwrap();
    let storage = app.session().await.unwrap().storage().unwrap();
    let (entered, observed) = std::sync::mpsc::sync_channel(1);
    let (release, held) = std::sync::mpsc::sync_channel(1);
    let worker = tokio::spawn(async move {
        storage
            .read(move |_| {
                entered.send(()).unwrap();
                held.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok::<_, cc_storage_core::StorageError>(())
            })
            .await
    });
    tokio::task::spawn_blocking(move || observed.recv_timeout(Duration::from_secs(5)).unwrap())
        .await
        .unwrap();
    let cancel = context.cancel.clone();
    let scope = context.scope.clone();
    let query = tokio::spawn(async move {
        let app = context.app()?;
        bounded_original(&scope, custody.stored_metadata(&app)).await
    });
    tokio::task::yield_now().await;
    cancel.cancel();
    assert!(
        tokio::time::timeout(Duration::from_secs(1), query)
            .await
            .unwrap()
            .unwrap()
            .is_err(),
        "original cancellation ends queued readonly wait while SAME worker is still held"
    );
    assert!(!worker.is_finished());
    release.send(()).unwrap();
    worker.await.unwrap().unwrap();
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_cancelled_original_carriers_keep_review_quota_until_actual_socket_destruction() {
    let _serial = serial().await;
    let (_root, app, frame) = fixture(Arc::new(InMemorySecureStore::new())).await;
    let context = Context::capture(&app, parse(&frame.to_string()).unwrap())
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let socket1 = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (peer1, _) = listener.accept().await.unwrap();
    let socket2 = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (peer2, _) = listener.accept().await.unwrap();
    let first = transport::Observation::new();
    let second = transport::Observation::new();
    first.retain_permit(review_permit().unwrap()).unwrap();
    second.retain_permit(review_permit().unwrap()).unwrap();
    let (release1, held1) = tokio::sync::oneshot::channel();
    let (release2, held2) = tokio::sync::oneshot::channel();
    let worker1 = tokio::spawn(
        transport::held_carrier_for_test(socket1, context.scope.clone(), first.clone(), held1)
            .unwrap(),
    );
    let worker2 = tokio::spawn(
        transport::held_carrier_for_test(socket2, context.scope.clone(), second.clone(), held2)
            .unwrap(),
    );
    context.cancel.cancel();
    first.abort();
    second.abort();
    first.release_closed();
    second.release_closed();
    assert_eq!(first.cleanup_state(), "pending");
    assert!(!worker1.is_finished());
    assert!(
        review_permit().is_err(),
        "cancel/Unknown cannot free capacity while actual resource still exists"
    );
    release1.send(()).unwrap();
    worker1.await.unwrap();
    assert_eq!(first.cleanup_state(), "closedObserved");
    let slot = review_permit().unwrap();
    assert!(review_permit().is_err());
    drop(slot);
    release2.send(()).unwrap();
    worker2.await.unwrap();
    assert_eq!(second.cleanup_state(), "closedObserved");
    let slot1 = review_permit().unwrap();
    let slot2 = review_permit().unwrap();
    drop((slot1, slot2, peer1, peer2));
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ended_carriers_do_not_free_quota_of_still_held_original_reviews() {
    let _serial = serial().await;
    let (_root, app, frame) = fixture(Arc::new(InMemorySecureStore::new())).await;
    let context = Context::capture(&app, parse(&frame.to_string()).unwrap())
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let socket1 = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (peer1, _) = listener.accept().await.unwrap();
    let socket2 = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (peer2, _) = listener.accept().await.unwrap();
    let first = transport::Observation::new();
    let second = transport::Observation::new();
    let original_review1 = first.retain_permit(review_permit().unwrap()).unwrap();
    let original_review2 = second.retain_permit(review_permit().unwrap()).unwrap();
    let (release1, held1) = tokio::sync::oneshot::channel();
    let (release2, held2) = tokio::sync::oneshot::channel();
    let worker1 = tokio::spawn(
        transport::held_carrier_for_test(socket1, context.scope.clone(), first.clone(), held1)
            .unwrap(),
    );
    let worker2 = tokio::spawn(
        transport::held_carrier_for_test(socket2, context.scope.clone(), second.clone(), held2)
            .unwrap(),
    );
    release1.send(()).unwrap();
    release2.send(()).unwrap();
    worker1.await.unwrap();
    worker2.await.unwrap();
    assert_eq!(first.cleanup_state(), "closedObserved");
    assert_eq!(second.cleanup_state(), "closedObserved");
    assert!(
        review_permit().is_err(),
        "observed carrier end alone cannot retire a still-held review"
    );
    drop(original_review1);
    let slot = review_permit().unwrap();
    assert!(review_permit().is_err());
    drop(slot);
    drop(original_review2);
    let slot1 = review_permit().unwrap();
    let slot2 = review_permit().unwrap();
    drop((slot1, slot2, peer1, peer2));
    app.shutdown().await.unwrap();
}

#[test]
fn revocation_public_request_never_accepts_caller_authority_or_unknown_fields() {
    let frame=json!({"version":1,"profileId":"p","notificationScopeId":"opaque nön-UUID",
        "associationId":"44444444-4444-4444-8444-444444444444"});
    assert!(parse::<super::revoke::RevokeRequest>(&frame.to_string()).is_ok());
    for field in ["ownerPin","certificate","key","now","hostId","approved","revoked"] {
        let mut bad=frame.clone();bad[field]=json!(true);
        assert!(parse::<super::revoke::RevokeRequest>(&bad.to_string()).is_err());
    }
    let mut missing=frame;missing.as_object_mut().unwrap().remove("notificationScopeId");
    assert!(parse::<super::revoke::RevokeRequest>(&missing.to_string()).is_err());
}
#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn revocation_old_original_nonce_denied_after_actual_lock_reunlock() {
    let _serial=serial().await;
    let (_root,app,frame)=fixture(Arc::new(InMemorySecureStore::new())).await;
    let original=Context::capture(&app,parse::<Frame>(&frame.to_string()).unwrap()).await.unwrap();
    app.lock().await.unwrap();app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    assert!(original.check().await.is_err());
    let mut request=frame;request["associationId"]=json!("44444444-4444-4444-8444-444444444444");
    assert!(app.agent_node_association_revoke_prepare_json(&request.to_string()).await.is_err());
    let (owner,u)=app.unlocked().await.unwrap();
    let fresh=json!({"version":1,"profileId":owner.profile_id.to_string(),"notificationScopeId":u.notices.scope_id().unwrap()});
    drop(u);drop(owner);
    assert!(Context::capture(&app,parse::<Frame>(&fresh.to_string()).unwrap()).await.is_ok());
    let status=value(app.agent_node_association_status_json(&fresh.to_string()).await.unwrap());
    assert_eq!(status["associations"],json!([]));
    // Fresh original admission works; missing association never adopts a pin or
    // creates a remote trust/bootstrap row. No SSH server/fake ACK in this test.
    let mut missing=fresh;missing["associationId"]=request["associationId"].clone();
    assert!(app.agent_node_association_revoke_prepare_json(&missing.to_string()).await.is_err());
    app.shutdown().await.unwrap();
}
#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn revocation_original_context_does_not_keep_idle_core_session_or_unlock() {
    let _serial=serial().await;
    let (_root,app,frame)=fixture(Arc::new(InMemorySecureStore::new())).await;
    let original=Context::capture(&app,parse::<Frame>(&frame.to_string()).unwrap()).await.unwrap();
    let weak=Arc::downgrade(&app.inner);let (owner,u)=app.unlocked().await.unwrap();
    let weak_owner=Arc::downgrade(&owner);let weak_unlock=Arc::downgrade(&u);
    drop(u);drop(owner);drop(app);
    assert!(weak.upgrade().is_none());assert!(weak_owner.upgrade().is_none());assert!(weak_unlock.upgrade().is_none());
    assert!(original.check().await.is_err());
}

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn rotation_factories_require_original_nonce_and_never_adopt_missing_rows() {
    let _serial=serial().await;let(_root,app,frame)=fixture(Arc::new(InMemorySecureStore::new())).await;
    let original=Context::capture(&app,parse::<Frame>(&frame.to_string()).unwrap()).await.unwrap();
    let mut request=frame;request["associationId"]=json!("44444444-4444-4444-8444-444444444444");
    app.lock().await.unwrap();app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    assert!(original.check().await.is_err());
    assert!(app.agent_node_association_rotation_prepare_json(&request.to_string()).await.is_err());
    assert!(app.agent_node_association_rotation_reconcile_prepare_json(&request.to_string()).await.is_err());
    let(owner,u)=app.unlocked().await.unwrap();let fresh=json!({"version":1,"profileId":owner.profile_id.to_string(),
        "notificationScopeId":u.notices.scope_id().unwrap()});drop(u);drop(owner);
    let status=value(app.agent_node_association_status_json(&fresh.to_string()).await.unwrap());assert_eq!(status["associations"],json!([]));
    let mut valid=fresh;valid["associationId"]=request["associationId"].clone();
    assert!(app.agent_node_association_rotation_prepare_json(&valid.to_string()).await.is_err());
    assert!(app.agent_node_association_rotation_reconcile_prepare_json(&valid.to_string()).await.is_err());
    app.shutdown().await.unwrap();
}


fn metrics_manifest_for_host(host:ObjectId)->Value {
    json!({"schema_version":1,"mode":"remote_write","labels":{"cc_host_id":host.to_string(),"site":"Lab","environment":"Test"},
        "scrape_interval_seconds":15,"targets":[{"id":"node","url":"http://127.0.0.1:9100/metrics","kind":"node_exporter",
            "sample_limit":1000,"series_limit":1000,"response_bytes":1048576,"label_limit":16}],
        "receiver":{"url":"http://127.0.0.1:8428/api/v1/write"},"buffer":{"max_bytes":33554432,"max_age_seconds":300}})
}
#[test]
fn metrics_factory_purpose_is_singleton_and_never_accepts_caller_authority() {
    assert!(AssociationPurpose::Metrics.check_scopes(&Scopes(vec![Scope::MetricsCollect])).is_ok());
    for scopes in [Scopes(vec![Scope::TunnelOpen]),Scopes(vec![Scope::TelemetryRead]),
        Scopes(vec![Scope::MetricsCollect,Scope::TunnelOpen]),Scopes(vec![])] {
        assert!(AssociationPurpose::Metrics.check_scopes(&scopes).is_err());
    }
    assert!(AssociationPurpose::Tunnel.check_scopes(&Scopes(vec![Scope::MetricsCollect])).is_err());
    let original=json!({"version":1,"profileId":"opaque","notificationScopeId":"original",
        "hostId":"11111111-1111-4111-8111-111111111111","expectedHostRevision":"1"});
    assert!(parse::<ProbeRequest>(&original.to_string()).is_ok());
    for field in ["scope","purpose","collectorId","manifest","ownerPin","grant","approved","now","path","url"] {
        let mut bad=original.clone();bad[field]=json!(true);
        assert!(parse::<ProbeRequest>(&bad.to_string()).is_err());
    }
}
#[test]
fn metrics_collector_shared_manifest_hash_binds_actual_saved_host_and_exact_schema() {
    let host=crate::dto::parse_id("host","11111111-1111-4111-8111-111111111111").unwrap();
    let other=crate::dto::parse_id("host","22222222-2222-4222-8222-222222222222").unwrap();
    let manifest=metrics_manifest_for_host(host);
    let digest=validate_metrics_manifest(&manifest,&host.to_string()).unwrap();
    let collector=protocol::MetricsCollector{collector_id:metrics_collector_id(digest).unwrap(),
        manifest_sha256:digest,manifest:manifest.clone()};
    assert!(collector.check(host).is_ok());assert!(collector.check(other).is_err());
    let mut retarget=collector.clone();retarget.manifest["scrape_interval_seconds"]=json!(30);
    let changed_digest=validate_metrics_manifest(&retarget.manifest,&host.to_string()).unwrap();
    retarget.manifest_sha256=changed_digest;
    let changed_id=metrics_collector_id(changed_digest).unwrap();
    assert_ne!(retarget.collector_id,changed_id);
    assert!(retarget.check(host).is_err(),"old lease ResourceId cannot designate a changed manifest even with its correct new full SHA");
    retarget.collector_id=changed_id;assert!(retarget.check(host).is_ok());
    let mut changed=collector.clone();changed.manifest_sha256=Hash::from_bytes([0;32]);assert!(changed.check(host).is_err());
    for (field,replacement) in [("schema_version",json!(2)),("mode",json!("caller_exec")),
        ("buffer",json!({"max_bytes":33554433,"max_age_seconds":300})),("receiver",json!({"url":"http://example.com:8428/api/v1/write"}))] {
        let mut bad=manifest.clone();bad[field]=replacement;
        assert!(validate_metrics_manifest(&bad,&host.to_string()).is_err());
    }
    let mut bad=collector.public();bad["collectorId"]=json!(Uuid::nil());
    assert!(serde_json::from_value::<protocol::MetricsCollector>(bad).is_err());
    let mut bad=manifest;bad["callerAuthority"]=json!(true);
    assert!(validate_metrics_manifest(&bad,&host.to_string()).is_err());
}
#[test]
fn metrics_probe_has_exact_seven_fields_and_requires_explicit_collector_null_or_object() {
    // Public schema fixture only: no signed reply, Node custody or Actor ACK is
    // manufactured. Scope/certificate acceptance is tested by real endpoints.
    let route:TunnelRoutePolicy=serde_json::from_str(include_str!("../../../../../agent-node-wire/src/tunnel_policy_vector.json")).unwrap();
    let key=PublicKey::from_bytes([7;32]);
    let pin=OwnerPin{context:route.context,key,epoch:1,checkpoint:2,expires_at:Timestamp::new(2000).unwrap()};
    let node=protocol::NodeIdentity{format:1,agent_id:route.agent_id,key_epoch:1,public_key:key,key_fingerprint:key.fingerprint()};
    let descriptor=json!({"node":node,"ownerPin":pin,"actorRevision":3,"certificate":null,"revoked":false,
        "advertisedScopes":["metrics.collect"],"collector":null});
    let probe:protocol::MetricsProbe=serde_json::from_value(descriptor.clone()).unwrap();assert!(probe.collector.is_none());
    for field in ["collector","certificate","advertisedScopes"] {
        let mut bad=descriptor.clone();bad.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<protocol::MetricsProbe>(bad).is_err());
    }
    let mut bad=descriptor;bad["accepted"]=json!(true);
    assert!(serde_json::from_value::<protocol::MetricsProbe>(bad).is_err());
}
#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn actual_metrics_idle_factory_labels_purpose_without_tls_key_or_remote_authority() {
    let _serial=serial().await;
    let(_root,app,frame)=fixture(Arc::new(InMemorySecureStore::new())).await;
    let(id,rev)=host(&app).await;
    let metrics=app.clone().agent_metrics_node_association_register_json(&request(&frame,id,rev)).await.unwrap();
    let view=value(metrics.details_json().await.unwrap());
    assert_eq!(view["purpose"],"metrics");assert_eq!(view["scopes"],json!(["metrics.collect"]));
    assert_eq!(view["phase"],"idle");assert_eq!(view["actorAdmission"],false);
    assert!(view["advertisedScopes"].is_null());assert!(view["collector"].is_null());
    let tunnel=app.agent_node_association_register_json(&request(&frame,id,rev)).await.unwrap();
    assert_eq!(value(tunnel.details_json().await.unwrap())["purpose"],"tunnel");
    let status=value(app.agent_node_association_status_json(&frame.to_string()).await.unwrap());
    assert_eq!(status["clientIdentity"]["state"],"missing");assert_eq!(status["associations"],json!([]));
    metrics.cancel();tunnel.cancel();
    assert_eq!(value(metrics.joined_json().await.unwrap())["cleanupState"],"closedObserved");
    assert_eq!(value(tunnel.joined_json().await.unwrap())["cleanupState"],"closedObserved");
    app.shutdown().await.unwrap();
}
#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn actual_collection_original_window_is_bounded_and_cannot_keep_or_replace_core() {
    let _serial=serial().await;
    let(_root,app,frame)=fixture(Arc::new(InMemorySecureStore::new())).await;
    for seconds in [0,29,301] {
        assert!(Context::capture_collection(&app,parse(&frame.to_string()).unwrap(),Duration::from_secs(seconds)).await.is_err());
    }
    let context30=Context::capture_collection(&app,parse(&frame.to_string()).unwrap(),Duration::from_secs(30)).await.unwrap();
    let original=Context::capture_collection(&app,parse(&frame.to_string()).unwrap(),Duration::from_secs(300)).await.unwrap();
    assert!(context30.scope.deadline()<=Instant::now()+Duration::from_secs(30));
    assert!(original.scope.deadline()<=Instant::now()+Duration::from_secs(300));
    app.lock().await.unwrap();app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    assert!(context30.check().await.is_err());assert!(original.check().await.is_err());
    let(owner,u)=app.unlocked().await.unwrap();let fresh=json!({"version":1,"profileId":owner.profile_id.to_string(),
        "notificationScopeId":u.notices.scope_id().unwrap()});drop(u);drop(owner);
    let fresh=Context::capture_collection(&app,parse(&fresh.to_string()).unwrap(),Duration::from_secs(300)).await.unwrap();
    let weak=Arc::downgrade(&app.inner);drop(app);assert!(weak.upgrade().is_none());assert!(fresh.check().await.is_err());
}

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn actual_metrics_registration_releases_sole_core_before_host_metadata_wait() {
    let _serial=serial().await;
    let(_root,app,frame)=fixture(Arc::new(InMemorySecureStore::new())).await;
    let(id,rev)=host(&app).await;
    let weak=Arc::downgrade(&app.inner);
    let(owner,unlocked)=app.unlocked().await.unwrap();
    let weak_owner=Arc::downgrade(&owner);let weak_unlocked=Arc::downgrade(&unlocked);
    drop(unlocked);drop(owner);
    // No supporting Core clone: after original capture the consuming factory
    // must drop its sole caller Core before metadata/SQL and fail closed.
    let result=app.agent_metrics_node_association_register_json(&request(&frame,id,rev)).await;
    assert!(result.is_err());assert!(weak.upgrade().is_none());
    assert!(weak_owner.upgrade().is_none());assert!(weak_unlocked.upgrade().is_none());
}
