//! Server rollback / restore recovery (protocol 1.3 epoch, ADR-0103
//! addendum) against the mock server's restore drills.

mod common;

use cc_models::host::Host;
use cc_models::{ObjectPayload, VaultObject};
use cc_protocol::paths;
use cc_protocol::VaultId;
use cc_storage_core::{LocalState, SyncCursor};
use cc_sync_core::mock::{FaultAction, MockServer};
use cc_sync_core::*;
use common::*;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn host(name: &str) -> Host {
    Host::new(name, "10.0.0.1")
}

fn p(h: &Host) -> ObjectPayload {
    ObjectPayload::new(VaultObject::Host(h.clone()))
}

fn renamed(h: &Host, name: &str) -> Host {
    let mut h = h.clone();
    h.name = name.into();
    h.updated_at = chrono::Utc::now();
    h
}

/// Names of the live hosts a device holds.
async fn names(store: &ObjectStore) -> BTreeSet<String> {
    let (objs, failed) = store.list().await.unwrap();
    assert!(failed.is_empty(), "undecryptable local objects: {failed:?}");
    objs.into_iter()
        .filter_map(|o| match o.payload.object {
            VaultObject::Host(h) => Some(h.name),
            _ => None,
        })
        .collect()
}

/// Names of the live hosts on the server (decrypted with the test codec).
fn server_names(server: &MockServer, vault_id: VaultId, codec: &TestCodec) -> BTreeSet<String> {
    server
        .live_objects(vault_id)
        .into_iter()
        .map(|id| {
            let o = server.object(vault_id, id).unwrap();
            match codec
                .decrypt(id, o.revision, o.body.as_ref().unwrap())
                .unwrap()
                .object
            {
                VaultObject::Host(h) => h.name,
                other => panic!("unexpected {other:?}"),
            }
        })
        .collect()
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

async fn cursor(c: &Client, vault_id: VaultId) -> SyncCursor {
    c.storage
        .read(move |tx| tx.get_sync_cursor(vault_id))
        .await
        .unwrap()
}

fn rollback_reasons(rx: &mut tokio::sync::broadcast::Receiver<SyncEvent>) -> Vec<RollbackReason> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|e| match e {
            SyncEvent::ServerRollbackDetected { reason, .. } => Some(reason),
            _ => None,
        })
        .collect()
}

async fn put(c: &Client, h: &Host) {
    c.engine.put(p(h)).await.unwrap();
}

/// Everything consistent: no pending work, no recovery, same live set on
/// both devices and the server.
async fn assert_converged(server: &MockServer, a: &Client, b: &Client, v: VaultId, want: &[&str]) {
    let want = set(want);
    assert_eq!(server_names(server, v, &a.codec), want, "server");
    assert_eq!(names(&a.store).await, want, "device A");
    assert_eq!(names(&b.store).await, want, "device B");
    for c in [a, b] {
        let st = c.engine.status();
        assert_eq!((st.pending, st.conflicts, st.failed), (0, 0, 0));
        assert_eq!(st.rollback_recovery, None);
    }
}

