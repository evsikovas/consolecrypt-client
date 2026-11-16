//! Negative metadata/lifecycle boundaries. These tests never construct a Ready,
//! Core publisher or archive witness. Positive selected-generation audit uses
//! the separately explicit actual SSH/TLS publication fixture.
use super::super::tests::{Fixture, serial};
use super::*;
use std::{future::Future, task::Poll};

async fn catalog(f: &Fixture) -> Value {
    f.app
        .unlocked()
        .await
        .unwrap()
        .0
        .storage()
        .unwrap()
        .setting_get(PROJECTS)
        .await
        .unwrap()
        .unwrap()
}
/// Deliberately forged SQL DATA only: it MUST NOT become a filesystem witness.
async fn metadata_only(f: &Fixture) -> Arc<AutomationProjectContext> {
    let owner = f.app.unlocked().await.unwrap().0;
    let project = f.project.clone();
    let txid = Uuid::new_v4();
    let target = owner
        .dir
        .as_ref()
        .unwrap()
        .join("automation")
        .join("project-revisions")
        .join(&project)
        .join(format!("generation-{txid}"));
    let marker = managed::PublicationMarker {
        version: 1,
        transaction: txid,
        operation: Uuid::new_v4(),
        installed_revision: 2,
        base_digest: "a".repeat(64),
        candidate_digest: "b".repeat(64),
        review_digest: "c".repeat(64),
        previous_row_sha256: "d".repeat(64),
        publication_fingerprint: "e".repeat(64),
    };
    owner
        .storage()
        .unwrap()
        .write(move |tx| -> AppResult<()> {
            let mut value: Value = tx.setting_get(PROJECTS)?.unwrap();
            let row = &mut value["records"][&project];
            row["revision"] = json!(2);
            row["source"]["path"] = json!(target.to_string_lossy());
            row["publication"] = serde_json::to_value(marker).unwrap();
            tx.setting_set(PROJECTS, &value)?;
            Ok(())
        })
        .await
        .unwrap();
    Arc::new(f.app.automation_project_context(&f.project).await.unwrap())
}

