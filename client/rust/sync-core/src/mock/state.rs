//! In-memory server state and the protocol rules (ADR-0003 sync semantics,
//! ADR-0004 authorization matrix, token rotation with reuse detection).

use super::{MockServerConfig, RequestProofStats};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use cc_protocol::auth::{
    AccountInfo, AuthResponse, ChangePasswordRequest, LoginRequest, LogoutRequest, RefreshRequest,
    RegisterRequest, ResetPasswordRequest, SecretString, TokenPair,
};
use cc_protocol::canonical::{device_approval_message, device_login_message};
use cc_protocol::devices::{
    ApproveDeviceRequest, AttestDeviceRequest, CreateDeviceTrustRequest, DeviceInfo,
    DeviceRegistration, DeviceRequestStatus, DeviceStatus, DeviceTrustRequest, ListDevicesResponse,
    RejectDeviceRequest, UpdateDeviceRequest,
};
use cc_protocol::envelopes::{KeyEnvelope, NewEnvelope, RecipientType};
use cc_protocol::events::ServerEvent;
use cc_protocol::limits::{
    DEFAULT_PAGE_LIMIT, ED25519_PUBLIC_KEY_LEN, ED25519_SIGNATURE_LEN, MAX_ACCOUNT_PASSWORD_LEN,
    MAX_DEVICE_NAME_LEN, MAX_OBJECT_CIPHERTEXT_BYTES, MAX_PAGE_LIMIT, MAX_PUSH_BATCH,
    MAX_SIGNATURE_SKEW_SECONDS, MIN_ACCOUNT_PASSWORD_LEN, NONCE_LEN, VAULT_ACCESS_KEY_LEN,
    WRAPPED_DEK_LEN, X25519_PUBLIC_KEY_LEN,
};
use cc_protocol::recovery::{ReplaceEnvelopeRequest, VaultRecoveryMaterial};
use cc_protocol::sync::{
    Change, ChangesQuery, ChangesResponse, EncryptedBody, MutationOp, MutationResult, PushRequest,
    PushResponse, SnapshotQuery, SnapshotResponse, OBJECT_FORMAT_V1,
};
use cc_protocol::vaults::{
    CreateEnvelopeRequest, CreateVaultRequest, ListEnvelopesResponse, ListVaultsResponse,
    VaultInfo, VaultRole, VaultState,
};
use cc_protocol::{
    ApiError, Bytes, DeviceId, DeviceRequestId, EnvelopeId, ErrorCode, MutationId, ObjectId,
    SessionId, Timestamp, UserId, VaultId,
};
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use tokio::sync::broadcast;
use uuid::Uuid;

// ---- errors -------------------------------------------------------------------------

/// Protocol error response.
#[derive(Debug)]
pub(crate) struct MockError(pub(crate) ApiError);

impl MockError {
    fn with_details(self, details: serde_json::Value) -> Self {
        Self(self.0.with_details(details))
    }
}

impl IntoResponse for MockError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.0.code.http_status())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut resp = (status, axum::Json(&self.0)).into_response();
        if let Some(s) = self.0.retry_after_seconds {
            if let Ok(v) = s.to_string().parse() {
                resp.headers_mut()
                    .insert(axum::http::header::RETRY_AFTER, v);
            }
        }
        resp
    }
}

pub(crate) fn err(code: ErrorCode, message: &str) -> MockError {
    MockError(ApiError::new(code, message))
}

pub(crate) type MResult<T> = Result<T, MockError>;

fn not_found() -> MockError {
    err(ErrorCode::NotFound, "not found")
}

fn bad(message: &str) -> MockError {
    err(ErrorCode::BadRequest, message)
}

// ---- events ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Audience {
    User(UserId),
    Vault(VaultId),
}

/// An event to fan out to WebSocket sessions, optionally closing some.
#[derive(Debug, Clone)]
pub(crate) struct Outgoing {
    pub(crate) audience: Audience,
    pub(crate) event: ServerEvent,
    pub(crate) close_device: Option<DeviceId>,
    pub(crate) close_sessions: Vec<SessionId>,
    /// Close every socket with this code (test hook).
    pub(crate) force_close: Option<u16>,
}

// ---- model ------------------------------------------------------------------------------

struct User {
    user_id: UserId,
    email: String,
    password: String,
    email_verified: bool,
    created_at: Timestamp,
}

struct Device {
    reg: DeviceRegistration,
    user_id: UserId,
    status: DeviceStatus,
    created_at: Timestamp,
    last_seen_at: Option<Timestamp>,
    revoked_at: Option<Timestamp>,
}

struct Session {
    user_id: UserId,
    device_id: DeviceId,
    revoked: bool,
}

struct AccessToken {
    session_id: SessionId,
    expires_at: Timestamp,
}

struct RefreshToken {
    session_id: SessionId,
    used: bool,
    expires_at: Timestamp,
}

/// Latest server state of one object (exposed to tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockObject {
    pub revision: i64,
    pub sequence: i64,
    pub deleted: bool,
    pub body: Option<EncryptedBody>,
    pub writer_device_id: DeviceId,
    pub updated_at: Timestamp,
}

struct Vault {
    vault_id: VaultId,
    owner: UserId,
    vak_hash: [u8; 32],
    state: VaultState,
    created_at: Timestamp,
    updated_at: Timestamp,
    last_sequence: i64,
    /// Protocol 1.3 vault epoch (random at creation; rotated on restore).
    epoch: uuid::Uuid,
    objects: HashMap<ObjectId, MockObject>,
    /// Accepted mutations only (conflicts are not recorded, ADR-0003).
    mutations: HashMap<MutationId, (ObjectId, MutationResult)>,
    envelopes: Vec<KeyEnvelope>,
}

/// Saved sync state of one vault (test hook: backups for restore drills).
struct Checkpoint {
    sequence: i64,
    objects: HashMap<ObjectId, MockObject>,
    mutations: HashMap<MutationId, (ObjectId, MutationResult)>,
}

struct TrustReq {
    request_id: DeviceRequestId,
    device_id: DeviceId,
    user_id: UserId,
    vault_ids: Vec<VaultId>,
    status: DeviceRequestStatus,
    created_at: Timestamp,
    expires_at: Timestamp,
    approved_by: Option<DeviceId>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenPurpose {
    VerifyEmail,
    ResetPassword,
}

struct EmailToken {
    user_id: UserId,
    purpose: TokenPurpose,
    used: bool,
}

/// Authenticated caller (principal A of ADR-0004).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Principal {
    pub(crate) user_id: UserId,
    pub(crate) device_id: DeviceId,
    pub(crate) session_id: SessionId,
}

pub(crate) struct State {
    pub(crate) cfg: MockServerConfig,
    users: HashMap<UserId, User>,
    emails: HashMap<String, UserId>,
    devices: HashMap<DeviceId, Device>,
    sessions: HashMap<SessionId, Session>,
    access: HashMap<String, AccessToken>,
    refresh: HashMap<String, RefreshToken>,
    vaults: HashMap<VaultId, Vault>,
    requests: HashMap<DeviceRequestId, TrustReq>,
    email_tokens: HashMap<String, EmailToken>,
    /// Protocol 1.4: login-proof nonces already used, per device (replay guard).
    login_nonces: HashSet<(DeviceId, [u8; 32])>,
    /// Test hook: vault "backups" by vault, oldest first.
    checkpoints: HashMap<VaultId, Vec<Checkpoint>>,
    /// Protocol 1.5: request-proof nonces already used, per device.
    request_nonces: HashSet<(DeviceId, [u8; 32])>,
    pub(crate) proof_stats: RequestProofStats,
    events: broadcast::Sender<Outgoing>,
}

