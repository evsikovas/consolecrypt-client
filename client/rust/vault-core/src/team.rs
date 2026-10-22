//! Team vault keys (protocol 1.9): a team vault is an ordinary
//! [`UnlockedVault`] whose VRK reaches members through user envelopes
//! sealed to their per-team identity, never through a passphrase.
//!
//! Key epochs: a rotation creates a fresh VRK and seals the previous one
//! under it (chain link), so a member holding only the newest envelope can
//! still read revisions written before the rotation.

use crate::error::Result;
use crate::unlock::{envelope_for, EnvelopeSource};
use crate::unlocked::UnlockedVault;
use cc_crypto_core::teams::{
    open_key_chain_link, seal_key_chain_link, seal_user_envelope, TeamIdentity,
};
use cc_crypto_core::Vrk;
use cc_protocol::envelopes::NewEnvelope;
use cc_protocol::teams::{TeamKeyChainLink, TeamMemberKeys};
use cc_protocol::{UserId, VaultId};

impl UnlockedVault {
    /// A new team vault with a fresh VRK (epoch 0).
    pub fn new_team(vault_id: VaultId) -> Result<Self> {
        Ok(Self::from_vrk(vault_id, Vrk::generate()?))
    }

    /// Open a team vault from this member's user envelope.
    pub fn open_team(
        identity: &TeamIdentity,
        envelope: &(impl EnvelopeSource + ?Sized),
        vault_id: VaultId,
        user_id: UserId,
    ) -> Result<Self> {
        let envelope = envelope_for(envelope, vault_id)?;
        let vrk = identity.open_user_envelope(&envelope, vault_id, user_id)?;
        Ok(Self::from_vrk(vault_id, vrk))
    }

    /// User envelope of this VRK for `member` (keys from a verified log).
    pub fn seal_for_member(&self, member: &TeamMemberKeys) -> Result<NewEnvelope> {
        Ok(seal_user_envelope(self.vrk(), self.vault_id(), member)?)
    }

    /// Key rotation: the vault of epoch `key_epoch` plus the link sealing
    /// this (previous) VRK under the new one.
    pub fn rotate_team(&self, key_epoch: u32) -> Result<(Self, TeamKeyChainLink)> {
        let next = Self::new_team(self.vault_id())?;
        let link = seal_key_chain_link(next.vrk(), self.vrk(), self.vault_id(), key_epoch)?;
        Ok((next, link))
    }

    /// The previous epoch's vault from a chain link sealed under this VRK.
    pub fn open_previous_epoch(&self, link: &TeamKeyChainLink) -> Result<Self> {
        let vrk = open_key_chain_link(self.vrk(), link, self.vault_id())?;
        Ok(Self::from_vrk(self.vault_id(), vrk))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_models::host::Host;
    use cc_models::{ObjectPayload, VaultObject};

    #[test]
    fn member_envelope_and_epoch_chain() {
        let vault_id = VaultId::new();
        let user = UserId::new();
        let identity = TeamIdentity::generate().unwrap();
        let v0 = UnlockedVault::new_team(vault_id).unwrap();
        let env = v0.seal_for_member(&identity.member_keys(user)).unwrap();
        let opened = UnlockedVault::open_team(&identity, &env, vault_id, user).unwrap();
        assert_eq!(opened.vault_access_key(), v0.vault_access_key());
        // Someone else's identity cannot open it.
        let other = TeamIdentity::generate().unwrap();
        assert!(UnlockedVault::open_team(&other, &env, vault_id, user).is_err());

        let h = Host::new("h", "192.0.2.1");
        let p = ObjectPayload::new(VaultObject::Host(h.clone()));
        let old_body = v0.encrypt_object(h.id, 1, &p).unwrap();
        let (v1, link) = v0.rotate_team(1).unwrap();
        assert_ne!(v1.vault_access_key(), v0.vault_access_key());
        assert!(v1.decrypt_object(h.id, 1, &old_body, None).is_err());
        let back = v1.open_previous_epoch(&link).unwrap();
        assert_eq!(back.decrypt_object(h.id, 1, &old_body, None).unwrap().0, p);
        // The old key does not open what the new epoch writes.
        let new_body = v1.encrypt_object(h.id, 2, &p).unwrap();
        assert!(v0.decrypt_object(h.id, 2, &new_body, None).is_err());
    }
}
