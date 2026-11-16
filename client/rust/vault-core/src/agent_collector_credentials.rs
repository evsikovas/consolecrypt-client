//! One closed DeviceIdentity purpose; no raw-key or caller byte signing port.
use crate::DeviceIdentity;
use cc_agent_node_wire::{CollectorCredentialScope, Error, Now, Result, Signed};
use cc_crypto_core::PreparedCollectorCredentialOwner;

impl DeviceIdentity {
    pub fn sign_agent_collector_credential_scope_v1(
        &self, prepared: PreparedCollectorCredentialOwner, now: Now,
    ) -> Result<Signed<CollectorCredentialScope>> {
        let actual = self.device_id().to_string();
        let expected = serde_json::to_value(prepared.owner_device_id()).map_err(|_| Error::InvalidWire)?;
        if expected.as_str() != Some(actual.as_str()) { return Err(Error::WrongClaim); }
        self.secret_keys().sign_agent_collector_credential_scope_v1(prepared, now)
    }
}
