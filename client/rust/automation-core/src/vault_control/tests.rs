use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn private_response_abort_cancel_timeout_fence_partial_stream_not_other_peer() {
    for mode in ["abort", "cancel", "timeout"] {
        let (native, mut fixture) = tokio::io::duplex(8);
        let (reader, writer) = tokio::io::split(native);
        let peer = HelperPeer::connect(reader, writer);
        let token = CancellationToken::new();
        let timeout_start = Arc::new(tokio::sync::Notify::new());
        let worker_peer = peer.clone();
        let worker_token = token.clone();
        let start = timeout_start.clone();
        let task = tokio::spawn(async move {
            let write = worker_peer.write_vault_frame(Zeroizing::new(vec![b'X'; 8192]), None);
            tokio::pin!(write);
            match mode {
                "timeout" => {
                    tokio::select! {result=&mut write=>result,_=start.notified()=>tokio::time::timeout(Duration::from_millis(50),&mut write).await.unwrap_or(Err(AutomationError::Timeout))}
                }
                "cancel" => {
                    tokio::select! {result=&mut write=>result,_=worker_token.cancelled()=>Err(AutomationError::Closed)}
                }
                _ => write.await,
            }
        });
        let mut prefix = [0u8; 4];
        tokio::time::timeout(Duration::from_secs(2), fixture.read_exact(&mut prefix))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&prefix, b"XXXX");
        // Each causal event happens BEFORE observing/closing the peer.
        match mode {
            "abort" => task.abort(),
            "cancel" => token.cancel(),
            _ => timeout_start.notify_one(),
        }
        let _ = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap();
        assert!(peer.is_closed(), "{mode}");
        let mut tail = vec![];
        tokio::time::timeout(Duration::from_secs(2), fixture.read_to_end(&mut tail))
            .await
            .unwrap()
            .unwrap();
        assert!(tail.len() <= 8);
        assert!(tail.iter().all(|byte| *byte == b'X'));
        let (other, mut guest) = tokio::io::duplex(4096);
        let (read, write) = tokio::io::split(other);
        let unrelated = HelperPeer::connect(read, write);
        let request_peer = unrelated.clone();
        let request = tokio::spawn(async move {
            request_peer
                .request("probe", serde_json::json!({}), Duration::from_secs(2))
                .await
        });
        let frame = crate::read_frame(&mut BufReader::new(&mut guest))
            .await
            .unwrap()
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&frame).unwrap();
        guest.write_all(format!("{{\"version\":1,\"type\":\"response\",\"request_id\":{},\"result\":{{\"healthy\":true}},\"error\":null}}\n",value["request_id"]).as_bytes()).await.unwrap();
        assert_eq!(request.await.unwrap().unwrap()["healthy"], true);
        assert!(!unrelated.is_closed());
        unrelated.close().await;
    }
}
#[tokio::test]
async fn default_peer_never_accepts_private_vault_requests() {
    let (native, mut fixture) = tokio::io::duplex(4096);
    let (reader, writer) = tokio::io::split(native);
    let peer = HelperPeer::connect(reader, writer);
    let body = serde_json::json!({"version":1,"purpose":"ansible_vault_validation_password","scope_id":Uuid::new_v4(),"phase":"syntax","call_index":0,"phase_nonce":Uuid::new_v4(),"revision_digest":"a".repeat(64),"reference_sha256":"b".repeat(64),"environment_digest":"c".repeat(64),"helper_digest":"d".repeat(64)});
    fixture.write_all(format!("{}\n",serde_json::json!({"version":1,"type":"vault_validation_request","request_id":Uuid::new_v4(),"body":body})).as_bytes()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while !peer.is_closed() {
            tokio::task::yield_now().await
        }
    })
    .await
    .unwrap();
    peer.close().await;
}

