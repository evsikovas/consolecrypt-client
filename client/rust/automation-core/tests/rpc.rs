use cc_automation_core::{read_frame, AutomationError, HelperLaunch, HelperPeer, MAX_FRAME_BYTES};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn fragmented_and_unterminated_frames_obey_the_limit() {
    let (mut output, input) = tokio::io::duplex(64);
    let writer = tokio::spawn(async move {
        output.write_all(b"{\"version\":").await.unwrap();
        output.write_all(b"1}\n").await.unwrap();
    });
    let mut input = BufReader::new(input);
    assert_eq!(
        read_frame(&mut input).await.unwrap().unwrap(),
        b"{\"version\":1}\n"
    );
    writer.await.unwrap();
    assert!(read_frame(&mut input).await.unwrap().is_none());
    let mut truncated = BufReader::new(&b"partial"[..]);
    assert!(matches!(
        read_frame(&mut truncated).await,
        Err(AutomationError::Protocol(_))
    ));
    let oversized = vec![b'x'; MAX_FRAME_BYTES + 1];
    let mut input = BufReader::new(oversized.as_slice());
    assert!(matches!(
        read_frame(&mut input).await,
        Err(AutomationError::Protocol(_))
    ));
}

#[tokio::test]
async fn concurrent_requests_are_correlated_even_when_replies_arrive_backwards() {
    let (client, server) = tokio::io::duplex(8192);
    let (reader, writer) = tokio::io::split(client);
    let peer = HelperPeer::connect(reader, writer);
    let server = tokio::spawn(async move {
        let (reader, mut writer) = tokio::io::split(server);
        let mut reader = BufReader::new(reader);
        let mut requests = Vec::new();
        for _ in 0..2 {
            requests.push(
                serde_json::from_slice::<Value>(&read_frame(&mut reader).await.unwrap().unwrap())
                    .unwrap(),
            );
        }
        for request in requests.into_iter().rev() {
            let mut frame = serde_json::to_vec(
                &json!({"version": 1, "type": "response", "request_id": request["request_id"],
                "result": request["body"], "error": null}),
            )
            .unwrap();
            frame.push(b'\n');
            writer.write_all(&frame).await.unwrap();
        }
    });
    let (first, second) = tokio::join!(
        peer.request("probe", json!({"one": 1}), Duration::from_secs(2)),
        peer.request("probe", json!({"two": 2}), Duration::from_secs(2))
    );
    assert_eq!(first.unwrap(), json!({"one": 1}));
    assert_eq!(second.unwrap(), json!({"two": 2}));
    server.await.unwrap();
    peer.close().await;
}

#[tokio::test]
async fn timeout_sends_cancel_without_replaying_the_operation() {
    let (client, server) = tokio::io::duplex(8192);
    let (reader, writer) = tokio::io::split(client);
    let peer = HelperPeer::connect(reader, writer);
    let server = tokio::spawn(async move {
        let (reader, _writer) = tokio::io::split(server);
        let mut reader = BufReader::new(reader);
        let original: Value =
            serde_json::from_slice(&read_frame(&mut reader).await.unwrap().unwrap()).unwrap();
        let cancel: Value =
            serde_json::from_slice(&read_frame(&mut reader).await.unwrap().unwrap()).unwrap();
        assert_eq!(original["operation"], "validate");
        assert_eq!(cancel["operation"], "cancel_request");
        assert_eq!(cancel["body"]["request_id"], original["request_id"]);
    });
    assert!(matches!(
        peer.request("validate", json!({}), Duration::from_millis(20))
            .await,
        Err(AutomationError::Timeout)
    ));
    server.await.unwrap();
    peer.close().await;
}

#[tokio::test]
async fn dropping_a_pending_future_releases_capacity_and_cancels_its_request() {
    let (client, server) = tokio::io::duplex(8192);
    let (reader, writer) = tokio::io::split(client);
    let peer = HelperPeer::connect(reader, writer);
    let other = peer.clone();
    let pending = tokio::spawn(async move {
        other
            .request("validate", json!({}), Duration::from_secs(20))
            .await
    });
    let (reader, _writer) = tokio::io::split(server);
    let mut reader = BufReader::new(reader);
    let original: Value =
        serde_json::from_slice(&read_frame(&mut reader).await.unwrap().unwrap()).unwrap();
    pending.abort();
    let _ = pending.await;
    let cancel = tokio::time::timeout(Duration::from_secs(2), read_frame(&mut reader))
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let cancel: Value = serde_json::from_slice(&cancel).unwrap();
    assert_eq!(cancel["body"]["request_id"], original["request_id"]);
    peer.close().await;
}

#[tokio::test]
async fn invalid_or_newer_response_fails_closed_and_drains_pending_requests() {
    let (client, mut server) = tokio::io::duplex(8192);
    let (reader, writer) = tokio::io::split(client);
    let peer = HelperPeer::connect(reader, writer);
    let other = peer.clone();
    let pending = tokio::spawn(async move {
        other
            .request("probe", json!({}), Duration::from_secs(2))
            .await
    });
    let mut buffer = [0; 1024];
    let _ = server.read(&mut buffer).await.unwrap();
    server.write_all(b"{\"version\":2}\n").await.unwrap();
    assert!(matches!(
        pending.await.unwrap(),
        Err(AutomationError::Closed)
    ));
    assert!(peer.is_closed());
}

#[test]
fn wsl_launch_uses_explicit_user_and_preserves_argument_boundaries() {
    let launch = HelperLaunch::Wsl {
        distribution: "Ubuntu Test".into(),
        user: "tester".into(),
        python: "/home/tester/env/bin/python".into(),
        module_dir: "/mnt/d/project with spaces/src".into(),
        root: "/home/tester/runner state".into(),
    };
    if cfg!(windows) {
        let command = launch.command().unwrap();
        let args: Vec<_> = command
            .get_args()
            .map(|item| item.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            &args[..5],
            &[
                "--distribution",
                "Ubuntu Test",
                "--user",
                "tester",
                "--exec"
            ]
        );
        assert!(args.contains(&"/mnt/d/project with spaces/src".to_owned()));
        assert!(args.contains(&"/home/tester/runner state".to_owned()));
        assert!(!args.contains(&"--shutdown".to_owned()));
    } else {
        assert!(launch.command().is_err());
    }
    let bad = HelperLaunch::Wsl {
        distribution: "--shutdown".into(),
        user: "root".into(),
        python: "/usr/bin/python3".into(),
        module_dir: "/module".into(),
        root: "/root".into(),
    };
    assert!(bad.command().is_err());
}
