use super::*;
use cc_host_mapping::Labels;
use cc_monitoring_core::{Route, TlsConfig};
use serde_json::json;

const PASSWORD: &str = "owned saved-panel fixture passphrase 42";
const QUERY: &str = "sum(rate(synthetic_saved_panel_private_marker_total[5m]))";
fn definition(source: &str) -> MonitoringPanelDefinition {
    MonitoringPanelDefinition {
        title: "Owned panel".into(),
        source_id: source.into(),
        source_revision: 1,
        query: QUERY.into(),
        unit_hint: MonitoringUnitHint::Unknown,
    }
}
fn source_config() -> SourceConfig {
    SourceConfig {
        id: String::new(),
        name: "Owned panel source".into(),
        endpoint: "https://owned-panel.example.invalid/prometheus".into(),
        route: Route::Direct,
        tls: TlsConfig::default(),
        auth: AuthRef::None,
    }
}
async fn setup() -> (tempfile::TempDir, AppCore, String) {
    let directory = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        directory.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("Owned panel profile".into(), PASSWORD.into())
        .await
        .unwrap();
    let view = app
        .monitoring_source_create(0, source_config())
        .await
        .unwrap();
    let source = view.sources[0].id.clone();
    (directory, app, source)
}
async fn catalog_raw(app: &AppCore) -> Value {
    let (_, u) = app.unlocked().await.unwrap();
    settings(&u).unwrap().monitoring.unwrap()
}
async fn install(app: &AppCore, raw: Value) {
    let (owner, u) = app.unlocked().await.unwrap();
    let _profile = app.hold_session(&owner).await.unwrap();
    let _unlock = owner.hold_unlocked(&u).await.unwrap();
    let mut settings = settings(&u).unwrap();
    settings.monitoring = Some(raw);
    u.writer
        .put(VaultObject::VaultSettings(settings))
        .await
        .unwrap();
}

