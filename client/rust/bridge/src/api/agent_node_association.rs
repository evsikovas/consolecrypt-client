//! Fixed original-owner Agent association. JSON is intent/metadata only;
//! issued handles never recapture the global Core, profile or credentials.
use crate::{api::error::BridgeError, state};
use std::sync::Arc;

#[flutter_rust_bridge::frb(opaque)]
pub struct AgentNodeAssociationHandle {
    original: Arc<cc_app_core::NodeAssociationOperation>,
}

impl Drop for AgentNodeAssociationHandle {
    fn drop(&mut self) {
        let _runtime = state::runtime().enter();
        self.original.cancel();
    }
}

impl AgentNodeAssociationHandle {
    pub async fn details(&self) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move { original.details_json().await.map_err(BridgeError::from) }).await
    }

    pub async fn probe(&self) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move { original.probe().await.map_err(BridgeError::from) }).await
    }

    pub async fn approve_offer(&self) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move { original.approve_offer().await.map_err(BridgeError::from) }).await
    }

    pub async fn approve_certificate(&self, lifetime_seconds: u32) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move {
            original
                .approve_certificate(lifetime_seconds)
                .await
                .map_err(BridgeError::from)
        })
        .await
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

#[flutter_rust_bridge::frb(opaque)]
pub struct AgentNodeClientIdentityHandle {
    original: Arc<cc_app_core::NodeClientIdentityReview>,
}

impl Drop for AgentNodeClientIdentityHandle {
    fn drop(&mut self) {
        let _runtime = state::runtime().enter();
        self.original.cancel();
    }
}

impl AgentNodeClientIdentityHandle {
    pub async fn details(&self) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move { original.details_json().await.map_err(BridgeError::from) }).await
    }

    pub async fn confirm_creation(&self) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move { original.confirm_creation().await.map_err(BridgeError::from) })
            .await
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

pub async fn agent_node_association_status(request_json: String) -> Result<String, BridgeError> {
    let original = state::core()?;
    state::run(async move {
        original
            .agent_node_association_status_json(&request_json)
            .await
            .map_err(BridgeError::from)
    })
    .await
}

pub async fn agent_node_association_register(
    request_json: String,
) -> Result<AgentNodeAssociationHandle, BridgeError> {
    let original = state::core()?;
    state::run(async move {
        let operation = original
            .agent_node_association_register_json(&request_json)
            .await?;
        Ok(AgentNodeAssociationHandle {
            original: Arc::new(operation),
        })
    })
    .await
}

pub async fn agent_node_client_identity_prepare(
    request_json: String,
) -> Result<AgentNodeClientIdentityHandle, BridgeError> {
    let original = state::core()?;
    state::run(async move {
        let review = original
            .agent_node_client_identity_prepare_json(&request_json)
            .await?;
        Ok(AgentNodeClientIdentityHandle {
            original: Arc::new(review),
        })
    })
    .await
}

/// One explicit original-owner revocation. No caller-supplied trust or key.
#[flutter_rust_bridge::frb(opaque)]
pub struct AgentNodeRevocationHandle {
    original: Arc<cc_app_core::NodeRevocationReview>,
}
impl Drop for AgentNodeRevocationHandle {
    fn drop(&mut self) { let _runtime = state::runtime().enter(); self.original.cancel(); }
}
impl AgentNodeRevocationHandle {
    pub async fn details(&self) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move {original.details_json().await.map_err(BridgeError::from)}).await
    }
    pub async fn confirm_once(&self) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move {original.confirm_once().await.map_err(BridgeError::from)}).await
    }
    #[flutter_rust_bridge::frb(sync)]
    pub fn cancel(&self) { let _runtime = state::runtime().enter(); self.original.cancel(); }
    pub async fn joined(&self) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move {original.joined_json().await.map_err(BridgeError::from)}).await
    }
}
pub async fn agent_node_association_revoke_prepare(request_json: String)
    -> Result<AgentNodeRevocationHandle, BridgeError> {
    let original = state::core()?;
    state::run(async move {
        let review = original.agent_node_association_revoke_prepare_json(&request_json).await?;
        Ok(AgentNodeRevocationHandle {original:Arc::new(review)})
    }).await
}

