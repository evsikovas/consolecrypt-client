//! Local SQLCipher metadata causals; these do not fabricate a Node ACK.
use super::*;
use crate::{Database, DatabaseKey, NodeAssociationPublic, Profile, VaultRecord};
use cc_protocol::{DeviceId, VaultId, vaults::VaultRole};

fn setup(db: &mut Database) -> (NodeAssociationOwner, NodeTunnelAccessRecord) {
    let owner = NodeAssociationOwner { profile_id: crate::ProfileId::new(), device_id: DeviceId::new(),
        vault_id: VaultId::new(), signing_fingerprint: "a".repeat(64), personal: None };
    db.write(|tx| {
        tx.put_profile(&Profile::new_local(owner.profile_id, owner.device_id, None))?;
        tx.put_vault(&VaultRecord::new(owner.vault_id, VaultRole::Owner))
    }).unwrap();
    let public = NodeAssociationPublic { association_id: Uuid::new_v4(), saved_host_id: ObjectId::new(),
        saved_host_revision: 1, route_trust_digest: "b".repeat(64), agent_id: Uuid::new_v4().to_string(),
        node_key_fingerprint: "c".repeat(64), owner_epoch: 1, checkpoint: 2, key_epoch: 1,
        expected_actor_revision: 3, client_tls_identity_id: Uuid::new_v4(), client_tls_certificate_sha256: "d".repeat(64),
        offer_digest: "e".repeat(64), signed_offer_json: b"metadata fixture; not authority".to_vec(),
        owner_pin_json: b"metadata fixture; not authority".to_vec(), certificate: None };
    db.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_reserve(&tx.node_association_snapshot(&owner)?, public.clone())
    }).unwrap();
    let record = NodeTunnelAccessRecord { operation_id: Uuid::new_v4(), association_id: public.association_id,
        host_id: public.saved_host_id, host_revision: 1, grant_id: Uuid::new_v4(), grant_digest: "f".repeat(64),
        policy_digest: "a".repeat(64), agent_id: public.agent_id, key_epoch: 1, protocol: "tcp".into(),
        destination_ip: "127.0.0.1".into(), destination_port: 22, expires_at: 100, observed_at: 10,
        event: NodeTunnelAccessEvent::OpenRequested, sent: 0, received: 0,
        counters_exact: true, native_control_finished: false };
    (owner, record)
}

#[test]
fn encrypted_local_observations_survive_reopen_without_entering_outbox() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tunnel-access.db");
    let key = DatabaseKey::from_bytes([81; 32]);
    let mut db = Database::open(&path, &key).unwrap();
    let (owner, mut record) = setup(&mut db);
    db.node_association_write::<_, StorageError>(|tx| tx.node_tunnel_access_append(&owner, record.clone())).unwrap();
    record.event = NodeTunnelAccessEvent::Unknown;
    record.observed_at = 101; // A terminal observation is valid after grant expiry.
    record.native_control_finished = true;
    record.sent = 13;
    record.counters_exact = false;
    db.node_association_write::<_, StorageError>(|tx| tx.node_tunnel_access_append(&owner, record.clone())).unwrap();
    drop(db);
    let mut db = Database::open(&path, &key).unwrap();
    let rows = db.read(|tx| tx.node_tunnel_access_records(&owner)).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].profile_id, owner.profile_id);
    assert_eq!(rows[1].device_id, owner.device_id);
    assert!(matches!(rows[1].record.event, NodeTunnelAccessEvent::Unknown));
    assert_eq!(rows[1].record.sent, 13);
    assert!(!rows[1].record.counters_exact);
    db.read(|tx| {
        assert_eq!(tx.c().query_row("SELECT count(*) FROM outbox", [], |r| r.get::<_, i64>(0))?, 0);
        Ok::<_, StorageError>(())
    }).unwrap();
}

#[test]
fn bounded_history_evicts_old_rows_and_foreign_owner_cannot_append() {
    let root = tempfile::tempdir().unwrap();
    let mut db = Database::open(root.path().join("bounded.db"), &DatabaseKey::from_bytes([82; 32])).unwrap();
    let (owner, record) = setup(&mut db);
    for _ in 0..130 {
        db.node_association_write::<_, StorageError>(|tx| tx.node_tunnel_access_append(&owner, record.clone())).unwrap();
    }
    let rows = db.read(|tx| tx.node_tunnel_access_records(&owner)).unwrap();
    assert_eq!(rows.len(), 128);
    assert_eq!(rows[0].sequence, 3);
    assert_eq!(rows[127].sequence, 130);
    let mut foreign = owner.clone();
    foreign.device_id = DeviceId::new();
    assert!(db.node_association_write::<_, StorageError>(|tx| tx.node_tunnel_access_append(&foreign, record.clone())).is_err());
    assert_eq!(db.read(|tx| tx.node_tunnel_access_records(&owner)).unwrap().last().unwrap().sequence, 130);
}

#[test]
fn unknown_journal_version_is_readonly_and_preserves_original_bytes() {
    let root = tempfile::tempdir().unwrap();
    let mut db = Database::open(root.path().join("future.db"), &DatabaseKey::from_bytes([83; 32])).unwrap();
    let (owner, record) = setup(&mut db);
    let raw = br#"{"version":99,"future":"retained without repair"}"#;
    db.write(|tx| {
        tx.c().execute("INSERT INTO local_settings(key,value,updated_at) VALUES(?1,?2,?3)",
            params![KEY, raw.as_slice(), crate::sql::ts(&crate::sql::now())])?;
        Ok::<_, StorageError>(())
    }).unwrap();
    assert!(db.node_association_write::<_, StorageError>(|tx| tx.node_tunnel_access_append(&owner, record.clone())).is_err());
    let retained = db.read(|tx| tx.node_tunnel_access_raw()).unwrap().unwrap();
    assert_eq!(retained, raw);
}
