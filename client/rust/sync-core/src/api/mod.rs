//! Typed HTTP client for every `cc_protocol::paths` endpoint.
//!
//! * Configurable, self-hosted server URL (no built-in host).
//! * Sends `x-cc-protocol-version`, `x-cc-client-version`, `x-cc-platform`
//!   on every request.
//! * Bearer auth with tokens from a caller-provided [`TokenStore`];
//!   proactive refresh shortly before expiry and single-flight refresh +
//!   one retry on `401 unauthorized`. `refresh_token_reused` clears the
//!   stored tokens and surfaces [`ApiError::RefreshTokenReused`].
//! * Protocol 1.5: with a [`RequestSigner`] set, every authenticated
//!   request and `POST /v1/auth/refresh` carries `x-cc-device-proof` over
//!   the exact request target and body bytes, freshly signed per attempt; a
//!   `stale` rejection is re-signed and retried once, other rejections
//!   surface as [`ApiError::RequestProofRejected`].
//! * Logs only method, path, status and timing — never tokens or bodies.

mod error;
pub(crate) mod signer;
mod tokens;

pub use error::ApiError;
pub use signer::{ProofRejection, RequestSigner, SignerError};
pub use tokens::{MemoryTokenStore, TokenStore, TokenStoreError};

use cc_protocol::auth::{
    AccountInfo, AuthResponse, ChangePasswordRequest, ForgotPasswordRequest, LoginRequest,
    LogoutRequest, RefreshRequest, RegisterRequest, ResetPasswordRequest, SecretString, TokenPair,
    VerifyEmailRequest,
};
use cc_protocol::devices::{
    ApproveDeviceRequest, AttestDeviceRequest, CreateDeviceTrustRequest, DeviceInfo,
    DeviceTrustRequest, ListDevicesResponse, RejectDeviceRequest, RevokeDeviceRequest,
    UpdateDeviceRequest,
};
use cc_protocol::envelopes::KeyEnvelope;
use cc_protocol::meta::ServerInfo;
use cc_protocol::paths;
use cc_protocol::recovery::{ReplaceEnvelopeRequest, VaultRecoveryMaterial};
use cc_protocol::sync::{
    ChangesQuery, ChangesResponse, PushRequest, PushResponse, SnapshotQuery, SnapshotResponse,
};
use cc_protocol::vaults::{
    CreateEnvelopeRequest, CreateVaultRequest, DeleteEnvelopeRequest, DeleteVaultRequest,
    ListEnvelopesResponse, ListVaultsResponse, VaultInfo,
};
use cc_protocol::version::{
    Platform, HEADER_CLIENT_VERSION, HEADER_DEVICE_PROOF, HEADER_PLATFORM, HEADER_PROTOCOL_VERSION,
    HEADER_REQUEST_ID,
};
use cc_protocol::{DeviceId, EnvelopeId, ErrorCode, VaultId, PROTOCOL_VERSION};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, RETRY_AFTER};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use url::Url;

/// Refresh the access token when it expires within this window.
const EXPIRY_SKEW: Duration = Duration::from_secs(30);

/// Client configuration.
#[derive(Debug, Clone)]
pub struct ApiConfig {
    /// Base URL of the self-hosted server, e.g. `https://sync.example.org/`
    /// (a path prefix is allowed).
    pub server_url: Url,
    /// App version for `x-cc-client-version`.
    pub client_version: String,
    /// Platform for `x-cc-platform`.
    pub platform: Platform,
    /// Whole-request timeout.
    pub request_timeout: Duration,
    /// TCP/TLS connect timeout.
    pub connect_timeout: Duration,
}

impl ApiConfig {
    /// Validate `server_url` and build a config with default timeouts.
    ///
    /// `https` is required, except for loopback hosts (local testing).
    pub fn new(
        server_url: &str,
        client_version: impl Into<String>,
        platform: Platform,
    ) -> Result<Self, ApiError> {
        Self::with_http_policy(server_url, client_version, platform, false)
    }

    /// Like [`ApiConfig::new`], optionally allowing plain `http` to any host
    /// (only for trusted LAN setups; tokens travel unencrypted).
    pub fn with_http_policy(
        server_url: &str,
        client_version: impl Into<String>,
        platform: Platform,
        allow_insecure_http: bool,
    ) -> Result<Self, ApiError> {
        let url = Url::parse(server_url.trim())
            .map_err(|e| ApiError::InvalidConfig(format!("server url: {e}")))?;
        let loopback = match url.host() {
            Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            None => false,
        };
        match url.scheme() {
            "https" => {}
            "http" if loopback || allow_insecure_http => {}
            "http" => {
                return Err(ApiError::InvalidConfig(
                    "server url must use https (http only for localhost)".into(),
                ))
            }
            other => {
                return Err(ApiError::InvalidConfig(format!(
                    "unsupported scheme {other}"
                )))
            }
        }
        if url.cannot_be_a_base()
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(ApiError::InvalidConfig(
                "server url must not contain credentials, query or fragment".into(),
            ));
        }
        Ok(Self {
            server_url: url,
            client_version: client_version.into(),
            platform,
            request_timeout: Duration::from_secs(60),
            connect_timeout: Duration::from_secs(10),
        })
    }
}

