use super::*;
fn marked(profile: &std::path::Path, id: &str) -> Value {
    let transaction = Uuid::new_v4();
    json!({"version":1,"name":"Own fixture","revision":2,
        "source":{"kind":"managed","path":profile.join("automation").join("project-revisions").join(id).join(format!("generation-{transaction}"))},
        "publication":PublicationMarker{version:1,transaction,operation:Uuid::new_v4(),installed_revision:2,
            base_digest:"a".repeat(64),candidate_digest:"b".repeat(64),review_digest:"c".repeat(64),previous_row_sha256:"d".repeat(64),publication_fingerprint:"e".repeat(64)}})
}
#[test]
fn strict_marker_profile_project_and_transaction_geometry_without_authority_or_digest_lock() {
    let own = tempfile::tempdir().unwrap();
    let id = Uuid::new_v4().to_string();
    let row = marked(own.path(), &id);
    assert!(supported_at(Some(own.path()), &id, &row));
    let mut edited = row.clone();
    edited["revision"] = json!(3); // ordinary later metadata/file editing is allowed
    assert!(supported_at(Some(own.path()), &id, &edited));
    assert!(!supported_at(None, &id, &row));
    assert!(!supported_at(
        Some(own.path()),
        &Uuid::new_v4().to_string(),
        &row
    ));
    let foreign = tempfile::tempdir().unwrap();
    assert!(!supported_at(Some(foreign.path()), &id, &row));
    for (pointer, value) in [
        ("/publication/version", json!(2)),
        ("/publication/version", json!(1.0)),
        ("/publication/installedRevision", json!(3)),
        ("/publication/transaction", json!(Uuid::new_v4())),
        (
            "/publication/candidateDigest",
            json!("not-an-archive-witness"),
        ),
        ("/publication/approved", json!(true)),
        ("/source/kind", json!("directory")),
        ("/source/path", json!(own.path().join("other"))),
    ] {
        let mut changed = row.clone();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        changed.pointer_mut(parent).unwrap()[key] = value;
        assert!(!supported_at(Some(own.path()), &id, &changed));
    }
    let mut legacy = row.clone();
    legacy.as_object_mut().unwrap().remove("publication");
    assert!(!supported_at(Some(own.path()), &id, &legacy));
    legacy["source"]["path"] = json!(own.path().join("automation").join("projects").join(&id));
    assert!(supported_at(Some(own.path()), &id, &legacy));
}

async fn fixture() -> (tempfile::TempDir, AppCore, String) {
    let root = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        root.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile(
        "Owned path fixture".into(),
        "own path fixture password 42".into(),
    )
    .await
    .unwrap();
    let id = app
        .automation_project_create("Managed fixture".into(), None)
        .await
        .unwrap()
        .id;
    app.automation_project_save(
        id.clone(),
        "original.yml".into(),
        None,
        "synthetic original\n".into(),
    )
    .await
    .unwrap();
    (root, app, id)
}

