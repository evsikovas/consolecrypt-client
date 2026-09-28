//! Integration tests for cc-storage-core.

use cc_models::known_host::KnownHostSource;
use cc_models::KekClass;
use cc_protocol::sync::{EncryptedBody, MutationResult, OBJECT_FORMAT_V1};
use cc_protocol::vaults::{VaultInfo, VaultRole, VaultState};
use cc_protocol::{Bytes, DeviceId, ObjectId, UserId, VaultId};
use cc_storage_core::*;

fn key() -> DatabaseKey {
    DatabaseKey::from_bytes([0x5a; 32])
}

fn body(tag: u8) -> EncryptedBody {
    EncryptedBody {
        format: OBJECT_FORMAT_V1,
        ciphertext: Bytes::new(vec![tag; 64]),
        nonce: Bytes::new(vec![tag; 24]),
        wrapped_dek: Bytes::new(vec![tag; 48]),
        wrapped_dek_nonce: Bytes::new(vec![tag; 24]),
    }
}

fn db() -> Database {
    Database::open_in_memory(&key()).unwrap()
}

fn put(db: &mut Database, v: VaultId, o: ObjectId, tag: u8) -> LocalMutationOutcome {
    db.write(|tx| {
        tx.record_local_put::<StorageError, _>(v, o, Some(KekClass::Inventory), |_rev| {
            Ok(body(tag))
        })
    })
    .unwrap()
}

fn remote(o: ObjectId, revision: i64, sequence: i64, tag: Option<u8>) -> RemoteChange {
    RemoteChange {
        object_id: o,
        revision,
        sequence,
        deleted: tag.is_none(),
        body: tag.map(body),
        kek_class_hint: None,
        writer_device_id: DeviceId::new(),
        updated_at: chrono::Utc::now(),
    }
}

// ---- encryption / migrations ----------------------------------------------------

#[test]
fn database_file_is_unreadable_without_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("profiles").join("p1").join("vault.db");
    let marker = "plaintext-marker-3f9c1b";
    {
        let mut db = Database::open(&path, &key()).unwrap();
        db.write(|tx| tx.setting_set("marker", marker)).unwrap();
    }
    // Raw bytes: no SQLite header, no plaintext.
    let raw = std::fs::read(&path).unwrap();
    assert!(!raw.starts_with(b"SQLite format 3"));
    assert!(!raw.windows(marker.len()).any(|w| w == marker.as_bytes()));

    // Plain open (no key) cannot read it.
    let plain = rusqlite::Connection::open(&path).unwrap();
    let err = plain
        .query_row("SELECT count(*) FROM sqlite_master", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap_err();
    assert!(
        matches!(&err, rusqlite::Error::SqliteFailure(e, _) if e.code == rusqlite::ErrorCode::NotADatabase),
        "{err:?}"
    );
    drop(plain);

    // Wrong key is rejected cleanly.
    let err = Database::open(&path, &DatabaseKey::from_bytes([0x11; 32])).unwrap_err();
    assert!(
        matches!(err, StorageError::WrongKeyOrNotADatabase),
        "{err:?}"
    );

    // Right key works.
    let mut db = Database::open(&path, &key()).unwrap();
    let v: Option<String> = db.read(|tx| tx.setting_get("marker")).unwrap();
    assert_eq!(v.as_deref(), Some(marker));
}

#[test]
fn migrations_are_idempotent_and_downgrade_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v.db");
    {
        let db = Database::open(&path, &key()).unwrap();
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
    }
    {
        let db = Database::open(&path, &key()).unwrap();
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
    }
    {
        // Simulate a file written by a newer client.
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(&format!("PRAGMA key = \"x'{}'\";", "5A".repeat(32)))
            .unwrap();
        conn.pragma_update(None, "user_version", SCHEMA_VERSION + 5)
            .unwrap();
    }
    let err = Database::open(&path, &key()).unwrap_err();
    assert!(matches!(err, StorageError::SchemaTooNew { .. }), "{err:?}");
}

// ---- local mutations ----------------------------------------------------------------

#[test]
fn local_put_updates_objects_and_outbox_atomically() {
    let mut db = db();
    let (v, o) = (VaultId::new(), ObjectId::new());
    let out = put(&mut db, v, o, 1);
    let LocalMutationOutcome::Queued {
        base_revision,
        revision,
        ..
    } = out
    else {
        panic!("{out:?}")
    };
    assert_eq!((base_revision, revision), (0, 1));
    let (obj, entries) = db
        .read(|tx| Ok::<_, StorageError>((tx.get_object(v, o)?.unwrap(), tx.outbox_list(v)?)))
        .unwrap();
    assert_eq!(obj.local_state, LocalState::Pending);
    assert_eq!(obj.revision, 1);
    assert_eq!(obj.server_revision, 0);
    assert_eq!(obj.body, Some(body(1)));
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].base_revision, 0);
    assert_eq!(entries[0].body, Some(body(1)));

    // Encryption failure → nothing written.
    let o2 = ObjectId::new();
    let err = db
        .write(|tx| {
            tx.record_local_put::<StorageError, _>(v, o2, None, |_| {
                Err(StorageError::Invalid("codec locked".into()))
            })
        })
        .unwrap_err();
    assert!(matches!(err, StorageError::Invalid(_)));
    let (obj2, n) = db
        .read(|tx| Ok::<_, StorageError>((tx.get_object(v, o2)?, tx.outbox_list(v)?.len())))
        .unwrap();
    assert!(obj2.is_none());
    assert_eq!(n, 1);
}