#[derive(Default)]
struct TokenCache {
    loaded: bool,
    tokens: Option<TokenPair>,
}

struct Inner {
    http: reqwest::Client,
    config: ApiConfig,
    store: Arc<dyn TokenStore>,
    cache: std::sync::Mutex<TokenCache>,
    refresh_lock: tokio::sync::Mutex<()>,
    signer: std::sync::RwLock<Option<Arc<dyn RequestSigner>>>,
}

/// Cloneable handle to the ConsoleCrypt server API.
#[derive(Clone)]
pub struct ApiClient {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for ApiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiClient")
            .field("server_url", &self.inner.config.server_url.as_str())
            .finish_non_exhaustive()
    }
}

/// A raw response.
pub(crate) struct Raw {
    pub(crate) status: StatusCode,
    headers: HeaderMap,
    body: bytes::Bytes,
}

impl ApiClient {
    /// Build a client. Tokens are loaded lazily from `tokens`.
    pub fn new(config: ApiConfig, tokens: Arc<dyn TokenStore>) -> Result<Self, ApiError> {
        let mut headers = HeaderMap::new();
        let hv = |v: &str| {
            HeaderValue::from_str(v).map_err(|_| ApiError::InvalidConfig("header value".into()))
        };
        headers.insert(HEADER_PROTOCOL_VERSION, hv(&PROTOCOL_VERSION.to_string())?);
        headers.insert(HEADER_CLIENT_VERSION, hv(&config.client_version)?);
        headers.insert(HEADER_PLATFORM, hv(config.platform.as_str())?);
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .user_agent(format!("ConsoleCrypt/{}", config.client_version))
            .timeout(config.request_timeout)
            .connect_timeout(config.connect_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| ApiError::InvalidConfig(format!("http client: {e}")))?;
        Ok(Self {
            inner: Arc::new(Inner {
                http,
                config,
                store: tokens,
                cache: std::sync::Mutex::new(TokenCache::default()),
                refresh_lock: tokio::sync::Mutex::new(()),
                signer: std::sync::RwLock::new(None),
            }),
        })
    }

    /// Sign requests with the device key from now on (protocol 1.5, see
    /// [`RequestSigner`]). Builder form of [`ApiClient::set_request_signer`].
    pub fn with_request_signer(self, signer: Arc<dyn RequestSigner>) -> Self {
        self.set_request_signer(Some(signer));
        self
    }

    /// Install (or remove) the request signer; affects every clone of this
    /// client, including a running [`crate::EventStream`] on reconnect.
    pub fn set_request_signer(&self, signer: Option<Arc<dyn RequestSigner>>) {
        *self.inner.signer.write().unwrap_or_else(|p| p.into_inner()) = signer;
    }

    /// Whether a request signer is installed.
    pub fn has_request_signer(&self) -> bool {
        self.signer().is_some()
    }

    fn signer(&self) -> Option<Arc<dyn RequestSigner>> {
        self.inner
            .signer
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// `x-cc-device-proof` value for one request (`None` without a signer).
    pub(crate) fn request_proof_header(
        &self,
        method: &str,
        url: &Url,
        body: &[u8],
    ) -> Result<Option<String>, ApiError> {
        let Some(signer) = self.signer() else {
            return Ok(None);
        };
        let proof = signer.request_proof(method, &signer::request_target(url), body)?;
        Ok(Some(proof.encode()))
    }

    /// Configuration.
    pub fn config(&self) -> &ApiConfig {
        &self.inner.config
    }

    /// `ws(s)://…/v1/events/ws` for this server.
    pub fn events_url(&self) -> Url {
        let mut u = self.endpoint(paths::EVENTS_WS);
        let scheme = if u.scheme() == "https" { "wss" } else { "ws" };
        // Only fails for non-special schemes, which ApiConfig excludes.
        let _ = u.set_scheme(scheme);
        u
    }

    fn endpoint(&self, path: &str) -> Url {
        let mut u = self.inner.config.server_url.clone();
        let base = u.path().trim_end_matches('/').to_owned();
        u.set_path(&format!("{base}{path}"));
        u
    }

    // ---- tokens --------------------------------------------------------------

    async fn cached_tokens(&self) -> Result<Option<TokenPair>, ApiError> {
        {
            let c = self.lock_cache();
            if c.loaded {
                return Ok(c.tokens.clone());
            }
        }
        let loaded = self
            .inner
            .store
            .load()
            .await
            .map_err(|e| ApiError::TokenStore(e.to_string()))?;
        let mut c = self.lock_cache();
        if !c.loaded {
            c.loaded = true;
            c.tokens = loaded;
        }
        Ok(c.tokens.clone())
    }

    fn lock_cache(&self) -> std::sync::MutexGuard<'_, TokenCache> {
        // The cache holds plain data; a poisoned lock is still consistent.
        self.inner.cache.lock().unwrap_or_else(|p| p.into_inner())
    }

