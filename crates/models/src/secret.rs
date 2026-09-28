//! Raw secret material. The only place in the model where passwords, private
//! keys, passphrases and API keys live.

use crate::{ObjectId, Timestamp};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// A secret string: redacted `Debug`/`Display`, zeroized on drop.
///
/// Serialization is intentionally allowed (it is how the secret gets into the
/// encrypted payload); callers must only serialize into buffers that are
/// themselves zeroized and encrypted.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct SecretValue(String);

impl SecretValue {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Explicit, grep-able access to the secret.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretValue(<redacted>)")
    }
}

impl Serialize for SecretValue {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SecretValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d).map(SecretValue)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    Password,
    /// OpenSSH-format private key (PEM text), possibly passphrase-protected —
    /// the original protection is never removed automatically.
    SshPrivateKey,
    /// Passphrase of an encrypted SSH key ("Remember SSH key passphrase").
    SshKeyPassphrase,
    ApiKey,
    Token,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Secret {
    pub id: ObjectId,
    pub kind: SecretKind,
    pub value: SecretValue,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Secret {
    pub fn new(kind: SecretKind, value: SecretValue) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: ObjectId::new(),
            kind,
            value,
            created_at: now,
            updated_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_is_redacted() {
        let s = Secret::new(SecretKind::Password, SecretValue::new("correct horse"));
        let dbg = format!("{s:?}");
        assert!(!dbg.contains("correct horse"), "{dbg}");
    }
}
