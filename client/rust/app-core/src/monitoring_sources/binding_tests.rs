//! Native catalog migration, revision fences and scoped identity helper tests.
use super::*;
use cc_host_mapping::{Labels, Resolution};
use cc_monitoring_core::{QueryData, QueryOutcome, Route, Sample, SampleValue, Series, TlsConfig};
use serde_json::json;

const PASSWORD: &str = "next binding ephemeral profile password 42";
const END: f64 = 1_700_000_000.0;
fn labels(job: &str) -> Labels {
    BTreeMap::from([
        ("job".into(), job.into()),
        ("instance".into(), "synthetic:9100".into()),
    ])
}
fn config() -> SourceConfig {
    SourceConfig {
        id: String::new(),
        name: "Synthetic binding source".into(),
        endpoint: "https://binding-fixture.example.invalid/prometheus".into(),
        route: Route::Direct,
        tls: TlsConfig::default(),
        auth: AuthRef::None,
    }
}
async fn setup() -> (tempfile::TempDir, AppCore, String, String) {
    let root = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        root.path().to_string_lossy().to_string(),
    ))
    .unwrap();
    app.create_local_profile("Host binding fixture".into(), PASSWORD.into())
        .await
        .unwrap();
    let host = app
        .save_host(crate::HostDto::new(
            "Native fixture host",
            "host-fixture.example.invalid",
        ))
        .await
        .unwrap();
    let view = app.monitoring_source_create(0, config()).await.unwrap();
    let source = view.sources[0].id.clone();
    (root, app, host.id, source)
}
async fn raw(app: &AppCore) -> Value {
    let (_, u) = app.unlocked().await.unwrap();
    settings(&u).unwrap().monitoring.unwrap()
}
async fn install_raw(app: &AppCore, value: Value) {
    let (owner, u) = app.unlocked().await.unwrap();
    let _profile = app.hold_session(&owner).await.unwrap();
    let _unlock = owner.hold_unlocked(&u).await.unwrap();
    let mut settings = settings(&u).unwrap();
    settings.monitoring = Some(value);
    u.writer
        .put(VaultObject::VaultSettings(settings))
        .await
        .unwrap();
}
async fn create(
    app: &AppCore,
    revision: u64,
    host: &str,
    source: &str,
    job: &str,
) -> MonitoringCatalogView {
    app.monitoring_binding_create(
        revision,
        format!("{job} exporter"),
        host.into(),
        source.into(),
        1,
        labels(job),
    )
    .await
    .unwrap()
}
fn vector(rows: Vec<Labels>) -> QueryOutcome {
    QueryOutcome {
        status: "success".into(),
        http_status: 200,
        data: Some(QueryData::Vector {
            series: rows
                .into_iter()
                .map(|mut labels| {
                    labels.insert("__name__".into(), "up".into());
                    Series {
                        labels,
                        samples: vec![Sample {
                            timestamp: END,
                            value: SampleValue::Finite { value: 1.0 },
                        }],
                    }
                })
                .collect(),
        }),
        warnings: vec![],
        infos: vec![],
        partial: false,
        error: None,
        effective_step_seconds: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_explicit_binding_atomically_migrates_v1_and_preserves_source_records() {
    let (_root, app, host, source) = setup().await;
    let before = raw(&app).await;
    assert_eq!(before["version"], 1);
    assert!(before.get("bindings").is_none());
    let mut configuration = app.monitoring_sources().await.unwrap().sources[0]
        .configuration
        .clone()
        .unwrap();
    configuration.name = "Source metadata edit".into();
    app.monitoring_source_update(source.clone(), 1, 1, configuration)
        .await
        .unwrap();
    let legacy = raw(&app).await;
    assert_eq!(legacy["version"], 1);
    assert_eq!(legacy.as_object().unwrap().len(), 3);
    let view = app
        .monitoring_binding_create(
            2,
            " Node exporter ".into(),
            host.clone(),
            source.clone(),
            2,
            labels("node"),
        )
        .await
        .unwrap();
    assert_eq!(view.schema_version, Some(2));
    assert_eq!(view.revision, Some(3));
    assert_eq!(view.bindings.len(), 1);
    assert_eq!(view.bindings[0].name, "Node exporter");
    assert_eq!(view.bindings[0].state, MonitoringBindingState::Current);
    assert_eq!(view.bindings[0].source_revision, 2);
    canonical_id(&view.bindings[0].id).unwrap();
    let after = raw(&app).await;
    assert_eq!(after["sources"], legacy["sources"]);
    assert_eq!(after["bindings"]["version"], 1);
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_only_crud_preserves_v2_and_all_existing_binding_records() {
    let (_root, app, host, source) = setup().await;
    create(&app, 1, &host, &source, "node").await;
    let bindings = raw(&app).await["bindings"].clone();
    let added = app.monitoring_source_create(2, config()).await.unwrap();
    let other = added.sources.iter().find(|row| row.id != source).unwrap();
    let other_id = other.id.clone();
    let mut configuration = other.configuration.clone().unwrap();
    configuration.name = "Unrelated source metadata edit".into();
    assert_eq!(added.schema_version, Some(2));
    assert_eq!(raw(&app).await["bindings"], bindings);
    let changed = app
        .monitoring_source_update(other_id.clone(), 3, 1, configuration)
        .await
        .unwrap();
    assert_eq!(changed.schema_version, Some(2));
    assert_eq!(raw(&app).await["bindings"], bindings);
    let deleted = app.monitoring_source_delete(other_id, 4, 2).await.unwrap();
    assert_eq!(deleted.schema_version, Some(2));
    assert_eq!(deleted.revision, Some(5));
    assert_eq!(deleted.sources.len(), 1);
    assert_eq!(deleted.sources[0].id, source);
    assert_eq!(deleted.bindings[0].state, MonitoringBindingState::Current);
    assert_eq!(raw(&app).await["bindings"], bindings);
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn migration_and_mutation_cas_admit_exactly_one_concurrent_writer() {
    let (_root, app, host, source) = setup().await;
    let (one, two) = tokio::join!(
        app.monitoring_binding_create(
            1,
            "Node".into(),
            host.clone(),
            source.clone(),
            1,
            labels("node")
        ),
        app.monitoring_binding_create(
            1,
            "cAdvisor".into(),
            host.clone(),
            source.clone(),
            1,
            labels("cadvisor")
        ),
    );
    assert_eq!(usize::from(one.is_ok()) + usize::from(two.is_ok()), 1);
    let current = app.monitoring_sources().await.unwrap();
    assert_eq!(current.revision, Some(2));
    assert_eq!(current.bindings.len(), 1);
    let binding = &current.bindings[0];
    assert!(app
        .monitoring_binding_delete(binding.id.clone(), 1, 1)
        .await
        .is_err());
    assert!(app
        .monitoring_binding_delete(binding.id.clone(), 2, 2)
        .await
        .is_err());
    assert!(app
        .monitoring_binding_create(
            2,
            "Wrong source revision".into(),
            host,
            source,
            2,
            labels("other")
        )
        .await
        .is_err());
    assert_eq!(app.monitoring_sources().await.unwrap().revision, Some(2));
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multiple_named_exporters_per_host_source_are_supported_but_exact_duplicates_are_rejected()
{
    let (_root, app, host, source) = setup().await;
    let first = create(&app, 1, &host, &source, "node").await;
    let first_id = first.bindings[0].id.clone();
    let second = create(&app, 2, &host, &source, "cadvisor").await;
    assert_eq!(second.bindings.len(), 2);
    let before = raw(&app).await;
    assert!(app
        .monitoring_binding_create(
            3,
            "Another name".into(),
            host.clone(),
            source.clone(),
            1,
            labels("node")
        )
        .await
        .is_err());
    assert!(raw(&app).await == before);
    let edited = app
        .monitoring_binding_update(
            first_id.clone(),
            3,
            1,
            "Node explicit rereview".into(),
            host.clone(),
            source.clone(),
            1,
            labels("node"),
        )
        .await
        .unwrap();
    assert_eq!(edited.revision, Some(4));
    let row = edited
        .bindings
        .iter()
        .find(|row| row.id == first_id)
        .unwrap();
    assert_eq!(row.revision, 2);
    assert!(app
        .monitoring_binding_update(
            first_id,
            4,
            2,
            "Clobber second".into(),
            host,
            source,
            1,
            labels("cadvisor")
        )
        .await
        .is_err());
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_delete_cannot_orphan_bindings_and_removed_host_binding_is_explicitly_deletable() {
    let (_root, app, host, source) = setup().await;
    let current = create(&app, 1, &host, &source, "node").await;
    let id = current.bindings[0].id.clone();
    assert!(app
        .monitoring_source_delete(source.clone(), 2, 1)
        .await
        .is_err());
    app.delete_host(host).await.unwrap();
    let missing = app.monitoring_sources().await.unwrap();
    assert_eq!(
        missing.bindings[0].state,
        MonitoringBindingState::MissingHost
    );
    let (_, u) = app.unlocked().await.unwrap();
    assert!(binding_for_request(&u, &id, 1, 1).is_err());
    drop(u);
    assert!(app
        .monitoring_source_delete(source.clone(), 2, 1)
        .await
        .is_err());
    app.monitoring_binding_delete(id, 2, 1).await.unwrap();
    let empty = app.monitoring_source_delete(source, 3, 1).await.unwrap();
    assert_eq!(empty.schema_version, Some(2));
    assert!(empty.sources.is_empty() && empty.bindings.is_empty());
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_source_orphan_from_external_catalog_state_is_unavailable_but_explicitly_removable() {
    let (_root, app, host, source) = setup().await;
    let current = create(&app, 1, &host, &source, "node").await;
    let id = current.bindings[0].id.clone();
    let mut damaged = raw(&app).await;
    damaged["sources"].as_object_mut().unwrap().remove(&source);
    install_raw(&app, damaged).await;
    let view = app.monitoring_sources().await.unwrap();
    assert!(!view.read_only);
    assert_eq!(
        view.bindings[0].state,
        MonitoringBindingState::MissingSource
    );
    let (_, u) = app.unlocked().await.unwrap();
    assert!(binding_for_request(&u, &id, 1, 1).is_err());
    drop(u);
    let clean = app.monitoring_binding_delete(id, 2, 1).await.unwrap();
    assert!(clean.bindings.is_empty());
    assert_eq!(clean.revision, Some(3));
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn binding_count_budget_rejects_without_mutation_and_duplicate_raw_records_are_readonly() {
    let (_root, app, host, source) = setup().await;
    let current = create(&app, 1, &host, &source, "node").await;
    let id = current.bindings[0].id.clone();
    let mut value = raw(&app).await;
    let template = value["bindings"]["records"][&id].clone();
    let mut records = serde_json::Map::new();
    for index in 0..bindings::MAX_BINDINGS {
        let mut row = template.clone();
        row["name"] = json!(format!("Exporter {index}"));
        row["binding"]["selector"]["job"] = json!(format!("job{index}"));
        records.insert(Uuid::new_v4().to_string(), row);
    }
    value["bindings"]["records"] = Value::Object(records);
    install_raw(&app, value.clone()).await;
    assert_eq!(
        app.monitoring_sources().await.unwrap().bindings.len(),
        bindings::MAX_BINDINGS
    );
    assert!(app
        .monitoring_binding_create(2, "Exceeds count".into(), host, source, 1, labels("new"))
        .await
        .is_err());
    assert!(raw(&app).await == value);
    let mut duplicates = value;
    let records = duplicates["bindings"]["records"].as_object_mut().unwrap();
    records.clear();
    records.insert(Uuid::new_v4().to_string(), template.clone());
    records.insert(Uuid::new_v4().to_string(), template);
    install_raw(&app, duplicates.clone()).await;
    assert!(app.monitoring_sources().await.unwrap().read_only);
    assert!(raw(&app).await == duplicates);
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_update_preserves_bindings_but_requires_explicit_source_revision_rereview() {
    let (_root, app, host, source) = setup().await;
    let current = create(&app, 1, &host, &source, "node").await;
    let id = current.bindings[0].id.clone();
    let (_, u) = app.unlocked().await.unwrap();
    let old_plan = discovery_plan_for_binding(&u, &id, 1, 1, END).unwrap();
    drop(u);
    let mut config = current.sources[0].configuration.clone().unwrap();
    config.endpoint = "https://changed-fixture.example.invalid/prometheus".into();
    let changed = app
        .monitoring_source_update(source.clone(), 2, 1, config)
        .await
        .unwrap();
    assert_eq!(
        changed.bindings[0].state,
        MonitoringBindingState::StaleSource
    );
    assert_eq!(changed.bindings[0].source_revision, 1);
    assert_eq!(changed.bindings[0].revision, 1);
    let (_, u) = app.unlocked().await.unwrap();
    assert!(binding_for_request(&u, &id, 1, 1).is_err());
    assert!(resolve_discovery(
        &u,
        &old_plan,
        &source,
        1,
        END,
        &vector(vec![labels("node")])
    )
    .is_err());
    drop(u);
    let reviewed = app
        .monitoring_binding_update(
            id.clone(),
            3,
            1,
            "Node rereviewed".into(),
            host,
            source.clone(),
            2,
            labels("node"),
        )
        .await
        .unwrap();
    assert_eq!(reviewed.bindings[0].source_revision, 2);
    assert_eq!(reviewed.bindings[0].revision, 2);
    assert_eq!(reviewed.bindings[0].state, MonitoringBindingState::Current);
    let (_, u) = app.unlocked().await.unwrap();
    assert!(binding_for_request(&u, &id, 2, 2).is_ok());
    assert!(binding_for_request(&u, &id, 1, 2).is_err());
    drop(u);
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_versions_fields_and_legacy_bindings_null_are_readonly_and_generic_settings_preserve_raw(
) {
    let (_root, app, host, source) = setup().await;
    let legacy = raw(&app).await;
    let current = create(&app, 1, &host, &source, "node").await;
    let id = current.bindings[0].id.clone();
    let good = raw(&app).await;
    let mut cases = Vec::new();
    let mut legacy_null = legacy;
    legacy_null["bindings"] = Value::Null;
    cases.push(legacy_null);
    let mut future = good.clone();
    future["version"] = json!(3);
    cases.push(future);
    let mut future = good.clone();
    future["bindings"]["version"] = json!(2);
    cases.push(future);
    let mut future = good.clone();
    future["bindings"]["records"][&id]["futureField"] = json!(true);
    cases.push(future);
    let mut future = good.clone();
    future["bindings"]["records"][&id]["binding"]["version"] = json!(2);
    cases.push(future);
    let mut future = good.clone();
    future["unexpectedCatalogField"] = json!({"preserve":"raw"});
    cases.push(future);
    for unsupported in cases {
        install_raw(&app, unsupported.clone()).await;
        assert!(app.monitoring_sources().await.unwrap().read_only);
        assert!(app
            .monitoring_binding_delete(id.clone(), 2, 1)
            .await
            .is_err());
        assert!(app
            .monitoring_source_delete(source.clone(), 2, 1)
            .await
            .is_err());
        let mut old = app.get_vault_settings().await.unwrap();
        old.vault_name = "Generic old form".into();
        app.save_vault_settings(old).await.unwrap();
        assert!(raw(&app).await == unsupported);
    }
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bindings_require_native_host_ids_bounded_names_and_current_profile_unlock() {
    let (_root, app, host, source) = setup().await;
    for invalid_host in [
        Uuid::new_v4().to_string(),
        "host-fixture.example.invalid".into(),
        "127.0.0.1".into(),
    ] {
        assert!(app
            .monitoring_binding_create(
                1,
                "Bad host".into(),
                invalid_host,
                source.clone(),
                1,
                labels("node")
            )
            .await
            .is_err());
    }
    for name in [String::new(), "x".repeat(121), "line\nname".into()] {
        assert!(app
            .monitoring_binding_create(1, name, host.clone(), source.clone(), 1, labels("node"))
            .await
            .is_err());
    }
    app.lock().await.unwrap();
    assert!(app
        .monitoring_binding_create(
            1,
            "Locked".into(),
            host.clone(),
            source.clone(),
            1,
            labels("node")
        )
        .await
        .is_err());
    app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    assert!(app.monitoring_sources().await.unwrap().bindings.is_empty());
    app.create_local_profile("Another binding profile".into(), PASSWORD.into())
        .await
        .unwrap();
    assert!(app
        .monitoring_binding_create(1, "Wrong profile".into(), host, source, 1, labels("node"))
        .await
        .is_err());
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discovery_uses_scoped_up_at_selected_end_and_preserves_duplicate_row_ambiguity() {
    let (_root, app, host, source) = setup().await;
    let current = create(&app, 1, &host, &source, "node").await;
    let id = &current.bindings[0].id;
    let (_, u) = app.unlocked().await.unwrap();
    let plan = discovery_plan_for_binding(&u, id, 1, 1, END).unwrap();
    let scoped = "up{instance=\"synthetic:9100\",job=\"node\"}";
    assert_eq!(plan.expression(), scoped);
    assert_eq!(serde_json::to_value(&plan).unwrap()["expression"], scoped);
    assert_eq!(plan.selected_end(), END);
    assert_eq!(plan.source_id(), source);
    assert_eq!(plan.source_revision(), 1);
    assert_eq!(
        resolve_discovery(&u, &plan, &source, 1, END, &vector(vec![])).unwrap(),
        Resolution::Missing
    );
    assert_eq!(
        resolve_discovery(&u, &plan, &source, 1, END, &vector(vec![labels("node")])).unwrap(),
        Resolution::Resolved { candidate_index: 0 }
    );
    assert_eq!(
        resolve_discovery(
            &u,
            &plan,
            &source,
            1,
            END,
            &vector(vec![labels("node"), labels("node")])
        )
        .unwrap(),
        Resolution::Ambiguous {
            candidate_indices: vec![0, 1]
        }
    );
    assert_eq!(
        resolve_discovery(
            &u,
            &plan,
            &source,
            1,
            END,
            &vector(vec![labels("foreign"), labels("node"), labels("node")])
        )
        .unwrap(),
        Resolution::Ambiguous {
            candidate_indices: vec![1, 2]
        }
    );
    let claimed = BTreeMap::from([
        ("cc_host_id".into(), host),
        ("instance".into(), "host-fixture.example.invalid".into()),
    ]);
    assert_eq!(
        resolve_discovery(&u, &plan, &source, 1, END, &vector(vec![claimed])).unwrap(),
        Resolution::Missing
    );
    assert!(resolve_discovery(&u, &plan, "another-source", 1, END, &vector(vec![])).is_err());
    assert!(resolve_discovery(&u, &plan, &source, 2, END, &vector(vec![])).is_err());
    assert!(resolve_discovery(&u, &plan, &source, 1, END + 1.0, &vector(vec![])).is_err());
    drop(u);
    let injected = "node\"} or vector(1) #\\\n";
    app.monitoring_binding_update(
        id.clone(),
        2,
        1,
        "Escaped explicit selector".into(),
        current.bindings[0].binding.host_id().to_string(),
        source.clone(),
        1,
        labels(injected),
    )
    .await
    .unwrap();
    let (_, u) = app.unlocked().await.unwrap();
    assert!(resolve_discovery(&u, &plan, &source, 1, END, &vector(vec![labels("node")])).is_err());
    let escaped = discovery_plan_for_binding(&u, id, 2, 1, END).unwrap();
    let expected = format!(
        "up{{instance=\"synthetic:9100\",job={}}}",
        serde_json::to_string(injected).unwrap()
    );
    assert_eq!(escaped.expression(), expected);
    assert_eq!(
        serde_json::to_value(&escaped).unwrap()["expression"],
        expected
    );
    assert!(!escaped.expression().contains('\n'));
    drop(u);
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn partial_or_nonvector_or_stale_discovery_cannot_authorize_a_panel() {
    let (_root, app, host, source) = setup().await;
    let current = create(&app, 1, &host, &source, "node").await;
    let (_, u) = app.unlocked().await.unwrap();
    let id = &current.bindings[0].id;
    let plan = discovery_plan_for_binding(&u, id, 1, 1, END).unwrap();
    let good = vector(vec![labels("node")]);
    let mut cases = Vec::new();
    let mut bad = good.clone();
    bad.partial = true;
    cases.push(bad);
    let mut foreign_partial = vector(vec![labels("foreign")]);
    foreign_partial.partial = true;
    cases.push(foreign_partial);
    let mut bad = good.clone();
    bad.warnings.push("partial discovery".into());
    cases.push(bad);
    let mut bad = good.clone();
    bad.http_status = 403;
    cases.push(bad);
    let mut bad = good.clone();
    if let Some(QueryData::Vector { series }) = &mut bad.data {
        series[0]
            .labels
            .insert("__name__".into(), "another_metric".into());
    }
    cases.push(bad);
    let mut bad = good.clone();
    bad.data = Some(QueryData::Matrix { series: vec![] });
    cases.push(bad);
    let mut bad = good.clone();
    if let Some(QueryData::Vector { series }) = &mut bad.data {
        series[0].samples[0].value = SampleValue::NaN;
    }
    cases.push(bad);
    let mut bad = good.clone();
    if let Some(QueryData::Vector { series }) = &mut bad.data {
        series[0].samples[0].timestamp = END - 10.0;
    }
    cases.push(bad);
    for bad in cases {
        assert!(resolve_discovery(&u, &plan, &source, 1, END, &bad).is_err());
    }
    assert!(discovery_plan_for_binding(&u, id, 1, 1, f64::NAN).is_err());
    assert!(discovery_plan_for_binding(&u, id, 1, 1, 253_402_300_800.0).is_err());
    let mut down = good;
    if let Some(QueryData::Vector { series }) = &mut down.data {
        series[0].samples[0].value = SampleValue::Finite { value: 0.0 };
    }
    assert!(matches!(
        resolve_discovery(&u, &plan, &source, 1, END, &down).unwrap(),
        Resolution::Resolved { .. }
    ));
    drop(u);
    app.shutdown().await.unwrap();
}
