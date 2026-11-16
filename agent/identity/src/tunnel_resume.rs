//! Original admission reuse, not a new lease or a deserialized authority.
//! No IPC or transport owner is installed by this module. The private clean
//! retirement seam MUST only be wired after positive real channel-owner joins.
use super::*;
use sha2::{Digest, Sha256};

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
        crate::wire::canonical_json(&serde_json::to_value(self).map_err(|_| Error::InvalidWire)?, &mut bytes)?;
        Ok(crate::wire::hash(TUNNEL_POLICY_DOMAIN, &bytes))
    }
    pub fn operation(&self) -> Result<Operation> {
        Ok(Operation::TunnelOpenBound { route: self.route.route_id, policy_hash: self.digest()? })
    }
    pub fn check_binding(&self, binding: &NodeRequestBinding) -> Result<()> {
        self.validate()?;
        let route = &self.route;
        let cert = &binding.certificate.body;
        let lease = &binding.lease.body;
        if binding.operation != self.operation()? || lease.operation != binding.operation {
            return Err(Error::ScopeDenied);
        }
        if cert.header.context != route.context || lease.header.context != route.context
            || binding.freshness.body.header.context != route.context { return Err(Error::WrongContext); }
        if cert.agent_id != route.agent_id || lease.agent_id != route.agent_id
            || cert.digest()? != route.agent_certificate_hash
            || lease.certificate_hash != route.agent_certificate_hash { return Err(Error::WrongClaim); }
        if cert.key_epoch != route.key_epoch || lease.key_epoch != route.key_epoch { return Err(Error::WrongEpoch); }
        if cert.capabilities.version != 1 || cert.capabilities.scopes.0.as_slice() != [Scope::TunnelOpen]
            || cert.scopes.0.as_slice() != [Scope::TunnelOpen] { return Err(Error::CapabilityDenied); }
        if lease.expires_at.seconds().checked_sub(lease.issued_at.seconds())
            .filter(|s| *s > 0 && *s <= route.limits.max_ttl_seconds).is_none() { return Err(Error::Expired); }
        Ok(())
    }
}

/// Rebuilt exclusively by verified Admit replay. Never Serialize/Deserialize.
/// Full signed lease SHA is internal integrity DATA, not a new wire signature
/// domain. Both its BODY AND exact signature must match on every reuse.
#[derive(Clone)]
pub(super) struct OriginalTunnelAdmission {
    lease: Signed<Lease>,
    // Derived only from the original verified Admit transition during replay.
    // Option preserves explicit absence: product resume never fills a new one.
    freshness: Option<Signed<Freshness>>,
    signed_lease_digest: Hash,
    admitted_at: Timestamp,
    // Derived ONLY from verified Admit / committed TunnelDebit Entry.at.
    last_activity: Timestamp,
    pub(super) current_channel: Nonce,
    pub(super) drained: bool,
    total_debited: u64,
    channel_debited: u64,
}
fn signed_digest(lease: &Signed<Lease>) -> Result<Hash> {
    let mut bytes = Vec::new();
    crate::wire::canonical_json(&serde_json::to_value(lease).map_err(|_| Error::InvalidWire)?, &mut bytes)?;
    Ok(Hash::from_bytes(Sha256::digest(bytes).into()))
}
impl OriginalTunnelAdmission {
    #[cfg(test)]
    pub(super) fn debited(&self) -> (u64, u64) { (self.total_debited, self.channel_debited) }
    pub(super) fn admitted(lease: Signed<Lease>, freshness:Option<Signed<Freshness>>, channel: Nonce, now: Now) -> Result<Self> {
        Ok(Self { signed_lease_digest: signed_digest(&lease)?, lease, freshness,
            admitted_at: now.0, last_activity: now.0, current_channel: channel, drained: false,
            total_debited: 0, channel_debited: 0 })
    }
    fn exact_lease(&self, lease: &Signed<Lease>) -> Result<()> {
        if self.lease.body != lease.body || self.lease.signature != lease.signature
            || self.signed_lease_digest != signed_digest(lease)? {
            return Err(Error::WrongClaim);
        }
        Ok(())
    }
}

