use super::*;
use crate::{DatabaseKey, Profile, Storage, VaultRecord};
use cc_protocol::vaults::VaultRole;

fn owner() -> ServiceOwnerIdentity {
    ServiceOwnerIdentity {
        profile_id: ProfileId::new(),
        vault_id: VaultId::new(),
        device_id: DeviceId::new(),
        signing_fingerprint: "a".repeat(64),
        personal: None,
    }
}
fn record() -> ServiceOwnerRecord {
    ServiceOwnerRecord {
        service_id: Uuid::new_v4(),
        node_local_profile: "node-profile-is-not-native-profile".into(),
        owner_epoch: 1,
        policy_revision: 1,
        revocation_epoch: 1,
        clock_floor: 100,
        signer_fingerprint: "b".repeat(64),
        recipient_fingerprint: "c".repeat(64),
        route_trust_digest: "d".repeat(64),
        policy_digest: "e".repeat(64),
        status_digest: "f".repeat(64),
        delivery: ServiceOwnerDelivery::PendingEnrollment,
    }
}
fn setup(db: &mut Database, owner: &ServiceOwnerIdentity) {
    db.write(|tx| {
        tx.put_profile(&Profile::new_local(owner.profile_id, owner.device_id, None))?;
        tx.put_vault(&VaultRecord::new(owner.vault_id, VaultRole::Owner))
    })
    .unwrap();
}
fn initialize(db: &mut Database, owner: &ServiceOwnerIdentity, row: ServiceOwnerRecord) {
    db.service_owner_write::<_, StorageError>(|tx| {
        let absent = tx.service_owner_snapshot(owner)?;
        assert!(absent.revision().is_none());
        tx.service_owner_compare_exchange(&absent, vec![row])
    })
    .unwrap();
}

#[test]
fn full_file_commit_reopen_separate_settings_and_node_namespace() {
    let owned = tempfile::tempdir().unwrap();
    let path = owned.path().join("service.db");
    let key = DatabaseKey::from_bytes([91; 32]);
    let o = owner();
    let r = record();
    {
        let mut db = Database::open(&path, &key).unwrap();
        setup(&mut db, &o);
        db.service_owner_write::<_, StorageError>(|tx| {
            let synchronous: i64 = tx.c().query_row("PRAGMA synchronous", [], |r| r.get(0))?;
            assert!(synchronous >= 2);
            let s = tx.service_owner_snapshot(&o)?;
            tx.service_owner_compare_exchange(&s, vec![r.clone()])
        })
        .unwrap();
        db.write(|tx| {
            tx.setting_set(
                "automation.runners.v1",
                &serde_json::json!({"version":1,"records":{}}),
            )?;
            tx.setting_set("vault-settings-test", &serde_json::json!({"changed":true}))
        })
        .unwrap();
    }
    let mut db = Database::open(&path, &key).unwrap();
    let s = db.read(|tx| tx.service_owner_snapshot(&o)).unwrap();
    assert_eq!(s.revision(), Some(1));
    assert_eq!(s.records()[0].node_local_profile, r.node_local_profile);
    assert_ne!(r.node_local_profile, o.profile_id.to_string());
    let outbox: i64 = db
        .conn
        .query_row("SELECT count(*) FROM outbox", [], |r| r.get(0))
        .unwrap();
    assert_eq!(outbox, 0);
}