    async fn store_tokens(&self, tokens: &TokenPair) -> Result<(), ApiError> {
        self.inner
            .store
            .save(tokens)
            .await
            .map_err(|e| ApiError::TokenStore(e.to_string()))?;
        let mut c = self.lock_cache();
        c.loaded = true;
        c.tokens = Some(tokens.clone());
        Ok(())
    }

    /// Forget tokens locally (store + cache) without calling the server.
    pub async fn clear_tokens(&self) -> Result<(), ApiError> {
        {
            let mut c = self.lock_cache();
            c.loaded = true;
            c.tokens = None;
        }
        self.inner
            .store
            .clear()
            .await
            .map_err(|e| ApiError::TokenStore(e.to_string()))
    }

    /// Install tokens obtained elsewhere (e.g. restored session).
    pub async fn set_tokens(&self, tokens: &TokenPair) -> Result<(), ApiError> {
        self.store_tokens(tokens).await
    }

    /// Whether tokens are available (not whether they are still valid).
    pub async fn is_authenticated(&self) -> Result<bool, ApiError> {
        Ok(self.cached_tokens().await?.is_some())
    }

    /// A currently valid access token (refreshing first if it is about to
    /// expire). Used by the WebSocket client.
    pub(crate) async fn access_token(&self) -> Result<SecretString, ApiError> {
        let tokens = self
            .cached_tokens()
            .await?
            .ok_or(ApiError::NotAuthenticated)?;
        if expiring(&tokens) {
            self.refresh_after_rejection(tokens.access_token.expose_secret())
                .await?;
            return Ok(self
                .cached_tokens()
                .await?
                .ok_or(ApiError::NotAuthenticated)?
                .access_token);
        }
        Ok(tokens.access_token)
    }

    /// Single-flight refresh after `rejected` was refused (or is expiring).
    /// If another task already rotated the tokens, this is a no-op.
    pub(crate) async fn refresh_after_rejection(&self, rejected: &str) -> Result<(), ApiError> {
        let _guard = self.inner.refresh_lock.lock().await;
        let current = self
            .cached_tokens()
            .await?
            .ok_or(ApiError::NotAuthenticated)?;
        if current.access_token.expose_secret() != rejected && !expiring(&current) {
            return Ok(());
        }
        self.refresh_with(current.refresh_token).await.map(|_| ())
    }

    /// Force a token refresh now (rotates the refresh token).
    pub async fn refresh(&self) -> Result<TokenPair, ApiError> {
        let _guard = self.inner.refresh_lock.lock().await;
        let current = self
            .cached_tokens()
            .await?
            .ok_or(ApiError::NotAuthenticated)?;
        self.refresh_with(current.refresh_token).await
    }

    async fn refresh_with(&self, refresh_token: SecretString) -> Result<TokenPair, ApiError> {
        let body = to_json(&RefreshRequest { refresh_token })?;
        // Bound to the device key too: the server checks the proof against
        // the refresh token's session before rotating it.
        let result = match self
            .send_signed(
                Method::POST,
                self.endpoint(paths::AUTH_REFRESH),
                Some(body),
                None,
            )
            .await
        {
            Ok(raw) => check(raw).and_then(|raw| decode::<TokenPair>(&raw)),
            Err(e) => Err(e),
        };
        match result {
            Ok(pair) => {
                self.store_tokens(&pair).await?;
                tracing::debug!("access token refreshed");
                Ok(pair)
            }
            Err(e) => {
                let terminal = match &e {
                    ApiError::RefreshTokenReused => Some(ApiError::RefreshTokenReused),
                    ApiError::DeviceRevoked => Some(ApiError::DeviceRevoked),
                    ApiError::SessionExpired => Some(ApiError::SessionExpired),
                    ApiError::Server { status: 401, .. } => Some(ApiError::SessionExpired),
                    _ => None,
                };
                match terminal {
                    Some(t) => {
                        tracing::warn!(error = %t, "token refresh rejected; clearing session");
                        let _ = self.clear_tokens().await;
                        Err(t)
                    }
                    None => Err(e),
                }
            }
        }
    }

