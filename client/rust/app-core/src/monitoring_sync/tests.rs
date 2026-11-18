use super::*;
use crate::{AccountMode, AppConfig, AppCore, VaultStateDto};
use cc_models::VaultObject;
use cc_monitoring_core::{AuthRef, Route, SourceConfig, TlsConfig};
use cc_protocol::{bytes::Bytes, version::Platform, ObjectId, VaultId};
use cc_sync_core::mock::{FaultAction, MockServer, MockServerConfig};
use chrono::{TimeZone, Utc};
use serde_json::{json, Value};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
const PASS: &str = "owned monitoring sync passphrase 42";
const ACCOUNT: &str = "owned account password for fixtures 42";
const QUERY: &str = "sum(rate(owned_encrypted_panel_private_marker_total{job=\"explicit\"}[5m]))";
fn id(n: u128) -> DeviceId {
    DeviceId::from_str(&uuid::Uuid::from_u128(n).to_string()).unwrap()
}
fn device(n: u128, version: Option<&str>) -> DeviceInfo {
    DeviceInfo {
        device_id: id(n),
        name: "Synthetic owned device".into(),
        platform: Platform::Cli,
        encryption_public_key: Bytes::new(vec![1; 32]),
        signing_public_key: Bytes::new(vec![2; 32]),
        status: DeviceStatus::Active,
        trusted_vaults: vec![],
        created_at: Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
        last_seen_at: None,
        revoked_at: None,
        is_current: n == 1,
        trusted: false,
        quarantine_until: None,
        client_version: version.map(str::to_owned),
    }
}
fn floor() -> ReleasedFloor {
    ReleasedFloor::parse("2.0.0").unwrap()
} // SYNTHETIC; not a shipped floor.
fn app(path: &std::path::Path, version: &str) -> AppCore {
    let mut config = AppConfig::for_tests(path.to_string_lossy().into_owned());
    config.client_version = version.into();
    config.background_sync = false;
    AppCore::new(config).unwrap()
}
async fn register(app: &AppCore, server: &MockServer, email: &str) -> String {
    app.create_synced_profile(
        "Owned compatibility fixture".into(),
        server.url().to_string(),
        email.into(),
        ACCOUNT.into(),
        AccountMode::Register,
    )
    .await
    .unwrap();
    app.create_vault(PASS.into())
        .await
        .unwrap()
        .vault_id
        .clone()
}
async fn login(app: &AppCore, server: &MockServer, email: &str, vault: &str) {
    let joined = app
        .create_synced_profile(
            "Owned second fixture".into(),
            server.url().to_string(),
            email.into(),
            ACCOUNT.into(),
            AccountMode::Login,
        )
        .await
        .unwrap();
    assert_eq!(joined.profile.vault_state, VaultStateDto::NoVault);
    app.join_vault_with_passphrase(vault.into(), PASS.into())
        .await
        .unwrap();
    app.sync_now().await.unwrap();
}
async fn raw(app: &AppCore) -> Value {
    let (_, u) = app.unlocked().await.unwrap();
    u.working()
        .all_vault_settings()
        .into_iter()
        .max_by_key(|s| s.updated_at)
        .unwrap()
        .monitoring
        .unwrap()
}
async fn install_raw(app: &AppCore, value: Value) {
    let (owner, u) = app.unlocked().await.unwrap();
    let _profile = app.hold_session(&owner).await.unwrap();
    let _unlock = owner.hold_unlocked(&u).await.unwrap();
    let mut settings = u
        .working()
        .all_vault_settings()
        .into_iter()
        .max_by_key(|s| s.updated_at)
        .unwrap();
    settings.monitoring = Some(value);
    u.writer
        .put(VaultObject::VaultSettings(settings))
        .await
        .unwrap();
}
fn source() -> SourceConfig {
    SourceConfig {
        id: String::new(),
        name: "Owned encrypted source".into(),
        endpoint: "https://fixture.invalid/prometheus".into(),
        route: Route::Direct,
        tls: TlsConfig::default(),
        auth: AuthRef::None,
    }
}
fn definition(source: &str, revision: u64) -> crate::monitoring_sources::MonitoringPanelDefinition {
    crate::monitoring_sources::MonitoringPanelDefinition {
        title: "Owned encrypted panel".into(),
        source_id: source.into(),
        source_revision: revision,
        query: QUERY.into(),
        unit_hint: crate::monitoring_sources::MonitoringUnitHint::Unknown,
    }
}
fn ciphertext_only(server: &MockServer, vault: &str, settings: &str, marker: &str) {
    let object = server
        .object(
            VaultId::from_str(vault).unwrap(),
            ObjectId::from_str(settings).unwrap(),
        )
        .unwrap();
    let body = object.body.unwrap();
    assert!(!object.deleted);
    assert_eq!(body.format, 1);
    assert_eq!(body.nonce.len(), 24);
    assert_eq!(body.wrapped_dek.len(), 48);
    assert!(body.ciphertext.len() > marker.len());
    assert!(!body
        .ciphertext
        .as_slice()
        .windows(marker.len())
        .any(|part| part == marker.as_bytes()));
    let wire = serde_json::to_value(body).unwrap();
    assert_eq!(wire.as_object().unwrap().len(), 5);
    assert!(wire.get("monitoring").is_none());
    assert!(wire.get("query").is_none());
}
#[test]
fn unknown_old_malformed_prerelease_and_dormant_active_devices_never_disappear() {
    for (version, reason) in [
        (None, DeviceCompatibilityReason::UnknownVersion),
        (Some("1.9.9"), DeviceCompatibilityReason::OlderVersion),
        (
            Some("not-a-release"),
            DeviceCompatibilityReason::InvalidVersion,
        ),
        (
            Some("3.0.0-beta.1"),
            DeviceCompatibilityReason::PrereleaseVersion,
        ),
        (Some(" 2.0.0 "), DeviceCompatibilityReason::InvalidVersion),
    ] {
        let mut old = device(2, version);
        old.last_seen_at = Some(Utc.timestamp_opt(1, 0).unwrap());
        let view = evaluate(&[device(1, Some("2.0.0")), old], id(1), &floor());
        assert_eq!(view.state, MonitoringSyncState::DevicesUnsupported);
        assert_eq!(view.checked_active_devices, 2);
        assert_eq!(
            view.blocked_devices,
            vec![DeviceCompatibilityBlocker {
                device_id: id(2),
                reason
            }]
        );
    }
}
#[test]
fn only_actual_revocation_excludes_an_old_device_and_all_compatible_is_not_an_admission_grant() {
    let mut revoked = device(2, None);
    revoked.status = DeviceStatus::Revoked;
    let view = evaluate(
        &[device(1, Some("2.0.0+synthetic")), revoked],
        id(1),
        &floor(),
    );
    assert_eq!(view.state, MonitoringSyncState::ServerAdmissionUnverified);
    assert_eq!(view.checked_active_devices, 1);
    assert!(view.blocked_devices.is_empty());
    assert_eq!(RELEASED_PRESERVATION_FLOOR, Some("1.2.0"));
    for invalid in [
        "0.9",
        "2.0.0-beta",
        "2.0.0+preview",
        " 2.0.0",
        "999999999999999999999.0.0",
    ] {
        assert!(ReleasedFloor::parse(invalid).is_none());
    }
}
#[test]
fn roster_identity_duplicates_missing_current_and_quotas_fail_closed_without_partial_snapshot() {
    for rows in [
        vec![],
        vec![device(2, Some("2.0.0"))],
        vec![device(1, Some("2.0.0")), device(1, None)],
        (1..=257).map(|n| device(n, Some("2.0.0"))).collect(),
        {
            let mut row = device(1, Some("2.0.0"));
            row.status = DeviceStatus::Revoked;
            vec![row]
        },
        vec![device(1, Some("2.0.0")), {
            let mut row = device(2, None);
            row.is_current = true;
            row
        }],
    ] {
        let view = evaluate(&rows, id(1), &floor());
        assert_eq!(view.state, MonitoringSyncState::RosterInvalid);
        assert!(view.blocked_devices.is_empty());
    }
    assert_eq!(
        evaluate(&[device(1, Some("2.0.0"))], id(99), &floor()).state,
        MonitoringSyncState::RosterInvalid
    );
}
#[test]
fn bounded_version_and_exact_maximum_roster_never_truncate_into_compatibility() {
    let mut oversized = device(2, Some("2.0.0"));
    oversized.client_version = Some("2.0.0+".to_owned() + &"b".repeat(64));
    let result = evaluate(&[device(1, Some("2.0.0")), oversized], id(1), &floor());
    assert_eq!(
        result.blocked_devices[0].reason,
        DeviceCompatibilityReason::InvalidVersion
    );
    let rows: Vec<_> = (1..=256).map(|n| device(n, Some("2.0.0"))).collect();
    let result = evaluate(&rows, id(1), &floor());
    assert_eq!(result.checked_active_devices, 256);
    assert_eq!(result.state, MonitoringSyncState::ServerAdmissionUnverified);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn declared_floor_does_not_grant_admission_and_reported_client_versions_travel(
) {
    let temp = tempfile::tempdir().unwrap();
    let local = app(&temp.path().join("local"), "2.0.0");
    local
        .create_local_profile("Owned local".into(), PASS.into())
        .await
        .unwrap();
    assert_eq!(
        local
            .monitoring_settings_sync_readiness()
            .await
            .unwrap()
            .state,
        MonitoringSyncState::LocalOnly
    );
    let server = MockServer::start_with(MockServerConfig {
        require_request_proof: true,
        ..Default::default()
    })
    .await;
    let a = app(&temp.path().join("a"), "2.0.0");
    let vault = register(&a, &server, "monitoring-compat@example.invalid").await;
    assert_eq!(
        a.monitoring_settings_sync_readiness().await.unwrap().state,
        MonitoringSyncState::ServerAdmissionUnverified
    );
    let b = app(&temp.path().join("b"), "1.0.0");
    login(&b, &server, "monitoring-compat@example.invalid", &vault).await;
    let view = a
        .monitoring_sync_readiness_with_floor(Some(floor()))
        .await
        .unwrap();
    assert_eq!(view.state, MonitoringSyncState::DevicesUnsupported);
    assert_eq!(view.checked_active_devices, 2);
    assert_eq!(
        view.blocked_devices[0].reason,
        DeviceCompatibilityReason::OlderVersion
    );
    let bid = b.session().await.unwrap().device_id();
    assert!(server.revoke_device(bid));
    assert_eq!(
        a.monitoring_sync_readiness_with_floor(Some(floor()))
            .await
            .unwrap()
            .state,
        MonitoringSyncState::ServerAdmissionUnverified
    );
    a.shutdown().await.unwrap();
    b.shutdown().await.unwrap();
    local.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_future_status_and_unreachable_actual_roster_never_become_empty_success() {
    let temp = tempfile::tempdir().unwrap();
    let server = MockServer::start_with(MockServerConfig {
        require_request_proof: true,
        ..Default::default()
    })
    .await;
    let a = app(temp.path(), "2.0.0");
    register(&a, &server, "monitoring-unknown@example.invalid").await;
    server.inject(
        "/v1/devices",
        1,
        FaultAction::Rewrite(Arc::new(|json| {
            json["devices"][0]
                .as_object_mut()
                .unwrap()
                .remove("client_version");
        })),
    );
    let view = a
        .monitoring_sync_readiness_with_floor(Some(floor()))
        .await
        .unwrap();
    assert_eq!(view.state, MonitoringSyncState::DevicesUnsupported);
    assert_eq!(
        view.blocked_devices[0].reason,
        DeviceCompatibilityReason::UnknownVersion
    );
    server.inject(
        "/v1/devices",
        1,
        FaultAction::Rewrite(Arc::new(|json| {
            json["devices"][0]["status"] = json!("inactive-future");
        })),
    );
    assert_eq!(
        a.monitoring_sync_readiness_with_floor(Some(floor()))
            .await
            .unwrap()
            .state,
        MonitoringSyncState::RosterUnavailable
    );
    server.set_offline(true);
    assert_eq!(
        a.monitoring_sync_readiness_with_floor(Some(floor()))
            .await
            .unwrap()
            .state,
        MonitoringSyncState::RosterUnavailable
    );
    server.set_offline(false);
    a.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lock_or_profile_switch_while_actual_roster_response_is_held_rejects_original_scope() {
    for switch_profile in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let server = MockServer::start_with(MockServerConfig {
            require_request_proof: true,
            ..Default::default()
        })
        .await;
        let a = Arc::new(app(temp.path(), "2.0.0"));
        register(&a, &server, "monitoring-roster-lock@example.invalid").await;
        let entered = Arc::new(tokio::sync::Notify::new());
        let (release, receive) = std::sync::mpsc::sync_channel::<()>(1);
        let n = entered.clone();
        let receive = Mutex::new(receive);
        server.inject(
            "/v1/devices",
            1,
            FaultAction::Rewrite(Arc::new(move |_| {
                n.notify_one();
                receive
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(10))
                    .unwrap();
            })),
        );
        let task = tokio::spawn({
            let a = a.clone();
            async move { a.monitoring_sync_readiness_with_floor(Some(floor())).await }
        });
        tokio::time::timeout(Duration::from_secs(5), entered.notified())
            .await
            .unwrap();
        if switch_profile {
            a.create_local_profile("Other owned compatibility scope".into(), PASS.into())
                .await
                .unwrap();
        } else {
            a.lock().await.unwrap();
        }
        release.send(()).unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        a.shutdown().await.unwrap();
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_real_appcores_transport_existing_v3_generic_edits_stale_source_and_explicit_delete() {
    let temp = tempfile::tempdir().unwrap();
    let server = MockServer::start_with(MockServerConfig {
        require_request_proof: true,
        ..Default::default()
    })
    .await;
    let a = app(&temp.path().join("a"), "2.0.0");
    let vault = register(&a, &server, "monitoring-v3@example.invalid").await;
    // A legitimate pre-existing v3/import fixture is made by the LOCAL Native
    // catalog. Synced public create/update are blocked until real admission.
    let local = app(&temp.path().join("local-existing-import"), "2.0.0");
    local
        .create_local_profile("Owned existing v3 import".into(), PASS.into())
        .await
        .unwrap();
    let created = local.monitoring_source_create(0, source()).await.unwrap();
    let source = created.sources[0].id.clone();
    let created = local
        .monitoring_panel_create(1, definition(&source, 1))
        .await
        .unwrap();
    let panel = created.panels.unwrap()[0].id.clone();
    install_raw(&a, raw(&local).await).await;
    local.shutdown().await.unwrap();
    a.sync_now().await.unwrap();
    let settings = a.get_vault_settings().await.unwrap().id;
    ciphertext_only(&server, &vault, &settings, QUERY);
    let b = app(&temp.path().join("b"), "2.0.0");
    login(&b, &server, "monitoring-v3@example.invalid", &vault).await;
    assert_eq!(
        b.monitoring_panel_definition(panel.clone(), 1)
            .await
            .unwrap()
            .query,
        QUERY
    );
    assert_eq!(raw(&a).await, raw(&b).await);
    let mut generic = b.get_vault_settings().await.unwrap();
    generic.vault_name = "Owned B metadata retained".into();
    b.save_vault_settings(generic).await.unwrap();
    b.sync_now().await.unwrap();
    a.sync_now().await.unwrap();
    assert_eq!(
        a.get_vault_settings().await.unwrap().vault_name,
        "Owned B metadata retained"
    );
    assert_eq!(
        a.monitoring_panel_definition(panel.clone(), 1)
            .await
            .unwrap()
            .query,
        QUERY
    );
    let mut config = a.monitoring_sources().await.unwrap().sources[0]
        .configuration
        .clone()
        .unwrap();
    config.name = "Owned source revision two".into();
    a.monitoring_source_update(source.clone(), 2, 1, config)
        .await
        .unwrap();
    a.sync_now().await.unwrap();
    b.sync_now().await.unwrap();
    assert!(b
        .monitoring_panel_definition(panel.clone(), 1)
        .await
        .is_err());
    assert_eq!(
        b.monitoring_sources().await.unwrap().panels.unwrap()[0].state,
        crate::monitoring_sources::MonitoringPanelState::StaleSource
    );
    let before = raw(&b).await;
    let denied = b
        .monitoring_panel_update(panel.clone(), 3, 1, definition(&source, 2))
        .await
        .unwrap_err();
    assert!(
        matches!(&denied,crate::AppError::InvalidInput{field,reason} if field=="monitoring_sync"&&reason=="policy_unavailable"),
        "{denied:?}"
    );
    assert_eq!(raw(&b).await, before);
    a.lock().await.unwrap();
    assert!(a
        .monitoring_panel_definition(panel.clone(), 1)
        .await
        .is_err());
    a.unlock_with_passphrase(PASS.into()).await.unwrap();
    a.sync_now().await.unwrap();
    assert_eq!(raw(&a).await, raw(&b).await);
    b.monitoring_panel_delete(panel, 3, 1).await.unwrap();
    b.sync_now().await.unwrap();
    a.sync_now().await.unwrap();
    let after = raw(&a).await;
    assert_eq!(after["version"], 3);
    assert!(after["panels"]["records"].as_object().unwrap().is_empty());
    assert_eq!(after, raw(&b).await);
    ciphertext_only(&server, &vault, &settings, QUERY);
    assert_eq!(
        a.monitoring_settings_sync_readiness().await.unwrap().state,
        MonitoringSyncState::ServerAdmissionUnverified
    );
    assert!(server.request_proof_stats().valid > 0);
    assert_eq!(server.request_proof_stats().rejected, 0);
    a.shutdown().await.unwrap();
    b.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_devices_preserve_future_raw_catalog_during_real_generic_settings_writes_and_encrypted_roundtrip(
) {
    let temp = tempfile::tempdir().unwrap();
    let server = MockServer::start_with(MockServerConfig {
        require_request_proof: true,
        ..Default::default()
    })
    .await;
    let a = app(&temp.path().join("a"), "2.0.0");
    let vault = register(&a, &server, "monitoring-future@example.invalid").await;
    let future = json!({"version":99,"opaque":{"private_marker":"owned_future_encrypted_marker","futureFields":[1,null,{"shape":true}]}});
    install_raw(&a, future.clone()).await;
    a.sync_now().await.unwrap();
    let b = app(&temp.path().join("b"), "2.0.0");
    login(&b, &server, "monitoring-future@example.invalid", &vault).await;
    assert_eq!(raw(&b).await, future);
    assert!(b.monitoring_sources().await.unwrap().read_only);
    let mut generic = b.get_vault_settings().await.unwrap();
    generic.vault_name = "Future raw preserved by B".into();
    b.save_vault_settings(generic).await.unwrap();
    assert!(b.monitoring_source_create(0, source()).await.is_err());
    assert_eq!(raw(&b).await, future);
    b.sync_now().await.unwrap();
    a.sync_now().await.unwrap();
    assert_eq!(raw(&a).await, future);
    assert_eq!(
        a.get_vault_settings().await.unwrap().vault_name,
        "Future raw preserved by B"
    );
    let settings = a.get_vault_settings().await.unwrap().id;
    ciphertext_only(&server, &vault, &settings, "owned_future_encrypted_marker");
    a.shutdown().await.unwrap();
    b.shutdown().await.unwrap();
}