fn random_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn sha256(b: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(&Sha256::digest(b));
    out
}

fn ct_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn validate_registration(d: &DeviceRegistration) -> MResult<()> {
    if d.encryption_public_key.len() != X25519_PUBLIC_KEY_LEN
        || d.signing_public_key.len() != ED25519_PUBLIC_KEY_LEN
    {
        return Err(bad("device public keys must be 32 bytes"));
    }
    let n = d.name.trim().chars().count();
    if n == 0 || n > MAX_DEVICE_NAME_LEN {
        return Err(bad("invalid device name"));
    }
    Ok(())
}

fn validate_envelope(
    e: &NewEnvelope,
    expected: RecipientType,
    recipient: Option<Uuid>,
) -> MResult<()> {
    e.validate().map_err(|_| bad("invalid envelope"))?;
    if e.recipient_type != expected || e.recipient_id != recipient {
        return Err(bad("unexpected envelope recipient"));
    }
    Ok(())
}

pub(crate) fn invalid_proof(reason: &str) -> MockError {
    MockError(
        ApiError::new(ErrorCode::InvalidProof, "device proof rejected")
            .with_details(serde_json::json!({ "reason": reason })),
    )
}

impl State {
    /// Protocol 1.4 (ADR-0006): verify a login/register device proof against
    /// `signing_public_key`. `required` = the device id is already known.
    fn check_device_proof(
        &mut self,
        device_id: DeviceId,
        signing_public_key: &cc_protocol::Bytes,
        proof: Option<&cc_protocol::devices::DeviceProof>,
        required: bool,
    ) -> MResult<()> {
        let Some(proof) = proof else {
            return if required {
                Err(invalid_proof("device_proof_required"))
            } else {
                Ok(())
            };
        };
        if (proof.issued_at - Utc::now().timestamp()).abs() > MAX_SIGNATURE_SKEW_SECONDS {
            return Err(invalid_proof("stale"));
        }
        let (Some(nonce), Some(vk_bytes), Some(sig_bytes)) = (
            proof.nonce.to_array::<32>(),
            signing_public_key.to_array::<32>(),
            proof.signature.to_array::<ED25519_SIGNATURE_LEN>(),
        ) else {
            return Err(invalid_proof("invalid_signature"));
        };
        let msg = device_login_message(device_id, proof.issued_at, &nonce);
        let ok = ed25519_dalek::VerifyingKey::from_bytes(&vk_bytes)
            .ok()
            .is_some_and(|vk| {
                vk.verify_strict(&msg, &ed25519_dalek::Signature::from_bytes(&sig_bytes))
                    .is_ok()
            });
        if !ok {
            return Err(invalid_proof("invalid_signature"));
        }
        if !self.login_nonces.insert((device_id, nonce)) {
            return Err(invalid_proof("replayed"));
        }
        Ok(())
    }

    pub(crate) fn new(cfg: MockServerConfig, events: broadcast::Sender<Outgoing>) -> Self {
        Self {
            cfg,
            users: HashMap::new(),
            emails: HashMap::new(),
            devices: HashMap::new(),
            sessions: HashMap::new(),
            access: HashMap::new(),
            refresh: HashMap::new(),
            vaults: HashMap::new(),
            requests: HashMap::new(),
            email_tokens: HashMap::new(),
            login_nonces: HashSet::new(),
            checkpoints: HashMap::new(),
            request_nonces: HashSet::new(),
            proof_stats: RequestProofStats::default(),
            events,
        }
    }

    fn emit(&self, out: Outgoing) {
        let _ = self.events.send(out);
    }

    fn emit_to(&self, audience: Audience, event: ServerEvent) {
        self.emit(Outgoing {
            audience,
            event,
            close_device: None,
            close_sessions: Vec::new(),
            force_close: None,
        });
    }

    // ---- request proofs (protocol 1.5) -------------------------------------------------

    /// Device (id, signing key) bound to an access token's session, expired
    /// or not; `None` for unknown tokens (the handler answers `401`).
    pub(crate) fn device_of_access_token(&self, token: &str) -> Option<(DeviceId, Bytes)> {
        let at = self.access.get(token)?;
        self.session_device(at.session_id)
    }

    /// Device bound to a refresh token's session (verified before reuse
    /// detection, like the real server).
    pub(crate) fn device_of_refresh_token(&self, token: &str) -> Option<(DeviceId, Bytes)> {
        let rt = self.refresh.get(token)?;
        self.session_device(rt.session_id)
    }

    fn session_device(&self, session_id: SessionId) -> Option<(DeviceId, Bytes)> {
        let s = self.sessions.get(&session_id)?;
        let d = self.devices.get(&s.device_id)?;
        Some((s.device_id, d.reg.signing_public_key.clone()))
    }

    /// Verify an `x-cc-device-proof` header for one request.
    pub(crate) fn verify_request_proof(
        &mut self,
        device_id: DeviceId,
        signing_public_key: &Bytes,
        method: &str,
        path_and_query: &str,
        body: &[u8],
        header: Option<&str>,
    ) -> MResult<()> {
        let Some(header) = header else {
            self.proof_stats.missing += 1;
            return if self.cfg.require_request_proof {
                Err(invalid_proof("missing"))
            } else {
                Ok(())
            };
        };
        let result = (|| {
            let proof = cc_protocol::devices::RequestProof::decode(header)
                .ok_or_else(|| invalid_proof("malformed"))?;
            let skew = cc_protocol::limits::MAX_REQUEST_PROOF_SKEW_SECONDS;
            if (proof.issued_at - Utc::now().timestamp()).abs() > skew {
                return Err(invalid_proof("stale"));
            }
            let body_hash: [u8; 32] = sha256(body);
            let msg = cc_protocol::canonical::request_proof_message(
                device_id,
                method,
                path_and_query,
                &body_hash,
                proof.issued_at,
                &proof.nonce,
            );
            let ok = signing_public_key
                .to_array::<32>()
                .and_then(|k| ed25519_dalek::VerifyingKey::from_bytes(&k).ok())
                .is_some_and(|vk| {
                    vk.verify_strict(
                        &msg,
                        &ed25519_dalek::Signature::from_bytes(&proof.signature),
                    )
                    .is_ok()
                });
            if !ok {
                return Err(invalid_proof("invalid_signature"));
            }
            if !self.request_nonces.insert((device_id, proof.nonce)) {
                return Err(invalid_proof("replayed"));
            }
            Ok(())
        })();
        match &result {
            Ok(()) => self.proof_stats.valid += 1,
            Err(_) => self.proof_stats.rejected += 1,
        }
        result
    }

    // ---- authentication ---------------------------------------------------------------

