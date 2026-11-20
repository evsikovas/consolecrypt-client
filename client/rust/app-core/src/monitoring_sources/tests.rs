use super::*;
use crate::dto::EditableDto;
use cc_monitoring_core::{Route, TlsConfig};
use serde_json::json;

const PASSPHRASE: &str = "monitoring ephemeral vault password 42";
const MARKER: &str = "synthetic-monitoring-source-plaintext-must-not-persist";

fn config() -> SourceConfig {
    SourceConfig {
        id: String::new(),
        name: MARKER.into(),
        endpoint: format!("https://{MARKER}.example.invalid/prometheus"),
        route: Route::Direct,
        tls: TlsConfig::default(),
        auth: AuthRef::None,
    }
}

async fn setup() -> (tempfile::TempDir, AppCore) {
    let directory = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        directory.path().to_string_lossy().to_string(),
    ))
    .unwrap();
    app.create_local_profile("Monitoring ephemeral".into(), PASSPHRASE.into())
        .await
        .unwrap();
    (directory, app)
}

async fn install_raw(app: &AppCore, raw: Value) {
    let (owner, u) = app.unlocked().await.unwrap();
    let _profile = app.hold_session(&owner).await.unwrap();
    let _unlock = owner.hold_unlocked(&u).await.unwrap();
    let mut current = settings(&u).unwrap();
    current.monitoring = Some(raw);
    u.writer
        .put(VaultObject::VaultSettings(current))
        .await
        .unwrap();
}

