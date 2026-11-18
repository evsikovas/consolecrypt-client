//! Versioned private helper IPC, independent of a terminal or sync server.

mod approval;
mod manual_owner;
pub use manual_owner::ManualOwnerGrant;
mod cleanup;
mod dependency_archive;
mod dependency_launch;
mod dependency_preview;
mod dependency_transfer;
mod diagnostic;
mod diff;
pub use diagnostic::structural_summary;
mod download;
mod events;
mod freeze;
mod inventory;
mod inventory_artifact;
mod inventory_binding;
mod inventory_source;
mod json;
mod launch;
mod network;
mod probe;
mod project;
mod queue;
mod remote;
mod rpc;
mod scope;
mod setup;
mod source;
mod target;
mod upload;
mod validation;
mod vault_inputs;
mod witness;

pub use approval::{EnrolledJobGrant, SignedJobApproval, VerifiedJob};
pub use cleanup::{
    ManagedCleanupApproval, ManagedCleanupCandidate, ManagedCleanupIdentity, ManagedCleanupPaths,
    ManagedCleanupProtection, ManagedCleanupRemoved, ManagedCleanupResult, ManagedCleanupScan,
    cleanup_delete_environment, cleanup_delete_remote_environment, cleanup_scan_environment,
    cleanup_scan_remote_environment,
};
pub use dependency_archive::{
    AdmissionError as DependencyArchiveError, ArtifactSource as DependencyArtifactSource,
    DependencyKind, SourceSelection as DependencySourceSelection,
    VerifiedArchive as DependencyArchiveWitness, verify_archive as verify_dependency_archive,
};
pub use dependency_launch::DependencyHelperConnection;
pub use dependency_preview::{
    DependencyBasePreflight, VerifiedDependencyPreview, dependency_preflight,
};
pub use dependency_transfer::{VerifiedDependencyArchive, receive_dependency_archive};
pub use diff::{ProjectChange, ProjectDiff, ProjectFileDiff, compare_file, compare_revisions};
pub use events::{JobEvent, JobEventPage};
pub use inventory::{InventorySelection, ProjectInventory};
pub use inventory_artifact::{InventoryArtifact, InventoryReference};
pub use inventory_binding::{
    BindingResolution, ExternalInventoryConfig, InventoryBinding, InventoryMode, SourceEndpoint,
};
pub use inventory_source::{
    CapturedInventory, InventoryFormat, InventorySource, SourceInventory, SourceInventoryGroup,
    SourceInventoryHost, resolve_inventory_sources,
};
pub use launch::{HelperLaunch, HelperProcess};
pub use network::OriginStream;
pub use probe::EnvironmentProbe;
pub use project::{
    ProjectEntry, ProjectFile, ProjectRevision, ProjectRevisionPolicy, ProjectRoot,
    validate_project_path,
};
pub use queue::{JobCoordinator, JobTicket};
pub use remote::RemoteHelperConfig;
pub use rpc::{HelperPeer, MAX_FRAME_BYTES, RemoteFailure, read_frame};
pub use scope::{JobScopeRegistry, JobTargetScope, target_from_session};
pub use setup::{SetupTarget, prerequisite_script, setup_environment, setup_remote_environment};
pub use source::{GIT_SOURCE_FILE, GitProjectSource};
#[path = "become.rs"]
mod sudo_input;
pub use sudo_input::ScopedBecomePassword;
mod git_workstation;
pub use git_workstation::{capture_git_workstation, git_workstation_available};
pub use target::{TargetFailure, TargetFuture, TargetHandler, TargetRequest};
pub use upload::UploadedRevision;
pub use validation::{RunParameters, ValidationCheck, ValidationProof, VerifiedValidation};
pub use witness::InputDigestWitness;

#[derive(Debug, thiserror::Error)]
pub enum AutomationError {
    #[error("project publication unsupported: {0}")]
    PublicationUnsupported(&'static str),
    #[error("project file changed since review; reload before saving")]
    FileChanged,
    #[error("invalid automation configuration: {0}")]
    Invalid(&'static str),
    #[error("automation helper protocol error: {0}")]
    Protocol(&'static str),
    #[error("automation helper channel closed")]
    Closed,
    #[error("automation helper request timed out; it has not been retried")]
    Timeout,
    #[error("automation helper request queue is full")]
    QueueFull,
    #[error("automation helper operation failed: {0}")]
    Remote(String),
    #[error("automation helper I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, AutomationError>;

pub use vault_inputs::{
    AdmittedVaultPasswordLease, VAULT_INPUT_FILE, VaultInputBinding, VaultPasswordLease,
    VaultPasswordPurpose, VaultPasswordRequest,
};

mod vault_job;
mod vault_validation;
pub use vault_job::VaultRunBinding;
pub use vault_validation::{
    VaultValidationBinding, VaultValidationMarker, VaultValidationPasswordLease,
    VaultValidationPasswordRequest, VaultValidationPhase, VaultValidationPurpose,
    VaultValidationRuntime,
};

mod vault_control;
pub use vault_control::{
    VaultJobControlRequest, VaultJobResponder, VaultValidationControlFuture,
    VaultValidationControlRequest, VaultValidationHandler, VaultValidationResponder,
};

mod validation_launch;
pub use validation_launch::{
    ValidationHelperConnection, ValidationPeerObserver, VaultValidationResult,
};

#[cfg(windows)]
#[allow(unsafe_code)] // one audited read-only FileIdInfo adapter; no path/open/write API
mod project_identity_windows;
#[cfg(windows)]
pub use project_identity_windows::same as same_existing_file_identity;

#[cfg(test)]
mod project_identity_tests;

// Private ANS04 prerequisite, no Bridge or automatic export/import.
pub mod portable_package;

#[cfg(test)]
mod project_publication_policy_tests;

// Rust-only publication owner. Never expose these types as Bridge JSON inputs.
mod dependency_publication;
pub use dependency_publication::{
    CatalogPublicationAck, DependencyPublicationDurability, DependencyPublicationSource,
    PinnedSelectedDependencyGeneration, PublicationAuditObservation, PublicationRecoveryReference,
    PublishedDependencyGeneration, check_dependency_publication_base, open_selected_generation,
    prepare_publish, reconcile_selected_catalog, recover_publish,
};

// Private Native integration only; no generic RPC or imported authority.
pub mod portable_publication;

pub use dependency_preview::{ProjectCollectionPin, project_collection_pins};

mod service_execution;
pub use service_execution::{
    ServiceCapsuleInput, bind_service_job_scope, service_known_hosts_digest,
};
mod service_supervisor;
pub use service_supervisor::{ServiceExecutionObservation, ServiceExecutionSupervisor};
mod service_node;
pub use service_node::ServiceNode;

mod service_remote;
mod service_stdio;
pub use service_stdio::{
    service_node_stdio, service_node_stdio_with_identity, service_node_unix_with_identity,
};

mod service_identity;
pub use service_identity::{ServiceIdentity, ServiceIdentityFacts, ServiceLocalNamespace};

mod service_owner_status;

mod service_restart;

pub use service_remote::{ServicePeerObservation, ServiceRemoteConfig, ServiceState};
