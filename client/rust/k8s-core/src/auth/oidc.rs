//! Browser-based OpenID Connect authorization code flow with PKCE. Browser
//! launch and callback delivery belong to the platform runtime; this module
//! owns the verifier, nonce, token exchange and strict ID-token validation.

use super::{AuthError, Credential};
use crate::session::Authentication;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use futures::StreamExt;
use ring::signature;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Deserializer};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;
use zeroize::Zeroizing;

const MAX_RESPONSE: usize = 2 * 1024 * 1024;
const MAX_TOKEN: usize = 256 * 1024;
const LOGIN_TTL: Duration = Duration::from_secs(600);

pub struct OidcConfig {
    pub issuer: Url,
    pub client_id: String,
    pub client_secret: Option<SecretString>,
    pub scopes: Vec<String>,
    pub certificate_authority_pem: Option<String>,
}

impl fmt::Debug for OidcConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OidcConfig").finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct OidcClient {
    http: reqwest::Client,
    issuer: Url,
    client_id: String,
    client_secret: Option<SecretString>,
    scopes: Vec<String>,
    metadata: Metadata,
}

impl fmt::Debug for OidcClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OidcClient").finish_non_exhaustive()
    }
}

/// Tokens never implement Serialize. The caller may persist the refresh
/// token only as an existing encrypted Secret object after explicit policy.
pub struct TokenSet {
    pub id_token: SecretString,
    pub access_token: SecretString,
    pub refresh_token: Option<SecretString>,
    pub expires_at: SystemTime,
    issuer: String,
    client_id: String,
    subject: SecretString,
    nonce: Option<SecretString>,
}

/// The minimum previously verified identity needed for a refresh. It contains
/// no expired bearer or access token and may remain in native unlocked memory.
#[derive(Clone)]
pub struct RefreshIdentity {
    issuer: String,
    client_id: String,
    subject: SecretString,
    nonce: Option<SecretString>,
}

impl fmt::Debug for RefreshIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RefreshIdentity").finish_non_exhaustive()
    }
}

impl fmt::Debug for TokenSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenSet")
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

impl TokenSet {
    pub fn refresh_identity(&self) -> RefreshIdentity {
        RefreshIdentity {
            issuer: self.issuer.clone(),
            client_id: self.client_id.clone(),
            subject: self.subject.clone(),
            nonce: self.nonce.clone(),
        }
    }

    /// Kubernetes OIDC authenticates the ID token, not the OAuth access token.
    pub fn credential(&self) -> Result<Credential, AuthError> {
        if SystemTime::now() >= self.expires_at {
            return Err(AuthError::InvalidToken);
        }
        Ok(Credential {
            authentication: Authentication::Bearer(self.id_token.clone()),
            expires_at: self.expires_at,
        })
    }
}

pub struct LoginFlow {
    client: OidcClient,
    redirect: Url,
    authorization_url: Url,
    verifier: SecretString,
    state: SecretString,
    nonce: SecretString,
    created: Instant,
}

impl fmt::Debug for LoginFlow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoginFlow").finish_non_exhaustive()
    }
}

impl LoginFlow {
    /// Send only to the operating system browser/authentication session. Do
    /// not log this URL or put it into synchronized history.
    pub fn authorization_url(&self) -> &Url {
        &self.authorization_url
    }

    pub fn redirect_uri(&self) -> &Url {
        &self.redirect
    }

    /// A loopback listener may reject unrelated requests without consuming the
    /// pending login flow. This does not contact the issuer or redeem a code.
    pub fn validate_callback(&self, callback: &Url) -> Result<(), AuthError> {
        if self.created.elapsed() >= LOGIN_TTL {
            return Err(AuthError::Timeout);
        }
        callback_code(callback, &self.redirect, self.state.expose_secret())?;
        if callback
            .query_pairs()
            .any(|(name, value)| name == "iss" && value != self.client.issuer.as_str())
        {
            return Err(AuthError::InvalidCallback);
        }
        Ok(())
    }