    /// Resolve the Bearer token to principal A (re-checked on every request).
    pub(crate) fn authenticate(&mut self, headers: &HeaderMap) -> MResult<Principal> {
        let unauthorized = || err(ErrorCode::Unauthorized, "missing or invalid access token");
        let token = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(unauthorized)?;
        let at = self.access.get(token).ok_or_else(unauthorized)?;
        if at.expires_at <= Utc::now() {
            return Err(unauthorized());
        }
        let session_id = at.session_id;
        let session = self.sessions.get(&session_id).ok_or_else(unauthorized)?;
        let (user_id, device_id, revoked) = (session.user_id, session.device_id, session.revoked);
        let device = self.devices.get_mut(&device_id).ok_or_else(unauthorized)?;
        if device.status == DeviceStatus::Revoked {
            return Err(err(ErrorCode::DeviceRevoked, "device has been revoked"));
        }
        if revoked {
            return Err(unauthorized());
        }
        device.last_seen_at = Some(Utc::now());
        Ok(Principal {
            user_id,
            device_id,
            session_id,
        })
    }

    fn issue_tokens(&mut self, session_id: SessionId) -> TokenPair {
        let now = Utc::now();
        let access = random_token();
        let refresh = random_token();
        let access_expires_at = now + self.cfg.access_token_ttl;
        let refresh_expires_at = now + self.cfg.refresh_token_ttl;
        self.access.insert(
            access.clone(),
            AccessToken {
                session_id,
                expires_at: access_expires_at,
            },
        );
        self.refresh.insert(
            refresh.clone(),
            RefreshToken {
                session_id,
                used: false,
                expires_at: refresh_expires_at,
            },
        );
        TokenPair {
            session_id,
            access_token: SecretString::new(access),
            access_expires_at,
            refresh_token: SecretString::new(refresh),
            refresh_expires_at,
        }
    }

    fn new_session(&mut self, user_id: UserId, device_id: DeviceId) -> TokenPair {
        let session_id = SessionId::new();
        self.sessions.insert(
            session_id,
            Session {
                user_id,
                device_id,
                revoked: false,
            },
        );
        self.issue_tokens(session_id)
    }

    fn revoke_sessions(&mut self, pred: impl Fn(&Session) -> bool) -> Vec<SessionId> {
        let mut ids = Vec::new();
        for (id, s) in self.sessions.iter_mut() {
            if !s.revoked && pred(s) {
                s.revoked = true;
                ids.push(*id);
            }
        }
        ids
    }

    fn close_sessions(&self, user_id: UserId, ids: Vec<SessionId>) {
        for id in &ids {
            self.emit(Outgoing {
                audience: Audience::User(user_id),
                event: ServerEvent::SessionRevoked { session_id: *id },
                close_device: None,
                close_sessions: vec![*id],
                force_close: None,
            });
        }
    }

    // ---- auth endpoints -----------------------------------------------------------------

    pub(crate) fn register(&mut self, req: RegisterRequest) -> MResult<AuthResponse> {
        if !self.cfg.registration_open {
            return Err(err(ErrorCode::Forbidden, "registration is closed"));
        }
        let email = req.email.trim().to_lowercase();
        if !email.contains('@') || email.len() > 320 {
            return Err(bad("invalid email"));
        }
        let pw_len = req.password.expose_secret().len();
        if !(MIN_ACCOUNT_PASSWORD_LEN..=MAX_ACCOUNT_PASSWORD_LEN).contains(&pw_len) {
            return Err(bad("password length out of bounds"));
        }
        validate_registration(&req.device)?;
        if self.emails.contains_key(&email) {
            return Err(err(ErrorCode::AlreadyExists, "already registered")
                .with_details(serde_json::json!({ "field": "email" })));
        }
        if self.devices.contains_key(&req.device.device_id) {
            return Err(
                err(ErrorCode::AlreadyExists, "device id already registered")
                    .with_details(serde_json::json!({ "field": "device_id" })),
            );
        }
        self.check_device_proof(
            req.device.device_id,
            &req.device.signing_public_key,
            req.device_proof.as_ref(),
            false,
        )?;
        let user_id = UserId::new();
        let now = Utc::now();
        self.users.insert(
            user_id,
            User {
                user_id,
                email: email.clone(),
                password: req.password.expose_secret().to_owned(),
                email_verified: false,
                created_at: now,
            },
        );
        self.emails.insert(email, user_id);
        self.email_tokens.insert(
            random_token(),
            EmailToken {
                user_id,
                purpose: TokenPurpose::VerifyEmail,
                used: false,
            },
        );
        let device_id = req.device.device_id;
        self.devices.insert(
            device_id,
            Device {
                reg: req.device,
                user_id,
                status: DeviceStatus::Active,
                created_at: now,
                last_seen_at: Some(now),
                revoked_at: None,
            },
        );
        let tokens = self.new_session(user_id, device_id);
        Ok(AuthResponse {
            user_id,
            device_id,
            device_status: DeviceStatus::Active,
            email_verified: false,
            tokens,
        })
    }

    pub(crate) fn login(&mut self, req: LoginRequest) -> MResult<AuthResponse> {
        let invalid = || err(ErrorCode::InvalidCredentials, "invalid email or password");
        let email = req.email.trim().to_lowercase();
        let user_id = *self.emails.get(&email).ok_or_else(invalid)?;
        let user = self.users.get(&user_id).ok_or_else(invalid)?;
        if user.password != req.password.expose_secret() {
            return Err(invalid());
        }
        if self.cfg.email_verification_required && !user.email_verified {
            return Err(err(ErrorCode::EmailNotVerified, "email not verified"));
        }
        let email_verified = user.email_verified;
        validate_registration(&req.device)?;
        let device_id = req.device.device_id;
        match self.devices.get(&device_id) {
            Some(d) => {
                if d.user_id != user_id
                    || d.reg.encryption_public_key != req.device.encryption_public_key
                    || d.reg.signing_public_key != req.device.signing_public_key
                {
                    return Err(err(ErrorCode::AlreadyExists, "device keys do not match")
                        .with_details(serde_json::json!({ "field": "device_id" })));
                }
                if d.status == DeviceStatus::Revoked {
                    return Err(err(ErrorCode::DeviceRevoked, "device has been revoked"));
                }
                // Known device: proof of possession of its signing key is REQUIRED.
                let signing_key = d.reg.signing_public_key.clone();
                self.check_device_proof(device_id, &signing_key, req.device_proof.as_ref(), true)?;
            }
            None => {
                self.check_device_proof(
                    device_id,
                    &req.device.signing_public_key,
                    req.device_proof.as_ref(),
                    false,
                )?;
                let now = Utc::now();
                self.devices.insert(
                    device_id,
                    Device {
                        reg: req.device,
                        user_id,
                        status: DeviceStatus::Active,
                        created_at: now,
                        last_seen_at: Some(now),
                        revoked_at: None,
                    },
                );
                self.emit_to(
                    Audience::User(user_id),
                    ServerEvent::DeviceAdded { device_id },
                );
            }
        }
        let tokens = self.new_session(user_id, device_id);
        Ok(AuthResponse {
            user_id,
            device_id,
            device_status: DeviceStatus::Active,
            email_verified,
            tokens,
        })
    }

