use super::{ReaderError as Error, ReaderResult as Result};
use crate::{AgentId, Context, Hash, Operation, PublicKey, SignatureBytes, Timestamp};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{de, Deserialize, Deserializer, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const READER_BUNDLE_BYTES: usize = 8192;
pub const READER_FRAME_BYTES: usize = 16384;
const PREFIX: &[u8] = b"consolecrypt.agent.reader.personal-prototype\0v1\0";
macro_rules! identifier {
    ($($name:ident),+) => {$ (
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);
        impl $name {
            pub fn new(value: &str) -> Result<Self> {
                let id = Uuid::parse_str(value).map_err(|_| Error::InvalidWire)?;
                if id.is_nil() || id.hyphenated().to_string() != value { return Err(Error::InvalidWire); }
                Ok(Self(id))
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self,D::Error> {
                Self::new(&String::deserialize(d)?).map_err(de::Error::custom)
            }
        }
    )+};
}
identifier!(
    ReaderIssuerId,
    ReaderDeviceId,
    NativeProfileId,
    NativeHostId,
    NativeBindingId,
    ReaderAssociationId,
    ReaderGrantId,
    ReaderOperationId,
    ReaderAudienceId
);

pub(crate) fn hash(domain: &[u8], bytes: &[u8]) -> Hash {
    crate::wire::hash(domain, bytes)
}
pub fn reader_key_fingerprint(key: PublicKey) -> Hash {
    hash(b"consolecrypt.agent.reader.device-key.v1", key.as_bytes())
}
pub fn reader_node_trust_origin(pin: &crate::OwnerPin) -> Result<Hash> {
    context(&pin.context)?;
    pin.key.validate().map_err(|_| Error::WrongTarget)?;
    timestamp(pin.expires_at)?;
    if pin.epoch == 0 {
        return Err(Error::WrongTarget);
    }
    Ok(hash(
        b"consolecrypt.agent.reader.node-trust-origin.v1",
        &canonical(pin)?,
    ))
}
pub(crate) fn canonical<T: Serialize>(body: &T) -> Result<Vec<u8>> {
    let value = serde_json::to_value(body).map_err(|_| Error::InvalidWire)?;
    let mut bytes = Vec::new();
    crate::wire::canonical_json(&value, &mut bytes).map_err(|_| Error::InvalidWire)?;
    if bytes.len() > READER_FRAME_BYTES {
        return Err(Error::Quota);
    }
    Ok(bytes)
}
fn timestamp(t: Timestamp) -> Result<()> {
    Timestamp::new(t.seconds()).map(|_| ()).map_err(Into::into)
}
pub(crate) fn live(start: Timestamp, end: Timestamp, now: Timestamp) -> Result<()> {
    timestamp(start)?;
    timestamp(end)?;
    timestamp(now)?;
    if now < start {
        return Err(Error::InvalidTime);
    }
    if now >= end {
        return Err(Error::Expired);
    }
    Ok(())
}
pub(crate) fn recent(issued: Timestamp, now: Timestamp, age: u64) -> Result<()> {
    timestamp(issued)?;
    timestamp(now)?;
    if issued > now || now.seconds() - issued.seconds() > age {
        return Err(Error::InvalidTime);
    }
    Ok(())
}
fn window(start: Timestamp, end: Timestamp, age: u64) -> Result<()> {
    timestamp(start)?;
    timestamp(end)?;
    if end <= start || end.seconds() - start.seconds() > age {
        return Err(Error::InvalidTime);
    }
    Ok(())
}
fn context(value: &Context) -> Result<()> {
    if !matches!(value.tenant, crate::Tenant::LocalProfile { .. }) {
        return Err(Error::WrongContext);
    }
    Ok(())
}
fn operation(value: &Operation) -> Result<()> {
    if !matches!(value, Operation::TelemetryRead { .. }) {
        return Err(Error::WrongOperation);
    }
    Ok(())
}
fn nonzero(value: Hash) -> Result<()> {
    if value.as_bytes() == &[0; 32] {
        return Err(Error::InvalidWire);
    }
    Ok(())
}
mod sealed {
    pub trait Sealed {}
}
pub trait ReaderStatement: Serialize + de::DeserializeOwned + Clone + sealed::Sealed {
    const PURPOSE: &'static str;
    fn validate(&self) -> Result<()>;
    fn signing_bytes(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let body = canonical(self)?;
        let mut bytes = PREFIX.to_vec();
        bytes.extend_from_slice(Self::PURPOSE.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&(body.len() as u32).to_be_bytes());
        bytes.extend(body);
        Ok(bytes)
    }
    fn digest(&self) -> Result<Hash> {
        Ok(hash(
            b"consolecrypt.agent.reader.statement-hash.v1",
            &self.signing_bytes()?,
        ))
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, bound(deserialize = "T:de::DeserializeOwned"))]
pub struct ReaderSigned<T> {
    pub body: T,
    pub signature: SignatureBytes,
}
impl<T: ReaderStatement> ReaderSigned<T> {
    pub fn verify(&self, key: PublicKey) -> Result<()> {
        key.validate().map_err(|_| Error::InvalidSignature)?;
        VerifyingKey::from_bytes(key.as_bytes())
            .map_err(|_| Error::InvalidSignature)?
            .verify_strict(
                &self.body.signing_bytes()?,
                &Signature::from_bytes(self.signature.as_bytes()),
            )
            .map_err(|_| Error::InvalidSignature)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReaderIssuerPin {
    pub format: String,
    pub context: Context,
    pub native_profile_id: NativeProfileId,
    pub issuer_id: ReaderIssuerId,
    pub issuer_epoch: u64,
    pub issuer_key: PublicKey,
    pub audience_id: ReaderAudienceId,
    pub expires_at: Timestamp,
    pub node_trust_origin_hash: Hash,
}
impl ReaderIssuerPin {
    pub fn validate(&self) -> Result<()> {
        if self.format != "cc-reader-issuer-pin/v1" || self.issuer_epoch != 1 {
            return Err(Error::WrongIssuer);
        }
        context(&self.context)?;
        timestamp(self.expires_at)?;
        self.issuer_key.validate().map_err(|_| Error::WrongIssuer)
    }
    pub fn genesis_hash(&self) -> Result<Hash> {
        self.validate()?;
        Ok(hash(
            b"consolecrypt.agent.reader.authority-genesis.v1",
            &canonical(self)?,
        ))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReaderHead {
    pub context: Context,
    pub issuer_id: ReaderIssuerId,
    pub issuer_epoch: u64,
    pub authority_revision: u64,
    pub authority_hash: Hash,
}
impl ReaderHead {
    fn validate(&self) -> Result<()> {
        context(&self.context)?;
        if self.issuer_epoch != 1 {
            return Err(Error::WrongIssuer);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReaderAction {
    HeartbeatRead,
    HistoryRead,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReaderDatasetKind {
    HeartbeatV1,
    LocalAgentHistoryV1,
    FixedOsHistoryV1,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReaderDataset {
    pub kind: ReaderDatasetKind,
    pub schema_version: u32,
    pub descriptor: Value,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReaderTargetDescriptor {
    pub format: String,
    pub agent_id: AgentId,
    pub configured_host_id: NativeHostId,
    pub action: ReaderAction,
    pub resource_id: crate::ResourceId,
    pub dataset: ReaderDataset,
    pub max_response_bytes: u32,
    pub max_query_points: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HeartbeatDescriptor {
    agent_id: AgentId,
    host_id: NativeHostId,
    heartbeat_resource_id: crate::ResourceId,
    history_kind: String,
    schema_version: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalDescriptor {
    agent_id: AgentId,
    host_id: NativeHostId,
    history_resource_id: crate::ResourceId,
    sample_interval_seconds: u32,
    max_bytes: u64,
    max_points: u32,
    max_age_seconds: u64,
    history_kind: String,
    schema_version: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FixedDescriptor {
    agent_id: AgentId,
    host_id: NativeHostId,
    history_resource_id: crate::ResourceId,
    collection_resource_id: crate::ResourceId,
    plan_sha256: Hash,
    agent_reserved_bytes: u64,
    snapshot_reserved_bytes: u64,
    total_max_bytes: u64,
    max_points: u32,
    max_age_seconds: u64,
    history_kind: String,
    schema_version: u32,
}
impl ReaderTargetDescriptor {
    pub fn validate(&self) -> Result<()> {
        if self.format != "cc-reader-target/v1" || self.dataset.schema_version != 1 {
            return Err(Error::WrongTarget);
        }
        let ids =
            |a, h, r| a == self.agent_id && h == self.configured_host_id && r == self.resource_id;
        let valid = match self.dataset.kind {
            ReaderDatasetKind::HeartbeatV1 => {
                let d: HeartbeatDescriptor =
                    serde_json::from_value(self.dataset.descriptor.clone())
                        .map_err(|_| Error::InvalidWire)?;
                self.action == ReaderAction::HeartbeatRead
                    && self.max_response_bytes == 65536
                    && self.max_query_points == 0
                    && ids(d.agent_id, d.host_id, d.heartbeat_resource_id)
                    && d.history_kind == "heartbeat"
                    && d.schema_version == 1
            }
            ReaderDatasetKind::LocalAgentHistoryV1 => {
                let d: LocalDescriptor = serde_json::from_value(self.dataset.descriptor.clone())
                    .map_err(|_| Error::InvalidWire)?;
                self.action == ReaderAction::HistoryRead
                    && self.max_response_bytes == 262144
                    && self.max_query_points == 1000
                    && ids(d.agent_id, d.host_id, d.history_resource_id)
                    && d.history_kind == "agent_gauge_history"
                    && d.schema_version == 1
                    && (1..=300).contains(&d.sample_interval_seconds)
                    && (1024..=268435456).contains(&d.max_bytes)
                    && (1..=5760).contains(&d.max_points)
                    && (1..=86400).contains(&d.max_age_seconds)
            }
            ReaderDatasetKind::FixedOsHistoryV1 => {
                let d: FixedDescriptor = serde_json::from_value(self.dataset.descriptor.clone())
                    .map_err(|_| Error::InvalidWire)?;
                self.action == ReaderAction::HistoryRead
                    && self.max_response_bytes == 262144
                    && self.max_query_points == 100
                    && ids(d.agent_id, d.host_id, d.history_resource_id)
                    && d.history_kind == "fixed_os_snapshot"
                    && d.schema_version == 1
                    && d.collection_resource_id != d.history_resource_id
                    && d.plan_sha256 != Hash::from_bytes([0; 32])
                    && (1024..=268435456).contains(&d.agent_reserved_bytes)
                    && (32768..=268435456).contains(&d.snapshot_reserved_bytes)
                    && d.total_max_bytes <= 268435456
                    && d.agent_reserved_bytes
                        .checked_add(d.snapshot_reserved_bytes)
                        .is_some_and(|v| v <= d.total_max_bytes)
                    && (1..=5760).contains(&d.max_points)
                    && (1..=86400).contains(&d.max_age_seconds)
            }
        };
        if !valid {
            return Err(Error::WrongTarget);
        }
        Ok(())
    }
    pub fn digest(&self) -> Result<Hash> {
        self.validate()?;
        Ok(hash(
            b"consolecrypt.agent.reader.target.v1",
            &canonical(self)?,
        ))
    }
    pub fn operation(&self) -> Operation {
        Operation::TelemetryRead {
            source: self.resource_id,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReaderTargetSet {
    pub format: String,
    pub targets: Vec<ReaderTargetDescriptor>,
}
impl ReaderTargetSet {
    pub fn validate(&self) -> Result<()> {
        if self.format != "cc-reader-target-set/v1" || self.targets.len() != 2 {
            return Err(Error::WrongTarget);
        }
        let (a, b) = (&self.targets[0], &self.targets[1]);
        a.validate()?;
        b.validate()?;
        if a.action != ReaderAction::HeartbeatRead
            || b.action != ReaderAction::HistoryRead
            || a.resource_id == b.resource_id
            || a.agent_id != b.agent_id
            || a.configured_host_id != b.configured_host_id
        {
            return Err(Error::WrongTarget);
        }
        if b.dataset.kind == ReaderDatasetKind::FixedOsHistoryV1 {
            let d: FixedDescriptor = serde_json::from_value(b.dataset.descriptor.clone())
                .map_err(|_| Error::InvalidWire)?;
            if a.resource_id == d.collection_resource_id {
                return Err(Error::WrongTarget);
            }
        }
        Ok(())
    }
    pub fn target(&self, op: &Operation) -> Result<&ReaderTargetDescriptor> {
        self.validate()?;
        self.targets
            .iter()
            .find(|t| &t.operation() == op)
            .ok_or(Error::WrongOperation)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReaderAssociation {
    pub version: u32,
    pub issuer_id: ReaderIssuerId,
    pub issuer_epoch: u64,
    pub authority_revision: u64,
    pub previous_authority_hash: Hash,
    pub context: Context,
    pub native_profile_id: NativeProfileId,
    pub native_host_id: NativeHostId,
    pub native_host_revision: u64,
    pub native_binding_id: NativeBindingId,
    pub native_binding_revision: u64,
    pub native_binding_digest: Hash,
    pub native_source_id: String,
    pub native_source_revision: u64,
    pub association_id: ReaderAssociationId,
    pub agent_id: AgentId,
    pub node_trust_origin_hash: Hash,
    pub agent_certificate_hash: Hash,
    pub agent_key_epoch: u64,
    pub device_id: ReaderDeviceId,
    pub device_epoch: u64,
    pub device_key: PublicKey,
    pub audience_id: ReaderAudienceId,
    pub transport_peer_hash: Hash,
    pub operation: Operation,
    pub dataset_kind: ReaderDatasetKind,
    pub dataset_schema_version: u32,
    pub target_descriptor_hash: Hash,
    pub issued_at: Timestamp,
    pub not_before: Timestamp,
    pub expires_at: Timestamp,
}
impl sealed::Sealed for ReaderAssociation {}
impl ReaderStatement for ReaderAssociation {
    const PURPOSE: &'static str = "association";
    fn validate(&self) -> Result<()> {
        if self.version != 1
            || self.issuer_epoch != 1
            || self.device_epoch != 1
            || self.authority_revision == 0
            || self.native_host_revision == 0
            || self.native_binding_revision == 0
            || self.native_source_revision == 0
            || self.agent_key_epoch == 0
            || self.dataset_schema_version != 1
            || self.native_source_id.is_empty()
            || self.native_source_id.len() > 128
            || self.native_source_id.chars().any(char::is_control)
        {
            return Err(Error::InvalidWire);
        }
        context(&self.context)?;
        operation(&self.operation)?;
        self.device_key.validate().map_err(|_| Error::WrongDevice)?;
        window(self.issued_at, self.expires_at, 3600)?;
        timestamp(self.not_before)?;
        if self.not_before < self.issued_at || self.not_before >= self.expires_at {
            return Err(Error::InvalidTime);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReaderFreshness {
    pub version: u32,
    pub head: ReaderHead,
    pub issued_at: Timestamp,
    pub expires_at: Timestamp,
}
impl sealed::Sealed for ReaderFreshness {}
impl ReaderStatement for ReaderFreshness {
    const PURPOSE: &'static str = "freshness";
    fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err(Error::InvalidWire);
        }
        self.head.validate()?;
        window(self.issued_at, self.expires_at, 30)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReaderChallenge {
    pub version: u32,
    pub head: ReaderHead,
    pub challenge_id: Hash,
    pub server_nonce: Hash,
    pub association_hash: Hash,
    pub operation_id: ReaderOperationId,
    pub device_id: ReaderDeviceId,
    pub device_epoch: u64,
    pub device_key_fingerprint: Hash,
    pub agent_id: AgentId,
    pub agent_certificate_hash: Hash,
    pub agent_key_epoch: u64,
    pub node_owner_epoch: u64,
    pub node_checkpoint: u64,
    pub native_profile_id: NativeProfileId,
    pub native_binding_id: NativeBindingId,
    pub native_binding_revision: u64,
    pub operation: Operation,
    pub target_descriptor_hash: Hash,
    pub issued_at: Timestamp,
    pub expires_at: Timestamp,
}
impl ReaderChallenge {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1
            || self.device_epoch != 1
            || self.agent_key_epoch == 0
            || self.node_owner_epoch == 0
            || self.native_binding_revision == 0
        {
            return Err(Error::InvalidWire);
        }
        self.head.validate()?;
        operation(&self.operation)?;
        nonzero(self.challenge_id)?;
        nonzero(self.server_nonce)?;
        window(self.issued_at, self.expires_at, 30)
    }
    pub fn digest(&self) -> Result<Hash> {
        self.validate()?;
        Ok(hash(
            b"consolecrypt.agent.reader.challenge-body.v1",
            &canonical(self)?,
        ))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReaderGrant {
    pub version: u32,
    pub head: ReaderHead,
    pub grant_id: ReaderGrantId,
    pub association_hash: Hash,
    pub challenge_id: Hash,
    pub challenge_hash: Hash,
    pub operation_id: ReaderOperationId,
    pub device_id: ReaderDeviceId,
    pub device_epoch: u64,
    pub agent_id: AgentId,
    pub operation: Operation,
    pub target_descriptor_hash: Hash,
    pub issued_at: Timestamp,
    pub expires_at: Timestamp,
}
impl sealed::Sealed for ReaderGrant {}
impl ReaderStatement for ReaderGrant {
    const PURPOSE: &'static str = "grant";
    fn validate(&self) -> Result<()> {
        if self.version != 1 || self.device_epoch != 1 {
            return Err(Error::InvalidWire);
        }
        self.head.validate()?;
        operation(&self.operation)?;
        nonzero(self.challenge_id)?;
        window(self.issued_at, self.expires_at, 30)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReaderProof {
    pub version: u32,
    pub head: ReaderHead,
    pub operation_id: ReaderOperationId,
    pub device_id: ReaderDeviceId,
    pub device_epoch: u64,
    pub association_hash: Hash,
    pub grant_hash: Hash,
    pub challenge_hash: Hash,
    pub nonce: Hash,
    pub issued_at: Timestamp,
}
impl sealed::Sealed for ReaderProof {}
impl ReaderStatement for ReaderProof {
    const PURPOSE: &'static str = "request-proof";
    fn validate(&self) -> Result<()> {
        if self.version != 1 || self.device_epoch != 1 {
            return Err(Error::InvalidWire);
        }
        self.head.validate()?;
        nonzero(self.nonce)?;
        timestamp(self.issued_at)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ReaderRevocationSubject {
    #[serde(rename_all = "camelCase")]
    Association {
        association_id: ReaderAssociationId,
        body_hash: Hash,
    },
    #[serde(rename_all = "camelCase")]
    Device {
        device_id: ReaderDeviceId,
        device_epoch: u64,
        key_fingerprint: Hash,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReaderRevocation {
    pub version: u32,
    pub context: Context,
    pub issuer_id: ReaderIssuerId,
    pub issuer_epoch: u64,
    pub authority_revision: u64,
    pub previous_authority_hash: Hash,
    pub subject: ReaderRevocationSubject,
    pub issued_at: Timestamp,
}
impl sealed::Sealed for ReaderRevocation {}
impl ReaderStatement for ReaderRevocation {
    const PURPOSE: &'static str = "revocation";
    fn validate(&self) -> Result<()> {
        if self.version != 1 || self.issuer_epoch != 1 || self.authority_revision == 0 {
            return Err(Error::InvalidWire);
        }
        context(&self.context)?;
        timestamp(self.issued_at)
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReaderBundle {
    pub association: ReaderSigned<ReaderAssociation>,
    pub grant: ReaderSigned<ReaderGrant>,
    pub proof: ReaderSigned<ReaderProof>,
    pub freshness: ReaderSigned<ReaderFreshness>,
}
impl ReaderBundle {
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        strict_json(bytes, READER_BUNDLE_BYTES)
    }
}

/// Duplicate detection is structural, before any typed fields or authority.
pub fn strict_json<T: de::DeserializeOwned>(bytes: &[u8], limit: usize) -> Result<T> {
    if bytes.len() > limit {
        return Err(Error::Quota);
    }
    let Strict(value) = serde_json::from_slice(bytes).map_err(|_| Error::InvalidWire)?;
    fn check(value: &Value, depth: usize) -> Result<()> {
        if depth > 16 {
            return Err(Error::InvalidWire);
        }
        match value {
            Value::Number(v) if !(v.is_i64() || v.is_u64()) => Err(Error::InvalidWire),
            Value::Object(v) => {
                for x in v.values() {
                    check(x, depth + 1)?;
                }
                Ok(())
            }
            Value::Array(v) => {
                for x in v {
                    check(x, depth + 1)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
    check(&value, 0)?;
    serde_json::from_value(value).map_err(|_| Error::InvalidWire)
}
struct Strict(Value);
impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Visitor;
        impl<'de> de::Visitor<'de> for Visitor {
            type Value = Strict;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("reader JSON")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<Strict, E> {
                Ok(Strict(Value::Bool(v)))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> std::result::Result<Strict, E> {
                Err(E::custom("invalid reader number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_none<E: de::Error>(self) -> std::result::Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_seq<A: de::SeqAccess<'de>>(
                self,
                mut a: A,
            ) -> std::result::Result<Strict, A::Error> {
                let mut v = Vec::new();
                while let Some(x) = a.next_element::<Strict>()? {
                    v.push(x.0);
                }
                Ok(Strict(v.into()))
            }
            fn visit_map<A: de::MapAccess<'de>>(
                self,
                mut a: A,
            ) -> std::result::Result<Strict, A::Error> {
                let mut v = serde_json::Map::new();
                while let Some(k) = a.next_key::<String>()? {
                    if v.contains_key(&k) {
                        return Err(de::Error::custom("duplicate reader key"));
                    }
                    let x: Strict = a.next_value()?;
                    v.insert(k, x.0);
                }
                Ok(Strict(v.into()))
            }
        }
        d.deserialize_any(Visitor)
    }
}