    /// Consumes the flow, including failed callbacks: codes are never retried
    /// automatically, and a second callback cannot reuse the verifier.
    pub async fn finish(
        self,
        callback: &Url,
        cancellation: CancellationToken,
    ) -> Result<TokenSet, AuthError> {
        self.validate_callback(callback)?;
        let code = callback_code(callback, &self.redirect, self.state.expose_secret())?;
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("client_id", self.client.client_id.as_str()),
            ("code", code.expose_secret()),
            ("redirect_uri", self.redirect.as_str()),
            ("code_verifier", self.verifier.expose_secret()),
        ];
        if let Some(secret) = &self.client.client_secret {
            form.push(("client_secret", secret.expose_secret()));
        }
        let response = self.client.exchange(&form, cancellation.clone()).await?;
        self.client
            .verify_response(response, Some(self.nonce), None, cancellation)
            .await
    }
}

#[derive(Debug, Clone, Deserialize)]
struct Metadata {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
    #[serde(default)]
    code_challenge_methods_supported: Vec<String>,
    #[serde(default)]
    id_token_signing_alg_values_supported: Vec<String>,
    #[serde(default)]
    token_endpoint_auth_methods_supported: Vec<String>,
}

impl OidcClient {
    pub async fn discover(
        config: OidcConfig,
        cancellation: CancellationToken,
    ) -> Result<Self, AuthError> {
        validate_https(&config.issuer)?;
        if config.client_id.is_empty()
            || config.client_id.len() > 1024
            || config.client_id.chars().any(char::is_control)
            || config.scopes.len() > 64
            || config.scopes.iter().any(|scope| {
                scope.is_empty()
                    || scope.len() > 256
                    || scope.bytes().any(|b| !b.is_ascii_graphic())
            })
            || config.client_secret.as_ref().is_some_and(|secret| {
                secret.expose_secret().is_empty() || secret.expose_secret().len() > MAX_TOKEN
            })
        {
            return Err(AuthError::InvalidConfiguration);
        }
        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_proxy()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30));
        if let Some(pem) = config.certificate_authority_pem {
            if pem.is_empty() || pem.len() > 4 * 1024 * 1024 {
                return Err(AuthError::InvalidConfiguration);
            }
            let certificates = reqwest::Certificate::from_pem_bundle(pem.as_bytes())
                .map_err(|_| AuthError::InvalidConfiguration)?;
            if certificates.is_empty() || certificates.len() > 64 {
                return Err(AuthError::InvalidConfiguration);
            }
            builder = builder.tls_certs_only(certificates);
        }
        let http = builder.build().map_err(|_| AuthError::Transport)?;
        let discovery = format!(
            "{}/.well-known/openid-configuration",
            config.issuer.as_str().trim_end_matches('/')
        );
        let bytes = fetch(
            http.get(discovery).header("Accept", "application/json"),
            cancellation,
        )
        .await?;
        let metadata: Metadata =
            serde_json::from_slice(&bytes).map_err(|_| AuthError::InvalidResponse)?;
        if metadata.issuer != config.issuer.as_str()
            || !metadata
                .code_challenge_methods_supported
                .iter()
                .any(|method| method == "S256")
            || !metadata
                .id_token_signing_alg_values_supported
                .iter()
                .any(|alg| matches!(alg.as_str(), "RS256" | "ES256"))
        {
            return Err(AuthError::InvalidConfiguration);
        }
        for endpoint in [
            &metadata.authorization_endpoint,
            &metadata.token_endpoint,
            &metadata.jwks_uri,
        ] {
            validate_https(&Url::parse(endpoint).map_err(|_| AuthError::InvalidConfiguration)?)?;
        }
        if config.client_secret.is_some()
            && !metadata.token_endpoint_auth_methods_supported.is_empty()
            && !metadata
                .token_endpoint_auth_methods_supported
                .iter()
                .any(|method| method == "client_secret_post" || method == "client_secret_basic")
        {
            return Err(AuthError::InvalidConfiguration);
        }
        let mut scopes = config.scopes;
        if !scopes.iter().any(|scope| scope == "openid") {
            scopes.insert(0, "openid".into());
        }
        scopes.sort();
        scopes.dedup();
        Ok(Self {
            http,
            issuer: config.issuer,
            client_id: config.client_id,
            client_secret: config.client_secret,
            scopes,
            metadata,
        })
    }

    pub fn begin(&self, redirect: Url) -> Result<LoginFlow, AuthError> {
        validate_redirect(&redirect)?;
        let verifier = random_secret();
        let state = random_secret();
        let nonce = random_secret();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.expose_secret().as_bytes()));
        let mut authorization_url = Url::parse(&self.metadata.authorization_endpoint)
            .map_err(|_| AuthError::InvalidConfiguration)?;
        authorization_url
            .query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.client_id)
            .append_pair("redirect_uri", redirect.as_str())
            .append_pair("scope", &self.scopes.join(" "))
            .append_pair("state", state.expose_secret())
            .append_pair("nonce", nonce.expose_secret())
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256");
        Ok(LoginFlow {
            client: self.clone(),
            redirect,
            authorization_url,
            verifier,
            state,
            nonce,
            created: Instant::now(),
        })
    }

    pub async fn refresh(
        &self,
        refresh_token: SecretString,
        previous: Option<&TokenSet>,
        cancellation: CancellationToken,
    ) -> Result<TokenSet, AuthError> {
        let identity = previous.map(TokenSet::refresh_identity);
        self.refresh_with_identity(refresh_token, identity.as_ref(), cancellation)
            .await
    }

    pub async fn refresh_with_identity(
        &self,
        refresh_token: SecretString,
        previous: Option<&RefreshIdentity>,
        cancellation: CancellationToken,
    ) -> Result<TokenSet, AuthError> {
        if refresh_token.expose_secret().is_empty()
            || refresh_token.expose_secret().len() > MAX_TOKEN
            || previous.is_some_and(|tokens| {
                tokens.issuer != self.issuer.as_str() || tokens.client_id != self.client_id
            })
        {
            return Err(AuthError::InvalidConfiguration);
        }
        let mut form = vec![
            ("grant_type", "refresh_token"),
            ("client_id", self.client_id.as_str()),
            ("refresh_token", refresh_token.expose_secret()),
        ];
        if let Some(secret) = &self.client_secret {
            form.push(("client_secret", secret.expose_secret()));
        }
        let response = self.exchange(&form, cancellation.clone()).await?;
        let mut result = self
            .verify_response(response, None, previous, cancellation)
            .await?;
        if result.refresh_token.is_none() {
            result.refresh_token = Some(refresh_token);
        }
        Ok(result)
    }

    async fn exchange(
        &self,
        form: &[(&str, &str)],
        cancellation: CancellationToken,
    ) -> Result<TokenResponse, AuthError> {
        // reqwest owns the encoded HTTP body; no URL contains credentials.
        let mut request = self
            .http
            .post(&self.metadata.token_endpoint)
            .header("Accept", "application/json");
        let use_basic = self.client_secret.is_some()
            && !self
                .metadata
                .token_endpoint_auth_methods_supported
                .iter()
                .any(|method| method == "client_secret_post");
        let filtered: Vec<_> = form
            .iter()
            .copied()
            .filter(|(key, _)| !use_basic || *key != "client_secret")
            .collect();
        if use_basic {
            // RFC 6749 requires form escaping both Basic components.
            let escaped = |value: &str| {
                url::form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>()
            };
            request = request.basic_auth(
                escaped(&self.client_id),
                self.client_secret
                    .as_ref()
                    .map(|value| escaped(value.expose_secret())),
            );
        }
        let encoded = Zeroizing::new(
            url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(filtered)
                .finish(),
        );
        let bytes = fetch(
            request
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(encoded.to_string()),
            cancellation,
        )
        .await?;
        serde_json::from_slice(&bytes).map_err(|_| AuthError::InvalidResponse)
    }

    async fn verify_response(
        &self,
        response: TokenResponse,
        login_nonce: Option<SecretString>,
        previous: Option<&RefreshIdentity>,
        cancellation: CancellationToken,
    ) -> Result<TokenSet, AuthError> {
        if !response.token_type.eq_ignore_ascii_case("bearer")
            || response.access_token.expose_secret().is_empty()
            || response.access_token.expose_secret().len() > MAX_TOKEN
            || response.refresh_token.as_ref().is_some_and(|token| {
                token.expose_secret().is_empty() || token.expose_secret().len() > MAX_TOKEN
            })
        {
            return Err(AuthError::InvalidResponse);
        }
        let raw = response.id_token.expose_secret();
        if raw.is_empty() || raw.len() > MAX_TOKEN {
            return Err(AuthError::InvalidToken);
        }
        let bytes = fetch(
            self.http
                .get(&self.metadata.jwks_uri)
                .header("Accept", "application/json"),
            cancellation,
        )
        .await?;
        let jwks: Jwks = serde_json::from_slice(&bytes).map_err(|_| AuthError::InvalidToken)?;
        let claims = verify_jwt(
            raw,
            &jwks,
            &self.metadata.id_token_signing_alg_values_supported,
        )?;
        let nonce = login_nonce
            .as_ref()
            .or_else(|| previous.and_then(|tokens| tokens.nonce.as_ref()));
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| AuthError::InvalidToken)?
            .as_secs();
        let audiences = match &claims.aud {
            Audience::One(value) => std::slice::from_ref(value),
            Audience::Many(values) => values.as_slice(),
        };
        if claims.iss != self.issuer.as_str()
            || claims.sub.expose_secret().is_empty()
            || claims.sub.expose_secret().len() > 255
            || !audiences.contains(&self.client_id)
            || audiences.len() > 64
            || (audiences.len() > 1 && claims.azp.as_deref() != Some(&self.client_id))
            || claims
                .azp
                .as_ref()
                .is_some_and(|azp| azp != &self.client_id)
            || claims.exp <= now
            || claims.iat > now.saturating_add(60)
            || claims.iat >= claims.exp
            || claims.nbf.is_some_and(|nbf| nbf > now.saturating_add(60))
            || previous.is_some_and(|old| old.subject.expose_secret() != claims.sub.expose_secret())
        {
            return Err(AuthError::InvalidToken);
        }
        // Initial authorization requires the nonce. Refresh responses may omit
        // it, but any nonce they include must equal the original.
        if let Some(expected) = nonce {
            match &claims.nonce {
                Some(actual) if actual.expose_secret() == expected.expose_secret() => {}
                None if login_nonce.is_none() => {}
                _ => return Err(AuthError::InvalidToken),
            }
        } else if login_nonce.is_some() {
            return Err(AuthError::InvalidToken);
        }
        if let Some(actual) = claims.at_hash {
            let hash = Sha256::digest(response.access_token.expose_secret().as_bytes());
            if actual != URL_SAFE_NO_PAD.encode(&hash[..16]) {
                return Err(AuthError::InvalidToken);
            }
        }
        let expiry = response
            .expires_in
            .map(|seconds| now.saturating_add(seconds).min(claims.exp))
            .unwrap_or(claims.exp)
            .min(now.saturating_add(24 * 60 * 60));
        if expiry <= now {
            return Err(AuthError::InvalidToken);
        }
        Ok(TokenSet {
            id_token: response.id_token,
            access_token: response.access_token,
            refresh_token: response.refresh_token,
            expires_at: UNIX_EPOCH
                .checked_add(Duration::from_secs(expiry))
                .ok_or(AuthError::InvalidToken)?,
            issuer: self.issuer.to_string(),
            client_id: self.client_id.clone(),
            subject: claims.sub,
            nonce: nonce.cloned(),
        })
    }
}

