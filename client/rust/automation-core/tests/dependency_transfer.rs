//! Real HelperPeer framing tests. The immutable bodies were emitted by the
//! actual Python private Helper; the ignored case drives that Helper live.
use cc_automation_core::{
    read_frame, receive_dependency_archive, AutomationError, DependencyArtifactSource,
    DependencyKind, DependencySourceSelection, HelperPeer,
};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
// These tests exercise the real process-wide admission budget; serialize test
// scenarios so unrelated fixtures do not consume each other's two slots.
static TRANSFER_TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
fn fixture() -> Value {
    serde_json::from_slice(include_bytes!("fixtures/dependency-transfer.json")).unwrap()
}
fn selected(value: &Value) -> DependencySourceSelection {
    DependencySourceSelection::new(
        DependencyKind::Role,
        "demo".into(),
        "1.2.3".into(),
        serde_json::from_value::<DependencyArtifactSource>(value["selection"]["source"].clone())
            .unwrap(),
        0,
    )
    .unwrap()
}
fn replay(
    value: Value,
    mut change: impl FnMut(&str, Value) -> Value + Send + 'static,
) -> Arc<HelperPeer> {
    let (client, server) = tokio::io::duplex(4096);
    let (reader, writer) = tokio::io::split(client);
    tokio::spawn(async move {
        let (reader, mut writer) = tokio::io::split(server);
        let mut reader = BufReader::new(reader);
        let mut chunk = 0;
        while let Some(raw) = read_frame(&mut reader).await.unwrap() {
            let request: Value = serde_json::from_slice(&raw).unwrap();
            let operation = request["operation"].as_str().unwrap();
            if operation == "cancel_request" {
                continue;
            }
            let response = match operation {
                "dependency_archive_info" => value["info"].clone(),
                "dependency_archive_chunk" => {
                    assert_eq!(request["body"], value["chunks"][chunk]["request"]);
                    let body = value["chunks"][chunk]["response"].clone();
                    chunk += 1;
                    body
                }
                _ => panic!("unexpected private operation"),
            };
            let response = change(operation, response);
            let mut wire=serde_json::to_vec(&json!({"version":1,"type":"response","request_id":request["request_id"],"result":response,"error":null})).unwrap();
            wire.push(b'\n');
            if writer.write_all(&wire).await.is_err() {
                break;
            }
        }
    });
    HelperPeer::connect(reader, writer)
}
async fn receive(
    peer: &Arc<HelperPeer>,
    value: &Value,
    cancel: &CancellationToken,
) -> cc_automation_core::Result<cc_automation_core::VerifiedDependencyArchive> {
    receive_dependency_archive(
        peer,
        &selected(value),
        Uuid::parse_str(value["info"]["preparation_id"].as_str().unwrap()).unwrap(),
        value["info"]["review_digest"].as_str().unwrap(),
        cancel,
    )
    .await
}
#[tokio::test]
async fn actual_python_bodies_cross_helperpeer_framing_and_independent_tree_verification() {
    let _serial = TRANSFER_TEST.lock().await;
    let value = fixture();
    let peer = replay(value.clone(), |_, body| body);
    let result = receive(&peer, &value, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.archive().tree().digest, value["tree_digest"]);
    assert_eq!(result.candidate_digest(), value["info"]["review_digest"]);
    assert!(result.is_from_current_peer(&peer));
    peer.close().await;
    assert!(!result.is_from_current_peer(&peer));
}
#[tokio::test]
async fn candidate_hash_schema_lengths_offsets_and_eof_fail_closed() {
    let _serial = TRANSFER_TEST.lock().await;
    for (operation, field, bad) in [
        (
            "dependency_archive_info",
            "review_digest",
            json!("0".repeat(64)),
        ),
        (
            "dependency_archive_info",
            "archive_sha256",
            json!("0".repeat(64)),
        ),
        ("dependency_archive_info", "archive_bytes", json!(33554433)),
        ("dependency_archive_info", "future", json!(true)),
        ("dependency_archive_chunk", "offset", json!(1)),
        ("dependency_archive_chunk", "data", json!("")),
        ("dependency_archive_chunk", "eof", json!(true)),
    ] {
        let value = fixture();
        let peer = replay(value.clone(), move |op, mut body| {
            if op == operation {
                body[field] = bad.clone();
            }
            body
        });
        assert!(receive(&peer, &value, &CancellationToken::new())
            .await
            .is_err());
        peer.close().await;
    }
}
#[tokio::test]
async fn corrupted_actual_bytes_and_oversized_result_are_not_witnesses() {
    let _serial = TRANSFER_TEST.lock().await;
    use base64::{engine::general_purpose::STANDARD, Engine};
    let value = fixture();
    let mut changed = false;
    let peer = replay(value.clone(), move |op, mut body| {
        if op == "dependency_archive_chunk" && !changed {
            let mut raw = STANDARD.decode(body["data"].as_str().unwrap()).unwrap();
            raw[30] ^= 1;
            body["data"] = json!(STANDARD.encode(raw));
            changed = true;
        }
        body
    });
    assert!(receive(&peer, &value, &CancellationToken::new())
        .await
        .is_err());
    peer.close().await;
    let peer = replay(value.clone(), |_, mut body| {
        body["padding"] = json!("x".repeat(4096));
        body
    });
    assert!(matches!(
        receive(&peer, &value, &CancellationToken::new()).await,
        Err(AutomationError::Protocol(_))
    ));
    peer.close().await;
}
#[tokio::test]
async fn cancelled_before_network_and_concurrent_receive_do_not_queue_or_succeed() {
    let _serial = TRANSFER_TEST.lock().await;
    let value = fixture();
    let token = CancellationToken::new();
    token.cancel();
    let peer = replay(value.clone(), |_, _| {
        panic!("cancelled request must not reach helper")
    });
    assert!(matches!(
        receive(&peer, &value, &token).await,
        Err(AutomationError::Closed)
    ));
    peer.close().await;
    let (client, server) = tokio::io::duplex(4096);
    let (reader, writer) = tokio::io::split(client);
    let peer = HelperPeer::connect(reader, writer);
    let (seen_tx, seen_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let mut reader = BufReader::new(server);
        read_frame(&mut reader).await.unwrap().unwrap();
        let _ = seen_tx.send(());
        tokio::time::sleep(Duration::from_secs(5)).await;
    });
    let first_peer = peer.clone();
    let first_value = value.clone();
    let token = CancellationToken::new();
    let first_token = token.clone();
    let first = tokio::spawn(async move { receive(&first_peer, &first_value, &first_token).await });
    seen_rx.await.unwrap();
    assert!(matches!(
        receive(&peer, &value, &CancellationToken::new()).await,
        Err(AutomationError::QueueFull)
    ));
    token.cancel();
    assert!(matches!(first.await.unwrap(), Err(AutomationError::Closed)));
    peer.close().await;
    server.abort();
}