    pub(crate) fn refresh(&mut self, req: RefreshRequest) -> MResult<TokenPair> {
        let unauthorized = || err(ErrorCode::Unauthorized, "invalid refresh token");
        let rt = self
            .refresh
            .get_mut(req.refresh_token.expose_secret())
            .ok_or_else(unauthorized)?;
        let session_id = rt.session_id;
        if rt.used {
            // Replay: revoke the whole family (session) and its tokens.
            let user = self.sessions.get(&session_id).map(|s| s.user_id);
            if let Some(s) = self.sessions.get_mut(&session_id) {
                s.revoked = true;
            }
            self.access.retain(|_, a| a.session_id != session_id);
            if let Some(u) = user {
                self.close_sessions(u, vec![session_id]);
            }
            return Err(err(
                ErrorCode::RefreshTokenReused,
                "refresh token reuse detected",
            ));
        }
        if rt.expires_at <= Utc::now() {
            return Err(unauthorized());
        }
        let session = self.sessions.get(&session_id).ok_or_else(unauthorized)?;
        let device = self
            .devices
            .get(&session.device_id)
            .ok_or_else(unauthorized)?;
        if device.status == DeviceStatus::Revoked {
            return Err(err(ErrorCode::DeviceRevoked, "device has been revoked"));
        }
        if session.revoked {
            return Err(unauthorized());
        }
        if let Some(rt) = self.refresh.get_mut(req.refresh_token.expose_secret()) {
            rt.used = true;
        }
        // Each refresh also invalidates the previous access token(s).
        self.access.retain(|_, a| a.session_id != session_id);
        Ok(self.issue_tokens(session_id))
    }

    pub(crate) fn logout(&mut self, p: Principal, req: LogoutRequest) {
        let ids = if req.all_sessions {
            self.revoke_sessions(|s| s.user_id == p.user_id)
        } else {
            match self.sessions.get_mut(&p.session_id) {
                Some(s) if !s.revoked => {
                    s.revoked = true;
                    vec![p.session_id]
                }
                _ => Vec::new(),
            }
        };
        self.close_sessions(p.user_id, ids);
    }

    pub(crate) fn me(&self, p: Principal) -> MResult<AccountInfo> {
        let u = self.users.get(&p.user_id).ok_or_else(not_found)?;
        Ok(AccountInfo {
            user_id: u.user_id,
            email: u.email.clone(),
            email_verified: u.email_verified,
            created_at: u.created_at,
            current_device_id: p.device_id,
            current_session_id: p.session_id,
        })
    }

    pub(crate) fn forgot(&mut self, email: &str) {
        if let Some(user_id) = self.emails.get(&email.trim().to_lowercase()).copied() {
            self.email_tokens.insert(
                random_token(),
                EmailToken {
                    user_id,
                    purpose: TokenPurpose::ResetPassword,
                    used: false,
                },
            );
        }
    }

    fn take_email_token(&mut self, token: &str, purpose: TokenPurpose) -> MResult<UserId> {
        match self.email_tokens.get_mut(token) {
            Some(t) if t.purpose == purpose && !t.used => {
                t.used = true;
                Ok(t.user_id)
            }
            _ => Err(bad("invalid or expired token")),
        }
    }

    pub(crate) fn reset(&mut self, req: ResetPasswordRequest) -> MResult<()> {
        let len = req.new_password.expose_secret().len();
        if !(MIN_ACCOUNT_PASSWORD_LEN..=MAX_ACCOUNT_PASSWORD_LEN).contains(&len) {
            return Err(bad("password length out of bounds"));
        }
        let user_id =
            self.take_email_token(req.token.expose_secret(), TokenPurpose::ResetPassword)?;
        if let Some(u) = self.users.get_mut(&user_id) {
            u.password = req.new_password.expose_secret().to_owned();
        }
        let ids = self.revoke_sessions(|s| s.user_id == user_id);
        self.close_sessions(user_id, ids);
        Ok(())
    }

    pub(crate) fn change_password(
        &mut self,
        p: Principal,
        req: ChangePasswordRequest,
    ) -> MResult<()> {
        let len = req.new_password.expose_secret().len();
        if !(MIN_ACCOUNT_PASSWORD_LEN..=MAX_ACCOUNT_PASSWORD_LEN).contains(&len) {
            return Err(bad("password length out of bounds"));
        }
        let u = self.users.get_mut(&p.user_id).ok_or_else(not_found)?;
        if u.password != req.current_password.expose_secret() {
            return Err(err(
                ErrorCode::InvalidCredentials,
                "current password is wrong",
            ));
        }
        u.password = req.new_password.expose_secret().to_owned();
        Ok(())
    }

    pub(crate) fn verify_email(&mut self, token: &str) -> MResult<()> {
        let user_id = self.take_email_token(token, TokenPurpose::VerifyEmail)?;
        if let Some(u) = self.users.get_mut(&user_id) {
            u.email_verified = true;
        }
        Ok(())
    }

    // ---- trust helpers --------------------------------------------------------------------

    fn member_vault(&self, p: &Principal, vault_id: VaultId) -> MResult<&Vault> {
        match self.vaults.get(&vault_id) {
            Some(v) if v.owner == p.user_id && v.state != VaultState::Deleted => Ok(v),
            _ => Err(not_found()),
        }
    }

    fn is_trusted(&self, device_id: DeviceId, vault: &Vault) -> bool {
        vault.envelopes.iter().any(|e| {
            e.recipient_type == RecipientType::Device
                && e.recipient_id == Some(device_id.0)
                && e.revoked_at.is_none()
        })
    }

    /// T(V): member + non-revoked device envelope.
    fn trusted_vault(&self, p: &Principal, vault_id: VaultId) -> MResult<&Vault> {
        let v = self.member_vault(p, vault_id)?;
        if !self.is_trusted(p.device_id, v) {
            return Err(err(
                ErrorCode::DeviceNotTrusted,
                "device is not trusted for this vault",
            ));
        }
        Ok(v)
    }

    /// K(V): the vault access key matches the stored verifier.
    fn check_vak(v: &Vault, vak: &Bytes) -> MResult<()> {
        if vak.len() != VAULT_ACCESS_KEY_LEN {
            return Err(bad("vault access key must be 32 bytes"));
        }
        if !ct_eq(&sha256(vak.as_slice()), &v.vak_hash) {
            return Err(err(ErrorCode::InvalidProof, "vault access key rejected"));
        }
        Ok(())
    }

    pub(crate) fn is_member(&self, user_id: UserId, vault_id: VaultId) -> bool {
        self.vaults
            .get(&vault_id)
            .is_some_and(|v| v.owner == user_id && v.state != VaultState::Deleted)
    }

    fn vault_info(&self, v: &Vault, p: &Principal) -> VaultInfo {
        VaultInfo {
            vault_id: v.vault_id,
            owner_user_id: v.owner,
            role: VaultRole::Owner,
            state: v.state,
            created_at: v.created_at,
            updated_at: v.updated_at,
            latest_sequence: v.last_sequence,
            caller_trusted: self.is_trusted(p.device_id, v),
            deletion_scheduled_at: None,
            epoch: Some(v.epoch),
        }
    }

