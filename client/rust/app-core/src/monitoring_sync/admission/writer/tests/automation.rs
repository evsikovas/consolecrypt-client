//! Actual crypto/sync with a clearly synthetic test-only policy response.
//! The separately authored owned HTTP/TestDb canary covers real server opt-in.
use super::*;
const NAME: &str = "owned-private-automation-metadata-reference";
async fn readback_barrier(app: &AppCore) {
    // sync_now joins the sync worker, not the separate FIFO object-readback
    // task. A real harmless local object queues behind those readbacks. Wait
    // for its exact event before measuring a supposedly idle metadata read.
    // This changes no metadata, policy, read guard or production behavior.
    let mut events = app.subscribe_events();
    let host = cc_models::host::Host::new("Owned metadata read barrier", "192.0.2.29");
    let id = host.id.to_string();
    let (_, u) = app.unlocked().await.unwrap();
    u.writer
        .put(cc_models::VaultObject::Host(host))
        .await
        .unwrap();
    drop(u);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let crate::AppEvent::ObjectsChanged {
                kind: cc_models::ObjectKind::Host,
                origin: crate::ChangeOriginDto::Local,
                ids,
            } = events.recv().await.unwrap()
            {
                if ids.contains(&id) {
                    break;
                }
            }
        }
    })
    .await
    .unwrap();
}
async fn project(app: &AppCore) -> String {
    let project = app
        .automation_project_create(NAME.into(), None)
        .await
        .unwrap();
    app.automation_project_save(
        project.id.clone(),
        "site.yml".into(),
        None,
        "---\n- hosts: all\n  tasks: []\n# file body must not sync as metadata\n".into(),
    )
    .await
    .unwrap();
    project.id
}
async fn request(app: &AppCore, id: &str, expected: u64) -> String {
    let context = app
        .automation_project_metadata_context(id.into())
        .await
        .unwrap();
    let mut frame = context["frame"].clone();
    frame["expected_reference_revision"] = json!(expected);
    frame.to_string()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn automation_metadata_uses_same_ciphertext_and_second_device_stays_rootless_readonly() {
    let f = Fixture::new().await;
    let project = project(&f.a).await;
    f.enabled(&f.a).await;
    let input = request(&f.a, &project, 0).await;
    let result = f
        .a
        .automation_project_metadata_publish_with_policy(input, PanelWritePolicy::ReviewedCandidate)
        .await
        .unwrap();
    assert_eq!(result["catalog"]["records"].as_array().unwrap().len(), 1);
    let row = &result["catalog"]["records"][0];
    assert_eq!(row["runnable"], false);
    assert_eq!(row["local_root_bound"], false);
    // Managed creation retains its scaffold playbook.yml plus our site.yml.
    assert_eq!(row["metadata"]["files"], 2);
    assert!(row["metadata"].get("path").is_none());
    assert!(!row.to_string().contains("file body must not sync"));
    f.a.sync_now().await.unwrap();
    let (_, u) = f.a.unlocked().await.unwrap();
    let id = catalog_checkpoint(&u).unwrap().settings_id;
    let vault = u.vault_id;
    drop(u);
    ciphertext(&f.server, vault, id, NAME);
    let b = f.login("automation-reader-b", "2.0.0").await;
    readback_barrier(&b).await;
    let received = b.automation_project_metadata_catalog().await.unwrap();
    assert_eq!(received["catalog"], result["catalog"]);
    assert!(b.automation_projects().await.unwrap().projects.is_empty());
    let mut dto = b.get_vault_settings().await.unwrap();
    dto.vault_name = "Metadata preserved by ordinary settings".into();
    b.save_vault_settings(dto).await.unwrap();
    b.sync_now().await.unwrap();
    f.a.sync_now().await.unwrap();
    readback_barrier(&f.a).await;
    assert_eq!(
        f.a.automation_project_metadata_catalog().await.unwrap()["catalog"],
        result["catalog"]
    );
    assert_eq!(f.a.automation_projects().await.unwrap().projects.len(), 1);
    b.shutdown().await.unwrap();
    f.a.shutdown().await.unwrap();
}
#[tokio::test]
async fn public_operator_off_preserves_ciphertext_and_does_not_backfill_local_rows() {
    let f = Fixture::new().await;
    let id = project(&f.a).await;
    let input = request(&f.a, &id, 0).await;
    let (owner, u) = f.a.unlocked().await.unwrap();
    let before = settings_raw(&u);
    drop(u);
    let count = f.server.latest_sequence(owner.require_vault_id().unwrap());
    drop(owner);
    assert!(f
        .a
        .automation_project_metadata_publish(input)
        .await
        .is_err());
    let (owner, u) = f.a.unlocked().await.unwrap();
    assert_eq!(settings_raw(&u), before);
    assert_eq!(
        f.server.latest_sequence(owner.require_vault_id().unwrap()),
        count
    );
    drop(u);
    drop(owner);
    assert!(
        f.a.automation_project_metadata_catalog().await.unwrap()["catalog"]["records"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    f.a.shutdown().await.unwrap();
}
fn settings_raw(u: &crate::session::Unlocked) -> Option<Value> {
    u.working()
        .all_vault_settings()
        .into_iter()
        .max_by_key(|s| s.updated_at)
        .unwrap()
        .automation
}
#[tokio::test]
async fn stale_original_settings_head_cannot_replace_synced_reference_metadata() {
    let f = Fixture::new().await;
    let id = project(&f.a).await;
    let stale = request(&f.a, &id, 0).await;
    f.enabled(&f.a).await;
    f.a.automation_project_metadata_publish_with_policy(
        stale.clone(),
        PanelWritePolicy::ReviewedCandidate,
    )
    .await
    .unwrap();
    let before = f.a.automation_project_metadata_catalog().await.unwrap();
    assert!(f
        .a
        .automation_project_metadata_publish_with_policy(stale, PanelWritePolicy::ReviewedCandidate)
        .await
        .is_err());
    assert_eq!(
        f.a.automation_project_metadata_catalog().await.unwrap(),
        before
    );
    f.a.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stale_generic_edit_with_legacy_absence_preserves_actual_remote_automation() {
    let f = Fixture::new().await;
    let b = f.login("legacy-absence-b", "2.0.0").await;
    let id = project(&f.a).await;
    f.enabled(&f.a).await;
    let expected =
        f.a.automation_project_metadata_publish_with_policy(
            request(&f.a, &id, 0).await,
            PanelWritePolicy::ReviewedCandidate,
        )
        .await
        .unwrap();
    f.a.sync_now().await.unwrap();
    let mut dto = b.get_vault_settings().await.unwrap();
    dto.vault_name = "Offline generic edit with absent automation".into();
    b.save_vault_settings(dto).await.unwrap();
    b.sync_now().await.unwrap();
    f.a.sync_now().await.unwrap();
    assert_eq!(
        b.automation_project_metadata_catalog().await.unwrap()["catalog"],
        expected["catalog"]
    );
    assert_eq!(
        f.a.automation_project_metadata_catalog().await.unwrap()["catalog"],
        expected["catalog"]
    );
    assert_eq!(
        f.a.get_vault_settings().await.unwrap().vault_name,
        "Offline generic edit with absent automation"
    );
    b.shutdown().await.unwrap();
    f.a.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn different_nonnull_catalogs_preserve_both_ciphertexts_and_expose_existing_conflict_readonly(
) {
    let f = Fixture::new().await;
    let id = project(&f.a).await;
    f.enabled(&f.a).await;
    f.a.automation_project_metadata_publish_with_policy(
        request(&f.a, &id, 0).await,
        PanelWritePolicy::ReviewedCandidate,
    )
    .await
    .unwrap();
    f.a.sync_now().await.unwrap();
    let b = f.login("divergent-b", "2.0.0").await;
    let file =
        f.a.automation_project_file(id.clone(), "site.yml".into())
            .await
            .unwrap();
    f.a.automation_project_save(
        id.clone(),
        file.path,
        Some(file.sha256),
        file.text + "# actual new source revision\n",
    )
    .await
    .unwrap();
    f.enabled(&f.a).await;
    let newest =
        f.a.automation_project_metadata_publish_with_policy(
            request(&f.a, &id, 1).await,
            PanelWritePolicy::ReviewedCandidate,
        )
        .await
        .unwrap();
    f.a.sync_now().await.unwrap();
    let (_, u) = f.a.unlocked().await.unwrap();
    let settings = catalog_checkpoint(&u).unwrap().settings_id;
    let vault = u.vault_id;
    drop(u);
    let server_before = f.server.object(vault, settings).unwrap();
    let mut dto = b.get_vault_settings().await.unwrap();
    dto.vault_name = "Generic edit from stale nonnull snapshot".into();
    b.save_vault_settings(dto).await.unwrap();
    assert!(b.sync_now().await.is_err());
    let server_after = f.server.object(vault, settings).unwrap();
    assert_eq!(server_after.revision, server_before.revision);
    assert_eq!(server_after.body, server_before.body);
    let seen = b.automation_project_metadata_catalog().await.unwrap();
    assert_eq!(seen["catalog"]["read_only"], true);
    assert_eq!(seen["catalog"]["sync_state"], "reconciliation_required");
    assert_eq!(
        f.a.automation_project_metadata_catalog().await.unwrap()["catalog"],
        newest["catalog"]
    );
    let (_, u) = b.unlocked().await.unwrap();
    assert_eq!(
        u.store()
            .stored(settings)
            .await
            .unwrap()
            .unwrap()
            .local_state,
        cc_storage_core::LocalState::Conflict
    );
    drop(u);
    b.shutdown().await.unwrap();
    f.a.shutdown().await.unwrap();
}