#[test]
fn unsent_edits_coalesce_and_in_flight_edits_chain() {
    let mut db = db();
    let (v, o) = (VaultId::new(), ObjectId::new());
    let first = put(&mut db, v, o, 1);
    let second = put(&mut db, v, o, 2);
    let (
        LocalMutationOutcome::Queued {
            mutation_id: m1, ..
        },
        LocalMutationOutcome::Queued {
            mutation_id: m2,
            base_revision,
            ..
        },
    ) = (first, second)
    else {
        panic!()
    };
    assert_eq!(m1, m2, "never-sent entry is rewritten in place");
    assert_eq!(base_revision, 0);
    assert_eq!(db.read(|tx| tx.outbox_list(v)).unwrap().len(), 1);

    // Hand it to a push (attempted, outcome unknown) …
    let batch = db
        .write(|tx| tx.outbox_begin_batch(v, 500, usize::MAX))
        .unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].attempts, 1);
    assert_eq!(batch[0].body, Some(body(2)));
    // … then edit again: must chain, not touch the in-flight body.
    let third = put(&mut db, v, o, 3);
    let LocalMutationOutcome::Queued {
        mutation_id: m3,
        base_revision,
        revision,
    } = third
    else {
        panic!()
    };
    assert_ne!(m3, m1);
    assert_eq!((base_revision, revision), (1, 2));
    let entries = db.read(|tx| tx.outbox_list(v)).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].body, Some(body(2)));
    assert_eq!(entries[1].body, Some(body(3)));
    // Only the first entry of the object is eligible.
    let batch = db
        .write(|tx| tx.outbox_begin_batch(v, 500, usize::MAX))
        .unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].mutation_id, m1);
    assert_eq!(batch[0].attempts, 2);

    // Accept m1 → object still pending (m3 queued), server_revision = 1.
    let out = db
        .write(|tx| {
            tx.apply_push_results(
                v,
                1,
                &[MutationResult::Accepted {
                    mutation_id: m1,
                    object_id: o,
                    revision: 1,
                    sequence: 1,
                    replayed: false,
                }],
            )
        })
        .unwrap();
    assert!(matches!(
        out[0],
        PushOutcome::Committed { synced: false, .. }
    ));
    let obj = db.read(|tx| tx.get_object(v, o)).unwrap().unwrap();
    assert_eq!((obj.server_revision, obj.revision), (1, 2));
    assert_eq!(obj.local_state, LocalState::Pending);
    assert_eq!(obj.body, Some(body(3)));
}

#[test]
fn deleting_an_unsent_create_drops_it() {
    let mut db = db();
    let (v, o) = (VaultId::new(), ObjectId::new());
    put(&mut db, v, o, 1);
    let out = db.write(|tx| tx.record_local_delete(v, o)).unwrap();
    assert_eq!(out, LocalMutationOutcome::DroppedUnsent);
    let (obj, n) = db
        .read(|tx| Ok::<_, StorageError>((tx.get_object(v, o)?, tx.outbox_list(v)?.len())))
        .unwrap();
    assert!(obj.is_none());
    assert_eq!(n, 0);
    // Deleting something unknown is a no-op.
    assert_eq!(
        db.write(|tx| tx.record_local_delete(v, ObjectId::new()))
            .unwrap(),
        LocalMutationOutcome::NoOp
    );
}

#[test]
fn inconsistent_push_results_change_nothing() {
    let mut db = db();
    let (v, o) = (VaultId::new(), ObjectId::new());
    let LocalMutationOutcome::Queued { mutation_id, .. } = put(&mut db, v, o, 1) else {
        panic!()
    };
    db.write(|tx| tx.outbox_begin_batch(v, 10, usize::MAX))
        .unwrap();
    let err = db
        .write(|tx| {
            tx.apply_push_results(
                v,
                9,
                &[MutationResult::Accepted {
                    mutation_id,
                    object_id: o,
                    revision: 7, // must be base + 1 = 1
                    sequence: 9,
                    replayed: false,
                }],
            )
        })
        .unwrap_err();
    assert!(matches!(err, StorageError::Invalid(_)));
    let (obj, n) = db
        .read(|tx| Ok::<_, StorageError>((tx.get_object(v, o)?.unwrap(), tx.outbox_list(v)?.len())))
        .unwrap();
    assert_eq!(obj.local_state, LocalState::Pending);
    assert_eq!(n, 1);
}

// ---- remote pages ---------------------------------------------------------------------

#[test]
fn remote_page_applies_or_stashes_and_advances_cursor_atomically() {
    let mut db = db();
    let v = VaultId::new();
    let (a, b) = (ObjectId::new(), ObjectId::new());
    put(&mut db, v, b, 9); // b has a local edit
    let out = db
        .write(|tx| {
            tx.apply_remote_page(
                v,
                &[remote(a, 1, 1, Some(1)), remote(b, 1, 2, Some(2))],
                PageCursor::Changes {
                    next_after: 2,
                    latest_sequence: 2,
                },
            )
        })
        .unwrap();
    assert!(matches!(out.changes[0], RemoteApplyOutcome::Applied { .. }));
    assert!(matches!(out.changes[1], RemoteApplyOutcome::Stashed { .. }));
    let (oa, ob, rb, cur) = db
        .read(|tx| {
            Ok::<_, StorageError>((
                tx.get_object(v, a)?.unwrap(),
                tx.get_object(v, b)?.unwrap(),
                tx.get_remote_object(v, b)?.unwrap(),
                tx.get_sync_cursor(v)?,
            ))
        })
        .unwrap();
    assert_eq!(oa.local_state, LocalState::Synced);
    assert_eq!(ob.body, Some(body(9)), "local edit untouched");
    assert_eq!(rb.revision, 1);
    assert_eq!(cur.last_sequence, 2);

    // Replaying the same page is a no-op.
    let out = db
        .write(|tx| {
            tx.apply_remote_page(
                v,
                &[remote(a, 1, 1, Some(1))],
                PageCursor::Changes {
                    next_after: 2,
                    latest_sequence: 2,
                },
            )
        })
        .unwrap();
    assert!(matches!(out.changes[0], RemoteApplyOutcome::Skipped { .. }));

    // An invalid change rolls back the whole page including the cursor.
    let mut bad = remote(ObjectId::new(), 1, 3, Some(3));
    bad.body = None; // live object without body
    let err = db
        .write(|tx| {
            tx.apply_remote_page(
                v,
                &[remote(ObjectId::new(), 1, 3, Some(1)), bad],
                PageCursor::Changes {
                    next_after: 4,
                    latest_sequence: 4,
                },
            )
        })
        .unwrap_err();
    assert!(matches!(err, StorageError::Invalid(_)));
    let (cur, live) = db
        .read(|tx| Ok::<_, StorageError>((tx.get_sync_cursor(v)?, tx.count_live_objects(v)?)))
        .unwrap();
    assert_eq!(cur.last_sequence, 2);
    assert_eq!(live, 2);
}

