use super::*;
use cc_models::host::{HostKeyPolicy, SshBackend};
use cc_ssh_core::{Endpoint, ShellInput};
use std::sync::Mutex as StdMutex;
use std::time::Duration;

/// Fake remote shell: echoes input, answers `exit` with status 0, records resizes.
struct EchoOpener {
    resizes: Arc<StdMutex<Vec<(u32, u32)>>>,
    fail: bool,
}

#[async_trait]
impl ShellOpener for EchoOpener {
    async fn open_shell(
        &self,
        _plan: &ConnectionPlan,
        pty: PtyRequest,
    ) -> Result<OpenedShell, TerminalError> {
        if self.fail {
            return Err(TerminalError::Open("host key CHANGED".into()));
        }
        assert_eq!(pty.term, "xterm-256color");
        let (channel, mut remote) = ShellChannel::pair(64);
        let resizes = self.resizes.clone();
        tokio::spawn(async move {
            let _ = remote
                .events
                .send(ShellEvent::Data(Bytes::from_static(
                    b"welcome\r\nuser@host:~$ ",
                )))
                .await;
            while let Some(input) = remote.inputs.recv().await {
                match input {
                    ShellInput::Data(d) => {
                        let text = String::from_utf8_lossy(&d).to_string();
                        let _ = remote.events.send(ShellEvent::Data(d)).await;
                        if text.contains("fail\r") {
                            let _ = remote
                                .events
                                .send(ShellEvent::Data(Bytes::from_static(
                                    b"\r\nbash: fail: command not found\r\nuser@host:~$ ",
                                )))
                                .await;
                        }
                        if text.contains("exit") {
                            let _ = remote.events.send(ShellEvent::ExitStatus(0)).await;
                            let _ = remote.events.send(ShellEvent::Closed).await;
                            return;
                        }
                    }
                    ShellInput::Resize { cols, rows } => resizes.lock().unwrap().push((cols, rows)),
                    ShellInput::Close => {
                        let _ = remote.events.send(ShellEvent::Closed).await;
                        return;
                    }
                    ShellInput::Eof => {}
                }
            }
        });
        Ok(OpenedShell {
            channel,
            session: None,
            lifetime: None,
        })
    }
}

fn plan() -> ConnectionPlan {
    ConnectionPlan {
        host_id: ObjectId::new(),
        name: "db".into(),
        target: Endpoint::new("10.0.0.1", 22),
        username: "user".into(),
        credential: None,
        host_key_policy: HostKeyPolicy::Ask,
        route: vec![],
        proxy: None,
        forwards: vec![],
        backend: SshBackend::Native,
        keepalive_secs: None,
        agent_forwarding: false,
        warnings: vec![],
    }
}

type Resizes = Arc<StdMutex<Vec<(u32, u32)>>>;

fn manager(fail: bool) -> (TerminalManager, Resizes) {
    let resizes = Arc::new(StdMutex::new(Vec::new()));
    let m = TerminalManager::with_config(
        Arc::new(EchoOpener {
            resizes: resizes.clone(),
            fail,
        }),
        TerminalConfig {
            scrollback_bytes: 1024,
            ..Default::default()
        },
    );
    (m, resizes)
}

async fn read_until(rx: &mut broadcast::Receiver<Bytes>, needle: &str) -> String {
    let mut acc = String::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !acc.contains(needle) {
            let b = rx.recv().await.unwrap();
            acc.push_str(&String::from_utf8_lossy(&b));
        }
    })
    .await
    .expect("timeout");
    acc
}