    // ---- transport -------------------------------------------------------------

    /// One HTTP exchange. With `sign`, attaches a fresh device proof over the
    /// exact target and body bytes (if a signer is installed).
    async fn send(
        &self,
        method: Method,
        url: Url,
        body: Option<bytes::Bytes>,
        bearer: Option<&str>,
        sign: bool,
    ) -> Result<Raw, ApiError> {
        let path = url.path().to_owned();
        let proof = if sign {
            self.request_proof_header(method.as_str(), &url, body.as_deref().unwrap_or_default())?
        } else {
            None
        };
        let mut rb = self.inner.http.request(method.clone(), url);
        if let Some(body) = body {
            rb = rb.header(CONTENT_TYPE, "application/json").body(body);
        }
        if let Some(token) = bearer {
            rb = rb.bearer_auth(token);
        }
        if let Some(proof) = proof {
            rb = rb.header(HEADER_DEVICE_PROOF, proof);
        }
        let started = Instant::now();
        let resp = rb.send().await.map_err(map_transport)?;
        let status = resp.status();
        let headers = resp.headers().clone();
        let body = resp.bytes().await.map_err(map_transport)?;
        tracing::debug!(
            method = %method,
            path = %path,
            status = status.as_u16(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            request_id = headers.get(HEADER_REQUEST_ID).and_then(|v| v.to_str().ok()).unwrap_or(""),
            "api request"
        );
        Ok(Raw {
            status,
            headers,
            body,
        })
    }

    /// Signed exchange (protocol 1.5): re-signs and retries once on
    /// `422 invalid_proof` / `stale`; any other proof rejection becomes
    /// [`ApiError::RequestProofRejected`] (never retried unsigned).
    async fn send_signed(
        &self,
        method: Method,
        url: Url,
        body: Option<Vec<u8>>,
        bearer: Option<&str>,
    ) -> Result<Raw, ApiError> {
        let body = body.map(bytes::Bytes::from);
        let mut attempt = 0;
        loop {
            let raw = self
                .send(method.clone(), url.clone(), body.clone(), bearer, true)
                .await?;
            match proof_rejection(&raw) {
                None => return Ok(raw),
                Some(ProofRejection::Stale) if attempt == 0 => {
                    attempt += 1;
                    tracing::debug!(path = %url.path(), "request proof stale; re-signing once");
                }
                Some(reason) => {
                    tracing::warn!(path = %url.path(), reason = reason.as_str(), "request proof rejected");
                    return Err(ApiError::RequestProofRejected { reason });
                }
            }
        }
    }

    /// Authenticated request with refresh-and-retry on `401 unauthorized`.
    /// Returns the raw response for any status (callers map non-2xx).
    async fn send_authed(
        &self,
        method: Method,
        url: Url,
        body: Option<Vec<u8>>,
    ) -> Result<Raw, ApiError> {
        let token = self.access_token().await?;
        let raw = self
            .send_signed(
                method.clone(),
                url.clone(),
                body.clone(),
                Some(token.expose_secret()),
            )
            .await?;
        if raw.status == StatusCode::UNAUTHORIZED
            && error_body(&raw).is_none_or(|e| e.code == ErrorCode::Unauthorized)
        {
            self.refresh_after_rejection(token.expose_secret()).await?;
            let token = self
                .cached_tokens()
                .await?
                .ok_or(ApiError::NotAuthenticated)?;
            return self
                .send_signed(method, url, body, Some(token.access_token.expose_secret()))
                .await;
        }
        Ok(raw)
    }

    async fn public<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
    ) -> Result<T, ApiError> {
        let raw = check(
            self.send(
                method,
                self.endpoint(path),
                body.map(Into::into),
                None,
                false,
            )
            .await?,
        )?;
        decode(&raw)
    }

    async fn public_empty(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
    ) -> Result<(), ApiError> {
        check(
            self.send(
                method,
                self.endpoint(path),
                body.map(Into::into),
                None,
                false,
            )
            .await?,
        )
        .map(|_| ())
    }

    async fn authed<T: DeserializeOwned>(
        &self,
        method: Method,
        url: Url,
        body: Option<Vec<u8>>,
    ) -> Result<T, ApiError> {
        let raw = check(self.send_authed(method, url, body).await?)?;
        decode(&raw)
    }

    async fn authed_empty(
        &self,
        method: Method,
        url: Url,
        body: Option<Vec<u8>>,
    ) -> Result<(), ApiError> {
        check(self.send_authed(method, url, body).await?).map(|_| ())
    }

