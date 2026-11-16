mod common;
use base64::{engine::general_purpose::STANDARD, Engine};
use cc_monitoring_core::{AuthRef, Route, SourceConfig, TlsConfig};
use common::*;
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::Notify;
use uuid::Uuid;

const PREFIX: &str = "/api/datasources/proxy/uid/owned-fixture";
const USER: &str = "owned-fixture-user";
const PASSWORD: &str = "synthetic-http-basic-password-4792";
fn encoded() -> String {
    STANDARD.encode(format!("{USER}:{PASSWORD}"))
}
fn response() -> Value {
    json!({"status":"success","data":{"resultType":"vector","result":[
    {"metric":{"job":"owned-fixture"},"value":[1,"1"]}]},
    "warnings":[format!("untrusted echo: Basic {} {USER}:{PASSWORD} {PASSWORD}",encoded())]})
}
fn source(endpoint: String, credential: String) -> SourceConfig {
    SourceConfig {
        id: String::new(),
        name: "Owned HTTP Basic fixture".into(),
        endpoint,
        route: Route::Direct,
        tls: TlsConfig::default(),
        auth: AuthRef::BasicRef {
            credential_id: credential,
        },
    }
}
fn request(id: &str, revision: u64) -> String {
    json!({"operationId":Uuid::new_v4(),"sourceId":id,"sourceRevision":revision,
    "action":{"kind":"query","expression":"up","time":null}})
    .to_string()
}
struct Gate {
    requests: AtomicUsize,
    entered: Notify,
    release: Notify,
    slow: bool,
}
async fn query(
    axum::extract::State(gate): axum::extract::State<Arc<Gate>>,
    headers: axum::http::HeaderMap,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
) -> axum::Json<Value> {
    assert_eq!(uri.path(), format!("{PREFIX}/api/v1/query"));
    assert!(
        headers.get("authorization").unwrap().to_str().unwrap() == format!("Basic {}", encoded()),
        "synthetic Basic header mismatch"
    );
    assert!(headers.get("proxy-authorization").is_none());
    gate.requests.fetch_add(1, Ordering::SeqCst);
    gate.entered.notify_one();
    if gate.slow {
        gate.release.notified().await;
    }
    axum::Json(response())
}
async fn fixture(slow: bool) -> (Arc<Gate>, String, tokio::task::JoinHandle<()>) {
    let gate = Arc::new(Gate {
        requests: AtomicUsize::new(0),
        entered: Notify::new(),
        release: Notify::new(),
        slow,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}{PREFIX}", listener.local_addr().unwrap());
    let router = axum::Router::new()
        .route(
            &format!("{PREFIX}/api/v1/query"),
            axum::routing::post(query),
        )
        .with_state(gate.clone());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (gate, endpoint, task)
}
fn no_material(value: &Value) {
    let serialized = value.to_string();
    for material in [PASSWORD.to_owned(), format!("{USER}:{PASSWORD}"), encoded()] {
        assert!(!serialized.contains(&material));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_basic_native_reference_survives_sql_restart_and_fixed_prefix_echo_is_redacted() {
    let temporary = tempfile::tempdir().unwrap();
    let store = memory_store();
    let first = app_with(temporary.path(), store.clone(), false);
    let profile = first
        .create_local_profile("Owned HTTP metrics".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let credential = first
        .add_password_credential(
            "Owned Basic credential".into(),
            Some(USER.into()),
            PASSWORD.into(),
        )
        .await
        .unwrap();
    let (gate, endpoint, server) = fixture(false).await;
    let catalog = first
        .monitoring_source_create(0, source(endpoint.clone(), credential.id.clone()))
        .await
        .unwrap();
    let source_id = catalog.sources[0].id.clone();
    no_material(&serde_json::to_value(&catalog).unwrap());
    first.shutdown().await.unwrap();
    let reopened = app_with(temporary.path(), store, false);
    reopened.open_profile(profile.profile.id).await.unwrap();
    reopened
        .unlock_with_passphrase(PASSPHRASE.into())
        .await
        .unwrap();
    let saved = reopened.monitoring_sources().await.unwrap();
    let config = saved.sources[0].configuration.as_ref().unwrap();
    assert_eq!(config.endpoint, endpoint);
    assert_eq!(
        config.auth,
        AuthRef::BasicRef {
            credential_id: credential.id
        }
    );
    no_material(&serde_json::to_value(&saved).unwrap());
    let result = reopened
        .monitoring_read(request(&source_id, 1))
        .await
        .unwrap();
    assert_eq!(result["result"]["status"], "success");
    assert!(result.to_string().contains("[REDACTED]"));
    no_material(&result);
    assert_eq!(gate.requests.load(Ordering::SeqCst), 1);
    reopened
        .create_local_profile("Another owned profile".into(), PASSPHRASE.into())
        .await
        .unwrap();
    assert!(reopened
        .monitoring_sources()
        .await
        .unwrap()
        .sources
        .is_empty());
    assert!(reopened
        .monitoring_read(request(&source_id, 1))
        .await
        .is_err());
    assert_eq!(gate.requests.load(Ordering::SeqCst), 1);
    server.abort();
    reopened.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_basic_password_username_delete_source_and_profile_mutations_discard_inflight_metrics()
{
    for mutation in [
        "password", "username", "delete", "source", "lock", "profile",
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let app = app(temporary.path());
        app.create_local_profile("Owned mutation fixture".into(), PASSPHRASE.into())
            .await
            .unwrap();
        let credential = app
            .add_password_credential(
                "Owned fixture credential".into(),
                Some(USER.into()),
                PASSWORD.into(),
            )
            .await
            .unwrap();
        let (gate, endpoint, server) = fixture(true).await;
        let catalog = app
            .monitoring_source_create(0, source(endpoint, credential.id.clone()))
            .await
            .unwrap();
        let source_id = catalog.sources[0].id.clone();
        let input = request(&source_id, 1);
        let reader = app.clone();
        let mut task = tokio::spawn(async move { reader.monitoring_read(input).await });
        tokio::select! {
            entered=tokio::time::timeout(Duration::from_secs(5),gate.entered.notified())=>entered.unwrap(),
            ended=&mut task=>panic!("read ended before owned fixture entry: {ended:?}"),
        }
        match mutation {
            "password" => {
                app.set_credential_password(
                    credential.id.clone(),
                    "replacement-synthetic-password".into(),
                )
                .await
                .unwrap();
            }
            "username" => {
                app.update_credential(
                    credential.id.clone(),
                    "Owned fixture credential".into(),
                    Some("changed-user".into()),
                    None,
                )
                .await
                .unwrap();
            }
            "delete" => {
                app.delete_credential(credential.id.clone()).await.unwrap();
            }
            "source" => {
                let mut next = catalog.sources[0].configuration.clone().unwrap();
                next.name = "Changed source".into();
                app.monitoring_source_update(source_id.clone(), 1, 1, next)
                    .await
                    .unwrap();
            }
            "lock" => {
                app.lock().await.unwrap();
            }
            "profile" => {
                app.create_local_profile("Different owned profile".into(), PASSPHRASE.into())
                    .await
                    .unwrap();
            }
            _ => unreachable!(),
        }
        gate.release.notify_one();
        let result = tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap();
        assert!(
            result.is_err(),
            "stale metrics were returned after {mutation}"
        );
        assert_eq!(gate.requests.load(Ordering::SeqCst), 1);
        if mutation == "delete" {
            assert!(app.monitoring_read(request(&source_id, 1)).await.is_err());
            assert_eq!(gate.requests.load(Ordering::SeqCst), 1);
        }
        server.abort();
        app.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn http_bearer_or_url_credentials_cannot_enter_native_catalog() {
    let temporary = tempfile::tempdir().unwrap();
    let app = app(temporary.path());
    app.create_local_profile("Owned invalid source fixture".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let credential = app
        .add_password_credential("Owned ref".into(), Some(USER.into()), PASSWORD.into())
        .await
        .unwrap();
    let mut configuration = source("http://127.0.0.1:1/prefix".into(), credential.id.clone());
    configuration.auth = AuthRef::BearerRef {
        credential_id: credential.id.clone(),
    };
    assert!(app
        .monitoring_source_create(0, configuration)
        .await
        .is_err());
    for endpoint in [
        "http://user:password@127.0.0.1:1/prefix",
        "http://127.0.0.1:1/prefix?token=secret",
        "http://127.0.0.1:1/prefix#secret",
    ] {
        assert!(app
            .monitoring_source_create(0, source(endpoint.into(), credential.id.clone()))
            .await
            .is_err());
    }
    assert!(app.monitoring_sources().await.unwrap().sources.is_empty());
    app.shutdown().await.unwrap();
}
