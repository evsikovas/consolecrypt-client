//! Protocol 1.5 sender-constrained tokens (THREAT_MODEL gap 4): every
//! authenticated request, `POST /v1/auth/refresh` and the WebSocket upgrade
//! carry `x-cc-device-proof`, an Ed25519 signature by the device key over
//! `canonical::request_proof_message(device_id, method, path_and_query,
//! sha256(body), issued_at, nonce)`. A stolen bearer or refresh token is then
//! useless without the device key.
//!
//! sync-core never holds keys: the app injects a [`RequestSigner`] (app-core
//! wraps `cc_vault_core::DeviceIdentity::request_proof`).

use cc_protocol::devices::RequestProof;
use url::Url;

/// Signs requests with this installation's device key.
///
/// Called on the async runtime right before each request is sent (and again
/// for every retry): must be fast, must not block, and must use a fresh
/// random nonce and the current time on every call.
pub trait RequestSigner: Send + Sync {
    /// Proof for one request. `method` is the HTTP method (`"GET"`, …),
    /// `path_and_query` the request target exactly as sent on the wire
    /// (`/v1/sync/changes?vault_id=…&after=0`), `body` the exact body bytes
    /// (empty when there is none).
    fn request_proof(
        &self,
        method: &str,
        path_and_query: &str,
        body: &[u8],
    ) -> Result<RequestProof, SignerError>;
}

/// A [`RequestSigner`] could not produce a proof. Messages never contain key
/// material.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SignerError {
    /// The device key is not available (e.g. secure store locked).
    #[error("device signing key unavailable")]
    Unavailable,
    /// Anything else (RNG failure, …).
    #[error("request signing failed: {0}")]
    Failed(String),
}

/// Why the server refused a request proof (`422 invalid_proof`,
/// `details.reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProofRejection {
    /// No proof was sent to a server that requires one (this client has no
    /// signer configured).
    Missing,
    /// The header could not be parsed.
    Malformed,
    /// `issued_at` outside the allowed skew even after a re-signed retry:
    /// the device clock is off.
    Stale,
    /// The nonce was already used.
    Replayed,
    /// The signature does not verify against the session's device key
    /// (wrong key, or the request was altered in transit).
    InvalidSignature,
    /// A reason this client does not know.
    Unknown,
}

impl ProofRejection {
    /// Wire value of `details.reason`.
    pub fn as_str(self) -> &'static str {
        match self {
            ProofRejection::Missing => "missing",
            ProofRejection::Malformed => "malformed",
            ProofRejection::Stale => "stale",
            ProofRejection::Replayed => "replayed",
            ProofRejection::InvalidSignature => "invalid_signature",
            ProofRejection::Unknown => "unknown",
        }
    }

    pub(crate) fn parse(reason: Option<&str>) -> Self {
        match reason {
            Some("missing") => ProofRejection::Missing,
            Some("malformed") => ProofRejection::Malformed,
            Some("stale") => ProofRejection::Stale,
            Some("replayed") => ProofRejection::Replayed,
            Some("invalid_signature") => ProofRejection::InvalidSignature,
            _ => ProofRejection::Unknown,
        }
    }
}

impl std::fmt::Display for ProofRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ProofRejection::Missing => "missing (this client does not sign requests)",
            ProofRejection::Malformed => "malformed",
            ProofRejection::Stale => {
                "stale (the device clock differs from the server's by more than 2 minutes)"
            }
            ProofRejection::Replayed => "replayed",
            ProofRejection::InvalidSignature => "invalid signature",
            ProofRejection::Unknown => "unknown reason",
        })
    }
}

/// The request target as it goes on the wire: path plus `?query` (already
/// percent-encoded by `url`, exactly what reqwest / tungstenite send).
pub(crate) fn request_target(url: &Url) -> String {
    match url.query() {
        Some(q) => format!("{}?{q}", url.path()),
        None => url.path().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_target_keeps_the_encoded_query() {
        let mut u = Url::parse("https://h.example/prefix/v1/sync/changes").unwrap();
        u.query_pairs_mut()
            .append_pair("vault_id", "a b")
            .append_pair("after", "0");
        assert_eq!(
            request_target(&u),
            "/prefix/v1/sync/changes?vault_id=a+b&after=0"
        );
        assert_eq!(
            request_target(&Url::parse("wss://h.example/v1/events/ws").unwrap()),
            "/v1/events/ws"
        );
        for r in [
            "missing",
            "malformed",
            "stale",
            "replayed",
            "invalid_signature",
        ] {
            assert_eq!(ProofRejection::parse(Some(r)).as_str(), r);
        }
        assert_eq!(ProofRejection::parse(None), ProofRejection::Unknown);
    }
}
