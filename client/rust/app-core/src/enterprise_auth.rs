//! Enterprise sign-in to the sync server (protocol 1.8, TZ-2026-10-21
//! ТР-4/ТР-5/ТР-11): the server's `capabilities.auth` says the account is
//! authenticated by the organisation's IdP (OIDC) or directory (LDAP / AD)
//! instead of an account password.
//!
//! * **OIDC** — [`AppCore::enterprise_oidc_begin`] creates the PKCE
//!   `code_verifier` (S256) and `state`, a redirect target (desktop: an RFC
//!   8252 loopback listener on `127.0.0.1`/`[::1]`; mobile:
//!   `consolecrypt://auth/callback` through Custom Tabs /
//!   `ASWebAuthenticationSession`) and the `GET /v1/auth/oidc/start` URL
//!   that the UI opens in the system browser.
//!   [`AppCore::enterprise_oidc_complete`] waits for (desktop) or receives
//!   (mobile) the redirect, checks `state`, redeems the single-use
//!   `login_code` once and continues with the ordinary device/token setup.
//! * **LDAP** — login + directory password, sent once over TLS to
//!   `POST /v1/auth/ldap/login`; the password is never stored or logged.
//!
//! Nothing here can decrypt a vault: after the sign-in the device is an
//! ordinary new device (device protection, vault passphrase / Recovery Key /
//! approval). The browser `login_code` is bound to the device id given at
//! start, so the profile storage + device identity of a new profile are
//! created before the browser opens.

use crate::app::{server_host, AppCore, Credential};
use crate::dto::{AuthStateDto, ProfileInfo, SyncedAccountDto};
use crate::error::{AppError, AppResult};
use crate::loopback::{await_callback, LoopbackError};
use crate::secrets;
use base64::Engine as _;
use cc_protocol::auth::SecretString;
use cc_protocol::ErrorCode;
use cc_storage_core::{ProfileId, ProfileKind, Storage};
use cc_sync_core::api::enterprise::{
    callback_params, is_allowed_app_redirect_uri, OidcStartQuery, APP_REDIRECT_PATH,
    APP_REDIRECT_URI_MOBILE, CODE_CHALLENGE_METHOD_S256, MAX_ENTERPRISE_LOGIN_LEN,
    OIDC_FLOW_TTL_SECONDS,
};
use cc_sync_core::ApiError;
use cc_vault_core::DeviceIdentity;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;
use zeroize::Zeroizing;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// Longest `login_code` accepted from a redirect.
const MAX_LOGIN_CODE_LEN: usize = 1024;

fn flow_ttl() -> Duration {
    Duration::from_secs(OIDC_FLOW_TTL_SECONDS.max(60) as u64)
}

fn invalid(rule: &'static str) -> AppError {
    AppError::invalid("enterprise_auth", rule)
}

// ---- PKCE / state ---------------------------------------------------------------------

/// 32 CSPRNG bytes, base64url without padding (43 characters).
fn random_token() -> AppResult<Zeroizing<String>> {
    let mut bytes = Zeroizing::new([0u8; 32]);
    cc_crypto_core::fill_random(&mut bytes[..]).map_err(|e| AppError::Crypto(e.to_string()))?;
    Ok(Zeroizing::new(B64.encode(&bytes[..])))
}

/// RFC 7636 S256: `BASE64URL-NOPAD(SHA-256(verifier))`.
pub(crate) fn code_challenge(verifier: &str) -> String {
    B64.encode(Sha256::digest(verifier.as_bytes()))
}

/// A PKCE pair. The verifier only leaves the process in the exchange body.
pub(crate) struct Pkce {
    verifier: Zeroizing<String>,
    challenge: String,
}

impl Pkce {
    pub(crate) fn generate() -> AppResult<Self> {
        let verifier = random_token()?;
        let challenge = code_challenge(&verifier);
        Ok(Self {
            verifier,
            challenge,
        })
    }
}

/// Constant-time string equality (state comparison).
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