#[tokio::test]
#[ignore = "requires explicitly labelled disposable Unix Helper container; see staged driver"]
async fn live_private_python_helper_and_default_gate() {
    let _serial = TRANSFER_TEST.lock().await;
    use tokio::process::Command;
    let value = fixture();
    let container =
        std::env::var("CC_DEPENDENCY_HELPER_CONTAINER").expect("owned fixture required");
    let token = std::env::var("CC_DEPENDENCY_HELPER_TOKEN").unwrap();
    assert_eq!(Uuid::parse_str(&token).unwrap().to_string(), token);
    assert_eq!(container, format!("cc-ans05-core-{token}"));
    let inspected = std::process::Command::new("docker.exe")
        .args(["inspect", &container])
        .env("DOCKER_HOST", "npipe:////./pipe/dockerDesktopLinuxEngine")
        .env("DOCKER_CONTEXT", "")
        .output()
        .unwrap();
    assert!(inspected.status.success());
    let row: Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(
        row[0]["Config"]["Labels"]["consolecrypt.test"],
        "ans05-core-transfer"
    );
    assert_eq!(row[0]["Config"]["Labels"]["consolecrypt.fixture"], token);
    for disabled in [false, true] {
        let mut command = Command::new("docker.exe");
        command.args([
            "exec",
            "-i",
            &container,
            "/opt/cc-runtime/bin/python",
            "-I",
            "-B",
            "/fixture/dependency-helper.py",
        ]);
        if disabled {
            command.arg("--disabled");
        }
        let mut child = command
            .env("DOCKER_HOST", "npipe:////./pipe/dockerDesktopLinuxEngine")
            .env("DOCKER_CONTEXT", "")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let line = read_frame(&mut stdout).await.unwrap().unwrap();
        assert!(line.len() <= 512);
        let hello: Value = serde_json::from_slice(&line).unwrap();
        let peer = HelperPeer::connect(stdout, child.stdin.take().unwrap());
        let result = receive_dependency_archive(
            &peer,
            &selected(&value),
            Uuid::parse_str(hello["preparation_id"].as_str().unwrap()).unwrap(),
            value["info"]["review_digest"].as_str().unwrap(),
            &CancellationToken::new(),
        )
        .await;
        if disabled {
            assert!(
                matches!(result,Err(AutomationError::Remote(code)) if code=="unsupported_operation")
            );
        } else {
            let archive = result.unwrap();
            assert_eq!(archive.archive().tree().digest, value["tree_digest"]);
            assert_eq!(hello["review_digest"], value["info"]["review_digest"]);
        }
        peer.close().await;
        assert!(tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success());
    }
}
