use crate::{AutomationError, Result, TargetHandler, TargetRequest};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
const MAX_PENDING: usize = 64;

/// Reads fragmented input without allocating past the frame budget, including
/// when a compromised helper never sends a newline.
pub async fn read_frame<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Option<Vec<u8>>> {
    let mut frame = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return if frame.is_empty() {
                Ok(None)
            } else {
                Err(AutomationError::Protocol("truncated frame"))
            };
        }
        let take = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |index| index + 1);
        if frame.len() + take > MAX_FRAME_BYTES {
            return Err(AutomationError::Protocol("frame exceeds one MiB"));
        }
        frame.extend_from_slice(&available[..take]);
        reader.consume(take);
        if frame.last() == Some(&b'\n') {
            return Ok(Some(frame));
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteFailure {
    pub code: String,
    pub message: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    version: u8,
    #[serde(rename = "type")]
    kind: String,
    request_id: Uuid,
    result: Value,
    error: Option<RemoteFailure>,
}

struct PendingResponse {
    sender: oneshot::Sender<Result<Value>>,
    max_result_bytes: usize,
}
type Pending = HashMap<Uuid, PendingResponse>;
type Writer = Box<dyn AsyncWrite + Unpin + Send>;

fn result_fits(value: &Value, maximum: usize) -> bool {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, raw: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_sub(raw.len())
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidData))?;
            Ok(raw.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Counter(maximum), value).is_ok()
}

pub struct HelperPeer {
    pub(crate) writer: tokio::sync::Mutex<Option<Writer>>,
    pending: Mutex<Pending>,
    pub(crate) closed: CancellationToken,
    target_handler: Option<Arc<dyn TargetHandler>>,
    vault_handler: Option<Arc<dyn crate::VaultValidationHandler>>,
    vault_tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    vault_seen: Mutex<HashSet<Uuid>>,
    vault_slots: Arc<tokio::sync::Semaphore>,
    vault_admission: Arc<tokio::sync::Semaphore>,
    target_slots: Arc<tokio::sync::Semaphore>,
    diagnostic_slot: Arc<tokio::sync::Semaphore>,
    target_seen: Mutex<HashSet<Uuid>>,
    dependency_archive_slot: Arc<tokio::sync::Semaphore>,
}

impl std::fmt::Debug for HelperPeer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HelperPeer")
            .field("closed", &self.closed.is_cancelled())
            .finish_non_exhaustive()
    }
}

