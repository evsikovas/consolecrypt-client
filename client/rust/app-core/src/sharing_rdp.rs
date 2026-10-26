//! Shared RDP hosts (FEAT-05): a card — name, address, port, user, domain,
//! tags and, when chosen, notes — and the saved password only after
//! explicit consent. It travels as a `Host` item, so neither the server nor
//! the protocol changes; clients before 0.9.3 cannot read the card and never
//! turn it into an SSH host.

use crate::dto::parse_id;
use crate::sharing_projection::SharedProjection;
use crate::{AppCore, AppError, AppResult, HostDto};
use cc_models::credential::{Credential, CredentialKind};
use cc_models::rdp::RdpHost;
use cc_models::secret::{Secret, SecretKind, SecretValue};
use cc_models::{ObjectId, VaultObject};
use cc_protocol::sharing::SharedItemKind;
use serde::{Deserialize, Serialize};
use std::fmt;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// The default RDP port when the card leaves it out.
const RDP_PORT: u16 = 3389;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
pub struct SharedRdpProjection {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub username: Option<String>,
    pub domain: Option<String>,
    pub tags: Vec<String>,
    /// Excluded by default. May contain sensitive text; preview explicitly.
    pub notes: Option<String>,
    /// Present only after explicit credential-sharing consent.
    pub password: Option<SecretValue>,
}

impl fmt::Debug for SharedRdpProjection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SharedRdpProjection(<redacted>)")
    }
}

fn invalid() -> AppError {
    AppError::invalid("shared_rdp_host", "invalid RDP host or password")
}

impl SharedRdpProjection {
    /// The card describes a valid RDP target; a password is a plain one.
    pub fn validate(&self) -> AppResult<()> {
        self.host(ObjectId::new()).map(|_| ())?;
        if let Some(password) = &self.password {
            if password.is_empty()
                || password.len() > 64 * 1024
                || password.expose_secret().contains('\0')
            {
                return Err(invalid());
            }
        }
        Ok(())
    }

    /// The receiver's own RDP host with a fresh identity.
    fn host(&self, id: ObjectId) -> AppResult<RdpHost> {
        let mut host = RdpHost::new(&self.name, &self.address);
        host.id = id;
        host.port = Some(self.port);
        host.username = self.username.clone();
        host.domain = self.domain.clone();
        host.tags = self.tags.clone();
        host.notes = self.notes.clone().unwrap_or_default();
        host.validate().map_err(|_| invalid())?;
        Ok(host)
    }

