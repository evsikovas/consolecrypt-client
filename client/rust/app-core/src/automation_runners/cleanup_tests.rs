use super::*;
use cc_automation_core::{ManagedCleanupPaths, RemoteHelperConfig};

const PASSWORD: &str = "own cleanup test profile password 42";
fn candidate() -> ManagedCleanupCandidate {
    let id = "11111111-1111-4111-8111-111111111111".to_owned();
    let path = format!("/home/fixture/.local/share/consolecrypt/runner-environments/{id}");
    ManagedCleanupCandidate {
        id: id.clone(),
        environment_id: id,
        path: path.clone(),
        quarantined: false,
        eligible: true,
        blocked_reason: None,
        paths: Some(ManagedCleanupPaths {
            python: format!("{path}/venv/bin/python"),
            module_dir: format!("{path}/src"),
            root: format!("{path}/state"),
        }),
        revision: Some("a".repeat(64)),
        receipt_sha256: Some("b".repeat(64)),
        device: Some(1),
        inode: Some(2),
        nodes: Some(10),
        bytes: Some(100),
        profile_ownership: Some("unknown".into()),
        contains_state: Some(true),
    }
}
fn identity() -> ManagedCleanupIdentity {
    ManagedCleanupIdentity {
        uid: 10001,
        user: "fixture".into(),
        home: "/home/fixture".into(),
        parent: "/home/fixture/.local/share/consolecrypt/runner-environments".into(),
        device: 1,
        inode: 1,
        mode: 0o700,
    }
}
fn remote(host: String, row: &ManagedCleanupCandidate) -> AutomationRunnerConfig {
    let paths = row.paths.as_ref().unwrap();
    AutomationRunnerConfig::Remote {
        host_id: host,
        helper: RemoteHelperConfig {
            python: paths.python.clone(),
            module_dir: paths.module_dir.clone(),
            root: paths.root.clone(),
        },
    }
}
async fn setup() -> (tempfile::TempDir, AppCore) {
    let directory = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        directory.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("Cleanup own synthetic profile".into(), PASSWORD.into())
        .await
        .unwrap();
    (directory, app)
}
async fn plan(app: &AppCore) -> (Arc<Session>, Uuid) {
    let (owner, u) = app.unlocked().await.unwrap();
    let token = Uuid::new_v4();
    let value = PreparedCleanup {
        target: SetupTarget::Native,
        selected_python: None,
        identity: identity(),
        candidates: vec![candidate()],
        catalog_digest: catalog(&owner).await.unwrap().1,
        working_version: u.working().version(),
        generation: owner.automation.unlocked_generation().unwrap(),
        expires: Instant::now() + Duration::from_secs(60),
    };
    owner
        .automation
        .cleanup_plans
        .lock()
        .unwrap()
        .insert(token, value);
    (owner, token)
}
fn commit(token: Uuid, operation: Uuid, selection: Option<&str>) -> String {
    json!({"operationId":operation,"planToken":token,"selectedPython":selection}).to_string()
}
fn rule(error: &AppError) -> String {
    error.args().get("rule").cloned().unwrap_or_default()
}

#[test]
fn cleanup_request_frames_reject_paths_authority_flags_and_noncanonical_identifiers() {
    let operation = Uuid::new_v4().to_string();
    let token = Uuid::new_v4().to_string();
    for id in [
        operation.replace('-', ""),
        operation.to_uppercase(),
        Uuid::nil().to_string(),
    ] {
        assert!(parse::<ScanRequest>(
            &json!({"operationId":id,"target":{"kind":"native"}}).to_string()
        )
        .is_err());
    }
    for (field, value) in [
        ("path", json!("/unapproved")),
        ("target", json!({"kind":"native"})),
        ("candidateIds", json!([token])),
        ("eligible", json!(true)),
    ] {
        let mut input = json!({"operationId":operation,"planToken":token});
        input[field] = value;
        assert!(parse::<CommitRequest>(&input.to_string()).is_err());
    }
    assert!(parse::<CommitRequest>(&commit(
        Uuid::parse_str(&token).unwrap(),
        Uuid::parse_str(&operation).unwrap(),
        None
    ))
    .is_ok());
}

