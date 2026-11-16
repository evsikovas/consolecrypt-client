use super::*;
use crate::{DatabaseKey, Profile};
fn owner() -> TunnelTlsOwner {
    TunnelTlsOwner {
        profile_id: ProfileId::new(),
        device_id: DeviceId::new(),
        signer_fingerprint: "a".repeat(64),
        context_fingerprint: "b".repeat(64),
    }
}
fn public() -> TunnelTlsPublic {
    TunnelTlsPublic {
        identity_id: Uuid::new_v4(),
        certificate_der: vec![1, 2, 3],
        certificate_sha256: "c".repeat(64),
        private_key_sha256: "d".repeat(64),
    }
}
fn setup(db: &mut Database, o: &TunnelTlsOwner) {
    db.write(|tx| tx.put_profile(&Profile::new_local(o.profile_id, o.device_id, None)))
        .unwrap()
}
#[test]
fn pending_is_persistent_and_only_exact_frozen_identity_can_complete() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("identity.db");
    let key = DatabaseKey::from_bytes([19; 32]);
    let o = owner();
    let p = public();
    {
        let mut db = Database::open(&path, &key).unwrap();
        setup(&mut db, &o);
        db.tunnel_tls_write::<_, StorageError>(|tx| {
            let s = tx.tunnel_tls_snapshot(&o)?;
            tx.tunnel_tls_reserve(&s, p.clone())
        })
        .unwrap();
    }
    let mut db = Database::open(&path, &key).unwrap();
    let s = db.read(|tx| tx.tunnel_tls_snapshot(&o)).unwrap();
    assert_eq!(s.phase(), Some(TunnelTlsPhase::Pending));
    assert_eq!(s.public().unwrap().identity_id, p.identity_id);
    db.tunnel_tls_write::<_, StorageError>(|tx| tx.tunnel_tls_complete(&s))
        .unwrap();
    assert!(db
        .tunnel_tls_write::<_, StorageError>(|tx| tx.tunnel_tls_complete(&s))
        .is_err());
    let s = db.read(|tx| tx.tunnel_tls_snapshot(&o)).unwrap();
    assert_eq!(s.phase(), Some(TunnelTlsPhase::Stored));
    assert!(db
        .tunnel_tls_write::<_, StorageError>(|tx| tx.tunnel_tls_reserve(&s, public()))
        .is_err());
    drop(db);
    let mut db = Database::open(&path, &key).unwrap();
    assert_eq!(
        db.read(|tx| tx.tunnel_tls_snapshot(&o))
            .unwrap()
            .public()
            .unwrap()
            .identity_id,
        p.identity_id
    );
}
#[test]
fn two_connections_to_same_profile_have_one_identity_reservation_winner() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("race.db");
    let key = DatabaseKey::from_bytes([20; 32]);
    let o = owner();
    let mut a = Database::open(&path, &key).unwrap();
    setup(&mut a, &o);
    let mut b = Database::open(&path, &key).unwrap();
    let sa = a.read(|tx| tx.tunnel_tls_snapshot(&o)).unwrap();
    let sb = b.read(|tx| tx.tunnel_tls_snapshot(&o)).unwrap();
    let p = public();
    a.tunnel_tls_write::<_, StorageError>(|tx| tx.tunnel_tls_reserve(&sa, p.clone()))
        .unwrap();
    assert!(b
        .tunnel_tls_write::<_, StorageError>(|tx| tx.tunnel_tls_reserve(&sb, public()))
        .is_err());
    assert_eq!(
        b.read(|tx| tx.tunnel_tls_snapshot(&o))
            .unwrap()
            .public()
            .unwrap()
            .identity_id,
        p.identity_id
    );
}
#[test]
fn foreign_device_context_and_corrupt_oversize_record_fail_closed() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = Database::open(
        dir.path().join("foreign.db"),
        &DatabaseKey::from_bytes([21; 32]),
    )
    .unwrap();
    let o = owner();
    setup(&mut db, &o);
    db.tunnel_tls_write::<_, StorageError>(|tx| {
        tx.tunnel_tls_reserve(&tx.tunnel_tls_snapshot(&o)?, public())
    })
    .unwrap();
    for other in [
        TunnelTlsOwner {
            device_id: DeviceId::new(),
            ..o.clone()
        },
        TunnelTlsOwner {
            context_fingerprint: "f".repeat(64),
            ..o.clone()
        },
    ] {
        assert!(db.read(|tx| tx.tunnel_tls_snapshot(&other)).is_err());
    }
    db.conn
        .execute(
            "UPDATE local_settings SET value=?1 WHERE key=?2",
            params!["x".repeat(MAX_BYTES + 1), KEY],
        )
        .unwrap();
    assert!(db.read(|tx| tx.tunnel_tls_snapshot(&o)).is_err());
    db.conn
        .execute(
            "UPDATE local_settings SET value='{}' WHERE key=?1",
            params![KEY],
        )
        .unwrap();
    assert!(db.read(|tx| tx.tunnel_tls_snapshot(&o)).is_err());
}
#[test]
fn ordinary_settings_do_not_replace_identity_and_memory_never_acknowledges() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = Database::open(
        dir.path().join("settings.db"),
        &DatabaseKey::from_bytes([22; 32]),
    )
    .unwrap();
    let o = owner();
    setup(&mut db, &o);
    let p = public();
    db.tunnel_tls_write::<_, StorageError>(|tx| {
        tx.tunnel_tls_reserve(&tx.tunnel_tls_snapshot(&o)?, p.clone())
    })
    .unwrap();
    db.write(|tx| {
        tx.setting_set(
            "automation.runners.v1",
            &serde_json::json!({"changed":true}),
        )
    })
    .unwrap();
    assert_eq!(
        db.read(|tx| tx.tunnel_tls_snapshot(&o))
            .unwrap()
            .public()
            .unwrap()
            .identity_id,
        p.identity_id
    );
    let mut memory = Database::open_in_memory(&DatabaseKey::from_bytes([23; 32])).unwrap();
    let mut called = false;
    assert!(memory
        .tunnel_tls_write::<_, StorageError>(|_| {
            called = true;
            Ok(())
        })
        .is_err());
    assert!(!called);
}
