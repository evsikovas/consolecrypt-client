use super::wire::{canonical, live};
use super::*;
use serde::{Deserialize, Serialize};

/// Frozen public expectation supplied by the Native approval/head owner.
/// Neither this value nor its constructor is approval, a Permit or an ack.
pub struct AgentReaderExpectedV1 {
    pub issuer_pin: AgentReaderIssuerPinV1,
    pub current_head: AgentReaderHeadV1,
    pub association_body: AgentReaderAssociationV1,
    pub target: AgentReaderTargetDescriptorV1,
}
impl std::fmt::Debug for AgentReaderExpectedV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AgentReaderExpectedV1([redacted])")
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentReaderSigningPacketV1 {
    pub association: AgentReaderSigned<AgentReaderAssociationV1>,
    pub grant: AgentReaderSigned<AgentReaderGrantV1>,
    pub freshness: AgentReaderSigned<AgentReaderFreshnessV1>,
    pub challenge: AgentReaderChallengeV1,
}
impl std::fmt::Debug for AgentReaderSigningPacketV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AgentReaderSigningPacketV1([redacted])")
    }
}

/// Cryptographic/structural validation only. AppCore retains approval, original
/// session/identity, current-head delivery, one-action and transport obligations.
pub struct AgentReaderValidatedInputsV1 {
    pub(super) expected: AgentReaderExpectedV1,
    pub(super) packet: AgentReaderSigningPacketV1,
}
impl std::fmt::Debug for AgentReaderValidatedInputsV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AgentReaderValidatedInputsV1([redacted])")
    }
}
impl AgentReaderValidatedInputsV1 {
    pub fn from_packet(
        bytes: &[u8],
        expected: AgentReaderExpectedV1,
        now: cc_protocol::Timestamp,
    ) -> AgentReaderResult<Self> {
        let packet = wire::strict_json(bytes, AGENT_READER_FRAME_BYTES)?;
        let result = Self { expected, packet };
        result.recheck(AgentReaderTimestamp::from_native(now)?)?;
        Ok(result)
    }
    pub(super) fn recheck(&self, now: AgentReaderTimestamp) -> AgentReaderResult<()> {
        let e = &self.expected;
        let pin = &e.issuer_pin;
        let p = &self.packet;
        pin.validate()?;
        e.target.validate()?;
        p.association.verify(pin.issuer_key)?;
        p.grant.verify(pin.issuer_key)?;
        p.freshness.verify(pin.issuer_key)?;
        p.challenge.validate()?;
        let a = &p.association.body;
        let g = &p.grant.body;
        let c = &p.challenge;
        let f = &p.freshness.body;
        if a != &e.association_body {
            return Err(AgentReaderError::WrongAssociation);
        }
        if a.context != pin.context
            || a.issuer_id != pin.issuer_id
            || a.issuer_epoch != pin.issuer_epoch
            || a.native_profile_id != pin.native_profile_id
            || a.audience_id != pin.audience_id
            || a.node_trust_origin_hash != pin.node_trust_origin_hash
        {
            return Err(AgentReaderError::WrongContext);
        }
        if a.device_key == pin.issuer_key {
            return Err(AgentReaderError::WrongDevice);
        }
        if a.agent_id != e.target.agent_id
            || a.operation != e.target.operation()
            || a.target_descriptor_hash != e.target.descriptor_hash()?
            || a.dataset_kind != e.target.dataset.kind
            || a.dataset_schema_version != e.target.dataset.schema_version
        {
            return Err(AgentReaderError::WrongTarget);
        }
        let head = &e.current_head;
        if head.context != pin.context
            || head.issuer_id != pin.issuer_id
            || head.issuer_epoch != pin.issuer_epoch
        {
            return Err(AgentReaderError::WrongIssuer);
        }
        // V2: association installation revision may be older, never equated
        // with the current signed Grant/Challenge/Freshness/Proof head.
        if &g.head != head
            || &c.head != head
            || &f.head != head
            || a.authority_revision > head.authority_revision
        {
            return Err(AgentReaderError::WrongHead);
        }
        let association_hash = a.body_digest()?;
        if g.association_hash != association_hash
            || c.association_hash != association_hash
            || g.challenge_id != c.challenge_id
            || g.challenge_hash != c.challenge_hash()?
            || g.operation_id != c.operation_id
        {
            return Err(AgentReaderError::WrongAssociation);
        }
        if g.device_id != a.device_id
            || g.device_epoch != a.device_epoch
            || c.device_id != a.device_id
            || c.device_epoch != a.device_epoch
            || c.device_key_fingerprint != agent_reader_key_fingerprint(a.device_key)
        {
            return Err(AgentReaderError::WrongDevice);
        }
        if g.agent_id != a.agent_id
            || c.agent_id != a.agent_id
            || c.agent_certificate_hash != a.agent_certificate_hash
            || c.agent_key_epoch != a.agent_key_epoch
            || c.native_profile_id != a.native_profile_id
            || c.native_binding_id != a.native_binding_id
            || c.native_binding_revision != a.native_binding_revision
            || g.operation != a.operation
            || c.operation != a.operation
            || g.target_descriptor_hash != a.target_descriptor_hash
            || c.target_descriptor_hash != a.target_descriptor_hash
        {
            return Err(AgentReaderError::WrongTarget);
        }
        live(AgentReaderTimestamp::new(0)?, pin.expires_at, now)?;
        live(a.not_before, a.expires_at, now)?;
        live(c.issued_at, c.expires_at, now)?;
        live(g.issued_at, g.expires_at, now)?;
        live(f.issued_at, f.expires_at, now)?;
        if a.expires_at > pin.expires_at
            || f.expires_at > pin.expires_at
            || c.issued_at < a.not_before
            || c.expires_at > a.expires_at
            || c.expires_at > f.expires_at
            || g.issued_at < c.issued_at
            || g.expires_at > c.expires_at
            || g.expires_at > a.expires_at
            || g.expires_at > f.expires_at
            || g.expires_at > pin.expires_at
        {
            return Err(AgentReaderError::InvalidTime);
        }
        // Bound detached wire values before they can reach a signer.
        if canonical(&p.association)?.len()
            + canonical(&p.grant)?.len()
            + canonical(&p.freshness)?.len()
            > AGENT_READER_FRAME_BYTES
        {
            return Err(AgentReaderError::Quota);
        }
        Ok(())
    }
}
