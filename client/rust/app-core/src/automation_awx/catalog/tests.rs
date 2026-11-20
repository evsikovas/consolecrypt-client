use super::*;
use cc_monitoring_core::TlsConfig;
use serde_json::json;
use std::time::Duration;

const PASSWORD: &str = "Owned AWX catalog ephemeral password 42";
const MARKER: &str = "owned-awx-connection-plaintext-must-stay-encrypted";
async fn fixture() -> (tempfile::TempDir, AppCore, String) {
    let own = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        own.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("Owned AWX catalog".into(), PASSWORD.into())
        .await
        .unwrap();
    let credential = app
        .add_password_credential(
            "Owned AWX API token".into(),
            Some("owned-user".into()),
            "OWNED_AWX_TOKEN_NOT_IN_CONNECTION_JSON".into(),
        )
        .await
        .unwrap();
    (own, app, credential.id)
}
fn configuration(credential: &str) -> SourceConfig {
    SourceConfig {
        id: String::new(),
        name: MARKER.into(),
        endpoint: format!("https://{MARKER}.invalid/prefix"),
        route: Route::Direct,
        tls: TlsConfig::default(),
        auth: AuthRef::BearerRef {
            credential_id: credential.into(),
        },
    }
}
async fn raw(app: &AppCore) -> Option<Value> {
    app.unlocked()
        .await
        .unwrap()
        .0
        .storage()
        .unwrap()
        .setting_get(KEY)
        .await
        .unwrap()
}
async fn install(app: &AppCore, value: Value) {
    let (owner, _) = app.unlocked().await.unwrap();
    owner
        .storage()
        .unwrap()
        .write(move |tx| tx.setting_set(KEY, &value))
        .await
        .unwrap();
}

#[tokio::test]
async fn connection_catalog_uses_native_ids_exact_cas_and_separate_encrypted_credentials() {
    let (own, app, credential) = fixture().await;
    let scope = app.automation_awx_scope().await.unwrap();
    let empty = app.automation_awx_connections(&scope).await.unwrap();
    assert_eq!(empty.catalog_revision, Some(0));
    assert!(!empty.read_only);
    let created = app
        .automation_awx_save_connection(&scope, 0, None, configuration(&credential))
        .await
        .unwrap();
    assert_eq!(created.catalog_revision, Some(1));
    let row = &created.connections[0];
    canonical(&row.id).unwrap();
    assert_eq!(row.revision, Some(1));
    let mut edited = row.configuration.clone().unwrap();
    assert_eq!(edited.id, row.id);
    edited.name = "Owned renamed connection".into();
    assert!(app
        .automation_awx_save_connection(&scope, 0, Some(1), edited.clone())
        .await
        .is_err());
    let updated = app
        .automation_awx_save_connection(&scope, 1, Some(1), edited)
        .await
        .unwrap();
    assert_eq!(updated.connections[0].revision, Some(2));
    let persisted = raw(&app).await.unwrap();
    let bytes = serde_json::to_vec(&persisted).unwrap();
    assert!(!String::from_utf8(bytes)
        .unwrap()
        .contains("OWNED_AWX_TOKEN"));
    assert!(app
        .automation_awx_delete_connection(&scope, 2, row.id.clone(), 1)
        .await
        .is_err());
    let deleted = app
        .automation_awx_delete_connection(&scope, 2, row.id.clone(), 2)
        .await
        .unwrap();
    assert!(deleted.connections.is_empty());
    assert_eq!(deleted.catalog_revision, Some(3));
    assert!(
        app.get_credential(credential).await.is_ok(),
        "Deleting connection never deletes its user's credential"
    );
    app.shutdown().await.unwrap();
    fn check(path: &std::path::Path) {
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                check(&path)
            } else {
                let bytes = std::fs::read(path).unwrap();
                assert!(!bytes.windows(MARKER.len()).any(|w| w == MARKER.as_bytes()));
            }
        }
    }
    check(own.path());
}