#[tokio::test]
async fn restore_with_epoch_rotation_repushes_lost_objects_and_devices_converge() {
    let server = MockServer::start().await;
    let (a, b, vault) = two_devices(&server).await;
    let v = vault.vault_id;
    let (h1, h2) = (host("h1"), host("h2"));
    put(&a, &h1).await;
    put(&a, &h2).await;
    sync_ok(&a.engine).await;
    sync_ok(&b.engine).await;
    let backup = server.checkpoint(v);

    // Work after the backup, on both devices.
    let h3 = host("h3");
    put(&a, &renamed(&h1, "h1-v2")).await;
    put(&a, &h3).await;
    a.engine.delete(h2.id).await.unwrap();
    sync_ok(&a.engine).await;
    let h4 = host("h4");
    put(&b, &h4).await;
    sync_ok(&b.engine).await;
    sync_ok(&a.engine).await;
    let before = server.latest_sequence(v);
    let old_epoch = server.epoch(v).unwrap();
    assert_eq!(cursor(&a, v).await.epoch, Some(old_epoch), "epoch stored");

    server.simulate_restore(v, backup);
    assert!(server.latest_sequence(v) < before);
    assert_eq!(server_names(&server, v, &a.codec), set(&["h1", "h2"]));

    let mut ev_a = a.engine.subscribe_events();
    let r = sync_ok(&a.engine).await;
    assert_eq!(
        rollback_reasons(&mut ev_a),
        vec![RollbackReason::EpochChanged]
    );
    assert_eq!(r.rollback_recoveries, 1);
    assert_eq!(r.repushed, 3, "h1 (older on server), h3 and h4 (lost)");
    assert_eq!(r.tombstones_reapplied, 1, "h2");
    assert_eq!(
        server_names(&server, v, &a.codec),
        set(&["h1-v2", "h3", "h4"])
    );
    assert!(server.object(v, h2.id).unwrap().deleted);
    assert_eq!(cursor(&a, v).await.epoch, server.epoch(v));

    // B learns about the restore from the epoch; everything it holds is
    // already back on the server, byte for byte — nothing to push.
    let pushes = server.hits(paths::SYNC_PUSH);
    let seq = server.latest_sequence(v);
    let mut ev_b = b.engine.subscribe_events();
    let r = sync_ok(&b.engine).await;
    assert_eq!(
        rollback_reasons(&mut ev_b),
        vec![RollbackReason::EpochChanged]
    );
    assert_eq!(
        (r.rollback_recoveries, r.repushed, r.tombstones_reapplied),
        (1, 0, 0)
    );
    assert_eq!((r.conflicts, r.resolved), (0, 0));
    assert_eq!(server.hits(paths::SYNC_PUSH), pushes, "no duplicate pushes");
    assert_eq!(server.latest_sequence(v), seq);
    sync_ok(&a.engine).await;
    assert_converged(&server, &a, &b, v, &["h1-v2", "h3", "h4"]).await;

    // Steady state again: no further recovery.
    let r = sync_ok(&a.engine).await;
    assert_eq!(r.rollback_recoveries, 0);
}

#[tokio::test]
async fn restore_without_epoch_rotation_is_detected_by_sequence() {
    let server = MockServer::start().await;
    let (a, b, vault) = two_devices(&server).await;
    let v = vault.vault_id;
    let h1 = host("h1");
    put(&a, &h1).await;
    sync_ok(&a.engine).await;
    let backup = server.checkpoint(v);
    let (h2, h3) = (host("h2"), host("h3"));
    put(&a, &h2).await;
    put(&a, &h3).await;
    sync_ok(&a.engine).await;
    sync_ok(&b.engine).await;

    server.simulate_restore_keep_epoch(v, backup);
    let mut ev = a.engine.subscribe_events();
    let r = sync_ok(&a.engine).await;
    assert_eq!(
        rollback_reasons(&mut ev),
        vec![RollbackReason::SequenceRegressed]
    );
    assert_eq!((r.rollback_recoveries, r.repushed), (1, 2));
    // A re-pushed exactly what was lost (same ciphertexts, same sequences),
    // so B — without an epoch change — has nothing to notice or fix.
    let r = sync_ok(&b.engine).await;
    assert_eq!(r.rollback_recoveries, 0);
    assert_converged(&server, &a, &b, v, &["h1", "h2", "h3"]).await;
}

