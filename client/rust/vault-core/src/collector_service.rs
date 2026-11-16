//! Two closed service purposes through the existing original Device identity.
//! No caller byte signing, key export or replacement identity lookup.
use crate::DeviceIdentity;
use cc_agent_node_wire::{CollectorServiceOwnerSigned,Now,PreparedCollectorServiceOwner,Result};
impl DeviceIdentity {
    pub fn sign_collector_service_owner_v1(&self,prepared:PreparedCollectorServiceOwner,now:Now)
        ->Result<CollectorServiceOwnerSigned> {
        self.secret_keys().sign_collector_service_owner_v1(prepared,now)
    }
}
