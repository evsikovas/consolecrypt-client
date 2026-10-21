use super::*;
use crate::AppConfig;
use cc_models::kubernetes::{KubeAuth, KubeContext};
use cc_models::secret::{Secret, SecretKind, SecretValue};
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_rustls::TlsAcceptor;

mod ai_revocation;
mod ai_synced_history;
mod db_route;
mod insights_watch;
mod response_revocation;
mod view_preferences;

struct Fixture {
    address: std::net::SocketAddr,
    certificate: String,
    requests: Arc<Mutex<Vec<String>>>,
    reached_version: CancellationToken,
    release_version: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
        self.release_version.cancel();
    }
}
fn pod() -> Value {
    json!({"apiVersion":"v1","kind":"Pod","metadata":{"name":"web","namespace":"default","uid":"pod-uid","resourceVersion":"1"},
        "spec":{"containers":[{"name":"web","image":"example.invalid/web:1"}]},"status":{"phase":"Running"}})
}
impl Fixture {
    async fn start(paused: bool) -> Self {
        Self::start_options(paused, false).await
    }
    async fn start_gitops() -> Self {
        Self::start_options(false, true).await
    }
    async fn start_options(paused: bool, gitops: bool) -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let certificate =
            rcgen::generate_simple_self_signed(vec!["kube.runtime.test".into()]).unwrap();
        let pem = certificate.cert.pem();
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![certificate.cert.der().clone()],
                rustls::pki_types::PrivatePkcs8KeyDer::from(
                    certificate.signing_key.serialize_der(),
                )
                .into(),
            )
            .unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        let reached_version = CancellationToken::new();
        let release_version = CancellationToken::new();
        if !paused {
            release_version.cancel();
        }
        let reached = reached_version.clone();
        let release = release_version.clone();
        let task = tokio::spawn(async move {
            let mut children = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    Some(_) = children.join_next(), if !children.is_empty() => {},
                    accepted = listener.accept() => {
                        let Ok((socket, _)) = accepted else { break; };
                        let acceptor = acceptor.clone();
                        let seen = seen.clone();
                        let reached = reached.clone();
                        let release = release.clone();
                        children.spawn(async move {
                            let Ok(mut stream) = acceptor.accept(socket).await else { return; };
                            let mut header = Vec::new();
                            let mut byte = [0];
                            while header.len() < 32768 && !header.ends_with(b"\r\n\r\n") {
                                if stream.read_exact(&mut byte).await.is_err() { return; }
                                header.push(byte[0]);
                            }
                            let header = String::from_utf8(header).unwrap();
                            assert!(header.to_ascii_lowercase().contains("authorization: bearer synthetic-runtime-token"));
                            let line = header.lines().next().unwrap().to_owned();
                            seen.lock().unwrap().push(line.clone());
                            let path = line.split(' ').nth(1).unwrap();
                            if header.lines().any(|line| line.eq_ignore_ascii_case("upgrade: websocket")) {
                                let key = header.lines().filter_map(|line| line.split_once(':'))
                                    .find(|(key,_)| key.eq_ignore_ascii_case("sec-websocket-key")).unwrap().1.trim();
                                let accept = tokio_tungstenite::tungstenite::handshake::derive_accept_key(key.as_bytes());
                                let reply = format!("HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: {accept}\r\nSec-WebSocket-Protocol: v4.channel.k8s.io\r\n\r\n");
                                if stream.write_all(reply.as_bytes()).await.is_err() { return; }
                                use tokio_tungstenite::tungstenite::{protocol::Role, Message};
                                let mut stream = tokio_tungstenite::WebSocketStream::from_raw_socket(stream, Role::Server, None).await;
                                if path.contains("/portforward?") {
                                    let _ = stream.send(Message::Binary(vec![0,80,0].into())).await;
                                    let _ = stream.send(Message::Binary(vec![1,80,0].into())).await;
                                }
                                while let Some(Ok(Message::Binary(bytes))) = stream.next().await {
                                    if bytes.first() == Some(&0) {
                                        let mut echoed = vec![if path.contains("/portforward?") { 0 } else { 1 }];
                                        echoed.extend_from_slice(&bytes[1..]);
                                        if stream.send(Message::Binary(echoed.into())).await.is_err() { break; }
                                    }
                                }
                                return;
                            }
                            if path == "/version" {
                                reached.cancel();
                                release.cancelled().await;
                            }
                            let mut status = "200 OK";
                            let body = if path == "/version" {
                                json!({"gitVersion":"v1.32.0"})
                            } else if path == "/apis" {
                                if gitops { json!({"groups":[
                                    {"name":"source.toolkit.fluxcd.io","preferredVersion":{"version":"v1"},
                                        "versions":[{"version":"v1beta2"},{"version":"v1"}]},
                                    {"name":"helm.toolkit.fluxcd.io","versions":[{"version":"v2"}]}
                                ]}) } else { json!({"groups":[]}) }
                            } else if gitops && ["/apis/source.toolkit.fluxcd.io/v1", "/apis/source.toolkit.fluxcd.io/v1beta2"].contains(&path) {
                                json!({"resources":[{"name":"gitrepositories","kind":"GitRepository","namespaced":true,"verbs":["get","list","watch"]}]})
                            } else if gitops && path == "/apis/helm.toolkit.fluxcd.io/v2" {
                                json!({"resources":[{"name":"helmreleases","kind":"HelmRelease","namespaced":true,"verbs":["get","list","watch"]}]})
                            } else if gitops && path.split('?').next().unwrap().ends_with("/helmreleases") {
                                status = "403 Forbidden";
                                json!({"message":"synthetic-private-rbac-error"})
                            } else if gitops && path.split('?').next().unwrap().ends_with("/gitrepositories") {
                                if path.contains("/namespaces/blocked/") {
                                    status = "403 Forbidden";
                                    json!({"message":"synthetic-private-rbac-error"})
                                } else {
                                let source = json!({"apiVersion":"source.toolkit.fluxcd.io/v1","kind":"GitRepository",
                                    "metadata":{"name":"private-source","namespace":"default","uid":"source-id","resourceVersion":"2"},
                                    "spec":{"url":"https://synthetic-private-source.invalid/repo"}});
                                if path.contains("watch=true") { json!({"type":"MODIFIED","object":source}) }
                                else { json!({"metadata":{"resourceVersion":"1"},"items":[source]}) }
                                }
                            } else if path == "/api/v1" {
                                json!({"resources":[{"name":"pods","kind":"Pod","namespaced":true,"verbs":["get","list","watch","patch","delete"]},
                                    {"name":"events","kind":"Event","namespaced":true,"verbs":["get","list","watch"]},
                                    {"name":"namespaces","kind":"Namespace","namespaced":false,"verbs":["list"]}]})
                            } else if path.split('?').next().unwrap().ends_with("/events") {
                                let event = json!({"apiVersion":"v1","kind":"Event","metadata":{"uid":"event-uid","name":"web.event","namespace":"default","resourceVersion":"2"},
                                    "type":"Warning","reason":"BackOff","message":"password=synthetic-event-secret","count":3,
                                    "involvedObject":{"uid":"pod-uid","name":"web","kind":"Pod","namespace":"default"},
                                    "firstTimestamp":chrono::Utc::now().to_rfc3339(),"lastTimestamp":chrono::Utc::now().to_rfc3339()});
                                if path.contains("watch=true") { json!({"type":"MODIFIED","object":event}) }
                                else { json!({"metadata":{"resourceVersion":"2"},"items":[event]}) }
                            } else if line.starts_with("PATCH ") || line.starts_with("DELETE ") || path.split('?').next().unwrap().ends_with("/pods/web") {
                                pod()
                            } else {
                                json!({"metadata":{"resourceVersion":"1"},"items":[pod()]})
                            }.to_string();
                            let length = header.lines().filter_map(|line| line.split_once(':'))
                                .find(|(key,_)| key.eq_ignore_ascii_case("content-length"))
                                .and_then(|(_,value)| value.trim().parse::<usize>().ok()).unwrap_or(0);
                            if length > 2*1024*1024 { return; }
                            let mut request_body = vec![0;length];
                            if stream.read_exact(&mut request_body).await.is_err() { return; }
                            let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                            let _ = stream.write_all(response.as_bytes()).await;
                            let _ = stream.shutdown().await;
                        });
                    }
                }
            }
        });
        Self {
            address,
            certificate: pem,
            requests,
            reached_version,
            release_version,
            task,
        }
    }
    async fn app(&self) -> (tempfile::TempDir, AppCore, KubeContext) {
        let dir = tempfile::tempdir().unwrap();
        let app = AppCore::new(AppConfig::for_tests(
            dir.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile("runtime".into(), "synthetic-runtime-test-passphrase".into())
            .await
            .unwrap();
        let mut cluster = KubeCluster::new("test", format!("https://{}", self.address));
        cluster.tls_server_name = Some("kube.runtime.test".into());
        cluster.certificate_authority_pem = Some(self.certificate.clone());
        let secret = Secret::new(
            SecretKind::Token,
            SecretValue::new("synthetic-runtime-token"),
        );
        let mut context = KubeContext::new("production test", cluster.id);
        context.production = true;
        context.auth = KubeAuth::Token {
            secret_id: secret.id,
        };
        app.import_kube_connection(cluster, context.clone(), vec![secret])
            .await
            .unwrap();
        (dir, app, context)
    }
    fn writes(&self) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|line| line.starts_with("PATCH ") || line.starts_with("DELETE "))
            .count()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn production_guard_audits_before_wire_and_closed_storage_blocks_writes() {
    let fixture = Fixture::start(false).await;
    let (_dir, app, context) = fixture.app().await;
    let session = app.kube_connect(context.id.to_string()).await.unwrap();
    let discovery = app.kube_discover(session.id.clone()).await.unwrap();
    let kind = discovery
        .resources
        .into_iter()
        .find(|k| k.resource == "pods")
        .unwrap();
    let reference = ResourceRef {
        api: kind,
        name: "web".into(),
        namespace: Some("default".into()),
    };
    assert!(app
        .kube_resource_delete(
            session.id.clone(),
            reference.clone(),
            "pod-uid".into(),
            "1".into()
        )
        .await
        .is_err());
    assert_eq!(fixture.writes(), 0);
    assert!(app
        .kube_open_write_window(session.id.clone(), "wrong".into())
        .await
        .is_err());
    app.kube_open_write_window(session.id.clone(), context.name.clone())
        .await
        .unwrap();
    app.kube_resource_delete(
        session.id.clone(),
        reference.clone(),
        "pod-uid".into(),
        "1".into(),
    )
    .await
    .unwrap();
    assert_eq!(fixture.writes(), 1);
    let audit = app.kube_audit().await.unwrap();
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].method, "DELETE");
    assert_eq!(audit[0].path, "/api/v1/namespaces/default/pods/web");
    let (owner, _) = app.unlocked().await.unwrap();
    owner.storage().unwrap().close().await;
    assert!(app
        .kube_resource_delete(session.id, reference, "pod-uid".into(), "1".into())
        .await
        .is_err());
    assert_eq!(
        fixture.writes(),
        1,
        "an uncommitted audit must never dispatch"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn credential_and_connection_changes_invalidate_live_handles() {
    let fixture = Fixture::start(false).await;
    let (_dir, app, context) = fixture.app().await;
    let session = app.kube_connect(context.id.to_string()).await.unwrap();
    app.set_kube_token(context.id, SecretValue::new("synthetic-runtime-token"))
        .await
        .unwrap();
    assert!(app.kube_discover(session.id).await.is_err());
    let session = app.kube_connect(context.id.to_string()).await.unwrap();
    let mut cluster = app.list_kube_clusters().await.unwrap().remove(0);
    cluster.name = "changed while connected".into();
    app.save_kube_cluster(cluster).await.unwrap();
    assert!(app.kube_discover(session.id).await.is_err());
    let session = app.kube_connect(context.id.to_string()).await.unwrap();
    app.delete_kube_context(context.id.to_string())
        .await
        .unwrap();
    assert!(app.kube_discover(session.id).await.is_err());
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lock_cancels_pending_connect_without_returning_a_late_handle() {
    let fixture = Fixture::start(true).await;
    let (_dir, app, context) = fixture.app().await;
    let app = Arc::new(app);
    let connecting = {
        let app = app.clone();
        tokio::spawn(async move { app.kube_connect(context.id.to_string()).await })
    };
    tokio::time::timeout(Duration::from_secs(5), fixture.reached_version.cancelled())
        .await
        .unwrap();
    app.lock().await.unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(2), connecting)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    fixture.release_version.cancel();
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_tls_never_sends_credentials_and_disconnect_releases_slots() {
    let fixture = Fixture::start(false).await;
    let (_dir, app, context) = fixture.app().await;
    let mut cluster = app.list_kube_clusters().await.unwrap().remove(0);
    cluster.certificate_authority_pem = None;
    app.save_kube_cluster(cluster.clone()).await.unwrap();
    assert!(app.kube_connect(context.id.to_string()).await.is_err());
    assert!(fixture.requests.lock().unwrap().is_empty());
    cluster.certificate_authority_pem = Some(fixture.certificate.clone());
    app.save_kube_cluster(cluster).await.unwrap();
    for _ in 0..MAX_SESSIONS + 1 {
        let session = app.kube_connect(context.id.to_string()).await.unwrap();
        app.kube_disconnect(session.id).await.unwrap();
    }
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pod_shell_uses_normal_terminal_input_output_and_lock_lifecycle() {
    let fixture = Fixture::start(false).await;
    let (_dir, app, mut context) = fixture.app().await;
    context.allow_exec_in_prod = true;
    app.save_kube_context(context.clone()).await.unwrap();
    let session = app.kube_connect(context.id.to_string()).await.unwrap();
    app.kube_discover(session.id.clone()).await.unwrap();
    let terminal = app
        .kube_terminal_open(
            session.id,
            cc_k8s_core::resources::PodTerminalRequest {
                namespace: "default".into(),
                pod: "web".into(),
                container: Some("web".into()),
                command: Some(vec!["/bin/sh".into()]),
                tty: true,
            },
            120,
            32,
        )
        .await
        .unwrap();
    assert!(terminal.host_id.is_none());
    assert!(matches!(
        terminal.target,
        cc_terminal_core::TerminalTarget::Kubernetes { .. }
    ));
    let mut attachment = app.attach_terminal(terminal.id.clone()).await.unwrap();
    app.terminal_write(terminal.id.clone(), b"echo synthetic\n".to_vec())
        .await
        .unwrap();
    let chunk = tokio::time::timeout(Duration::from_secs(3), attachment.output.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(chunk, crate::dto::TerminalChunk::Data(bytes) if bytes == b"echo synthetic\n")
    );
    app.terminal_resize(terminal.id.clone(), 90, 25)
        .await
        .unwrap();
    app.lock().await.unwrap();
    let mut closed = false;
    while let Some(chunk) = tokio::time::timeout(Duration::from_secs(3), attachment.output.recv())
        .await
        .unwrap()
    {
        if matches!(chunk, crate::dto::TerminalChunk::Closed(_)) {
            closed = true;
            break;
        }
    }
    assert!(closed);
    assert!(app
        .terminal_write(terminal.id, b"must not send".to_vec())
        .await
        .is_err());
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_forward_is_listed_and_disconnect_closes_its_listener() {
    let fixture = Fixture::start(false).await;
    let (_dir, app, mut context) = fixture.app().await;
    context.allow_port_forward_in_prod = true;
    app.save_kube_context(context.clone()).await.unwrap();
    let session = app.kube_connect(context.id.to_string()).await.unwrap();
    app.kube_discover(session.id.clone()).await.unwrap();
    let forward = app
        .kube_forward_start(session.id.clone(), "default".into(), "web".into(), 80, 0)
        .await
        .unwrap();
    assert!(forward.address.starts_with("127.0.0.1:"));
    assert_eq!(app.kube_forward_list().await.unwrap().len(), 1);
    let mut connection = tokio::net::TcpStream::connect(&forward.address)
        .await
        .unwrap();
    connection.write_all(b"hello").await.unwrap();
    let mut bytes = [0; 5];
    tokio::time::timeout(Duration::from_secs(3), connection.read_exact(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&bytes, b"hello");
    app.kube_disconnect(session.id).await.unwrap();
    assert!(app.kube_forward_list().await.unwrap().is_empty());
    let mut byte = [0];
    let closed = tokio::time::timeout(Duration::from_secs(3), connection.read(&mut byte))
        .await
        .unwrap();
    assert!(matches!(closed, Ok(0) | Err(_)));
    assert!(tokio::net::TcpStream::connect(&forward.address)
        .await
        .is_err());
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_cli_proxy_pins_tls_replaces_identity_and_enforces_production_policy() {
    use base64::Engine;
    use cc_k8s_core::local_proxy::LocalApiProxy;
    let fixture = Fixture::start(false).await;
    let (_dir, app, context) = fixture.app().await;
    let session = app.kube_connect(context.id.to_string()).await.unwrap();
    let live = app.kube_live(&session.id).await.unwrap();
    let proxy = LocalApiProxy::start(&live.session).await.unwrap();
    let config = proxy.kubeconfig("default").unwrap();
    assert!(!String::from_utf8_lossy(&config).contains("synthetic-runtime-token"));
    let config: Value = serde_json::from_slice(&config).unwrap();
    let token = config["users"][0]["user"]["token"].as_str().unwrap();
    let ca = base64::engine::general_purpose::STANDARD
        .decode(
            config["clusters"][0]["cluster"]["certificate-authority-data"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
    let client = reqwest::Client::builder()
        .no_proxy()
        .add_root_certificate(reqwest::Certificate::from_pem(&ca).unwrap())
        .build()
        .unwrap();
    let base = format!("https://{}", proxy.address());
    assert!(reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("{base}/version"))
        .send()
        .await
        .is_err());
    let baseline = fixture.requests.lock().unwrap().len();
    let denied = client
        .get(format!("{base}/version"))
        .bearer_auth("wrong")
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status().as_u16(), 401);
    let denied = client
        .get(format!("{base}/version"))
        .bearer_auth(token)
        .header("origin", "https://example.invalid")
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status().as_u16(), 400);
    assert_eq!(fixture.requests.lock().unwrap().len(), baseline);
    let response = client
        .get(format!("{base}/version"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    assert!(response.text().await.unwrap().contains("v1.32.0"));
    let denied = client
        .delete(format!("{base}/api/v1/namespaces/default/pods/web"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status().as_u16(), 403);
    assert_eq!(fixture.writes(), 0);
    app.kube_open_write_window(session.id.clone(), context.name)
        .await
        .unwrap();
    let applied = client
        .delete(format!("{base}/api/v1/namespaces/default/pods/web"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert_eq!(applied.status().as_u16(), 200);
    assert_eq!(fixture.writes(), 1);
    app.kube_disconnect(session.id).await.unwrap();
    assert!(client
        .get(format!("{base}/version"))
        .bearer_auth(token)
        .send()
        .await
        .is_err());
    assert!(proxy.kubeconfig("default").is_err());
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_capability_files_are_private_and_removed_on_lock_and_drop() {
    let fixture = Fixture::start(false).await;
    let (_dir, app, context) = fixture.app().await;
    let session = app.kube_connect(context.id.to_string()).await.unwrap();
    let lease = app.kube_cli_proxy(session.id.clone()).await.unwrap();
    let path = lease.path();
    assert!(path.is_file());
    let bytes = std::fs::read(&path).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("synthetic-runtime-token"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    drop(lease);
    assert!(!path.exists());
    let lease = app.kube_cli_proxy(session.id).await.unwrap();
    let path = lease.path();
    app.lock().await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while path.exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(lease.cancellation().is_cancelled());
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exports_are_explicit_private_non_overwriting_and_inventory_edit_preserves_auth() {
    let fixture = Fixture::start(false).await;
    let (dir, app, context) = fixture.app().await;
    let redacted = dir.path().join("redacted.yaml");
    app.kube_export_context(
        context.id.to_string(),
        redacted.to_string_lossy().into(),
        false,
    )
    .await
    .unwrap();
    let bytes = std::fs::read(&redacted).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("synthetic-runtime-token"));
    let full = dir.path().join("full-kubeconfig.yaml");
    app.kube_export_context(context.id.to_string(), full.to_string_lossy().into(), true)
        .await
        .unwrap();
    let bytes = std::fs::read(&full).unwrap();
    let parsed =
        cc_k8s_core::import::parse(std::str::from_utf8(&bytes).unwrap(), Some(&full)).unwrap();
    assert_eq!(parsed.entries.len(), 1);
    assert!(String::from_utf8_lossy(&bytes).contains("synthetic-runtime-token"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&full).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert!(app
        .kube_export_context(context.id.to_string(), full.to_string_lossy().into(), false)
        .await
        .is_err());
    assert_eq!(std::fs::read(&full).unwrap(), bytes);
    let found = app
        .kube_scan_config_files(dir.path().to_string_lossy().into())
        .await
        .unwrap();
    assert!(found.contains(
        &dunce::canonicalize(&full)
            .unwrap()
            .to_string_lossy()
            .into_owned()
    ));
    let edited = app
        .kube_context_update(
            context.id.to_string(),
            crate::kubernetes_inventory::KubeContextUpdate {
                name: "renamed".into(),
                namespace: "other".into(),
                production: true,
                allow_exec_in_prod: true,
                allow_port_forward_in_prod: false,
                record_events: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(edited.auth, context.auth);
    assert_eq!(edited.cluster_id, context.cluster_id);
    assert!(edited.event_history_enabled);
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_proxy_relays_websockets_and_revokes_live_shell_on_disconnect() {
    use base64::Engine;
    use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
    let fixture = Fixture::start(false).await;
    let (_dir, app, mut context) = fixture.app().await;
    context.allow_exec_in_prod = true;
    app.save_kube_context(context.clone()).await.unwrap();
    let session = app.kube_connect(context.id.to_string()).await.unwrap();
    let lease = app.kube_cli_proxy(session.id.clone()).await.unwrap();
    let config: Value = serde_json::from_slice(&std::fs::read(lease.path()).unwrap()).unwrap();
    let server = config["clusters"][0]["cluster"]["server"].as_str().unwrap();
    let url = url::Url::parse(server).unwrap();
    let address = format!("127.0.0.1:{}", url.port().unwrap());
    let ca = base64::engine::general_purpose::STANDARD
        .decode(
            config["clusters"][0]["cluster"]["certificate-authority-data"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(rustls::pki_types::CertificateDer::from(
            pem::parse(ca).unwrap().into_contents(),
        ))
        .unwrap();
    let tls = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let tcp = tokio::net::TcpStream::connect(address).await.unwrap();
    let stream = tokio_rustls::TlsConnector::from(Arc::new(tls))
        .connect(
            rustls::pki_types::ServerName::try_from("127.0.0.1").unwrap(),
            tcp,
        )
        .await
        .unwrap();
    let mut request = format!("wss://127.0.0.1:{}/api/v1/namespaces/default/pods/web/exec?stdin=true&stdout=true&command=sh", url.port().unwrap()).into_client_request().unwrap();
    request.headers_mut().insert(
        "authorization",
        format!(
            "Bearer {}",
            config["users"][0]["user"]["token"].as_str().unwrap()
        )
        .parse()
        .unwrap(),
    );
    request.headers_mut().insert(
        "sec-websocket-protocol",
        "v4.channel.k8s.io".parse().unwrap(),
    );
    let (mut socket, _) = tokio_tungstenite::client_async(request, stream)
        .await
        .unwrap();
    socket
        .send(Message::Binary(vec![0, b'o', b'k'].into()))
        .await
        .unwrap();
    let output = tokio::time::timeout(Duration::from_secs(3), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(output.into_data(), vec![1, b'o', b'k']);
    app.kube_disconnect(session.id).await.unwrap();
    let next = tokio::time::timeout(Duration::from_secs(3), socket.next())
        .await
        .unwrap();
    assert!(
        next.is_none()
            || next.is_some_and(|item| item.is_err() || matches!(item, Ok(Message::Close(_))))
    );
    app.shutdown().await.unwrap();
}

async fn ai_provider(app: &AppCore, base: &str) -> cc_models::ai::AiProviderConfig {
    let now = chrono::Utc::now();
    let provider = cc_models::ai::AiProviderConfig {
        id: ObjectId::new(),
        name: "fixture model".into(),
        provider: cc_models::ai::AiProviderKind::OpenaiCompatible,
        base_url: base.into(),
        api_key_secret_id: None,
        chat_model: "fixture".into(),
        embedding_model: None,
        timeout_secs: 10,
        streaming: false,
        tool_support: true,
        privacy_profile: cc_models::ai::PrivacyProfile::Strict,
        is_default: true,
        created_at: now,
        updated_at: now,
    };
    app.unlocked()
        .await
        .unwrap()
        .1
        .writer
        .put(VaultObject::AiProvider(provider.clone()))
        .await
        .unwrap();
    provider
}
fn ai_question() -> crate::kubernetes_ai::KubeAiQuestion {
    crate::kubernetes_ai::KubeAiQuestion {
        provider_id: None,
        question: "Why is web running? password=synthetic-hidden-value".into(),
        selected: None,
        config_map_values: false,
        conversation_id: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_external_consent_is_single_use_bound_to_provider_and_revocable() {
    let fixture = Fixture::start(false).await;
    let (_dir, app, context) = fixture.app().await;
    let provider = ai_provider(&app, "https://model.example.invalid/v1").await;
    let session = app.kube_connect(context.id.to_string()).await.unwrap();
    let baseline = fixture.requests.lock().unwrap().len();
    let review = app
        .kube_ai_prepare(session.id.clone(), ai_question())
        .await
        .unwrap();
    assert!(review.external);
    assert!(app
        .kube_ai_start(session.id.clone(), review.ticket.clone(), false)
        .await
        .is_err());
    assert!(app
        .kube_ai_start(session.id.clone(), review.ticket, true)
        .await
        .is_err());
    assert_eq!(
        baseline,
        fixture.requests.lock().unwrap().len(),
        "no cluster data is read before consent"
    );
    let review = app
        .kube_ai_prepare(session.id.clone(), ai_question())
        .await
        .unwrap();
    let mut changed = provider;
    changed.base_url = "https://another.example.invalid/v1".into();
    app.unlocked()
        .await
        .unwrap()
        .1
        .writer
        .put(VaultObject::AiProvider(changed))
        .await
        .unwrap();
    assert!(app
        .kube_ai_start(session.id.clone(), review.ticket, true)
        .await
        .is_err());
    let review = app
        .kube_ai_prepare(session.id.clone(), ai_question())
        .await
        .unwrap();
    app.kube_ai_revoke(session.id.clone()).await.unwrap();
    assert!(app
        .kube_ai_start(session.id.clone(), review.ticket, true)
        .await
        .is_err());
    app.lock().await.unwrap();
    assert!(app
        .kube_ai_prepare(session.id, ai_question())
        .await
        .is_err());
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_read_tools_are_visible_sanitized_and_never_write() {
    let fixture = Fixture::start(false).await;
    let (_dir, app, context) = fixture.app().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let received = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured = received.clone();
    let mock = tokio::spawn(async move {
        for index in 0..3 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut header = Vec::new();
            let mut byte = [0];
            while !header.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).await.unwrap();
                header.push(byte[0]);
                assert!(header.len() < 32768);
            }
            let header = String::from_utf8(header).unwrap();
            let len = header
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .unwrap()
                .1
                .trim()
                .parse::<usize>()
                .unwrap();
            assert!(len < 512 * 1024);
            let mut body = vec![0; len];
            socket.read_exact(&mut body).await.unwrap();
            captured
                .lock()
                .unwrap()
                .push(serde_json::from_slice(&body).unwrap());
            let message = if index == 0 {
                json!({"role":"assistant","content":"","tool_calls":[{
                    "id":"read-pods","type":"function","function":{"name":"list_resources","arguments":
                    "{\"resource\":{\"group\":\"\",\"version\":\"v1\",\"resource\":\"pods\"},\"max_rows\":10}"}
                }]})
            } else {
                json!({"role":"assistant","content":"The selected Pod is running."})
            };
            let response = json!({"id":"fixture","object":"chat.completion","created":0,"model":"fixture",
                "choices":[{"index":0,"message":message,"finish_reason":if index == 0 {"tool_calls"} else {"stop"}}]}).to_string();
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",response.len()).as_bytes()).await.unwrap();
        }
    });
    ai_provider(&app, &base).await;
    let session = app.kube_connect(context.id.to_string()).await.unwrap();
    app.kube_discover(session.id.clone()).await.unwrap();
    let review = app
        .kube_ai_prepare(session.id.clone(), ai_question())
        .await
        .unwrap();
    assert!(!review.external);
    let request = app
        .kube_ai_start(session.id.clone(), review.ticket, false)
        .await
        .unwrap();
    let mut events = Vec::new();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let poll = app
                .kube_ai_poll(session.id.clone(), request.clone())
                .await
                .unwrap();
            events.extend(poll.events);
            if poll.closed {
                break;
            }
        }
    })
    .await
    .unwrap();
    assert!(events.iter().any(|event| matches!(event, crate::kubernetes_ai::KubeAiEvent::Context {tool,..} if tool == "list_resources")));
    assert!(events.iter().any(|event| matches!(event, crate::kubernetes_ai::KubeAiEvent::Answer {content,..} if content.contains("running"))));
    assert!(!events
        .iter()
        .any(|event| matches!(event, crate::kubernetes_ai::KubeAiEvent::Error { .. })));
    assert_eq!(fixture.writes(), 0);
    let outbound = serde_json::to_string(&*received.lock().unwrap()).unwrap();
    assert!(!outbound.contains("synthetic-runtime-token"));
    assert!(!outbound.contains("synthetic-hidden-value"));
    let history = app.kube_ai_history(context.id.to_string()).await.unwrap();
    assert_eq!(history.len(), 1);
    assert!(!history[0].synced);
    assert!(!history[0].conversation.messages[0]
        .content
        .contains("synthetic-hidden-value"));
    let conversation_id = history[0].conversation.id.to_string();
    let mut follow_up = ai_question();
    follow_up.conversation_id = Some(conversation_id.clone());
    follow_up.question = "Explain the previous answer".into();
    let review = app
        .kube_ai_prepare(session.id.clone(), follow_up)
        .await
        .unwrap();
    assert_eq!(review.history_messages, 2);
    let request = app
        .kube_ai_start(session.id.clone(), review.ticket, false)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let poll = app
                .kube_ai_poll(session.id.clone(), request.clone())
                .await
                .unwrap();
            assert!(!poll
                .events
                .iter()
                .any(|e| matches!(e, crate::kubernetes_ai::KubeAiEvent::Error { .. })));
            if poll.closed {
                break;
            }
        }
    })
    .await
    .unwrap();
    let history = app.kube_ai_history(context.id.to_string()).await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].conversation.messages.len(), 4);
    assert_eq!(history[0].conversation.id.to_string(), conversation_id);
    let transcript = received.lock().unwrap().last().unwrap()["messages"].to_string();
    assert!(transcript.contains("Why is web running?"));
    assert!(transcript.contains("The selected Pod is running."));
    assert!(transcript.contains("Explain the previous answer"));
    assert!(!transcript.contains("synthetic-hidden-value"));
    let mut pending_follow_up = ai_question();
    pending_follow_up.conversation_id = Some(conversation_id.clone());
    let pending = app
        .kube_ai_prepare(session.id.clone(), pending_follow_up)
        .await
        .unwrap();
    app.kube_ai_delete_conversation(conversation_id)
        .await
        .unwrap();
    assert!(app
        .kube_ai_start(session.id.clone(), pending.ticket, false)
        .await
        .is_err());
    assert!(app
        .kube_ai_history(context.id.to_string())
        .await
        .unwrap()
        .is_empty());
    mock.await.unwrap();
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn event_recording_is_opt_in_encrypted_local_and_survives_disconnect() {
    let fixture = Fixture::start(false).await;
    let (_dir, app, mut context) = fixture.app().await;
    let session = app.kube_connect(context.id.to_string()).await.unwrap();
    assert!(
        !app.kube_history_status(session.id.clone())
            .await
            .unwrap()
            .enabled
    );
    assert!(!fixture
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|request| request.contains("/events")));
    app.kube_disconnect(session.id).await.unwrap();
    context.event_history_enabled = true;
    app.save_kube_context(context.clone()).await.unwrap();
    let session = app.kube_connect(context.id.to_string()).await.unwrap();
    let records = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let records = app
                .kube_event_history(context.id.to_string(), None, false, None, None, 200)
                .await
                .unwrap();
            if !records.is_empty() {
                break records;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].count, 3);
    assert!(!records[0].message.contains("synthetic-event-secret"));
    app.kube_disconnect(session.id).await.unwrap();
    assert_eq!(
        app.kube_event_history(
            context.id.to_string(),
            Some("default".into()),
            true,
            None,
            None,
            200
        )
        .await
        .unwrap()
        .len(),
        1
    );
    assert!(app
        .kube_event_history(
            context.id.to_string(),
            Some("other".into()),
            false,
            None,
            None,
            200
        )
        .await
        .unwrap()
        .is_empty());
    app.kube_event_history_clear(context.id.to_string())
        .await
        .unwrap();
    assert!(app
        .kube_event_history(context.id.to_string(), None, false, None, None, 200)
        .await
        .unwrap()
        .is_empty());
    app.shutdown().await.unwrap();
}
