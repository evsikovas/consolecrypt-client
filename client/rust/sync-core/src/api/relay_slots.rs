//! One-use Relay carrier only. Never an Agent grant, key or destination.
//! Fixed once-only signed exchanges deliberately bypass refresh/re-sign loops.
use super::*;
use zeroize::{Zeroize,Zeroizing};
use futures::{Sink, Stream};
use serde::{Deserialize, Serialize};
use std::{
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Weak,
    },
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_tungstenite::{
    tungstenite::{self, client::IntoClientRequest, protocol::WebSocketConfig, Message},
    Connector, MaybeTlsStream, WebSocketStream,
};

// Private bounded HTTP-output custody. Only our allocation is covered, not
// reqwest/rustls internal buffers. Capacity is fixed before sensitive chunks.
struct SensitiveRelayOutput {
    bytes:Zeroizing<Vec<u8>>,
    #[cfg(test)] wiped:Option<Arc<std::sync::atomic::AtomicBool>>,
}
impl SensitiveRelayOutput {
    fn new()->Self {Self{bytes:Zeroizing::new(Vec::with_capacity(4096)),#[cfg(test)]wiped:None}}
    fn finish(mut self)->Zeroizing<Vec<u8>> {std::mem::replace(&mut self.bytes,Zeroizing::new(Vec::new()))}
}
impl std::ops::Deref for SensitiveRelayOutput {type Target=Vec<u8>;fn deref(&self)->&Vec<u8>{&self.bytes}}
impl std::ops::DerefMut for SensitiveRelayOutput {fn deref_mut(&mut self)->&mut Vec<u8>{&mut self.bytes}}
impl Drop for SensitiveRelayOutput {fn drop(&mut self){
    self.bytes.as_mut_slice().zeroize();
    #[cfg(test)] if let Some(wiped)=&self.wiped {
        wiped.store(!self.bytes.is_empty()&&self.bytes.iter().all(|byte|*byte==0),Ordering::SeqCst);
    }
}}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelaySlotLimits {
    pub ttl_seconds: u64,
    pub idle_seconds: u64,
    pub max_bytes: u64,
}
impl RelaySlotLimits {
    fn validate(self) -> Result<(), RelaySlotError> {
        if self.ttl_seconds == 0
            || self.ttl_seconds > 300
            || self.idle_seconds == 0
            || self.idle_seconds > 30
            || self.idle_seconds > self.ttl_seconds
            || self.max_bytes == 0
            || self.max_bytes > 8 * 1024 * 1024
        {
            return Err(RelaySlotError::Rejected);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelaySlotError {
    Rejected,
    Unavailable,
    OutcomeUnknown,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Created {
    slot_id: uuid::Uuid,
    agent_token: SecretString,
    expires_in_seconds: u64,
}
/// Only this capability may be explicitly provisioned to Agent. No account token.
pub struct RelayAgentCapability(SecretString);
impl std::fmt::Debug for RelayAgentCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RelayAgentCapability([redacted])")
    }
}
impl RelayAgentCapability {
    pub fn provision_once(self) -> SecretString {
        self.0
    }
}
fn capability(value: &str) -> bool {
    let Some(t) = value.strip_prefix("ccrl_") else {
        return false;
    };
    t.len() == 43
        && t.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        && t.as_bytes()
            .last()
            .is_some_and(|c| b"AEIMQUYcgkosw048".contains(c))
}
pub struct RelaySlot {
    client: Weak<Inner>,
    subject: Arc<ReaderAuthenticatedSubject>,
    id: uuid::Uuid,
    deadline: tokio::time::Instant,
    agent: Option<RelayAgentCapability>,
    attached: AtomicBool,
    revoked: AtomicBool,
}
impl std::fmt::Debug for RelaySlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RelaySlot([redacted])")
    }
}
fn unavailable(_: ApiError) -> RelaySlotError {
    RelaySlotError::Unavailable
}
fn api(
    client: &Weak<Inner>,
    subject: &ReaderAuthenticatedSubject,
) -> Result<ApiClient, RelaySlotError> {
    let api = ApiClient {
        inner: client.upgrade().ok_or(RelaySlotError::Unavailable)?,
    };
    subject.check_current(&api).map_err(unavailable)?;
    Ok(api)
}
impl ApiClient {
    pub async fn create_relay_slot_once(
        &self,
        subject: ReaderAuthenticatedSubject,
        limits: RelaySlotLimits,
    ) -> Result<RelaySlot, RelaySlotError> {
        limits.validate()?;
        let started = tokio::time::Instant::now();
        let deadline = started + Duration::from_secs(limits.ttl_seconds);
        let body = serde_json::to_vec(&limits).map_err(|_| RelaySlotError::Rejected)?;
        let raw = self
            .relay_exchange_once(&subject, Method::POST, "/v1/relay/slots", body, deadline)
            .await?;
        let created: Created =
            serde_json::from_slice(&raw).map_err(|_| RelaySlotError::OutcomeUnknown)?;
        if created.slot_id.is_nil()
            || !capability(created.agent_token.expose_secret())
            || created.expires_in_seconds == 0
            || created.expires_in_seconds > limits.ttl_seconds
        {
            return Err(RelaySlotError::OutcomeUnknown);
        }
        subject
            .check_current(self)
            .map_err(|_| RelaySlotError::OutcomeUnknown)?;
        let deadline = deadline.min(started + Duration::from_secs(created.expires_in_seconds));
        if tokio::time::Instant::now() >= deadline {
            return Err(RelaySlotError::OutcomeUnknown);
        }
        Ok(RelaySlot {
            client: Arc::downgrade(&self.inner),
            subject: Arc::new(subject),
            id: created.slot_id,
            deadline,
            agent: Some(RelayAgentCapability(created.agent_token)),
            attached: AtomicBool::new(false),
            revoked: AtomicBool::new(false),
        })
    }
    async fn relay_exchange_once(
        &self,
        subject: &ReaderAuthenticatedSubject,
        method: Method,
        path: &str,
        body: Vec<u8>,
        deadline: tokio::time::Instant,
    ) -> Result<Vec<u8>, RelaySlotError> {
        self.relay_exchange_status_once(subject,method,path,body,deadline).await.map(|(_,raw)|raw.to_vec())
    }
    // Fixed sibling control API needs exact200/204 semantics. The existing
    // slot wrapper retains its successful2xx behavior and all original checks.
    pub(super) async fn relay_exchange_status_once(
        &self, subject:&ReaderAuthenticatedSubject, method:Method, path:&str,
        body:Vec<u8>, deadline:tokio::time::Instant,
    )->Result<(StatusCode,Zeroizing<Vec<u8>>),RelaySlotError> {
        if self.config().server_url.scheme() != "https" || body.len() > 1024 {
            return Err(RelaySlotError::Rejected);
        }
        subject.check_current(self).map_err(unavailable)?;
        let tokens = self
            .cached_tokens()
            .await
            .map_err(unavailable)?
            .ok_or(RelaySlotError::Unavailable)?;
        if tokens.session_id != subject.session_id()
            || tokens.access_expires_at <= chrono::Utc::now()
        {
            return Err(RelaySlotError::Unavailable);
        }
        let url = self.endpoint(path);
        let proof = subject
            .with_current(self, || {
                self.request_proof_header(method.as_str(), &url, &body)
            })
            .map_err(unavailable)?
            .map_err(unavailable)?
            .ok_or(RelaySlotError::Unavailable)?;
        let mut http = reqwest::Client::builder()
            .retry(reqwest::retry::never())
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .connect_timeout(Duration::from_secs(5));
        if let Some(tls)=self.config().extra_server_tls(true).map_err(|_|RelaySlotError::Unavailable)?{http=http.tls_backend_preconfigured((*tls).clone());}
        let http=http.build().map_err(|_|RelaySlotError::Unavailable)?;
        let mut request = http
            .request(method, url)
            .bearer_auth(tokens.access_token.expose_secret())
            .header(HEADER_DEVICE_PROOF, proof)
            .header(HEADER_PROTOCOL_VERSION, PROTOCOL_VERSION.to_string())
            .header(HEADER_CLIENT_VERSION, &self.config().client_version)
            .header(HEADER_PLATFORM, self.config().platform.as_str());
        if !body.is_empty() {
            request = request.header(CONTENT_TYPE, "application/json").body(body);
        }
        subject.check_current(self).map_err(unavailable)?;
        // Marked as dispatched from here. Any transport/parse/status uncertainty
        // is terminal; never refresh, re-sign, resubmit or create a replacement.
        let limit = deadline.min(tokio::time::Instant::now() + Duration::from_secs(5));
        let exchange = async {
            let mut response = request
                .send()
                .await
                .map_err(|_| RelaySlotError::OutcomeUnknown)?;
            if !response.status().is_success() {
                return Err(RelaySlotError::OutcomeUnknown);
            }
            let status=response.status();
            if response.content_length().is_some_and(|n| n > 4096) {
                return Err(RelaySlotError::OutcomeUnknown);
            }
            // Fixed cap avoids a reallocating secret-bearing accumulation.
            // Native-owned bytes wipe on rejection, cancellation and return.
            let mut output = SensitiveRelayOutput::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| RelaySlotError::OutcomeUnknown)?
            {
                if output.len() + chunk.len() > 4096 {
                    return Err(RelaySlotError::OutcomeUnknown);
                }
                output.extend_from_slice(&chunk);
            }
            subject
                .check_current(self)
                .map_err(|_| RelaySlotError::OutcomeUnknown)?;
            Ok((status,output.finish()))
        };
        tokio::time::timeout_at(limit, exchange)
            .await
            .map_err(|_| RelaySlotError::OutcomeUnknown)?
    }
}
impl RelaySlot {
    pub async fn retire_control_carrier_once(&self)->Result<(),RelaySlotError> {
        if self.revoked.swap(true,Ordering::SeqCst) {return Err(RelaySlotError::Rejected)}
        let api=api(&self.client,&self.subject)?;
        let path=format!("/v1/relay/slots/{}",self.id);
        let (status,raw)=api.relay_exchange_status_once(&self.subject,Method::DELETE,&path,Vec::new(),self.deadline).await?;
        if status!=StatusCode::NO_CONTENT || !raw.is_empty() {return Err(RelaySlotError::OutcomeUnknown)} Ok(())
    }
    pub(super) fn control_deadline(&self)->Result<tokio::time::Instant,RelaySlotError> {
        self.remaining_seconds()?;Ok(self.deadline)
    }
    pub(super) fn check_control_owner(&self,api:&ApiClient,subject:&ReaderAuthenticatedSubject,
        remaining:u64)->Result<(),RelaySlotError> {
        subject.check_current(api).map_err(unavailable)?;
        self.subject.check_current(api).map_err(unavailable)?;
        if !Weak::ptr_eq(&self.client,&Arc::downgrade(&api.inner))
            || self.subject.user_id()!=subject.user_id() || self.subject.device_id()!=subject.device_id()
            || self.subject.session_id()!=subject.session_id() || self.remaining_seconds()?>remaining
            || self.attached.load(Ordering::SeqCst) || self.revoked.load(Ordering::SeqCst) {
            return Err(RelaySlotError::Rejected)
        }
        Ok(())
    }
    /// Read-only ORIGINAL deadline observation; never renews or mints a slot.
    pub fn remaining_seconds(&self)->Result<u64,RelaySlotError>{
        if self.revoked.load(Ordering::SeqCst){return Err(RelaySlotError::Rejected);}
        let current=api(&self.client,&self.subject)?;
        let seconds=self.deadline.checked_duration_since(tokio::time::Instant::now()).ok_or(RelaySlotError::Rejected)?.as_secs();
        if seconds==0||seconds>300||self.revoked.load(Ordering::SeqCst){return Err(RelaySlotError::Rejected);}
        self.subject.check_current(&current).map_err(unavailable)?;Ok(seconds)
    }
    pub fn id(&self) -> uuid::Uuid {
        self.id
    }
    pub fn take_agent_capability(&mut self) -> Result<RelayAgentCapability, RelaySlotError> {
        self.agent.take().ok_or(RelaySlotError::Rejected)
    }
    pub async fn connect_client_once(&self) -> Result<RelayBinaryStream, RelaySlotError> {
        if self.attached.swap(true, Ordering::SeqCst)
            || self.revoked.load(Ordering::SeqCst)
            || tokio::time::Instant::now() >= self.deadline
        {
            return Err(RelaySlotError::Rejected);
        }
        let api = api(&self.client, &self.subject)?;
        let tokens = api
            .cached_tokens()
            .await
            .map_err(unavailable)?
            .ok_or(RelaySlotError::Unavailable)?;
        if tokens.session_id != self.subject.session_id()
            || tokens.access_expires_at <= chrono::Utc::now()
        {
            return Err(RelaySlotError::Unavailable);
        }
        let path = format!("/v1/relay/slots/{}/client", self.id);
        let mut url = api.endpoint(&path);
        if url.scheme() != "https" {
            return Err(RelaySlotError::Rejected);
        }
        let proof = self
            .subject
            .with_current(&api, || api.request_proof_header("GET", &url, &[]))
            .map_err(unavailable)?
            .map_err(unavailable)?
            .ok_or(RelaySlotError::Unavailable)?;
        url.set_scheme("wss")
            .map_err(|_| RelaySlotError::Rejected)?;
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|_| RelaySlotError::Rejected)?;
        let headers = request.headers_mut();
        for (name, value) in [
            (
                "authorization",
                format!("Bearer {}", tokens.access_token.expose_secret()),
            ),
            (HEADER_DEVICE_PROOF, proof),
            (HEADER_PROTOCOL_VERSION, PROTOCOL_VERSION.to_string()),
            (HEADER_CLIENT_VERSION, api.config().client_version.clone()),
            (HEADER_PLATFORM, api.config().platform.as_str().to_owned()),
        ] {
            headers.insert(
                name,
                tungstenite::http::HeaderValue::from_str(&value)
                    .map_err(|_| RelaySlotError::Rejected)?,
            );
        }
        use rustls_platform_verifier::BuilderVerifierExt;
        let tls=match api.config().extra_server_tls(true).map_err(|_|RelaySlotError::Unavailable)? {
            Some(tls)=>tls,
            None=>{let provider=Arc::new(rustls::crypto::aws_lc_rs::default_provider());Arc::new(rustls::ClientConfig::builder_with_provider(provider)
                .with_protocol_versions(&[&rustls::version::TLS13]).map_err(|_|RelaySlotError::Unavailable)?
                .with_platform_verifier().map_err(|_|RelaySlotError::Unavailable)?.with_no_client_auth())}
        };
        let config = WebSocketConfig::default()
            .max_frame_size(Some(16384))
            .max_message_size(Some(16384))
            .write_buffer_size(0)
            .max_write_buffer_size(17408);
        self.subject.check_current(&api).map_err(unavailable)?;
        let limit = self
            .deadline
            .min(tokio::time::Instant::now() + Duration::from_secs(5));
        let result = tokio::time::timeout_at(
            limit,
            tokio_tungstenite::connect_async_tls_with_config(
                request,
                Some(config),
                true,
                Some(Connector::Rustls(tls)),
            ),
        )
        .await;
        let (socket, _) = result
            .map_err(|_| RelaySlotError::OutcomeUnknown)?
            .map_err(|_| RelaySlotError::OutcomeUnknown)?;
        self.subject
            .check_current(&api)
            .map_err(|_| RelaySlotError::OutcomeUnknown)?;
        let subject = self.subject.clone();
        let cancelled = Box::pin(async move { subject.cancelled().await });
        Ok(RelayBinaryStream {
            socket,
            pending: None,
            closed: false,
            client: self.client.clone(),
            subject: self.subject.clone(),
            cancelled,
            timer: Box::pin(tokio::time::sleep_until(self.deadline)),
        })
    }
    pub async fn revoke_once(&self) -> Result<(), RelaySlotError> {
        if self.revoked.swap(true, Ordering::SeqCst) {
            return Err(RelaySlotError::Rejected);
        }
        let api = api(&self.client, &self.subject)?;
        let path = format!("/v1/relay/slots/{}", self.id);
        let raw = api
            .relay_exchange_once(
                &self.subject,
                Method::DELETE,
                &path,
                Vec::new(),
                self.deadline,
            )
            .await?;
        if !raw.is_empty() {
            return Err(RelaySlotError::OutcomeUnknown);
        }
        Ok(())
    }
}
/// Maintained WebSocket binary frames as bytes, no unbounded producer queue.
pub struct RelayBinaryStream {
    socket: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    pending: Option<(bytes::Bytes, usize)>,
    closed: bool,
    client: Weak<Inner>,
    subject: Arc<ReaderAuthenticatedSubject>,
    cancelled: Pin<Box<dyn std::future::Future<Output = ()> + Send>>,
    timer: Pin<Box<tokio::time::Sleep>>,
}
impl std::fmt::Debug for RelayBinaryStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RelayBinaryStream([redacted])")
    }
}
fn io_denied() -> std::io::Error {
    std::io::Error::other("relay unavailable")
}
impl RelayBinaryStream {
    /// Original ApiClient identity/generation check; metadata is not authority.
    pub fn check_original(&self, api: &ApiClient) -> Result<(), RelaySlotError> {
        self.subject.check_current(api).map_err(unavailable)
    }
    pub fn original_user_id(&self) -> cc_protocol::UserId {
        self.subject.user_id()
    }
    pub fn original_device_id(&self) -> cc_protocol::DeviceId {
        self.subject.device_id()
    }
    fn live(&mut self, cx: &mut Context<'_>) -> std::io::Result<()> {
        use std::future::Future;
        if self.closed
            || self.cancelled.as_mut().poll(cx).is_ready()
            || self.timer.as_mut().poll(cx).is_ready()
            || api(&self.client, &self.subject).is_err()
        {
            self.closed = true;
            return Err(io_denied());
        }
        Ok(())
    }
}
impl AsyncRead for RelayBinaryStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if let Err(e) = self.live(cx) {
            return Poll::Ready(Err(e));
        }
        if out.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        for _ in 0..16 {
            if let Some((bytes, pos)) = &mut self.pending {
                let n = out.remaining().min(bytes.len() - *pos);
                out.put_slice(&bytes[*pos..*pos + n]);
                *pos += n;
                if *pos == bytes.len() {
                    self.pending = None;
                }
                return Poll::Ready(Ok(()));
            }
            match Pin::new(&mut self.socket).poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None | Some(Ok(Message::Close(_)))) => {
                    self.closed = true;
                    return Poll::Ready(Ok(()));
                }
                Poll::Ready(Some(Ok(Message::Binary(bytes)))) if bytes.len() <= 16384 => {
                    if !bytes.is_empty() {
                        self.pending = Some((bytes, 0));
                    }
                }
                Poll::Ready(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => {}
                Poll::Ready(_) => {
                    self.closed = true;
                    return Poll::Ready(Err(io_denied()));
                }
            }
        }
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}
impl AsyncWrite for RelayBinaryStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if let Err(e) = self.live(cx) {
            return Poll::Ready(Err(e));
        }
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        match Pin::new(&mut self.socket).poll_flush(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(_)) => return Poll::Ready(Err(io_denied())),
            Poll::Ready(Ok(())) => {}
        }
        match Pin::new(&mut self.socket).poll_ready(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(_)) => Poll::Ready(Err(io_denied())),
            Poll::Ready(Ok(())) => {
                let n = bytes.len().min(16384);
                if Pin::new(&mut self.socket)
                    .start_send(Message::Binary(bytes[..n].to_vec().into()))
                    .is_err()
                {
                    return Poll::Ready(Err(io_denied()));
                }
                Poll::Ready(Ok(n))
            }
        }
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        if let Err(e) = self.live(cx) {
            return Poll::Ready(Err(e));
        }
        Pin::new(&mut self.socket)
            .poll_flush(cx)
            .map_err(|_| io_denied())
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.closed = true;
        Pin::new(&mut self.socket)
            .poll_close(cx)
            .map_err(|_| io_denied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capability_role_prefix_and_base64_canonical_are_closed() {
        assert!(capability(&format!("ccrl_{}A", "A".repeat(42))));
        for p in ["cct_", "Bearer ccrl_", "ccrl_"] {
            assert!(!capability(&format!("{p}{}B", "A".repeat(42))));
        }
    }
    #[test]
    fn relay_limits_never_imply_node_access() {
        assert!(RelaySlotLimits {
            ttl_seconds: 300,
            idle_seconds: 30,
            max_bytes: 8 * 1024 * 1024
        }
        .validate()
        .is_ok());
        for n in [0, 8 * 1024 * 1024 + 1] {
            assert!(RelaySlotLimits {
                ttl_seconds: 300,
                idle_seconds: 30,
                max_bytes: n
            }
            .validate()
            .is_err());
        }
    }
}

#[cfg(test)]
mod sensitive_output_tests {
    use super::*;
    #[tokio::test]
    async fn cancelling_pending_actual_accumulator_wipes_native_owned_bytes() {
        let wiped=Arc::new(AtomicBool::new(false));let observed=wiped.clone();
        let (entered,ready)=tokio::sync::oneshot::channel();
        let mut pending=Box::pin(async move {
            let mut output=SensitiveRelayOutput::new();output.wiped=Some(observed);
            output.extend_from_slice(b"ccrc_native-owned-sensitive-output");
            entered.send(()).unwrap();std::future::pending::<()>().await;drop(output);
        });
        tokio::select! {biased;_=&mut pending=>panic!("fixture should remain pending"),_=ready=>{}}
        assert!(!wiped.load(Ordering::SeqCst));drop(pending);
        assert!(wiped.load(Ordering::SeqCst));
    }
}
