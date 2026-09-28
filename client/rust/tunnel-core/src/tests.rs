//! Unit tests with a fake transport: "direct-tcpip" = plain TCP connect,
//! "tcpip-forward" = a local listener standing in for the remote side.

use super::*;
use async_trait::async_trait;
use cc_ssh_core::proxy::socks5_connect;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, Notify};

#[derive(Default)]
struct FakeTransport {
    closed: Notify,
    is_closed: std::sync::atomic::AtomicBool,
    cancelled: Mutex<Vec<u16>>,
}

impl FakeTransport {
    fn close(&self) {
        self.is_closed.store(true, Ordering::SeqCst);
        self.closed.notify_waiters();
    }
}

#[async_trait]
impl TunnelTransport for FakeTransport {
    async fn open_direct(
        &self,
        host: &str,
        port: u16,
        _o: SocketAddr,
    ) -> Result<BoxStream, TransportError> {
        let s = TcpStream::connect(bind_addr(host, port))
            .await
            .map_err(|e| TransportError(format!("ConnectFailed: {e}")))?;
        Ok(Box::new(s))
    }
    async fn remote_forward(
        &self,
        bind_host: &str,
        bind_port: u16,
    ) -> Result<RemoteListener, TransportError> {
        let l = TcpListener::bind(bind_addr(bind_host, bind_port))
            .await
            .map_err(|e| TransportError(e.to_string()))?;
        let port = l.local_addr().unwrap().port();
        let (tx, rx) = mpsc::channel(8);
        tokio::spawn(async move {
            while let Ok((s, peer)) = l.accept().await {
                let c = IncomingConnection {
                    stream: Box::new(s),
                    originator: (peer.ip().to_string(), peer.port()),
                };
                if tx.send(c).await.is_err() {
                    break;
                }
            }
        });
        Ok(RemoteListener {
            bound_port: port,
            incoming: rx,
        })
    }
    async fn cancel_remote_forward(&self, _h: &str, port: u16) -> Result<(), TransportError> {
        self.cancelled.lock().unwrap().push(port);
        Ok(())
    }
    async fn closed(&self) {
        if self.is_closed.load(Ordering::SeqCst) {
            return;
        }
        self.closed.notified().await
    }
    fn is_closed(&self) -> bool {
        self.is_closed.load(Ordering::SeqCst)
    }
}

async fn echo_server() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = l.accept().await {
            tokio::spawn(async move {
                let (mut r, mut w) = s.split();
                let _ = tokio::io::copy(&mut r, &mut w).await;
            });
        }
    });
    port
}

fn tunnel(kind: TunnelKind, bind: &str, target: Option<(&str, u16)>) -> Tunnel {
    let now = chrono::Utc::now();
    Tunnel {
        id: ObjectId::new(),
        name: format!("{kind:?}"),
        kind,
        host_id: ObjectId::new(),
        bind_host: bind.into(),
        bind_port: 0,
        target_host: target.map(|t| t.0.to_string()),
        target_port: target.map(|t| t.1),
        auto_start: false,
        created_at: now,
        updated_at: now,
    }
}

async fn roundtrip(s: &mut TcpStream, msg: &[u8]) {
    s.write_all(msg).await.unwrap();
    let mut buf = vec![0u8; msg.len()];
    s.read_exact(&mut buf).await.unwrap();
    assert_eq!(buf, msg);
}

async fn eventually(f: impl Fn() -> bool) {
    for _ in 0..100 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("condition not reached");
}

#[tokio::test]
async fn local_forward_roundtrip_and_counters() {
    let echo = echo_server().await;
    let m = TunnelManager::new();
    let t = tunnel(TunnelKind::Local, "127.0.0.1", Some(("127.0.0.1", echo)));
    let st = m
        .start(&t, Arc::new(FakeTransport::default()))
        .await
        .unwrap();
    assert_eq!(st.state, TunnelState::Running);
    assert!(st.warnings.is_empty());
    let mut c = TcpStream::connect(&st.listen).await.unwrap();
    roundtrip(&mut c, b"hello tunnel").await;
    eventually(|| m.status(t.id).unwrap().active_connections == 1).await;
    drop(c);
    eventually(|| m.status(t.id).unwrap().active_connections == 0).await;
    let s = m.status(t.id).unwrap();
    assert_eq!(s.bytes_sent, 12);
    assert_eq!(s.bytes_received, 12);
    assert_eq!(s.total_connections, 1);
    assert!(matches!(
        m.start(&t, Arc::new(FakeTransport::default())).await,
        Err(TunnelError::AlreadyRunning(_))
    ));

    m.stop(t.id).await.unwrap();
    assert!(m.status(t.id).is_none());
    assert!(
        TcpStream::connect(&st.listen).await.is_err(),
        "listener closed"
    );
    assert!(matches!(
        m.stop(t.id).await,
        Err(TunnelError::NotRunning(_))
    ));
}

