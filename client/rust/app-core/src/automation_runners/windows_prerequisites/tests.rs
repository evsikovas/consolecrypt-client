use super::*;
fn input(profile: &str, nonce: &str) -> String {
    json!({"version":1,"operationId":Uuid::new_v4(),"profileId":profile,"notificationScopeId":nonce}).to_string()
}
#[test]
fn closed_request_keeps_opaque_original_nonce_and_has_no_command_or_install_fields() {
    let profile = "abcdefab-1234-4234-8234-123456789abc".to_string();
    assert!(request(&input(&profile, "original:opaque-é")).is_ok());
    for nonce in ["".to_string(), "x\n".into(), "é".repeat(129)] {
        assert!(request(&input(&profile, &nonce)).is_err());
    }
    for name in [
        "command",
        "elevate",
        "install",
        "distribution",
        "script",
        "systemRoot",
    ] {
        let mut raw: Value = serde_json::from_str(&input(&profile, "original")).unwrap();
        raw[name] = json!(true);
        assert!(request(&raw.to_string()).is_err());
    }
    let valid = input(&profile, "original");
    assert!(request(&valid.replace("\"version\":1", "\"version\":true")).is_err());
    assert!(request(&valid.replace(&profile, &profile.to_uppercase())).is_err());
    assert!(request(&valid.replacen("{", "{\"version\":1,", 1)).is_err());
}
async fn fixture() -> (tempfile::TempDir, AppCore, Original) {
    let root = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        root.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile(
        "Owned Windows snapshot".into(),
        "owned Windows snapshot passphrase42".into(),
    )
    .await
    .unwrap();
    let (owner, u) = app.unlocked().await.unwrap();
    let original = Original {
        core: Arc::downgrade(&app.inner),
        profile: owner.profile_id.to_string(),
        nonce: u.notices.scope_id().unwrap(),
        generation: owner.automation.unlocked_generation().unwrap(),
        unlock: Arc::downgrade(&u),
        owner,
        expires: Instant::now() + DEADLINE,
        dropped: CancellationToken::new(),
    };
    (root, app, original)
}
#[tokio::test]
async fn original_scope_lock_and_real_core_drop_deny_without_a_replacement_lookup() {
    let (_root, app, original) = fixture().await;
    let cancel = CancellationToken::new();
    original.check(&cancel).await.unwrap();
    app.lock().await.unwrap();
    app.unlock_with_passphrase("owned Windows snapshot passphrase42".into())
        .await
        .unwrap();
    assert!(original.check(&cancel).await.is_err());
    app.shutdown().await.unwrap();
    let (_root, app, original) = fixture().await;
    drop(app);
    assert!(original.core.upgrade().is_none());
    assert!(original.check(&cancel).await.is_err());
}
#[tokio::test]
async fn same_setup_map_cancel_before_start_and_original_final_cancel_are_enforced() {
    let (_root, app, original) = fixture().await;
    let id = Uuid::new_v4().to_string();
    app.automation_runner_cancel_setup(id.clone())
        .await
        .unwrap();
    assert!(matches!(
        begin_setup(&original.owner.automation, &id),
        Err(AppError::Cancelled)
    ));
    let id = Uuid::new_v4().to_string();
    let (registration, cancel) = begin_setup(&original.owner.automation, &id).unwrap();
    original.check(&cancel).await.unwrap();
    app.automation_runner_cancel_setup(id.clone())
        .await
        .unwrap();
    assert!(original.check(&cancel).await.is_err());
    assert!(
        original
            .owner
            .automation
            .setup_operations
            .lock()
            .unwrap()
            .contains_key(&Uuid::parse_str(&id).unwrap())
    );
    drop(registration);
    assert!(
        !original
            .owner
            .automation
            .setup_operations
            .lock()
            .unwrap()
            .contains_key(&Uuid::parse_str(&id).unwrap())
    );
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_held_profile_guard_cancel_drop_and_expiry_release_worker_and_original_slot() {
    for mode in ["cancel", "drop", "expiry"] {
        let (_root, app, mut original) = fixture().await;
        if mode == "expiry" {
            original.expires = Instant::now() + Duration::from_secs(1);
        }
        let owner = original.owner.clone();
        let unlock = original.unlock.clone();
        let core = original.core.clone();
        let before_unlock = unlock.strong_count();
        let before_core = core.strong_count();
        let mut caller = Some(CancelOnDrop(original.dropped.clone()));
        let id = Uuid::new_v4();
        // Actual original Session guard, not a mock wait/fake scope check.
        let gate = app.inner.active.lock().await;
        let worker = tokio::spawn(async move {
            let (_registration, cancel) = begin_setup(&original.owner.automation, &id.to_string())?;
            original.check(&cancel).await
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if owner
                    .automation
                    .setup_operations
                    .lock()
                    .unwrap()
                    .contains_key(&id)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        if mode == "cancel" {
            // Exact already-authorized token stored by the existing setup map.
            owner
                .automation
                .setup_operations
                .lock()
                .unwrap()
                .get(&id)
                .unwrap()
                .cancel
                .cancel();
        }
        if mode == "drop" {
            drop(caller.take());
        }
        assert!(
            tokio::time::timeout(Duration::from_secs(3), worker)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
        drop(caller);
        assert!(
            !owner
                .automation
                .setup_operations
                .lock()
                .unwrap()
                .contains_key(&id)
        );
        assert_eq!(
            unlock.strong_count(),
            before_unlock,
            "no retained original vault while guard still held"
        );
        assert_eq!(
            core.strong_count(),
            before_core,
            "no retained upgraded Core while guard still held"
        );
        drop(gate);
        app.shutdown().await.unwrap();
        drop(owner);
        drop(app);
        assert!(core.upgrade().is_none());
        assert!(unlock.upgrade().is_none());
    }
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit owned-profile read-only Windows OS snapshot, no install/elevation"]
async fn actual_owned_windows_readonly_prerequisite_snapshot_original_frame() {
    assert_eq!(
        std::env::var("CC_OWNED_WINDOWS_PREREQUISITE_PROOF").as_deref(),
        Ok("readonly-v1")
    );
    // An independent controller queries its genuine process TOKEN_ELEVATION;
    // this child inherits that same token without RunAs/elevation flags.
    let expected = std::env::var("CC_OWNED_WINDOWS_ELEVATION_EXPECTED").unwrap();
    assert!(["elevated", "notElevated", "unknown"].contains(&expected.as_str()));
    let (_root, app, original) = fixture().await;
    let raw = input(&original.profile, &original.nonce);
    let wire = app
        .automation_windows_prerequisites_json(&raw)
        .await
        .unwrap();
    assert!(wire.len() <= 16 * 1024);
    let value: Value = serde_json::from_str(&wire).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 8);
    assert_eq!(value["version"], 1);
    assert_eq!(value["source"], "windowsReadOnlyV1");
    assert_eq!(value["profileId"], original.profile);
    assert_eq!(value["notificationScopeId"], original.nonce);
    assert_eq!(value["elevation"]["state"], expected);
    assert!(
        value["capturedAtUtc"]
            .as_str()
            .is_some_and(|s| s.ends_with('Z') && chrono::DateTime::parse_from_rfc3339(s).is_ok())
    );
    assert!(
        original
            .owner
            .automation
            .setup_operations
            .lock()
            .unwrap()
            .is_empty()
    );
    println!("CC_WSL03_SYNTHETIC_SCOPE_SNAPSHOT={wire}");
    drop(original);
    app.shutdown().await.unwrap();
}