impl HelperPeer {
    pub fn connect<R, W>(reader: R, writer: W) -> Arc<Self>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        Self::connect_with_targets(reader, writer, None)
    }

    pub fn connect_with_targets<R, W>(
        reader: R,
        writer: W,
        handler: Option<Arc<dyn TargetHandler>>,
    ) -> Arc<Self>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        Self::connect_channels(reader, writer, handler, None)
    }
    pub(crate) fn connect_with_vault<R, W>(
        reader: R,
        writer: W,
        handler: Arc<dyn crate::VaultValidationHandler>,
    ) -> Arc<Self>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        Self::connect_channels(reader, writer, None, Some(handler))
    }
    fn connect_channels<R, W>(
        reader: R,
        writer: W,
        handler: Option<Arc<dyn TargetHandler>>,
        vault_handler: Option<Arc<dyn crate::VaultValidationHandler>>,
    ) -> Arc<Self>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let peer = Arc::new(Self {
            writer: tokio::sync::Mutex::new(Some(Box::new(writer))),
            pending: Mutex::new(HashMap::new()),
            closed: CancellationToken::new(),
            target_handler: handler,
            vault_handler,
            vault_tasks: Mutex::new(Vec::new()),
            vault_seen: Mutex::new(HashSet::new()),
            vault_slots: Arc::new(tokio::sync::Semaphore::new(1)),
            vault_admission: Arc::new(tokio::sync::Semaphore::new(2)),
            target_slots: Arc::new(tokio::sync::Semaphore::new(5)),
            diagnostic_slot: Arc::new(tokio::sync::Semaphore::new(1)),
            target_seen: Mutex::new(HashSet::new()),
            dependency_archive_slot: Arc::new(tokio::sync::Semaphore::new(1)),
        });
        let weak = Arc::downgrade(&peer);
        let closed = peer.closed.clone();
        let shutdown_peer = Arc::downgrade(&peer);
        let shutdown_token = closed.clone();
        tokio::spawn(async move {
            shutdown_token.cancelled().await;
            if let Some(peer) = shutdown_peer.upgrade() {
                peer.writer.lock().await.take();
            }
        });
        tokio::spawn(async move {
            let mut reader = BufReader::new(reader);
            loop {
                let frame = tokio::select! {
                    _ = closed.cancelled() => break,
                    frame = read_frame(&mut reader) => frame,
                };
                let Some(peer) = weak.upgrade() else { break };
                match frame {
                    Ok(Some(bytes)) if peer.receive(&bytes).is_ok() => {}
                    _ => {
                        peer.fail();
                        break;
                    }
                }
            }
        });
        peer
    }

    fn receive(self: &Arc<Self>, bytes: &[u8]) -> Result<()> {
        let value = crate::json::parse(bytes)?;
        if value.get("type").and_then(Value::as_str) == Some("vault_job_request") {
            if bytes.len() > 2048 {
                return Err(AutomationError::Protocol(
                    "private Vault request exceeds budget",
                ));
            }
            return self.receive_vault_job(value);
        }
        if value.get("type").and_then(Value::as_str) == Some("vault_validation_request") {
            if bytes.len() > 2048 {
                return Err(AutomationError::Protocol(
                    "private Vault request exceeds budget",
                ));
            }
            return self.receive_vault(value);
        }
        if self.vault_handler.is_some()
            && matches!(
                value.get("type").and_then(Value::as_str),
                Some("target_request" | "target_revoke")
            )
        {
            return Err(AutomationError::Protocol(
                "target control forbidden on validation peer",
            ));
        }
        if value.get("type").and_then(Value::as_str) == Some("target_revoke") {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Revoke {
                version: u8,
                #[serde(rename = "type")]
                kind: String,
                job_id: Uuid,
                manifest_digest: String,
            }
            let revoke: Revoke = serde_json::from_value(value)
                .map_err(|_| AutomationError::Protocol("invalid target revocation"))?;
            if revoke.version != 1
                || revoke.kind != "target_revoke"
                || !cc_models::automation::is_digest(&revoke.manifest_digest)
            {
                return Err(AutomationError::Protocol("invalid target revocation"));
            }
            if let Some(handler) = &self.target_handler {
                handler.revoke(revoke.job_id, &revoke.manifest_digest);
            }
            return Ok(());
        }
        if value.get("type").and_then(Value::as_str) == Some("target_request") {
            return self.receive_target(value);
        }
        let response: Response = serde_json::from_value(value)
            .map_err(|_| AutomationError::Protocol("invalid response"))?;
        if response.version != 1 || response.kind != "response" {
            return Err(AutomationError::Protocol(
                "unsupported response version or type",
            ));
        }
        if let Some(error) = &response.error {
            if error.code.is_empty()
                || error.code.len() > 80
                || !error
                    .code
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
                || error.message.len() > 4096
            {
                return Err(AutomationError::Protocol("invalid error response"));
            }
        }
        if let Some(pending) = self
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&response.request_id)
        {
            // The entire envelope was already limited to one MiB before the
            // strict JSON parse. Apply this smaller operation-body bound before
            // any archive base64 decoding/re-encoding; do not allocate a copy.
            let result = match response.error {
                Some(error) => Err(AutomationError::Remote(error.code)),
                None if !result_fits(&response.result, pending.max_result_bytes) => Err(
                    AutomationError::Protocol("helper result exceeds operation budget"),
                ),
                None => Ok(response.result),
            };
            let _ = pending.sender.send(result);
        }
        // A response can race with local timeout/cancellation. It must not
        // complete a different request or cause a replay of the timed-out one.
        Ok(())
    }

    fn receive_vault(self: &Arc<Self>, value: Value) -> Result<()> {
        let handler = self.vault_handler.clone().ok_or(AutomationError::Protocol(
            "Vault control unavailable on this peer",
        ))?;
        let request: crate::VaultValidationControlRequest = serde_json::from_value(value)
            .map_err(|_| AutomationError::Protocol("invalid private Vault request"))?;
        if request.version != 1
            || request.kind != "vault_validation_request"
            || request.body.version != 1
            || request.body.call_index >= 2
            || request.request_id.is_nil()
        {
            return Err(AutomationError::Protocol("invalid private Vault child"));
        }
        {
            let mut seen = self.vault_seen.lock().unwrap_or_else(|e| e.into_inner());
            if seen.len() >= 4 || !seen.insert(request.request_id) {
                return Err(AutomationError::Protocol("private Vault replay or budget"));
            }
        }
        // One active +ONE bounded handoff waiter, no unbounded queue. Reader
        // must remain free for current handler's fresh environment-probe reply.
        let queued = self
            .vault_admission
            .clone()
            .try_acquire_owned()
            .map_err(|_| AutomationError::QueueFull)?;
        let mut tasks = self.vault_tasks.lock().unwrap_or_else(|e| e.into_inner());
        if self.is_closed() {
            return Err(AutomationError::Closed);
        }
        let peer = self.clone();
        let slots = self.vault_slots.clone();
        let closed = self.closed.clone();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        tasks.push(tokio::spawn(async move {
            let _queued=queued;
            let permit=tokio::select!{biased;
                _=closed.cancelled()=>return,
                result=tokio::time::timeout(Duration::from_secs(5),slots.acquire_owned())=>match result {Ok(Ok(permit))=>permit,_=>{peer.abort_dependency_exchange();return;}},
            };
            let _permit=permit;
            if peer.is_closed(){return;}
            let responder=crate::vault_control::VaultValidationResponder::new(&peer,request.request_id,deadline);
            handler.handle(request,responder).await;
        }));
        Ok(())
    }

    fn receive_vault_job(self: &Arc<Self>, value: Value) -> Result<()> {
        let handler = self.vault_handler.clone().ok_or(AutomationError::Protocol(
            "Vault control unavailable on this peer",
        ))?;
        let request: crate::VaultJobControlRequest = serde_json::from_value(value)
            .map_err(|_| AutomationError::Protocol("invalid private job material request"))?;
        if request.version != 1
            || request.kind != "vault_job_request"
            || request.request_id.is_nil()
            || request.body.version != 1
            || request.body.job_id.is_nil()
            || request.body.purpose != crate::VaultPasswordPurpose::AnsibleVaultPassword
            || [
                &request.body.manifest_digest,
                &request.body.revision_digest,
                &request.body.reference_sha256,
            ]
            .iter()
            .any(|v| !cc_models::automation::is_digest(v))
        {
            return Err(AutomationError::Protocol(
                "invalid private job material request",
            ));
        }
        {
            let mut seen = self.vault_seen.lock().unwrap_or_else(|e| e.into_inner());
            // Four existing syntax/lint grants plus ONE measured execution
            // loader. No separate map, unlimited waiter or material resolver.
            if seen.len() >= 5 || !seen.insert(request.request_id) {
                return Err(AutomationError::Protocol("private Vault replay or budget"));
            }
        }
        let queued = self
            .vault_admission
            .clone()
            .try_acquire_owned()
            .map_err(|_| AutomationError::QueueFull)?;
        let mut tasks = self.vault_tasks.lock().unwrap_or_else(|e| e.into_inner());
        if self.is_closed() {
            return Err(AutomationError::Closed);
        }
        let peer = self.clone();
        let slots = self.vault_slots.clone();
        let closed = self.closed.clone();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        tasks.push(tokio::spawn(async move {
            let _queued=queued;
            let permit=tokio::select!{biased;_=closed.cancelled()=>return,
                result=tokio::time::timeout(Duration::from_secs(5),slots.acquire_owned())=>match result {Ok(Ok(p))=>p,_=>{peer.abort_dependency_exchange();return;}}};
            let _permit=permit;if peer.is_closed(){return;}
            let responder=crate::vault_control::VaultJobResponder::new(&peer,request.request_id,deadline);
            handler.handle_job(request,responder).await;
        }));
        Ok(())
    }
    pub(crate) async fn drain_vault_handlers(&self) -> Result<()> {
        loop {
            let tasks =
                std::mem::take(&mut *self.vault_tasks.lock().unwrap_or_else(|e| e.into_inner()));
            if tasks.is_empty() {
                return Ok(());
            }
            for task in tasks {
                task.await.map_err(|_| AutomationError::Closed)?;
            }
        }
    }

    fn receive_target(self: &Arc<Self>, value: Value) -> Result<()> {
        let request: TargetRequest = serde_json::from_value(value)
            .map_err(|_| AutomationError::Protocol("invalid target request"))?;
        if request.version != 1
            || request.kind != "target_request"
            || !request.body.is_object()
            || request.manifest_digest.len() != 64
            || !request
                .manifest_digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !matches!(
                request.operation.as_str(),
                "exec" | "put_chunk" | "get_chunk" | "file_close" | "redact_event"
            )
        {
            return Err(AutomationError::Protocol("invalid scoped target operation"));
        }
        // Pure diagnostic projection has no side effects/authority to replay.
        // Do not spend executable-operation replay capacity on log volume.
        if request.operation!="redact_event" {
            let mut seen = self.target_seen.lock().unwrap_or_else(|e| e.into_inner());
            if seen.len() >= 10000 || !seen.insert(request.request_id) {
                return Err(AutomationError::Protocol(
                    "duplicate target request or session limit",
                ));
            }
        }
        let permit = if request.operation=="redact_event" {
            // Diagnostic saturation never consumes executable admission or
            // tears down the control peer. No unbounded busy-reply tasks:
            // absent response means the parent's fixed privacy timeout censors.
            let Ok(permit)=self.diagnostic_slot.clone().try_acquire_owned() else{return Ok(())};
            permit
        } else {
            self.target_slots.clone().try_acquire_owned()
                .map_err(|_|AutomationError::Protocol("target concurrency exceeded"))?
        };
        let weak = Arc::downgrade(self);
        let handler = self.target_handler.clone();
        let closed = self.closed.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let id = request.request_id;
            let outcome = match &handler {
                Some(handler) => tokio::select! {
                    _ = closed.cancelled() => return,
                    result = tokio::time::timeout(Duration::from_secs(3600), handler.handle(request)) =>
                        result.unwrap_or(Err(crate::TargetFailure::ConnectionLost)),
                },
                None => Err(crate::TargetFailure::TargetDenied),
            };
            let outcome = match outcome {
                Ok(value)
                    if serde_json::to_vec(&value)
                        .map_or(true, |raw| raw.len() > MAX_FRAME_BYTES - 8192) =>
                {
                    Err(crate::TargetFailure::OutputLimit)
                }
                other => other,
            };
            let (result, error) = match outcome {
                Ok(value) => (value, Value::Null),
                Err(code) => (
                    Value::Null,
                    serde_json::json!({"code": code, "message": "Scoped target request failed"}),
                ),
            };
            if let Some(peer) = weak.upgrade() {
                let _ = peer
                    .write_value(serde_json::json!({"version": 1, "type": "target_response",
                    "request_id": id, "result": result, "error": error}))
                    .await;
            }
        });
        Ok(())
    }

    fn fail(&self) {
        self.closed.cancel();
        if let Some(handler) = &self.vault_handler {
            handler.revoke();
        }
        for (_, pending) in self
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain()
        {
            let _ = pending.sender.send(Err(AutomationError::Closed));
        }
    }

    pub fn is_closed(&self) -> bool {
        self.closed.is_cancelled()
    }
    // An abandoned fixed dependency exchange might have only written part of
    // its frame. Never reuse that stream or append a cancellation frame to it.
    pub(crate) fn abort_dependency_exchange(&self) {
        self.fail();
    }
    pub(crate) fn dependency_archive_permit(&self) -> Result<tokio::sync::OwnedSemaphorePermit> {
        self.dependency_archive_slot
            .clone()
            .try_acquire_owned()
            .map_err(|_| AutomationError::QueueFull)
    }

    async fn send(&self, id: Uuid, operation: &str, body: Value) -> Result<()> {
        if self.is_closed() {
            return Err(AutomationError::Closed);
        }
        if operation.is_empty() || operation.len() > 64 || !body.is_object() {
            return Err(AutomationError::Invalid("invalid request"));
        }
        self.write_value(serde_json::json!({"version": 1, "request_id": id,
            "operation": operation, "body": body}))
            .await
    }

    async fn write_value(&self, value: Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(&value)
            .map_err(|_| AutomationError::Invalid("request serialization"))?;
        bytes.push(b'\n');
        if bytes.len() > MAX_FRAME_BYTES {
            return Err(AutomationError::Invalid("request exceeds one MiB"));
        }
        let outcome = tokio::time::timeout(Duration::from_secs(5), async {
            let mut guard = self.writer.lock().await;
            let writer = guard
                .as_mut()
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::BrokenPipe))?;
            writer.write_all(&bytes).await?;
            writer.flush().await
        })
        .await;
        match outcome {
            Ok(Ok(())) => Ok(()),
            _ => {
                self.fail();
                Err(AutomationError::Closed)
            }
        }
    }

    pub async fn request(
        self: &Arc<Self>,
        operation: &str,
        body: Value,
        timeout: Duration,
    ) -> Result<Value> {
        self.request_with_id(Uuid::new_v4(), operation, body, timeout)
            .await
    }

    pub async fn request_with_id(
        self: &Arc<Self>,
        id: Uuid,
        operation: &str,
        body: Value,
        timeout: Duration,
    ) -> Result<Value> {
        self.request_with_id_bounded(
            id,
            operation,
            body,
            timeout,
            MAX_FRAME_BYTES,
            AbandonPolicy::CancelRequest,
        )
        .await
    }
    pub(crate) async fn request_dependency_bounded(
        self: &Arc<Self>,
        operation: &str,
        body: Value,
        timeout: Duration,
        max_result_bytes: usize,
    ) -> Result<Value> {
        self.request_with_id_bounded(
            Uuid::new_v4(),
            operation,
            body,
            timeout,
            max_result_bytes,
            AbandonPolicy::ClosePeer,
        )
        .await
    }
    async fn request_with_id_bounded(
        self: &Arc<Self>,
        id: Uuid,
        operation: &str,
        body: Value,
        timeout: Duration,
        max_result_bytes: usize,
        abandon: AbandonPolicy,
    ) -> Result<Value> {
        if max_result_bytes == 0 || max_result_bytes > MAX_FRAME_BYTES {
            return Err(AutomationError::Invalid("invalid helper result budget"));
        }
        let (sender, receiver) = oneshot::channel();
        {
            let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            if pending.len() >= MAX_PENDING {
                return Err(AutomationError::QueueFull);
            }
            if pending.contains_key(&id) {
                return Err(AutomationError::Invalid("request ID is already pending"));
            }
            pending.insert(
                id,
                PendingResponse {
                    sender,
                    max_result_bytes,
                },
            );
        }
        let mut registration = RequestRegistration {
            peer: Arc::downgrade(self),
            id,
            abandon,
            completed: false,
        };
        if let Err(error) = self.send(id, operation, body).await {
            self.pending
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
            return Err(error);
        }
        let result = match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(AutomationError::Closed),
            Err(_) => {
                if matches!(abandon, AbandonPolicy::ClosePeer) {
                    // No cancellation frame may be appended to an abandoned
                    // dependency stream, including a write/response race.
                    self.fail();
                    return Err(AutomationError::Timeout);
                }
                self.pending
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&id);
                let _ = self
                    .send(
                        Uuid::new_v4(),
                        "cancel_request",
                        serde_json::json!({"request_id": id}),
                    )
                    .await;
                Err(AutomationError::Timeout)
            }
        };
        // A complete write AND response/error body have finished. In
        // particular an early hostile response alone cannot disarm the guard
        // while send() is still writing the original frame.
        registration.completed = true;
        result
    }

    pub async fn close(&self) {
        self.fail();
        self.writer.lock().await.take();
        let tasks =
            std::mem::take(&mut *self.vault_tasks.lock().unwrap_or_else(|e| e.into_inner()));
        for task in tasks {
            let _ = task.await;
        }
    }
}

