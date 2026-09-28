//! SyncEngine integration tests against the in-process mock server.

mod common;

use cc_models::host::Host;
use cc_models::secret::{Secret, SecretKind, SecretValue};
use cc_models::settings::VaultSettings;
use cc_models::{ObjectPayload, VaultObject};
use cc_protocol::paths;
use cc_protocol::ErrorCode;
use cc_storage_core::{LocalState, OutboxState};
use cc_sync_core::mock::{FaultAction, MockServer};
use cc_sync_core::*;
use common::*;
use std::sync::Arc;
use std::time::Duration;

fn host(name: &str, addr: &str) -> Host {
    Host::new(name, addr)
}

fn p(obj: VaultObject) -> ObjectPayload {
    ObjectPayload::new(obj)
}

fn renamed(mut h: Host, name: &str) -> Host {
    h.name = name.into();
    h.updated_at = chrono::Utc::now();
    h
}

async fn host_named(store: &ObjectStore, id: cc_protocol::ObjectId) -> Option<String> {
    match store.get(id).await.unwrap()?.payload.object {
        VaultObject::Host(h) => Some(h.name),
        _ => None,
    }
}

#[tokio::test]
async fn offline_edits_are_queued_and_synced_on_reconnect() {
    let server = MockServer::start().await;
    let (a, b, vault) = two_devices(&server).await;

    server.set_offline(true);
    let web = host("web", "10.0.0.1");
    let db = host("db", "10.0.0.2");
    a.engine
        .put(p(VaultObject::Host(web.clone())))
        .await
        .unwrap();
    a.engine
        .put(p(VaultObject::Host(db.clone())))
        .await
        .unwrap();
    a.engine
        .put(p(VaultObject::Host(renamed(web.clone(), "web-1"))))
        .await
        .unwrap();
    let err = a.engine.sync_now().await.unwrap_err();
    assert!(err.is_offline(), "{err}");
    let st = a.engine.status();
    assert_eq!(st.phase, SyncPhase::Offline);
    assert_eq!(st.pending, 2, "unsent edits of one object coalesce");

    // Background worker keeps retrying with backoff until back online.
    a.engine.start();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(server.latest_sequence(vault.vault_id), 0);
    server.set_offline(false);
    eventually(10, || async {
        a.engine.status().pending == 0 && a.engine.status().phase == SyncPhase::Idle
    })
    .await;
    assert_eq!(server.live_objects(vault.vault_id).len(), 2);

    let r = sync_ok(&b.engine).await;
    assert_eq!(r.snapshot_pages, 1);
    assert_eq!(host_named(&b.store, web.id).await.as_deref(), Some("web-1"));
    assert_eq!(host_named(&b.store, db.id).await.as_deref(), Some("db"));
    a.engine.stop().await;
}

#[tokio::test]
async fn retry_after_lost_response_is_idempotent() {
    let server = MockServer::start().await;
    let (a, _b, vault) = two_devices(&server).await;
    let h = host("web", "10.0.0.1");
    a.engine.put(p(VaultObject::Host(h.clone()))).await.unwrap();

    server.inject(paths::SYNC_PUSH, 1, FaultAction::LoseResponse);
    let err = a.engine.sync_now().await.unwrap_err();
    assert!(err.is_offline(), "{err}");
    // Committed server-side although the client never saw the response.
    assert_eq!(server.latest_sequence(vault.vault_id), 1);
    let entry = a
        .storage
        .read(move |tx| tx.outbox_list(vault.vault_id))
        .await
        .unwrap()
        .remove(0);
    assert_eq!(entry.attempts, 1);

    let r = sync_ok(&a.engine).await;
    assert_eq!((r.pushed, r.replayed), (1, 1), "same mutation_id replayed");
    assert_eq!(
        server.latest_sequence(vault.vault_id),
        1,
        "no duplicate write"
    );
    let stored = a.store.stored(h.id).await.unwrap().unwrap();
    assert_eq!(stored.local_state, LocalState::Synced);
    assert_eq!((stored.revision, stored.server_revision), (1, 1));
}

