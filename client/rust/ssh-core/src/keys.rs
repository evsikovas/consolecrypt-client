//! SSH key generation, import and inspection (CLIENT_SPEC §7).
//!
//! * Generation: Ed25519 (default), RSA 3072/4096.
//! * Import: OpenSSH private keys (encrypted or not), legacy PEM / PKCS#8,
//!   public keys, OpenSSH certificates.
//! * Private keys are decrypted **in memory only**; the original
//!   passphrase protection of a stored key is never removed.

use base64::engine::general_purpose::STANDARD_NO_PAD as B64_NO_PAD;
use base64::Engine as _;
use cc_models::credential::{Credential, KeyAlgorithm};
use russh::keys::ssh_encoding::Encode;
use russh::keys::ssh_key::certificate::CertType;
use russh::keys::ssh_key::private::KeypairData;
use russh::keys::ssh_key::public::KeyData;
use russh::keys::ssh_key::{
    self, Algorithm, Certificate, HashAlg, LineEnding, PrivateKey, PublicKey,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Key handling errors. Never contain key material.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum KeyError {
    #[error("unrecognized or malformed key: {0}")]
    InvalidFormat(String),
    #[error("the private key is passphrase-protected; a passphrase is required")]
    PassphraseRequired,
    #[error("wrong passphrase")]
    WrongPassphrase,
    #[error("unsupported key algorithm: {0}")]
    Unsupported(String),
    #[error("key generation failed: {0}")]
    Generation(String),
    #[error("certificate does not belong to this key")]
    CertificateMismatch,
}

/// Format of a private key text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrivateKeyFormat {
    /// `-----BEGIN OPENSSH PRIVATE KEY-----`
    OpenSsh,
    /// Legacy `BEGIN RSA/EC PRIVATE KEY` (PKCS#1 / SEC1) or PKCS#8.
    LegacyPem,
    /// PuTTY `.ppk`.
    Putty,
}

/// Algorithms offered by key generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum KeyGenAlgorithm {
    #[default]
    Ed25519,
    Rsa3072,
    Rsa4096,
}

impl KeyGenAlgorithm {
    pub fn model(self) -> KeyAlgorithm {
        match self {
            KeyGenAlgorithm::Ed25519 => KeyAlgorithm::Ed25519,
            KeyGenAlgorithm::Rsa3072 => KeyAlgorithm::Rsa3072,
            KeyGenAlgorithm::Rsa4096 => KeyAlgorithm::Rsa4096,
        }
    }
}

/// A freshly generated key pair.
pub struct GeneratedKey {
    /// OpenSSH private key text; encrypted when a passphrase was given.
    pub private_openssh: SecretString,
    /// `ssh-ed25519 AAAA… comment`
    pub public_openssh: String,
    pub fingerprint_sha256: String,
    pub algorithm: KeyAlgorithm,
    pub encrypted: bool,
}

impl std::fmt::Debug for GeneratedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeneratedKey")
            .field("private_openssh", &"<redacted>")
            .field("public_openssh", &self.public_openssh)
            .field("fingerprint_sha256", &self.fingerprint_sha256)
            .field("algorithm", &self.algorithm)
            .field("encrypted", &self.encrypted)
            .finish()
    }
}

/// Non-secret facts about a private key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivateKeyInfo {
    pub format: PrivateKeyFormat,
    /// Model algorithm, if representable (`None` e.g. for RSA-1024).
    pub algorithm: Option<KeyAlgorithm>,
    /// SSH algorithm name, e.g. `ssh-ed25519`.
    pub algorithm_name: String,
    pub public_openssh: String,
    pub fingerprint_sha256: String,
    pub encrypted: bool,
    pub comment: String,
}

/// Facts about a public key line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicKeyInfo {
    pub algorithm: Option<KeyAlgorithm>,
    pub algorithm_name: String,
    pub fingerprint_sha256: String,
    pub comment: String,
    /// Normalized `type base64 comment` line.
    pub openssh: String,
}

/// Certificate kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CertificateKind {
    User,
    Host,
}

/// Facts about an OpenSSH certificate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CertificateInfo {
    pub kind: CertificateKind,
    pub key_id: String,
    pub serial: u64,
    pub principals: Vec<String>,
    /// Unix seconds.
    pub valid_after: u64,
    /// Unix seconds (`u64::MAX` = forever).
    pub valid_before: u64,
    /// Fingerprint of the certified public key.
    pub public_key_fingerprint: String,
    /// Fingerprint of the signing CA key.
    pub ca_fingerprint: String,
    pub algorithm_name: String,
    pub critical_options: Vec<String>,
    pub extensions: Vec<String>,
}

