//! SQLCipher metadata causals only; these tests do not fabricate a Node ACK.
use super::*;
use crate::{DatabaseKey, Profile, VaultRecord};
use cc_protocol::vaults::VaultRole;
fn owner() -> NodeAssociationOwner {
    NodeAssociationOwner {
        profile_id: ProfileId::new(),
        device_id: DeviceId::new(),
        vault_id: VaultId::new(),
        signing_fingerprint: "a".repeat(64),
        personal: None,
    }
}
fn public() -> NodeAssociationPublic {
    NodeAssociationPublic {
        association_id: Uuid::new_v4(),
        saved_host_id: ObjectId::new(),
        saved_host_revision: 1,
        route_trust_digest: "b".repeat(64),
        agent_id: Uuid::new_v4().to_string(),
        node_key_fingerprint: "c".repeat(64),
        owner_epoch: 1,
        checkpoint: 2,
        key_epoch: 1,
        expected_actor_revision: 3,
        client_tls_identity_id: Uuid::new_v4(),
        client_tls_certificate_sha256: "d".repeat(64),
        offer_digest: "e".repeat(64),
        signed_offer_json: b"public metadata fixture; Native must cryptographically verify"
            .to_vec(),
        certificate: None,
        owner_pin_json: b"public metadata fixture; not authority".to_vec(),
    }
}
fn setup(db: &mut Database, o: &NodeAssociationOwner) {
    db.write(|tx| {
        tx.put_profile(&Profile::new_local(o.profile_id, o.device_id, None))?;
        tx.put_vault(&VaultRecord::new(o.vault_id, VaultRole::Owner))
    })
    .unwrap();
}
fn reserve(db: &mut Database, o: &NodeAssociationOwner, p: NodeAssociationPublic) {
    db.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_reserve(&tx.node_association_snapshot(o)?, p)
    })
    .unwrap();
}

#[test]
fn sqlcipher_pending_unknown_survive_reopen_and_do_not_complete_or_enter_outbox() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("association.db");
    let key = DatabaseKey::from_bytes([61; 32]);
    let o = owner();
    let p = public();
    let mut db = Database::open(&path, &key).unwrap();
    setup(&mut db, &o);
    reserve(&mut db, &o, p.clone());
    let pending = db.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    assert_eq!(pending.records()[0].phase, NodeAssociationPhase::Pending);
    assert!(db
        .node_association_write::<_, StorageError>(
            |tx| tx.node_association_complete(&pending, p.association_id)
        )
        .is_err());
    db.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_mark_unknown(&pending, p.association_id)
    })
    .unwrap();
    drop(db);
    let mut db = Database::open(&path, &key).unwrap();
    let unknown = db.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    assert_eq!(unknown.records()[0].phase, NodeAssociationPhase::Unknown);
    assert_eq!(unknown.records()[0].public, p);
    assert_eq!(
        db.conn
            .query_row("SELECT count(*) FROM outbox", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn exact_full_cas_has_one_winner_and_sticky_block_prevents_old_ack_completion() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("race.db");
    let key = DatabaseKey::from_bytes([62; 32]);
    let o = owner();
    let p = public();
    let mut a = Database::open(&path, &key).unwrap();
    setup(&mut a, &o);
    let mut b = Database::open(&path, &key).unwrap();
    let absent_a = a.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    let absent_b = b.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    a.node_association_write::<_, StorageError>(|tx| {
        let full: i64 = tx.c().query_row("PRAGMA synchronous", [], |r| r.get(0))?;
        assert!(full >= 2);
        tx.node_association_reserve(&absent_a, p.clone())
    })
    .unwrap();
    assert!(b
        .node_association_write::<_, StorageError>(
            |tx| tx.node_association_reserve(&absent_b, public())
        )
        .is_err());
    let s = a.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    a.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_mark_unknown(&s, p.association_id)
    })
    .unwrap();
    let before_block = a.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    let stale = b.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    a.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_block(&before_block, p.association_id)
    })
    .unwrap();
    assert!(b
        .node_association_write::<_, StorageError>(
            |tx| tx.node_association_complete(&stale, p.association_id)
        )
        .is_err());
    let blocked = a.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    assert_eq!(blocked.records()[0].phase, NodeAssociationPhase::Blocked);
    assert!(a
        .node_association_write::<_, StorageError>(
            |tx| tx.node_association_complete(&blocked, p.association_id)
        )
        .is_err());
    assert!(a
        .node_association_write::<_, StorageError>(
            |tx| tx.node_association_reserve(&blocked, p.clone())
        )
        .is_err());
}