#[tokio::test]
async fn edit_during_in_flight_push_is_chained_not_lost() {
    let server = MockServer::start().await;
    let (a, b, vault) = two_devices(&server).await;
    let h = host("web", "10.0.0.1");
    a.engine.put(p(VaultObject::Host(h.clone()))).await.unwrap();
    server.inject(paths::SYNC_PUSH, 1, FaultAction::LoseResponse);
    assert!(a.engine.sync_now().await.is_err());
    // Edit while the first push's outcome is unknown.
    a.engine
        .put(p(VaultObject::Host(renamed(h.clone(), "web-2"))))
        .await
        .unwrap();
    sync_ok(&a.engine).await;
    let obj = server.object(vault.vault_id, h.id).unwrap();
    assert_eq!(obj.revision, 2);
    sync_ok(&b.engine).await;
    assert_eq!(host_named(&b.store, h.id).await.as_deref(), Some("web-2"));
}

#[tokio::test]
async fn host_conflict_keeps_remote_and_creates_conflict_copy() {
    let server = MockServer::start().await;
    let (a, b, vault) = two_devices(&server).await;
    let h = host("web", "10.0.0.1");
    a.engine.put(p(VaultObject::Host(h.clone()))).await.unwrap();
    sync_ok(&a.engine).await;
    sync_ok(&b.engine).await;

    a.engine
        .put(p(VaultObject::Host(renamed(h.clone(), "web-A"))))
        .await
        .unwrap();
    b.engine
        .put(p(VaultObject::Host(renamed(h.clone(), "web-B"))))
        .await
        .unwrap();
    sync_ok(&a.engine).await;

    let mut events = b.engine.subscribe_events();
    let r = sync_ok(&b.engine).await;
    assert_eq!((r.conflicts, r.resolved), (1, 1));
    assert_eq!(
        host_named(&b.store, h.id).await.as_deref(),
        Some("web-A"),
        "remote wins in place"
    );
    let (objs, failed) = b.store.list().await.unwrap();
    assert!(failed.is_empty());
    let copy = objs
        .iter()
        .find(|o| o.object_id != h.id)
        .expect("conflict copy");
    assert_eq!(copy.conflict_origin, Some(h.id));
    let VaultObject::Host(ch) = &copy.payload.object else {
        panic!()
    };
    assert_eq!(ch.name, format!("web-B{CONFLICT_COPY_SUFFIX}"));
    assert_eq!(ch.id, copy.object_id);

    let mut saw = false;
    while let Ok(ev) = events.try_recv() {
        if let SyncEvent::ConflictResolved {
            object_id,
            resolution,
            conflict_copy,
            ..
        } = ev
        {
            assert_eq!(object_id, h.id);
            assert_eq!(resolution, ConflictResolution::RemoteKeptLocalCopied);
            assert_eq!(conflict_copy, Some(copy.object_id));
            saw = true;
        }
    }
    assert!(saw, "user is notified");
    // The copy reached the server and device A.
    assert_eq!(server.live_objects(vault.vault_id).len(), 2);
    sync_ok(&a.engine).await;
    assert_eq!(a.store.list().await.unwrap().0.len(), 2);
    assert_eq!(b.engine.status().pending, 0);
}

