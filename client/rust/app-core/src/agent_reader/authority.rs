//! STAGED ONLY. Native owner-bound profile approval/issuer custody, not a Node grant.
//! No network, DeviceIdentity seed import, automatic initialization or retry.
//! The parent owns explicit UI consumption and transport owns authenticated peer
//! construction. Every returned authority packet follows FULL CAS + OS witness.
use super::scope::{ReaderContextSnapshot, ReaderOriginalContext};
use super::transport::{
    BoundReaderControlReply, ReaderChallengeRequest, ReaderPeerBinding, SignedReaderAdmin,
    VerifiedReaderControlPeer,
};
use crate::AppCore;
use cc_crypto_core::agent_reader::*;
use cc_platform_core::SecureStore;
use cc_storage_core::{ReaderAuthorityWriteError, Storage, StorageError};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;

const BYTES: usize = 8 * 1024 * 1024;
const ENTRIES: usize = 10_000;
const RESERVATIONS: usize = 4096;
const ASSOCIATIONS: usize = 128;
type Result<T> = std::result::Result<T, AuthorityError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(super) enum AuthorityError {
    #[error("reader authority rejected")]
    Rejected,
    #[error("reader authority unavailable")]
    Unavailable,
    #[error("reader authority outcome unknown")]
    Unknown,
    #[error("reader authority quota")]
    Quota,
}
impl From<StorageError> for AuthorityError {
    fn from(_: StorageError) -> Self {
        Self::Unavailable
    }
}
impl From<AgentReaderError> for AuthorityError {
    fn from(_: AgentReaderError) -> Self {
        Self::Rejected
    }
}