#[test]
fn stage_unknown_cannot_complete_without_one_exact_certificate_attachment() {
    let root = tempfile::tempdir().unwrap();
    let mut db = Database::open(
        root.path().join("staged.db"),
        &DatabaseKey::from_bytes([65; 32]),
    )
    .unwrap();
    let o = owner();
    setup(&mut db, &o);
    let mut p = public();
    // Actual initial Actor revisions/checkpoints are zero, not UUID-derived.
    p.checkpoint = 0;
    p.expected_actor_revision = 0;
    reserve(&mut db, &o, p.clone());
    let s = db.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    db.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_mark_unknown(&s, p.association_id)
    })
    .unwrap();
    let s = db.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    assert!(db
        .node_association_write::<_, StorageError>(
            |tx| tx.node_association_complete(&s, p.association_id)
        )
        .is_err());
    let cert = NodeAssociationCertificate {
        request_digest: "e".repeat(64),
        certificate_digest: "f".repeat(64),
        signed_certificate_json: b"public metadata; does not fabricate a Node ACK".to_vec(),
        committed_owner_pin_json: b"public metadata only".to_vec(),
        expected_actor_revision: 3,
    };
    db.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_attach_certificate(&s, p.association_id, cert.clone())
    })
    .unwrap();
    // Exact once-only publication preserves both original Offer and cert data.
    assert!(db
        .node_association_write::<_, StorageError>(|tx| tx.node_association_attach_certificate(
            &s,
            p.association_id,
            cert.clone()
        ))
        .is_err());
    let attached = db.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    assert_eq!(attached.records()[0].phase, NodeAssociationPhase::Unknown);
    assert_eq!(
        attached.records()[0].public.signed_offer_json,
        p.signed_offer_json
    );
    // Native caller (separately crypto tested) is responsible for actual ACK.
    db.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_complete(&attached, p.association_id)
    })
    .unwrap();
    let associated = db.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    assert_eq!(
        associated.records()[0].phase,
        NodeAssociationPhase::Associated
    );
    assert_eq!(
        associated.records()[0].public.certificate.as_ref(),
        Some(&cert)
    );
    assert!(db
        .node_association_write::<_, StorageError>(
            |tx| tx.node_association_complete(&associated, p.association_id)
        )
        .is_err());
}

