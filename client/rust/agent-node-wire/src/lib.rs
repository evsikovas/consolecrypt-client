//! Exact vendored Agent public wire/policy, not an Actor, store or enrollment grant.
#![forbid(unsafe_code)]
use serde::{Deserialize, Serialize};
mod tunnel;
mod wire;
pub use tunnel::*;
pub use wire::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidWire,
    InvalidKey,
    InvalidSignature,
    InvalidTime,
    Expired,
    WrongContext,
    WrongEpoch,
    WrongCheckpoint,
    UnsupportedTeam,
    Replay,
    NotFound,
    WrongState,
    InvalidToken,
    WrongClaim,
    ScopeDenied,
    CapabilityDenied,
    MissingFreshness,
    Revoked,
    Quota,
    Conflict,
    Storage,
    Entropy,
    KeyReuse,
    UncertainCommit,
}
pub type Result<T> = std::result::Result<T, Error>;
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnerPin {
    pub context: Context,
    pub key: PublicKey,
    pub epoch: u64,
    pub checkpoint: u64,
    pub expires_at: Timestamp,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeRequestBinding {
    pub certificate: Signed<Certificate>,
    pub lease: Signed<Lease>,
    pub freshness: Signed<Freshness>,
    pub operation: Operation,
}
/// Cryptographic verification only; creates no Actor admission or authority.
pub fn verify_statement<T: Statement>(signed: &Signed<T>, key: PublicKey) -> Result<()> {
    signed.verify(key)
}
pub fn validate_pin(pin: &OwnerPin, now: Now) -> Result<()> {
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
    Ok(())
}
pub fn check_live(start: Timestamp, end: Timestamp, now: Now) -> Result<()> {
    wire::live(start, end, now)
}
pub fn check_recent(issued: Timestamp, now: Now) -> Result<()> {
    wire::recent(issued, now)
}

mod owner;
pub use owner::{PreparedTunnelOwner, TunnelOwnerSigned, metrics_collector_id, validate_metrics_manifest, metrics_manifest_runtime, MetricsRuntime};

mod reader_node_owner;
pub use reader_node_owner::{PreparedReaderNodeOwner, ReaderNodeOwnerReviewBody, ReaderNodeOwnerReviewDetails, ReaderNodeOwnerSigned};
#[cfg(test)]
mod reader_node_owner_tests;

mod inner;
pub use inner::{open_frame, verify_ack, verify_binding, VerifiedTunnelAck};

#[cfg(test)]
mod owner_tests;

mod tunnel_v2;
pub use tunnel_v2::TunnelRoutePolicyV2;
mod inner_v2;
pub use inner_v2::{open_original_frame, verify_binding_v2, verify_original_challenge, verify_original_ack, OriginalTunnelMode, PendingOriginalChallenge, VerifiedOriginalAck};

mod collector_service_owner;
pub use collector_service_owner::{PreparedCollectorServiceOwner, CollectorServiceOwnerSigned};

mod exporter;
pub use exporter::{ExporterArtifact,ManagedExporterPolicy};