impl CertificateInfo {
    /// Is `now` (unix seconds) inside the validity window?
    pub fn is_valid_at(&self, now: u64) -> bool {
        self.valid_after <= now && now < self.valid_before
    }
}

/// `SHA256:<base64 no padding>` fingerprint of a public key blob (OpenSSH format).
pub fn fingerprint_sha256(key_blob: &[u8]) -> String {
    let digest = Sha256::digest(key_blob);
    format!("SHA256:{}", B64_NO_PAD.encode(digest))
}

/// Wire encoding of a public key (the known_hosts / authorized_keys blob).
pub fn key_blob(key: &KeyData) -> Vec<u8> {
    key.encode_vec().unwrap_or_default()
}

/// Map an SSH algorithm (+ key data for RSA sizes) to the model enum.
pub fn model_algorithm(key: &KeyData) -> Option<KeyAlgorithm> {
    use russh::keys::ssh_key::EcdsaCurve;
    match key.algorithm() {
        Algorithm::Ed25519 => Some(KeyAlgorithm::Ed25519),
        Algorithm::Rsa { .. } => match key.rsa().map(|k| k.key_size()) {
            Some(2048) => Some(KeyAlgorithm::Rsa2048),
            Some(3072) => Some(KeyAlgorithm::Rsa3072),
            Some(4096) => Some(KeyAlgorithm::Rsa4096),
            _ => None,
        },
        Algorithm::Ecdsa {
            curve: EcdsaCurve::NistP256,
        } => Some(KeyAlgorithm::EcdsaP256),
        Algorithm::Ecdsa {
            curve: EcdsaCurve::NistP384,
        } => Some(KeyAlgorithm::EcdsaP384),
        Algorithm::Ecdsa {
            curve: EcdsaCurve::NistP521,
        } => Some(KeyAlgorithm::EcdsaP521),
        Algorithm::SkEd25519 => Some(KeyAlgorithm::SkEd25519),
        Algorithm::SkEcdsaSha2NistP256 => Some(KeyAlgorithm::SkEcdsaP256),
        _ => None,
    }
}

fn rng() -> impl russh::keys::ssh_key::rand_core::CryptoRng {
    russh::keys::key::safe_rng()
}

/// Generate a new key pair. With a passphrase, the private key is stored
/// encrypted (OpenSSH bcrypt-pbkdf + aes256-ctr).
pub fn generate_key(
    algorithm: KeyGenAlgorithm,
    comment: &str,
    passphrase: Option<&SecretString>,
) -> Result<GeneratedKey, KeyError> {
    let mut rng = rng();
    let keypair = match algorithm {
        KeyGenAlgorithm::Ed25519 => PrivateKey::random(&mut rng, Algorithm::Ed25519)
            .map_err(|e| KeyError::Generation(e.to_string()))?,
        KeyGenAlgorithm::Rsa3072 | KeyGenAlgorithm::Rsa4096 => {
            let bits = if algorithm == KeyGenAlgorithm::Rsa3072 {
                3072
            } else {
                4096
            };
            let rsa = ssh_key::private::RsaKeypair::random(&mut rng, bits)
                .map_err(|e| KeyError::Generation(e.to_string()))?;
            PrivateKey::new(KeypairData::from(rsa), "")
                .map_err(|e| KeyError::Generation(e.to_string()))?
        }
    };
    let mut keypair = keypair;
    keypair.set_comment(comment);
    let public_openssh = keypair
        .public_key()
        .to_openssh()
        .map_err(|e| KeyError::Generation(e.to_string()))?;
    let fingerprint = fingerprint_sha256(&key_blob(keypair.public_key().key_data()));
    let (stored, encrypted) = match passphrase {
        Some(p) if !p.expose_secret().is_empty() => (
            keypair
                .encrypt(&mut rng, p.expose_secret().as_bytes())
                .map_err(|e| KeyError::Generation(e.to_string()))?,
            true,
        ),
        _ => (keypair, false),
    };
    let pem = stored
        .to_openssh(LineEnding::LF)
        .map_err(|e| KeyError::Generation(e.to_string()))?;
    Ok(GeneratedKey {
        private_openssh: SecretString::from(pem.as_str()),
        public_openssh,
        fingerprint_sha256: fingerprint,
        algorithm: algorithm.model(),
        encrypted,
    })
}