fn bound_policy(state: &State, original: &OriginalTunnelAdmission, policy: &TunnelRoutePolicyV2, now: Now) -> Result<()> {
    let grant = &original.lease.body;
    policy.validate()?;
    current_header(state, &grant.header, now)?;
    live(grant.issued_at, grant.expires_at, now)?;
    if now.0 < original.admitted_at || now.0 < original.last_activity || grant.operation != policy.operation()? {
        return Err(Error::ScopeDenied);
    }
    if now.0 >= idle_deadline(original, policy)? { return Err(Error::Expired); }
    let agent = state.agents.get(&grant.agent_id).ok_or(Error::NotFound)?;
    if agent.revoked { return Err(Error::Revoked); }
    let cert = &agent.certificate;
    live(cert.not_before, cert.expires_at, now)?;
    let route = &policy.route;
    if route.context != grant.header.context || route.context != cert.header.context {
        return Err(Error::WrongContext);
    }
    if route.agent_id != grant.agent_id || route.agent_id != cert.agent_id
        || route.agent_certificate_hash != grant.certificate_hash
        || cert.digest()? != grant.certificate_hash {
        return Err(Error::WrongClaim);
    }
    if route.key_epoch != grant.key_epoch || cert.key_epoch != grant.key_epoch {
        return Err(Error::WrongEpoch);
    }
    if cert.scopes.0.as_slice() != [Scope::TunnelOpen]
        || cert.capabilities.version != 1
        || cert.capabilities.scopes.0.as_slice() != [Scope::TunnelOpen] {
        return Err(Error::CapabilityDenied);
    }
    if grant.expires_at > cert.expires_at || grant.expires_at > state.pin.expires_at
        || grant.expires_at.seconds().checked_sub(grant.issued_at.seconds())
            .filter(|v| *v > 0 && *v <= route.limits.max_ttl_seconds).is_none() {
        return Err(Error::Expired);
    }
    Ok(())
}
fn idle_deadline(original: &OriginalTunnelAdmission, policy: &TunnelRoutePolicyV2) -> Result<Timestamp> {
    Ok(original.lease.body.expires_at.min(original.last_activity.checked_add(policy.route.limits.idle_timeout_seconds)?))
}
fn original_permit(original: &OriginalTunnelAdmission) -> Permit {
    let grant = &original.lease.body;
    Permit { agent_id: grant.agent_id, key_epoch: grant.key_epoch,
        checkpoint: grant.header.checkpoint, owner_epoch: grant.header.owner_epoch,
        certificate_hash: grant.certificate_hash, operation: grant.operation.clone(),
        expires_at: grant.expires_at, tunnel: Some((grant.id, original.current_channel)) }
}

impl<S: CompareAndSwap> Actor<S> {
    /// Read-only startup preview. No Node signature/nonce, lease consumption,
    /// Permit, journal mutation or target/network operation. Actual TLS-bound
    /// admission remains the existing supervisor's sole state transition.
    pub(crate) fn preview_original_tunnel_binding(&mut self, binding: &NodeRequestBinding,
        policy: &TunnelRoutePolicyV2, now: Now) -> Result<()> {
        policy.check_binding(binding)?;
        let state=self.read(now)?;
        let grant=&binding.lease.body;let fresh=&binding.freshness.body;
        current_header(&state,&grant.header,now)?;current_header(&state,&fresh.header,now)?;
        binding.lease.verify(state.pin.key)?;binding.freshness.verify(state.pin.key)?;
        live(grant.issued_at,grant.expires_at,now)?;live(fresh.issued_at,fresh.expires_at,now)?;
        if grant.header!=fresh.header || grant.expires_at>fresh.expires_at
            || fresh.expires_at>state.pin.expires_at {return Err(Error::Expired)}
        let agent=state.agents.get(&grant.agent_id).ok_or(Error::NotFound)?;
        if agent.revoked {return Err(Error::Revoked)}
        let certificate=&agent.certificate;
        certificate.validate()?;live(certificate.not_before,certificate.expires_at,now)?;
        if certificate!=&binding.certificate.body || certificate.digest()?!=grant.certificate_hash
            || certificate.key_epoch!=grant.key_epoch || grant.expires_at>certificate.expires_at
            || grant.expires_at>state.pin.expires_at {return Err(Error::WrongClaim)}
        if certificate.header.owner_epoch==state.pin.epoch {binding.certificate.verify(state.pin.key)?;}
        if state.leases.contains(&grant.id) {return Err(Error::Replay)}
        if state.leases.len()>=MAX_NONCES || state.nonces.len()>=MAX_NONCES
            || state.tunnel_admissions.len()>=MAX_NONCES {return Err(Error::Quota)}
        Ok(())
    }

