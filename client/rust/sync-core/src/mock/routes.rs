//! Axum routes of the mock server: one handler per `cc_protocol::paths`
//! endpoint, plus the gate middleware (protocol version, fault injection).

use super::state::{err, Audience, MResult, MockError, Outgoing, Principal};
use super::{FaultAction, Shared};
use axum::body::{Body, Bytes};
use axum::extract::rejection::QueryRejection;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use cc_protocol::auth::{
    ChangePasswordRequest, ForgotPasswordRequest, LoginRequest, LogoutRequest, RefreshRequest,
    RegisterRequest, ResetPasswordRequest, VerifyEmailRequest,
};
use cc_protocol::devices::{
    ApproveDeviceRequest, AttestDeviceRequest, CreateDeviceTrustRequest, RejectDeviceRequest,
    UpdateDeviceRequest,
};
use cc_protocol::envelopes::RecipientType;
use cc_protocol::events::ServerEvent;
use cc_protocol::limits::MAX_PUSH_BODY_BYTES;
use cc_protocol::meta::ServerInfo;
use cc_protocol::paths;
use cc_protocol::recovery::{RecoveryMaterialQuery, ReplaceEnvelopeRequest};
use cc_protocol::sync::{ChangesQuery, PushRequest, SnapshotQuery};
use cc_protocol::vaults::{
    CreateEnvelopeRequest, CreateVaultRequest, DeleteEnvelopeRequest, DeleteVaultRequest,
};
use cc_protocol::version::{ProtocolVersion, HEADER_DEVICE_PROOF, HEADER_PROTOCOL_VERSION};
use cc_protocol::{DeviceId, EnvelopeId, ErrorCode, VaultId, PROTOCOL_VERSION};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

type S = State<Arc<Shared>>;

pub(crate) fn router(shared: Arc<Shared>) -> Router {
    Router::new()
        .route(paths::META, get(meta))
        .route(paths::AUTH_REGISTER, post(register))
        .route(paths::AUTH_LOGIN, post(login))
        .route(paths::AUTH_REFRESH, post(refresh))
        .route(paths::AUTH_LOGOUT, post(logout))
        .route(paths::AUTH_ME, get(me))
        .route(paths::AUTH_PASSWORD_FORGOT, post(forgot))
        .route(paths::AUTH_PASSWORD_RESET, post(reset))
        .route(paths::AUTH_PASSWORD_CHANGE, post(change_password))
        .route(paths::AUTH_EMAIL_VERIFY, post(verify_email))
        .route(paths::DEVICES, get(list_devices).post(create_trust_request))
        .route(paths::DEVICE, patch(update_device))
        .route(paths::DEVICE_APPROVE, post(approve))
        .route(paths::DEVICE_REJECT, post(reject))
        .route(paths::DEVICE_ATTEST, post(attest))
        .route(paths::DEVICE_REVOKE, post(revoke))
        .route(paths::VAULTS, get(list_vaults).post(create_vault))
        .route(paths::VAULT, get(get_vault).delete(delete_vault))
        .route(
            paths::VAULT_ENVELOPES,
            get(list_envelopes).post(create_envelope),
        )
        .route(paths::VAULT_ENVELOPE, delete(delete_envelope))
        .route(paths::SYNC_PUSH, post(push))
        .route(paths::SYNC_CHANGES, get(changes))
        .route(paths::SYNC_SNAPSHOT, get(snapshot))
        .route(paths::EVENTS_WS, get(events_ws))
        .route(paths::RECOVERY_ACCOUNT_START, post(forgot))
        .route(paths::RECOVERY_ACCOUNT_CONFIRM, post(reset))
        .route(paths::RECOVERY_VAULT_ENVELOPE, get(recovery_material))
        .route(
            paths::RECOVERY_VAULT_PASSWORD_REPLACE,
            post(replace_password),
        )
        .route(
            paths::RECOVERY_VAULT_RECOVERY_REPLACE,
            post(replace_recovery),
        )
        .fallback(|| async { err(ErrorCode::NotFound, "no such route") })
        .layer(DefaultBodyLimit::max(MAX_PUSH_BODY_BYTES + 1024 * 1024))
        .layer(middleware::from_fn_with_state(
            shared.clone(),
            request_proof,
        ))
        .layer(middleware::from_fn_with_state(shared.clone(), gate))
        .with_state(shared)
}

// ---- helpers ------------------------------------------------------------------------------

fn parse<T: DeserializeOwned>(body: &[u8]) -> MResult<T> {
    serde_json::from_slice(body).map_err(|_| err(ErrorCode::BadRequest, "malformed JSON body"))
}

