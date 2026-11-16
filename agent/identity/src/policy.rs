use crate::durable::{Entry, Transition};
use crate::wire::{live, recent};
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use subtle::ConstantTimeEq;

pub const MAX_INVITATIONS: usize = 128;
pub const MAX_PENDING: usize = 32;
pub const MAX_AGENTS: usize = 128;
pub const MAX_NONCES: usize = 4096;
/// Lifetime owner-key history, including the independently pinned genesis key.
pub const MAX_OWNER_KEYS: usize = 128;

/// Explicit independently pinned local trust, never a server ACL. Bootstrap
/// ownership and any extension past this expiry require a separate adapter/UI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnerPin {
    pub context: Context,
    pub key: PublicKey,
    pub epoch: u64,
    pub checkpoint: u64,
    pub expires_at: Timestamp,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvitationStatus {
    Offered,
    Claimed,
    Approved,
    Rejected,
    Expired,
}
#[derive(Clone)]
struct Invitation {
    offer: Offer,
    claim: Option<Claim>,
    status: InvitationStatus,
}
#[derive(Clone)]
struct Agent {
    certificate: Certificate,
    revoked: bool,
}

/// Opaque validated actor state. FileStore restores it only by replaying signed
/// transitions against an independent pin and externally retained journal anchor.
#[derive(Clone)]
pub struct State {
    pub(crate) latest: Option<Entry>,
    revision: u64,
    pin: OwnerPin,
    last_time: Timestamp,
    invitations: BTreeMap<InvitationId, Invitation>,
    agents: BTreeMap<AgentId, Agent>,
    agent_keys: BTreeSet<PublicKey>,
    owner_keys: BTreeSet<PublicKey>,
    nonces: BTreeSet<Nonce>,
    leases: BTreeSet<LeaseId>,
    tunnel_admissions: BTreeMap<LeaseId, tunnel_resume::OriginalTunnelAdmission>,
    collector_services: BTreeMap<ResourceId, collector_service::OriginalCollectorService>,
}
/// Public bounded counters only. No keys, invitations, tokens or nonce values.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyCapacity {
    pub retained_invitations: usize,
    pub pending_invitations: usize,
    pub retained_agents: usize,
    pub consumed_nonces: usize,
    pub consumed_leases: usize,
    pub retained_agent_key_tombstones: usize,
    pub max_agent_key_tombstones: usize,
    pub retained_owner_key_tombstones: usize,
    pub max_owner_key_tombstones: usize,
    pub max_invitations: usize,
    pub max_pending: usize,
    pub max_agents: usize,
    pub max_nonces: usize,
    pub max_leases: usize,
    pub remaining_admissions_upper_bound: usize,
}
impl State {
    pub fn bootstrap(pin: OwnerPin, now: Now) -> Result<Self> {
        pin.context.personal()?;
        pin.key.validate()?;
        pin.expires_at.valid()?;
        now.0.valid()?;
        if pin.epoch == 0 {
            return Err(Error::WrongEpoch);
        }
        if now.0 >= pin.expires_at {
            return Err(Error::Expired);
        }
        Ok(Self {
            latest: None,
            revision: 0,
            owner_keys: BTreeSet::from([pin.key]),
            pin,
            last_time: now.0,
            invitations: BTreeMap::new(),
            agents: BTreeMap::new(),
            agent_keys: BTreeSet::new(),
            nonces: BTreeSet::new(),
            leases: BTreeSet::new(),
            tunnel_admissions: BTreeMap::new(),
            collector_services: BTreeMap::new(),
        })
    }
    pub fn capacity(&self) -> PolicyCapacity {
        PolicyCapacity {
            retained_invitations: self.invitations.len(),
            pending_invitations: pending(self),
            retained_agents: self.agents.len(),
            consumed_nonces: self.nonces.len(),
            consumed_leases: self.leases.len(),
            retained_agent_key_tombstones: self.agent_keys.len(),
            max_agent_key_tombstones: MAX_NONCES,
            retained_owner_key_tombstones: self.owner_keys.len(),
            max_owner_key_tombstones: MAX_OWNER_KEYS,
            max_invitations: MAX_INVITATIONS,
            max_pending: MAX_PENDING,
            max_agents: MAX_AGENTS,
            max_nonces: MAX_NONCES,
            max_leases: MAX_NONCES,
            remaining_admissions_upper_bound: (MAX_NONCES - self.nonces.len())
                .min(MAX_NONCES - self.leases.len()),
        }
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn owner(&self) -> &OwnerPin {
        &self.pin
    }
    pub fn invitation_status(&self, id: InvitationId) -> Option<InvitationStatus> {
        self.invitations.get(&id).map(|v| v.status)
    }
    pub fn certificate(&self, id: AgentId) -> Option<&Certificate> {
        self.agents.get(&id).map(|v| &v.certificate)
    }
    pub fn revoked(&self, id: AgentId) -> bool {
        self.agents.get(&id).is_some_and(|v| v.revoked)
    }
}

/// CAS must atomically persist state,
/// invitation/lease tombstones, high-water and nonce before returning success.
/// It must not return old snapshots after restore; external rollback protection
/// and fresh owner checkpoints are required for any future durable adapter.
pub trait CompareAndSwap {
    fn load(&self) -> Result<State>;
    fn compare_and_swap(&mut self, expected_revision: u64, next: State) -> Result<()>;
}
pub struct Actor<S: CompareAndSwap> {
    store: S,
    context: Context,
    seen: (u64, u64, u64, Timestamp),
    last_observed_time: Timestamp,
}
#[cfg(target_os = "linux")]
impl Actor<SqliteJournalStore> {
    /// Concrete verified observation, never a portable acknowledgement. Samples
    /// around provider I/O so replay latency cannot hide expiry/clock rollback.
    pub(crate) fn observe_local_rotation(
        &mut self,
        expected: &Certificate,
        clock: &mut impl FnMut() -> Result<Now>,
    ) -> Result<OwnerPin> {
        let before = clock()?;
        let state = self.read(before)?;
        let after = clock()?;
        after.0.valid()?;
        if after.0 < before.0 || after.0 < self.last_observed_time || after.0 < state.last_time {
            return Err(Error::InvalidTime);
        }
        self.last_observed_time = after.0;
        State::bootstrap(state.pin.clone(), after)?;
        let agent = state
            .agents
            .get(&expected.agent_id)
            .ok_or(Error::NotFound)?;
        if agent.revoked {
            return Err(Error::Revoked);
        }
        if agent.certificate != *expected {
            return Err(Error::WrongClaim);
        }
        live(expected.not_before, expected.expires_at, after)?;
        Ok(state.pin)
    }
}
impl<S: CompareAndSwap> Actor<S> {
    /// Reader-only live observation. It neither admits a Node request nor
    /// reconstructs a Permit from a caller-supplied certificate or boolean.
    pub(crate) fn observe_reader_target(
        &mut self,
        expected: &crate::reader::ReaderNodeTarget,
        reader_keys: [PublicKey; 2],
        clock: &mut impl FnMut() -> Result<Now>,
    ) -> Result<crate::reader::ReaderNodeObservation> {
        let before = clock()?;
        let state = self.read(before)?;
        let after = clock()?;
        after.0.valid()?;
        if after.0 < before.0 || after.0 < self.last_observed_time || after.0 < state.last_time {
            return Err(Error::InvalidTime);
        }
        self.last_observed_time = after.0;
        if state.pin.context != expected.context {
            return Err(Error::WrongContext);
        }
        State::bootstrap(state.pin.clone(), after)?;
        if reader_keys[0] == reader_keys[1]
            || reader_keys
                .iter()
                .any(|key| key_reserved(&state, *key, None))
        {
            return Err(Error::KeyReuse);
        }
        let agent = state
            .agents
            .get(&expected.agent_id)
            .ok_or(Error::NotFound)?;
        if agent.revoked {
            return Err(Error::Revoked);
        }
        let cert = &agent.certificate;
        if cert.key_epoch != expected.key_epoch || cert.digest()? != expected.certificate_hash {
            return Err(Error::WrongEpoch);
        }
        if !cert.scopes.contains(Scope::TelemetryRead)
            || !cert.capabilities.scopes.contains(Scope::TelemetryRead)
        {
            return Err(Error::ScopeDenied);
        }
        live(cert.not_before, cert.expires_at, after)?;
        Ok(crate::reader::ReaderNodeObservation {
            owner_epoch: state.pin.epoch,
            checkpoint: state.pin.checkpoint,
            owner_expires_at: state.pin.expires_at,
            certificate_expires_at: cert.expires_at,
        })
    }
    pub fn new(store: S) -> Result<Self> {
        let state = store.load()?;
        state.pin.context.personal()?;
        Ok(Self {
            context: state.pin.context.clone(),
            seen: watermark(&state),
            last_observed_time: state.last_time,
            store,
        })
    }
    pub fn snapshot(&self) -> Result<State> {
        self.store.load()
    }
    pub fn into_store(self) -> S {
        self.store
    }
    fn read(&mut self, now: Now) -> Result<State> {
        let state = self.store.load()?;
        now.0.valid()?;
        if now.0 < self.last_observed_time {
            return Err(Error::InvalidTime);
        }
        self.last_observed_time = now.0;
        state.pin.context.personal()?;
        if state.pin.context != self.context {
            return Err(Error::WrongContext);
        }
        let seen = watermark(&state);
        if seen.0 < self.seen.0
            || seen.1 < self.seen.1
            || seen.2 < self.seen.2
            || seen.3 < self.seen.3
        {
            return Err(Error::WrongCheckpoint);
        }
        if now.0 < state.last_time {
            return Err(Error::InvalidTime);
        }
        self.seen = seen;
        Ok(state)
    }
    fn commit(&mut self, mut state: State, now: Now, transition: Transition) -> Result<()> {
        if state.invitations.len() > MAX_INVITATIONS
            || state.agents.len() > MAX_AGENTS
            || state.nonces.len() > MAX_NONCES
            || state.leases.len() > MAX_NONCES
            || state.tunnel_admissions.len() > MAX_NONCES
            || state.collector_services.len() > collector_service::MAX_COLLECTOR_SERVICES
            || state.agent_keys.len() > MAX_NONCES
        {
            return Err(Error::Quota);
        }
        let revision = state.revision;
        state.revision = revision.checked_add(1).ok_or(Error::Quota)?;
        state.last_time = now.0;
        state.latest = Some(Entry {
            at: now.0,
            transition,
        });
        let seen = watermark(&state);
        self.store.compare_and_swap(revision, state)?;
        self.seen = seen;
        Ok(())
    }
    pub fn offer(&mut self, signed: Signed<Offer>, now: Now) -> Result<()> {
        let transition = Transition::Offer(signed.clone());
        let mut state = self.read(now)?;
        let offer = &signed.body;
        owner_next(&state, &offer.header, now)?;
        signed.verify(state.pin.key)?;
        recent(offer.issued_at, now)?;
        live(offer.issued_at, offer.expires_at, now)?;
        if offer.expires_at > state.pin.expires_at {
            return Err(Error::Expired);
        }
        if state.invitations.contains_key(&offer.invitation_id)
            || state.agents.contains_key(&offer.agent_id)
            || state.invitations.values().any(|i| {
                i.offer.agent_id == offer.agent_id
                    && !matches!(
                        i.status,
                        InvitationStatus::Rejected | InvitationStatus::Expired
                    )
            })
        {
            return Err(Error::Replay);
        }
        // An unregistered persistent node may retry only after an explicitly
        // retained terminal invitation, under a fresh ID/token/context. This is
        // not a lost-key recovery path: its stable AgentId stays bound to its key.
        if state
            .invitations
            .values()
            .any(|i| i.offer.agent_id == offer.agent_id && i.offer.agent_key != offer.agent_key)
        {
            return Err(Error::WrongClaim);
        }
        // A TTL retry must not resurrect the old one-use secret even if an
        // administrator accidentally signs a fresh invitation using that hash.
        if state
            .invitations
            .values()
            .any(|i| i.offer.agent_id == offer.agent_id && i.offer.token_hash == offer.token_hash)
        {
            return Err(Error::Replay);
        }
        if state.invitations.len() >= MAX_INVITATIONS || pending(&state) >= MAX_PENDING {
            return Err(Error::Quota);
        }
        if key_reserved(&state, offer.agent_key, None) {
            return Err(Error::KeyReuse);
        }
        state.pin.checkpoint = offer.header.checkpoint;
        state.invitations.insert(
            offer.invitation_id,
            Invitation {
                offer: signed.body,
                claim: None,
                status: InvitationStatus::Offered,
            },
        );
        self.commit(state, now, transition)
    }
    pub fn consume(
        &mut self,
        token: &InvitationToken,
        signed: Signed<Claim>,
        now: Now,
    ) -> Result<Hash> {
        self.consume_digest(token.digest(), signed, now)
    }
    pub(crate) fn consume_digest(
        &mut self,
        token_hash: Hash,
        signed: Signed<Claim>,
        now: Now,
    ) -> Result<Hash> {
        let transition = Transition::Consume {
            token_hash,
            signed: signed.clone(),
        };
        let mut state = self.read(now)?;
        let claim = &signed.body;
        let invitation = state
            .invitations
            .get(&claim.invitation_id)
            .ok_or(Error::NotFound)?;
        if invitation.status != InvitationStatus::Offered {
            return Err(Error::Replay);
        }
        let offer = &invitation.offer;
        live(offer.issued_at, offer.expires_at, now)?;
        same_context(&state, &claim.header)?;
        if claim.header != offer.header
            || claim.offer_hash != offer.digest()?
            || claim.agent_id != offer.agent_id
            || claim.agent_key != offer.agent_key
        {
            return Err(Error::WrongClaim);
        }
        if !bool::from(token_hash.as_bytes().ct_eq(offer.token_hash.as_bytes())) {
            return Err(Error::InvalidToken);
        }
        recent(claim.issued_at, now)?;
        signed.verify(offer.agent_key)?;
        if claim.issued_at < offer.issued_at {
            return Err(Error::InvalidTime);
        }
        nonce(&mut state, claim.nonce)?;
        let digest = claim.digest()?;
        let invitation = state
            .invitations
            .get_mut(&claim.invitation_id)
            .ok_or(Error::NotFound)?;
        invitation.claim = Some(signed.body);
        invitation.status = InvitationStatus::Claimed;
        self.commit(state, now, transition)?;
        Ok(digest)
    }
    /// The owner signature is explicit final approval of the exact claim and
    /// compared fingerprint, not the earlier offer or possession of a URL/token.
    pub fn approve(&mut self, signed: Signed<Certificate>, now: Now) -> Result<()> {
        let transition = Transition::Approve(signed.clone());
        let mut state = self.read(now)?;
        let cert = &signed.body;
        owner_next(&state, &cert.header, now)?;
        signed.verify(state.pin.key)?;
        certificate_time(&state, cert, now)?;
        let CertificateOrigin::Enrollment {
            invitation_id,
            claim_hash,
            key_fingerprint,
        } = &cert.origin
        else {
            return Err(Error::WrongState);
        };
        let invitation_id = *invitation_id;
        let invitation = state
            .invitations
            .get(&invitation_id)
            .ok_or(Error::NotFound)?;
        if invitation.status != InvitationStatus::Claimed {
            return Err(Error::WrongState);
        }
        live(invitation.offer.issued_at, invitation.offer.expires_at, now)?;
        let claim = invitation.claim.as_ref().ok_or(Error::WrongState)?;
        if *claim_hash != claim.digest()?
            || *key_fingerprint != claim.agent_key.fingerprint()
            || cert.agent_id != claim.agent_id
            || cert.agent_key != claim.agent_key
            || cert.key_epoch != 1
            || cert.capabilities != claim.capabilities
        {
            return Err(Error::WrongClaim);
        }
        if !cert.scopes.subset(&invitation.offer.scopes) {
            return Err(Error::ScopeDenied);
        }
        capability_ceiling(cert)?;
        if state.agents.contains_key(&cert.agent_id) {
            return Err(Error::Replay);
        }
        if state.agents.len() >= MAX_AGENTS {
            return Err(Error::Quota);
        }
        if key_reserved(&state, cert.agent_key, Some(invitation_id)) {
            return Err(Error::KeyReuse);
        }
        state.agent_keys.insert(cert.agent_key);
        state.pin.checkpoint = cert.header.checkpoint;
        state.agents.insert(
            cert.agent_id,
            Agent {
                certificate: signed.body,
                revoked: false,
            },
        );
        state
            .invitations
            .get_mut(&invitation_id)
            .ok_or(Error::NotFound)?
            .status = InvitationStatus::Approved;
        self.commit(state, now, transition)
    }
    pub fn reject(&mut self, signed: Signed<Rejection>, now: Now) -> Result<()> {
        let transition = Transition::Reject(signed.clone());
        let mut state = self.read(now)?;
        let reject = &signed.body;
        owner_next(&state, &reject.header, now)?;
        signed.verify(state.pin.key)?;
        recent(reject.issued_at, now)?;
        let invitation = state
            .invitations
            .get_mut(&reject.invitation_id)
            .ok_or(Error::NotFound)?;
        if invitation.status != InvitationStatus::Claimed
            || invitation
                .claim
                .as_ref()
                .ok_or(Error::WrongState)?
                .digest()?
                != reject.claim_hash
        {
            return Err(Error::WrongClaim);
        }
        invitation.status = InvitationStatus::Rejected;
        state.pin.checkpoint = reject.header.checkpoint;
        self.commit(state, now, transition)
    }
    pub fn expire(&mut self, id: InvitationId, now: Now) -> Result<()> {
        let transition = Transition::Expire(id);
        let mut state = self.read(now)?;
        let invitation = state.invitations.get_mut(&id).ok_or(Error::NotFound)?;
        if !matches!(
            invitation.status,
            InvitationStatus::Offered | InvitationStatus::Claimed
        ) {
            return Err(Error::WrongState);
        }
        if now.0 < invitation.offer.expires_at {
            return Err(Error::InvalidTime);
        }
        invitation.status = InvitationStatus::Expired;
        self.commit(state, now, transition)
    }
    pub fn revoke(&mut self, signed: Signed<Revocation>, now: Now) -> Result<()> {
        let transition = Transition::Revoke(signed.clone());
        let mut state = self.read(now)?;
        let revoke = &signed.body;
        owner_next(&state, &revoke.header, now)?;
        signed.verify(state.pin.key)?;
        recent(revoke.issued_at, now)?;
        let agent = state
            .agents
            .get_mut(&revoke.agent_id)
            .ok_or(Error::NotFound)?;
        if agent.revoked {
            return Err(Error::Revoked);
        }
        if revoke.key_epoch != agent.certificate.key_epoch
            || revoke.certificate_hash != agent.certificate.digest()?
        {
            return Err(Error::WrongEpoch);
        }
        agent.revoked = true;
        state.pin.checkpoint = revoke.header.checkpoint;
        self.commit(state, now, transition)
    }
    pub fn rotate(
        &mut self,
        certificate: Signed<Certificate>,
        old: Signed<RotationProof>,
        new: Signed<NewKeyProof>,
        now: Now,
    ) -> Result<()> {
        let transition = Transition::Rotate {
            certificate: certificate.clone(),
            old: old.clone(),
            new: new.clone(),
        };
        let mut state = self.read(now)?;
        let cert = &certificate.body;
        owner_next(&state, &cert.header, now)?;
        certificate.verify(state.pin.key)?;
        certificate_time(&state, cert, now)?;
        let previous = state.agents.get(&cert.agent_id).ok_or(Error::NotFound)?;
        if previous.revoked {
            return Err(Error::Revoked);
        }
        live(
            previous.certificate.not_before,
            previous.certificate.expires_at,
            now,
        )?;
        let CertificateOrigin::Rotation {
            previous_certificate,
            previous_key_epoch,
        } = cert.origin
        else {
            return Err(Error::WrongState);
        };
        if previous_certificate != previous.certificate.digest()?
            || previous_key_epoch != previous.certificate.key_epoch
            || cert.key_epoch != previous_key_epoch.checked_add(1).ok_or(Error::WrongEpoch)?
            || cert.agent_key == previous.certificate.agent_key
        {
            return Err(Error::WrongEpoch);
        }
        if !cert.scopes.subset(&previous.certificate.scopes) {
            return Err(Error::ScopeDenied);
        }
        capability_ceiling(cert)?;
        if old.body != new.body.0
            || old.body.header != cert.header
            || old.body.agent_id != cert.agent_id
            || old.body.certificate_hash != cert.digest()?
        {
            return Err(Error::WrongClaim);
        }
        recent(old.body.issued_at, now)?;
        old.verify(previous.certificate.agent_key)?;
        new.verify(cert.agent_key)?;
        if key_reserved(&state, cert.agent_key, None) {
            return Err(Error::KeyReuse);
        }
        nonce(&mut state, old.body.nonce)?;
        state.agent_keys.insert(cert.agent_key);
        state.pin.checkpoint = cert.header.checkpoint;
        state.agents.insert(
            cert.agent_id,
            Agent {
                certificate: certificate.body,
                revoked: false,
            },
        );
        self.commit(state, now, transition)
    }
    /// Owner-key continuity does not extend authority expiry. Existing verified
    /// certificates retain their ceiling; all new leases need the new owner pin.
    pub fn rotate_owner(
        &mut self,
        old: Signed<OwnerRotation>,
        new: Signed<NewOwnerProof>,
        now: Now,
    ) -> Result<()> {
        let transition = Transition::RotateOwner {
            old: old.clone(),
            new: new.clone(),
        };
        let mut state = self.read(now)?;
        let change = &old.body;
        owner_next(&state, &change.header, now)?;
        old.verify(state.pin.key)?;
        if change != &new.body.0
            || change.new_epoch != state.pin.epoch.checked_add(1).ok_or(Error::WrongEpoch)?
            || change.new_key == state.pin.key
        {
            return Err(Error::WrongEpoch);
        }
        recent(change.issued_at, now)?;
        live(change.issued_at, change.expires_at, now)?;
        if change.expires_at > state.pin.expires_at {
            return Err(Error::Expired);
        }
        new.verify(change.new_key)?;
        if owner_key_reserved(&state, change.new_key) {
            return Err(Error::KeyReuse);
        }
        if state.owner_keys.len() >= MAX_OWNER_KEYS {
            return Err(Error::Quota);
        }
        state.owner_keys.insert(change.new_key);
        state.pin.key = change.new_key;
        state.pin.epoch = change.new_epoch;
        state.pin.checkpoint = change.header.checkpoint;
        state.pin.expires_at = change.expires_at;
        self.commit(state, now, transition)
    }
    pub fn admit(
        &mut self,
        lease: Signed<Lease>,
        proof: Signed<AgentRequest>,
        freshness: Option<&Signed<Freshness>>,
        operation: &Operation,
        now: Now,
    ) -> Result<Permit> {
        let fresh = freshness.ok_or(Error::MissingFreshness)?;
        let transition = Transition::Admit {
            lease: lease.clone(),
            proof: proof.clone(),
            freshness: fresh.clone(),
            operation: operation.clone(),
        };
        let mut state = self.read(now)?;
        current_header(&state, &fresh.body.header, now)?;
        fresh.verify(state.pin.key)?;
        live(fresh.body.issued_at, fresh.body.expires_at, now)?;
        if fresh.body.expires_at > state.pin.expires_at {
            return Err(Error::Expired);
        }
        let grant = &lease.body;
        current_header(&state, &grant.header, now)?;
        lease.verify(state.pin.key)?;
        live(grant.issued_at, grant.expires_at, now)?;
        if grant.header != fresh.body.header || grant.operation != *operation {
            return Err(Error::ScopeDenied);
        }
        let agent = state.agents.get(&grant.agent_id).ok_or(Error::NotFound)?;
        if agent.revoked {
            return Err(Error::Revoked);
        }
        let certificate = &agent.certificate;
        live(certificate.not_before, certificate.expires_at, now)?;
        if grant.certificate_hash != certificate.digest()?
            || grant.key_epoch != certificate.key_epoch
        {
            return Err(Error::WrongEpoch);
        }
        if grant.expires_at > certificate.expires_at
            || grant.expires_at > state.pin.expires_at
            || grant.expires_at > fresh.body.expires_at
        {
            return Err(Error::Expired);
        }
        if let Some(scope) = operation.scope() {
            if !certificate.scopes.contains(scope) {
                return Err(Error::ScopeDenied);
            }
            if !certificate.capabilities.scopes.contains(scope) {
                return Err(Error::CapabilityDenied);
            }
        }
        if proof.body.header != grant.header
            || proof.body.agent_id != grant.agent_id
            || proof.body.key_epoch != grant.key_epoch
            || proof.body.lease_hash != grant.digest()?
        {
            return Err(Error::WrongClaim);
        }
        recent(proof.body.issued_at, now)?;
        if proof.body.issued_at < grant.issued_at {
            return Err(Error::InvalidTime);
        }
        proof.verify(certificate.agent_key)?;
        if state.leases.contains(&grant.id) {
            return Err(Error::Replay);
        }
        nonce(&mut state, proof.body.nonce)?;
        state.leases.insert(grant.id);
        // Derived ONLY from the existing verified signed Admit. No cached Permit.
        let tunnel = if matches!(operation, Operation::TunnelOpenBound { .. }) {
            if state.tunnel_admissions.len() >= MAX_NONCES { return Err(Error::Quota); }
            state.tunnel_admissions.insert(grant.id,
                tunnel_resume::OriginalTunnelAdmission::admitted(lease.clone(), Some(fresh.clone()), proof.body.nonce, now)?);
            Some((grant.id, proof.body.nonce))
        } else { None };
        // Admission does not return a permit until the replay records commit.
        let permit = Permit {
            agent_id: grant.agent_id,
            key_epoch: grant.key_epoch,
            checkpoint: grant.header.checkpoint,
            owner_epoch: grant.header.owner_epoch,
            certificate_hash: grant.certificate_hash,
            operation: operation.clone(),
            expires_at: grant.expires_at,
            tunnel,
        };
        self.commit(state, now, transition)?;
        Ok(permit)
    }

