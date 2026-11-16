//! Full V2 wrapper DATA; no Actor, admission or resume authority.
use crate::*;
/// Opt-in DATA signed indirectly by the ORIGINAL owner Lease's policyHash.
/// The v1 BODY, canonical bytes and domain remain unchanged. The closed v2
/// wrapper uses the existing policy domain; no new signing purpose is added.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TunnelRoutePolicyV2 {
    pub format: String,
    pub route: TunnelRoutePolicy,
    pub resume_original: bool,
}
impl std::fmt::Debug for TunnelRoutePolicyV2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TunnelRoutePolicyV2([redacted])")
    }
}
impl TunnelRoutePolicyV2 {
    pub fn validate(&self) -> Result<()> {
        self.route.validate()?;
        // The signed bit selects resume consent; false still permits one Admit.
        if self.format != "cc-agent-tunnel-route-policy/v2" {
            return Err(Error::ScopeDenied);
        }
        Ok(())
    }
    pub fn digest(&self) -> Result<Hash> {
        self.validate()?;
        let mut bytes = Vec::new();
        crate::wire::canonical_json(
            &serde_json::to_value(self).map_err(|_| Error::InvalidWire)?,
            &mut bytes,
        )?;
        Ok(crate::wire::hash(TUNNEL_POLICY_DOMAIN, &bytes))
    }
    pub fn operation(&self) -> Result<Operation> {
        Ok(Operation::TunnelOpenBound {
            route: self.route.route_id,
            policy_hash: self.digest()?,
        })
    }
    pub fn check_binding(&self, binding: &NodeRequestBinding) -> Result<()> {
        self.validate()?;
        let route = &self.route;
        let cert = &binding.certificate.body;
        let lease = &binding.lease.body;
        if binding.operation != self.operation()? || lease.operation != binding.operation {
            return Err(Error::ScopeDenied);
        }
        if cert.header.context != route.context
            || lease.header.context != route.context
            || binding.freshness.body.header.context != route.context
        {
            return Err(Error::WrongContext);
        }
        if cert.agent_id != route.agent_id
            || lease.agent_id != route.agent_id
            || cert.digest()? != route.agent_certificate_hash
            || lease.certificate_hash != route.agent_certificate_hash
        {
            return Err(Error::WrongClaim);
        }
        if cert.key_epoch != route.key_epoch || lease.key_epoch != route.key_epoch {
            return Err(Error::WrongEpoch);
        }
        if cert.capabilities.version != 1
            || cert.capabilities.scopes.0.as_slice() != [Scope::TunnelOpen]
            || cert.scopes.0.as_slice() != [Scope::TunnelOpen]
        {
            return Err(Error::CapabilityDenied);
        }
        if lease
            .expires_at
            .seconds()
            .checked_sub(lease.issued_at.seconds())
            .filter(|s| *s > 0 && *s <= route.limits.max_ttl_seconds)
            .is_none()
        {
            return Err(Error::Expired);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "tunnel_v2_tests.rs"]
mod tests;
