//! Fixed five-frame service contract: probe, grant, recipient, two separate
//! encrypted credentials. Old enrollment/interactive/rotation limits unchanged.
use super::*;
impl Peer {
    pub(in crate::agent_tunnel::association) async fn open_service_observed_weak(
        core:&Weak<crate::app::Inner>,scope:Arc<OriginalScope>,host:ObjectId,revision:i64,
        observation:Observation)->AppResult<Self>{
        let mut peer=Self::open_observed_weak(core,scope,host,revision,observation).await?;
        peer.request_limit=5;Ok(peer)
    }
}