    /// A future resource adapter must consume the permit and recheck here at
    /// access; admission alone is not cancellation of an already running task.
    pub fn check_permit(&mut self, permit: &Permit, now: Now) -> Result<()> {
        let state = self.read(now)?;
        if permit.owner_epoch != state.pin.epoch {
            return Err(Error::WrongEpoch);
        }
        if permit.checkpoint != state.pin.checkpoint {
            return Err(Error::WrongCheckpoint);
        }
        if now.0 >= permit.expires_at || now.0 >= state.pin.expires_at {
            return Err(Error::Expired);
        }
        if let Some((lease, channel)) = permit.tunnel {
            let original = state.tunnel_admissions.get(&lease).ok_or(Error::WrongState)?;
            if original.drained || original.current_channel != channel { return Err(Error::WrongState); }
        }
        let agent = state.agents.get(&permit.agent_id).ok_or(Error::NotFound)?;
        if agent.revoked {
            return Err(Error::Revoked);
        }
        if agent.certificate.key_epoch != permit.key_epoch
            || agent.certificate.digest()? != permit.certificate_hash
        {
            return Err(Error::WrongEpoch);
        }
        live(
            agent.certificate.not_before,
            agent.certificate.expires_at,
            now,
        )
    }
}

/// In-process authority only. No Serialize/Deserialize/Clone: a server-supplied
/// boolean or cached wire DTO cannot stand in for successful live admission.
#[derive(Debug)]
pub struct Permit {
    agent_id: AgentId,
    key_epoch: u64,
    checkpoint: u64,
    owner_epoch: u64,
    certificate_hash: Hash,
    operation: Operation,
    expires_at: Timestamp,
    tunnel: Option<(LeaseId, Nonce)>,
}
impl Permit {
    pub fn agent_id(&self) -> AgentId {
        self.agent_id
    }
    pub fn key_epoch(&self) -> u64 {
        self.key_epoch
    }
    pub fn operation(&self) -> &Operation {
        &self.operation
    }
    pub fn expires_at(&self) -> Timestamp {
        self.expires_at
    }
}
// Never let the monitoring principal share any retained owner key, another
// active pairing key or any key ever approved for an agent (including rotations
// and revoked agents). Terminal never-approved invitations release only their
// pending reservation; all invitation and nonce tombstones remain retained.
fn key_reserved(s: &State, key: PublicKey, except_invitation: Option<InvitationId>) -> bool {
    s.owner_keys.contains(&key)
        || s.agent_keys.contains(&key)
        || s.agents
            .values()
            .any(|agent| agent.certificate.agent_key == key)
        || s.invitations.iter().any(|(id, invitation)| {
            Some(*id) != except_invitation
                && matches!(
                    invitation.status,
                    InvitationStatus::Offered | InvitationStatus::Claimed
                )
                && invitation.offer.agent_key == key
        })
}
// A key ever advertised in this retained agent history cannot later become an
// owner key, even if its invitation expired/rejected before approval. This does
// not alter normal same-node TTL retries among never-owner agent keys.
fn owner_key_reserved(s: &State, key: PublicKey) -> bool {
    key_reserved(s, key, None)
        || s.invitations
            .values()
            .any(|invite| invite.offer.agent_key == key)
}
fn watermark(s: &State) -> (u64, u64, u64, Timestamp) {
    (s.revision, s.pin.checkpoint, s.pin.epoch, s.last_time)
}
fn pending(s: &State) -> usize {
    s.invitations
        .values()
        .filter(|i| {
            matches!(
                i.status,
                InvitationStatus::Offered | InvitationStatus::Claimed
            )
        })
        .count()
}
fn same_context(s: &State, h: &Header) -> Result<()> {
    if h.context != s.pin.context {
        return Err(Error::WrongContext);
    }
    if h.owner_epoch != s.pin.epoch {
        return Err(Error::WrongEpoch);
    }
    Ok(())
}
fn current_header(s: &State, h: &Header, now: Now) -> Result<()> {
    same_context(s, h)?;
    if now.0 >= s.pin.expires_at {
        return Err(Error::Expired);
    }
    if h.checkpoint != s.pin.checkpoint {
        return Err(Error::WrongCheckpoint);
    }
    Ok(())
}
fn owner_next(s: &State, h: &Header, now: Now) -> Result<()> {
    same_context(s, h)?;
    if now.0 >= s.pin.expires_at {
        return Err(Error::Expired);
    }
    if h.checkpoint
        != s.pin
            .checkpoint
            .checked_add(1)
            .ok_or(Error::WrongCheckpoint)?
    {
        return Err(Error::WrongCheckpoint);
    }
    Ok(())
}
fn nonce(s: &mut State, n: Nonce) -> Result<()> {
    if s.nonces.contains(&n) {
        return Err(Error::Replay);
    }
    if s.nonces.len() >= MAX_NONCES {
        return Err(Error::Quota);
    }
    s.nonces.insert(n);
    Ok(())
}
fn certificate_time(s: &State, c: &Certificate, now: Now) -> Result<()> {
    live(c.not_before, c.expires_at, now)?;
    recent(c.not_before, now)?;
    if c.expires_at > s.pin.expires_at {
        return Err(Error::Expired);
    }
    Ok(())
}
fn capability_ceiling(c: &Certificate) -> Result<()> {
    if !c.scopes.subset(&c.capabilities.scopes) {
        Err(Error::CapabilityDenied)
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

#[path = "tunnel_resume.rs"]
mod tunnel_resume;
pub use tunnel_resume::TunnelRoutePolicyV2;

#[path = "collector_service.rs"]
mod collector_service;
pub use collector_service::{CollectorServicePermit, CollectorServiceClockObservation, CollectorServiceAccess};

pub(crate) use collector_service::{CollectorServiceStoredObservation,ActiveCollectorServiceObservation,CollectorServiceEndReason};

pub(crate) use collector_service::{StoredCollectorServiceCredentialSet, LifetimeReadback};