#[test]
fn snapshot_uses_first_page_latest_sequence_and_drops_unseen_objects() {
    let mut db = db();
    let v = VaultId::new();
    let (stale, keep, x) = (ObjectId::new(), ObjectId::new(), ObjectId::new());
    // Previously synced state.
    db.write(|tx| {
        tx.apply_remote_page(
            v,
            &[remote(stale, 1, 1, Some(1)), remote(keep, 1, 2, Some(2))],
            PageCursor::Changes {
                next_after: 2,
                latest_sequence: 2,
            },
        )
    })
    .unwrap();
    db.write(|tx| tx.reset_for_resnapshot(v)).unwrap();
    // Page 1 (latest = 10), page 2 (latest = 12 now) — last.
    db.write(|tx| {
        tx.apply_remote_page(
            v,
            &[remote(keep, 1, 2, Some(2))],
            PageCursor::Snapshot {
                next_cursor: Some(2),
                latest_sequence: 10,
            },
        )
    })
    .unwrap();
    let out = db
        .write(|tx| {
            tx.apply_remote_page(
                v,
                &[remote(x, 3, 11, Some(3))],
                PageCursor::Snapshot {
                    next_cursor: None,
                    latest_sequence: 12,
                },
            )
        })
        .unwrap();
    assert!(out.snapshot_completed);
    assert_eq!(out.removed, vec![stale]);
    let cur = out.cursor.unwrap();
    assert!(cur.snapshot_complete);
    assert_eq!(
        cur.last_sequence, 10,
        "changes resume after the FIRST page's latest_sequence"
    );
    let ids: Vec<_> = db
        .read(|tx| tx.list_objects(v, true))
        .unwrap()
        .into_iter()
        .map(|o| o.object_id)
        .collect();
    assert!(ids.contains(&keep) && ids.contains(&x) && !ids.contains(&stale));
}

// ---- conflicts ---------------------------------------------------------------------------

fn conflicted_object(db: &mut Database) -> (VaultId, ObjectId) {
    let (v, o) = (VaultId::new(), ObjectId::new());
    // Server revision 1 known and synced.
    db.write(|tx| {
        tx.apply_remote_page(
            v,
            &[remote(o, 1, 1, Some(1))],
            PageCursor::Changes {
                next_after: 1,
                latest_sequence: 1,
            },
        )
    })
    .unwrap();
    let LocalMutationOutcome::Queued {
        mutation_id,
        base_revision,
        ..
    } = put(db, v, o, 5)
    else {
        panic!()
    };
    assert_eq!(base_revision, 1);
    db.write(|tx| tx.outbox_begin_batch(v, 10, usize::MAX))
        .unwrap();
    let out = db
        .write(|tx| {
            tx.apply_push_results(
                v,
                2,
                &[MutationResult::Conflict {
                    mutation_id,
                    object_id: o,
                    current_revision: 2,
                    current_sequence: 2,
                    current_deleted: false,
                }],
            )
        })
        .unwrap();
    assert!(matches!(out[0], PushOutcome::Conflicted { .. }));
    // Pull brings revision 2 → stashed.
    db.write(|tx| {
        tx.apply_remote_page(
            v,
            &[remote(o, 2, 2, Some(2))],
            PageCursor::Changes {
                next_after: 2,
                latest_sequence: 2,
            },
        )
    })
    .unwrap();
    (v, o)
}

#[test]
fn accept_remote_with_conflict_copy() {
    let mut db = db();
    let (v, o) = conflicted_object(&mut db);
    assert_eq!(db.read(|tx| tx.conflicted_objects(v)).unwrap(), vec![o]);
    let ctx = db.read(|tx| tx.conflict_context(v, o)).unwrap().unwrap();
    assert_eq!(ctx.conflict_revision, 2);
    assert_eq!(ctx.remote.as_ref().unwrap().revision, 2);
    assert_eq!(ctx.local.body, Some(body(5)));
    // Blocked from pushing while in conflict.
    assert!(db
        .write(|tx| tx.outbox_begin_batch(v, 10, usize::MAX))
        .unwrap()
        .is_empty());

    let copy_id = ObjectId::new();
    let out = db
        .write(|tx| {
            tx.apply_resolution(
                v,
                o,
                Resolution::AcceptRemote {
                    copy: Some(ConflictCopy {
                        object_id: copy_id,
                        body: body(6),
                        kek_class_hint: Some(KekClass::Inventory),
                    }),
                },
            )
        })
        .unwrap();
    assert_eq!(out.revision, 2);
    assert_eq!(out.copy.map(|c| c.0), Some(copy_id));
    let (orig, copy, entries) = db
        .read(|tx| {
            Ok::<_, StorageError>((
                tx.get_object(v, o)?.unwrap(),
                tx.get_object(v, copy_id)?.unwrap(),
                tx.outbox_list(v)?,
            ))
        })
        .unwrap();
    assert_eq!(orig.local_state, LocalState::Synced);
    assert_eq!(orig.body, Some(body(2)));
    assert_eq!(copy.conflict_origin, Some(o));
    assert_eq!(copy.local_state, LocalState::Pending);
    assert_eq!(entries.len(), 1);
    assert_eq!(
        (entries[0].object_id, entries[0].base_revision),
        (copy_id, 0)
    );
}