#[test]
fn foreign_profile_device_signer_and_future_or_oversize_rows_are_preserved_denied() {
    let root = tempfile::tempdir().unwrap();
    let mut db = Database::open(
        root.path().join("foreign.db"),
        &DatabaseKey::from_bytes([63; 32]),
    )
    .unwrap();
    let o = owner();
    setup(&mut db, &o);
    reserve(&mut db, &o, public());
    for changed in [
        NodeAssociationOwner {
            profile_id: ProfileId::new(),
            ..o.clone()
        },
        NodeAssociationOwner {
            device_id: DeviceId::new(),
            ..o.clone()
        },
        NodeAssociationOwner {
            signing_fingerprint: "b".repeat(64),
            ..o.clone()
        },
    ] {
        assert!(db
            .read(|tx| tx.node_association_snapshot(&changed))
            .is_err());
    }
    for raw in ["{\"version\":99}".to_owned(), "x".repeat(MAX_BYTES + 1)] {
        db.conn
            .execute(
                "UPDATE local_settings SET value=?2 WHERE key=?1",
                params![KEY, raw],
            )
            .unwrap();
        assert!(db.read(|tx| tx.node_association_snapshot(&o)).is_err());
        let actual: String = db
            .conn
            .query_row(
                "SELECT value FROM local_settings WHERE key=?1",
                params![KEY],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(actual, raw);
    }
}

#[test]
fn durable_association_metadata_rejects_memory_and_reader_role() {
    let mut memory = Database::open_in_memory(&DatabaseKey::from_bytes([64; 32])).unwrap();
    let o = owner();
    setup(&mut memory, &o);
    assert!(memory
        .node_association_write::<_, StorageError>(
            |tx| tx.node_association_reserve(&tx.node_association_snapshot(&o)?, public())
        )
        .is_err());
    let root = tempfile::tempdir().unwrap();
    let mut db = Database::open(
        root.path().join("role.db"),
        &DatabaseKey::from_bytes([65; 32]),
    )
    .unwrap();
    setup(&mut db, &o);
    db.write(|tx| tx.put_vault(&VaultRecord::new(o.vault_id, VaultRole::Viewer)))
        .unwrap();
    assert!(db.read(|tx| tx.node_association_snapshot(&o)).is_err());
}

// Storage-state tests only. These public metadata placeholders do not simulate
// a Node signature/readback or authorize calling Native completion.
fn associated_for_revocation(db: &mut Database, o: &NodeAssociationOwner) -> NodeAssociationPublic {
    let p = public();
    reserve(db, o, p.clone());
    let snapshot = db.read(|tx| tx.node_association_snapshot(o)).unwrap();
    db.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_mark_unknown(&snapshot, p.association_id)
    })
    .unwrap();
    let snapshot = db.read(|tx| tx.node_association_snapshot(o)).unwrap();
    db.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_attach_certificate(
            &snapshot,
            p.association_id,
            NodeAssociationCertificate {
                request_digest: "1".repeat(64),
                certificate_digest: "2".repeat(64),
                signed_certificate_json: b"STORAGE ONLY NOT SIGNED".to_vec(),
                committed_owner_pin_json: b"PUBLIC METADATA".to_vec(),
                expected_actor_revision: 3,
            },
        )
    })
    .unwrap();
    let snapshot = db.read(|tx| tx.node_association_snapshot(o)).unwrap();
    db.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_complete(&snapshot, p.association_id)
    })
    .unwrap();
    db.read(|tx| tx.node_association_snapshot(o))
        .unwrap()
        .records()[0]
        .public
        .clone()
}
fn revocation_intent() -> NodeAssociationRevocation {
    NodeAssociationRevocation {
        signed_revocation_json: b"STORAGE ONLY NOT SIGNED".to_vec(),
        committed_owner_pin_json: b"PUBLIC METADATA".to_vec(),
        expected_actor_revision: 4,
    }
}
#[test]
fn revocation_intent_block_survives_reopen_and_never_retries_or_reports_observed() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("revocation.db");
    let key = DatabaseKey::from_bytes([75; 32]);
    let mut db = Database::open(&path, &key).unwrap();
    let o = owner();
    setup(&mut db, &o);
    let p = associated_for_revocation(&mut db, &o);
    let before = db.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    let expected = before.records()[0].clone();
    let mut foreign = expected.clone();
    foreign.public.node_key_fingerprint = "3".repeat(64);
    assert!(db
        .node_association_write::<_, StorageError>(|tx| tx.node_association_begin_revocation(
            &before,
            &foreign,
            revocation_intent()
        ))
        .is_err());
    db.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_begin_revocation(&before, &expected, revocation_intent())
    })
    .unwrap();
    drop(db);
    let mut db = Database::open(&path, &key).unwrap();
    let blocked = db.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    assert_eq!(blocked.records()[0].phase, NodeAssociationPhase::Blocked);
    assert_eq!(
        blocked.records()[0].revocation.as_ref(),
        Some(&revocation_intent())
    );
    assert!(db
        .node_association_write::<_, StorageError>(|tx| tx.node_association_begin_revocation(
            &blocked,
            &expected,
            revocation_intent()
        ))
        .is_err());
    assert!(db
        .node_association_write::<_, StorageError>(|tx| tx.node_association_complete_revocation(
            &before,
            p.association_id,
            &revocation_intent()
        ))
        .is_err());
    assert_eq!(
        db.read(|tx| tx.node_association_snapshot(&o))
            .unwrap()
            .records()[0]
            .phase,
        NodeAssociationPhase::Blocked
    );
}
#[test]
fn revocation_completion_requires_current_exact_intent_and_legacy_pair_is_optional() {
    let root = tempfile::tempdir().unwrap();
    let mut db = Database::open(
        root.path().join("revocation.db"),
        &DatabaseKey::from_bytes([76; 32]),
    )
    .unwrap();
    let o = owner();
    setup(&mut db, &o);
    let p = associated_for_revocation(&mut db, &o);
    let snapshot = db.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    let legacy = serde_json::to_value(&snapshot.records()[0]).unwrap();
    assert!(legacy.get("revocation").is_none());
    let decoded: NodeAssociationRecord = serde_json::from_value(legacy).unwrap();
    assert!(decoded.revocation.is_none());
    db.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_begin_revocation(&snapshot, &snapshot.records()[0], revocation_intent())
    })
    .unwrap();
    let blocked = db.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    let mut wrong = revocation_intent();
    wrong.expected_actor_revision += 1;
    assert!(db
        .node_association_write::<_, StorageError>(|tx| tx.node_association_complete_revocation(
            &blocked,
            p.association_id,
            &wrong
        ))
        .is_err());
    db.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_complete_revocation(&blocked, p.association_id, &revocation_intent())
    })
    .unwrap();
    assert_eq!(
        db.read(|tx| tx.node_association_snapshot(&o))
            .unwrap()
            .records()[0]
            .phase,
        NodeAssociationPhase::Revoked
    );
    assert_eq!(
        db.conn
            .query_row("SELECT count(*) FROM outbox", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn rotation_same_row_intent_is_monotonic_and_unknown_survives_reopen() {
    // Storage mechanics only; these unsigned bytes do NOT fabricate Node ACK.
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("rotation.db");
    let key = DatabaseKey::from_bytes([79; 32]);
    let mut db = Database::open(&path, &key).unwrap();
    let o = owner();
    setup(&mut db, &o);
    let p = associated_for_revocation(&mut db, &o);
    let before = db.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    let expected = before.records()[0].clone();
    let empty = NodeAssociationRotation {
        candidate_json: None,
        signed_certificate_json: None,
        old_proof_json: None,
        new_proof_json: None,
        committed_owner_pin_json: None,
        expected_actor_revision: 4,
        observed_owner_pin_json: None,
        observed_actor_revision: None,
        selected_observed: false,
    };
    db.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_begin_rotation(&before, &expected, empty.clone())
    })
    .unwrap();
    let blocked = db.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    let mut candidate = empty.clone();
    candidate.candidate_json = Some(b"PUBLIC FIXTURE ONLY".to_vec());
    db.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_update_rotation(&blocked, p.association_id, &empty, candidate.clone())
    })
    .unwrap();
    drop(db);
    let mut db = Database::open(&path, &key).unwrap();
    let blocked = db.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    assert_eq!(blocked.records()[0].phase, NodeAssociationPhase::Blocked);
    assert_eq!(blocked.records()[0].rotation.as_ref(), Some(&candidate));
    assert_eq!(blocked.records()[0].public, p);
    let mut replacement = candidate.clone();
    replacement.candidate_json = Some(b"DIFFERENT".to_vec());
    assert!(db
        .node_association_write::<_, StorageError>(|tx| tx.node_association_update_rotation(
            &blocked,
            p.association_id,
            &candidate,
            replacement
        ))
        .is_err());
    assert!(db
        .node_association_write::<_, StorageError>(
            |tx| tx.node_association_begin_rotation(&blocked, &expected, empty)
        )
        .is_err());
    assert_eq!(
        db.read(|tx| tx.node_association_snapshot(&o))
            .unwrap()
            .records()[0]
            .phase,
        NodeAssociationPhase::Blocked
    );
    assert_eq!(
        db.conn
            .query_row("SELECT count(*) FROM outbox", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

fn associated_for_service(db: &mut Database, o: &NodeAssociationOwner) -> Uuid {
    let p = public();
    let id = p.association_id;
    reserve(db, o, p);
    db.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_mark_unknown(&tx.node_association_snapshot(o)?, id)
    })
    .unwrap();
    let c = NodeAssociationCertificate {
        request_digest: "f".repeat(64),
        certificate_digest: "e".repeat(64),
        signed_certificate_json: b"fixture public bytes: not a signature proof".to_vec(),
        committed_owner_pin_json: b"fixture metadata only".to_vec(),
        expected_actor_revision: 3,
    };
    db.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_attach_certificate(&tx.node_association_snapshot(o)?, id, c)
    })
    .unwrap();
    db.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_complete(&tx.node_association_snapshot(o)?, id)
    })
    .unwrap();
    id
}
fn service_intent() -> NodeCollectorService {
    NodeCollectorService {
        credential_set: None,
        service_id: Uuid::new_v4(),
        delegation_sha256: "a".repeat(64),
        signed_delegation_json: b"fixture Native-verification responsibility; not authority"
            .to_vec(),
        manifest_json: b"fixture full manifest bytes, checked by Native".to_vec(),
        owner_pin_json: b"fixture original pin bytes, checked by Native".to_vec(),
        expected_actor_revision: 4,
        signed_claim_json: None,
        signed_receipt_json: None,
        configuration_recorded: false,revocation:None,
    }
}
#[test]
fn collector_service_unknown_survives_reopen_and_cannot_be_reconfigured_or_enter_outbox() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("service.db");
    let key = DatabaseKey::from_bytes([91; 32]);
    let o = owner();
    let mut db = Database::open(&path, &key).unwrap();
    setup(&mut db, &o);
    let id = associated_for_service(&mut db, &o);
    let s = db.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    let row = s.records()[0].clone();
    let intent = service_intent();
    db.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_begin_collector_service(&s, &row, intent.clone())
    })
    .unwrap();
    drop(db);
    let mut db = Database::open(&path, &key).unwrap();
    let actual = db.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    assert_eq!(actual.records()[0].public.association_id, id);
    assert_eq!(actual.records()[0].collector_service, Some(intent));
    let row = actual.records()[0].clone();
    assert!(db
        .node_association_write::<_, StorageError>(
            |tx| tx.node_association_begin_collector_service(&actual, &row, service_intent())
        )
        .is_err());
    assert_eq!(
        db.conn
            .query_row("SELECT count(*) FROM outbox", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn collector_service_completion_rejects_stale_or_foreign_intent_and_is_one_use() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("service-cas.db");
    let key = DatabaseKey::from_bytes([92; 32]);
    let o = owner();
    let mut a = Database::open(&path, &key).unwrap();
    setup(&mut a, &o);
    let id = associated_for_service(&mut a, &o);
    let mut b = Database::open(&path, &key).unwrap();
    let stale = b.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    let s = a.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    let intent = service_intent();
    a.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_begin_collector_service(&s, &s.records()[0], intent.clone())
    })
    .unwrap();
    assert!(b
        .node_association_write::<_, StorageError>(|tx| tx
            .node_association_begin_collector_service(
                &stale,
                &stale.records()[0],
                service_intent()
            ))
        .is_err());
    let s = a.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    let mut foreign = intent.clone();
    foreign.service_id = Uuid::new_v4();
    assert!(a
        .node_association_write::<_, StorageError>(|tx| tx
            .node_association_complete_collector_service(
                &s,
                id,
                &foreign,
                b"claim".to_vec(),
                b"receipt".to_vec()
            ))
        .is_err());
    a.node_association_write::<_, StorageError>(|tx| {
        tx.node_association_complete_collector_service(
            &s,
            id,
            &intent,
            b"claim data checked by caller".to_vec(),
            b"receipt data checked by caller".to_vec(),
        )
    })
    .unwrap();
    let done = a.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    let recorded = done.records()[0].collector_service.clone().unwrap();
    assert!(recorded.configuration_recorded);
    assert!(a
        .node_association_write::<_, StorageError>(|tx| tx
            .node_association_complete_collector_service(
                &done,
                id,
                &recorded,
                b"replacement".to_vec(),
                b"replacement".to_vec()
            ))
        .is_err());
}
#[test]
fn collector_service_malformed_pair_and_absent_or_blocked_original_row_are_denied() {
    let root = tempfile::tempdir().unwrap();
    let o = owner();
    let mut db = Database::open(
        root.path().join("service-refusal.db"),
        &DatabaseKey::from_bytes([93; 32]),
    )
    .unwrap();
    setup(&mut db, &o);
    let id = associated_for_service(&mut db, &o);
    let s = db.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    let mut malformed = service_intent();
    malformed.signed_claim_json = Some(b"partial".to_vec());
    assert!(db
        .node_association_write::<_, StorageError>(
            |tx| tx.node_association_begin_collector_service(&s, &s.records()[0], malformed)
        )
        .is_err());
    db.node_association_write::<_, StorageError>(|tx| tx.node_association_block(&s, id))
        .unwrap();
    let blocked = db.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    assert!(db
        .node_association_write::<_, StorageError>(|tx| tx
            .node_association_begin_collector_service(
                &blocked,
                &blocked.records()[0],
                service_intent()
            ))
        .is_err());
    assert!(blocked.records()[0].collector_service.is_none());
}


