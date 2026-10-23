//! The production write window through the app core: vault passphrase, the
//! typed connection name, review, history marks and re-locking.
//! Opt-in: CC_DB_LIVE_PG=host:port:user:password:database.

mod common;

use common::*;

fn request(text: &str, ticket: Option<String>) -> cc_db_core::DbQueryRequest {
    cc_db_core::DbQueryRequest {
        text: text.into(),
        parameters: Vec::new(),
        approval_ticket: ticket,
    }
}

/// Reviews, approves and runs one query to the end; returns the error code.
async fn run(app: &cc_app_core::AppCore, session: &str, sql: &str) -> Option<String> {
    let review = app
        .db_review(session.into(), request(sql, None))
        .await
        .unwrap();
    if review.risk == "blocked" {
        return Some("blocked".into());
    }
    let query = app
        .db_execute(session.into(), request(sql, Some(review.ticket)))
        .await
        .unwrap();
    loop {
        for event in app.db_poll(query.clone(), 500).await.unwrap() {
            match event {
                cc_db_core::DbQueryEvent::Done => return None,
                cc_db_core::DbQueryEvent::Error { code, .. } => return Some(code),
                _ => {}
            }
        }
    }
}

#[tokio::test]
async fn production_write_window_needs_passphrase_and_name_and_relocks() {
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
    let mut connection = cc_models::database::DbConnection::new(
        "orders-prod",
        parts[0],
        cc_models::database::DbEngine::Postgresql,
    );
    connection.port = parts[1].parse().unwrap();
    connection.username = parts[2].into();
    connection.database = parts[4].into();
    connection.production = true;
    connection.tls.mode = cc_models::database::DbTlsMode::Disable;
    // A new connection from the form has no id yet.
    connection.id = cc_models::ObjectId::NIL;
    let saved = app
        .db_save_connection(
            connection,
            Some(cc_models::secret::SecretValue::new(parts[3].to_owned())),
            None,
        )
        .await
        .unwrap();
    let live = app.db_connect(saved.id.to_string(), None).await.unwrap();
    let session = live.info.id.clone();
    assert!(live.can_unlock_production && !live.writes_allowed);
    assert_eq!(live.production_write_expires_at_ms, None);
    // No window: changing statements are blocked.
    assert_eq!(
        run(&app, &session, "CREATE TABLE cc_app_window (id int)")
            .await
            .as_deref(),
        Some("blocked")
    );
    // A wrong passphrase or an unknown length opens nothing.
    let error = app
        .db_unlock_production(
            session.clone(),
            cc_app_core::StepUp::Passphrase("wrong".into()),
            5,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(error, cc_app_core::AppError::WrongPassphrase),
        "{error:?}"
    );
    assert!(app
        .db_unlock_production(
            session.clone(),
            cc_app_core::StepUp::Passphrase(PASSPHRASE.into()),
            60
        )
        .await
        .is_err());
    assert!(
        !app.db_session_state(session.clone())
            .await
            .unwrap()
            .writes_allowed
    );
    // The window opens for the chosen length.
    let before = chrono::Utc::now().timestamp_millis();
    let open = app
        .db_unlock_production(
            session.clone(),
            cc_app_core::StepUp::Passphrase(PASSPHRASE.into()),
            15,
        )
        .await
        .unwrap();
    let expires = open.production_write_expires_at_ms.unwrap();
    assert!(open.writes_allowed && !open.info.read_only);
    assert!(expires > before + 14 * 60_000 && expires <= before + 15 * 60_000 + 1000);
    assert_eq!(
        run(&app, &session, "DROP TABLE IF EXISTS cc_app_window").await,
        None
    );
    assert_eq!(
        run(
            &app,
            &session,
            "CREATE TABLE cc_app_window (id int primary key, note text)"
        )
        .await,
        None
    );
    assert_eq!(run(&app, &session, "SELECT 1").await, None);
    // History marks the production changes, not the reads.
    let page = app
        .db_history(saved.id.to_string(), String::new(), None, false, None, 50)
        .await
        .unwrap();
    let marked: Vec<_> = page
        .entries
        .iter()
        .map(|entry| {
            (
                entry.text.expose_secret().to_owned(),
                entry.production_write,
            )
        })
        .collect();
    assert!(
        marked.contains(&(
            "CREATE TABLE cc_app_window (id int primary key, note text)".into(),
            true
        )),
        "{marked:?}"
    );
    assert!(marked.contains(&("SELECT 1".into(), false)), "{marked:?}");
    // Locking ends it at once.
    let locked = app.db_lock_production(session.clone()).await.unwrap();
    assert!(!locked.writes_allowed && locked.production_write_expires_at_ms.is_none());
    assert_eq!(
        run(&app, &session, "INSERT INTO cc_app_window VALUES (1, 'x')")
            .await
            .as_deref(),
        Some("blocked")
    );
    // A new session (after disconnect) starts locked again.
    app.db_unlock_production(
        session.clone(),
        cc_app_core::StepUp::Passphrase(PASSPHRASE.into()),
        5,
    )
    .await
    .unwrap();
    assert_eq!(run(&app, &session, "DROP TABLE cc_app_window").await, None);
    app.db_disconnect(session.clone()).await.unwrap();
    let again = app.db_connect(saved.id.to_string(), None).await.unwrap();
    assert!(!again.writes_allowed && again.production_write_expires_at_ms.is_none());
    // Locking the vault drops the session and with it any window.
    app.db_unlock_production(
        again.info.id.clone(),
        cc_app_core::StepUp::Passphrase(PASSPHRASE.into()),
        5,
    )
    .await
    .unwrap();
    app.lock().await.unwrap();
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert!(app.db_session_state(again.info.id.clone()).await.is_err());
}