#[tokio::test]
async fn original_scope_never_rebinds_to_same_profile_new_unlock_or_other_profile() {
    let (_own, app, credential) = fixture().await;
    let original = app.automation_awx_scope().await.unwrap();
    app.automation_awx_save_connection(&original, 0, None, configuration(&credential))
        .await
        .unwrap();
    app.lock().await.unwrap();
    assert!(app.automation_awx_connections(&original).await.is_err());
    app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    let reopened = app.automation_awx_scope().await.unwrap();
    assert_eq!(original.profile_id, reopened.profile_id);
    assert_ne!(
        original.notification_scope_id,
        reopened.notification_scope_id
    );
    assert!(app.automation_awx_connections(&original).await.is_err());
    assert!(app
        .automation_awx_save_connection(&original, 1, None, configuration(&credential))
        .await
        .is_err());
    assert_eq!(
        app.automation_awx_connections(&reopened)
            .await
            .unwrap()
            .connections
            .len(),
        1
    );
    app.create_local_profile("Owned second AWX profile".into(), PASSWORD.into())
        .await
        .unwrap();
    assert!(app.automation_awx_connections(&reopened).await.is_err());
    let fresh = app.automation_awx_scope().await.unwrap();
    assert!(app
        .automation_awx_connections(&fresh)
        .await
        .unwrap()
        .connections
        .is_empty());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn future_or_unknown_catalog_and_rows_remain_read_only_without_rewrite() {
    let (_own, app, credential) = fixture().await;
    let scope = app.automation_awx_scope().await.unwrap();
    app.automation_awx_save_connection(&scope, 0, None, configuration(&credential))
        .await
        .unwrap();
    let current = raw(&app).await.unwrap();
    for variant in 0..3 {
        let mut future = current.clone();
        match variant {
            0 => future["version"] = json!(2),
            1 => future["futureField"] = json!({"preserve":"owned-opaque"}),
            _ => {
                let id = future["connections"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .next()
                    .unwrap()
                    .clone();
                future["connections"][id]["version"] = json!(2);
            }
        }
        install(&app, future.clone()).await;
        let displayed = app.automation_awx_connections(&scope).await.unwrap();
        assert!(displayed.read_only);
        assert_eq!(displayed.catalog_revision, None);
        assert!(displayed.connections.is_empty());
        assert!(app
            .automation_awx_save_connection(&scope, 1, None, configuration(&credential))
            .await
            .is_err());
        assert_eq!(raw(&app).await.unwrap(), future);
    }
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn credential_kinds_names_and_routes_are_checked_without_secret_or_http_access() {
    let (_own, app, credential) = fixture().await;
    let scope = app.automation_awx_scope().await.unwrap();
    let missing_username = app
        .add_password_credential("Owned missing Basic user".into(), None, "owned".into())
        .await
        .unwrap();
    let mut basic = configuration(&credential);
    basic.auth = AuthRef::BasicRef {
        credential_id: missing_username.id,
    };
    assert!(app
        .automation_awx_save_connection(&scope, 0, None, basic)
        .await
        .is_err());
    let mut invalid = configuration(&credential);
    invalid.auth = AuthRef::BearerRef {
        credential_id: Uuid::new_v4().to_string(),
    };
    assert!(app
        .automation_awx_save_connection(&scope, 0, None, invalid)
        .await
        .is_err());
    for choice in 0..4 {
        let mut config = configuration(&credential);
        match choice {
            0 => {
                config.route = Route::SshTunnel {
                    route_id: Uuid::new_v4().to_string(),
                }
            }
            1 => config.endpoint = "http://127.0.0.1:1/must-not-connect".into(),
            2 => config.endpoint = "https://user:password@awx.invalid".into(),
            _ => config.name = "\n".into(),
        };
        assert!(app
            .automation_awx_save_connection(&scope, 0, None, config)
            .await
            .is_err());
    }
    assert!(raw(&app).await.is_none());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn strict_json_rpc_refuses_unknown_duplicate_future_and_oversize_inputs() {
    let (_own, app, _credential) = fixture().await;
    let scope = app.automation_awx_scope().await.unwrap();
    let request = json!({"kind":"list","version":1,"profileId":scope.profile_id,"notificationScopeId":scope.notification_scope_id});
    let result: Value = serde_json::from_str(
        &app.automation_awx_catalog_json(&request.to_string())
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(result["catalogRevision"], 0);
    let mut unknown = request.clone();
    unknown["approved"] = json!(true);
    assert!(app
        .automation_awx_catalog_json(&unknown.to_string())
        .await
        .is_err());
    let duplicate = request.to_string().replacen("{", "{\"version\":1,", 1);
    assert!(app.automation_awx_catalog_json(&duplicate).await.is_err());
    let mut future = request.clone();
    future["version"] = json!(2);
    assert!(app
        .automation_awx_catalog_json(&future.to_string())
        .await
        .is_err());
    let oversize = format!("{}{}", request, " ".repeat(MAX_REQUEST));
    assert!(app.automation_awx_catalog_json(&oversize).await.is_err());
    assert!(raw(&app).await.is_none());
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_catalog_write_rechecks_fresh_sql_row_and_preserves_future_metadata() {
    let (_own, app, credential) = fixture().await;
    let scope = app.automation_awx_scope().await.unwrap();
    let (owner, _) = app.unlocked().await.unwrap();
    let storage = owner.storage().unwrap().clone();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let future = json!({"version":99,"opaque":{"owned":"must-remain"}});
    let stored = future.clone();
    let blocker = tokio::spawn(async move {
        storage
            .write(move |tx| -> AppResult<()> {
                tx.setting_set(KEY, &stored)?;
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                Ok(())
            })
            .await
    });
    entered_rx.await.unwrap();
    let call = app.clone();
    let selected_scope = scope.clone();
    let config = configuration(&credential);
    let waiting = tokio::spawn(async move {
        call.automation_awx_save_connection(&selected_scope, 0, None, config)
            .await
    });
    release_tx.send(()).unwrap();
    blocker.await.unwrap().unwrap();
    assert!(waiting.await.unwrap().is_err());
    assert_eq!(raw(&app).await.unwrap(), future);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn successful_catalog_mutation_retires_captured_metadata_epoch_without_granting_access() {
    let (_own, app, credential) = fixture().await;
    let scope = app.automation_awx_scope().await.unwrap();
    let (_, u) = app.unlocked().await.unwrap();
    let mut before = u.awx_catalog.changes();
    let initial = *before.borrow_and_update();
    let created = app
        .automation_awx_save_connection(&scope, 0, None, configuration(&credential))
        .await
        .unwrap();
    assert!(before.has_changed().unwrap());
    assert!(*before.borrow_and_update() > initial);
    let row = &created.connections[0];
    app.automation_awx_delete_connection(&scope, 1, row.id.clone(), 1)
        .await
        .unwrap();
    assert!(before.has_changed().unwrap());
    assert_eq!(
        app.automation_awx_connections(&scope)
            .await
            .unwrap()
            .catalog_revision,
        Some(2)
    );
    drop(u);
    app.shutdown().await.unwrap();
}
