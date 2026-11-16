use super::*;
use cc_automation_core::VAULT_INPUT_FILE;
fn runtime() -> VaultValidationRuntime {
    VaultValidationRuntime::parse_pinned_probe(
        serde_json::from_str(include_str!(
            "../../../../../automation-core/src/vault_validation/runtime-pin-v1.json"
        ))
        .unwrap(),
    )
    .unwrap()
}

struct Fixture {
    _directory: tempfile::TempDir,
    app: AppCore,
    project: String,
    credential: String,
    digest: String,
}
impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            dir.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile(
            "Owned validation purpose fixture".into(),
            "synthetic profile password 42".into(),
        )
        .await
        .unwrap();
        let project = app
            .automation_project_create("Vault validation fixture".into(), None)
            .await
            .unwrap()
            .id;
        let credential = app
            .add_password_credential(
                "Explicit selected Vault password".into(),
                None,
                "synthetic-password-42".into(),
            )
            .await
            .unwrap()
            .id;
        app.automation_project_save(
            project.clone(),
            VAULT_INPUT_FILE.into(),
            None,
            json!({"version":1,"ansible_vault":{"credential_id":credential}}).to_string(),
        )
        .await
        .unwrap();
        let digest = app
            .automation_project_files(project.clone())
            .await
            .unwrap()
            .digest;
        Self {
            _directory: dir,
            app,
            project,
            credential,
            digest,
        }
    }
    async fn capture(&self) -> VaultCredentialReview {
        self.app
            .automation_vault_capture(
                self.project.clone(),
                1,
                self.digest.clone(),
                self.credential.clone(),
                &CancellationToken::new(),
            )
            .await
            .unwrap()
    }
    async fn scope(&self) -> Arc<NativeVaultValidation> {
        self.capture()
            .await
            .begin_validation(&self.app, "playbook.yml".into(), runtime())
            .await
            .unwrap()
    }
}
#[tokio::test]
async fn actual_encrypted_credential_distinct_syntax_lint_one_use_and_no_debug_material() {
    let f = Fixture::new().await;
    let scope = f.scope().await;
    let syntax = scope.request(VaultValidationPhase::Syntax);
    let lint = scope.request(VaultValidationPhase::Lint);
    assert_ne!(syntax.phase_nonce, lint.phase_nonce);
    for req in [&syntax, &lint] {
        let mut password = scope.phase(&f.app, req, runtime()).await.unwrap();
        assert!(!format!("{password:?}").contains("synthetic-password-42"));
        let mut output = vec![];
        password.lease.write_once(req, &mut output).unwrap();
        assert_eq!(output, b"synthetic-password-42\n");
        assert!(scope.phase(&f.app, req, runtime()).await.is_err());
    }
    drop(scope);
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn possession_of_wrong_scope_nonce_or_hash_never_issues_or_consumes_correct_grant() {
    let f = Fixture::new().await;
    let scope = f.scope().await;
    for key in [
        "scope_id",
        "phase_nonce",
        "revision_digest",
        "reference_sha256",
    ] {
        let mut v = serde_json::to_value(scope.request(VaultValidationPhase::Syntax)).unwrap();
        v[key] = if key.ends_with("digest") || key == "reference_sha256" {
            json!("d".repeat(64))
        } else {
            json!(Uuid::new_v4())
        };
        let req = serde_json::from_value(v).unwrap();
        assert!(scope.phase(&f.app, &req, runtime()).await.is_err());
    }
    let req = scope.request(VaultValidationPhase::Syntax);
    assert!(scope.phase(&f.app, &req, runtime()).await.is_ok());
    drop(scope);
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn current_credential_metadata_mutation_invalidates_phase_and_already_issued_material() {
    let f = Fixture::new().await;
    let scope = f.scope().await;
    let req = scope.request(VaultValidationPhase::Syntax);
    let mut password = scope.phase(&f.app, &req, runtime()).await.unwrap();
    f.app
        .set_credential_password(
            f.credential.clone(),
            "synthetic replacement password".into(),
        )
        .await
        .unwrap();
    assert!(scope
        .phase(
            &f.app,
            &scope.request(VaultValidationPhase::Lint),
            runtime()
        )
        .await
        .is_err());
    let mut output = vec![];
    assert!(password.lease.write_once(&req, &mut output).is_err());
    assert!(output.is_empty());
    drop(password);
    drop(scope);
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn lock_close_cancel_sql_and_user_source_edits_fence_original_scope() {
    for cause in ["lock", "close", "cancel", "sql", "filesystem"] {
        let f = Fixture::new().await;
        let scope = f.scope().await;
        match cause {
            "lock" => f.app.lock().await.unwrap(),
            "close" => f.app.close_profile().await.unwrap(),
            "cancel" => scope.revoke(),
            "sql" => {
                f.app
                    .automation_project_rename(
                        f.project.clone(),
                        1,
                        "Changed project metadata".into(),
                    )
                    .await
                    .unwrap();
            }
            _ => std::fs::write(
                scope.review.project.path.join("playbook.yml"),
                b"later user edit\n",
            )
            .unwrap(),
        }
        assert!(
            scope
                .phase(
                    &f.app,
                    &scope.request(VaultValidationPhase::Syntax),
                    runtime()
                )
                .await
                .is_err(),
            "{cause}"
        );
        drop(scope);
        f.app.shutdown().await.unwrap();
    }
}
#[tokio::test]
async fn unsupported_password_line_never_reaches_private_pipe() {
    let f = Fixture::new().await;
    for value in [
        "synthetic\nsecond-line",
        "synthetic\rsecond-line",
        "synthetic\0suffix",
    ] {
        f.app
            .set_credential_password(f.credential.clone(), value.into())
            .await
            .unwrap();
        let scope = f.scope().await;
        let request = scope.request(VaultValidationPhase::Syntax);
        let result = scope.phase(&f.app, &request, runtime()).await;
        assert!(result.is_err());
        assert!(!format!("{result:?}").contains(value));
        drop(scope);
    }
    f.app.shutdown().await.unwrap();
}
fn verified(revision: &str, output: &str) -> VerifiedValidation {
    use cc_automation_core::{RunParameters, ValidationProof};
    use cc_models::automation::ApprovedTarget;
    use sha2::{Digest, Sha256};
    let parameters = RunParameters {
        extra_vars: BTreeMap::new(),
        tags: vec![],
        skip_tags: vec![],
        diff: false,
        vault: None,
    };
    let inventory = json!({"fixture":"native-owned"});
    let targets:Vec<ApprovedTarget>=serde_json::from_value(json!([{"host_id":Uuid::new_v4(),"alias":"fixture","address":"192.0.2.1","port":22,"username":"fixture","route_digest":"a".repeat(64),"credential_refs":[]}])).unwrap();
    // A real canonical witness, matching the production fixed input format.
    fn canonical(v: Value) -> Value {
        match v {
            Value::Object(m) => {
                Value::Object(m.into_iter().map(|(k, v)| (k, canonical(v))).collect())
            }
            Value::Array(a) => Value::Array(a.into_iter().map(canonical).collect()),
            v => v,
        }
    }
    let witness = |value: Value| {
        let raw = serde_json::to_string(&canonical(value)).unwrap();
        json!({"digest":hex::encode(Sha256::digest(raw.as_bytes())),"canonical_json":raw})
    };
    let pw = witness(serde_json::to_value(&parameters).unwrap());
    let iw = witness(inventory.clone());
    let td = hex::encode(Sha256::digest(
        serde_json::to_vec(&canonical(serde_json::to_value(&targets).unwrap())).unwrap(),
    ));
    let checks=["syntax","lint"].map(|name|json!({"name":name,"exit_code":1,"output":output,"truncated":false,"cancelled":false,"timed_out":false,"elapsed_ms":1}));
    serde_json::from_value::<ValidationProof>(json!({"revision_digest":revision,"environment_digest":runtime().environment_digest(),"playbook":"playbook.yml","parameters_digest":pw["digest"],"inventory_digest":iw["digest"],"targets_digest":td,"valid":false,"checks":checks,"input_witnesses":{"parameters":pw,"inventory":iw}})).unwrap()
        .verify(revision,runtime().environment_digest(),"playbook.yml",&parameters,&inventory,&targets).unwrap()
}
#[tokio::test]
async fn native_potential_vault_marker_blocks_decrypted_value_from_actual_ai_request() {
    let f = Fixture::new().await;
    let scope = f.scope().await;
    let decrypted = "SYNTHETIC_DECRYPTED_API_VALUE_917";
    // No phase/material lookup has happened: potential use already requires
    // trusted withholding, independently of password masking or guest no_log.
    let raw = verified(&f.digest, decrypted);
    assert!(serde_json::to_string(&raw).unwrap().contains(decrypted));
    let sealed = scope.seal_result(raw).unwrap();
    let projection = sealed.ai_summary().to_string();
    let request = cc_ai_core::automation::PreparedAnsibleRequest::new(
        cc_ai_core::sanitizer::PrivacyProfile::Strict,
        "Explain validation status",
        "playbook.yml",
        Some(&projection),
    )
    .unwrap();
    for text in [
        request.preview(),
        serde_json::to_string(&sealed).unwrap(),
        projection,
        format!("{sealed:?}"),
    ] {
        assert!(!text.contains(decrypted));
        assert!(!text.contains("synthetic-password-42"));
    }
    assert!(sealed
        .checks()
        .iter()
        .all(|c| c.output == "Vault-sensitive diagnostics withheld"));
    drop(scope);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn four_distinct_native_child_grants_do_not_cache_or_replay_password() {
    let f = Fixture::new().await;
    let scope = f.scope().await;
    let mut nonces = std::collections::HashSet::new();
    for phase in [VaultValidationPhase::Syntax, VaultValidationPhase::Lint] {
        for call in 0..2 {
            let req = scope.request_call(phase, call).unwrap();
            assert!(nonces.insert(req.phase_nonce));
            let mut owner = scope.phase(&f.app, &req, runtime()).await.unwrap();
            let mut pipe = vec![];
            owner.lease.write_once(&req, &mut pipe).unwrap();
            assert_eq!(pipe, b"synthetic-password-42\n");
            assert!(scope.phase(&f.app, &req, runtime()).await.is_err());
        }
        assert!(scope.request_call(phase, 2).is_err());
        let mut third = scope.request(phase);
        third.call_index = 2;
        assert!(scope.phase(&f.app, &third, runtime()).await.is_err());
    }
    assert_eq!(nonces.len(), 4);
    drop(scope);
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn second_loader_rechecks_real_metadata_sql_and_files_instead_of_cached_phase_material() {
    for cause in ["credential", "sql", "filesystem", "lock"] {
        let f = Fixture::new().await;
        let scope = f.scope().await;
        let first = scope.request_call(VaultValidationPhase::Syntax, 0).unwrap();
        let mut owner = scope.phase(&f.app, &first, runtime()).await.unwrap();
        owner.lease.write_once(&first, &mut vec![]).unwrap();
        drop(owner);
        match cause {
            "credential" => {
                f.app
                    .set_credential_password(f.credential.clone(), "synthetic replacement".into())
                    .await
                    .unwrap();
            }
            "sql" => {
                f.app
                    .automation_project_rename(
                        f.project.clone(),
                        1,
                        "Changed between loader calls".into(),
                    )
                    .await
                    .unwrap();
            }
            "lock" => f.app.lock().await.unwrap(),
            _ => std::fs::write(
                scope.review.project.path.join("playbook.yml"),
                b"actual user edit\n",
            )
            .unwrap(),
        }
        let second = scope.request_call(VaultValidationPhase::Syntax, 1).unwrap();
        assert!(
            scope.phase(&f.app, &second, runtime()).await.is_err(),
            "{cause}"
        );
        drop(scope);
        f.app.shutdown().await.unwrap();
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn simultaneous_same_child_has_exactly_one_winner() {
    let f = Fixture::new().await;
    let scope = f.scope().await;
    let a = scope.request_call(VaultValidationPhase::Syntax, 0).unwrap();
    let b = scope.request_call(VaultValidationPhase::Syntax, 0).unwrap();
    let (left, right) = tokio::join!(
        scope.phase(&f.app, &a, runtime()),
        scope.phase(&f.app, &b, runtime())
    );
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    drop(left);
    drop(right);
    assert!(scope
        .phase(
            &f.app,
            &scope.request_call(VaultValidationPhase::Syntax, 1).unwrap(),
            runtime()
        )
        .await
        .is_ok());
    drop(scope);
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn malformed_runtime_or_third_child_does_not_consume_any_correct_native_grant() {
    let f = Fixture::new().await;
    let scope = f.scope().await;
    let mut third = scope.request(VaultValidationPhase::Syntax);
    third.call_index = 2;
    assert!(scope.phase(&f.app, &third, runtime()).await.is_err());
    let mut wrong = scope.request(VaultValidationPhase::Syntax);
    wrong.helper_digest = "d".repeat(64);
    assert!(scope.phase(&f.app, &wrong, runtime()).await.is_err());
    assert!(scope
        .issued
        .iter()
        .flatten()
        .all(|used| !used.load(Ordering::Acquire)));
    let valid = scope.request_call(VaultValidationPhase::Syntax, 0).unwrap();
    assert!(scope.phase(&f.app, &valid, runtime()).await.is_ok());
    drop(scope);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn current_root_replacement_or_blocked_namespace_then_content_edit_denies_next_loader() {
    let f = Fixture::new().await;
    // A dedicated source ancestor, NOT the profile/catalog ancestor. Every
    // rename/recreation below remains inside this newly owned TempDir.
    let parent = f._directory.path().join("owned-source-parent");
    let path = parent.join("project");
    let moved = f._directory.path().join("owned-original-parent");
    std::fs::create_dir(&parent).unwrap();
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("playbook.yml"),b"---\n- name: Owned validation fixture\n  hosts: all\n  gather_facts: false\n  tasks: []\n").unwrap();
    let project = f
        .app
        .automation_project_create(
            "Own named source".into(),
            Some(path.to_string_lossy().into_owned()),
        )
        .await
        .unwrap();
    f.app
        .automation_project_save(
            project.id.clone(),
            VAULT_INPUT_FILE.into(),
            None,
            json!({"version":1,"ansible_vault":{"credential_id":f.credential}}).to_string(),
        )
        .await
        .unwrap();
    // Same fixture/ACL can rename before capture. The later OS deny therefore
    // cannot pass merely because the parent was unwritable from the outset.
    std::fs::rename(&parent, &moved).unwrap();
    std::fs::rename(&moved, &parent).unwrap();
    let revision = f
        .app
        .automation_project_files(project.id.clone())
        .await
        .unwrap();
    let review = f
        .app
        .automation_vault_capture(
            project.id,
            1,
            revision.digest,
            f.credential.clone(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    let scope = review
        .begin_validation(&f.app, "playbook.yml".into(), runtime())
        .await
        .unwrap();
    assert_eq!(
        scope.review.project.path.canonicalize().unwrap(),
        path.canonicalize().unwrap()
    );
    let first = scope.request_call(VaultValidationPhase::Syntax, 0).unwrap();
    let mut issued = scope.phase(&f.app, &first, runtime()).await.unwrap();
    issued.lease.write_once(&first, &mut vec![]).unwrap();
    drop(issued);
    assert_eq!(parent.parent(), Some(f._directory.path()));
    assert_eq!(moved.parent(), Some(f._directory.path()));
    match std::fs::rename(&parent, &moved) {
        Ok(()) => {
            std::fs::create_dir(&parent).unwrap();
            std::fs::create_dir(&path).unwrap();
            let replacement = ProjectRoot::open(&path).unwrap();
            scope
                .review
                .root
                .freeze_to(
                    &scope.review.revision,
                    &replacement,
                    &CancellationToken::new(),
                )
                .unwrap();
            assert_eq!(replacement.revision().unwrap(), scope.review.revision);
            assert_eq!(scope.review.root.revision().unwrap(), scope.review.revision);
            assert!(!scope.review.root.same_directory(&replacement).unwrap());
        }
        Err(error) => {
            #[cfg(windows)]
            {
                assert!(matches!(error.raw_os_error(), Some(5 | 32)));
                assert!(path.is_dir() && !moved.exists());
                assert_eq!(scope.review.root.revision().unwrap(), scope.review.revision);
                assert!(scope
                    .review
                    .root
                    .reopen_same_directory(&scope.review.project.path)
                    .is_ok());
                let sibling = f._directory.path().join("owned-identical-sibling");
                std::fs::create_dir(&sibling).unwrap();
                let replacement = ProjectRoot::open(&sibling).unwrap();
                scope
                    .review
                    .root
                    .freeze_to(
                        &scope.review.revision,
                        &replacement,
                        &CancellationToken::new(),
                    )
                    .unwrap();
                assert_eq!(replacement.revision().unwrap(), scope.review.revision);
                assert!(!scope.review.root.same_directory(&replacement).unwrap());
                assert!(scope.review.root.reopen_same_directory(&sibling).is_err());
                std::fs::write(
                    path.join("playbook.yml"),
                    b"actual current user source edit\n",
                )
                .unwrap();
                assert_ne!(scope.review.root.revision().unwrap(), scope.review.revision);
            }
            #[cfg(not(windows))]
            panic!("owned Unix ancestor rename unexpectedly denied: {error}");
        }
    }
    let second = scope.request_call(VaultValidationPhase::Syntax, 1).unwrap();
    assert!(scope.phase(&f.app, &second, runtime()).await.is_err());
    drop(scope);
    f.app.shutdown().await.unwrap();
}

#[test]
fn blocked_phase_filesystem_worker_does_not_retain_unlocked_keys() {
    let owned_runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(1)
        .enable_all()
        .build()
        .unwrap();
    owned_runtime.block_on(async {
        let f = Fixture::new().await;
        let scope = f.scope().await;
        let baseline = scope.review.project.unlock.strong_count();
        let slots = scope
            .review
            .project
            .owner
            .automation
            .io_slots
            .available_permits();
        let (started, ready) = std::sync::mpsc::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let blocker = tokio::task::spawn_blocking(move || {
            started.send(()).unwrap();
            let _ = wait.recv();
        });
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        let called = f.app.clone();
        let retained = scope.clone();
        let phase = tokio::spawn(async move {
            let request = retained
                .request_call(VaultValidationPhase::Syntax, 0)
                .unwrap();
            retained.phase(&called, &request, runtime()).await
        });
        tokio::time::timeout(Duration::from_secs(2), async {
            while scope
                .review
                .project
                .owner
                .automation
                .io_slots
                .available_permits()
                == slots
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        // Existing real phase has acquired its FS permit and queued behind the
        // occupied blocking worker. No mock resolver or phase hook is used.
        assert!(
            scope.review.project.unlock.strong_count() <= baseline,
            "phase retained strong unlock/keys across blocked filesystem await"
        );
        release.send(()).unwrap();
        blocker.await.unwrap();
        let password = phase.await.unwrap().unwrap();
        drop(password);
        drop(scope);
        f.app.shutdown().await.unwrap();
    });
}
