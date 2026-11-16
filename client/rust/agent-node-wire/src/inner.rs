//! The accepted inner Open/Ack protocol, never a Node key or worker handle.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct Open<'a> {
    format: &'static str,
    binding: &'a NodeRequestBinding,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Ack {
    format: String,
    agent_id: AgentId,
    route: ResourceId,
    policy_hash: Hash,
    expires_at: Timestamp,
    proof: Signed<AgentRequest>,
}
/// Checked cryptographic observation, not an Actor Permit or reusable grant.
pub struct VerifiedTunnelAck {
    expires: Timestamp,
}
impl std::fmt::Debug for VerifiedTunnelAck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VerifiedTunnelAck([redacted])")
    }
}
impl VerifiedTunnelAck {
    pub fn expires_at(&self) -> Timestamp {
        self.expires
    }
}
fn frame<T: Serialize>(v: &T, max: usize) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    wire::canonical_json(
        &serde_json::to_value(v).map_err(|_| Error::InvalidWire)?,
        &mut out,
    )?;
    out.push(b'\n');
    if out.len() > max {
        return Err(Error::Quota);
    }
    Ok(out)
}
pub fn open_frame(binding: &NodeRequestBinding, policy: &TunnelRoutePolicy) -> Result<Vec<u8>> {
    policy.check_binding(binding)?;
    frame(
        &Open {
            format: "cc-agent-tunnel-open/v1",
            binding,
        },
        16384,
    )
}
pub fn verify_binding(
    pin: &OwnerPin,
    binding: &NodeRequestBinding,
    policy: &TunnelRoutePolicy,
    now: Now,
) -> Result<()> {
    validate_pin(pin, now)?;
    policy.check_binding(binding)?;
    if policy.context != pin.context {
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
    if l.expires_at > c.expires_at || l.expires_at > f.expires_at || l.expires_at > pin.expires_at {
        return Err(Error::Expired);
    }
    Ok(())
}
pub fn verify_ack(
    raw: &[u8],
    pin: &OwnerPin,
    binding: &NodeRequestBinding,
    policy: &TunnelRoutePolicy,
    now: Now,
) -> Result<VerifiedTunnelAck> {
    if raw.is_empty()
        || raw.len() > 4096
        || raw.last() != Some(&b'\n')
        || raw[..raw.len() - 1].contains(&b'\n')
    {
        return Err(Error::InvalidWire);
    }
    let ack: Ack = serde_json::from_slice(&raw[..raw.len() - 1]).map_err(|_| Error::InvalidWire)?;
    if frame(&ack, 4096)? != raw {
        return Err(Error::InvalidWire);
    }
    verify_binding(pin, binding, policy, now)?;
    if ack.format != "cc-agent-tunnel-open-ack/v1"
        || ack.agent_id != policy.agent_id
        || ack.route != policy.route_id
        || ack.policy_hash != policy.digest()?
        || ack.expires_at != binding.lease.body.expires_at
    {
        return Err(Error::WrongClaim);
    }
    verify_statement(&ack.proof, binding.certificate.body.agent_key)?;
    let p = &ack.proof.body;
    let l = &binding.lease.body;
    if p.header != l.header
        || p.agent_id != l.agent_id
        || p.key_epoch != l.key_epoch
        || p.lease_hash != l.digest()?
    {
        return Err(Error::WrongClaim);
    }
    // Proof recency is checked by Actor at the ONE original admission. Ack on
    // a later channel carries that original proof, never a re-sign/reset.
    if p.issued_at < l.issued_at || p.issued_at > now.0 {
        return Err(Error::InvalidTime);
    }
    Ok(VerifiedTunnelAck {
        expires: ack.expires_at,
    })
}
