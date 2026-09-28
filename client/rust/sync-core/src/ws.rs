//! Realtime events: WebSocket client for `GET /v1/events/ws` (rustls,
//! platform certificate verifier) with Bearer auth, a device proof on the
//! upgrade request (protocol 1.5, when the [`ApiClient`] has a signer),
//! token refresh on `401`, keep-alive pings and reconnect with backoff.
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

enum ConnectError {
    Retry(String),
    Terminal(StopReason),
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
        }
        if reconnect_now {
            continue;
        }
        attempt = attempt.saturating_add(1);
        let delay = cfg.backoff.delay(attempt, None);
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            r = shutdown.changed() => {
                if r.is_err() || *shutdown.borrow() { return; }
            }
        }
    }
}

fn classify_api(e: ApiError) -> ConnectError {
    match e {
        ApiError::DeviceRevoked => ConnectError::Terminal(StopReason::DeviceRevoked),
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
                    (426, _) => return Err(ConnectError::Terminal(StopReason::UpgradeRequired)),
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