struct RequestRegistration {
    peer: std::sync::Weak<HelperPeer>,
    id: Uuid,
    abandon: AbandonPolicy,
    completed: bool,
}

#[derive(Clone, Copy)]
enum AbandonPolicy {
    CancelRequest,
    ClosePeer,
}

impl Drop for RequestRegistration {
    fn drop(&mut self) {
        if let Some(peer) = self.peer.upgrade() {
            if !self.completed && matches!(self.abandon, AbandonPolicy::ClosePeer) {
                // Fail synchronously before checking/removing pending. A
                // hostile early response may have already removed this ID
                // while send() still has a partial frame in the writer.
                peer.fail();
                return;
            }
            let removed = peer
                .pending
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&self.id)
                .is_some();
            if removed && !peer.is_closed() {
                let id = self.id;
                if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                    runtime.spawn(async move {
                        let _ = peer
                            .send(
                                Uuid::new_v4(),
                                "cancel_request",
                                serde_json::json!({"request_id": id}),
                            )
                            .await;
                    });
                }
            }
        }
    }
}

impl Drop for HelperPeer {
    fn drop(&mut self) {
        self.closed.cancel();
        self.writer.get_mut().take();
        for (_, pending) in self
            .pending
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .drain()
        {
            let _ = pending.sender.send(Err(AutomationError::Closed));
        }
    }
}