fn random_secret() -> SecretString {
    // Uuid::new_v4 uses the operating system CSPRNG. Two UUIDs provide 244
    // random bits and a valid 64-character PKCE verifier.
    SecretString::from(format!(
        "{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    ))
}

fn validate_https(url: &Url) -> Result<(), AuthError> {
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AuthError::InvalidConfiguration);
    }
    Ok(())
}

fn validate_redirect(url: &Url) -> Result<(), AuthError> {
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AuthError::InvalidConfiguration);
    }
    let loopback = url.scheme() == "http"
        && url.port().is_some_and(|port| port != 0)
        && url
            .host_str()
            .and_then(|host| {
                host.trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .ok()
            })
            .is_some_and(|address| address.is_loopback());
    let mobile = url.as_str() == "consolecrypt://kubernetes/oauth/callback";
    // Claimed HTTPS links are allowed only when the platform runtime owns the
    // corresponding app/universal-link association.
    if loopback || mobile || (url.scheme() == "https" && url.host_str().is_some()) {
        Ok(())
    } else {
        Err(AuthError::InvalidConfiguration)
    }
}

fn callback_code(callback: &Url, redirect: &Url, state: &str) -> Result<SecretString, AuthError> {
    if callback.as_str().len() > 32 * 1024 {
        return Err(AuthError::InvalidCallback);
    }
    let mut base = callback.clone();
    base.set_query(None);
    if &base != redirect || callback.fragment().is_some() {
        return Err(AuthError::InvalidCallback);
    }
    let mut code = None;
    let mut received_state = None;
    let mut error = false;
    let mut seen = std::collections::BTreeSet::new();
    for (key, value) in callback.query_pairs() {
        if !seen.insert(key.to_string()) {
            return Err(AuthError::InvalidCallback);
        }
        match key.as_ref() {
            "code" if !value.is_empty() && value.len() <= 8192 => {
                code = Some(SecretString::from(value.into_owned()))
            }
            "state" => received_state = Some(SecretString::from(value.into_owned())),
            "error" => error = true,
            "code" => return Err(AuthError::InvalidCallback),
            _ => {}
        }
    }
    if received_state.as_ref().map(ExposeSecret::expose_secret) != Some(state) {
        return Err(AuthError::InvalidCallback);
    }
    if error {
        return Err(AuthError::Rejected);
    }
    code.ok_or(AuthError::InvalidCallback)
}

