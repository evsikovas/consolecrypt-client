//! SQLCipher storage causals only. Metadata fixtures are NOT signed grants,
//! Node ACKs or permission to reconnect. Draft tests; execution is separate.
use super::*;
use crate::{Database, DatabaseKey, NodeAssociationCertificate, NodeAssociationPersonalOwner,
    NodeAssociationPublic, Profile, ProfileId, VaultRecord};
use cc_models::ObjectId;
use cc_protocol::{DeviceId, UserId, VaultId, vaults::VaultRole};

const ASSOCIATIONS: &str = "cc.native.agent.node-associations.v1";

fn owner(personal: bool) -> NodeAssociationOwner {
    NodeAssociationOwner { profile_id: ProfileId::new(), device_id: DeviceId::new(), vault_id: VaultId::new(),
        signing_fingerprint: "a".repeat(64), personal: personal.then(|| NodeAssociationPersonalOwner {
            user_id: UserId::new(), server_url: "https://personal.example/".into(),
            origin_fingerprint: "b".repeat(64), vault_commitment: "c".repeat(64),
        }) }
}
fn setup(db: &mut Database, owner: &NodeAssociationOwner, scope: &str) -> NodeTunnelDestination {
    db.write(|tx| {
        let profile = match &owner.personal {
            None => Profile::new_local(owner.profile_id, owner.device_id, None),
            Some(p) => Profile::new_synced(owner.profile_id, owner.device_id, &p.server_url, Some(p.user_id), None),
        };
        tx.put_profile(&profile)?;
        let mut vault = VaultRecord::new(owner.vault_id, VaultRole::Owner);
        if let Some(p) = &owner.personal { vault.owner_user_id = Some(p.user_id); vault.caller_trusted = true; }
        tx.put_vault(&vault)
    }).unwrap();
    let public = NodeAssociationPublic { association_id: Uuid::new_v4(), saved_host_id: ObjectId::new(),
        saved_host_revision: 1, route_trust_digest: "1".repeat(64), agent_id: Uuid::new_v4().to_string(),
        node_key_fingerprint: "2".repeat(64), owner_epoch: 1, checkpoint: 1, key_epoch: 1,
        expected_actor_revision: 1, client_tls_identity_id: Uuid::new_v4(), client_tls_certificate_sha256: "3".repeat(64),
        offer_digest: "4".repeat(64), signed_offer_json: b"UNSIGNED STORAGE METADATA ONLY".to_vec(),
        owner_pin_json: b"UNSIGNED INITIAL PIN METADATA".to_vec(), certificate: None };
    let destination = NodeTunnelDestination { association_id: public.association_id, destination_ip: "127.0.0.1".into(), destination_port: 22 };
    db.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_reserve(&tx.node_association_snapshot(owner)?, public.clone())
    }).unwrap();
    db.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_mark_unknown(&tx.node_association_snapshot(owner)?, public.association_id)
    }).unwrap();
    let certificate = NodeAssociationCertificate { request_digest: "5".repeat(64), certificate_digest: "6".repeat(64),
        signed_certificate_json: serde_json::to_vec(&serde_json::json!({"body": {"scopes":[scope],
            "capabilities":{"version":1,"scopes":[scope]}},"signature":"UNSIGNED METADATA FIXTURE"})).unwrap(),
        committed_owner_pin_json: b"UNSIGNED COMMITTED PIN METADATA".to_vec(), expected_actor_revision: 3 };
    db.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_attach_certificate(&tx.node_association_snapshot(owner)?, public.association_id, certificate)
    }).unwrap();
    db.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_complete(&tx.node_association_snapshot(owner)?, public.association_id)
    }).unwrap();
    destination
}
fn set(db: &mut Database, o: &NodeAssociationOwner, d: &NodeTunnelDestination, enabled: bool) {
    db.node_association_write::<_, StorageError>(|tx| tx.node_tunnel_set_resume_preference(o, d, enabled)).unwrap();
}
fn get(db: &mut Database, o: &NodeAssociationOwner, d: &NodeTunnelDestination) -> bool {
    db.read(|tx| tx.node_tunnel_resume_preference(o, d)).unwrap()
}
fn raw(db: &mut Database) -> Option<Vec<u8>> { db.read(|tx| tx.node_tunnel_preference_raw()).unwrap() }
fn install_raw(db: &mut Database, bytes: &[u8]) {
    db.write(|tx| {
        tx.c().execute("INSERT OR REPLACE INTO local_settings(key,value,updated_at) VALUES(?1,?2,?3)",
            params![KEY, bytes, crate::sql::ts(&crate::sql::now())])?;
        Ok::<_, StorageError>(())
    }).unwrap();
}
fn change_association(db: &mut Database, change: impl FnOnce(&mut serde_json::Value)) {
    // Deliberately change actual stored row metadata, not the getter's result.
    // Existing ledger validator still runs on subsequent public reads.
    db.write(|tx| {
        let bytes: Vec<u8> = tx.c().query_row("SELECT CAST(value AS BLOB) FROM local_settings WHERE key=?1", [ASSOCIATIONS], |r| r.get(0))?;
        let mut ledger: serde_json::Value = serde_json::from_slice(&bytes)?;
        change(&mut ledger["records"][0]);
        tx.c().execute("UPDATE local_settings SET value=?2 WHERE key=?1", params![ASSOCIATIONS, serde_json::to_vec(&ledger)?])?;
        Ok::<_, StorageError>(())
    }).unwrap();
}

