//! Receive-only Native evidence over the real private HelperPeer. No fetching,
//! project download/publication, UI approval, AppCore state or capability toggle.
use crate::dependency_archive::{verify_archive, SourceSelection, VerifiedArchive, MAX_ARCHIVE};
use crate::{AutomationError, HelperPeer, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    sync::{Arc, OnceLock, Weak},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const CHUNK: usize = 128 * 1024;
const CHUNK_RESULT: usize = 176 * 1024;
#[cfg(test)]
pub(crate) static DEPENDENCY_TEST_SERIAL: tokio::sync::Mutex<()> =
    tokio::sync::Mutex::const_new(());
// Covers receiving raw bytes AND bounded expanded-tree verification across all
// HelperPeers in this process. Never queue work behind this memory budget.
static PROCESS_ADMISSION: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
struct Admission {
    _peer: tokio::sync::OwnedSemaphorePermit,
    _process: tokio::sync::OwnedSemaphorePermit,
}
fn admit(peer: &HelperPeer) -> Result<Admission> {
    let peer_permit = peer.dependency_archive_permit()?;
    let process_permit = PROCESS_ADMISSION
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
        .clone()
        .try_acquire_owned()
        .map_err(|_| AutomationError::QueueFull)?;
    Ok(Admission {
        _peer: peer_permit,
        _process: process_permit,
    })
}
fn spawn_admitted<T: Send + 'static>(
    admission: Admission,
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> tokio::task::JoinHandle<Result<T>> {
    tokio::task::spawn_blocking(move || {
        // Both owned permits outlive a detached/cancelled caller, including
        // while this bounded worker is waiting to start in Tokio's CPU pool.
        let _admission = admission;
        work()
    })
}
async fn await_worker<T>(
    worker: tokio::task::JoinHandle<Result<T>>,
    deadline: Instant,
    cancel: &CancellationToken,
) -> Result<T> {
    tokio::select! {biased;
        _=cancel.cancelled()=>Err(AutomationError::Closed),
        result=tokio::time::timeout(remaining(deadline)?,worker)=>
            result.map_err(|_|AutomationError::Timeout)?.map_err(|_|invalid())?,
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Info {
    version: u32,
    preparation_id: String,
    review_digest: String,
    archive_sha256: String,
    archive_bytes: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Chunk {
    version: u32,
    preparation_id: String,
    review_digest: String,
    archive_sha256: String,
    archive_bytes: usize,
    offset: usize,
    data: String,
    eof: bool,
}
fn invalid() -> AutomationError {
    AutomationError::Protocol("invalid dependency archive transfer")
}
fn check(ok: bool) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(invalid())
    }
}
fn remaining(deadline: Instant) -> Result<Duration> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        Err(AutomationError::Timeout)
    } else {
        Ok(remaining)
    }
}
struct ExchangeGuard<'a> {
    peer: &'a HelperPeer,
    completed: bool,
}
impl Drop for ExchangeGuard<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.peer.abort_dependency_exchange();
        }
    }
}
async fn exchange(
    peer: &Arc<HelperPeer>,
    operation: &str,
    body: Value,
    bound: usize,
    deadline: Instant,
    cancel: &CancellationToken,
) -> Result<Value> {
    // The request's response timer starts after its bounded write. Wrap the
    // entire operation as well so a blocked write cannot extend this deadline.
    let budget = remaining(deadline)?.min(Duration::from_secs(30));
    let mut guard = ExchangeGuard {
        peer,
        completed: false,
    };
    let result = tokio::select! {biased;
        _=cancel.cancelled()=>return Err(AutomationError::Closed),
        response=tokio::time::timeout(budget, peer.request_dependency_bounded(operation,body,budget,bound))=>
            response.map_err(|_|AutomationError::Timeout)?,
    };
    guard.completed = true;
    result
}

/// Actual archive bytes independently verified against the Native-selected pin.
/// The retained candidate digest is an identity binding, NOT a claim that its
/// project snapshot/base/diff was verified. No Deserialize or approval methods.
pub struct VerifiedDependencyArchive {
    archive: VerifiedArchive,
    preparation_id: Uuid,
    candidate_digest: String,
    peer: Weak<HelperPeer>,
}
impl VerifiedDependencyArchive {
    pub fn archive(&self) -> &VerifiedArchive {
        &self.archive
    }
    pub fn preparation_id(&self) -> Uuid {
        self.preparation_id
    }
    pub fn candidate_digest(&self) -> &str {
        &self.candidate_digest
    }
    pub fn is_from_current_peer(&self, peer: &Arc<HelperPeer>) -> bool {
        !peer.is_closed() && self.peer.ptr_eq(&Arc::downgrade(peer))
    }
}