#[test]
fn rebase_requeues_on_top_of_remote() {
    let mut db = db();
    let (v, o) = conflicted_object(&mut db);
    let out = db
        .write(|tx| {
            tx.apply_resolution(
                v,
                o,
                Resolution::Rebase {
                    base_revision: 2,
                    body: body(7),
                    kek_class_hint: None,
                },
            )
        })
        .unwrap();
    assert!(out.requeued.is_some());
    let (obj, entries) = db
        .read(|tx| Ok::<_, StorageError>((tx.get_object(v, o)?.unwrap(), tx.outbox_list(v)?)))
        .unwrap();
    assert_eq!((obj.revision, obj.server_revision), (3, 2));
    assert_eq!(obj.local_state, LocalState::Pending);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].base_revision, 2);
    assert_eq!(entries[0].state, OutboxState::Queued);
    assert!(db.read(|tx| tx.get_remote_object(v, o)).unwrap().is_none());
}

// ---- other tables -------------------------------------------------------------------------

#[test]
fn profile_vaults_known_hosts_sessions_settings() {
    let mut db = db();
    let device = DeviceId::new();
    db.write(|tx| {
        tx.put_profile(&Profile::new_synced(
            ProfileId::new(),
            device,
            "https://sync.example.test",
            Some(UserId::new()),
            Some("user@example.test".into()),
        ))
    })
    .unwrap();
    assert_eq!(
        db.read(|tx| tx.get_profile()).unwrap().unwrap().device_id,
        device
    );

    let vault_id = VaultId::new();
    let now = chrono::Utc::now();
    let rec = db
        .write(|tx| {
            tx.upsert_vault_info(&VaultInfo {
                vault_id,
                owner_user_id: UserId::new(),
                role: VaultRole::Owner,
                state: VaultState::Active,
                created_at: now,
                updated_at: now,
                latest_sequence: 3,
                caller_trusted: true,
                deletion_scheduled_at: None,
                epoch: None,
            })
        })
        .unwrap();
    assert!(rec.caller_trusted);
    assert_eq!(db.read(|tx| tx.list_vaults()).unwrap().len(), 1);
    put(&mut db, vault_id, ObjectId::new(), 1);
    db.write(|tx| tx.purge_vault(vault_id)).unwrap();
    assert!(db.read(|tx| tx.list_vaults()).unwrap().is_empty());
    assert!(db.read(|tx| tx.outbox_list(vault_id)).unwrap().is_empty());

    let kh = KnownHostRecord {
        id: None,
        host_pattern: "[10.0.0.1]:2222".into(),
        key_type: "ssh-ed25519".into(),
        public_key: "AAAAC3NzaC1lZDI1NTE5AAAAITestKey".into(),
        fingerprint_sha256: "SHA256:test".into(),
        source: KnownHostSource::Tofu,
        revoked: false,
        vault_id: None,
        object_id: None,
        added_at: now,
        updated_at: now,
    };
    let id = db.write(|tx| tx.known_host_upsert(&kh)).unwrap();
    assert_eq!(db.write(|tx| tx.known_host_upsert(&kh)).unwrap(), id);
    assert!(db.write(|tx| tx.known_host_revoke(id)).unwrap());
    let found = db.read(|tx| tx.known_hosts_for("[10.0.0.1]:2222")).unwrap();
    assert_eq!(found.len(), 1);
    assert!(found[0].revoked);

    let sid = uuid::Uuid::new_v4();
    db.write(|tx| {
        tx.terminal_session_upsert(&TerminalSessionRecord {
            session_id: sid,
            vault_id: None,
            host_id: None,
            title: "prod-db".into(),
            started_at: now,
            ended_at: None,
            exit_status: None,
            metadata: serde_json::json!({"cols": 80}),
        })
    })
    .unwrap();
    assert!(db
        .write(|tx| tx.terminal_session_end(sid, chrono::Utc::now(), Some(0)))
        .unwrap());
    let s = db.read(|tx| tx.terminal_session_get(sid)).unwrap().unwrap();
    assert_eq!(s.exit_status, Some(0));

    db.write(|tx| tx.setting_set("theme", &serde_json::json!({"dark": true})))
        .unwrap();
    let v: serde_json::Value = db.read(|tx| tx.setting_get("theme")).unwrap().unwrap();
    assert_eq!(v["dark"], true);
    assert_eq!(db.read(|tx| tx.setting_keys()).unwrap(), vec!["theme"]);
}

// ---- async facade -------------------------------------------------------------------------

#[tokio::test]
async fn async_storage_close_is_synchronous_and_final() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("close.db");
    let storage = Storage::open(&path, key()).await.unwrap();
    let clone = storage.clone();
    storage
        .write(|tx| tx.setting_set("k", &1u32))
        .await
        .unwrap();
    storage.close().await;
    // Every handle is closed; closing again is a no-op.
    assert!(matches!(
        clone.read(|tx| tx.setting_get::<u32>("k")).await,
        Err(StorageError::Closed)
    ));
    clone.close().await;
    // The file was closed cleanly and reopens with the data.
    let reopened = Storage::open(&path, key()).await.unwrap();
    assert_eq!(reopened.setting_get::<u32>("k").await.unwrap(), Some(1));
    reopened.close().await;
}