    // ---- meta -------------------------------------------------------------------

    /// `GET /v1/meta` (unauthenticated).
    pub async fn meta(&self) -> Result<ServerInfo, ApiError> {
        self.public(Method::GET, paths::META, None).await
    }

    // ---- auth ---------------------------------------------------------------------

    /// `POST /v1/auth/register`; stores the returned tokens.
    ///
    /// `409 already_exists` with `details.field = "device_id"` maps to
    /// [`ApiError::DeviceIdentityConflict`] (generate a new identity);
    /// `field = "email"` stays a server error (account exists → log in).
    /// Always send `device_proof` (protocol 1.4, ADR-0006).
    pub async fn register(&self, req: &RegisterRequest) -> Result<AuthResponse, ApiError> {
        let resp: AuthResponse = self
            .public(Method::POST, paths::AUTH_REGISTER, Some(to_json(req)?))
            .await
            .map_err(|e| match e {
                ApiError::Server { ref error, .. }
                    if error.code == ErrorCode::AlreadyExists
                        && error
                            .details
                            .as_ref()
                            .and_then(|d| d.get("field"))
                            .and_then(|f| f.as_str())
                            == Some("device_id") =>
                {
                    ApiError::DeviceIdentityConflict
                }
                e => e,
            })?;
        self.store_tokens(&resp.tokens).await?;
        Ok(resp)
    }

    /// `POST /v1/auth/login`; stores the returned tokens.
    ///
    /// Device ids are global: [`ApiError::DeviceIdentityRevoked`] (the id was
    /// revoked) and [`ApiError::DeviceIdentityConflict`] (id owned by another
    /// account or registered with other keys) mean the installation must
    /// generate a new device identity before logging in again. Always send
    /// `device_proof`: the server requires it for known device ids
    /// (`422 invalid_proof`, `details.reason = device_proof_required`).
    pub async fn login(&self, req: &LoginRequest) -> Result<AuthResponse, ApiError> {
        let resp: AuthResponse = self
            .public(Method::POST, paths::AUTH_LOGIN, Some(to_json(req)?))
            .await
            .map_err(|e| match e {
                ApiError::DeviceRevoked => ApiError::DeviceIdentityRevoked,
                e if e.is_code(ErrorCode::AlreadyExists) => ApiError::DeviceIdentityConflict,
                e => e,
            })?;
        self.store_tokens(&resp.tokens).await?;
        Ok(resp)
    }

    /// `POST /v1/auth/logout`; clears local tokens unless the request failed
    /// for network reasons.
    pub async fn logout(&self, req: &LogoutRequest) -> Result<(), ApiError> {
        let r = self
            .authed_empty(
                Method::POST,
                self.endpoint(paths::AUTH_LOGOUT),
                Some(to_json(req)?),
            )
            .await;
        match &r {
            Err(e) if e.is_offline() => {}
            _ => self.clear_tokens().await?,
        }
        r
    }

    /// `GET /v1/auth/me`.
    pub async fn me(&self) -> Result<AccountInfo, ApiError> {
        self.authed(Method::GET, self.endpoint(paths::AUTH_ME), None)
            .await
    }

    /// `POST /v1/auth/password/forgot` (always `202`).
    pub async fn password_forgot(&self, req: &ForgotPasswordRequest) -> Result<(), ApiError> {
        self.public_empty(
            Method::POST,
            paths::AUTH_PASSWORD_FORGOT,
            Some(to_json(req)?),
        )
        .await
    }

    /// `POST /v1/auth/password/reset`.
    pub async fn password_reset(&self, req: &ResetPasswordRequest) -> Result<(), ApiError> {
        self.public_empty(
            Method::POST,
            paths::AUTH_PASSWORD_RESET,
            Some(to_json(req)?),
        )
        .await
    }

    /// `POST /v1/auth/password/change`.
    pub async fn password_change(&self, req: &ChangePasswordRequest) -> Result<(), ApiError> {
        self.authed_empty(
            Method::POST,
            self.endpoint(paths::AUTH_PASSWORD_CHANGE),
            Some(to_json(req)?),
        )
        .await
    }

    /// `POST /v1/auth/email/verify`. `Some` when the server also logs in
    /// (tokens are stored), `None` on `204`.
    pub async fn email_verify(
        &self,
        req: &VerifyEmailRequest,
    ) -> Result<Option<AuthResponse>, ApiError> {
        let raw = check(
            self.send(
                Method::POST,
                self.endpoint(paths::AUTH_EMAIL_VERIFY),
                Some(to_json(req)?.into()),
                None,
                false,
            )
            .await?,
        )?;
        if raw.body.is_empty() {
            return Ok(None);
        }
        let resp: AuthResponse = decode(&raw)?;
        self.store_tokens(&resp.tokens).await?;
        Ok(Some(resp))
    }

