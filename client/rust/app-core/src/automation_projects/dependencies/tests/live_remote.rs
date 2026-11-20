//! Ignored private acceptance test. The coordinator supplies ONLY an owned
//! fixture's pins/control files; all connection/RPC/archive witnesses are real.
use super::*;
use crate::{automation_runners::AutomationRunnerConfig, HostDto};
use cc_automation_core::{DependencyArtifactSource, DependencyKind, RemoteHelperConfig};
use cc_models::host::HostKeyPolicy;
use cc_ssh_core::keys::{generate_key, KeyGenAlgorithm};
use secrecy::ExposeSecret;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

const ARCHIVE: &str = "64f6cd4bc82fc6a8b63a59286a2a0e7196de67f9398ed80cc4eac815c820e8b5";
const TREE: &str = "49094b48853bc24514b21e7c2f983db3b6adf038b157e635fdec53b51a2bdebd";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    version: u32,
    token: String,
    ssh_port: u16,
    host_public_key: String,
    tls_port: u16,
    archive_sha256: String,
    egress_blocked: bool,
}
fn new_json(root: &Path, name: &str, value: Value) {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(root.join(name)).unwrap();
    let bytes = serde_json::to_vec(&value).unwrap();
    assert!(bytes.len() <= 16 * 1024);
    file.write_all(&bytes).unwrap();
    file.sync_all().unwrap();
}
fn input(root: &Path) -> Input {
    let path = root.join("fixture.json");
    assert!(std::fs::symlink_metadata(&path).unwrap().is_file());
    let file = std::fs::File::open(path).unwrap();
    let mut bytes = Vec::new();
    file.take(16 * 1024 + 1).read_to_end(&mut bytes).unwrap();
    assert!(bytes.len() <= 16 * 1024);
    serde_json::from_slice(&bytes).unwrap()
}

