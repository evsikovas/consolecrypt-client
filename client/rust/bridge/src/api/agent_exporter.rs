//! Explicit finite exporter intent. Every issued call retains the original Arc.
use crate::{api::error::BridgeError, state};
use std::sync::Arc;

#[flutter_rust_bridge::frb(opaque)]
pub struct AgentExporterHandle { original: Arc<cc_app_core::AgentExporterOperation> }
impl Drop for AgentExporterHandle {
    fn drop(&mut self) { let _runtime=state::runtime().enter(); self.original.cancel(); }
}
impl AgentExporterHandle {
    pub async fn details(&self) -> Result<String, BridgeError> {
        let original=self.original.clone();
        state::run(async move {original.details_json().await.map_err(BridgeError::from)}).await
    }
    pub async fn approve_once(&self) -> Result<String, BridgeError> {
        let original=self.original.clone();
        state::run(async move {original.approve_once().await.map_err(BridgeError::from)}).await
    }
    pub async fn stop(&self) -> Result<String, BridgeError> {
        let original=self.original.clone();
        state::run(async move {original.stop().await.map_err(BridgeError::from)}).await
    }
    #[flutter_rust_bridge::frb(sync)]
    pub fn cancel(&self) { let _runtime=state::runtime().enter(); self.original.cancel(); }
    pub async fn joined(&self) -> Result<String, BridgeError> {
        let original=self.original.clone();
        state::run(async move {original.joined_json().await.map_err(BridgeError::from)}).await
    }
}
pub async fn agent_exporter_prepare(request_json: String) -> Result<AgentExporterHandle, BridgeError> {
    let original=state::core()?;
    state::run(async move {
        let issued=original.agent_exporter_prepare_json(&request_json).await?;
        Ok(AgentExporterHandle {original:Arc::new(issued)})
    }).await
}
pub async fn agent_exporter_catalog(request_json: String) -> Result<String, BridgeError> {
    let original=state::core()?;
    state::run(async move {original.agent_exporter_catalog_json(&request_json).await.map_err(BridgeError::from)}).await
}