    // ---- devices ----------------------------------------------------------------------

    /// `GET /v1/devices`.
    pub async fn list_devices(&self) -> Result<ListDevicesResponse, ApiError> {
        self.authed(Method::GET, self.endpoint(paths::DEVICES), None)
            .await
    }

    /// `POST /v1/devices` — ask to be trusted (device approval flow).
    pub async fn create_trust_request(
        &self,
        req: &CreateDeviceTrustRequest,
    ) -> Result<DeviceTrustRequest, ApiError> {
        self.authed(
            Method::POST,
            self.endpoint(paths::DEVICES),
            Some(to_json(req)?),
        )
        .await
    }

    /// `PATCH /v1/devices/{id}` — rename.
    pub async fn update_device(
        &self,
        device_id: DeviceId,
        req: &UpdateDeviceRequest,
    ) -> Result<DeviceInfo, ApiError> {
        self.authed(
            Method::PATCH,
            self.device_url(paths::DEVICE, device_id),
            Some(to_json(req)?),
        )
        .await
    }

    /// `POST /v1/devices/{id}/approve` → the updated request.
    pub async fn approve_device(
        &self,
        device_id: DeviceId,
        req: &ApproveDeviceRequest,
    ) -> Result<DeviceTrustRequest, ApiError> {
        self.authed(
            Method::POST,
            self.device_url(paths::DEVICE_APPROVE, device_id),
            Some(to_json(req)?),
        )
        .await
    }

    /// `POST /v1/devices/{id}/reject`.
    pub async fn reject_device(
        &self,
        device_id: DeviceId,
        req: &RejectDeviceRequest,
    ) -> Result<(), ApiError> {
        self.authed_empty(
            Method::POST,
            self.device_url(paths::DEVICE_REJECT, device_id),
            Some(to_json(req)?),
        )
        .await
    }

    /// `POST /v1/devices/{self}/attest` — self-trust with the vault access
    /// key → the stored device envelope.
    pub async fn attest_device(
        &self,
        device_id: DeviceId,
        req: &AttestDeviceRequest,
    ) -> Result<KeyEnvelope, ApiError> {
        self.authed(
            Method::POST,
            self.device_url(paths::DEVICE_ATTEST, device_id),
            Some(to_json(req)?),
        )
        .await
    }

    /// `POST /v1/devices/{id}/revoke`.
    pub async fn revoke_device(
        &self,
        device_id: DeviceId,
        req: &RevokeDeviceRequest,
    ) -> Result<(), ApiError> {
        self.authed_empty(
            Method::POST,
            self.device_url(paths::DEVICE_REVOKE, device_id),
            Some(to_json(req)?),
        )
        .await
    }

    fn device_url(&self, template: &str, device_id: DeviceId) -> Url {
        self.endpoint(&paths::fill(
            template,
            &[("device_id", &device_id.to_string())],
        ))
    }

    // ---- vaults & envelopes ----------------------------------------------------------------

    /// `POST /v1/vaults` → `201`.
    pub async fn create_vault(&self, req: &CreateVaultRequest) -> Result<VaultInfo, ApiError> {
        self.authed(
            Method::POST,
            self.endpoint(paths::VAULTS),
            Some(to_json(req)?),
        )
        .await
    }

    /// `GET /v1/vaults`.
    pub async fn list_vaults(&self) -> Result<ListVaultsResponse, ApiError> {
        self.authed(Method::GET, self.endpoint(paths::VAULTS), None)
            .await
    }

    /// `GET /v1/vaults/{id}`.
    pub async fn get_vault(&self, vault_id: VaultId) -> Result<VaultInfo, ApiError> {
        self.authed(Method::GET, self.vault_url(paths::VAULT, vault_id), None)
            .await
    }

    /// `DELETE /v1/vaults/{id}`.
    pub async fn delete_vault(
        &self,
        vault_id: VaultId,
        req: &DeleteVaultRequest,
    ) -> Result<(), ApiError> {
        self.authed_empty(
            Method::DELETE,
            self.vault_url(paths::VAULT, vault_id),
            Some(to_json(req)?),
        )
        .await
    }

    /// `GET /v1/vaults/{id}/envelopes`.
    pub async fn list_envelopes(
        &self,
        vault_id: VaultId,
    ) -> Result<ListEnvelopesResponse, ApiError> {
        self.authed(
            Method::GET,
            self.vault_url(paths::VAULT_ENVELOPES, vault_id),
            None,
        )
        .await
    }

