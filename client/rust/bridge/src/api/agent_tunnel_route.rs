//! Thin intent/opaque-handle adapter. Never accepts keys, grants or network URLs.
use crate::{api::error::BridgeError,state};
use std::sync::Arc;
#[flutter_rust_bridge::frb(opaque)]
pub struct AgentTunnelRouteHandle {original:Arc<cc_app_core::AgentTunnelRouteOperation>}
impl Drop for AgentTunnelRouteHandle {
    fn drop(&mut self){let _runtime=state::runtime().enter();self.original.release_ui();}
}
impl AgentTunnelRouteHandle {
    pub async fn detach_auto_open(&self)->Result<String,BridgeError>{let original=self.original.clone();
        state::run(async move{original.detach_auto_open_json().await.map_err(BridgeError::from)}).await}
    pub async fn details(&self)->Result<String,BridgeError>{let original=self.original.clone();
        state::run(async move{original.details_json().await.map_err(BridgeError::from)}).await}
    pub async fn approve_once(&self)->Result<String,BridgeError>{let original=self.original.clone();
        state::run(async move{original.approve_once().await.map_err(BridgeError::from)}).await}
    pub async fn stop(&self)->Result<String,BridgeError>{let original=self.original.clone();
        state::run(async move{original.stop().await.map_err(BridgeError::from)}).await}
    #[flutter_rust_bridge::frb(sync)]
    pub fn cancel(&self){let _runtime=state::runtime().enter();self.original.cancel();}
    pub async fn joined(&self)->Result<String,BridgeError>{let original=self.original.clone();
        state::run(async move{original.joined_json().await.map_err(BridgeError::from)}).await}
}
pub async fn agent_tunnel_route_prepare(request_json:String)->Result<AgentTunnelRouteHandle,BridgeError>{
    let original=state::core()?;
    state::run(async move{let issued=original.agent_tunnel_route_prepare_json(&request_json).await?;
        Ok(AgentTunnelRouteHandle{original:Arc::new(issued)})}).await
}
/// Existing original operation only. No implicit preparation, signing or retry.
pub async fn agent_tunnel_route_attach_saved(request_json:String)->Result<Option<AgentTunnelRouteHandle>,BridgeError>{
    let original=state::core()?;
    state::run(async move{original.agent_tunnel_route_attach_saved_json(&request_json).await
        .map(|issued|issued.map(|original|AgentTunnelRouteHandle{original:Arc::new(original)}))
        .map_err(BridgeError::from)}).await
}

/// Bounded encrypted LOCAL observations; no remote delivery/permission claim.
pub async fn agent_tunnel_access_audit(request_json:String)->Result<String,BridgeError>{
    let original=state::core()?;
    state::run(async move{original.agent_tunnel_access_audit_json(&request_json).await.map_err(BridgeError::from)}).await
}

/// LocalOnly destination intent. No SSH, Relay, grant, key or listener is opened.
pub async fn agent_tunnel_resume_preference(request_json:String)->Result<String,BridgeError>{
    let original=state::core()?;
    state::run(async move{original.agent_tunnel_resume_preference_json(&request_json).await.map_err(BridgeError::from)}).await
}