#[tokio::test]
async fn original_revocation_while_writer_mutex_blocked_emits_zero_bytes() {
    for cause in ["credential revision", "lock/cancel"] {
        let (native, mut fixture) = tokio::io::duplex(8192);
        let (reader, writer) = tokio::io::split(native);
        let peer = HelperPeer::connect(reader, writer);
        let held = peer.writer.lock().await;
        let authority = CancellationToken::new();
        let fence = crate::vault_validation::VaultWriteFence {
            revoked: authority.clone(),
            job_cancel: None,
            deadline: std::time::Instant::now() + Duration::from_secs(5),
        };
        let write = peer.write_vault_frame(
            Zeroizing::new(b"synthetic-private-response\n".to_vec()),
            Some(fence),
        );
        tokio::pin!(write);
        std::future::poll_fn(|cx| {
            assert!(std::future::Future::poll(write.as_mut(), cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        authority.cancel();
        drop(held);
        assert!(write.await.is_err(), "{cause}");
        assert!(peer.is_closed());
        let mut bytes = vec![];
        tokio::time::timeout(Duration::from_secs(2), fixture.read_to_end(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        assert!(bytes.is_empty());
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn original_revocation_after_partial_response_fences_without_appended_frame() {
    let (native, mut fixture) = tokio::io::duplex(8);
    let (reader, writer) = tokio::io::split(native);
    let peer = HelperPeer::connect(reader, writer);
    let authority = CancellationToken::new();
    let writer_peer = peer.clone();
    let fence = crate::vault_validation::VaultWriteFence {
        revoked: authority.clone(),
            job_cancel: None,
        deadline: std::time::Instant::now() + Duration::from_secs(5),
    };
    let task = tokio::spawn(async move {
        writer_peer
            .write_vault_frame(Zeroizing::new(vec![b'P'; 8192]), Some(fence))
            .await
    });
    let mut prefix = [0; 4];
    tokio::time::timeout(Duration::from_secs(2), fixture.read_exact(&mut prefix))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&prefix, b"PPPP");
    authority.cancel();
    assert!(tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    assert!(peer.is_closed());
    let mut tail = vec![];
    tokio::time::timeout(Duration::from_secs(2), fixture.read_to_end(&mut tail))
        .await
        .unwrap()
        .unwrap();
    assert!(tail.len() <= 8);
    assert!(tail.iter().all(|b| *b == b'P'));
}
struct DelayedDenial {
    first: Arc<std::sync::atomic::AtomicBool>,
    flushed: Arc<tokio::sync::Notify>,
    release: CancellationToken,
}
impl crate::VaultValidationHandler for DelayedDenial {
    fn handle(
        &self,
        _: crate::VaultValidationControlRequest,
        mut responder: crate::VaultValidationResponder,
    ) -> crate::VaultValidationControlFuture {
        let first = self.first.clone();
        let flushed = self.flushed.clone();
        let release = self.release.clone();
        Box::pin(async move {
            let _ = responder.denied().await;
            if first.swap(false, std::sync::atomic::Ordering::AcqRel) {
                flushed.notify_one();
                release.cancelled().await;
            }
        })
    }
    fn revoke(&self) {
        self.release.cancel();
    }
}
fn private_request(index: u8) -> serde_json::Value {
    serde_json::json!({"version":1,"type":"vault_validation_request","request_id":Uuid::new_v4(),"body":{"version":1,"purpose":"ansible_vault_validation_password","scope_id":Uuid::new_v4(),"phase":"syntax","call_index":index,"phase_nonce":Uuid::new_v4(),"revision_digest":"a".repeat(64),"reference_sha256":"b".repeat(64),"environment_digest":"c".repeat(64),"helper_digest":"d".repeat(64)}})
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn flushed_response_then_delayed_native_handler_release_hands_off_and_reader_stays_live() {
    let (native, fixture) = tokio::io::duplex(16384);
    let (reader, writer) = tokio::io::split(native);
    let flushed = Arc::new(tokio::sync::Notify::new());
    let release = CancellationToken::new();
    let peer = HelperPeer::connect_with_vault(
        reader,
        writer,
        Arc::new(DelayedDenial {
            first: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            flushed: flushed.clone(),
            release: release.clone(),
        }),
    );
    let (read, mut write) = tokio::io::split(fixture);
    let mut read = BufReader::new(read);
    let first = private_request(0);
    write
        .write_all(format!("{first}\n").as_bytes())
        .await
        .unwrap();
    let response = crate::read_frame(&mut read).await.unwrap().unwrap();
    let response: serde_json::Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(response["request_id"], first["request_id"]);
    flushed.notified().await;
    let second = private_request(1);
    write
        .write_all(format!("{second}\n").as_bytes())
        .await
        .unwrap();
    // Native reader must still handle current/fresh-probe responses while one
    // private request waits for the previous handler's post-flush permit drop.
    let p = peer.clone();
    let probe = tokio::spawn(async move {
        p.request("probe", serde_json::json!({}), Duration::from_secs(2))
            .await
    });
    let frame = crate::read_frame(&mut read).await.unwrap().unwrap();
    let value: serde_json::Value = serde_json::from_slice(&frame).unwrap();
    assert_eq!(value["operation"], "probe");
    write.write_all(format!("{}\n",serde_json::json!({"version":1,"type":"response","request_id":value["request_id"],"result":{"alive":true},"error":null})).as_bytes()).await.unwrap();
    assert_eq!(probe.await.unwrap().unwrap()["alive"], true);
    assert!(!peer.is_closed());
    release.cancel();
    let frame = tokio::time::timeout(Duration::from_secs(2), crate::read_frame(&mut read))
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&frame).unwrap();
    assert_eq!(value["request_id"], second["request_id"]);
    assert!(value["error"].is_object());
    peer.close().await;
}

struct HeldFlush {
    bytes:Arc<std::sync::Mutex<Vec<u8>>>,
    entered:Arc<tokio::sync::Notify>,
}
impl tokio::io::AsyncWrite for HeldFlush {
    fn poll_write(self:std::pin::Pin<&mut Self>,_:&mut std::task::Context<'_>,data:&[u8])->std::task::Poll<std::io::Result<usize>> {
        let mut bytes=self.bytes.lock().unwrap();assert!(bytes.len()+data.len()<=8192);bytes.extend_from_slice(data);
        std::task::Poll::Ready(Ok(data.len()))
    }
    fn poll_flush(self:std::pin::Pin<&mut Self>,_:&mut std::task::Context<'_>)->std::task::Poll<std::io::Result<()>> {
        self.entered.notify_one();std::task::Poll::Pending
    }
    fn poll_shutdown(self:std::pin::Pin<&mut Self>,_:&mut std::task::Context<'_>)->std::task::Poll<std::io::Result<()>> {std::task::Poll::Ready(Ok(()))}
}
#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn original_revocation_during_blocked_flush_fences_without_appended_frame() {
    let (reader,_kept_open)=tokio::io::duplex(16);
    let bytes=Arc::new(std::sync::Mutex::new(Vec::new()));let entered=Arc::new(tokio::sync::Notify::new());
    let peer=HelperPeer::connect(reader,HeldFlush{bytes:bytes.clone(),entered:entered.clone()});
    let authority=CancellationToken::new();let retained=peer.clone();
    let fence=crate::vault_validation::VaultWriteFence{revoked:authority.clone(),job_cancel:None,deadline:std::time::Instant::now()+Duration::from_secs(5)};
    let task=tokio::spawn(async move{retained.write_vault_frame(Zeroizing::new(vec![b'F';16]),Some(fence)).await});
    tokio::time::timeout(Duration::from_secs(2),entered.notified()).await.unwrap();
    assert_eq!(*bytes.lock().unwrap(),vec![b'F';16]); // delivered bytes cannot be retracted
    authority.cancel();assert!(tokio::time::timeout(Duration::from_secs(2),task).await.unwrap().unwrap().is_err());
    assert!(peer.is_closed());assert_eq!(*bytes.lock().unwrap(),vec![b'F';16]); // no cancel/denial frame appended
}
