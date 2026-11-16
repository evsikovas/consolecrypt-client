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
#[tokio::test]
async fn agent_scoped_get_preserves_original_origin_and_expires_without_a_fallback_dial() {
    let route=Route::AgentTunnel{route_id:"11111111-1111-4111-8111-111111111111".into()};
    let(client,current,server)=fixture_client_route(response(200,"",VECTOR),Budgets::default(),route).await;
    assert!(client.query("up",None).await.is_ok());
    let request=server.await.unwrap().to_ascii_lowercase();
    assert!(request.starts_with("get /prefix/api/v1/query?"));
    assert!(request.contains("host: selected.prometheus.invalid:9090\r\n"));
    current.store(false,Ordering::Release);
    assert_eq!(client.query("up",None).await.unwrap_err().code,ErrorCode::Transport);
}
async fn tls_client(
    trust: bool,
    name: &str,
) -> (MonitoringClient, tokio::task::JoinHandle<Option<String>>) {
    tls_client_route(trust,name,false).await
}
async fn tls_client_route(trust:bool,name:&str,agent_destination:bool)
    -> (MonitoringClient,tokio::task::JoinHandle<Option<String>>) {
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
    let logical_port=if agent_destination&&address.port()==9090 {9091}else{9090};
    let mut config = source(format!("https://{name}:{logical_port}"));
    if agent_destination {
        config.route=Route::AgentDestination{association_id:"11111111-1111-4111-8111-111111111111".into(),
            host_id:"22222222-2222-4222-8222-222222222222".into(),destination_ip:"127.0.0.1".into(),destination_port:address.port()};
    }
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

#[derive(Debug)]
struct ProbeFixture { tcp: TcpFixture, connects: std::sync::atomic::AtomicUsize }
impl MonitoringSourceConnector for ProbeFixture {
    fn source(&self)->&SourceConfig {&self.tcp.source}
    fn is_current(&self)->bool {self.tcp.is_current()}
    fn connect(&self)->MonitoringConnect<'_> {
        self.connects.fetch_add(1,Ordering::SeqCst);self.tcp.connect()
    }
}
const PROBE_MATRIX:&str=r#"{"status":"success","data":{"resultType":"matrix","result":[{"metric":{},"values":[[0,"1"],[1,"1"]]}]}}"#;
async fn probe_fixture()->(MonitoringClient,Arc<ProbeFixture>,TcpListener) {
    let listener=TcpListener::bind("127.0.0.1:0").await.unwrap();let address=listener.local_addr().unwrap();
    let mut source=source(format!("http://127.0.0.1:{}/prefix",address.port()));
    source.route=Route::AgentDestination{association_id:"11111111-1111-4111-8111-111111111111".into(),
        host_id:"22222222-2222-4222-8222-222222222222".into(),destination_ip:"127.0.0.1".into(),destination_port:address.port()};
    let connector=Arc::new(ProbeFixture{tcp:TcpFixture{source:source.clone(),address,current:Arc::new(AtomicBool::new(true))},
        connects:std::sync::atomic::AtomicUsize::new(0)});
    let client=MonitoringClient::with_scoped_connector(source,Budgets::default(),None,connector.clone()).unwrap();
    (client,connector,listener)
}
async fn probe_head(io:&mut TcpStream)->String {
    let mut bytes=Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        let mut byte=[0];let n=io.read(&mut byte).await.unwrap();assert_eq!(n,1);bytes.push(byte[0]);assert!(bytes.len()<=32768);
    }
    String::from_utf8(bytes).unwrap()
}
async fn probe_reply(io:&mut TcpStream,body:&str,close:bool) {
    io.write_all(format!("HTTP/1.1 200 Owned\r\nContent-Length: {}\r\nConnection: {}\r\n\r\n{}",body.len(),if close{"close"}else{"keep-alive"},body).as_bytes()).await.unwrap();
}
#[tokio::test]
async fn agent_probe_uses_one_original_connection_two_fixed_gets_and_no_third() {
    let(client,connector,listener)=probe_fixture().await;
    let server=tokio::spawn(async move {
        let(mut io,_)=listener.accept().await.unwrap();let first=probe_head(&mut io).await;
        probe_reply(&mut io,VECTOR,false).await;let second=probe_head(&mut io).await;
        probe_reply(&mut io,PROBE_MATRIX,true).await;(first,second)
    });
    let result=client.probe().await;assert!(result.query.supported&&result.query_range.supported);
    assert_eq!(connector.connects.load(Ordering::SeqCst),1);
    let(first,second)=server.await.unwrap();
    for (head,path,expected) in [(&first,"/prefix/api/v1/query",vec![("query","vector(1)"),("time","1"),("timeout","15s"),("limit","101")]),
        (&second,"/prefix/api/v1/query_range",vec![("query","vector(1)"),("start","0"),("end","1"),("step","1"),("timeout","15s"),("limit","101")])] {
        let target=head.lines().next().unwrap().split_whitespace().nth(1).unwrap();let parsed=url::Url::parse(&format!("http://original{target}")).unwrap();
        assert_eq!(parsed.path(),path);assert_eq!(parsed.query_pairs().map(|(k,v)|(k.into_owned(),v.into_owned())).collect::<Vec<_>>(),expected.into_iter().map(|(k,v)|(k.to_owned(),v.to_owned())).collect::<Vec<_>>());
    }
    assert!(first.to_ascii_lowercase().contains("connection: keep-alive\r\n"));
    assert!(second.to_ascii_lowercase().contains("connection: close\r\n"));
    assert_eq!(client.permits.available_permits(),4);
}
#[tokio::test]
async fn agent_probe_first_connection_close_is_partial_without_reconnect() {
    let(client,connector,listener)=probe_fixture().await;
    let server=tokio::spawn(async move {let(mut io,_)=listener.accept().await.unwrap();let _=probe_head(&mut io).await;probe_reply(&mut io,VECTOR,true).await;});
    let result=client.probe().await;server.await.unwrap();assert!(result.query.supported);assert!(!result.query_range.supported);
    assert_eq!(result.query_range.error.unwrap().code,ErrorCode::Transport);assert_eq!(connector.connects.load(Ordering::SeqCst),1);
}
#[tokio::test]
async fn agent_probe_same_session_scope_retirement_and_third_dispatch_are_denied() {
    let(client,connector,listener)=probe_fixture().await;
    let server=tokio::spawn(async move {let(mut io,_)=listener.accept().await.unwrap();let _=probe_head(&mut io).await;probe_reply(&mut io,VECTOR,false).await;
        let _=probe_head(&mut io).await;probe_reply(&mut io,PROBE_MATRIX,true).await;});
    let end=std::time::Instant::now()+std::time::Duration::from_secs(15);
    let mut held=client.scoped.as_ref().unwrap().probe_session(&client.budgets,end).await.unwrap();
    assert!(held.next_fixed(None).await.is_ok());assert!(held.next_fixed(None).await.is_ok());
    assert_eq!(held.next_fixed(None).await.unwrap_err().code,ErrorCode::Transport);
    held.close_join().await.unwrap();server.await.unwrap();assert_eq!(connector.connects.load(Ordering::SeqCst),1);
    let(client,connector,listener)=probe_fixture().await;
    let server=tokio::spawn(async move{let(mut io,_)=listener.accept().await.unwrap();let _=probe_head(&mut io).await;probe_reply(&mut io,VECTOR,false).await;
        let mut byte=[0];assert_eq!(io.read(&mut byte).await.unwrap(),0);});
    let mut held=client.scoped.as_ref().unwrap().probe_session(&client.budgets,end).await.unwrap();
    assert!(held.next_fixed(None).await.is_ok());connector.tcp.current.store(false,Ordering::Release);
    assert_eq!(held.next_fixed(None).await.unwrap_err().code,ErrorCode::Transport);
    held.close_join().await.unwrap();server.await.unwrap();assert_eq!(connector.connects.load(Ordering::SeqCst),1);
}
#[tokio::test]
async fn agent_probe_total_bytes_and_original_expiry_are_not_reset_between_requests() {
    let(mut client,connector,listener)=probe_fixture().await;client.budgets.max_response_bytes=VECTOR.len()+PROBE_MATRIX.len()-1;
    let server=tokio::spawn(async move{let(mut io,_)=listener.accept().await.unwrap();let _=probe_head(&mut io).await;probe_reply(&mut io,VECTOR,false).await;
        let _=probe_head(&mut io).await;probe_reply(&mut io,PROBE_MATRIX,true).await;});
    let result=client.probe().await;server.await.unwrap();assert!(result.query.supported);assert_eq!(result.query_range.error.unwrap().code,ErrorCode::ResponseTooLarge);
    assert_eq!(connector.connects.load(Ordering::SeqCst),1);
    let(client,connector,_listener)=probe_fixture().await;
    assert!(client.scoped.as_ref().unwrap().probe_session(&client.budgets,std::time::Instant::now()).await.is_err());
    assert_eq!(connector.connects.load(Ordering::SeqCst),0);
    let(client,connector,listener)=probe_fixture().await;
    let server=tokio::spawn(async move{let(mut io,_)=listener.accept().await.unwrap();let _=probe_head(&mut io).await;
        probe_reply(&mut io,VECTOR,false).await;let mut byte=[0];assert_eq!(io.read(&mut byte).await.unwrap(),0);});
    let end=std::time::Instant::now()+std::time::Duration::from_millis(250);
    let mut held=client.scoped.as_ref().unwrap().probe_session(&client.budgets,end).await.unwrap();
    assert!(held.next_fixed(None).await.is_ok());
    // Let the ORIGINAL data budget elapse, retaining part of its own cleanup
    // reserve. No clock substitution, deadline reset or second request is used.
    tokio::time::sleep_until(tokio::time::Instant::from_std(end-std::time::Duration::from_millis(80))).await;
    assert_eq!(held.next_fixed(None).await.unwrap_err().code,ErrorCode::Timeout);
    held.close_join().await.unwrap();server.await.unwrap();
    assert_eq!(connector.connects.load(Ordering::SeqCst),1);
    let(mut client,connector,_listener)=probe_fixture().await;client.budgets.max_query_bytes=1;
    let result=client.probe().await;assert_eq!(result.query.error.unwrap().code,ErrorCode::InvalidQuery);
    assert_eq!(connector.connects.load(Ordering::SeqCst),0);
}