#[test]
fn collector_service_legacy_absent_field_reads_none_without_rewrite_or_adoption() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("legacy-service.db");
    let key = DatabaseKey::from_bytes([94; 32]);
    let o = owner();
    let mut db = Database::open(&path, &key).unwrap();
    setup(&mut db, &o);
    let id = associated_for_service(&mut db, &o);
    let before = db.read(|tx| tx.node_association_raw()).unwrap().unwrap();
    let value: serde_json::Value = serde_json::from_slice(&before).unwrap();
    assert!(value["records"][0].get("collector_service").is_none());
    drop(db);

    let mut db = Database::open(&path, &key).unwrap();
    let snapshot = db.read(|tx| tx.node_association_snapshot(&o)).unwrap();
    assert_eq!(snapshot.records()[0].public.association_id, id);
    assert_eq!(snapshot.records()[0].phase, NodeAssociationPhase::Associated);
    assert!(snapshot.records()[0].collector_service.is_none());
    assert_eq!(db.read(|tx| tx.node_association_raw()).unwrap().unwrap(), before);
    assert_eq!(db.conn.query_row("SELECT count(*) FROM outbox", [], |r| r.get::<_, i64>(0)).unwrap(), 0);

    // Malformed present metadata is corruption, never legacy absence/None.
    let mut malformed = value;
    malformed["records"][0]["collector_service"] = serde_json::json!(false);
    let bad = serde_json::to_string(&malformed).unwrap();
    db.conn.execute("UPDATE local_settings SET value=?1 WHERE key=?2", params![bad, KEY]).unwrap();
    assert!(db.read(|tx| tx.node_association_snapshot(&o)).is_err());
    assert_eq!(db.read(|tx| tx.node_association_raw()).unwrap().unwrap(), bad.as_bytes());
}

#[path="collector_service_lifetime_tests.rs"]
mod lifetime_tests;
