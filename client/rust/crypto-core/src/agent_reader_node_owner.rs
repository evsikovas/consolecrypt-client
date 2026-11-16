//! Closed existing Node enrollment domains, using the actual device key.
use crate::device::DeviceSecretKeys;
use cc_agent_node_wire::{Error, Now, PreparedReaderNodeOwner, ReaderNodeOwnerSigned, Result};

impl DeviceSecretKeys {
    pub fn sign_agent_reader_node_owner_v1(
        &self,
        prepared: PreparedReaderNodeOwner,
        now: Now,
    ) -> Result<ReaderNodeOwnerSigned> {
        prepared.recheck(now)?;
        if *prepared.expected_key().as_bytes() != self.public_keys().signing {
            return Err(Error::WrongClaim);
        }
        let signature = self.sign_message(&prepared.signing_transcript()?);
        prepared.finish(signature)
    }
}
