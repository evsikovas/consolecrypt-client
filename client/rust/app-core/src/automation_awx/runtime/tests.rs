use super::*;
use cc_monitoring_core::{AuthRef, Route, TlsConfig};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Notify,
};

const PASSWORD: &str = "Owned AWX runtime passphrase 42";
const TOKEN: &str = "synthetic-native-awx-token";

#[derive(Default)]
struct ServerState {
    requests: Mutex<Vec<(String, String, Value)>>,
    posts: AtomicUsize,
    block: AtomicBool,
    entered: Notify,
    release: Notify,
}
struct Fixture {
    _own: tempfile::TempDir,
    app: AppCore,
    scope: AwxScope,
    connection: String,
    state: Arc<ServerState>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Fixture {
    async fn new() -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let cert = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
        let ca = cert.cert.pem();
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.cert.der().clone()],
                rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der())
                    .into(),
            )
            .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("https://{}", listener.local_addr().unwrap());
        let state = Arc::new(ServerState::default());
        let server = state.clone();
        let task = tokio::spawn(async move {
            let mut requests = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    Some(result) = requests.join_next(), if !requests.is_empty() => { result.unwrap(); },
                    accepted = listener.accept() => {
                        let (socket, _) = accepted.unwrap();
                        let acceptor = acceptor.clone();
                        let server = server.clone();
                        requests.spawn(async move {
                            let Ok(mut socket) = acceptor.accept(socket).await else { return; };
                            let mut headers = Vec::new();
                            while headers.len() < 32768 && !headers.ends_with(b"\r\n\r\n") {
                                let mut byte = [0];
                                if socket.read_exact(&mut byte).await.is_err() { return; }
                                headers.push(byte[0]);
                            }
                            let headers = String::from_utf8(headers).unwrap();
                            assert!(headers.to_ascii_lowercase().contains(&format!("authorization: bearer {TOKEN}")));
                            let mut first = headers.lines().next().unwrap().split(' ');
                            let method = first.next().unwrap().to_owned();
                            let path = first.next().unwrap().to_owned();
                            let length = headers.lines().find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().unwrap())
                            }).unwrap_or(0);
                            assert!(length <= REQUEST_LIMIT);
                            let mut body = vec![0; length];
                            if socket.read_exact(&mut body).await.is_err() { return; }
                            let body = if length == 0 { Value::Null } else { serde_json::from_slice(&body).unwrap() };
                            server.requests.lock().unwrap().push((method.clone(), path.clone(), body));
                            if method == "POST" { server.posts.fetch_add(1, Ordering::SeqCst); }
                            if server.block.load(Ordering::SeqCst) {
                                server.entered.notify_one();
                                server.release.notified().await;
                            }
                            let template = json!({"id":7,"name":"Owned template","modified":"2026-11-12T15:00:00Z"});
                            let (status, value) = match (method.as_str(), path.split('?').next().unwrap()) {
                                ("GET", "/api/v2/job_templates/") => (200, json!({"next":null,"results":[template]})),
                                ("GET", "/api/v2/job_templates/7/") => (200, template),
                                ("GET", "/api/v2/job_templates/7/launch/") => (200, json!({
                                    "passwords_needed_to_start":[],"variables_needed_to_start":[],
                                    "credential_needed_to_start":false,"inventory_needed_to_start":false,
                                    "ask_job_type_on_launch":true,"ask_limit_on_launch":true,
                                    "ask_tags_on_launch":false,"ask_skip_tags_on_launch":false,"ask_variables_on_launch":false})),
                                ("POST", "/api/v2/job_templates/7/launch/") => (201, json!({"job":31,"id":31,"name":"Owned job","status":"pending","started":null,"finished":null,"ignored_fields":{}})),
                                _ => (404, json!({})),
                            };
                            let bytes = serde_json::to_vec(&value).unwrap();
                            let response = format!("HTTP/1.1 {status} Owned\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len());
                            let _ = socket.write_all(response.as_bytes()).await;
                            let _ = socket.write_all(&bytes).await;
                            let _ = socket.shutdown().await;
                        });
                    }
                }
            }
        });
        let own = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            own.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile("Owned AWX runtime".into(), PASSWORD.into())
            .await
            .unwrap();
        let credential = app
            .add_password_credential("Owned token".into(), None, TOKEN.into())
            .await
            .unwrap();
        let scope = app.automation_awx_scope().await.unwrap();
        let catalog = app
            .automation_awx_save_connection(
                &scope,
                0,
                None,
                SourceConfig {
                    id: String::new(),
                    name: "Owned TLS AWX".into(),
                    endpoint,
                    route: Route::Direct,
                    tls: TlsConfig {
                        custom_ca_pem: Some(ca),
                    },
                    auth: AuthRef::BearerRef {
                        credential_id: credential.id,
                    },
                },
            )
            .await
            .unwrap();
        Self {
            _own: own,
            app,
            scope,
            connection: catalog.connections[0].id.clone(),
            state,
            task,
        }
    }
    async fn register(&self, action: ReadAction) -> AwxNativeOperation {
        self.app
            .automation_awx_register_read(&self.scope, &self.connection, 1, action)
            .await
            .unwrap()
    }
    async fn prepare(&self) -> String {
        let operation = self
            .register(ReadAction::Prepare {
                template: 7,
                parameters: AwxLaunchParameters {
                    job_type: Some("check".into()),
                    limit: Some("owned-group".into()),
                    ..Default::default()
                },
            })
            .await;
        let result = self.app.automation_awx_start(&operation).await.unwrap();
        operation.joined().await;
        assert_eq!(
            result["data"]["parameters"],
            json!({"job_type":"check","limit":"owned-group"})
        );
        result["data"]["reviewTicket"].as_str().unwrap().to_owned()
    }
}

