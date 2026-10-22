//! Cassandra / ScyllaDB connections stay on this device in 0.9.2: they are
//! listed and usable, but never become vault objects (no sync, sharing or
//! backup), survive a lock, and cannot switch to a synced engine. With
//! CC_DB_LIVE_SCYLLA=host:port:user:password the stored password also opens
//! a real session.

mod common;

use cc_models::database::{DbConnection, DbEngine, DbTlsMode};
use cc_models::secret::SecretValue;
use common::*;

fn draft(engine: DbEngine, host: &str, port: u16) -> DbConnection {
    let mut connection = DbConnection::new("ring", host, engine);
    connection.id = cc_models::ObjectId::NIL;
    connection.port = port;
    connection.username = "cassandra".into();
    connection.production = false;
    connection.tls.mode = DbTlsMode::Disable;
    connection
}

async fn vault_objects(app: &cc_app_core::AppCore, dir: &std::path::Path, name: &str) -> u64 {
    app.export_backup(dir.join(name).to_string_lossy().to_string())
        .await
        .unwrap()
        .objects
}

#[tokio::test]
async fn cassandra_connections_stay_on_this_device() {
    let tmp = tempfile::tempdir().unwrap();
    let app = common::app(&tmp.path().join("app"));
    app.create_local_profile("DB".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let before = vault_objects(&app, tmp.path(), "before.ccbackup").await;

    let (host, port, user, password) = match std::env::var("CC_DB_LIVE_SCYLLA") {
        Ok(spec) => {
            let parts: Vec<String> = spec.splitn(4, ':').map(str::to_owned).collect();
            (
                parts[0].clone(),
                parts[1].parse().unwrap(),
                parts[2].clone(),
                parts[3].clone(),
            )
        }
        Err(_) => (
            "127.0.0.1".into(),
            9042,
            "cassandra".into(),
            "cassandra-canary".into(),
        ),
    };
    let mut connection = draft(DbEngine::Cassandra, &host, port);
    connection.username = user;
    let saved = app
        .db_save_connection(connection, Some(SecretValue::new(password.clone())), None)
        .await
        .unwrap();
    assert!(saved.auth.password_secret_id.is_some());
    let listed = app.db_connections().await.unwrap();
    assert!(listed
        .iter()
        .any(|c| c.id == saved.id && c.engine == DbEngine::Cassandra));
    // Neither the connection nor its password became a vault object.
    assert_eq!(
        vault_objects(&app, tmp.path(), "after.ccbackup").await,
        before
    );

    // A synced connection still does (control).
    let postgres = app
        .db_save_connection(
            draft(DbEngine::Postgresql, "db.example.test", 5432),
            Some(SecretValue::new("pg-password")),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        vault_objects(&app, tmp.path(), "control.ccbackup").await,
        before + 2
    );

    // No sharing, no synced history, no engine change across the stores.
    let shared = app
        .sharing_preview_db_connection(saved.id.to_string(), false)
        .await;
    assert!(shared.is_err());
    assert!(app
        .db_set_history_policy(
            saved.id.to_string(),
            serde_json::from_str("\"encrypted_sync\"").unwrap()
        )
        .await
        .is_err());
    app.db_set_history_policy(
        saved.id.to_string(),
        serde_json::from_str("\"local\"").unwrap(),
    )
    .await
    .unwrap();
    let mut moved = saved.clone();
    moved.engine = DbEngine::Postgresql;
    moved.options = cc_models::database::DbOptions::for_engine(DbEngine::Postgresql);
    let error = app.db_save_connection(moved, None, None).await.unwrap_err();
    assert!(
        format!("{error:?}").contains("engine_change_unsupported"),
        "{error:?}"
    );
    let mut moved = postgres.clone();
    moved.engine = DbEngine::Cassandra;
    moved.options = cc_models::database::DbOptions::for_engine(DbEngine::Cassandra);
    assert!(app.db_save_connection(moved, None, None).await.is_err());

    // Edits keep the stored password; the store survives a lock.
    let mut renamed = saved.clone();
    renamed.name = "ring (renamed)".into();
    let renamed = app.db_save_connection(renamed, None, None).await.unwrap();
    assert_eq!(
        renamed.auth.password_secret_id,
        saved.auth.password_secret_id
    );
    app.lock().await.unwrap();
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    let listed = app.db_connections().await.unwrap();
    let restored = listed.iter().find(|c| c.id == saved.id).unwrap();
    assert_eq!(restored.name, "ring (renamed)");
    assert_eq!(
        restored.auth.password_secret_id,
        saved.auth.password_secret_id
    );

    if std::env::var("CC_DB_LIVE_SCYLLA").is_ok() {
        // The password kept on this device opens the session.
        let live = app.db_connect(saved.id.to_string(), None).await.unwrap();
        assert!(live.info.server_version.starts_with("ScyllaDB"));
        // Saving the connection again closes its live sessions.
        app.db_save_connection(restored.clone(), None, None)
            .await
            .unwrap();
        assert!(app.db_disconnect(live.info.id.clone()).await.is_ok());
    }

    app.db_delete_connection(saved.id.to_string())
        .await
        .unwrap();
    assert!(!app
        .db_connections()
        .await
        .unwrap()
        .iter()
        .any(|c| c.id == saved.id));
    app.lock().await.unwrap();
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert!(!app
        .db_connections()
        .await
        .unwrap()
        .iter()
        .any(|c| c.id == saved.id));
    app.shutdown().await.unwrap();
}
