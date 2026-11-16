//! Isolated actor/SSH boundary, not a Native execution/PreparedReview claim.
//! Actual original-owner signatures + actual ServiceCustody persistence; no ACK fake.
use super::*;
use cc_crypto_core::{DevicePublicKeys, ServicePolicyTenant, SignedServiceDelegation};
use cc_ssh_core::Endpoint;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned Linux pinned Python/helper fixture and loopback"]
async fn actual_stalled_ssh_owner_revoke_interleaves_without_owner_lock_and_denies_raw_bool() {
    let python =
        std::env::var_os("CC_SERVICE_OWNER_TEST_PYTHON").expect("owned pinned interpreter");
    let modules = std::env::var_os("CC_SERVICE_OWNER_TEST_MODULE").expect("readonly helper source");
    assert!(
        std::path::Path::new(&python).is_absolute() && std::path::Path::new(&modules).is_absolute()
    );
    let own = tempfile::tempdir().unwrap();
    let mut child = tokio::process::Command::new(python)
        .args(["-I", "-B", "-c", include_str!("owner_status_fixture.py")])
        .arg(own.path())
        .arg(modules)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let writer = child.stdin.take().unwrap();
    let mut reader = tokio::io::BufReader::new(child.stdout.take().unwrap());
    let mut raw = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), reader.read_until(b'\n', &mut raw))
        .await
        .unwrap()
        .unwrap();
    assert!(raw.len() <= 65536);
    let setup: Value = serde_json::from_slice(&raw).unwrap();
    let policy = &setup["policy"];
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let owner = DevicePublicKeys {
        signing: hex::decode(setup["ownerSigning"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap(),
        encryption: [1; 32],
    };
    let tenant: ServicePolicyTenant =
        serde_json::from_value(policy["body"]["tenant"].clone()).unwrap();
    let verified = SignedServiceDelegation::verify_json(
        &serde_json::to_vec(policy).unwrap(),
        policy["body"]["owner_id"].as_str().unwrap(),
        &owner,
        &tenant,
        now,
    )
    .unwrap();
    let verifier = verified.owner_status_verifier().unwrap();
    let peer = crate::HelperPeer::connect(reader, writer);
    let stop = CancellationToken::new();
    let unrelated = CancellationToken::new();
    let actor = Arc::new(tokio::sync::Mutex::new(()));
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let endpoint = listener.local_addr().unwrap();
    let plan = ConnectionPlan {
        host_id: Uuid::new_v4().into(),
        name: "owned stalled handshake".into(),
        target: Endpoint::new("127.0.0.1", endpoint.port()),
        username: "owned".into(),
        credential: None,
        host_key_policy: HostKeyPolicy::Strict,
        route: vec![],
        proxy: None,
        forwards: vec![],
        backend: SshBackend::Native,
        keepalive_secs: None,
        agent_forwarding: false,
        warnings: vec![],
    };
    let retained = actor.clone();
    let cancel = stop.child_token();
    let handshake = tokio::spawn(async move {
        let _owner = retained.lock().await;
        let socket = tokio::net::TcpStream::connect(endpoint).await.unwrap();
        let stream = ServiceIo::new(
            socket,
            std::time::Instant::now() + Duration::from_secs(60),
            now + 60,
            cancel,
        );
        let connector = SshConnector::new(
            Arc::new(MemoryCredentialResolver::new()),
            Arc::new(MemoryKnownHosts::new()),
            Arc::new(FixedHostKeyPrompt::new(HostKeyDecision::Reject)),
        );
        connector.connect_over_stream(&plan, stream, [3; 32]).await
    });
    let (mut server, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap();
    // Real SSH client banner proves entry into the actual handshake. The owned
    // server sends no banner/key, so no authentication or host-key bypass occurs.
    let mut banner = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !banner.ends_with(b"\n") {
            let b = server.read_u8().await.unwrap();
            banner.push(b);
            assert!(banner.len() <= 1024);
        }
    })
    .await
    .unwrap();
    assert!(banner.starts_with(b"SSH-2.0-"));
    assert!(actor.try_lock().is_err());
    assert!(!handshake.is_finished());
    let bad = crate::service_owner_status::forward(
        &peer,
        json!({"delegation":policy,"statement":setup["forged"]}),
        &verifier,
        &stop,
    )
    .await;
    assert!(bad.is_err() && !stop.is_cancelled() && !handshake.is_finished());
    let untouched = peer
        .request("fixture_status", json!({}), Duration::from_secs(3))
        .await
        .unwrap();
    assert_eq!(untouched, json!({"recorded":0,"revoked":false,"floor":1}));
    let initial = crate::service_owner_status::forward(
        &peer,
        json!({"delegation":policy,"statement":setup["initial"]}),
        &verifier,
        &stop,
    )
    .await
    .unwrap();
    assert_eq!(initial["recorded"], true);
    assert!(!stop.is_cancelled() && !handshake.is_finished());
    let status_cleanup = Arc::new(tokio::sync::Mutex::new(()));
    let cleanup_attempted = Arc::new(tokio::sync::Notify::new());
    let body = json!({"delegation":policy,"statement":setup["revoked"]});
    let held_cleanup = status_cleanup.clone().lock_owned().await;
    let queued = Arc::new(tokio::sync::Notify::new());
    let forward_queued = queued.clone();
    let observed = tokio::spawn({
        let peer = peer.clone();
        let stop = stop.clone();
        let cleanup = status_cleanup.clone();
        async move {
            use std::future::Future;
            use std::task::Poll;
            let delivery = crate::service_owner_status::verify_and_cancel(body, &verifier, &stop)?;
            let mut locking = Box::pin(cleanup.lock());
            std::future::poll_fn(|cx| {
                assert!(matches!(locking.as_mut().poll(cx), Poll::Pending));
                forward_queued.notify_one();
                Poll::Ready(())
            })
            .await;
            let _ack = locking.await;
            crate::service_owner_status::persist(&peer, delivery).await
        }
    });
    tokio::time::timeout(Duration::from_secs(2), queued.notified())
        .await
        .unwrap();
    assert!(
        stop.is_cancelled(),
        "signed revoke cancels before held cleanup ACK lane"
    );
    assert!(
        !own.path().join("status-entered").exists(),
        "no persistence before cleanup is released"
    );
    drop(held_cleanup);
    let retirement = tokio::spawn({
        let peer = peer.clone();
        let stop = stop.clone();
        let gate = status_cleanup.clone();
        let attempted = cleanup_attempted.clone();
        async move {
            stop.cancelled().await;
            attempted.notify_one();
            let _ack = gate.lock().await;
            peer.close().await;
        }
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !own.path().join("status-entered").is_file() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(
        !observed.is_finished(),
        "actual persistence ACK remains deliberately held"
    );
    tokio::time::timeout(Duration::from_secs(2), cleanup_attempted.notified())
        .await
        .unwrap();
    assert!(
        !retirement.is_finished(),
        "trusted cleanup must retain the original peer through actual ACK"
    );
    let result = tokio::time::timeout(Duration::from_secs(2), handshake)
        .await
        .expect("accepted owner revoke must wake the stalled SSH IO before ACK")
        .unwrap();
    assert!(result.is_err() && stop.is_cancelled() && !unrelated.is_cancelled());
    assert!(
        actor.try_lock().is_ok(),
        "status handling cannot wait for the pending actor turn"
    );
    let mut byte = [0u8; 1];
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), server.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    tokio::fs::write(own.path().join("release-status"), b"release only own ACK")
        .await
        .unwrap();
    assert_eq!(observed.await.unwrap().unwrap()["recorded"], true);
    retirement.await.unwrap();
    let header: Value = serde_json::from_slice(
        &std::fs::read(own.path().join("custody/service-version.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(header["revoked"], true);
    assert_eq!(header["revocationFloor"], 2);
    assert!(
        tokio::time::timeout(Duration::from_secs(10), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}
