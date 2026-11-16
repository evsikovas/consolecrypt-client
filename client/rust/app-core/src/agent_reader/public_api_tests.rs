//! DRAFT: actual Local AppCore / loaded device / opaque handle and private files.
//! No Actor install ACK, SSH, human UI acceptance or live telemetry is fabricated.
use super::*;
use cc_monitoring_core::{AuthRef, Route, SourceConfig, TlsConfig};
use std::collections::BTreeMap;
use std::path::PathBuf;
const PASSWORD: &str = "reader original context ephemeral password 42";
pub(super) async fn fixture() -> (tempfile::TempDir, AppCore, ReaderSourceSelection) {
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
        .save_host(crate::HostDto::new(
            "Reader fixture writer barrier",
            "127.0.0.2",
        ))
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

pub(super) fn request(
    selection: &ReaderSourceSelection,
    pin: &Path,
    hello: &Path,
    notice: &str,
) -> String {
    json!({"notificationScopeId":notice,"selection":{"profileId":selection.profile_id,"hostId":selection.host_id,
        "sourceId":selection.source_id,"sourceRevision":selection.source_revision,
        "bindingId":selection.binding_id,"bindingRevision":selection.binding_revision,
        "knownHostId":"77777777-7777-4777-8777-777777777777"},
        "action":{"kind":"nodeOffer","ownerPinFile":pin.to_string_lossy(),"nodeHelloFile":hello.to_string_lossy(),
        "offerExpiresAt":stamp().unwrap().seconds()+300}}).to_string()
}
pub(super) async fn files(app: &AppCore, root: &Path) -> (PathBuf, PathBuf) {
    let owner = app.session().await.unwrap().identity();
    let node = cc_crypto_core::DeviceSecretKeys::generate().unwrap();
    let pin = node::OwnerPin {
        context: node::Context {
            tenant: node::Tenant::LocalProfile {
                id: node::LocalProfileId::new("11111111-1111-4111-8111-111111111111").unwrap(),
            },
            deployment_id: node::DeploymentId::new("22222222-2222-4222-8222-222222222222").unwrap(),
        },
        key: node::PublicKey::from_bytes(owner.public_keys().signing),
        epoch: 1,
        checkpoint: 0,
        expires_at: stamp().unwrap().checked_add(10000).unwrap(),
    };
    let key = node::PublicKey::from_bytes(node.public_keys().signing);
    let a = root.join("owner-pin.json");
    let b = root.join("hello.json");
    fs::write(&a, serde_json::to_vec(&pin).unwrap()).unwrap();
    fs::write(&b,json!({"format":1,"agentId":"33333333-3333-4333-8333-333333333333","keyEpoch":1,"publicKey":key,"keyFingerprint":key.fingerprint()}).to_string()).unwrap();
    (a, b)
}
#[test]
fn public_reader_input_denies_raw_grant_duplicate_and_unknown_purposes() {
    assert!(decode::<Action>(br#"{"kind":"grant","lease":{}}"#).is_err());
    assert!(decode::<Action>(br#"{"kind":"issuerInit","kind":"read"}"#).is_err());
    assert!(decode::<Action>(br#"{"kind":"issuerInit","approved":true}"#).is_err());
    assert!(decode::<Action>(&vec![b' '; 16385]).is_err());
    assert!(matches!(
        decode::<Action>(br#"{"kind":"issuerInit"}"#).unwrap(),
        Action::IssuerInit {}
    ));
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn public_reader_actual_owner_handle_exports_private_separate_files_once() {
    let (root, app, selection) = fixture().await;
    let (a, b) = files(&app, root.path()).await;
    assert_eq!(app.inner.reader_native.public_counts().unwrap(), (0, 0));
    let handle = app
        .reader_prepare_action_json(&request(&selection, &a, &b, &live_nonce(&app).await))
        .await
        .unwrap();
    assert_eq!(app.inner.reader_native.public_counts().unwrap(), (1, 0));
    let details = handle.details_json().unwrap();
    assert!(details.contains("telemetry.read"));
    assert!(!details.contains("invitationToken"));
    let result: Value = serde_json::from_str(&handle.confirm_json().await.unwrap()).unwrap();
    assert_eq!(result["actorApply"], "notObserved");
    assert_eq!(app.inner.reader_native.public_counts().unwrap(), (1, 0));
    let public = PathBuf::from(result["publicArtifactPath"].as_str().unwrap());
    let private = PathBuf::from(result["privateInvitationPath"].as_str().unwrap());
    assert_ne!(public, private);
    let signed: node::Signed<node::Offer> =
        serde_json::from_slice(&fs::read(&public).unwrap()).unwrap();
    let pin: node::OwnerPin = witness(a.to_str().unwrap()).unwrap();
    node::verify_statement(&signed, pin.key).unwrap();
    let bytes = fs::read(&private).unwrap();
    assert_eq!(bytes.len(), 64);
    let mut raw = [0u8; 32];
    hex::decode_to_slice(bytes, &mut raw).unwrap();
    assert_eq!(
        node::InvitationToken::from_bytes(raw).unwrap().digest(),
        signed.body.token_hash
    );
    assert!(handle.confirm_json().await.is_err());
    assert!(private.exists());
    handle.cancel();
    assert!(!public.exists() && !private.exists());
    assert_eq!(app.inner.reader_native.public_counts().unwrap(), (0, 0));
    app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn public_reader_original_lock_denies_confirm_and_releases_metadata_slot() {
    let (root, app, selection) = fixture().await;
    let (a, b) = files(&app, root.path()).await;
    let handle = app
        .reader_prepare_action_json(&request(&selection, &a, &b, &live_nonce(&app).await))
        .await
        .unwrap();
    app.lock().await.unwrap();
    assert!(handle.confirm_json().await.is_err());
    handle.cancel();
    assert_eq!(app.inner.reader_native.public_counts().unwrap(), (0, 0));
    app.shutdown().await.unwrap();
}

pub(super) async fn live_nonce(app: &AppCore) -> String {
    app.session()
        .await
        .unwrap()
        .unlocked()
        .await
        .unwrap()
        .notices
        .scope_id()
        .unwrap()
}
fn selection_json(s: &ReaderSourceSelection) -> Value {
    json!({"profileId":s.profile_id,"hostId":s.host_id,"sourceId":s.source_id,"sourceRevision":s.source_revision,
        "bindingId":s.binding_id,"bindingRevision":s.binding_revision,"knownHostId":"77777777-7777-4777-8777-777777777777"})
}
#[test]
fn public_reader_missing_nonce_denied_and_opaque_nonce_is_exact_not_uuid() {
    let selection = json!({"profileId":"11111111-1111-4111-8111-111111111111","hostId":"22222222-2222-4222-8222-222222222222",
        "sourceId":"33333333-3333-4333-8333-333333333333","sourceRevision":1,"bindingId":"44444444-4444-4444-8444-444444444444",
        "bindingRevision":1,"knownHostId":"55555555-5555-4555-8555-555555555555"});
    assert!(decode::<Prepare>(
        json!({"selection":selection,"action":{"kind":"issuerInit"}})
            .to_string()
            .as_bytes()
    )
    .is_err());
    assert!(decode::<Status>(
        json!({"selection":selection,"association":null})
            .to_string()
            .as_bytes()
    )
    .is_err());
    let opaque = "unlock / opaque / 日本語 / é";
    let parsed: Prepare = decode(
        json!({"notificationScopeId":opaque,"selection":selection,"action":{"kind":"issuerInit"}})
            .to_string()
            .as_bytes(),
    )
    .unwrap();
    assert_eq!(parsed.notification_scope_id, opaque);
    // Current emitter creates UUIDs; this tests only the future-proof opaque comparator,
    // not fabricated AppCore authority or a claim that emitter currently emits Unicode.
    ReaderOriginalContext::notification_matches(opaque, Some(opaque)).unwrap();
    assert!(ReaderOriginalContext::notification_matches(
        opaque,
        Some("unlock / opaque / 日本語 / é")
    )
    .is_err());
    assert!(
        ReaderOriginalContext::notification_matches(&"é".repeat(129), Some(&"é".repeat(129)))
            .is_err()
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn public_reader_queued_old_nonce_same_profile_reopen_denied_fresh_nonce_prepares() {
    let (_root, app, selection) = fixture().await;
    let old = live_nonce(&app).await;
    let selected = selection_json(&selection);
    let queued =
        json!({"notificationScopeId":old,"selection":selected,"action":{"kind":"issuerInit"}})
            .to_string();
    app.lock().await.unwrap();
    app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    assert_eq!(
        app.session().await.unwrap().profile_id.to_string(),
        selection.profile_id
    );
    let fresh = live_nonce(&app).await;
    assert_ne!(old, fresh);
    assert!(app.reader_prepare_action_json(&queued).await.is_err());
    assert_eq!(app.inner.reader_native.public_counts().unwrap(), (0, 0));
    assert!(app
        .reader_status_json(
            &json!({"notificationScopeId":old,"selection":selected,"association":null}).to_string()
        )
        .await
        .is_err());
    assert_eq!(app.inner.reader_native.public_counts().unwrap(), (0, 0));
    let handle=app.reader_prepare_action_json(&json!({"notificationScopeId":fresh,"selection":selected,"action":{"kind":"issuerInit"}}).to_string()).await.unwrap();
    let details: Value = serde_json::from_str(&handle.details_json().unwrap()).unwrap();
    assert_eq!(details["details"]["native"]["notificationScopeId"], fresh);
    assert_eq!(app.inner.reader_native.public_counts().unwrap(), (1, 0));
    // Prepare only: no issuer commit or secret generation is confirmed.
    handle.cancel();
    assert_eq!(app.inner.reader_native.public_counts().unwrap(), (0, 0));
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn public_reader_transfer_is_owned_noclobber_and_keeps_original_export() {
    let (root, app, selection) = fixture().await;
    let (a, b) = files(&app, root.path()).await;
    let handle = app
        .reader_prepare_action_json(&request(&selection, &a, &b, &live_nonce(&app).await))
        .await
        .unwrap();
    let original: Value = serde_json::from_str(&handle.confirm_json().await.unwrap()).unwrap();
    let destination = root.path().join("copied-offer.json");
    let invitation = root.path().join("copied-invitation.hex");
    let result: Value = serde_json::from_str(
        &handle
            .transfer_export_json(
                destination.to_string_lossy().into_owned(),
                Some(invitation.to_string_lossy().into_owned()),
            )
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(result["actorApply"], "notObserved");
    assert_eq!(result["copiesIrreversible"], true);
    assert_eq!(
        fs::read(&destination).unwrap(),
        fs::read(original["publicArtifactPath"].as_str().unwrap()).unwrap()
    );
    assert_eq!(
        fs::read(&invitation).unwrap(),
        fs::read(original["privateInvitationPath"].as_str().unwrap()).unwrap()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&invitation).unwrap().permissions().mode() & 0o077,
            0
        );
    }
    assert!(handle
        .transfer_export_json(destination.to_string_lossy().into_owned(), None)
        .await
        .is_err());
    assert!(Path::new(original["publicArtifactPath"].as_str().unwrap()).exists());
    let copied = fs::read(&destination).unwrap();
    handle.cancel();
    assert_eq!(fs::read(&destination).unwrap(), copied);
    assert!(!Path::new(original["privateInvitationPath"].as_str().unwrap()).exists());
    app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn public_reader_transfer_original_lock_and_real_expiry_deny_destination_write() {
    let (root, app, selection) = fixture().await;
    let (a, b) = files(&app, root.path()).await;
    let handle = app
        .reader_prepare_action_json(&request(&selection, &a, &b, &live_nonce(&app).await))
        .await
        .unwrap();
    handle.confirm_json().await.unwrap();
    app.lock().await.unwrap();
    let denied = root.path().join("must-not-exist.json");
    assert!(handle
        .transfer_export_json(denied.to_string_lossy().into_owned(), None)
        .await
        .is_err());
    assert!(!denied.exists());
    drop(handle);
    app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    let mut request: Value =
        serde_json::from_str(&request(&selection, &a, &b, &live_nonce(&app).await)).unwrap();
    let expires = stamp().unwrap().seconds() + 8;
    request["action"]["offerExpiresAt"] = json!(expires);
    let handle = app
        .reader_prepare_action_json(&request.to_string())
        .await
        .unwrap();
    handle.confirm_json().await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while stamp().unwrap().seconds() < expires {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(handle
        .transfer_export_json(denied.to_string_lossy().into_owned(), None)
        .await
        .is_err());
    assert!(!denied.exists());
    handle.cancel();
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn public_reader_one_original_arc_node_and_facade_no_recapture_or_healthy_retirement() {
    let (root, app, selection) = fixture().await;
    let (pin, hello) = files(&app, root.path()).await;
    let handle = app
        .reader_prepare_action_json(&request(&selection, &pin, &hello, &live_nonce(&app).await))
        .await
        .unwrap();
    {
        let held = handle.shared.held.lock().unwrap();
        let Some(Held::Prepared(Prepared::Node { review, .. })) = held.as_ref() else {
            panic!("actual Node review missing")
        };
        assert!(review.uses_original(&handle.shared.original));
    }
    handle.confirm_json().await.unwrap();
    // Real Node review consumed/dropped; held public attempt must still export.
    handle.shared.original.check(&app).await.unwrap();
    handle.cancel();
    let selected = selection_json(&selection);
    let init = app.reader_prepare_action_json(&json!({"notificationScopeId":live_nonce(&app).await,"selection":selected,"action":{"kind":"issuerInit"}}).to_string()).await.unwrap();
    {
        let held = init.shared.held.lock().unwrap();
        let Some(Held::Prepared(Prepared::Issuer(review))) = held.as_ref() else {
            panic!("actual facade review missing")
        };
        assert!(review.uses_original(&init.shared.original));
    }
    // Successful actual issuer commit + healthy lease Drop must NOT retire the
    // borrowed public attempt before confirm's original publication fence.
    assert!(init.confirm_json().await.unwrap().contains("issuerReady"));
    init.shared.original.check(&app).await.unwrap();
    init.cancel();
    assert!(init.shared.original.check(&app).await.is_err());
    assert_eq!(app.inner.reader_native.public_counts().unwrap(), (0, 0));
    app.shutdown().await.unwrap();
}
#[cfg(unix)]
#[test]
#[allow(unsafe_code)] // test-only owned mkfifo; production opens remain safe std APIs.
fn public_reader_actual_fifo_witness_and_export_readback_deny_without_blocking() {
    use std::os::unix::ffi::OsStrExt;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("owned-fifo");
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // Only creates this test's FIFO; no writer, so a blocking open would hang.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let before = Instant::now();
    assert!(witness::<Value>(path.to_str().unwrap()).is_err());
    assert!(export::open_regular(&path).is_err());
    assert!(before.elapsed() < Duration::from_secs(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn public_reader_distinct_actual_captures_cannot_substitute_for_original_attempt() {
    let (_root, app, selection) = fixture().await;
    let cancel = CancellationToken::new();
    let nonce = live_nonce(&app).await;
    let a = Arc::new(
        ReaderOriginalContext::capture_for_notification(&app, selection.clone(), &cancel, &nonce)
            .await
            .unwrap(),
    );
    let b = Arc::new(
        ReaderOriginalContext::capture_for_notification(&app, selection.clone(), &cancel, &nonce)
            .await
            .unwrap(),
    );
    assert!(a.snapshot() == b.snapshot());
    assert!(a.scope_identity().check(&b).is_err());
    let native = Selection {
        profile_id: selection.profile_id.clone(),
        host_id: selection.host_id.clone(),
        source_id: selection.source_id.clone(),
        source_revision: selection.source_revision,
        binding_id: selection.binding_id.clone(),
        binding_revision: selection.binding_revision,
        known_host_id: "77777777-7777-4777-8777-777777777777".into(),
    }
    .native()
    .unwrap();
    let review = app
        .reader_prepare_issuer_initialization_from_original(native, a.clone())
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert!(review.uses_original(&a));
    assert!(!review.uses_original(&b));
    drop(review);
    a.retire();
    b.retire();
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn public_reader_details_repeated_delay_decreases_and_original_deadline_denies() {
    let (root, app, selection) = fixture().await;
    let (a, b) = files(&app, root.path()).await;
    let mut handle = app
        .reader_prepare_action_json(&request(&selection, &a, &b, &live_nonce(&app).await))
        .await
        .unwrap();
    // Shorten ONLY the owned test handle deadline. The real original context,
    // prepared review and OS clock are unchanged; no production renewal hook.
    let deadline = (Instant::now() + Duration::from_millis(200)).min(handle.shared.deadline);
    let held = handle.shared.held.lock().unwrap().take();
    handle.shared = Arc::new(Shared {
        transferring: AtomicBool::new(false),
        held: Mutex::new(held),
        running: Mutex::new(None),
        cancelled: handle.shared.cancelled.clone(),
        deadline,
        original: handle.shared.original.clone(),
    });
    let first: Value = serde_json::from_str(&handle.details_json().unwrap()).unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    let second: Value = serde_json::from_str(&handle.details_json().unwrap()).unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    let third: Value = serde_json::from_str(&handle.details_json().unwrap()).unwrap();
    assert!(
        first["remainingMilliseconds"].as_u64().unwrap()
            > second["remainingMilliseconds"].as_u64().unwrap()
    );
    assert!(
        second["remainingMilliseconds"].as_u64().unwrap()
            > third["remainingMilliseconds"].as_u64().unwrap()
    );
    assert_eq!(first["details"], third["details"]);
    assert_eq!(handle.shared.deadline, deadline);
    tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await;
    assert!(handle.details_json().is_err());
    assert!(handle.details_json().is_err());
    handle.cancel();
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn public_reader_idle_handle_does_not_retain_original_root_or_keys() {
    let (_root, app, selection) = fixture().await;
    let (inner, owner, unlock, key) = {
        let owner = app.session().await.unwrap();
        let unlock = owner.unlocked().await.unwrap();
        let key = owner.identity();
        (Arc::downgrade(&app.inner), Arc::downgrade(&owner),
            Arc::downgrade(&unlock), Arc::downgrade(&key))
    };
    let handle = app.reader_prepare_action_json(&json!({
        "notificationScopeId":live_nonce(&app).await,
        "selection":selection_json(&selection), "action":{"kind":"issuerInit"}
    }).to_string()).await.unwrap();
    assert!(handle.details_json().is_ok());
    // No cancel, shutdown, shortened timer, SSH or issuer commit before the
    // observation. Only the real opaque handle and Weak original owners remain.
    drop(app);
    let disappeared = tokio::time::timeout(Duration::from_secs(1), async {
        while inner.strong_count() != 0 || owner.strong_count() != 0
            || unlock.strong_count() != 0 || key.strong_count() != 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.is_ok();
    let remaining = (inner.strong_count(), owner.strong_count(),
        unlock.strong_count(), key.strong_count());
    let details_denied = handle.details_json().is_err();
    // On the RED implementation do NOT confirm a still-live IssuerInit: that
    // would commit an issuer. Confirm is exercised only after owner disappearance.
    let confirm_denied = if disappeared {
        Some(tokio::time::timeout(Duration::from_secs(1), handle.confirm_json())
            .await.expect("denial must be bounded").is_err())
    } else { None };
    // Cleanup follows all causal observations, including on the expected RED.
    handle.cancel();
    if let Some(inner) = inner.upgrade() {
        let _ = AppCore { inner }.shutdown().await;
    }
    assert!(disappeared, "idle Reader retained original owners: {remaining:?}");
    assert!(details_denied, "old handle details must deny after root drop");
    assert_eq!(confirm_denied, Some(true));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn public_reader_polled_original_waiters_do_not_retain_session_or_keys() {
    let (_root, app, selection) = fixture().await;
    let (inner, owner, unlock, key) = {
        let owner = app.session().await.unwrap();
        let unlock = owner.unlocked().await.unwrap();
        let key = owner.identity();
        (Arc::downgrade(&app.inner), Arc::downgrade(&owner),
            Arc::downgrade(&unlock), Arc::downgrade(&key))
    };
    let cancel = CancellationToken::new();
    let original = Arc::new(ReaderOriginalContext::capture_for_notification(
        &app, selection, &cancel, &live_nonce(&app).await).await.unwrap());
    let scope = original.scope_identity();
    let (ready_scope, scope_ready) = tokio::sync::oneshot::channel();
    let scope_waiter = tokio::spawn(async move {
        let wait = scope.cancelled();
        tokio::pin!(wait);
        // Causal barrier: the REAL waiter reached Pending before root drop.
        std::future::poll_fn(|cx| match std::future::Future::poll(wait.as_mut(), cx) {
            std::task::Poll::Pending => std::task::Poll::Ready(()),
            std::task::Poll::Ready(()) => panic!("live scope waiter ended before root drop"),
        }).await;
        ready_scope.send(()).unwrap();
        wait.await;
    });
    let (ready_original, original_ready) = tokio::sync::oneshot::channel();
    let context = original.clone();
    let original_waiter = tokio::spawn(async move {
        let wait = context.cancelled();
        tokio::pin!(wait);
        std::future::poll_fn(|cx| match std::future::Future::poll(wait.as_mut(), cx) {
            std::task::Poll::Pending => std::task::Poll::Ready(()),
            std::task::Poll::Ready(()) => panic!("live original waiter ended before root drop"),
        }).await;
        ready_original.send(()).unwrap();
        wait.await;
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        scope_ready.await.unwrap(); original_ready.await.unwrap();
    }).await.expect("both real waiters must be polled");
    drop(app);
    let disappeared = tokio::time::timeout(Duration::from_secs(1), async {
        while inner.strong_count() != 0 || owner.strong_count() != 0
            || unlock.strong_count() != 0 || key.strong_count() != 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.is_ok();
    let remaining = (inner.strong_count(), owner.strong_count(),
        unlock.strong_count(), key.strong_count());
    let denied = original.now().is_err();
    // Only now retire the original attempt, so RED cleanup cannot cause PASS.
    original.retire();
    tokio::time::timeout(Duration::from_secs(1), async {
        scope_waiter.await.unwrap(); original_waiter.await.unwrap();
    }).await.expect("real cancellation waiters must join");
    assert!(disappeared, "idle waiters retained original owners: {remaining:?}");
    assert!(denied);
}
