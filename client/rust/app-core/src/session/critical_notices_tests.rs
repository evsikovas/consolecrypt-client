use super::*;
use crate::dto::VaultStateDto;
use crate::AppCore;
use serde_json::{json, Value};

fn fixture(limit: usize) -> (Arc<RuntimeNoticeEmitter>, broadcast::Receiver<AppEvent>) {
    let (sender, receiver) = broadcast::channel(1024);
    (
        RuntimeNoticeEmitter::create(ProfileId::new(), sender, limit),
        receiver,
    )
}

fn changes(receiver: &mut broadcast::Receiver<AppEvent>) -> Vec<AppEvent> {
    let mut result = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        if matches!(
            event,
            AppEvent::CriticalStateChanged { .. } | AppEvent::CriticalNoticeGap { .. }
        ) {
            result.push(event);
        }
    }
    result
}

#[test]
fn startup_snapshot_recovers_before_subscription_without_allocating_new_revisions() {
    let (owner, receiver) = fixture(128);
    drop(receiver); // The application shell has not subscribed yet.
    let first = Uuid::new_v4();
    owner.transfer(first, CriticalTransferStateDto::Completed);
    assert!(owner.profile_snapshot().unwrap()["records"]
        .as_array()
        .unwrap()
        .is_empty());
    owner.transfer(first, CriticalTransferStateDto::Failed);
    let initial = owner.profile_snapshot().unwrap();
    let first_row = initial["records"][0].clone();
    assert_eq!(first_row.as_object().unwrap().len(), 6);
    assert_eq!(first_row["profile_id"], owner.profile_id.to_string());
    assert_eq!(first_row["scope_id"], initial["scope_id"]);
    assert_eq!(
        initial,
        owner.profile_snapshot().unwrap(),
        "reads cannot recreate a dismissed revision"
    );
    owner.transfer(Uuid::new_v4(), CriticalTransferStateDto::Failed);
    let newer = owner.profile_snapshot().unwrap();
    let unchanged = newer["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["record_id"] == first.to_string())
        .unwrap();
    assert_eq!(
        unchanged, &first_row,
        "unrelated changes cannot make old notices unread again"
    );
    owner.transfer(first, CriticalTransferStateDto::Completed);
    let completed = owner.profile_snapshot().unwrap();
    assert_eq!(completed["records"][0]["change"]["state"], "completed");
    assert!(
        completed["records"][0]["revision"].as_u64().unwrap()
            > first_row["revision"].as_u64().unwrap()
    );
    owner.close();
    assert!(owner.profile_snapshot().is_none());
}

#[test]
fn startup_snapshot_is_bounded_and_truncation_is_explicit() {
    let (owner, _) = fixture(128);
    let ids: Vec<_> = (0..65).map(|_| Uuid::new_v4()).collect();
    for id in &ids {
        owner.transfer(*id, CriticalTransferStateDto::Failed);
    }
    let snapshot = owner.profile_snapshot().unwrap();
    let rows = snapshot["records"].as_array().unwrap();
    assert_eq!(rows.len(), 64);
    assert_eq!(snapshot["incomplete"], true);
    assert_eq!(rows[0]["record_id"], ids[64].to_string());
    assert_eq!(rows[63]["record_id"], ids[1].to_string());
    assert!(serde_json::to_vec(&snapshot).unwrap().len() < 32 * 1024);
    let (limited, _) = fixture(1);
    limited.transfer(ids[0], CriticalTransferStateDto::Failed);
    limited.transfer(ids[1], CriticalTransferStateDto::Failed);
    assert_eq!(limited.profile_snapshot().unwrap()["incomplete"], true);
}

#[tokio::test]
async fn authoritative_profile_read_includes_only_its_original_unlocked_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        dir.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    let password = "owned snapshot profile password42".to_owned();
    app.create_local_profile("Owned startup snapshot".into(), password.clone())
        .await
        .unwrap();
    let (_, u) = app.unlocked().await.unwrap();
    let original = u.notices.clone();
    drop(u);
    original.security(ObjectId::new(), CriticalSecurityStateDto::IntegrityWarning);
    let info = app.active_profile().await.unwrap().unwrap();
    let snapshot = info.critical_notice_snapshot.unwrap();
    assert_eq!(
        snapshot["scope_id"].as_str(),
        info.notification_scope_id.as_deref()
    );
    assert_eq!(snapshot["profile_id"], info.id);
    assert_eq!(
        snapshot["records"][0]["change"]["state"],
        "integrity_warning"
    );
    app.lock().await.unwrap();
    let locked = app.active_profile().await.unwrap().unwrap();
    assert!(locked.critical_notice_snapshot.is_none());
    assert!(locked.notification_scope_id.is_none());
    assert!(original.profile_snapshot().is_none());
    app.unlock_with_passphrase(password).await.unwrap();
    let fresh = app.active_profile().await.unwrap().unwrap();
    assert_ne!(fresh.notification_scope_id, info.notification_scope_id);
    assert!(fresh.critical_notice_snapshot.unwrap()["records"]
        .as_array()
        .unwrap()
        .is_empty());
    app.shutdown().await.unwrap();
}