fn detect_format(text: &str) -> Result<PrivateKeyFormat, KeyError> {
    let t = text.trim_start();
    if t.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----") {
        Ok(PrivateKeyFormat::OpenSsh)
    } else if t.starts_with("PuTTY-User-Key-File-") {
        Ok(PrivateKeyFormat::Putty)
    } else if t.starts_with("-----BEGIN ") && t.contains("PRIVATE KEY-----") {
        Ok(PrivateKeyFormat::LegacyPem)
    } else {
        Err(KeyError::InvalidFormat("not a private key".into()))
    }
}

fn legacy_is_encrypted(text: &str) -> bool {
    text.contains("Proc-Type: 4,ENCRYPTED") || text.contains("BEGIN ENCRYPTED PRIVATE KEY")
}

fn putty_is_encrypted(text: &str) -> bool {
    text.lines()
        .find_map(|l| l.strip_prefix("Encryption:"))
        .is_some_and(|v| v.trim() != "none")
}

/// Inspect a private key without removing its protection. For encrypted
/// OpenSSH keys the public part is readable without the passphrase; when a
/// passphrase *is* given it is verified. Legacy encrypted PEM needs the
/// passphrase to reveal the public key.
pub fn inspect_private_key(
    text: &str,
    passphrase: Option<&SecretString>,
) -> Result<PrivateKeyInfo, KeyError> {
    let format = detect_format(text)?;
    match format {
        PrivateKeyFormat::OpenSsh => {
            let key = PrivateKey::from_openssh(text.trim())
                .map_err(|e| KeyError::InvalidFormat(e.to_string()))?;
            let encrypted = key.is_encrypted();
            if encrypted {
                if let Some(p) = passphrase {
                    key.decrypt(p.expose_secret().as_bytes())
                        .map_err(|_| KeyError::WrongPassphrase)?;
                }
            }
            Ok(info_from_public(
                format,
                key.public_key(),
                encrypted,
                key.comment().as_str_lossy(),
            ))
        }
        PrivateKeyFormat::LegacyPem | PrivateKeyFormat::Putty => {
            let encrypted = if format == PrivateKeyFormat::Putty {
                putty_is_encrypted(text)
            } else {
                legacy_is_encrypted(text)
            };
            if encrypted && passphrase.is_none() {
                return Err(KeyError::PassphraseRequired);
            }
            let key = decode_legacy(text, passphrase, encrypted)?;
            Ok(info_from_public(
                format,
                key.public_key(),
                encrypted,
                key.comment().as_str_lossy(),
            ))
        }
    }
}

fn info_from_public(
    format: PrivateKeyFormat,
    public: &PublicKey,
    encrypted: bool,
    comment: &str,
) -> PrivateKeyInfo {
    let blob = key_blob(public.key_data());
    let mut public = public.clone();
    public.set_comment(comment);
    PrivateKeyInfo {
        format,
        algorithm: model_algorithm(public.key_data()),
        algorithm_name: public.algorithm().as_str().to_string(),
        public_openssh: public.to_openssh().unwrap_or_default(),
        fingerprint_sha256: fingerprint_sha256(&blob),
        encrypted,
        comment: comment.to_string(),
    }
}

fn decode_legacy(
    text: &str,
    passphrase: Option<&SecretString>,
    encrypted: bool,
) -> Result<PrivateKey, KeyError> {
    russh::keys::decode_secret_key(text, passphrase.map(|p| p.expose_secret())).map_err(|e| {
        if encrypted {
            KeyError::WrongPassphrase
        } else {
            KeyError::InvalidFormat(e.to_string())
        }
    })
}

