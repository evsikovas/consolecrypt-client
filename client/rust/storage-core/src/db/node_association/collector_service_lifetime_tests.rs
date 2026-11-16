//! DRAFT/UNRUN actual LocalOnly SQLite CAS. Shape fixtures are NOT signed ACKs;
//! actual signature/whole-hash verification is Native's separate responsibility.
use super::*;
fn set_intent()->NodeCollectorServiceCredentialSet {
    NodeCollectorServiceCredentialSet {authorization_json:b"Native-verified original owner statement fixture".to_vec(),
        authorization_sha256:"b".repeat(64),ordered_capsule_sha256:vec!["c".repeat(64),"d".repeat(64)],
        minimum_actor_revision:6,signed_admission_receipt_json:None}
}
fn pending_service(db:&mut Database,o:&NodeAssociationOwner)->(Uuid,NodeCollectorService) {
    let id=associated_for_service(db,o);let s=db.read(|tx|tx.node_association_snapshot(o)).unwrap();
    let intent=service_intent();
    db.node_association_write::<_,StorageError>(|tx|tx.node_association_begin_collector_service(&s,&s.records()[0],intent.clone())).unwrap();
    (id,intent)
}
#[test]
fn lifetime_pending_intent_survives_reopen_and_never_enters_outbox() {
    let root=tempfile::tempdir().unwrap();let path=root.path().join("lifetime.db");let key=DatabaseKey::from_bytes([94;32]);
    let o=owner();let mut db=Database::open(&path,&key).unwrap();setup(&mut db,&o);
    let (id,service)=pending_service(&mut db,&o);let s=db.read(|tx|tx.node_association_snapshot(&o)).unwrap();
    let intent=set_intent();db.node_association_write::<_,StorageError>(|tx|tx.node_association_begin_service_credential_set(&s,id,&service,intent.clone())).unwrap();
    drop(db);let mut db=Database::open(&path,&key).unwrap();let s=db.read(|tx|tx.node_association_snapshot(&o)).unwrap();
    let stored=s.records()[0].collector_service.clone().unwrap();
    assert!(stored.credential_set==Some(intent));
    assert!(db.node_association_write::<_,StorageError>(|tx|tx.node_association_begin_service_credential_set(&s,id,&stored,set_intent())).is_err());
    assert_eq!(db.conn.query_row("SELECT count(*) FROM outbox",[],|r|r.get::<_,i64>(0)).unwrap(),0);
}
#[test]
fn lifetime_stale_snapshot_and_foreign_pair_cannot_complete_sql_intent() {
    let root=tempfile::tempdir().unwrap();let path=root.path().join("lifetime-cas.db");let key=DatabaseKey::from_bytes([95;32]);
    let o=owner();let mut a=Database::open(&path,&key).unwrap();setup(&mut a,&o);
    let (id,service)=pending_service(&mut a,&o);let mut b=Database::open(&path,&key).unwrap();
    let stale=b.read(|tx|tx.node_association_snapshot(&o)).unwrap();
    let s=a.read(|tx|tx.node_association_snapshot(&o)).unwrap();
    a.node_association_write::<_,StorageError>(|tx|tx.node_association_begin_service_credential_set(&s,id,&service,set_intent())).unwrap();
    assert!(b.node_association_write::<_,StorageError>(|tx|tx.node_association_begin_service_credential_set(&stale,id,&service,set_intent())).is_err());
    let s=a.read(|tx|tx.node_association_snapshot(&o)).unwrap();let stored=s.records()[0].collector_service.clone().unwrap();
    let mut foreign=stored.clone();foreign.credential_set.as_mut().unwrap().ordered_capsule_sha256.swap(0,1);
    assert!(a.node_association_write::<_,StorageError>(|tx|tx.node_association_complete_service_credential_set(&s,id,&foreign,b"caller must verify actual signed receipt".to_vec())).is_err());
    a.node_association_write::<_,StorageError>(|tx|tx.node_association_complete_service_credential_set(&s,id,&stored,b"Native verified receipt fixture".to_vec())).unwrap();
    let s=a.read(|tx|tx.node_association_snapshot(&o)).unwrap();let done=s.records()[0].collector_service.clone().unwrap();
    assert!(a.node_association_write::<_,StorageError>(|tx|tx.node_association_complete_service_credential_set(&s,id,&done,b"replacement".to_vec())).is_err());
}
#[test]
fn lifetime_shape_bounds_reject_partial_or_precompleted_begin_before_sql_mutation() {
    let root=tempfile::tempdir().unwrap();let o=owner();let mut db=Database::open(root.path().join("bounds.db"),&DatabaseKey::from_bytes([96;32])).unwrap();
    setup(&mut db,&o);let (id,service)=pending_service(&mut db,&o);let s=db.read(|tx|tx.node_association_snapshot(&o)).unwrap();
    for kind in 0..3 {
        let mut bad=set_intent();
        match kind {0=>bad.ordered_capsule_sha256.clear(),1=>bad.authorization_json=vec![b'x';12289],_=>bad.signed_admission_receipt_json=Some(b"unverified".to_vec())}
        assert!(db.node_association_write::<_,StorageError>(|tx|tx.node_association_begin_service_credential_set(&s,id,&service,bad.clone())).is_err());
    }
    let current=db.read(|tx|tx.node_association_snapshot(&o)).unwrap();
    assert!(current.records()[0].collector_service.as_ref().unwrap().credential_set.is_none());
}
