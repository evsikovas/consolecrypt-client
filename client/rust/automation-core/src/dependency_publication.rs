//! Rust-only private Native integration. No JSON request, approval constructor,
//! SQL connection, credential, installer dispatch or public Bridge activation.
//! Native must retain its actual accepted operation and current-profile gates.
use crate::project::publication::{self, FilesystemPublished, PublicationStage, Recovered};
use crate::{
    AutomationError, DependencySourceSelection, ProjectRevision, ProjectRoot, Result,
    VerifiedDependencyPreview,
};
use std::{
    fmt,
    path::{Component, Path, PathBuf},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

fn invalid() -> AutomationError {
    AutomationError::Invalid("dependency publication context unavailable")
}
fn require(value: bool) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err(invalid())
    }
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn bounded_profile(profile: &Path) -> bool {
    profile.is_absolute()
        && profile
            .to_str()
            .is_some_and(|s| s.len() <= 4096 && !s.chars().any(char::is_control))
        && !profile
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
}

/// Bounded ORIGINAL saved Native metadata, not permission. Never bind/deserialize
/// this through FRB. The real preview + owned roots are independently required.
/// This source is consumed; its old row facts cannot be replaced after SQL CAS.
pub struct DependencyPublicationSource {
    profile: PathBuf,
    project: Uuid,
    operation: Uuid,
    old_revision: u64,
    old_row_sha256: String,
}
impl DependencyPublicationSource {
    pub fn new(
        profile: &Path,
        saved_kind: &str,
        project: Uuid,
        operation: Uuid,
        old_revision: u64,
        old_row_sha256: &str,
    ) -> Result<Self> {
        if saved_kind != "managed" {
            return Err(AutomationError::PublicationUnsupported(
                "only native managed new generations",
            ));
        }
        require(
            bounded_profile(profile)
                && !project.is_nil()
                && !operation.is_nil()
                && old_revision > 0
                && old_revision < u64::MAX
                && digest(old_row_sha256),
        )?;
        Ok(Self {
            profile: profile.to_owned(),
            project,
            operation,
            old_revision,
            old_row_sha256: old_row_sha256.into(),
        })
    }
}
impl fmt::Debug for DependencyPublicationSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DependencyPublicationSource")
            .field("project", &self.project)
            .field("old_revision", &self.old_revision)
            .finish_non_exhaustive()
    }
}

/// Native supplies these DATA only AFTER its actual same-transaction SQL ack.
/// Core cannot inspect a database or infer that ack from a boolean/journal.
/// Non-Serde; recording it never selects a catalog row or mints approval.
pub struct CatalogPublicationAck {
    project: Uuid,
    transaction: Uuid,
    revision: u64,
    row_sha256: String,
}
impl CatalogPublicationAck {
    pub(crate) fn selected_facts(&self)->(Uuid,Uuid,u64,&str) {
        (self.project,self.transaction,self.revision,&self.row_sha256)
    }

    pub fn new(project: Uuid, transaction: Uuid, revision: u64, row_sha256: &str) -> Result<Self> {
        require(!project.is_nil() && !transaction.is_nil() && revision > 1 && digest(row_sha256))?;
        Ok(Self {
            project,
            transaction,
            revision,
            row_sha256: row_sha256.into(),
        })
    }
}
impl fmt::Debug for CatalogPublicationAck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CatalogPublicationAck")
            .field("project", &self.project)
            .field("transaction", &self.transaction)
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}
/// Original journal IDs are recovery provenance DATA, never resurrected authority.
/// Unselected recovery still requires a NEW genuine Native verified preview.
pub struct PublicationRecoveryReference {
    transaction: Uuid,
    original_operation: Uuid,
}
impl PublicationRecoveryReference {
    pub fn new(transaction: Uuid, original_operation: Uuid) -> Result<Self> {
        require(!transaction.is_nil() && !original_operation.is_nil())?;
        Ok(Self {
            transaction,
            original_operation,
        })
    }
}
impl fmt::Debug for PublicationRecoveryReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PublicationRecoveryReference")
            .field("transaction", &self.transaction)
            .finish_non_exhaustive()
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum PublicationAuditObservation {
    FilesystemPublished,
    CatalogCommitted,
    Complete,
}
#[derive(Debug, PartialEq, Eq)]
pub enum DependencyPublicationDurability {
    LinuxDirectoryFsync,
    MacosFileFullSyncDirectoryFsync,
    WindowsFileFlushProcessRecovery,
}

