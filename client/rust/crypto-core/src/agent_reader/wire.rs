use super::{AgentReaderError as Error, AgentReaderResult as Result};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const AGENT_READER_BUNDLE_BYTES: usize = 8192;
pub const AGENT_READER_FRAME_BYTES: usize = 16384;
pub const AGENT_READER_MAX_TIMESTAMP: u64 = 253_402_300_799;
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
            pub fn as_uuid(&self) -> &Uuid { &self.0 }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self,D::Error> {
                Self::new(&String::deserialize(d)?).map_err(de::Error::custom)
            }
        }
    )+};
}
identifier!(
    AgentReaderIssuerId,
    AgentReaderDeviceId,
    AgentReaderNativeProfileId,
    AgentReaderNativeHostId,
    AgentReaderNativeBindingId,
    AgentReaderAssociationId,
    AgentReaderGrantId,
    AgentReaderOperationId,
    AgentReaderAudienceId,
    AgentReaderAgentId,
    AgentReaderConfiguredHostId,
    AgentReaderResourceId,
    AgentReaderProfileId,
    AgentReaderPersonalAccountId,
    AgentReaderDeploymentId
);
impl AgentReaderDeviceId {
    pub fn from_device_id(value: cc_protocol::DeviceId) -> Result<Self> {
        Self::new(&value.to_string())
    }
    pub fn as_device_id(&self) -> cc_protocol::DeviceId {
        cc_protocol::DeviceId::from_uuid(self.0)
    }
}

/// Syntax-only account identifier. The authority owner must supply the actual
/// authenticated UserId; parsing a frontend UUID never authenticates it.
impl AgentReaderPersonalAccountId {
    pub fn from_user_id(value: cc_protocol::UserId) -> Result<Self> {
        Self::new(&value.to_string())
    }
}

