use super::*;
use cc_platform_core::{InMemorySecureStore, UnsupportedDeviceKeys};
const PASSWORD: &str = "Agent wizard original context fixture passphrase42";

async fn fixture() -> (tempfile::TempDir, AppCore, Value, crate::HostDto) {
    let root = tempfile::tempdir().unwrap();
    let app = AppCore::with_platform(
        crate::AppConfig::for_tests(root.path().to_string_lossy().into_owned()),
        Arc::new(InMemorySecureStore::new()),
        Arc::new(UnsupportedDeviceKeys),
    )
    .unwrap();
    app.create_local_profile("Agent setup".into(), PASSWORD.into())
        .await
        .unwrap();
    let mut events = app.subscribe_events();
    let mut host = crate::HostDto::new("Original Agent target", "127.0.0.1");
    host.host_key_policy = cc_ssh_core::HostKeyPolicy::Strict;
    let host = app.save_host(host).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let crate::AppEvent::ObjectsChanged {
                kind: cc_models::ObjectKind::Host,
                ids,
                ..
            } = events.recv().await.unwrap()
            {
                if ids.contains(&host.id) {
                    break;
                }
            }
        }
    })
    .await
    .unwrap();
    let (owner, u) = app.unlocked().await.unwrap();
    let frame = json!({"version":1,"profileId":owner.profile_id.to_string(),
        "notificationScopeId":u.notices.scope_id().unwrap()});
    drop(u);
    drop(owner);
    (root, app, frame, host)
}
async fn request(app: &AppCore, frame: &Value, host: &crate::HostDto) -> Value {
    let revision = app
        .unlocked()
        .await
        .unwrap()
        .1
        .working()
        .revision_of(host_id(&host.id).unwrap())
        .unwrap();
    let mut request = frame.clone();
    request["hostId"] = json!(host.id);
    request["expectedHostRevision"] = json!(revision.to_string());
    request["monitoringProfileId"] = json!(PROFILE);
    request
}

#[test]
fn caller_cannot_supply_release_key_command_or_authority() {
    let frame = json!({"version":1,"profileId":"p","notificationScopeId":"n"});
    assert!(parse::<Frame>(&frame.to_string()).is_ok());
    for field in ["approved", "releaseKey", "command", "installed", "ownerPin"] {
        let mut bad = frame.clone();
        bad[field] = json!(true);
        assert!(parse::<Frame>(&bad.to_string()).is_err());
    }
    for raw in ["0", "01", "+1", "-1", "1.0", "9223372036854775808"] {
        assert!(revision(raw).is_err());
    }
}
#[test]
fn absent_authenticated_catalog_never_produces_install_or_readiness_claim() {
    let host = HostRef {
        id: Uuid::new_v4().to_string(),
        revision: "1".into(),
        name: "target".into(),
        address: "127.0.0.1".into(),
    };
    let plan = plan(Uuid::new_v4(), &host, None);
    assert_eq!(plan["blocker"], "releaseUnavailable");
    assert_eq!(plan["canProceed"], false);
    assert_eq!(plan["observations"]["software"], "notObserved");
    assert_eq!(plan["observations"]["collectorRuntime"], "notObserved");
    assert!(plan["release"].is_null());
    assert!(plan["actions"].as_array().unwrap().is_empty());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn context_and_prepare_are_read_only_and_install_is_closed() {
    let (_root, app, frame, host) = fixture().await;
    let before = app.unlocked().await.unwrap().1.working().version();
    let context: Value = serde_json::from_str(
        &app.agent_setup_context_json(&frame.to_string())
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(context["hosts"][0]["id"], host.id);
    assert!(context["releases"].as_array().unwrap().is_empty());
    assert!(context["destinations"].as_array().unwrap().is_empty());
    let operation = app
        .agent_setup_prepare_json(&request(&app, &frame, &host).await.to_string())
        .await
        .unwrap();
    let first = operation.review_json().await.unwrap();
    assert_eq!(first, operation.review_json().await.unwrap());
    assert!(operation.install_software_json().await.is_err());
    assert_eq!(before, app.unlocked().await.unwrap().1.working().version());
    operation.cancel();
    assert!(operation.review_json().await.is_err());
    let cleanup: Value = serde_json::from_str(&operation.joined_json().await.unwrap()).unwrap();
    assert_eq!(cleanup["closedObserved"], true);
    assert_eq!(cleanup["remoteMutationAttempted"], false);
    app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn relock_cannot_refresh_original_operation_or_frame() {
    let (_root, app, frame, host) = fixture().await;
    let selection = request(&app, &frame, &host).await;
    let operation = app
        .agent_setup_prepare_json(&selection.to_string())
        .await
        .unwrap();
    app.lock().await.unwrap();
    app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    assert!(operation.review_json().await.is_err());
    assert!(operation.install_software_json().await.is_err());
    assert!(
        app.agent_setup_prepare_json(&selection.to_string())
            .await
            .is_err()
    );
    app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changed_host_and_caller_query_source_cannot_retarget_frozen_plan() {
    let (_root, app, frame, mut host) = fixture().await;
    let mut selection = request(&app, &frame, &host).await;
    selection["destinationRef"] = json!(Uuid::new_v4());
    assert!(
        app.agent_setup_prepare_json(&selection.to_string())
            .await
            .is_err()
    );
    selection.as_object_mut().unwrap().remove("destinationRef");
    let operation = app
        .agent_setup_prepare_json(&selection.to_string())
        .await
        .unwrap();
    host.address = "127.0.0.2".into();
    app.save_host(host).await.unwrap();
    assert!(operation.review_json().await.is_err());
    assert!(operation.install_software_json().await.is_err());
    app.shutdown().await.unwrap();
}
