//! Realtime events: WebSocket client for `GET /v1/events/ws` (rustls,
//! platform certificate verifier) with Bearer auth, a device proof on the
//! upgrade request (protocol 1.5, when the [`ApiClient`] has a signer),
//! token refresh on `401`, keep-alive pings and reconnect with backoff
//! (never earlier than a `429`'s `Retry-After`, protocol 1.11).
//! Events are hints; the [`crate::SyncEngine`] pulls after `vault_changed`.

use crate::api::{ApiClient, ApiError, ProofRejection};
use crate::backoff::BackoffConfig;
use crate::error::StopReason;
use cc_protocol::events::ServerEvent;
use cc_protocol::version::{
    HEADER_CLIENT_VERSION, HEADER_DEVICE_PROOF, HEADER_PLATFORM, HEADER_PROTOCOL_VERSION,
};
use cc_protocol::{ErrorCode, PROTOCOL_VERSION};
use futures::{SinkExt, StreamExt};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream};

type Ws = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// What the event stream reports to subscribers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsEvent {
    /// (Re)connected — subscribers should catch up (pull).
    Connected,
    /// A server event (including the initial `hello`).
    Event(ServerEvent),
    /// Connection lost or not established; reconnecting with backoff.
    Disconnected { reason: String },
    /// Gave up for good (device revoked, session gone, upgrade required).
    Terminated { reason: StopReason },
}

/// Event stream settings.
#[derive(Debug, Clone)]
pub struct EventStreamConfig {
    pub backoff: BackoffConfig,
    /// Client ping interval; a missing pong by the next tick drops the
    /// connection.
    pub ping_interval: Duration,
    /// Broadcast channel capacity.
    pub capacity: usize,
}

impl Default for EventStreamConfig {
    fn default() -> Self {
        Self {
            backoff: BackoffConfig {
                initial: Duration::from_secs(1),
                max: Duration::from_secs(60),
                multiplier: 2.0,
            },
            ping_interval: Duration::from_secs(30),
            capacity: 256,
        }
    }
}

/// Running WebSocket client. Dropping it does not stop the task; call
/// [`EventStream::shutdown`].
#[derive(Debug)]
pub struct EventStream {
    tx: broadcast::Sender<WsEvent>,
    shutdown: watch::Sender<bool>,
    task: JoinHandle<()>,
}

#[derive(Debug)]
enum ConnectError {
    Retry(String),
    /// `429 rate_limited` (e.g. `ws_connect_account`, `ws_connections`):
    /// reconnect no earlier than `after`.
    RateLimited {
        after: Option<Duration>,
        limit: Option<String>,
    },
    Terminal(StopReason),
}

#[cfg(test)]
impl ConnectError {
    /// Server-requested minimum delay before the next attempt.
    fn retry_after(&self) -> Option<Duration> {
        match self {
            ConnectError::RateLimited { after, .. } => *after,
            _ => None,
        }
    }
}

enum PumpEnd {
    Shutdown,
    Closed(String),
    /// Close code 4001: this session/device was revoked.
    Revoked,
    /// Close code 4002: we fell behind; reconnect immediately and pull.
    Lagged,
    /// Close code 4003: the access token this socket was opened with expired
    /// or was rotated; reconnect now (connect() refreshes the token).
    TokenExpired,
}

/// Server close code: session or device revoked (stop, surface).
pub const CLOSE_CODE_REVOKED: u16 = cc_protocol::events::CLOSE_REVOKED;
/// Server close code: client lagged behind (reconnect + pull).
pub const CLOSE_CODE_LAGGED: u16 = cc_protocol::events::CLOSE_LAGGED;
/// Server close code: access token expired/rotated (refresh + reconnect).
pub const CLOSE_CODE_TOKEN_EXPIRED: u16 = cc_protocol::events::CLOSE_TOKEN_EXPIRED;