// ---- redirect / callback --------------------------------------------------------------

/// Loopback redirect URI for a listener port (RFC 8252 §7.3).
pub(crate) fn loopback_redirect_uri(addr: std::net::SocketAddr) -> String {
    match addr {
        std::net::SocketAddr::V4(a) => {
            format!("http://127.0.0.1:{}{APP_REDIRECT_PATH}", a.port())
        }
        std::net::SocketAddr::V6(a) => format!("http://[::1]:{}{APP_REDIRECT_PATH}", a.port()),
    }
}

/// Bind the loopback listener: IPv4 first, IPv6 when IPv4 is unavailable.
async fn bind_loopback() -> AppResult<(TcpListener, String)> {
    let listener = match TcpListener::bind("127.0.0.1:0").await {
        Ok(l) => l,
        Err(_) => TcpListener::bind("[::1]:0")
            .await
            .map_err(|_| invalid("browser_unavailable"))?,
    };
    let addr = listener
        .local_addr()
        .map_err(|_| invalid("browser_unavailable"))?;
    Ok((listener, loopback_redirect_uri(addr)))
}

fn same_endpoint(callback: &Url, redirect: &Url) -> bool {
    callback.scheme() == redirect.scheme()
        && callback.host_str() == redirect.host_str()
        && callback.port() == redirect.port()
        && callback.path() == redirect.path()
        && callback.username().is_empty()
        && callback.password().is_none()
        && callback.fragment().is_none()
}

fn params(callback: &Url) -> BTreeMap<String, Vec<String>> {
    let mut m: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (k, v) in callback.query_pairs() {
        m.entry(k.into_owned()).or_default().push(v.into_owned());
    }
    m
}

fn single<'a>(p: &'a BTreeMap<String, Vec<String>>, key: &str) -> Option<&'a str> {
    match p.get(key).map(Vec::as_slice) {
        Some([v]) => Some(v.as_str()),
        _ => None,
    }
}

/// Whether `callback` is a redirect to `redirect` carrying exactly the
/// expected `state` (checked before anything else in it is trusted).
pub(crate) fn callback_state_matches(callback: &Url, redirect: &Url, state: &str) -> bool {
    same_endpoint(callback, redirect)
        && single(&params(callback), callback_params::STATE).is_some_and(|s| same(s, state))
}

/// The server's redirect back to the app → the `login_code`, or the error
/// it reports. `state` must match exactly (ТР-4 step 3).
pub(crate) fn parse_app_callback(
    callback: &str,
    redirect_uri: &str,
    expected_state: &str,
) -> AppResult<SecretString> {
    let callback = Zeroizing::new(callback.to_owned());
    let url = Url::parse(&callback).map_err(|_| invalid("invalid_callback"))?;
    let redirect = Url::parse(redirect_uri).map_err(|_| invalid("invalid_callback"))?;
    if !callback_state_matches(&url, &redirect, expected_state) {
        return Err(invalid("invalid_callback"));
    }
    let p = params(&url);
    if p.contains_key(callback_params::ERROR) {
        let code = single(&p, callback_params::ERROR).unwrap_or("bad_request");
        let description = single(&p, callback_params::ERROR_DESCRIPTION).unwrap_or("");
        return Err(callback_error(code, description));
    }
    match single(&p, callback_params::LOGIN_CODE) {
        Some(code) if !code.is_empty() && code.len() <= MAX_LOGIN_CODE_LEN => {
            Ok(SecretString::new(code))
        }
        _ => Err(invalid("invalid_callback")),
    }
}