#[tokio::test]
async fn future_marker_denies_all_selected_row_writers_and_loaders_preserving_raw_catalog() {
    let (_root, app, id) = fixture().await;
    let context = app.automation_project_context(&id).await.unwrap();
    let original = std::fs::read(context.path.join("original.yml")).unwrap();
    let (owner, u) = app.unlocked().await.unwrap();
    let scope = u.notices.scope_id().unwrap();
    drop(u);
    let selected = id.clone();
    let raw = owner
        .storage()
        .unwrap()
        .write(move |tx| -> AppResult<Value> {
            let mut catalog: Catalog = tx.setting_get(PROJECTS)?.unwrap();
            catalog.records.get_mut(&selected).unwrap()["publication"] =
                json!({"version":2,"unknown":"preserve"});
            tx.setting_set(PROJECTS, &catalog)?;
            Ok(serde_json::to_value(catalog).unwrap())
        })
        .await
        .unwrap();
    assert!(app.automation_project_context(&id).await.is_err());
    assert!(app
        .automation_project_rename(id.clone(), 1, "Denied".into())
        .await
        .is_err());
    assert!(app
        .automation_project_save_inventory(id.clone(), 1, "{\"version\":1,\"groups\":{}}".into())
        .await
        .is_err());
    assert!(app.automation_inventory_clear(id.clone(), 1).await.is_err());
    assert!(app.automation_project_folder_binding_json(json!({"version":1,"project_id":id,"expected_revision":1,
        "profile_id":owner.profile_id.to_string(),"notification_scope_id":scope,"action":{"type":"unlink","name":"group"}}).to_string()).await.is_err());
    assert!(app.automation_project_files(id.clone()).await.is_err());
    assert!(app
        .automation_project_file(id.clone(), "original.yml".into())
        .await
        .is_err());
    assert!(app
        .automation_project_save(
            id.clone(),
            "original.yml".into(),
            None,
            "never written".into()
        )
        .await
        .is_err());
    assert!(app
        .automation_project_create_role(id.clone(), "never-created".into())
        .await
        .is_err());
    assert!(app
        .automation_project_open_editor(id, "original.yml".into(), "a".repeat(64), None)
        .await
        .is_err());
    assert_eq!(
        owner
            .storage()
            .unwrap()
            .setting_get::<Value>(PROJECTS)
            .await
            .unwrap()
            .unwrap(),
        raw
    );
    assert_eq!(
        std::fs::read(context.path.join("original.yml")).unwrap(),
        original
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn queued_real_sql_switch_after_capture_denies_io_admission_before_any_file_action() {
    use std::{future::Future, task::Poll, time::Duration};
    let (_root, app, id) = fixture().await;
    let context = app.automation_project_context(&id).await.unwrap();
    let original = std::fs::read(context.path.join("original.yml")).unwrap();
    let owner = context.owner.clone();
    let storage = owner.storage().unwrap().clone();
    let selected = id.clone();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let sql = tokio::spawn(async move {
        storage
            .write(move |tx| -> AppResult<()> {
                let mut catalog: Catalog = tx.setting_get(PROJECTS)?.unwrap();
                let row = catalog.records.get_mut(&selected).unwrap();
                row["revision"] = json!(2);
                row["name"] = json!("actual queued change");
                tx.setting_set(PROJECTS, &catalog)?;
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                Ok(())
            })
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), entered_rx)
        .await
        .unwrap()
        .unwrap();
    let mut admission = Box::pin(admit_io(&app, &context));
    std::future::poll_fn(|cx| match admission.as_mut().poll(cx) {
        Poll::Pending => Poll::Ready(()),
        Poll::Ready(_) => panic!("actual read must queue behind occupied SQL worker"),
    })
    .await;
    assert_eq!(owner.automation.io_slots.available_permits(), 3);
    release_tx.send(()).unwrap();
    sql.await.unwrap().unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(5), &mut admission)
        .await
        .unwrap()
        .is_err());
    drop(admission);
    assert_eq!(owner.automation.io_slots.available_permits(), 4);
    assert_eq!(
        std::fs::read(context.path.join("original.yml")).unwrap(),
        original
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn original_scope_not_latest_same_profile_and_retained_io_blocks_publication_capacity() {
    let (_root, app, id) = fixture().await;
    let context = app.automation_project_context(&id).await.unwrap();
    let admitted = admit_io(&app, &context).await.unwrap();
    assert!(context
        .owner
        .automation
        .io_slots
        .clone()
        .try_acquire_many_owned(crate::automation_runners::AUTOMATION_IO_CAPACITY)
        .is_err());
    let worker = admitted.clone();
    drop(admitted);
    assert_eq!(context.owner.automation.io_slots.available_permits(), 3);
    drop(worker);
    assert_eq!(context.owner.automation.io_slots.available_permits(), 4);
    app.lock().await.unwrap();
    assert!(admit_io(&app, &context).await.is_err());
    assert_eq!(context.owner.automation.io_slots.available_permits(), 4);
    app.shutdown().await.unwrap();
}
