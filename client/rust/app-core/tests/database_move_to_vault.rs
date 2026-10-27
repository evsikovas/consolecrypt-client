//! FEAT-01: a device-local Cassandra connection (or SQLite through a host)
//! moves into the vault with its password, then syncs and is backed up with
//! it; an SQLite file on this computer stays. On a synced account the move
//! waits until every active device runs 0.9.2 or later.

mod common;

use cc_app_core::*;
use cc_models::database::{DbConnection, DbEngine, DbOptions, DbRoute, DbTlsMode};
use cc_models::secret::SecretValue;
use cc_sync_core::mock::MockServer;
use common::*;

fn cassandra() -> DbConnection {
    let mut connection = DbConnection::new("ring", "10.0.0.5", DbEngine::Cassandra);
    connection.id = cc_models::ObjectId::NIL;
    connection.username = "cassandra".into();
    connection.production = false;
    connection.tls.mode = DbTlsMode::Disable;
    connection
}

async fn vault_objects(app: &AppCore, dir: &std::path::Path, name: &str) -> u64 {
    app.export_backup(dir.join(name).to_string_lossy().to_string())
        .await
        .unwrap()
        .objects
}

#[tokio::test]
async fn a_local_profile_moves_cassandra_into_the_vault() {
    let tmp = tempfile::tempdir().unwrap();
    let app = common::app(&tmp.path().join("app"));
    app.create_local_profile("DB".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let saved = app
        .db_save_connection(cassandra(), Some(SecretValue::new("ring-password")), None)
        .await
        .unwrap();
    assert_eq!(
        app.db_device_only_ids().await.unwrap(),
        [saved.id.to_string()]
    );
    let before = vault_objects(&app, tmp.path(), "before.ccbackup").await;
    assert!(app.db_sync_readiness().await.unwrap().ready);

    let moved = app
        .db_move_connection_to_vault(saved.id.to_string())
        .await
        .unwrap();
    assert_eq!(moved.id, saved.id);
    assert!(app.db_device_only_ids().await.unwrap().is_empty());
    // The connection and its password are vault objects now.
    assert_eq!(
        vault_objects(&app, tmp.path(), "after.ccbackup").await,
        before + 2
    );
    let listed = app.db_connections().await.unwrap();
    assert_eq!(listed.iter().filter(|c| c.id == saved.id).count(), 1);
    // Edits keep it in the vault with its password.
    let mut renamed = moved.clone();
    renamed.name = "ring (vault)".into();
    let renamed = app.db_save_connection(renamed, None, None).await.unwrap();
    assert_eq!(
        renamed.auth.password_secret_id,
        saved.auth.password_secret_id
    );
    assert!(app.db_device_only_ids().await.unwrap().is_empty());
    assert_eq!(
        vault_objects(&app, tmp.path(), "edited.ccbackup").await,
        before + 2
    );
    // Moving twice is refused.
    assert!(app
        .db_move_connection_to_vault(saved.id.to_string())
        .await
        .is_err());

    // An SQLite file on this computer stays here.
    let mut file = DbConnection::new("local file", "localhost", DbEngine::Sqlite);
    file.id = cc_models::ObjectId::NIL;
    file.database = tmp.path().join("x.db").to_string_lossy().into_owned();
    file.options = DbOptions::Sqlite { read_only: true };
    file.route = DbRoute::Direct;
    let file = app.db_save_connection(file, None, None).await.unwrap();
    let error = app
        .db_move_connection_to_vault(file.id.to_string())
        .await
        .unwrap_err();
    assert!(format!("{error:?}").contains("device_file"), "{error:?}");
    // Close the storage threads before the process exits (Storage::close).
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_synced_account_waits_for_its_devices() {
    let server = MockServer::start().await;
    let tmp = tempfile::tempdir().unwrap();
    // Tests run as app 0.1.0: an old app for the purpose of this check.
    let app = common::app(tmp.path());
    app.create_synced_profile(
        "Work".into(),
        server.url().to_string(),
        "move@example.org".into(),
        ACCOUNT_PASSWORD.into(),
        AccountMode::Register,
    )
    .await
    .unwrap();
    app.create_vault(PASSPHRASE.into()).await.unwrap();
    let saved = app
        .db_save_connection(cassandra(), Some(SecretValue::new("ring-password")), None)
        .await
        .unwrap();
    let readiness = app.db_sync_readiness().await.unwrap();
    assert!(!readiness.ready);
    assert_eq!(readiness.outdated_devices.len(), 1);
    assert_eq!(
        readiness.outdated_devices[0].client_version.as_deref(),
        Some("0.1.0")
    );
    let error = app
        .db_move_connection_to_vault(saved.id.to_string())
        .await
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("devices_outdated"),
        "{error:?}"
    );
    assert_eq!(
        app.db_device_only_ids().await.unwrap(),
        [saved.id.to_string()]
    );

    // The same account from an up-to-date app.
    let tmp2 = tempfile::tempdir().unwrap();
    let mut config = common::config(tmp2.path());
    config.client_version = "0.9.3".into();
    let current = AppCore::new(config).unwrap();
    current
        .create_synced_profile(
            "Work".into(),
            server.url().to_string(),
            "current@example.org".into(),
            ACCOUNT_PASSWORD.into(),
            AccountMode::Register,
        )
        .await
        .unwrap();
    current.create_vault(PASSPHRASE.into()).await.unwrap();
    assert!(current.db_sync_readiness().await.unwrap().ready);
    let saved = current
        .db_save_connection(cassandra(), Some(SecretValue::new("ring-password")), None)
        .await
        .unwrap();
    current
        .db_move_connection_to_vault(saved.id.to_string())
        .await
        .unwrap();
    assert!(current.db_device_only_ids().await.unwrap().is_empty());
    // It reaches the server like any vault object.
    current.sync_now().await.unwrap();
    current.shutdown().await.unwrap();
    app.shutdown().await.unwrap();
}