#[tokio::test]
async fn secret_conflict_is_never_merged() {
    let server = MockServer::start().await;
    let (a, b, _vault) = two_devices(&server).await;
    let s = Secret::new(SecretKind::Password, SecretValue::new("initial-pass"));
    a.engine
        .put(p(VaultObject::Secret(s.clone())))
        .await
        .unwrap();
    sync_ok(&a.engine).await;
    sync_ok(&b.engine).await;

    let mut sa = s.clone();
    sa.value = SecretValue::new("pass-from-A");
    let mut sb = s.clone();
    sb.value = SecretValue::new("pass-from-B");
    a.engine.put(p(VaultObject::Secret(sa))).await.unwrap();
    b.engine.put(p(VaultObject::Secret(sb))).await.unwrap();
    sync_ok(&a.engine).await;
    let mut events = b.engine.subscribe_events();
    sync_ok(&b.engine).await;

    let value = |o: &DecryptedObject| match &o.payload.object {
        VaultObject::Secret(s) => s.value.expose_secret().to_owned(),
        _ => panic!(),
    };
    let (objs, _) = b.store.list().await.unwrap();
    let orig = objs.iter().find(|o| o.object_id == s.id).unwrap();
    let copy = objs.iter().find(|o| o.object_id != s.id).unwrap();
    assert_eq!(value(orig), "pass-from-A");
    assert_eq!(value(copy), "pass-from-B");
    let notified = std::iter::from_fn(|| events.try_recv().ok()).any(|e| {
        matches!(
            e,
            SyncEvent::ConflictResolved {
                kek_class: Some(cc_models::KekClass::Secrets),
                resolution: ConflictResolution::RemoteKeptLocalCopied,
                ..
            }
        )
    });
    assert!(notified);
}

#[tokio::test]
async fn local_edit_resurrects_remote_tombstone() {
    let server = MockServer::start().await;
    let (a, b, vault) = two_devices(&server).await;
    let h = host("web", "10.0.0.1");
    a.engine.put(p(VaultObject::Host(h.clone()))).await.unwrap();
    sync_ok(&a.engine).await;
    sync_ok(&b.engine).await;

    a.engine.delete(h.id).await.unwrap();
    sync_ok(&a.engine).await;
    assert!(server.object(vault.vault_id, h.id).unwrap().deleted);

    b.engine
        .put(p(VaultObject::Host(renamed(h.clone(), "still-needed"))))
        .await
        .unwrap();
    let mut events = b.engine.subscribe_events();
    sync_ok(&b.engine).await;
    let srv = server.object(vault.vault_id, h.id).unwrap();
    assert!(!srv.deleted);
    assert_eq!(srv.revision, 3, "new revision on top of the tombstone");
    assert!(
        std::iter::from_fn(|| events.try_recv().ok()).any(|e| matches!(
            e,
            SyncEvent::ConflictResolved {
                resolution: ConflictResolution::LocalEditResurrected,
                ..
            }
        ))
    );
    sync_ok(&a.engine).await;
    assert_eq!(
        host_named(&a.store, h.id).await.as_deref(),
        Some("still-needed")
    );
}

#[tokio::test]
async fn remote_edit_beats_local_delete() {
    let server = MockServer::start().await;
    let (a, b, _vault) = two_devices(&server).await;
    let h = host("web", "10.0.0.1");
    a.engine.put(p(VaultObject::Host(h.clone()))).await.unwrap();
    sync_ok(&a.engine).await;
    sync_ok(&b.engine).await;
    a.engine
        .put(p(VaultObject::Host(renamed(h.clone(), "edited"))))
        .await
        .unwrap();
    sync_ok(&a.engine).await;
    b.engine.delete(h.id).await.unwrap();
    sync_ok(&b.engine).await;
    assert_eq!(host_named(&b.store, h.id).await.as_deref(), Some("edited"));
}

