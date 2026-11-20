//! Private Native-purpose facade for a NEW managed project. Source is bounded
//! Native metadata, not approval; genuine authenticated package stays mandatory.
//! No UI/helper destination, imported catalog ID, inventory, Runner or resolver.
use crate::dependency_publication::CatalogPublicationAck;
use crate::portable_package::PortableImport;
use crate::project::publication::portable_import::{
    self, PinnedPortableImport, PreparedPortableImport, PublishedPortableImport,
};
use crate::{AutomationError, ProjectRoot, Result};
use std::path::{Component, Path, PathBuf};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

fn valid_profile(profile: &Path) -> bool {
    profile.is_absolute()
        && profile
            .to_str()
            .is_some_and(|s| s.len() <= 4096 && !s.chars().any(char::is_control))
        && !profile
            .components()
            .any(|c| matches!(c, Component::CurDir | Component::ParentDir))
}
fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn invalid() -> AutomationError {
    AutomationError::Invalid("portable publication requires explicit recovery")
}

/// Captured AFTER Native's genuine absent-ID→pending reservation SQL CAS.
/// IDs are Native-created, paths derive ONLY from the original profile. This
/// is non-Serde DATA and cannot construct a package, approval or publication.
pub struct PortablePublicationSource {
    profile: PathBuf,
    project: Uuid,
    operation: Uuid,
    transaction: Uuid,
    reservation_row_sha256: String,
}
impl PortablePublicationSource {
    pub fn new(
        profile: &Path,
        project: Uuid,
        operation: Uuid,
        transaction: Uuid,
        reservation_row_sha256: &str,
    ) -> Result<Self> {
        if !valid_profile(profile)
            || project.is_nil()
            || operation.is_nil()
            || transaction.is_nil()
            || !valid_hash(reservation_row_sha256)
        {
            return Err(invalid());
        }
        Ok(Self {
            profile: profile.to_owned(),
            project,
            operation,
            transaction,
            reservation_row_sha256: reservation_row_sha256.into(),
        })
    }
}
impl std::fmt::Debug for PortablePublicationSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PortablePublicationSource([NATIVE METADATA])")
    }
}
/// Read-only observation, no mkdir/repair/approval/capability. Native repeats
/// actual original scope checks and this namespace validation before mutation.
pub fn check_portable_publication_base(profile: &Path, project: Uuid) -> Result<()> {
    if !valid_profile(profile) || project.is_nil() {
        return Err(invalid());
    }
    portable_import::admit(profile, project)
}
pub fn check_portable_import_base(
    profile: &Path,
    project: Uuid,
    package: &PortableImport,
) -> Result<()> {
    portable_import::supported_package(package)?;
    check_portable_publication_base(profile, project)
}
pub struct PreparedPortableProject {
    inner: PreparedPortableImport,
}
pub struct PublishedPortableProject {
    inner: PublishedPortableImport,
}
pub struct PinnedSelectedPortableProject {
    inner: PinnedPortableImport,
}
impl std::fmt::Debug for PinnedSelectedPortableProject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PinnedSelectedPortableProject([OPAQUE])")
    }
}
pub fn open_selected_portable_project(
    profile: &Path,
    project: Uuid,
    transaction: Uuid,
    publication_fingerprint: &str,
) -> Result<PinnedSelectedPortableProject> {
    if !valid_profile(profile) || !valid_hash(publication_fingerprint) {
        return Err(invalid());
    }
    Ok(PinnedSelectedPortableProject {
        inner: PinnedPortableImport::open(profile, project, transaction, publication_fingerprint)?,
    })
}
impl PinnedSelectedPortableProject {
    pub fn root(&self) -> &ProjectRoot {
        self.inner.root()
    }
    pub fn recheck(&self) -> Result<()> {
        self.inner.recheck()
    }
}
impl std::fmt::Debug for PreparedPortableProject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PreparedPortableProject([OPAQUE])")
    }
}
impl std::fmt::Debug for PublishedPortableProject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PublishedPortableProject([OPAQUE])")
    }
}
pub fn prepare_portable_project(
    source: PortablePublicationSource,
    package: PortableImport,
    cancel: &CancellationToken,
) -> Result<PreparedPortableProject> {
    Ok(PreparedPortableProject {
        inner: PreparedPortableImport::prepare(
            &source.profile,
            source.project,
            source.operation,
            source.transaction,
            &source.reservation_row_sha256,
            package,
            cancel,
        )?,
    })
}
/// Bounded read-only observation of the exact recovery target. No rename,
/// marker repair or approval; the genuine package remains owned by Native.
pub fn check_portable_project_recovery(
    source: &PortablePublicationSource,
    package: &PortableImport,
    publication_fingerprint: &str,
    current_sql_ack: &CatalogPublicationAck,
    cancel: &CancellationToken,
) -> Result<()> {
    portable_import::observe_recovery(
        &source.profile,
        source.project,
        source.operation,
        source.transaction,
        &source.reservation_row_sha256,
        publication_fingerprint,
        package,
        current_sql_ack,
        cancel,
    )
}
/// Explicit Native recovery of CURRENT prepared/ready SQL only. The caller
/// supplies a newly authenticated package and its actual current SQL ACK;
/// neither the source metadata nor journal is an approval constructor.
pub fn recover_portable_project(
    source: PortablePublicationSource,
    package: PortableImport,
    publication_fingerprint: &str,
    current_sql_ack: &CatalogPublicationAck,
    cancel: &CancellationToken,
) -> Result<PublishedPortableProject> {
    if !valid_hash(publication_fingerprint) {
        return Err(invalid());
    }
    Ok(PublishedPortableProject {
        inner: portable_import::recover(
            &source.profile,
            source.project,
            source.operation,
            source.transaction,
            &source.reservation_row_sha256,
            publication_fingerprint,
            package,
            current_sql_ack,
            cancel,
        )?,
    })
}
impl PreparedPortableProject {
    /// Native persists THIS actual stage identity fingerprint into the SAME
    /// pending row at revision2 before calling publish. Journal is not authority.
    pub fn fingerprint(&self) -> Result<String> {
        self.inner.fingerprint()
    }
    pub fn publish(
        self,
        prepared_sql_ack: &CatalogPublicationAck,
        cancel: &CancellationToken,
    ) -> Result<PublishedPortableProject> {
        Ok(PublishedPortableProject {
            inner: self.inner.publish(prepared_sql_ack, cancel)?,
        })
    }
}
impl PublishedPortableProject {
    pub fn project_id(&self) -> Uuid {
        self.inner.project()
    }
    pub fn transaction(&self) -> Uuid {
        self.inner.transaction()
    }
    pub fn target_path(&self) -> PathBuf {
        self.inner.target_path()
    }
    pub fn candidate_digest(&self) -> &str {
        self.inner.digest()
    }
    pub fn fingerprint(&self) -> Result<String> {
        self.inner.fingerprint()
    }
    pub fn recheck(&self) -> Result<()> {
        self.inner.recheck()
    }
    /// Native supplies actual SAME-TX selected-ready row revision3 +row SHA.
    /// Recording it never selects a row; retry is idempotent only exact bytes.
    pub fn record_catalog_ack(&self, ack: &CatalogPublicationAck) -> Result<()> {
        self.inner.record_catalog_ack(ack)
    }
    pub fn record_complete(&self, ack: &CatalogPublicationAck) -> Result<()> {
        self.inner.record_complete(ack)
    }
}