#[tokio::test]
async fn push_accepted_with_a_reused_sequence_triggers_recovery() {
    let server = MockServer::start().await;
    let (a, b, vault) = two_devices(&server).await;
    let v = vault.vault_id;
    put(&a, &host("h1")).await;
    put(&a, &host("h2")).await;
    sync_ok(&a.engine).await;
    let backup = server.checkpoint(v); // sequence 2

    // A pushes h3..h5 (sequences 3..5) but its final pull fails, so its
    // cursor stays at 2 while it holds sequences up to 5.
    for n in ["h3", "h4", "h5"] {
        put(&a, &host(n)).await;
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let n = calls.clone();
    server.inject(
        paths::SYNC_CHANGES,
        2,
        FaultAction::Rewrite(Arc::new(move |v| {
            if n.fetch_add(1, Ordering::SeqCst) == 1 {
                if let Some(c) = v["changes"].as_array_mut() {
                    c.reverse();
                }
            }
        })),
    );
    assert!(matches!(
        a.engine.sync_now().await,
        Err(SyncError::MalformedResponse(_))
    ));
    assert_eq!(server.latest_sequence(v), 5);
    assert_eq!(cursor(&a, v).await.last_sequence, 2);

    // Restore without epoch rotation; B (which never saw h3..h5) writes two
    // objects, so the server's latest (4) is not below A's cursor (2).
    server.simulate_restore_keep_epoch(v, backup);
    put(&b, &host("b1")).await;
    put(&b, &host("b2")).await;
    let r = sync_ok(&b.engine).await;
    assert_eq!(r.rollback_recoveries, 0, "B cannot tell");
    assert_eq!(server.latest_sequence(v), 4);

    // A's next push is accepted at sequence 5 — which A already holds for h5.
    put(&a, &host("a6")).await;
    let mut ev = a.engine.subscribe_events();
    let r = sync_ok(&a.engine).await;
    assert_eq!(
        rollback_reasons(&mut ev),
        vec![RollbackReason::SequenceReused]
    );
    assert_eq!((r.rollback_recoveries, r.repushed), (1, 3));
    sync_ok(&b.engine).await;
    assert_converged(
        &server,
        &a,
        &b,
        v,
        &["h1", "h2", "h3", "h4", "h5", "b1", "b2", "a6"],
    )
    .await;
}

#[tokio::test]
async fn epoch_rotation_alone_resyncs_without_pushing_anything() {
    let server = MockServer::start().await;
    let (a, b, vault) = two_devices(&server).await;
    let v = vault.vault_id;
    let (h1, h2, h3) = (host("h1"), host("h2"), host("h3"));
    put(&a, &h1).await;
    put(&a, &h2).await;
    sync_ok(&a.engine).await;
    sync_ok(&b.engine).await;
    put(&b, &h3).await;
    b.engine.delete(h2.id).await.unwrap();
    sync_ok(&b.engine).await;
    sync_ok(&a.engine).await;

    let seq = server.latest_sequence(v);
    let pushes = server.hits(paths::SYNC_PUSH);
    let new_epoch = server.rotate_epoch(v);
    for c in [&a, &b] {
        let mut ev = c.engine.subscribe_events();
        let r = sync_ok(&c.engine).await;
        assert_eq!(
            rollback_reasons(&mut ev),
            vec![RollbackReason::EpochChanged]
        );
        assert_eq!(r.rollback_recoveries, 1);
        assert_eq!(
            (r.pushed, r.repushed, r.tombstones_reapplied, r.conflicts),
            (0, 0, 0, 0)
        );
        assert_eq!(cursor(c, v).await.epoch, Some(new_epoch));
        assert_eq!(cursor(c, v).await.last_sequence, seq);
        let r = sync_ok(&c.engine).await;
        assert_eq!(r.rollback_recoveries, 0, "recovered once");
    }
    assert_eq!(server.hits(paths::SYNC_PUSH), pushes, "nothing pushed");
    assert_eq!(server.latest_sequence(v), seq);
    assert_converged(&server, &a, &b, v, &["h1", "h3"]).await;
}

#[tokio::test]
async fn interrupted_recovery_listing_resumes_after_restart() {
    let server = MockServer::start().await;
    let (a, b, vault) = two_devices(&server).await;
    let v = vault.vault_id;
    let mut all = Vec::new();
    for i in 0..6 {
        let h = host(&format!("h{i}"));
        put(&a, &h).await;
        all.push(h);
    }
    sync_ok(&a.engine).await;
    let backup = server.checkpoint(v);
    for i in 6..10 {
        let h = host(&format!("h{i}"));
        put(&a, &h).await;
        all.push(h);
    }
    sync_ok(&a.engine).await;
    sync_ok(&b.engine).await;
    server.simulate_restore(v, backup);

    // A restarts with small pages; the 3rd `changes` request (2nd page of the
    // recovery listing) comes back malformed.
    let small = |c: &Client| {
        let mut cfg = fast_config(c.keys.device_id);
        cfg.page_limit = 2;
        cfg
    };
    let engine = SyncEngine::new(a.store.clone(), a.api.clone(), small(&a))
        .await
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let n = calls.clone();
    server.inject(
        paths::SYNC_CHANGES,
        3,
        FaultAction::Rewrite(Arc::new(move |v| {
            if n.fetch_add(1, Ordering::SeqCst) == 2 {
                if let Some(c) = v["changes"].as_array_mut() {
                    c.reverse();
                }
            }
        })),
    );
    let mut ev = engine.subscribe_events();
    let err = engine.sync_now().await.unwrap_err();
    assert!(matches!(err, SyncError::MalformedResponse(_)), "{err}");
    assert_eq!(
        rollback_reasons(&mut ev),
        vec![RollbackReason::EpochChanged]
    );
    let rec = cursor(&a, v).await.recovery.expect("recovery persisted");
    assert!(!rec.listed);
    assert_eq!(rec.cursor, 2, "first listing page stored");
    assert_eq!(
        engine.status().rollback_recovery,
        Some(RollbackReason::EpochChanged)
    );
    assert_eq!(
        server.live_objects(v).len(),
        6,
        "nothing pushed mid-recovery"
    );
    engine.stop().await;

    // "App restart": a new engine on the same database resumes the listing
    // (no second detection event) and finishes the recovery.
    let engine = SyncEngine::new(a.store.clone(), a.api.clone(), small(&a))
        .await
        .unwrap();
    assert_eq!(
        engine.status().rollback_recovery,
        Some(RollbackReason::EpochChanged)
    );
    let mut ev = engine.subscribe_events();
    let r = sync_ok(&engine).await;
    assert!(rollback_reasons(&mut ev).is_empty());
    assert_eq!((r.rollback_recoveries, r.repushed), (1, 4));
    assert_eq!(engine.status().rollback_recovery, None);
    sync_ok(&b.engine).await;
    let want: Vec<String> = all.iter().map(|h| h.name.clone()).collect();
    let want: Vec<&str> = want.iter().map(String::as_str).collect();
    assert_converged(&server, &a, &b, v, &want).await;
}

#[tokio::test]
async fn reconcile_waits_for_unlock_when_reencryption_is_needed() {
    let server = MockServer::start().await;
    let (a, b, vault) = two_devices(&server).await;
    let v = vault.vault_id;
    let h1 = host("h1");
    put(&a, &h1).await;
    sync_ok(&a.engine).await;
    let backup = server.checkpoint(v);
    // Two edits after the backup: server keeps revision 1, A holds 3 → the
    // re-push needs a fresh ciphertext for revision 2.
    put(&a, &renamed(&h1, "h1-v2")).await;
    sync_ok(&a.engine).await;
    put(&a, &renamed(&h1, "h1-v3")).await;
    sync_ok(&a.engine).await;
    // Created then edited after the backup: lost, needs revision 1 again.
    let h2 = host("h2");
    put(&a, &h2).await;
    sync_ok(&a.engine).await;
    put(&a, &renamed(&h2, "h2-v2")).await;
    sync_ok(&a.engine).await;
    sync_ok(&b.engine).await;
    server.simulate_restore(v, backup);

    a.codec.set_locked(true);
    let err = a.engine.sync_now().await.unwrap_err();
    assert!(matches!(err, SyncError::Codec(CodecError::Locked)), "{err}");
    let rec = cursor(&a, v).await.recovery.expect("still pending");
    assert!(rec.listed, "listing done; reconcile waits for keys");
    assert_eq!(server_names(&server, v, &b.codec), set(&["h1"]));

    a.codec.set_locked(false);
    let r = sync_ok(&a.engine).await;
    assert_eq!((r.rollback_recoveries, r.repushed), (1, 2));
    let o1 = server.object(v, h1.id).unwrap();
    assert_eq!(o1.revision, 2);
    let o2 = server.object(v, h2.id).unwrap();
    assert_eq!(o2.revision, 1);
    // B holds the same payloads at higher revisions: it adopts A's restored
    // copies instead of pushing identical content again.
    let pushes = server.hits(paths::SYNC_PUSH);
    let r = sync_ok(&b.engine).await;
    assert_eq!((r.rollback_recoveries, r.repushed), (1, 0));
    assert_eq!(server.hits(paths::SYNC_PUSH), pushes);
    assert_converged(&server, &a, &b, v, &["h1-v3", "h2-v2"]).await;
}

#[tokio::test]
async fn local_tombstone_is_reapplied_and_reaches_a_lagging_device() {
    let server = MockServer::start().await;
    let (a, b, vault) = two_devices(&server).await;
    let v = vault.vault_id;
    let (keep, gone) = (host("keep"), host("gone"));
    put(&a, &keep).await;
    put(&a, &gone).await;
    sync_ok(&a.engine).await;
    sync_ok(&b.engine).await;
    let backup = server.checkpoint(v);
    // A deletes after the backup; B does not sync before the restore.
    a.engine.delete(gone.id).await.unwrap();
    sync_ok(&a.engine).await;
    server.simulate_restore(v, backup);
    assert!(
        !server.object(v, gone.id).unwrap().deleted,
        "restore revived it"
    );

    let r = sync_ok(&a.engine).await;
    assert_eq!(
        (r.rollback_recoveries, r.tombstones_reapplied, r.repushed),
        (1, 1, 0)
    );
    let o = server.object(v, gone.id).unwrap();
    assert!(o.deleted);
    assert_eq!(o.revision, 2);

    // B's cursor equals the restored sequence: only the epoch tells.
    let r = sync_ok(&b.engine).await;
    assert_eq!(r.rollback_recoveries, 1);
    assert!(b.store.get(gone.id).await.unwrap().is_none());
    assert_converged(&server, &a, &b, v, &["keep"]).await;
}

#[tokio::test]
async fn pending_edit_does_not_overwrite_another_devices_post_restore_write() {
    let server = MockServer::start().await;
    let (a, b, vault) = two_devices(&server).await;
    let v = vault.vault_id;
    let h = host("web");
    put(&a, &h).await;
    sync_ok(&a.engine).await;
    sync_ok(&b.engine).await;
    let backup = server.checkpoint(v);
    put(&a, &renamed(&h, "A-1")).await; // revision 2 (lost by the restore)
    sync_ok(&a.engine).await;
    server.simulate_restore(v, backup);

    // A edits offline on top of its revision 2.
    put(&a, &renamed(&h, "A-2")).await;
    // B, which never saw A-1, writes revision 2 on the restored server.
    put(&b, &renamed(&h, "B-1")).await;
    sync_ok(&b.engine).await;
    assert_eq!(server.object(v, h.id).unwrap().revision, 2);

    // A must not push "A-2" on base 2 over B's different revision 2.
    let mut ev = a.engine.subscribe_events();
    let r = sync_ok(&a.engine).await;
    assert_eq!(r.rollback_recoveries, 1);
    assert_eq!((r.conflicts, r.resolved), (0, 1), "fork resolved by policy");
    let evs: Vec<_> = std::iter::from_fn(|| ev.try_recv().ok()).collect();
    assert!(evs.iter().any(|e| matches!(
        e,
        SyncEvent::ConflictResolved {
            resolution: ConflictResolution::RemoteKeptLocalCopied,
            ..
        }
    )));
    sync_ok(&b.engine).await;
    let copy = format!("A-2{CONFLICT_COPY_SUFFIX}");
    assert_converged(&server, &a, &b, v, &["B-1", &copy]).await;
}

#[tokio::test]
async fn vault_info_epoch_is_stored_and_a_change_starts_recovery() {
    let server = MockServer::start().await;
    let (a, _b, vault) = two_devices(&server).await;
    let v = vault.vault_id;
    let info = a.api.get_vault(v).await.unwrap();
    assert_eq!(a.engine.observe_vault_info(&info).await.unwrap(), None);
    assert_eq!(cursor(&a, v).await.epoch, info.epoch, "first epoch seen");
    put(&a, &host("h1")).await;
    sync_ok(&a.engine).await;

    server.rotate_epoch(v);
    let info = a.api.get_vault(v).await.unwrap();
    let mut ev = a.engine.subscribe_events();
    assert_eq!(
        a.engine.observe_vault_info(&info).await.unwrap(),
        Some(RollbackReason::EpochChanged)
    );
    // Idempotent while pending.
    assert_eq!(
        a.engine.observe_vault_info(&info).await.unwrap(),
        Some(RollbackReason::EpochChanged)
    );
    assert_eq!(
        rollback_reasons(&mut ev),
        vec![RollbackReason::EpochChanged]
    );
    assert_eq!(
        a.engine.status().rollback_recovery,
        Some(RollbackReason::EpochChanged)
    );
    let r = sync_ok(&a.engine).await;
    assert_eq!((r.rollback_recoveries, r.repushed), (1, 0));
    assert_eq!(cursor(&a, v).await.epoch, info.epoch);
    assert_eq!(a.engine.observe_vault_info(&info).await.unwrap(), None);

    // Unknown epochs never trigger.
    let mut no_epoch = info.clone();
    no_epoch.epoch = None;
    assert_eq!(a.engine.observe_vault_info(&no_epoch).await.unwrap(), None);
    assert_eq!(names(&a.store).await, set(&["h1"]));
}

#[tokio::test]
async fn no_local_state_is_lost_across_a_restore_with_mixed_edits() {
    // Invariant check over every object state a device can be in when the
    // server is restored: synced, pending edit, pending create, pending
    // delete, conflict copy.
    let server = MockServer::start().await;
    let (a, b, vault) = two_devices(&server).await;
    let v = vault.vault_id;
    let hosts: Vec<Host> = (0..5).map(|i| host(&format!("s{i}"))).collect();
    for h in &hosts {
        put(&a, h).await;
    }
    sync_ok(&a.engine).await;
    sync_ok(&b.engine).await;
    let backup = server.checkpoint(v);
    put(&a, &renamed(&hosts[0], "s0-synced-edit")).await;
    sync_ok(&a.engine).await;
    sync_ok(&b.engine).await;
    server.simulate_restore(v, backup);
    // Local, unpushed at restore time:
    put(&a, &renamed(&hosts[1], "s1-pending")).await;
    let fresh = host("fresh");
    put(&a, &fresh).await;
    a.engine.delete(hosts[2].id).await.unwrap();
    put(&b, &renamed(&hosts[3], "s3-b-pending")).await;

    let before_a = names(&a.store).await;
    let before_b = names(&b.store).await;
    sync_ok(&a.engine).await;
    sync_ok(&b.engine).await;
    sync_ok(&a.engine).await;
    let want = [
        "s0-synced-edit",
        "s1-pending",
        "s3-b-pending",
        "s4",
        "fresh",
    ];
    assert_converged(&server, &a, &b, v, &want).await;
    // Everything either device held survives, except versions superseded by
    // a local edit / delete made on top of them.
    let superseded = set(&["s1", "s2", "s3"]);
    for n in before_a.iter().chain(&before_b) {
        assert!(
            want.contains(&n.as_str()) || superseded.contains(n),
            "{n} lost"
        );
    }
    for c in [&a, &b] {
        let objs = c.store.list().await.unwrap().0;
        assert!(objs.iter().all(|o| o.local_state == LocalState::Synced));
    }
}