impl EventStream {
    /// Connect in the background and keep reconnecting.
    pub fn spawn(api: ApiClient, config: EventStreamConfig) -> Self {
        let (tx, _) = broadcast::channel(config.capacity.max(16));
        let (shutdown, rx) = watch::channel(false);
        let task = tokio::spawn(run(api, config, tx.clone(), rx));
        Self { tx, shutdown, task }
    }

    /// Subscribe to events (only events after subscribing are received).
    pub fn subscribe(&self) -> broadcast::Receiver<WsEvent> {
        self.tx.subscribe()
    }

    /// Close the connection and stop reconnecting.
    pub async fn shutdown(self) {
        let _ = self.shutdown.send(true);
        let _ = tokio::time::timeout(Duration::from_secs(5), self.task).await;
    }
}

async fn run(
    api: ApiClient,
    cfg: EventStreamConfig,
    tx: broadcast::Sender<WsEvent>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut attempt: u32 = 0;
    loop {
        if *shutdown.borrow() {
            return;
        }
        let mut reconnect_now = false;
        let mut retry_after = None;
        match connect(&api).await {
            Ok(ws) => {
                attempt = 0;
                let _ = tx.send(WsEvent::Connected);
                match pump(ws, &tx, &mut shutdown, cfg.ping_interval).await {
                    PumpEnd::Shutdown => return,
                    PumpEnd::Closed(reason) => {
                        tracing::debug!(%reason, "event stream disconnected");
                        let _ = tx.send(WsEvent::Disconnected { reason });
                    }
                    PumpEnd::Lagged => {
                        let _ = tx.send(WsEvent::Disconnected {
                            reason: "lagged".into(),
                        });
                        reconnect_now = true;
                    }
                    PumpEnd::TokenExpired => {
                        let _ = tx.send(WsEvent::Disconnected {
                            reason: "token expired".into(),
                        });
                        reconnect_now = true;
                    }
                    PumpEnd::Revoked => {
                        // Find out what was revoked (device vs session) with
                        // one immediate attempt; either way we stop unless it
                        // unexpectedly succeeds.
                        let reason = match connect(&api).await {
                            Ok(_) => {
                                reconnect_now = true;
                                None
                            }
                            Err(ConnectError::Terminal(r)) => Some(r),
                            // Throttled after authentication: the session
                            // is alive; reconnect when the server allows.
                            Err(ConnectError::RateLimited { after, .. }) => {
                                retry_after = after;
                                None
                            }
                            Err(ConnectError::Retry(_)) => Some(StopReason::ReauthRequired),
                        };
                        if let Some(reason) = reason {
                            tracing::info!(%reason, "event stream closed: revoked");
                            let _ = tx.send(WsEvent::Terminated { reason });
                            return;
                        }
                    }
                }
            }
            Err(ConnectError::Terminal(reason)) => {
                tracing::info!(%reason, "event stream terminated");
                let _ = tx.send(WsEvent::Terminated { reason });
                return;
            }
            Err(ConnectError::Retry(reason)) => {
                let _ = tx.send(WsEvent::Disconnected { reason });
            }
            Err(ConnectError::RateLimited { after, limit }) => {
                retry_after = after;
                tracing::info!(
                    retry_after_s = retry_after.map(|d| d.as_secs()),
                    limit = limit.as_deref().unwrap_or("-"),
                    "event stream rate limited"
                );
                let _ = tx.send(WsEvent::Disconnected {
                    reason: "rate limited".into(),
                });
            }
        }
        if reconnect_now {
            continue;
        }
        attempt = attempt.saturating_add(1);
        let delay = reconnect_delay(&cfg.backoff, attempt, retry_after);
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            r = shutdown.changed() => {
                if r.is_err() || *shutdown.borrow() { return; }
            }
        }
    }
}

