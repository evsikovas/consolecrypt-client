//! Explicit intent plus an opaque issued handle; no caller grant or process API.
use crate::{api::error::BridgeError, state};
use std::sync::Arc;

#[flutter_rust_bridge::frb(opaque)]
pub struct AgentCollectorHandle { original: Arc<cc_app_core::AgentCollectorOperation> }
impl Drop for AgentCollectorHandle {
    fn drop(&mut self) { let _runtime = state::runtime().enter(); self.original.cancel(); }
}
impl AgentCollectorHandle {
    pub async fn details(&self) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move { original.details_json().await.map_err(BridgeError::from) }).await
    }
    pub async fn approve_once(&self) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move { original.approve_once().await.map_err(BridgeError::from) }).await
    }
    pub async fn stop(&self) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move { original.stop().await.map_err(BridgeError::from) }).await
    }
    #[flutter_rust_bridge::frb(sync)]
    pub fn cancel(&self) { let _runtime = state::runtime().enter(); self.original.cancel(); }
    pub async fn joined(&self) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move { original.joined_json().await.map_err(BridgeError::from) }).await
    }
}
pub async fn agent_collector_prepare(request_json: String) -> Result<AgentCollectorHandle, BridgeError> {
    let original = state::core()?;
    // The sole local Core is moved into the consuming factory, which drops it
    // immediately after original capture, before SQL/SSH/Actor/unit Pending.
    state::run(async move {
        let issued = original.agent_collector_prepare_json(&request_json).await?;
        Ok(AgentCollectorHandle { original: Arc::new(issued) })
    }).await
}
