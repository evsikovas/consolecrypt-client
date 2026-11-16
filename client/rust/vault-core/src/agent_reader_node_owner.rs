//! Already-loaded original DeviceIdentity only: no load/save/create/export.
use crate::DeviceIdentity;
use cc_agent_node_wire::{Error, Now, PreparedReaderNodeOwner, ReaderNodeOwnerSigned, Result};

impl DeviceIdentity {
    pub fn sign_agent_reader_node_owner_v1(
        &self,
        prepared: PreparedReaderNodeOwner,
        now: Now,
    ) -> Result<ReaderNodeOwnerSigned> {
        prepared.recheck(now)?;
        if *prepared.expected_key().as_bytes() != self.public_keys().signing {
            return Err(Error::WrongClaim);
        }
        self.secret_keys().sign_agent_reader_node_owner_v1(prepared, now)
    }
}
