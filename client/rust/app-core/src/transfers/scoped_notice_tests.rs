//! Real manager mutations and real two-profile/SQL wait. No SFTP commands,
//! fabricated authorization, production files or caller-success witness.
use super::*;
use crate::AppCore;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Poll;

const PASSWORD: &str = "owned scoped producer test password42";
async fn fixture() -> (
    tempfile::TempDir,
    AppCore,
    Arc<crate::session::Session>,
    Arc<crate::session::Unlocked>,
) {
    let dir = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        dir.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("Original producer owner".into(), PASSWORD.into())
        .await
        .unwrap();
    let (s, u) = app.unlocked().await.unwrap();
    (dir, app, s, u)
}
fn metadata_job(id: &str, notice_id: uuid::Uuid, task: Option<tokio::task::AbortHandle>) -> Job {
    let request = TransferRequestDto {
        transfer_id: id.into(),
        sftp_id: "private-session".into(),
        direction: TransferDirectionDto::Upload,
        local_path: "private-source".into(),
        remote_path: "/private-target".into(),
    };
    let dto = TransferJobDto {
        transfer_id: id.into(),
        sftp_id: request.sftp_id.clone(),
        direction: request.direction,
        local_path: request.local_path.clone(),
        remote_path: request.remote_path.clone(),
        state: TransferStateDto::Running,
        transferred: 0,
        total: None,
        files_done: 0,
        files_total: 0,
        is_directory: false,
        current_path: None,
        created_at_ms: 0,
        started_at_ms: None,
        finished_at_ms: None,
        elapsed_ms: 0,
        error: None,
    };
    Job {
        seq: 1,
        notice_id,
        request,
        dto,
        cancel: CancelToken::new(),
        task,
    }
}