    fn device_info(&self, d: &Device, current: DeviceId) -> DeviceInfo {
        let mut trusted: Vec<VaultId> = self
            .vaults
            .values()
            .filter(|v| v.state != VaultState::Deleted && self.is_trusted(d.reg.device_id, v))
            .map(|v| v.vault_id)
            .collect();
        trusted.sort();
        DeviceInfo {
            device_id: d.reg.device_id,
            name: d.reg.name.clone(),
            platform: d.reg.platform,
            encryption_public_key: d.reg.encryption_public_key.clone(),
            signing_public_key: d.reg.signing_public_key.clone(),
            status: d.status,
            trusted_vaults: trusted,
            created_at: d.created_at,
            last_seen_at: d.last_seen_at,
            revoked_at: d.revoked_at,
            is_current: d.reg.device_id == current,
        }
    }

    fn store_envelope(&mut self, vault_id: VaultId, e: NewEnvelope, by: DeviceId) -> KeyEnvelope {
        let now = Utc::now();
        let ke = KeyEnvelope {
            envelope_id: EnvelopeId::new(),
            vault_id,
            recipient_type: e.recipient_type,
            recipient_id: e.recipient_id,
            kind: e.kind,
            metadata: e.metadata,
            ciphertext: e.ciphertext,
            nonce: e.nonce,
            created_at: now,
            created_by_device_id: Some(by),
            revoked_at: None,
        };
        if let Some(v) = self.vaults.get_mut(&vault_id) {
            // Replace an older envelope of the same recipient.
            for old in v.envelopes.iter_mut() {
                if old.revoked_at.is_none()
                    && old.recipient_type == ke.recipient_type
                    && old.recipient_id == ke.recipient_id
                {
                    old.revoked_at = Some(now);
                }
            }
            v.envelopes.push(ke.clone());
            v.updated_at = now;
        }
        ke
    }

    // ---- devices --------------------------------------------------------------------------

    fn trust_request_dto(&self, r: &TrustReq, current: DeviceId) -> Option<DeviceTrustRequest> {
        let d = self.devices.get(&r.device_id)?;
        Some(DeviceTrustRequest {
            request_id: r.request_id,
            device: self.device_info(d, current),
            vault_ids: r.vault_ids.clone(),
            status: r.status,
            created_at: r.created_at,
            expires_at: r.expires_at,
            approved_by_device_id: r.approved_by,
        })
    }

    pub(crate) fn list_devices(&self, p: Principal) -> ListDevicesResponse {
        let mut devices: Vec<&Device> = self
            .devices
            .values()
            .filter(|d| d.user_id == p.user_id)
            .collect();
        devices.sort_by_key(|d| d.created_at);
        let now = Utc::now();
        let pending_requests = self
            .requests
            .values()
            .filter(|r| {
                r.user_id == p.user_id
                    && r.status == DeviceRequestStatus::Pending
                    && r.expires_at > now
            })
            .filter_map(|r| self.trust_request_dto(r, p.device_id))
            .collect();
        ListDevicesResponse {
            devices: devices
                .into_iter()
                .map(|d| self.device_info(d, p.device_id))
                .collect(),
            pending_requests,
        }
    }

    pub(crate) fn create_trust_request(
        &mut self,
        p: Principal,
        req: CreateDeviceTrustRequest,
    ) -> MResult<DeviceTrustRequest> {
        let vault_ids: Vec<VaultId> = if req.vault_ids.is_empty() {
            let mut all: Vec<VaultId> = self
                .vaults
                .values()
                .filter(|v| v.owner == p.user_id && v.state != VaultState::Deleted)
                .map(|v| v.vault_id)
                .collect();
            all.sort();
            all
        } else {
            for v in &req.vault_ids {
                self.member_vault(&p, *v)?;
            }
            req.vault_ids.clone()
        };
        for r in self.requests.values_mut() {
            if r.device_id == p.device_id && r.status == DeviceRequestStatus::Pending {
                r.status = DeviceRequestStatus::Expired;
            }
        }
        let now = Utc::now();
        let r = TrustReq {
            request_id: DeviceRequestId::new(),
            device_id: p.device_id,
            user_id: p.user_id,
            vault_ids,
            status: DeviceRequestStatus::Pending,
            created_at: now,
            expires_at: now + chrono::Duration::hours(24),
            approved_by: None,
        };
        let dto = self
            .trust_request_dto(&r, p.device_id)
            .ok_or_else(not_found)?;
        self.emit_to(
            Audience::User(p.user_id),
            ServerEvent::DeviceApprovalRequested {
                request_id: r.request_id,
                device_id: p.device_id,
            },
        );
        self.requests.insert(r.request_id, r);
        Ok(dto)
    }

    fn own_account_device(&self, p: &Principal, device_id: DeviceId) -> MResult<&Device> {
        match self.devices.get(&device_id) {
            Some(d) if d.user_id == p.user_id => Ok(d),
            _ => Err(not_found()),
        }
    }

    pub(crate) fn update_device(
        &mut self,
        p: Principal,
        device_id: DeviceId,
        req: UpdateDeviceRequest,
    ) -> MResult<DeviceInfo> {
        self.own_account_device(&p, device_id)?;
        if device_id != p.device_id {
            return Err(err(
                ErrorCode::Forbidden,
                "only the current device can be renamed",
            ));
        }
        let n = req.name.trim().chars().count();
        if n == 0 || n > MAX_DEVICE_NAME_LEN {
            return Err(bad("invalid device name"));
        }
        if let Some(d) = self.devices.get_mut(&device_id) {
            d.reg.name = req.name.trim().to_owned();
        }
        let d = self.devices.get(&device_id).ok_or_else(not_found)?;
        Ok(self.device_info(d, p.device_id))
    }

