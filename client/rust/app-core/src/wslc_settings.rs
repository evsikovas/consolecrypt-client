//! WSLc templates may contain environment secrets: store them only in the
//! active profile's SQLCipher database, without sync or plaintext side files.
use crate::{AppCore, AppError, AppResult};
const SETTINGS: &str = "containers.wslc.settings.v1";
impl AppCore {
    pub async fn wslc_settings_load(&self, expected_profile: &str) -> AppResult<serde_json::Value> {
        let (owner, _) = self.unlocked().await?;
        if owner.profile_id.to_string() != expected_profile {
            return Err(AppError::Cancelled);
        }
        Ok(owner
            .storage()?
            .setting_get(SETTINGS)
            .await?
            .unwrap_or(serde_json::Value::Null))
    }
    pub async fn wslc_settings_save(
        &self,
        expected_profile: &str,
        value: serde_json::Value,
    ) -> AppResult<()> {
        if !value.is_object()
            || serde_json::to_vec(&value)
                .map_err(|_| AppError::invalid("wslc", "settings_encoding"))?
                .len()
                > 4 * 1024 * 1024
        {
            return Err(AppError::invalid("wslc", "settings_limit"));
        }
        let (owner, _) = self.unlocked().await?;
        if owner.profile_id.to_string() != expected_profile {
            return Err(AppError::Cancelled);
        }
        owner.storage()?.setting_set(SETTINGS, value).await?;
        Ok(())
    }
}