/// Delay before reconnect attempt `attempt`: exponential backoff with
/// jitter, never shorter than the server's `Retry-After`.
fn reconnect_delay(
    backoff: &BackoffConfig,
    attempt: u32,
    retry_after: Option<Duration>,
) -> Duration {
    backoff.delay(attempt, retry_after)
}

/// A `429` answer to the upgrade request → [`ConnectError::RateLimited`]
/// with the delay from `retry_after_seconds` (body) or `Retry-After`.
fn rate_limited(resp: &tungstenite::http::Response<Option<Vec<u8>>>) -> ConnectError {
    let error = resp
        .body()
        .as_ref()
        .and_then(|b| serde_json::from_slice::<cc_protocol::ApiError>(b).ok());
    let header = resp
        .headers()
        .get(tungstenite::http::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_secs);
    let after = error
        .as_ref()
        .and_then(|e| e.retry_after_seconds)
        .map(|s| Duration::from_secs(u64::from(s)))
        .or(header);
    let limit = error
        .as_ref()
        .and_then(|e| e.details.as_ref())
        .and_then(|d| d.get("limit"))
        .and_then(|l| l.as_str())
        .filter(|l| crate::api::is_limit_name(l))
        .map(str::to_owned);
    ConnectError::RateLimited { after, limit }
}

fn classify_api(e: ApiError) -> ConnectError {
    match e {
        ApiError::RateLimited { retry_after, limit } => ConnectError::RateLimited {
            after: retry_after,
            limit,
        },
        ApiError::DeviceRevoked => ConnectError::Terminal(StopReason::DeviceRevoked),
        ApiError::AccountDisabled => ConnectError::Terminal(StopReason::AccountDisabled),
        ApiError::UpgradeRequired { .. } => ConnectError::Terminal(StopReason::UpgradeRequired),
        e if e.requires_reauth() => ConnectError::Terminal(StopReason::ReauthRequired),
        e => ConnectError::Retry(e.to_string()),
    }
}

fn tls_connector() -> Result<Connector, String> {
    static CONFIG: OnceLock<Result<Arc<rustls::ClientConfig>, String>> = OnceLock::new();
    let cfg = CONFIG.get_or_init(|| {
        use rustls_platform_verifier::BuilderVerifierExt;
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .and_then(|b| b.with_platform_verifier())
            .map(|b| Arc::new(b.with_no_client_auth()))
            .map_err(|e| format!("tls config: {e}"))
    });
    cfg.clone().map(Connector::Rustls)
}

