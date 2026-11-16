//! Finite collector-service review. No interactive session or caller policy.
use crate::api::error::BridgeError;
use crate::state;
use std::sync::Arc;

#[flutter_rust_bridge::frb(opaque)]
pub struct AgentCollectorServiceHandle {
    original: Arc<cc_app_core::AgentCollectorServiceReview>,
}
impl Drop for AgentCollectorServiceHandle {
    fn drop(&mut self) {
        let _runtime = state::runtime().enter();
        self.original.cancel();
    }
}
impl AgentCollectorServiceHandle {
    pub async fn details(&self) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move { original.details_json().await.map_err(BridgeError::from) }).await
    }
    pub async fn confirm_configuration(&self) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move { original.confirm_configuration().await.map_err(BridgeError::from) }).await
    }
    #[flutter_rust_bridge::frb(sync)]
    pub fn cancel(&self) {
        let _runtime = state::runtime().enter();
        self.original.cancel();
    }
    pub async fn joined(&self) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move { original.joined_json().await.map_err(BridgeError::from) }).await
    }
}
pub async fn agent_collector_service_prepare(
    request_json: String,
) -> Result<AgentCollectorServiceHandle, BridgeError> {
    let original = state::core()?;
    state::run(async move {
        let issued = original.agent_collector_service_prepare_json(&request_json).await?;
        Ok(AgentCollectorServiceHandle { original: Arc::new(issued) })
    }).await
}
pub async fn agent_collector_service_catalog(request_json: String) -> Result<String, BridgeError> {
    let original = state::core()?;
    state::run(async move {
        original.agent_collector_service_catalog_json(&request_json).await.map_err(BridgeError::from)
    }).await
}
pub async fn agent_collector_service_status(request_json: String) -> Result<String, BridgeError> {
    let original = state::core()?;
    state::run(async move {
        original.agent_collector_service_status_json(&request_json).await.map_err(BridgeError::from)
    }).await
}

/// Original targeted service revocation; cancelling this proxy only aborts its
/// caller channel. Persistent service state comes exclusively from Native.
#[flutter_rust_bridge::frb(opaque)]
pub struct AgentCollectorServiceRevocationHandle {
    original: Arc<cc_app_core::AgentCollectorServiceRevocationReview>,
}
impl Drop for AgentCollectorServiceRevocationHandle {
    fn drop(&mut self) {
        let _runtime = state::runtime().enter();
        self.original.cancel();
    }
}
impl AgentCollectorServiceRevocationHandle {
    pub async fn details(&self) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move { original.details_json().await.map_err(BridgeError::from) }).await
    }
    pub async fn confirm_revocation(&self) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move { original.confirm_revocation().await.map_err(BridgeError::from) }).await
    }
    #[flutter_rust_bridge::frb(sync)]
    pub fn cancel(&self) {
        let _runtime = state::runtime().enter();
        self.original.cancel();
    }
    pub async fn joined(&self) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move { original.joined_json().await.map_err(BridgeError::from) }).await
    }
}
pub async fn agent_collector_service_revoke_prepare(
    request_json: String,
) -> Result<AgentCollectorServiceRevocationHandle, BridgeError> {
    let original = state::core()?;
    state::run(async move {
        let issued = original.agent_collector_service_revoke_prepare_json(&request_json).await?;
        Ok(AgentCollectorServiceRevocationHandle { original: Arc::new(issued) })
    }).await
}

pub async fn agent_collector_service_credential_choices(request_json: String) -> Result<String, BridgeError> {
    let original = state::core()?;
    state::run(async move {
        original.agent_collector_service_credential_choices_json(&request_json).await.map_err(BridgeError::from)
    }).await
}

/// Separate finite service-lifetime prepare. Returns the existing opaque handle;
/// confirm has no protocol flag and cannot upgrade a previously issued review.
pub async fn agent_collector_service_lifetime_prepare(
    request_json: String,
) -> Result<AgentCollectorServiceHandle, BridgeError> {
    let original=state::core()?;
    state::run(async move {
        let issued=original.agent_collector_service_lifetime_prepare_json(&request_json).await?;
        Ok(AgentCollectorServiceHandle {original:Arc::new(issued)})
    }).await
}