#[tokio::test]
async fn unselected_legacy_and_stale_inputs_do_not_create_namespace_or_leak_io() {
    let _serial = serial().lock().await;
    let f = Fixture::new().await;
    let context = f.app.automation_project_context(&f.project).await.unwrap();
    let before = catalog(&f).await;
    let original = ProjectRoot::open(&context.path)
        .unwrap()
        .revision()
        .unwrap();
    let family = context
        .owner
        .dir
        .as_ref()
        .unwrap()
        .join("automation")
        .join("project-revisions");
    assert!(!family.exists());
    for (id, revision) in [
        (f.project.clone(), 0),
        (f.project.clone(), 1),
        (f.project.clone(), 2),
        ("not-native-uuid".into(), 2),
    ] {
        assert!(
            f.app
                .automation_dependency_reconcile_selected(id, revision)
                .await
                .is_err()
        );
    }
    assert_eq!(catalog(&f).await, before);
    assert_eq!(
        ProjectRoot::open(&context.path)
            .unwrap()
            .revision()
            .unwrap(),
        original
    );
    assert_eq!(context.owner.automation.io_slots.available_permits(), 4);
    assert!(!family.exists());
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn structurally_valid_sql_marker_is_not_a_witness_or_permission_to_create_generation() {
    let _serial = serial().lock().await;
    let f = Fixture::new().await;
    let context = metadata_only(&f).await;
    let before = catalog(&f).await;
    // The pure same-tx metadata capture succeeds, proving the later denial is
    // the genuine Core admission check, not malformed JSON.
    let selected = capture_selected(&f.app, &context, 2).await.unwrap();
    assert_eq!(selected.marker.installed_revision, 2);
    let family = context
        .owner
        .dir
        .as_ref()
        .unwrap()
        .join("automation")
        .join("project-revisions");
    assert!(!family.exists());
    assert!(
        f.app
            .automation_dependency_reconcile_selected(f.project.clone(), 2)
            .await
            .is_err()
    );
    context.owner.automation.dependencies.close_and_wait().await;
    assert!(!family.exists());
    assert_eq!(catalog(&f).await, before);
    assert_eq!(context.owner.automation.io_slots.available_permits(), 4);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn future_marker_and_foreign_path_remain_readonly_without_fallback() {
    let _serial = serial().lock().await;
    for cause in ["future", "path", "revision"] {
        let f = Fixture::new().await;
        let context = metadata_only(&f).await;
        let project = f.project.clone();
        context
            .owner
            .storage()
            .unwrap()
            .write(move |tx| -> AppResult<()> {
                let mut value: Value = tx.setting_get(PROJECTS)?.unwrap();
                let row = &mut value["records"][&project];
                match cause {
                    "future" => row["publication"]["futureExtension"] = json!(true),
                    "path" => row["source"]["path"] = json!("caller-supplied-foreign-generation"),
                    _ => row["revision"] = json!(3),
                }
                tx.setting_set(PROJECTS, &value)?;
                Ok(())
            })
            .await
            .unwrap();
        let before = catalog(&f).await;
        assert!(
            f.app
                .automation_dependency_reconcile_selected(f.project.clone(), 2)
                .await
                .is_err()
        );
        assert_eq!(catalog(&f).await, before);
        assert_eq!(context.owner.automation.io_slots.available_permits(), 4);
        f.app.shutdown().await.unwrap();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn selected_capture_reads_current_row_inside_real_queued_sql_transaction() {
    let _serial = serial().lock().await;
    let f = Fixture::new().await;
    let context = metadata_only(&f).await;
    assert!(capture_selected(&f.app, &context, 2).await.is_ok());
    let storage = context.owner.storage().unwrap().clone();
    let project = f.project.clone();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let blocker = tokio::spawn(async move {
        storage
            .write(move |tx| -> AppResult<Value> {
                let mut value: Value = tx.setting_get(PROJECTS)?.unwrap();
                // Same revision is insufficient: original exact row SHA must fail.
                value["records"][&project]["name"] =
                    json!("actual queued equal-revision metadata edit");
                tx.setting_set(PROJECTS, &value)?;
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                Ok(value)
            })
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), entered_rx)
        .await
        .unwrap()
        .unwrap();
    let mut read = Box::pin(capture_selected(&f.app, &context, 2));
    std::future::poll_fn(|cx| match read.as_mut().poll(cx) {
        Poll::Pending => Poll::Ready(()),
        Poll::Ready(_) => panic!("same transaction read must queue behind actual SQL mutation"),
    })
    .await;
    release_tx.send(()).unwrap();
    let current = blocker.await.unwrap().unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(5), &mut read)
            .await
            .unwrap()
            .is_err()
    );
    drop(read);
    assert_eq!(catalog(&f).await, current);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn old_original_scope_does_not_revive_after_real_lock_and_reunlock() {
    let _serial = serial().lock().await;
    let f = Fixture::new().await;
    let context = metadata_only(&f).await;
    assert!(capture_selected(&f.app, &context, 2).await.is_ok());
    let epoch = context
        .owner
        .automation
        .dependencies
        .local_filesystem_epoch()
        .unwrap();
    f.app.lock().await.unwrap();
    assert!(epoch.is_cancelled());
    assert!(capture_selected(&f.app, &context, 2).await.is_err());
    f.app
        .unlock_with_passphrase("owned dependency fixture passphrase 42".into())
        .await
        .unwrap();
    assert!(capture_selected(&f.app, &context, 2).await.is_err());
    let fresh = Arc::new(f.app.automation_project_context(&f.project).await.unwrap());
    assert!(capture_selected(&f.app, &fresh, 2).await.is_ok());
    assert!(
        !fresh
            .owner
            .automation
            .dependencies
            .local_filesystem_epoch()
            .unwrap()
            .is_cancelled()
    );
    assert!(epoch.is_cancelled());
    f.app.shutdown().await.unwrap();
}

#[test]
fn caller_cancellation_and_deadline_fence_never_become_a_new_grant() {
    let token = CancellationToken::new();
    assert!(live(&token, Instant::now() + Duration::from_secs(1)).is_ok());
    assert!(live(&token, Instant::now() - Duration::from_millis(1)).is_err());
    token.cancel();
    assert!(live(&token, Instant::now() + Duration::from_secs(1)).is_err());
}
