//! Lifetime-only fixed cap and actual Peer identity guard. Existing v1 cap5 stays.
use super::*;
impl Peer {
    pub(in crate::agent_tunnel::association) async fn open_lifetime_service_observed_weak(core: &Weak<crate::app::Inner>,
        scope: Arc<OriginalScope>, host: ObjectId, revision: i64, observation: Observation) -> AppResult<Self> {
        let mut peer = Self::open_service_observed_weak(core, scope, host, revision, observation).await?;
        peer.request_limit = 7;
        peer.lifetime_protocol = true;
        Ok(peer)
    }
    /// Called exactly after the actual checked probe on this original channel.
    pub(in crate::agent_tunnel::association) fn bind_checked_service_node(&mut self, original: &Arc<OriginalScope>, node: &protocol::NodeIdentity) -> AppResult<()> {
        if !Arc::ptr_eq(&self.scope, original) || self.authenticated_service_node.is_some()
            || node.public_key.fingerprint() != node.key_fingerprint { return Err(unavailable()); }
        self.scope.check_current()?;
        self.authenticated_service_node = Some(node.clone());
        Ok(())
    }
    pub(in crate::agent_tunnel::association) fn require_original_lifetime_service(&self, original: &Arc<OriginalScope>,
        host: ObjectId, revision: i64, route: &str, node: &protocol::NodeIdentity) -> AppResult<()> {
        self.scope.check_current()?;
        if !self.lifetime_protocol || self.request_limit != 7 || !Arc::ptr_eq(&self.scope, original)
            || self.host_id != host || self.host_revision != revision || self.route_digest != route
            || self.authenticated_service_node.as_ref() != Some(node) { return Err(unavailable()); }
        Ok(())
    }
}
