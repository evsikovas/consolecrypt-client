//! Component HTTP/TLS tests over actual sockets. This connector is deliberately
//! a test TCP fixture, not a claim of Native SSH/jump acceptance.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

#[derive(Debug)]
struct TcpFixture {
    source: SourceConfig,
    address: std::net::SocketAddr,
    current: Arc<AtomicBool>,
}
impl MonitoringSourceConnector for TcpFixture {
    fn source(&self) -> &SourceConfig {
        &self.source
    }
    fn is_current(&self) -> bool {
        self.current.load(Ordering::Acquire)
    }
    fn connect(&self) -> MonitoringConnect<'_> {
        Box::pin(async move {
            TcpStream::connect(self.address)
                .await
                .map(|io| Box::new(io) as MonitoringIo)
                .map_err(|_| Error::new(ErrorCode::Transport, "Owned fixture unavailable"))
        })
    }
}
fn source(endpoint: String) -> SourceConfig {
    SourceConfig {
        id: "scoped".into(),
        name: "Owned".into(),
        endpoint,
        route: Route::SshHost {
            host_id: "explicit-saved-host".into(),
        },
        auth: AuthRef::None,
        tls: TlsConfig::default(),
    }
}
fn response(status: u16, extra: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status} Owned\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}",
        body.len()
    )
}
const VECTOR: &str = r#"{"status":"success","data":{"resultType":"vector","result":[{"metric":{},"value":[1,"1"]}]}}"#;
async fn exchange<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    mut io: S,
    reply: String,
) -> String {
    let mut bytes = Vec::new();
    while !bytes.windows(4).any(|v| v == b"\r\n\r\n") {
        let mut part = [0; 4096];
        let n = io.read(&mut part).await.unwrap();
        assert!(n > 0 && bytes.len() + n <= 32768);
        bytes.extend_from_slice(&part[..n]);
    }
    io.write_all(reply.as_bytes()).await.unwrap();
    String::from_utf8(bytes).unwrap()
}
async fn fixture_client(
    reply: String,
    budgets: Budgets,
) -> (
    MonitoringClient,
    Arc<AtomicBool>,
    tokio::task::JoinHandle<String>,
) {
    fixture_client_route(reply,budgets,Route::SshHost{host_id:"explicit-saved-host".into()}).await
}
async fn fixture_client_route(reply:String,budgets:Budgets,route:Route)->(
    MonitoringClient,Arc<AtomicBool>,tokio::task::JoinHandle<String>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    // Deliberately unresolvable original authority. The connector supplies the
    // actual owned socket; URL/Host identity must remain this original value.
    let mut source = source("http://selected.prometheus.invalid:9090/prefix".into());
    source.route=route;
    let current = Arc::new(AtomicBool::new(true));
    let connector = Arc::new(TcpFixture {
        source: source.clone(),
        address,
        current: current.clone(),
    });
    let client = MonitoringClient::with_scoped_connector(source, budgets, None, connector).unwrap();
    let task =
        tokio::spawn(async move { exchange(listener.accept().await.unwrap().0, reply).await });
    (client, current, task)
}
#[tokio::test]
async fn fixed_query_is_get_preserves_original_authority_and_prefix_without_proxy() {
    let (client, _, server) = fixture_client(response(200, "", VECTOR), Budgets::default()).await;
    assert!(client.query("up{job=\"node\"}", Some(1.0)).await.is_ok());
    let request = server.await.unwrap();
    assert!(request.starts_with("GET /prefix/api/v1/query?query="));
    assert!(request.contains("limit=101") && request.contains("timeout=15s"));
    let lower = request.to_ascii_lowercase();
    assert!(lower.contains("host: selected.prometheus.invalid:9090\r\n"));
    assert!(
        !lower.contains("proxy-authorization")
            && !lower.contains("authorization:")
            && !lower.contains("post ")
    );
}
#[tokio::test]
async fn scoped_redirect_is_not_followed_and_body_budget_is_enforced() {
    let (client, _, server) = fixture_client(
        response(302, "Location: http://other.invalid/private\r\n", ""),
        Budgets::default(),
    )
    .await;
    assert_eq!(
        client.query("up", None).await.unwrap_err().code,
        ErrorCode::Transport
    );
    server.await.unwrap();
    let (client, _, server) = fixture_client(
        response(200, "", &"x".repeat(1025)),
        Budgets {
            max_response_bytes: 1024,
            ..Budgets::default()
        },
    )
    .await;
    assert_eq!(
        client.query("up", None).await.unwrap_err().code,
        ErrorCode::ResponseTooLarge
    );
    server.await.unwrap();
}
#[tokio::test]
async fn expired_original_scope_denies_before_any_connection_and_permit_is_released() {
    let (client, current, server) =
        fixture_client(response(200, "", VECTOR), Budgets::default()).await;
    current.store(false, Ordering::Release);
    assert_eq!(
        client.query("up", None).await.unwrap_err().code,
        ErrorCode::Transport
    );
    assert_eq!(client.permits.available_permits(), 4);
    assert!(!server.is_finished());
    server.abort();
    let _ = server.await;
}
#[tokio::test]
async fn mutation_during_actual_response_cannot_publish_a_result() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = source("http://original.invalid:9090".into());
    let current = Arc::new(AtomicBool::new(true));
    let connector = Arc::new(TcpFixture {
        source: config.clone(),
        address: listener.local_addr().unwrap(),
        current: current.clone(),
    });
    let client =
        MonitoringClient::with_scoped_connector(config, Budgets::default(), None, connector)
            .unwrap();
    let (seen, waiting) = tokio::sync::oneshot::channel();
    let (release, blocked) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut io, _) = listener.accept().await.unwrap();
        let mut data = [0; 4096];
        assert!(io.read(&mut data).await.unwrap() > 0);
        seen.send(()).unwrap();
        blocked.await.unwrap();
        let _ = io.write_all(response(200, "", VECTOR).as_bytes()).await;
    });
    let request = tokio::spawn(async move { client.query("up", None).await });
    waiting.await.unwrap();
    current.store(false, Ordering::Release);
    release.send(()).unwrap();
    assert!(request.await.unwrap().is_err());
    server.await.unwrap();
}
#[test]
fn stream_constructor_requires_scoped_supported_route_and_exact_original_configuration() {
    let config = source("http://original.invalid".into());
    for route in [
        Route::Direct,
        Route::SshTunnel {
            route_id: "explicit-saved-host".into(),
        },
    ] {
        let mut wrong = config.clone();
        wrong.route = route;
        let connector = Arc::new(TcpFixture {
            source: wrong.clone(),
            address: "127.0.0.1:1".parse().unwrap(),
            current: Arc::new(AtomicBool::new(true)),
        });
        assert!(MonitoringClient::with_scoped_connector(
            wrong,
            Budgets::default(),
            None,
            connector
        )
        .is_err());
    }
    let mut other = config.clone();
    other.endpoint = "http://other.invalid".into();
    let connector = Arc::new(TcpFixture {
        source: other,
        address: "127.0.0.1:1".parse().unwrap(),
        current: Arc::new(AtomicBool::new(true)),
    });
    assert!(
        MonitoringClient::with_scoped_connector(config, Budgets::default(), None, connector)
            .is_err()
    );
}
async fn tls_client(
    trust: bool,
    name: &str,
) -> (MonitoringClient, tokio::task::JoinHandle<Option<String>>) {
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["original.invalid".into()]).unwrap();
    let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![cert.der().clone()],
        rustls::pki_types::PrivatePkcs8KeyDer::from(signing_key.serialize_der()).into(),
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut config = source(format!("https://{name}:9090"));
    if trust {
        config.tls.custom_ca_pem = Some(cert.pem());
    }
    let connector = Arc::new(TcpFixture {
        source: config.clone(),
        address,
        current: Arc::new(AtomicBool::new(true)),
    });
    let client =
        MonitoringClient::with_scoped_connector(config, Budgets::default(), None, connector)
            .unwrap();
    let server = tokio::spawn(async move {
        let io = listener.accept().await.unwrap().0;
        match tokio_rustls::TlsAcceptor::from(Arc::new(tls))
            .accept(io)
            .await
        {
            Ok(io) => Some(exchange(io, response(200, "", VECTOR)).await),
            Err(_) => None,
        }
    });
    (client, server)
}
#[tokio::test]
async fn https_original_sni_and_saved_ca_verify_over_the_scoped_socket() {
    let (client, server) = tls_client(true, "original.invalid").await;
    assert!(client.query("up", None).await.is_ok());
    assert!(server
        .await
        .unwrap()
        .unwrap()
        .to_ascii_lowercase()
        .contains("host: original.invalid:9090\r\n"));
}
#[tokio::test]
async fn https_wrong_original_name_or_untrusted_ca_never_downgrades_or_sends_http() {
    for (trust, name) in [(false, "original.invalid"), (true, "other.invalid")] {
        let (client, server) = tls_client(trust, name).await;
        assert_eq!(
            client.query("up", None).await.unwrap_err().code,
            ErrorCode::Transport
        );
        assert!(server.await.unwrap().is_none());
    }
}
