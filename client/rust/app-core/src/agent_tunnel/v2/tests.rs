//! Real Local scope + maintained TLS/TCP; no Agent join/CAS or Relay claim.
use super::*;
use rustls::{pki_types::ServerName, RootCertStore};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_rustls::{client::TlsStream, server::TlsStream as ServerTlsStream};

const PASSWORD: &str = "owned original V2 TLS fixture password 42";
async fn original() -> (
    tempfile::TempDir,
    AppCore,
    Arc<OriginalSessionDeviceIdentity>,
    Budget,
) {
    let root = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        root.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("V2 stop fixture".into(), PASSWORD.into())
        .await
        .unwrap();
    let scope = Arc::new(
        OriginalSessionDeviceIdentity::capture(
            &app,
            &CancellationToken::new(),
            Duration::from_secs(30),
        )
        .await
        .unwrap(),
    );
    let p: TunnelRoutePolicy = serde_json::from_str(include_str!(
        "../../../../agent-node-wire/src/tunnel_policy_vector.json"
    ))
    .unwrap();
    let budget = Budget::new(p.limits, CancellationToken::new());
    (root, app, scope, budget)
}
fn expiry(scope: &OriginalSessionDeviceIdentity) -> Timestamp {
    Timestamp::new(scope.now().unwrap().0.seconds() + 30).unwrap()
}
async fn tls_pair() -> (
    TlsStream<tokio::io::DuplexStream>,
    ServerTlsStream<tokio::io::DuplexStream>,
) {
    let (a, b) = tokio::io::duplex(65536);
    tls_pair_on(a, b).await
}
async fn tls_pair_on<A, B>(a: A, b: B) -> (TlsStream<B>, ServerTlsStream<A>)
where
    A: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    B: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let agent = rcgen::generate_simple_self_signed(vec!["agent-v2.test".into()]).unwrap();
    let client = rcgen::generate_simple_self_signed(vec!["client-v2.test".into()]).unwrap();
    let mut policy: TunnelRoutePolicy = serde_json::from_str(include_str!(
        "../../../../agent-node-wire/src/tunnel_policy_vector.json"
    ))
    .unwrap();
    policy.agent_tls_certificate_sha256 = Hash::from_bytes(Sha256::digest(agent.cert.der()).into());
    policy.client_tls_certificate_sha256 =
        Hash::from_bytes(Sha256::digest(client.cert.der()).into());
    let mut agent_roots = RootCertStore::empty();
    agent_roots.add(agent.cert.der().clone()).unwrap();
    let mut client_roots = RootCertStore::empty();
    client_roots.add(client.cert.der().clone()).unwrap();
    let native = TunnelTls::provisioned(
        vec![client.cert.der().clone()],
        rustls::pki_types::PrivatePkcs8KeyDer::from(client.signing_key.serialize_der()).into(),
        agent_roots,
        ServerName::try_from("agent-v2.test").unwrap().to_owned(),
        &policy,
    )
    .unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
        Arc::new(client_roots),
        provider.clone(),
    )
    .build()
    .unwrap();
    let server = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_client_cert_verifier(verifier)
        .with_single_cert(
            vec![agent.cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(agent.signing_key.serialize_der()).into(),
        )
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server));
    let (client, server) = tokio::join!(native.connect(b, &policy), acceptor.accept(a));
    (client.unwrap(), server.unwrap())
}
async fn local_pair() -> (tokio::net::TcpStream, tokio::net::TcpStream) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (a, b) = tokio::join!(tokio::net::TcpStream::connect(address), listener.accept());
    (a.unwrap(), b.unwrap().0)
}
async fn observe_peer(
    mut server: ServerTlsStream<tokio::io::DuplexStream>,
) -> std::io::Result<usize> {
    tokio::time::timeout(Duration::from_secs(2), server.read(&mut [0u8; 1]))
        .await
        .expect("owned TLS peer observation timeout")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_original_stop_sends_actual_tls13_close_notify_request_only() {
    let (_root, app, scope, budget) = original().await;
    let stop = OriginalStop::new(scope.clone());
    let io = OriginalIo::new(&scope, &budget, expiry(&scope));
    let (mut client, server) = tls_pair().await;
    scope
        .with_loaded(&app, |_| stop.request_held())
        .await
        .unwrap();
    let (sent, observed) = tokio::join!(
        stop.send_request(&app, &mut client, &io),
        observe_peer(server)
    );
    assert!(sent);
    drop(client);
    assert_eq!(
        observed.unwrap(),
        0,
        "authenticated close_notify is a request, never a drain proof"
    );
    assert!(
        budget.check().is_ok(),
        "sending stop cannot renew or reset the original budget"
    );
    app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn raw_local_eof_after_json_looking_stop_is_unknown_without_tls_close_notify() {
    let (_root, app, scope, budget) = original().await;
    let io = OriginalIo::new(&scope, &budget, expiry(&scope));
    let channel = budget.open().unwrap();
    let (mut client, mut server) = tls_pair().await;
    let (mut local, mut peer) = local_pair().await;
    let payload = b"{\"action\":\"stop\"}\n";
    let mut totals = client::Totals::default();
    let (copy, ()) = tokio::join!(
        client::copy_data(&mut client, &mut local, &channel, &io, &mut totals),
        async {
            peer.write_all(payload).await.unwrap();
            peer.shutdown().await.unwrap();
        }
    );
    assert!(copy.is_err(), "raw EOF is unknown");
    assert_eq!(totals.sent, payload.len() as u64);
    drop(client);
    let mut observed = vec![0; payload.len()];
    server.read_exact(&mut observed).await.unwrap();
    assert_eq!(observed.as_slice(), payload);
    assert_eq!(
        observe_peer(server).await.unwrap_err().kind(),
        std::io::ErrorKind::UnexpectedEof
    );
    app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn raw_tls_carrier_loss_is_unknown_and_does_not_create_stop_intent() {
    let (_root, app, scope, budget) = original().await;
    let io = OriginalIo::new(&scope, &budget, expiry(&scope));
    let channel = budget.open().unwrap();
    let stop = OriginalStop::new(scope.clone());
    let (mut client, server) = tls_pair().await;
    let (mut local, _peer) = local_pair().await;
    drop(server);
    let mut totals = client::Totals::default();
    assert!(tokio::time::timeout(
        Duration::from_secs(2),
        client::copy_data(&mut client, &mut local, &channel, &io, &mut totals)
    )
    .await
    .unwrap()
    .is_err());
    assert!(!stop.is_requested());
    assert!(!stop.send_request(&app, &mut client, &io).await);
    drop(client);
    app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn requested_stop_after_real_lock_reunlock_aborts_without_tls_close_notify() {
    let (_root, app, scope, budget) = original().await;
    let stop = OriginalStop::new(scope.clone());
    let io = OriginalIo::new(&scope, &budget, expiry(&scope));
    let (mut client, server) = tls_pair().await;
    scope
        .with_loaded(&app, |_| stop.request_held())
        .await
        .unwrap();
    app.lock().await.unwrap();
    app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    assert!(scope.check(&app).await.is_err());
    assert!(!stop.send_request(&app, &mut client, &io).await);
    drop(client);
    assert_eq!(
        observe_peer(server).await.unwrap_err().kind(),
        std::io::ErrorKind::UnexpectedEof
    );
    app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropped_original_core_retires_pending_stop_without_retaining_loaded_owner() {
    let (_root, app, scope, budget) = original().await;
    let stop = OriginalStop::new(scope.clone());
    let (owner, unlock, identity) = {
        let owner = app.session().await.unwrap();
        let unlock = owner.unlocked().await.unwrap();
        let identity = owner.identity();
        (
            Arc::downgrade(&owner),
            Arc::downgrade(&unlock),
            Arc::downgrade(&identity),
        )
    };
    let io = OriginalIo::new(&scope, &budget, expiry(&scope));
    let (client, server) = tls_pair().await;
    drop(app);
    assert!(stop.request_held().is_err());
    assert!(io.check().is_err());
    assert!(
        owner.upgrade().is_none() && unlock.upgrade().is_none() && identity.upgrade().is_none()
    );
    drop(client);
    assert_eq!(
        observe_peer(server).await.unwrap_err().kind(),
        std::io::ErrorKind::UnexpectedEof
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn same_profile_new_capture_cannot_retarget_original_stop_or_poll_tls_shutdown() {
    let (_root, app, scope, budget) = original().await;
    let stop = OriginalStop::new(scope.clone());
    scope
        .with_loaded(&app, |_| stop.request_held())
        .await
        .unwrap();
    let replacement = OriginalSessionDeviceIdentity::capture(
        &app,
        &CancellationToken::new(),
        Duration::from_secs(30),
    )
    .await
    .unwrap();
    let io = OriginalIo::new(&replacement, &budget, expiry(&replacement));
    let (mut client, server) = tls_pair().await;
    assert!(!stop.send_request(&app, &mut client, &io).await);
    drop(client);
    assert_eq!(
        observe_peer(server).await.unwrap_err().kind(),
        std::io::ErrorKind::UnexpectedEof
    );
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_actual_write_cannot_dispatch_remaining_bytes_after_working_version_changes() {
    let (_root, app, scope, budget) = original().await;
    let io = OriginalIo::new(&scope, &budget, expiry(&scope));
    let (mut writer, mut reader) = tokio::io::duplex(1);
    let mut writing =
        Box::pin(io.run(async { writer.write_all(b"ab").await.map_err(|_| denied()) }));
    tokio::select! {biased;_= &mut writing=>panic!("owned writer unexpectedly completed"),_=tokio::task::yield_now()=>{}}
    // One byte has actually entered the owned transport; it is not retractable.
    // Mutate the SAME original working set while the next write is pending.
    let (owner, u) = app.unlocked().await.unwrap();
    u.working().clear();
    drop(u);
    drop(owner);
    let mut byte = [0];
    assert_eq!(reader.read(&mut byte).await.unwrap(), 1);
    assert_eq!(&byte, b"a");
    assert!(writing.await.is_err());
    drop(writer);
    assert_eq!(
        reader.read(&mut byte).await.unwrap(),
        0,
        "no second byte after original version invalidation"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pending_guarded_io_releases_original_loaded_owners_before_pending() {
    let (_root, app, scope, budget) = original().await;
    let io = OriginalIo::new(&scope, &budget, expiry(&scope));
    let (owner, unlock, identity) = {
        let owner = app.session().await.unwrap();
        let unlock = owner.unlocked().await.unwrap();
        let identity = owner.identity();
        (
            Arc::downgrade(&owner),
            Arc::downgrade(&unlock),
            Arc::downgrade(&identity),
        )
    };
    let (mut writer, _reader) = tokio::io::duplex(1);
    let mut writing =
        Box::pin(io.run(async { writer.write_all(b"ab").await.map_err(|_| denied()) }));
    tokio::select! {biased;_= &mut writing=>panic!("owned writer unexpectedly completed"),_=tokio::task::yield_now()=>{}}
    drop(app);
    assert!(
        owner.upgrade().is_none() && unlock.upgrade().is_none() && identity.upgrade().is_none()
    );
    assert!(tokio::time::timeout(Duration::from_secs(2), &mut writing)
        .await
        .unwrap()
        .is_err());
}

struct CountIo<S> {
    inner: S,
    written: Arc<std::sync::atomic::AtomicUsize>,
}
impl<S: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for CountIo<S> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}
impl<S: tokio::io::AsyncWrite + Unpin> tokio::io::AsyncWrite for CountIo<S> {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let r = std::pin::Pin::new(&mut self.inner).poll_write(cx, bytes);
        if let std::task::Poll::Ready(Ok(n)) = &r {
            self.written.fetch_add(*n, Ordering::SeqCst);
        }
        r
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pending_actual_tls_read_can_emit_alert_but_retired_original_blocks_that_poll() {
    for retire in [false, true] {
        let (_root, app, scope, budget) = original().await;
        let io = OriginalIo::new(&scope, &budget, expiry(&scope));
        let (a, b) = tokio::io::duplex(65536);
        let written = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (mut client, mut server) = tls_pair_on(
            a,
            CountIo {
                inner: b,
                written: written.clone(),
            },
        )
        .await;
        let before = written.load(Ordering::SeqCst);
        let mut byte = [0];
        let mut reading = Box::pin(io.read_live(&mut client, &mut byte));
        tokio::select! {biased;_=&mut reading=>panic!("owned actual TLS read unexpectedly ready"),_=tokio::task::yield_now()=>{}}
        if retire {
            let (owner, u) = app.unlocked().await.unwrap();
            u.working().clear();
            drop(u);
            drop(owner);
        }
        // Genuine malformed encrypted TLS1.3 record. Pinned tokio-rustls read_io
        // attempts a fatal-alert write from process_new_packets on this input.
        server
            .get_mut()
            .0
            .write_all(&[23, 3, 3, 0, 1, 0])
            .await
            .unwrap();
        server.get_mut().0.flush().await.unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(2), reading)
            .await
            .unwrap()
            .is_err());
        let after = written.load(Ordering::SeqCst);
        if retire {
            assert_eq!(
                after, before,
                "no actual carrier/control write after source retirement"
            );
        } else {
            assert!(
                after > before,
                "control proves this actual TLS read path can emit encrypted alert traffic"
            );
        }
        drop(client);
        drop(server);
        app.shutdown().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn guarded_read_waits_past_five_second_write_budget_until_original_idle() {
    let (_root, app, scope, budget) = original().await;
    let io = OriginalIo::new(&scope, &budget, expiry(&scope));
    let (mut client, mut server) = tls_pair().await;
    let mut byte = [0];
    let (read, ()) = tokio::join!(io.read_live(&mut client, &mut byte), async {
        tokio::time::sleep(Duration::from_millis(5100)).await;
        server.write_all(b"x").await.unwrap();
        server.flush().await.unwrap();
    });
    assert_eq!(read.unwrap(), 1);
    assert_eq!(&byte, b"x");
    drop(client);
    drop(server);
    app.shutdown().await.unwrap();
}

// Test-only maintained WebSocket byte carrier over ACTUAL outer TLS1.3.
// No HTTP/Relay admission, Node authority, join witness or production adapter.
struct WssIo<S> {
    socket: tokio_tungstenite::WebSocketStream<S>,
    pending: Option<(Vec<u8>, usize)>,
    shutdowns: Arc<std::sync::atomic::AtomicUsize>,
}
impl<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin> tokio::io::AsyncRead for WssIo<S> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        out: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use futures::Stream;
        use tokio_tungstenite::tungstenite::Message;
        loop {
            if let Some((bytes, pos)) = &mut self.pending {
                let n = out.remaining().min(bytes.len() - *pos);
                out.put_slice(&bytes[*pos..*pos + n]);
                *pos += n;
                if *pos == bytes.len() {
                    self.pending = None;
                }
                return std::task::Poll::Ready(Ok(()));
            }
            match std::pin::Pin::new(&mut self.socket).poll_next(cx) {
                std::task::Poll::Pending => return std::task::Poll::Pending,
                std::task::Poll::Ready(Some(Ok(Message::Binary(bytes)))) => {
                    assert!(bytes.len() <= 16384);
                    self.pending = Some((bytes.to_vec(), 0));
                }
                std::task::Poll::Ready(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => {}
                std::task::Poll::Ready(None | Some(Ok(Message::Close(_)))) => {
                    return std::task::Poll::Ready(Ok(()))
                }
                _ => return std::task::Poll::Ready(Err(std::io::Error::other("owned WSS failed"))),
            }
        }
    }
}
impl<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin> tokio::io::AsyncWrite for WssIo<S> {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        use futures::Sink;
        match std::pin::Pin::new(&mut self.socket).poll_ready(cx) {
            std::task::Poll::Pending => return std::task::Poll::Pending,
            std::task::Poll::Ready(Err(_)) => {
                return std::task::Poll::Ready(Err(std::io::Error::other("owned WSS failed")))
            }
            _ => {}
        }
        let n = bytes.len().min(16384);
        let r = std::pin::Pin::new(&mut self.socket).start_send(
            tokio_tungstenite::tungstenite::Message::Binary(bytes[..n].to_vec().into()),
        );
        std::task::Poll::Ready(
            r.map(|_| n)
                .map_err(|_| std::io::Error::other("owned WSS failed")),
        )
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use futures::Sink;
        std::pin::Pin::new(&mut self.socket)
            .poll_flush(cx)
            .map_err(|_| std::io::Error::other("owned WSS failed"))
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use futures::Sink;
        self.shutdowns.fetch_add(1, Ordering::SeqCst);
        std::pin::Pin::new(&mut self.socket)
            .poll_close(cx)
            .map_err(|_| std::io::Error::other("owned WSS failed"))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_tls_stop_flush_keeps_actual_wss_alive_until_peer_termination() {
    use futures::{SinkExt, StreamExt};
    use tokio_tungstenite::{
        tungstenite::{protocol::Role, Message},
        WebSocketStream,
    };
    let (_root, app, scope, budget) = original().await;
    let io = OriginalIo::new(&scope, &budget, expiry(&scope));
    let stop = OriginalStop::new(scope.clone());
    let (outer_client, outer_server) = tls_pair().await;
    let (client_ws, server_ws) = tokio::join!(
        WebSocketStream::from_raw_socket(
            outer_client,
            Role::Client,
            Some(
                tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
                    .max_frame_size(Some(16384))
                    .max_message_size(Some(16384))
                    .write_buffer_size(0)
                    .max_write_buffer_size(17408)
            )
        ),
        WebSocketStream::from_raw_socket(
            outer_server,
            Role::Server,
            Some(
                tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
                    .max_frame_size(Some(16384))
                    .max_message_size(Some(16384))
                    .write_buffer_size(0)
                    .max_write_buffer_size(17408)
            )
        )
    );
    let shutdowns = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (mut client, mut server) = tls_pair_on(
        WssIo {
            socket: server_ws,
            pending: None,
            shutdowns: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        },
        WssIo {
            socket: client_ws,
            pending: None,
            shutdowns: shutdowns.clone(),
        },
    )
    .await;
    scope
        .with_loaded(&app, |_| stop.request_held())
        .await
        .unwrap();
    let (alive_tx, alive_rx) = tokio::sync::oneshot::channel();
    let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
    let peer = tokio::spawn(async move {
        let mut byte = [0];
        assert_eq!(
            server.read(&mut byte).await.unwrap(),
            0,
            "actual authenticated close_notify received"
        );
        let socket = &mut server.get_mut().0.socket;
        socket
            .send(Message::Ping(b"owned-parent-alive".to_vec().into()))
            .await
            .unwrap();
        let pong = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(pong,Message::Pong(ref bytes)if bytes.as_ref()==b"owned-parent-alive"));
        alive_tx.send(()).unwrap();
        finish_rx.await.unwrap();
        socket.close(None).await.unwrap();
    });
    let mut sending = Box::pin(stop.send_request(&app, &mut client, &io));
    tokio::select! {biased;r=&mut sending=>panic!("request must retain live WSS until peer end: {r}"),r=alive_rx=>r.unwrap()}
    assert_eq!(
        shutdowns.load(Ordering::SeqCst),
        0,
        "no client TLS.poll_shutdown/WSSclose"
    );
    finish_tx.send(()).unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(2), sending)
        .await
        .unwrap());
    tokio::time::timeout(Duration::from_secs(2), peer)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(shutdowns.load(Ordering::SeqCst), 0);
    drop(client);
    app.shutdown().await.unwrap();
    // This is carrier-lifetime evidence only, never actual Agent joins/CAS.
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_stop_silent_peer_has_one_fixed_post_flush_cleanup_deadline() {
    let (_root, app, scope, budget) = original().await;
    let io = OriginalIo::new(&scope, &budget, expiry(&scope));
    let stop = OriginalStop::new(scope.clone());
    let (mut client, mut server) = tls_pair().await;
    scope
        .with_loaded(&app, |_| stop.request_held())
        .await
        .unwrap();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let peer = tokio::spawn(async move {
        let mut byte = [0];
        assert_eq!(server.read(&mut byte).await.unwrap(), 0);
        release_rx.await.unwrap();
        drop(server);
    });
    let started = std::time::Instant::now();
    assert!(
        !stop.send_request(&app, &mut client, &io).await,
        "silent peer is Unknown, not stopped/drained"
    );
    assert!(started.elapsed() >= Duration::from_millis(4800));
    assert!(started.elapsed() < Duration::from_secs(7));
    assert!(
        !peer.is_finished(),
        "carrier peer is still owned/open at cleanup timeout"
    );
    drop(client);
    release_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), peer)
        .await
        .unwrap()
        .unwrap();
    app.shutdown().await.unwrap();
}