fn json<T: Serialize>(status: StatusCode, v: T) -> Response {
    (status, Json(v)).into_response()
}

fn no_content() -> Response {
    StatusCode::NO_CONTENT.into_response()
}

fn id<T: std::str::FromStr>(s: &str) -> MResult<T> {
    s.parse().map_err(|_| err(ErrorCode::NotFound, "not found"))
}

fn query<T>(q: Result<Query<T>, QueryRejection>) -> MResult<T> {
    q.map(|Query(v)| v)
        .map_err(|_| err(ErrorCode::BadRequest, "invalid query"))
}

fn auth(s: &Shared, headers: &HeaderMap) -> MResult<Principal> {
    s.lock().authenticate(headers)
}

fn server_info(s: &Shared, upgrade_required: bool) -> ServerInfo {
    let st = s.lock();
    ServerInfo {
        server_version: "mock".into(),
        protocol_version: PROTOCOL_VERSION,
        minimum_supported_protocol: st.cfg.minimum_protocol,
        upgrade_required,
        registration_open: st.cfg.registration_open,
        email_verification_required: st.cfg.email_verification_required,
        source_code_url: None,
    }
}

fn client_version(headers: &HeaderMap) -> Option<Result<ProtocolVersion, ()>> {
    headers
        .get(HEADER_PROTOCOL_VERSION)
        .map(|v| v.to_str().ok().and_then(|s| s.parse().ok()).ok_or(()))
}

// ---- middleware -----------------------------------------------------------------------------

async fn gate(State(s): S, req: Request, next: Next) -> Response {
    let path = req.uri().path().to_owned();
    s.record(&path);
    if path != paths::META {
        let min = s.lock().cfg.minimum_protocol;
        match client_version(req.headers()) {
            None => {
                return err(ErrorCode::BadRequest, "missing x-cc-protocol-version").into_response()
            }
            Some(Err(())) => {
                return err(ErrorCode::BadRequest, "invalid x-cc-protocol-version").into_response()
            }
            Some(Ok(v)) if !PROTOCOL_VERSION.is_compatible(&v, &min) => {
                let details = serde_json::to_value(server_info(&s, true)).unwrap_or_default();
                return MockError(
                    cc_protocol::ApiError::new(
                        ErrorCode::UpgradeRequired,
                        "client upgrade required",
                    )
                    .with_details(details),
                )
                .into_response();
            }
            Some(Ok(_)) => {}
        }
    }
    match s.take_fault(&path) {
        None => next.run(req).await,
        Some(FaultAction::Error(code)) => err(code, "injected fault").into_response(),
        Some(FaultAction::RateLimit {
            retry_after_seconds,
        }) => {
            let mut e = cc_protocol::ApiError::new(ErrorCode::RateLimited, "rate limited");
            e.retry_after_seconds = Some(retry_after_seconds);
            MockError(e).into_response()
        }
        Some(FaultAction::LoseResponse) => {
            // Commit, then cut the connection before the response leaves.
            let resp = next.run(req).await;
            s.proxy.kill_all();
            tokio::time::sleep(Duration::from_millis(100)).await;
            resp
        }
        Some(FaultAction::Rewrite(f)) => {
            let resp = next.run(req).await;
            let (mut parts, body) = resp.into_parts();
            let bytes = axum::body::to_bytes(body, usize::MAX)
                .await
                .unwrap_or_default();
            let new_body = match serde_json::from_slice::<serde_json::Value>(&bytes) {
                Ok(mut v) => {
                    f(&mut v);
                    serde_json::to_vec(&v).unwrap_or_default()
                }
                Err(_) => bytes.to_vec(),
            };
            parts.headers.remove(header::CONTENT_LENGTH);
            Response::from_parts(parts, Body::from(new_body))
        }
        Some(FaultAction::InvalidJson) => {
            let resp = next.run(req).await;
            let (mut parts, _) = resp.into_parts();
            parts.headers.remove(header::CONTENT_LENGTH);
            Response::from_parts(parts, Body::from("{\"changes\": [{\"object_id\": "))
        }
    }
}