/// Call only from a Native-owned source/network review context. The source
/// constructor and these fields alone do not prove a user action or publisher
/// trust. Default Helper rejects these fixed private operations. At most one
/// transfer/verification per peer and two across this process, with no admission
/// wait queue (QueueFull is typed busy). A cancelled CPU worker retains both slots until
/// bounded archive parsing finishes, and its evidence is discarded. Abandoning
/// a network exchange closes this peer: a partial frame cannot be safely reused.
pub async fn receive_dependency_archive(
    peer: &Arc<HelperPeer>,
    selected: &SourceSelection,
    preparation_id: Uuid,
    candidate_snapshot_digest: &str,
    cancel: &CancellationToken,
) -> Result<VerifiedDependencyArchive> {
    check(!preparation_id.is_nil() && cc_models::automation::is_digest(candidate_snapshot_digest))?;
    if cancel.is_cancelled() {
        return Err(AutomationError::Closed);
    }
    let admission = admit(peer)?;
    let deadline = Instant::now() + Duration::from_secs(120);
    let identity =
        json!({"preparation_id":preparation_id,"review_digest":candidate_snapshot_digest});
    let info: Info = serde_json::from_value(
        exchange(
            peer,
            "dependency_archive_info",
            identity.clone(),
            4096,
            deadline,
            cancel,
        )
        .await?,
    )
    .map_err(|_| invalid())?;
    check(
        info.version == 1
            && info.preparation_id == preparation_id.to_string()
            && info.review_digest == candidate_snapshot_digest
            && info.archive_sha256 == selected.source().sha256
            && info.archive_bytes > 0
            && info.archive_bytes <= MAX_ARCHIVE,
    )?;
    let mut raw = Vec::new();
    while raw.len() < info.archive_bytes {
        let limit = CHUNK.min(info.archive_bytes - raw.len());
        let mut body = identity.clone();
        body["archive_sha256"] = json!(selected.source().sha256);
        body["offset"] = json!(raw.len());
        body["limit"] = json!(limit);
        let chunk: Chunk = serde_json::from_value(
            exchange(
                peer,
                "dependency_archive_chunk",
                body,
                CHUNK_RESULT,
                deadline,
                cancel,
            )
            .await?,
        )
        .map_err(|_| invalid())?;
        check(
            chunk.version == 1
                && chunk.preparation_id == preparation_id.to_string()
                && chunk.review_digest == candidate_snapshot_digest
                && chunk.archive_sha256 == info.archive_sha256
                && chunk.archive_bytes == info.archive_bytes
                && chunk.offset == raw.len()
                && chunk.data.len() <= limit.div_ceil(3) * 4,
        )?;
        let bytes = STANDARD.decode(&chunk.data).map_err(|_| invalid())?;
        check(
            bytes.len() == limit
                && !bytes.is_empty()
                && STANDARD.encode(&bytes) == chunk.data
                && chunk.eof == (raw.len() + bytes.len() == info.archive_bytes),
        )?;
        raw.extend_from_slice(&bytes);
    }
    if cancel.is_cancelled() {
        return Err(AutomationError::Closed);
    }
    let selection = selected.clone();
    let worker_cancel = cancel.clone();
    let worker = spawn_admitted(admission, move || {
        if worker_cancel.is_cancelled() {
            return Err(AutomationError::Closed);
        }
        let archive = verify_archive(selection, &raw).map_err(|_| invalid())?;
        if worker_cancel.is_cancelled() {
            return Err(AutomationError::Closed);
        }
        Ok(archive)
    });
    let archive = await_worker(worker, deadline, cancel).await?;
    if cancel.is_cancelled() || peer.is_closed() {
        return Err(AutomationError::Closed);
    }
    remaining(deadline)?;
    Ok(VerifiedDependencyArchive {
        archive,
        preparation_id,
        candidate_digest: candidate_snapshot_digest.into(),
        peer: Arc::downgrade(peer),
    })
}

