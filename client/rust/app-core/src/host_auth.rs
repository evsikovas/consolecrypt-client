//! Host authentication modes (ADR-0101 §6a, Termius-style): the host editor
//! sends the full desired state with [`AppCore::save_host_with_auth`].
//!
//! Until cc-models gains dedicated fields, two `Host.metadata` keys carry
//! the state:
//!
//! * [`META_AUTH_PROMPT`] `= "password"` — nothing stored; ask for the
//!   password at connect. This also stops group-credential inheritance: the
//!   inventory presents such a host with a synthetic, secret-less password
//!   credential ([`prompt_credential_id`]) whose resolution asks the UI
//!   ([`crate::PromptRequest::Password`]).
//! * [`META_INLINE_CREDENTIAL`] `= <credential id>` — the host's own
//!   Password credential (+ Secret), deleted with the host or when replaced.

use crate::app::AppCore;
use crate::dto::{parse_id, HostDto};
use crate::error::{AppError, AppResult};
use crate::session::Unlocked;
use cc_models::credential::{Credential, CredentialKind};
use cc_models::host::Host;
use cc_models::secret::{Secret, SecretKind, SecretValue};
use cc_models::{ObjectId, VaultObject};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// `Host.metadata` key: `"password"` = ask for the password at connect.
pub const META_AUTH_PROMPT: &str = "cc.auth.prompt";
/// `Host.metadata` key: id of the host-owned (inline) credential.
pub const META_INLINE_CREDENTIAL: &str = "cc.auth.inline_credential";

/// Which SSH agent an agent credential uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    /// The OS agent (`SSH_AUTH_SOCK`, Windows OpenSSH agent / Pageant).
    Os,
    /// A third-party agent socket / pipe (1Password, Secretive, …).
    External,
}

/// Desired authentication of a host. `InlinePassword` carries a secret:
/// redacted `Debug`, zeroized on drop.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum HostAuth {
    /// Use the group's credential (if any).
    Inherit,
    /// A shared credential of any kind.
    Credential { credential_id: String },
    /// The host's own password credential; `None` keeps the stored secret.
    InlinePassword { password: Option<String> },
    /// Nothing stored: ask at connect (also stops inheritance).
    PasswordPrompt,
    /// An agent credential (reused if one with the same kind/path exists).
    Agent {
        #[zeroize(skip)]
        kind: AgentKind,
        path: Option<String>,
    },
}

impl std::fmt::Debug for HostAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HostAuth::Inherit => f.write_str("HostAuth::Inherit"),
            HostAuth::Credential { credential_id } => f
                .debug_struct("HostAuth::Credential")
                .field("credential_id", credential_id)
                .finish(),
            HostAuth::InlinePassword { password } => f
                .debug_struct("HostAuth::InlinePassword")
                .field("password", &password.as_ref().map(|_| "<redacted>"))
                .finish(),
            HostAuth::PasswordPrompt => f.write_str("HostAuth::PasswordPrompt"),
            HostAuth::Agent { kind, path } => f
                .debug_struct("HostAuth::Agent")
                .field("kind", kind)
                .field("path", path)
                .finish(),
        }
    }
}

/// How a host authenticates (output-only summary in [`HostDto`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostAuthMode {
    Inherit,
    Credential,
    InlinePassword,
    PasswordPrompt,
}

impl HostAuthMode {
    pub(crate) fn of(h: &Host) -> Self {
        if prompts_for_password(h) {
            HostAuthMode::PasswordPrompt
        } else if h.credential_id.is_some() && inline_credential(h) == h.credential_id {
            HostAuthMode::InlinePassword
        } else if h.credential_id.is_some() {
            HostAuthMode::Credential
        } else {
            HostAuthMode::Inherit
        }
    }
}

/// The host asks for its password at connect.
pub(crate) fn prompts_for_password(h: &Host) -> bool {
    h.credential_id.is_none()
        && h.metadata
            .get(META_AUTH_PROMPT)
            .is_some_and(|v| v == "password")
}

