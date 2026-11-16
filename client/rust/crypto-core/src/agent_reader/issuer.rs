use super::*;
use ed25519_dalek::{Signer, SigningKey};
use zeroize::Zeroizing;

const STORE_MAGIC: &[u8; 4] = b"CCRI";
const STORE_VERSION: u8 = 1;
const STORE_BYTES: usize = 37;

/// Dedicated Reader issuer only. Store under its own profile SecureStore item;
/// never import/reuse DeviceIdentity/DeviceSecretKeys/Node/vault key material.
/// Encoding distinguishes accidental wrong-purpose import, not malicious seed
/// relabelling; the Native custody owner retains provenance responsibility.
pub struct ReaderIssuerSecretV1 {
    key: Box<SigningKey>,
}
impl std::fmt::Debug for ReaderIssuerSecretV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReaderIssuerSecretV1([redacted])")
    }
}
impl ReaderIssuerSecretV1 {
    pub fn generate() -> AgentReaderResult<Self> {
        let mut seed = Zeroizing::new([0u8; 32]);
        crate::rng::fill_random(seed.as_mut()).map_err(|_| AgentReaderError::Entropy)?;
        if *seed == [0; 32] {
            return Err(AgentReaderError::Entropy);
        }
        Ok(Self {
            key: Box::new(SigningKey::from_bytes(&seed)),
        })
    }
    pub fn from_secure_store_bytes(bytes: &[u8]) -> AgentReaderResult<Self> {
        if bytes.len() != STORE_BYTES || &bytes[..4] != STORE_MAGIC || bytes[4] != STORE_VERSION {
            return Err(AgentReaderError::InvalidWire);
        }
        let mut seed = Zeroizing::new([0u8; 32]);
        seed.copy_from_slice(&bytes[5..]);
        if *seed == [0; 32] {
            return Err(AgentReaderError::InvalidWire);
        }
        Ok(Self {
            key: Box::new(SigningKey::from_bytes(&seed)),
        })
    }
    pub fn to_secure_store_bytes(&self) -> Zeroizing<Vec<u8>> {
        let mut out = Zeroizing::new(Vec::with_capacity(STORE_BYTES));
        out.extend(STORE_MAGIC);
        out.push(STORE_VERSION);
        out.extend(self.key.as_bytes());
        out
    }
    pub fn public_key(&self) -> AgentReaderPublicKey {
        AgentReaderPublicKey::from_bytes(self.key.verifying_key().to_bytes())
    }
    fn pin(&self, pin: &AgentReaderIssuerPinV1) -> AgentReaderResult<()> {
        pin.validate()?;
        if self.public_key() != pin.issuer_key {
            return Err(AgentReaderError::WrongIssuer);
        }
        Ok(())
    }
    fn sign<T: AgentReaderStatementV1>(&self, body: T) -> AgentReaderResult<AgentReaderSigned<T>> {
        let signature = self.key.sign(&body.signing_bytes()?).to_bytes();
        Ok(AgentReaderSigned {
            body,
            signature: AgentReaderSignature::from_bytes(signature),
        })
    }
    /// Structural/pin verification only. Native authority must first enforce
    /// original approval, current head, time, transaction/CAS and disclosure.
    pub fn sign_association(
        &self,
        body: AgentReaderAssociationV1,
        pin: &AgentReaderIssuerPinV1,
    ) -> AgentReaderResult<AgentReaderSigned<AgentReaderAssociationV1>> {
        self.pin(pin)?;
        if body.context != pin.context
            || body.issuer_id != pin.issuer_id
            || body.issuer_epoch != pin.issuer_epoch
            || body.native_profile_id != pin.native_profile_id
            || body.audience_id != pin.audience_id
            || body.node_trust_origin_hash != pin.node_trust_origin_hash
        {
            return Err(AgentReaderError::WrongContext);
        }
        if body.device_key == pin.issuer_key {
            return Err(AgentReaderError::WrongDevice);
        }
        if body.expires_at > pin.expires_at {
            return Err(AgentReaderError::InvalidTime);
        }
        self.sign(body)
    }
    pub fn sign_grant(
        &self,
        body: AgentReaderGrantV1,
        pin: &AgentReaderIssuerPinV1,
    ) -> AgentReaderResult<AgentReaderSigned<AgentReaderGrantV1>> {
        self.pin(pin)?;
        head_pin(&body.head, pin)?;
        if body.expires_at > pin.expires_at {
            return Err(AgentReaderError::InvalidTime);
        }
        self.sign(body)
    }
    pub fn sign_freshness(
        &self,
        body: AgentReaderFreshnessV1,
        pin: &AgentReaderIssuerPinV1,
    ) -> AgentReaderResult<AgentReaderSigned<AgentReaderFreshnessV1>> {
        self.pin(pin)?;
        head_pin(&body.head, pin)?;
        if body.expires_at > pin.expires_at {
            return Err(AgentReaderError::InvalidTime);
        }
        self.sign(body)
    }
    pub fn sign_revocation(
        &self,
        body: AgentReaderRevocationV1,
        pin: &AgentReaderIssuerPinV1,
    ) -> AgentReaderResult<AgentReaderSigned<AgentReaderRevocationV1>> {
        self.pin(pin)?;
        if body.context != pin.context
            || body.issuer_id != pin.issuer_id
            || body.issuer_epoch != pin.issuer_epoch
        {
            return Err(AgentReaderError::WrongIssuer);
        }
        self.sign(body)
    }
}
fn head_pin(head: &AgentReaderHeadV1, pin: &AgentReaderIssuerPinV1) -> AgentReaderResult<()> {
    if head.context != pin.context
        || head.issuer_id != pin.issuer_id
        || head.issuer_epoch != pin.issuer_epoch
    {
        return Err(AgentReaderError::WrongIssuer);
    }
    Ok(())
}
/// Public hash primitive only; this is not a current-head or mutation ack.
pub fn advance_reader_head_v1(
    previous: &AgentReaderHeadV1,
    admin_body_digest: AgentReaderHash,
) -> AgentReaderResult<AgentReaderHeadV1> {
    let mut payload = previous.authority_hash.as_bytes().to_vec();
    payload.extend(admin_body_digest.as_bytes());
    Ok(AgentReaderHeadV1 {
        context: previous.context.clone(),
        issuer_id: previous.issuer_id,
        issuer_epoch: previous.issuer_epoch,
        authority_revision: previous
            .authority_revision
            .checked_add(1)
            .ok_or(AgentReaderError::Quota)?,
        authority_hash: wire::hash(b"consolecrypt.agent.reader.authority-head.v1", &payload),
    })
}
