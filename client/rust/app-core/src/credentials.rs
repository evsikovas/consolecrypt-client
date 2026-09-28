//! Credentials and their Secret objects (CLIENT_SPEC §7). Private keys,
//! passwords and remembered key passphrases live **only** in Secret objects
//! (Secrets KEK); the Credential carries metadata and references. Imported
//! keys keep their original passphrase protection (§7.6).

use crate::app::AppCore;
use crate::dto::{parse_id, CredentialDto, KeyGenAlgorithm};
use crate::error::{AppError, AppResult};
use crate::inventory::replace_secret;
use crate::secrets::blocking;
use crate::session::Unlocked;
use cc_models::credential::{Credential, CredentialKind};
use cc_models::secret::{Secret, SecretKind, SecretValue};
use cc_models::{ObjectId, ObjectKind, VaultObject};
use cc_ssh_core::keys;
use secrecy::{ExposeSecret, SecretString};
use std::sync::Arc;

fn clean(s: Option<String>) -> Option<String> {
    s.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

fn check_name(name: &str) -> AppResult<String> {
    let n = name.trim();
    if n.is_empty() {
        return Err(AppError::invalid("name", "must not be empty"));
    }
    Ok(n.to_owned())
}

async fn put_secret(u: &Unlocked, kind: SecretKind, value: &str) -> AppResult<ObjectId> {
    let s = Secret::new(kind, SecretValue::new(value));
    let id = s.id;
    u.writer.put(VaultObject::Secret(s)).await?;
    Ok(id)
}

/// DTO with the owning host (inline credentials, ADR-0101 §6a).
fn with_owner(u: &Unlocked, c: &Credential) -> CredentialDto {
    let mut d = CredentialDto::from_model(c);
    d.owner_host_id = crate::host_auth::owner_of(u, c.id).map(|h| h.id.to_string());
    d
}

impl AppCore {
    async fn credential_model(&self, id: &str) -> AppResult<(Arc<Unlocked>, Credential)> {
        let (_, u) = self.unlocked().await?;
        let id = parse_id("id", id)?;
        let c = u
            .working()
            .credential(id)
            .ok_or_else(|| AppError::not_found("credential", id))?;
        Ok((u, c))
    }

    async fn store_credential(&self, u: &Unlocked, c: Credential) -> AppResult<CredentialDto> {
        let id = c.id;
        crate::inventory::validate_refs(u.working(), &VaultObject::Credential(c.clone()))?;
        u.writer.put(VaultObject::Credential(c)).await?;
        u.working()
            .credential(id)
            .map(|c| with_owner(u, &c))
            .ok_or_else(|| AppError::internal("credential vanished after save"))
    }

    pub async fn list_credentials(&self) -> AppResult<Vec<CredentialDto>> {
        let (_, u) = self.unlocked().await?;
        let mut v: Vec<CredentialDto> = u
            .working()
            .credentials()
            .iter()
            .map(|c| with_owner(&u, c))
            .collect();
        v.sort_by_key(|a| a.name.to_lowercase());
        Ok(v)
    }

    pub async fn get_credential(&self, id: String) -> AppResult<CredentialDto> {
        let (u, c) = self.credential_model(&id).await?;
        Ok(with_owner(&u, &c))
    }

    /// Resolve a credential by id or (unique, case-insensitive) name.
    pub async fn find_credential(&self, id_or_name: String) -> AppResult<CredentialDto> {
        let all = self.list_credentials().await?;
        if let Some(c) = all.iter().find(|c| c.id == id_or_name.trim()) {
            return Ok(c.clone());
        }
        let m: Vec<&CredentialDto> = all
            .iter()
            .filter(|c| c.name.eq_ignore_ascii_case(id_or_name.trim()))
            .collect();
        match m.as_slice() {
            [one] => Ok((*one).clone()),
            [] => Err(AppError::not_found("credential", id_or_name)),
            _ => Err(AppError::invalid(
                "credential",
                "name is ambiguous; use the id",
            )),
        }
    }

    /// Password credential (the password becomes a Secret object).
    pub async fn add_password_credential(
        &self,
        name: String,
        username: Option<String>,
        password: String,
    ) -> AppResult<CredentialDto> {
        let name = check_name(&name)?;
        if password.is_empty() {
            return Err(AppError::invalid("password", "must not be empty"));
        }
        let (_, u) = self.unlocked().await?;
        let secret_id = put_secret(&u, SecretKind::Password, &password).await?;
        let mut c = Credential::new(name, CredentialKind::Password);
        c.username = clean(username);
        c.secret_id = Some(secret_id);
        self.store_credential(&u, c).await
    }

    /// Generate an SSH key pair (Ed25519 default; RSA 3072/4096 for
    /// compatibility). With `passphrase` the stored private key is
    /// encrypted; `remember_passphrase` stores the passphrase as its own
    /// Secret object so connects need no prompt.
    pub async fn generate_ssh_key(
        &self,
        name: String,
        username: Option<String>,
        algorithm: KeyGenAlgorithm,
        passphrase: Option<String>,
        remember_passphrase: bool,
    ) -> AppResult<CredentialDto> {
        let name = check_name(&name)?;
        let (_, u) = self.unlocked().await?;
        let passphrase = passphrase.filter(|p| !p.is_empty()).map(SecretString::from);
        let pp = passphrase.clone();
        let comment = name.clone();
        let generated =
            blocking(move || Ok(keys::generate_key(algorithm, &comment, pp.as_ref())?)).await?;
        let secret_id = put_secret(
            &u,
            SecretKind::SshPrivateKey,
            generated.private_openssh.expose_secret(),
        )
        .await?;
        let mut c = Credential::new(name, CredentialKind::SshPrivateKey);
        c.username = clean(username);
        c.secret_id = Some(secret_id);
        c.public_key = Some(generated.public_openssh.clone());
        c.fingerprint = Some(generated.fingerprint_sha256.clone());
        c.key_algorithm = Some(generated.algorithm);
        c.key_encrypted = generated.encrypted;
        if let (true, Some(p)) = (remember_passphrase, &passphrase) {
            c.passphrase_secret_id =
                Some(put_secret(&u, SecretKind::SshKeyPassphrase, p.expose_secret()).await?);
        }
        tracing::info!(credential_id = %c.id, algorithm = ?generated.algorithm, "ssh key generated");
        self.store_credential(&u, c).await
    }

    /// Import an OpenSSH / PEM / PPK private key (optionally with an OpenSSH
    /// user certificate). The key is stored exactly as given (passphrase
    /// protection is never removed); a given passphrase is verified and
    /// stored only if `remember_passphrase`.
    pub async fn import_ssh_key(
        &self,
        name: String,
        username: Option<String>,
        private_key: String,
        passphrase: Option<String>,
        remember_passphrase: bool,
        certificate: Option<String>,
    ) -> AppResult<CredentialDto> {
        let name = check_name(&name)?;
        let (_, u) = self.unlocked().await?;
        let passphrase = passphrase.filter(|p| !p.is_empty()).map(SecretString::from);
        let key_text = SecretString::from(private_key);
        let kt = key_text.clone();
        let pp = passphrase.clone();
        let info =
            blocking(move || Ok(keys::inspect_private_key(kt.expose_secret(), pp.as_ref())?))
                .await?;
        let certificate = clean(certificate);
        if let Some(cert) = &certificate {
            let ci = keys::parse_certificate(cert)?;
            if ci.public_key_fingerprint != info.fingerprint_sha256 {
                return Err(AppError::invalid(
                    "certificate",
                    "the certificate does not belong to this key",
                ));
            }
        }
        let secret_id = put_secret(&u, SecretKind::SshPrivateKey, key_text.expose_secret()).await?;
        let kind = if certificate.is_some() {
            CredentialKind::SshCertificate
        } else {
            CredentialKind::SshPrivateKey
        };
        let mut c = Credential::new(name, kind);
        c.username = clean(username);
        c.secret_id = Some(secret_id);
        c.certificate = certificate;
        keys::apply_key_metadata(&mut c, &info);
        if let (true, true, Some(p)) = (remember_passphrase, info.encrypted, &passphrase) {
            c.passphrase_secret_id =
                Some(put_secret(&u, SecretKind::SshKeyPassphrase, p.expose_secret()).await?);
        }
        tracing::info!(credential_id = %c.id, encrypted = info.encrypted, "ssh key imported");
        self.store_credential(&u, c).await
    }

    /// Credential backed by an SSH agent: the OS agent (`agent_path =
    /// None`) or a third-party agent socket / pipe.
    pub async fn add_agent_credential(
        &self,
        name: String,
        username: Option<String>,
        agent_path: Option<String>,
    ) -> AppResult<CredentialDto> {
        let name = check_name(&name)?;
        let (_, u) = self.unlocked().await?;
        let agent_path = clean(agent_path);
        let kind = if agent_path.is_some() {
            CredentialKind::ExternalAgent
        } else {
            CredentialKind::OsSshAgent
        };
        let mut c = Credential::new(name, kind);
        c.username = clean(username);
        c.agent_path = agent_path;
        self.store_credential(&u, c).await
    }

    /// Edit non-secret fields. `certificate`: `Some("")` removes it.
    pub async fn update_credential(
        &self,
        id: String,
        name: String,
        username: Option<String>,
        certificate: Option<String>,
    ) -> AppResult<CredentialDto> {
        let (u, mut c) = self.credential_model(&id).await?;
        c.name = check_name(&name)?;
        c.username = clean(username);
        if let Some(cert) = certificate {
            let cert = cert.trim().to_owned();
            if cert.is_empty() {
                c.certificate = None;
                if c.kind == CredentialKind::SshCertificate {
                    c.kind = CredentialKind::SshPrivateKey;
                }
            } else {
                let ci = keys::parse_certificate(&cert)?;
                if c.fingerprint.as_deref() != Some(ci.public_key_fingerprint.as_str()) {
                    return Err(AppError::invalid(
                        "certificate",
                        "the certificate does not belong to this key",
                    ));
                }
                c.certificate = Some(cert);
                c.kind = CredentialKind::SshCertificate;
            }
        }
        c.updated_at = chrono::Utc::now();
        self.store_credential(&u, c).await
    }

    /// Replace the password of a password credential.
    pub async fn set_credential_password(
        &self,
        id: String,
        password: String,
    ) -> AppResult<CredentialDto> {
        let (u, mut c) = self.credential_model(&id).await?;
        if c.kind != CredentialKind::Password {
            return Err(AppError::invalid("id", "not a password credential"));
        }
        let old = c.secret_id;
        c.secret_id = replace_secret(&u, old, Some(password), SecretKind::Password).await?;
        c.updated_at = chrono::Utc::now();
        let dto = self.store_credential(&u, c).await?;
        if let Some(o) = old {
            u.writer.delete(o).await?;
        }
        Ok(dto)
    }

    /// Remember (`Some`, verified against the key) or forget (`None`) the
    /// passphrase of an encrypted private key.
    pub async fn set_key_passphrase(
        &self,
        id: String,
        passphrase: Option<String>,
    ) -> AppResult<CredentialDto> {
        let (u, mut c) = self.credential_model(&id).await?;
        if !matches!(
            c.kind,
            CredentialKind::SshPrivateKey | CredentialKind::SshCertificate
        ) {
            return Err(AppError::invalid("id", "not a key credential"));
        }
        let old = c.passphrase_secret_id;
        c.passphrase_secret_id = match passphrase.filter(|p| !p.is_empty()) {
            Some(p) => {
                let key_id = c
                    .secret_id
                    .ok_or(AppError::invalid("id", "no key stored"))?;
                let key = u.writer.read_secret(key_id).await?;
                let pp = SecretString::from(p.as_str());
                let text = SecretString::from(key.value.expose_secret());
                blocking(move || {
                    keys::inspect_private_key(text.expose_secret(), Some(&pp))?;
                    Ok(())
                })
                .await?;
                Some(put_secret(&u, SecretKind::SshKeyPassphrase, &p).await?)
            }
            None => None,
        };
        c.updated_at = chrono::Utc::now();
        let dto = self.store_credential(&u, c).await?;
        if let Some(o) = old {
            u.writer.delete(o).await?;
        }
        Ok(dto)
    }

    /// The OpenSSH public key line of a key credential (for
    /// `authorized_keys`).
    pub async fn credential_public_key(&self, id: String) -> AppResult<String> {
        let (_, c) = self.credential_model(&id).await?;
        c.public_key
            .ok_or_else(|| AppError::invalid("id", "credential has no public key"))
    }

    /// Delete a credential and its Secret objects; refused while hosts or
    /// groups use it.
    pub async fn delete_credential(&self, id: String) -> AppResult<()> {
        let (id, u) = self.delete_object(&id, ObjectKind::Credential).await?;
        let refs: Vec<String> = u
            .working()
            .hosts()
            .iter()
            .filter(|h| h.credential_id == Some(id))
            .map(|h| format!("host '{}'", h.name))
            .chain(
                u.working()
                    .groups()
                    .iter()
                    .filter(|g| g.inherited_credential_id == Some(id))
                    .map(|g| format!("group '{}'", g.name)),
            )
            .collect();
        if !refs.is_empty() {
            return Err(AppError::InUse {
                what: "credential".into(),
                id: id.to_string(),
                used_by: refs.join(", "),
            });
        }
        let c = u.working().credential(id);
        u.writer.delete(id).await?;
        if let Some(c) = c {
            for s in [c.secret_id, c.passphrase_secret_id].into_iter().flatten() {
                u.writer.delete(s).await?;
            }
        }
        Ok(())
    }
}