#[cfg(test)]
mod exchange_tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};

    #[tokio::test]
    async fn process_ceiling_spans_peers_and_cancelled_cpu_keeps_both_permits() {
        let _serial = DEPENDENCY_TEST_SERIAL.lock().await;
        let make_peer = || {
            let (client, server) = tokio::io::duplex(4096);
            let (reader, writer) = tokio::io::split(client);
            (HelperPeer::connect(reader, writer), server)
        };
        let (first_peer, _first_server) = make_peer();
        let (second_peer, _second_server) = make_peer();
        let (third_peer, _third_server) = make_peer();
        let first = admit(&first_peer).unwrap();
        let second = admit(&second_peer).unwrap();
        assert!(matches!(
            admit(&third_peer),
            Err(AutomationError::QueueFull)
        ));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = std::sync::mpsc::channel();
        let worker = spawn_admitted(first, move || {
            let _ = started_tx.send(());
            finish_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            Ok(())
        });
        started_rx.await.unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(matches!(
            await_worker(worker, Instant::now() + Duration::from_secs(1), &cancel).await,
            Err(AutomationError::Closed)
        ));
        // The caller is gone, but a running CPU closure still owns memory and
        // BOTH permits. Neither another peer nor the old peer can bypass it.
        assert!(matches!(
            admit(&third_peer),
            Err(AutomationError::QueueFull)
        ));
        assert!(matches!(
            admit(&first_peer),
            Err(AutomationError::QueueFull)
        ));
        assert!(!first_peer.is_closed());
        drop(second);
        let third = admit(&third_peer).unwrap();
        finish_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                match admit(&first_peer) {
                    Ok(permit) => {
                        drop(permit);
                        break;
                    }
                    Err(AutomationError::QueueFull) => tokio::task::yield_now().await,
                    Err(error) => panic!("unexpected admission error: {error}"),
                }
            }
        })
        .await
        .unwrap();
        drop(third);
        first_peer.close().await;
        second_peer.close().await;
        third_peer.close().await;
    }

    #[tokio::test]
    async fn completed_remote_error_keeps_peer_and_next_frame_synchronized() {
        let (client, server) = tokio::io::duplex(4096);
        let (reader, writer) = tokio::io::split(client);
        let peer = HelperPeer::connect(reader, writer);
        let server = tokio::spawn(async move {
            let (reader, mut writer) = tokio::io::split(server);
            let mut reader = BufReader::new(reader);
            for fail in [true, false] {
                let raw = crate::read_frame(&mut reader).await.unwrap().unwrap();
                let request: Value = serde_json::from_slice(&raw).unwrap();
                let mut response = serde_json::to_vec(&json!({
                    "version":1,"type":"response","request_id":request["request_id"],
                    "result":if fail {Value::Null} else {json!({"ok":true})},
                    "error":if fail {json!({"code":"unsupported_operation","message":"fixture"})} else {Value::Null}
                })).unwrap();
                response.push(b'\n');
                writer.write_all(&response).await.unwrap();
            }
            let _ = crate::read_frame(&mut reader).await;
        });
        let cancel = CancellationToken::new();
        let result = exchange(
            &peer,
            "dependency_archive_info",
            json!({}),
            4096,
            Instant::now() + Duration::from_secs(1),
            &cancel,
        )
        .await;
        assert!(
            matches!(result, Err(AutomationError::Remote(code)) if code == "unsupported_operation")
        );
        assert!(!peer.is_closed());
        let result = exchange(
            &peer,
            "dependency_archive_info",
            json!({}),
            4096,
            Instant::now() + Duration::from_secs(1),
            &cancel,
        )
        .await
        .unwrap();
        assert_eq!(result, json!({"ok":true}));
        assert!(!peer.is_closed());
        peer.close().await;
        server.await.unwrap();
    }

    #[tokio::test]
    async fn whole_exchange_deadline_includes_blocked_write_and_closes_partial_stream() {
        let (client, _server) = tokio::io::duplex(1);
        let (reader, writer) = tokio::io::split(client);
        let peer = HelperPeer::connect(reader, writer);
        let result = exchange(
            &peer,
            "dependency_archive_info",
            json!({}),
            4096,
            Instant::now() + Duration::from_millis(20),
            &CancellationToken::new(),
        )
        .await;
        assert!(matches!(result, Err(AutomationError::Timeout)));
        assert!(peer.is_closed());
    }

    #[tokio::test]
    async fn task_abort_during_partial_write_closes_peer_before_framing_reuse() {
        let (client, mut server) = tokio::io::duplex(1);
        let (reader, writer) = tokio::io::split(client);
        let peer = HelperPeer::connect(reader, writer);
        let task_peer = peer.clone();
        let worker = tokio::spawn(async move {
            exchange(
                &task_peer,
                "dependency_archive_info",
                json!({}),
                4096,
                Instant::now() + Duration::from_secs(30),
                &CancellationToken::new(),
            )
            .await
        });
        let mut prefix = [0; 1];
        tokio::time::timeout(Duration::from_secs(1), server.read_exact(&mut prefix))
            .await
            .unwrap()
            .unwrap();
        worker.abort();
        assert!(worker.await.unwrap_err().is_cancelled());
        assert!(peer.is_closed());
    }
}
