use super::*;
fn record() -> Record {
    Record {
        version: 1,
        revision: 1,
        name: "Owned reference".into(),
        source_kind: "snapshot".into(),
        revision_digest: "a".repeat(64),
        files: 1,
        bytes: 42,
        git: None,
        inventory: cc_automation_core::ProjectInventory::default(),
        execution_credentials: None,
    }
}
#[test]
fn strict_portable_metadata_has_no_root_file_body_or_authority_fields() {
    let mut value = serde_json::to_value(record()).unwrap();
    for field in [
        "path",
        "local_root",
        "template",
        "extra_vars",
        "secret",
        "runner_launch",
        "approved",
    ] {
        value[field] = json!("must-not-be-imported");
        assert!(serde_json::from_value::<Record>(value.clone()).is_err());
        value.as_object_mut().unwrap().remove(field);
    }
    record().validate().unwrap();
}
#[test]
fn unknown_schema_and_excess_budget_are_readonly_not_migrated() {
    let mut raw = json!({"version":9,"revision":1,"records":{},"future":"retain"});
    let before = raw.clone();
    assert!(parse(Some(&raw)).is_err());
    assert_eq!(raw, before);
    raw = json!({"version":1,"revision":0,"records":{},"future":null});
    assert!(parse(Some(&raw)).is_err());
    raw = json!({"version":1,"revision":0,"records":{},"large":"x".repeat(MAX_BYTES)});
    assert!(parse(Some(&raw)).is_err());
}
#[test]
fn git_links_reject_embedded_credentials_queries_and_non_https() {
    let mut v = record();
    v.source_kind = "git".into();
    for url in [
        "https://user:password@example.invalid/repo.git",
        "file:///tmp/project",
        "ssh://example.invalid/repo",
        "https://example.invalid/repo?token=secret",
    ] {
        v.git = Some(GitReference {
            url: url.into(),
            reference: "main".into(),
            commit: "a".repeat(40),
        });
        assert!(v.validate().is_err());
    }
    v.git = Some(GitReference {
        url: "https://example.invalid/repo.git".into(),
        reference: "main".into(),
        commit: "a".repeat(40),
    });
    v.validate().unwrap();
}
#[test]
fn configured_reference_policy_keeps_same_credential_reuse_and_rejects_secret_fields() {
    let mut v = record();
    let host = Uuid::new_v4();
    let credential = Uuid::new_v4();
    v.execution_credentials = Some(
        json!({"version":1,"environments":[{"id":Uuid::new_v4(),"name":"Owned environment","production":true,
        "overrides":[{"host_id":host,"credential_id":credential}]}]}),
    );
    v.validate().unwrap();
    assert_eq!(references(&v).unwrap().len(), 2);
    v.execution_credentials.as_mut().unwrap()["password"] = json!("synthetic-forbidden");
    assert!(v.validate().is_err());
}
#[tokio::test]
async fn future_automation_raw_survives_real_generic_settings_edit_without_local_project_creation()
{
    let directory = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        directory.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile(
        "Owned raw preservation".into(),
        "owned encrypted settings phrase42".into(),
    )
    .await
    .unwrap();
    let raw = json!({"version":99,"future":[{"opaque":"owned future value"}]});
    let (_, u) = app.unlocked().await.unwrap();
    let mut current = settings(&u).unwrap();
    current.automation = Some(raw.clone());
    u.writer
        .put(VaultObject::VaultSettings(current))
        .await
        .unwrap();
    drop(u);
    let mut dto = app.get_vault_settings().await.unwrap();
    dto.vault_name = "Renamed without schema rewrite".into();
    app.save_vault_settings(dto).await.unwrap();
    let (_, u) = app.unlocked().await.unwrap();
    assert_eq!(settings(&u).unwrap().automation, Some(raw));
    drop(u);
    // Writer returns after immediate readback, while the SAME ordered event
    // task may still be applying earlier settings readbacks. Its unique last
    // Host event is a causal FIFO barrier, not a sleep/retry of a retired read.
    let mut applied = app.subscribe_events();
    let sentinel = app
        .save_host(crate::HostDto::new(
            "Owned metadata queue sentinel",
            "127.0.0.1",
        ))
        .await
        .unwrap()
        .id;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let crate::AppEvent::ObjectsChanged { ids, .. } = applied.recv().await.unwrap() {
                if ids.contains(&sentinel) {
                    break;
                }
            }
        }
    })
    .await
    .unwrap();
    let observed = app.automation_project_metadata_catalog().await.unwrap();
    assert_eq!(observed["catalog"]["read_only"], true);
    assert!(
        observed["catalog"]["records"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(app.automation_projects().await.unwrap().projects.is_empty());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn real_same_revision_project_setting_change_fails_final_sql_guard_without_a_metadata_write()
{
    let directory = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        directory.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile(
        "Owned final metadata guard".into(),
        "owned final metadata phrase42".into(),
    )
    .await
    .unwrap();
    let project = app
        .automation_project_create("Original source".into(), None)
        .await
        .unwrap();
    let context = app.automation_project_context(&project.id).await.unwrap();
    let storage = context.owner.storage().unwrap();
    let original = project.id.clone();
    let expected = context.catalog_row_sha256.clone();
    storage
        .read(move |tx| check_project_tx(tx, &original, &expected))
        .await
        .unwrap();
    let id = project.id.clone();
    storage
        .write(move |tx| {
            let mut value: Catalog = tx.setting_get(PROJECTS)?.unwrap();
            value.records.get_mut(&id).unwrap()["name"] = json!("Different same-revision body");
            tx.setting_set(PROJECTS, &value)
        })
        .await
        .unwrap();
    let id = project.id;
    let expected = context.catalog_row_sha256.clone();
    assert!(
        storage
            .read(move |tx| check_project_tx(tx, &id, &expected))
            .await
            .is_err()
    );
    let (_, u) = app.unlocked().await.unwrap();
    assert!(settings(&u).unwrap().automation.is_none());
    drop(u);
    app.shutdown().await.unwrap();
}