#[test]
fn two_connections_exact_cas_has_one_winner() {
    use std::sync::{Arc, Barrier};
    let owned = tempfile::tempdir().unwrap();
    let path = owned.path().join("race.db");
    let o = owner();
    {
        let mut db = Database::open(&path, &DatabaseKey::from_bytes([92; 32])).unwrap();
        setup(&mut db, &o);
        initialize(&mut db, &o, record());
    }
    let barrier = Arc::new(Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let path = path.clone();
            let o = o.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut db = Database::open(path, &DatabaseKey::from_bytes([92; 32])).unwrap();
                let s = db.read(|tx| tx.service_owner_snapshot(&o)).unwrap();
                let mut rows = s.records().to_vec();
                rows[0].clock_floor += 1;
                barrier.wait();
                db.service_owner_write::<_, StorageError>(|tx| {
                    tx.service_owner_compare_exchange(&s, rows)
                })
            })
        })
        .collect();
    let outcomes: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(outcomes.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|r| matches!(r, Err(ServiceOwnerWriteError::Rejected(_))))
            .count(),
        1
    );
}

#[test]
fn copied_ledger_cannot_adopt_new_profile_device_signer_or_synced_profile() {
    let owned = tempfile::tempdir().unwrap();
    let mut db = Database::open(
        owned.path().join("foreign.db"),
        &DatabaseKey::from_bytes([93; 32]),
    )
    .unwrap();
    let o = owner();
    setup(&mut db, &o);
    initialize(&mut db, &o, record());
    let old = db.read(|tx| tx.service_owner_snapshot(&o)).unwrap();
    for kind in 0..4 {
        let mut foreign = o.clone();
        match kind {
            0 => foreign.profile_id = ProfileId::new(),
            1 => foreign.device_id = DeviceId::new(),
            2 => foreign.signing_fingerprint = "0".repeat(64),
            _ => {}
        }
        setup(&mut db, &foreign);
        if kind == 3 {
            db.write(|tx| {
                let mut p = tx.get_profile()?.unwrap();
                p.kind = ProfileKind::Synced;
                p.server_url = Some("https://synthetic.invalid".into());
                tx.put_profile(&p)
            })
            .unwrap();
        }
        assert!(db.read(|tx| tx.service_owner_snapshot(&foreign)).is_err());
        if kind != 2 {
            assert!(db
                .service_owner_write::<_, StorageError>(
                    |tx| tx.service_owner_compare_exchange(&old, old.records().to_vec())
                )
                .is_err());
        }
    }
}

#[test]
fn corrupt_unknown_duplicate_and_oversized_rows_never_become_absence_or_epoch_one() {
    let owned = tempfile::tempdir().unwrap();
    let mut db = Database::open(
        owned.path().join("corrupt.db"),
        &DatabaseKey::from_bytes([94; 32]),
    )
    .unwrap();
    let o = owner();
    setup(&mut db, &o);
    let l = Ledger {
        version: 1,
        revision: 1,
        owner: o.clone(),
        records: vec![record()],
    };
    let valid = serde_json::to_string(&l).unwrap();
    let mut cases = vec![
        "{".to_string(),
        "null".into(),
        "x".repeat(MAX_BYTES + 1),
        valid.replacen("\"version\":1", "\"version\":2", 1),
        valid.replacen("\"version\":1", "\"version\":1,\"version\":1", 1),
        valid.replacen("\"version\":1", "\"unknown\":0,\"version\":1", 1),
    ];
    let mut duplicate = l.clone();
    duplicate.records.push(duplicate.records[0].clone());
    cases.push(serde_json::to_string(&duplicate).unwrap());
    let mut too_many = l.clone();
    too_many.records = (0..=MAX_RECORDS).map(|_| record()).collect();
    cases.push(serde_json::to_string(&too_many).unwrap());
    for raw in cases {
        db.conn.execute("INSERT OR REPLACE INTO local_settings(key,value,updated_at) VALUES(?1,?2,'synthetic')",params![KEY,raw]).unwrap();
        assert!(db.read(|tx| tx.service_owner_snapshot(&o)).is_err());
    }
}