#[test]
fn encrypted_reopen_default_false_and_exact_destination_isolation() {
    let root = tempfile::tempdir().unwrap(); let path = root.path().join("preference.db");
    let key = DatabaseKey::from_bytes([121; 32]); let mut db = Database::open(&path, &key).unwrap();
    let o = owner(false); let d = setup(&mut db, &o, "tunnel.open");
    assert!(!get(&mut db, &o, &d)); assert!(raw(&mut db).is_none());
    let mut other = d.clone(); other.destination_port = 443;
    set(&mut db, &o, &d, true); assert!(get(&mut db, &o, &d)); assert!(!get(&mut db, &o, &other));
    set(&mut db, &o, &other, true); set(&mut db, &o, &d, false);
    assert!(!get(&mut db, &o, &d)); assert!(get(&mut db, &o, &other));
    drop(db);
    let file = std::fs::read(&path).unwrap(); assert!(!file.starts_with(b"SQLite format 3"));
    assert!(!file.windows(KEY.len()).any(|w| w == KEY.as_bytes()));
    assert!(Database::open(&path, &DatabaseKey::from_bytes([122; 32])).is_err());
    let mut db = Database::open(&path, &key).unwrap();
    assert!(!get(&mut db, &o, &d)); assert!(get(&mut db, &o, &other));
    db.read(|tx| { assert_eq!(tx.c().query_row("SELECT count(*) FROM outbox", [], |r| r.get::<_, i64>(0))?, 0); Ok::<_, StorageError>(()) }).unwrap();
}

#[test]
fn exact_personal_owner_and_same_profile_other_device_cannot_read_or_change() {
    let root = tempfile::tempdir().unwrap(); let mut db = Database::open(root.path().join("personal.db"), &DatabaseKey::from_bytes([123; 32])).unwrap();
    let o = owner(true); let d = setup(&mut db, &o, "tunnel.open"); set(&mut db, &o, &d, true);
    let before = raw(&mut db);
    for field in ["device", "user", "origin", "commitment", "signing"] {
        let mut foreign = o.clone();
        match field {
            "device" => foreign.device_id = DeviceId::new(),
            "user" => foreign.personal.as_mut().unwrap().user_id = UserId::new(),
            "origin" => foreign.personal.as_mut().unwrap().origin_fingerprint = "d".repeat(64),
            "commitment" => foreign.personal.as_mut().unwrap().vault_commitment = "e".repeat(64),
            _ => foreign.signing_fingerprint = "f".repeat(64),
        }
        assert!(db.read(|tx| tx.node_tunnel_resume_preference(&foreign, &d)).is_err());
        assert!(db.node_association_write::<_, StorageError>(|tx| tx.node_tunnel_set_resume_preference(&foreign, &d, false)).is_err());
        assert_eq!(raw(&mut db), before);
    }
    assert!(get(&mut db, &o, &d));
}

#[test]
fn actual_row_revision_trust_certificate_key_and_pin_changes_invalidate_without_repair() {
    for field in ["host_revision", "trust", "certificate", "key_epoch", "node_key", "owner_pin", "tls", "selected_rotation"] {
        let root = tempfile::tempdir().unwrap(); let mut db = Database::open(root.path().join("stale.db"), &DatabaseKey::from_bytes([124; 32])).unwrap();
        let o = owner(false); let d = setup(&mut db, &o, "tunnel.open"); set(&mut db, &o, &d, true);
        let before = raw(&mut db);
        change_association(&mut db, |row| match field {
            "host_revision" => row["public"]["saved_host_revision"] = 2.into(),
            "trust" => row["public"]["route_trust_digest"] = "7".repeat(64).into(),
            "certificate" => row["public"]["certificate"]["certificate_digest"] = "8".repeat(64).into(),
            "key_epoch" => row["public"]["key_epoch"] = 2.into(),
            "node_key" => row["public"]["node_key_fingerprint"] = "9".repeat(64).into(),
            "owner_pin" => row["public"]["certificate"]["committed_owner_pin_json"] = serde_json::json!([42]),
            "tls" => row["public"]["client_tls_certificate_sha256"] = "a".repeat(64).into(),
            _ => { let cert = row["public"]["certificate"]["signed_certificate_json"].clone(); row["rotation"] = serde_json::json!({
                "candidate_json":[1],"signed_certificate_json":cert,"old_proof_json":[2],"new_proof_json":[3],
                "committed_owner_pin_json":[4],"expected_actor_revision":4,"observed_owner_pin_json":[5],
                "observed_actor_revision":4,"selected_observed":true}); }
        });
        assert!(!get(&mut db, &o, &d)); assert_eq!(raw(&mut db), before);
        set(&mut db, &o, &d, false); assert_eq!(raw(&mut db), before); // Cannot delete stale descriptor.
        set(&mut db, &o, &d, true); assert!(get(&mut db, &o, &d)); // Explicit new preference, never automatic repair.
    }
}