fn assert_projection(event: &AppEvent, profile: ProfileId, scope: &str) -> Value {
    let json = serde_json::to_value(event).unwrap();
    let fields = json.as_object().unwrap();
    assert_eq!(fields.len(), 6);
    for key in [
        "type",
        "profile_id",
        "scope_id",
        "record_id",
        "revision",
        "change",
    ] {
        assert!(fields.contains_key(key));
    }
    assert_eq!(json["type"], "critical_state_changed");
    assert_eq!(json["profile_id"], profile.to_string());
    assert_eq!(json["scope_id"], scope);
    assert_eq!(json["change"].as_object().unwrap().len(), 2);
    assert!(json["revision"]
        .as_u64()
        .is_some_and(|r| r > 0 && r <= MAX_REVISION));
    json
}

#[test]
fn wire_is_fixed_metadata_only_even_for_hostile_terminal_status_payloads() {
    let (owner, mut receiver) = fixture(8);
    let scope = owner.scope_id().unwrap();
    let id = Uuid::new_v4();
    owner.terminal(
        id,
        &TerminalStatusDto::Failed {
            message: "private-password host=fixture.example command=/private/path".into(),
        },
    );
    owner.terminal(
        id,
        &TerminalStatusDto::Closed {
            exit_status: None,
            reason: Some("private-command stderr secret".into()),
        },
    );
    let events = changes(&mut receiver);
    assert_eq!(events.len(), 2);
    for event in &events {
        let value = assert_projection(event, owner.profile_id, &scope);
        let text = value.to_string();
        for private in [
            "private",
            "password",
            "host=",
            "command",
            "stderr",
            "exit_status",
        ] {
            assert!(!text.contains(private));
        }
    }
    assert_eq!(
        serde_json::to_value(&events[1]).unwrap()["change"],
        json!({"domain":"terminal","state":"closed"})
    );
    for invalid in [
        json!({"domain":"terminal","state":"unknown"}),
        json!({"domain":"terminal","state":"failed","message":"private"}),
        json!({"domain":"sync","state":"connected"}),
    ] {
        assert!(serde_json::from_value::<CriticalRuntimeChangeDto>(invalid).is_err());
    }
}

#[test]
fn initial_normal_state_is_not_success_and_duplicate_actual_state_does_not_increment() {
    let (owner, mut receiver) = fixture(8);
    let id = ObjectId::new();
    owner.tunnel(id, CriticalTunnelStateDto::Running);
    assert!(changes(&mut receiver).is_empty());
    owner.tunnel(id, CriticalTunnelStateDto::Failed);
    owner.tunnel(id, CriticalTunnelStateDto::Failed);
    owner.tunnel(id, CriticalTunnelStateDto::Running);
    let events = changes(&mut receiver);
    assert_eq!(events.len(), 2);
    assert_eq!(serde_json::to_value(&events[0]).unwrap()["revision"], 1);
    assert_eq!(serde_json::to_value(&events[1]).unwrap()["revision"], 2);
}

#[test]
fn sync_failed_queue_is_not_idle_recovery_and_shutdown_is_not_a_new_failure() {
    let (owner, mut receiver) = fixture(8);
    let source = owner.sync_source();
    let mut status = SyncStatusDto::with_phase(SyncPhaseDto::Idle);
    source.status(&status);
    assert!(changes(&mut receiver).is_empty());
    status.failed = 2;
    status.last_error = Some("private server response /path token".into());
    source.status(&status);
    status.phase = SyncPhaseDto::Syncing;
    source.status(&status);
    let error = changes(&mut receiver);
    assert_eq!(error.len(), 1);
    let json = serde_json::to_value(&error[0]).unwrap();
    assert_eq!(json["change"], json!({"domain":"sync","state":"error"}));
    assert!(!json.to_string().contains("private"));
    assert_eq!(json["record_id"], owner.profile_id.to_string());
    status.phase = SyncPhaseDto::Stopped;
    status.stop_reason = Some(SyncStopReasonDto::Shutdown);
    source.status(&status);
    assert!(changes(&mut receiver).is_empty());
    status.stop_reason = Some(SyncStopReasonDto::VaultGone);
    source.status(&status);
    let stopped = changes(&mut receiver);
    assert_eq!(stopped.len(), 1);
    assert_eq!(
        serde_json::to_value(&stopped[0]).unwrap()["change"]["state"],
        "vault_gone"
    );
}