async fn connect(api: &ApiClient) -> Result<Ws, ConnectError> {
    let url = api.events_url();
    let connector = if url.scheme() == "wss" {
        Some(tls_connector().map_err(ConnectError::Retry)?)
    } else {
        Some(Connector::Plain)
    };
    let mut stale_retried = false;
    let mut attempt = 0;
    while attempt < 2 {
        attempt += 1;
        let token = api.access_token().await.map_err(classify_api)?;
        // Fresh proof per attempt: GET + events path (+ query), empty body.
        let proof = api
            .request_proof_header("GET", &url, b"")
            .map_err(|e| ConnectError::Retry(e.to_string()))?;
        let mut req = url
            .as_str()
            .into_client_request()
            .map_err(|e| ConnectError::Retry(format!("request: {e}")))?;
        {
            let h = req.headers_mut();
            let auth = HeaderValue::from_str(&format!("Bearer {}", token.expose_secret()))
                .map_err(|_| ConnectError::Retry("invalid token header".into()))?;
            h.insert("authorization", auth);
            if let Some(proof) = proof {
                let v = HeaderValue::from_str(&proof)
                    .map_err(|_| ConnectError::Retry("invalid proof header".into()))?;
                h.insert(HEADER_DEVICE_PROOF, v);
            }
            let cfg = api.config();
            for (name, value) in [
                (HEADER_PROTOCOL_VERSION, PROTOCOL_VERSION.to_string()),
                (HEADER_CLIENT_VERSION, cfg.client_version.clone()),
                (HEADER_PLATFORM, cfg.platform.as_str().to_owned()),
            ] {
                if let Ok(v) = HeaderValue::from_str(&value) {
                    h.insert(name, v);
                }
            }
        }
        match tokio_tungstenite::connect_async_tls_with_config(req, None, true, connector.clone())
            .await
        {
            Ok((ws, _resp)) => return Ok(ws),
            Err(tungstenite::Error::Http(resp)) => {
                let status = resp.status().as_u16();
                let error = resp
                    .body()
                    .as_ref()
                    .and_then(|b| serde_json::from_slice::<cc_protocol::ApiError>(b).ok());
                let code = error.as_ref().map(|e| e.code);
                match (status, code) {
                    (422, Some(ErrorCode::InvalidProof)) => {
                        let reason = ProofRejection::parse(
                            error
                                .as_ref()
                                .and_then(|e| e.details.as_ref())
                                .and_then(|d| d.get("reason"))
                                .and_then(|r| r.as_str()),
                        );
                        if reason == ProofRejection::Stale && !stale_retried {
                            stale_retried = true;
                            attempt -= 1; // re-sign once, not counted as an auth attempt
                            continue;
                        }
                        tracing::warn!(
                            reason = reason.as_str(),
                            "websocket request proof rejected"
                        );
                        return Err(ConnectError::Retry(
                            ApiError::RequestProofRejected { reason }.to_string(),
                        ));
                    }
                    (401, Some(ErrorCode::RefreshTokenReused)) => {
                        return Err(ConnectError::Terminal(StopReason::ReauthRequired))
                    }
                    (401, _) if attempt == 1 => {
                        api.refresh_after_rejection(token.expose_secret())
                            .await
                            .map_err(classify_api)?;
                        continue;
                    }
                    (401, _) => return Err(ConnectError::Terminal(StopReason::ReauthRequired)),
                    (403, Some(ErrorCode::DeviceRevoked)) => {
                        return Err(ConnectError::Terminal(StopReason::DeviceRevoked))
                    }
                    (403, Some(ErrorCode::AccountDisabled)) => {
                        return Err(ConnectError::Terminal(StopReason::AccountDisabled))
                    }
                    (426, _) => return Err(ConnectError::Terminal(StopReason::UpgradeRequired)),
                    (429, _) => return Err(rate_limited(&resp)),
                    _ => {
                        return Err(ConnectError::Retry(format!(
                            "websocket upgrade refused ({status})"
                        )))
                    }
                }
            }
            Err(e) => return Err(ConnectError::Retry(format!("websocket: {e}"))),
        }
    }
    Err(ConnectError::Retry(
        "websocket authentication failed".into(),
    ))
}

