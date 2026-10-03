//! Remote desktop runtime and resource grants are scoped to an unlocked profile.
use crate::{
    AppCore, AppError, AppResult, RdpCertificateInfo, RdpConnectConfig, RdpInput, RdpPollResult,
};
use cc_rdp_core::{DirectoryGrant, SessionCapabilities, SessionPermissions};
use secrecy::SecretString;
use zeroize::Zeroizing;

impl AppCore {
    pub async fn rdp_probe_certificate(
        &self,
        address: String,
        port: u16,
    ) -> AppResult<RdpCertificateInfo> {
        let session = self.session().await?;
        let unlocked = session.unlocked().await?;
        unlocked
            .rdp
            .probe_certificate(&address, port)
            .await
            .map_err(AppError::Rdp)
    }

    pub async fn rdp_connect(
        &self,
        config: RdpConnectConfig,
        password: SecretString,
    ) -> AppResult<String> {
        let session = self.session().await?;
        let unlocked = session.unlocked().await?;
        unlocked
            .rdp
            .connect(config, password)
            .map_err(AppError::Rdp)
    }

    pub async fn rdp_connect_with_permissions(
        &self,
        config: RdpConnectConfig,
        password: SecretString,
        permissions: SessionPermissions,
    ) -> AppResult<String> {
        let session = self.session().await?;
        let unlocked = session.unlocked().await?;
        unlocked
            .rdp
            .connect_with_permissions(config, password, permissions)
            .map_err(AppError::Rdp)
    }
    pub async fn rdp_capabilities(&self) -> AppResult<SessionCapabilities> {
        let session = self.session().await?;
        let unlocked = session.unlocked().await?;
        Ok(unlocked.rdp.capabilities())
    }
    pub async fn rdp_pick_directory(&self) -> AppResult<Option<DirectoryGrant>> {
        let session = self.session().await?;
        let unlocked = session.unlocked().await?;
        unlocked.rdp.pick_directory().await.map_err(AppError::Rdp)
    }
    pub async fn rdp_release_directory_grant(&self, id: String) -> AppResult<()> {
        let session = self.session().await?;
        let unlocked = session.unlocked().await?;
        unlocked
            .rdp
            .release_directory_grant(&id)
            .map_err(AppError::Rdp)
    }
    pub async fn rdp_set_permissions(
        &self,
        id: String,
        permissions: SessionPermissions,
    ) -> AppResult<()> {
        let session = self.session().await?;
        let unlocked = session.unlocked().await?;
        unlocked
            .rdp
            .set_permissions(&id, permissions)
            .map_err(AppError::Rdp)
    }
    pub async fn rdp_permissions(&self, id: String) -> AppResult<SessionPermissions> {
        let session = self.session().await?;
        let unlocked = session.unlocked().await?;
        unlocked.rdp.permissions(&id).map_err(AppError::Rdp)
    }
    pub async fn rdp_offer_clipboard_text(
        &self,
        id: String,
        text: Zeroizing<String>,
    ) -> AppResult<()> {
        let session = self.session().await?;
        let unlocked = session.unlocked().await?;
        unlocked
            .rdp
            .offer_clipboard_text(&id, text.to_string())
            .map_err(AppError::Rdp)
    }
    pub async fn rdp_request_clipboard_text(&self, id: String) -> AppResult<()> {
        let session = self.session().await?;
        let unlocked = session.unlocked().await?;
        unlocked
            .rdp
            .request_clipboard_text(&id)
            .map_err(AppError::Rdp)
    }
    pub async fn rdp_take_clipboard_text(
        &self,
        id: String,
    ) -> AppResult<Option<Zeroizing<String>>> {
        let session = self.session().await?;
        let unlocked = session.unlocked().await?;
        unlocked.rdp.take_clipboard_text(&id).map_err(AppError::Rdp)
    }

    pub async fn rdp_poll(&self, id: String) -> AppResult<RdpPollResult> {
        let session = self.session().await?;
        let unlocked = session.unlocked().await?;
        unlocked.rdp.poll(&id).map_err(AppError::Rdp)
    }

    pub async fn rdp_send_input(&self, id: String, events: Vec<RdpInput>) -> AppResult<()> {
        let session = self.session().await?;
        let unlocked = session.unlocked().await?;
        unlocked.rdp.send_input(&id, events).map_err(AppError::Rdp)
    }

    pub async fn rdp_disconnect(&self, id: String) -> AppResult<()> {
        let session = self.session().await?;
        if let Some(unlocked) = session.unlocked_opt().await {
            let _ = unlocked.rdp.disconnect(&id);
        }
        Ok(())
    }
}
