use super::*;
use crate::{HostDto, automation_runners::AutomationRunnerConfig};
fn request(action: Value) -> String {
    json!({"version":1,"action":action}).to_string()
}
fn proposal() -> Value {
    json!({"kind":"role","name":"demo","version":"1.2.3","url":"https://fixtures.invalid/artifact.tar.gz","archiveSha256":"a".repeat(64)})
}
#[test]
fn wire_schema_has_no_generic_operations_privileges_credentials_or_approval_flags() {
    for raw in ["{}".into(),"null".into(),"{\"version\":1,\"version\":1,\"action\":{\"type\":\"status\",\"operationId\":\"x\"}}".into(),"{\"version\":1,\"action\":{\"type\":\"status\",\"operationId\":\"x\",\"operationId\":\"y\"}}".into(),request(json!({"type":"request","operation":"exec","body":{}})),request(json!({"type":"start","reviewId":"x","url":"https://not-captured.invalid/"})),request(json!({"type":"accept","operationId":"x","reviewDigest":"a".repeat(64),"approved":true})),"x".repeat(INPUT+1)] {
        assert!(parse(&raw).is_err());
    }
    let valid = request(json!({"type":"status","operationId":Uuid::new_v4()}));
    assert!(parse(&valid).is_ok());
    let future = valid.replacen("\"version\":1", "\"version\":2", 1);
    assert!(parse(&future).is_err());
}
#[test]
fn source_is_exact_pin_proposal_and_uses_existing_policy_not_an_authority_tag() {
    for (field, value) in [
        ("url", json!("http://fixtures.invalid/a")),
        ("url", json!("https://user:password@fixtures.invalid/a")),
        ("url", json!("https://fixtures.invalid/a?token=sensitive")),
        ("url", json!("https://fixtures.invalid/a#fragment")),
        ("version", json!("latest")),
        ("archiveSha256", json!("not-a-hash")),
        ("name", json!("../outside")),
    ] {
        let mut source = proposal();
        source[field] = value;
        let typed: Proposal = serde_json::from_value(source).unwrap();
        assert!(typed.selected().is_err());
    }
    for extra in [
        "credentialId",
        "command",
        "trusted",
        "archiveVerified",
        "stripComponents",
    ] {
        let mut source = proposal();
        source[extra] = json!(true);
        assert!(serde_json::from_value::<Proposal>(source).is_err());
    }
    assert!(serde_json::from_value::<Proposal>(proposal())
        .unwrap()
        .selected()
        .is_ok());
}
#[test]
fn output_and_fixed_error_bounds_do_not_echo_backend_material() {
    assert!(output("test", json!({"text":"x".repeat(OUTPUT)})).is_err());
    let fixed = fixed(AppError::invalid(
        "some_backend",
        "SYNTHETIC_BACKEND_SECRET_MARKER",
    ));
    assert!(!format!("{fixed}").contains("SYNTHETIC_BACKEND_SECRET_MARKER"));
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_profile_review_is_network_free_and_stale_start_is_one_use_before_transport() {
    let _serial = super::super::tests::serial().lock().await;
    let directory = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        directory.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile(
        "Owned facade fixture".into(),
        "synthetic profile password 42".into(),
    )
    .await
    .unwrap();
    let host = app
        .save_host(HostDto::new("Uncontacted fixture metadata", "127.0.0.1"))
        .await
        .unwrap();
    let runner = app
        .automation_runner_create(
            "Uncontacted prepared Runner".into(),
            AutomationRunnerConfig::Remote {
                host_id: host.id,
                helper: cc_automation_core::RemoteHelperConfig {
                    python: "/home/runner/owned/python".into(),
                    module_dir: "/home/runner/owned/src".into(),
                    root: "/home/runner/owned/jobs".into(),
                },
            },
        )
        .await
        .unwrap();
    let project = app
        .automation_project_create("Own facade project".into(), None)
        .await
        .unwrap();
    let raw = request(
        json!({"type":"review","projectId":project.id,"projectRevision":1,"runnerId":runner.id,"runnerRevision":1,"dependency":proposal()}),
    );
    let context = app.automation_project_context(&project.id).await.unwrap();
    let receipt = context.path.join(".consolecrypt-dependency-receipts.json");
    let receipt_bytes = b"{\"version\":1,\"receipts\":[]}";
    std::fs::write(&receipt, receipt_bytes).unwrap();
    let error = app
        .automation_dependency_action_json(raw.clone())
        .await
        .unwrap_err();
    assert!(
        matches!(error,AppError::InvalidInput{reason,..}if reason=="dependency_existing_receipt_unsupported")
    );
    assert_eq!(std::fs::read(&receipt).unwrap(), receipt_bytes);
    std::fs::remove_file(receipt).unwrap(); // exact fixture file only
    let view: Value =
        serde_json::from_str(&app.automation_dependency_action_json(raw).await.unwrap()).unwrap();
    assert_eq!(view["kind"], "sourceReview");
    assert_eq!(view["data"]["oldLockSha256"], Value::Null);
    assert_eq!(view["data"]["destination"], "roles/demo");
    assert_eq!(view["data"]["networkOrigin"],"https://fixtures.invalid");
    let actual_root=cc_automation_core::ProjectRoot::open(&context.path).unwrap();
    let actual_available=cc_automation_core::check_dependency_publication_base(
        context.owner.dir.as_deref().unwrap(),Uuid::parse_str(&project.id).unwrap(),&actual_root,
    ).is_ok();
    assert_eq!(view["data"]["publicationSupported"],actual_available);
    if actual_available { assert_eq!(view["data"]["publicationReason"],Value::Null); }
    else { assert!(["publication_location_unavailable","excluded_project_entries","publication_platform_unsupported"].contains(&view["data"]["publicationReason"].as_str().unwrap())); }
    let review = view["data"]["reviewId"].as_str().unwrap().to_owned();
    let op = view["data"]["operationId"].as_str().unwrap().to_owned();
    // SQL-only project revision changes before explicit start. An accidental
    // fetch/SSH open cannot pass (no endpoint/server exists in this fixture).
    app.automation_project_rename(project.id, 1, "Metadata changed before network".into())
        .await
        .unwrap();
    let first = app
        .automation_dependency_action_json(request(json!({"type":"start","reviewId":review})))
        .await
        .unwrap_err();
    assert!(
        matches!(first,AppError::InvalidInput{reason,..}if reason=="dependency_context_changed")
    );
    let second = app
        .automation_dependency_action_json(request(json!({"type":"start","reviewId":review})))
        .await
        .unwrap_err();
    assert!(
        matches!(second,AppError::InvalidInput{reason,..}if reason=="dependency_review_expired")
    );
    assert!(app
        .automation_dependency_action_json(request(json!({"type":"status","operationId":op})))
        .await
        .is_err());
    assert!(app.automation_jobs().await.unwrap()["jobs"]
        .as_array()
        .unwrap()
        .is_empty());
    app.lock().await.unwrap();
    assert!(app
        .automation_dependency_action_json(request(json!({"type":"status","operationId":op})))
        .await
        .is_err());
    app.shutdown().await.unwrap();
}
#[tokio::test]
async fn facade_dispatch_future_fits_desktop_thread_budget() {
    let directory = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        directory.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    let future = app.automation_dependency_action_json(request(
        json!({"type":"status","operationId":Uuid::new_v4()}),
    ));
    assert!(std::mem::size_of_val(&future) <= 64 * 1024);
}


#[test]
fn publish_frame_never_accepts_url_command_source_or_serialized_acceptance() {
    let action=json!({"type":"publish","operationId":Uuid::new_v4(),"reviewDigest":"a".repeat(64)});
    assert!(matches!(parse(&request(action.clone())).unwrap().action,Action::Publish{..}));
    for name in ["url","source","command","approved","retainedCandidate","publicationSupported","credentialId"] {
        let mut extra=action.clone();extra[name]=json!(true);
        assert!(parse(&request(extra)).is_err());
    }
    for raw in [
        r#"{"version":1,"action":{"type":"publish","operationId":"x","reviewDigest":"a","reviewDigest":"b"}}"#,
        r#"{"version":1,"action":{"type":"publish","operationId":"x","reviewDigest":"a","review\u0044igest":"b"}}"#,
    ] { assert!(parse(raw).is_err()); }
}

#[test]
fn publication_uncertainty_is_fixed_recovery_required_never_generic_retry() {
    for code in ["dependency_publication_unsupported","dependency_publication_busy","dependency_publication_recovery_required"] {
        let error=fixed(invalid(code));
        assert!(matches!(error,AppError::InvalidInput{field,reason}if field=="automation_dependency" && reason==code));
    }
    let error=fixed(invalid("SYNTHETIC_UNTRUSTED_BACKEND_DETAILS"));
    assert!(matches!(error,AppError::InvalidInput{reason,..}if reason=="dependency_evidence_unavailable"));
}

#[tokio::test]
async fn publish_without_actual_retained_acceptance_cannot_select_or_start_helper() {
    let _serial=super::super::tests::serial().lock().await;
    let f=super::super::tests::Fixture::new().await;
    let (_registry,ticket)=f.ticket().await;
    let before=ticket.context.base.revision().unwrap();
    let sql:Value=ticket.context.project.owner.storage().unwrap().setting_get(PROJECTS).await.unwrap().unwrap();
    let error=f.app.automation_dependency_action_json(request(json!({
        "type":"publish","operationId":ticket.operation,"reviewDigest":"a".repeat(64)
    }))).await.unwrap_err();
    assert!(matches!(error,AppError::InvalidInput{reason,..}if reason=="dependency_operation_unavailable"));
    assert_eq!(ticket.context.base.revision().unwrap(),before);
    assert_eq!(ticket.context.project.owner.storage().unwrap().setting_get::<Value>(PROJECTS).await.unwrap().unwrap(),sql);
    assert_eq!(ticket.context.project.owner.automation.io_slots.available_permits(),4);
    assert!(f.app.automation_jobs().await.unwrap()["jobs"].as_array().unwrap().is_empty());
    f.app.shutdown().await.unwrap();
}