macro_rules! fixed_hex {
    ($name:ident, $len:expr) => {
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $name([u8; $len]);
        impl $name {
            pub fn from_bytes(value: [u8; $len]) -> Self {
                Self(value)
            }
            pub fn as_bytes(&self) -> &[u8; $len] {
                &self.0
            }
        }
        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(concat!(stringify!($name), "([redacted])"))
            }
        }
        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
                const DIGITS: &[u8] = b"0123456789abcdef";
                let mut value = String::with_capacity($len * 2);
                for byte in self.0 {
                    value.push(DIGITS[(byte >> 4) as usize] as char);
                    value.push(DIGITS[(byte & 15) as usize] as char);
                }
                s.serialize_str(&value)
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
                let text = String::deserialize(d)?;
                fn nibble(v: u8) -> Option<u8> {
                    match v {
                        b'0'..=b'9' => Some(v - b'0'),
                        b'a'..=b'f' => Some(v - b'a' + 10),
                        _ => None,
                    }
                }
                if text.len() != $len * 2 {
                    return Err(de::Error::custom("invalid reader hex"));
                }
                let mut bytes = [0; $len];
                for (i, pair) in text.as_bytes().chunks_exact(2).enumerate() {
                    bytes[i] = nibble(pair[0])
                        .ok_or_else(|| de::Error::custom("invalid reader hex"))?
                        * 16
                        + nibble(pair[1]).ok_or_else(|| de::Error::custom("invalid reader hex"))?;
                }
                Ok(Self(bytes))
            }
        }
    };
}
fixed_hex!(AgentReaderHash, 32);
fixed_hex!(AgentReaderPublicKey, 32);
fixed_hex!(AgentReaderSignature, 64);
impl AgentReaderPublicKey {
    pub fn validate(&self) -> Result<()> {
        let key = VerifyingKey::from_bytes(&self.0).map_err(|_| Error::InvalidSignature)?;
        if key.is_weak() {
            return Err(Error::InvalidSignature);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct AgentReaderTimestamp(u64);
impl AgentReaderTimestamp {
    pub fn new(value: u64) -> Result<Self> {
        if value > AGENT_READER_MAX_TIMESTAMP {
            Err(Error::InvalidTime)
        } else {
            Ok(Self(value))
        }
    }
    pub fn seconds(self) -> u64 {
        self.0
    }
    pub fn checked_add(self, seconds: u64) -> Result<Self> {
        Self::new(self.0.checked_add(seconds).ok_or(Error::InvalidTime)?)
    }
    pub fn from_native(value: cc_protocol::Timestamp) -> Result<Self> {
        Self::new(
            value
                .timestamp()
                .try_into()
                .map_err(|_| Error::InvalidTime)?,
        )
    }
}
impl<'de> Deserialize<'de> for AgentReaderTimestamp {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::new(u64::deserialize(d)?).map_err(de::Error::custom)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum AgentReaderTenantV1 {
    LocalProfile { id: AgentReaderProfileId },
    PersonalAccount { id: AgentReaderPersonalAccountId },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentReaderContextV1 {
    pub tenant: AgentReaderTenantV1,
    pub deployment_id: AgentReaderDeploymentId,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum AgentReaderOperationV1 {
    TelemetryRead { source: AgentReaderResourceId },
}

pub(crate) fn hash(domain: &[u8], bytes: &[u8]) -> AgentReaderHash {
    let mut h = Sha256::new();
    h.update(domain);
    h.update([0]);
    h.update(bytes);
    AgentReaderHash::from_bytes(h.finalize().into())
}
pub fn agent_reader_key_fingerprint(key: AgentReaderPublicKey) -> AgentReaderHash {
    hash(b"consolecrypt.agent.reader.device-key.v1", key.as_bytes())
}
pub(crate) fn canonical<T: Serialize>(body: &T) -> Result<Vec<u8>> {
    fn emit(value: &Value, out: &mut Vec<u8>) -> Result<()> {
        match value {
            Value::Object(map) => {
                out.push(b'{');
                let mut entries = map.iter().collect::<Vec<_>>();
                entries.sort_unstable_by(|a, b| a.0.cmp(b.0));
                for (i, (key, value)) in entries.into_iter().enumerate() {
                    if i > 0 {
                        out.push(b',');
                    }
                    serde_json::to_writer(&mut *out, key).map_err(|_| Error::InvalidWire)?;
                    out.push(b':');
                    emit(value, out)?;
                }
                out.push(b'}');
            }
            Value::Array(values) => {
                out.push(b'[');
                for (i, v) in values.iter().enumerate() {
                    if i > 0 {
                        out.push(b',');
                    }
                    emit(v, out)?;
                }
                out.push(b']');
            }
            Value::Number(v) if !(v.is_i64() || v.is_u64()) => return Err(Error::InvalidWire),
            _ => serde_json::to_writer(&mut *out, value).map_err(|_| Error::InvalidWire)?,
        }
        if out.len() > AGENT_READER_FRAME_BYTES {
            return Err(Error::Quota);
        }
        Ok(())
    }
    let value = serde_json::to_value(body).map_err(|_| Error::InvalidWire)?;
    let mut out = Vec::new();
    emit(&value, &mut out)?;
    Ok(out)
}
fn timestamp(value: AgentReaderTimestamp) -> Result<()> {
    AgentReaderTimestamp::new(value.seconds()).map(|_| ())
}
pub(crate) fn live(
    start: AgentReaderTimestamp,
    end: AgentReaderTimestamp,
    now: AgentReaderTimestamp,
) -> Result<()> {
    if now < start {
        return Err(Error::InvalidTime);
    }
    if now >= end {
        return Err(Error::Expired);
    }
    Ok(())
}
fn window(start: AgentReaderTimestamp, end: AgentReaderTimestamp, max: u64) -> Result<()> {
    if end <= start || end.seconds() - start.seconds() > max {
        Err(Error::InvalidTime)
    } else {
        Ok(())
    }
}
fn context(_: &AgentReaderContextV1) -> Result<()> {
    Ok(())
}
fn operation(_: &AgentReaderOperationV1) -> Result<()> {
    Ok(())
}
fn nonzero(value: AgentReaderHash) -> Result<()> {
    if value.as_bytes() == &[0; 32] {
        Err(Error::InvalidWire)
    } else {
        Ok(())
    }
}
mod sealed {
    pub trait Sealed {}
}
pub trait AgentReaderStatementV1:
    Serialize + de::DeserializeOwned + Clone + sealed::Sealed
{
    const PURPOSE: &'static str;
    fn validate(&self) -> Result<()>;
    fn canonical_bytes(&self) -> Result<Vec<u8>> {
        self.validate()?;
        canonical(self)
    }
    fn signing_bytes(&self) -> Result<Vec<u8>> {
        let body = self.canonical_bytes()?;
        let mut out = PREFIX.to_vec();
        out.extend(Self::PURPOSE.as_bytes());
        out.push(0);
        out.extend((body.len() as u32).to_be_bytes());
        out.extend(body);
        Ok(out)
    }
    fn body_digest(&self) -> Result<AgentReaderHash> {
        Ok(hash(
            b"consolecrypt.agent.reader.statement-hash.v1",
            &self.signing_bytes()?,
        ))
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, bound(deserialize = "T:de::DeserializeOwned"))]
pub struct AgentReaderSigned<T> {
    pub body: T,
    pub signature: AgentReaderSignature,
}
impl<T> std::fmt::Debug for AgentReaderSigned<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AgentReaderSigned([redacted])")
    }
}
impl<T: AgentReaderStatementV1> AgentReaderSigned<T> {
    pub fn verify(&self, key: AgentReaderPublicKey) -> Result<()> {
        key.validate()?;
        VerifyingKey::from_bytes(key.as_bytes())
            .map_err(|_| Error::InvalidSignature)?
            .verify_strict(
                &self.body.signing_bytes()?,
                &Signature::from_bytes(self.signature.as_bytes()),
            )
            .map_err(|_| Error::InvalidSignature)
    }
}

// Reader-specific DTOs preserve the reviewed Agent v1 wire.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentReaderIssuerPinV1 {
    pub format: String,
    pub context: AgentReaderContextV1,
    pub native_profile_id: AgentReaderNativeProfileId,
    pub issuer_id: AgentReaderIssuerId,
    pub issuer_epoch: u64,
    pub issuer_key: AgentReaderPublicKey,
    pub audience_id: AgentReaderAudienceId,
    pub expires_at: AgentReaderTimestamp,
    pub node_trust_origin_hash: AgentReaderHash,
}
impl AgentReaderIssuerPinV1 {
    pub fn validate(&self) -> Result<()> {
        if self.format != "cc-reader-issuer-pin/v1" || self.issuer_epoch != 1 {
            return Err(Error::WrongIssuer);
        }
        context(&self.context)?;
        timestamp(self.expires_at)?;
        self.issuer_key.validate().map_err(|_| Error::WrongIssuer)
    }
    pub fn genesis_hash(&self) -> Result<AgentReaderHash> {
        self.validate()?;
        Ok(hash(
            b"consolecrypt.agent.reader.authority-genesis.v1",
            &canonical(self)?,
        ))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentReaderHeadV1 {
    pub context: AgentReaderContextV1,
    pub issuer_id: AgentReaderIssuerId,
    pub issuer_epoch: u64,
    pub authority_revision: u64,
    pub authority_hash: AgentReaderHash,
}
impl AgentReaderHeadV1 {
    pub fn validate(&self) -> Result<()> {
        context(&self.context)?;
        if self.issuer_epoch != 1 {
            return Err(Error::WrongIssuer);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentReaderActionV1 {
    HeartbeatRead,
    HistoryRead,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentReaderDatasetKindV1 {
    HeartbeatV1,
    LocalAgentHistoryV1,
    FixedOsHistoryV1,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentReaderDatasetV1 {
    pub kind: AgentReaderDatasetKindV1,
    pub schema_version: u32,
    pub descriptor: Value,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentReaderTargetDescriptorV1 {
    pub format: String,
    pub agent_id: AgentReaderAgentId,
    pub configured_host_id: AgentReaderConfiguredHostId,
    pub action: AgentReaderActionV1,
    pub resource_id: AgentReaderResourceId,
    pub dataset: AgentReaderDatasetV1,
    pub max_response_bytes: u32,
    pub max_query_points: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HeartbeatDescriptor {
    agent_id: AgentReaderAgentId,
    host_id: AgentReaderConfiguredHostId,
    heartbeat_resource_id: AgentReaderResourceId,
    history_kind: String,
    schema_version: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalDescriptor {
    agent_id: AgentReaderAgentId,
    host_id: AgentReaderConfiguredHostId,
    history_resource_id: AgentReaderResourceId,
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
    agent_id: AgentReaderAgentId,
    host_id: AgentReaderConfiguredHostId,
    history_resource_id: AgentReaderResourceId,
    collection_resource_id: AgentReaderResourceId,
    plan_sha256: AgentReaderHash,
    agent_reserved_bytes: u64,
    snapshot_reserved_bytes: u64,
    total_max_bytes: u64,
    max_points: u32,
    max_age_seconds: u64,
    history_kind: String,
    schema_version: u32,
}
impl AgentReaderTargetDescriptorV1 {
    pub fn validate(&self) -> Result<()> {
        if self.format != "cc-reader-target/v1" || self.dataset.schema_version != 1 {
            return Err(Error::WrongTarget);
        }
        let ids =
            |a, h, r| a == self.agent_id && h == self.configured_host_id && r == self.resource_id;
        let valid = match self.dataset.kind {
            AgentReaderDatasetKindV1::HeartbeatV1 => {
                let d: HeartbeatDescriptor =
                    serde_json::from_value(self.dataset.descriptor.clone())
                        .map_err(|_| Error::InvalidWire)?;
                self.action == AgentReaderActionV1::HeartbeatRead
                    && self.max_response_bytes == 65536
                    && self.max_query_points == 0
                    && ids(d.agent_id, d.host_id, d.heartbeat_resource_id)
                    && d.history_kind == "heartbeat"
                    && d.schema_version == 1
            }
            AgentReaderDatasetKindV1::LocalAgentHistoryV1 => {
                let d: LocalDescriptor = serde_json::from_value(self.dataset.descriptor.clone())
                    .map_err(|_| Error::InvalidWire)?;
                self.action == AgentReaderActionV1::HistoryRead
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
            AgentReaderDatasetKindV1::FixedOsHistoryV1 => {
                let d: FixedDescriptor = serde_json::from_value(self.dataset.descriptor.clone())
                    .map_err(|_| Error::InvalidWire)?;
                self.action == AgentReaderActionV1::HistoryRead
                    && self.max_response_bytes == 262144
                    && self.max_query_points == 100
                    && ids(d.agent_id, d.host_id, d.history_resource_id)
                    && d.history_kind == "fixed_os_snapshot"
                    && d.schema_version == 1
                    && d.collection_resource_id != d.history_resource_id
                    && d.plan_sha256 != AgentReaderHash::from_bytes([0; 32])
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
    pub fn descriptor_hash(&self) -> Result<AgentReaderHash> {
        self.validate()?;
        Ok(hash(
            b"consolecrypt.agent.reader.target.v1",
            &canonical(self)?,
        ))
    }
    pub fn operation(&self) -> AgentReaderOperationV1 {
        AgentReaderOperationV1::TelemetryRead {
            source: self.resource_id,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentReaderTargetSetV1 {
    pub format: String,
    pub targets: Vec<AgentReaderTargetDescriptorV1>,
}
impl AgentReaderTargetSetV1 {
    pub fn validate(&self) -> Result<()> {
        if self.format != "cc-reader-target-set/v1" || self.targets.len() != 2 {
            return Err(Error::WrongTarget);
        }
        let (a, b) = (&self.targets[0], &self.targets[1]);
        a.validate()?;
        b.validate()?;
        if a.action != AgentReaderActionV1::HeartbeatRead
            || b.action != AgentReaderActionV1::HistoryRead
            || a.resource_id == b.resource_id
            || a.agent_id != b.agent_id
            || a.configured_host_id != b.configured_host_id
        {
            return Err(Error::WrongTarget);
        }
        if b.dataset.kind == AgentReaderDatasetKindV1::FixedOsHistoryV1 {
            let d: FixedDescriptor = serde_json::from_value(b.dataset.descriptor.clone())
                .map_err(|_| Error::InvalidWire)?;
            if a.resource_id == d.collection_resource_id {
                return Err(Error::WrongTarget);
            }
        }
        Ok(())
    }
    pub fn target(&self, op: &AgentReaderOperationV1) -> Result<&AgentReaderTargetDescriptorV1> {
        self.validate()?;
        self.targets
            .iter()
            .find(|t| &t.operation() == op)
            .ok_or(Error::WrongOperation)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentReaderAssociationV1 {
    pub version: u32,
    pub issuer_id: AgentReaderIssuerId,
    pub issuer_epoch: u64,
    pub authority_revision: u64,
    pub previous_authority_hash: AgentReaderHash,
    pub context: AgentReaderContextV1,
    pub native_profile_id: AgentReaderNativeProfileId,
    pub native_host_id: AgentReaderNativeHostId,
    pub native_host_revision: u64,
    pub native_binding_id: AgentReaderNativeBindingId,
    pub native_binding_revision: u64,
    pub native_binding_digest: AgentReaderHash,
    pub native_source_id: String,
    pub native_source_revision: u64,
    pub association_id: AgentReaderAssociationId,
    pub agent_id: AgentReaderAgentId,
    pub node_trust_origin_hash: AgentReaderHash,
    pub agent_certificate_hash: AgentReaderHash,
    pub agent_key_epoch: u64,
    pub device_id: AgentReaderDeviceId,
    pub device_epoch: u64,
    pub device_key: AgentReaderPublicKey,
    pub audience_id: AgentReaderAudienceId,
    pub transport_peer_hash: AgentReaderHash,
    pub operation: AgentReaderOperationV1,
    pub dataset_kind: AgentReaderDatasetKindV1,
    pub dataset_schema_version: u32,
    pub target_descriptor_hash: AgentReaderHash,
    pub issued_at: AgentReaderTimestamp,
    pub not_before: AgentReaderTimestamp,
    pub expires_at: AgentReaderTimestamp,
}
impl sealed::Sealed for AgentReaderAssociationV1 {}
impl AgentReaderStatementV1 for AgentReaderAssociationV1 {
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
pub struct AgentReaderFreshnessV1 {
    pub version: u32,
    pub head: AgentReaderHeadV1,
    pub issued_at: AgentReaderTimestamp,
    pub expires_at: AgentReaderTimestamp,
}
impl sealed::Sealed for AgentReaderFreshnessV1 {}
impl AgentReaderStatementV1 for AgentReaderFreshnessV1 {
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
pub struct AgentReaderChallengeV1 {
    pub version: u32,
    pub head: AgentReaderHeadV1,
    pub challenge_id: AgentReaderHash,
    pub server_nonce: AgentReaderHash,
    pub association_hash: AgentReaderHash,
    pub operation_id: AgentReaderOperationId,
    pub device_id: AgentReaderDeviceId,
    pub device_epoch: u64,
    pub device_key_fingerprint: AgentReaderHash,
    pub agent_id: AgentReaderAgentId,
    pub agent_certificate_hash: AgentReaderHash,
    pub agent_key_epoch: u64,
    pub node_owner_epoch: u64,
    pub node_checkpoint: u64,
    pub native_profile_id: AgentReaderNativeProfileId,
    pub native_binding_id: AgentReaderNativeBindingId,
    pub native_binding_revision: u64,
    pub operation: AgentReaderOperationV1,
    pub target_descriptor_hash: AgentReaderHash,
    pub issued_at: AgentReaderTimestamp,
    pub expires_at: AgentReaderTimestamp,
}
impl AgentReaderChallengeV1 {
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
    pub fn challenge_hash(&self) -> Result<AgentReaderHash> {
        self.validate()?;
        Ok(hash(
            b"consolecrypt.agent.reader.challenge-body.v1",
            &canonical(self)?,
        ))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentReaderGrantV1 {
    pub version: u32,
    pub head: AgentReaderHeadV1,
    pub grant_id: AgentReaderGrantId,
    pub association_hash: AgentReaderHash,
    pub challenge_id: AgentReaderHash,
    pub challenge_hash: AgentReaderHash,
    pub operation_id: AgentReaderOperationId,
    pub device_id: AgentReaderDeviceId,
    pub device_epoch: u64,
    pub agent_id: AgentReaderAgentId,
    pub operation: AgentReaderOperationV1,
    pub target_descriptor_hash: AgentReaderHash,
    pub issued_at: AgentReaderTimestamp,
    pub expires_at: AgentReaderTimestamp,
}
impl sealed::Sealed for AgentReaderGrantV1 {}
impl AgentReaderStatementV1 for AgentReaderGrantV1 {
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
pub struct AgentReaderProofV1 {
    pub version: u32,
    pub head: AgentReaderHeadV1,
    pub operation_id: AgentReaderOperationId,
    pub device_id: AgentReaderDeviceId,
    pub device_epoch: u64,
    pub association_hash: AgentReaderHash,
    pub grant_hash: AgentReaderHash,
    pub challenge_hash: AgentReaderHash,
    pub nonce: AgentReaderHash,
    pub issued_at: AgentReaderTimestamp,
}
impl sealed::Sealed for AgentReaderProofV1 {}
impl AgentReaderStatementV1 for AgentReaderProofV1 {
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
pub enum AgentReaderRevocationSubjectV1 {
    #[serde(rename_all = "camelCase")]
    Association {
        association_id: AgentReaderAssociationId,
        body_hash: AgentReaderHash,
    },
    #[serde(rename_all = "camelCase")]
    Device {
        device_id: AgentReaderDeviceId,
        device_epoch: u64,
        key_fingerprint: AgentReaderHash,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentReaderRevocationV1 {
    pub version: u32,
    pub context: AgentReaderContextV1,
    pub issuer_id: AgentReaderIssuerId,
    pub issuer_epoch: u64,
    pub authority_revision: u64,
    pub previous_authority_hash: AgentReaderHash,
    pub subject: AgentReaderRevocationSubjectV1,
    pub issued_at: AgentReaderTimestamp,
}
impl sealed::Sealed for AgentReaderRevocationV1 {}
impl AgentReaderStatementV1 for AgentReaderRevocationV1 {
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
pub struct AgentReaderBundleV1 {
    pub association: AgentReaderSigned<AgentReaderAssociationV1>,
    pub grant: AgentReaderSigned<AgentReaderGrantV1>,
    pub proof: AgentReaderSigned<AgentReaderProofV1>,
    pub freshness: AgentReaderSigned<AgentReaderFreshnessV1>,
}
impl AgentReaderBundleV1 {
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        strict_json(bytes, AGENT_READER_BUNDLE_BYTES)
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
