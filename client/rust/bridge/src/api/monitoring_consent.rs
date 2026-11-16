//! Original one-use account-policy review; never a panel-writer capability.
use crate::{api::error::BridgeError, state};
use std::sync::Arc;

#[flutter_rust_bridge::frb(opaque)]
pub struct MonitoringSettingsConsentHandle {
    original: Arc<cc_app_core::MonitoringSettingsConsent>,
}

impl Drop for MonitoringSettingsConsentHandle {
    fn drop(&mut self) {
        let _runtime = state::runtime().enter();
        self.original.cancel();
    }
}

impl MonitoringSettingsConsentHandle {
    pub async fn details(&self) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move { original.details_json().await.map_err(BridgeError::from) }).await
    }

    pub async fn confirm(&self) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move { original.confirm().await.map_err(BridgeError::from) }).await
    }

    pub async fn recheck_checkpoint(&self) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move { original.recheck_checkpoint().await.map_err(BridgeError::from) }).await
    }

    #[flutter_rust_bridge::frb(sync)]
    pub fn cancel(&self) {
        let _runtime = state::runtime().enter();
        self.original.cancel();
    }
}

pub async fn monitoring_settings_prepare_consent(
    request_json: String,
) -> Result<MonitoringSettingsConsentHandle, BridgeError> {
    // Capture once. Methods on the issued handle never resolve a replacement Core.
    let original = state::core()?;
    state::run(async move {
        let review = original
            .monitoring_settings_prepare_consent_json(&request_json)
            .await?;
        Ok(MonitoringSettingsConsentHandle {
            original: Arc::new(review),
        })
    })
    .await
}