#[flutter_rust_bridge::frb(opaque)]
pub struct AgentNodeRotationHandle {original:Arc<cc_app_core::NodeRotationReview>}
impl Drop for AgentNodeRotationHandle {fn drop(&mut self){let _runtime=state::runtime().enter();self.original.cancel();}}
impl AgentNodeRotationHandle {
    pub async fn details(&self)->Result<String,BridgeError>{let original=self.original.clone();
        state::run(async move{original.details_json().await.map_err(BridgeError::from)}).await}
    pub async fn prepare_candidate_once(&self)->Result<String,BridgeError>{let original=self.original.clone();
        state::run(async move{original.prepare_candidate_once().await.map_err(BridgeError::from)}).await}
    pub async fn confirm_once(&self)->Result<String,BridgeError>{let original=self.original.clone();
        state::run(async move{original.confirm_once().await.map_err(BridgeError::from)}).await}
    #[flutter_rust_bridge::frb(sync)]
    pub fn cancel(&self){let _runtime=state::runtime().enter();self.original.cancel();}
    pub async fn joined(&self)->Result<String,BridgeError>{let original=self.original.clone();
        state::run(async move{original.joined_json().await.map_err(BridgeError::from)}).await}
}
pub async fn agent_node_association_rotation_prepare(request_json:String)->Result<AgentNodeRotationHandle,BridgeError>{
    let original=state::core()?;state::run(async move{let review=original.agent_node_association_rotation_prepare_json(&request_json).await?;
        Ok(AgentNodeRotationHandle{original:Arc::new(review)})}).await
}
pub async fn agent_node_association_rotation_reconcile_prepare(request_json:String)->Result<AgentNodeRotationHandle,BridgeError>{
    let original=state::core()?;state::run(async move{let review=original.agent_node_association_rotation_reconcile_prepare_json(&request_json).await?;
        Ok(AgentNodeRotationHandle{original:Arc::new(review)})}).await
}

/// A distinct explicit MetricsOnly enrollment; never upgrades a Tunnel node.
pub async fn agent_metrics_node_association_register(request_json: String) -> Result<AgentNodeAssociationHandle, BridgeError> {
    let original = state::core()?;
    state::run(async move {
        let issued = original.agent_metrics_node_association_register_json(&request_json).await?;
        Ok(AgentNodeAssociationHandle { original: Arc::new(issued) })
    }).await
}
/// Same original owner revokes a MetricsOnly certificate even if its runtime
/// or collector catalog is unavailable. No TLS private key is loaded.
pub async fn agent_metrics_node_association_revoke_prepare(request_json: String) -> Result<AgentNodeRevocationHandle, BridgeError> {
    let original = state::core()?;
    state::run(async move {
        let issued = original.agent_metrics_node_association_revoke_prepare_json(&request_json).await?;
        Ok(AgentNodeRevocationHandle { original: Arc::new(issued) })
    }).await
}

/// Separate explicit carrier-registration review; never a Node/Actor grant.
#[flutter_rust_bridge::frb(opaque)]
pub struct AgentTunnelControlHandle {original:Arc<cc_app_core::NodeTunnelControlReview>}
impl Drop for AgentTunnelControlHandle {fn drop(&mut self){let _runtime=state::runtime().enter();self.original.cancel();}}
impl AgentTunnelControlHandle {
    pub async fn details(&self)->Result<String,BridgeError>{let original=self.original.clone();
        state::run(async move{original.details_json().await.map_err(BridgeError::from)}).await}
    pub async fn approve_once(&self)->Result<String,BridgeError>{let original=self.original.clone();
        state::run(async move{original.approve_once().await.map_err(BridgeError::from)}).await}
    #[flutter_rust_bridge::frb(sync)]
    pub fn cancel(&self){let _runtime=state::runtime().enter();self.original.cancel();}
    pub async fn joined(&self)->Result<String,BridgeError>{let original=self.original.clone();
        state::run(async move{original.joined_json().await.map_err(BridgeError::from)}).await}
}
pub async fn agent_tunnel_control_bootstrap_prepare(request_json:String)->Result<AgentTunnelControlHandle,BridgeError>{
    let original=state::core()?;
    state::run(async move{let review=original.agent_tunnel_control_bootstrap_prepare_json(&request_json).await?;
        Ok(AgentTunnelControlHandle{original:Arc::new(review)})}).await
}

/// A distinct explicit ExporterOnly enrollment; never upgrades an existing Tunnel/Metrics node.
pub async fn agent_exporter_node_association_register(request_json: String) -> Result<AgentNodeAssociationHandle, BridgeError> {
    let original = state::core()?;
    state::run(async move {
        let issued = original.agent_exporter_node_association_register_json(&request_json).await?;
        Ok(AgentNodeAssociationHandle { original: Arc::new(issued) })
    }).await
}
/// Same original owner revokes a ExporterOnly certificate even if its runtime
/// or collector catalog is unavailable. No TLS private key is loaded.
pub async fn agent_exporter_node_association_revoke_prepare(request_json: String) -> Result<AgentNodeRevocationHandle, BridgeError> {
    let original = state::core()?;
    state::run(async move {
        let issued = original.agent_exporter_node_association_revoke_prepare_json(&request_json).await?;
        Ok(AgentNodeRevocationHandle { original: Arc::new(issued) })
    }).await
}