/// Decode (and, if needed, decrypt) a private key **in memory**. The
/// returned key zeroizes its secret parts on drop.
pub fn load_private_key(
    text: &SecretString,
    passphrase: Option<&SecretString>,
) -> Result<PrivateKey, KeyError> {
    let text = text.expose_secret();
    match detect_format(text)? {
        PrivateKeyFormat::OpenSsh => {
            let key = PrivateKey::from_openssh(text.trim())
                .map_err(|e| KeyError::InvalidFormat(e.to_string()))?;
            if !key.is_encrypted() {
                return Ok(key);
            }
            let p = passphrase.ok_or(KeyError::PassphraseRequired)?;
            key.decrypt(p.expose_secret().as_bytes())
                .map_err(|_| KeyError::WrongPassphrase)
        }
        f @ (PrivateKeyFormat::LegacyPem | PrivateKeyFormat::Putty) => {
            let encrypted = if f == PrivateKeyFormat::Putty {
                putty_is_encrypted(text)
            } else {
                legacy_is_encrypted(text)
            };
            if encrypted && passphrase.is_none() {
                return Err(KeyError::PassphraseRequired);
            }
            decode_legacy(text, passphrase, encrypted)
        }
    }
}

/// Is this (OpenSSH / PEM / PPK) private key passphrase-protected?
pub fn is_private_key_encrypted(text: &str) -> Result<bool, KeyError> {
    match detect_format(text)? {
        PrivateKeyFormat::OpenSsh => Ok(PrivateKey::from_openssh(text.trim())
            .map_err(|e| KeyError::InvalidFormat(e.to_string()))?
            .is_encrypted()),
        PrivateKeyFormat::LegacyPem => Ok(legacy_is_encrypted(text)),
        PrivateKeyFormat::Putty => Ok(putty_is_encrypted(text)),
    }
}

/// `type base64 comment` line of a (decrypted or encrypted) private key.
pub fn public_key_line(key: &PrivateKey) -> String {
    key.public_key().to_openssh().unwrap_or_default()
}

/// Parse an OpenSSH public key line (`ssh-ed25519 AAAA… comment`).
pub fn parse_public_key(line: &str) -> Result<PublicKeyInfo, KeyError> {
    let key =
        PublicKey::from_openssh(line.trim()).map_err(|e| KeyError::InvalidFormat(e.to_string()))?;
    Ok(public_info(&key))
}

fn public_info(key: &PublicKey) -> PublicKeyInfo {
    PublicKeyInfo {
        algorithm: model_algorithm(key.key_data()),
        algorithm_name: key.algorithm().as_str().to_string(),
        fingerprint_sha256: fingerprint_sha256(&key_blob(key.key_data())),
        comment: key.comment().as_str_lossy().to_string(),
        openssh: key.to_openssh().unwrap_or_default(),
    }
}

/// Parse an OpenSSH certificate line (`ssh-ed25519-cert-v01@openssh.com AAAA…`).
pub fn parse_certificate(line: &str) -> Result<CertificateInfo, KeyError> {
    let cert = Certificate::from_openssh(line.trim())
        .map_err(|e| KeyError::InvalidFormat(e.to_string()))?;
    Ok(certificate_info(&cert))
}

pub(crate) fn certificate_info(cert: &Certificate) -> CertificateInfo {
    CertificateInfo {
        kind: match cert.cert_type() {
            CertType::User => CertificateKind::User,
            CertType::Host => CertificateKind::Host,
        },
        key_id: cert.key_id().to_string(),
        serial: cert.serial(),
        principals: cert.valid_principals().to_vec(),
        valid_after: cert.valid_after(),
        valid_before: cert.valid_before(),
        public_key_fingerprint: fingerprint_sha256(&key_blob(cert.public_key())),
        ca_fingerprint: fingerprint_sha256(&key_blob(cert.signature_key())),
        algorithm_name: cert.algorithm().to_certificate_type(),
        critical_options: cert.critical_options().keys().cloned().collect(),
        extensions: cert.extensions().keys().cloned().collect(),
    }
}

/// Does the certificate certify this private key's public key?
pub fn certificate_matches_key(cert_line: &str, key: &PrivateKey) -> Result<bool, KeyError> {
    let cert = Certificate::from_openssh(cert_line.trim())
        .map_err(|e| KeyError::InvalidFormat(e.to_string()))?;
    Ok(cert.public_key() == key.public_key().key_data())
}

/// Fill the non-secret metadata of a credential from an inspected key
/// (`public_key`, `fingerprint`, `key_algorithm`, `key_encrypted`).
pub fn apply_key_metadata(credential: &mut Credential, info: &PrivateKeyInfo) {
    credential.public_key = Some(info.public_openssh.clone());
    credential.fingerprint = Some(info.fingerprint_sha256.clone());
    credential.key_algorithm = info.algorithm;
    credential.key_encrypted = info.encrypted;
}