#[test]
fn stopped_or_replaced_engine_source_cannot_overwrite_current_source() {
    let (owner, mut receiver) = fixture(8);
    let original = owner.sync_source();
    original.status(&SyncStatusDto::with_phase(SyncPhaseDto::Offline));
    owner.stop_sync_source();
    original.status(&SyncStatusDto::with_phase(SyncPhaseDto::Error));
    let current = owner.sync_source();
    current.status(&SyncStatusDto::with_phase(SyncPhaseDto::Idle));
    original.status(&SyncStatusDto::with_phase(SyncPhaseDto::Error));
    let events = changes(&mut receiver);
    assert_eq!(events.len(), 2);
    assert_eq!(
        serde_json::to_value(&events[0]).unwrap()["change"]["state"],
        "offline"
    );
    assert_eq!(
        serde_json::to_value(&events[1]).unwrap()["change"]["state"],
        "idle"
    );
}

#[test]
fn close_is_synchronous_and_old_same_profile_scope_never_rebinds() {
    let (sender, mut receiver) = broadcast::channel(32);
    let profile = ProfileId::new();
    let original = RuntimeNoticeEmitter::new(profile, sender.clone());
    let current = RuntimeNoticeEmitter::new(profile, sender);
    let original_scope = original.scope_id().unwrap();
    let current_scope = current.scope_id().unwrap();
    assert_ne!(original_scope, current_scope);
    let source = original.sync_source();
    original.close();
    assert_eq!(original.scope_id(), None);
    source.status(&SyncStatusDto::with_phase(SyncPhaseDto::Error));
    original.tunnel(ObjectId::new(), CriticalTunnelStateDto::Failed);
    original.terminal(
        Uuid::new_v4(),
        &TerminalStatusDto::Failed {
            message: "private".into(),
        },
    );
    assert!(changes(&mut receiver).is_empty());
    current.tunnel(ObjectId::new(), CriticalTunnelStateDto::Failed);
    let events = changes(&mut receiver);
    assert_eq!(events.len(), 1);
    assert_projection(&events[0], profile, &current_scope);
}

#[test]
fn concurrent_domains_publish_monotonic_revisions_and_close_prevents_future_sends() {
    let (owner, mut receiver) = fixture(8);
    std::thread::scope(|threads| {
        for _ in 0..4 {
            let owner = owner.clone();
            threads.spawn(move || {
                let id = ObjectId::new();
                for _ in 0..16 {
                    owner.tunnel(id, CriticalTunnelStateDto::Failed);
                    owner.tunnel(id, CriticalTunnelStateDto::Running);
                }
            });
        }
    });
    owner.close();
    let events = changes(&mut receiver);
    assert_eq!(events.len(), 128);
    for (i, event) in events.iter().enumerate() {
        assert_eq!(
            serde_json::to_value(event).unwrap()["revision"],
            (i + 1) as u64
        );
    }
    owner.tunnel(ObjectId::new(), CriticalTunnelStateDto::Failed);
    assert!(changes(&mut receiver).is_empty());
}

#[test]
fn safe_maximum_and_overflow_send_one_gap_without_unsafe_revision_or_fake_state() {
    let (owner, mut receiver) = fixture(8);
    owner.lock().revision = MAX_REVISION - 1;
    let id = ObjectId::new();
    owner.tunnel(id, CriticalTunnelStateDto::Failed);
    owner.tunnel(id, CriticalTunnelStateDto::Running);
    owner.tunnel(id, CriticalTunnelStateDto::Stopped);
    let events = changes(&mut receiver);
    assert_eq!(events.len(), 2);
    assert_eq!(
        serde_json::to_value(&events[0]).unwrap()["revision"],
        MAX_REVISION
    );
    let gap = serde_json::to_value(&events[1]).unwrap();
    assert_eq!(gap["type"], "critical_notice_gap");
    assert_eq!(gap.as_object().unwrap().len(), 3);
    assert!(!gap.as_object().unwrap().contains_key("revision"));
}

#[test]
fn bounded_map_reports_one_gap_instead_of_retaining_new_record_or_failing_business_state() {
    let (owner, mut receiver) = fixture(2);
    for _ in 0..4 {
        owner.tunnel(ObjectId::new(), CriticalTunnelStateDto::Failed);
    }
    let events = changes(&mut receiver);
    assert_eq!(events.len(), 3);
    assert_eq!(owner.lock().records.len(), 2);
    assert!(matches!(
        events.last(),
        Some(AppEvent::CriticalNoticeGap { .. })
    ));
}