/// Protocol 1.5: verify `x-cc-device-proof` on bearer-authenticated requests
/// (incl. the WebSocket upgrade) and `POST /v1/auth/refresh`, against the
/// device of the token's session. Unknown tokens pass through (→ `401` from
/// the handler); login/register carry their own proof in the body.
async fn request_proof(State(s): S, req: Request, next: Next) -> Response {
    let path = req.uri().path().to_owned();
    let is_refresh = path == paths::AUTH_REFRESH && req.method() == axum::http::Method::POST;
    let bearer = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_owned);
    if bearer.is_none() && !is_refresh {
        return next.run(req).await;
    }
    let method = req.method().as_str().to_owned();
    let target = req
        .uri()
        .path_and_query()
        .map_or_else(|| path.clone(), |pq| pq.as_str().to_owned());
    let (parts, body) = req.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, MAX_PUSH_BODY_BYTES + 1024 * 1024).await else {
        return err(ErrorCode::PayloadTooLarge, "body too large").into_response();
    };
    let device = {
        let st = s.lock();
        match &bearer {
            Some(token) => st.device_of_access_token(token),
            None => serde_json::from_slice::<RefreshRequest>(&bytes)
                .ok()
                .and_then(|r| st.device_of_refresh_token(r.refresh_token.expose_secret())),
        }
    };
    if let Some((device_id, key)) = device {
        let header = parts
            .headers
            .get(HEADER_DEVICE_PROOF)
            .map(|v| v.to_str().unwrap_or("\u{0}"));
        if let Err(e) = s
            .lock()
            .verify_request_proof(device_id, &key, &method, &target, &bytes, header)
        {
            return e.into_response();
        }
    }
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}

// ---- meta & auth ------------------------------------------------------------------------------

async fn meta(State(s): S, headers: HeaderMap) -> Response {
    let min = s.lock().cfg.minimum_protocol;
    let upgrade = matches!(client_version(&headers), Some(Ok(v)) if !PROTOCOL_VERSION.is_compatible(&v, &min));
    json(StatusCode::OK, server_info(&s, upgrade))
}

async fn register(State(s): S, body: Bytes) -> MResult<Response> {
    let req: RegisterRequest = parse(&body)?;
    Ok(json(StatusCode::CREATED, s.lock().register(req)?))
}

async fn login(State(s): S, body: Bytes) -> MResult<Response> {
    let req: LoginRequest = parse(&body)?;
    Ok(json(StatusCode::OK, s.lock().login(req)?))
}

async fn refresh(State(s): S, body: Bytes) -> MResult<Response> {
    let req: RefreshRequest = parse(&body)?;
    Ok(json(StatusCode::OK, s.lock().refresh(req)?))
}

async fn logout(State(s): S, headers: HeaderMap, body: Bytes) -> MResult<Response> {
    let p = auth(&s, &headers)?;
    let req: LogoutRequest = if body.is_empty() {
        LogoutRequest::default()
    } else {
        parse(&body)?
    };
    s.lock().logout(p, req);
    Ok(no_content())
}

async fn me(State(s): S, headers: HeaderMap) -> MResult<Response> {
    let p = auth(&s, &headers)?;
    Ok(json(StatusCode::OK, s.lock().me(p)?))
}

async fn forgot(State(s): S, body: Bytes) -> MResult<Response> {
    let req: ForgotPasswordRequest = parse(&body)?;
    s.lock().forgot(&req.email);
    Ok(StatusCode::ACCEPTED.into_response())
}

async fn reset(State(s): S, body: Bytes) -> MResult<Response> {
    let req: ResetPasswordRequest = parse(&body)?;
    s.lock().reset(req)?;
    Ok(no_content())
}

async fn change_password(State(s): S, headers: HeaderMap, body: Bytes) -> MResult<Response> {
    let p = auth(&s, &headers)?;
    let req: ChangePasswordRequest = parse(&body)?;
    s.lock().change_password(p, req)?;
    Ok(no_content())
}

async fn verify_email(State(s): S, body: Bytes) -> MResult<Response> {
    let req: VerifyEmailRequest = parse(&body)?;
    s.lock().verify_email(req.token.expose_secret())?;
    Ok(no_content())
}

// ---- devices ----------------------------------------------------------------------------------

async fn list_devices(State(s): S, headers: HeaderMap) -> MResult<Response> {
    let p = auth(&s, &headers)?;
    Ok(json(StatusCode::OK, s.lock().list_devices(p)))
}

async fn create_trust_request(State(s): S, headers: HeaderMap, body: Bytes) -> MResult<Response> {
    let p = auth(&s, &headers)?;
    let req: CreateDeviceTrustRequest = if body.is_empty() {
        CreateDeviceTrustRequest::default()
    } else {
        parse(&body)?
    };
    Ok(json(
        StatusCode::CREATED,
        s.lock().create_trust_request(p, req)?,
    ))
}

