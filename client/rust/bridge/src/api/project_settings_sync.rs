//! One-use approval for the original account and unlocked session.
use crate::{api::error::BridgeError, state};
use std::sync::Arc;

#[flutter_rust_bridge::frb(opaque)]
pub struct ProjectSettingsSyncReview {
    original: Arc<cc_app_core::ProjectSettingsSyncConsent>,
}

impl Drop for ProjectSettingsSyncReview {
    fn drop(&mut self) {
        let _runtime = state::runtime().enter();
        self.original.cancel();
    }
}

impl ProjectSettingsSyncReview {
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

pub async fn project_settings_sync_prepare(
    request_json: String,
) -> Result<ProjectSettingsSyncReview, BridgeError> {
    // Capture once. Methods on the issued handle never resolve a replacement Core.
    let original = state::core()?;
    state::run(async move {
        let review = original
            .settings_sync_prepare_consent_json(&request_json)
            .await?;
        Ok(ProjectSettingsSyncReview {
            original: Arc::new(review),
        })
    })
    .await
}