/// An `error` from the redirect (an `ErrorCode` in snake_case) → the same
/// [`AppError`] the API call would have produced.
pub(crate) fn callback_error(code: &str, description: &str) -> AppError {
    if matches!(code, "access_denied" | "cancelled") {
        return AppError::Cancelled;
    }
    let message: String = description
        .chars()
        .filter(|c| !c.is_control())
        .take(200)
        .collect();
    match serde_json::from_value::<ErrorCode>(serde_json::Value::String(code.to_owned())) {
        Ok(ErrorCode::RateLimited) => ApiError::RateLimited { retry_after: None }.into(),
        Ok(code) => ApiError::Server {
            status: code.http_status(),
            error: Box::new(cc_protocol::ApiError::new(code, message)),
        }
        .into(),
        Err(_) => AppError::Server {
            status: 0,
            code: "bad_request".into(),
            message,
        },
    }
}

/// Validated LDAP login (email, UPN or `DOMAIN\user`).
pub(crate) fn ldap_login(login: &str) -> AppResult<String> {
    let t = login.trim();
    if t.is_empty() {
        return Err(AppError::invalid("login", "required"));
    }
    if t.len() > MAX_ENTERPRISE_LOGIN_LEN || t.chars().any(char::is_control) {
        return Err(AppError::invalid("login", "not a valid login"));
    }
    Ok(t.to_owned())
}

fn login_hint(hint: Option<String>) -> Option<String> {
    hint.map(|h| h.trim().to_owned())
        .filter(|h| !h.is_empty() && h.len() <= MAX_ENTERPRISE_LOGIN_LEN)
        .filter(|h| !h.chars().any(char::is_control))
}

// ---- pending flows --------------------------------------------------------------------

/// What a browser sign-in is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnterpriseSignInPurpose {
    /// "Connect to a server": a new synced profile.
    NewProfile,
    /// Sign the active synced profile in again.
    Reauthenticate,
    /// Local → Synced for the active (unlocked) profile.
    EnableSync,
}

impl std::str::FromStr for EnterpriseSignInPurpose {
    type Err = AppError;
    fn from_str(s: &str) -> AppResult<Self> {
        serde_json::from_value(serde_json::Value::String(s.to_owned()))
            .map_err(|_| AppError::invalid("purpose", "unknown sign-in purpose"))
    }
}

/// Open this URL in the system browser (`ticket` completes or cancels).
#[derive(Clone, Serialize)]
pub struct EnterpriseOidcChallengeDto {
    pub ticket: String,
    pub authorization_url: String,
    pub redirect_uri: String,
    /// Desktop: the core receives the redirect itself (complete without a
    /// callback URL). Mobile: pass the URL the OS auth session returned.
    pub loopback: bool,
    pub expires_at_ms: i64,
}

impl std::fmt::Debug for EnterpriseOidcChallengeDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnterpriseOidcChallengeDto")
            .field("ticket", &self.ticket)
            .field("loopback", &self.loopback)
            .finish_non_exhaustive()
    }
}

/// Outcome of an enterprise sign-in, by purpose.
#[derive(Debug, Clone, Serialize)]
pub struct EnterpriseSignInResultDto {
    pub purpose: EnterpriseSignInPurpose,
    /// [`EnterpriseSignInPurpose::NewProfile`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<SyncedAccountDto>,
    /// [`EnterpriseSignInPurpose::Reauthenticate`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_state: Option<AuthStateDto>,
    /// [`EnterpriseSignInPurpose::EnableSync`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProfileInfo>,
}

enum Target {
    NewProfile {
        pid: ProfileId,
        storage: Storage,
        identity: Arc<DeviceIdentity>,
        display_name: Option<String>,
    },
    Reauthenticate {
        pid: ProfileId,
    },
    EnableSync {
        pid: ProfileId,
    },
}

struct Pending {
    target: Target,
    server_url: String,
    pkce: Pkce,
    state: Zeroizing<String>,
    redirect_uri: String,
    listener: Option<TcpListener>,
    cancel: CancellationToken,
    expires: Instant,
}

#[derive(Default)]
struct FlowState {
    /// Started, not yet being completed.
    pending: BTreeMap<Uuid, Pending>,
    /// Being completed (waiting for the browser): cancel tokens.
    active: BTreeMap<Uuid, CancellationToken>,
}

/// Browser sign-ins in progress (at most one at a time).
#[derive(Default)]
pub struct PendingFlows {
    state: Mutex<FlowState>,
}

