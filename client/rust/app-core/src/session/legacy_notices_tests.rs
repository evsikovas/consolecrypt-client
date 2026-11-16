//! Actual existing object-event loop and filesystem backup producer tests.
//! Typed inputs are metadata, not fabricated sync/key/command authority.
use super::*;
use crate::{dto::CriticalSecurityStateDto, AppCore};
use cc_models::ObjectId;
use serde_json::{json, Value};

const PASSWORD: &str = "owned legacy notice profile password42";
async fn fixture() -> (tempfile::TempDir, AppCore, Arc<Session>, Arc<Unlocked>) {
    let dir = tempfile::tempdir().unwrap();
    let app = AppCore::new(AppConfig::for_tests(
        dir.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("Owned scoped legacy notices".into(), PASSWORD.into())
        .await
        .unwrap();
    let (session, u) = app.unlocked().await.unwrap();
    (dir, app, session, u)
}
async fn critical(rx: &mut broadcast::Receiver<AppEvent>) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = rx.recv().await.unwrap();
            if matches!(
                event,
                AppEvent::CriticalStateChanged { .. } | AppEvent::CriticalNoticeGap { .. }
            ) {
                return serde_json::to_value(event).unwrap();
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn existing_object_loop_projects_integrity_secret_conflict_and_rollback_without_raw_payloads()
{
    let (_dir, app, s, u) = fixture().await;
    let (sender, rx) = broadcast::channel(16);
    let mut events = app.subscribe_events();
    let scope = u.notices.scope_id().unwrap();
    let vault = u.vault_id;
    let task = tokio::spawn(object_event_task(
        u.writer.clone(),
        rx,
        s.ctx.clone(),
        s.device_id(),
        u.notices.clone(),
    ));
    let object = ObjectId::new();
    sender
        .send(SyncEvent::IntegrityWarning {
            vault_id: vault,
            object_id: object,
            revision: 7,
        })
        .unwrap();
    let first = critical(&mut events).await;
    assert_eq!(
        first["change"],
        json!({"domain":"security","state":"integrity_warning"})
    );
    assert_eq!(first["record_id"], object.to_string());
    assert_eq!(first["scope_id"], scope);
    sender
        .send(SyncEvent::ConflictResolved {
            vault_id: vault,
            object_id: object,
            resolution: cc_sync_core::ConflictResolution::RemoteKeptLocalCopied,
            conflict_copy: Some(ObjectId::new()),
            kek_class: Some(KekClass::Secrets),
        })
        .unwrap();
    assert_eq!(
        critical(&mut events).await["change"],
        json!({"domain":"security","state":"secret_conflict"})
    );
    sender
        .send(SyncEvent::ServerRollbackDetected {
            vault_id: vault,
            reason: cc_storage_core::RollbackReason::SequenceRegressed,
        })
        .unwrap();
    let rollback = critical(&mut events).await;
    assert_eq!(
        rollback["change"],
        json!({"domain":"security","state":"server_rollback"})
    );
    assert_eq!(rollback.as_object().unwrap().len(), 6);
    for forbidden in [
        "reason",
        "sequence_regressed",
        "kek_class",
        "resolution",
        "password",
        "path",
    ] {
        assert!(!rollback.to_string().contains(forbidden));
    }
    drop(sender);
    task.await.unwrap();
    drop(u);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn original_object_loop_after_real_lock_and_same_profile_unlock_cannot_publish_new_scope() {
    let (_dir, app, s, u) = fixture().await;
    let old_scope = u.notices.scope_id().unwrap();
    let vault = u.vault_id;
    let (sender, rx) = broadcast::channel(8);
    let mut events = app.subscribe_events();
    let task = tokio::spawn(object_event_task(
        u.writer.clone(),
        rx,
        s.ctx.clone(),
        s.device_id(),
        u.notices.clone(),
    ));
    drop(u);
    app.lock().await.unwrap();
    app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    let (_, current) = app.unlocked().await.unwrap();
    assert_ne!(current.notices.scope_id(), Some(old_scope));
    while events.try_recv().is_ok() {}
    let old_object = ObjectId::new();
    sender
        .send(SyncEvent::IntegrityWarning {
            vault_id: vault,
            object_id: old_object,
            revision: 1,
        })
        .unwrap();
    // Legacy is emitted AFTER the actual scoped publication attempt. This is a
    // causal barrier, not a sleep asserting absence before the producer ran.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match events.recv().await.unwrap() {
                AppEvent::IntegrityWarning { object_id } if object_id == old_object.to_string() => {
                    break
                }
                AppEvent::CriticalStateChanged { .. } => {
                    panic!("closed original producer emitted scoped metadata")
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    current
        .notices
        .security(ObjectId::new(), CriticalSecurityStateDto::IntegrityWarning);
    assert_eq!(
        critical(&mut events).await["scope_id"],
        current.notices.scope_id().unwrap()
    );
    drop(sender);
    task.await.unwrap();
    drop(current);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn actual_backup_filesystem_failure_emits_only_original_scope_fixed_metadata() {
    let (dir, app, _s, u) = fixture().await;
    let scope = u.notices.scope_id().unwrap();
    drop(u);
    let blocked = dir.path().join("owned-file-is-not-backup-directory");
    std::fs::write(&blocked, b"owned sentinel").unwrap();
    let schedule = crate::backup::BackupScheduleDto {
        folder: Some(blocked.to_string_lossy().into_owned()),
        ..Default::default()
    };
    app.set_backup_schedule(schedule).await.unwrap();
    let mut events = app.subscribe_events();
    assert!(app.backup_now().await.is_err());
    let event = critical(&mut events).await;
    assert_eq!(event["scope_id"], scope);
    assert_eq!(event["change"], json!({"domain":"backup","state":"failed"}));
    assert!(!event.to_string().contains("owned-file"));
    assert_eq!(std::fs::read(&blocked).unwrap(), b"owned sentinel");
    app.shutdown().await.unwrap();
}