/// Sign a user or host certificate (CA operation; used by tests and by
/// users who run their own SSH CA).
#[allow(clippy::too_many_arguments)]
pub fn sign_certificate(
    ca: &PrivateKey,
    subject: &PublicKey,
    kind: CertificateKind,
    key_id: &str,
    principals: &[&str],
    valid_after: u64,
    valid_before: u64,
    user_extensions: bool,
) -> Result<String, KeyError> {
    use ssh_key::certificate::Builder;
    let err = |e: ssh_key::Error| KeyError::Generation(e.to_string());
    let mut rng = rng();
    let mut b = Builder::new_with_random_nonce(&mut rng, subject, valid_after, valid_before)
        .map_err(err)?;
    b.serial(1).map_err(err)?;
    b.key_id(key_id).map_err(err)?;
    b.cert_type(match kind {
        CertificateKind::User => CertType::User,
        CertificateKind::Host => CertType::Host,
    })
    .map_err(err)?;
    for p in principals {
        b.valid_principal(*p).map_err(err)?;
    }
    if user_extensions && kind == CertificateKind::User {
        for ext in [
            "permit-X11-forwarding",
            "permit-agent-forwarding",
            "permit-port-forwarding",
            "permit-pty",
            "permit-user-rc",
        ] {
            b.extension(ext, "").map_err(err)?;
        }
    }
    let cert = b.sign(ca).map_err(err)?;
    cert.to_openssh().map_err(err)
}