    /// UI previews deliberately omit the password.
    pub fn preview_json(&self) -> AppResult<String> {
        serde_json::to_string(&serde_json::json!({
            "kind": "rdp_host",
            "data": {
                "name": self.name, "address": self.address, "port": self.port,
                "username": self.username, "domain": self.domain,
                "tags": self.tags, "notes": self.notes,
                "includes_password": self.password.is_some(),
            },
        }))
        .map_err(|_| invalid())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpSharingPreview {
    pub preview_json: String,
    pub snapshot_stamp: String,
}

/// The card of saved RDP host `id` and a stamp of everything it was read
/// from; publication rejects a review whose stamp changed.
pub(crate) async fn prepare(
    u: &crate::session::Unlocked,
    id: ObjectId,
    include_password: bool,
    include_notes: bool,
) -> AppResult<(SharedRdpProjection, String)> {
    let ws = u.working();
    let host = ws.rdp_host(id).ok_or_else(invalid)?;
    host.validate().map_err(|_| invalid())?;
    let credential = host.credential_id.and_then(|id| ws.credential(id));
    if credential
        .as_ref()
        .is_some_and(|c| c.kind != CredentialKind::Password)
    {
        return Err(invalid());
    }
    let secret_id = credential.as_ref().and_then(|c| c.secret_id);
    if include_password && secret_id.is_none() {
        return Err(AppError::invalid("password", "no saved password"));
    }
    let mut ids = vec![host.id];
    ids.extend(host.credential_id);
    if include_password {
        ids.extend(secret_id);
    }
    let revisions: Vec<_> = ids.iter().map(|id| (*id, ws.revision_of(*id))).collect();
    let serialized = serde_json::to_vec(&(
        u.vault_id,
        &host,
        &credential,
        &revisions,
        include_password,
        include_notes,
    ))
    .map_err(|_| invalid())?;
    let stamp: String = cc_crypto_core::request_body_sha256(&serialized)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let password = match (include_password, secret_id) {
        (true, Some(secret_id)) => {
            let secret = u.writer.read_secret(secret_id).await?;
            if secret.kind != SecretKind::Password {
                return Err(invalid());
            }
            Some(secret.value.clone())
        }
        _ => None,
    };
    if u.working().rdp_host(host.id).as_ref() != Some(&host)
        || revisions
            .iter()
            .any(|(id, revision)| u.working().revision_of(*id) != *revision)
    {
        return Err(AppError::invalid("host", "host changed; review it again"));
    }
    let username = host
        .username
        .clone()
        .or_else(|| credential.as_ref().and_then(|c| c.username.clone()))
        .filter(|name| !name.trim().is_empty());
    let projection = SharedRdpProjection {
        name: host.name.clone(),
        address: host.address.clone(),
        port: host.port.filter(|port| *port > 0).unwrap_or(RDP_PORT),
        username,
        domain: host.domain.clone(),
        tags: host.tags.clone(),
        notes: include_notes.then(|| host.notes.clone()),
        password,
    };
    projection.validate()?;
    Ok((projection, stamp))
}

impl AppCore {
    pub async fn sharing_preview_rdp_host(
        &self,
        host_id: String,
        include_password: bool,
        include_notes: bool,
    ) -> AppResult<RdpSharingPreview> {
        let (_, u) = self.unlocked().await?;
        let id = parse_id("host_id", &host_id)?;
        let (projection, snapshot_stamp) = prepare(&u, id, include_password, include_notes).await?;
        Ok(RdpSharingPreview {
            preview_json: projection.preview_json()?,
            snapshot_stamp,
        })
    }

    /// Saves a personal copy of a shared RDP host. The shared password is
    /// imported (as the copy's own credential) only on explicit request.
    pub async fn sharing_copy_rdp_host(
        &self,
        share_id: String,
        include_password: bool,
    ) -> AppResult<HostDto> {
        self.sharing_with_verified_projection(
            share_id,
            SharedItemKind::Host,
            move |u, _, projection| {
                Box::pin(async move {
                    let SharedProjection::RdpHost(card) = projection else {
                        return Err(invalid());
                    };
                    let mut host = card.host(ObjectId::new())?;
                    let mut objects = Vec::new();
                    if let (true, Some(password)) = (include_password, &card.password) {
                        let secret = Secret::new(SecretKind::Password, password.clone());
                        let mut credential = Credential::new(&card.name, CredentialKind::Password);
                        credential.secret_id = Some(secret.id);
                        credential.username = card.username.clone();
                        host.credential_id = Some(credential.id);
                        objects.push(VaultObject::Secret(secret));
                        objects.push(VaultObject::Credential(credential));
                    }
                    let dto = HostDto::from_rdp(&host);
                    objects.push(VaultObject::RdpHost(host));
                    u.writer.put_batch(objects).await?;
                    Ok(dto)
                })
            },
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(password: Option<&str>) -> SharedRdpProjection {
        SharedRdpProjection {
            name: "Office PC".into(),
            address: "10.0.0.5".into(),
            port: 3389,
            username: Some("Администратор".into()),
            domain: Some("CORP".into()),
            tags: vec!["office".into()],
            notes: None,
            password: password.map(SecretValue::new),
        }
    }

    #[test]
    fn cards_travel_as_hosts_and_previews_never_show_the_password() {
        let canary = "pa55-canary-word";
        let projection = SharedProjection::RdpHost(card(Some(canary)));
        assert_eq!(projection.kind(), SharedItemKind::Host);
        assert!(projection.carries_secrets());
        let bytes = projection.encode().unwrap();
        let decoded = SharedProjection::decode(SharedItemKind::Host, bytes.as_slice()).unwrap();
        assert_eq!(decoded, projection);
        let SharedProjection::RdpHost(decoded) = &decoded else {
            panic!("an RDP card");
        };
        let preview = decoded.preview_json().unwrap();
        assert!(!preview.contains(canary), "{preview}");
        assert!(preview.contains(r#""includes_password":true"#), "{preview}");
        assert!(preview.contains(r#""kind":"rdp_host""#));
        assert!(!SharedProjection::RdpHost(card(None)).carries_secrets());
        // A card is not an SSH host for the receiving side.
        assert!(SharedProjection::decode(SharedItemKind::Snippet, bytes.as_slice()).is_err());
    }

    #[test]
    fn invalid_cards_are_refused() {
        let mut bad = card(None);
        bad.address = String::new();
        assert!(bad.validate().is_err());
        let mut nul = card(Some("a\0b"));
        nul.name = "PC".into();
        assert!(nul.validate().is_err());
        assert!(card(Some("")).validate().is_err());
        assert!(card(Some("ok")).validate().is_ok());
    }
}
