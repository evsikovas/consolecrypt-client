//! Draft real original Local AppCore tests; no fake auth/signer or Relay claim.
use super::*;
const PASSWORD: &str = "tunnel original Native test password 42";
async fn local() -> (tempfile::TempDir, AppCore) {
    let root = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        root.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("Tunnel original".into(), PASSWORD.into())
        .await
        .unwrap();
    (root, app)
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn original_session_capture_needs_no_reader_host_source_or_grant() {
    let (_root, app) = local().await;
    let scope = OriginalSessionDeviceIdentity::capture(
        &app,
        &CancellationToken::new(),
        Duration::from_secs(30),
    )
    .await
    .unwrap();
    scope.check(&app).await.unwrap();
    let actual = app
        .session()
        .await
        .unwrap()
        .identity()
        .public_keys()
        .signing;
    let observed = scope
        .with_loaded(&app, |identity| Ok(identity.public_keys().signing))
        .await
        .unwrap();
    assert_eq!(actual, observed);
    app.shutdown().await.unwrap();
    assert!(scope.check_current().is_err());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_scope_never_runs_held_owner_signer() {
    let (_root, app) = local().await;
    let cancel = CancellationToken::new();
    let scope = OriginalSessionDeviceIdentity::capture(&app, &cancel, Duration::from_secs(30))
        .await
        .unwrap();
    cancel.cancel();
    let called = std::sync::atomic::AtomicBool::new(false);
    assert!(
        scope
            .with_loaded(&app, |_| {
                called.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            })
            .await
            .is_err()
    );
    assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
    app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lock_reunlock_cannot_renew_original_tunnel_scope() {
    let (_root, app) = local().await;
    let old = OriginalSessionDeviceIdentity::capture(
        &app,
        &CancellationToken::new(),
        Duration::from_secs(30),
    )
    .await
    .unwrap();
    app.lock().await.unwrap();
    app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    assert!(old.check(&app).await.is_err());
    let fresh = OriginalSessionDeviceIdentity::capture(
        &app,
        &CancellationToken::new(),
        Duration::from_secs(30),
    )
    .await
    .unwrap();
    fresh.check(&app).await.unwrap();
    app.shutdown().await.unwrap();
}
#[test]
fn application_route_budget_shared_channels_are_not_new_lease_generations() {
    let p: TunnelRoutePolicy = serde_json::from_str(include_str!(
        "../../../agent-node-wire/src/tunnel_policy_vector.json"
    ))
    .unwrap();
    let mut l = p.limits;
    l.max_channels = 1;
    l.max_bytes_total = 10;
    l.max_bytes_per_channel = 10;
    l.max_frame_bytes = 10;
    l.max_queued_bytes_per_channel = 10;
    l.max_queued_bytes_total = 10;
    let b = Budget::new(l, CancellationToken::new());
    let c = b.open().unwrap();
    assert!(b.open().is_err());
    let frame = c.reserve().unwrap();
    frame.charge(8).unwrap();
    drop(frame);
    drop(c);
    let c = b.open().unwrap();
    let frame = c.reserve().unwrap();
    assert_eq!(frame.capacity(), 2);
    frame.charge(2).unwrap();
    drop(frame);
    assert!(c.reserve().is_err());
    b.stop();
    assert!(b.open().is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_loaded_original_device_signs_only_explicit_matching_node_owner_offer() {
    let (_root, app) = local().await;
    let owner = app.session().await.unwrap();
    let actual = owner.identity();
    let independent = cc_crypto_core::DeviceSecretKeys::generate().unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut p: TunnelRoutePolicy = serde_json::from_str(include_str!(
        "../../../agent-node-wire/src/tunnel_policy_vector.json"
    ))
    .unwrap();
    let personal_context = p.context.clone();
    p.context.tenant = Tenant::LocalProfile {
        id: LocalProfileId::new("11111111-1111-4111-8111-111111111111").unwrap(),
    };
    let pin = OwnerPin {
        context: p.context.clone(),
        key: PublicKey::from_bytes(actual.public_keys().signing),
        epoch: 1,
        checkpoint: 0,
        expires_at: Timestamp::new(now + 600).unwrap(),
    };
    let offer = Offer {
        header: Header {
            version: 1,
            context: p.context.clone(),
            owner_epoch: 1,
            checkpoint: 1,
        },
        invitation_id: InvitationId::new("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap(),
        agent_id: p.agent_id,
        agent_key: PublicKey::from_bytes(independent.public_keys().signing),
        token_hash: Hash::from_bytes([7; 32]),
        scopes: Scopes(vec![Scope::TunnelOpen]),
        issued_at: Timestamp::new(now).unwrap(),
        expires_at: Timestamp::new(now + 300).unwrap(),
    };
    // The Node namespace is distinct from the original Native client profile.
    assert_ne!(
        owner.profile_id.to_string(),
        "11111111-1111-4111-8111-111111111111"
    );
    let local_scope = OriginalSessionDeviceIdentity::capture(
        &app,
        &CancellationToken::new(),
        Duration::from_secs(30),
    )
    .await
    .unwrap();
    p.client_subject = TunnelClientSubject::LocalProfile {
        id: owner.profile_id.to_string(),
    };
    p.client_device_id = actual.device_id().to_string();
    local_scope.bind_owner_policy(&pin, &p).unwrap();
    let mut foreign_client = p.clone();
    foreign_client.client_subject = TunnelClientSubject::LocalProfile {
        id: uuid::Uuid::new_v4().to_string(),
    };
    assert!(
        local_scope
            .bind_owner_policy(&pin, &foreign_client)
            .is_err()
    );
    let mut wrong_tenant_pin = pin.clone();
    wrong_tenant_pin.context = personal_context.clone();
    let mut wrong_tenant_offer = offer.clone();
    wrong_tenant_offer.header.context = personal_context;
    assert!(
        TunnelOwnerReview::enrollment_offer(
            &app,
            &wrong_tenant_pin,
            wrong_tenant_offer,
            &CancellationToken::new()
        )
        .await
        .is_err()
    );
    let cancel = CancellationToken::new();
    let reviewed = TunnelOwnerReview::enrollment_offer(&app, &pin, offer.clone(), &cancel)
        .await
        .unwrap();
    let signed = reviewed.approve_once(&app).await.unwrap();
    let TunnelOwnerSigned::Offer(signed) = signed else {
        panic!()
    };
    verify_statement(&signed, pin.key).unwrap();
    assert_eq!(signed.body, offer);
    let mut foreign = pin.clone();
    foreign.key = PublicKey::from_bytes(independent.public_keys().signing);
    assert!(
        TunnelOwnerReview::enrollment_offer(
            &app,
            &foreign,
            offer.clone(),
            &CancellationToken::new()
        )
        .await
        .is_err()
    );
    let reviewed = TunnelOwnerReview::enrollment_offer(&app, &pin, offer, &cancel)
        .await
        .unwrap();
    cancel.cancel();
    assert!(reviewed.approve_once(&app).await.is_err());
    drop(actual);
    drop(owner);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn stalled_actual_io_is_cancelled_at_original_idle_before_five_second_io_budget() {
    use tokio::io::AsyncWriteExt;
    let mut limits: TunnelRoutePolicy = serde_json::from_str(include_str!(
        "../../../agent-node-wire/src/tunnel_policy_vector.json"
    ))
    .unwrap();
    limits.limits.max_ttl_seconds = 10;
    limits.limits.idle_timeout_seconds = 1;
    let budget = Budget::new(limits.limits, CancellationToken::new());
    let channel = budget.open().unwrap();
    let reservation = channel.reserve().unwrap();
    let payload = vec![0u8; reservation.capacity()];
    reservation.charge(payload.len()).unwrap();
    let (mut writer, _unread_peer) = tokio::io::duplex(1);
    let started = std::time::Instant::now();
    let write = async {
        writer.write_all(&payload).await.unwrap();
        false
    };
    let cancelled = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::select! {biased;_=budget.cancelled()=>true,result=write=>result}
    })
    .await
    .expect("original idle did not interrupt blocked owned I/O");
    assert!(
        cancelled,
        "blocked write must not complete without a reader"
    );
    assert!(started.elapsed() >= Duration::from_millis(900));
    assert!(started.elapsed() < Duration::from_millis(2500));
    assert!(budget.check().is_err());
    assert!(budget.open().is_err());
    drop(reservation);
    drop(channel);
    // Actual in-memory I/O cancellation only; no Native/Agent authority claim.
}


#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn polled_tunnel_waiter_does_not_retain_original_session_or_device_keys() {
    let (_root, app) = local().await;
    let scope = OriginalSessionDeviceIdentity::capture(
        &app, &CancellationToken::new(), Duration::from_secs(30),
    ).await.unwrap();
    let (session, unlocked, identity) = {
        let owner = app.session().await.unwrap();
        let unlocked = owner.unlocked().await.unwrap();
        let identity = owner.identity();
        (Arc::downgrade(&owner), Arc::downgrade(&unlocked), Arc::downgrade(&identity))
    };
    let mut waiter = Box::pin(scope.cancelled());
    tokio::select! {
        _ = &mut waiter => panic!("original live scope unexpectedly retired"),
        _ = tokio::task::yield_now() => {}
    }
    drop(app);
    tokio::time::timeout(Duration::from_secs(2), &mut waiter).await
        .expect("waiting route must observe original loaded-owner disappearance");
    assert!(session.upgrade().is_none());
    assert!(unlocked.upgrade().is_none());
    assert!(identity.upgrade().is_none());
    assert!(scope.check_current().is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pending_tunnel_wait_observes_actual_api_subject_invalidation_without_native_auth_transition() {
    let server = cc_sync_core::mock::MockServer::start().await;
    let root = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        root.path().to_string_lossy().into_owned(),
    )).unwrap();
    app.create_synced_profile(
        "Tunnel personal fixture".into(), server.url().to_string(),
        "tunnel-waiter@example.org".into(), "tunnel account password 42".into(),
        crate::AccountMode::Register,
    ).await.unwrap();
    app.create_vault(PASSWORD.into()).await.unwrap();
    app.sync_now().await.unwrap();
    app.session().await.unwrap().unlocked().await.unwrap().stop_engine().await;
    let api = app.session().await.unwrap().api_opt().unwrap();
    let native_auth = crate::agent_reader::OriginalSessionAuth::capture(&app).unwrap();
    let scope = OriginalSessionDeviceIdentity::capture(
        &app, &CancellationToken::new(), Duration::from_secs(30),
    ).await.unwrap();
    let mut waiter = Box::pin(scope.cancelled());
    tokio::select! {
        _ = &mut waiter => panic!("original authenticated scope unexpectedly retired"),
        _ = tokio::task::yield_now() => {}
    }
    api.invalidate_reader_auth();
    assert!(native_auth.is_current(), "this is the underlying API subject, not a Native transition");
    tokio::time::timeout(Duration::from_secs(2), &mut waiter).await
        .expect("original account invalidation must wake the already-polled route");
    assert!(scope.check_current().is_err());
    drop(waiter);
    app.shutdown().await.unwrap();
}

// DRAFT_UNCOMPILED_UNRUN: actual Personal fixture, no fabricated auth subject.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn personal_relay_setup_does_not_reenter_api_auth_mutex_and_retires_original_subject() {
    use std::task::Poll;
    let server = cc_sync_core::mock::MockServer::start().await;
    let root = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        root.path().to_string_lossy().into_owned(),
    )).unwrap();
    app.create_synced_profile(
        "Tunnel relay setup fixture".into(), server.url().to_string(),
        "tunnel-relay-setup@example.org".into(), "tunnel account password 42".into(),
        crate::AccountMode::Register,
    ).await.unwrap();
    app.create_vault(PASSWORD.into()).await.unwrap();
    app.sync_now().await.unwrap();
    app.session().await.unwrap().unlocked().await.unwrap().stop_engine().await;
    let api = app.session().await.unwrap().api_opt().unwrap();
    // Capture the real opaque API subject OUTSIDE either synchronous guard.
    let subject = api.capture_reader_auth().await.unwrap();
    let native_auth = crate::agent_reader::OriginalSessionAuth::capture(&app).unwrap();
    let scope = OriginalSessionDeviceIdentity::capture(
        &app, &CancellationToken::new(), Duration::from_secs(30),
    ).await.unwrap();
    subject.check_current(&api).unwrap();

    // Demonstrate the old whole-I/O guard's recursive lock without ever
    // calling the blocking API accessor while that guard holds its mutex.
    let nested = scope.with_io_current(|_| {
        Ok(subject.try_with_current(&api, || Poll::<()>::Pending))
    }).unwrap();
    assert!(nested.is_err(), "the full socket guard already owns this Personal auth mutex");
    scope.check_current().unwrap();

    let pending = scope.with_api_setup_current(|_| {
        subject.try_with_current(&api, || Poll::<()>::Pending).map_err(|_| denied())
    }).unwrap();
    assert!(matches!(pending, Poll::Pending));
    // Returning Pending left no API mutex held by the synchronous guard.
    assert!(subject.try_with_current(&api, || ()).is_ok());

    let invalidated_during_poll = scope.with_api_setup_current(|_| {
        subject.try_with_current(&api, || ()).map_err(|_| denied())?;
        // try_with_current has returned and DROPPED its inner guard first.
        api.invalidate_reader_auth();
        Ok(Poll::<()>::Ready(()))
    });
    assert!(invalidated_during_poll.is_err(), "post-poll original provenance must discard Ready");
    assert!(native_auth.is_current(), "API-only invalidation is not a Native auth transition");
    assert!(scope.check_current().is_err());
    let invoked = std::cell::Cell::new(false);
    assert!(scope.with_api_setup_current(|_| {
        invoked.set(true);
        Ok(Poll::<()>::Ready(()))
    }).is_err());
    assert!(!invoked.get(), "old scope cannot dispatch a later setup poll");
    app.shutdown().await.unwrap();
}