#[tokio::test]
async fn settings_conflict_is_last_writer_wins() {
    let server = MockServer::start().await;
    let (a, b, vault) = two_devices(&server).await;
    let now = chrono::Utc::now();
    let settings = VaultSettings {
        id: cc_protocol::ObjectId::new(),
        vault_name: "Personal".into(),
        terminal_history_mode: Default::default(),
        default_privacy_profile: Default::default(),
        sync_ai_conversations: false,
        created_at: now,
        updated_at: now,
    };
    a.engine
        .put(p(VaultObject::VaultSettings(settings.clone())))
        .await
        .unwrap();
    sync_ok(&a.engine).await;
    sync_ok(&b.engine).await;

    // B edits later than A but A pushes first → B's (newer) value wins.
    let mut sa = settings.clone();
    sa.vault_name = "A-name".into();
    sa.updated_at = now + chrono::Duration::seconds(10);
    let mut sb = settings.clone();
    sb.vault_name = "B-name".into();
    sb.updated_at = now + chrono::Duration::seconds(20);
    a.engine
        .put(p(VaultObject::VaultSettings(sa)))
        .await
        .unwrap();
    b.engine
        .put(p(VaultObject::VaultSettings(sb)))
        .await
        .unwrap();
    sync_ok(&a.engine).await;
    sync_ok(&b.engine).await;
    sync_ok(&a.engine).await;
    let name = |o: DecryptedObject| match o.payload.object {
        VaultObject::VaultSettings(s) => s.vault_name,
        _ => panic!(),
    };
    assert_eq!(
        name(a.store.get(settings.id).await.unwrap().unwrap()),
        "B-name"
    );
    assert_eq!(
        server.live_objects(vault.vault_id).len(),
        1,
        "no conflict copy for settings"
    );

    // Now an older local edit loses.
    let mut sa = settings.clone();
    sa.vault_name = "A-late".into();
    sa.updated_at = now + chrono::Duration::seconds(40);
    let mut sb = settings.clone();
    sb.vault_name = "B-stale".into();
    sb.updated_at = now + chrono::Duration::seconds(30);
    a.engine
        .put(p(VaultObject::VaultSettings(sa)))
        .await
        .unwrap();
    b.engine
        .put(p(VaultObject::VaultSettings(sb)))
        .await
        .unwrap();
    sync_ok(&a.engine).await;
    sync_ok(&b.engine).await;
    assert_eq!(
        name(b.store.get(settings.id).await.unwrap().unwrap()),
        "A-late"
    );
}

#[tokio::test]
async fn snapshot_then_incremental_pull_across_pages() {
    let server = MockServer::start().await;
    let (a, _b, vault) = two_devices(&server).await;
    // Small batches and pages to exercise paging on both sides.
    let a = Client {
        engine: SyncEngine::new(a.store.clone(), a.api.clone(), {
            let mut c = fast_config(a.keys.device_id);
            c.push_batch_size = 8;
            c
        })
        .await
        .unwrap(),
        ..a
    };
    let mut hosts = Vec::new();
    for i in 0..30 {
        let h = host(&format!("h{i}"), &format!("10.0.1.{i}"));
        a.engine.put(p(VaultObject::Host(h.clone()))).await.unwrap();
        hosts.push(h);
    }
    let r = sync_ok(&a.engine).await;
    assert_eq!(r.pushed, 30);
    assert_eq!(
        server.hits(paths::SYNC_PUSH),
        4,
        "30 mutations in batches of 8"
    );

    // Fresh device C: initial snapshot in pages of 7.
    let kc = DeviceKeys::generate();
    let email = server_email(&server, &a).await;
    let api_c = login(&server, &email, &kc).await;
    attest(&api_c, &vault, kc.device_id).await;
    let c = client_with(&server, api_c, kc, vault.vault_id, |mut cfg| {
        cfg.page_limit = 7;
        cfg
    })
    .await;
    let r = sync_ok(&c.engine).await;
    assert_eq!(r.snapshot_pages, 5);
    assert_eq!(r.pulled, 30);
    assert_eq!(c.store.list().await.unwrap().0.len(), 30);
    assert_eq!(r.last_sequence, 30);

    // Incremental: 10 edits + 1 delete → `changes` pages of 7.
    for h in hosts.iter().take(10) {
        a.engine
            .put(p(VaultObject::Host(renamed(h.clone(), "edited"))))
            .await
            .unwrap();
    }
    a.engine.delete(hosts[29].id).await.unwrap();
    sync_ok(&a.engine).await;
    let r = sync_ok(&c.engine).await;
    assert_eq!(r.snapshot_pages, 0);
    assert_eq!(r.change_pages, 2);
    assert_eq!(r.pulled, 11);
    assert_eq!(r.last_sequence, 41);
    let (objs, _) = c.store.list().await.unwrap();
    assert_eq!(objs.len(), 29);
    assert_eq!(
        objs.iter()
            .filter(|o| matches!(&o.payload.object, VaultObject::Host(h) if h.name == "edited"))
            .count(),
        10
    );
}

