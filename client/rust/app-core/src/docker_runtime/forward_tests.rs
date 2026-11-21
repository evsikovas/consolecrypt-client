use super::*;
use tokio::io::{AsyncBufReadExt, BufReader};

/// An Engine which permits inspection of the saved name only. It publishes a
/// synthetic TCP service on loopback; nothing on the machine's Docker is used.
#[derive(Debug)]
struct TestEngine(u16);

#[async_trait]
impl DockerConnector for TestEngine {
    async fn connect(&self) -> Result<DockerIo, DockerError> {
        let (client, server) = tokio::io::duplex(4096);
        let port = self.0;
        tokio::spawn(async move {
            let mut server = BufReader::new(server);
            let mut request = String::new();
            server.read_line(&mut request).await.unwrap();
            assert_eq!(request, "GET /containers/saved-db/json HTTP/1.1\r\n");
            loop {
                request.clear();
                if server.read_line(&mut request).await.unwrap() == 0 || request == "\r\n" {
                    break;
                }
            }
            let body = serde_json::json!({
                "Name":"/saved-db", "State":{"Running":true},
                "NetworkSettings":{"Ports":{"5432/tcp":[{"HostIp":"127.0.0.1","HostPort":port.to_string()}]}}
            }).to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            server
                .get_mut()
                .write_all(response.as_bytes())
                .await
                .unwrap();
        });
        Ok(Box::new(client))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn saved_docker_forward_resolves_name_relays_tcp_and_rebinds_same_port() {
    let dir = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        dir.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("test".into(), "synthetic-forward-test-passphrase".into())
        .await
        .unwrap();
    let upstream = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let (_, unlocked) = app.unlocked().await.unwrap();
    unlocked.docker.sessions.lock().unwrap().insert(
        "session".into(),
        Arc::new(LiveDocker {
            info: DockerSessionDto {
                id: "session".into(),
                host_id: "local".into(),
                host_name: "fixture".into(),
                engine_version: "test".into(),
                os: "linux".into(),
                arch: "amd64".into(),
                containers_running: 1,
                containers_total: 1,
                images: 1,
            },
            client: DockerClient::new(Arc::new(TestEngine(upstream.local_addr().unwrap().port()))),
            ssh: None,
            local_endpoint: None,
            host_id: None,
            cancel: unlocked.docker.shutdown.child_token(),
        }),
    );
    let start = |port| app.docker_forward_start("session".into(), "saved-db".into(), 5432, port);
    let forward = start(0).await.unwrap();
    assert_eq!(forward.host_id, "local");
    assert_eq!(forward.container_name, "saved-db");
    let port = forward.address.parse::<SocketAddr>().unwrap().port();
    assert!(
        matches!(start(port).await, Err(AppError::InvalidInput{reason, ..}) if reason == "local_port_busy")
    );
    let mut client = tokio::net::TcpStream::connect(&forward.address)
        .await
        .unwrap();
    client.write_all(b"probe").await.unwrap();
    let (mut received, _) = tokio::time::timeout(Duration::from_secs(3), upstream.accept())
        .await
        .unwrap()
        .unwrap();
    let mut probe = [0u8; 5];
    tokio::time::timeout(Duration::from_secs(3), received.read_exact(&mut probe))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&probe, b"probe");
    drop(client);
    drop(received);
    app.docker_forward_stop(forward.id).await.unwrap();
    assert_eq!(start(port).await.unwrap().address, forward.address);
    app.docker_disconnect("session".into()).await.unwrap();
    let _released = tokio::net::TcpListener::bind(&forward.address)
        .await
        .unwrap();
    app.shutdown().await.unwrap();
}
