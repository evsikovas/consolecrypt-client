//! Existing Node purpose signatures using the actual DeviceSecretKeys.
//! No Reader issuer, generic byte signer or independent tunnel key registry.
use crate::device::DeviceSecretKeys;
use cc_agent_node_wire::{Error, Now, PreparedTunnelOwner, Result, TunnelOwnerSigned};
impl DeviceSecretKeys {
    pub fn sign_agent_tunnel_owner_v1(
        &self,
        prepared: PreparedTunnelOwner,
        now: Now,
    ) -> Result<TunnelOwnerSigned> {
        prepared.recheck(now)?;
        if *prepared.expected_key().as_bytes() != self.public_keys().signing {
            return Err(Error::WrongClaim);
        }
        let signatures = prepared
            .signing_transcripts()?
            .iter()
            .map(|message| self.sign_message(message))
            .collect();
        prepared.finish(signatures)
    }
}