#[test]
fn floors_tombstones_capacity_and_byte_preserving_cas_reject_without_reset() {
    let owned = tempfile::tempdir().unwrap();
    let mut db = Database::open(
        owned.path().join("floor.db"),
        &DatabaseKey::from_bytes([95; 32]),
    )
    .unwrap();
    let o = owner();
    setup(&mut db, &o);
    let mut r = record();
    r.delivery = ServiceOwnerDelivery::RevokedLocally;
    initialize(&mut db, &o, r);
    let s = db.read(|tx| tx.service_owner_snapshot(&o)).unwrap();
    for kind in 0..8 {
        let mut rows = s.records().to_vec();
        match kind {
            0 => rows.clear(),
            1 => rows[0].clock_floor -= 1,
            2 => rows[0].delivery = ServiceOwnerDelivery::Acknowledged,
            3 => rows[0].node_local_profile = "other-node".into(),
            4 => rows[0].policy_digest = "0".repeat(64),
            5 => rows[0].owner_epoch += 1,
            6 => rows[0].policy_revision = 0,
            _ => {
                while rows.len() <= MAX_RECORDS {
                    rows.push(record());
                }
            }
        }
        assert!(matches!(
            db.service_owner_write::<_, StorageError>(
                |tx| tx.service_owner_compare_exchange(&s, rows)
            ),
            Err(ServiceOwnerWriteError::Rejected(_))
        ));
    }
    // Semantically identical out-of-band rewrite must not satisfy exact-byte CAS.
    let raw = s.raw.as_ref().unwrap();
    let spaced = format!(" {} ", std::str::from_utf8(raw).unwrap());
    db.conn
        .execute(
            "UPDATE local_settings SET value=?1 WHERE key=?2",
            params![spaced, KEY],
        )
        .unwrap();
    assert!(db
        .service_owner_write::<_, StorageError>(
            |tx| tx.service_owner_compare_exchange(&s, s.records().to_vec())
        )
        .is_err());
}

#[test]
fn actual_deferred_commit_failure_is_unknown_and_returns_no_commit_receipt() {
    let owned = tempfile::tempdir().unwrap();
    let mut db = Database::open(
        owned.path().join("unknown.db"),
        &DatabaseKey::from_bytes([96; 32]),
    )
    .unwrap();
    let o = owner();
    setup(&mut db, &o);
    db.conn.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE owned_parent(id INTEGER PRIMARY KEY); CREATE TABLE owned_child(id INTEGER REFERENCES owned_parent(id) DEFERRABLE INITIALLY DEFERRED);").unwrap();
    let outcome = db.service_owner_write::<_, StorageError>(|tx| {
        let s = tx.service_owner_snapshot(&o)?;
        tx.service_owner_compare_exchange(&s, vec![record()])?;
        tx.c()
            .execute("INSERT INTO owned_child(id) VALUES(17)", [])?;
        Ok("must-not-escape")
    });
    assert!(matches!(outcome, Err(ServiceOwnerWriteError::Unknown)));
    assert!(db
        .read(|tx| tx.service_owner_snapshot(&o))
        .unwrap()
        .revision()
        .is_none());
}

#[test]
fn memory_and_disabled_sync_deny_before_callback() {
    let mut db = Database::open_in_memory(&DatabaseKey::from_bytes([97; 32])).unwrap();
    assert!(matches!(
        db.service_owner_write::<(), StorageError>(|_| panic!("must not run")),
        Err(ServiceOwnerWriteError::BeforeWrite(_))
    ));
    let owned = tempfile::tempdir().unwrap();
    let mut db = Database::open(
        owned.path().join("off.db"),
        &DatabaseKey::from_bytes([97; 32]),
    )
    .unwrap();
    db.conn.pragma_update(None, "synchronous", 0).unwrap();
    assert!(matches!(
        db.service_owner_write::<(), StorageError>(|_| panic!("must not run")),
        Err(ServiceOwnerWriteError::BeforeWrite(_))
    ));
}