async fn pump(
    ws: Ws,
    tx: &broadcast::Sender<WsEvent>,
    shutdown: &mut watch::Receiver<bool>,
    ping_every: Duration,
) -> PumpEnd {
    let (mut sink, mut stream) = ws.split();
    let mut ping = tokio::time::interval(ping_every);
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ping.tick().await; // first tick fires immediately
    let mut awaiting = false;
    loop {
        tokio::select! {
            r = shutdown.changed() => {
                if r.is_err() || *shutdown.borrow() {
                    let _ = sink.send(Message::Close(None)).await;
                    return PumpEnd::Shutdown;
                }
            }
            _ = ping.tick() => {
                if awaiting {
                    return PumpEnd::Closed("ping timeout".into());
                }
                awaiting = true;
                if let Err(e) = sink.send(Message::Ping(Default::default())).await {
                    return PumpEnd::Closed(format!("send ping: {e}"));
                }
            }
            msg = stream.next() => {
                awaiting = false;
                match msg {
                    None => return PumpEnd::Closed("connection closed".into()),
                    Some(Err(e)) => return PumpEnd::Closed(format!("read: {e}")),
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<ServerEvent>(text.as_str()) {
                            Ok(ev) => {
                                let _ = tx.send(WsEvent::Event(ev));
                            }
                            Err(_) => tracing::warn!("ignoring malformed server event"),
                        }
                    }
                    Some(Ok(Message::Close(frame))) => {
                        return match frame.map(|f| u16::from(f.code)) {
                            Some(CLOSE_CODE_REVOKED) => PumpEnd::Revoked,
                            Some(CLOSE_CODE_LAGGED) => PumpEnd::Lagged,
                            Some(CLOSE_CODE_TOKEN_EXPIRED) => PumpEnd::TokenExpired,
                            _ => PumpEnd::Closed("server closed the connection".into()),
                        };
                    }
                    // Pings are answered by tungstenite; flush the queued pong.
                    Some(Ok(Message::Ping(_))) => {
                        let _ = sink.flush().await;
                    }
                    Some(Ok(_)) => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tungstenite::http::Response;

    fn response(
        retry_after_header: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> Response<Option<Vec<u8>>> {
        let mut b = Response::builder().status(429);
        if let Some(v) = retry_after_header {
            b = b.header("retry-after", v);
        }
        b.body(body.map(|j| serde_json::to_vec(&j).unwrap()))
            .unwrap()
    }

    fn after_of(e: &ConnectError) -> Option<Duration> {
        assert!(matches!(e, ConnectError::RateLimited { .. }), "{e:?}");
        e.retry_after()
    }

    #[test]
    fn upgrade_429_reads_retry_after_from_body_then_header() {
        let body = serde_json::json!({
            "code": "rate_limited",
            "message": "too many requests",
            "retry_after_seconds": 120,
            "details": { "limit": "ws_connect_account" }
        });
        let e = rate_limited(&response(Some("7"), Some(body)));
        assert_eq!(after_of(&e), Some(Duration::from_secs(120)));
        let ConnectError::RateLimited { limit, .. } = e else {
            unreachable!()
        };
        assert_eq!(limit.as_deref(), Some("ws_connect_account"));

        // Header only (no or unparsable body).
        let e = rate_limited(&response(Some("45"), None));
        assert_eq!(after_of(&e), Some(Duration::from_secs(45)));
        let e = rate_limited(&response(Some(" 9 "), Some(serde_json::json!("garbage"))));
        assert_eq!(after_of(&e), Some(Duration::from_secs(9)));

        // Nothing usable: plain backoff.
        let e = rate_limited(&response(Some("Wed, 21 Oct 2026 07:28:00 GMT"), None));
        assert_eq!(after_of(&e), None);
    }

    #[test]
    fn rate_limited_token_refresh_keeps_its_delay() {
        let e = classify_api(ApiError::RateLimited {
            retry_after: Some(Duration::from_secs(30)),
            limit: Some("auth_ip".into()),
        });
        assert_eq!(after_of(&e), Some(Duration::from_secs(30)));
        assert!(matches!(
            classify_api(ApiError::Network("down".into())),
            ConnectError::Retry(_)
        ));
    }

    #[test]
    fn reconnect_waits_at_least_retry_after() {
        let backoff = EventStreamConfig::default().backoff;
        for attempt in [1, 2, 5, 30] {
            // Longer than the backoff cap: exactly Retry-After.
            assert_eq!(
                reconnect_delay(&backoff, attempt, Some(Duration::from_secs(600))),
                Duration::from_secs(600)
            );
            // Never below Retry-After, never above the cap without one.
            let d = reconnect_delay(&backoff, attempt, Some(Duration::from_secs(3)));
            assert!(d >= Duration::from_secs(3), "{d:?}");
            assert!(reconnect_delay(&backoff, attempt, None) <= backoff.max);
        }
        // Without Retry-After the first attempt is quick.
        assert!(reconnect_delay(&backoff, 1, None) <= Duration::from_secs(1));
    }
}