    /// Read-only preview of a consumed EXACT original lease after private owned
    /// retirement. No nonce, new lease, journal CAS, Permit or activity renewal.
    pub(crate) fn preview_original_tunnel_resume_binding(&mut self, binding: &NodeRequestBinding,
        policy: &TunnelRoutePolicyV2, now: Now) -> Result<(Timestamp, Timestamp)> {
        if !policy.resume_original { return Err(Error::ScopeDenied); }
        policy.check_binding(binding)?;
        let state = self.read(now)?;
        let original = state.tunnel_admissions.get(&binding.lease.body.id).ok_or(Error::NotFound)?;
        original.exact_lease(&binding.lease)?;
        let first=original.freshness.as_ref().ok_or(Error::MissingFreshness)?;
        if first.body!=binding.freshness.body || first.signature!=binding.freshness.signature {return Err(Error::WrongClaim)}
        bound_policy(&state, original, policy, now)?;
        if !original.drained { return Err(Error::WrongState); }
        let grant = &binding.lease.body;
        let fresh = &binding.freshness.body;
        current_header(&state, &fresh.header, now)?;
        binding.lease.verify(state.pin.key)?;
        binding.freshness.verify(state.pin.key)?;
        live(fresh.issued_at, fresh.expires_at, now)?;
        let certificate = &state.agents.get(&grant.agent_id).ok_or(Error::NotFound)?.certificate;
        if &binding.certificate.body != certificate { return Err(Error::WrongClaim); }
        if certificate.header.owner_epoch == state.pin.epoch { binding.certificate.verify(state.pin.key)?; }
        if fresh.header != grant.header || grant.expires_at > fresh.expires_at
            || fresh.expires_at > state.pin.expires_at { return Err(Error::Expired); }
        if original.total_debited >= policy.route.limits.max_bytes_total
            || original.channel_debited >= policy.route.limits.max_bytes_per_channel
            || state.nonces.len() >= MAX_NONCES { return Err(Error::Quota); }
        Ok((grant.expires_at, idle_deadline(original, policy)?))
    }

    /// Read-only checked original window, not a new grant or portable authority.
    /// Resume/drain/control never advance it; successful DATA debit alone can.
    pub(crate) fn original_tunnel_window(&mut self, permit: &Permit,
        policy: &TunnelRoutePolicyV2, now: Now) -> Result<(Timestamp, Timestamp)> {
        self.check_permit(permit, now)?;
        let (id, channel) = permit.tunnel.ok_or(Error::ScopeDenied)?;
        let state = self.read(now)?;
        let original = state.tunnel_admissions.get(&id).ok_or(Error::WrongState)?;
        bound_policy(&state, original, policy, now)?;
        if original.drained || original.current_channel != channel { return Err(Error::WrongState); }
        Ok((original.lease.body.expires_at, idle_deadline(original, policy)?))
    }

    /// Fresh Node signature and nonce, SAME original Lease, absolute TTL and
    /// cumulative channel/total byte budget. No re-admit or capability reload.
    /// Caller must bind the fresh proof to the SAME live TLS stream; that
    /// transport constructor is intentionally absent from this Actor-only stage.
    pub fn resume_tunnel(&mut self, lease: Signed<Lease>, proof: Signed<AgentRequest>,
        freshness: &Signed<Freshness>, policy: &TunnelRoutePolicyV2, now: Now) -> Result<Permit> {
        if !policy.resume_original { return Err(Error::ScopeDenied); }
        let mut state = self.read(now)?;
        let id = lease.body.id;
        let original = state.tunnel_admissions.get(&id).ok_or(Error::NotFound)?;
        original.exact_lease(&lease)?;
        bound_policy(&state, original, policy, now)?;
        // A restart with active/unknown old owners is not positive drainage.
        if !original.drained { return Err(Error::WrongState); }
        let grant = &lease.body;
        lease.verify(state.pin.key)?;
        current_header(&state, &freshness.body.header, now)?;
        freshness.verify(state.pin.key)?;
        live(freshness.body.issued_at, freshness.body.expires_at, now)?;
        if freshness.body.header != grant.header || grant.expires_at > freshness.body.expires_at
            || freshness.body.expires_at > state.pin.expires_at {
            return Err(Error::Expired);
        }
        if proof.body.header != grant.header || proof.body.agent_id != grant.agent_id
            || proof.body.key_epoch != grant.key_epoch || proof.body.lease_hash != grant.digest()? {
            return Err(Error::WrongClaim);
        }
        recent(proof.body.issued_at, now)?;
        if proof.body.issued_at < grant.issued_at { return Err(Error::InvalidTime); }
        proof.verify(state.agents.get(&grant.agent_id).ok_or(Error::NotFound)?.certificate.agent_key)?;
        nonce(&mut state, proof.body.nonce)?;
        let original = state.tunnel_admissions.get_mut(&id).ok_or(Error::WrongState)?;
        if original.total_debited >= policy.route.limits.max_bytes_total
            || original.channel_debited >= policy.route.limits.max_bytes_per_channel {
            return Err(Error::Quota);
        }
        original.current_channel = proof.body.nonce;
        original.drained = false;
        let permit = original_permit(original);
        self.commit(state, now, Transition::ResumeTunnel { lease, proof,
            freshness: freshness.clone(), policy: policy.clone() })?;
        Ok(permit)
    }

