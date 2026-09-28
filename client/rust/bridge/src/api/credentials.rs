//! Credentials (metadata only; secrets stay in app-core) and private-key
//! inspection before import. JSON results: `CredentialDto`.

use crate::api::error::BridgeError;
use crate::state::{opt_secret_string, secret_string, to_json, with_core};
use cc_app_core::KeyGenAlgorithm;
use zeroize::Zeroize;

/// Algorithms offered by "Generate key" (CLIENT_SPEC §7.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyGenChoice {
    Ed25519,
    Rsa3072,
    Rsa4096,
}

/// Non-secret facts about a pasted private key (nothing is imported).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyInspectionResult {
    pub valid: bool,
    /// cc-models `KeyAlgorithm` wire name (`ed25519`, `rsa3072`, …).
    pub algorithm: Option<String>,
    pub encrypted: bool,
    pub fingerprint: Option<String>,
    pub public_key: Option<String>,
    /// Why the key is not valid (never contains key material).
    pub error: Option<String>,
}

/// `Vec<CredentialDto>`.
pub async fn credentials_list() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.list_credentials().await?) }).await
}

pub async fn credentials_add_password(
    name: String,
    username: Option<String>,
    password: Vec<u8>,
) -> Result<String, BridgeError> {
    let password = secret_string("password", password)?;
    with_core(move |c| async move {
        to_json(&c.add_password_credential(name, username, password).await?)
    })
    .await
}

pub async fn credentials_generate_key(
    name: String,
    username: Option<String>,
    algorithm: KeyGenChoice,
    passphrase: Option<Vec<u8>>,
    remember_passphrase: bool,
) -> Result<String, BridgeError> {
    let passphrase = opt_secret_string("passphrase", passphrase)?;
    let algorithm = match algorithm {
        KeyGenChoice::Ed25519 => KeyGenAlgorithm::Ed25519,
        KeyGenChoice::Rsa3072 => KeyGenAlgorithm::Rsa3072,
        KeyGenChoice::Rsa4096 => KeyGenAlgorithm::Rsa4096,
    };
    with_core(move |c| async move {
        to_json(
            &c.generate_ssh_key(name, username, algorithm, passphrase, remember_passphrase)
                .await?,
        )
    })
    .await
}

pub async fn credentials_import_key(
    name: String,
    username: Option<String>,
    private_key: Vec<u8>,
    passphrase: Option<Vec<u8>>,
    remember_passphrase: bool,
    certificate: Option<String>,
) -> Result<String, BridgeError> {
    let private_key = secret_string("private_key", private_key)?;
    let passphrase = opt_secret_string("passphrase", passphrase)?;
    with_core(move |c| async move {
        to_json(
            &c.import_ssh_key(
                name,
                username,
                private_key,
                passphrase,
                remember_passphrase,
                certificate,
            )
            .await?,
        )
    })
    .await
}

/// OS agent (`agent_path = None`) or an external agent socket / pipe.
pub async fn credentials_add_agent(
    name: String,
    username: Option<String>,
    agent_path: Option<String>,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(&c.add_agent_credential(name, username, agent_path).await?)
    })
    .await
}

/// Rename / username / certificate (`Some("")` removes the certificate).
pub async fn credentials_update(
    id: String,
    name: String,
    username: Option<String>,
    certificate: Option<String>,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(&c.update_credential(id, name, username, certificate).await?)
    })
    .await
}

pub async fn credentials_set_password(
    id: String,
    password: Vec<u8>,
) -> Result<String, BridgeError> {
    let password = secret_string("password", password)?;
    with_core(move |c| async move { to_json(&c.set_credential_password(id, password).await?) })
        .await
}

pub async fn credentials_remember_passphrase(
    id: String,
    passphrase: Vec<u8>,
) -> Result<String, BridgeError> {
    let passphrase = secret_string("passphrase", passphrase)?;
    with_core(move |c| async move { to_json(&c.remember_key_passphrase(id, passphrase).await?) })
        .await
}

pub async fn credentials_forget_passphrase(id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.forget_key_passphrase(id).await?) }).await
}

/// Refused (`in_use`) while hosts/groups use it; removes its Secrets.
pub async fn credentials_delete(id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.delete_credential(id).await?) }).await
}

/// OpenSSH public key line of a key credential (not secret).
pub async fn credentials_public_key(id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { Ok(c.credential_public_key(id).await?) }).await
}

/// Parse a pasted OpenSSH / PEM / PuTTY private key without importing it
/// or removing its protection. The input buffer is zeroized afterwards.
pub fn credentials_inspect_key(private_key: Vec<u8>) -> KeyInspectionResult {
    let mut private_key = private_key;
    let result = match std::str::from_utf8(&private_key) {
        Err(_) => invalid("not a text key file"),
        Ok(text) => match cc_ssh_core::keys::inspect_private_key(text, None) {
            Ok(info) => KeyInspectionResult {
                valid: true,
                algorithm: info
                    .algorithm
                    .and_then(|a| serde_json::to_value(a).ok())
                    .and_then(|v| v.as_str().map(str::to_owned)),
                encrypted: info.encrypted,
                fingerprint: Some(info.fingerprint_sha256).filter(|s| !s.is_empty()),
                public_key: Some(info.public_openssh).filter(|s| !s.is_empty()),
                error: None,
            },
            Err(e) => invalid(&e.to_string()),
        },
    };
    private_key.zeroize();
    result
}

fn invalid(error: &str) -> KeyInspectionResult {
    KeyInspectionResult {
        valid: false,
        algorithm: None,
        encrypted: false,
        fingerprint: None,
        public_key: None,
        error: Some(error.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    #[test]
    fn inspect_rejects_garbage_without_echoing_it() {
        let r = credentials_inspect_key(b"-----BEGIN NONSENSE-----\nc2VjcmV0\n".to_vec());
        assert!(!r.valid);
        assert!(!r.error.unwrap_or_default().contains("c2VjcmV0"));
        let r = credentials_inspect_key(vec![0xff, 0x00]);
        assert!(!r.valid);
    }

    #[test]
    fn inspect_reads_a_generated_key() {
        let key =
            cc_ssh_core::keys::generate_key(KeyGenAlgorithm::Ed25519, "test@bridge", None).unwrap();
        let text = key.private_openssh.expose_secret().to_owned();
        let r = credentials_inspect_key(text.into_bytes());
        assert!(r.valid, "{:?}", r.error);
        assert_eq!(r.algorithm.as_deref(), Some("ed25519"));
        assert!(!r.encrypted);
        assert!(r.fingerprint.unwrap().starts_with("SHA256:"));
        assert!(r.public_key.unwrap().starts_with("ssh-ed25519 "));
    }
}
