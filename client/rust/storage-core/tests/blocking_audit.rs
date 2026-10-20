use cc_storage_core::{DatabaseKey, Storage, StorageError};

#[tokio::test]
async fn synchronous_audit_acknowledges_commit_and_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audit.db");
    let storage = Storage::open(&path, DatabaseKey::from_bytes([0x41; 32]))
        .await
        .unwrap();
    storage
        .write_blocking(|tx| tx.setting_set("audit", &vec!["first"]))
        .unwrap();
    assert_eq!(
        storage
            .setting_get::<Vec<String>>("audit")
            .await
            .unwrap()
            .unwrap(),
        ["first"]
    );
    let failed: Result<(), StorageError> = storage.write_blocking(|tx| {
        tx.setting_set("audit", &vec!["must roll back"])?;
        Err(StorageError::Closed)
    });
    assert!(failed.is_err());
    let panicked: Result<(), StorageError> = storage.write_blocking(|tx| {
        tx.setting_set("audit", &vec!["must roll back on panic"])?;
        panic!("synthetic audit failure")
    });
    assert!(panicked.is_err());
    assert_eq!(
        storage
            .setting_get::<Vec<String>>("audit")
            .await
            .unwrap()
            .unwrap(),
        ["first"]
    );
    storage.close().await;
    assert!(storage
        .write_blocking(|tx| tx.setting_set("audit", "closed"))
        .is_err());
    let reopened = Storage::open(path, DatabaseKey::from_bytes([0x41; 32]))
        .await
        .unwrap();
    assert_eq!(
        reopened
            .setting_get::<Vec<String>>("audit")
            .await
            .unwrap()
            .unwrap(),
        ["first"]
    );
    reopened.close().await;
}