#[tokio::test]
async fn agent_probe_timeout_reserves_original_driver_join_without_reconnect() {
    let(mut client,connector,listener)=probe_fixture().await;client.budgets.timeout_seconds=1;
    let server=tokio::spawn(async move {
        let(mut io,_)=listener.accept().await.unwrap();let head=probe_head(&mut io).await;
        assert!(head.contains("timeout=1s"));
        let mut byte=[0];assert_eq!(io.read(&mut byte).await.unwrap(),0);
    });
    let result=client.probe().await;
    // Timeout stays visible only if the original driver abort/join succeeded;
    // close_join failure would invalidate both capabilities as Transport.
    assert_eq!(result.query.error.unwrap().code,ErrorCode::Timeout);
    assert!(!result.query_range.supported);assert_eq!(connector.connects.load(Ordering::SeqCst),1);
    assert_eq!(client.permits.available_permits(),4);server.await.unwrap();
}

#[tokio::test]
async fn saved_agent_destination_preserves_fqdn_tls_ca_host_and_independent_physical_port() {
    let(client,server)=tls_client_route(true,"original.invalid",true).await;
    let logical=url::Url::parse(&client.source.endpoint).unwrap();
    assert_eq!(logical.host_str(),Some("original.invalid"));
    let logical_port=logical.port().unwrap();
    assert!(client.source.tls.custom_ca_pem.is_some());
    assert!(matches!(client.source.route,Route::AgentDestination{ref destination_ip,destination_port,..}
        if destination_ip=="127.0.0.1"&&destination_port!=logical_port));
    assert!(client.query("up",None).await.is_ok());
    let head=server.await.unwrap().unwrap().to_ascii_lowercase();
    assert!(head.contains(&format!("host: original.invalid:{logical_port}\r\n")));
    for(trust,name)in[(false,"original.invalid"),(true,"other.invalid")] {
        let(client,server)=tls_client_route(trust,name,true).await;
        assert_eq!(client.query("up",None).await.unwrap_err().code,ErrorCode::Transport);
        assert!(server.await.unwrap().is_none());
    }
}
