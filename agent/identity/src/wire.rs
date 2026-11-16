use crate::{Error, Result};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest as _, Sha256};
use std::fmt;
use uuid::Uuid;
use zeroize::Zeroizing;

pub const MAX_WIRE_BYTES: usize = 16 * 1024;
pub const OFFER_TTL: u64 = 600;
pub const CERTIFICATE_TTL: u64 = 30 * 24 * 60 * 60;
pub const LEASE_TTL: u64 = 300;
pub const PROOF_TTL: u64 = 60;

macro_rules! id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);
        impl $name {
            pub fn new(value: &str) -> Result<Self> {
                let id = Uuid::parse_str(value).map_err(|_| Error::InvalidWire)?;
                if id.is_nil() || id.hyphenated().to_string() != value {
                    return Err(Error::InvalidWire);
                }
                Ok(Self(id))
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
                Self::new(&String::deserialize(d)?).map_err(de::Error::custom)
            }
        }
    };
}
id!(AgentId);
id!(InvitationId);
id!(LocalProfileId);
id!(PersonalAccountId);
id!(TeamId);
id!(DeploymentId);
id!(ResourceId);
id!(LeaseId);

macro_rules! bytes {
    ($name:ident, $n:expr) => {
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $name(pub(crate) [u8; $n]);
        impl $name {
            pub fn from_bytes(value: [u8; $n]) -> Self {
                Self(value)
            }
            pub fn as_bytes(&self) -> &[u8; $n] {
                &self.0
            }
        }
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), hex::encode(self.0))
            }
        }
        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
                s.serialize_str(&hex::encode(self.0))
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                if s.len() != $n * 2
                    || !s
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    return Err(de::Error::custom("invalid canonical hex"));
                }
                let mut out = [0; $n];
                hex::decode_to_slice(s, &mut out).map_err(|_| de::Error::custom("invalid hex"))?;
                Ok(Self(out))
            }
        }
    };
}
bytes!(Hash, 32);
bytes!(PublicKey, 32);
bytes!(SignatureBytes, 64);
bytes!(Nonce, 16);
impl PublicKey {
    pub fn validate(&self) -> Result<()> {
        let key = VerifyingKey::from_bytes(&self.0).map_err(|_| Error::InvalidKey)?;
        if key.is_weak() {
            return Err(Error::InvalidKey);
        }
        Ok(())
    }
    pub fn fingerprint(&self) -> Hash {
        hash(b"consolecrypt.agent.public-key.v1", &self.0)
    }
}
impl Nonce {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.0 == [0; 16] {
            Err(Error::InvalidWire)
        } else {
            Ok(())
        }
    }
}

