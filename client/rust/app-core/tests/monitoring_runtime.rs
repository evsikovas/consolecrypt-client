mod common;
use cc_monitoring_core::{AuthRef, Route, SourceConfig, TlsConfig};
use common::{PASSPHRASE, app};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Notify,
};
use uuid::Uuid;

fn source(endpoint: String) -> SourceConfig {
    SourceConfig {
        id: String::new(),
        name: "Own monitoring fixture".into(),
        endpoint,
        route: Route::Direct,
        tls: TlsConfig::default(),
        auth: AuthRef::None,
    }
}
fn request(id: &str, revision: u64, operation: Uuid) -> String {
    json!({"operationId":operation,"sourceId":id,"sourceRevision":revision,
        "action":{"kind":"query","expression":"up","time":null}})
    .to_string()
}
fn response() -> Value {
    json!({"status":"success","data":{"resultType":"vector","result":[
        {"metric":{"job":"own-fixture"},"value":[1,"1"]}]}})
}

#[derive(Default)]
struct Gate {
    entered: Notify,
    release: Notify,
    requests: AtomicUsize,
}
async fn slow(axum::extract::State(gate): axum::extract::State<Arc<Gate>>) -> axum::Json<Value> {
    gate.requests.fetch_add(1, Ordering::SeqCst);
    gate.entered.notify_one();
    gate.release.notified().await;
    axum::Json(response())
}
async fn fixture() -> (Arc<Gate>, String, tokio::task::JoinHandle<()>) {
    let gate = Arc::new(Gate::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let router = axum::Router::new()
        .route("/api/v1/query", axum::routing::post(slow))
        .with_state(gate.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (gate, endpoint, task)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn saved_source_reads_cancel_on_lock_and_old_unlock_does_not_return_metrics() {
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    app.create_local_profile("Own metrics".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let (gate, endpoint, server) = fixture().await;
    let catalog = app
        .monitoring_source_create(0, source(endpoint))
        .await
        .unwrap();
    let id = catalog.sources[0].id.clone();
    let operation = Uuid::new_v4();
    let input = request(&id, 1, operation);
    let reader = app.clone();
    let mut task = tokio::spawn(async move { reader.monitoring_read(input).await });
    // An admission error is distinct from slow fixture startup. Report it
    // immediately instead of hiding the cause behind the HTTP-entry deadline.
    tokio::select! {
        entered = tokio::time::timeout(Duration::from_secs(5), gate.entered.notified()) => entered.unwrap(),
        result = &mut task => panic!("monitoring read ended before fixture HTTP entry: {result:?}"),
    }
    app.lock().await.unwrap();
    let error = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code(), "vault_locked");
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    app.monitoring_cancel(operation.to_string()).await.unwrap();
    assert!(
        app.monitoring_read(request(&id, 1, operation))
            .await
            .is_err()
    );
    assert_eq!(gate.requests.load(Ordering::SeqCst), 1);
    gate.release.notify_one();
    server.abort();
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_cas_changes_discard_inflight_data_and_explicit_cancel_is_private() {
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    app.create_local_profile("Own metrics".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let (gate, endpoint, server) = fixture().await;
    let catalog = app
        .monitoring_source_create(0, source(endpoint))
        .await
        .unwrap();
    let row = &catalog.sources[0];
    let id = row.id.clone();
    let operation = Uuid::new_v4();
    let input = request(&id, 1, operation);
    let reader = app.clone();
    let task = tokio::spawn(async move { reader.monitoring_read(input).await });
    tokio::time::timeout(Duration::from_secs(5), gate.entered.notified())
        .await
        .unwrap();
    let mut next = row.configuration.clone().unwrap();
    next.name = "New source generation".into();
    app.monitoring_source_update(id.clone(), 1, 1, next)
        .await
        .unwrap();
    gate.release.notify_one();
    assert!(task.await.unwrap().is_err());
    let operation = Uuid::new_v4();
    let input = request(&id, 2, operation);
    let reader = app.clone();
    let task = tokio::spawn(async move { reader.monitoring_read(input).await });
    tokio::time::timeout(Duration::from_secs(5), gate.entered.notified())
        .await
        .unwrap();
    app.monitoring_cancel(operation.to_string()).await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    gate.release.notify_one();
    server.abort();
    app.shutdown().await.unwrap();
}

/// A real TLS request exercises native credential resolution and prevents an
/// Authorization echo in backend warnings from reaching Flutter JSON.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bearer_credential_is_resolved_natively_tls_verified_and_echo_redacted() {
    const TOKEN: &str = "synthetic-native-monitoring-token-4792";
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    app.create_local_profile("Own TLS metrics".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let credential = app
        .add_password_credential("Own token".into(), None, TOKEN.into())
        .await
        .unwrap();
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let pem = cert.cert.pem();
    let server_config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![cert.cert.der().clone()],
        rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()).into(),
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server_config));
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut stream = acceptor.accept(socket).await.unwrap();
        let mut bytes = Vec::new();
        let mut chunk = [0; 1024];
        while !bytes.windows(4).any(|v| v == b"\r\n\r\n") {
            let count = stream.read(&mut chunk).await.unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&chunk[..count]);
            assert!(bytes.len() < 16 * 1024);
        }
        let headers = String::from_utf8(bytes).unwrap();
        assert!(headers.contains(&format!("Bearer {TOKEN}")));
        let mut body = response();
        body["warnings"] = json!([format!("Untrusted echo: {TOKEN}")]);
        let body = body.to_string();
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).as_bytes()).await.unwrap();
        stream.shutdown().await.unwrap();
    });
    let mut configuration = source(format!("https://localhost:{port}"));
    configuration.tls.custom_ca_pem = Some(pem);
    configuration.auth = AuthRef::BearerRef {
        credential_id: credential.id,
    };
    let created = app
        .monitoring_source_create(0, configuration)
        .await
        .unwrap();
    let result = app
        .monitoring_read(request(&created.sources[0].id, 1, Uuid::new_v4()))
        .await
        .unwrap();
    assert_eq!(result["result"]["status"], "success");
    assert!(result.to_string().contains("[REDACTED]"));
    assert!(!result.to_string().contains(TOKEN));
    server.await.unwrap();
    app.shutdown().await.unwrap();
}
