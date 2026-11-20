use cc_automation_core::{
    read_frame, HelperPeer, TargetFailure, TargetFuture, TargetHandler, TargetRequest,
};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::io::{AsyncWriteExt, BufReader};
use uuid::Uuid;

struct FixtureGate {
    job: Uuid,
    host: Uuid,
    calls: AtomicUsize,
}
impl TargetHandler for FixtureGate {
    fn revoke(&self, _job_id: Uuid, _manifest_digest: &str) {}
    fn handle(&self, request: TargetRequest) -> TargetFuture {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let approved = request.job_id == self.job
            && request.host_id == self.host
            && request.manifest_digest == "a".repeat(64);
        Box::pin(async move {
            if approved {
                Ok(json!({"fixture": true}))
            } else {
                Err(TargetFailure::TargetDenied)
            }
        })
    }
}

async fn frame(writer: &mut (impl AsyncWriteExt + Unpin), value: Value) {
    let mut raw = serde_json::to_vec(&value).unwrap();
    raw.push(b'\n');
    writer.write_all(&raw).await.unwrap();
}

#[tokio::test]
async fn trusted_callback_contract_preserves_the_exact_job_and_target_scope() {
    let job = Uuid::new_v4();
    let host = Uuid::new_v4();
    let gate = Arc::new(FixtureGate {
        job,
        host,
        calls: AtomicUsize::new(0),
    });
    let (client, server) = tokio::io::duplex(8192);
    let (read, write) = tokio::io::split(client);
    let peer = HelperPeer::connect_with_targets(read, write, Some(gate.clone()));
    let (read, mut write) = tokio::io::split(server);
    let mut read = BufReader::new(read);
    for (target, allowed) in [(host, true), (Uuid::new_v4(), false)] {
        let id = Uuid::new_v4();
        frame(&mut write, json!({"version": 1, "type": "target_request", "request_id": id,
            "job_id": job, "manifest_digest": "a".repeat(64), "host_id": target, "operation": "exec", "body": {"command": "fixture"}})).await;
        let raw = tokio::time::timeout(Duration::from_secs(2), read_frame(&mut read))
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let response: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(response["request_id"], id.to_string());
        assert_eq!(response["type"], "target_response");
        assert_eq!(response["error"].is_null(), allowed);
        if !allowed {
            assert_eq!(response["error"]["code"], "target_denied");
        }
    }
    assert_eq!(gate.calls.load(Ordering::SeqCst), 2);
    peer.close().await;
}

#[tokio::test]
async fn no_handler_never_grants_access_and_target_replay_closes_the_channel() {
    let (client, server) = tokio::io::duplex(8192);
    let (read, write) = tokio::io::split(client);
    let peer = HelperPeer::connect(read, write);
    let (read, mut write) = tokio::io::split(server);
    let mut read = BufReader::new(read);
    let request = json!({"version": 1, "type": "target_request", "request_id": Uuid::new_v4(),
        "job_id": Uuid::new_v4(), "manifest_digest": "a".repeat(64), "host_id": Uuid::new_v4(), "operation": "exec", "body": {}});
    frame(&mut write, request.clone()).await;
    let response: Value =
        serde_json::from_slice(&read_frame(&mut read).await.unwrap().unwrap()).unwrap();
    assert_eq!(response["error"]["code"], "target_denied");
    frame(&mut write, request).await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while !peer.is_closed() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