    pub(crate) fn approve(
        &mut self,
        p: Principal,
        new_device_id: DeviceId,
        req: ApproveDeviceRequest,
    ) -> MResult<DeviceTrustRequest> {
        let now = Utc::now();
        let r = self.requests.get(&req.request_id).ok_or_else(not_found)?;
        if r.user_id != p.user_id {
            return Err(not_found());
        }
        if r.device_id != new_device_id {
            return Err(bad("request does not belong to this device"));
        }
        if r.status != DeviceRequestStatus::Pending || r.expires_at <= now {
            return Err(err(ErrorCode::Gone, "request is no longer pending"));
        }
        if req.envelopes.is_empty() {
            return Err(bad("no envelopes"));
        }
        let requested: HashSet<VaultId> = r.vault_ids.iter().copied().collect();
        let mut vault_ids = Vec::new();
        for ve in &req.envelopes {
            if !requested.contains(&ve.vault_id) {
                return Err(bad("vault not part of the request"));
            }
            self.trusted_vault(&p, ve.vault_id)?;
            validate_envelope(&ve.envelope, RecipientType::Device, Some(new_device_id.0))?;
            vault_ids.push(ve.vault_id);
        }
        if (req.issued_at - now.timestamp()).abs() > MAX_SIGNATURE_SKEW_SECONDS {
            return Err(err(
                ErrorCode::InvalidProof,
                "approval timestamp outside allowed skew",
            ));
        }
        let new_dev = self.devices.get(&new_device_id).ok_or_else(not_found)?;
        let approver = self.devices.get(&p.device_id).ok_or_else(not_found)?;
        let (Some(enc), Some(sig_pk)) = (
            new_dev.reg.encryption_public_key.to_array::<32>(),
            new_dev.reg.signing_public_key.to_array::<32>(),
        ) else {
            return Err(bad("stored device keys malformed"));
        };
        let msg = device_approval_message(
            req.request_id,
            p.device_id,
            new_device_id,
            &enc,
            &sig_pk,
            req.issued_at,
            &vault_ids,
        );
        let verified = (|| {
            let vk_bytes = approver.reg.signing_public_key.to_array::<32>()?;
            let vk = ed25519_dalek::VerifyingKey::from_bytes(&vk_bytes).ok()?;
            let sig_bytes = req.signature.to_array::<ED25519_SIGNATURE_LEN>()?;
            let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes);
            vk.verify_strict(&msg, &sig).ok()
        })();
        if verified.is_none() {
            return Err(err(ErrorCode::InvalidProof, "approval signature rejected"));
        }
        for ve in req.envelopes {
            self.store_envelope(ve.vault_id, ve.envelope, p.device_id);
        }
        if let Some(r) = self.requests.get_mut(&req.request_id) {
            r.status = DeviceRequestStatus::Approved;
            r.approved_by = Some(p.device_id);
        }
        self.emit_to(
            Audience::User(p.user_id),
            ServerEvent::DeviceApproved {
                device_id: new_device_id,
                vault_ids,
            },
        );
        let r = self.requests.get(&req.request_id).ok_or_else(not_found)?;
        self.trust_request_dto(r, p.device_id).ok_or_else(not_found)
    }

    pub(crate) fn reject(
        &mut self,
        p: Principal,
        device_id: DeviceId,
        req: RejectDeviceRequest,
    ) -> MResult<()> {
        let r = self.requests.get(&req.request_id).ok_or_else(not_found)?;
        if r.user_id != p.user_id || r.device_id != device_id {
            return Err(not_found());
        }
        let any_trusted = r
            .vault_ids
            .iter()
            .any(|v| self.trusted_vault(&p, *v).is_ok());
        if !any_trusted {
            return Err(err(
                ErrorCode::DeviceNotTrusted,
                "not trusted for any requested vault",
            ));
        }
        if let Some(r) = self.requests.get_mut(&req.request_id) {
            r.status = DeviceRequestStatus::Rejected;
        }
        Ok(())
    }

    pub(crate) fn attest(
        &mut self,
        p: Principal,
        device_id: DeviceId,
        req: AttestDeviceRequest,
    ) -> MResult<KeyEnvelope> {
        self.own_account_device(&p, device_id)?;
        if device_id != p.device_id {
            return Err(err(ErrorCode::Forbidden, "a device can only attest itself"));
        }
        let v = self.member_vault(&p, req.vault_id)?;
        validate_envelope(&req.envelope, RecipientType::Device, Some(p.device_id.0))?;
        Self::check_vak(v, &req.vault_access_key)?;
        Ok(self.store_envelope(req.vault_id, req.envelope, p.device_id))
    }

    /// Revoke a device (API or admin). Returns false if already revoked.
    pub(crate) fn revoke_device_internal(&mut self, device_id: DeviceId) -> bool {
        let now = Utc::now();
        let Some(d) = self.devices.get_mut(&device_id) else {
            return false;
        };
        if d.status == DeviceStatus::Revoked {
            return false;
        }
        d.status = DeviceStatus::Revoked;
        d.revoked_at = Some(now);
        let user_id = d.user_id;
        let sessions = self.revoke_sessions(|s| s.device_id == device_id);
        for v in self.vaults.values_mut() {
            for e in v.envelopes.iter_mut() {
                if e.recipient_type == RecipientType::Device
                    && e.recipient_id == Some(device_id.0)
                    && e.revoked_at.is_none()
                {
                    e.revoked_at = Some(now);
                }
            }
        }
        for r in self.requests.values_mut() {
            if r.device_id == device_id && r.status == DeviceRequestStatus::Pending {
                r.status = DeviceRequestStatus::Rejected;
            }
        }
        self.emit(Outgoing {
            audience: Audience::User(user_id),
            event: ServerEvent::DeviceRevoked { device_id },
            close_device: Some(device_id),
            close_sessions: sessions,
            force_close: None,
        });
        true
    }

    pub(crate) fn revoke(&mut self, p: Principal, device_id: DeviceId) -> MResult<()> {
        self.own_account_device(&p, device_id)?;
        self.revoke_device_internal(device_id);
        Ok(())
    }

    // ---- vaults & envelopes -----------------------------------------------------------------

    pub(crate) fn create_vault(
        &mut self,
        p: Principal,
        req: CreateVaultRequest,
    ) -> MResult<VaultInfo> {
        if self.vaults.contains_key(&req.vault_id) {
            return Err(err(ErrorCode::AlreadyExists, "vault id already exists"));
        }
        if req.vault_access_key.len() != VAULT_ACCESS_KEY_LEN {
            return Err(bad("vault access key must be 32 bytes"));
        }
        validate_envelope(&req.password_envelope, RecipientType::Password, None)?;
        validate_envelope(&req.recovery_envelope, RecipientType::Recovery, None)?;
        validate_envelope(
            &req.device_envelope,
            RecipientType::Device,
            Some(p.device_id.0),
        )?;
        let now = Utc::now();
        self.vaults.insert(
            req.vault_id,
            Vault {
                vault_id: req.vault_id,
                owner: p.user_id,
                vak_hash: sha256(req.vault_access_key.as_slice()),
                state: VaultState::Active,
                created_at: now,
                updated_at: now,
                last_sequence: 0,
                epoch: uuid::Uuid::new_v4(),
                objects: HashMap::new(),
                mutations: HashMap::new(),
                envelopes: Vec::new(),
            },
        );
        for e in [
            req.password_envelope,
            req.recovery_envelope,
            req.device_envelope,
        ] {
            self.store_envelope(req.vault_id, e, p.device_id);
        }
        let v = self.vaults.get(&req.vault_id).ok_or_else(not_found)?;
        Ok(self.vault_info(v, &p))
    }

    pub(crate) fn list_vaults(&self, p: Principal) -> ListVaultsResponse {
        let mut vaults: Vec<VaultInfo> = self
            .vaults
            .values()
            .filter(|v| v.owner == p.user_id && v.state != VaultState::Deleted)
            .map(|v| self.vault_info(v, &p))
            .collect();
        vaults.sort_by_key(|v| v.vault_id);
        ListVaultsResponse { vaults }
    }

    pub(crate) fn get_vault(&self, p: Principal, vault_id: VaultId) -> MResult<VaultInfo> {
        let v = self.member_vault(&p, vault_id)?;
        Ok(self.vault_info(v, &p))
    }

    pub(crate) fn delete_vault(
        &mut self,
        p: Principal,
        vault_id: VaultId,
        vak: Option<Bytes>,
    ) -> MResult<()> {
        let v = self.trusted_vault(&p, vault_id)?;
        let vak = vak.ok_or_else(|| err(ErrorCode::InvalidProof, "vault access key required"))?;
        Self::check_vak(v, &vak)?;
        if let Some(v) = self.vaults.get_mut(&vault_id) {
            v.state = VaultState::Deleted;
            v.updated_at = Utc::now();
        }
        Ok(())
    }

    pub(crate) fn list_envelopes(
        &self,
        p: Principal,
        vault_id: VaultId,
    ) -> MResult<ListEnvelopesResponse> {
        let v = self.member_vault(&p, vault_id)?;
        let trusted = self.is_trusted(p.device_id, v);
        let envelopes = v
            .envelopes
            .iter()
            .filter(|e| e.revoked_at.is_none())
            .filter(|e| {
                trusted
                    || matches!(
                        e.recipient_type,
                        RecipientType::Password | RecipientType::Recovery
                    )
                    || (e.recipient_type == RecipientType::Device
                        && e.recipient_id == Some(p.device_id.0))
            })
            .cloned()
            .collect();
        Ok(ListEnvelopesResponse { envelopes })
    }

    pub(crate) fn create_envelope(
        &mut self,
        p: Principal,
        vault_id: VaultId,
        req: CreateEnvelopeRequest,
    ) -> MResult<KeyEnvelope> {
        let v = self.trusted_vault(&p, vault_id)?;
        Self::check_vak(v, &req.vault_access_key)?;
        req.envelope
            .validate()
            .map_err(|_| bad("invalid envelope"))?;
        match req.envelope.recipient_type {
            RecipientType::User => {}
            RecipientType::Device if req.envelope.recipient_id == Some(p.device_id.0) => {}
            _ => {
                return Err(bad(
                    "use device approval or the recovery endpoints for this recipient",
                ))
            }
        }
        Ok(self.store_envelope(vault_id, req.envelope, p.device_id))
    }

    pub(crate) fn delete_envelope(
        &mut self,
        p: Principal,
        vault_id: VaultId,
        envelope_id: EnvelopeId,
        vak: Option<Bytes>,
    ) -> MResult<()> {
        let v = self.trusted_vault(&p, vault_id)?;
        let vak = vak.ok_or_else(|| err(ErrorCode::InvalidProof, "vault access key required"))?;
        Self::check_vak(v, &vak)?;
        let e = v
            .envelopes
            .iter()
            .find(|e| e.envelope_id == envelope_id && e.revoked_at.is_none())
            .ok_or_else(not_found)?;
        if matches!(
            e.recipient_type,
            RecipientType::Password | RecipientType::Recovery
        ) {
            return Err(bad("password/recovery envelopes can only be replaced"));
        }
        let now = Utc::now();
        if let Some(v) = self.vaults.get_mut(&vault_id) {
            for e in v.envelopes.iter_mut() {
                if e.envelope_id == envelope_id {
                    e.revoked_at = Some(now);
                }
            }
        }
        Ok(())
    }

    pub(crate) fn recovery_material(
        &self,
        p: Principal,
        vault_id: VaultId,
    ) -> MResult<VaultRecoveryMaterial> {
        let v = self.member_vault(&p, vault_id)?;
        let find = |t: RecipientType, id: Option<Uuid>| {
            v.envelopes
                .iter()
                .find(|e| e.revoked_at.is_none() && e.recipient_type == t && e.recipient_id == id)
                .cloned()
        };
        Ok(VaultRecoveryMaterial {
            vault_id,
            password_envelope: find(RecipientType::Password, None),
            recovery_envelope: find(RecipientType::Recovery, None),
            device_envelope: find(RecipientType::Device, Some(p.device_id.0)),
        })
    }

    pub(crate) fn replace_envelope(
        &mut self,
        p: Principal,
        req: ReplaceEnvelopeRequest,
        expected: RecipientType,
    ) -> MResult<KeyEnvelope> {
        let v = self.trusted_vault(&p, req.vault_id)?;
        Self::check_vak(v, &req.vault_access_key)?;
        validate_envelope(&req.envelope, expected, None)?;
        let ke = self.store_envelope(req.vault_id, req.envelope, p.device_id);
        self.emit_to(
            Audience::Vault(req.vault_id),
            ServerEvent::RecoveryChanged {
                vault_id: req.vault_id,
                recipient_type: expected,
            },
        );
        Ok(ke)
    }

    // ---- sync (ADR-0003) --------------------------------------------------------------------

    pub(crate) fn push(
        &mut self,
        p: Principal,
        req: PushRequest,
    ) -> MResult<(StatusCode, PushResponse)> {
        if req.device_id != p.device_id {
            return Err(err(
                ErrorCode::Forbidden,
                "device_id does not match the session",
            ));
        }
        self.trusted_vault(&p, req.vault_id)?;
        // Validate the whole batch before applying anything.
        if req.mutations.len() > MAX_PUSH_BATCH {
            return Err(err(ErrorCode::PayloadTooLarge, "too many mutations"));
        }
        let mut objects = HashSet::new();
        for m in &req.mutations {
            if !objects.insert(m.object_id) {
                return Err(bad("duplicate object_id in batch"));
            }
            if m.base_revision < 0 {
                return Err(bad("negative base_revision"));
            }
            if let MutationOp::Put { body } = &m.op {
                if body.format != OBJECT_FORMAT_V1 {
                    return Err(bad("unsupported object format"));
                }
                if body.ciphertext.len() > MAX_OBJECT_CIPHERTEXT_BYTES {
                    return Err(err(ErrorCode::PayloadTooLarge, "object too large"));
                }
                if body.ciphertext.is_empty()
                    || body.nonce.len() != NONCE_LEN
                    || body.wrapped_dek.len() != WRAPPED_DEK_LEN
                    || body.wrapped_dek_nonce.len() != NONCE_LEN
                {
                    return Err(bad("malformed encrypted body"));
                }
            }
        }
        let vault = self.vaults.get_mut(&req.vault_id).ok_or_else(not_found)?;
        for m in &req.mutations {
            if let Some((obj, _)) = vault.mutations.get(&m.mutation_id) {
                if *obj != m.object_id {
                    return Err(bad("mutation_id reused for another object"));
                }
            }
        }
        let before = vault.last_sequence;
        let now = Utc::now();
        let mut results = Vec::with_capacity(req.mutations.len());
        for m in req.mutations {
            if let Some((_, stored)) = vault.mutations.get(&m.mutation_id) {
                let mut r = stored.clone();
                if let MutationResult::Accepted { replayed, .. } = &mut r {
                    *replayed = true;
                }
                results.push(r);
                continue;
            }
            let current = vault.objects.get(&m.object_id);
            let current_revision = current.map_or(0, |o| o.revision);
            if m.base_revision != current_revision {
                results.push(MutationResult::Conflict {
                    mutation_id: m.mutation_id,
                    object_id: m.object_id,
                    current_revision,
                    current_sequence: current.map_or(0, |o| o.sequence),
                    current_deleted: current.is_some_and(|o| o.deleted),
                });
                continue;
            }
            vault.last_sequence += 1;
            let sequence = vault.last_sequence;
            let revision = m.base_revision + 1;
            let (deleted, body) = match m.op {
                MutationOp::Put { body } => (false, Some(body)),
                MutationOp::Delete => (true, None),
            };
            vault.objects.insert(
                m.object_id,
                MockObject {
                    revision,
                    sequence,
                    deleted,
                    body,
                    writer_device_id: p.device_id,
                    updated_at: now,
                },
            );
            let r = MutationResult::Accepted {
                mutation_id: m.mutation_id,
                object_id: m.object_id,
                revision,
                sequence,
                replayed: false,
            };
            vault
                .mutations
                .insert(m.mutation_id, (m.object_id, r.clone()));
            results.push(r);
        }
        let latest = vault.last_sequence;
        if latest != before {
            vault.updated_at = now;
            self.emit_to(
                Audience::Vault(req.vault_id),
                ServerEvent::VaultChanged {
                    vault_id: req.vault_id,
                    latest_sequence: latest,
                },
            );
        }
        let status = if results.iter().any(MutationResult::is_conflict) {
            StatusCode::CONFLICT
        } else {
            StatusCode::OK
        };
        Ok((
            status,
            PushResponse {
                results,
                latest_sequence: latest,
            },
        ))
    }

    fn page_limit(limit: Option<u32>) -> MResult<usize> {
        match limit {
            None => Ok(DEFAULT_PAGE_LIMIT as usize),
            Some(0) => Err(bad("limit must be positive")),
            Some(l) => Ok(l.min(MAX_PAGE_LIMIT) as usize),
        }
    }

    fn change_of(id: ObjectId, o: &MockObject) -> Change {
        Change {
            object_id: id,
            revision: o.revision,
            sequence: o.sequence,
            deleted: o.deleted,
            body: o.body.clone(),
            writer_device_id: o.writer_device_id,
            updated_at: o.updated_at,
        }
    }

    pub(crate) fn changes(&self, p: Principal, q: ChangesQuery) -> MResult<ChangesResponse> {
        let v = self.trusted_vault(&p, q.vault_id)?;
        if q.after < 0 {
            return Err(bad("after must be >= 0"));
        }
        let limit = Self::page_limit(q.limit)?;
        let mut all: Vec<(&ObjectId, &MockObject)> = v
            .objects
            .iter()
            .filter(|(_, o)| o.sequence > q.after)
            .collect();
        all.sort_by_key(|(_, o)| o.sequence);
        let has_more = all.len() > limit;
        let changes: Vec<Change> = all
            .into_iter()
            .take(limit)
            .map(|(id, o)| Self::change_of(*id, o))
            .collect();
        let next_after = changes.last().map_or(q.after, |c| c.sequence);
        Ok(ChangesResponse {
            changes,
            next_after,
            has_more,
            latest_sequence: v.last_sequence,
            epoch: Some(v.epoch),
        })
    }

    pub(crate) fn snapshot(&self, p: Principal, q: SnapshotQuery) -> MResult<SnapshotResponse> {
        let v = self.trusted_vault(&p, q.vault_id)?;
        let cursor = q.cursor.unwrap_or(0);
        if cursor < 0 {
            return Err(bad("cursor must be >= 0"));
        }
        let limit = Self::page_limit(q.limit)?;
        let mut live: Vec<(&ObjectId, &MockObject)> = v
            .objects
            .iter()
            .filter(|(_, o)| !o.deleted && o.sequence > cursor)
            .collect();
        live.sort_by_key(|(_, o)| o.sequence);
        let has_more = live.len() > limit;
        let objects: Vec<Change> = live
            .into_iter()
            .take(limit)
            .map(|(id, o)| Self::change_of(*id, o))
            .collect();
        let next_cursor = if has_more {
            objects.last().map(|c| c.sequence)
        } else {
            None
        };
        Ok(SnapshotResponse {
            objects,
            next_cursor,
            latest_sequence: v.last_sequence,
            epoch: Some(v.epoch),
        })
    }

    // ---- test inspection ------------------------------------------------------------------------

    pub(crate) fn object(&self, vault_id: VaultId, object_id: ObjectId) -> Option<MockObject> {
        self.vaults.get(&vault_id)?.objects.get(&object_id).cloned()
    }

    pub(crate) fn latest_sequence(&self, vault_id: VaultId) -> i64 {
        self.vaults.get(&vault_id).map_or(0, |v| v.last_sequence)
    }

    pub(crate) fn live_objects(&self, vault_id: VaultId) -> Vec<ObjectId> {
        let mut ids: Vec<ObjectId> = self
            .vaults
            .get(&vault_id)
            .map(|v| {
                v.objects
                    .iter()
                    .filter(|(_, o)| !o.deleted)
                    .map(|(id, _)| *id)
                    .collect()
            })
            .unwrap_or_default();
        ids.sort();
        ids
    }

    pub(crate) fn email_tokens(&self, email: &str) -> Vec<String> {
        let Some(user_id) = self.emails.get(&email.trim().to_lowercase()) else {
            return Vec::new();
        };
        self.email_tokens
            .iter()
            .filter(|(_, t)| t.user_id == *user_id && !t.used)
            .map(|(k, _)| k.clone())
            .collect()
    }

    pub(crate) fn user_id(&self, email: &str) -> Option<UserId> {
        self.emails.get(&email.trim().to_lowercase()).copied()
    }

    pub(crate) fn device_trusted(&self, device_id: DeviceId, vault_id: VaultId) -> bool {
        self.vaults
            .get(&vault_id)
            .is_some_and(|v| self.is_trusted(device_id, v))
    }

    // ---- restore drills (test hooks) -------------------------------------------------------

    /// Save the vault's objects, idempotency records and sequence (like an
    /// operator backup). Returns the saved sequence.
    pub(crate) fn checkpoint(&mut self, vault_id: VaultId) -> Option<i64> {
        let v = self.vaults.get(&vault_id)?;
        let cp = Checkpoint {
            sequence: v.last_sequence,
            objects: v.objects.clone(),
            mutations: v.mutations.clone(),
        };
        let sequence = cp.sequence;
        self.checkpoints.entry(vault_id).or_default().push(cp);
        Some(sequence)
    }

    /// Roll the vault back to the checkpoint taken at `to_sequence` (latest
    /// checkpoint if `None`), optionally rotating the epoch like
    /// `consolecrypt-server admin rotate-epoch`. Devices, envelopes and
    /// accounts are left as they are. Returns the restored sequence.
    pub(crate) fn restore(
        &mut self,
        vault_id: VaultId,
        to_sequence: Option<i64>,
        rotate_epoch: bool,
    ) -> Option<i64> {
        let cps = self.checkpoints.get(&vault_id)?;
        let cp = match to_sequence {
            Some(seq) => cps.iter().rev().find(|c| c.sequence == seq)?,
            None => cps.last()?,
        };
        let (sequence, objects, mutations) =
            (cp.sequence, cp.objects.clone(), cp.mutations.clone());
        let v = self.vaults.get_mut(&vault_id)?;
        v.objects = objects;
        v.mutations = mutations;
        v.last_sequence = sequence;
        v.updated_at = Utc::now();
        if rotate_epoch {
            v.epoch = uuid::Uuid::new_v4();
        }
        self.emit_to(
            Audience::Vault(vault_id),
            ServerEvent::VaultChanged {
                vault_id,
                latest_sequence: sequence,
            },
        );
        Some(sequence)
    }

    pub(crate) fn rotate_epoch(&mut self, vault_id: VaultId) -> Option<uuid::Uuid> {
        let v = self.vaults.get_mut(&vault_id)?;
        v.epoch = uuid::Uuid::new_v4();
        Some(v.epoch)
    }

    pub(crate) fn epoch(&self, vault_id: VaultId) -> Option<uuid::Uuid> {
        self.vaults.get(&vault_id).map(|v| v.epoch)
    }

    pub(crate) fn expire_access_tokens(&mut self) {
        let past = Utc::now() - chrono::Duration::seconds(1);
        for t in self.access.values_mut() {
            t.expires_at = past;
        }
    }
}