async fn fetch(
    request: reqwest::RequestBuilder,
    cancellation: CancellationToken,
) -> Result<Zeroizing<Vec<u8>>, AuthError> {
    let work = async {
        let response = request.send().await.map_err(|_| AuthError::Transport)?;
        if !response.status().is_success() {
            return Err(AuthError::Rejected);
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_RESPONSE as u64)
        {
            return Err(AuthError::ResponseTooLarge);
        }
        let mut stream = response.bytes_stream();
        let mut result = Zeroizing::new(Vec::new());
        while let Some(part) = stream.next().await {
            let part = part.map_err(|_| AuthError::Transport)?;
            if result.len() + part.len() > MAX_RESPONSE {
                return Err(AuthError::ResponseTooLarge);
            }
            result.extend_from_slice(&part);
        }
        Ok(result)
    };
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(AuthError::Cancelled),
        value = work => value,
    }
}

fn secret<'de, D>(deserializer: D) -> Result<SecretString, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(deserializer).map(SecretString::from)
}

fn optional_secret<'de, D>(deserializer: D) -> Result<Option<SecretString>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(|value| value.map(SecretString::from))
}

#[derive(Deserialize)]
struct TokenResponse {
    #[serde(deserialize_with = "secret")]
    id_token: SecretString,
    #[serde(deserialize_with = "secret")]
    access_token: SecretString,
    #[serde(default, deserialize_with = "optional_secret")]
    refresh_token: Option<SecretString>,
    token_type: String,
    expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}