#[tokio::test]
async fn cancelled_old_worker_cannot_mutate_or_clear_reused_id_new_native_attempt() {
    let (_dir, app, _s, u) = fixture().await;
    let manager = u.transfers.clone();
    let mut events = app.subscribe_events();
    let id = "caller-defined-non-uuid-private-path";
    let original = uuid::Uuid::new_v4();
    manager
        .lock()
        .insert(id.into(), metadata_job(id, original, None));
    let (entered, entered_rx) = tokio::sync::oneshot::channel();
    let (release, release_rx) = tokio::sync::oneshot::channel();
    let old = manager.clone();
    let mutation = Arc::new(AtomicBool::new(false));
    let attempted = mutation.clone();
    let task = tokio::spawn(async move {
        entered.send(()).unwrap();
        release_rx.await.unwrap();
        old.update(id, original, |dto| {
            attempted.store(true, Ordering::SeqCst);
            dto.state = TransferStateDto::Failed;
        });
        old.finish_task(id, original);
    });
    entered_rx.await.unwrap();
    assert!(manager.cancel(id));
    let new_id = uuid::Uuid::new_v4();
    let pending = tokio::spawn(std::future::pending::<()>());
    manager.lock().insert(
        id.into(),
        metadata_job(id, new_id, Some(pending.abort_handle())),
    );
    let before = manager.get(id).unwrap();
    release.send(()).unwrap();
    task.await.unwrap();
    assert!(
        !mutation.load(Ordering::SeqCst),
        "old mutation closure must not touch replacement"
    );
    assert_eq!(manager.get(id).unwrap(), before);
    assert!(manager.lock().get(id).unwrap().task.is_some());
    while let Ok(event) = events.try_recv() {
        assert!(!matches!(event, AppEvent::CriticalStateChanged { .. }));
    }
    manager.update(id, new_id, |dto| dto.state = TransferStateDto::Failed);
    let mut count = 0;
    while let Ok(event) = events.try_recv() {
        if matches!(event, AppEvent::CriticalStateChanged { .. }) {
            let value = serde_json::to_value(event).unwrap();
            assert_eq!(value["record_id"], new_id.to_string());
            for raw in [id, "private-source", "private-target"] {
                assert!(!value.to_string().contains(raw));
            }
            count += 1;
        }
    }
    assert_eq!(count, 1);
    pending.abort();
    drop(u);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn pending_backup_sql_never_recaptures_new_active_profile_during_real_activation_close_wait()
{
    let (dir, app, s, u) = fixture().await;
    let output = dir.path().join("owned-original-backups");
    std::fs::create_dir(&output).unwrap();
    app.set_backup_schedule(crate::backup::BackupScheduleDto {
        folder: Some(output.to_string_lossy().into_owned()),
        ..Default::default()
    })
    .await
    .unwrap();
    let storage = s.storage().unwrap();
    let mut closing = u.sharing_shutdown.subscribe();
    let original_vault = u.vault_id;
    drop(u);
    let (entered, entered_rx) = tokio::sync::oneshot::channel();
    let (release, release_rx) = std::sync::mpsc::channel();
    let blocked = storage.clone();
    let blocker = tokio::spawn(async move {
        blocked
            .call(move |_| {
                entered.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(15)).unwrap();
                Ok::<_, cc_storage_core::StorageError>(())
            })
            .await
            .unwrap()
    });
    entered_rx.await.unwrap();
    let mut backup = Box::pin(app.backup_now());
    // Current-thread deterministic poll: original Session/Unlocked ready,
    // then its first actual schedule SQL read queues behind the held worker.
    std::future::poll_fn(|cx| {
        assert!(backup.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    let creator = app.clone();
    let changed = tokio::spawn(async move {
        creator
            .create_local_profile("Next actual producer owner".into(), PASSWORD.into())
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), closing.changed())
        .await
        .unwrap()
        .unwrap();
    assert!(
        *closing.borrow(),
        "actual original close began after activate swapped the new unlocked owner"
    );
    let (next, next_u) = app.unlocked().await.unwrap();
    assert_ne!(next.profile_id, s.profile_id);
    let next_vault = next_u.vault_id;
    drop(next_u);
    let before_schedule = app.backup_schedule().await.unwrap();
    let before_recent = app.recent_backups().await.unwrap();
    let mut events = app.subscribe_events();
    release.send(()).unwrap();
    blocker.await.unwrap();
    assert!(
        backup.await.is_err(),
        "original storage/key scope closed; it must not execute on next owner"
    );
    changed.await.unwrap().unwrap();
    assert_eq!(app.backup_schedule().await.unwrap(), before_schedule);
    assert_eq!(app.recent_backups().await.unwrap(), before_recent);
    let artifacts: Vec<_> = std::fs::read_dir(&output)
        .unwrap()
        .take(3)
        .map(|entry| entry.unwrap())
        .collect();
    assert!(
        artifacts.len() <= 1,
        "one authorized attempt can leave at most one final original-owner archive"
    );
    for entry in &artifacts {
        let path = entry.path();
        let meta = entry.metadata().unwrap();
        assert!(
            entry.file_type().unwrap().is_file(),
            "unexpected link/directory is not an authorized archive"
        );
        let final_archive = path.extension().is_some_and(|ext| ext == "ccbackup");
        let mut decoded = false;
        let mut header_a = false;
        let mut header_b = false;
        let mut authenticated_a = false;
        if meta.is_file() && meta.len() <= 1024 * 1024 && final_archive {
            if let Ok(backup) =
                cc_vault_core::decode_backup_from(std::fs::File::open(&path).unwrap())
            {
                decoded = true;
                header_a = backup.vault_id == original_vault;
                header_b = backup.vault_id == next_vault;
                if let Ok(restored) = cc_vault_core::restore_backup_with_passphrase(
                    backup,
                    &cc_crypto_core::SecretString::from(PASSWORD),
                ) {
                    authenticated_a = restored.unlocked.vault_id() == original_vault
                        && restored.backup.vault_id == original_vault;
                }
            }
        }
        println!(
            "NAV09_OWNED_BACKUP_ARTIFACT {}",
            serde_json::json!({"count":artifacts.len(),"bytes":meta.len(),
            "finalArchive":final_archive,"decoded":decoded,"originalVault":header_a,"otherVault":header_b,
            "authenticatedOriginalManifestAndObjects":authenticated_a})
        );
        assert!(
            final_archive && decoded && header_a && !header_b && authenticated_a,
            "remaining artifact must be a complete MAC/object-verified original A archive, never B/incomplete/temp data"
        );
    }
    while let Ok(event) = events.try_recv() {
        if let AppEvent::CriticalStateChanged { change, .. } = event {
            assert!(
                !matches!(change, crate::dto::CriticalRuntimeChangeDto::Backup { .. }),
                "no foreign backup outcome may be attributed"
            );
        }
    }
    app.shutdown().await.unwrap();
}
