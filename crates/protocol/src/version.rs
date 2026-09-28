//! Protocol versioning and client identification headers.
//!
//! Every request from a client carries:
//!
//! * `x-cc-protocol-version: <major>.<minor>` — [`PROTOCOL_VERSION`] of the client build
//! * `x-cc-client-version: <semver of the app>`
//! * `x-cc-platform: macos|windows|linux|ios|android|cli`
//!
//! If the client's protocol major differs from the server's, or the client is
//! older than the server's minimum, the server answers
//! `426 Upgrade Required` with [`crate::ErrorCode::UpgradeRequired`] and a
//! [`crate::meta::ServerInfo`] in `details`.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Protocol version implemented by this build of the crate.
///
/// * major — incompatible wire change (new `/vN` API prefix);
/// * minor — additive, backwards compatible change.
pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 1, minor: 5 };

/// URL prefix for the current major version.
pub const API_PREFIX: &str = "/v1";

pub const HEADER_PROTOCOL_VERSION: &str = "x-cc-protocol-version";
pub const HEADER_CLIENT_VERSION: &str = "x-cc-client-version";
pub const HEADER_PLATFORM: &str = "x-cc-platform";
pub const HEADER_REQUEST_ID: &str = "x-request-id";
/// Per-request device proof (protocol 1.5): `<issued_at>.<nonce>.<signature>`,
/// see [`crate::devices::RequestProof`] and
/// [`crate::canonical::request_proof_message`].
pub const HEADER_DEVICE_PROOF: &str = "x-cc-device-proof";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}

impl ProtocolVersion {
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    /// A peer speaking `other` can talk to us if majors match and `other` is
    /// not below our `minimum`.
    pub fn is_compatible(&self, other: &ProtocolVersion, minimum: &ProtocolVersion) -> bool {
        other.major == self.major && other >= minimum
    }
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid protocol version string")]
pub struct ParseVersionError;

impl FromStr for ProtocolVersion {
    type Err = ParseVersionError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (maj, min) = s.trim().split_once('.').ok_or(ParseVersionError)?;
        Ok(Self {
            major: maj.parse().map_err(|_| ParseVersionError)?,
            minor: min.parse().map_err(|_| ParseVersionError)?,
        })
    }
}

impl Serialize for ProtocolVersion {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ProtocolVersion {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = <std::borrow::Cow<'de, str>>::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Client platform reported in `x-cc-platform` and in device registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Macos,
    Windows,
    Linux,
    Ios,
    Android,
    /// Headless tooling (tests, `cc-cli`).
    Cli,
}

impl Platform {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Platform::Macos => "macos",
            Platform::Windows => "windows",
            Platform::Linux => "linux",
            Platform::Ios => "ios",
            Platform::Android => "android",
            Platform::Cli => "cli",
        }
    }
}

impl FromStr for Platform {
    type Err = ParseVersionError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "macos" => Platform::Macos,
            "windows" => Platform::Windows,
            "linux" => Platform::Linux,
            "ios" => Platform::Ios,
            "android" => Platform::Android,
            "cli" => Platform::Cli,
            _ => return Err(ParseVersionError),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_display() {
        let v: ProtocolVersion = "1.4".parse().unwrap();
        assert_eq!(v, ProtocolVersion::new(1, 4));
        assert_eq!(v.to_string(), "1.4");
        assert!("1".parse::<ProtocolVersion>().is_err());
        assert_eq!(serde_json::to_string(&v).unwrap(), "\"1.4\"");
    }

    #[test]
    fn compatibility() {
        let server = ProtocolVersion::new(1, 3);
        let min = ProtocolVersion::new(1, 1);
        assert!(server.is_compatible(&ProtocolVersion::new(1, 1), &min));
        assert!(server.is_compatible(&ProtocolVersion::new(1, 9), &min));
        assert!(!server.is_compatible(&ProtocolVersion::new(1, 0), &min));
        assert!(!server.is_compatible(&ProtocolVersion::new(2, 0), &min));
    }
}