    /// `POST /v1/vaults/{id}/envelopes` → `201`.
    pub async fn create_envelope(
        &self,
        vault_id: VaultId,
        req: &CreateEnvelopeRequest,
    ) -> Result<KeyEnvelope, ApiError> {
        self.authed(
            Method::POST,
            self.vault_url(paths::VAULT_ENVELOPES, vault_id),
            Some(to_json(req)?),
        )
        .await
    }

    /// `DELETE /v1/vaults/{id}/envelopes/{envelope_id}` (protocol 1.2 body
    /// [`DeleteEnvelopeRequest`]; only device/user envelopes).
    pub async fn delete_envelope(
        &self,
        vault_id: VaultId,
        envelope_id: EnvelopeId,
        req: &DeleteEnvelopeRequest,
    ) -> Result<(), ApiError> {
        let url = self.endpoint(&paths::fill(
            paths::VAULT_ENVELOPE,
            &[
                ("vault_id", &vault_id.to_string()),
                ("envelope_id", &envelope_id.to_string()),
            ],
        ));
        self.authed_empty(Method::DELETE, url, Some(to_json(req)?))
            .await
    }

    fn vault_url(&self, template: &str, vault_id: VaultId) -> Url {
        self.endpoint(&paths::fill(
            template,
            &[("vault_id", &vault_id.to_string())],
        ))
    }

    // ---- sync ----------------------------------------------------------------------------------

    /// `POST /v1/sync/push`. Returns the per-mutation results for both `200`
    /// (all accepted) and `409` (at least one conflict — accepted mutations
    /// in the batch are committed).
    pub async fn push(&self, req: &PushRequest) -> Result<PushResponse, ApiError> {
        let raw = self
            .send_authed(
                Method::POST,
                self.endpoint(paths::SYNC_PUSH),
                Some(to_json(req)?),
            )
            .await?;
        if raw.status == StatusCode::CONFLICT {
            if let Ok(resp) = serde_json::from_slice::<PushResponse>(&raw.body) {
                return Ok(resp);
            }
        }
        decode(&check(raw)?)
    }

    /// `GET /v1/sync/changes`.
    pub async fn changes(&self, q: &ChangesQuery) -> Result<ChangesResponse, ApiError> {
        let mut url = self.endpoint(paths::SYNC_CHANGES);
        {
            let mut qp = url.query_pairs_mut();
            qp.append_pair("vault_id", &q.vault_id.to_string());
            qp.append_pair("after", &q.after.to_string());
            if let Some(l) = q.limit {
                qp.append_pair("limit", &l.to_string());
            }
        }
        self.authed(Method::GET, url, None).await
    }

    /// `GET /v1/sync/snapshot`.
    pub async fn snapshot(&self, q: &SnapshotQuery) -> Result<SnapshotResponse, ApiError> {
        let mut url = self.endpoint(paths::SYNC_SNAPSHOT);
        {
            let mut qp = url.query_pairs_mut();
            qp.append_pair("vault_id", &q.vault_id.to_string());
            if let Some(c) = q.cursor {
                qp.append_pair("cursor", &c.to_string());
            }
            if let Some(l) = q.limit {
                qp.append_pair("limit", &l.to_string());
            }
        }
        self.authed(Method::GET, url, None).await
    }

    // ---- recovery ---------------------------------------------------------------------------------

    /// `POST /v1/recovery/account/start` (always `202`).
    pub async fn recovery_account_start(
        &self,
        req: &ForgotPasswordRequest,
    ) -> Result<(), ApiError> {
        self.public_empty(
            Method::POST,
            paths::RECOVERY_ACCOUNT_START,
            Some(to_json(req)?),
        )
        .await
    }

    /// `POST /v1/recovery/account/confirm`.
    pub async fn recovery_account_confirm(
        &self,
        req: &ResetPasswordRequest,
    ) -> Result<(), ApiError> {
        self.public_empty(
            Method::POST,
            paths::RECOVERY_ACCOUNT_CONFIRM,
            Some(to_json(req)?),
        )
        .await
    }

    /// `GET /v1/recovery/vault/envelope?vault_id=…`.
    pub async fn recovery_vault_envelope(
        &self,
        vault_id: VaultId,
    ) -> Result<VaultRecoveryMaterial, ApiError> {
        let mut url = self.endpoint(paths::RECOVERY_VAULT_ENVELOPE);
        url.query_pairs_mut()
            .append_pair("vault_id", &vault_id.to_string());
        self.authed(Method::GET, url, None).await
    }

    /// `POST /v1/recovery/vault/password-envelope/replace`.
    pub async fn replace_password_envelope(
        &self,
        req: &ReplaceEnvelopeRequest,
    ) -> Result<KeyEnvelope, ApiError> {
        self.authed(
            Method::POST,
            self.endpoint(paths::RECOVERY_VAULT_PASSWORD_REPLACE),
            Some(to_json(req)?),
        )
        .await
    }