/// Hash algorithm name helper for fingerprints of other types.
pub fn fingerprint_of_public(key: &PublicKey) -> String {
    let _ = HashAlg::Sha256;
    fingerprint_sha256(&key_blob(key.key_data()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pass(s: &str) -> SecretString {
        SecretString::from(s)
    }

    #[test]
    fn generate_ed25519_plain_and_encrypted() {
        let k = generate_key(KeyGenAlgorithm::Ed25519, "me@test", None).unwrap();
        assert!(!k.encrypted);
        assert!(k.public_openssh.starts_with("ssh-ed25519 "));
        assert!(k.public_openssh.ends_with(" me@test"));
        assert!(k.fingerprint_sha256.starts_with("SHA256:"));
        assert_eq!(k.algorithm, KeyAlgorithm::Ed25519);
        let dbg = format!("{k:?}");
        assert!(!dbg.contains("PRIVATE KEY"), "{dbg}");

        let info = inspect_private_key(k.private_openssh.expose_secret(), None).unwrap();
        assert_eq!(info.fingerprint_sha256, k.fingerprint_sha256);
        assert!(!info.encrypted);
        assert_eq!(info.format, PrivateKeyFormat::OpenSsh);

        let e = generate_key(KeyGenAlgorithm::Ed25519, "enc", Some(&pass("s3cret pass"))).unwrap();
        assert!(e.encrypted);
        let text = e.private_openssh.expose_secret();
        assert!(is_private_key_encrypted(text).unwrap());
        // public part readable without passphrase
        let info = inspect_private_key(text, None).unwrap();
        assert!(info.encrypted);
        assert_eq!(info.fingerprint_sha256, e.fingerprint_sha256);
        // wrong / right passphrase
        assert_eq!(
            inspect_private_key(text, Some(&pass("nope"))),
            Err(KeyError::WrongPassphrase)
        );
        inspect_private_key(text, Some(&pass("s3cret pass"))).unwrap();
        assert_eq!(
            load_private_key(&e.private_openssh, None).unwrap_err(),
            KeyError::PassphraseRequired
        );
        assert_eq!(
            load_private_key(&e.private_openssh, Some(&pass("nope"))).unwrap_err(),
            KeyError::WrongPassphrase
        );
        let key = load_private_key(&e.private_openssh, Some(&pass("s3cret pass"))).unwrap();
        assert!(!key.is_encrypted());
        assert_eq!(
            fingerprint_of_public(key.public_key()),
            e.fingerprint_sha256
        );
        // loading never changes the stored text: still encrypted
        assert!(is_private_key_encrypted(e.private_openssh.expose_secret()).unwrap());
    }

    #[test]
    fn generate_rsa3072() {
        let k = generate_key(KeyGenAlgorithm::Rsa3072, "rsa", None).unwrap();
        assert!(k.public_openssh.starts_with("ssh-rsa "));
        let info = inspect_private_key(k.private_openssh.expose_secret(), None).unwrap();
        assert_eq!(info.algorithm, Some(KeyAlgorithm::Rsa3072));
        assert_eq!(info.algorithm_name, "ssh-rsa");
    }

    #[test]
    #[ignore = "RSA-4096 generation is slow in debug builds"]
    fn generate_rsa4096() {
        let k = generate_key(KeyGenAlgorithm::Rsa4096, "rsa", None).unwrap();
        let info = inspect_private_key(k.private_openssh.expose_secret(), None).unwrap();
        assert_eq!(info.algorithm, Some(KeyAlgorithm::Rsa4096));
    }

    #[test]
    fn public_key_parsing() {
        let k = generate_key(KeyGenAlgorithm::Ed25519, "c", None).unwrap();
        let info = parse_public_key(&k.public_openssh).unwrap();
        assert_eq!(info.fingerprint_sha256, k.fingerprint_sha256);
        assert_eq!(info.comment, "c");
        assert_eq!(info.algorithm, Some(KeyAlgorithm::Ed25519));
        assert!(parse_public_key("ssh-ed25519 notbase64!!").is_err());
        assert!(parse_public_key("garbage").is_err());
    }

    #[test]
    fn invalid_private_keys() {
        assert!(matches!(
            inspect_private_key("hello", None),
            Err(KeyError::InvalidFormat(_))
        ));
        assert!(matches!(
            inspect_private_key(
                "-----BEGIN OPENSSH PRIVATE KEY-----\nAAAA\n-----END OPENSSH PRIVATE KEY-----\n",
                None
            ),
            Err(KeyError::InvalidFormat(_))
        ));
        assert_eq!(
            inspect_private_key(
                "-----BEGIN RSA PRIVATE KEY-----\nProc-Type: 4,ENCRYPTED\nDEK-Info: AES-128-CBC,00\n\nAAAA\n-----END RSA PRIVATE KEY-----\n",
                None
            ),
            Err(KeyError::PassphraseRequired)
        );
    }

    #[test]
    fn certificates_sign_parse_match() {
        let ca = generate_key(KeyGenAlgorithm::Ed25519, "ca", None).unwrap();
        let ca_key = load_private_key(&ca.private_openssh, None).unwrap();
        let user = generate_key(KeyGenAlgorithm::Ed25519, "user", None).unwrap();
        let user_key = load_private_key(&user.private_openssh, None).unwrap();
        let other = generate_key(KeyGenAlgorithm::Ed25519, "other", None).unwrap();
        let other_key = load_private_key(&other.private_openssh, None).unwrap();

        let line = sign_certificate(
            &ca_key,
            user_key.public_key(),
            CertificateKind::User,
            "alice-cert",
            &["alice", "deploy"],
            0,
            u64::MAX,
            true,
        )
        .unwrap();
        assert!(line.starts_with("ssh-ed25519-cert-v01@openssh.com "));
        let info = parse_certificate(&line).unwrap();
        assert_eq!(info.kind, CertificateKind::User);
        assert_eq!(info.key_id, "alice-cert");
        assert_eq!(info.principals, vec!["alice", "deploy"]);
        assert_eq!(info.ca_fingerprint, ca.fingerprint_sha256);
        assert_eq!(info.public_key_fingerprint, user.fingerprint_sha256);
        assert!(info.extensions.contains(&"permit-pty".to_string()));
        assert!(info.is_valid_at(1_700_000_000));
        assert!(certificate_matches_key(&line, &user_key).unwrap());
        assert!(!certificate_matches_key(&line, &other_key).unwrap());
    }

    #[test]
    fn credential_metadata() {
        let k = generate_key(KeyGenAlgorithm::Ed25519, "c", Some(&pass("x"))).unwrap();
        let info = inspect_private_key(k.private_openssh.expose_secret(), None).unwrap();
        let mut cred = Credential::new("k", cc_models::credential::CredentialKind::SshPrivateKey);
        apply_key_metadata(&mut cred, &info);
        assert!(cred.key_encrypted);
        assert_eq!(
            cred.fingerprint.as_deref(),
            Some(k.fingerprint_sha256.as_str())
        );
        assert_eq!(cred.key_algorithm, Some(KeyAlgorithm::Ed25519));
    }

    #[test]
    fn fingerprint_format_matches_openssh() {
        // OpenSSH: SHA256 digest, base64 without padding.
        let fp = fingerprint_sha256(b"abc");
        assert_eq!(fp, "SHA256:ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0");
    }
}