async fn update_device(
    State(s): S,
    headers: HeaderMap,
    Path(device): Path<String>,
    body: Bytes,
) -> MResult<Response> {
    let p = auth(&s, &headers)?;
    let device_id: DeviceId = id(&device)?;
    let req: UpdateDeviceRequest = parse(&body)?;
    Ok(json(
        StatusCode::OK,
        s.lock().update_device(p, device_id, req)?,
    ))
}

async fn approve(
    State(s): S,
    headers: HeaderMap,
    Path(device): Path<String>,
    body: Bytes,
) -> MResult<Response> {
    let p = auth(&s, &headers)?;
    let device_id: DeviceId = id(&device)?;
    let req: ApproveDeviceRequest = parse(&body)?;
    Ok(json(StatusCode::OK, s.lock().approve(p, device_id, req)?))
}

async fn reject(
    State(s): S,
    headers: HeaderMap,
    Path(device): Path<String>,
    body: Bytes,
) -> MResult<Response> {
    let p = auth(&s, &headers)?;
    let device_id: DeviceId = id(&device)?;
    let req: RejectDeviceRequest = parse(&body)?;
    s.lock().reject(p, device_id, req)?;
    Ok(no_content())
}

async fn attest(
    State(s): S,
    headers: HeaderMap,
    Path(device): Path<String>,
    body: Bytes,
) -> MResult<Response> {
    let p = auth(&s, &headers)?;
    let device_id: DeviceId = id(&device)?;
    let req: AttestDeviceRequest = parse(&body)?;
    Ok(json(StatusCode::OK, s.lock().attest(p, device_id, req)?))
}

async fn revoke(State(s): S, headers: HeaderMap, Path(device): Path<String>) -> MResult<Response> {
    let p = auth(&s, &headers)?;
    let device_id: DeviceId = id(&device)?;
    s.lock().revoke(p, device_id)?;
    Ok(no_content())
}

// ---- vaults & envelopes -------------------------------------------------------------------------

async fn create_vault(State(s): S, headers: HeaderMap, body: Bytes) -> MResult<Response> {
    let p = auth(&s, &headers)?;
    let req: CreateVaultRequest = parse(&body)?;
    Ok(json(StatusCode::CREATED, s.lock().create_vault(p, req)?))
}

async fn list_vaults(State(s): S, headers: HeaderMap) -> MResult<Response> {
    let p = auth(&s, &headers)?;
    Ok(json(StatusCode::OK, s.lock().list_vaults(p)))
}

async fn get_vault(
    State(s): S,
    headers: HeaderMap,
    Path(vault): Path<String>,
) -> MResult<Response> {
    let p = auth(&s, &headers)?;
    let vault_id: VaultId = id(&vault)?;
    Ok(json(StatusCode::OK, s.lock().get_vault(p, vault_id)?))
}

async fn delete_vault(
    State(s): S,
    headers: HeaderMap,
    Path(vault): Path<String>,
    body: Bytes,
) -> MResult<Response> {
    let p = auth(&s, &headers)?;
    let vault_id: VaultId = id(&vault)?;
    let req: DeleteVaultRequest = if body.is_empty() {
        DeleteVaultRequest::default()
    } else {
        parse(&body)?
    };
    s.lock().delete_vault(p, vault_id, req.vault_access_key)?;
    Ok(no_content())
}

async fn list_envelopes(
    State(s): S,
    headers: HeaderMap,
    Path(vault): Path<String>,
) -> MResult<Response> {
    let p = auth(&s, &headers)?;
    let vault_id: VaultId = id(&vault)?;
    Ok(json(StatusCode::OK, s.lock().list_envelopes(p, vault_id)?))
}

async fn create_envelope(
    State(s): S,
    headers: HeaderMap,
    Path(vault): Path<String>,
    body: Bytes,
) -> MResult<Response> {
    let p = auth(&s, &headers)?;
    let vault_id: VaultId = id(&vault)?;
    let req: CreateEnvelopeRequest = parse(&body)?;
    Ok(json(
        StatusCode::CREATED,
        s.lock().create_envelope(p, vault_id, req)?,
    ))
}

async fn delete_envelope(
    State(s): S,
    headers: HeaderMap,
    Path((vault, envelope)): Path<(String, String)>,
    body: Bytes,
) -> MResult<Response> {
    let p = auth(&s, &headers)?;
    let vault_id: VaultId = id(&vault)?;
    let envelope_id: EnvelopeId = id(&envelope)?;
    let req: DeleteEnvelopeRequest = parse(&body)?;
    s.lock()
        .delete_envelope(p, vault_id, envelope_id, Some(req.vault_access_key))?;
    Ok(no_content())
}

