use super::*;
use crate::service_credential_issuer::PersonalServiceFixture as Fixture;
use cc_sync_core::mock::FaultAction;
async fn input(f: &Fixture) -> (String, String) {
    let (owner, u) = f.app.unlocked().await.unwrap();
    (json!({"version":1,"profileId":owner.profile_id.to_string(),"notificationScopeId":u.notices.scope_id().unwrap()}).to_string(),
     format!("/v1/vaults/{}/compatibility/monitoring-settings",u.vault_id))
}
async fn observed(f: &Fixture) -> (SettingsSyncConsent, String) {
    let (raw, path) = input(f).await;
    let (owner, u) = f.app.unlocked().await.unwrap();
    let profile = owner.profile().await.unwrap();
    f.server.inject(&path,1,FaultAction::RawResponse {status:200,body:json!({"version":1,"scope":"personalAccount","accountId":profile.user_id.unwrap(),"revision":0,
        "state":"off","minimumClientVersion":null,"offerMinimumClientVersion":"2.0.0","rosterDigest":"a".repeat(64),
        "checkpoint":{"vaultId":u.vault_id,"vaultEpoch":uuid::Uuid::new_v4(),"vaultSequence":9007199254740993i64}})});
    let before_version = u.working().version();
    let before_catalog = settings_checkpoint(&u).unwrap();
    let original_nonce = u.notices.scope_id();
    let original_identity = Arc::downgrade(&owner.identity());
    let original_unlock = Arc::downgrade(&u);
    drop(u);
    let result = f.app.settings_sync_prepare_consent_json(&raw).await;
    if let Err(error) = &result {
        let current = original_unlock.upgrade().unwrap();
        panic!("owned consent success fixture denied: {error:?}; policy_gets={}; same_working_version={}; same_catalog={}; same_nonce={}; same_identity={}; shutdown={}",
            f.server.hits(&path), before_version == current.working().version(),
            settings_checkpoint(&current).is_ok_and(|c|c == before_catalog), current.notices.scope_id() == original_nonce,
            Weak::ptr_eq(&original_identity,&Arc::downgrade(&owner.identity())), *current.sharing_shutdown.borrow());
    }
    (result.unwrap(), path)
}
#[test]
fn closed_request_preserves_opaque_nonce_and_denies_caller_authority() {
    let id = uuid::Uuid::new_v4().to_string();
    assert!(
        request(
            &json!({"version":1,"profileId":id,"notificationScopeId":"opaque-original:\u{e9}"})
                .to_string()
        )
        .is_ok()
    );
    for nonce in ["".to_string(), "x\n".to_string(), "\u{e9}".repeat(129)] {
        assert!(
            request(&json!({"version":1,"profileId":id,"notificationScopeId":nonce}).to_string())
                .is_err()
        );
    }
    for field in ["floor", "ready", "rosterDigest", "key", "checkpoint"] {
        let mut raw = json!({"version":1,"profileId":id,"notificationScopeId":"opaque"});
        raw[field] = json!(true);
        assert!(request(&raw.to_string()).is_err());
    }
    assert!(request(&format!(r#"{{"version":1,"profileId":"{id}","notificationScopeId":"a","notificationScopeId":"b"}}"#)).is_err());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_owner_incompatible_candidate_sends_no_put_and_consumes_once() {
    let f = Fixture::new().await;
    let (handle, path) = observed(&f).await;
    let first: Value = serde_json::from_str(&handle.details_json().await.unwrap()).unwrap();
    // Owned synthetic loopback fixture ONLY: emits no keys/tokens/paths or
    // user data, and exercises the actual Native closed JSON serializer.
    let wire=serde_json::to_string(&first).unwrap();
    assert!(wire.len()<=16*1024);
    println!("CC_MON07_SYNTHETIC_DETAILS={wire}");
    assert_eq!(first["scope"], "allPersonalVaults");
    assert_eq!(first["kind"], "blocked");
    assert_eq!(first["releasedMinimumClientVersion"], "1.2.0");
    assert_eq!(first["readiness"]["state"], "devicesUnsupported");
    assert_eq!(first["canConfirm"], false);
    assert_eq!(first["canRecheck"], false);
    assert_eq!(first["policyRevision"], "0");
    assert_eq!(first["checkpoint"]["vaultSequence"], "9007199254740993");
    assert!(first["catalog"]["objectRevision"].is_string());
    tokio::time::sleep(Duration::from_millis(3)).await;
    let second: Value = serde_json::from_str(&handle.details_json().await.unwrap()).unwrap();
    assert!(second["remainingMs"].as_u64().unwrap() < first["remainingMs"].as_u64().unwrap());
    let before = f.server.hits(&path);
    assert!(handle.confirm().await.is_err());
    assert!(handle.confirm().await.is_err());
    assert!(handle.details_json().await.is_err());
    assert_eq!(before, f.server.hits(&path));
    f.app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_same_runtime_budget_drop_and_cancel_release_original_slots() {
    let f = Fixture::new().await;
    let (handle, _) = observed(&f).await;
    let (_, u) = f.app.unlocked().await.unwrap();
    let weak = Arc::downgrade(&u);
    let mut leases = Vec::new();
    for _ in 0..7 {
        leases.push(SettingsReviewRuntime::begin_owned(weak.clone()).unwrap());
    }
    assert!(SettingsReviewRuntime::begin_owned(weak.clone()).is_err());
    handle.cancel();
    let extra = SettingsReviewRuntime::begin_owned(weak.clone()).unwrap();
    assert!(SettingsReviewRuntime::begin_owned(weak.clone()).is_err());
    drop(extra);
    drop(leases);
    drop(handle);
    let (another, _) = observed(&f).await;
    drop(another);
    let leases: Vec<_> = (0..8)
        .map(|_| SettingsReviewRuntime::begin_owned(weak.clone()).unwrap())
        .collect();
    assert!(SettingsReviewRuntime::begin_owned(weak).is_err());
    drop(leases);
    drop(u);
    f.app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_lock_reunlock_and_old_nonce_cannot_retarget_handle() {
    let f = Fixture::new().await;
    let (handle, _) = observed(&f).await;
    let (old, path) = input(&f).await;
    let before = f.server.hits(&path);
    f.app.lock().await.unwrap();
    f.app
        .unlock_with_passphrase("owned personal service vault42".into())
        .await
        .unwrap();
    assert!(handle.details_json().await.is_err());
    assert!(handle.recheck_checkpoint().await.is_err());
    assert!(
        f.app
            .settings_sync_prepare_consent_json(&old)
            .await
            .is_err()
    );
    assert_eq!(before, f.server.hits(&path));
    f.app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_logout_invalidates_old_and_new_consent_while_vault_unlocked() {
    let f = Fixture::new().await;
    let (handle, path) = observed(&f).await;
    let (raw, _) = input(&f).await;
    let before = f.server.hits(&path);
    f.app.logout().await.unwrap();
    assert!(f.app.unlocked().await.is_ok());
    assert!(handle.details_json().await.is_err());
    assert!(handle.confirm().await.is_err());
    assert!(
        f.app
            .settings_sync_prepare_consent_json(&raw)
            .await
            .is_err()
    );
    assert_eq!(before, f.server.hits(&path));
    f.app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_policy_io_lock_cancels_without_retaining_unlocked_vault() {
    let f = Fixture::new().await;
    let (raw, path) = input(&f).await;
    let (_, u) = f.app.unlocked().await.unwrap();
    let weak = Arc::downgrade(&u);
    drop(u);
    let (entered, wait_entered) = tokio::sync::oneshot::channel();
    let entered = Arc::new(Mutex::new(Some(entered)));
    let (release, wait_release) = std::sync::mpsc::channel();
    let wait_release = Arc::new(Mutex::new(wait_release));
    f.server.inject(
        &path,
        1,
        FaultAction::Rewrite(Arc::new(move |_| {
            entered.lock().unwrap().take().unwrap().send(()).unwrap();
            wait_release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
        })),
    );
    let app = f.app.clone();
    let worker =
        tokio::spawn(async move { app.settings_sync_prepare_consent_json(&raw).await });
    tokio::time::timeout(Duration::from_secs(10), wait_entered)
        .await
        .unwrap()
        .unwrap();
    let locked = tokio::time::timeout(Duration::from_secs(5), f.app.lock()).await;
    let finished = tokio::time::timeout(Duration::from_secs(5), worker).await;
    release.send(()).unwrap();
    locked.unwrap().unwrap();
    assert!(finished.unwrap().unwrap().is_err());
    assert!(weak.upgrade().is_none());
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_original_deadline_retires_review_and_releases_same_budget() {
    let f = Fixture::new().await;
    let (handle, _) = observed(&f).await;
    let (deadline, auth, shutdown) = {
        let mut guard = handle.control.review.lock().unwrap();
        let review = guard.as_mut().unwrap();
        review.context.expires = Instant::now() + Duration::from_millis(20);
        (
            review.context.expires,
            review.auth.clone(),
            review.context.shutdown.clone(),
        )
    };
    // Exact production timer/retirement path; only the owned test deadline is shortened.
    handle.control.arm(deadline, auth, shutdown);
    tokio::time::timeout(Duration::from_secs(2), handle.control.cancel.cancelled())
        .await
        .unwrap();
    assert!(handle.details_json().await.is_err());
    assert!(handle.confirm().await.is_err());
    let (_, u) = f.app.unlocked().await.unwrap();
    let leases: Vec<_> = (0..8)
        .map(|_| SettingsReviewRuntime::begin_owned(Arc::downgrade(&u)).unwrap())
        .collect();
    drop(leases);
    drop(u);
    f.app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn existing_auth_transition_blocks_before_any_policy_request() {
    let f = Fixture::new().await;
    let (handle, path) = observed(&f).await;
    let (raw, _) = input(&f).await;
    let hits = f.server.hits(&path);
    let transition = f.app.original_auth_transition();
    assert!(handle.details_json().await.is_err());
    assert!(
        f.app
            .settings_sync_prepare_consent_json(&raw)
            .await
            .is_err()
    );
    assert_eq!(hits, f.server.hits(&path));
    drop(transition);
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dropping_last_core_does_not_leave_inner_or_original_vault_held_by_handle() {
    let f = Fixture::new().await;
    let (handle, _) = observed(&f).await;
    let inner = Arc::downgrade(&f.app.inner);
    let (owner, u) = f.app.unlocked().await.unwrap();
    let session = Arc::downgrade(&owner);
    let unlocked = Arc::downgrade(&u);
    drop(u);
    drop(owner);
    drop(f.app);
    assert!(
        inner.upgrade().is_none(),
        "review must not retain originating Core"
    );
    assert!(
        session.upgrade().is_none(),
        "review must not indirectly retain Session/Unlocked"
    );
    assert!(
        unlocked.upgrade().is_none(),
        "review must not retain original vault"
    );
    assert!(handle.details_json().await.is_err());
    assert!(handle.confirm().await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn delayed_final_auth_guard_cannot_publish_after_original_cancel_or_expiry() {
    for expires in [false, true] {
        let f = Fixture::new().await;
        let (handle, _) = observed(&f).await;
        let (deadline, auth, shutdown) = {
            let mut guard = handle.control.review.lock().unwrap();
            let review = guard.as_mut().unwrap();
            if expires {
                review.context.expires = Instant::now() + Duration::from_secs(2);
            }
            (
                review.context.expires,
                review.auth.clone(),
                review.context.shutdown.clone(),
            )
        };
        if expires {
            handle.control.arm(deadline, auth.clone(), shutdown);
        }
        let (entered, wait_entered) = tokio::sync::oneshot::channel();
        let (release, wait_release) = std::sync::mpsc::channel();
        let blocker = std::thread::spawn(move || {
            auth.with_current(|| {
                entered.send(()).unwrap();
                wait_release.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(())
            })
        });
        wait_entered.await.unwrap();
        let handle = Arc::new(handle);
        let task_handle = handle.clone();
        let worker = tokio::spawn(async move { task_handle.details_json().await });
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match handle.control.review.try_lock() {
                    Err(std::sync::TryLockError::WouldBlock) => break,
                    Err(_) => panic!("poisoned owned review"),
                    Ok(_) => {}
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let cancel = if expires {
            None
        } else {
            let h = handle.clone();
            Some(std::thread::spawn(move || h.cancel()))
        };
        // This fixture deliberately stalls a synchronous auth mutex. The expiry
        // task may itself wait for the held review mutex, so releasing the auth
        // holder must not depend on that Tokio worker driving its timer.
        // Observe the ORIGINAL cancellation/deadline on an independent bounded
        // OS thread; no production clock, lease or outcome is changed.
        let cancelled = handle.control.cancel.clone();
        let releaser = std::thread::spawn(move || {
            if expires {
                std::thread::sleep(
                    (deadline + Duration::from_millis(5))
                        .saturating_duration_since(Instant::now()),
                );
                assert!(Instant::now() >= deadline);
            } else {
                let wait_until = Instant::now() + Duration::from_secs(2);
                while !cancelled.is_cancelled() && Instant::now() < wait_until {
                    std::thread::sleep(Duration::from_millis(1));
                }
                assert!(cancelled.is_cancelled());
            }
            release.send(()).unwrap();
        });
        blocker.join().unwrap().unwrap();
        releaser.join().unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(2), worker)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
        if let Some(t) = cancel {
            t.join().unwrap();
        }
        drop(handle);
        f.app.shutdown().await.unwrap();
    }
}