async fn server_email(server: &MockServer, a: &Client) -> String {
    let me = a.api.me().await.unwrap();
    assert!(server.user_id(&me.email).is_some());
    me.email
}

#[tokio::test]
async fn websocket_vault_changed_triggers_pull() {
    let server = MockServer::start().await;
    let (a, b, _vault) = two_devices(&server).await;
    let events = EventStream::spawn(b.api.clone(), EventStreamConfig::default());
    let mut rx = events.subscribe();
    b.engine.attach_events(events.subscribe());
    b.engine.start();
    // Wait for the connection (hello).
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(WsEvent::Event(cc_protocol::events::ServerEvent::Hello { .. })) =
                rx.recv().await
            {
                break;
            }
        }
    })
    .await
    .unwrap();

    let h = host("pushed-by-a", "10.0.0.9");
    a.engine.put(p(VaultObject::Host(h.clone()))).await.unwrap();
    sync_ok(&a.engine).await;
    eventually(5, || async { b.store.get(h.id).await.unwrap().is_some() }).await;

    // Server drops the socket as "lagged" (4002): client reconnects and pulls.
    let h2 = host("after-lag", "10.0.0.10");
    server.close_websockets(CLOSE_CODE_LAGGED);
    a.engine
        .put(p(VaultObject::Host(h2.clone())))
        .await
        .unwrap();
    sync_ok(&a.engine).await;
    eventually(10, || async { b.store.get(h2.id).await.unwrap().is_some() }).await;

    // Access token behind the socket expired/rotated (4003): reconnect + pull.
    let h3 = host("after-token-expiry", "10.0.0.11");
    server.close_websockets(CLOSE_CODE_TOKEN_EXPIRED);
    a.engine
        .put(p(VaultObject::Host(h3.clone())))
        .await
        .unwrap();
    sync_ok(&a.engine).await;
    eventually(10, || async { b.store.get(h3.id).await.unwrap().is_some() }).await;

    // Revocation arrives over WS: engine stops and the stream terminates.
    let mut b_events = b.engine.subscribe_events();
    server.revoke_device(b.keys.device_id);
    eventually(5, || async {
        b.engine.stop_reason() == Some(StopReason::DeviceRevoked)
    })
    .await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(WsEvent::Terminated { reason }) = rx.recv().await {
                assert_eq!(reason, StopReason::DeviceRevoked);
                break;
            }
        }
    })
    .await
    .unwrap();
    assert!(
        std::iter::from_fn(|| b_events.try_recv().ok()).any(|e| matches!(
            e,
            SyncEvent::Stopped {
                reason: StopReason::DeviceRevoked,
                ..
            }
        ))
    );
    events.shutdown().await;
}

#[tokio::test]
async fn revoked_device_gets_403_and_engine_stops() {
    let server = MockServer::start().await;
    let (a, b, _vault) = two_devices(&server).await;
    let h = host("web", "10.0.0.1");
    b.engine.put(p(VaultObject::Host(h.clone()))).await.unwrap();
    a.api
        .revoke_device(b.keys.device_id, &Default::default())
        .await
        .unwrap();
    let err = b.engine.sync_now().await.unwrap_err();
    assert!(
        matches!(err, SyncError::Api(ApiError::DeviceRevoked)),
        "{err}"
    );
    assert_eq!(err.stop_reason(), Some(StopReason::DeviceRevoked));
    let st = b.engine.status();
    assert_eq!(st.phase, SyncPhase::Stopped);
    assert_eq!(st.stop_reason, Some(StopReason::DeviceRevoked));
    assert!(matches!(
        b.engine.sync_now().await,
        Err(SyncError::Stopped(StopReason::DeviceRevoked))
    ));
    // Worker refuses to start; local data stays readable.
    b.engine.start();
    assert_eq!(host_named(&b.store, h.id).await.as_deref(), Some("web"));
}

