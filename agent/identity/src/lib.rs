//! Isolated owner-pinned personal identity prototype with a bounded durable
//! signed-transition journal and narrow subprocess gate. No team authority,
//! auto-enrollment, vault access or E2EE claim. Node-local initial key provisioning
//! is explicit, Linux-only and never supplies owner/account/vault keys.
#![forbid(unsafe_code)]

mod durable;
mod node_activation;
mod node_inspect;
mod node_prepare;
mod policy;
mod provision;
pub mod reader;
mod release;
#[cfg(target_os = "linux")]
mod sqlite;
mod tunnel;
mod wire;
pub use durable::{FileStore, StorageStatus};
#[cfg(target_os = "linux")]
pub use node_activation::{activate_prepared_rotation, activate_prepared_tunnel_rotation};
pub use node_activation::{LocalActivationInput, LocalActivationPublic};
pub use node_inspect::{
    inspect_node_state, GuardIdentity, LocalNodeInspection, NodeInspectionGuard, PrivateLayout,
    SelectionState,
};
pub use node_prepare::{prepare_node_rotation, PreparedNodeKeyPublic};
pub use policy::*;
pub use provision::{
    create_node_claim, create_node_request, create_tunnel_node_claim, create_tunnel_node_request,
    create_original_tunnel_node_request,
    create_metrics_node_claim, create_metrics_node_request,
    inspect_existing_node_identity, provision_node, sign_prepared_node_rotation, sign_prepared_tunnel_node_rotation, NodeRequestBinding, PreparedRotationInput,
    PreparedRotationProofs, PublicNodeIdentity,
};
pub use release::*;
#[cfg(target_os = "linux")]
pub use sqlite::{SqliteJournalStore, SqliteStorageBudget, SqliteStorageStatus};
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

#[cfg(target_os = "linux")]
mod tunnel_supervisor;
#[cfg(target_os = "linux")]
pub use tunnel_supervisor::run_tunnel_supervised_worker;

#[cfg(target_os = "linux")]
mod enrollment_control;
#[cfg(target_os = "linux")]
pub use enrollment_control::run_enrollment_control_worker;

#[cfg(target_os="linux")]
pub(crate) use enrollment_control::{RegisteredCredentialWriter,RegisteredServiceNamespace};

#[cfg(target_os="linux")]
pub fn run_collector_service_worker(args:&[String])->Result<()> {
    enrollment_control::run_fixed_collector_service(args)
}

pub(crate) use policy::{CollectorServiceStoredObservation,ActiveCollectorServiceObservation,CollectorServiceEndReason};

#[cfg(target_os="linux")]
mod collector_credentials;

pub(crate) use policy::{StoredCollectorServiceCredentialSet, LifetimeReadback};

/// Default-OFF owned protocol component. Not a production capability/worker.
#[cfg(all(target_os="linux", feature="owned-lifetime-protocol-proof"))]
pub fn run_owned_lifetime_protocol_owner(args: &[String]) -> Result<()> {
    enrollment_control::metrics::service_supervisor::run_owned_protocol_component(args)
}

mod exporter;
pub use exporter::{ExporterArtifact,ManagedExporterPolicy};

pub use provision::{create_exporter_node_claim,create_exporter_node_request};
#[cfg(target_os="linux")]
pub use enrollment_control::run_managed_exporter_exec;