#[tokio::test]
async fn async_storage_serializes_and_survives_panics() {
    let storage = Storage::open_in_memory(key()).await.unwrap();
    let v = VaultId::new();
    let mut handles = Vec::new();
    for i in 0..20u8 {
        let s = storage.clone();
        handles.push(tokio::spawn(async move {
            s.write(move |tx| {
                tx.record_local_put::<StorageError, _>(v, ObjectId::new(), None, |_| Ok(body(i)))
            })
            .await
            .unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(
        storage
            .read(move |tx| tx.outbox_counts(v))
            .await
            .unwrap()
            .queued,
        20
    );

    let r = storage
        .call(|_db| -> Result<(), StorageError> { panic!("boom") })
        .await;
    assert!(matches!(r, Err(StorageError::Closed)));
    // Thread still alive.
    assert_eq!(
        storage
            .read(move |tx| tx.outbox_counts(v))
            .await
            .unwrap()
            .queued,
        20
    );

    storage.setting_set("k", 42u32).await.unwrap();
    assert_eq!(storage.setting_get::<u32>("k").await.unwrap(), Some(42));
}

// ---- local-only profiles (ADR-0106) ---------------------------------------------------------

fn local_db() -> Database {
    let mut db = db();
    db.write(|tx| tx.put_profile(&Profile::new_local(ProfileId::new(), DeviceId::new(), None)))
        .unwrap();
    db
}

fn synced_profile() -> Profile {
    Profile::new_synced(
        ProfileId::new(),
        DeviceId::new(),
        "https://sync.example.test",
        None,
        None,
    )
}

#[test]
fn local_profile_never_writes_the_outbox() {
    let mut db = local_db();
    let (v, o, gone) = (VaultId::new(), ObjectId::new(), ObjectId::new());
    assert_eq!(
        put(&mut db, v, o, 1),
        LocalMutationOutcome::LocalOnly {
            revision: 1,
            deleted: false
        }
    );
    assert_eq!(
        put(&mut db, v, o, 2),
        LocalMutationOutcome::LocalOnly {
            revision: 2,
            deleted: false
        }
    );
    put(&mut db, v, gone, 3);
    // Never synced → removed outright on delete.
    assert_eq!(
        db.write(|tx| tx.record_local_delete(v, gone)).unwrap(),
        LocalMutationOutcome::DroppedUnsent
    );
    let (obj, n, cursor_complete) = db
        .read(|tx| {
            Ok::<_, StorageError>((
                tx.get_object(v, o)?.unwrap(),
                tx.outbox_list(v)?.len(),
                tx.get_sync_cursor(v)?.snapshot_complete,
            ))
        })
        .unwrap();
    assert_eq!(obj.local_state, LocalState::LocalOnly);
    assert_eq!((obj.revision, obj.server_revision), (2, 0));
    assert_eq!(obj.body, Some(body(2)));
    assert_eq!(n, 0);
    assert!(!cursor_complete);
}

#[test]
fn attach_fresh_queues_creates_and_drops_tombstones() {
    let mut db = local_db();
    let v = VaultId::new();
    let (a, b) = (ObjectId::new(), ObjectId::new());
    put(&mut db, v, a, 1);
    put(&mut db, v, a, 2); // local revision 2
    put(&mut db, v, b, 3);
    // Pretend b had been synced before (server_revision 4), then deleted locally.
    db.write(|tx| {
        let mut o = tx.get_object(v, b)?.unwrap();
        o.server_revision = 4;
        o.revision = 4;
        tx.upsert_object(&o)
    })
    .unwrap();
    db.write(|tx| tx.record_local_delete(v, b)).unwrap();

    let profile = synced_profile();
    let mut seen = Vec::new();
    let summary = db
        .write(|tx| {
            tx.attach_to_server::<StorageError, _>(v, &profile, AttachMode::Fresh, |obj, rev| {
                seen.push((obj.object_id, obj.revision, rev));
                Ok(body(9))
            })
        })
        .unwrap();
    assert_eq!(
        summary,
        AttachSummary {
            queued: 1,
            unchanged: 0,
            dropped: 1
        }
    );
    assert_eq!(
        seen,
        vec![(a, 2, 1)],
        "re-encrypted from local rev 2 for rev 1"
    );
    let (objs, entries, cursor, prof) = db
        .read(|tx| {
            Ok::<_, StorageError>((
                tx.list_objects(v, true)?,
                tx.outbox_list(v)?,
                tx.get_sync_cursor(v)?,
                tx.get_profile()?.unwrap(),
            ))
        })
        .unwrap();
    assert_eq!(objs.len(), 1);
    assert_eq!((objs[0].revision, objs[0].server_revision), (1, 0));
    assert_eq!(objs[0].local_state, LocalState::Pending);
    assert_eq!(entries.len(), 1);
    assert_eq!((entries[0].object_id, entries[0].base_revision), (a, 0));
    assert!(cursor.snapshot_complete);
    assert_eq!(prof.kind, ProfileKind::Synced);
    // Now in a synced profile: local edits go to the outbox again.
    put(&mut db, v, ObjectId::new(), 5);
    assert_eq!(db.read(|tx| tx.outbox_list(v)).unwrap().len(), 2);
}

#[test]
fn detach_then_reconnect_keeps_revision_knowledge() {
    let mut db = db();
    let v = VaultId::new();
    let (same, edited, deleted, fresh) = (
        ObjectId::new(),
        ObjectId::new(),
        ObjectId::new(),
        ObjectId::new(),
    );
    db.write(|tx| {
        tx.apply_remote_page(
            v,
            &[
                remote(same, 3, 1, Some(1)),
                remote(edited, 2, 2, Some(2)),
                remote(deleted, 5, 3, Some(3)),
            ],
            PageCursor::Changes {
                next_after: 3,
                latest_sequence: 3,
            },
        )
    })
    .unwrap();
    put(&mut db, v, edited, 7); // pending, base 2
    let local = Profile::new_local(ProfileId::new(), DeviceId::new(), Some("Personal".into()));
    let d = db.write(|tx| tx.detach_to_local(v, &local)).unwrap();
    assert_eq!(
        d,
        DetachSummary {
            discarded_mutations: 1,
            objects: 3
        }
    );
    assert!(db.read(|tx| tx.outbox_list(v)).unwrap().is_empty());
    // Offline work in the local profile.
    put(&mut db, v, edited, 8); // local rev 4 (server knows 2)
    db.write(|tx| tx.record_local_delete(v, deleted)).unwrap(); // tombstone rev 6
    put(&mut db, v, fresh, 9);

    let profile = synced_profile();
    let summary = db
        .write(|tx| {
            tx.attach_to_server::<StorageError, _>(
                v,
                &profile,
                AttachMode::Reconnect,
                |_obj, rev| {
                    assert_eq!(rev, if _obj.object_id == fresh { 1 } else { 3 });
                    Ok(body(20 + rev as u8))
                },
            )
        })
        .unwrap();
    assert_eq!(
        summary,
        AttachSummary {
            queued: 3,
            unchanged: 1,
            dropped: 0
        }
    );
    let entries = db.read(|tx| tx.outbox_list(v)).unwrap();
    let by_obj = |o: ObjectId| entries.iter().find(|e| e.object_id == o).unwrap().clone();
    assert_eq!(
        (by_obj(edited).base_revision, by_obj(edited).op),
        (2, OutboxOp::Put)
    );
    assert_eq!(
        (by_obj(deleted).base_revision, by_obj(deleted).op),
        (5, OutboxOp::Delete)
    );
    assert_eq!(
        (by_obj(fresh).base_revision, by_obj(fresh).op),
        (0, OutboxOp::Put)
    );
    let s = db.read(|tx| tx.get_object(v, same)).unwrap().unwrap();
    assert_eq!(s.local_state, LocalState::Synced);
    let c = db.read(|tx| tx.get_sync_cursor(v)).unwrap();
    assert!(!c.snapshot_complete, "reconnect re-snapshots");
    assert_eq!(c.last_sequence, 0);
}

#[test]
fn backup_export_import_roundtrip() {
    let mut src = local_db();
    let v = VaultId::new();
    src.write(|tx| tx.put_vault(&VaultRecord::new(v, VaultRole::Owner)))
        .unwrap();
    let (a, b) = (ObjectId::new(), ObjectId::new());
    put(&mut src, v, a, 1);
    put(&mut src, v, a, 2);
    put(&mut src, v, b, 3);
    let export = src.read(|tx| tx.export_vault(v)).unwrap();
    assert_eq!(export.objects.len(), 2);
    // Serializable (vault-core embeds it into the .ccbackup container).
    let json = serde_json::to_string(&export).unwrap();
    assert!(!json.contains("kek_class"), "KEK class is not exported");
    let back: VaultExport = serde_json::from_str(&json).unwrap();

    let mut dst = local_db();
    let s = dst.write(|tx| tx.import_vault(&back)).unwrap();
    assert_eq!(s.objects, 2);
    let objs = dst.read(|tx| tx.list_objects(v, false)).unwrap();
    let oa = objs.iter().find(|o| o.object_id == a).unwrap();
    assert_eq!((oa.revision, oa.local_state), (2, LocalState::LocalOnly));
    assert_eq!(oa.body, Some(body(2)));
    // A second import into a non-empty vault is refused atomically.
    assert!(dst.write(|tx| tx.import_vault(&back)).is_err());
    // Importing into a synced profile is refused.
    let mut synced = db();
    synced
        .write(|tx| tx.put_profile(&synced_profile()))
        .unwrap();
    assert!(synced.write(|tx| tx.import_vault(&back)).is_err());
}

#[test]
fn profile_directory_index() {
    let dir = tempfile::tempdir().unwrap();
    let pd = ProfileDirectory::new(dir.path());
    assert!(pd.list().unwrap().is_empty());
    let p1 = pd.create("Personal", ProfileKind::Local).unwrap();
    let mut p2 = pd.create("Work", ProfileKind::Synced).unwrap();
    assert!(pd
        .db_path(p1.profile_id)
        .ends_with(format!("profiles/{}/vault.db", p1.profile_id)));
    {
        let mut db = Database::open(pd.db_path(p1.profile_id), &key()).unwrap();
        db.write(|tx| tx.put_profile(&Profile::new_local(p1.profile_id, DeviceId::new(), None)))
            .unwrap();
    }
    pd.set_active(Some(p2.profile_id)).unwrap();
    p2.kind = ProfileKind::Local;
    pd.update(&p2).unwrap();
    let list = pd.list().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[1].kind, ProfileKind::Local);
    assert_eq!(pd.active().unwrap(), Some(p2.profile_id));
    let index = std::fs::read_to_string(dir.path().join("profiles.json")).unwrap();
    assert!(
        !index.contains("sync.example"),
        "index holds no server data"
    );
    assert!(pd.remove(p1.profile_id).unwrap());
    assert!(!pd.profile_dir(p1.profile_id).exists());
    assert_eq!(pd.list().unwrap().len(), 1);
}

// ---- server rollback recovery (ADR-0103 addendum) -------------------------------------

fn changes_page(db: &mut Database, v: VaultId, changes: &[RemoteChange], latest: i64) {
    db.write(|tx| {
        tx.apply_remote_page(
            v,
            changes,
            PageCursor::Changes {
                next_after: changes.iter().map(|c| c.sequence).max().unwrap_or(0),
                latest_sequence: latest,
            },
        )
    })
    .unwrap();
}

fn cursor(db: &mut Database, v: VaultId) -> SyncCursor {
    db.read(|tx| tx.get_sync_cursor(v)).unwrap()
}

#[test]
fn rollback_evidence_rules() {
    let mut db = db();
    let v = VaultId::new();
    let (e1, e2) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let o = ObjectId::new();
    changes_page(&mut db, v, &[remote(o, 1, 5, Some(1))], 5);
    let c = cursor(&mut db, v);
    assert_eq!(c.epoch, None);
    // Unknown stored epoch: only sequences count.
    assert_eq!(c.rollback_evidence(Some(e1), 5), None);
    assert_eq!(
        c.rollback_evidence(Some(e1), 4),
        Some(RollbackReason::SequenceRegressed)
    );
    db.write(|tx| tx.note_server_epoch(v, Some(e1))).unwrap();
    db.write(|tx| tx.note_server_epoch(v, None)).unwrap();
    let c = cursor(&mut db, v);
    assert_eq!(c.epoch, Some(e1), "first epoch kept, None ignored");
    assert_eq!(c.rollback_evidence(Some(e1), 9), None);
    assert_eq!(
        c.rollback_evidence(None, 9),
        None,
        "unknown epoch never triggers"
    );
    assert_eq!(
        c.rollback_evidence(Some(e2), 9),
        Some(RollbackReason::EpochChanged)
    );
    assert!(matches!(
        db.write(|tx| tx.note_server_epoch(v, Some(e2))),
        Err(StorageError::Invalid(_))
    ));

    // Push evidence: a fresh acceptance at a sequence we hold elsewhere.
    let accepted = |seq: i64, replayed: bool| MutationResult::Accepted {
        mutation_id: cc_protocol::MutationId::new(),
        object_id: ObjectId::new(),
        revision: 1,
        sequence: seq,
        replayed,
    };
    let ev = |db: &mut Database, latest, r: &[MutationResult]| {
        db.read(|tx| tx.push_rollback_evidence(v, latest, r))
            .unwrap()
    };
    assert_eq!(ev(&mut db, 6, &[accepted(6, false)]), None);
    assert_eq!(
        ev(&mut db, 6, &[accepted(5, true)]),
        None,
        "replays are old"
    );
    assert_eq!(
        ev(&mut db, 6, &[accepted(5, false)]),
        Some(RollbackReason::SequenceReused)
    );
    assert_eq!(ev(&mut db, 4, &[]), Some(RollbackReason::SequenceRegressed));
}

#[test]
fn recovery_begin_is_idempotent_and_listing_restarts_on_a_new_epoch() {
    let mut db = db();
    let v = VaultId::new();
    let (e1, e2) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    assert!(db
        .write(|tx| tx.begin_rollback_recovery(v, RollbackReason::EpochChanged, Some(e1)))
        .unwrap());
    assert!(!db
        .write(|tx| tx.begin_rollback_recovery(v, RollbackReason::SequenceRegressed, None))
        .unwrap());
    let o = ObjectId::new();
    let rec = db
        .write(|tx| {
            tx.record_recovery_page(
                v,
                &[remote(o, 3, 7, Some(1))],
                RecoveryPage {
                    next_after: 7,
                    has_more: true,
                    latest_sequence: 9,
                    epoch: Some(e1),
                },
            )
        })
        .unwrap();
    assert_eq!((rec.cursor, rec.listed, rec.latest_sequence), (7, false, 9));
    assert_eq!(
        rec.reason,
        RollbackReason::EpochChanged,
        "first reason kept"
    );
    assert!(db
        .read(|tx| tx.get_recovery_remote(v, o))
        .unwrap()
        .is_some());
    // A page from another epoch is refused; a new epoch restarts the listing.
    assert!(db
        .write(|tx| tx.record_recovery_page(
            v,
            &[],
            RecoveryPage {
                next_after: 7,
                has_more: false,
                latest_sequence: 9,
                epoch: Some(e2),
            },
        ))
        .is_err());
    assert!(!db
        .write(|tx| tx.begin_rollback_recovery(v, RollbackReason::EpochChanged, Some(e2)))
        .unwrap());
    let rec = cursor(&mut db, v).recovery.unwrap();
    assert_eq!((rec.cursor, rec.listed, rec.epoch), (0, false, Some(e2)));
    assert!(db
        .read(|tx| tx.get_recovery_remote(v, o))
        .unwrap()
        .is_none());
    // Reconcile before the listing is complete is refused.
    struct Never;
    impl ReconcileCodec<StorageError> for Never {
        fn reencrypt(&mut self, _: &StoredObject, _: i64) -> Result<Option<EncryptedBody>> {
            unreachable!()
        }
        fn same_payload(&mut self, _: &StoredObject, _: &RemoteObject) -> Result<bool> {
            unreachable!()
        }
    }
    assert!(db.write(|tx| tx.reconcile_rollback(v, &mut Never)).is_err());
}

/// Test codec for reconcile: re-encryption yields a marker body; payload
/// equality is decided by the first ciphertext byte.
#[derive(Default)]
struct FakeCodec {
    reencrypted: Vec<(ObjectId, i64)>,
}

impl ReconcileCodec<StorageError> for FakeCodec {
    fn reencrypt(&mut self, local: &StoredObject, revision: i64) -> Result<Option<EncryptedBody>> {
        self.reencrypted.push((local.object_id, revision));
        Ok(Some(body(200 + revision as u8)))
    }
    fn same_payload(&mut self, local: &StoredObject, server: &RemoteObject) -> Result<bool> {
        let tag = |b: &Option<EncryptedBody>| b.as_ref().map(|b| b.ciphertext.as_slice()[0]);
        Ok(tag(&local.body) == tag(&server.body))
    }
}

#[test]
fn reconcile_after_rollback_decision_table() {
    let mut db = db();
    let v = VaultId::new();
    let ids: Vec<ObjectId> = (0..13).map(|_| ObjectId::new()).collect();
    let [same, newer, older, lost, lost3, tomb, both_del, fork, pending, pending_older, server_only, twin, pending_fork] =
        ids[..]
    else {
        unreachable!()
    };
    // One writer for every server state; `pending_fork` is rewritten by
    // another device after the restore.
    let writer = DeviceId::new();
    let by = |changes: Vec<RemoteChange>| -> Vec<RemoteChange> {
        changes
            .into_iter()
            .map(|mut c| {
                c.writer_device_id = writer;
                c
            })
            .collect()
    };
    // Pre-restore server history as this device saw it.
    changes_page(
        &mut db,
        v,
        &by(vec![
            remote(same, 1, 1, Some(1)),
            remote(newer, 1, 2, Some(2)),
            remote(older, 2, 3, Some(3)),
            remote(lost, 1, 4, Some(4)),
            remote(lost3, 3, 5, Some(5)),
            remote(tomb, 2, 6, None),
            remote(both_del, 2, 7, None),
            remote(fork, 1, 8, Some(8)),
            remote(pending, 1, 9, Some(9)),
            remote(pending_older, 2, 10, Some(10)),
            remote(twin, 2, 11, Some(11)),
            remote(pending_fork, 1, 12, Some(12)),
        ]),
        12,
    );
    put(&mut db, v, pending, 90); // base 1
    put(&mut db, v, pending_older, 91); // base 2 → revision 3
    put(&mut db, v, pending_fork, 92); // base 1
    let e2 = uuid::Uuid::new_v4();
    db.write(|tx| tx.begin_rollback_recovery(v, RollbackReason::EpochChanged, Some(e2)))
        .unwrap();
    // The restored server (two pages).
    let page1 = by(vec![
        remote(same, 1, 1, Some(1)),
        remote(newer, 2, 2, Some(20)),
        remote(older, 1, 3, Some(30)),
        remote(tomb, 1, 4, Some(40)),
    ]);
    let mut page2 = by(vec![
        remote(both_del, 1, 5, None),
        remote(fork, 1, 6, Some(80)),
        remote(pending, 1, 7, Some(9)),
        remote(pending_older, 1, 8, Some(100)),
        remote(server_only, 1, 9, Some(110)),
        remote(twin, 1, 10, Some(11)), // same payload, older revision
    ]);
    page2.push(remote(pending_fork, 1, 11, Some(120))); // another writer
    db.write(|tx| {
        tx.record_recovery_page(
            v,
            &page1,
            RecoveryPage {
                next_after: 4,
                has_more: true,
                latest_sequence: 10,
                epoch: Some(e2),
            },
        )?;
        tx.record_recovery_page(
            v,
            &page2,
            RecoveryPage {
                next_after: 11,
                has_more: false,
                latest_sequence: 11,
                epoch: Some(e2),
            },
        )
    })
    .unwrap();
    let mut codec = FakeCodec::default();
    let out = db.write(|tx| tx.reconcile_rollback(v, &mut codec)).unwrap();

    let sorted = |mut v: Vec<ObjectId>| {
        v.sort();
        v
    };
    assert_eq!(
        sorted(out.applied.iter().map(|a| a.0).collect()),
        sorted(vec![newer, server_only])
    );
    assert_eq!(
        sorted(out.repushed.iter().map(|a| a.0).collect()),
        sorted(vec![older, lost, lost3, pending_older])
    );
    assert_eq!(out.tombstones_reapplied, vec![(tomb, 2)]);
    assert_eq!(
        sorted(out.conflicts.clone()),
        sorted(vec![fork, pending_fork])
    );
    assert!(out.unrecoverable.is_empty());
    // Re-encryption only where the local ciphertext is bound to another
    // revision: lost3 (3 → 1) and pending_older (3 → 2).
    assert_eq!(
        sorted(codec.reencrypted.iter().map(|r| r.0).collect()),
        sorted(vec![lost3, pending_older])
    );

    let (objs, outbox, remote_rows, cur) = db
        .read(|tx| {
            Ok::<_, StorageError>((
                tx.list_objects(v, true)?,
                tx.outbox_list(v)?,
                tx.list_recovery_remote(v)?,
                tx.get_sync_cursor(v)?,
            ))
        })
        .unwrap();
    let obj = |id| objs.iter().find(|o| o.object_id == id).unwrap().clone();
    let entry = |id| {
        let es: Vec<_> = outbox.iter().filter(|e| e.object_id == id).collect();
        assert!(es.len() <= 1, "one entry per object");
        es.first().map(|e| (e.base_revision, e.op, e.state))
    };
    assert_eq!(entry(same), None);
    assert_eq!(obj(same).local_state, LocalState::Synced);
    assert_eq!(entry(newer), None);
    assert_eq!(obj(newer).body, Some(body(20)));
    assert_eq!(entry(older), Some((1, OutboxOp::Put, OutboxState::Queued)));
    assert_eq!(
        obj(older).body,
        Some(body(3)),
        "revision-2 ciphertext reused"
    );
    assert_eq!(entry(lost), Some((0, OutboxOp::Put, OutboxState::Queued)));
    assert_eq!((obj(lost).revision, obj(lost).server_revision), (1, 0));
    assert_eq!(entry(lost3), Some((0, OutboxOp::Put, OutboxState::Queued)));
    assert_eq!(obj(lost3).body, Some(body(201)));
    assert_eq!(
        entry(tomb),
        Some((1, OutboxOp::Delete, OutboxState::Queued))
    );
    assert!(obj(tomb).deleted);
    assert_eq!(entry(both_del), None);
    assert_eq!((obj(both_del).revision, obj(both_del).deleted), (1, true));
    assert_eq!(entry(fork), Some((1, OutboxOp::Put, OutboxState::Conflict)));
    assert_eq!(obj(fork).local_state, LocalState::Conflict);
    let stash = db
        .read(|tx| tx.get_remote_object(v, fork))
        .unwrap()
        .unwrap();
    assert_eq!(stash.body, Some(body(80)));
    assert_eq!(
        entry(pending),
        Some((1, OutboxOp::Put, OutboxState::Queued)),
        "kept"
    );
    assert_eq!(
        entry(pending_fork),
        Some((1, OutboxOp::Put, OutboxState::Conflict)),
        "base rewritten by another device: conflict policy decides"
    );
    let stash = db
        .read(|tx| tx.get_remote_object(v, pending_fork))
        .unwrap()
        .unwrap();
    assert_eq!(stash.body, Some(body(120)));
    assert_eq!(obj(pending).body, Some(body(90)));
    assert_eq!(
        entry(pending_older),
        Some((1, OutboxOp::Put, OutboxState::Queued))
    );
    assert_eq!(obj(pending_older).body, Some(body(202)));
    assert_eq!(obj(server_only).local_state, LocalState::Synced);
    assert_eq!(entry(twin), None, "same payload: adopt, do not push");
    assert_eq!(obj(twin).revision, 1);
    assert!(remote_rows.is_empty());
    assert!(cur.recovery.is_none());
    assert!(cur.snapshot_complete);
    assert_eq!(
        (cur.last_sequence, cur.server_latest_sequence, cur.epoch),
        (11, 11, Some(e2))
    );
    // Invariant: synced iff no outbox entries.
    for o in &objs {
        assert_eq!(
            o.local_state == LocalState::Synced,
            entry(o.object_id).is_none(),
            "{:?}",
            o.object_id
        );
    }
}