/// Caller later supplies OS-generated entropy through a reviewed adapter. This
/// crate generates/stores no invitation token; state retains only its digest.
pub struct InvitationToken(Zeroizing<[u8; 32]>);
impl InvitationToken {
    pub fn from_bytes(value: [u8; 32]) -> Result<Self> {
        if value == [0; 32] {
            return Err(Error::InvalidToken);
        }
        Ok(Self(Zeroizing::new(value)))
    }
    pub fn digest(&self) -> Hash {
        hash(b"consolecrypt.agent.invitation-token.v1", self.0.as_ref())
    }
}
impl fmt::Debug for InvitationToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InvitationToken([redacted])")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(u64);
impl Timestamp {
    pub fn new(seconds: u64) -> Result<Self> {
        if seconds > 253_402_300_799 {
            Err(Error::InvalidTime)
        } else {
            Ok(Self(seconds))
        }
    }
    pub fn seconds(self) -> u64 {
        self.0
    }
    pub fn checked_add(self, seconds: u64) -> Result<Self> {
        Self::new(self.0.checked_add(seconds).ok_or(Error::InvalidTime)?)
    }
    pub(crate) fn valid(self) -> Result<()> {
        Self::new(self.0).map(|_| ())
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Now(pub Timestamp);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Tenant {
    LocalProfile { id: LocalProfileId },
    PersonalAccount { id: PersonalAccountId },
    Team { id: TeamId },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Context {
    pub tenant: Tenant,
    pub deployment_id: DeploymentId,
}
impl Context {
    pub(crate) fn personal(&self) -> Result<()> {
        if matches!(self.tenant, Tenant::Team { .. }) {
            Err(Error::UnsupportedTeam)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Scope {
    #[serde(rename = "telemetry.read")]
    TelemetryRead,
    #[serde(rename = "metrics.collect")]
    MetricsCollect,
    #[serde(rename = "exporters.manage")]
    ExportersManage,
    #[serde(rename = "tunnel.open")]
    TunnelOpen,
    #[serde(rename = "jobs.execute")]
    JobsExecute,
    #[serde(rename = "agent.update")]
    AgentUpdate,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Scopes(pub Vec<Scope>);
impl Scopes {
    pub fn validate(&self) -> Result<()> {
        if self.0.len() > 6 || !self.0.windows(2).all(|p| p[0] < p[1]) {
            Err(Error::InvalidWire)
        } else {
            Ok(())
        }
    }
    pub fn contains(&self, scope: Scope) -> bool {
        self.0.contains(&scope)
    }
    pub(crate) fn subset(&self, other: &Self) -> bool {
        self.0.iter().all(|s| other.contains(*s))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Capabilities {
    pub version: u32,
    pub scopes: Scopes,
}
impl Capabilities {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err(Error::InvalidWire);
        }
        self.scopes.validate()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Header {
    pub version: u32,
    pub context: Context,
    pub owner_epoch: u64,
    pub checkpoint: u64,
}
impl Header {
    fn validate(&self) -> Result<()> {
        if self.version != 1 || self.owner_epoch == 0 || self.checkpoint == 0 {
            Err(Error::InvalidWire)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Offer {
    pub header: Header,
    pub invitation_id: InvitationId,
    pub agent_id: AgentId,
    pub agent_key: PublicKey,
    pub token_hash: Hash,
    pub scopes: Scopes,
    pub issued_at: Timestamp,
    pub expires_at: Timestamp,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Claim {
    pub header: Header,
    pub invitation_id: InvitationId,
    pub offer_hash: Hash,
    pub agent_id: AgentId,
    pub agent_key: PublicKey,
    pub capabilities: Capabilities,
    pub nonce: Nonce,
    pub issued_at: Timestamp,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum CertificateOrigin {
    Enrollment {
        #[serde(rename = "invitationId")]
        invitation_id: InvitationId,
        #[serde(rename = "claimHash")]
        claim_hash: Hash,
        #[serde(rename = "keyFingerprint")]
        key_fingerprint: Hash,
    },
    Rotation {
        #[serde(rename = "previousCertificate")]
        previous_certificate: Hash,
        #[serde(rename = "previousKeyEpoch")]
        previous_key_epoch: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Certificate {
    pub header: Header,
    pub agent_id: AgentId,
    pub key_epoch: u64,
    pub agent_key: PublicKey,
    pub scopes: Scopes,
    pub capabilities: Capabilities,
    pub origin: CertificateOrigin,
    pub not_before: Timestamp,
    pub expires_at: Timestamp,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Rejection {
    pub header: Header,
    pub invitation_id: InvitationId,
    pub claim_hash: Hash,
    pub issued_at: Timestamp,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Revocation {
    pub header: Header,
    pub agent_id: AgentId,
    pub key_epoch: u64,
    pub certificate_hash: Hash,
    pub issued_at: Timestamp,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Freshness {
    pub header: Header,
    pub issued_at: Timestamp,
    pub expires_at: Timestamp,
}

/// Heartbeat has no telemetry scope: it authenticates only this agent's own
/// metadata. It cannot read other metrics or imply any of the six scopes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Operation {
    OwnHeartbeat,
    TelemetryRead {
        source: ResourceId,
    },
    MetricsCollect {
        collector: ResourceId,
    },
    ExportersManage {
        manifest: Hash,
    },
    TunnelOpen {
        route: ResourceId,
    },
    #[serde(rename_all = "camelCase")]
    TunnelOpenBound {
        route: ResourceId,
        policy_hash: Hash,
    },
    JobsExecute {
        manifest: Hash,
    },
    AgentUpdate {
        artifact: Hash,
    },
}
impl Operation {
    pub fn scope(&self) -> Option<Scope> {
        Some(match self {
            Self::OwnHeartbeat => return None,
            Self::TelemetryRead { .. } => Scope::TelemetryRead,
            Self::MetricsCollect { .. } => Scope::MetricsCollect,
            Self::ExportersManage { .. } => Scope::ExportersManage,
            Self::TunnelOpen { .. } | Self::TunnelOpenBound { .. } => Scope::TunnelOpen,
            Self::JobsExecute { .. } => Scope::JobsExecute,
            Self::AgentUpdate { .. } => Scope::AgentUpdate,
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Lease {
    pub header: Header,
    pub id: LeaseId,
    pub agent_id: AgentId,
    pub key_epoch: u64,
    pub certificate_hash: Hash,
    pub operation: Operation,
    pub issued_at: Timestamp,
    pub expires_at: Timestamp,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentRequest {
    pub header: Header,
    pub agent_id: AgentId,
    pub key_epoch: u64,
    pub lease_hash: Hash,
    pub nonce: Nonce,
    pub issued_at: Timestamp,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RotationProof {
    pub header: Header,
    pub agent_id: AgentId,
    pub certificate_hash: Hash,
    pub nonce: Nonce,
    pub issued_at: Timestamp,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnerRotation {
    pub header: Header,
    pub new_epoch: u64,
    pub new_key: PublicKey,
    pub issued_at: Timestamp,
    pub expires_at: Timestamp,
}

mod sealed {
    pub trait Sealed {}
}
/// Sealed typed statement domains: adapters cannot invent a new raw-signing API.
pub trait Statement: Serialize + de::DeserializeOwned + Clone + sealed::Sealed {
    const DOMAIN: &'static str;
    fn validate(&self) -> Result<()>;
    fn signing_bytes(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let value = serde_json::to_value(self).map_err(|_| Error::InvalidWire)?;
        // Explicit sorting survives serde_json/preserve_order feature unification
        // in a future workspace. This is not advertised as RFC8785/JCS.
        let mut body = Vec::new();
        canonical_json(&value, &mut body)?;
        if body.len() > MAX_WIRE_BYTES {
            return Err(Error::Quota);
        }
        let mut bytes = b"consolecrypt.agent.identity.personal-prototype\0v1\0".to_vec();
        bytes.extend_from_slice(Self::DOMAIN.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&(body.len() as u32).to_be_bytes());
        bytes.extend(body);
        Ok(bytes)
    }
    fn digest(&self) -> Result<Hash> {
        Ok(hash(
            b"consolecrypt.agent.statement-hash.v1",
            &self.signing_bytes()?,
        ))
    }
}
macro_rules! statement {
    ($name:ident,$domain:literal,$value:ident,$body:block) => {
        impl sealed::Sealed for $name {}
        impl Statement for $name {
            const DOMAIN: &'static str = $domain;
            fn validate(&self) -> Result<()> {
                let $value = self;
                $value.header.validate()?;
                $body
            }
        }
    };
}
statement!(Offer, "offer", s, {
    s.agent_key.validate()?;
    s.scopes.validate()?;
    window(s.issued_at, s.expires_at, OFFER_TTL)
});
statement!(Claim, "claim", s, {
    s.agent_key.validate()?;
    s.capabilities.validate()?;
    s.nonce.validate()?;
    s.issued_at.valid()
});
statement!(Certificate, "certificate-approval", s, {
    s.agent_key.validate()?;
    s.scopes.validate()?;
    s.capabilities.validate()?;
    if s.key_epoch == 0 {
        return Err(Error::InvalidWire);
    }
    window(s.not_before, s.expires_at, CERTIFICATE_TTL)
});
statement!(Rejection, "reject", s, { s.issued_at.valid() });
statement!(Revocation, "revoke", s, {
    if s.key_epoch == 0 {
        return Err(Error::InvalidWire);
    }
    s.issued_at.valid()
});
statement!(Freshness, "freshness", s, {
    window(s.issued_at, s.expires_at, LEASE_TTL)
});
statement!(Lease, "operation-lease", s, {
    if s.key_epoch == 0 {
        return Err(Error::InvalidWire);
    }
    window(s.issued_at, s.expires_at, LEASE_TTL)
});
statement!(AgentRequest, "agent-request", s, {
    if s.key_epoch == 0 {
        return Err(Error::InvalidWire);
    }
    s.nonce.validate()?;
    s.issued_at.valid()
});
statement!(RotationProof, "agent-rotation-old", s, {
    s.nonce.validate()?;
    s.issued_at.valid()
});
statement!(OwnerRotation, "owner-rotation-old", s, {
    s.new_key.validate()?;
    window(s.issued_at, s.expires_at, CERTIFICATE_TTL)
});

/// Separate PoP domain for the new key; same exact typed transition body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NewKeyProof(pub RotationProof);
impl sealed::Sealed for NewKeyProof {}
impl Statement for NewKeyProof {
    const DOMAIN: &'static str = "agent-rotation-new";
    fn validate(&self) -> Result<()> {
        self.0.validate()
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NewOwnerProof(pub OwnerRotation);
impl sealed::Sealed for NewOwnerProof {}
impl Statement for NewOwnerProof {
    const DOMAIN: &'static str = "owner-rotation-new";
    fn validate(&self) -> Result<()> {
        self.0.validate()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, bound(deserialize = "T:de::DeserializeOwned"))]
pub struct Signed<T> {
    pub body: T,
    pub signature: SignatureBytes,
}
impl<T: Statement> Signed<T> {
    pub fn from_json(raw: &[u8]) -> Result<Self> {
        if raw.len() > MAX_WIRE_BYTES {
            return Err(Error::Quota);
        }
        let value: Self = serde_json::from_slice(raw).map_err(|_| Error::InvalidWire)?;
        value.body.validate()?;
        Ok(value)
    }
    pub(crate) fn verify(&self, key: PublicKey) -> Result<()> {
        key.validate()?;
        let key = VerifyingKey::from_bytes(&key.0).map_err(|_| Error::InvalidKey)?;
        key.verify_strict(
            &self.body.signing_bytes()?,
            &Signature::from_bytes(&self.signature.0),
        )
        .map_err(|_| Error::InvalidSignature)
    }
}
pub(crate) fn hash(domain: &[u8], bytes: &[u8]) -> Hash {
    let mut h = Sha256::new();
    h.update(domain);
    h.update([0]);
    h.update(bytes);
    Hash(h.finalize().into())
}

pub(crate) fn canonical_json(value: &serde_json::Value, out: &mut Vec<u8>) -> Result<()> {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            out.push(b'{');
            let mut entries = map.iter().collect::<Vec<_>>();
            entries.sort_unstable_by(|a, b| a.0.cmp(b.0));
            for (index, (key, value)) in entries.into_iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                serde_json::to_writer(&mut *out, key).map_err(|_| Error::InvalidWire)?;
                out.push(b':');
                canonical_json(value, out)?;
            }
            out.push(b'}');
        }
        Value::Array(values) => {
            out.push(b'[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                canonical_json(value, out)?;
            }
            out.push(b']');
        }
        Value::Number(number) if !number.is_i64() && !number.is_u64() => {
            return Err(Error::InvalidWire)
        }
        _ => serde_json::to_writer(&mut *out, value).map_err(|_| Error::InvalidWire)?,
    }
    if out.len() > MAX_WIRE_BYTES {
        return Err(Error::Quota);
    }
    Ok(())
}
pub(crate) fn window(start: Timestamp, end: Timestamp, max: u64) -> Result<()> {
    start.valid()?;
    end.valid()?;
    if end.0 <= start.0 || end.0 - start.0 > max {
        Err(Error::InvalidTime)
    } else {
        Ok(())
    }
}
pub(crate) fn live(start: Timestamp, end: Timestamp, now: Now) -> Result<()> {
    now.0.valid()?;
    if now.0 < start {
        Err(Error::InvalidTime)
    } else if now.0 >= end {
        Err(Error::Expired)
    } else {
        Ok(())
    }
}
pub(crate) fn recent(issued: Timestamp, now: Now) -> Result<()> {
    issued.valid()?;
    now.0.valid()?;
    if issued > now.0 || now.0 .0 - issued.0 > PROOF_TTL {
        Err(Error::InvalidTime)
    } else {
        Ok(())
    }
}

/// Signed release metadata uses a separate signer pin and domain. Version is a
/// monotonic release sequence; display_version identifies the reviewed package.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseManifest {
    pub format: u32,
    pub version: u64,
    pub display_version: String,
    pub protocol: u32,
    pub capabilities: Capabilities,
    pub platform: String,
    pub artifact_sha256: Hash,
    pub artifact_bytes: u64,
    pub issued_at: Timestamp,
    pub expires_at: Timestamp,
}
impl sealed::Sealed for ReleaseManifest {}
impl Statement for ReleaseManifest {
    const DOMAIN: &'static str = "agent-release-manifest";
    fn validate(&self) -> Result<()> {
        if self.format != 1
            || self.version == 0
            || self.protocol == 0
            || self.display_version.is_empty()
            || self.display_version.len() > 64
            || !self
                .display_version
                .bytes()
                .all(|v| v.is_ascii_alphanumeric() || b".-+".contains(&v))
            || !matches!(self.platform.as_str(), "linux-x86_64" | "linux-aarch64")
            || self.artifact_bytes == 0
            || self.artifact_bytes > 256 * 1024 * 1024
        {
            return Err(Error::InvalidWire);
        }
        self.capabilities.validate()?;
        window(self.issued_at, self.expires_at, 90 * 24 * 60 * 60)
    }
}

#[path = "wire/collector_service.rs"]
mod collector_service;
pub use collector_service::*;

mod collector_credentials;
pub use collector_credentials::*;

#[path="wire/collector_service_credentials.rs"]
mod collector_service_credentials;
pub use collector_service_credentials::*;
