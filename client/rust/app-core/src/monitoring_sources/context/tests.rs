use super::*;
use cc_host_mapping::Labels;
use cc_monitoring_core::{AuthRef, Route, SourceConfig, TlsConfig};
use serde_json::{Value, json};

const PASSWORD: &str = "owned correlation context fixture password 42";
const PRIVATE: &str = "never-project-host-context-route-or-secret";

async fn setup() -> (tempfile::TempDir, AppCore, Value) {
    let directory = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        directory.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("Correlation fixture".into(), PASSWORD.into())
        .await
        .unwrap();
    let host = app
        .save_host(crate::HostDto::new(
            "Owned host",
            &format!("{PRIVATE}.example.invalid"),
        ))
        .await
        .unwrap();
    let source = app
        .monitoring_source_create(
            0,
            SourceConfig {
                id: String::new(),
                name: PRIVATE.into(),
                endpoint: format!("https://{PRIVATE}.example.invalid/prometheus"),
                route: Route::Direct,
                tls: TlsConfig::default(),
                auth: AuthRef::None,
            },
        )
        .await
        .unwrap();
    let source = source.sources[0].id.clone();
    let bindings = app
        .monitoring_binding_create(
            1,
            "Explicit context".into(),
            host.id.clone(),
            source.clone(),
            1,
            Labels::from([("instance".into(), PRIVATE.into())]),
        )
        .await
        .unwrap();
    let (owner, _) = app.unlocked().await.unwrap();
    let request = json!({"kind":"capture","version":1,"profileId":owner.profile_id.to_string(),
        "hostId":host.id,"sourceId":source,"sourceRevision":1,"bindingId":bindings.bindings[0].id,"bindingRevision":1});
    (directory, app, request)
}
async fn call(app: &AppCore, request: &Value) -> AppResult<Value> {
    Ok(serde_json::from_str(
        &app.monitoring_host_context_json(&request.to_string())
            .await?,
    )
    .unwrap())
}
fn recheck(capture: &Value, view: &Value) -> Value {
    let mut request = capture.clone();
    request["kind"] = json!("recheck");
    request["notificationScopeId"] = view["notificationScopeId"].clone();
    request["hostRevision"] = view["hostRevision"].clone();
    request
}

#[test]
fn context_request_strict_bounds_versions_and_duplicate_fields() {
    let value = json!({"kind":"capture","version":1,"profileId":Uuid::new_v4(),"hostId":Uuid::new_v4(),
        "sourceId":Uuid::new_v4(),"sourceRevision":1,"bindingId":Uuid::new_v4(),"bindingRevision":1});
    assert!(request(&value.to_string()).is_ok());
    for key in [
        "url",
        "endpoint",
        "selector",
        "credentialRef",
        "notificationScopeId",
        "hostRevision",
        "approved",
    ] {
        let mut bad = value.clone();
        bad[key] = json!(true);
        assert!(request(&bad.to_string()).is_err(), "{key}");
    }
    let raw = value.to_string();
    let duplicate = format!("{{\"profileId\":\"{}\",{}", Uuid::new_v4(), &raw[1..]);
    assert!(request(&duplicate).is_err());
    assert!(request(&" ".repeat(MAX_REQUEST + 1)).is_err());
    for version in [json!(2), json!(1.0), Value::Null] {
        let mut bad = value.clone();
        bad["version"] = version;
        assert!(request(&bad.to_string()).is_err());
    }
    let mut missing = value.clone();
    missing["kind"] = json!("recheck");
    assert!(request(&missing.to_string()).is_err());
    let mut noncanonical = value;
    noncanonical["hostId"] = json!(Uuid::new_v4().simple().to_string());
    assert!(request(&noncanonical.to_string()).is_err());
}