#[tokio::test]
async fn untrusted_device_is_stopped() {
    let server = MockServer::start().await;
    let (a, _b, vault) = two_devices(&server).await;
    let kc = DeviceKeys::generate();
    let email = a.api.me().await.unwrap().email;
    let api_c = login(&server, &email, &kc).await; // logged in, never attested
    let c = client(&server, api_c, kc, vault.vault_id).await;
    let err = c.engine.sync_now().await.unwrap_err();
    assert_eq!(err.stop_reason(), Some(StopReason::NotTrusted), "{err}");
}

#[tokio::test]
async fn malformed_responses_do_not_corrupt_local_state() {
    let server = MockServer::start().await;
    let (a, b, vault) = two_devices(&server).await;
    let h1 = host("one", "10.0.0.1");
    let h2 = host("two", "10.0.0.2");
    a.engine
        .put(p(VaultObject::Host(h1.clone())))
        .await
        .unwrap();
    a.engine
        .put(p(VaultObject::Host(h2.clone())))
        .await
        .unwrap();
    sync_ok(&a.engine).await;

    let snapshot_of = |c: &Client| {
        let s = c.storage.clone();
        let v = vault.vault_id;
        async move {
            s.read(move |tx| {
                let c = tx.get_sync_cursor(v)?;
                Ok::<_, cc_storage_core::StorageError>((
                    tx.list_objects(v, true)?,
                    (
                        c.last_sequence,
                        c.snapshot_complete,
                        c.snapshot_cursor,
                        c.snapshot_start_sequence,
                    ),
                    tx.outbox_list(v)?,
                ))
            })
            .await
            .unwrap()
        }
    };
    let before = snapshot_of(&b).await;

    // Out-of-order sequences in a snapshot page.
    server.inject(
        paths::SYNC_SNAPSHOT,
        1,
        FaultAction::Rewrite(Arc::new(|v| {
            if let Some(objs) = v["objects"].as_array_mut() {
                objs.reverse();
            }
        })),
    );
    let err = b.engine.sync_now().await.unwrap_err();
    assert!(matches!(err, SyncError::MalformedResponse(_)), "{err}");
    assert_eq!(b.engine.status().phase, SyncPhase::Error);
    // Truncated JSON.
    server.inject(paths::SYNC_SNAPSHOT, 1, FaultAction::InvalidJson);
    let err = b.engine.sync_now().await.unwrap_err();
    assert!(
        matches!(err, SyncError::Api(ApiError::InvalidResponse { .. })),
        "{err}"
    );
    // Live object without body.
    server.inject(
        paths::SYNC_SNAPSHOT,
        1,
        FaultAction::Rewrite(Arc::new(|v| {
            v["objects"][0]["body"] = serde_json::Value::Null;
        })),
    );
    assert!(matches!(
        b.engine.sync_now().await,
        Err(SyncError::MalformedResponse(_))
    ));
    assert_eq!(snapshot_of(&b).await, before, "nothing applied");

    // Now sync properly, then attack the push path.
    sync_ok(&b.engine).await;
    b.engine
        .put(p(VaultObject::Host(renamed(h1.clone(), "b-edit"))))
        .await
        .unwrap();
    server.inject(
        paths::SYNC_PUSH,
        1,
        FaultAction::Rewrite(Arc::new(|v| {
            v["results"][0]["revision"] = serde_json::json!(99);
        })),
    );
    let err = b.engine.sync_now().await.unwrap_err();
    assert!(matches!(err, SyncError::MalformedResponse(_)), "{err}");
    let entries = b
        .storage
        .read(move |tx| tx.outbox_list(vault.vault_id))
        .await
        .unwrap();
    assert_eq!(entries.len(), 1, "entry kept for idempotent retry");
    assert_eq!(entries[0].state, OutboxState::Queued);
    let r = sync_ok(&b.engine).await;
    assert_eq!(r.replayed, 1);
    assert_eq!(b.engine.status().pending, 0);

    // `has_more` with an empty page would loop forever: rejected.
    server.inject(
        paths::SYNC_CHANGES,
        1,
        FaultAction::Rewrite(Arc::new(|v| {
            v["has_more"] = serde_json::json!(true);
        })),
    );
    assert!(matches!(
        b.engine.sync_now().await,
        Err(SyncError::MalformedResponse(_))
    ));

    // Tampered ciphertext passes the structure check: stored as received,
    // flagged, and never decrypts.
    a.engine
        .put(p(VaultObject::Host(renamed(h2.clone(), "a-edit"))))
        .await
        .unwrap();
    sync_ok(&a.engine).await;
    server.inject(
        paths::SYNC_CHANGES,
        1,
        FaultAction::Rewrite(Arc::new(|v| {
            v["changes"][0]["body"]["ciphertext"] = serde_json::json!("AAAAAAAA");
        })),
    );
    let mut events = b.engine.subscribe_events();
    sync_ok(&b.engine).await;
    assert!(std::iter::from_fn(|| events.try_recv().ok())
        .any(|e| matches!(e, SyncEvent::IntegrityWarning { object_id, .. } if object_id == h2.id)));
    assert!(matches!(b.store.get(h2.id).await, Err(SyncError::Codec(_))));
}