#[tokio::test]
async fn open_write_resize_attach_and_exit() {
    let (m, resizes) = manager(false);
    let mut events = m.events();
    let id = m
        .open(
            plan(),
            TerminalSize {
                cols: 100,
                rows: 30,
            },
        )
        .await
        .unwrap();
    assert_eq!(m.status(id).unwrap(), TerminalStatus::Connected);
    assert_eq!(
        events.recv().await.unwrap().status,
        TerminalStatus::Connecting
    );
    assert_eq!(
        events.recv().await.unwrap().status,
        TerminalStatus::Connected
    );

    let mut att = m.attach(id).unwrap();
    m.write(id, b"ls -la\r").await.unwrap();
    let out = read_until(&mut att.output, "ls -la").await;
    assert!(out.contains("ls -la"));
    // late attach sees the welcome banner in the snapshot
    let late = m.attach(id).unwrap();
    let snap = String::from_utf8_lossy(&late.snapshot);
    assert!(snap.contains("welcome"), "{snap}");
    assert!(snap.contains("ls -la"), "{snap}");

    m.resize(
        id,
        TerminalSize {
            cols: 120,
            rows: 40,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        m.info(id).unwrap().size,
        TerminalSize {
            cols: 120,
            rows: 40
        }
    );

    assert_eq!(m.last_command(id).as_deref(), Some("ls -la"));
    m.write(id, b"fail\r").await.unwrap();
    read_until(&mut att.output, "command not found").await;
    assert_eq!(m.last_command(id).as_deref(), Some("fail"));
    assert_eq!(
        m.last_error(id).as_deref(),
        Some("$ fail\nbash: fail: command not found")
    );

    m.write(id, b"exit\r").await.unwrap();
    let st = m.wait_closed(id).await.unwrap();
    assert_eq!(
        st,
        TerminalStatus::Closed {
            exit_status: Some(0),
            reason: None
        }
    );
    assert_eq!(*resizes.lock().unwrap(), vec![(100, 30), (120, 40)]);
    assert!(matches!(
        m.write(id, b"x").await,
        Err(TerminalError::NotConnected(_))
    ));
    assert_eq!(m.list().len(), 1);
    m.remove(id).await.unwrap();
    assert!(m.list().is_empty());
    assert!(matches!(m.status(id), Err(TerminalError::NotFound(_))));
}

#[tokio::test]
async fn failed_open_reports_error() {
    let (m, _) = manager(true);
    let err = m.open(plan(), TerminalSize::default()).await.unwrap_err();
    assert!(err.to_string().contains("CHANGED"));
    // spawn_open keeps the failed session listed for the UI
    let id = m.spawn_open(plan(), TerminalSize::default());
    let st = m.wait_closed(id).await.unwrap();
    assert!(matches!(st, TerminalStatus::Failed(_)));
}

#[tokio::test]
async fn close_by_user() {
    let (m, _) = manager(false);
    let id = m.open(plan(), TerminalSize::default()).await.unwrap();
    m.close(id).await.unwrap();
    let st = m.wait_closed(id).await.unwrap();
    assert!(
        matches!(
            st,
            TerminalStatus::Closed {
                exit_status: None,
                ..
            }
        ),
        "{st:?}"
    );
}

#[tokio::test]
async fn scrollback_is_bounded() {
    let (m, _) = manager(false);
    let id = m.open(plan(), TerminalSize::default()).await.unwrap();
    let mut att = m.attach(id).unwrap();
    let line = "x".repeat(100) + "\r";
    for _ in 0..30 {
        m.write(id, line.as_bytes()).await.unwrap();
    }
    m.write(id, b"END\r").await.unwrap();
    read_until(&mut att.output, "END").await;
    let snap = m.attach(id).unwrap().snapshot;
    assert!(snap.len() <= 1024);
    assert!(snap.ends_with(b"END\r"));
    assert_eq!(m.scrollback_tail(id, 4).unwrap(), b"END\r");
}

#[tokio::test]
async fn final_status_is_stored_without_subscribers() {
    // Regression: watch::Sender::send drops values when nobody listens.
    let (m, _) = manager(false);
    let id = m.spawn_open(plan(), TerminalSize::default());
    for _ in 0..100 {
        if m.status(id).unwrap() == TerminalStatus::Connected {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    m.write(id, b"exit\r").await.unwrap();
    for _ in 0..100 {
        if m.status(id).unwrap().is_terminal() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("status never became terminal: {:?}", m.status(id));
}

struct DelayedOpener {
    release: Arc<tokio::sync::Notify>,
    closed: Arc<AtomicBool>,
    resizes: Resizes,
}

struct TestLifetime(Arc<AtomicBool>);
impl ShellLifetime for TestLifetime {
    fn close(&self) {
        self.0.store(true, Ordering::Release);
    }
}

#[async_trait]
impl ShellOpener for DelayedOpener {
    async fn open_shell(
        &self,
        _plan: &ConnectionPlan,
        _pty: PtyRequest,
    ) -> Result<OpenedShell, TerminalError> {
        self.release.notified().await;
        let (channel, mut remote) = ShellChannel::pair(64);
        let resizes = self.resizes.clone();
        let closed = self.closed.clone();
        tokio::spawn(async move {
            while let Some(input) = remote.inputs.recv().await {
                match input {
                    ShellInput::Resize { cols, rows } => resizes.lock().unwrap().push((cols, rows)),
                    ShellInput::Close => {
                        closed.store(true, Ordering::Release);
                        break;
                    }
                    _ => {}
                }
            }
        });
        Ok(OpenedShell {
            channel,
            session: None,
            lifetime: Some(Arc::new(TestLifetime(self.closed.clone()))),
        })
    }
}

fn delayed_manager() -> (
    TerminalManager,
    Arc<tokio::sync::Notify>,
    Arc<AtomicBool>,
    Resizes,
) {
    let release = Arc::new(tokio::sync::Notify::new());
    let closed = Arc::new(AtomicBool::new(false));
    let resizes = Arc::new(StdMutex::new(Vec::new()));
    let manager = TerminalManager::new(Arc::new(DelayedOpener {
        release: release.clone(),
        closed: closed.clone(),
        resizes: resizes.clone(),
    }));
    (manager, release, closed, resizes)
}

#[tokio::test]
async fn close_during_open_closes_late_native_process_and_never_reconnects() {
    let (manager, release, closed, _) = delayed_manager();
    let id = manager.spawn_open(plan(), TerminalSize::default());
    manager.close(id).await.unwrap();
    release.notify_one();
    tokio::time::timeout(Duration::from_secs(2), async {
        while !closed.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(manager.status(id).unwrap().is_terminal());
    assert!(manager.write(id, b"must not run\r").await.is_err());
}

#[tokio::test]
async fn pending_resize_is_installed_and_shutdown_fences_late_opens() {
    let (manager, release, _, resizes) = delayed_manager();
    let id = manager.spawn_open(plan(), TerminalSize::default());
    manager
        .resize(
            id,
            TerminalSize {
                cols: 137,
                rows: 49,
            },
        )
        .await
        .unwrap();
    release.notify_one();
    tokio::time::timeout(Duration::from_secs(2), async {
        while resizes.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(*resizes.lock().unwrap(), vec![(137, 49)]);
    manager.shutdown().await;
    let late = manager.spawn_open(plan(), TerminalSize::default());
    assert!(manager.status(late).unwrap().is_terminal());
    assert!(manager
        .open_local("macos:zsh", TerminalSize::default())
        .await
        .is_err());
}

#[tokio::test]
async fn shutdown_between_preparation_and_registration_cannot_publish_connected() {
    let (manager, _) = manager(false);
    // Models the shared-session open path after SSH returned its channel but
    // before registration. The old closing=false snapshot is intentionally stale.
    let session = Arc::new(Session {
        id: uuid::Uuid::new_v4(),
        target: TerminalTarget::Ssh {
            host_id: ObjectId::new(),
        },
        title: "fixture".into(),
        status: watch::channel(TerminalStatus::Connecting).0,
        output: broadcast::channel(1).0,
        inner: Mutex::new(Inner {
            writer: None,
            session: None,
            lifetime: None,
            closing: false,
            size: TerminalSize::default(),
            scrollback: Scrollback::new(1024),
            tracker: CommandTracker::new(),
        }),
    });
    let (channel, mut remote) = ShellChannel::pair(1);
    let closed = Arc::new(AtomicBool::new(false));
    manager.begin_shutdown();
    manager.register(session.clone());
    let installed = TerminalManager::install(
        &session,
        OpenedShell {
            channel,
            session: None,
            lifetime: Some(Arc::new(TestLifetime(closed.clone()))),
        },
        &manager.events,
    )
    .await;
    assert!(installed.is_none());
    assert!(closed.load(Ordering::Acquire));
    assert!(matches!(
        remote.inputs.recv().await,
        Some(ShellInput::Close)
    ));
    assert!(manager.status(session.id).unwrap().is_terminal());
    assert!(manager.write(session.id, b"must not run\r").await.is_err());
}