// ---- sync -----------------------------------------------------------------------------------------

async fn push(State(s): S, headers: HeaderMap, body: Bytes) -> MResult<Response> {
    let p = auth(&s, &headers)?;
    let req: PushRequest = parse(&body)?;
    let (status, resp) = s.lock().push(p, req)?;
    Ok(json(status, resp))
}

async fn changes(
    State(s): S,
    headers: HeaderMap,
    q: Result<Query<ChangesQuery>, QueryRejection>,
) -> MResult<Response> {
    let p = auth(&s, &headers)?;
    let q = query(q)?;
    Ok(json(StatusCode::OK, s.lock().changes(p, q)?))
}

async fn snapshot(
    State(s): S,
    headers: HeaderMap,
    q: Result<Query<SnapshotQuery>, QueryRejection>,
) -> MResult<Response> {
    let p = auth(&s, &headers)?;
    let q = query(q)?;
    Ok(json(StatusCode::OK, s.lock().snapshot(p, q)?))
}

// ---- recovery ----------------------------------------------------------------------------------------

async fn recovery_material(
    State(s): S,
    headers: HeaderMap,
    q: Result<Query<RecoveryMaterialQuery>, QueryRejection>,
) -> MResult<Response> {
    let p = auth(&s, &headers)?;
    let q = query(q)?;
    Ok(json(
        StatusCode::OK,
        s.lock().recovery_material(p, q.vault_id)?,
    ))
}

async fn replace_password(State(s): S, headers: HeaderMap, body: Bytes) -> MResult<Response> {
    let p = auth(&s, &headers)?;
    let req: ReplaceEnvelopeRequest = parse(&body)?;
    Ok(json(
        StatusCode::OK,
        s.lock().replace_envelope(p, req, RecipientType::Password)?,
    ))
}

async fn replace_recovery(State(s): S, headers: HeaderMap, body: Bytes) -> MResult<Response> {
    let p = auth(&s, &headers)?;
    let req: ReplaceEnvelopeRequest = parse(&body)?;
    Ok(json(
        StatusCode::OK,
        s.lock().replace_envelope(p, req, RecipientType::Recovery)?,
    ))
}

// ---- websocket ----------------------------------------------------------------------------------------

/// Close code: this session/device was revoked.
pub(crate) const CLOSE_REVOKED: u16 = 4001;
/// Close code: the client fell behind; reconnect and pull.
pub(crate) const CLOSE_LAGGED: u16 = 4002;

async fn events_ws(State(s): S, headers: HeaderMap, upgrade: WebSocketUpgrade) -> Response {
    let p = match auth(&s, &headers) {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    let rx = s.events.subscribe();
    upgrade.on_upgrade(move |socket| ws_session(s, p, socket, rx))
}

async fn send_event(socket: &mut WebSocket, ev: &ServerEvent) -> Result<(), ()> {
    let text = serde_json::to_string(ev).map_err(|_| ())?;
    socket
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| ())
}

async fn close(socket: &mut WebSocket, code: u16, reason: &str) {
    let _ = socket
        .send(Message::Close(Some(CloseFrame {
            code,
            reason: reason.to_owned().into(),
        })))
        .await;
}

async fn ws_session(
    s: Arc<Shared>,
    p: Principal,
    mut socket: WebSocket,
    mut rx: broadcast::Receiver<Outgoing>,
) {
    let hello = ServerEvent::Hello {
        protocol_version: PROTOCOL_VERSION,
        server_time: chrono::Utc::now(),
        session_id: p.session_id,
    };
    if send_event(&mut socket, &hello).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            ev = rx.recv() => match ev {
                Ok(out) => {
                    if let Some(code) = out.force_close {
                        close(&mut socket, code, "server closed").await;
                        return;
                    }
                    let visible = match out.audience {
                        Audience::User(u) => u == p.user_id,
                        Audience::Vault(v) => s.lock().is_member(p.user_id, v),
                    };
                    if visible && send_event(&mut socket, &out.event).await.is_err() {
                        return;
                    }
                    if out.close_device == Some(p.device_id) || out.close_sessions.contains(&p.session_id) {
                        close(&mut socket, CLOSE_REVOKED, "revoked").await;
                        return;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    close(&mut socket, CLOSE_LAGGED, "lagged").await;
                    return;
                }
                Err(broadcast::error::RecvError::Closed) => return,
            },
            msg = socket.recv() => match msg {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return,
                Some(Ok(_)) => {}
            },
        }
    }
}