// Sorted integer-only JSON is a PRIVATE Native storage encoding. It is never
// substituted for the reviewed crypto statement canonicalization/signing bytes.
fn canonical<T: Serialize>(value: &T, limit: usize) -> Result<Vec<u8>> {
    fn sorted(v: Value) -> Result<Value> {
        Ok(match v {
            Value::Object(m) => Value::Object(
                m.into_iter()
                    .map(|(k, v)| Ok((k, sorted(v)?)))
                    .collect::<Result<BTreeMap<_, _>>>()?
                    .into_iter()
                    .collect(),
            ),
            Value::Array(a) => Value::Array(a.into_iter().map(sorted).collect::<Result<_>>()?),
            Value::Number(ref n) if !n.is_u64() && !n.is_i64() => {
                return Err(AuthorityError::Rejected);
            }
            other => other,
        })
    }
    let value = sorted(serde_json::to_value(value).map_err(|_| AuthorityError::Rejected)?)?;
    let bytes = serde_json::to_vec(&value).map_err(|_| AuthorityError::Rejected)?;
    if bytes.len() > limit {
        return Err(AuthorityError::Quota);
    }
    Ok(bytes)
}
fn digest(domain: &[u8], bytes: &[u8]) -> AgentReaderHash {
    let mut h = Sha256::new();
    h.update(domain);
    h.update([0]);
    h.update(bytes);
    AgentReaderHash::from_bytes(h.finalize().into())
}
fn decode<T: DeserializeOwned + Serialize>(bytes: &[u8]) -> Result<T> {
    if bytes.len() > BYTES {
        return Err(AuthorityError::Quota);
    }
    let value: T = serde_json::from_slice(bytes).map_err(|_| AuthorityError::Unknown)?;
    // Equality also denies duplicate fields, noncanonical whitespace/number forms.
    if canonical(&value, BYTES)? != bytes {
        return Err(AuthorityError::Unknown);
    }
    Ok(value)
}
fn snapshot_value(s: &ReaderContextSnapshot) -> Result<Value> {
    serde_json::to_value(s).map_err(|_| AuthorityError::Rejected)
}
// Durable approval is stable across a fresh unlock, but every review/action
// still borrows and checks its exact live OriginalContext. Do not use this
// projection for an existing attempt, original scope, cancellation or generation.
fn stable_snapshot_matches(stored: &Value, current: &ReaderContextSnapshot) -> Result<bool> {
    let mut stored = stored.clone();
    let mut current = snapshot_value(current)?;
    for value in [&mut stored, &mut current] {
        let map = value.as_object_mut().ok_or(AuthorityError::Unknown)?;
        if !map
            .remove("notificationScopeId")
            .is_some_and(|v| v.as_str().is_some_and(|s| !s.is_empty()))
        {
            return Err(AuthorityError::Unknown);
        }
    }
    Ok(stored == current)
}
// None is exactly the historical Local namespace. Never adopt it into a
// PersonalAccount scope, even when profile/issuer UUIDs happen to match.
fn check_owner_binding(
    expected: Option<AgentReaderHash>,
    actual: Option<AgentReaderHash>,
) -> Result<()> {
    if expected != actual || actual.is_some_and(|v| v == AgentReaderHash::from_bytes([0; 32])) {
        return Err(AuthorityError::Rejected);
    }
    Ok(())
}
fn expected_owner_binding(ctx: &ReaderOriginalContext) -> Result<Option<AgentReaderHash>> {
    let value = if ctx.is_personal_account() {
        Some(ctx.stable_owner_binding_hash())
    } else {
        None
    };
    check_owner_binding(value, value)?;
    Ok(value)
}
fn snapshot_owner_binding(v: &Value) -> Result<Option<AgentReaderHash>> {
    let value = v
        .get("personalOwner")
        .map(|value| {
            serde_json::from_value::<AgentReaderHash>(value.clone())
                .map_err(|_| AuthorityError::Rejected)
        })
        .transpose()?;
    check_owner_binding(value, value)?;
    Ok(value)
}
fn hash_text(s: &str) -> Result<AgentReaderHash> {
    let bytes: [u8; 32] = hex::decode(s)
        .map_err(|_| AuthorityError::Rejected)?
        .try_into()
        .map_err(|_| AuthorityError::Rejected)?;
    if hex::encode(bytes) != s || bytes == [0; 32] {
        return Err(AuthorityError::Rejected);
    }
    Ok(AgentReaderHash::from_bytes(bytes))
}
fn transport_hash(peer: &VerifiedReaderControlPeer) -> Result<AgentReaderHash> {
    peer.binding_digest()
        .map_err(|_| AuthorityError::Unavailable)
}
fn live(
    start: AgentReaderTimestamp,
    end: AgentReaderTimestamp,
    now: AgentReaderTimestamp,
) -> Result<()> {
    if start > now || now >= end {
        return Err(AuthorityError::Rejected);
    }
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    format: String,
    revision: u64,
    previous: AgentReaderHash,
    body: Value,
    digest: AgentReaderHash,
    witness_pending: bool,
}
impl Record {
    fn new<T: Serialize>(old: Option<&Self>, body: &T) -> Result<Self> {
        let revision = old.map_or(Ok(1), |v| {
            v.revision.checked_add(1).ok_or(AuthorityError::Quota)
        })?;
        let previous = old.map_or(AgentReaderHash::from_bytes([0; 32]), |v| v.digest);
        let body = serde_json::to_value(body).map_err(|_| AuthorityError::Rejected)?;
        let value = serde_json::json!({"format":"cc-native-reader-record/v1","revision":revision,
            "previous":previous,"body":body});
        let hash = digest(
            b"consolecrypt.native.reader.record.v1",
            &canonical(&value, BYTES)?,
        );
        let r = Self {
            format: "cc-native-reader-record/v1".into(),
            revision,
            previous,
            body,
            digest: hash,
            witness_pending: true,
        };
        canonical(&r, BYTES)?;
        Ok(r)
    }
    fn check(&self) -> Result<()> {
        if self.format != "cc-native-reader-record/v1" || self.revision == 0 {
            return Err(AuthorityError::Unknown);
        }
        let value = serde_json::json!({"format":self.format,"revision":self.revision,
            "previous":self.previous,"body":self.body});
        if self.digest
            != digest(
                b"consolecrypt.native.reader.record.v1",
                &canonical(&value, BYTES)?,
            )
        {
            return Err(AuthorityError::Unknown);
        }
        Ok(())
    }
    fn witness(&self, key: &str) -> Result<Vec<u8>> {
        canonical(
            &serde_json::json!({"format":"cc-native-reader-witness/v1", "key":key,
            "revision":self.revision,"digest":self.digest}),
            2048,
        )
    }
    fn body<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_value(self.body.clone()).map_err(|_| AuthorityError::Unknown)
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RootState {
    format: String,
    profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner_binding_hash: Option<AgentReaderHash>,
    issuer_id: AgentReaderIssuerId,
    issuer_key: AgentReaderPublicKey,
    ready: bool,
    audiences: BTreeMap<AgentReaderAudienceId, AgentReaderHash>,
    entries: u64,
    reservations: u64,
    associations: u64,
    peer_sizes: BTreeMap<AgentReaderHash, u64>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PeerIdentity {
    profile: AgentReaderNativeProfileId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner_binding_hash: Option<AgentReaderHash>,
    agent: AgentReaderAgentId,
    audience: AgentReaderAudienceId,
    node_origin: AgentReaderHash,
    pin_genesis: AgentReaderHash,
}
impl PeerIdentity {
    fn hash(&self) -> Result<AgentReaderHash> {
        Ok(digest(
            b"consolecrypt.native.reader.peer.v1",
            &canonical(self, 2048)?,
        ))
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Event {
    Association {
        signed: AgentReaderSigned<AgentReaderAssociationV1>,
        target: AgentReaderTargetDescriptorV1,
        native_snapshot: Value,
        transport_binding: AgentReaderHash,
        transport_binding_body: ReaderPeerBinding,
    },
    Revoke {
        signed: AgentReaderSigned<AgentReaderRevocationV1>,
    },
    Installed {
        head: AgentReaderHeadV1,
    },
    Reserve {
        operation: AgentReaderOperationId,
        association: AgentReaderHash,
    },
    ChallengePrepared {
        operation: AgentReaderOperationId,
        freshness: AgentReaderSigned<AgentReaderFreshnessV1>,
    },
    Issued {
        operation: AgentReaderOperationId,
        grant: AgentReaderSigned<AgentReaderGrantV1>,
        freshness: AgentReaderSigned<AgentReaderFreshnessV1>,
        challenge: AgentReaderChallengeV1,
    },
    Retired {
        operation: AgentReaderOperationId,
    },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    at: AgentReaderTimestamp,
    previous: AgentReaderHash,
    event: Event,
    hash: AgentReaderHash,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PeerState {
    format: String,
    identity: PeerIdentity,
    pin: AgentReaderIssuerPinV1,
    events: Vec<Entry>,
}
type RegisteredApproval = (
    AgentReaderSigned<AgentReaderAssociationV1>,
    AgentReaderTargetDescriptorV1,
    Value,
    AgentReaderHash,
    ReaderPeerBinding,
);
struct Replay {
    head: AgentReaderHeadV1,
    installed: AgentReaderHeadV1,
    associations: BTreeMap<AgentReaderHash, RegisteredApproval>,
    revoked: BTreeSet<AgentReaderHash>,
    revoked_devices: BTreeSet<(AgentReaderDeviceId, u64, AgentReaderHash)>,
    reservations: BTreeMap<AgentReaderOperationId, (AgentReaderHash, u8)>,
    freshness: BTreeMap<AgentReaderOperationId, AgentReaderSigned<AgentReaderFreshnessV1>>,
}
impl PeerState {
    fn new(identity: PeerIdentity, pin: AgentReaderIssuerPinV1) -> Self {
        Self {
            format: "cc-native-reader-peer/v1".into(),
            identity,
            pin,
            events: vec![],
        }
    }
    fn push(&mut self, at: AgentReaderTimestamp, event: Event) -> Result<()> {
        if self.events.len() >= ENTRIES || self.events.last().is_some_and(|v| v.at > at) {
            return Err(AuthorityError::Quota);
        }
        let previous = self
            .events
            .last()
            .map_or(self.pin.genesis_hash()?, |v| v.hash);
        let hash = digest(
            b"consolecrypt.native.reader.event.v1",
            &canonical(
                &serde_json::json!({"at":at,"previous":previous,"event":event}),
                BYTES,
            )?,
        );
        self.events.push(Entry {
            at,
            previous,
            event,
            hash,
        });
        canonical(self, BYTES)?;
        self.replay()?;
        Ok(())
    }
    fn replay(&self) -> Result<Replay> {
        self.pin.validate()?;
        check_owner_binding(
            self.identity.owner_binding_hash,
            self.identity.owner_binding_hash,
        )?;
        if matches!(
            &self.pin.context.tenant,
            AgentReaderTenantV1::PersonalAccount { .. }
        ) != self.identity.owner_binding_hash.is_some()
        {
            return Err(AuthorityError::Unknown);
        }
        if self.format != "cc-native-reader-peer/v1"
            || self.events.len() > ENTRIES
            || self.identity.profile != self.pin.native_profile_id
            || self.identity.audience != self.pin.audience_id
            || self.identity.node_origin != self.pin.node_trust_origin_hash
            || self.identity.pin_genesis != self.pin.genesis_hash()?
        {
            return Err(AuthorityError::Unknown);
        }
        let genesis = AgentReaderHeadV1 {
            context: self.pin.context.clone(),
            issuer_id: self.pin.issuer_id,
            issuer_epoch: self.pin.issuer_epoch,
            authority_revision: 0,
            authority_hash: self.pin.genesis_hash()?,
        };
        let mut r = Replay {
            head: genesis.clone(),
            installed: genesis,
            associations: BTreeMap::new(),
            revoked: BTreeSet::new(),
            revoked_devices: BTreeSet::new(),
            reservations: BTreeMap::new(),
            freshness: BTreeMap::new(),
        };
        let mut previous = self.pin.genesis_hash()?;
        let mut at = AgentReaderTimestamp::new(0)?;
        for e in &self.events {
            let h = digest(
                b"consolecrypt.native.reader.event.v1",
                &canonical(
                    &serde_json::json!({"at":e.at,"previous":e.previous,"event":e.event}),
                    BYTES,
                )?,
            );
            if e.at < at || e.previous != previous || e.hash != h {
                return Err(AuthorityError::Unknown);
            }
            // Historical replay uses event.at, never today's Node/expiry/source.
            match &e.event {
                Event::Association {
                    signed,
                    target,
                    native_snapshot,
                    transport_binding,
                    transport_binding_body,
                } => {
                    signed.verify(self.pin.issuer_key)?;
                    target.validate()?;
                    let a = &signed.body;
                    check_binding(
                        transport_binding_body,
                        *transport_binding,
                        &self.pin,
                        a,
                        target,
                    )?;
                    if r.head != r.installed
                        || a.issued_at != e.at
                        || a.context != self.pin.context
                        || a.issuer_id != self.pin.issuer_id
                        || a.issuer_epoch != 1
                        || a.authority_revision != r.head.authority_revision + 1
                        || a.previous_authority_hash != r.head.authority_hash
                        || a.native_profile_id != self.identity.profile
                        || a.audience_id != self.identity.audience
                        || a.agent_id != self.identity.agent
                        || a.node_trust_origin_hash != self.identity.node_origin
                        || a.operation != target.operation()
                        || a.target_descriptor_hash != target.descriptor_hash()?
                        || a.dataset_kind != target.dataset.kind
                        || a.dataset_schema_version != target.dataset.schema_version
                        || a.device_key == self.pin.issuer_key
                        || a.expires_at > self.pin.expires_at
                        || *transport_binding == AgentReaderHash::from_bytes([0; 32])
                        || r.associations.len() >= ASSOCIATIONS
                    {
                        return Err(AuthorityError::Unknown);
                    }
                    live(a.not_before, a.expires_at, e.at)?;
                    check_owner_binding(
                        self.identity.owner_binding_hash,
                        snapshot_owner_binding(native_snapshot)?,
                    )?;
                    check_snapshot(native_snapshot, a)?;
                    let hash = a.body_digest()?;
                    if r.associations.contains_key(&hash)
                        || r.associations
                            .values()
                            .any(|(x, _, _, _, _)| x.body.association_id == a.association_id)
                    {
                        return Err(AuthorityError::Unknown);
                    }
                    r.head = advance_reader_head_v1(&r.head, hash)?;
                    r.associations.insert(
                        hash,
                        (
                            signed.clone(),
                            target.clone(),
                            native_snapshot.clone(),
                            *transport_binding,
                            transport_binding_body.clone(),
                        ),
                    );
                }
                Event::Revoke { signed } => {
                    signed.verify(self.pin.issuer_key)?;
                    let b = &signed.body;
                    if r.head != r.installed
                        || b.context != self.pin.context
                        || b.issuer_id != self.pin.issuer_id
                        || b.issuer_epoch != 1
                        || b.authority_revision != r.head.authority_revision + 1
                        || b.previous_authority_hash != r.head.authority_hash
                        || b.issued_at != e.at
                    {
                        return Err(AuthorityError::Unknown);
                    }
                    match &b.subject {
                        AgentReaderRevocationSubjectV1::Association {
                            association_id,
                            body_hash,
                        } => {
                            if r.associations.get(body_hash).is_none_or(|(a, _, _, _, _)| {
                                a.body.association_id != *association_id
                            }) || !r.revoked.insert(*body_hash)
                            {
                                return Err(AuthorityError::Unknown);
                            }
                        }
                        AgentReaderRevocationSubjectV1::Device {
                            device_id,
                            device_epoch,
                            key_fingerprint,
                        } => {
                            if !r.associations.values().any(|(a, _, _, _, _)| {
                                a.body.device_id == *device_id
                                    && a.body.device_epoch == *device_epoch
                                    && agent_reader_key_fingerprint(a.body.device_key)
                                        == *key_fingerprint
                            }) || !r.revoked_devices.insert((
                                *device_id,
                                *device_epoch,
                                *key_fingerprint,
                            )) {
                                return Err(AuthorityError::Unknown);
                            }
                        }
                    }
                    r.head = advance_reader_head_v1(&r.head, b.body_digest()?)?;
                }
                Event::Installed { head } => {
                    if *head != r.head || *head == r.installed {
                        return Err(AuthorityError::Unknown);
                    }
                    r.installed = head.clone();
                }
                Event::Reserve {
                    operation,
                    association,
                } => {
                    if r.head != r.installed
                        || r.reservations.len() >= RESERVATIONS
                        || r.reservations.contains_key(operation)
                    {
                        return Err(AuthorityError::Quota);
                    }
                    r.live_association(*association, e.at)?;
                    r.reservations.insert(*operation, (*association, 0));
                }
                Event::ChallengePrepared {
                    operation,
                    freshness,
                } => {
                    let (hash, phase) = r
                        .reservations
                        .get(operation)
                        .copied()
                        .ok_or(AuthorityError::Unknown)?;
                    if phase != 0 || r.head != r.installed {
                        return Err(AuthorityError::Unknown);
                    }
                    let (a, _, _, _, _) = r.live_association(hash, e.at)?;
                    freshness.verify(self.pin.issuer_key)?;
                    if freshness.body.head != r.head
                        || freshness.body.issued_at != e.at
                        || freshness.body.expires_at > a.body.expires_at
                        || freshness.body.expires_at > self.pin.expires_at
                    {
                        return Err(AuthorityError::Unknown);
                    }
                    live(freshness.body.issued_at, freshness.body.expires_at, e.at)?;
                    r.freshness.insert(*operation, freshness.clone());
                    r.reservations.insert(*operation, (hash, 1));
                }
                Event::Issued {
                    operation,
                    grant,
                    freshness,
                    challenge,
                } => {
                    let (hash, phase) = r
                        .reservations
                        .get(operation)
                        .copied()
                        .ok_or(AuthorityError::Unknown)?;
                    if phase != 1
                        || r.head != r.installed
                        || grant.body.operation_id != *operation
                        || challenge.operation_id != *operation
                    {
                        return Err(AuthorityError::Unknown);
                    }
                    let prepared = r.freshness.get(operation).ok_or(AuthorityError::Unknown)?;
                    if canonical(prepared, 16384)? != canonical(freshness, 16384)? {
                        return Err(AuthorityError::Unknown);
                    }
                    let (a, target, _, _, _) = r.live_association(hash, e.at)?;
                    validate_packet(
                        &self.pin, &r.head, a, target, grant, freshness, challenge, e.at,
                    )?;
                    r.reservations.insert(*operation, (hash, 2));
                }
                Event::Retired { operation } => {
                    let (hash, phase) = r
                        .reservations
                        .get(operation)
                        .copied()
                        .ok_or(AuthorityError::Unknown)?;
                    if phase == 3 {
                        return Err(AuthorityError::Unknown);
                    }
                    r.reservations.insert(*operation, (hash, 3));
                }
            }
            previous = e.hash;
            at = e.at;
        }
        Ok(r)
    }
}
impl Replay {
    fn live_association(
        &self,
        hash: AgentReaderHash,
        now: AgentReaderTimestamp,
    ) -> Result<&RegisteredApproval> {
        let value = self
            .associations
            .get(&hash)
            .ok_or(AuthorityError::Rejected)?;
        let a = &value.0.body;
        live(a.not_before, a.expires_at, now)?;
        if self.revoked.contains(&hash)
            || self.revoked_devices.contains(&(
                a.device_id,
                a.device_epoch,
                agent_reader_key_fingerprint(a.device_key),
            ))
        {
            return Err(AuthorityError::Rejected);
        }
        Ok(value)
    }
}
fn check_snapshot(v: &Value, a: &AgentReaderAssociationV1) -> Result<()> {
    if v.get("nativeProfileId") != Some(&serde_json::json!(a.native_profile_id)) {
        return Err(AuthorityError::Rejected);
    }
    for (key, expected) in [
        ("nativeHostId", serde_json::json!(a.native_host_id)),
        (
            "nativeHostRevision",
            serde_json::json!(a.native_host_revision),
        ),
        ("nativeSourceId", serde_json::json!(a.native_source_id)),
        (
            "nativeSourceRevision",
            serde_json::json!(a.native_source_revision),
        ),
        ("nativeBindingId", serde_json::json!(a.native_binding_id)),
        (
            "nativeBindingRevision",
            serde_json::json!(a.native_binding_revision),
        ),
        (
            "nativeBindingDigest",
            serde_json::json!(a.native_binding_digest),
        ),
        ("deviceId", serde_json::json!(a.device_id)),
        (
            "deviceSigningKey",
            serde_json::json!(a.device_key.as_bytes()),
        ),
    ] {
        if v.get(key) != Some(&expected) {
            return Err(AuthorityError::Rejected);
        }
    }
    // Recompute the closed Root binding BODY, including full Host/source/
    // binding snapshots. The full binding snapshot digest includes selector.
    let mut body = BTreeMap::from([
        ("format", serde_json::json!("cc-native-reader-binding/v1")),
        ("profileId", v["nativeProfileId"].clone()),
        ("hostId", v["nativeHostId"].clone()),
        ("hostRevision", v["nativeHostRevision"].clone()),
        ("hostBodyDigest", v["hostBodyDigest"].clone()),
        ("sourceId", v["nativeSourceId"].clone()),
        ("sourceRevision", v["nativeSourceRevision"].clone()),
        (
            "sourceConfigurationDigest",
            v["sourceConfigurationDigest"].clone(),
        ),
        ("bindingId", v["nativeBindingId"].clone()),
        ("bindingRevision", v["nativeBindingRevision"].clone()),
        ("bindingBodyDigest", v["bindingBodyDigest"].clone()),
    ]);
    let personal_owner = snapshot_owner_binding(v)?;
    if matches!(
        &a.context.tenant,
        AgentReaderTenantV1::PersonalAccount { .. }
    ) != personal_owner.is_some()
    {
        return Err(AuthorityError::Rejected);
    }
    if let Some(owner) = personal_owner {
        body.insert("format", serde_json::json!("cc-native-reader-binding/v2"));
        body.insert("personalOwner", serde_json::json!(owner));
    }
    for field in [
        "hostBodyDigest",
        "sourceConfigurationDigest",
        "bindingBodyDigest",
    ] {
        hash_text(v[field].as_str().ok_or(AuthorityError::Rejected)?)?;
    }
    if digest(
        b"consolecrypt.native.reader.binding.v1",
        &canonical(&body, 262144)?,
    ) != a.native_binding_digest
    {
        return Err(AuthorityError::Rejected);
    }
    Ok(())
}
fn check_binding(
    binding: &ReaderPeerBinding,
    hash: AgentReaderHash,
    pin: &AgentReaderIssuerPinV1,
    a: &AgentReaderAssociationV1,
    target: &AgentReaderTargetDescriptorV1,
) -> Result<()> {
    let index = match target.action {
        AgentReaderActionV1::HeartbeatRead => 0,
        AgentReaderActionV1::HistoryRead => 1,
    };
    if binding.digest().map_err(|_| AuthorityError::Unknown)? != hash
        || binding.native_profile_id != a.native_profile_id
        || binding.agent_id != a.agent_id
        || binding.audience_id != a.audience_id
        || binding.node_trust_origin_hash != a.node_trust_origin_hash
        || binding.issuer_pin_genesis_hash != pin.genesis_hash()?
        || binding.transport_peer_hash != a.transport_peer_hash
        || binding.agent_certificate_hash != a.agent_certificate_hash
        || binding.agent_key_epoch != a.agent_key_epoch
        || binding.target_hashes[index] != target.descriptor_hash()?
    {
        return Err(AuthorityError::Unknown);
    }
    Ok(())
}
fn validate_packet(
    pin: &AgentReaderIssuerPinV1,
    head: &AgentReaderHeadV1,
    a: &AgentReaderSigned<AgentReaderAssociationV1>,
    target: &AgentReaderTargetDescriptorV1,
    g: &AgentReaderSigned<AgentReaderGrantV1>,
    f: &AgentReaderSigned<AgentReaderFreshnessV1>,
    c: &AgentReaderChallengeV1,
    now: AgentReaderTimestamp,
) -> Result<()> {
    let packet = AgentReaderSigningPacketV1 {
        association: a.clone(),
        grant: g.clone(),
        freshness: f.clone(),
        challenge: c.clone(),
    };
    let bytes = canonical(&packet, AGENT_READER_FRAME_BYTES)?;
    let timestamp = chrono::DateTime::from_timestamp(now.seconds() as i64, 0)
        .ok_or(AuthorityError::Rejected)?;
    AgentReaderValidatedInputsV1::from_packet(
        &bytes,
        AgentReaderExpectedV1 {
            issuer_pin: pin.clone(),
            current_head: head.clone(),
            association_body: a.body.clone(),
            target: target.clone(),
        },
        timestamp,
    )?;
    Ok(())
}

// Review is not Serialize/Clone; only this module constructs it from actual
// original-context metadata and a delivery-created authenticated control peer.
pub(super) struct ReaderAssociationReview<'a> {
    snapshot: Value,
    pin: AgentReaderIssuerPinV1,
    target: AgentReaderTargetDescriptorV1,
    identity: PeerIdentity,
    transport_binding: AgentReaderHash,
    transport_binding_body: ReaderPeerBinding,
    transport_peer: AgentReaderHash,
    certificate: AgentReaderHash,
    key_epoch: u64,
    expires: Instant,
    _slot: OwnedSemaphorePermit,
    peer: &'a VerifiedReaderControlPeer,
    original: &'a ReaderOriginalContext,
}
pub(super) struct ReservedReaderAction<'a> {
    peer: AgentReaderHash,
    operation: AgentReaderOperationId,
    association: AgentReaderHash,
    deadline: Instant,
    challenge_attempted: bool,
    original: &'a ReaderOriginalContext,
}
pub(super) struct ReaderIssuerPublic {
    pub issuer_id: AgentReaderIssuerId,
    pub issuer_key: AgentReaderPublicKey,
}
/// Reconnection expectation from verified LOCAL durable records. This view is
/// public metadata, not an action/permission or an authenticated remote ack.
pub(super) struct RegisteredReaderAssociation {
    pub issuer_pin: AgentReaderIssuerPinV1,
    pub latest_signed_head: AgentReaderHeadV1,
    pub installed_head: AgentReaderHeadV1,
    pub association: AgentReaderSigned<AgentReaderAssociationV1>,
    pub target: AgentReaderTargetDescriptorV1,
    pub peer_digest: AgentReaderHash,
    pub transport_binding_hash: AgentReaderHash,
    pub transport_binding: ReaderPeerBinding,
}
fn registered_value(state:&PeerState,r:&Replay,current:&ReaderContextSnapshot,
    peer_digest:AgentReaderHash,association_hash:AgentReaderHash,now:AgentReaderTimestamp)->Result<RegisteredReaderAssociation> {
    if state.identity.hash()?!=peer_digest {return Err(AuthorityError::Rejected);}
    if r.head!=r.installed {return Err(AuthorityError::Unavailable);}
    let (association,target,snapshot,hash,binding)=r.live_association(association_hash,now)?;
    if !stable_snapshot_matches(snapshot,current)? {return Err(AuthorityError::Rejected);}
    check_binding(binding,*hash,&state.pin,&association.body,target)?;
    Ok(RegisteredReaderAssociation{issuer_pin:state.pin.clone(),latest_signed_head:r.head.clone(),installed_head:r.installed.clone(),
        association:association.clone(),target:target.clone(),peer_digest,transport_binding_hash:*hash,transport_binding:binding.clone()})
}
pub(super) struct ReaderRevocationReview<'a> {
    peer: AgentReaderHash,
    snapshot: Value,
    subject: AgentReaderRevocationSubjectV1,
    expires: Instant,
    _slot: OwnedSemaphorePermit,
    original: &'a ReaderOriginalContext,
}
pub(super) enum ReaderRevokeSelection {
    Association,
    OriginalDevice,
}
fn review_slot() -> Result<OwnedSemaphorePermit> {
    static SLOTS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    SLOTS
        .get_or_init(|| Arc::new(Semaphore::new(8)))
        .clone()
        .try_acquire_owned()
        .map_err(|_| AuthorityError::Quota)
}
pub(super) struct PendingReaderAdministration {
    pub peer_digest: AgentReaderHash,
    pub head: AgentReaderHeadV1,
    pub statement: SignedReaderAdmin,
}
pub(super) struct LocallyIssuedReaderPacket<'a> {
    pub packet: AgentReaderSigningPacketV1,
    pub retirement: ReservedReaderAction<'a>,
}
impl std::fmt::Debug for ReaderAssociationReview<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReaderAssociationReview([redacted])")
    }
}
impl std::fmt::Debug for ReservedReaderAction<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReservedReaderAction([redacted])")
    }
}