async fn raw(app: &AppCore) -> Option<Value> {
    let (_, u) = app.unlocked().await.unwrap();
    settings(&u).unwrap().monitoring
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_catalog_crud_is_encrypted_profile_scoped_and_unlocked_only() {
    let (directory, app) = setup().await;
    let initial = app.monitoring_sources().await.unwrap();
    assert_eq!(initial.revision, Some(0));
    assert!(initial.sources.is_empty());
    assert!(!initial.read_only);
    let created = app.monitoring_source_create(0, config()).await.unwrap();
    assert_eq!(created.revision, Some(1));
    assert_eq!(created.sources.len(), 1);
    let source = created.sources[0].clone();
    canonical_id(&source.id).unwrap();
    assert_eq!(source.revision, Some(1));
    let mut updated = source.configuration.unwrap();
    assert_eq!(updated.id, source.id);
    updated.name = "Renamed source".into();
    let result = app
        .monitoring_source_update(source.id.clone(), 1, 1, updated.clone())
        .await
        .unwrap();
    assert_eq!(result.revision, Some(2));
    assert_eq!(result.sources[0].revision, Some(2));
    let (owner, u) = app.unlocked().await.unwrap();
    let _profile = app.hold_session(&owner).await.unwrap();
    let _unlock = owner.hold_unlocked(&u).await.unwrap();
    assert_eq!(
        source_for_request(&u, &source.id, 2).unwrap().name,
        "Renamed source"
    );
    assert!(source_for_request(&u, &source.id, 1).is_err());
    drop(_unlock);
    drop(_profile);
    drop(u);
    drop(owner);
    app.lock().await.unwrap();
    assert!(app.monitoring_sources().await.is_err());
    assert!(app.monitoring_source_create(2, config()).await.is_err());
    assert!(app
        .monitoring_source_delete(source.id.clone(), 2, 2)
        .await
        .is_err());
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert_eq!(
        app.monitoring_sources().await.unwrap().sources[0].id,
        source.id
    );
    app.create_local_profile("Other ephemeral".into(), PASSPHRASE.into())
        .await
        .unwrap();
    assert!(app.monitoring_sources().await.unwrap().sources.is_empty());
    assert!(app.monitoring_source_delete(source.id, 2, 2).await.is_err());
    app.shutdown().await.unwrap();

    fn check_files(path: &std::path::Path) {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                check_files(&path)
            } else {
                let bytes = std::fs::read(&path).unwrap();
                assert!(
                    !bytes
                        .windows(MARKER.len())
                        .any(|window| window == MARKER.as_bytes()),
                    "plaintext source in {}",
                    path.display()
                );
            }
        }
    }
    check_files(directory.path());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn catalog_and_source_cas_conflicts_preserve_stable_native_ids() {
    let (_directory, app) = setup().await;
    let created = app.monitoring_source_create(0, config()).await.unwrap();
    let id = created.sources[0].id.clone();
    let original = created.sources[0].configuration.clone().unwrap();
    assert!(app.monitoring_source_create(0, config()).await.is_err());
    assert!(app
        .monitoring_source_update(id.clone(), 0, 1, original.clone())
        .await
        .is_err());
    assert!(app
        .monitoring_source_update(id.clone(), 1, 2, original.clone())
        .await
        .is_err());
    assert!(app
        .monitoring_source_delete(id.clone(), 0, 1)
        .await
        .is_err());
    assert!(app
        .monitoring_source_delete(id.clone(), 1, 0)
        .await
        .is_err());
    let mut changed = original.clone();
    changed.id = Uuid::new_v4().to_string();
    assert!(app
        .monitoring_source_update(id.clone(), 1, 1, changed)
        .await
        .is_err());
    assert!(app
        .monitoring_source_create(1, original.clone())
        .await
        .is_err());
    let final_view = app.monitoring_sources().await.unwrap();
    assert_eq!(final_view.revision, Some(1));
    assert_eq!(final_view.sources[0].id, id);
    let deleted = app.monitoring_source_delete(id, 1, 1).await.unwrap();
    assert_eq!(deleted.revision, Some(2));
    assert!(deleted.sources.is_empty());
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_cas_admits_one_create_and_generic_settings_save_preserves_catalog() {
    let (_directory, app) = setup().await;
    let stale = app.get_vault_settings().await.unwrap();
    let (one, two) = tokio::join!(
        app.monitoring_source_create(0, config()),
        app.monitoring_source_create(0, config())
    );
    assert_eq!(usize::from(one.is_ok()) + usize::from(two.is_ok()), 1);
    let created = app.monitoring_sources().await.unwrap();
    let source = created.sources[0].clone();
    let mut next = source.configuration.clone().unwrap();
    next.name = "New source name".into();
    let mut generic = stale;
    generic.vault_name = "Generic settings edited concurrently".into();
    let (updated, saved) = tokio::join!(
        app.monitoring_source_update(source.id.clone(), 1, 1, next),
        app.save_vault_settings(generic),
    );
    updated.unwrap();
    saved.unwrap();
    let after = app.monitoring_sources().await.unwrap();
    assert_eq!(after.revision, Some(2));
    assert_eq!(after.sources[0].revision, Some(2));
    assert_eq!(
        after.sources[0].configuration.as_ref().unwrap().name,
        "New source name"
    );
    assert_eq!(
        app.get_vault_settings().await.unwrap().vault_name,
        "Generic settings edited concurrently"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generic_dto_of_an_older_settings_id_preserves_newest_singleton_catalog() {
    let (_directory, app) = setup().await;
    let (_, u) = app.unlocked().await.unwrap();
    let mut older = settings(&u).unwrap();
    drop(u);
    let created = app.monitoring_source_create(0, config()).await.unwrap();
    older.id = cc_models::ObjectId::new();
    older.updated_at -= chrono::Duration::seconds(1);
    assert!(older.monitoring.is_none());
    let old_dto = crate::VaultSettingsDto::from_model(&older);
    let (_, u) = app.unlocked().await.unwrap();
    u.writer
        .put(VaultObject::VaultSettings(older))
        .await
        .unwrap();
    assert_eq!(u.working().all_vault_settings().len(), 2);
    drop(u);
    let mut edit = old_dto;
    edit.vault_name = "Older settings editor".into();
    app.save_vault_settings(edit).await.unwrap();
    let after = app.monitoring_sources().await.unwrap();
    assert_eq!(after.revision, Some(1));
    assert_eq!(after.sources[0].id, created.sources[0].id);
    assert_eq!(
        app.get_vault_settings().await.unwrap().vault_name,
        "Older settings editor"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_future_and_unknown_fields_remain_readonly_and_raw_preserved_by_old_dto() {
    let (_directory, app) = setup().await;
    let id = Uuid::new_v4().to_string();
    for unsupported in [
        json!({"version":2,"revision":90,"sources":{},"future":{"opaque":[1,2,3]}}),
        json!({"version":1,"revision":1,"sources":{},"future":true}),
        json!({"version":1,"revision":0,"sources":{}}),
        json!({"version":1,"revision":1,"sources":{"not-a-uuid":{"version":1,"revision":1,"configuration":config()}}}),
        json!(["malformed"]),
        json!({"version":1,"revision":1,"sources":{id.clone():{"version":2,"revision":1,"configuration":config()}}}),
    ] {
        install_raw(&app, unsupported.clone()).await;
        assert!(app.monitoring_sources().await.unwrap().read_only);
        assert!(app.monitoring_source_create(1, config()).await.is_err());
        assert!(app
            .monitoring_source_delete(id.clone(), 1, 1)
            .await
            .is_err());
        let (_, u) = app.unlocked().await.unwrap();
        assert!(source_for_request(&u, &id, 1).is_err());
        drop(u);
        let mut old = app.get_vault_settings().await.unwrap();
        old.vault_name = "Old generic DTO edit".into();
        app.save_vault_settings(old).await.unwrap();
        assert_eq!(raw(&app).await, Some(unsupported));
    }
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_urls_plaintext_auth_payloads_and_missing_credential_refs_do_not_mutate_catalog() {
    let (_directory, app) = setup().await;
    for endpoint in [
        "https://user:secret@example.invalid/",
        "https://example.invalid/?token=secret",
        "https://example.invalid/#secret",
        "file:///tmp/metrics",
    ] {
        let mut bad = config();
        bad.endpoint = endpoint.into();
        assert!(app.monitoring_source_create(0, bad).await.is_err());
    }
    for auth in [
        AuthRef::BearerRef {
            credential_id: Uuid::new_v4().to_string(),
        },
        AuthRef::BasicRef {
            credential_id: "not-a-native-id".into(),
        },
    ] {
        let mut bad = config();
        bad.auth = auth;
        assert!(app.monitoring_source_create(0, bad).await.is_err());
    }
    let mut wire = serde_json::to_value(config()).unwrap();
    wire["auth"] = json!({"kind":"bearerRef","credentialId":"credential-1","token":"never accept"});
    assert!(serde_json::from_value::<SourceConfig>(wire).is_err());
    assert_eq!(app.monitoring_sources().await.unwrap().revision, Some(0));
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_count_and_serialized_catalog_byte_quotas_fail_before_write() {
    let (_directory, app) = setup().await;
    for revision in 0..32 {
        app.monitoring_source_create(revision, config())
            .await
            .unwrap();
    }
    assert!(app.monitoring_source_create(32, config()).await.is_err());
    let sources = app.monitoring_sources().await.unwrap().sources;
    let mut failed_at = None;
    let mut successes = 0;
    for (index, source) in sources.iter().take(5).enumerate() {
        let mut oversized = source.configuration.clone().unwrap();
        // Syntactically bounded public PEM passes configuration validation.
        // DER parsing belongs to the transport builder, which is never called.
        oversized.tls.custom_ca_pem = Some(format!(
            "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
            "QUFB".repeat(255 * 1024 / 4),
        ));
        oversized.validate().unwrap();
        let revision = 32 + index as u64;
        let before = raw(&app).await;
        match app
            .monitoring_source_update(source.id.clone(), revision, 1, oversized)
            .await
        {
            Ok(view) => {
                successes += 1;
                assert_eq!(view.revision, Some(revision + 1));
            }
            Err(error) => {
                assert!(matches!(error, AppError::InvalidInput { ref reason, .. }
                    if reason == "monitoring catalog byte limit exceeded"));
                // Keep failure diagnostics bounded: never dump MiBs of PEM.
                assert!(
                    raw(&app).await == before,
                    "failed quota write changed raw catalog"
                );
                failed_at = Some(index);
                break;
            }
        }
    }
    assert!(matches!(failed_at, Some(3 | 4)));
    assert!(successes >= 3);
    let after = app.monitoring_sources().await.unwrap();
    assert_eq!(after.sources.len(), 32);
    assert_eq!(after.revision, Some(32 + successes));
    app.shutdown().await.unwrap();
}

#[test]
fn legacy_model_defaults_and_generic_dto_conversion_preserve_raw_catalog_without_exposing_it() {
    let now = chrono::Utc::now();
    let old = json!({"id":cc_models::ObjectId::new(),"vault_name":"Legacy","created_at":now,"updated_at":now});
    let mut model: VaultSettings = serde_json::from_value(old).unwrap();
    assert!(model.monitoring.is_none());
    let future = json!({"version":200,"revision":77,"opaque":{"unchanged":true}});
    model.monitoring = Some(future.clone());
    let dto = crate::VaultSettingsDto::from_model(&model);
    assert!(serde_json::to_value(&dto)
        .unwrap()
        .get("monitoring")
        .is_none());
    let converted = dto.to_model(model.id, Some(&model)).unwrap();
    assert_eq!(converted.monitoring, Some(future));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_destination_is_durable_data_and_unknown_host_is_denied() {
    let (_directory, app) = setup().await;
    let mut events=app.subscribe_events();
    let host = app.save_host(crate::HostDto::new("Agent host", "host-fixture.example.invalid")).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5),async {
        loop {if let crate::AppEvent::ObjectsChanged {kind:cc_models::ObjectKind::Host,
            ids,origin:crate::ChangeOriginDto::Local}=events.recv().await.unwrap() {
            if ids.iter().any(|id|id==&host.id){break}
        }}
    }).await.unwrap();drop(events);
    let route = cc_monitoring_core::Route::LegacyDestination {
        association_id: "11111111-1111-4111-8111-111111111111".into(),
        host_id: host.id.clone(), destination_ip: "127.0.0.1".into(), destination_port: 9090,
    };
    let mut source = config();source.endpoint = "http://127.0.0.1:9090".into();source.route = route.clone();
    let mut unknown_host = source.clone();
    if let cc_monitoring_core::Route::LegacyDestination { host_id, .. } = &mut unknown_host.route {
        *host_id="22222222-2222-4222-8222-222222222222".into();
    }
    assert!(app.monitoring_source_create(0, unknown_host).await.is_err());
    assert_eq!(app.monitoring_sources().await.unwrap().revision, Some(0));
    let saved=app.monitoring_source_create(0, source).await.unwrap().sources[0].clone();
    assert_eq!(saved.configuration.as_ref().unwrap().route,route);
    app.lock().await.unwrap();
    assert!(app.monitoring_sources().await.is_err());
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    let restored=app.monitoring_sources().await.unwrap().sources[0].clone();
    assert_eq!(restored.configuration.unwrap().route,route);
    assert_eq!(restored.id,saved.id);assert_eq!(restored.revision,Some(1));
    // Neither source save nor restore prepares or signs a live tunnel policy.
    let data=raw(&app).await.unwrap();
    let stored=serde_json::to_value(data["sources"].as_object().unwrap().get(&saved.id).unwrap()).unwrap();
    let route_json=&stored["configuration"]["route"];
    assert_eq!(route_json.as_object().unwrap().len(),5);
    for key in ["routeId","grantId","certificate","expiresAt","lease"] {
        assert!(route_json.get(key).is_none());
    }
    app.shutdown().await.unwrap();
}