#[tokio::test]
async fn existing_worker_closed_channel_is_unknown_not_false_rejected() {
    let owned = tempfile::tempdir().unwrap();
    let storage = Storage::open(
        owned.path().join("worker.db"),
        DatabaseKey::from_bytes([98; 32]),
    )
    .await
    .unwrap();
    storage.close().await;
    let outcome = storage
        .write_service_owner::<(), StorageError, _>(|_| panic!("closed worker"))
        .await;
    assert!(matches!(outcome, Err(ServiceOwnerWriteError::Unknown)));
}

#[test]
fn old_local_owner_encoding_is_unchanged_and_personal_pin_is_closed() {
    let local=owner();let wire=serde_json::to_value(&local).unwrap();
    assert!(wire.get("personal").is_none());
    let old:ServiceOwnerIdentity=serde_json::from_value(wire).unwrap();assert!(old==local);
    let mut personal=owner();personal.personal=Some(ServicePersonalOwnerPin{user_id:cc_protocol::UserId::new(),
        server_origin_fingerprint:"1".repeat(64),vault_commitment:"2".repeat(64)});
    let mut wire=serde_json::to_value(&personal).unwrap();wire["personal"]["future"]=serde_json::json!(1);
    assert!(serde_json::from_value::<ServiceOwnerIdentity>(wire).is_err());
}

#[test]
fn personal_sqlcipher_owner_cas_binds_actual_profile_user_vault_device_and_origin_pin() {
    let owned=tempfile::tempdir().unwrap();let key=DatabaseKey::from_bytes([111;32]);
    let mut db=Database::open(owned.path().join("personal.db"),&key).unwrap();
    let mut o=owner();let user=cc_protocol::UserId::new();
    o.personal=Some(ServicePersonalOwnerPin{user_id:user,server_origin_fingerprint:"1".repeat(64),vault_commitment:"2".repeat(64)});
    db.write(|tx| {
        let mut p=Profile::new_local(o.profile_id,o.device_id,None);p.kind=ProfileKind::Synced;
        p.user_id=Some(user);p.server_url=Some("https://owned-personal.example/".into());tx.put_profile(&p)?;
        let mut v=VaultRecord::new(o.vault_id,VaultRole::Owner);v.owner_user_id=Some(user);v.caller_trusted=true;tx.put_vault(&v)
    }).unwrap();
    initialize(&mut db,&o,record());
    let before:Vec<u8>=db.conn.query_row("SELECT CAST(value AS BLOB) FROM local_settings WHERE key=?1",[KEY],|r|r.get(0)).unwrap();
    let mut foreign=o.clone();foreign.personal.as_mut().unwrap().server_origin_fingerprint="3".repeat(64);
    assert!(db.read(|tx|tx.service_owner_snapshot(&foreign)).is_err());
    for field in ["user","device","vault_owner","role","trusted"] {
        let result=db.service_owner_write::<_,StorageError>(|tx| {
            let expected=tx.service_owner_snapshot(&o)?;
            if field=="user" || field=="device" {
                let mut p=tx.get_profile()?.unwrap();
                if field=="user" {p.user_id=Some(cc_protocol::UserId::new());} else {p.device_id=DeviceId::new();}
                tx.put_profile(&p)?;
            } else {
                let mut v=tx.get_vault(o.vault_id)?.unwrap();
                match field {"vault_owner"=>v.owner_user_id=Some(cc_protocol::UserId::new()),"role"=>v.role=VaultRole::Viewer,_=>v.caller_trusted=false}
                tx.put_vault(&v)?;
            }
            tx.service_owner_compare_exchange(&expected,expected.records().to_vec())
        });
        assert!(matches!(result,Err(ServiceOwnerWriteError::Rejected(_))),"{field}");
        let after:Vec<u8>=db.conn.query_row("SELECT CAST(value AS BLOB) FROM local_settings WHERE key=?1",[KEY],|r|r.get(0)).unwrap();
        assert_eq!(before,after);
    }
    assert_eq!(db.read(|tx|tx.service_owner_snapshot(&o)).unwrap().revision(),Some(1));
}
