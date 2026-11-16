//! Cryptographic observations on one caller-owned TLS stream, never a Permit.
use crate::*;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

const FRAME_MAX: usize = 16384;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OriginalTunnelMode {
    Admit,
    Resume,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct Open<'a> {
    format: &'static str,
    mode: OriginalTunnelMode,
    binding: &'a NodeRequestBinding,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Challenge {
    format: String,
    proof: Signed<AgentRequest>,
    policy_hash: Hash,
    expires_at: Timestamp,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Echo<'a> {
    format: &'static str,
    nonce: Nonce,
    policy_hash: Hash,
    lease_hash: Hash,
    device_id: &'a str,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Ack {
    format: String,
    proof: Signed<AgentRequest>,
    policy_hash: Hash,
    expires_at: Timestamp,
}

/// Nonportable one-shot consistency state. It cannot attest to the TLS stream;
/// the caller must keep the SAME actual stream across Open/challenge/echo/ACK.
pub struct PendingOriginalChallenge {
    proof: Signed<AgentRequest>,
    binding_bytes: Vec<u8>,
    pin: OwnerPin,
    policy_hash: Hash,
    expires_at: Timestamp,
    device: String,
    echoed: bool,
}
impl std::fmt::Debug for PendingOriginalChallenge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PendingOriginalChallenge([redacted])")
    }
}
impl PendingOriginalChallenge {
    /// Exactly one echo of the Node's nonce, never a newly minted nonce/proof.
    /// Failure consumes this attempt too; a caller must not retry the channel.
    pub fn echo_once(&mut self) -> Result<Vec<u8>> {
        if self.echoed {
            return Err(Error::WrongState);
        }
        self.echoed = true;
        frame(&Echo {
            format: "cc-agent-tunnel-channel-echo/v2",
            nonce: self.proof.body.nonce,
            policy_hash: self.policy_hash,
            lease_hash: self.proof.body.lease_hash,
            device_id: &self.device,
        })
    }
}

/// ACK observation only: no Actor Permit, drained marker or lifetime renewal.
pub struct VerifiedOriginalAck {
    expires: Timestamp,
}
impl std::fmt::Debug for VerifiedOriginalAck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VerifiedOriginalAck([redacted])")
    }
}
impl VerifiedOriginalAck {
    pub fn expires_at(&self) -> Timestamp {
        self.expires
    }
}

fn canonical<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    wire::canonical_json(
        &serde_json::to_value(value).map_err(|_| Error::InvalidWire)?,
        &mut out,
    )?;
    Ok(out)
}
fn frame<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut out = canonical(value)?;
    out.push(b'\n');
    if out.len() > FRAME_MAX {
        return Err(Error::Quota);
    }
    Ok(out)
}
fn parse<T: Serialize + DeserializeOwned>(raw: &[u8]) -> Result<T> {
    if raw.is_empty()
        || raw.len() > FRAME_MAX
        || raw.last() != Some(&b'\n')
        || raw[..raw.len() - 1].contains(&b'\n')
        || raw.contains(&b'\r')
    {
        return Err(Error::InvalidWire);
    }
    let value: T = serde_json::from_slice(&raw[..raw.len() - 1]).map_err(|_| Error::InvalidWire)?;
    // Derived typed parsing rejects duplicates recursively; exact bytes also
    // reject whitespace, alternate encodings and noncanonical field ordering.
    if frame(&value)? != raw {
        return Err(Error::InvalidWire);
    }
    Ok(value)
}

pub fn open_original_frame(
    binding: &NodeRequestBinding,
    policy: &TunnelRoutePolicyV2,
    mode: OriginalTunnelMode,
) -> Result<Vec<u8>> {
    policy.check_binding(binding)?;
    if mode == OriginalTunnelMode::Resume && !policy.resume_original {
        return Err(Error::ScopeDenied);
    }
    frame(&Open {
        format: "cc-agent-tunnel-open/v2",
        mode,
        binding,
    })
}

