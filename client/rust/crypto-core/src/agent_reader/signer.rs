use super::*;
use crate::device::DeviceSecretKeys;
use zeroize::Zeroizing;

/// Not serializable/cloneable; consumes a single prepared proof value. This is
/// not a durable replay guard or evidence of Native user approval.
pub struct AgentReaderPreparedProofV1 {
    expected_key: AgentReaderPublicKey,
    proof: AgentReaderProofV1,
}
impl std::fmt::Debug for AgentReaderPreparedProofV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AgentReaderPreparedProofV1([redacted])")
    }
}
impl AgentReaderValidatedInputsV1 {
    pub fn prepare_for_device_once(
        self,
        device_id: cc_protocol::DeviceId,
        public_key: [u8; 32],
        now: cc_protocol::Timestamp,
    ) -> AgentReaderResult<AgentReaderPreparedProofV1> {
        self.prepare_inner(device_id, public_key, now, &mut |nonce| {
            crate::rng::fill_random(nonce).map_err(|_| AgentReaderError::Entropy)
        })
    }
    fn prepare_inner(
        self,
        device_id: cc_protocol::DeviceId,
        public_key: [u8; 32],
        now: cc_protocol::Timestamp,
        entropy: &mut impl FnMut(&mut [u8]) -> AgentReaderResult<()>,
    ) -> AgentReaderResult<AgentReaderPreparedProofV1> {
        let now = AgentReaderTimestamp::from_native(now)?;
        self.recheck(now)?;
        let id = AgentReaderDeviceId::from_device_id(device_id)?;
        let key = AgentReaderPublicKey::from_bytes(public_key);
        key.validate()?;
        let a = &self.packet.association.body;
        let g = &self.packet.grant.body;
        if a.device_id != id || a.device_key != key || a.device_epoch != 1 {
            return Err(AgentReaderError::WrongDevice);
        }
        let mut nonce = Zeroizing::new([0u8; 32]);
        entropy(nonce.as_mut())?;
        if *nonce == [0; 32] {
            return Err(AgentReaderError::Entropy);
        }
        let proof = AgentReaderProofV1 {
            version: 1,
            head: g.head.clone(),
            operation_id: g.operation_id,
            device_id: id,
            device_epoch: 1,
            association_hash: a.body_digest()?,
            grant_hash: g.body_digest()?,
            challenge_hash: g.challenge_hash,
            nonce: AgentReaderHash::from_bytes(*nonce),
            issued_at: now,
        };
        proof.validate()?;
        Ok(AgentReaderPreparedProofV1 {
            expected_key: key,
            proof,
        })
    }
    #[cfg(test)]
    pub(super) fn prepare_with_test_entropy(
        self,
        device_id: cc_protocol::DeviceId,
        public_key: [u8; 32],
        now: cc_protocol::Timestamp,
        entropy: &mut impl FnMut(&mut [u8]) -> AgentReaderResult<()>,
    ) -> AgentReaderResult<AgentReaderPreparedProofV1> {
        self.prepare_inner(device_id, public_key, now, entropy)
    }
}
impl DeviceSecretKeys {
    /// Only the closed ReaderProof domain; no account/automation/raw signer.
    pub fn sign_agent_reader_proof_v1(
        &self,
        prepared: AgentReaderPreparedProofV1,
    ) -> AgentReaderResult<AgentReaderSigned<AgentReaderProofV1>> {
        if self.public_keys().signing != *prepared.expected_key.as_bytes() {
            return Err(AgentReaderError::WrongDevice);
        }
        let signature = self.sign_message(&prepared.proof.signing_bytes()?);
        Ok(AgentReaderSigned {
            body: prepared.proof,
            signature: AgentReaderSignature::from_bytes(signature),
        })
    }
}
