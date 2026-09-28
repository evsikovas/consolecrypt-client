//! Credentials: *how* to authenticate. Raw material is referenced by
//! `secret_id` and stored in a separate [`crate::secret::Secret`] object.

use crate::{ObjectId, Timestamp};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    Password,
    SshPrivateKey,
    /// Private key + OpenSSH certificate.
    SshCertificate,
    /// Keys from the OS SSH agent (ssh-agent / Windows OpenSSH agent / Pageant).
    OsSshAgent,
    /// FIDO2 security key (`sk-ssh-ed25519@openssh.com`). Post-MVP.
    Fido2,
    /// Third-party agent socket (1Password, Secretive, …).
    ExternalAgent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyAlgorithm {
    Ed25519,
    Rsa2048,
    Rsa3072,
    Rsa4096,
    EcdsaP256,
    EcdsaP384,
    EcdsaP521,
    SkEd25519,
    SkEcdsaP256,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Credential {
    pub id: ObjectId,
    pub name: String,
    pub kind: CredentialKind,
    /// Optional username override. Effective username precedence (resolved by
    /// the connection planner): `Host.username` > `Credential.username` >
    /// nearest `Group.inherited_username` up the parent chain > app default
    /// (OS user).
    #[serde(default)]
    pub username: Option<String>,
    /// `Secret` holding the password or the OpenSSH private key.
    #[serde(default)]
    pub secret_id: Option<ObjectId>,
    /// `Secret` holding the key passphrase, only if the user opted in to
    /// "Remember SSH key passphrase".
    #[serde(default)]
    pub passphrase_secret_id: Option<ObjectId>,
    /// Whether the stored private key is itself passphrase-protected.
    #[serde(default)]
    pub key_encrypted: bool,
    #[serde(default)]
    pub key_algorithm: Option<KeyAlgorithm>,
    /// OpenSSH public key line (`ssh-ed25519 AAAA… comment`). Not secret.
    #[serde(default)]
    pub public_key: Option<String>,
    /// OpenSSH certificate line. Not secret.
    #[serde(default)]
    pub certificate: Option<String>,
    /// `SHA256:…` fingerprint for display.
    #[serde(default)]
    pub fingerprint: Option<String>,
    /// For `ExternalAgent`: socket path / pipe name.
    #[serde(default)]
    pub agent_path: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Credential {
    pub fn new(name: impl Into<String>, kind: CredentialKind) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: ObjectId::new(),
            name: name.into(),
            kind,
            username: None,
            secret_id: None,
            passphrase_secret_id: None,
            key_encrypted: false,
            key_algorithm: None,
            public_key: None,
            certificate: None,
            fingerprint: None,
            agent_path: None,
            created_at: now,
            updated_at: now,
        }
    }
}
