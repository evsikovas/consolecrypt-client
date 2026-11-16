//! Purpose-specific wrapper over the already-loaded original device identity.
//! AppCore owns original Session/Arc/profile/unlock/approval/action/transport
//! fences. This wrapper never loads, saves, creates or exports an identity.
use crate::DeviceIdentity;
use cc_crypto_core::agent_reader::{
    AgentReaderError, AgentReaderProofV1, AgentReaderSigned, AgentReaderValidatedInputsV1,
};

impl DeviceIdentity {
    pub fn sign_agent_reader_proof_v1(
        &self,
        inputs: AgentReaderValidatedInputsV1,
        now: cc_protocol::Timestamp,
    ) -> Result<AgentReaderSigned<AgentReaderProofV1>, AgentReaderError> {
        let proof =
            inputs.prepare_for_device_once(self.device_id(), self.public_keys().signing, now)?;
        self.secret_keys().sign_agent_reader_proof_v1(proof)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_crypto_core::agent_reader::*;
    use serde_json::{Value, json};
    fn decode<T: serde::de::DeserializeOwned>(value: Value) -> T {
        serde_json::from_value(value).unwrap()
    }
    fn inputs(identity: &DeviceIdentity) -> AgentReaderValidatedInputsV1 {
        // Public golden fields, with fresh test issuer and the actual identity's
        // public device values. No SecureStore or device-secret serialization.
        let v: Value = serde_json::from_str(include_str!(
            "../../crypto-core/src/agent_reader/vectors.json"
        ))
        .unwrap();
        let issuer = ReaderIssuerSecretV1::generate().unwrap();
        let mut pin: AgentReaderIssuerPinV1 = decode(v["pin"]["body"].clone());
        pin.issuer_key = issuer.public_key();
        let mut a: AgentReaderAssociationV1 = decode(v["statements"][0]["body"].clone());
        a.device_id = AgentReaderDeviceId::from_device_id(identity.device_id()).unwrap();
        a.device_key = AgentReaderPublicKey::from_bytes(identity.public_keys().signing);
        a.previous_authority_hash = pin.genesis_hash().unwrap();
        let genesis = AgentReaderHeadV1 {
            context: pin.context.clone(),
            issuer_id: pin.issuer_id,
            issuer_epoch: 1,
            authority_revision: 0,
            authority_hash: pin.genesis_hash().unwrap(),
        };
        let head = advance_reader_head_v1(&genesis, a.body_digest().unwrap()).unwrap();
        let mut c: AgentReaderChallengeV1 = decode(v["challenge"]["body"].clone());
        c.head = head.clone();
        c.device_id = a.device_id;
        c.device_key_fingerprint = agent_reader_key_fingerprint(a.device_key);
        c.association_hash = a.body_digest().unwrap();
        let mut g: AgentReaderGrantV1 = decode(v["statements"][1]["body"].clone());
        g.head = head.clone();
        g.device_id = a.device_id;
        g.association_hash = c.association_hash;
        g.challenge_hash = c.challenge_hash().unwrap();
        let mut f: AgentReaderFreshnessV1 = decode(v["statements"][4]["body"].clone());
        f.head = head.clone();
        let packet = json!({"association":issuer.sign_association(a.clone(),&pin).unwrap(),
            "grant":issuer.sign_grant(g,&pin).unwrap(),"freshness":issuer.sign_freshness(f,&pin).unwrap(),"challenge":c});
        let expected = AgentReaderExpectedV1 {
            issuer_pin: pin,
            current_head: head,
            association_body: a,
            target: decode(v["target"]["body"].clone()),
        };
        AgentReaderValidatedInputsV1::from_packet(
            &serde_json::to_vec(&packet).unwrap(),
            expected,
            chrono::DateTime::<chrono::Utc>::from_timestamp(1002, 0).unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn existing_identity_purpose_wrapper_binds_actual_device_without_storage() {
        let original = DeviceIdentity::generate().unwrap();
        let signed = original
            .sign_agent_reader_proof_v1(
                inputs(&original),
                chrono::DateTime::<chrono::Utc>::from_timestamp(1002, 0).unwrap(),
            )
            .unwrap();
        signed
            .verify(AgentReaderPublicKey::from_bytes(
                original.public_keys().signing,
            ))
            .unwrap();
        assert_eq!(signed.body.device_id.as_device_id(), original.device_id());
        let foreign = DeviceIdentity::generate().unwrap();
        assert!(matches!(
            foreign.sign_agent_reader_proof_v1(
                inputs(&original),
                chrono::DateTime::<chrono::Utc>::from_timestamp(1002, 0).unwrap()
            ),
            Err(AgentReaderError::WrongDevice)
        ));
    }
}
