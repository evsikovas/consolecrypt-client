use super::*;
use crate::{AccountMode, AppConfig};
use cc_sync_core::mock::{FaultAction, MockServer};
use serde_json::json;
const PASS: &str = "owned diagnostics passphrase42";
fn app(path: &std::path::Path) -> AppCore {
    let mut config = AppConfig::for_tests(path.to_string_lossy().into_owned());
    config.background_sync = false;
    AppCore::new(config).unwrap()
}
async fn synced(app: &AppCore, server: &MockServer) {
    app.create_synced_profile(
        "Owned diagnostics".into(),
        server.url().to_string(),
        format!("{}@example.test", Uuid::new_v4().simple()),
        "owned account password42".into(),
        AccountMode::Register,
    )
    .await
    .unwrap();
    app.create_vault(PASS.into()).await.unwrap();
}
async fn input(app: &AppCore) -> String {
    let value = app.monitoring_sources().await.unwrap().panel_sync.unwrap();
    serde_json::to_string(&json!({"version":1,"profileId":value["profileId"],"notificationScopeId":value["notificationScopeId"]})).unwrap()
}
#[test]
fn old_server_timeout_and_unavailable_have_distinct_fixed_observations() {
    assert_eq!(
        observation(MonitoringCompatibilityError::Unsupported),
        ServerObservation::Unsupported
    );
    assert_eq!(
        observation(MonitoringCompatibilityError::Timeout),
        ServerObservation::TimedOut
    );
    assert_eq!(
        observation(MonitoringCompatibilityError::Unavailable),
        ServerObservation::Unavailable
    );
    assert!(request(r#"{"version":1,"profileId":"foreign","notificationScopeId":"x"}"#).is_err());
}
#[tokio::test]
async fn local_catalog_has_no_remote_observation_and_local_crud_is_not_disabled() {
    let temp = tempfile::tempdir().unwrap();
    let app = app(temp.path());
    app.create_local_profile("Owned local".into(), PASS.into())
        .await
        .unwrap();
    let catalog = app.monitoring_sources().await.unwrap();
    let value = catalog.panel_sync.unwrap();
    assert_eq!(value["storage"], "local");
    assert_eq!(value["readiness"]["state"], "localOnly");
    assert_eq!(value["server"], "notChecked");
    assert!(value["account"].is_null());
    assert!(!catalog.read_only);
    let raw = app
        .monitoring_settings_sync_check_json(&input(&app).await)
        .await
        .unwrap();
    let checked: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(checked["readiness"]["state"], "localOnly");
    assert_eq!(checked["server"], "notChecked");
    assert_eq!(checked.as_object().unwrap().len(), 8);
}
#[tokio::test]
async fn old_server_get_is_explicit_and_unauthorized_or_stale_proof_are_never_retried() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    let app = app(temp.path());
    synced(&app, &server).await;
    let vault = app.unlocked().await.unwrap().0.require_vault_id().unwrap();
    let path = format!("/v1/vaults/{vault}/compatibility/monitoring-settings");
    let raw = input(&app).await;
    assert_eq!(server.hits(&path), 0, "catalog must not perform HTTP");
    let checked: Value =
        serde_json::from_str(&app.monitoring_settings_sync_check_json(&raw).await.unwrap())
            .unwrap();
    assert_eq!(checked["server"], "unsupported");
    assert_eq!(checked["readiness"]["state"], "serverAdmissionUnverified");
    assert_eq!(checked["releasedMinimumClientVersion"], "1.2.0");
    assert_eq!(server.hits(&path), 1);
    for (status, body, expected) in [
        (
            401,
            json!({"code":"unauthorized","message":"PRIVATE"}),
            "authenticationRequired",
        ),
        (
            422,
            json!({"code":"invalid_proof","message":"PRIVATE","details":{"reason":"stale"}}),
            "proofRequired",
        ),
    ] {
        let before = server.hits(&path);
        let refreshes = server.hits("/v1/auth/refresh");
        server.inject(&path, 1, FaultAction::RawResponse { status, body });
        let checked = app.monitoring_settings_sync_check_json(&raw).await.unwrap();
        assert!(!checked.contains("PRIVATE"));
        let checked: Value = serde_json::from_str(&checked).unwrap();
        assert_eq!(checked["server"], expected);
        assert_eq!(server.hits(&path), before + 1);
        assert_eq!(server.hits("/v1/auth/refresh"), refreshes);
    }
}
#[tokio::test]
async fn original_nonce_cannot_adopt_reunlock_or_different_local_profile_before_network() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    let app = app(temp.path());
    synced(&app, &server).await;
    let raw = input(&app).await;
    let vault = app.unlocked().await.unwrap().0.require_vault_id().unwrap();
    let path = format!("/v1/vaults/{vault}/compatibility/monitoring-settings");
    app.lock().await.unwrap();
    app.unlock_with_passphrase(PASS.into()).await.unwrap();
    assert!(app.monitoring_settings_sync_check_json(&raw).await.is_err());
    assert_eq!(server.hits(&path), 0);
    app.close_profile().await.unwrap();
    app.create_local_profile("Other local".into(), PASS.into())
        .await
        .unwrap();
    assert!(app.monitoring_settings_sync_check_json(&raw).await.is_err());
    assert_eq!(server.hits(&path), 0);
}
#[tokio::test]
async fn enabled_account_and_minimum_version_remain_observation_not_ready_or_writer() {
    let server = MockServer::start().await;
    let temp = tempfile::tempdir().unwrap();
    let app = app(temp.path());
    synced(&app, &server).await;
    let raw = input(&app).await;
    let context = Context::capture(&app).await.unwrap();
    let path = format!(
        "/v1/vaults/{}/compatibility/monitoring-settings",
        context.vault
    );
    server.inject(
        &path,
        1,
        FaultAction::RawResponse {
            status: 200,
            body: json!({"version":1,"scope":"personalAccount",
        "accountId":context.account,"revision":1,"state":"enabled","minimumClientVersion":"2.0.0",
        "offerMinimumClientVersion":null,"rosterDigest":"a".repeat(64),
        "checkpoint":{"vaultId":context.vault,"vaultEpoch":Uuid::new_v4(),"vaultSequence":0}}),
        },
    );
    let checked: Value =
        serde_json::from_str(&app.monitoring_settings_sync_check_json(&raw).await.unwrap())
            .unwrap();
    assert_eq!(checked["account"]["state"], "enabled");
    assert_eq!(checked["account"]["minimumClientVersion"], "2.0.0");
    assert_eq!(checked["readiness"]["state"], "serverAdmissionUnverified");
    assert_eq!(checked["releasedMinimumClientVersion"], "1.2.0");
    assert!(checked.get("ready").is_none());
    let catalog = app
        .monitoring_source_create(
            0,
            cc_monitoring_core::SourceConfig {
                id: String::new(),
                name: "Owned".into(),
                endpoint: "https://fixture.invalid/prometheus".into(),
                route: cc_monitoring_core::Route::Direct,
                tls: cc_monitoring_core::TlsConfig::default(),
                auth: cc_monitoring_core::AuthRef::None,
            },
        )
        .await
        .unwrap();
    let source = &catalog.sources[0];
    let definition = crate::monitoring_sources::MonitoringPanelDefinition {
        title: "Owned".into(),
        source_id: source.id.clone(),
        source_revision: source.revision.unwrap(),
        query: "up".into(),
        unit_hint: crate::monitoring_sources::MonitoringUnitHint::Unknown,
    };
    assert!(
        app.monitoring_panel_create(catalog.revision.unwrap(), definition)
            .await
            .is_err()
    );
}
