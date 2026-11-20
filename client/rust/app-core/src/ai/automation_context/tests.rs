//! Real profile/SQL/revision boundaries. No fabricated SSH/Runner authority.
use super::*;
use crate::{HostDto, config::AppConfig};
use std::{
    future::Future,
    task::{Context, Poll},
};
#[path = "../../../tests/common/ai_mock.rs"]
mod ai_mock;
const PASSWORD: &str = "owned AI selected-context profile password42";
struct Fixture {
    app: Arc<AppCore>,
    project: String,
    host: String,
    _dir: tempfile::TempDir,
}
impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let app = Arc::new(
            AppCore::new(AppConfig::for_tests(
                dir.path().to_string_lossy().into_owned(),
            ))
            .unwrap(),
        );
        app.create_local_profile("Owned selected context".into(), PASSWORD.into())
            .await
            .unwrap();
        let project = app
            .automation_project_create("Owned project".into(), None)
            .await
            .unwrap()
            .id;
        let mut host = HostDto::new("Unreachable selected metadata only", "127.0.0.1");
        host.port = Some(1);
        host.tags = vec!["env:owned".into()];
        let host = app.save_host(host).await.unwrap().id;
        Self {
            app,
            project,
            host,
            _dir: dir,
        }
    }
    async fn original(&self) -> (Arc<Session>, Arc<Unlocked>, Original, ProjectFence) {
        let (owner, u) = self.app.unlocked().await.unwrap();
        let original = Original::capture(
            &owner,
            &u,
            &owner.profile_id.to_string(),
            &u.notices.scope_id().unwrap(),
        )
        .unwrap();
        let context = self
            .app
            .automation_project_context_original(&self.project, owner.clone(), Arc::downgrade(&u))
            .await
            .unwrap();
        let project = ProjectFence::capture(&context, 1).unwrap();
        (owner, u, original, project)
    }
}
#[test]
fn typed_capture_rejects_duplicate_unknown_and_unbounded_json() {
    let base = json!({"operationId":Uuid::new_v4(),"profileId":Uuid::new_v4(),"notificationScopeId":Uuid::new_v4(),"projectId":Uuid::new_v4(),"expectedProjectRevision":1,"kind":"hostOs","subjectId":Uuid::new_v4()});
    assert!(parse::<CaptureRequest>(&base.to_string()).is_ok());
    let mut extra = base.clone();
    extra["command"] = json!("uname -a");
    assert!(parse::<CaptureRequest>(&extra.to_string()).is_err());
    let duplicate = base
        .to_string()
        .replacen('{', "{\"kind\":\"runnerVersions\",", 1);
    assert!(parse::<CaptureRequest>(&duplicate).is_err());
    assert!(parse::<CaptureRequest>(&"x".repeat(65537)).is_err());
}
#[tokio::test]
async fn original_scope_rejects_same_profile_lock_and_reunlock() {
    let f = Fixture::new().await;
    let (owner, u, original, _) = f.original().await;
    drop(u);
    f.app.lock().await.unwrap();
    f.app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    let (current, new_u) = f.app.unlocked().await.unwrap();
    assert_eq!(current.profile_id, owner.profile_id);
    assert!(original.check(&current, &new_u).is_err());
    let payload = json!({"operationId":Uuid::new_v4(),"profileId":original.profile,"notificationScopeId":original.scope,"projectId":f.project,"expectedProjectRevision":1,"kind":"hostOs","subjectId":f.host});
    assert!(
        f.app
            .automation_ai_context_capture_json(payload.to_string())
            .await
            .is_err()
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tag_only_explicit_preview_does_not_open_ssh_send_model_or_create_job() {
    let f = Fixture::new().await;
    let (owner, u, original, _) = f.original().await;
    let mock = ai_mock::MockLlm::start().await;
    let provider = f
        .app
        .save_ai_provider(crate::dto::AiProviderDto {
            id: String::new(),
            name: "Owned mock".into(),
            provider: cc_models::ai::AiProviderKind::OpenaiCompatible,
            base_url: mock.url(),
            has_api_key: false,
            chat_model: "mock-chat".into(),
            embedding_model: None,
            timeout_secs: 10,
            streaming: true,
            tool_support: false,
            privacy_profile: cc_ai_core::PrivacyProfile::Strict,
            is_default: true,
            created_at_ms: 0,
            updated_at_ms: 0,
        })
        .await
        .unwrap();
    let request = json!({"profileId":original.profile,"notificationScopeId":original.scope,"projectId":f.project,"expectedProjectRevision":1,
        "providerId":provider.id,"task":"Write a read-only draft","path":"playbook.yml","selectedText":null,"selections":[],"selectedGroups":[],"selectedTags":["env:owned"],"selectedHostId":f.host});
    let ready: Value = serde_json::from_str(
        &f.app
            .automation_ai_prepare_context_json(request.to_string())
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(ready["preview"].as_str().unwrap().contains("env:owned"));
    assert!(!ready["preview"].as_str().unwrap().contains("127.0.0.1"));
    assert_eq!(mock.count("/v1/chat/completions"), 0);
    assert!(owner.automation.connections.lock().unwrap().is_empty());
    assert!(
        f.app.automation_jobs().await.unwrap()["jobs"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    f.app
        .automation_ai_discard(ready["review_id"].as_str().unwrap().into())
        .await
        .unwrap();
    drop(u);
}
#[tokio::test(flavor = "current_thread")]
async fn queued_real_sql_equal_revision_project_rewrite_is_rejected_under_original_guards() {
    let f = Fixture::new().await;
    let (owner, u, original, project) = f.original().await;
    let contribution = ContextContribution {
        original,
        project,
        deadline: Instant::now() + TTL,
        facts: vec![],
        selected: json!({}),
        tag_metadata: None,
    };
    let _profile = f.app.hold_session(&owner).await.unwrap();
    let _unlock = owner.hold_unlocked(&u).await.unwrap();
    let storage = owner.storage().unwrap();
    let project = f.project.clone();
    let (entered, waiting) = tokio::sync::oneshot::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let writer = tokio::spawn(async move {
        storage
            .write(move |tx| {
                let _ = entered.send(());
                blocked.recv().unwrap();
                let mut value: Value = tx.setting_get("automation.projects.v1")?.unwrap();
                assert_eq!(value["records"][&project]["revision"], 1);
                value["records"][&project]["name"] = json!("equal revision actual SQL mutation");
                tx.setting_set("automation.projects.v1", &value)?;
                Ok::<_, AppError>(())
            })
            .await
    });
    waiting.await.unwrap();
    let check = contribution.check_held(&f.app, &owner, &u);
    tokio::pin!(check);
    let mut context = Context::from_waker(futures::task::noop_waker_ref());
    assert!(matches!(check.as_mut().poll(&mut context), Poll::Pending));
    release.send(()).unwrap();
    writer.await.unwrap().unwrap();
    assert!(check.await.is_err());
}

fn pending(original: Original, project: ProjectFence, cancel: CancellationToken) -> ContextCapture {
    ContextCapture {
        deadline: Instant::now() + TTL,
        original,
        project,
        fact: None,
        cancel: crate::automation_prepare::CancelOnDrop(cancel),
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registration_takes_exact_review_guard_before_active_without_deadlock() {
    let f = Fixture::new().await;
    let (owner, _u, original, project) = f.original().await;
    let runtime = owner.automation.clone();
    let worker_runtime = runtime.clone();
    let active = runtime.ai_active.lock().unwrap();
    let (entered, observed) = std::sync::mpsc::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let operation = Uuid::new_v4();
    let capture = Uuid::new_v4();
    let cancel = CancellationToken::new();
    let thread = std::thread::spawn(move || {
        register_capture(
            &worker_runtime,
            operation,
            capture,
            pending(original, project, cancel.clone()),
            cancel,
            move || {
                entered.send(()).unwrap();
                blocked.recv_timeout(Duration::from_secs(5)).unwrap();
            },
        )
    });
    // Under the old active-first order this causal boundary is unreachable
    // while main owns active, rather than merely being a scheduling race.
    observed.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(runtime.ai_reviews.try_lock().is_err());
    release.send(()).unwrap();
    drop(active);
    let flight = thread.join().unwrap().unwrap();
    assert!(runtime.ai_reviews.lock().unwrap().contains_key(&capture));
    assert!(runtime.ai_active.lock().unwrap().contains_key(&operation));
    drop(flight);
    assert!(runtime.ai_reviews.lock().unwrap().is_empty());
    assert!(runtime.ai_active.lock().unwrap().is_empty());
}
#[tokio::test]
async fn operation_and_native_capture_ids_cannot_alias_either_existing_map() {
    let f = Fixture::new().await;
    let (owner, _u, original, project) = f.original().await;
    let runtime = owner.automation.clone();
    let existing = Uuid::new_v4();
    let existing_cancel = CancellationToken::new();
    runtime.ai_reviews.lock().unwrap().insert(
        existing,
        AutomationAiPrepared::Context(pending(
            original.clone(),
            project.clone(),
            existing_cancel.clone(),
        )),
    );
    for (operation, capture) in [(existing, Uuid::new_v4()), (Uuid::new_v4(), existing)] {
        let cancel = CancellationToken::new();
        assert!(
            register_capture(
                &runtime,
                operation,
                capture,
                pending(original.clone(), project.clone(), cancel.clone()),
                cancel,
                || {}
            )
            .is_err()
        );
        assert!(runtime.ai_reviews.lock().unwrap().contains_key(&existing));
        assert!(!existing_cancel.is_cancelled());
    }
    let active = Uuid::new_v4();
    runtime
        .ai_active
        .lock()
        .unwrap()
        .insert(active, CancellationToken::new());
    for (operation, capture) in [(active, Uuid::new_v4()), (Uuid::new_v4(), active)] {
        let cancel = CancellationToken::new();
        assert!(
            register_capture(
                &runtime,
                operation,
                capture,
                pending(original.clone(), project.clone(), cancel.clone()),
                cancel,
                || {}
            )
            .is_err()
        );
        assert!(runtime.ai_active.lock().unwrap().contains_key(&active));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn original_project_name_replacement_with_identical_bytes_cannot_reuse_captured_pins() {
    let f = Fixture::new().await;
    let (owner, u, _, project) = f.original().await;
    let context = f
        .app
        .automation_project_context_original(&f.project, owner.clone(), Arc::downgrade(&u))
        .await
        .unwrap();
    let path = context.path.clone();
    drop(context);
    // Managed fixtures begin with a plain, newly created Native directory.
    let pins = project.pins(&owner).await.unwrap();
    pins.check(&owner).await.unwrap();
    let moved = path.with_extension("original-held");
    std::fs::rename(&path, &moved).unwrap();
    std::fs::create_dir(&path).unwrap();
    for entry in std::fs::read_dir(&moved).unwrap() {
        let entry = entry.unwrap();
        assert!(entry.file_type().unwrap().is_file());
        std::fs::copy(entry.path(), path.join(entry.file_name())).unwrap();
    }
    let replacement = project.selection.open().unwrap();
    assert_eq!(replacement.revision().unwrap().digest, pins.digest);
    assert_eq!(pins.root.revision().unwrap().digest, pins.digest);
    pins.root.recheck().unwrap(); // Proves the old plain-handle check was insufficient.
    assert!(!pins.root.same_directory(&replacement).unwrap());
    assert!(pins.check(&owner).await.is_err());
}

#[tokio::test]
async fn captured_project_pins_reject_actual_content_edit_without_identity_change() {
    let f = Fixture::new().await;
    let (owner, u, _, project) = f.original().await;
    let context = f
        .app
        .automation_project_context_original(&f.project, owner.clone(), Arc::downgrade(&u))
        .await
        .unwrap();
    let path = context.path.clone();
    drop(context);
    let pins = project.pins(&owner).await.unwrap();
    pins.check(&owner).await.unwrap();
    std::fs::write(
        path.join("context-current-content.yml"),
        b"---\n- hosts: all\n  gather_facts: false\n",
    )
    .unwrap();
    let current = project.selection.open().unwrap();
    assert!(pins.root.same_directory(&current).unwrap());
    assert_ne!(current.revision().unwrap().digest, pins.digest);
    assert!(pins.check(&owner).await.is_err());
}