/// Read-only physical/name/ACL pin for the generation ALREADY selected by the
/// original Native catalog context. No copy, historical-content assertion,
/// receipt/SQL acknowledgment, publication, acceptance or job capability.
/// Retain through the actual filesystem operation and its final recheck.
pub struct PinnedSelectedDependencyGeneration {
    inner: publication::PinnedSelectedGeneration,
}
impl fmt::Debug for PinnedSelectedDependencyGeneration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PinnedSelectedDependencyGeneration")
            .finish_non_exhaustive()
    }
}
impl PinnedSelectedDependencyGeneration {
    pub fn root(&self) -> &ProjectRoot {
        self.inner.root()
    }
    pub fn recheck(&self) -> Result<()> {
        self.inner.recheck()
    }
}
/// Inputs are ONLY actual saved Native metadata, not permission and never a UI
/// target path. The complete profile/project/transaction geometry is derived.
pub fn open_selected_generation(
    profile: &Path,
    project: Uuid,
    transaction: Uuid,
    publication_fingerprint: &str,
) -> Result<PinnedSelectedDependencyGeneration> {
    require(
        bounded_profile(profile)
            && !project.is_nil()
            && !transaction.is_nil()
            && digest(publication_fingerprint),
    )?;
    Ok(PinnedSelectedDependencyGeneration {
        inner: publication::open_selected_generation(
            profile,
            project,
            transaction,
            publication_fingerprint,
        )?,
    })
}

/// Opaque, owned, non-Clone/non-Serde result. Retains the actual namespace handles
/// and existing process admission across Native SQL/cleanup handoff. Never use
/// this value by itself to select a catalog row or authorize a job.
pub struct PublishedDependencyGeneration {
    inner: FilesystemPublished,
    source: DependencyPublicationSource,
    target: PathBuf,
}
impl fmt::Debug for PublishedDependencyGeneration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PublishedDependencyGeneration")
            .field("project", &self.source.project)
            .field("transaction", &self.transaction())
            .finish_non_exhaustive()
    }
}
impl PublishedDependencyGeneration {
    fn wrap(source: DependencyPublicationSource, inner: FilesystemPublished) -> Result<Self> {
        let leaf = inner.target_name();
        require(leaf == format!("generation-{}", inner.transaction()))?;
        // Only Native-captured profile/project plus Core-generated transaction.
        // The retained namespace was independently derived and checked by C.
        let target = source
            .profile
            .join("automation")
            .join("project-revisions")
            .join(source.project.to_string())
            .join(leaf);
        Ok(Self {
            inner,
            source,
            target,
        })
    }
    pub fn project_id(&self) -> Uuid {
        self.source.project
    }
    pub fn transaction(&self) -> Uuid {
        self.inner.transaction()
    }
    pub fn target_path(&self) -> &Path {
        &self.target
    }
    pub fn candidate_digest(&self) -> &str {
        self.inner.candidate_digest()
    }
    pub fn audit_fingerprint(&self) -> Result<String> {
        self.inner.audit_fingerprint()
    }
    pub fn old_catalog_revision(&self) -> u64 {
        self.source.old_revision
    }
    pub fn old_row_sha256(&self) -> &str {
        &self.source.old_row_sha256
    }
    pub fn durability(&self) -> DependencyPublicationDurability {
        match self.inner.durability() {
            publication::PublicationDurability::LinuxDirectoryFsync => {
                DependencyPublicationDurability::LinuxDirectoryFsync
            }
            publication::PublicationDurability::MacosFileFullSyncDirectoryFsync => {
                DependencyPublicationDurability::MacosFileFullSyncDirectoryFsync
            }
            publication::PublicationDurability::WindowsFileFlushProcessRecovery => {
                DependencyPublicationDurability::WindowsFileFlushProcessRecovery
            }
        }
    }
    /// Immutable OLD source facts. A caller cannot substitute its NEW SQL row
    /// revision/hash here. Full actual old roots/preview checks are repeated.
    pub fn recheck(
        &mut self,
        original: &ProjectRoot,
        before: &ProjectRevision,
        selected: &DependencySourceSelection,
        candidate: &ProjectRoot,
        preview: &VerifiedDependencyPreview,
    ) -> Result<()> {
        self.inner.recheck(
            "managed",
            self.source.old_revision,
            &self.source.old_row_sha256,
            original,
            before,
            selected,
            candidate,
            preview,
        )
    }
    fn checked_ack(&self, ack: &CatalogPublicationAck) -> Result<()> {
        require(
            ack.project == self.source.project
                && ack.transaction == self.transaction()
                && self.source.old_revision.checked_add(1) == Some(ack.revision)
                && digest(&ack.row_sha256),
        )
    }
    /// Fixed bounded audit observation AFTER Native SQL acknowledgment. Failure
    /// is recovery-required; it never means the acknowledged SQL rolled back.
    pub fn record_catalog_ack(
        &mut self,
        ack: &CatalogPublicationAck,
        original: &ProjectRoot,
        before: &ProjectRevision,
        selected: &DependencySourceSelection,
        candidate: &ProjectRoot,
        preview: &VerifiedDependencyPreview,
    ) -> Result<()> {
        self.checked_ack(ack)?;
        self.recheck(original, before, selected, candidate, preview)?;
        self.inner.record_catalog_ack(ack.revision, &ack.row_sha256)
    }
    /// Complete describes FS+recorded catalog ACK audit, NOT helper/remote-exit
    /// proof. Native retains its own resources/IO permits until cleanup joins.
    pub fn record_complete(
        &mut self,
        ack: &CatalogPublicationAck,
        original: &ProjectRoot,
        before: &ProjectRevision,
        selected: &DependencySourceSelection,
        candidate: &ProjectRoot,
        preview: &VerifiedDependencyPreview,
    ) -> Result<()> {
        self.checked_ack(ack)?;
        self.recheck(original, before, selected, candidate, preview)?;
        self.inner.record_complete(ack.revision, &ack.row_sha256)
    }
    /// Read-only DATA. An observed Complete never reconstructs this opaque owner,
    /// an archive witness, current Native scope, SQL admission or job authority.
    pub fn audit_observation(&self) -> Result<PublicationAuditObservation> {
        Ok(match self.inner.catalog_audit_observation()? {
            publication::CatalogAuditObservation::FilesystemPublished => {
                PublicationAuditObservation::FilesystemPublished
            }
            publication::CatalogAuditObservation::CatalogCommitted => {
                PublicationAuditObservation::CatalogCommitted
            }
            publication::CatalogAuditObservation::Complete => PublicationAuditObservation::Complete,
        })
    }
}

