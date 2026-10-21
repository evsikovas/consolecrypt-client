//! Opt-in acceptance against the local engine: CC_DOCKER_LIVE=1. Uses its own
//! throwaway alpine container labelled as a compose service.
#![cfg(unix)]
use cc_docker_core::{local_socket, ContainerAction, DockerClient, UnixConnector};
use std::{sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

fn client() -> Option<DockerClient> {
    std::env::var("CC_DOCKER_LIVE").ok()?;
    Some(DockerClient::new(Arc::new(UnixConnector {
        path: local_socket()?,
    })))
}

#[tokio::test]
async fn list_inspect_logs_exec_and_actions() {
    let Some(docker) = client() else {
        eprintln!("CC_DOCKER_LIVE not set; skipped");
        return;
    };
    let name = "cc-docker-live";
    let _ = std::process::Command::new("docker")
        .args(["rm", "-f", name])
        .output();
    let run = std::process::Command::new("docker")
        .args([
            "run",
            "-d",
            "--name",
            name,
            "--label",
            "com.docker.compose.project=cclive",
            "--label",
            "com.docker.compose.service=worker",
            "-v",
            "cc-docker-live-data:/data",
            "-e",
            "DB_PASSWORD=hunter2",
            "-p",
            "127.0.0.1::8080",
            "alpine:3.20",
            "sh",
            "-c",
            "echo out-line; echo err-line >&2; sleep 600",
        ])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );

    let info = docker.info().await.unwrap();
    assert!(!info.engine_version.is_empty() && info.containers_running >= 1);

    let list = docker.containers(true).await.unwrap();
    let mine = list.iter().find(|c| c.name == name).expect("listed");
    assert_eq!(mine.compose_project.as_deref(), Some("cclive"));
    assert_eq!(mine.compose_service.as_deref(), Some("worker"));
    assert_eq!(mine.state, "running");
    assert!(mine
        .ports
        .iter()
        .any(|p| p.private_port == 8080 && p.public_port.is_some()));

    let inspect = docker.inspect(name).await.unwrap();
    let env = inspect["Config"]["Env"].to_string();
    assert!(
        env.contains("DB_PASSWORD=••••••") && !env.contains("hunter2"),
        "{env}"
    );

    // Logs: both streams, then the follow ends when cancelled.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (tx, mut rx) = mpsc::channel(8);
    let cancel = CancellationToken::new();
    let logs = {
        let docker = docker.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move { docker.logs(name, 100, true, false, tx, cancel).await })
    };
    let mut seen = Vec::new();
    while seen.len() < 2 {
        let batch = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        seen.extend(batch);
    }
    assert!(seen
        .iter()
        .any(|l| l.stream == "stdout" && l.text == "out-line"));
    assert!(seen
        .iter()
        .any(|l| l.stream == "stderr" && l.text == "err-line"));
    cancel.cancel();
    logs.await.unwrap().unwrap();

    // Interactive exec over the hijacked stream.
    let mut exec = docker
        .exec(name, &["/bin/sh".into()], 80, 24)
        .await
        .unwrap();
    docker.resize_exec(&exec.exec_id, 100, 30).await.unwrap();
    exec.io
        .write_all(b"echo cc-$((40+2)); exit\n")
        .await
        .unwrap();
    let mut out = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), exec.io.read_to_end(&mut out)).await;
    assert!(
        String::from_utf8_lossy(&out).contains("cc-42"),
        "{}",
        String::from_utf8_lossy(&out)
    );

    let env = docker.environment(name).await.unwrap();
    assert!(env.iter().any(|e| e == "DB_PASSWORD=hunter2"), "{env:?}");
    let volumes = docker.volumes().await.unwrap();
    let data = volumes
        .iter()
        .find(|v| v.name == "cc-docker-live-data")
        .expect("volume listed");
    assert_eq!(data.used_by, [name.to_string()]);

    docker.action(name, ContainerAction::Stop).await.unwrap();
    let stopped = docker.containers(true).await.unwrap();
    assert_eq!(
        stopped.iter().find(|c| c.name == name).unwrap().state,
        "exited"
    );
    docker.action(name, ContainerAction::Remove).await.unwrap();
    assert!(docker
        .containers(true)
        .await
        .unwrap()
        .iter()
        .all(|c| c.name != name));
    let volumes = docker.volumes().await.unwrap();
    assert!(volumes
        .iter()
        .any(|v| v.name == "cc-docker-live-data" && v.used_by.is_empty()));
    let _ = std::process::Command::new("docker")
        .args(["volume", "rm", "cc-docker-live-data"])
        .output();
    assert!(matches!(
        docker.inspect(name).await,
        Err(cc_docker_core::DockerError::NotFound)
    ));
}