#[tokio::test]
async fn actual_profile_info_nonce_is_current_unlocked_only_and_changes_on_same_profile_reunlock() {
    let directory = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        directory.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    let password = "owned runtime notice fixture passphrase 42";
    app.create_local_profile("Owned runtime notice fixture".into(), password.into())
        .await
        .unwrap();
    let info = app.active_profile().await.unwrap().unwrap();
    let (session, unlocked) = app.unlocked().await.unwrap();
    let first = info.notification_scope_id.clone().unwrap();
    assert_eq!(info.vault_state, VaultStateDto::Unlocked);
    assert_eq!(Some(first.clone()), unlocked.notices.scope_id());
    assert_eq!(info.id, session.profile_id.to_string());
    let mut old_json = serde_json::to_value(&info).unwrap();
    old_json
        .as_object_mut()
        .unwrap()
        .remove("notification_scope_id");
    assert_eq!(
        serde_json::from_value::<crate::dto::ProfileInfo>(old_json)
            .unwrap()
            .notification_scope_id,
        None
    );
    app.lock().await.unwrap();
    let locked = app.active_profile().await.unwrap().unwrap();
    assert_eq!(locked.vault_state, VaultStateDto::Locked);
    assert_eq!(locked.notification_scope_id, None);
    assert_eq!(unlocked.notices.scope_id(), None);
    app.unlock_with_passphrase(password.into()).await.unwrap();
    let reopened = app.active_profile().await.unwrap().unwrap();
    assert_ne!(reopened.notification_scope_id, Some(first));
    assert!(reopened.notification_scope_id.is_some());
    app.close_profile().await.unwrap();
    let list = app.list_profiles().await.unwrap();
    assert!(list.iter().all(|p| p.notification_scope_id.is_none()));
}

#[tokio::test]
async fn actual_profile_switch_closes_original_emitter_and_never_labels_it_as_new_profile() {
    let directory = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        directory.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile(
        "Original runtime notice fixture".into(),
        "owned original runtime passphrase42".into(),
    )
    .await
    .unwrap();
    let (original_session, original) = app.unlocked().await.unwrap();
    let original_scope = original.notices.scope_id().unwrap();
    app.create_local_profile(
        "Next runtime notice fixture".into(),
        "owned next runtime passphrase42".into(),
    )
    .await
    .unwrap();
    let (current_session, current) = app.unlocked().await.unwrap();
    assert_ne!(original_session.profile_id, current_session.profile_id);
    assert_ne!(Some(original_scope), current.notices.scope_id());
    let mut receiver = app.subscribe_events();
    original
        .notices
        .tunnel(ObjectId::new(), CriticalTunnelStateDto::Failed);
    current
        .notices
        .tunnel(ObjectId::new(), CriticalTunnelStateDto::Failed);
    let events = changes(&mut receiver);
    assert_eq!(events.len(), 1);
    assert_projection(
        &events[0],
        current_session.profile_id,
        &current.notices.scope_id().unwrap(),
    );
    app.close_profile().await.unwrap();
}

#[test]
fn engine_identity_and_notice_generation_are_one_interleaving_boundary() {
    for transition in ["replace", "stop"] {
        let (owner, mut receiver) = fixture(8);
        let old = owner.sync_source();
        let slot = Arc::new(RwLock::new(Some(1u8)));
        let (entered, entered_rx) = std::sync::mpsc::channel();
        let (release, release_rx) = std::sync::mpsc::channel();
        let first_slot = slot.clone();
        let first_owner = owner.clone();
        let first = std::thread::spawn(move || {
            // The SAME helper used by production replace/take is parked
            // between pointer mutation and generation publication. Scalar
            // identity is test data, not a fabricated sync/authority witness.
            engine_transition(&first_slot, |current| {
                *current = if transition == "replace" {
                    Some(2)
                } else {
                    None
                };
                entered.send(()).unwrap();
                release_rx.recv().unwrap();
                if transition == "replace" {
                    Some(first_owner.sync_source())
                } else {
                    first_owner.stop_sync_source();
                    None
                }
            })
        });
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        // Deterministic: the callback is known to be parked at the exact
        // mutation/publication seam. A guard dropped before it would fail.
        assert!(slot.try_write().is_err());
        let next_slot = slot.clone();
        let next_owner = owner.clone();
        let next = std::thread::spawn(move || next_owner.replace_engine(&next_slot, 3));
        release.send(()).unwrap();
        let first_source = first.join().unwrap();
        let current_source = next.join().unwrap();
        assert_eq!(*slot.read().unwrap(), Some(3));
        let mut status = SyncStatusDto::local_only();
        status.phase = SyncPhaseDto::Offline;
        old.status(&status);
        if let Some(source) = first_source {
            source.status(&status);
        }
        assert!(changes(&mut receiver).is_empty());
        current_source.status(&status);
        let events = changes(&mut receiver);
        assert_eq!(events.len(), 1);
        assert_eq!(
            serde_json::to_value(&events[0]).unwrap()["change"],
            json!({"domain":"sync","state":"offline"})
        );
    }
}