    /// `POST /v1/recovery/vault/recovery-envelope/replace`.
    pub async fn replace_recovery_envelope(
        &self,
        req: &ReplaceEnvelopeRequest,
    ) -> Result<KeyEnvelope, ApiError> {
        self.authed(
            Method::POST,
            self.endpoint(paths::RECOVERY_VAULT_RECOVERY_REPLACE),
            Some(to_json(req)?),
        )
        .await
    }
}

fn expiring(tokens: &TokenPair) -> bool {
    let skew = chrono::Duration::from_std(EXPIRY_SKEW).unwrap_or_default();
    tokens.access_expires_at <= chrono::Utc::now() + skew
}

fn to_json<T: Serialize>(v: &T) -> Result<Vec<u8>, ApiError> {
    serde_json::to_vec(v).map_err(|e| ApiError::InvalidConfig(format!("encode request: {e}")))
}

/// Decode a successful body. The error reason carries only the position and
/// category, never echoes body content (which may include tokens).
fn decode<T: DeserializeOwned>(raw: &Raw) -> Result<T, ApiError> {
    serde_json::from_slice(&raw.body).map_err(|e| ApiError::InvalidResponse {
        status: raw.status.as_u16(),
        reason: format!(
            "malformed {:?} body at line {} column {}",
            e.classify(),
            e.line(),
            e.column()
        ),
    })
}

fn error_body(raw: &Raw) -> Option<cc_protocol::ApiError> {
    serde_json::from_slice(&raw.body).ok()
}

/// `422 invalid_proof` for a signed request → the reason.
fn proof_rejection(raw: &Raw) -> Option<ProofRejection> {
    if raw.status != StatusCode::UNPROCESSABLE_ENTITY {
        return None;
    }
    let e = error_body(raw)?;
    (e.code == ErrorCode::InvalidProof).then(|| {
        ProofRejection::parse(
            e.details
                .as_ref()
                .and_then(|d| d.get("reason"))
                .and_then(|r| r.as_str()),
        )
    })
}

fn check(raw: Raw) -> Result<Raw, ApiError> {
    if raw.status.is_success() {
        Ok(raw)
    } else {
        Err(map_error(&raw))
    }
}

/// Map a non-2xx response to an [`ApiError`].
pub(crate) fn map_error(raw: &Raw) -> ApiError {
    let status = raw.status.as_u16();
    let err = error_body(raw).unwrap_or_else(|| {
        cc_protocol::ApiError::new(code_for_status(status), "error response without JSON body")
    });
    let header_retry = raw
        .headers
        .get(RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_secs);
    match err.code {
        ErrorCode::Unauthorized if status == 401 => ApiError::SessionExpired,
        ErrorCode::RefreshTokenReused => ApiError::RefreshTokenReused,
        ErrorCode::DeviceRevoked => ApiError::DeviceRevoked,
        ErrorCode::UpgradeRequired => ApiError::UpgradeRequired {
            server: err
                .details
                .clone()
                .and_then(|d| serde_json::from_value::<ServerInfo>(d).ok())
                .map(Box::new),
        },
        ErrorCode::RateLimited => ApiError::RateLimited {
            retry_after: err
                .retry_after_seconds
                .map(|s| Duration::from_secs(u64::from(s)))
                .or(header_retry),
        },
        _ => ApiError::Server {
            status,
            error: Box::new(err),
        },
    }
}

fn code_for_status(status: u16) -> ErrorCode {
    match status {
        400 => ErrorCode::BadRequest,
        401 => ErrorCode::Unauthorized,
        403 => ErrorCode::Forbidden,
        404 => ErrorCode::NotFound,
        409 => ErrorCode::Conflict,
        410 => ErrorCode::Gone,
        413 => ErrorCode::PayloadTooLarge,
        422 => ErrorCode::InvalidProof,
        426 => ErrorCode::UpgradeRequired,
        429 => ErrorCode::RateLimited,
        503 => ErrorCode::Unavailable,
        s if s >= 500 => ErrorCode::Internal,
        _ => ErrorCode::BadRequest,
    }
}

fn map_transport(e: reqwest::Error) -> ApiError {
    if e.is_timeout() {
        return ApiError::Timeout;
    }
    // Error chain (connection refused, reset, …) without the URL.
    let e = e.without_url();
    let mut msg = e.to_string();
    let mut src = std::error::Error::source(&e);
    while let Some(s) = src {
        msg.push_str(": ");
        msg.push_str(&s.to_string());
        src = s.source();
    }
    ApiError::Network(msg.chars().take(300).collect())
}
