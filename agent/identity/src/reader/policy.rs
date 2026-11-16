use super::wire::{hash, live, recent};
use super::*;
use crate::{Actor, AgentId, CompareAndSwap, Context, Hash, Now, Operation, PublicKey, Timestamp};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const MAX_ASSOCIATIONS: usize = 128;
const MAX_LIFETIME: usize = 4096;
pub(crate) struct ReaderNodeTarget {
    pub context: Context,
    pub agent_id: AgentId,
    pub certificate_hash: Hash,
    pub key_epoch: u64,
}
#[derive(Clone, PartialEq, Eq)]
pub struct ReaderNodeObservation {
    pub(crate) owner_epoch: u64,
    pub(crate) checkpoint: u64,
    pub(crate) owner_expires_at: Timestamp,
    pub(crate) certificate_expires_at: Timestamp,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum ReaderAdministration {
    #[serde(rename = "installAssociation")]
    InstallAssociation {
        signed: ReaderSigned<ReaderAssociation>,
    },
    #[serde(rename = "revoke")]
    Revoke {
        signed: ReaderSigned<ReaderRevocation>,
    },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", deny_unknown_fields)]
pub(crate) enum ReaderTransition {
    Install {
        signed: ReaderSigned<ReaderAssociation>,
        target: ReaderTargetDescriptor,
    },
    Revoke(ReaderSigned<ReaderRevocation>),
    Challenge {
        challenge: ReaderChallenge,
        freshness: ReaderSigned<ReaderFreshness>,
    },
    Consume {
        bundle: ReaderBundle,
        expected_agent: AgentId,
        operation: Operation,
        target_hash: Hash,
    },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReaderEntry {
    pub at: Timestamp,
    pub transition: ReaderTransition,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReaderGenesis {
    pub version: u32,
    pub pin: ReaderIssuerPin,
    pub at: Timestamp,
}
#[derive(Clone)]
struct Registered {
    association: ReaderAssociation,
    target: ReaderTargetDescriptor,
}
#[derive(Clone)]
struct Challenge {
    body: ReaderChallenge,
    used: bool,
}
/// No Deserialize or unsigned snapshot authority. Every mutation is replayed.
#[derive(Clone)]
pub struct ReaderState {
    pub(crate) pin: ReaderIssuerPin,
    pub(crate) head: ReaderHead,
    revision: u64,
    last_time: Timestamp,
    associations: BTreeMap<Hash, Registered>,
    association_ids: BTreeMap<ReaderAssociationId, Hash>,
    devices: BTreeMap<ReaderDeviceId, (u64, PublicKey)>,
    revoked_associations: BTreeSet<Hash>,
    revoked_devices: BTreeSet<ReaderDeviceId>,
    challenges: BTreeMap<Hash, Challenge>,
    server_nonces: BTreeSet<Hash>,
    operations: BTreeSet<(ReaderDeviceId, ReaderOperationId)>,
    grants: BTreeSet<ReaderGrantId>,
    proof_nonces: BTreeSet<(ReaderDeviceId, u64, Hash)>,
    pub(crate) latest: Option<ReaderEntry>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderCapacity {
    pub associations: usize,
    pub devices: usize,
    pub challenges: usize,
    pub operations: usize,
    pub consumed_grants: usize,
    pub consumed_nonces: usize,
}
impl ReaderState {
    pub fn bootstrap(pin: ReaderIssuerPin, now: Now) -> ReaderResult<Self> {
        pin.validate()?;
        live(Timestamp::new(0)?, pin.expires_at, now.0)?;
        let head = ReaderHead {
            context: pin.context.clone(),
            issuer_id: pin.issuer_id,
            issuer_epoch: 1,
            authority_revision: 0,
            authority_hash: pin.genesis_hash()?,
        };
        Ok(Self {
            pin,
            head,
            revision: 0,
            last_time: now.0,
            associations: BTreeMap::new(),
            association_ids: BTreeMap::new(),
            devices: BTreeMap::new(),
            revoked_associations: BTreeSet::new(),
            revoked_devices: BTreeSet::new(),
            challenges: BTreeMap::new(),
            server_nonces: BTreeSet::new(),
            operations: BTreeSet::new(),
            grants: BTreeSet::new(),
            proof_nonces: BTreeSet::new(),
            latest: None,
        })
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn owner(&self) -> &ReaderIssuerPin {
        &self.pin
    }
    pub fn authority(&self) -> &ReaderHead {
        &self.head
    }
    pub fn capacity(&self) -> ReaderCapacity {
        ReaderCapacity {
            associations: self.associations.len(),
            devices: self.devices.len(),
            challenges: self.challenges.len(),
            operations: self.operations.len(),
            consumed_grants: self.grants.len(),
            consumed_nonces: self.proof_nonces.len(),
        }
    }
    fn at(&self, now: Timestamp) -> ReaderResult<()> {
        Timestamp::new(now.seconds())?;
        if now < self.last_time {
            return Err(ReaderError::InvalidTime);
        }
        live(Timestamp::new(0)?, self.pin.expires_at, now)
    }
    fn registered(&self, digest: Hash, now: Timestamp) -> ReaderResult<&Registered> {
        let item = self
            .associations
            .get(&digest)
            .ok_or(ReaderError::NotFound)?;
        if self.revoked_associations.contains(&digest)
            || self.revoked_devices.contains(&item.association.device_id)
        {
            return Err(ReaderError::Revoked);
        }
        live(
            item.association.not_before,
            item.association.expires_at,
            now,
        )?;
        Ok(item)
    }
    fn freshness(
        &self,
        signed: &ReaderSigned<ReaderFreshness>,
        now: Timestamp,
    ) -> ReaderResult<()> {
        signed.verify(self.pin.issuer_key)?;
        if signed.body.head != self.head {
            return Err(ReaderError::WrongHead);
        }
        live(signed.body.issued_at, signed.body.expires_at, now)?;
        if signed.body.expires_at > self.pin.expires_at {
            return Err(ReaderError::Expired);
        }
        Ok(())
    }
    fn next(
        &self,
        context: &Context,
        issuer: ReaderIssuerId,
        epoch: u64,
        revision: u64,
        previous: Hash,
    ) -> ReaderResult<()> {
        if context != &self.pin.context {
            return Err(ReaderError::WrongContext);
        }
        if issuer != self.pin.issuer_id || epoch != 1 {
            return Err(ReaderError::WrongIssuer);
        }
        if revision
            != self
                .head
                .authority_revision
                .checked_add(1)
                .ok_or(ReaderError::Quota)?
            || previous != self.head.authority_hash
        {
            return Err(ReaderError::WrongHead);
        }
        Ok(())
    }
    fn advance_head(&mut self, digest: Hash) -> ReaderResult<()> {
        let mut bytes = self.head.authority_hash.as_bytes().to_vec();
        bytes.extend(digest.as_bytes());
        self.head.authority_hash = hash(b"consolecrypt.agent.reader.authority-head.v1", &bytes);
        self.head.authority_revision = self
            .head
            .authority_revision
            .checked_add(1)
            .ok_or(ReaderError::Quota)?;
        Ok(())
    }
}
pub trait ReaderCompareAndSwap {
    fn load(&self) -> ReaderResult<ReaderState>;
    fn compare_and_swap(&mut self, expected: u64, next: ReaderState) -> ReaderResult<()>;
}
pub(crate) fn replay(state: ReaderState, entry: ReaderEntry) -> ReaderResult<ReaderState> {
    apply(state, entry)
}
fn apply(mut state: ReaderState, entry: ReaderEntry) -> ReaderResult<ReaderState> {
    let now = entry.at;
    state.at(now)?;
    match &entry.transition {
        ReaderTransition::Install { signed, target } => {
            signed.verify(state.pin.issuer_key)?;
            let a = &signed.body;
            state.next(
                &a.context,
                a.issuer_id,
                a.issuer_epoch,
                a.authority_revision,
                a.previous_authority_hash,
            )?;
            recent(a.issued_at, now, 60)?;
            live(a.not_before, a.expires_at, now)?;
            if a.expires_at > state.pin.expires_at {
                return Err(ReaderError::Expired);
            }
            if a.native_profile_id != state.pin.native_profile_id
                || a.audience_id != state.pin.audience_id
                || a.node_trust_origin_hash != state.pin.node_trust_origin_hash
            {
                return Err(ReaderError::WrongContext);
            }
            if a.operation != target.operation()
                || a.agent_id != target.agent_id
                || a.target_descriptor_hash != target.digest()?
                || a.dataset_kind != target.dataset.kind
                || a.dataset_schema_version != target.dataset.schema_version
            {
                return Err(ReaderError::WrongTarget);
            }
            if a.device_key == state.pin.issuer_key
                || state
                    .devices
                    .iter()
                    .any(|(id, (_, key))| *id != a.device_id && *key == a.device_key)
            {
                return Err(ReaderError::WrongDevice);
            }
            if let Some(old) = state.devices.get(&a.device_id) {
                if *old != (a.device_epoch, a.device_key) {
                    return Err(ReaderError::WrongDevice);
                }
            }
            if state.revoked_devices.contains(&a.device_id) {
                return Err(ReaderError::Revoked);
            }
            if state.association_ids.contains_key(&a.association_id) {
                return Err(ReaderError::Replay);
            }
            if state.associations.len() >= MAX_ASSOCIATIONS
                || (!state.devices.contains_key(&a.device_id)
                    && state.devices.len() >= MAX_ASSOCIATIONS)
            {
                return Err(ReaderError::Quota);
            }
            for (digest, old) in &state.associations {
                let old = &old.association;
                if old.native_binding_id == a.native_binding_id {
                    if a.native_binding_revision < old.native_binding_revision
                        || (a.native_binding_revision == old.native_binding_revision
                            && (a.native_binding_digest != old.native_binding_digest
                                || a.native_host_id != old.native_host_id
                                || a.native_host_revision != old.native_host_revision
                                || a.native_source_id != old.native_source_id
                                || a.native_source_revision != old.native_source_revision))
                    {
                        return Err(ReaderError::WrongAssociation);
                    }
                    if a.native_binding_revision > old.native_binding_revision {
                        state.revoked_associations.insert(*digest);
                    }
                }
            }
            let digest = a.digest()?;
            state
                .devices
                .insert(a.device_id, (a.device_epoch, a.device_key));
            state.association_ids.insert(a.association_id, digest);
            state.associations.insert(
                digest,
                Registered {
                    association: a.clone(),
                    target: target.clone(),
                },
            );
            state.advance_head(digest)?;
        }
        ReaderTransition::Revoke(signed) => {
            signed.verify(state.pin.issuer_key)?;
            let r = &signed.body;
            state.next(
                &r.context,
                r.issuer_id,
                r.issuer_epoch,
                r.authority_revision,
                r.previous_authority_hash,
            )?;
            recent(r.issued_at, now, 60)?;
            match &r.subject {
                ReaderRevocationSubject::Association {
                    association_id,
                    body_hash,
                } => {
                    if state.association_ids.get(association_id) != Some(body_hash) {
                        return Err(ReaderError::WrongAssociation);
                    }
                    if !state.revoked_associations.insert(*body_hash) {
                        return Err(ReaderError::Replay);
                    }
                }
                ReaderRevocationSubject::Device {
                    device_id,
                    device_epoch,
                    key_fingerprint,
                } => {
                    let (epoch, key) = state.devices.get(device_id).ok_or(ReaderError::NotFound)?;
                    if epoch != device_epoch || reader_key_fingerprint(*key) != *key_fingerprint {
                        return Err(ReaderError::WrongDevice);
                    }
                    if !state.revoked_devices.insert(*device_id) {
                        return Err(ReaderError::Replay);
                    }
                }
            }
            state.advance_head(r.digest()?)?;
        }
        ReaderTransition::Challenge {
            challenge: c,
            freshness,
        } => {
            c.validate()?;
            state.freshness(freshness, now)?;
            let a = &state.registered(c.association_hash, now)?.association;
            if c.head != state.head
                || c.issued_at != now
                || c.expires_at > freshness.body.expires_at
                || c.expires_at > a.expires_at
                || c.device_id != a.device_id
                || c.device_epoch != a.device_epoch
                || c.device_key_fingerprint != reader_key_fingerprint(a.device_key)
                || c.agent_id != a.agent_id
                || c.agent_certificate_hash != a.agent_certificate_hash
                || c.agent_key_epoch != a.agent_key_epoch
                || c.native_profile_id != a.native_profile_id
                || c.native_binding_id != a.native_binding_id
                || c.native_binding_revision != a.native_binding_revision
                || c.operation != a.operation
                || c.target_descriptor_hash != a.target_descriptor_hash
            {
                return Err(ReaderError::WrongAssociation);
            }
            if state.operations.contains(&(c.device_id, c.operation_id))
                || state.challenges.contains_key(&c.challenge_id)
                || state.server_nonces.contains(&c.server_nonce)
            {
                return Err(ReaderError::Replay);
            }
            let pending = state
                .challenges
                .values()
                .filter(|v| !v.used && v.body.expires_at > now);
            if pending.clone().count() >= 32
                || pending.filter(|v| v.body.device_id == c.device_id).count() >= 8
                || state.challenges.len() >= MAX_LIFETIME
            {
                return Err(ReaderError::Quota);
            }
            state.operations.insert((c.device_id, c.operation_id));
            state.server_nonces.insert(c.server_nonce);
            state.challenges.insert(
                c.challenge_id,
                Challenge {
                    body: c.clone(),
                    used: false,
                },
            );
        }
        ReaderTransition::Consume {
            bundle,
            expected_agent,
            operation,
            target_hash,
        } => {
            bundle.association.verify(state.pin.issuer_key)?;
            state.freshness(&bundle.freshness, now)?;
            bundle.grant.verify(state.pin.issuer_key)?;
            let a = &bundle.association.body;
            let digest = a.digest()?;
            if state.registered(digest, now)?.association != *a {
                return Err(ReaderError::WrongAssociation);
            }
            let g = &bundle.grant.body;
            let c = &state
                .challenges
                .get(&g.challenge_id)
                .ok_or(ReaderError::NotFound)?;
            if c.used {
                return Err(ReaderError::Replay);
            }
            let c = &c.body;
            if g.head != state.head || c.head != state.head {
                return Err(ReaderError::WrongHead);
            }
            live(c.issued_at, c.expires_at, now)?;
            live(g.issued_at, g.expires_at, now)?;
            if g.association_hash != digest
                || c.association_hash != digest
                || g.challenge_hash != c.digest()?
                || g.operation_id != c.operation_id
                || g.device_id != a.device_id
                || g.device_epoch != a.device_epoch
                || g.agent_id != a.agent_id
                || g.operation != a.operation
                || g.target_descriptor_hash != a.target_descriptor_hash
                || g.operation != c.operation
                || g.target_descriptor_hash != c.target_descriptor_hash
            {
                return Err(ReaderError::WrongAssociation);
            }
            if *expected_agent != g.agent_id
                || operation != &g.operation
                || *target_hash != g.target_descriptor_hash
            {
                return Err(ReaderError::WrongTarget);
            }
            if g.issued_at < c.issued_at
                || g.expires_at > c.expires_at
                || g.expires_at > a.expires_at
                || g.expires_at > bundle.freshness.body.expires_at
                || g.expires_at > state.pin.expires_at
            {
                return Err(ReaderError::Expired);
            }
            let p = &bundle.proof.body;
            if p.head != g.head
                || p.operation_id != g.operation_id
                || p.device_id != g.device_id
                || p.device_epoch != g.device_epoch
                || p.association_hash != digest
                || p.grant_hash != g.digest()?
                || p.challenge_hash != g.challenge_hash
            {
                return Err(ReaderError::WrongDevice);
            }
            recent(p.issued_at, now, 30)?;
            if p.issued_at < g.issued_at {
                return Err(ReaderError::InvalidTime);
            }
            bundle.proof.verify(a.device_key)?;
            if state.grants.contains(&g.grant_id)
                || state
                    .proof_nonces
                    .contains(&(p.device_id, p.device_epoch, p.nonce))
            {
                return Err(ReaderError::Replay);
            }
            if state.grants.len() >= MAX_LIFETIME || state.proof_nonces.len() >= MAX_LIFETIME {
                return Err(ReaderError::Quota);
            }
            state.grants.insert(g.grant_id);
            state
                .proof_nonces
                .insert((p.device_id, p.device_epoch, p.nonce));
            state
                .challenges
                .get_mut(&g.challenge_id)
                .ok_or(ReaderError::Storage)?
                .used = true;
        }
    }
    state.revision = state.revision.checked_add(1).ok_or(ReaderError::Quota)?;
    state.last_time = now;
    state.latest = Some(entry);
    Ok(state)
}

/// Actual in-process authority; never Clone/Serialize/Deserialize/Debug.
pub struct ReaderPermit {
    association_hash: Hash,
    head: ReaderHead,
    node: ReaderNodeObservation,
    device_id: ReaderDeviceId,
    agent_id: AgentId,
    operation: Operation,
    target_hash: Hash,
    expires_at: Timestamp,
    native_profile_id: NativeProfileId,
    native_host_id: NativeHostId,
    native_binding_id: NativeBindingId,
    native_binding_revision: u64,
}
impl ReaderPermit {
    pub fn reader_device_id(&self) -> ReaderDeviceId {
        self.device_id
    }
    pub fn target_agent_id(&self) -> AgentId {
        self.agent_id
    }
    pub fn operation(&self) -> &Operation {
        &self.operation
    }
    pub fn target_descriptor_hash(&self) -> Hash {
        self.target_hash
    }
    pub fn association_hash(&self) -> Hash {
        self.association_hash
    }
    pub fn expires_at(&self) -> Timestamp {
        self.expires_at
    }
    pub fn native_profile_id(&self) -> NativeProfileId {
        self.native_profile_id
    }
    pub fn native_host_id(&self) -> NativeHostId {
        self.native_host_id
    }
    pub fn native_binding_id(&self) -> NativeBindingId {
        self.native_binding_id
    }
    pub fn native_binding_revision(&self) -> u64 {
        self.native_binding_revision
    }
}
pub struct ReaderActor<R: ReaderCompareAndSwap, N: CompareAndSwap> {
    store: R,
    node: Actor<N>,
    targets: ReaderTargetSet,
    origin: Hash,
    seen: u64,
    last_time: Timestamp,
    pin: ReaderIssuerPin,
    seen_head: ReaderHead,
}
impl<R: ReaderCompareAndSwap, N: CompareAndSwap> ReaderActor<R, N> {
    pub fn new(
        store: R,
        node: Actor<N>,
        targets: ReaderTargetSet,
        node_origin: Hash,
    ) -> ReaderResult<Self> {
        targets.validate()?;
        let state = store.load()?;
        if state.pin.node_trust_origin_hash != node_origin {
            return Err(ReaderError::WrongTarget);
        }
        if node.snapshot()?.owner().context != state.pin.context {
            return Err(ReaderError::WrongContext);
        }
        Ok(Self {
            seen: state.revision,
            last_time: state.last_time,
            pin: state.pin.clone(),
            seen_head: state.head.clone(),
            store,
            node,
            targets,
            origin: node_origin,
        })
    }
    pub fn snapshot(&self) -> ReaderResult<ReaderState> {
        self.store.load()
    }
    pub fn into_store(self) -> R {
        self.store
    }
    fn read(&mut self, now: Timestamp) -> ReaderResult<ReaderState> {
        if Timestamp::new(now.seconds()).is_err() || now < self.last_time {
            return Err(ReaderError::ClockUnavailable);
        }
        let state = self.store.load()?;
        state.at(now).map_err(|e| {
            if e == ReaderError::InvalidTime {
                ReaderError::ClockUnavailable
            } else {
                e
            }
        })?;
        if state.pin != self.pin {
            return Err(ReaderError::WrongIssuer);
        }
        if state.revision < self.seen
            || state.head.authority_revision < self.seen_head.authority_revision
            || (state.head.authority_revision == self.seen_head.authority_revision
                && state.head != self.seen_head)
        {
            return Err(ReaderError::WrongHead);
        }
        if now < self.last_time {
            return Err(ReaderError::ClockUnavailable);
        }
        self.seen = state.revision;
        self.seen_head = state.head.clone();
        self.last_time = now;
        Ok(state)
    }
    fn commit(&mut self, state: ReaderState, entry: ReaderEntry) -> ReaderResult<()> {
        let expected = state.revision;
        let next = apply(state, entry)?;
        let now = next.last_time;
        let head = next.head.clone();
        self.store.compare_and_swap(expected, next)?;
        self.seen = expected + 1;
        self.seen_head = head;
        self.last_time = now;
        Ok(())
    }
    fn observe(
        &mut self,
        a: &ReaderAssociation,
        clock: &mut impl FnMut() -> ReaderResult<Now>,
    ) -> ReaderResult<ReaderNodeObservation> {
        if a.node_trust_origin_hash != self.origin {
            return Err(ReaderError::WrongTarget);
        }
        if a.context != self.pin.context {
            return Err(ReaderError::WrongContext);
        }
        let expected = ReaderNodeTarget {
            context: a.context.clone(),
            agent_id: a.agent_id,
            certificate_hash: a.agent_certificate_hash,
            key_epoch: a.agent_key_epoch,
        };
        let mut clock_error = None;
        let mut bridge = || match clock() {
            Ok(now) => {
                let validation = live(a.not_before, a.expires_at, now.0).and_then(|_| {
                    live(
                        Timestamp::new(0).map_err(ReaderError::from)?,
                        self.pin.expires_at,
                        now.0,
                    )
                });
                match validation {
                    Ok(()) => Ok(now),
                    Err(error) => {
                        clock_error = Some(error);
                        Err(crate::Error::InvalidTime)
                    }
                }
            }
            Err(error) => {
                clock_error = Some(error);
                Err(crate::Error::InvalidTime)
            }
        };
        let result = self.node.observe_reader_target(
            &expected,
            [self.pin.issuer_key, a.device_key],
            &mut bridge,
        );
        if let Some(error) = clock_error {
            return Err(error);
        }
        result.map_err(|error| match error {
            crate::Error::InvalidTime => ReaderError::ClockUnavailable,
            crate::Error::WrongCheckpoint
            | crate::Error::WrongContext
            | crate::Error::UncertainCommit => ReaderError::UncertainCommit,
            crate::Error::Expired => ReaderError::Expired,
            crate::Error::Revoked => ReaderError::Revoked,
            crate::Error::WrongEpoch => ReaderError::WrongTarget,
            crate::Error::NotFound => ReaderError::NotFound,
            crate::Error::KeyReuse => ReaderError::WrongDevice,
            crate::Error::ScopeDenied | crate::Error::CapabilityDenied => {
                ReaderError::CapabilityDenied
            }
            crate::Error::Conflict => ReaderError::Conflict,
            crate::Error::Quota => ReaderError::Quota,
            _ => ReaderError::Storage,
        })
    }
    pub fn administer(
        &mut self,
        admin: ReaderAdministration,
        clock: &mut impl FnMut() -> ReaderResult<Now>,
    ) -> ReaderResult<()> {
        let before = clock()?.0;
        let state = self.read(before)?;
        let transition = match admin {
            ReaderAdministration::InstallAssociation { signed } => {
                signed.verify(state.pin.issuer_key)?;
                let target = self.targets.target(&signed.body.operation)?.clone();
                self.observe(&signed.body, clock)?;
                ReaderTransition::Install { signed, target }
            }
            ReaderAdministration::Revoke { signed } => ReaderTransition::Revoke(signed),
        };
        let after = clock()?.0;
        if after < before {
            return Err(ReaderError::ClockUnavailable);
        }
        self.commit(
            state,
            ReaderEntry {
                at: after,
                transition,
            },
        )
    }
    pub fn challenge(
        &mut self,
        association_hash: Hash,
        operation_id: ReaderOperationId,
        agent: AgentId,
        op: &Operation,
        target_hash: Hash,
        freshness: ReaderSigned<ReaderFreshness>,
        clock: &mut impl FnMut() -> ReaderResult<Now>,
    ) -> ReaderResult<ReaderChallenge> {
        let before = clock()?.0;
        let state = self.read(before)?;
        state.freshness(&freshness, before)?;
        let a = state
            .registered(association_hash, before)?
            .association
            .clone();
        let target = self.targets.target(op)?;
        if agent != a.agent_id
            || op != &a.operation
            || target_hash != a.target_descriptor_hash
            || target.digest()? != target_hash
        {
            return Err(ReaderError::WrongTarget);
        }
        if state.operations.contains(&(a.device_id, operation_id)) {
            return Err(ReaderError::Replay);
        }
        let pending = state
            .challenges
            .values()
            .filter(|c| !c.used && c.body.expires_at > before);
        if state.challenges.len() >= MAX_LIFETIME
            || pending.clone().count() >= 32
            || pending.filter(|c| c.body.device_id == a.device_id).count() >= 8
        {
            return Err(ReaderError::Quota);
        }
        let node = self.observe(&a, &mut || {
            let now = clock()?;
            live(freshness.body.issued_at, freshness.body.expires_at, now.0)?;
            Ok(now)
        })?;
        let now = clock()?.0;
        state.at(now).map_err(|e| {
            if e == ReaderError::InvalidTime {
                ReaderError::ClockUnavailable
            } else {
                e
            }
        })?;
        if now < before {
            return Err(ReaderError::ClockUnavailable);
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (operation_id, node);
            return Err(ReaderError::CapabilityDenied);
        }
        #[cfg(target_os = "linux")]
        {
            let mut bytes = zeroize::Zeroizing::new([0u8; 64]);
            getrandom::fill(bytes.as_mut()).map_err(|_| ReaderError::Entropy)?;
            let id = Hash::from_bytes(bytes[..32].try_into().map_err(|_| ReaderError::Entropy)?);
            let nonce = Hash::from_bytes(bytes[32..].try_into().map_err(|_| ReaderError::Entropy)?);
            if id == Hash::from_bytes([0; 32])
                || nonce == Hash::from_bytes([0; 32])
                || state.challenges.contains_key(&id)
                || state.server_nonces.contains(&nonce)
            {
                return Err(ReaderError::Entropy);
            }
            let issued = clock()?.0;
            if issued < now {
                return Err(ReaderError::ClockUnavailable);
            }
            state.at(issued).map_err(|e| {
                if e == ReaderError::InvalidTime {
                    ReaderError::ClockUnavailable
                } else {
                    e
                }
            })?;
            let expires = *[
                issued.checked_add(30)?,
                a.expires_at,
                state.pin.expires_at,
                freshness.body.expires_at,
                node.owner_expires_at,
                node.certificate_expires_at,
            ]
            .iter()
            .min()
            .ok_or(ReaderError::Storage)?;
            live(issued, expires, issued)?;
            let c = ReaderChallenge {
                version: 1,
                head: state.head.clone(),
                challenge_id: id,
                server_nonce: nonce,
                association_hash,
                operation_id,
                device_id: a.device_id,
                device_epoch: a.device_epoch,
                device_key_fingerprint: reader_key_fingerprint(a.device_key),
                agent_id: a.agent_id,
                agent_certificate_hash: a.agent_certificate_hash,
                agent_key_epoch: a.agent_key_epoch,
                node_owner_epoch: node.owner_epoch,
                node_checkpoint: node.checkpoint,
                native_profile_id: a.native_profile_id,
                native_binding_id: a.native_binding_id,
                native_binding_revision: a.native_binding_revision,
                operation: op.clone(),
                target_descriptor_hash: target_hash,
                issued_at: issued,
                expires_at: expires,
            };
            self.commit(
                state,
                ReaderEntry {
                    at: issued,
                    transition: ReaderTransition::Challenge {
                        challenge: c.clone(),
                        freshness,
                    },
                },
            )?;
            let after = clock()?.0;
            live(c.issued_at, c.expires_at, after)?;
            let current = self.read(after)?;
            if current.head != c.head
                || self.observe(&a, &mut || {
                    let now = clock()?;
                    live(c.issued_at, c.expires_at, now.0)?;
                    Ok(now)
                })? != node
            {
                return Err(ReaderError::WrongHead);
            }
            let final_now = clock()?.0;
            if final_now < after {
                return Err(ReaderError::ClockUnavailable);
            }
            live(c.issued_at, c.expires_at, final_now)?;
            current.at(final_now).map_err(|error| {
                if error == ReaderError::InvalidTime {
                    ReaderError::ClockUnavailable
                } else {
                    error
                }
            })?;
            self.last_time = final_now;
            Ok(c)
        }
    }
    pub fn begin(
        &mut self,
        bundle: ReaderBundle,
        agent: AgentId,
        op: &Operation,
        target_hash: Hash,
        clock: &mut impl FnMut() -> ReaderResult<Now>,
    ) -> ReaderResult<ReaderPermit> {
        let before = clock()?.0;
        let state = self.read(before)?;
        bundle.association.verify(state.pin.issuer_key)?;
        let a = bundle.association.body.clone();
        state.freshness(&bundle.freshness, before)?;
        bundle.grant.verify(state.pin.issuer_key)?;
        if state.registered(a.digest()?, before)?.association != a {
            return Err(ReaderError::WrongAssociation);
        }
        live(
            bundle.grant.body.issued_at,
            bundle.grant.body.expires_at,
            before,
        )?;
        if self.targets.target(op)?.digest()? != target_hash {
            return Err(ReaderError::WrongTarget);
        }
        let node = self.observe(&a, &mut || {
            let now = clock()?;
            live(
                bundle.grant.body.issued_at,
                bundle.grant.body.expires_at,
                now.0,
            )?;
            live(
                bundle.freshness.body.issued_at,
                bundle.freshness.body.expires_at,
                now.0,
            )?;
            Ok(now)
        })?;
        let c = &state
            .challenges
            .get(&bundle.grant.body.challenge_id)
            .ok_or(ReaderError::NotFound)?
            .body;
        if c.node_owner_epoch != node.owner_epoch || c.node_checkpoint != node.checkpoint {
            return Err(ReaderError::WrongTarget);
        }
        let p = ReaderPermit {
            association_hash: a.digest()?,
            head: state.head.clone(),
            node,
            device_id: a.device_id,
            agent_id: agent,
            operation: op.clone(),
            target_hash,
            expires_at: bundle.grant.body.expires_at,
            native_profile_id: a.native_profile_id,
            native_host_id: a.native_host_id,
            native_binding_id: a.native_binding_id,
            native_binding_revision: a.native_binding_revision,
        };
        let now = clock()?.0;
        if now < before {
            return Err(ReaderError::ClockUnavailable);
        }
        self.commit(
            state,
            ReaderEntry {
                at: now,
                transition: ReaderTransition::Consume {
                    bundle,
                    expected_agent: agent,
                    operation: op.clone(),
                    target_hash,
                },
            },
        )?;
        self.check_permit(&p, clock)?;
        Ok(p)
    }
    pub fn check_permit(
        &mut self,
        p: &ReaderPermit,
        clock: &mut impl FnMut() -> ReaderResult<Now>,
    ) -> ReaderResult<()> {
        let before = clock()?.0;
        if before >= p.expires_at {
            return Err(ReaderError::Expired);
        }
        let state = self.read(before)?;
        if state.head != p.head {
            return Err(ReaderError::WrongHead);
        }
        let a = state
            .registered(p.association_hash, before)?
            .association
            .clone();
        if p.device_id != a.device_id
            || p.agent_id != a.agent_id
            || p.operation != a.operation
            || p.target_hash != a.target_descriptor_hash
            || self.targets.target(&p.operation)?.digest()? != p.target_hash
        {
            return Err(ReaderError::WrongTarget);
        }
        live(a.not_before, p.expires_at, before)?;
        if self.observe(&a, &mut || {
            let now = clock()?;
            live(a.not_before, p.expires_at, now.0)?;
            Ok(now)
        })? != p.node
        {
            return Err(ReaderError::WrongTarget);
        }
        let after = clock()?.0;
        if after < before {
            return Err(ReaderError::ClockUnavailable);
        }
        live(a.not_before, p.expires_at, after)?;
        let state = self.read(after)?;
        if state.head != p.head {
            return Err(ReaderError::WrongHead);
        }
        state.registered(p.association_hash, after)?;
        let final_now = clock()?.0;
        if final_now < after {
            return Err(ReaderError::ClockUnavailable);
        }
        live(a.not_before, p.expires_at, final_now)?;
        state.at(final_now).map_err(|error| {
            if error == ReaderError::InvalidTime {
                ReaderError::ClockUnavailable
            } else {
                error
            }
        })?;
        self.last_time = final_now;
        Ok(())
    }
}
