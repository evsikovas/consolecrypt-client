//! Actual metadata transactions with Native-issued review Context. No Helper is
//! started, no verified Ready/publisher is forged, and no catalog target selected.
use super::*;
use std::{future::Future, task::Poll};

#[tokio::test]
async fn original_scope_after_deferred_cleanup_denies_late_success_on_actual_lock_without_sql_rollback(
) {
    use std::sync::atomic::{AtomicBool, Ordering};
    let _serial = super::super::tests::serial().lock().await;
    for cause in ["lock", "switch"] {
        let f = super::super::tests::Fixture::new().await;
        let (_registry, ticket) = f.ticket().await;
        let context = ticket.context;
        let original_profile = context.project.owner.profile_id.to_string();
        let selected = f.project.clone();
        // Actual owned SQL mutation, deliberately no claim of a Core filesystem
        // publication. This test isolates ONLY the post-cleanup response fence.
        let committed = context
            .project
            .owner
            .storage()
            .unwrap()
            .write(move |tx| -> AppResult<Value> {
                let mut catalog: Value = tx.setting_get(PROJECTS)?.unwrap();
                catalog["records"][&selected]["revision"] = json!(2);
                catalog["records"][&selected]["name"] = json!("Acknowledged own SQL metadata");
                tx.setting_set(PROJECTS, &catalog)?;
                Ok(catalog)
            })
            .await
            .unwrap();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let returned = AtomicBool::new(false);
        let mut response = Box::pin(async {
            release_rx.await.unwrap();
            final_original_scope(&f.app, &context, || returned.store(true, Ordering::SeqCst)).await
        });
        std::future::poll_fn(|cx| match response.as_mut().poll(cx) {
            Poll::Pending => Poll::Ready(()),
            Poll::Ready(_) => panic!("deferred cleanup has not joined"),
        })
        .await;
        if cause == "lock" {
            f.app.lock().await.unwrap();
        } else {
            f.app
                .create_local_profile(
                    "New original-scope fixture".into(),
                    "owned alternate profile password 42".into(),
                )
                .await
                .unwrap();
        }
        release_tx.send(()).unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(5), &mut response)
            .await
            .unwrap()
            .is_err());
        drop(response);
        assert!(!returned.load(Ordering::SeqCst));
        if cause == "switch" {
            f.app.open_profile(original_profile).await.unwrap();
        }
        // Real lock deliberately denies old-owner storage access. Reopen the
        // OWNED profile explicitly for the persistence observation; never bypass
        // that guard or treat the old captured owner as a fresh unlocked scope.
        f.app
            .unlock_with_passphrase("owned dependency fixture passphrase 42".into())
            .await
            .unwrap();
        let check_owner = f.app.unlocked().await.unwrap().0;
        assert!(
            final_original_scope(&f.app, &context, || returned.store(true, Ordering::SeqCst))
                .await
                .is_err(),
            "reunlock/reopen must not resurrect the original captured scope"
        );
        assert!(!returned.load(Ordering::SeqCst));
        assert_eq!(
            check_owner
                .storage()
                .unwrap()
                .setting_get::<Value>(PROJECTS)
                .await
                .unwrap()
                .unwrap(),
            committed
        );
        f.app.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn same_transaction_rows_deny_queued_project_and_runner_changes_without_overwriting_them() {
    let _serial = super::super::tests::serial().lock().await;
    for changed in ["project", "runner"] {
        let f = super::super::tests::Fixture::new().await;
        let (_registry, ticket) = f.ticket().await;
        let context = ticket.context;
        let storage = context.project.owner.storage().unwrap().clone();
        let selected = if changed == "project" {
            f.project.clone()
        } else {
            f.runner.clone()
        };
        let key = if changed == "project" {
            PROJECTS
        } else {
            crate::automation_runners::RUNNERS
        };
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let blocker = tokio::spawn(async move {
            storage
                .write(move |tx| -> AppResult<Value> {
                    let mut catalog: Value = tx.setting_get(key)?.unwrap();
                    // SAME revision, actual changed payload. A revision-only check or
                    // earlier async read must not authorize the stale captured row.
                    catalog["records"][&selected]["name"] = json!("actual queued changed metadata");
                    tx.setting_set(key, &catalog)?;
                    entered_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                    Ok(catalog)
                })
                .await
        });
        tokio::time::timeout(Duration::from_secs(5), entered_rx)
            .await
            .unwrap()
            .unwrap();
        let storage = context.project.owner.storage().unwrap().clone();
        let read_context = context.clone();
        let mut checked =
            Box::pin(storage.write(move |tx| checked_catalog_rows(tx, &read_context).map(|_| ())));
        std::future::poll_fn(|cx| match checked.as_mut().poll(cx) {
            Poll::Pending => Poll::Ready(()),
            Poll::Ready(_) => panic!("CAS read must queue behind real SQL mutation"),
        })
        .await;
        release_tx.send(()).unwrap();
        let current = blocker.await.unwrap().unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(5), &mut checked)
            .await
            .unwrap()
            .is_err());
        drop(checked);
        assert_eq!(
            context
                .project
                .owner
                .storage()
                .unwrap()
                .setting_get::<Value>(key)
                .await
                .unwrap()
                .unwrap(),
            current
        );
        f.app.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn same_transaction_gate_keeps_unrelated_project_records_and_marker_readonly_fields() {
    let _serial = super::super::tests::serial().lock().await;
    let f = super::super::tests::Fixture::new().await;
    let (_registry, ticket) = f.ticket().await;
    let context = ticket.context;
    let unrelated = f
        .app
        .automation_project_create("Unrelated preserved".into(), None)
        .await
        .unwrap()
        .id;
    let storage = context.project.owner.storage().unwrap();
    let before: Value = storage.setting_get(PROJECTS).await.unwrap().unwrap();
    let own = context.clone();
    let checked = storage
        .write(move |tx| checked_catalog_rows(tx, &own))
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(checked).unwrap()["records"][&unrelated],
        before["records"][&unrelated]
    );
    assert_eq!(
        storage
            .setting_get::<Value>(PROJECTS)
            .await
            .unwrap()
            .unwrap(),
        before
    );
    f.app.shutdown().await.unwrap();
}