/// Exact V2 full-wrapper verification; never calls V1 verify_binding(route).
pub fn verify_binding_v2(
    pin: &OwnerPin,
    binding: &NodeRequestBinding,
    policy: &TunnelRoutePolicyV2,
    now: Now,
) -> Result<()> {
    validate_pin(pin, now)?;
    policy.check_binding(binding)?;
    if policy.route.context != pin.context {
        return Err(Error::WrongContext);
    }
    verify_statement(&binding.certificate, pin.key)?;
    verify_statement(&binding.lease, pin.key)?;
    verify_statement(&binding.freshness, pin.key)?;
    let c = &binding.certificate.body;
    let l = &binding.lease.body;
    let f = &binding.freshness.body;
    if c.header.owner_epoch != pin.epoch
        || c.header.checkpoint > pin.checkpoint
        || l.header.owner_epoch != pin.epoch
        || l.header.checkpoint != pin.checkpoint
        || f.header != l.header
    {
        return Err(Error::WrongCheckpoint);
    }
    check_live(c.not_before, c.expires_at, now)?;
    check_live(l.issued_at, l.expires_at, now)?;
    check_live(f.issued_at, f.expires_at, now)?;
    if l.expires_at > c.expires_at
        || l.expires_at > f.expires_at
        || l.expires_at > pin.expires_at
        || f.expires_at > pin.expires_at
    {
        return Err(Error::Expired);
    }
    Ok(())
}
fn check_proof(proof: &Signed<AgentRequest>, binding: &NodeRequestBinding, now: Now) -> Result<()> {
    verify_statement(proof, binding.certificate.body.agent_key)?;
    let p = &proof.body;
    let l = &binding.lease.body;
    if p.header != l.header
        || p.agent_id != l.agent_id
        || p.key_epoch != l.key_epoch
        || p.lease_hash != l.digest()?
    {
        return Err(Error::WrongClaim);
    }
    check_recent(p.issued_at, now)?;
    if p.issued_at < l.issued_at {
        return Err(Error::InvalidTime);
    }
    Ok(())
}

pub fn verify_original_challenge(
    raw: &[u8],
    pin: &OwnerPin,
    binding: &NodeRequestBinding,
    policy: &TunnelRoutePolicyV2,
    now: Now,
) -> Result<PendingOriginalChallenge> {
    let value: Challenge = parse(raw)?;
    verify_binding_v2(pin, binding, policy, now)?;
    let policy_hash = policy.digest()?;
    if value.format != "cc-agent-tunnel-channel-challenge/v2"
        || value.policy_hash != policy_hash
        || value.expires_at != binding.lease.body.expires_at
    {
        return Err(Error::WrongClaim);
    }
    check_proof(&value.proof, binding, now)?;
    Ok(PendingOriginalChallenge {
        proof: value.proof,
        binding_bytes: canonical(binding)?,
        pin: pin.clone(),
        policy_hash,
        expires_at: value.expires_at,
        device: policy.route.client_device_id.clone(),
        echoed: false,
    })
}

/// Consumes the original challenge after one echo. The caller must separately
/// recheck original session/TLS ownership; EOF never certifies Agent drainage.
pub fn verify_original_ack(
    raw: &[u8],
    pending: PendingOriginalChallenge,
    pin: &OwnerPin,
    binding: &NodeRequestBinding,
    policy: &TunnelRoutePolicyV2,
    now: Now,
) -> Result<VerifiedOriginalAck> {
    if !pending.echoed {
        return Err(Error::WrongState);
    }
    let value: Ack = parse(raw)?;
    verify_binding_v2(pin, binding, policy, now)?;
    check_proof(&value.proof, binding, now)?;
    if *pin != pending.pin
        || canonical(binding)? != pending.binding_bytes
        || policy.digest()? != pending.policy_hash
        || value.format != "cc-agent-tunnel-open-ack/v2"
        || value.policy_hash != pending.policy_hash
        || value.expires_at != pending.expires_at
        || value.proof.body != pending.proof.body
        || value.proof.signature != pending.proof.signature
    {
        return Err(Error::WrongClaim);
    }
    Ok(VerifiedOriginalAck {
        expires: value.expires_at,
    })
}