pub(super) struct NativeReaderAuthority {
    storage: Storage,
    secure: Arc<dyn SecureStore>,
    profile: String,
    owner_binding_hash: Option<AgentReaderHash>,
    // Fresh RAW SystemTime observations; not a persisted trusted OS clock floor.
    clock: Mutex<Duration>,
}
impl NativeReaderAuthority {
    pub async fn for_original_context(app: &AppCore, ctx: &ReaderOriginalContext) -> Result<Self> {
        let owner_binding_hash = expected_owner_binding(ctx)?;
        let (storage, secure) = ctx
            .with_held(app, |h| {
                Ok((h.owner.storage()?, h.owner.ctx.secure.clone()))
            })
            .await
            .map_err(|_| AuthorityError::Unavailable)?;
        // In-memory tests may exercise pure transition logic, but this concrete
        // authority backend requires an actual per-profile encrypted database.
        if storage.path().is_none() {
            return Err(AuthorityError::Unavailable);
        }
        Ok(Self {
            storage,
            secure,
            profile: ctx.snapshot().native_profile_id.clone(),
            owner_binding_hash,
            clock: Mutex::new(Duration::ZERO),
        })
    }
    async fn check(
        &self,
        app: &AppCore,
        ctx: &ReaderOriginalContext,
    ) -> Result<AgentReaderTimestamp> {
        check_owner_binding(self.owner_binding_hash, expected_owner_binding(ctx)?)?;
        check_owner_binding(
            self.owner_binding_hash,
            snapshot_owner_binding(&snapshot_value(ctx.snapshot())?)?,
        )?;
        if ctx.snapshot().native_profile_id != self.profile {
            return Err(AuthorityError::Rejected);
        }
        ctx.check(app)
            .await
            .map_err(|_| AuthorityError::Unavailable)?;
        let raw = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| AuthorityError::Unavailable)?;
        let mut floor = self.clock.lock().map_err(|_| AuthorityError::Unavailable)?;
        if raw < *floor || raw.as_secs() > AGENT_READER_MAX_TIMESTAMP {
            return Err(AuthorityError::Unavailable);
        }
        *floor = raw;
        Ok(AgentReaderTimestamp::new(raw.as_secs())?)
    }
    async fn action_check(
        &self,
        app: &AppCore,
        ctx: &ReaderOriginalContext,
        deadline: Instant,
        published_state: bool,
    ) -> Result<AgentReaderTimestamp> {
        let error = if published_state {
            AuthorityError::Unknown
        } else {
            AuthorityError::Unavailable
        };
        if Instant::now() >= deadline {
            return Err(error);
        }
        let now = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            self.check(app, ctx),
        )
        .await
        .map_err(|_| error)?
        .map_err(|e| {
            if published_state {
                AuthorityError::Unknown
            } else {
                e
            }
        })?;
        if Instant::now() >= deadline {
            return Err(error);
        }
        Ok(now)
    }
    fn root_key(&self) -> String {
        format!("cc.native.reader.root.v1/{}", self.profile)
    }
    fn peer_key(&self, hash: AgentReaderHash) -> String {
        format!(
            "cc.native.reader.peer.v1/{}/{}",
            self.profile,
            hex::encode(hash.as_bytes())
        )
    }
    fn issuer_key(&self) -> String {
        format!("profiles/{}/reader-issuer-v1", self.profile)
    }
    fn witness_key(&self, key: &str) -> String {
        if key == self.root_key() {
            format!("profiles/{}/reader-root-v1", self.profile)
        } else {
            format!(
                "profiles/{}/reader-head/{}",
                self.profile,
                key.rsplit('/').next().unwrap_or("invalid")
            )
        }
    }
    async fn os<T: Send + 'static>(
        &self,
        deadline: Instant,
        f: impl FnOnce(Arc<dyn SecureStore>) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        static SLOTS: OnceLock<Arc<Semaphore>> = OnceLock::new();
        let permit = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            SLOTS
                .get_or_init(|| Arc::new(Semaphore::new(4)))
                .clone()
                .acquire_owned(),
        )
        .await
        .map_err(|_| AuthorityError::Unavailable)?
        .map_err(|_| AuthorityError::Unavailable)?;
        let store = self.secure.clone();
        // Permit remains in the OS job if awaiting caller times out/cancels.
        let job = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            f(store)
        });
        tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), job)
            .await
            .map_err(|_| AuthorityError::Unknown)?
            .map_err(|_| AuthorityError::Unknown)?
    }
    async fn raw(&self, key: String, deadline: Instant) -> Result<Option<Vec<u8>>> {
        let work = self
            .storage
            .call(move |db| db.read(|tx| tx.reader_authority_get_bounded(&key, BYTES)));
        tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), work)
            .await
            .map_err(|_| AuthorityError::Unavailable)?
            .map_err(|_| AuthorityError::Unavailable)
    }
    async fn load(&self, key: &str, deadline: Instant) -> Result<Option<(Vec<u8>, Record)>> {
        let bytes = self.raw(key.into(), deadline).await?;
        let witness_key = self.witness_key(key);
        let witness = self
            .os(deadline, move |s| {
                s.get(&witness_key)
                    .map(|v| v.map(|v| v.expose_secret().to_vec()))
                    .map_err(|_| AuthorityError::Unavailable)
            })
            .await?;
        match (bytes, witness) {
            (None, None) => Ok(None),
            (Some(bytes), Some(w)) => {
                let r: Record = decode(&bytes)?;
                r.check()?;
                if r.witness_pending || r.witness(key)? != w {
                    return Err(AuthorityError::Unknown);
                }
                Ok(Some((bytes, r)))
            }
            _ => Err(AuthorityError::Unknown),
        }
    }
    // Caller already validated complete expected records/witnesses. CAS checks
    // ALL rows before the first put; a failed cross-store phase is quarantined.
    async fn commit(
        &self,
        changes: Vec<(String, Option<Vec<u8>>, Record)>,
        deadline: Instant,
    ) -> Result<()> {
        let mut pending = Vec::new();
        let mut ready = Vec::new();
        let mut witnesses = Vec::new();
        for (key, old, record) in changes {
            record.check()?;
            let bytes = canonical(&record, BYTES)?;
            witnesses.push((self.witness_key(&key), record.witness(&key)?));
            let mut done = record;
            done.witness_pending = false;
            ready.push((key.clone(), Some(bytes.clone()), canonical(&done, BYTES)?));
            pending.push((key, old, bytes));
        }
        self.cas(pending, deadline).await?;
        self.os(deadline, move |s| {
            for (key, bytes) in witnesses {
                s.set(&key, &bytes).map_err(|_| AuthorityError::Unknown)?;
                let read = s
                    .get(&key)
                    .map_err(|_| AuthorityError::Unknown)?
                    .ok_or(AuthorityError::Unknown)?;
                if read.expose_secret() != bytes.as_slice() {
                    return Err(AuthorityError::Unknown);
                }
            }
            Ok(())
        })
        .await
        .map_err(|_| AuthorityError::Unknown)?;
        self.cas(ready.clone(), deadline)
            .await
            .map_err(|_| AuthorityError::Unknown)?;
        for (key, _, expected) in ready {
            if self
                .load(&key, deadline)
                .await
                .map_err(|_| AuthorityError::Unknown)?
                .map(|(b, _)| b)
                != Some(expected)
            {
                return Err(AuthorityError::Unknown);
            }
        }
        Ok(())
    }
    async fn cas(
        &self,
        rows: Vec<(String, Option<Vec<u8>>, Vec<u8>)>,
        deadline: Instant,
    ) -> Result<()> {
        let work = self.storage.call(move |db| {
            db.reader_authority_write::<_, AuthorityError>(|tx| {
                for (key, old, _) in &rows {
                    if tx.reader_authority_get_bounded(key, BYTES)? != *old {
                        return Err(AuthorityError::Rejected);
                    }
                }
                for (key, _, new) in &rows {
                    tx.reader_authority_put_bounded(key, new, BYTES)?;
                }
                Ok(())
            })
        });
        match tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), work).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(ReaderAuthorityWriteError::Rejected(e))) => Err(e),
            Ok(Err(ReaderAuthorityWriteError::BeforeWrite(_))) => Err(AuthorityError::Unavailable),
            _ => Err(AuthorityError::Unknown),
        }
    }
    async fn root(&self, deadline: Instant) -> Result<(Vec<u8>, Record, RootState)> {
        let (bytes, record) = self
            .load(&self.root_key(), deadline)
            .await?
            .ok_or(AuthorityError::Unavailable)?;
        let root: RootState = record.body()?;
        check_owner_binding(self.owner_binding_hash, root.owner_binding_hash)?;
        if root.format != "cc-native-reader-root/v1"
            || root.profile != self.profile
            || !root.ready
            || root.audiences.len() > ASSOCIATIONS
            || root.entries > ENTRIES as u64
            || root.reservations > RESERVATIONS as u64
            || root.associations > ASSOCIATIONS as u64
            || root.peer_sizes.len() != root.audiences.len()
            || root
                .audiences
                .values()
                .any(|v| !root.peer_sizes.contains_key(v))
            || root
                .peer_sizes
                .values()
                .try_fold(0u64, |a, b| a.checked_add(*b))
                .is_none_or(|v| v > BYTES as u64)
        {
            return Err(AuthorityError::Unknown);
        }
        root.issuer_key.validate()?;
        Ok((bytes, record, root))
    }
    async fn commit_peer(
        &self,
        ctx: &ReaderOriginalContext,
        old: Option<(Vec<u8>, Record)>,
        state: &PeerState,
        deadline: Instant,
    ) -> Result<()> {
        check_owner_binding(self.owner_binding_hash, expected_owner_binding(ctx)?)?;
        check_owner_binding(self.owner_binding_hash, state.identity.owner_binding_hash)?;
        ctx.check_agent_context(&state.pin.context)
            .map_err(|_| AuthorityError::Rejected)?;
        let (root_bytes, root_record, mut root) = self.root(deadline).await?;
        let hash = state.identity.hash()?;
        let key = self.peer_key(hash);
        if state.pin.issuer_key != root.issuer_key
            || state.pin.issuer_id != root.issuer_id
            || root
                .audiences
                .get(&state.identity.audience)
                .is_some_and(|v| *v != hash)
        {
            return Err(AuthorityError::Rejected);
        }
        let new = state.replay()?;
        let (old_entries, old_reservations, old_associations) = if let Some((bytes, r)) = &old {
            let p: PeerState = r.body()?;
            check_owner_binding(self.owner_binding_hash, p.identity.owner_binding_hash)?;
            let replay = p.replay()?;
            if p.identity.hash() != Ok(hash)
                || root.peer_sizes.get(&hash) != Some(&(bytes.len() as u64))
            {
                return Err(AuthorityError::Unknown);
            }
            (
                p.events.len() as u64,
                replay.reservations.len() as u64,
                replay.associations.len() as u64,
            )
        } else {
            if root.audiences.contains_key(&state.identity.audience)
                || root.peer_sizes.contains_key(&hash)
            {
                return Err(AuthorityError::Unknown);
            }
            (0, 0, 0)
        };
        fn count(total: u64, old: u64, new: u64, cap: u64) -> Result<u64> {
            let next = total
                .checked_sub(old)
                .and_then(|v| v.checked_add(new))
                .ok_or(AuthorityError::Unknown)?;
            if next > cap {
                return Err(AuthorityError::Quota);
            }
            Ok(next)
        }
        root.entries = count(
            root.entries,
            old_entries,
            state.events.len() as u64,
            ENTRIES as u64,
        )?;
        root.reservations = count(
            root.reservations,
            old_reservations,
            new.reservations.len() as u64,
            RESERVATIONS as u64,
        )?;
        root.associations = count(
            root.associations,
            old_associations,
            new.associations.len() as u64,
            ASSOCIATIONS as u64,
        )?;
        let next = Record::new(old.as_ref().map(|(_, r)| r), state)?;
        let mut done = next.clone();
        done.witness_pending = false;
        root.audiences.insert(state.identity.audience, hash);
        root.peer_sizes
            .insert(hash, canonical(&done, BYTES)?.len() as u64);
        let root_next = Record::new(Some(&root_record), &root)?;
        let total = root
            .peer_sizes
            .values()
            .try_fold(0u64, |a, b| a.checked_add(*b))
            .ok_or(AuthorityError::Quota)?;
        // Inclusive logical current row + witness envelope, not SQLite/WAL/page
        // allocation. 512 bytes per current OS witness conservatively fits the
        // fixed row key/revision/hash frame; issuer secret is exactly 37 bytes.
        let overhead = canonical(&root_next, BYTES)?.len() as u64 + 1 // ready false is one byte longer than pending true
            + (root.peer_sizes.len() as u64 + 1) * 512
            + 37;
        if total.checked_add(overhead).is_none_or(|v| v > BYTES as u64) {
            return Err(AuthorityError::Quota);
        }
        self.commit(
            vec![
                (key, old.map(|(b, _)| b), next),
                (self.root_key(), Some(root_bytes), root_next),
            ],
            deadline,
        )
        .await
    }
    async fn key(&self, root: &RootState, deadline: Instant) -> Result<ReaderIssuerSecretV1> {
        let name = self.issuer_key();
        let expected = root.issuer_key;
        self.os(deadline, move |s| {
            let bytes = s
                .get(&name)
                .map_err(|_| AuthorityError::Unavailable)?
                .ok_or(AuthorityError::Unavailable)?;
            let key = ReaderIssuerSecretV1::from_secure_store_bytes(bytes.expose_secret())?;
            if key.public_key() != expected {
                return Err(AuthorityError::Unknown);
            }
            Ok(key)
        })
        .await
    }
    pub async fn issuer_public(
        &self,
        app: &AppCore,
        ctx: &ReaderOriginalContext,
    ) -> Result<ReaderIssuerPublic> {
        self.check(app, ctx).await?;
        let (_, _, root) = self.root(ctx.deadline()).await?;
        // A missing/replaced pinned private key is not a fresh issuer invitation.
        self.key(&root, ctx.deadline()).await?;
        self.check(app, ctx).await?;
        Ok(ReaderIssuerPublic {
            issuer_id: root.issuer_id,
            issuer_key: root.issuer_key,
        })
    }
    pub async fn initialize_issuer_explicit(
        &self,
        app: &AppCore,
        ctx: &ReaderOriginalContext,
    ) -> Result<ReaderIssuerPublic> {
        self.check(app, ctx).await?;
        if self.load(&self.root_key(), ctx.deadline()).await?.is_some() {
            return Err(AuthorityError::Rejected);
        }
        let name = self.issuer_key();
        let key = self
            .os(ctx.deadline(), move |s| {
                if s.get(&name)
                    .map_err(|_| AuthorityError::Unavailable)?
                    .is_some()
                {
                    return Err(AuthorityError::Unknown);
                }
                Ok(ReaderIssuerSecretV1::generate()?)
            })
            .await?;
        if key.public_key().as_bytes() == &ctx.snapshot().device_signing_key {
            return Err(AuthorityError::Rejected);
        }
        self.check(app, ctx).await?;
        let mut root = RootState {
            format: "cc-native-reader-root/v1".into(),
            profile: self.profile.clone(),
            owner_binding_hash: self.owner_binding_hash,
            issuer_id: AgentReaderIssuerId::new(&Uuid::new_v4().to_string())?,
            issuer_key: key.public_key(),
            ready: false,
            audiences: BTreeMap::new(),
            entries: 0,
            reservations: 0,
            associations: 0,
            peer_sizes: BTreeMap::new(),
        };
        let pending = Record::new(None, &root)?;
        self.commit(vec![(self.root_key(), None, pending)], ctx.deadline())
            .await?;
        // Only this winning invocation retains the candidate. Pending state is
        // never resumed by generating/importing/replacing an issuer automatically.
        let name = self.issuer_key();
        let secret = key.to_secure_store_bytes();
        let expected = key.public_key();
        self.os(ctx.deadline(), move |s| {
            if s.get(&name).map_err(|_| AuthorityError::Unknown)?.is_some() {
                return Err(AuthorityError::Unknown);
            }
            s.set(&name, &secret).map_err(|_| AuthorityError::Unknown)?;
            let value = s
                .get(&name)
                .map_err(|_| AuthorityError::Unknown)?
                .ok_or(AuthorityError::Unknown)?;
            if ReaderIssuerSecretV1::from_secure_store_bytes(value.expose_secret())?.public_key()
                != expected
            {
                return Err(AuthorityError::Unknown);
            }
            Ok(())
        })
        .await
        .map_err(|_| AuthorityError::Unknown)?;
        let (bytes, old) = self
            .load(&self.root_key(), ctx.deadline())
            .await?
            .ok_or(AuthorityError::Unknown)?;
        let prior: RootState = old.body()?;
        check_owner_binding(self.owner_binding_hash, prior.owner_binding_hash)?;
        if prior.ready || prior.issuer_id != root.issuer_id || prior.issuer_key != root.issuer_key {
            return Err(AuthorityError::Unknown);
        }
        root.ready = true;
        let next = Record::new(Some(&old), &root)?;
        self.commit(vec![(self.root_key(), Some(bytes), next)], ctx.deadline())
            .await?;
        self.check(app, ctx)
            .await
            .map_err(|_| AuthorityError::Unknown)?;
        Ok(ReaderIssuerPublic {
            issuer_id: root.issuer_id,
            issuer_key: expected,
        })
    }
    pub async fn review_association<'a>(
        &self,
        app: &AppCore,
        ctx: &'a ReaderOriginalContext,
        pin: AgentReaderIssuerPinV1,
        target: AgentReaderTargetDescriptorV1,
        peer: &'a VerifiedReaderControlPeer,
    ) -> Result<ReaderAssociationReview<'a>> {
        if !peer.is_live_for(ctx) {
            return Err(AuthorityError::Rejected);
        }
        let now = self.check(app, ctx).await?;
        ctx.check_agent_context(&pin.context)
            .map_err(|_| AuthorityError::Rejected)?;
        pin.validate()?;
        target.validate()?;
        let (_, _, root) = self.root(ctx.deadline()).await?;
        let b = peer.binding();
        let s = ctx.snapshot();
        if !peer.is_live_for(ctx)
            || pin.issuer_id != root.issuer_id
            || pin.issuer_key != root.issuer_key
            || pin.native_profile_id.as_uuid().to_string() != s.native_profile_id
            || b.native_profile_id != pin.native_profile_id
            || b.agent_id != target.agent_id
            || b.audience_id != pin.audience_id
            || b.node_trust_origin_hash != pin.node_trust_origin_hash
            || b.issuer_pin_genesis_hash != pin.genesis_hash()?
            || !b.target_hashes.contains(&target.descriptor_hash()?)
            || b.agent_key_epoch == 0
            || pin.issuer_key.as_bytes() == &s.device_signing_key
        {
            return Err(AuthorityError::Rejected);
        }
        live(AgentReaderTimestamp::new(0)?, pin.expires_at, now)?;
        self.check(app, ctx).await?;
        Ok(ReaderAssociationReview {
            snapshot: snapshot_value(s)?,
            identity: PeerIdentity {
                profile: pin.native_profile_id,
                owner_binding_hash: self.owner_binding_hash,
                agent: target.agent_id,
                audience: pin.audience_id,
                node_origin: pin.node_trust_origin_hash,
                pin_genesis: pin.genesis_hash()?,
            },
            pin,
            target,
            transport_binding: transport_hash(peer)?,
            transport_binding_body: peer.binding().clone(),
            transport_peer: b.transport_peer_hash,
            certificate: b.agent_certificate_hash,
            key_epoch: b.agent_key_epoch,
            expires: ctx.deadline().min(Instant::now() + Duration::from_secs(60)),
            _slot: review_slot()?,
            peer,
            original: ctx,
        })
    }
    pub async fn approve_association_once(
        &self,
        app: &AppCore,
        ctx: &ReaderOriginalContext,
        review: ReaderAssociationReview<'_>,
    ) -> Result<PendingReaderAdministration> {
        if !std::ptr::eq(review.original, ctx) {
            return Err(AuthorityError::Rejected);
        }
        if !review.peer.is_live_for(ctx) {
            return Err(AuthorityError::Rejected);
        }
        let now = self.check(app, ctx).await?;
        ctx.check_agent_context(&review.pin.context)
            .map_err(|_| AuthorityError::Rejected)?;
        check_owner_binding(self.owner_binding_hash, review.identity.owner_binding_hash)?;
        if Instant::now() >= review.expires
            || review.snapshot != snapshot_value(ctx.snapshot())?
            || !review.peer.is_live_for(ctx)
            || transport_hash(review.peer)? != review.transport_binding
        {
            return Err(AuthorityError::Rejected);
        }
        let (_, _, root) = self.root(ctx.deadline()).await?;
        if review.pin.issuer_id != root.issuer_id || review.pin.issuer_key != root.issuer_key {
            return Err(AuthorityError::Rejected);
        }
        let peer_hash = review.identity.hash()?;
        let key = self.peer_key(peer_hash);
        if root
            .audiences
            .get(&review.pin.audience_id)
            .is_some_and(|v| *v != peer_hash)
        {
            return Err(AuthorityError::Rejected);
        }
        let old = self.load(&key, ctx.deadline()).await?;
        let mut state = if let Some((_, r)) = &old {
            let p: PeerState = r.body()?;
            if p.identity.hash() != Ok(peer_hash) || p.pin != review.pin {
                return Err(AuthorityError::Unknown);
            }
            p
        } else {
            if root.audiences.contains_key(&review.pin.audience_id) {
                return Err(AuthorityError::Unknown);
            }
            PeerState::new(review.identity.clone(), review.pin.clone())
        };
        let replay = state.replay()?;
        if replay.head != replay.installed {
            return Err(AuthorityError::Unavailable);
        }
        let s = ctx.snapshot();
        let expires = AgentReaderTimestamp::new(
            now.checked_add(3600)?
                .seconds()
                .min(review.pin.expires_at.seconds()),
        )?;
        let body = AgentReaderAssociationV1 {
            version: 1,
            issuer_id: review.pin.issuer_id,
            issuer_epoch: 1,
            authority_revision: replay
                .head
                .authority_revision
                .checked_add(1)
                .ok_or(AuthorityError::Quota)?,
            previous_authority_hash: replay.head.authority_hash,
            context: review.pin.context.clone(),
            native_profile_id: review.pin.native_profile_id,
            native_host_id: AgentReaderNativeHostId::new(&s.native_host_id)?,
            native_host_revision: s.native_host_revision,
            native_binding_id: AgentReaderNativeBindingId::new(&s.native_binding_id)?,
            native_binding_revision: s.native_binding_revision,
            native_binding_digest: hash_text(&s.native_binding_digest)?,
            native_source_id: s.native_source_id.clone(),
            native_source_revision: s.native_source_revision,
            association_id: AgentReaderAssociationId::new(&Uuid::new_v4().to_string())?,
            agent_id: review.target.agent_id,
            node_trust_origin_hash: review.pin.node_trust_origin_hash,
            agent_certificate_hash: review.certificate,
            agent_key_epoch: review.key_epoch,
            device_id: AgentReaderDeviceId::new(&s.device_id)?,
            device_epoch: 1,
            device_key: AgentReaderPublicKey::from_bytes(s.device_signing_key),
            audience_id: review.pin.audience_id,
            transport_peer_hash: review.transport_peer,
            operation: review.target.operation(),
            dataset_kind: review.target.dataset.kind.clone(),
            dataset_schema_version: review.target.dataset.schema_version,
            target_descriptor_hash: review.target.descriptor_hash()?,
            issued_at: now,
            not_before: now,
            expires_at: expires,
        };
        let issuer = self.key(&root, ctx.deadline()).await?;
        let signed = ctx
            .with_held(app, |_| Ok(issuer.sign_association(body, &review.pin)))
            .await
            .map_err(|_| AuthorityError::Unavailable)??;
        state.push(
            now,
            Event::Association {
                signed: signed.clone(),
                target: review.target,
                native_snapshot: review.snapshot,
                transport_binding: review.transport_binding,
                transport_binding_body: review.transport_binding_body,
            },
        )?;
        let signed_head = state.replay()?.head;
        self.check(app, ctx).await?;
        self.commit_peer(ctx, old, &state, ctx.deadline()).await?;
        let final_now = self
            .check(app, ctx)
            .await
            .map_err(|_| AuthorityError::Unknown)?;
        live(signed.body.not_before, signed.body.expires_at, final_now)
            .map_err(|_| AuthorityError::Unknown)?;
        if !review.peer.is_live_for(ctx) {
            return Err(AuthorityError::Unknown);
        }
        Ok(PendingReaderAdministration {
            peer_digest: peer_hash,
            head: signed_head,
            statement: SignedReaderAdmin::InstallAssociation { signed },
        })
    }
    async fn peer(
        &self,
        ctx: &ReaderOriginalContext,
        hash: AgentReaderHash,
        deadline: Instant,
    ) -> Result<(Vec<u8>, Record, PeerState, Replay)> {
        check_owner_binding(self.owner_binding_hash, expected_owner_binding(ctx)?)?;
        let (_, _, root) = self.root(deadline).await?;
        let key = self.peer_key(hash);
        let (bytes, record) = self
            .load(&key, deadline)
            .await?
            .ok_or(AuthorityError::Unavailable)?;
        let state: PeerState = record.body()?;
        check_owner_binding(self.owner_binding_hash, state.identity.owner_binding_hash)?;
        ctx.check_agent_context(&state.pin.context)
            .map_err(|_| AuthorityError::Rejected)?;
        if state.identity.hash() != Ok(hash)
            || root.audiences.get(&state.identity.audience) != Some(&hash)
            || state.pin.issuer_id != root.issuer_id
            || state.pin.issuer_key != root.issuer_key
        {
            return Err(AuthorityError::Unknown);
        }
        let replay = state.replay()?;
        Ok((bytes, record, state, replay))
    }
    pub async fn registered_association(
        &self,
        app: &AppCore,
        ctx: &ReaderOriginalContext,
        peer_digest: AgentReaderHash,
        association_hash: AgentReaderHash,
    ) -> Result<RegisteredReaderAssociation> {
        let now = self.check(app, ctx).await?;
        let (_, _, state, r) = self.peer(ctx, peer_digest, ctx.deadline()).await?;
        let value=registered_value(&state,&r,ctx.snapshot(),peer_digest,association_hash,now)?;
        let final_now = self.check(app, ctx).await?;
        live(
            value.association.body.not_before,
            value.association.body.expires_at,
            final_now,
        )?;
        Ok(value)
    }
    /// Bounded existing LOCAL records only, not a remote observation or new authority.
    pub async fn known_associations(&self,app:&AppCore,ctx:&ReaderOriginalContext)->Result<(Vec<RegisteredReaderAssociation>,bool)> {
        self.check(app,ctx).await?;
        let (_,before,root)=self.root(ctx.deadline()).await?;
        let mut result=Vec::new();let mut truncated=false;let mut candidates=0usize;
        'peers:for hash in root.audiences.values() {
            let (_,_,_,replay)=match self.peer(ctx,*hash,ctx.deadline()).await {
                Ok(v)=>v,Err(AuthorityError::Rejected)=>continue,Err(e)=>return Err(e),
            };
            if replay.head!=replay.installed {continue;}
            for association in replay.associations.keys() {
                candidates+=1;if candidates>ASSOCIATIONS {return Err(AuthorityError::Unknown);}
                let value=match self.registered_association(app,ctx,*hash,*association).await {
                    Ok(v)=>v,Err(AuthorityError::Rejected)=>continue,Err(e)=>return Err(e),
                };
                if result.len()==16 {truncated=true;break 'peers;}
                result.push(value);
            }
        }
        let (_,after,_)=self.root(ctx.deadline()).await?;
        if before.revision!=after.revision||before.digest!=after.digest {return Err(AuthorityError::Unavailable);}
        let now=self.check(app,ctx).await?;
        result.retain(|v|v.association.body.not_before<=now&&now<v.association.body.expires_at);
        Ok((result,truncated))
    }
    pub async fn confirm_admin_applied(
        &self,
        app: &AppCore,
        ctx: &ReaderOriginalContext,
        hash: AgentReaderHash,
        peer: &VerifiedReaderControlPeer,
        reply: &BoundReaderControlReply<AgentReaderHeadV1>,
    ) -> Result<()> {
        if !peer.is_live_for(ctx) {
            return Err(AuthorityError::Rejected);
        }
        let now = self.check(app, ctx).await?;
        let head = reply
            .metadata_for(peer)
            .map_err(|_| AuthorityError::Unavailable)?;
        let (bytes, record, mut state, replay) = self.peer(ctx, hash, ctx.deadline()).await?;
        check_peer(peer, &state, ctx)?;
        let expected_transport = state
            .events
            .iter()
            .rev()
            .find_map(|e| match &e.event {
                Event::Association {
                    transport_binding, ..
                } => Some(*transport_binding),
                _ => None,
            })
            .ok_or(AuthorityError::Unknown)?;
        if transport_hash(peer)? != expected_transport {
            return Err(AuthorityError::Rejected);
        }
        if head != &replay.head || replay.head == replay.installed {
            return Err(AuthorityError::Rejected);
        }
        state.push(now, Event::Installed { head: head.clone() })?;
        self.commit_peer(ctx, Some((bytes, record)), &state, ctx.deadline())
            .await?;
        self.check(app, ctx)
            .await
            .map_err(|_| AuthorityError::Unknown)?;
        Ok(())
    }
    pub async fn review_revocation<'a>(
        &self,
        app: &AppCore,
        ctx: &'a ReaderOriginalContext,
        hash: AgentReaderHash,
        association: AgentReaderHash,
        selection: ReaderRevokeSelection,
    ) -> Result<ReaderRevocationReview<'a>> {
        self.check(app, ctx).await?;
        let (_, _, _, r) = self.peer(ctx, hash, ctx.deadline()).await?;
        let (a, _, snapshot, _, _) = r
            .associations
            .get(&association)
            .ok_or(AuthorityError::Rejected)?;
        if !stable_snapshot_matches(snapshot, ctx.snapshot())? {
            return Err(AuthorityError::Rejected);
        }
        let subject = match selection {
            ReaderRevokeSelection::OriginalDevice => AgentReaderRevocationSubjectV1::Device {
                device_id: a.body.device_id,
                device_epoch: a.body.device_epoch,
                key_fingerprint: agent_reader_key_fingerprint(a.body.device_key),
            },
            ReaderRevokeSelection::Association => AgentReaderRevocationSubjectV1::Association {
                association_id: a.body.association_id,
                body_hash: association,
            },
        };
        Ok(ReaderRevocationReview {
            peer: hash,
            snapshot: snapshot_value(ctx.snapshot())?,
            subject,
            expires: ctx.deadline().min(Instant::now() + Duration::from_secs(60)),
            _slot: review_slot()?,
            original: ctx,
        })
    }
    /// Parent must cancel its matching original-context attempt generation on
    /// durable local revocation, including an Unknown return. No automatic remote
    /// resend, old-head fallback or assertion that remote revocation was applied.
    pub async fn approve_revoke_once(
        &self,
        app: &AppCore,
        ctx: &ReaderOriginalContext,
        review: ReaderRevocationReview<'_>,
    ) -> Result<PendingReaderAdministration> {
        if !std::ptr::eq(review.original, ctx) {
            return Err(AuthorityError::Rejected);
        }
        let now = self.check(app, ctx).await?;
        if Instant::now() >= review.expires || snapshot_value(ctx.snapshot())? != review.snapshot {
            return Err(AuthorityError::Rejected);
        }
        let (bytes, record, mut state, r) = self.peer(ctx, review.peer, ctx.deadline()).await?;
        if r.head != r.installed {
            return Err(AuthorityError::Unavailable);
        }
        let (_, _, root) = self.root(ctx.deadline()).await?;
        let issuer = self.key(&root, ctx.deadline()).await?;
        let body = AgentReaderRevocationV1 {
            version: 1,
            context: state.pin.context.clone(),
            issuer_id: state.pin.issuer_id,
            issuer_epoch: 1,
            authority_revision: r
                .head
                .authority_revision
                .checked_add(1)
                .ok_or(AuthorityError::Quota)?,
            previous_authority_hash: r.head.authority_hash,
            subject: review.subject,
            issued_at: now,
        };
        let signed = ctx
            .with_held(app, |_| Ok(issuer.sign_revocation(body, &state.pin)))
            .await
            .map_err(|_| AuthorityError::Unavailable)??;
        state.push(
            now,
            Event::Revoke {
                signed: signed.clone(),
            },
        )?;
        self.check(app, ctx).await?;
        self.commit_peer(ctx, Some((bytes, record)), &state, ctx.deadline())
            .await?;
        self.check(app, ctx)
            .await
            .map_err(|_| AuthorityError::Unknown)?;
        Ok(PendingReaderAdministration {
            peer_digest: review.peer,
            head: state.replay()?.head,
            statement: SignedReaderAdmin::Revoke { signed },
        })
    }
    pub async fn reserve_action_once<'a>(
        &self,
        app: &AppCore,
        ctx: &'a ReaderOriginalContext,
        hash: AgentReaderHash,
        association: AgentReaderHash,
        operation: AgentReaderOperationId,
    ) -> Result<ReservedReaderAction<'a>> {
        let now = self.check(app, ctx).await?;
        let (bytes, record, mut state, r) = self.peer(ctx, hash, ctx.deadline()).await?;
        let (_, _, snapshot, _, _) = r.live_association(association, now)?;
        if !stable_snapshot_matches(snapshot, ctx.snapshot())? {
            return Err(AuthorityError::Rejected);
        }
        state.push(
            now,
            Event::Reserve {
                operation,
                association,
            },
        )?;
        self.commit_peer(ctx, Some((bytes, record)), &state, ctx.deadline())
            .await?;
        self.check(app, ctx)
            .await
            .map_err(|_| AuthorityError::Unknown)?;
        Ok(ReservedReaderAction {
            peer: hash,
            operation,
            association,
            deadline: ctx.deadline().min(Instant::now() + Duration::from_secs(30)),
            challenge_attempted: false,
            original: ctx,
        })
    }
    pub async fn challenge_request_once(
        &self,
        app: &AppCore,
        ctx: &ReaderOriginalContext,
        action: &mut ReservedReaderAction<'_>,
        peer: &VerifiedReaderControlPeer,
    ) -> Result<ReaderChallengeRequest> {
        if !std::ptr::eq(action.original, ctx) {
            return Err(AuthorityError::Rejected);
        }
        if !peer.is_live_for(ctx) {
            return Err(AuthorityError::Rejected);
        }
        let deadline = action.deadline.min(ctx.deadline());
        if action.challenge_attempted || Instant::now() >= deadline {
            return Err(AuthorityError::Rejected);
        }
        // Local attempted latch never resets even after a known-not-sent error.
        action.challenge_attempted = true;
        let now = self.action_check(app, ctx, deadline, false).await?;
        let (bytes, record, mut state, r) = self.peer(ctx, action.peer, deadline).await?;
        check_peer(peer, &state, ctx)?;
        if r.head != r.installed
            || r.reservations.get(&action.operation) != Some(&(action.association, 0))
        {
            return Err(AuthorityError::Rejected);
        }
        let (a, target, snapshot, transport, _) = r.live_association(action.association, now)?;
        if !stable_snapshot_matches(snapshot, ctx.snapshot())?
            || *transport != transport_hash(peer)?
        {
            return Err(AuthorityError::Rejected);
        }
        let (_, _, root) = self.root(deadline).await?;
        let issuer = self.key(&root, deadline).await?;
        self.action_check(app, ctx, deadline, false).await?;
        let end = now
            .checked_add(30)?
            .seconds()
            .min(a.body.expires_at.seconds())
            .min(state.pin.expires_at.seconds());
        let body = AgentReaderFreshnessV1 {
            version: 1,
            head: r.head.clone(),
            issued_at: now,
            expires_at: AgentReaderTimestamp::new(end)?,
        };
        let freshness = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            ctx.with_held(app, |_| {
                if Instant::now() >= deadline {
                    return Err(crate::AppError::invalid(
                        "agent_reader",
                        "reader_authority_unavailable",
                    ));
                }
                Ok(issuer.sign_freshness(body, &state.pin))
            }),
        )
        .await
        .map_err(|_| AuthorityError::Unavailable)?
        .map_err(|_| AuthorityError::Unavailable)??;
        self.action_check(app, ctx, deadline, false).await?;
        let request = ReaderChallengeRequest {
            association_hash: action.association,
            operation_id: action.operation,
            expected_agent_id: a.body.agent_id,
            operation: a.body.operation.clone(),
            target_descriptor_hash: target.descriptor_hash()?,
            freshness: freshness.clone(),
        };
        state.push(
            now,
            Event::ChallengePrepared {
                operation: action.operation,
                freshness,
            },
        )?;
        self.commit_peer(ctx, Some((bytes, record)), &state, deadline)
            .await?;
        let final_now = self.action_check(app, ctx, deadline, true).await?;
        live(
            request.freshness.body.issued_at,
            request.freshness.body.expires_at,
            final_now,
        )
        .map_err(|_| AuthorityError::Unknown)?;
        if !peer.is_live_for(ctx) {
            return Err(AuthorityError::Unknown);
        }
        if Instant::now() >= deadline {
            return Err(AuthorityError::Unknown);
        }
        Ok(request)
    }
    pub async fn issue_grant_once<'a>(
        &self,
        app: &AppCore,
        ctx: &'a ReaderOriginalContext,
        action: ReservedReaderAction<'a>,
        peer: &VerifiedReaderControlPeer,
        reply: &BoundReaderControlReply<AgentReaderChallengeV1>,
    ) -> Result<LocallyIssuedReaderPacket<'a>> {
        if !std::ptr::eq(action.original, ctx) {
            return Err(AuthorityError::Rejected);
        }
        if !peer.is_live_for(ctx) {
            return Err(AuthorityError::Rejected);
        }
        let deadline = action.deadline.min(ctx.deadline());
        let now = self.action_check(app, ctx, deadline, false).await?;
        if Instant::now() >= deadline {
            return Err(AuthorityError::Unavailable);
        }
        let challenge = reply
            .metadata_for(peer)
            .map_err(|_| AuthorityError::Unavailable)?
            .clone();
        let (bytes, record, mut state, r) = self.peer(ctx, action.peer, deadline).await?;
        check_peer(peer, &state, ctx)?;
        if r.head != r.installed
            || !action.challenge_attempted
            || r.reservations.get(&action.operation) != Some(&(action.association, 1))
        {
            return Err(AuthorityError::Rejected);
        }
        let (association, target, snapshot, transport, _) =
            r.live_association(action.association, now)?;
        if !stable_snapshot_matches(snapshot, ctx.snapshot())?
            || *transport != transport_hash(peer)?
            || challenge.operation_id != action.operation
        {
            return Err(AuthorityError::Rejected);
        }
        let (_, _, root) = self.root(deadline).await?;
        let issuer = self.key(&root, deadline).await?;
        self.action_check(app, ctx, deadline, false).await?;
        let freshness = r
            .freshness
            .get(&action.operation)
            .ok_or(AuthorityError::Unknown)?
            .clone();
        let grant_body = AgentReaderGrantV1 {
            version: 1,
            head: r.head.clone(),
            grant_id: AgentReaderGrantId::new(&Uuid::new_v4().to_string())?,
            association_hash: action.association,
            challenge_id: challenge.challenge_id,
            challenge_hash: challenge.challenge_hash()?,
            operation_id: action.operation,
            device_id: association.body.device_id,
            device_epoch: 1,
            agent_id: association.body.agent_id,
            operation: association.body.operation.clone(),
            target_descriptor_hash: target.descriptor_hash()?,
            issued_at: now,
            expires_at: AgentReaderTimestamp::new(
                freshness
                    .body
                    .expires_at
                    .seconds()
                    .min(challenge.expires_at.seconds()),
            )?,
        };
        let grant = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            ctx.with_held(app, |_| {
                if Instant::now() >= deadline {
                    return Err(crate::AppError::invalid(
                        "agent_reader",
                        "reader_authority_unavailable",
                    ));
                }
                Ok(issuer.sign_grant(grant_body, &state.pin))
            }),
        )
        .await
        .map_err(|_| AuthorityError::Unavailable)?
        .map_err(|_| AuthorityError::Unavailable)?;
        self.action_check(app, ctx, deadline, false).await?;
        let grant = grant?;
        validate_packet(
            &state.pin,
            &r.head,
            association,
            target,
            &grant,
            &freshness,
            &challenge,
            now,
        )?;
        let packet = AgentReaderSigningPacketV1 {
            association: association.clone(),
            grant: grant.clone(),
            freshness: freshness.clone(),
            challenge: challenge.clone(),
        };
        state.push(
            now,
            Event::Issued {
                operation: action.operation,
                grant,
                freshness,
                challenge,
            },
        )?;
        self.action_check(app, ctx, deadline, false).await?;
        self.commit_peer(ctx, Some((bytes, record)), &state, deadline)
            .await?;
        let final_now = self.action_check(app, ctx, deadline, true).await?;
        live(
            packet.association.body.not_before,
            packet.association.body.expires_at,
            final_now,
        )
        .map_err(|_| AuthorityError::Unknown)?;
        live(
            packet.grant.body.issued_at,
            packet.grant.body.expires_at,
            final_now,
        )
        .map_err(|_| AuthorityError::Unknown)?;
        live(
            packet.freshness.body.issued_at,
            packet.freshness.body.expires_at,
            final_now,
        )
        .map_err(|_| AuthorityError::Unknown)?;
        // Publication uncertainty never returns a reusable action or packet.
        if !peer.is_live_for(ctx) {
            return Err(AuthorityError::Unknown);
        }
        if Instant::now() >= deadline {
            return Err(AuthorityError::Unknown);
        }
        Ok(LocallyIssuedReaderPacket {
            packet,
            retirement: action,
        })
    }
    pub async fn retire_local(
        &self,
        app: &AppCore,
        ctx: &ReaderOriginalContext,
        action: ReservedReaderAction<'_>,
    ) -> Result<()> {
        if !std::ptr::eq(action.original, ctx) {
            return Err(AuthorityError::Rejected);
        }
        let now = self.check(app, ctx).await?;
        let (bytes, record, mut state, _) = self.peer(ctx, action.peer, ctx.deadline()).await?;
        state.push(
            now,
            Event::Retired {
                operation: action.operation,
            },
        )?;
        self.commit_peer(ctx, Some((bytes, record)), &state, ctx.deadline())
            .await
    }
}
fn check_peer(
    peer: &VerifiedReaderControlPeer,
    state: &PeerState,
    ctx: &ReaderOriginalContext,
) -> Result<()> {
    check_owner_binding(
        expected_owner_binding(ctx)?,
        state.identity.owner_binding_hash,
    )?;
    ctx.check_agent_context(&state.pin.context)
        .map_err(|_| AuthorityError::Rejected)?;
    let b = peer.binding();
    if !peer.is_live_for(ctx)
        || b.native_profile_id != state.identity.profile
        || b.agent_id != state.identity.agent
        || b.audience_id != state.identity.audience
        || b.node_trust_origin_hash != state.identity.node_origin
        || b.issuer_pin_genesis_hash != state.identity.pin_genesis
    {
        return Err(AuthorityError::Rejected);
    }
    Ok(())
}

#[cfg(test)]
#[path = "authority_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "personal_owner_tests.rs"]
mod personal_owner_tests;
