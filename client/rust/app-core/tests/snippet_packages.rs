//! Package membership and starter provenance travel inside encrypted snippets.
mod common;

use cc_app_core::*;
use cc_sync_core::mock::MockServer;
use common::*;

fn snippet(package: &str, catalog: &str) -> SnippetDto {
    SnippetDto {
        id: String::new(),
        name: "Disk space".into(),
        description: String::new(),
        package_name: Some(package.into()),
        catalog_id: Some(catalog.into()),
        snippet_type: SnippetType::Shell,
        shell: Some("sh".into()),
        template: "df -h".into(),
        variables: vec![],
        tags: vec![],
        risk_level: RiskLevel::ReadOnly,
        source: SnippetSource::Imported,
        created_by_device_id: None,
        last_used_at_ms: None,
        usage_count: 0,
        created_at_ms: 0,
        updated_at_ms: 0,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn packages_and_catalog_identity_sync_with_edits_and_deletion() {
    let server = MockServer::start().await;
    let tmp = tempfile::tempdir().unwrap();
    let a = app(&tmp.path().join("a"));
    a.create_synced_profile(
        "A".into(),
        server.url().to_string(),
        "snippets@example.org".into(),
        ACCOUNT_PASSWORD.into(),
        AccountMode::Register,
    )
    .await
    .unwrap();
    let vault = a
        .create_vault(PASSPHRASE.into())
        .await
        .unwrap()
        .vault_id
        .clone();
    let saved = a
        .save_snippet(snippet("Linux", "consolecrypt.linux.disk.v1"))
        .await
        .unwrap();
    a.sync_now().await.unwrap();
    let b = app(&tmp.path().join("b"));
    b.create_synced_profile(
        "B".into(),
        server.url().to_string(),
        "snippets@example.org".into(),
        ACCOUNT_PASSWORD.into(),
        AccountMode::Login,
    )
    .await
    .unwrap();
    b.join_vault_with_passphrase(vault, PASSPHRASE.into())
        .await
        .unwrap();
    wait_until!(
        10,
        "starter snippet arrives",
        b.list_snippets().await.unwrap().len() == 1
    );
    let mut remote = b.list_snippets().await.unwrap().remove(0);
    assert_eq!(remote.package_name.as_deref(), Some("Linux"));
    assert_eq!(
        remote.catalog_id.as_deref(),
        Some("consolecrypt.linux.disk.v1")
    );
    remote.package_name = Some("My diagnostics".into());
    remote.template = "df -h /".into();
    b.save_snippet(remote).await.unwrap();
    b.sync_now().await.unwrap();
    wait_until!(10, "package rename and edit arrive", a.list_snippets().await.unwrap().iter()
        .any(|s| s.package_name.as_deref() == Some("My diagnostics") && s.template == "df -h /"));
    assert_eq!(
        a.list_snippets().await.unwrap()[0].catalog_id.as_deref(),
        Some("consolecrypt.linux.disk.v1")
    );
    b.delete_snippet(saved.id).await.unwrap();
    b.sync_now().await.unwrap();
    wait_until!(
        10,
        "deletion arrives",
        a.list_snippets().await.unwrap().is_empty()
    );
    a.lock().await.unwrap();
    a.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert!(
        a.list_snippets().await.unwrap().is_empty(),
        "deleted starters must not reappear on unlock"
    );
    a.shutdown().await.unwrap();
    b.shutdown().await.unwrap();
}

#[tokio::test]
async fn package_metadata_is_local_when_sync_is_disabled_and_survives_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let store = memory_store();
    let a = app_with(tmp.path(), store.clone(), false);
    let pid = a
        .create_local_profile("Local".into(), PASSPHRASE.into())
        .await
        .unwrap()
        .profile
        .id;
    a.save_snippet(snippet(" Linux ", "consolecrypt.linux.disk.v1"))
        .await
        .unwrap();
    a.shutdown().await.unwrap();
    let b = app_with(tmp.path(), store, false);
    b.open_profile(pid).await.unwrap();
    b.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    let s = &b.list_snippets().await.unwrap()[0];
    assert_eq!(s.package_name.as_deref(), Some("Linux"));
    assert_eq!(s.catalog_id.as_deref(), Some("consolecrypt.linux.disk.v1"));
    b.shutdown().await.unwrap();
}