/// The host-owned credential, if any.
pub(crate) fn inline_credential(h: &Host) -> Option<ObjectId> {
    h.metadata
        .get(META_INLINE_CREDENTIAL)
        .and_then(|v| v.parse().ok())
}

const PROMPT_MASK: [u8; 16] = *b"cc.auth.prompt!!";

fn xor(id: ObjectId) -> ObjectId {
    let mut b = *id.as_bytes();
    for (x, m) in b.iter_mut().zip(PROMPT_MASK) {
        *x ^= m;
    }
    ObjectId::from_uuid(uuid::Uuid::from_bytes(b))
}

/// Synthetic credential id standing for "ask for the password of host
/// `host_id` at connect" (never stored).
pub(crate) fn prompt_credential_id(host_id: ObjectId) -> ObjectId {
    xor(host_id)
}

/// Host a synthetic prompt credential id belongs to (inverse of
/// [`prompt_credential_id`]; the caller checks the host exists and prompts).
pub(crate) fn prompt_host_of(credential_id: ObjectId) -> ObjectId {
    xor(credential_id)
}

/// The synthetic secret-less password credential of a prompting host.
pub(crate) fn prompt_credential(h: &Host) -> Credential {
    let mut c = Credential::new(format!("password for {}", h.name), CredentialKind::Password);
    c.id = prompt_credential_id(h.id);
    c.created_at = h.created_at;
    c.updated_at = h.updated_at;
    c
}

/// Hosts that own `credential_id` as their inline credential.
pub(crate) fn owner_of(u: &Unlocked, credential_id: ObjectId) -> Option<Host> {
    u.working()
        .hosts()
        .into_iter()
        .find(|h| inline_credential(h) == Some(credential_id))
}

async fn new_password_credential(
    u: &Unlocked,
    name: String,
    password: String,
) -> AppResult<ObjectId> {
    if password.is_empty() {
        return Err(AppError::invalid("password", "must not be empty"));
    }
    crate::validate::secret_text("password", &password)?;
    let s = Secret::new(SecretKind::Password, SecretValue::new(password));
    let secret_id = s.id;
    u.writer.put(VaultObject::Secret(s)).await?;
    let mut c = Credential::new(name, CredentialKind::Password);
    c.secret_id = Some(secret_id);
    let id = c.id;
    u.writer.put(VaultObject::Credential(c)).await?;
    Ok(id)
}

/// Delete a credential and its secrets unless something still uses it.
pub(crate) async fn delete_credential_if_unused(u: &Unlocked, id: ObjectId) -> AppResult<()> {
    let used = u
        .working()
        .hosts()
        .iter()
        .any(|h| h.credential_id == Some(id))
        || u.working()
            .groups()
            .iter()
            .any(|g| g.inherited_credential_id == Some(id));
    if used {
        return Ok(());
    }
    if let Some(c) = u.working().credential(id) {
        u.writer.delete(id).await?;
        for s in [c.secret_id, c.passphrase_secret_id].into_iter().flatten() {
            u.writer.delete(s).await?;
        }
    }
    Ok(())
}

