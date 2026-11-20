use crate::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use std::{
    fmt,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

/// Rust-only material boundary for AppCore after it authorizes a saved source.
/// Never expose through FRB/Serde/JSON. Contains no resolver or lookup authority.
/// AppCore must enforce source revision, unlocked lease and request cancellation.
/// This grant is neither Clone nor serializable and binds the full source config.
pub struct SourceAuthorization {
    pub(crate) source: SourceConfig,
    pub(crate) expires: Instant,
    pub(crate) authorization: Zeroizing<String>,
    pub(crate) redact: Vec<Zeroizing<String>>,
}
impl fmt::Debug for SourceAuthorization {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SourceAuthorization([REDACTED])")
    }
}
impl SourceAuthorization {
    pub fn bearer(
        source: &SourceConfig,
        token: Zeroizing<String>,
        ttl: Duration,
    ) -> Result<Self, Error> {
        source.validate()?;
        if !matches!(source.auth, AuthRef::BearerRef { .. })
            || token.is_empty()
            || token.len() > 16 * 1024
            || !token.bytes().all(|b| {
                b.is_ascii_alphanumeric()
                    || matches!(b, b'-' | b'.' | b'_' | b'~' | b'+' | b'/' | b'=')
            })
        {
            return Err(invalid_grant());
        }
        let expires = deadline(ttl)?;
        let authorization = Zeroizing::new(format!("Bearer {}", token.as_str()));
        Ok(Self {
            source: source.clone(),
            expires,
            authorization,
            redact: vec![token],
        })
    }
    /// Uses UTF-8 Basic credentials. Username cannot contain ':'; neither input
    /// may contain control characters. Password/header copies stay zeroizing.
    pub fn basic(
        source: &SourceConfig,
        username: Zeroizing<String>,
        password: Zeroizing<String>,
        ttl: Duration,
    ) -> Result<Self, Error> {
        source.validate()?;
        if !matches!(source.auth, AuthRef::BasicRef { .. })
            || username.is_empty()
            || username.len() > 1024
            || username.contains(':')
            || username.chars().any(char::is_control)
            || password.is_empty()
            || password.len() > 16 * 1024
            || password.chars().any(char::is_control)
        {
            return Err(invalid_grant());
        }
        let expires = deadline(ttl)?;
        let raw = Zeroizing::new(format!("{}:{}", username.as_str(), password.as_str()));
        let encoded = Zeroizing::new(STANDARD.encode(raw.as_bytes()));
        let authorization = Zeroizing::new(format!("Basic {}", encoded.as_str()));
        Ok(Self {
            source: source.clone(),
            expires,
            authorization,
            redact: vec![password, raw, encoded],
        })
    }
}
fn deadline(ttl: Duration) -> Result<Instant, Error> {
    if ttl.is_zero() || ttl > Duration::from_secs(30) {
        return Err(invalid_grant());
    }
    Ok(Instant::now() + ttl)
}
fn invalid_grant() -> Error {
    Error::new(
        ErrorCode::AuthUnavailable,
        "Invalid source authorization material or lifetime",
    )
}
