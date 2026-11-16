//! Closed owner purpose, with the actual existing DeviceIdentity custody.
use crate::DeviceIdentity;
use cc_agent_node_wire::{CollectorServiceCredentialAuthorization, Error, Now, Result, Signed};
use cc_crypto_core::PreparedCollectorServiceCredentialOwner;
impl DeviceIdentity {
    pub fn sign_collector_service_credential_authorization_v1(
        &self, prepared: PreparedCollectorServiceCredentialOwner, now: Now,
    ) -> Result<Signed<CollectorServiceCredentialAuthorization>> {
        let expected = serde_json::to_value(prepared.owner_device_id()).map_err(|_| Error::InvalidWire)?;
        if expected.as_str() != Some(self.device_id().to_string().as_str()) {
            return Err(Error::WrongClaim);
        }
        self.secret_keys().sign_collector_service_credential_authorization_v1(prepared, now)
    }
}