#[test]
fn upstream_gap_is_bounded_original_owner_metadata_and_closed_owner_cannot_emit() {
    let (owner, mut receiver) = fixture(8);
    let original_scope = owner.scope_id().unwrap();
    owner.source_gap();
    owner.source_gap();
    let events = changes(&mut receiver);
    assert_eq!(events.len(), 1);
    let gap = serde_json::to_value(&events[0]).unwrap();
    assert_eq!(gap.as_object().unwrap().len(), 3);
    assert_eq!(
        gap,
        json!({"type":"critical_notice_gap","profile_id":owner.profile_id.to_string(),"scope_id":original_scope})
    );
    owner.close();
    owner.source_gap();
    assert!(changes(&mut receiver).is_empty());
}

#[test]
fn new_domains_remain_closed_metadata_and_literal_completions_do_not_invent_recovery() {
    let (owner, mut receiver) = fixture(64);
    let id = Uuid::new_v4();
    owner.backup(CriticalBackupStateDto::Completed);
    owner.edit(id, CriticalEditStateDto::Synced);
    owner.transfer(id, CriticalTransferStateDto::Completed);
    assert!(changes(&mut receiver).is_empty());
    owner.backup(CriticalBackupStateDto::Failed);
    owner.backup(CriticalBackupStateDto::Completed);
    owner.edit(id, CriticalEditStateDto::Error);
    owner.edit(id, CriticalEditStateDto::Synced);
    owner.transfer(id, CriticalTransferStateDto::Failed);
    owner.transfer(id, CriticalTransferStateDto::Completed);
    owner.security(Uuid::new_v4(), CriticalSecurityStateDto::IntegrityWarning);
    owner.device(Uuid::new_v4(), CriticalDeviceStateDto::KeyConflict);
    owner.team(Uuid::new_v4(), CriticalTeamStateDto::InvalidLog);
    let rows = changes(&mut receiver);
    assert_eq!(rows.len(), 9);
    for event in rows {
        let value = assert_projection(&event, owner.profile_id, &owner.scope_id().unwrap());
        assert!(!value.to_string().contains("recovered"));
        let mut injected = value["change"].clone();
        injected["reason"] = json!("secret private path");
        assert!(serde_json::from_value::<CriticalRuntimeChangeDto>(injected).is_err());
    }
    for change in [
        json!({"domain":"security","state":"completed"}),
        json!({"domain":"backup","state":"key_conflict"}),
        json!({"domain":"device","state":"approved"}),
    ] {
        assert!(serde_json::from_value::<CriticalRuntimeChangeDto>(change).is_err());
    }
}

#[test]
fn device_websocket_notices_retain_original_engine_generation_and_never_relabel_loss() {
    let (owner, mut receiver) = fixture(64);
    let old = owner.sync_source();
    old.device(Uuid::new_v4(), CriticalDeviceStateDto::Registered);
    assert_eq!(changes(&mut receiver).len(), 1);
    let current = owner.sync_source();
    old.device(Uuid::new_v4(), CriticalDeviceStateDto::Revoked);
    old.recovery_changed();
    old.source_gap();
    assert!(changes(&mut receiver).is_empty());
    current.recovery_changed();
    assert_eq!(changes(&mut receiver).len(), 1);
    owner.close();
    for state in [
        CriticalSecurityStateDto::IntegrityWarning,
        CriticalSecurityStateDto::SecretConflict,
    ] {
        owner.security(Uuid::new_v4(), state);
    }
    owner.backup(CriticalBackupStateDto::Failed);
    owner.team(Uuid::new_v4(), CriticalTeamStateDto::Removed);
    owner.edit(Uuid::new_v4(), CriticalEditStateDto::Error);
    owner.transfer(Uuid::new_v4(), CriticalTransferStateDto::Failed);
    current.device(Uuid::new_v4(), CriticalDeviceStateDto::Registered);
    assert!(changes(&mut receiver).is_empty());
}
