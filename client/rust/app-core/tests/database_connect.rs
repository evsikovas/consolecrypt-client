//! A new connection exactly as the editor sends it: tested before it exists,
//! saved, then opened. Opt-in: CC_DB_LIVE_PG=host:port:user:password:database.

mod common;

use common::*;
use secrecy::SecretString;

#[tokio::test]
async fn new_connection_from_the_form_tests_saves_and_opens() {
    let Ok(spec) = std::env::var("CC_DB_LIVE_PG") else {
        eprintln!("CC_DB_LIVE_PG not set; skipped");
        return;
    };
    let parts: Vec<&str> = spec.splitn(5, ':').collect();
    let tmp = tempfile::tempdir().unwrap();
    let app = common::app(&tmp.path().join("app"));
    app.create_local_profile("DB".into(), PASSPHRASE.into())
        .await
        .unwrap();
    // What the bridge makes of the form's JSON (nil id, fresh timestamps).
    let now = chrono::Utc::now();
    let connection: cc_models::database::DbConnection = serde_json::from_value(serde_json::json!({
        "id": cc_models::ObjectId::NIL, "name": "forever/forever-postgres-0", "engine": "postgresql",
        "host": parts[0], "port": parts[1].parse::<u16>().unwrap(), "database": parts[4],
        "username": parts[2], "auth": {"password_secret_id": null, "mechanism": "default", "client_certificate": null},
        "tls": {"mode": "prefer", "certificate_authority_pem": null, "server_name": null},
        "route": {"kind": "direct"}, "production": false,
        "limits": {"max_rows": 1000, "statement_timeout_seconds": 60, "connect_timeout_seconds": 10, "result_memory_bytes": 67108864},
        "options": {"kind": "postgresql", "application_name": "ConsoleCrypt", "search_path": []},
        "group_id": null, "tags": ["kubernetes"], "color": null, "created_at": now, "updated_at": now,
    }))
    .unwrap();
    let password = || SecretString::from(parts[3].to_owned());
    let tested = app
        .db_test_connection(connection.clone(), Some(password()))
        .await
        .expect("test an unsaved connection");
    assert!(tested.info.server_version.starts_with("17"), "{tested:?}");
    // "prefer" to a loopback address after resolution: no TLS, nothing exposed.
    assert_eq!(
        tested.info.transport_security,
        cc_db_core::DbTransportSecurity::Tunnel
    );
    let saved = app
        .db_save_connection(
            connection,
            Some(cc_models::secret::SecretValue::new(parts[3].to_owned())),
            None,
        )
        .await
        .expect("save a new connection");
    assert_ne!(saved.id, cc_models::ObjectId::NIL);
    let live = app
        .db_connect(saved.id.to_string(), None)
        .await
        .expect("open with the stored password");
    app.db_disconnect(live.info.id.clone()).await.unwrap();
}