impl std::fmt::Debug for PendingFlows {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingFlows").finish_non_exhaustive()
    }
}

impl PendingFlows {
    fn lock(&self) -> std::sync::MutexGuard<'_, FlowState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }
}

fn parse_ticket(ticket: &str) -> AppResult<Uuid> {
    ticket
        .trim()
        .parse()
        .map_err(|_| AppError::invalid("ticket", "not a valid id"))
}

impl AppCore {
    async fn drop_pending(&self, pending: Pending) {
        pending.cancel.cancel();
        if let Target::NewProfile { pid, storage, .. } = pending.target {
            self.discard_profile(pid, Some(storage)).await;
        }
    }

    /// Cancel every browser sign-in (a new one replaces the old).
    async fn cancel_all_enterprise_flows(&self) {
        let old: Vec<Pending> = {
            let mut st = self.inner.enterprise.lock();
            for token in st.active.values() {
                token.cancel();
            }
            std::mem::take(&mut st.pending).into_values().collect()
        };
        for p in old {
            self.drop_pending(p).await;
        }
    }

    /// Start a browser (OIDC) sign-in at `server_url` → open
    /// `authorization_url` in the system browser, then
    /// [`AppCore::enterprise_oidc_complete`].
    ///
    /// * `NewProfile`: creates the profile storage and device identity now
    ///   (named `display_name`, else the account e-mail once known).
    /// * `Reauthenticate`: the active synced profile (`server_url` must be
    ///   its server); `allow_new_device_identity` first replaces a revoked /
    ///   conflicting device identity (the device must attest again).
    /// * `EnableSync`: the active, unlocked local profile.
    pub async fn enterprise_oidc_begin(
        &self,
        server_url: String,
        purpose: EnterpriseSignInPurpose,
        login_hint_value: Option<String>,
        display_name: Option<String>,
        allow_new_device_identity: bool,
    ) -> AppResult<EnterpriseOidcChallengeDto> {
        let server_url = crate::validate::server_url(&server_url)?;
        let server_url = server_url.trim().to_owned();
        self.inner.ctx.check_server_url(&server_url)?;
        let display_name = display_name
            .map(|n| n.trim().to_owned())
            .filter(|n| !n.is_empty());
        if let Some(n) = &display_name {
            crate::validate::display_name("display_name", n)?;
        }
        self.cancel_all_enterprise_flows().await;
        let (target, device_id) = match purpose {
            EnterpriseSignInPurpose::NewProfile => {
                let name = display_name
                    .clone()
                    .unwrap_or_else(|| server_host(&server_url));
                let (pid, storage, identity) =
                    self.new_profile_storage(&name, ProfileKind::Synced).await?;
                let device_id = identity.device_id();
                (
                    Target::NewProfile {
                        pid,
                        storage,
                        identity,
                        display_name,
                    },
                    device_id,
                )
            }
            EnterpriseSignInPurpose::Reauthenticate => {
                let s = self.session().await?;
                if s.kind().await? != ProfileKind::Synced {
                    return Err(AppError::LocalProfile);
                }
                let profile = s.profile().await?;
                let same_server = profile
                    .server_url
                    .as_deref()
                    .is_some_and(|u| u.trim_end_matches('/') == server_url.trim_end_matches('/'));
                if !same_server {
                    return Err(AppError::invalid("server_url", "not the profile's server"));
                }
                if allow_new_device_identity {
                    let fresh = Arc::new(
                        secrets::regenerate_identity(self.inner.ctx.secure.clone(), s.profile_id)
                            .await?,
                    );
                    s.set_identity(fresh);
                    s.set_needs_attest(true).await?;
                }
                (Target::Reauthenticate { pid: s.profile_id }, s.device_id())
            }
            EnterpriseSignInPurpose::EnableSync => {
                let (s, _) = self.unlocked().await?;
                if s.kind().await? == ProfileKind::Synced {
                    return Err(AppError::AlreadySynced);
                }
                (Target::EnableSync { pid: s.profile_id }, s.device_id())
            }
        };
        let prepared = async {
            let (listener, redirect_uri) = if cfg!(any(target_os = "android", target_os = "ios")) {
                (None, APP_REDIRECT_URI_MOBILE.to_owned())
            } else {
                let (l, uri) = bind_loopback().await?;
                (Some(l), uri)
            };
            debug_assert!(is_allowed_app_redirect_uri(&redirect_uri));
            let pkce = Pkce::generate()?;
            let state = random_token()?;
            Ok::<_, AppError>((listener, redirect_uri, pkce, state))
        }
        .await;
        let (listener, redirect_uri, pkce, state) = match prepared {
            Ok(v) => v,
            Err(e) => {
                if let Target::NewProfile { pid, storage, .. } = target {
                    self.discard_profile(pid, Some(storage)).await;
                }
                return Err(e);
            }
        };
        let query = OidcStartQuery {
            redirect_uri: redirect_uri.clone(),
            code_challenge: pkce.challenge.clone(),
            code_challenge_method: Some(CODE_CHALLENGE_METHOD_S256.to_owned()),
            state: state.to_string(),
            device_id,
            login_hint: login_hint(login_hint_value),
        };
        let authorization_url = query.to_url(&server_url);
        let ticket = Uuid::new_v4();
        let expires = Instant::now() + flow_ttl();
        let loopback = listener.is_some();
        self.inner.enterprise.lock().pending.insert(
            ticket,
            Pending {
                target,
                server_url,
                pkce,
                state,
                redirect_uri: redirect_uri.clone(),
                listener,
                cancel: CancellationToken::new(),
                expires,
            },
        );
        tracing::info!(purpose = ?purpose, loopback, "enterprise sign-in started");
        Ok(EnterpriseOidcChallengeDto {
            ticket: ticket.to_string(),
            authorization_url,
            redirect_uri,
            loopback,
            expires_at_ms: chrono::Utc::now().timestamp_millis() + flow_ttl().as_millis() as i64,
        })
    }

