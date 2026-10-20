use cc_k8s_core::{
    policy::{Audit, AuditEntry, DeviceClass, Policy},
    session::{Authentication, ConnectOptions, ConnectionRoute, KubernetesSession},
};
use http::Request;
use secrecy::SecretString;
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};
use tokio_rustls::TlsAcceptor;
use tower::BoxError;
use url::Url;
use uuid::Uuid;

#[derive(Default)]
struct TestAudit(Mutex<Vec<AuditEntry>>);
impl Audit for TestAudit {
    fn record(&self, entry: AuditEntry) -> Result<(), BoxError> {
        self.0.lock().unwrap().push(entry);
        Ok(())
    }
}

struct Fixture {
    address: SocketAddr,
    certificate: Vec<u8>,
    requests: Arc<AtomicUsize>,
    headers: Arc<Mutex<Vec<String>>>,
    task: JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Fixture {
    async fn start() -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let certificate =
            rcgen::generate_simple_self_signed(vec!["api.example.test".into()]).unwrap();
        let der = certificate.cert.der().to_vec();
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
        let requests = Arc::new(AtomicUsize::new(0));
        let headers = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        let captured = headers.clone();
        let task = tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let Ok(mut stream) = acceptor.accept(socket).await else {
                    continue;
                };
                let mut request = Vec::new();
                let mut byte = [0];
                while request.len() < 16 * 1024 && !request.ends_with(b"\r\n\r\n") {
                    if stream.read_exact(&mut byte).await.is_err() {
                        break;
                    }
                    request.push(byte[0]);
                }
                if !request.ends_with(b"\r\n\r\n") {
                    continue;
                }
                seen.fetch_add(1, Ordering::SeqCst);
                captured
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(request).unwrap());
                let body = r#"{"gitVersion":"v1.32.0"}"#;
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(reply.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });
        Self {
            address,
            certificate: der,
            requests,
            headers,
            task,
        }
    }

    fn options(&self) -> ConnectOptions {
        ConnectOptions {
            server: Url::parse("https://api.example.test:6443").unwrap(),
            namespace: "default".into(),
            root_certificates: vec![self.certificate.clone()],
            tls_server_name: None,
            insecure_skip_tls_verify: false,
            route: ConnectionRoute::SshLoopback(self.address),
        }
    }
}

fn policy() -> Policy {
    Policy {
        context_id: Uuid::new_v4(),
        context_name: "Production".into(),
        production: true,
        device: DeviceClass::Desktop,
        allow_exec_in_production: false,
        allow_port_forward_in_production: false,
    }
}

fn auth() -> Authentication {
    Authentication::Bearer(SecretString::from("synthetic-fixture-token"))
}

fn request(method: &str, path: &str) -> Request<Vec<u8>> {
    Request::builder()
        .method(method)
        .uri(path)
        .body(Vec::new())
        .unwrap()
}

#[tokio::test]
async fn native_kube_transport_enforces_guard_before_any_http_bytes() {
    let fixture = Fixture::start().await;
    let audit = Arc::new(TestAudit::default());
    let session =
        KubernetesSession::new(fixture.options(), auth(), policy(), audit.clone()).unwrap();
    let client = session.client();
    let version = tokio::time::timeout(
        Duration::from_secs(5),
        client.request_text(request("GET", "/version")),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(version.contains("v1.32.0"));
    assert_eq!(fixture.requests.load(Ordering::SeqCst), 1);
    let headers = fixture.headers.lock().unwrap()[0].to_lowercase();
    assert!(headers.contains("host: api.example.test:6443\r\n"));
    assert!(headers.contains("authorization: bearer synthetic-fixture-token\r\n"));
    for (method, path) in [
        (
            "PATCH",
            "/apis/apps/v1/namespaces/default/deployments/app/scale",
        ),
        ("DELETE", "/api/v1/namespaces/default/pods/app"),
        (
            "POST",
            "/apis/kustomize.toolkit.fluxcd.io/v1/namespaces/default/kustomizations",
        ),
        ("GET", "/api/v1/namespaces/default/pods/app/exec"),
        ("GET", "/api/v1/namespaces/default/pods/app/portforward"),
        ("GET", "/api/v1/nodes/node/proxy"),
        ("GET", "/api/v1/namespaces/default/pods/app/%65xec"),
    ] {
        assert!(client.request_text(request(method, path)).await.is_err());
    }
    assert_eq!(fixture.requests.load(Ordering::SeqCst), 1);
    assert!(audit.0.lock().unwrap().is_empty());
    session.guard().open_write_window("Production").unwrap();
    client
        .request_text(request(
            "PATCH",
            "/apis/apps/v1/namespaces/default/deployments/app",
        ))
        .await
        .unwrap();
    assert_eq!(fixture.requests.load(Ordering::SeqCst), 2);
    assert_eq!(audit.0.lock().unwrap().len(), 1);
    drop(session);
    assert!(client
        .request_text(request("GET", "/version"))
        .await
        .is_err());
    assert_eq!(fixture.requests.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn ssh_loopback_does_not_weaken_tls_name_or_certificate_validation() {
    let fixture = Fixture::start().await;
    let mut wrong_name = fixture.options();
    wrong_name.tls_server_name = Some("other.example.test".into());
    let other = rcgen::generate_simple_self_signed(vec!["api.example.test".into()]).unwrap();
    let mut wrong_ca = fixture.options();
    wrong_ca.root_certificates = vec![other.cert.der().to_vec()];
    for settings in [wrong_name, wrong_ca] {
        let session =
            KubernetesSession::new(settings, auth(), policy(), Arc::new(TestAudit::default()))
                .unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            session.client().request_text(request("GET", "/version")),
        )
        .await
        .expect("TLS failure must not hang");
        assert!(result.is_err());
    }
    assert_eq!(fixture.requests.load(Ordering::SeqCst), 0);
}