/// 0.9.2: the window opens with a presence token from the OS check (Touch
/// ID / Windows Hello), bound to this session and used once.
#[tokio::test]
async fn production_write_window_takes_a_presence_token_for_this_session_once() {
    let Ok(spec) = std::env::var("CC_DB_LIVE_PG") else {
        eprintln!("CC_DB_LIVE_PG not set; skipped");
        return;
    };
    let parts: Vec<&str> = spec.splitn(5, ':').collect();
    let tmp = tempfile::tempdir().unwrap();
    let keys = std::sync::Arc::new(cc_app_core::platform::InMemoryDeviceKeys::new());
    let app = common::app_with_keys(&tmp.path().join("app"), memory_store(), keys);
    app.create_local_profile("DB".into(), PASSPHRASE.into())
        .await
        .unwrap();
    app.set_device_unlock_enabled(true, "r".into(), false)
        .await
        .unwrap();
    let mut connection = cc_models::database::DbConnection::new(
        "orders-prod",
        parts[0],
        cc_models::database::DbEngine::Postgresql,
    );
    connection.port = parts[1].parse().unwrap();
    connection.username = parts[2].into();
    connection.database = parts[4].into();
    connection.production = true;
    connection.tls.mode = cc_models::database::DbTlsMode::Disable;
    connection.id = cc_models::ObjectId::NIL;
    let saved = app
        .db_save_connection(
            connection,
            Some(cc_models::secret::SecretValue::new(parts[3].to_owned())),
            None,
        )
        .await
        .unwrap();
    let session = app
        .db_connect(saved.id.to_string(), None)
        .await
        .unwrap()
        .info
        .id;
    let token = |object: String| {
        let app = &app;
        async move {
            app.verify_user_presence("db_production_write".into(), object, "r".into())
                .await
                .unwrap()
        }
    };
    let presence = |t: String| cc_app_core::StepUp::Presence(t);
    // A token for another session or another action opens nothing.
    let other = token("other-session".into()).await;
    let error = app
        .db_unlock_production(session.clone(), presence(other), 5)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "passphrase_required");
    let confirm = app
        .verify_user_presence("confirm".into(), session.clone(), "r".into())
        .await
        .unwrap();
    assert!(app
        .db_unlock_production(session.clone(), presence(confirm), 5)
        .await
        .is_err());
    assert!(
        !app.db_session_state(session.clone())
            .await
            .unwrap()
            .writes_allowed
    );
    // The right one opens the window once.
    let t = token(session.clone()).await;
    let open = app
        .db_unlock_production(session.clone(), presence(t.clone()), 5)
        .await
        .unwrap();
    assert!(open.writes_allowed);
    app.db_lock_production(session.clone()).await.unwrap();
    assert_eq!(
        app.db_unlock_production(session.clone(), presence(t), 5)
            .await
            .unwrap_err()
            .code(),
        "passphrase_required"
    );
    app.db_disconnect(session).await.unwrap();
}
