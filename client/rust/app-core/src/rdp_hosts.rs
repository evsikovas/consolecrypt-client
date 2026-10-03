//! Saved RDP targets. The UI receives a public snapshot; passwords stay native.
use crate::session::{Session, Unlocked};
use crate::{AppCore, AppError, AppResult, RdpConnectConfig, RdpSessionPermissions};
use cc_models::{credential::CredentialKind, secret::SecretKind, ObjectId};
use secrecy::SecretString;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RdpSavedHostTicket {
    pub host_id: String,
    pub name: String,
    pub address: String,
    pub port: u16,
    pub username: String,
    pub domain: String,
    pub width: u16,
    pub height: u16,
    pub fingerprint: String,
    pub snapshot_stamp: String,
    pub has_saved_password: bool,
}

fn snapshot(
    s: &Session,
    u: &Unlocked,
    id: ObjectId,
) -> AppResult<(RdpSavedHostTicket, Option<ObjectId>)> {
    let ws = u.working();
    let h = ws
        .rdp_host(id)
        .ok_or_else(|| AppError::not_found("rdp_host", id))?;
    h.validate()
        .map_err(|_| AppError::invalid("host", "invalid RDP target"))?;
    let credential = h
        .credential_id
        .map(|id| {
            ws.credential(id)
                .ok_or_else(|| AppError::not_found("credential", id))
        })
        .transpose()?;
    if credential
        .as_ref()
        .is_some_and(|c| c.kind != CredentialKind::Password)
    {
        return Err(AppError::invalid(
            "credential",
            "RDP requires password authentication",
        ));
    }
    let secret_id = credential.as_ref().and_then(|c| c.secret_id);
    if secret_id.is_some_and(|id| ws.secret_kind(id) != Some(SecretKind::Password)) {
        return Err(AppError::invalid("credential", "password is unavailable"));
    }
    let username = h
        .username
        .as_ref()
        .or_else(|| credential.as_ref().and_then(|c| c.username.as_ref()))
        .filter(|v| !v.trim().is_empty())
        .cloned()
        .ok_or_else(|| AppError::invalid("username", "required for RDP"))?;
    let stamp_bytes = serde_json::to_vec(&(
        s.profile_id,
        u.vault_id,
        &h,
        &credential,
        ws.revision_of(id),
        h.credential_id.and_then(|id| ws.revision_of(id)),
        secret_id.and_then(|id| ws.revision_of(id)),
    ))
    .map_err(|_| AppError::invalid("host", "invalid snapshot"))?;
    let stamp = cc_crypto_core::request_body_sha256(&stamp_bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok((
        RdpSavedHostTicket {
            host_id: id.to_string(),
            name: h.name.clone(),
            address: h.address.clone(),
            port: h.port.unwrap_or(cc_models::rdp::DEFAULT_RDP_PORT),
            username,
            domain: h.domain.clone().unwrap_or_default(),
            width: h.desktop_width,
            height: h.desktop_height,
            fingerprint: String::new(),
            snapshot_stamp: stamp,
            has_saved_password: secret_id.is_some(),
        },
        secret_id,
    ))
}

fn parse_id(value: &str) -> AppResult<ObjectId> {
    value
        .parse()
        .map_err(|_| AppError::invalid("host_id", "invalid id"))
}

fn certificate_pin(value: &str) -> AppResult<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::invalid("certificate", "expected SHA-256"));
    }
    let mut pin = [0; 32];
    for (i, b) in pin.iter_mut().enumerate() {
        *b = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16)
            .map_err(|_| AppError::invalid("certificate", "expected SHA-256"))?;
    }
    Ok(pin)
}

fn ensure_current(ticket: &RdpSavedHostTicket, mut current: RdpSavedHostTicket) -> AppResult<()> {
    current.fingerprint.clone_from(&ticket.fingerprint);
    if current != *ticket {
        return Err(AppError::invalid(
            "host",
            "RDP target changed; review connection again",
        ));
    }
    Ok(())
}