    /// Finish a browser sign-in: wait for the loopback redirect (desktop,
    /// `callback_url = None`) or take the URL the OS auth session returned
    /// (mobile), check `state`, redeem the `login_code` once and continue
    /// like a password sign-in of the same purpose.
    pub async fn enterprise_oidc_complete(
        &self,
        ticket: String,
        callback_url: Option<String>,
    ) -> AppResult<EnterpriseSignInResultDto> {
        let id = parse_ticket(&ticket)?;
        let pending = {
            let mut st = self.inner.enterprise.lock();
            let p = st.pending.remove(&id).ok_or(AppError::Cancelled)?;
            st.active.insert(id, p.cancel.clone());
            p
        };
        let r = self.complete_pending(&pending, callback_url).await;
        self.inner.enterprise.lock().active.remove(&id);
        let credential = match r {
            Ok(c) => c,
            Err(e) => {
                self.drop_pending(pending).await;
                return Err(e);
            }
        };
        let Pending {
            target, server_url, ..
        } = pending;
        let mut out = EnterpriseSignInResultDto {
            purpose: EnterpriseSignInPurpose::NewProfile,
            account: None,
            auth_state: None,
            profile: None,
        };
        match target {
            Target::NewProfile {
                pid,
                storage,
                identity,
                display_name,
            } => {
                out.account = Some(
                    self.create_synced_profile_with(
                        display_name,
                        server_url,
                        credential,
                        Some((pid, storage, identity)),
                    )
                    .await?,
                );
            }
            Target::Reauthenticate { pid } => {
                if self.session().await?.profile_id != pid {
                    return Err(AppError::Cancelled);
                }
                out.purpose = EnterpriseSignInPurpose::Reauthenticate;
                out.auth_state = Some(self.reauthenticate_with(credential, false).await?);
            }
            Target::EnableSync { pid } => {
                if self.session().await?.profile_id != pid {
                    return Err(AppError::Cancelled);
                }
                out.purpose = EnterpriseSignInPurpose::EnableSync;
                out.profile = Some(self.enable_sync_with(server_url, credential).await?);
            }
        }
        tracing::info!(purpose = ?out.purpose, "enterprise sign-in finished");
        Ok(out)
    }