struct Observed {
    operation: Arc<Operation>,
    candidate_path: PathBuf,
    candidate: Weak<ProjectRoot>,
    peer: Weak<cc_automation_core::HelperPeer>,
    digest: String,
}
async fn facade(app: &AppCore, action: Value) -> AppResult<Value> {
    let raw = app
        .automation_dependency_action_json(json!({"version":1,"action":action}).to_string())
        .await?;
    let result: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(result["version"], 1);
    Ok(result["data"].clone())
}
async fn prepared(
    app: &AppCore,
    project: &str,
    runner: &str,
    selected: DependencySourceSelection,
    registry: &Arc<DependencyRegistry>,
    public_facade: bool,
) -> Observed {
    let (id, review_id) = if public_facade {
        let selected = serde_json::to_value(&selected).unwrap();
        let review=facade(app,json!({"type":"review","projectId":project,"projectRevision":1,"runnerId":runner,"runnerRevision":1,
            "dependency":{"kind":selected["kind"],"name":selected["name"],"version":selected["version"],
                "url":selected["source"]["url"],"archiveSha256":selected["source"]["sha256"]}})).await.unwrap();
        (
            Uuid::parse_str(review["operationId"].as_str().unwrap()).unwrap(),
            review["reviewId"].as_str().unwrap().to_owned(),
        )
    } else {
        let review = app
            .automation_dependency_review(project.into(), 1, runner.into(), 1, selected)
            .await
            .unwrap();
        (review.operation_id, review.review_id.to_string())
    };
    if public_facade {
        let started = facade(app, json!({"type":"start","reviewId":review_id}))
            .await
            .unwrap();
        assert_eq!(started["operationId"], id.to_string());
        assert!(facade(app, json!({"type":"start","reviewId":review_id}))
            .await
            .is_err());
    } else {
        let started = app
            .automation_dependency_start(review_id.clone())
            .await
            .unwrap();
        assert_eq!(started.operation_id, id);
        assert!(app.automation_dependency_start(review_id).await.is_err());
    }
    tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            if public_facade {
                let status = facade(app, json!({"type":"status","operationId":id}))
                    .await
                    .unwrap();
                assert_eq!(status["operationId"], id.to_string());
            }
            let status = app
                .automation_dependency_status(id.to_string())
                .await
                .unwrap();
            if status.phase == Phase::CandidatePreview {
                break;
            }
            assert!(
                !matches!(
                    status.phase,
                    Phase::Failed | Phase::Cancelled | Phase::Expired | Phase::ContextChanged
                ),
                "actual AppCore prepare failed: {}",
                serde_json::to_string(&status).unwrap()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("actual Remote SSH factory/helper preparation deadline");
    let operation = registry.get(id).unwrap();
    let (candidate_path, candidate, digest) = {
        let state = operation.state.lock().unwrap_or_else(|e| e.into_inner());
        let State::Ready(view, ready) = &*state else {
            panic!("expected actual verified Ready");
        };
        assert!(view.archive_verified);
        assert_eq!(view.phase, Phase::CandidatePreview);
        assert_eq!(ready.preview.archive().archive().tree().digest, TREE);
        assert!(view.change_count >= 6);
        assert!(!view.changes_truncated);
        assert_eq!(
            ready.candidate.revision().unwrap(),
            *ready.preview.candidate()
        );
        assert_eq!(
            ready
                .candidate
                .read_file("roles/demo/tasks/main.yml")
                .unwrap()
                .text,
            "---\n# untrusted bytes only\n"
        );
        let path = ready
            ._resources
            .staging
            .as_ref()
            .unwrap()
            .path()
            .to_path_buf();
        assert!(path.is_dir());
        (
            path,
            Arc::downgrade(&ready.candidate),
            view.review_digest.clone().unwrap(),
        )
    };
    assert!(operation.retained_transport.load(Ordering::Acquire));
    let peer = operation
        .peer
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    assert!(!peer.upgrade().unwrap().is_closed());
    Observed {
        operation,
        candidate_path,
        candidate,
        peer,
        digest,
    }
}
async fn cleaned(observed: &Observed, registry: &DependencyRegistry) {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let gone = !observed.candidate_path.exists() && observed.candidate.upgrade().is_none();
            let closed = observed.peer.upgrade().is_none_or(|peer| peer.is_closed());
            // Remote SSH close was awaited, not promoted to remote process join.
            if gone && closed && observed.operation.cleanup.load(Ordering::Acquire) == 3 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("owned candidate/peer cleanup did not complete");
    assert!(registry
        .maps
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .projects
        .is_empty());
    assert_eq!(ACTIVE.get().unwrap().available_permits(), 2);
}
fn unchanged(base: &ProjectRoot, before: &ProjectRevision) {
    assert_eq!(&base.revision().unwrap(), before);
    assert_eq!(
        base.read_file("owned-proof.yml").unwrap().text,
        "---\n# original owned project bytes\n"
    );
    assert!(!before.entries.iter().any(|entry| matches!(
        entry.path(),
        ".consolecrypt-dependencies.json" | ".consolecrypt-dependency-receipts.json"
    )));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires explicitly owned pinned SSH/TLS coordinator; no user endpoint/profile"]
async fn remote_appcore_dependency_prepare_review_accept_and_cleanup() {
    let token = std::env::var("CC_ANS05_APPCORE_TOKEN").expect("owned coordinator UUID required");
    let id = Uuid::parse_str(&token).unwrap();
    assert!(!id.is_nil());
    assert_eq!(id.to_string(), token);
    let root = PathBuf::from(
        std::env::var_os("CC_ANS05_APPCORE_ROOT").expect("owned control path required"),
    );
    assert!(root.is_absolute());
    assert_eq!(
        root.file_name().unwrap().to_str().unwrap(),
        format!("owned-{token}")
    );
    assert!(!std::fs::symlink_metadata(&root)
        .unwrap()
        .file_type()
        .is_symlink());
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let allowed = workspace
        .join("automation-e1-next/app-core-integration/dependency-live-remote-v1/control")
        .canonicalize()
        .unwrap();
    assert_eq!(root.canonicalize().unwrap().parent().unwrap(), allowed);
    let keys = generate_key(
        KeyGenAlgorithm::Ed25519,
        "owned AppCore dependency proof",
        None,
    )
    .unwrap();
    new_json(
        &root,
        "native-public.json",
        json!({"version":1,"token":token,"public_key":keys.public_openssh}),
    );
    tokio::time::timeout(Duration::from_secs(60), async {
        while !root.join("fixture.json").is_file() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("owned fixture pins were not supplied");
    let fixture = input(&root);
    assert_eq!(fixture.version, 1);
    assert_eq!(fixture.token, token);
    assert!(fixture.ssh_port > 0 && fixture.tls_port > 0);
    assert_eq!(fixture.archive_sha256, ARCHIVE);
    assert!(
        fixture.host_public_key.starts_with("ssh-ed25519 ")
            && fixture.host_public_key.len() < 8192
            && !fixture.host_public_key.chars().any(char::is_control)
    );
    let profile = tempfile::Builder::new()
        .prefix("native-profile-")
        .tempdir_in(&root)
        .unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        profile.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile(
        "Owned AppCore dependency fixture".into(),
        "synthetic profile passphrase 42".into(),
    )
    .await
    .unwrap();
    let credential = app
        .import_ssh_key(
            "Own ephemeral fixture key".into(),
            Some("runner".into()),
            keys.private_openssh.expose_secret().to_owned(),
            None,
            false,
            None,
        )
        .await
        .unwrap();
    app.import_known_hosts(format!(
        "[127.0.0.1]:{} {}\n",
        fixture.ssh_port, fixture.host_public_key
    ))
    .await
    .unwrap();
    let mut host = HostDto::new("Owned dependency fixture", "127.0.0.1");
    host.port = Some(fixture.ssh_port);
    host.username = Some("runner".into());
    host.credential_id = Some(credential.id);
    host.host_key_policy = HostKeyPolicy::Strict;
    let host = app.save_host(host).await.unwrap();
    let configured = format!("/home/runner/proof-{token}/jobs");
    let runner = app
        .automation_runner_create(
            "Dedicated Remote SSH dependency fixture".into(),
            AutomationRunnerConfig::Remote {
                host_id: host.id,
                helper: RemoteHelperConfig {
                    python: format!("/home/runner/proof-{token}/python"),
                    module_dir: "/opt/consolecrypt/src".into(),
                    root: configured,
                },
            },
        )
        .await
        .unwrap();
    let project = app
        .automation_project_create("Owned dependency base".into(), None)
        .await
        .unwrap();
    app.automation_project_save(
        project.id.clone(),
        "owned-proof.yml".into(),
        None,
        "---\n# original owned project bytes\n".into(),
    )
    .await
    .unwrap();
    let context = app.automation_project_context(&project.id).await.unwrap();
    let base = ProjectRoot::open(&context.path).unwrap();
    let before = base.revision().unwrap();
    let (owner, u) = app.unlocked().await.unwrap();
    drop(u);
    let registry = owner.automation.dependencies.clone();
    let selected = DependencySourceSelection::new(
        DependencyKind::Role,
        "demo".into(),
        "1.2.3".into(),
        DependencyArtifactSource {
            url: format!("https://127.0.0.1:{}/artifact.tar.gz", fixture.tls_port),
            sha256: ARCHIVE.into(),
        },
        0,
    )
    .unwrap();
    let first = prepared(
        &app,
        &project.id,
        &runner.id,
        selected.clone(),
        &registry,
        true,
    )
    .await;
    let operation = first.operation.id.to_string();
    assert!(facade(&app,json!({"type":"changes","operationId":operation,"reviewDigest":"f".repeat(64),"offset":0,"limit":1})).await.is_err());
    let evidence = facade(
        &app,
        json!({"type":"evidence","operationId":operation,"reviewDigest":first.digest}),
    )
    .await
    .unwrap();
    assert_eq!(evidence["archiveSha256"], ARCHIVE);
    assert_eq!(evidence["archiveTreeDigest"], TREE);
    assert_eq!(
        evidence["candidateReceiptSha256"].as_str().unwrap().len(),
        64
    );
    assert_eq!(evidence["candidateLockSha256"].as_str().unwrap().len(), 64);
    assert_eq!(evidence["published"], false);
    assert_eq!(evidence["executionEnabled"], false);
    let page=facade(&app,json!({"type":"changes","operationId":operation,"reviewDigest":first.digest,"offset":0,"limit":2})).await.unwrap();
    assert_eq!(page["changes"].as_array().unwrap().len(), 2);
    assert!(page["total"].as_u64().unwrap() > 2);
    let next=facade(&app,json!({"type":"changes","operationId":operation,"reviewDigest":first.digest,"offset":2,"limit":100})).await.unwrap();
    assert_eq!(next["offset"], 2);
    assert_eq!(next["total"], page["total"]);
    let file=facade(&app,json!({"type":"file","operationId":operation,"reviewDigest":first.digest,"path":"roles/demo/lookup_plugins/evil.py"})).await.unwrap();
    assert_eq!(file["before"], Value::Null);
    assert_eq!(file["after"], "raise RuntimeError(\"must never execute\")");
    let accepted = facade(
        &app,
        json!({"type":"accept","operationId":operation,"reviewDigest":first.digest}),
    )
    .await
    .unwrap();
    assert_eq!(accepted["retainedCandidate"], true);
    assert_eq!(accepted["published"], false);
    assert_eq!(accepted["executionEnabled"], false);
    assert!(
        matches!(&*first.operation.state.lock().unwrap(),State::Accepted(_,_,acceptance)if acceptance.registry_owned)
    );
    assert!(facade(
        &app,
        json!({"type":"accept","operationId":operation,"reviewDigest":first.digest})
    )
    .await
    .is_err());
    facade(
        &app,
        json!({"type":"reject","operationId":operation,"reviewDigest":first.digest}),
    )
    .await
    .unwrap();
    cleaned(&first, &registry).await;
    unchanged(&base, &before);

    let second = prepared(
        &app,
        &project.id,
        &runner.id,
        selected.clone(),
        &registry,
        false,
    )
    .await;
    let accepted = app
        .automation_dependency_accept(second.operation.id.to_string(), second.digest.clone())
        .await
        .unwrap();
    assert!(accepted.is_retained());
    drop(accepted);
    cleaned(&second, &registry).await;
    unchanged(&base, &before);
    assert_eq!(second.operation.view().phase, Phase::Rejected);

    let cancelled = prepared(
        &app,
        &project.id,
        &runner.id,
        selected.clone(),
        &registry,
        false,
    )
    .await;
    app.automation_dependency_cancel(cancelled.operation.id.to_string())
        .await
        .unwrap();
    assert!(app
        .automation_dependency_accept(cancelled.operation.id.to_string(), cancelled.digest.clone())
        .await
        .is_err());
    cleaned(&cancelled, &registry).await;
    unchanged(&base, &before);
    let facade_proof = match std::env::var("CC_ANS05_FACADE_PROOF") {
        Ok(value) => {
            assert_eq!(value, token);
            true
        }
        Err(std::env::VarError::NotPresent) => false,
        Err(_) => panic!("invalid owned proof flag"),
    };
    let extra = if facade_proof {
        // Source-parent is a newly owned fixture, NEVER profile/catalog ancestry.
        let parent = root.join("facade-source-parent");
        let held = root.join("facade-source-parent-held");
        let source = parent.join("project");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            source.join("owned-proof.yml"),
            "---\n# original owned project bytes\n",
        )
        .unwrap();
        std::fs::rename(&parent, &held).unwrap();
        std::fs::rename(&held, &parent).unwrap(); // ACL/control before capture
        let own = app
            .automation_project_create(
                "Owned facade current-name fixture".into(),
                Some(source.to_string_lossy().into_owned()),
            )
            .await
            .unwrap();
        let named = ProjectRoot::open(&source).unwrap();
        let original = named.revision().unwrap();
        let prepared = prepared(&app, &own.id, &runner.id, selected.clone(), &registry, true).await;
        facade(&app,json!({"type":"changes","operationId":prepared.operation.id,"reviewDigest":prepared.digest,"offset":0,"limit":1})).await.unwrap();
        match std::fs::rename(&parent, &held) {
            Ok(()) => {
                std::fs::create_dir_all(&source).unwrap();
                let replacement = ProjectRoot::open(&source).unwrap();
                named
                    .freeze_to(&original, &replacement, &CancellationToken::new())
                    .unwrap();
                assert_eq!(replacement.revision().unwrap(), original);
                assert!(!named.same_directory(&replacement).unwrap());
            }
            Err(error) => {
                #[cfg(windows)]
                {
                    assert!(
                        matches!(error.raw_os_error(), Some(5 | 32)),
                        "unexpected owned namespace error: {error}"
                    );
                    assert_eq!(
                        ProjectRoot::open(&source).unwrap().revision().unwrap(),
                        original
                    );
                    let sibling = root.join("facade-identical-sibling");
                    std::fs::create_dir(&sibling).unwrap();
                    let replacement = ProjectRoot::open(&sibling).unwrap();
                    named
                        .freeze_to(&original, &replacement, &CancellationToken::new())
                        .unwrap();
                    assert_eq!(replacement.revision().unwrap(), original);
                    assert!(!named.same_directory(&replacement).unwrap());
                    std::fs::write(
                        source.join("owned-proof.yml"),
                        "---\n# actual content changed after review\n",
                    )
                    .unwrap();
                }
                #[cfg(not(windows))]
                panic!("owned ancestor replacement unexpectedly denied: {error}");
            }
        }
        for action in [
            json!({"type":"changes","operationId":prepared.operation.id,"reviewDigest":prepared.digest,"offset":0,"limit":1}),
            json!({"type":"file","operationId":prepared.operation.id,"reviewDigest":prepared.digest,"path":"roles/demo/tasks/main.yml"}),
            json!({"type":"accept","operationId":prepared.operation.id,"reviewDigest":prepared.digest}),
        ] {
            assert!(
                facade(&app, action).await.is_err(),
                "current-name/content fence admitted a stale reviewed tree"
            );
        }
        facade(
            &app,
            json!({"type":"cancel","operationId":prepared.operation.id}),
        )
        .await
        .unwrap();
        cleaned(&prepared, &registry).await;
        // Only restore this fixture's own changed file; no publisher touched it.
        std::fs::write(
            source.join("owned-proof.yml"),
            "---\n# original owned project bytes\n",
        )
        .unwrap();
        assert_eq!(
            ProjectRoot::open(&source).unwrap().revision().unwrap(),
            original
        );
        unchanged(&base, &before);
        Some(prepared.operation.id)
    } else {
        None
    };
    // Explicit test-only coordinator mode; never selected by an ordinary test,
    // UI flag, journal or Helper response. Existing 3/4/5-operation proofs stay
    // unchanged unless the owned coordinator supplies this exact UUID token.
    let reconciliation_proof = std::env::var("CC_ANS05_RECONCILIATION_PROOF").ok().as_deref() == Some(token.as_str());
    if reconciliation_proof {
        assert_eq!(std::env::var("CC_ANS05_PUBLICATION_PROOF").ok().as_deref(),Some(token.as_str()));
    }
    let publication_operation = if std::env::var("CC_ANS05_PUBLICATION_PROOF").ok().as_deref() == Some(token.as_str()) {
        assert!(facade_proof);
        let own = app.automation_project_create("Owned publication generation".into(),None).await.unwrap();
        app.automation_project_save(own.id.clone(),"owned-proof.yml".into(),None,"---\n# original owned project bytes\n".into()).await.unwrap();
        let original_context=app.automation_project_context(&own.id).await.unwrap();
        let original_root=ProjectRoot::open(&original_context.path).unwrap();
        let original_revision=original_root.revision().unwrap();
        let proof=prepared(&app,&own.id,&runner.id,selected.clone(),&registry,true).await;
        let (expected,expected_lock,expected_receipt)={
            let state=proof.operation.state.lock().unwrap();
            let State::Ready(_,ready)=&*state else {panic!("genuine verified Ready required")};
            (ready.preview.candidate().clone(),
                ready.candidate.read_file(".consolecrypt-dependencies.json").unwrap().sha256,
                ready.candidate.read_file(".consolecrypt-dependency-receipts.json").unwrap().sha256)
        };
        facade(&app,json!({"type":"accept","operationId":proof.operation.id,"reviewDigest":proof.digest})).await.unwrap();
        let result=app.automation_dependency_publish(proof.operation.id.to_string(),proof.digest.clone()).await.unwrap();
        let result=serde_json::to_value(result).unwrap();
        assert_eq!(result["revision"],2);
        assert_eq!(result["candidateDigest"],expected.digest);
        assert_eq!(proof.operation.view().phase,Phase::Published);
        cleaned(&proof,&registry).await;
        assert_eq!(owner.automation.io_slots.available_permits(),4);
        assert_eq!(original_root.revision().unwrap(),original_revision);
        let reloaded=app.automation_project_context(&own.id).await.unwrap();
        assert_eq!(reloaded.project.source_kind.as_deref(),Some("managed"));
        assert_eq!(reloaded.project.revision,Some(2));
        assert_ne!(reloaded.path,original_context.path);
        assert_eq!(app.automation_project_files(own.id.clone()).await.unwrap(),expected);
        assert_eq!(app.automation_project_file(own.id.clone(),".consolecrypt-dependencies.json".into()).await.unwrap().sha256,expected_lock);
        assert_eq!(app.automation_project_file(own.id.clone(),".consolecrypt-dependency-receipts.json".into()).await.unwrap().sha256,expected_receipt);
        let selected_catalog:Value=owner.storage().unwrap().setting_get(super::super::super::PROJECTS).await.unwrap().unwrap();
        if reconciliation_proof {
            // GENUINE actual Core-published generation and actual current SQL;
            // no witness is reconstructed and no extra Helper request is made.
            for _ in 0..2 {
                let audit=app.automation_dependency_reconcile_selected(own.id.clone(),2).await.unwrap();
                let audit=serde_json::to_value(audit).unwrap();
                assert_eq!(audit["version"],1);
                assert_eq!(audit["selectedRevision"],2);
                assert_eq!(audit["transaction"],result["transaction"]);
                assert_eq!(audit["observation"],"complete");
                assert_eq!(owner.automation.io_slots.available_permits(),4);
            }
            assert_eq!(owner.storage().unwrap().setting_get::<Value>(super::super::super::PROJECTS).await.unwrap().unwrap(),selected_catalog);
            assert_eq!(app.automation_project_files(own.id.clone()).await.unwrap(),expected);
        }
        let editable=app.automation_project_file(own.id.clone(),"roles/demo/tasks/main.yml".into()).await.unwrap();
        app.automation_project_save(own.id.clone(),editable.path,Some(editable.sha256),format!("{}\n# owned ordinary post-install edit\n",editable.text)).await.unwrap();
        assert_ne!(app.automation_project_files(own.id.clone()).await.unwrap().digest,expected.digest);
        if reconciliation_proof {
            // Historical audit refuses edited content; the ordinary pinned
            // project reader/editor deliberately continues to allow edits.
            assert_eq!(owner.storage().unwrap().setting_get::<Value>(super::super::super::PROJECTS).await.unwrap().unwrap(),selected_catalog);
            assert!(app.automation_dependency_reconcile_selected(own.id.clone(),2).await.is_err());
            let edited=app.automation_project_file(own.id.clone(),"roles/demo/tasks/main.yml".into()).await.unwrap();
            assert!(edited.text.ends_with("# owned ordinary post-install edit\n"));
            let again=app.automation_project_save(own.id.clone(),edited.path,Some(edited.sha256),format!("{}# second explicit ordinary edit\n",edited.text)).await.unwrap();
            assert!(again.text.ends_with("# second explicit ordinary edit\n"));
            assert_eq!(owner.storage().unwrap().setting_get::<Value>(super::super::super::PROJECTS).await.unwrap().unwrap(),selected_catalog);
        }
        assert_eq!(original_root.revision().unwrap(),original_revision);
        assert!(app.automation_dependency_publish(proof.operation.id.to_string(),proof.digest.clone()).await.is_err());
        assert!(app.automation_jobs().await.unwrap()["jobs"].as_array().unwrap().is_empty());
        Some(proof.operation.id)
    } else {None};

    let third = prepared(&app, &project.id, &runner.id, selected, &registry, false).await;
    app.lock().await.unwrap();
    registry.close_and_wait().await;
    assert!(app
        .automation_dependency_accept(third.operation.id.to_string(), third.digest.clone())
        .await
        .is_err());
    cleaned(&third, &registry).await;
    unchanged(&base, &before);
    assert!(registry
        .workers
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_empty());
    assert!(registry
        .cleanups
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_empty());
    assert!(owner
        .automation
        .connections
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_empty());
    app.shutdown().await.unwrap();
    drop(profile);
    let mut operations = vec![
        first.operation.id,
        second.operation.id,
        cancelled.operation.id,
        third.operation.id,
    ];
    if let Some(extra) = extra {
        operations.push(extra);
    }
    if let Some(published) = publication_operation {operations.push(published);}
    new_json(
        &root,
        "native-result.json",
        json!({"version":1,"token":token,"ok":true,"operations":operations,
        "actualRemoteFactory":true,"nativeArchiveVerified":true,"localCandidateDirectoriesRemoved":true,"nativeOwnedCloseAwaited":true,
        "remoteStop":"unconfirmed","originalDigest":before.digest,"sourceUnchanged":true,"published":publication_operation.is_some(),"publicationProof":publication_operation.is_some(),"reconciliationProof":reconciliation_proof,
        "executedProjectCode":false,"egressBlocked":fixture.egress_blocked,"facadeProof":facade_proof,"currentNameOrContentFenceDenied":extra.is_some()}),
    );
    println!("owned actual AppCore Remote SSH dependency proof PASS; publication flag={}, reconciliation flag={}, executed project code=false", publication_operation.is_some(), reconciliation_proof);
}