#[derive(Debug, Deserialize)]
struct Jwk {
    kty: String,
    kid: Option<String>,
    alg: Option<String>,
    #[serde(rename = "use")]
    usage: Option<String>,
    key_ops: Option<Vec<String>>,
    n: Option<String>,
    e: Option<String>,
    crv: Option<String>,
    x: Option<String>,
    y: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JwtHeader {
    alg: String,
    kid: Option<String>,
    typ: Option<String>,
    crit: Option<Vec<String>>,
    b64: Option<bool>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
struct Claims {
    iss: String,
    #[serde(deserialize_with = "secret")]
    sub: SecretString,
    aud: Audience,
    exp: u64,
    iat: u64,
    nbf: Option<u64>,
    azp: Option<String>,
    #[serde(default, deserialize_with = "optional_secret")]
    nonce: Option<SecretString>,
    at_hash: Option<String>,
}

fn verify_jwt(raw: &str, jwks: &Jwks, allowed_algs: &[String]) -> Result<Claims, AuthError> {
    let parts: Vec<_> = raw.split('.').collect();
    if parts.len() != 3
        || parts.iter().any(|part| part.is_empty())
        || parts[0].len() > 8192
        || jwks.keys.is_empty()
        || jwks.keys.len() > 128
    {
        return Err(AuthError::InvalidToken);
    }
    let decode = |value: &str| {
        URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|_| AuthError::InvalidToken)
    };
    let header: JwtHeader =
        serde_json::from_slice(&decode(parts[0])?).map_err(|_| AuthError::InvalidToken)?;
    if !matches!(header.alg.as_str(), "RS256" | "ES256")
        || !allowed_algs.contains(&header.alg)
        || header
            .typ
            .as_ref()
            .is_some_and(|typ| !typ.eq_ignore_ascii_case("JWT"))
        || header.crit.is_some_and(|crit| !crit.is_empty())
        || header.b64 == Some(false)
    {
        return Err(AuthError::InvalidToken);
    }
    let keys: Vec<_> = jwks
        .keys
        .iter()
        .filter(|key| {
            (header.kid.is_none() || key.kid == header.kid)
                && key.alg.as_ref().is_none_or(|alg| alg == &header.alg)
                && key.usage.as_ref().is_none_or(|usage| usage == "sig")
                && key
                    .key_ops
                    .as_ref()
                    .is_none_or(|ops| ops.iter().any(|op| op == "verify"))
                && matches!(
                    (header.alg.as_str(), key.kty.as_str()),
                    ("RS256", "RSA") | ("ES256", "EC")
                )
        })
        .collect();
    if keys.len() != 1 {
        return Err(AuthError::InvalidToken);
    }
    let key = keys[0];
    let input_length = parts[0].len() + 1 + parts[1].len();
    let message = &raw.as_bytes()[..input_length];
    let signature = decode(parts[2])?;
    match header.alg.as_str() {
        "RS256" => {
            let modulus = decode(key.n.as_deref().ok_or(AuthError::InvalidToken)?)?;
            let exponent = decode(key.e.as_deref().ok_or(AuthError::InvalidToken)?)?;
            signature::RsaPublicKeyComponents {
                n: &modulus,
                e: &exponent,
            }
            .verify(&signature::RSA_PKCS1_2048_8192_SHA256, message, &signature)
            .map_err(|_| AuthError::InvalidToken)?;
        }
        "ES256" => {
            if key.crv.as_deref() != Some("P-256") {
                return Err(AuthError::InvalidToken);
            }
            let x = decode(key.x.as_deref().ok_or(AuthError::InvalidToken)?)?;
            let y = decode(key.y.as_deref().ok_or(AuthError::InvalidToken)?)?;
            if x.len() != 32 || y.len() != 32 {
                return Err(AuthError::InvalidToken);
            }
            let mut public_key = Vec::with_capacity(65);
            public_key.push(4);
            public_key.extend_from_slice(&x);
            public_key.extend_from_slice(&y);
            signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, public_key)
                .verify(message, &signature)
                .map_err(|_| AuthError::InvalidToken)?;
        }
        _ => return Err(AuthError::InvalidToken),
    }
    let payload = Zeroizing::new(decode(parts[1])?);
    serde_json::from_slice(&payload).map_err(|_| AuthError::InvalidToken)
}

#[cfg(test)]
#[path = "oidc_tests.rs"]
mod tests;