#[tokio::test]
async fn local_forward_counts_failed_targets() {
    let dead = {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap().port()
    };
    let m = TunnelManager::new();
    let t = tunnel(TunnelKind::Local, "127.0.0.1", Some(("127.0.0.1", dead)));
    let st = m
        .start(&t, Arc::new(FakeTransport::default()))
        .await
        .unwrap();
    let mut c = TcpStream::connect(&st.listen).await.unwrap();
    let mut b = [0u8; 1];
    assert_eq!(c.read(&mut b).await.unwrap_or(0), 0);
    eventually(|| m.status(t.id).unwrap().failed_connections == 1).await;
}

#[tokio::test]
async fn dynamic_socks5_ipv4_domain_ipv6() {
    let echo = echo_server().await;
    let m = TunnelManager::new();
    let t = tunnel(TunnelKind::Dynamic, "127.0.0.1", None);
    let st = m
        .start(&t, Arc::new(FakeTransport::default()))
        .await
        .unwrap();

    for host in ["127.0.0.1", "localhost"] {
        let mut c = TcpStream::connect(&st.listen).await.unwrap();
        socks5_connect(&mut c, None, host, echo).await.unwrap();
        roundtrip(&mut c, b"socks!").await;
    }
    // IPv6 when the host supports it.
    if let Ok(l6) = TcpListener::bind("[::1]:0").await {
        let p6 = l6.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut s, _) = l6.accept().await.unwrap();
            let (mut r, mut w) = s.split();
            let _ = tokio::io::copy(&mut r, &mut w).await;
        });
        let mut c = TcpStream::connect(&st.listen).await.unwrap();
        socks5_connect(&mut c, None, "::1", p6).await.unwrap();
        roundtrip(&mut c, b"v6").await;
    }
    // refused target → SOCKS error reply
    let dead = {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap().port()
    };
    let mut c = TcpStream::connect(&st.listen).await.unwrap();
    let err = socks5_connect(&mut c, None, "127.0.0.1", dead)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("refused"), "{err}");
    eventually(|| m.status(t.id).unwrap().failed_connections >= 1).await;
}

#[tokio::test]
async fn remote_forward_delivers_to_local_target() {
    let echo = echo_server().await;
    let m = TunnelManager::new();
    let tr = Arc::new(FakeTransport::default());
    let t = tunnel(TunnelKind::Remote, "127.0.0.1", Some(("127.0.0.1", echo)));
    let st = m.start(&t, tr.clone()).await.unwrap();
    // `listen` is the "remote" side
    let mut c = TcpStream::connect(&st.listen).await.unwrap();
    roundtrip(&mut c, b"from remote").await;
    let port: u16 = st.listen.rsplit(':').next().unwrap().parse().unwrap();
    m.stop(t.id).await.unwrap();
    assert_eq!(*tr.cancelled.lock().unwrap(), vec![port]);
}

#[tokio::test]
async fn transport_close_fails_tunnel() {
    let m = TunnelManager::new();
    let mut events = m.events();
    let tr = Arc::new(FakeTransport::default());
    let t = tunnel(TunnelKind::Dynamic, "127.0.0.1", None);
    m.start(&t, tr.clone()).await.unwrap();
    assert_eq!(events.recv().await.unwrap(), TunnelEvent::Started(t.id));
    tr.close();
    assert!(matches!(
        events.recv().await.unwrap(),
        TunnelEvent::Failed { .. }
    ));
    assert!(matches!(
        m.status(t.id).unwrap().state,
        TunnelState::Failed(_)
    ));
    m.stop_all().await;
    assert!(m.list().is_empty());
}

#[tokio::test]
async fn warns_on_public_binds_and_rejects_invalid() {
    let m = TunnelManager::new();
    let t = tunnel(TunnelKind::Dynamic, "0.0.0.0", None);
    let st = m
        .start(&t, Arc::new(FakeTransport::default()))
        .await
        .unwrap();
    assert_eq!(st.warnings.len(), 1);
    assert!(st.warnings[0].contains("reachable from the network"));
    m.stop(t.id).await.unwrap();

    assert!(is_loopback_host("127.0.0.2"));
    assert!(is_loopback_host("[::1]"));
    assert!(is_loopback_host("LOCALHOST"));
    assert!(!is_loopback_host("::"));
    assert!(!is_loopback_host("192.168.1.1"));

    let bad = tunnel(TunnelKind::Local, "127.0.0.1", None);
    assert!(matches!(
        m.start(&bad, Arc::new(FakeTransport::default())).await,
        Err(TunnelError::Invalid(_))
    ));
    // bind conflict
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut t = tunnel(TunnelKind::Dynamic, "127.0.0.1", None);
    t.bind_port = l.local_addr().unwrap().port();
    assert!(matches!(
        m.start(&t, Arc::new(FakeTransport::default())).await,
        Err(TunnelError::Bind { .. })
    ));
}