    async fn complete_pending(
        &self,
        pending: &Pending,
        callback_url: Option<String>,
    ) -> AppResult<Credential> {
        if Instant::now() >= pending.expires {
            return Err(invalid("timeout"));
        }
        let redirect =
            Url::parse(&pending.redirect_uri).map_err(|_| invalid("invalid_callback"))?;
        let callback = Zeroizing::new(match (&pending.listener, callback_url) {
            (Some(listener), None) => {
                let state: &str = &pending.state;
                await_callback(
                    listener,
                    &redirect,
                    |u| callback_state_matches(u, &redirect, state),
                    pending.cancel.clone(),
                    pending.expires,
                )
                .await
                .map_err(loopback_error)?
                .to_string()
            }
            (None, Some(callback)) => callback,
            _ => return Err(invalid("invalid_callback")),
        });
        let login_code = parse_app_callback(&callback, &pending.redirect_uri, &pending.state)?;
        Ok(Credential::Oidc {
            login_code,
            code_verifier: SecretString::new(pending.pkce.verifier.to_string()),
        })
    }

    /// Abandon a browser sign-in (the UI's Cancel, or the OS auth session
    /// was dismissed). A new profile prepared for it is removed.
    pub async fn enterprise_oidc_cancel(&self, ticket: String) -> AppResult<()> {
        let id = parse_ticket(&ticket)?;
        let pending = {
            let mut st = self.inner.enterprise.lock();
            if let Some(token) = st.active.get(&id) {
                token.cancel();
            }
            st.pending.remove(&id)
        };
        if let Some(p) = pending {
            self.drop_pending(p).await;
        }
        Ok(())
    }

    /// New synced profile signed in with the organisation's directory
    /// (LDAP / Active Directory) → as [`AppCore::create_synced_profile`].
    pub async fn create_synced_profile_ldap(
        &self,
        display_name: Option<String>,
        server_url: String,
        login: String,
        password: String,
    ) -> AppResult<SyncedAccountDto> {
        let credential = ldap_credential(login, password)?;
        let display_name = display_name
            .map(|n| n.trim().to_owned())
            .filter(|n| !n.is_empty());
        self.create_synced_profile_with(display_name, server_url, credential, None)
            .await
    }

    /// Sign the active synced profile in again with the directory.
    pub async fn reauthenticate_ldap(
        &self,
        login: String,
        password: String,
        allow_new_device_identity: bool,
    ) -> AppResult<AuthStateDto> {
        let credential = ldap_credential(login, password)?;
        self.reauthenticate_with(credential, allow_new_device_identity)
            .await
    }

    /// Local → Synced, signing in with the directory.
    pub async fn enable_sync_ldap(
        &self,
        server_url: String,
        login: String,
        password: String,
    ) -> AppResult<ProfileInfo> {
        let credential = ldap_credential(login, password)?;
        self.enable_sync_with(server_url, credential).await
    }
}

fn loopback_error(e: LoopbackError) -> AppError {
    match e {
        LoopbackError::Cancelled => AppError::Cancelled,
        LoopbackError::Timeout => invalid("timeout"),
        LoopbackError::Unavailable => invalid("browser_unavailable"),
    }
}

fn ldap_credential(login: String, password: String) -> AppResult<Credential> {
    let login = ldap_login(&login)?;
    crate::validate::secret_text("password", &password)?;
    if password.is_empty() {
        return Err(AppError::invalid("password", "required"));
    }
    Ok(Credential::Ldap {
        login,
        password: SecretString::new(password),
    })
}

#[cfg(test)]
#[path = "enterprise_auth_tests.rs"]
mod tests;
