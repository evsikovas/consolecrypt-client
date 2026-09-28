//! Terminal sessions over a real 2-hop chain (Docker).
//! Run with `CC_SSH_IT=1 cargo test -p cc-terminal-core --test docker_it -- --nocapture`.

use cc_ssh_core::testing::{self, connector, SshTestbed};
use cc_ssh_core::{ConnectOptions, ConnectionPlanner, HostKeyDecision, MemoryKnownHosts};
use cc_terminal_core::{SshShellOpener, TerminalManager, TerminalSize, TerminalStatus};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn docker_terminal_sessions() {
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
    let m = TerminalManager::new(Arc::new(SshShellOpener::new(c)));

    let id = m
        .open(plan.clone(), TerminalSize { cols: 90, rows: 20 })
        .await
        .unwrap();
    let mut att = m.attach(id).unwrap();
    m.write(id, b"cat /does-not-exist\r").await.unwrap();
    m.resize(
        id,
        TerminalSize {
            cols: 132,
            rows: 43,
        },
    )
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    m.write(id, b"stty size; exit 5\r").await.unwrap();
    let status = tokio::time::timeout(Duration::from_secs(10), m.wait_closed(id))
        .await
        .expect("closed")
        .unwrap();
    assert_eq!(
        status,
        TerminalStatus::Closed {
            exit_status: Some(5),
            reason: None
        }
    );
    let mut out = String::from_utf8_lossy(&m.attach(id).unwrap().snapshot).to_string();
    while let Ok(b) = att.output.try_recv() {
        out.push_str(&String::from_utf8_lossy(&b));
    }
    assert!(out.contains("43 132"), "{out}");
    assert_eq!(m.last_command(id).as_deref(), Some("stty size; exit 5"));
    let err = m.last_error(id).unwrap_or_default();
    assert!(err.contains("No such file or directory"), "{err}");
    println!("IT terminal_pty_session ... ok");

    // Connection loss of the first hop → Failed (reconnect UX).
    let (c, _) = connector(
        ti.resolver.clone(),
        Arc::new(MemoryKnownHosts::new()),
        HostKeyDecision::Reject,
    );
    let c = c.with_options(ConnectOptions {
        default_keepalive: Some(Duration::from_secs(1)),
        keepalive_max: 2,
        ..Default::default()
    });
    let m = TerminalManager::new(Arc::new(SshShellOpener::new(c)));
    let started = std::time::Instant::now();
    let id = m.open(plan, TerminalSize::default()).await.unwrap();
    // Kill the server side of hop 1 (the whole chain dies with it).
    tb.exec_in(tb.bastion1.as_str(), "pkill -f '[s]shd: tester' || true")
        .unwrap();
    let status = tokio::time::timeout(Duration::from_secs(15), m.wait_closed(id))
        .await
        .expect("status")
        .unwrap();
    assert!(
        matches!(
            status,
            TerminalStatus::Failed(_) | TerminalStatus::Closed { .. }
        ),
        "{status:?}"
    );
    println!(
        "IT terminal_connection_loss ... ok ({status:?} after {:.1}s)",
        started.elapsed().as_secs_f32()
    );
    drop(tb);
}