#[tokio::test]
async fn rejected_mutation_is_isolated_and_recoverable() {
    let server = MockServer::start().await;
    let (a, _b, vault) = two_devices(&server).await;
    let first = host("first", "10.0.0.1");
    let second = host("second", "10.0.0.2");
    a.engine
        .put(p(VaultObject::Host(first.clone())))
        .await
        .unwrap();
    a.engine
        .put(p(VaultObject::Host(second.clone())))
        .await
        .unwrap();
    // The batch is rejected (400), then the isolated retry of `first` too:
    // only `first` fails, `second` goes through.
    server.inject(
        paths::SYNC_PUSH,
        2,
        FaultAction::Error(ErrorCode::BadRequest),
    );
    let r = sync_ok(&a.engine).await;
    assert_eq!((r.pushed, r.failed), (1, 1));
    let st = a.engine.status();
    assert_eq!((st.pending, st.failed), (0, 1));
    assert_eq!(server.live_objects(vault.vault_id), vec![second.id]);
    let failed = a
        .storage
        .read(move |tx| tx.outbox_list(vault.vault_id))
        .await
        .unwrap();
    assert_eq!(failed[0].state, OutboxState::Failed);
    assert!(failed[0].last_error.is_some());
    // A new local edit re-queues it under a new mutation id.
    a.engine
        .put(p(VaultObject::Host(renamed(first.clone(), "first-2"))))
        .await
        .unwrap();
    sync_ok(&a.engine).await;
    assert_eq!(a.engine.status().failed, 0);
    assert_eq!(server.live_objects(vault.vault_id).len(), 2);
}

#[tokio::test]
async fn rate_limit_and_server_errors_back_off_then_recover() {
    let server = MockServer::start().await;
    let (a, _b, vault) = two_devices(&server).await;
    a.engine
        .put(p(VaultObject::Host(host("x", "10.0.0.1"))))
        .await
        .unwrap();
    server.inject(
        paths::SYNC_PUSH,
        1,
        FaultAction::RateLimit {
            retry_after_seconds: 1,
        },
    );
    let err = a.engine.sync_now().await.unwrap_err();
    match &err {
        SyncError::Api(ApiError::RateLimited { retry_after }) => {
            assert_eq!(*retry_after, Some(Duration::from_secs(1)))
        }
        e => panic!("{e}"),
    }
    assert_eq!(a.engine.status().phase, SyncPhase::Error);
    // Worker: 429 (Retry-After 1 s), then two 503s with short backoff.
    server.inject(
        paths::SYNC_PUSH,
        1,
        FaultAction::RateLimit {
            retry_after_seconds: 1,
        },
    );
    server.inject(
        paths::SYNC_PUSH,
        2,
        FaultAction::Error(ErrorCode::Unavailable),
    );
    let started = tokio::time::Instant::now();
    a.engine.start();
    eventually(10, || async { a.engine.status().pending == 0 }).await;
    assert!(
        started.elapsed() >= Duration::from_millis(900),
        "Retry-After honoured"
    );
    assert_eq!(server.live_objects(vault.vault_id).len(), 1);
    assert_eq!(a.engine.status().phase, SyncPhase::Idle);
    a.engine.stop().await;
    assert!(matches!(
        a.engine.sync_now().await,
        Err(SyncError::Stopped(StopReason::Shutdown))
    ));
}