impl AppCore {
    /// Save a host together with its authentication (ADR-0101 §6a).
    /// Creates/updates/deletes the host-owned inline credential atomically
    /// from the caller's point of view (rolled back if the host save fails).
    pub async fn save_host_with_auth(&self, host: HostDto, auth: HostAuth) -> AppResult<HostDto> {
        let (_, u) = self.unlocked().await?;
        let existing = if host.id.trim().is_empty() {
            None
        } else {
            let id = parse_id("id", &host.id)?;
            Some(
                u.working()
                    .host(id)
                    .ok_or_else(|| AppError::not_found("host", id))?,
            )
        };
        let old_inline = existing.as_ref().and_then(inline_credential);
        let mut dto = host;
        dto.metadata.remove(META_AUTH_PROMPT);
        dto.metadata.remove(META_INLINE_CREDENTIAL);
        let mut keep_inline: Option<ObjectId> = None;
        let mut created: Option<ObjectId> = None;
        match &auth {
            HostAuth::Inherit => dto.credential_id = None,
            HostAuth::Credential { credential_id } => {
                let id = parse_id("credential_id", credential_id)?;
                if u.working().credential(id).is_none() {
                    return Err(AppError::invalid(
                        "credential_id",
                        "references a missing credential",
                    ));
                }
                if Some(id) == old_inline {
                    keep_inline = Some(id);
                } else if owner_of(&u, id).is_some() {
                    return Err(AppError::invalid(
                        "credential_id",
                        "belongs to another host (inline credential)",
                    ));
                }
                dto.credential_id = Some(id.to_string());
            }
            HostAuth::InlinePassword { password } => {
                let inline = old_inline.filter(|c| u.working().credential(*c).is_some());
                let id = match (inline, password.clone()) {
                    (Some(c), None) => c,
                    (Some(c), Some(p)) => {
                        self.set_credential_password(c.to_string(), p).await?;
                        c
                    }
                    (None, Some(p)) => {
                        let name = format!("{} (password)", dto.name.trim());
                        let id = new_password_credential(&u, name, p).await?;
                        created = Some(id);
                        id
                    }
                    (None, None) => {
                        return Err(AppError::invalid(
                            "password",
                            "required for a new inline password",
                        ))
                    }
                };
                keep_inline = Some(id);
                dto.credential_id = Some(id.to_string());
            }
            HostAuth::PasswordPrompt => {
                dto.credential_id = None;
                dto.metadata
                    .insert(META_AUTH_PROMPT.to_owned(), "password".to_owned());
            }
            HostAuth::Agent { kind, path } => {
                let path = path
                    .as_deref()
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .map(str::to_owned);
                let (ckind, path) = match kind {
                    AgentKind::Os => (CredentialKind::OsSshAgent, None),
                    AgentKind::External => (
                        CredentialKind::ExternalAgent,
                        Some(path.ok_or_else(|| {
                            AppError::invalid(
                                "path",
                                "an external agent needs a socket / pipe path",
                            )
                        })?),
                    ),
                };
                let reuse = u.working().credentials().into_iter().find(|c| {
                    c.kind == ckind && c.agent_path == path && owner_of(&u, c.id).is_none()
                });
                let id = match reuse {
                    Some(c) => c.id,
                    None => {
                        let name = match &path {
                            Some(p) => format!("SSH agent ({p})"),
                            None => "SSH agent".to_owned(),
                        };
                        let dto = self.add_agent_credential(name, None, path.clone()).await?;
                        parse_id("id", &dto.id)?
                    }
                };
                dto.credential_id = Some(id.to_string());
            }
        }
        if let Some(i) = keep_inline {
            dto.metadata
                .insert(META_INLINE_CREDENTIAL.to_owned(), i.to_string());
        }
        let saved = match self.save_host(dto).await {
            Ok(s) => s,
            Err(e) => {
                if let Some(c) = created {
                    let _ = delete_credential_if_unused(&u, c).await;
                }
                return Err(e);
            }
        };
        if let Some(old) = old_inline.filter(|o| Some(*o) != keep_inline) {
            delete_credential_if_unused(&u, old).await?;
        }
        Ok(saved)
    }

    /// Remember the passphrase of an encrypted key credential (verified).
    pub async fn remember_key_passphrase(
        &self,
        credential_id: String,
        passphrase: String,
    ) -> AppResult<crate::CredentialDto> {
        self.set_key_passphrase(credential_id, Some(passphrase))
            .await
    }

    /// Forget a remembered key passphrase (connects will ask again).
    pub async fn forget_key_passphrase(
        &self,
        credential_id: String,
    ) -> AppResult<crate::CredentialDto> {
        self.set_key_passphrase(credential_id, None).await
    }
}
