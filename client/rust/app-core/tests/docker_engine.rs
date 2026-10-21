//! Docker section through app-core: the local engine and a saved SSH host
//! whose `docker.sock` is forwarded over `direct-streamlocal`. Gated by
//! `CC_SSH_IT=1` (needs Docker):
//!
//! ```sh
//! CC_SSH_IT=1 cargo test -p cc-app-core --test docker_engine -- --nocapture
//! ```

mod common;

use cc_app_core::*;
use cc_models::host::HostKeyPolicy;
use cc_ssh_core::testing::{self, SshTestbed};
use common::*;
use secrecy::ExposeSecret;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn docker_over_ssh_reports_missing_rights_then_lists_logs_and_forwards() {
    if !testing::enabled() {
        eprintln!("skipped: set CC_SSH_IT=1 to run Docker integration tests");
        return;
    }
    let tb = tokio::task::spawn_blocking(SshTestbed::start_docker_host)
        .await
        .unwrap()
        .expect("docker host testbed");
    let tmp = tempfile::tempdir().unwrap();
    let app = common::app(&tmp.path().join("app"));
    app.create_local_profile("Docker IT".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let s = &tb.secrets;
    let cred = app
        .import_ssh_key(
            "it key".into(),
            Some(s.user.clone()),
            s.key.private_openssh.expose_secret().to_owned(),
            None,
            false,
            None,
        )
        .await
        .unwrap();
    let mut h = host("docker-host", "127.0.0.1", Some(&cred.id));
    h.port = Some(tb.bastion1_port);
    h.host_key_policy = HostKeyPolicy::AcceptNew;
    let saved = app.save_host(h).await.unwrap();

    let hosts = app.docker_hosts().await.unwrap();
    assert!(hosts.iter().any(|h| h.id == saved.id && h.kind == "ssh"));

    // Without the docker group the socket is refused, with the group named.
    match app.docker_connect(saved.id.clone()).await.unwrap() {
        DockerConnectOutcome::Failed { failure, detail } => {
            assert_eq!(failure, "permission_denied");
            assert_eq!(detail, "docker");
        }
        other => panic!("expected a permission failure, got {other:?}"),
    }
    tb.exec_in(&tb.bastion1, "addgroup tester docker").unwrap();

    let session = match app.docker_connect(saved.id.clone()).await.unwrap() {
        DockerConnectOutcome::Connected(session) => session,
        other => panic!("expected a session, got {other:?}"),
    };
    assert!(!session.engine_version.is_empty());
    let containers = app
        .docker_containers(session.id.clone(), true)
        .await
        .unwrap();
    let me = containers
        .iter()
        .find(|c| c.name == tb.bastion1)
        .expect("the SSH host container is listed");
    assert_eq!(me.state, "running");
    let inspect = app
        .docker_inspect(session.id.clone(), me.id.clone())
        .await
        .unwrap();
    let env = inspect["Config"]["Env"].to_string();
    assert!(env.contains("CC_IT_PASSWORD=••••••"), "{env}");

    // Logs of the host container contain sshd's start line.
    let stream = app
        .docker_logs_start(session.id.clone(), me.id.clone(), 200, false, false)
        .await
        .unwrap();
    let mut lines = Vec::new();
    while let Some(batch) = app.docker_logs_poll(stream.clone(), 1000).await.unwrap() {
        lines.extend(batch);
    }
    assert!(
        lines.iter().any(|l| l.text.contains("Server listening on")),
        "{lines:?}"
    );

    // A shell inside the container as a terminal tab (auto-detected shell).
    let term = app
        .docker_terminal_open(
            session.id.clone(),
            me.id.clone(),
            me.name.clone(),
            vec!["/__consolecrypt_auto_shell__".into()],
            100,
            30,
        )
        .await
        .unwrap();
    let mut output = app.attach_terminal(term.id.clone()).await.unwrap().output;
    app.terminal_write(term.id.clone(), b"echo ctr-$((40+2))-ok\n".to_vec())
        .await
        .unwrap();
    let mut seen = Vec::new();
    let found = tokio::time::timeout(Duration::from_secs(15), async {
        while let Some(chunk) = output.recv().await {
            if let TerminalChunk::Data(d) = chunk {
                seen.extend_from_slice(&d);
                if String::from_utf8_lossy(&seen).contains("ctr-42-ok") {
                    return true;
                }
            }
        }
        false
    })
    .await
    .unwrap_or(false);
    assert!(
        found,
        "container terminal: {}",
        String::from_utf8_lossy(&seen)
    );
    app.terminal_resize(term.id.clone(), 120, 40).await.unwrap();
    app.terminal_write(term.id.clone(), b"exit\n".to_vec())
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(15),
        app.terminal_wait_closed(term.id.clone()),
    )
    .await
    .unwrap()
    .unwrap();

    // Port 8080 of the container through the SSH host.
    let forward = app
        .docker_forward_start(session.id.clone(), me.id.clone(), 8080, 0)
        .await
        .unwrap();
    let mut socket = tokio::net::TcpStream::connect(&forward.address)
        .await
        .unwrap();
    socket
        .write_all(b"GET / HTTP/1.0\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    let mut page = String::new();
    tokio::time::timeout(Duration::from_secs(10), socket.read_to_string(&mut page))
        .await
        .unwrap()
        .unwrap();
    assert!(page.contains("hello from"), "{page}");
    drop(socket);
    // Counters land when the tunnelled connection has finished.
    let mut counted = false;
    for _ in 0..50 {
        let listed = app.docker_forwards().await.unwrap();
        if listed
            .iter()
            .any(|f| f.id == forward.id && f.bytes_received > 0)
        {
            counted = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(counted, "forward counted the transfer");
    app.docker_forward_stop(forward.id.clone()).await.unwrap();
    app.docker_disconnect(session.id.clone()).await.unwrap();
    assert!(app.docker_containers(session.id, true).await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn docker_on_this_computer() {
    if !testing::enabled() {
        eprintln!("skipped: set CC_SSH_IT=1 to run Docker integration tests");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let app = common::app(&tmp.path().join("app"));
    app.create_local_profile("Docker local".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let hosts = app.docker_hosts().await.unwrap();
    assert_eq!(hosts.first().map(|h| h.kind), Some("local"));
    match app.docker_connect("local".into()).await.unwrap() {
        DockerConnectOutcome::Connected(session) => {
            assert!(session.containers_total >= 1);
            // Secrets show only after the vault passphrase.
            let name = "cc-docker-reveal";
            let _ = std::process::Command::new("docker")
                .args(["rm", "-f", name])
                .output();
            let run = std::process::Command::new("docker")
                .args([
                    "run",
                    "-d",
                    "--name",
                    name,
                    "-e",
                    "API_TOKEN=s3cr3t",
                    "-v",
                    "cc-docker-reveal-data:/data",
                    "alpine:3.20",
                    "sleep",
                    "300",
                ])
                .output()
                .unwrap();
            assert!(run.status.success());
            let masked = app
                .docker_inspect(session.id.clone(), name.into())
                .await
                .unwrap();
            assert!(!masked["Config"]["Env"].to_string().contains("s3cr3t"));
            let wrong = app
                .docker_reveal_environment(
                    session.id.clone(),
                    name.into(),
                    "wrong passphrase".into(),
                )
                .await;
            assert!(matches!(wrong, Err(AppError::WrongPassphrase)), "{wrong:?}");
            let env = app
                .docker_reveal_environment(session.id.clone(), name.into(), PASSPHRASE.into())
                .await
                .unwrap();
            assert!(env.iter().any(|e| e == "API_TOKEN=s3cr3t"));
            let volumes = app.docker_volumes(session.id.clone()).await.unwrap();
            assert!(volumes
                .iter()
                .any(|v| v.name == "cc-docker-reveal-data" && v.used_by == [name.to_string()]));
            let _ = std::process::Command::new("docker")
                .args(["rm", "-f", name])
                .output();
            let _ = std::process::Command::new("docker")
                .args(["volume", "rm", "cc-docker-reveal-data"])
                .output();
            app.docker_disconnect(session.id).await.unwrap();
        }
        other => panic!("local engine: {other:?}"),
    }
}
