//! Opaque binary payloads (ciphertexts, nonces, public keys, signatures).
//!
//! Serialized as standard base64 with padding (RFC 4648 §4). The `Debug`
//! implementation never prints content — only the length — so ciphertexts and
//! envelopes cannot leak into logs through `{:?}`.

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

#[derive(Clone, PartialEq, Eq, Default, Hash)]
pub struct Bytes(pub Vec<u8>);

impl Bytes {
    pub fn new(v: impl Into<Vec<u8>>) -> Self {
        Self(v.into())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }

    pub fn to_base64(&self) -> String {
        STANDARD.encode(&self.0)
    }

    pub fn from_base64(s: &str) -> Result<Self, base64::DecodeError> {
        STANDARD.decode(s).map(Self)
    }

    /// Returns the bytes as a fixed-size array if the length matches.
    pub fn to_array<const N: usize>(&self) -> Option<[u8; N]> {
        self.0.as_slice().try_into().ok()
    }
}

impl fmt::Debug for Bytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bytes(<{} bytes>)", self.0.len())
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(v: Vec<u8>) -> Self {
        Self(v)
    }
}

impl From<&[u8]> for Bytes {
    fn from(v: &[u8]) -> Self {
        Self(v.to_vec())
    }
}

impl<const N: usize> From<[u8; N]> for Bytes {
    fn from(v: [u8; N]) -> Self {
        Self(v.to_vec())
    }
}

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Serialize for Bytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_base64())
    }
}

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        Bytes::from_base64(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_roundtrip() {
        let b = Bytes::new(vec![0u8, 1, 2, 250, 255]);
        let json = serde_json::to_string(&b).unwrap();
        assert_eq!(json, "\"AAEC+v8=\"");
        let back: Bytes = serde_json::from_str(&json).unwrap();
        assert_eq!(back, b);
    }

    #[test]
    fn debug_does_not_print_content() {
        let b = Bytes::new(b"super secret ciphertext".to_vec());
        let dbg = format!("{b:?}");
        assert_eq!(dbg, "Bytes(<23 bytes>)");
        assert!(!dbg.contains("secret"));
    }

    #[test]
    fn rejects_invalid_base64() {
        assert!(serde_json::from_str::<Bytes>("\"not base64!!\"").is_err());
    }
}