#[allow(clippy::too_many_arguments)]
pub fn prepare_publish(
    source: DependencyPublicationSource,
    original: &ProjectRoot,
    before: &ProjectRevision,
    selected: &DependencySourceSelection,
    candidate: &ProjectRoot,
    preview: &VerifiedDependencyPreview,
    cancel: &CancellationToken,
) -> Result<PublishedDependencyGeneration> {
    let prepared = PublicationStage::prepare(
        &source.profile,
        "managed",
        source.project,
        source.operation,
        source.old_revision,
        &source.old_row_sha256,
        original,
        before,
        selected,
        candidate,
        preview,
        cancel,
    )?;
    let inner = prepared.publish_new(preview.review_digest(), cancel)?;
    PublishedDependencyGeneration::wrap(source, inner)
}
#[allow(clippy::too_many_arguments)]
pub fn recover_publish(
    source: DependencyPublicationSource,
    reference: PublicationRecoveryReference,
    original: &ProjectRoot,
    before: &ProjectRevision,
    selected: &DependencySourceSelection,
    candidate: &ProjectRoot,
    preview: &VerifiedDependencyPreview,
    cancel: &CancellationToken,
) -> Result<PublishedDependencyGeneration> {
    let recovered = PublicationStage::recover(
        &source.profile,
        "managed",
        source.project,
        reference.original_operation,
        reference.transaction,
        source.old_revision,
        &source.old_row_sha256,
        original,
        before,
        selected,
        candidate,
        preview,
        cancel,
    )?;
    let inner = match recovered {
        Recovered::Ready(prepared) => prepared.publish_new(preview.review_digest(), cancel)?,
        Recovered::Published(published) => published,
    };
    PublishedDependencyGeneration::wrap(source, inner)
}

/// Repair ONLY bounded completion observations for a generation ALREADY selected
/// by Native's actual current SQL acknowledgment. No publisher/approval/preview
/// is returned. Edited/unknown old facts or target stay recovery-required.
pub fn reconcile_selected_catalog(
    source: DependencyPublicationSource,
    reference: PublicationRecoveryReference,
    ack: &CatalogPublicationAck,
    installed_digest: &str,
    publication_fingerprint: &str,
) -> Result<PublicationAuditObservation> {
    require(
        source.project == ack.project
            && reference.transaction == ack.transaction
            && digest(installed_digest)
            && digest(publication_fingerprint)
            && source.old_revision.checked_add(1) == Some(ack.revision),
    )?;
    let result =
        publication::reconcile_selected_catalog(publication::SelectedCatalogAuditContext {
            profile: &source.profile,
            project: source.project,
            original_operation: reference.original_operation,
            transaction: reference.transaction,
            old_revision: source.old_revision,
            old_row_sha256: &source.old_row_sha256,
            selected_revision: ack.revision,
            selected_row_sha256: &ack.row_sha256,
            installed_digest,
            publication_fingerprint,
        })?;
    Ok(match result {
        publication::CatalogAuditObservation::FilesystemPublished => {
            PublicationAuditObservation::FilesystemPublished
        }
        publication::CatalogAuditObservation::CatalogCommitted => {
            PublicationAuditObservation::CatalogCommitted
        }
        publication::CatalogAuditObservation::Complete => PublicationAuditObservation::Complete,
    })
}

/// Read-only DATA observation, NOT a publication grant or durable availability.
/// Native supplies the original saved managed root under its own scope guards.
/// Creates/replaces/repairs nothing and must be repeated before managed fetch.
pub fn check_dependency_publication_base(profile: &Path, project: Uuid, original: &ProjectRoot) -> Result<()> {
    require(bounded_profile(profile) && !project.is_nil())?;
    publication::check_publication_base(profile, project, original)
}
