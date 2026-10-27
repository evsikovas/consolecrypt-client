//! SQLite connections stay on this device: saved outside the vault, listed
//! with the vault's connections, opened read-only, recorded in local history.

mod common;

use cc_db_core::{DbCell, DbQueryEvent, DbQueryRequest};
use cc_models::database::{DbConnection, DbEngine, DbOptions};
use common::*;

#[tokio::test]
async fn sqlite_connections_are_device_local_and_open_files_read_only() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("notes.db");
    rusqlite::Connection::open(&file)
        .unwrap()
        .execute_batch("CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT); INSERT INTO notes (body) VALUES ('hello');")
        .unwrap();
    let app = common::app(&tmp.path().join("app"));
    app.create_local_profile("DB".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let objects = app.object_count().await.unwrap();
    let mut draft = DbConnection::new("Notes", "localhost", DbEngine::Sqlite);
    draft.id = cc_models::ObjectId::NIL;
    draft.database = file.to_string_lossy().into_owned();
    draft.production = false;
    let saved = app.db_save_connection(draft, None, None).await.unwrap();
    assert_eq!(saved.options, DbOptions::Sqlite { read_only: true });
    let listed = app.db_connections().await.unwrap();
    assert_eq!(listed, vec![saved.clone()]);
    // Not a vault object: nothing to sync, back up or share.
    assert_eq!(app.object_count().await.unwrap(), objects);

    // A secret is refused, and an engine never moves into the vault.
    assert!(app
        .db_save_connection(
            saved.clone(),
            Some(cc_models::secret::SecretValue::new("x".to_owned())),
            None
        )
        .await
        .is_err());
    let mut moved = saved.clone();
    moved.engine = DbEngine::Postgresql;
    moved.options = DbOptions::for_engine(DbEngine::Postgresql);
    moved.database.clear();
    moved.port = 5432;
    assert!(app.db_save_connection(moved, None, None).await.is_err());

    let live = app.db_connect(saved.id.to_string(), None).await.unwrap();
    assert!(live.info.read_only);
    let request = |text: &str| DbQueryRequest {
        text: text.into(),
        parameters: Vec::new(),
        approval_ticket: None,
    };
    let query = app
        .db_execute(live.info.id.clone(), request("SELECT body FROM notes"))
        .await
        .unwrap();
    let mut rows = Vec::new();
    for _ in 0..50 {
        let events = app.db_poll(query.clone(), 200).await.unwrap();
        let done = events
            .iter()
            .any(|event| matches!(event, DbQueryEvent::Done));
        for event in events {
            if let DbQueryEvent::Rows { rows: batch, .. } = event {
                rows.extend(batch);
            }
        }
        if done {
            break;
        }
    }
    assert_eq!(rows, vec![vec![DbCell::Text("hello".into())]]);
    assert!(app
        .db_execute(
            live.info.id.clone(),
            request("DELETE FROM notes WHERE id = 1")
        )
        .await
        .is_err());
    let history = app
        .db_history(saved.id.to_string(), String::new(), None, false, None, 50)
        .await
        .unwrap();
    assert_eq!(history.entries.len(), 1);
    assert_eq!(history.entries[0].engine, DbEngine::Sqlite);
    app.db_disconnect(live.info.id).await.unwrap();

    app.db_delete_connection(saved.id.to_string())
        .await
        .unwrap();
    assert!(app.db_connections().await.unwrap().is_empty());
    // Creating a file is explicit and refuses an existing one.
    let fresh = tmp.path().join("fresh.db");
    app.db_sqlite_create_file(fresh.to_string_lossy().into_owned())
        .await
        .unwrap();
    assert!(app
        .db_sqlite_create_file(fresh.to_string_lossy().into_owned())
        .await
        .is_err());
    // Close the storage threads before the process exits (Storage::close).
    app.shutdown().await.unwrap();
}