#[tokio::test]
async fn metadata_capture_is_exact_bounded_and_contains_no_object_material() {
    let (_directory, app, input) = setup().await;
    let view = call(&app, &input).await.unwrap();
    assert_eq!(view.as_object().unwrap().len(), 10);
    for key in [
        "profileId",
        "hostId",
        "sourceId",
        "sourceRevision",
        "bindingId",
        "bindingRevision",
    ] {
        assert_eq!(view[key], input[key]);
    }
    assert_eq!(view["protocol"], "ssh");
    assert!(view["hostRevision"].as_u64().unwrap() > 0);
    assert!(!view.to_string().contains(PRIVATE));
    assert!(view.to_string().len() < 1024);
    assert_eq!(call(&app, &recheck(&input, &view)).await.unwrap(), view);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn foreign_profile_host_and_nonce_cannot_retarget_metadata() {
    let (_directory, app, input) = setup().await;
    let view = call(&app, &input).await.unwrap();
    for key in ["profileId", "hostId", "sourceId", "bindingId"] {
        let mut changed = input.clone();
        changed[key] = json!(Uuid::new_v4());
        assert!(call(&app, &changed).await.is_err(), "{key}");
    }
    let mut changed = recheck(&input, &view);
    changed["notificationScopeId"] = json!(Uuid::new_v4());
    assert!(call(&app, &changed).await.is_err());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn actual_host_revision_edit_requires_fresh_capture_and_explicit_recheck() {
    let (_directory, app, input) = setup().await;
    let before = call(&app, &input).await.unwrap();
    let mut host = app.list_hosts().await.unwrap().remove(0);
    host.name = "Actual host metadata edit".into();
    app.save_host(host).await.unwrap();
    assert!(call(&app, &recheck(&input, &before)).await.is_err());
    let after = call(&app, &input).await.unwrap();
    assert!(after["hostRevision"].as_u64().unwrap() > before["hostRevision"].as_u64().unwrap());
    assert_eq!(call(&app, &recheck(&input, &after)).await.unwrap(), after);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn source_edit_never_silently_rereviews_existing_binding() {
    let (_directory, app, input) = setup().await;
    let mut source = app.monitoring_sources().await.unwrap().sources[0]
        .configuration
        .clone()
        .unwrap();
    source.name = "Explicit source metadata edit".into();
    app.monitoring_source_update(input["sourceId"].as_str().unwrap().into(), 2, 1, source)
        .await
        .unwrap();
    assert!(call(&app, &input).await.is_err());
    let mut newer = input;
    newer["sourceRevision"] = json!(2);
    assert!(call(&app, &newer).await.is_err());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn binding_revision_edit_invalidates_original_context() {
    let (_directory, app, input) = setup().await;
    let before = call(&app, &input).await.unwrap();
    app.monitoring_binding_update(
        input["bindingId"].as_str().unwrap().into(),
        2,
        1,
        "Explicit rereview".into(),
        input["hostId"].as_str().unwrap().into(),
        input["sourceId"].as_str().unwrap().into(),
        1,
        Labels::from([("instance".into(), "changed".into())]),
    )
    .await
    .unwrap();
    assert!(call(&app, &recheck(&input, &before)).await.is_err());
    let mut new_input = input;
    new_input["bindingRevision"] = json!(2);
    assert!(call(&app, &new_input).await.is_ok());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn same_profile_close_reopen_changes_native_nonce_and_denies_old_initial_reply() {
    let (_directory, app, input) = setup().await;
    let old_initial = call(&app, &input).await.unwrap();
    app.close_profile().await.unwrap();
    app.open_profile(input["profileId"].as_str().unwrap().into())
        .await
        .unwrap();
    app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    let current = call(&app, &input).await.unwrap();
    assert_eq!(current["profileId"], old_initial["profileId"]);
    assert_ne!(
        current["notificationScopeId"],
        old_initial["notificationScopeId"]
    );
    assert!(call(&app, &recheck(&input, &old_initial)).await.is_err());
    assert!(call(&app, &recheck(&input, &current)).await.is_ok());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn lock_and_deleted_host_are_unavailable_without_implicit_repair() {
    let (_directory, app, input) = setup().await;
    let before = call(&app, &input).await.unwrap();
    app.lock().await.unwrap();
    assert!(call(&app, &input).await.is_err());
    app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    assert!(call(&app, &recheck(&input, &before)).await.is_err());
    app.delete_host(input["hostId"].as_str().unwrap().into())
        .await
        .unwrap();
    assert!(call(&app, &input).await.is_err());
    app.shutdown().await.unwrap();
}