#[test]
fn metrics_nonassociated_and_noncanonical_destinations_are_rejected() {
    let root = tempfile::tempdir().unwrap(); let mut db = Database::open(root.path().join("scope.db"), &DatabaseKey::from_bytes([125; 32])).unwrap();
    let o = owner(false); let d = setup(&mut db, &o, "metrics.collect");
    assert!(db.read(|tx| tx.node_tunnel_resume_preference(&o, &d)).is_err());
    assert!(db.node_association_write::<_, StorageError>(|tx| tx.node_tunnel_set_resume_preference(&o, &d, true)).is_err());
    assert!(raw(&mut db).is_none());
    // Independent Tunnel-only row: phase denial cannot be caused by Metrics.
    let mut db = Database::open(root.path().join("phase.db"), &DatabaseKey::from_bytes([125; 32])).unwrap();
    let o = owner(false); let d = setup(&mut db, &o, "tunnel.open");
    assert!(!get(&mut db, &o, &d));
    for phase in ["pending", "unknown", "blocked"] {
        change_association(&mut db, |row| row["phase"] = phase.into());
        assert!(db.read(|tx| tx.node_tunnel_resume_preference(&o, &d)).is_err());
    }
    change_association(&mut db, |row| {
        row["phase"] = "revoked".into();
        row["revocation"] = serde_json::json!({"signed_revocation_json":[1],"committed_owner_pin_json":[2],"expected_actor_revision":4});
    });
    assert!(db.read(|tx| tx.node_tunnel_resume_preference(&o, &d)).is_err());
    for (ip, port) in [("127.000.0.1",22),("localhost",22),("192.0.2.1",22),("::ffff:127.0.0.1",22),("::1",0)] {
        let mut invalid = d.clone(); invalid.destination_ip = ip.into(); invalid.destination_port = port;
        assert!(!destination_valid(&invalid));
    }
    let mut ipv6 = d.clone(); ipv6.destination_ip = "::1".into(); assert!(destination_valid(&ipv6));
}

#[test]
fn unknown_version_duplicate_entries_fields_and_oversize_preserve_exact_bytes() {
    let root = tempfile::tempdir().unwrap(); let mut db = Database::open(root.path().join("future.db"), &DatabaseKey::from_bytes([126; 32])).unwrap();
    let o = owner(false); let d = setup(&mut db, &o, "tunnel.open"); set(&mut db, &o, &d, true);
    let valid = raw(&mut db).unwrap(); let mut future: serde_json::Value = serde_json::from_slice(&valid).unwrap(); future["version"] = 99.into();
    let mut duplicate: serde_json::Value = serde_json::from_slice(&valid).unwrap();
    let first = duplicate["entries"][0].clone(); duplicate["entries"].as_array_mut().unwrap().push(first);
    let duplicate_field = String::from_utf8(valid.clone()).unwrap().replacen("\"version\":1", "\"version\":1,\"version\":1", 1).into_bytes();
    for bytes in [serde_json::to_vec(&future).unwrap(), serde_json::to_vec(&duplicate).unwrap(), duplicate_field, vec![b'x'; MAX_BYTES + 1]] {
        install_raw(&mut db, &bytes);
        assert!(db.read(|tx| tx.node_tunnel_resume_preference(&o, &d)).is_err());
        for enabled in [true, false] {
            assert!(db.node_association_write::<_, StorageError>(|tx| tx.node_tunnel_set_resume_preference(&o, &d, enabled)).is_err());
            let retained: Vec<u8> = db.read(|tx| tx.c().query_row("SELECT CAST(value AS BLOB) FROM local_settings WHERE key=?1", [KEY], |r| r.get(0)).map_err(StorageError::from)).unwrap();
            assert_eq!(retained, bytes);
        }
    }
}

#[test]
fn entry_and_byte_limits_refuse_addition_without_evicting_existing_preferences() {
    let root = tempfile::tempdir().unwrap(); let mut db = Database::open(root.path().join("bounds.db"), &DatabaseKey::from_bytes([127; 32])).unwrap();
    let o = owner(false); let d = setup(&mut db, &o, "tunnel.open");
    // With full snapshots the byte ceiling may be reached before128. Neither
    // cap is silently enlarged and older destinations are never evicted.
    let mut accepted = Vec::new(); let mut refused = false;
    for port in 1..=129 {
        let mut next = d.clone(); next.destination_port = port;
        let before = raw(&mut db);
        match db.node_association_write::<_, StorageError>(|tx| tx.node_tunnel_set_resume_preference(&o, &next, true)) {
            Ok(()) => accepted.push(next),
            Err(_) => { assert_eq!(raw(&mut db), before); refused = true; break; }
        }
    }
    assert!(refused); assert!(!accepted.is_empty()); assert!(accepted.len() <= MAX_ENTRIES);
    for previous in accepted { assert!(get(&mut db, &o, &previous)); }
}