impl AppCore {
    pub async fn rdp_saved_host_probe(&self, host_id: String) -> AppResult<RdpSavedHostTicket> {
        let (s, u) = self.unlocked().await?;
        let id = parse_id(&host_id)?;
        let (mut ticket, _) = snapshot(&s, &u, id)?;
        let cert = u
            .rdp
            .probe_certificate(&ticket.address, ticket.port)
            .await
            .map_err(AppError::Rdp)?;
        ensure_current(&ticket, snapshot(&s, &u, id)?.0)?;
        ticket.fingerprint = cert.fingerprint.replace(':', "").to_lowercase();
        Ok(ticket)
    }

    pub async fn rdp_saved_host_connect(
        &self,
        ticket: RdpSavedHostTicket,
        password: Option<SecretString>,
        permissions: RdpSessionPermissions,
    ) -> AppResult<String> {
        let pin = certificate_pin(&ticket.fingerprint)?;
        let (s, u) = self.unlocked().await?;
        let id = parse_id(&ticket.host_id)?;
        let (current, secret_id) = snapshot(&s, &u, id)?;
        ensure_current(&ticket, current)?;
        let password = match password {
            Some(password) => password,
            None => {
                let id = secret_id.ok_or_else(|| AppError::invalid("password", "required"))?;
                let secret = u.writer.read_secret(id).await?;
                if secret.kind != SecretKind::Password {
                    return Err(AppError::invalid("password", "invalid secret kind"));
                }
                SecretString::from(secret.value.expose_secret().to_owned())
            }
        };
        // The read above is asynchronous: reject a concurrent sync/edit before
        // any authentication bytes reach the previously confirmed server.
        ensure_current(&ticket, snapshot(&s, &u, id)?.0)?;
        u.rdp
            .connect_with_permissions(
                RdpConnectConfig {
                    address: ticket.address,
                    port: ticket.port,
                    username: ticket.username,
                    domain: (!ticket.domain.is_empty()).then_some(ticket.domain),
                    width: ticket.width,
                    height: ticket.height,
                    accepted_certificate_sha256: pin,
                },
                password,
                permissions,
            )
            .map_err(AppError::Rdp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppConfig, HostAuth, HostDto, HostProtocol};

    #[tokio::test]
    async fn confirmed_snapshot_binds_target_account_and_secret_revision() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppCore::new(AppConfig::for_tests(
            dir.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        let phrase = uuid::Uuid::new_v4().to_string() + " runtime test vault";
        app.create_local_profile("RDP snapshot".into(), phrase)
            .await
            .unwrap();
        let mut h = HostDto::new("Windows", "127.0.0.1");
        h.protocol = HostProtocol::Rdp;
        h.username = Some("operator".into());
        let h = app
            .save_host_with_auth(
                h,
                HostAuth::InlinePassword {
                    password: Some(uuid::Uuid::new_v4().to_string()),
                },
            )
            .await
            .unwrap();
        let (s, u) = app.unlocked().await.unwrap();
        let id = parse_id(&h.id).unwrap();
        let (mut ticket, secret) = snapshot(&s, &u, id).unwrap();
        assert_eq!(ticket.port, 3389);
        assert!(ticket.has_saved_password);
        assert!(secret.is_some());
        ticket.fingerprint = "00".repeat(32);
        ensure_current(&ticket, snapshot(&s, &u, id).unwrap().0).unwrap();
        let mut changed = ticket.clone();
        changed.address = "changed.example.test".into();
        assert!(ensure_current(&changed, snapshot(&s, &u, id).unwrap().0).is_err());
        let mut changed = ticket.clone();
        changed.username = "different".into();
        assert!(ensure_current(&changed, snapshot(&s, &u, id).unwrap().0).is_err());
        app.set_credential_password(h.credential_id.unwrap(), uuid::Uuid::new_v4().to_string())
            .await
            .unwrap();
        assert!(ensure_current(&ticket, snapshot(&s, &u, id).unwrap().0).is_err());
        app.shutdown().await.unwrap();
    }
}
