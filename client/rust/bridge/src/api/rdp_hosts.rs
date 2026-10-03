//! Saved RDP hosts: no stored password crosses this interface.
use crate::api::{
    error::BridgeError,
    rdp::{RdpPermissions, RdpSessionInfo},
};
use crate::state::{self, with_core};
use cc_app_core::{AppError, RdpSavedHostTicket as CoreTicket, RdpSessionPermissions};
use zeroize::Zeroize;

#[derive(Debug, Clone)]
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
impl From<CoreTicket> for RdpSavedHostTicket {
    fn from(t: CoreTicket) -> Self {
        Self {
            host_id: t.host_id,
            name: t.name,
            address: t.address,
            port: t.port,
            username: t.username,
            domain: t.domain,
            width: t.width,
            height: t.height,
            fingerprint: t.fingerprint,
            snapshot_stamp: t.snapshot_stamp,
            has_saved_password: t.has_saved_password,
        }
    }
}
impl From<RdpSavedHostTicket> for CoreTicket {
    fn from(t: RdpSavedHostTicket) -> Self {
        Self {
            host_id: t.host_id,
            name: t.name,
            address: t.address,
            port: t.port,
            username: t.username,
            domain: t.domain,
            width: t.width,
            height: t.height,
            fingerprint: t.fingerprint,
            snapshot_stamp: t.snapshot_stamp,
            has_saved_password: t.has_saved_password,
        }
    }
}
fn error(e: AppError) -> BridgeError {
    match e {
        AppError::Rdp(e) => BridgeError::new(e.code(), e.to_string()),
        other => other.into(),
    }
}
pub async fn rdp_saved_host_probe(host_id: String) -> Result<RdpSavedHostTicket, BridgeError> {
    with_core(move |c| async move {
        c.rdp_saved_host_probe(host_id)
            .await
            .map(Into::into)
            .map_err(error)
    })
    .await
}
pub async fn rdp_saved_host_connect(
    ticket: RdpSavedHostTicket,
    password: Option<Vec<u8>>,
    permissions: RdpPermissions,
) -> Result<RdpSessionInfo, BridgeError> {
    let password = match password {
        None => None,
        Some(mut bytes) => {
            if bytes.is_empty() || bytes.len() > 4096 {
                bytes.zeroize();
                return Err(BridgeError::invalid("password", "invalid length"));
            }
            Some(cc_vault_core::SecretString::from(state::secret_string(
                "password", bytes,
            )?))
        }
    };
    with_core(move |c| async move {
        let width = ticket.width;
        let height = ticket.height;
        let id = c
            .rdp_saved_host_connect(
                ticket.into(),
                password,
                RdpSessionPermissions {
                    clipboard_enabled: permissions.clipboard_enabled,
                    directory_grant_id: permissions.directory_grant_id,
                    directory_writable: permissions.directory_writable,
                },
            )
            .await
            .map_err(error)?;
        Ok(RdpSessionInfo { id, width, height })
    })
    .await
}
