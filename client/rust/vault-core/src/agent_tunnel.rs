//! Already-loaded original identity only. No load/create/save/export path.
use crate::DeviceIdentity;
use cc_agent_node_wire::{Error, Now, PreparedTunnelOwner, Result, TunnelOwnerSigned};
impl DeviceIdentity {
    pub fn sign_agent_tunnel_owner_v1(
        &self,
        prepared: PreparedTunnelOwner,
        now: Now,
    ) -> Result<TunnelOwnerSigned> {
        prepared.recheck(now)?;
        if *prepared.expected_key().as_bytes() != self.public_keys().signing {
            return Err(Error::WrongClaim);
        }
        if prepared
            .expected_device_id()
            .is_some_and(|id| id != self.device_id().to_string())
        {
            return Err(Error::WrongClaim);
        }
        self.secret_keys().sign_agent_tunnel_owner_v1(prepared, now)
    }
}