#[tokio::test]
async fn explicit_panel_create_migrates_v1_and_native_crud_preserves_exact_query_unknown_unit_and_ids(
) {
    let (_directory, app, source) = setup().await;
    assert_eq!(catalog_raw(&app).await["version"], 1);
    assert!(
        !serde_json::to_value(app.monitoring_sources().await.unwrap())
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("panels")
    );
    let created = app
        .monitoring_panel_create(1, definition(&source))
        .await
        .unwrap();
    assert_eq!(created.schema_version, Some(3));
    assert_eq!(created.revision, Some(2));
    let panel = created.panels.unwrap().pop().unwrap();
    canonical_id(&panel.id).unwrap();
    assert_eq!(panel.revision, 1);
    assert_eq!(panel.definition.query, QUERY);
    assert_eq!(panel.definition.unit_hint, MonitoringUnitHint::Unknown);
    assert_eq!(
        app.monitoring_panel_definition(panel.id.clone(), 1)
            .await
            .unwrap(),
        definition(&source)
    );
    let mut updated = definition(&source);
    updated.title = "Updated panel".into();
    updated.query = "\n up{job=\"literal\"}\n".into();
    updated.unit_hint = MonitoringUnitHint::Count;
    let changed = app
        .monitoring_panel_update(panel.id.clone(), 2, 1, updated.clone())
        .await
        .unwrap();
    let saved = changed.panels.unwrap().pop().unwrap();
    assert_eq!(saved.id, panel.id);
    assert_eq!(saved.revision, 2);
    assert_eq!(saved.definition, updated);
    assert!(app
        .monitoring_panel_definition(panel.id.clone(), 1)
        .await
        .is_err());
    let deleted = app
        .monitoring_panel_delete(panel.id.clone(), 3, 2)
        .await
        .unwrap();
    assert_eq!(deleted.schema_version, Some(3));
    assert!(deleted.panels.unwrap().is_empty());
    assert!(app.monitoring_panel_definition(panel.id, 2).await.is_err());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn existing_bindings_survive_v2_to_v3_and_future_binding_create_never_downgrades_panels() {
    let (_directory, app, source) = setup().await;
    let host = app
        .save_host(crate::HostDto::new(
            "Owned panel host",
            "panel-host.example.invalid",
        ))
        .await
        .unwrap();
    let selector = Labels::from([("instance".into(), "explicit:9100".into())]);
    let bound = app
        .monitoring_binding_create(
            1,
            "First exporter".into(),
            host.id.clone(),
            source.clone(),
            1,
            selector,
        )
        .await
        .unwrap();
    let before = serde_json::to_value(&bound.bindings).unwrap();
    assert_eq!(bound.schema_version, Some(2));
    let created = app
        .monitoring_panel_create(2, definition(&source))
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(&created.bindings).unwrap(), before);
    let panels_before = catalog_raw(&app).await["panels"].clone();
    app.monitoring_binding_create(
        3,
        "Other exporter".into(),
        host.id,
        source.clone(),
        1,
        Labels::from([("instance".into(), "explicit:9200".into())]),
    )
    .await
    .unwrap();
    let after = catalog_raw(&app).await;
    assert_eq!(after["version"], 3);
    assert_eq!(after["panels"], panels_before);
    assert_eq!(after["bindings"]["records"].as_object().unwrap().len(), 2);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn source_edits_stale_panels_until_explicit_rereview_and_delete_blocks_references() {
    let (_directory, app, source) = setup().await;
    let created = app
        .monitoring_panel_create(1, definition(&source))
        .await
        .unwrap();
    let panel = created.panels.unwrap().pop().unwrap();
    let before = catalog_raw(&app).await;
    assert!(app
        .monitoring_source_delete(source.clone(), 2, 1)
        .await
        .is_err());
    assert_eq!(catalog_raw(&app).await, before);
    let mut config = created.sources[0].configuration.clone().unwrap();
    config.endpoint = "https://changed-panel.example.invalid/prometheus".into();
    let changed = app
        .monitoring_source_update(source.clone(), 2, 1, config)
        .await
        .unwrap();
    assert_eq!(
        changed.panels.as_ref().unwrap()[0].state,
        MonitoringPanelState::StaleSource
    );
    assert!(app
        .monitoring_panel_definition(panel.id.clone(), 1)
        .await
        .is_err());
    let mut reviewed = definition(&source);
    reviewed.source_revision = 2;
    let updated = app
        .monitoring_panel_update(panel.id.clone(), 3, 1, reviewed)
        .await
        .unwrap();
    assert_eq!(
        updated.panels.unwrap()[0].state,
        MonitoringPanelState::Current
    );
    app.monitoring_panel_delete(panel.id, 4, 2).await.unwrap();
    app.monitoring_source_delete(source, 5, 2).await.unwrap();
    assert_eq!(catalog_raw(&app).await["version"], 3);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn malformed_and_future_panel_schemas_stay_raw_readonly_through_generic_settings_saves() {
    let (_directory, app, source) = setup().await;
    app.monitoring_panel_create(1, definition(&source))
        .await
        .unwrap();
    let original = catalog_raw(&app).await;
    let id = original["panels"]["records"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    let mut cases = vec![];
    for (pointer, value) in [
        ("/version", json!(99)),
        ("/panels/version", json!(2)),
        ("/panels/future", json!({"opaque":[1,2]})),
    ] {
        let mut raw = original.clone();
        if let Some(slot) = raw.pointer_mut(pointer) {
            *slot = value;
        } else {
            raw["panels"]["future"] = value;
        }
        cases.push(raw);
    }
    let mut unknown = original.clone();
    unknown["panels"]["records"][&id]["definition"]["unitHint"] = json!("futureUnit");
    cases.push(unknown);
    let mut measurements = original.clone();
    measurements["panels"]["records"][&id]["definition"]["measurements"] = json!([123]);
    cases.push(measurements);
    for unsupported in cases {
        install(&app, unsupported.clone()).await;
        let view = app.monitoring_sources().await.unwrap();
        assert!(view.read_only);
        assert!(view.panels.is_none());
        assert!(view.sources.is_empty());
        assert!(app
            .monitoring_panel_create(2, definition(&source))
            .await
            .is_err());
        assert!(app
            .monitoring_source_create(2, source_config())
            .await
            .is_err());
        assert!(app.monitoring_panel_delete(id.clone(), 2, 1).await.is_err());
        assert!(app
            .monitoring_panel_definition(id.clone(), 1)
            .await
            .is_err());
        assert!(app
            .monitoring_source_delete(source.clone(), 2, 1)
            .await
            .is_err());
        let mut attempted_source = source_config();
        attempted_source.id = source.clone();
        assert!(app
            .monitoring_source_update(source.clone(), 2, 1, attempted_source)
            .await
            .is_err());
        let mut generic = app.get_vault_settings().await.unwrap();
        generic.vault_name = "Generic metadata edit preserves future raw".into();
        let (saved, denied) = tokio::join!(
            app.save_vault_settings(generic),
            app.monitoring_panel_update(id.clone(), 2, 1, definition(&source))
        );
        saved.unwrap();
        assert!(denied.is_err());
        assert_eq!(catalog_raw(&app).await, unsupported);
    }
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn all_source_crud_preserves_v3_panels_without_reinterpreting_or_downgrading() {
    let (_directory, app, source) = setup().await;
    app.monitoring_panel_create(1, definition(&source))
        .await
        .unwrap();
    let original = catalog_raw(&app).await["panels"].clone();
    let created = app
        .monitoring_source_create(2, source_config())
        .await
        .unwrap();
    let other = created
        .sources
        .into_iter()
        .find(|row| row.id != source)
        .unwrap();
    let mut config = other.configuration.unwrap();
    config.name = "Other source edit".into();
    app.monitoring_source_update(other.id.clone(), 3, 1, config)
        .await
        .unwrap();
    app.monitoring_source_delete(other.id, 4, 2).await.unwrap();
    let after = catalog_raw(&app).await;
    assert_eq!(after["version"], 3);
    assert_eq!(after["panels"], original);
    assert_eq!(after["revision"], 5);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn v1_v2_literal_schemas_reject_panel_fields_and_normal_source_writes_never_migrate() {
    let (_directory, app, source) = setup().await;
    let original = catalog_raw(&app).await;
    for version in [1, 2] {
        let mut raw = original.clone();
        raw["version"] = json!(version);
        if version == 2 {
            raw["bindings"] = json!({"version":1,"records":{}});
        }
        raw["panels"] = Value::Null;
        install(&app, raw.clone()).await;
        assert!(app
            .monitoring_panel_create(1, definition(&source))
            .await
            .is_err());
        assert_eq!(catalog_raw(&app).await, raw);
    }
    install(&app, original).await;
    let mut config = app.monitoring_sources().await.unwrap().sources[0]
        .configuration
        .clone()
        .unwrap();
    config.name = "Metadata only source edit".into();
    app.monitoring_source_update(source, 1, 1, config)
        .await
        .unwrap();
    let after = catalog_raw(&app).await;
    assert_eq!(after["version"], 1);
    assert!(after.get("panels").is_none());
    assert!(after.get("bindings").is_none());
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_panel_create_cas_has_one_winner_and_stale_mutations_preserve_catalog() {
    let (_directory, app, source) = setup().await;
    let (a, b) = tokio::join!(
        app.monitoring_panel_create(1, definition(&source)),
        app.monitoring_panel_create(1, definition(&source))
    );
    assert_ne!(a.is_ok(), b.is_ok());
    let view = app.monitoring_sources().await.unwrap();
    assert_eq!(view.revision, Some(2));
    let panel = view.panels.unwrap().pop().unwrap();
    let before = catalog_raw(&app).await;
    assert!(app
        .monitoring_panel_update(panel.id.clone(), 1, 1, definition(&source))
        .await
        .is_err());
    assert!(app
        .monitoring_panel_update(panel.id.clone(), 2, 2, definition(&source))
        .await
        .is_err());
    assert!(app
        .monitoring_panel_delete(panel.id.clone(), 1, 1)
        .await
        .is_err());
    assert!(app.monitoring_panel_delete(panel.id, 2, 0).await.is_err());
    assert_eq!(catalog_raw(&app).await, before);
    app.shutdown().await.unwrap();
}

#[test]
fn panel_definition_rejects_authority_fields_duplicate_json_unknown_units_and_invalid_bounds() {
    let source = Uuid::new_v4().to_string();
    let good = definition(&source);
    let raw = serde_json::to_value(&good).unwrap();
    for (key, value) in [
        ("credentials", json!(true)),
        ("measurements", json!([])),
        ("url", json!("https://not-a-panel-source.invalid")),
        ("version", json!(2)),
    ] {
        let mut invalid = raw.clone();
        invalid[key] = value;
        assert!(serde_json::from_value::<MonitoringPanelDefinition>(invalid).is_err());
    }
    let duplicate = serde_json::to_string(&good).unwrap().replacen(
        "\"title\":",
        "\"title\":\"first\",\"title\":",
        1,
    );
    assert!(serde_json::from_str::<MonitoringPanelDefinition>(&duplicate).is_err());
    for query in [
        String::new(),
        "  \n".into(),
        "x".repeat(16 * 1024 + 1),
        "query\0suffix".into(),
    ] {
        let mut bad = good.clone();
        bad.query = query;
        assert!(bad.validate().is_err());
    }
    assert!(!format!("{good:?}").contains(QUERY));
    assert!(!format!("{good:?}").contains(&good.title));
}

#[tokio::test]
async fn panel_count_and_definition_byte_limits_fail_without_mutating_raw_catalog() {
    let (_directory, app, source) = setup().await;
    app.monitoring_panel_create(1, definition(&source))
        .await
        .unwrap();
    let mut raw = catalog_raw(&app).await;
    let row = raw["panels"]["records"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .clone();
    raw["panels"]["records"] = json!({});
    for _ in 0..panels::MAX_PANELS {
        raw["panels"]["records"][Uuid::new_v4().to_string()] = row.clone();
    }
    install(&app, raw.clone()).await;
    assert!(app
        .monitoring_panel_create(2, definition(&source))
        .await
        .is_err());
    assert_eq!(catalog_raw(&app).await, raw);
    let mut invalid = definition(&source);
    invalid.query = "x".repeat(panels::MAX_QUERY_BYTES + 1);
    assert!(app.monitoring_panel_create(2, invalid).await.is_err());
    assert_eq!(catalog_raw(&app).await, raw);
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generic_settings_save_concurrent_with_panel_and_source_cas_preserves_both_in_either_poll_order(
) {
    for generic_first in [true, false] {
        let (_directory, app, source) = setup().await;
        let mut generic = app.get_vault_settings().await.unwrap();
        generic.vault_name = "Concurrent panel-create metadata".into();
        let edit = app.save_vault_settings(generic);
        let create = app.monitoring_panel_create(1, definition(&source));
        let (saved, panel) = if generic_first {
            tokio::join!(edit, create)
        } else {
            let (p, s) = tokio::join!(create, edit);
            (s, p)
        };
        saved.unwrap();
        let created = panel.unwrap();
        let panel_id = created.panels.unwrap()[0].id.clone();
        let panels = catalog_raw(&app).await["panels"].clone();
        let mut generic = app.get_vault_settings().await.unwrap();
        generic.vault_name = "Concurrent source-update metadata".into();
        let mut config = created.sources[0].configuration.clone().unwrap();
        config.name = "Source edit survived generic save".into();
        let edit = app.save_vault_settings(generic);
        let update = app.monitoring_source_update(source.clone(), 2, 1, config);
        let (saved, updated) = if generic_first {
            tokio::join!(edit, update)
        } else {
            let (u, s) = tokio::join!(update, edit);
            (s, u)
        };
        saved.unwrap();
        updated.unwrap();
        assert_eq!(catalog_raw(&app).await["panels"], panels);
        let mut generic = app.get_vault_settings().await.unwrap();
        generic.vault_name = "Concurrent panel-update metadata".into();
        let mut reviewed = definition(&source);
        reviewed.source_revision = 2;
        reviewed.title = "Rereviewed panel".into();
        let edit = app.save_vault_settings(generic);
        let update = app.monitoring_panel_update(panel_id.clone(), 3, 1, reviewed);
        let (saved, updated) = if generic_first {
            tokio::join!(edit, update)
        } else {
            let (u, s) = tokio::join!(update, edit);
            (s, u)
        };
        saved.unwrap();
        let updated = updated.unwrap();
        assert_eq!(updated.panels.unwrap()[0].revision, 2);
        let mut generic = app.get_vault_settings().await.unwrap();
        generic.vault_name = "Concurrent panel-delete metadata".into();
        let edit = app.save_vault_settings(generic);
        let delete = app.monitoring_panel_delete(panel_id, 4, 2);
        let (saved, deleted) = if generic_first {
            tokio::join!(edit, delete)
        } else {
            let (d, s) = tokio::join!(delete, edit);
            (s, d)
        };
        saved.unwrap();
        let deleted = deleted.unwrap();
        assert!(deleted.panels.unwrap().is_empty());
        assert_eq!(deleted.revision, Some(5));
        assert_eq!(
            deleted.sources[0].configuration.as_ref().unwrap().name,
            "Source edit survived generic save"
        );
        assert_eq!(
            app.get_vault_settings().await.unwrap().vault_name,
            "Concurrent panel-delete metadata"
        );
        app.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn generic_save_with_source_create_delete_preserves_panels_and_unrelated_metadata() {
    for generic_first in [true, false] {
        let (_directory, app, source) = setup().await;
        app.monitoring_panel_create(1, definition(&source))
            .await
            .unwrap();
        let panels = catalog_raw(&app).await["panels"].clone();
        let mut generic = app.get_vault_settings().await.unwrap();
        generic.vault_name = "Concurrent source creation metadata".into();
        let edit = app.save_vault_settings(generic);
        let create = app.monitoring_source_create(2, source_config());
        let (saved, created) = if generic_first {
            tokio::join!(edit, create)
        } else {
            let (c, s) = tokio::join!(create, edit);
            (s, c)
        };
        saved.unwrap();
        let created = created.unwrap();
        let other = created
            .sources
            .iter()
            .find(|s| s.id != source)
            .unwrap()
            .id
            .clone();
        assert_eq!(catalog_raw(&app).await["panels"], panels);
        assert_eq!(
            app.get_vault_settings().await.unwrap().vault_name,
            "Concurrent source creation metadata"
        );
        let mut generic = app.get_vault_settings().await.unwrap();
        generic.vault_name = "Concurrent source deletion metadata".into();
        let edit = app.save_vault_settings(generic);
        let delete = app.monitoring_source_delete(other, 3, 1);
        let (saved, deleted) = if generic_first {
            tokio::join!(edit, delete)
        } else {
            let (d, s) = tokio::join!(delete, edit);
            (s, d)
        };
        saved.unwrap();
        deleted.unwrap();
        let after = catalog_raw(&app).await;
        assert_eq!(after["version"], 3);
        assert_eq!(after["revision"], 4);
        assert_eq!(after["panels"], panels);
        assert_eq!(
            app.get_vault_settings().await.unwrap().vault_name,
            "Concurrent source deletion metadata"
        );
        app.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn whole_object_actual_serializer_limit_includes_other_settings_and_rejects_before_write() {
    let (_directory, app, source) = setup().await;
    {
        let (owner, u) = app.unlocked().await.unwrap();
        let _profile = app.hold_session(&owner).await.unwrap();
        let _unlock = owner.hold_unlocked(&u).await.unwrap();
        let mut current = settings(&u).unwrap();
        current.description = Some("x".repeat(900 * 1024));
        u.writer
            .put(VaultObject::VaultSettings(current))
            .await
            .unwrap();
    }
    let mut successes = 0;
    let mut failed = false;
    for _ in 0..16 {
        let before = catalog_raw(&app).await;
        let mut next = definition(&source);
        next.query = "q".repeat(panels::MAX_QUERY_BYTES);
        next.title = format!("Bounded full-object panel {successes}");
        match app.monitoring_panel_create(1 + successes, next).await {
            Ok(_) => successes += 1,
            Err(error) => {
                // No guessed padding/headroom: this is the real ObjectPayload
                // -> JSON -> crypto serialize_padded -> ObjectStore boundary.
                assert!(
                    matches!(error,AppError::Crypto(ref reason) if reason.contains("object too large")&&reason.len()<256)
                );
                assert!(serde_json::to_vec(&before).unwrap().len() < 256 * 1024);
                assert_eq!(catalog_raw(&app).await, before);
                failed = true;
                break;
            }
        }
    }
    assert!(failed);
    assert!(successes > 0 && successes < 16);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn saved_panels_are_encrypted_unlocked_profile_scoped_and_not_plaintext_files() {
    let (directory, app, source) = setup().await;
    let created = app
        .monitoring_panel_create(1, definition(&source))
        .await
        .unwrap();
    let id = created.panels.unwrap()[0].id.clone();
    app.lock().await.unwrap();
    assert!(app
        .monitoring_panel_definition(id.clone(), 1)
        .await
        .is_err());
    assert!(app.monitoring_panel_delete(id.clone(), 2, 1).await.is_err());
    app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    assert_eq!(
        app.monitoring_panel_definition(id.clone(), 1)
            .await
            .unwrap()
            .query,
        QUERY
    );
    app.create_local_profile("Other owned panel profile".into(), PASSWORD.into())
        .await
        .unwrap();
    assert!(app.monitoring_panel_definition(id, 1).await.is_err());
    assert!(app.monitoring_sources().await.unwrap().panels.is_none());
    app.shutdown().await.unwrap();
    fn check(path: &std::path::Path) {
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                check(&path)
            } else {
                let bytes = std::fs::read(path).unwrap();
                assert!(!bytes
                    .windows(QUERY.len())
                    .any(|part| part == QUERY.as_bytes()));
            }
        }
    }
    check(directory.path());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn synced_panel_create_update_require_admission_but_existing_cas_delete_preserves_v3_and_future_raw(
) {
    use cc_sync_core::mock::{MockServer, MockServerConfig};
    let (_local_dir, local, source) = setup().await;
    let view = local
        .monitoring_panel_create(1, definition(&source))
        .await
        .unwrap();
    let panel = view.panels.unwrap()[0].id.clone();
    let existing = catalog_raw(&local).await;
    let directory = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        directory.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    let server = MockServer::start_with(MockServerConfig {
        require_request_proof: true,
        ..Default::default()
    })
    .await;
    app.create_synced_profile(
        "Owned gated profile".into(),
        server.url().to_string(),
        "panel-gate@example.invalid".into(),
        "owned account password fixture 42".into(),
        crate::AccountMode::Register,
    )
    .await
    .unwrap();
    app.create_vault(PASSWORD.into()).await.unwrap();
    // Existing imported/older-preview data, through the ACTUAL encrypted writer.
    // This test fixture is not a public writer authorization or fake admission.
    install(&app, existing.clone()).await;
    for result in [
        app.monitoring_panel_create(2, definition(&source)).await,
        app.monitoring_panel_update(panel.clone(), 2, 1, definition(&source))
            .await,
    ] {
        let error = result.unwrap_err();
        assert!(
            matches!(error,AppError::InvalidInput{ref field,ref reason} if field=="monitoring_panels"&&reason=="panel_sync_admission_required")
        );
        assert_eq!(catalog_raw(&app).await, existing);
    }
    assert!(app
        .monitoring_panel_delete(panel.clone(), 1, 1)
        .await
        .is_err());
    assert_eq!(catalog_raw(&app).await, existing);
    app.monitoring_panel_delete(panel.clone(), 2, 1)
        .await
        .unwrap();
    let after = catalog_raw(&app).await;
    assert_eq!(after["version"], 3);
    assert_eq!(after["revision"], 3);
    assert_eq!(after["sources"], existing["sources"]);
    assert_eq!(after["bindings"], existing["bindings"]);
    assert!(after["panels"]["records"].as_object().unwrap().is_empty());
    let future = serde_json::json!({"version":99,"opaque":{"keep":[true,1,null]}});
    install(&app, future.clone()).await;
    assert!(app.monitoring_panel_delete(panel, 3, 1).await.is_err());
    assert_eq!(catalog_raw(&app).await, future);
    app.shutdown().await.unwrap();
    local.shutdown().await.unwrap();
}
