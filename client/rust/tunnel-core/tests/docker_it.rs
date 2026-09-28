//! Tunnel integration tests over a real 2-hop jump chain (Docker).
//! Run with `CC_SSH_IT=1 cargo test -p cc-tunnel-core --test docker_it -- --nocapture`.

use cc_models::tunnel::{Tunnel, TunnelKind};
use cc_models::ObjectId;
use cc_ssh_core::proxy::socks5_connect;
use cc_ssh_core::testing::{self, connector, SshTestbed};
use cc_ssh_core::{ConnectionPlanner, HostKeyDecision, MemoryKnownHosts, SshSession};
use cc_tunnel_core::{TunnelManager, TunnelState, TunnelTransport};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

fn tunnel(kind: TunnelKind, target: Option<(&str, u16)>) -> Tunnel {
    let now = chrono::Utc::now();
    Tunnel {
        id: ObjectId::new(),
        name: format!("it-{kind:?}"),
        kind,
        host_id: ObjectId::new(),
        bind_host: "127.0.0.1".into(),
        bind_port: 0,
        target_host: target.map(|t| t.0.to_string()),
        target_port: target.map(|t| t.1),
        auto_start: false,
        created_at: now,
        updated_at: now,
    }
}

async fn http_get(stream: &mut TcpStream) -> String {
    stream
        .write_all(b"GET /index.html HTTP/1.0\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    let mut out = String::new();
    tokio::time::timeout(Duration::from_secs(10), stream.read_to_string(&mut out))
        .await
        .expect("http timeout")
        .unwrap();
    out
}

async fn echo(stream: &mut TcpStream, msg: &[u8]) {
    stream.write_all(msg).await.unwrap();
    let mut buf = vec![0u8; msg.len()];
    tokio::time::timeout(Duration::from_secs(10), stream.read_exact(&mut buf))
        .await
        .expect("echo timeout")
        .unwrap();
    assert_eq!(buf, msg);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn docker_tunnels_over_jump_chain() {
    if !testing::enabled() {
        eprintln!("skipped: set CC_SSH_IT=1 to run Docker integration tests");
        return;
    }
    let tb = tokio::task::spawn_blocking(|| SshTestbed::start(true))
        .await
        .unwrap()
        .expect("testbed");
    let ti = tb.inventory();
    let (c, _) = connector(
        ti.resolver.clone(),
        Arc::new(MemoryKnownHosts::new()),
        HostKeyDecision::Reject,
    );
    let plan = ConnectionPlanner::new(ti.inventory.clone())
        .plan(ti.target.unwrap())
        .await
        .unwrap();
    let session: Arc<SshSession> = Arc::new(c.connect(&plan).await.unwrap());
    let transport: Arc<dyn TunnelTransport> = session.clone();
    let target_name = tb.target.clone().unwrap();
    let bastion2 = tb.bastion2.clone().unwrap();
    let m = TunnelManager::new();

    // Local: HTTP and echo services of the target (localhost from its view).
    let http = tunnel(TunnelKind::Local, Some(("127.0.0.1", 8080)));
    let st = m.start(&http, transport.clone()).await.unwrap();
    let mut s = TcpStream::connect(&st.listen).await.unwrap();
    let body = http_get(&mut s).await;
    assert!(
        body.contains(&format!("hello from {target_name}")),
        "{body}"
    );
    println!("IT local_forward_http ... ok");

    let echo_t = tunnel(TunnelKind::Local, Some(("127.0.0.1", 7777)));
    let st = m.start(&echo_t, transport.clone()).await.unwrap();
    let mut s = TcpStream::connect(&st.listen).await.unwrap();
    let big: Vec<u8> = (0..256 * 1024u32).map(|i| (i % 251) as u8).collect();
    echo(&mut s, &big).await;
    drop(s);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let status = m.status(echo_t.id).unwrap();
    assert_eq!(status.bytes_sent, big.len() as u64);
    assert_eq!(status.bytes_received, big.len() as u64);
    println!("IT local_forward_echo_256k ... ok");

    // Dynamic SOCKS5: IPv4 literal and a docker DNS name resolved remotely.
    let socks = tunnel(TunnelKind::Dynamic, None);
    let st = m.start(&socks, transport.clone()).await.unwrap();
    let mut s = TcpStream::connect(&st.listen).await.unwrap();
    socks5_connect(&mut s, None, "127.0.0.1", 8080)
        .await
        .unwrap();
    assert!(http_get(&mut s).await.contains(&target_name));
    let mut s = TcpStream::connect(&st.listen).await.unwrap();
    socks5_connect(&mut s, None, &bastion2, 8080).await.unwrap();
    let body = http_get(&mut s).await;
    assert!(body.contains(&format!("hello from {bastion2}")), "{body}");
    let mut s = TcpStream::connect(&st.listen).await.unwrap();
    let err = socks5_connect(&mut s, None, "127.0.0.1", 1)
        .await
        .unwrap_err();
    println!("IT dynamic_socks5 ... ok (refused target -> {err})");

    // Remote: target 127.0.0.1:<allocated> -> local echo server here.
    let local_echo = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let lport = local_echo.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = local_echo.accept().await {
            tokio::spawn(async move {
                let (mut r, mut w) = s.split();
                let _ = tokio::io::copy(&mut r, &mut w).await;
            });
        }
    });
    let remote = tunnel(TunnelKind::Remote, Some(("127.0.0.1", lport)));
    let st = m.start(&remote, transport.clone()).await.unwrap();
    let rport: u16 = st.listen.rsplit(':').next().unwrap().parse().unwrap();
    assert_ne!(rport, 0);
    // (a) from inside the target via exec
    let out = session
        .exec(&format!("echo ping-remote | nc -w 3 127.0.0.1 {rport}"))
        .await
        .unwrap();
    assert_eq!(
        out.stdout_lossy().trim(),
        "ping-remote",
        "stderr: {}",
        out.stderr_lossy()
    );
    // (b) local forward to the remote-forwarded port closes the loop
    let loop_t = tunnel(TunnelKind::Local, Some(("127.0.0.1", rport)));
    let st2 = m.start(&loop_t, transport.clone()).await.unwrap();
    let mut s = TcpStream::connect(&st2.listen).await.unwrap();
    echo(&mut s, b"full circle").await;
    println!("IT remote_forward ... ok (remote port {rport})");

    // Stop remote forward: the port is released on the server.
    m.stop(remote.id).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let out = session
        .exec(&format!(
            "nc -z -w 2 127.0.0.1 {rport} && echo open || echo closed"
        ))
        .await
        .unwrap();
    assert_eq!(out.stdout_lossy().trim(), "closed");
    println!("IT remote_forward_cancel ... ok");

    // Session loss → tunnels fail.
    session.disconnect().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if matches!(m.status(socks.id).unwrap().state, TunnelState::Failed(_)) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("tunnel should fail after disconnect");
    println!("IT tunnel_fails_on_disconnect ... ok");
    m.stop_all().await;
    drop(tb);
}