    /// The exact bounded DATA slice already read from a real stream is charged
    /// durably BEFORE any copy/forward. No payload enters the journal. If later
    /// forwarding fails, quota stays lost; no refund/rollback method exists.
    pub fn debit_tunnel(&mut self, permit: &Permit, policy: &TunnelRoutePolicyV2,
        data: &[u8], now: Now) -> Result<()> {
        self.check_permit(permit, now)?;
        let (id, channel) = permit.tunnel.ok_or(Error::ScopeDenied)?;
        let state = self.read(now)?;
        let original = state.tunnel_admissions.get(&id).ok_or(Error::WrongState)?;
        self.replay_tunnel_debit(id, channel, policy, data.len().try_into().map_err(|_| Error::Quota)?,
            original.total_debited, original.channel_debited, now)
    }
    pub(crate) fn replay_tunnel_debit(&mut self, id: LeaseId, channel: Nonce,
        policy: &TunnelRoutePolicyV2, bytes: u64, prior_total: u64, prior_channel: u64, now: Now) -> Result<()> {
        let mut state = self.read(now)?;
        let original = state.tunnel_admissions.get(&id).ok_or(Error::WrongState)?;
        bound_policy(&state, original, policy, now)?;
        if original.drained || original.current_channel != channel { return Err(Error::WrongState); }
        if original.total_debited != prior_total || original.channel_debited != prior_channel {
            return Err(Error::Conflict);
        }
        let limits = &policy.route.limits;
        if bytes == 0 || bytes > limits.max_frame_bytes { return Err(Error::Quota); }
        let total = prior_total.checked_add(bytes).ok_or(Error::Quota)?;
        let per_channel = prior_channel.checked_add(bytes).ok_or(Error::Quota)?;
        if total > limits.max_bytes_total || per_channel > limits.max_bytes_per_channel { return Err(Error::Quota); }
        let original = state.tunnel_admissions.get_mut(&id).ok_or(Error::WrongState)?;
        original.total_debited = total;
        original.channel_debited = per_channel;
        original.last_activity = now.0;
        self.commit(state, now, Transition::TunnelDebit { lease_id: id, channel,
            policy: policy.clone(), bytes, prior_total, prior_channel })
    }

    /// PRIVATE worker-owned seam, NOT an IPC/JSON acknowledgement and NOT proof
    /// that an Actor can observe sockets. Future integration must call it ONLY
    /// after positive target/carrier/thread closure, consuming the prior Permit.
    /// No constructor taking PID/marker/bool or generic retirement RPC is added.
    #[allow(dead_code)]
    pub(crate) fn retire_tunnel_after_owned_drain(&mut self, permit: Permit, now: Now) -> Result<()> {
        self.check_permit(&permit, now)?;
        let (id, channel) = permit.tunnel.ok_or(Error::ScopeDenied)?;
        self.replay_tunnel_drained(id, channel, now)
    }
    pub(crate) fn replay_tunnel_drained(&mut self, id: LeaseId, channel: Nonce, now: Now) -> Result<()> {
        let mut state = self.read(now)?;
        let original = state.tunnel_admissions.get(&id).ok_or(Error::WrongState)?;
        self.check_permit(&original_permit(original), now)?;
        if original.drained || original.current_channel != channel { return Err(Error::WrongState); }
        state.tunnel_admissions.get_mut(&id).ok_or(Error::WrongState)?.drained = true;
        self.commit(state, now, Transition::TunnelDrained { lease_id: id, channel })
    }
}