#[cfg(test)]
mod dependency_bounds_tests {
    use super::*;
    use serde_json::json;
    use std::{
        pin::Pin,
        task::{Context, Poll, Waker},
    };

    #[derive(Default)]
    struct Transcript {
        bytes: Vec<u8>,
        released: bool,
        blocked: Option<Waker>,
    }
    struct PartialWriter {
        transcript: Arc<Mutex<Transcript>>,
        started: Option<oneshot::Sender<()>>,
    }
    impl AsyncWrite for PartialWriter {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            raw: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            let shared = self.transcript.clone();
            let mut transcript = shared.lock().unwrap();
            if let Some(started) = self.started.take() {
                transcript.bytes.push(raw[0]);
                let _ = started.send(());
                return Poll::Ready(Ok(1));
            }
            if !transcript.released {
                transcript.blocked = Some(cx.waker().clone());
                return Poll::Pending;
            }
            transcript.bytes.extend_from_slice(raw);
            Poll::Ready(Ok(raw.len()))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dependency_registration_abandon_never_appends_cancel_after_partial_frame() {
        // No outer ExchangeGuard participates: this specifically proves the
        // nested request destructor fails synchronously on a real multithread
        // scheduler before any cancel_request task can be spawned.
        for mode in ["abort", "cancel", "deadline", "early_response_then_abort"] {
            let transcript = Arc::new(Mutex::new(Transcript::default()));
            let (started_tx, started_rx) = oneshot::channel();
            let (reader, mut server) = tokio::io::duplex(4096);
            let peer = HelperPeer::connect(
                reader,
                PartialWriter {
                    transcript: transcript.clone(),
                    started: Some(started_tx),
                },
            );
            let task_peer = peer.clone();
            let id = Uuid::new_v4();
            let cancel = CancellationToken::new();
            let task_cancel = cancel.clone();
            let worker = tokio::spawn(async move {
                let request = task_peer.request_with_id_bounded(
                    id,
                    "dependency_archive_info",
                    json!({}),
                    Duration::from_secs(30),
                    4096,
                    AbandonPolicy::ClosePeer,
                );
                match mode {
                    "deadline" => tokio::time::timeout(Duration::from_millis(30), request)
                        .await
                        .unwrap_or(Err(AutomationError::Timeout)),
                    "cancel" => tokio::select! {biased;
                        _=task_cancel.cancelled()=>Err(AutomationError::Closed),
                        result=request=>result,
                    },
                    _ => request.await,
                }
            });
            tokio::time::timeout(Duration::from_secs(1), started_rx)
                .await
                .unwrap()
                .unwrap();
            if mode == "early_response_then_abort" {
                // A hostile helper can answer a known ID before send finishes.
                // Empty pending must NOT disarm a partial-write drop policy.
                let mut response = serde_json::to_vec(&json!({"version":1,"type":"response",
                    "request_id":id,"result":{},"error":null}))
                .unwrap();
                response.push(b'\n');
                server.write_all(&response).await.unwrap();
                tokio::time::timeout(Duration::from_secs(1), async {
                    while peer.pending.lock().unwrap().contains_key(&id) {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
            }
            if mode == "cancel" {
                cancel.cancel();
            }
            if mode == "abort" || mode == "early_response_then_abort" {
                worker.abort();
            }
            let result = worker.await;
            if mode == "abort" || mode == "early_response_then_abort" {
                assert!(result.unwrap_err().is_cancelled());
            } else {
                assert!(result.unwrap().is_err());
            }
            assert!(
                peer.is_closed(),
                "nested registration must close before caller returns: {mode}"
            );
            let wake = {
                let mut log = transcript.lock().unwrap();
                log.released = true;
                log.blocked.take()
            };
            if let Some(waker) = wake {
                waker.wake();
            }
            // Permit any wrongly spawned cancel task to run/write; an eventual
            // outer close alone is insufficient evidence for this transcript.
            tokio::time::sleep(Duration::from_millis(20)).await;
            assert_eq!(
                transcript.lock().unwrap().bytes,
                b"{",
                "no cancellation suffix: {mode}"
            );
            peer.close().await;
        }
    }

    #[tokio::test]
    async fn bounded_result_failure_does_not_change_normal_request_budget() {
        let (client, server) = tokio::io::duplex(4096);
        let (reader, writer) = tokio::io::split(client);
        let peer = HelperPeer::connect(reader, writer);
        let worker = tokio::spawn(async move {
            let (reader, mut writer) = tokio::io::split(server);
            let mut reader = BufReader::new(reader);
            for _ in 0..2 {
                let raw = read_frame(&mut reader).await.unwrap().unwrap();
                let request: Value = serde_json::from_slice(&raw).unwrap();
                let mut response = serde_json::to_vec(&json!({
                    "version":1, "type":"response", "request_id":request["request_id"],
                    "result":{"payload":"x".repeat(256)}, "error":null
                }))
                .unwrap();
                response.push(b'\n');
                writer.write_all(&response).await.unwrap();
            }
            // Keep the read side alive until the peer has consumed response 2.
            let _ = read_frame(&mut reader).await;
        });
        assert!(matches!(
            peer.request_dependency_bounded("fixture", json!({}), Duration::from_secs(1), 64)
                .await,
            Err(AutomationError::Protocol(_))
        ));
        assert!(!peer.is_closed());
        let result = peer
            .request("fixture", json!({}), Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(result["payload"].as_str().unwrap().len(), 256);
        peer.close().await;
        worker.await.unwrap();
    }
}

#[cfg(test)]mod diagnostic_admission_tests{
    use super::*;
    struct Held;
    impl TargetHandler for Held{
        fn handle(&self,_:TargetRequest)->crate::TargetFuture{Box::pin(std::future::pending())}
        fn revoke(&self,_:Uuid,_:&str){}
    }
    fn frame(operation:&str)->Value{serde_json::json!({"version":1,"type":"target_request","request_id":Uuid::new_v4(),"job_id":Uuid::new_v4(),"manifest_digest":"a".repeat(64),"host_id":Uuid::new_v4(),"operation":operation,"body":{}})}
    #[tokio::test]async fn diagnostic_does_not_steal_fifth_executable_admission(){
        let(local,_remote)=tokio::io::duplex(8192);let(read,write)=tokio::io::split(local);
        let peer=HelperPeer::connect_with_targets(read,write,Some(Arc::new(Held)));
        for _ in 0..4{peer.receive_target(frame("exec")).unwrap();}
        peer.receive_target(frame("redact_event")).unwrap();
        assert_eq!(peer.target_slots.available_permits(),1);
        peer.receive_target(frame("exec")).unwrap();
        assert_eq!(peer.target_slots.available_permits(),0);
        assert_eq!(peer.diagnostic_slot.available_permits(),0);
        // A diagnostic overflow does not close/revoke the original peer.
        peer.receive_target(frame("redact_event")).unwrap();assert!(!peer.closed.is_cancelled());
        peer.fail();
        tokio::time::timeout(Duration::from_secs(1),async{while peer.target_slots.available_permits()!=5||peer.diagnostic_slot.available_permits()!=1{tokio::task::yield_now().await;}}).await.unwrap();
    }
    #[tokio::test]async fn executable_saturation_does_not_block_one_diagnostic(){
        let(local,_remote)=tokio::io::duplex(8192);let(read,write)=tokio::io::split(local);
        let peer=HelperPeer::connect_with_targets(read,write,Some(Arc::new(Held)));
        for _ in 0..5{peer.receive_target(frame("exec")).unwrap();}
        peer.receive_target(frame("redact_event")).unwrap();assert!(!peer.closed.is_cancelled());
        assert_eq!(peer.target_slots.available_permits(),0);assert_eq!(peer.diagnostic_slot.available_permits(),0);
        peer.fail();
    }
}