#[tokio::test]
async fn selected_and_registered_paths_are_denied_without_target_matching_or_prefix_collision() {
    let (_directory, app) = setup().await;
    let (owner, _) = app.unlocked().await.unwrap();
    let row = candidate();
    let mut scan = ManagedCleanupScan {
        version: 1,
        identity: Some(identity()),
        candidates: vec![row.clone()],
        other_profiles_checked: false,
    };
    protect(
        &mut scan,
        &[],
        &owner,
        &Some(row.paths.as_ref().unwrap().python.clone()),
        false,
    );
    assert_eq!(
        scan.candidates[0].blocked_reason.as_deref(),
        Some("cleanup_selected")
    );
    scan.candidates[0] = row.clone();
    let configuration = remote(Uuid::new_v4().to_string(), &row);
    protect(&mut scan, &[configuration], &owner, &None, false);
    assert_eq!(
        scan.candidates[0].blocked_reason.as_deref(),
        Some("cleanup_registered")
    );
    assert!(!under(
        &format!("{}-different/venv/bin/python", row.path),
        &row.path
    ));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn changed_selection_consumes_plan_once_and_lock_clears_other_pending_plans() {
    let (_directory, app) = setup().await;
    let (owner, token) = plan(&app).await;
    let error = app
        .automation_runner_cleanup_commit(commit(token, Uuid::new_v4(), Some("/usr/bin/python3")))
        .await
        .unwrap_err();
    assert_eq!(rule(&error), "cleanup_context_changed");
    assert!(!owner
        .automation
        .cleanup_plans
        .lock()
        .unwrap()
        .contains_key(&token));
    assert_eq!(
        rule(
            &app.automation_runner_cleanup_commit(commit(token, Uuid::new_v4(), None))
                .await
                .unwrap_err()
        ),
        "cleanup_plan_expired"
    );
    let (_, token) = plan(&app).await;
    app.lock().await.unwrap();
    assert!(owner.automation.cleanup_plans.lock().unwrap().is_empty());
    assert!(matches!(
        app.automation_runner_cleanup_commit(commit(token, Uuid::new_v4(), None))
            .await,
        Err(AppError::VaultLocked)
    ));
    app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    assert_eq!(
        rule(
            &app.automation_runner_cleanup_commit(commit(token, Uuid::new_v4(), None))
                .await
                .unwrap_err()
        ),
        "cleanup_plan_expired"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn catalog_reference_change_invalidates_plan_before_any_cleanup_process() {
    let (_directory, app) = setup().await;
    let host = app
        .save_host(crate::HostDto::new(
            "Own cleanup metadata host",
            "cleanup.example.invalid",
        ))
        .await
        .unwrap();
    let (_, token) = plan(&app).await;
    app.automation_runner_create(
        "Registered after review".into(),
        remote(host.id, &candidate()),
    )
    .await
    .unwrap();
    let error = app
        .automation_runner_cleanup_commit(commit(token, Uuid::new_v4(), None))
        .await
        .unwrap_err();
    assert_eq!(rule(&error), "cleanup_context_changed");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn active_admission_denies_cleanup_and_pause_is_restored_on_error() {
    let (_directory, app) = setup().await;
    let (owner, token) = plan(&app).await;
    let job = Uuid::new_v4();
    owner
        .automation
        .live_jobs
        .lock()
        .unwrap()
        .insert(job, (tokio_util::sync::CancellationToken::new(), false));
    let error = app
        .automation_runner_cleanup_commit(commit(token, Uuid::new_v4(), None))
        .await
        .unwrap_err();
    assert_eq!(rule(&error), "cleanup_busy");
    assert!(!owner.automation.admission_paused.load(Ordering::Acquire));
    owner.automation.live_jobs.lock().unwrap().clear();
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn plan_expiring_while_waiting_for_connect_gate_never_starts_cleanup() {
    let (_directory, app) = setup().await;
    let (owner, token) = plan(&app).await;
    owner
        .automation
        .cleanup_plans
        .lock()
        .unwrap()
        .get_mut(&token)
        .unwrap()
        .expires = Instant::now() + Duration::from_millis(200);
    let serial = owner.automation.connect_gate.lock().await;
    let reader = app.clone();
    let mut task = tokio::spawn(async move {
        reader
            .automation_runner_cleanup_commit(commit(token, Uuid::new_v4(), None))
            .await
    });
    // The attempt consumed a valid plan and is waiting, rather than merely
    // rejecting a plan that was already expired at entry.
    assert!(tokio::time::timeout(Duration::from_millis(100), &mut task)
        .await
        .is_err());
    assert!(!owner
        .automation
        .cleanup_plans
        .lock()
        .unwrap()
        .contains_key(&token));
    tokio::time::sleep(Duration::from_millis(150)).await;
    drop(serial);
    let error = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(rule(&error), "cleanup_plan_expired");
    assert!(!owner.automation.admission_paused.load(Ordering::Acquire));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn unsaved_live_connection_alias_is_included_in_guest_deny_only_paths() {
    let (_directory, app) = setup().await;
    let (owner, _) = app.unlocked().await.unwrap();
    let (client, _server) = tokio::io::duplex(1024);
    let (reader, writer) = tokio::io::split(client);
    let peer = HelperPeer::connect(reader, writer);
    let configuration = AutomationRunnerConfig::Remote {
        host_id: Uuid::new_v4().to_string(),
        helper: RemoteHelperConfig {
            python: "/home/fixture/manual-alias-to-managed-python".into(),
            module_dir: "/home/fixture/manual-source".into(),
            root: "/home/fixture/manual-state".into(),
        },
    };
    let expected = paths(&configuration);
    owner.automation.connections.lock().unwrap().insert(
        "unsaved".into(),
        Arc::new(RunnerConnection {
            id: "unsaved".into(),
            revision: 0,
            configuration,
            peer,
            probe: json!({}),
            remote: None,
            local: None,
            authorization: tokio::sync::Mutex::new(None),
        }),
    );
    let protected = protection(&[], &owner, &None).unwrap();
    assert_eq!(protected.paths.len(), 3);
    for path in expected {
        assert!(protected.paths.contains(&path));
    }
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn waiting_cleanup_cancel_and_vault_lock_do_not_hold_metadata_guards_or_start_process() {
    let (_directory, app) = setup().await;
    for lock in [false, true] {
        let (owner, token) = plan(&app).await;
        let serial = owner.automation.connect_gate.lock().await;
        let operation = Uuid::new_v4();
        let reader = app.clone();
        let task = tokio::spawn(async move {
            reader
                .automation_runner_cleanup_commit(commit(token, operation, None))
                .await
        });
        for _ in 0..100 {
            if !owner
                .automation
                .cleanup_plans
                .lock()
                .unwrap()
                .contains_key(&token)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        if lock {
            app.lock().await.unwrap();
        } else {
            app.automation_runner_cancel_setup(operation.to_string())
                .await
                .unwrap();
        }
        let result = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("cleanup held a vault guard while waiting")
            .unwrap();
        if lock {
            assert!(matches!(result, Err(AppError::VaultLocked)));
        } else {
            assert!(matches!(result, Err(AppError::Cancelled)));
        }
        assert!(!owner.automation.admission_paused.load(Ordering::Acquire));
        drop(serial);
        if lock {
            app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
        }
    }
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn runner_registration_uses_same_exclusion_gate_as_cleanup_commit() {
    let (_directory, app) = setup().await;
    let (owner, _) = app.unlocked().await.unwrap();
    let serial = owner.automation.connect_gate.lock().await;
    let writer = app.clone();
    let mut task = tokio::spawn(async move {
        writer
            .automation_runner_create(
                "Queued registration".into(),
                remote(Uuid::new_v4().to_string(), &candidate()),
            )
            .await
    });
    assert!(tokio::time::timeout(Duration::from_millis(30), &mut task)
        .await
        .is_err());
    drop(serial);
    task.await.unwrap().unwrap();
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn persisted_job_states_and_unknown_schema_are_conservatively_checked() {
    let (_directory, app) = setup().await;
    let (owner, _) = app.unlocked().await.unwrap();
    for state in [
        "succeeded",
        "failed",
        "cancelled",
        "interrupted",
        "acknowledged",
        "queued",
        "dispatching",
        "running",
        "unknown",
    ] {
        let value = json!({"version":1,"records":{"synthetic":{"version":1,"state":state}}});
        owner
            .storage()
            .unwrap()
            .write(move |tx| {
                tx.setting_set("automation.jobs.v1", &value)?;
                Ok::<_, AppError>(())
            })
            .await
            .unwrap();
        assert_eq!(
            busy(&owner).await.unwrap(),
            ["queued", "dispatching", "running", "unknown"].contains(&state)
        );
    }
    owner
        .storage()
        .unwrap()
        .write(|tx| {
            tx.setting_set("automation.jobs.v1", &json!({"version":2,"records":{}}))?;
            Ok::<_, AppError>(())
        })
        .await
        .unwrap();
    assert_eq!(
        rule(&busy(&owner).await.unwrap_err()),
        "cleanup_catalog_unavailable"
    );
    // Remove synthetic fixture rows before shutdown (no real jobs were admitted).
    owner
        .storage()
        .unwrap()
        .write(|tx| {
            tx.setting_set("automation.jobs.v1", &json!({"version":1,"records":{}}))?;
            Ok::<_, AppError>(())
        })
        .await
        .unwrap();
    app.shutdown().await.unwrap();
}