#[tokio::test]
async fn genuine_https_registration_has_no_http_and_cancel_before_start_denies_body() {
    let f = Fixture::new().await;
    let op = f
        .register(ReadAction::Templates {
            page: 1,
            search: String::new(),
        })
        .await;
    assert!(!op.id().is_nil());
    assert!(f.state.requests.lock().unwrap().is_empty());
    op.cancel();
    op.joined().await;
    assert!(f.app.automation_awx_start(&op).await.is_err());
    assert!(f.state.requests.lock().unwrap().is_empty());
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn actual_native_credentials_prepare_reads_and_exact_one_use_launch_only_posts_once() {
    let f = Fixture::new().await;
    let ticket = f.prepare().await;
    assert_eq!(f.state.posts.load(Ordering::SeqCst), 0);
    let launch = f
        .app
        .automation_awx_register_launch(&f.scope, &ticket)
        .await
        .unwrap();
    assert!(
        f.app
            .automation_awx_register_launch(&f.scope, &ticket)
            .await
            .is_err()
    );
    let result = f.app.automation_awx_start(&launch).await.unwrap();
    launch.joined().await;
    assert_eq!(result["data"]["status"], "started");
    assert_eq!(f.state.posts.load(Ordering::SeqCst), 1);
    assert!(f.app.automation_awx_start(&launch).await.is_err());
    let requests = f.state.requests.lock().unwrap();
    assert_eq!(
        requests.iter().find(|r| r.0 == "POST").unwrap().2,
        json!({"job_type":"check","limit":"owned-group"})
    );
    drop(requests);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn actual_lock_during_tls_read_revokes_original_handle_and_join_without_latest_scope() {
    let f = Fixture::new().await;
    f.state.block.store(true, Ordering::SeqCst);
    let op = Arc::new(
        f.register(ReadAction::Templates {
            page: 1,
            search: String::new(),
        })
        .await,
    );
    let app = f.app.clone();
    let running_op = op.clone();
    let running = tokio::spawn(async move { app.automation_awx_start(&running_op).await });
    tokio::time::timeout(Duration::from_secs(5), f.state.entered.notified())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), f.app.lock())
        .await
        .unwrap()
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(5), running)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    tokio::time::timeout(Duration::from_secs(5), op.joined())
        .await
        .unwrap();
    f.app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    assert!(f.app.automation_awx_start(&op).await.is_err());
    assert_eq!(f.state.posts.load(Ordering::SeqCst), 0);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn saved_connection_change_retires_real_review_before_any_launch_http() {
    let f = Fixture::new().await;
    let ticket = f.prepare().await;
    let catalog = f.app.automation_awx_connections(&f.scope).await.unwrap();
    let mut config = catalog.connections[0].configuration.clone().unwrap();
    config.name = "Explicitly changed connection".into();
    f.app
        .automation_awx_save_connection(&f.scope, 1, Some(1), config)
        .await
        .unwrap();
    assert!(
        f.app
            .automation_awx_register_launch(&f.scope, &ticket)
            .await
            .is_err()
    );
    assert_eq!(f.state.posts.load(Ordering::SeqCst), 0);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn actual_idle_lock_clears_body_before_joined_without_start_or_explicit_cancel() {
    let f = Fixture::new().await;
    let operation = f
        .register(ReadAction::Prepare {
            template: 7,
            parameters: AwxLaunchParameters::default(),
        })
        .await;
    assert!(operation.action.lock().unwrap().is_some());
    f.app.lock().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), operation.joined())
        .await
        .unwrap();
    assert!(operation.action.lock().unwrap().is_none());
    assert!(f.app.automation_awx_start(&operation).await.is_err());
    assert!(f.state.requests.lock().unwrap().is_empty());
    f.app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    assert!(f.app.automation_awx_start(&operation).await.is_err());
    f.app.shutdown().await.unwrap();
}