#[tokio::test]
async fn conflict_waits_while_vault_is_locked() {
    let server = MockServer::start().await;
    let (a, b, _vault) = two_devices(&server).await;
    let h = host("web", "10.0.0.1");
    a.engine.put(p(VaultObject::Host(h.clone()))).await.unwrap();
    sync_ok(&a.engine).await;
    sync_ok(&b.engine).await;
    a.engine
        .put(p(VaultObject::Host(renamed(h.clone(), "A"))))
        .await
        .unwrap();
    b.engine
        .put(p(VaultObject::Host(renamed(h.clone(), "B"))))
        .await
        .unwrap();
    sync_ok(&a.engine).await;

    b.codec.set_locked(true);
    let r = sync_ok(&b.engine).await; // ciphertext still moves while locked
    assert_eq!((r.conflicts, r.resolved), (1, 0));
    assert_eq!(b.engine.status().conflicts, 1);
    assert_eq!(
        b.store.stored(h.id).await.unwrap().unwrap().local_state,
        LocalState::Conflict
    );

    b.codec.set_locked(false);
    let r = sync_ok(&b.engine).await;
    assert_eq!(r.resolved, 1);
    assert_eq!(b.engine.status().conflicts, 0);
    assert_eq!(host_named(&b.store, h.id).await.as_deref(), Some("A"));
    assert_eq!(b.store.list().await.unwrap().0.len(), 2);
}

#[tokio::test]
async fn gone_cursor_triggers_resnapshot() {
    let server = MockServer::start().await;
    let (a, b, _vault) = two_devices(&server).await;
    let keep = host("keep", "10.0.5.1");
    let doomed = host("doomed", "10.0.5.2");
    a.engine
        .put(p(VaultObject::Host(keep.clone())))
        .await
        .unwrap();
    a.engine
        .put(p(VaultObject::Host(doomed.clone())))
        .await
        .unwrap();
    sync_ok(&a.engine).await;
    sync_ok(&b.engine).await;
    a.engine.delete(doomed.id).await.unwrap();
    let fresh = host("fresh", "10.0.5.3");
    a.engine
        .put(p(VaultObject::Host(fresh.clone())))
        .await
        .unwrap();
    sync_ok(&a.engine).await;

    // B's cursor is below a (simulated) tombstone horizon.
    server.inject(paths::SYNC_CHANGES, 1, FaultAction::Error(ErrorCode::Gone));
    let mut events = b.engine.subscribe_events();
    let r = sync_ok(&b.engine).await;
    assert_eq!(r.snapshot_pages, 1);
    assert!(
        b.store.get(doomed.id).await.unwrap().is_none(),
        "vanished object dropped"
    );
    assert!(b.store.get(fresh.id).await.unwrap().is_some());
    assert!(b.store.get(keep.id).await.unwrap().is_some());
    let evs: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();
    assert!(evs.iter().any(
        |e| matches!(e, SyncEvent::ObjectRemoved { object_id, .. } if *object_id == doomed.id)
    ));
    assert!(evs
        .iter()
        .any(|e| matches!(e, SyncEvent::SnapshotCompleted { .. })));
}
