//! Private filesystem-only new managed generation. No SQL, approval minting,
//! execution, deletion, arbitrary destination or automatic restart recovery.
//! Run in a registered blocking worker; cancellation does not abort OS I/O.
use super::{ProjectEntry, ProjectRevision, ProjectRoot};
use crate::{AutomationError, DependencySourceSelection, Result, VerifiedDependencyPreview};
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
use cap_fs_ext::DirExt;
use cap_fs_ext::{FollowSymlinks, MetadataExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::Path,
    sync::{Arc, OnceLock},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub(crate) mod agent_installer_seed;
mod namespace;
pub(crate) mod portable_import;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn check_local_directory(dir: &Dir) -> Result<()> {
    namespace::check_local_directory(dir)
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn open_boundary_directory(dir: &Dir, name: &str) -> Result<Dir> {
    namespace::open_child(dir, name)
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn read_boundary_file(dir: &Dir, path: &Path) -> Result<cap_std::fs::File> {
    namespace::read_boundary_file(dir, path)
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn create_boundary_file(dir: &Dir, path: &Path, mode: u32) -> Result<cap_std::fs::File> {
    namespace::create_boundary_file(dir, path, mode)
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn boundary_metadata(dir: &Dir, path: &Path) -> Result<cap_std::fs::Metadata> {
    namespace::boundary_metadata(dir, path)
}
#[cfg(all(test, target_os = "linux"))]
#[path = "publication/linux_boundary_tests.rs"]
mod linux_boundary_tests;
#[cfg(windows)]
#[allow(unsafe_code)] // Handle-bound private publication ACL/create-only adapter.
mod private_acl;
#[cfg(test)]
mod tests;
#[cfg(windows)]
#[allow(unsafe_code)] // Narrow extracted synchronous handle-relative NT adapter.
mod windows;

#[cfg(target_os = "macos")]
pub(super) fn full_sync_file(file: &cap_std::fs::File) -> Result<()> {
    namespace::full_sync_file(file)
}
#[cfg(target_os = "macos")]
pub(super) fn create_boundary_directory(parent: &Dir, name: &str) -> Result<Dir> {
    namespace::mkdir(parent, name)
}

const INTENT: &str = "intent.json";
const CREATED: &str = "created.json";
const READY: &str = "ready.json";
const PUBLISHED: &str = "published.json";
const CATALOG_COMMITTED: &str = "catalog-committed.json";
const COMPLETE: &str = "complete.json";
const MAX_MARKER: usize = 8192;
static SLOTS: OnceLock<Arc<Semaphore>> = OnceLock::new();

fn recovery() -> AutomationError {
    AutomationError::Invalid("publication requires explicit recovery")
}
fn require(value: bool) -> Result<()> {
    if value { Ok(()) } else { Err(recovery()) }
}
fn check_cancel(cancel: &CancellationToken) -> Result<()> {
    if cancel.is_cancelled() {
        Err(AutomationError::Closed)
    } else {
        Ok(())
    }
}
fn hash(raw: &[u8]) -> String {
    hex::encode(Sha256::digest(raw))
}
fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn permit() -> Result<OwnedSemaphorePermit> {
    SLOTS
        .get_or_init(|| Arc::new(Semaphore::new(2)))
        .clone()
        .try_acquire_owned()
        .map_err(|_| AutomationError::QueueFull)
}
fn managed_only(source_kind: &str) -> Result<()> {
    if source_kind != "managed" {
        return Err(AutomationError::PublicationUnsupported(
            "only native managed new generations",
        ));
    }
    Ok(())
}

/// All serialized types are observations. None can construct a Review or
/// substitute for the independent archive/preview witness after restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "platform", rename_all = "snake_case", deny_unknown_fields)]
enum DirectoryIdentity {
    Unix { device: u64, inode: u64 },
    Windows { volume: u64, file_id: [u8; 16] },
}
fn identity(dir: &Dir) -> Result<DirectoryIdentity> {
    #[cfg(unix)]
    {
        let metadata = dir.dir_metadata()?;
        require(metadata.ino() != 0)?;
        Ok(DirectoryIdentity::Unix {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
    #[cfg(windows)]
    {
        let file = dir.try_clone()?.into_std_file();
        let (volume, file_id) = crate::project_identity_windows::stable(&file)?;
        Ok(DirectoryIdentity::Windows { volume, file_id })
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = dir;
        Err(recovery())
    }
}

pub(super) fn directory_identity_digest(dir: &Dir) -> Result<String> {
    let mut digest = Sha256::new();
    digest.update(b"consolecrypt.project-directory-freshness.v1\0");
    digest.update(
        serde_json::to_vec(&identity(dir)?)
            .map_err(|_| AutomationError::Invalid("project identity unavailable"))?,
    );
    Ok(hex::encode(digest.finalize()))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Facts {
    project: Uuid,
    operation: Uuid,
    catalog_revision: u64,
    catalog_row_sha256: String,
    base: DirectoryIdentity,
    base_digest: String,
    candidate_digest: String,
    review_digest: String,
    selection_sha256: String,
    archive_sha256: String,
    tree_digest: String,
    old_lock: Option<String>,
    old_receipt: Option<String>,
    candidate_lock: Option<String>,
    candidate_receipt: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    version: u32,
    transaction: Uuid,
    parent: DirectoryIdentity,
    facts: Facts,
}
impl Intent {
    fn stage(&self) -> String {
        format!("stage-{}", self.transaction)
    }
    fn target(&self) -> String {
        format!("generation-{}", self.transaction)
    }
    fn encoded(&self) -> Result<Vec<u8>> {
        encode(self)
    }
    fn marker(&self, stage: DirectoryIdentity) -> Result<Marker> {
        Ok(Marker {
            version: 1,
            intent_sha256: hash(&self.encoded()?),
            stage,
        })
    }
}
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    version: u32,
    intent_sha256: String,
    stage: DirectoryIdentity,
}

/// Audit DATA only. SQL exists in AppCore, not in this filesystem kernel.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogAudit {
    version: u32,
    intent_sha256: String,
    project: Uuid,
    transaction: Uuid,
    old_catalog_revision: u64,
    old_row_sha256: String,
    selected_catalog_revision: u64,
    selected_row_sha256: String,
    selected_digest: String,
    selected: DirectoryIdentity,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CatalogAuditObservation {
    FilesystemPublished,
    CatalogCommitted,
    Complete,
}
impl CatalogAudit {
    fn recorded(intent: &Intent, marker: &Marker, revision: u64, row_sha256: &str) -> Result<Self> {
        require(
            intent.facts.catalog_revision.checked_add(1) == Some(revision)
                && valid_hash(row_sha256),
        )?;
        Ok(Self {
            version: 1,
            intent_sha256: hash(&intent.encoded()?),
            project: intent.facts.project,
            transaction: intent.transaction,
            old_catalog_revision: intent.facts.catalog_revision,
            old_row_sha256: intent.facts.catalog_row_sha256.clone(),
            selected_catalog_revision: revision,
            selected_row_sha256: row_sha256.into(),
            selected_digest: intent.facts.candidate_digest.clone(),
            selected: marker.stage.clone(),
        })
    }
    fn validate(&self, intent: &Intent, marker: &Marker) -> Result<()> {
        require(
            *self
                == Self::recorded(
                    intent,
                    marker,
                    self.selected_catalog_revision,
                    &self.selected_row_sha256,
                )?,
        )
    }
}
fn exact_observation(parent: &Dir, name: &str, expected: &CatalogAudit) -> Result<()> {
    if exists(parent, name)? {
        require(parsed::<CatalogAudit>(&read_small(parent, name)?)? == *expected)
    } else {
        write_new(parent, name, expected)
    }
}
/// Historical metadata, not content freshness/SQL authority. Production writes
/// call the genuine Review recheck immediately before this observation step.
fn published_marker(namespace: &namespace::ManagedNamespace, intent: &Intent) -> Result<Marker> {
    checked_entries(namespace, intent)?;
    require(
        !exists(&namespace.root, &intent.stage())? && exists(&namespace.root, &intent.target())?,
    )?;
    let created: Marker = parsed(&read_small(&namespace.root, CREATED)?)?;
    let ready: Marker = parsed(&read_small(&namespace.root, READY)?)?;
    let published: Marker = parsed(&read_small(&namespace.root, PUBLISHED)?)?;
    require(
        created == ready && ready == published && ready == intent.marker(ready.stage.clone())?,
    )?;
    let target = namespace::open_child(&namespace.root, &intent.target())?;
    namespace::verify_private_child(&target)?;
    require(identity(&target)? == ready.stage)?;
    Ok(ready)
}
fn catalog_audit(
    namespace: &namespace::ManagedNamespace,
    intent: &Intent,
    marker: &Marker,
) -> Result<CatalogAuditObservation> {
    let committed = exists(&namespace.root, CATALOG_COMMITTED)?;
    let complete = exists(&namespace.root, COMPLETE)?;
    require(!complete || committed)?;
    if !committed {
        return Ok(CatalogAuditObservation::FilesystemPublished);
    }
    let audit: CatalogAudit = parsed(&read_small(&namespace.root, CATALOG_COMMITTED)?)?;
    audit.validate(intent, marker)?;
    if complete {
        require(parsed::<CatalogAudit>(&read_small(&namespace.root, COMPLETE)?)? == audit)?;
        Ok(CatalogAuditObservation::Complete)
    } else {
        Ok(CatalogAuditObservation::CatalogCommitted)
    }
}

fn audit_fingerprint(intent: &Intent, marker: &Marker) -> Result<String> {
    Ok(hash(
        &serde_json::to_vec(&serde_json::json!({
            "version":1,"intent_sha256":hash(&intent.encoded()?),
            "generation_marker_sha256":hash(&encode(marker)?),
        }))
        .map_err(|_| recovery())?,
    ))
}
fn record_catalog_ack(
    namespace: &namespace::ManagedNamespace,
    intent: &Intent,
    revision: u64,
    row_sha256: &str,
) -> Result<()> {
    let marker = published_marker(namespace, intent)?;
    catalog_audit(namespace, intent, &marker)?;
    let expected = CatalogAudit::recorded(intent, &marker, revision, row_sha256)?;
    exact_observation(&namespace.root, CATALOG_COMMITTED, &expected)?;
    catalog_audit(namespace, intent, &marker)?;
    namespace.check()
}
fn record_complete(
    namespace: &namespace::ManagedNamespace,
    intent: &Intent,
    revision: u64,
    row_sha256: &str,
) -> Result<()> {
    let marker = published_marker(namespace, intent)?;
    require(matches!(
        catalog_audit(namespace, intent, &marker)?,
        CatalogAuditObservation::CatalogCommitted | CatalogAuditObservation::Complete
    ))?;
    let expected = CatalogAudit::recorded(intent, &marker, revision, row_sha256)?;
    require(parsed::<CatalogAudit>(&read_small(&namespace.root, CATALOG_COMMITTED)?)? == expected)?;
    exact_observation(&namespace.root, COMPLETE, &expected)?;
    catalog_audit(namespace, intent, &marker)?;
    namespace.check()
}
/// Already-selected Native SQL metadata. No archive/publication/approval type
/// can be constructed from this view or from its read-only result.
pub(crate) struct SelectedCatalogAuditContext<'a> {
    pub profile: &'a Path,
    pub project: Uuid,
    pub original_operation: Uuid,
    pub transaction: Uuid,
    pub old_revision: u64,
    pub old_row_sha256: &'a str,
    pub selected_revision: u64,
    pub selected_row_sha256: &'a str,
    pub installed_digest: &'a str,
    pub publication_fingerprint: &'a str,
}
/// Metadata-only selected-root pin. This cannot publish, acknowledge SQL or
/// reconstruct an archive witness. Contents may be legitimately edited.
pub(crate) struct PinnedSelectedGeneration {
    namespace: namespace::ManagedNamespace,
    original: ProjectRoot,
    target: ProjectRoot,
    intent: Intent,
    marker: Marker,
}
impl PinnedSelectedGeneration {
    pub(crate) fn root(&self) -> &ProjectRoot {
        &self.target
    }
    pub(crate) fn recheck(&self) -> Result<()> {
        self.namespace.check_named_source(&self.original)?;
        require(identity(&self.original.root)? == self.intent.facts.base)?;
        let marker = published_marker(&self.namespace, &self.intent)?;
        namespace::verify_private_child(&self.target.root)?;
        require(marker == self.marker && identity(&self.target.root)? == self.marker.stage)?;
        catalog_audit(&self.namespace, &self.intent, &marker)?;
        Ok(())
    }
}
pub(crate) fn open_selected_generation(
    profile: &Path,
    project: Uuid,
    transaction: Uuid,
    fingerprint: &str,
) -> Result<PinnedSelectedGeneration> {
    require(
        profile.is_absolute()
            && !project.is_nil()
            && !transaction.is_nil()
            && valid_hash(fingerprint),
    )?;
    let original_path = profile
        .join("automation")
        .join("projects")
        .join(project.to_string());
    let target_path = profile
        .join("automation")
        .join("project-revisions")
        .join(project.to_string())
        .join(format!("generation-{transaction}"));
    let original = ProjectRoot::open(&original_path)?;
    let target = ProjectRoot::open(&target_path)?;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let target = target.publication_root()?;
    let namespace = namespace::ManagedNamespace::existing(profile, project, &original, &target)?;
    let intent: Intent = parsed(&read_small(&namespace.root, INTENT)?)?;
    require(
        intent.version == 1
            && intent.transaction == transaction
            && intent.facts.project == project
            && intent.parent == identity(&namespace.root)?
            && intent.facts.base == identity(&original.root)?,
    )?;
    let marker = published_marker(&namespace, &intent)?;
    require(
        identity(&target.root)? == marker.stage
            && audit_fingerprint(&intent, &marker)? == fingerprint,
    )?;
    let pinned = PinnedSelectedGeneration {
        namespace,
        original,
        target,
        intent,
        marker,
    };
    pinned.recheck()?;
    Ok(pinned)
}
pub(crate) fn reconcile_selected_catalog(
    context: SelectedCatalogAuditContext<'_>,
) -> Result<CatalogAuditObservation> {
    let _permit = permit()?;
    require(
        !context.project.is_nil()
            && !context.original_operation.is_nil()
            && !context.transaction.is_nil()
            && context.old_revision > 0
            && context.old_revision.checked_add(1) == Some(context.selected_revision)
            && valid_hash(context.old_row_sha256)
            && valid_hash(context.selected_row_sha256)
            && valid_hash(context.installed_digest)
            && valid_hash(context.publication_fingerprint),
    )?;
    // Paths derive ONLY from original Native profile, project and transaction.
    // Opening caps performs no source-byte read before full namespace proof.
    let original_path = context
        .profile
        .join("automation/projects")
        .join(context.project.to_string());
    let target_path = context
        .profile
        .join("automation/project-revisions")
        .join(context.project.to_string())
        .join(format!("generation-{}", context.transaction));
    let original = ProjectRoot::open(&original_path)?;
    let target = ProjectRoot::open(&target_path)?;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let target = target.publication_root()?;
    let namespace = namespace::ManagedNamespace::existing(
        context.profile,
        context.project,
        &original,
        &target,
    )?;
    let intent: Intent = parsed(&read_small(&namespace.root, INTENT)?)?;
    require(
        intent.version == 1
            && intent.transaction == context.transaction
            && intent.parent == identity(&namespace.root)?
            && intent.facts.project == context.project
            && intent.facts.operation == context.original_operation
            && intent.facts.catalog_revision == context.old_revision
            && intent.facts.catalog_row_sha256 == context.old_row_sha256
            && intent.facts.base == identity(&original.root)?
            && intent.facts.candidate_digest == context.installed_digest
            && intent.facts.old_receipt.is_none()
            && intent
                .facts
                .candidate_lock
                .as_deref()
                .is_some_and(valid_hash)
            && intent
                .facts
                .candidate_receipt
                .as_deref()
                .is_some_and(valid_hash),
    )?;
    namespace::verify_private_child(&target.root)?;
    let before = original.revision_for_publication()?;
    let selected = target.revision_for_publication()?;
    let file_hash = |r: &ProjectRevision, path: &str| {
        r.entries.iter().find_map(|entry| match entry {
            ProjectEntry::File {
                path: p, sha256, ..
            } if p == path => Some(sha256.clone()),
            _ => None,
        })
    };
    require(
        before.digest == intent.facts.base_digest
            && selected.digest == context.installed_digest
            && file_hash(&before, ".consolecrypt-dependencies.json").as_deref()
                == intent.facts.old_lock.as_deref()
            && file_hash(&before, ".consolecrypt-dependency-receipts.json").as_deref()
                == intent.facts.old_receipt.as_deref()
            && file_hash(&selected, ".consolecrypt-dependencies.json").as_deref()
                == intent.facts.candidate_lock.as_deref()
            && file_hash(&selected, ".consolecrypt-dependency-receipts.json").as_deref()
                == intent.facts.candidate_receipt.as_deref(),
    )?;
    let (marker, has_target) = checked_ready(&namespace, &intent, &selected)?;
    require(
        has_target
            && identity(&target.root)? == marker.stage
            && exists(&namespace.root, PUBLISHED)?
            && audit_fingerprint(&intent, &marker)? == context.publication_fingerprint,
    )?;
    // No copy/rename/create namespace, no acceptance/witness reconstruction.
    record_catalog_ack(
        &namespace,
        &intent,
        context.selected_revision,
        context.selected_row_sha256,
    )?;
    record_complete(
        &namespace,
        &intent,
        context.selected_revision,
        context.selected_row_sha256,
    )?;
    namespace.check_named_source(&original)?;
    require(
        original.revision_for_publication()? == before
            && target.revision_for_publication()? == selected,
    )?;
    catalog_audit(&namespace, &intent, &marker)
}

fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    let raw = serde_json::to_vec(value).map_err(|_| recovery())?;
    require(raw.len() <= MAX_MARKER)?;
    Ok(raw)
}
fn parsed<T: serde::de::DeserializeOwned>(raw: &[u8]) -> Result<T> {
    require(raw.len() <= MAX_MARKER)?;
    serde_json::from_value(crate::json::parse(raw)?).map_err(|_| recovery())
}
fn write_new(parent: &Dir, name: &str, value: &impl Serialize) -> Result<()> {
    let raw = encode(value)?;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let mut output = create_boundary_file(parent, Path::new(name), 0o600)?;
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let mut output = {
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        #[cfg(windows)]
        {
            use cap_std::fs::OpenOptionsExt;
            options
                .custom_flags(windows::WRITE_THROUGH)
                .share_mode(windows::SHARE_READ_WRITE);
        }
        parent.open_with(name, &options)?
    };
    let metadata = output.metadata()?;
    ordinary_file(&metadata)?;
    require(metadata.is_file() && !metadata.file_type().is_symlink() && metadata.nlink() == 1)?;
    output.write_all(&raw)?;
    #[cfg(target_os = "macos")]
    full_sync_file(&output)?;
    #[cfg(not(target_os = "macos"))]
    output.sync_all()?;
    sync_directory(parent)
}
fn read_small(parent: &Dir, name: &str) -> Result<Vec<u8>> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let file = read_boundary_file(parent, Path::new(name))?;
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let file = {
        let metadata = parent.symlink_metadata(name)?;
        ordinary_file(&metadata)?;
        require(
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.nlink() == 1
                && metadata.len() <= MAX_MARKER as u64,
        )?;
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        parent.open_with(name, &options)?
    };
    let opened = file.metadata()?;
    ordinary_file(&opened)?;
    require(opened.is_file() && opened.nlink() == 1 && opened.len() <= MAX_MARKER as u64)?;
    let mut raw = Vec::new();
    file.take(MAX_MARKER as u64 + 1).read_to_end(&mut raw)?;
    require(raw.len() <= MAX_MARKER)?;
    Ok(raw)
}
fn ordinary_file(metadata: &cap_std::fs::Metadata) -> Result<()> {
    require(metadata.is_file() && !metadata.file_type().is_symlink() && metadata.nlink() == 1)?;
    #[cfg(windows)]
    require(cap_std::fs::MetadataExt::file_attributes(metadata) & 0x400 == 0)?;
    Ok(())
}
fn exists(parent: &Dir, name: &str) -> Result<bool> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let metadata = boundary_metadata(parent, Path::new(name));
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let metadata = parent.symlink_metadata(name).map_err(AutomationError::from);
    match metadata {
        Ok(_) => Ok(true),
        Err(AutomationError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}
fn sync_directory(dir: &Dir) -> Result<()> {
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No).custom_flags(
            (rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::NOFOLLOW).bits() as i32,
        );
        dir.open_with(".", &options)?.sync_all()?;
    }
    // Windows file flush + process-crash recovery, NOT namespace fsync or a
    // power-loss guarantee. Do not issue directory FlushFileBuffers fiction.
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}
fn sync_tree(root: &ProjectRoot, expected: &ProjectRevision) -> Result<()> {
    for entry in expected.entries.iter().rev() {
        if let ProjectEntry::Directory { path } = entry {
            let mut dir = root.root.try_clone()?;
            for component in path.split('/') {
                dir = namespace::open_child(&dir, component)?;
            }
            sync_directory(&dir)?;
        }
    }
    sync_directory(&root.root)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Intent = 0,
    Created = 1,
    Ready = 2,
    Renamed = 3,
    Published = 4,
    BeforeRename = 5,
    Frozen = 6,
}

/// Native current catalog guards remain outside this Core filesystem layer.
/// Not Deserialize/Clone: original roots and the actual archive witness stay
/// borrowed through publication and subsequent SQL/recovery handoff.
struct Review<'a> {
    original: &'a ProjectRoot,
    before: &'a ProjectRevision,
    selected: &'a DependencySourceSelection,
    candidate: &'a ProjectRoot,
    preview: &'a VerifiedDependencyPreview,
}
impl Review<'_> {
    fn initial_check(&self) -> Result<()> {
        self.initial_check_after_walks(|| Ok(()))
    }
    fn initial_check_after_walks(&self, after_walks: impl FnOnce() -> Result<()>) -> Result<()> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let strict_original = self.original.publication_root()?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let strict_candidate = self.candidate.publication_root()?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let (original, candidate) = (&strict_original, &strict_candidate);
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let (original, candidate) = (self.original, self.candidate);
        // Use the SAME constrained roots for initial walks and the actual
        // preview's parser/walker reads. A transient preflight clone is not enough.
        require(
            original.revision_for_publication()? == *self.before
                && candidate.revision_for_publication()? == *self.preview.candidate(),
        )?;
        supported_tree(self.preview.candidate())?;
        after_walks()?;
        self.preview
            .revalidate(original, self.before, self.selected, candidate)
    }
    fn check(&self, namespace: &namespace::ManagedNamespace) -> Result<()> {
        namespace.check_named_source(self.original)?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let strict_original = self.original.publication_root()?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let strict_candidate = self.candidate.publication_root()?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let (original, candidate) = (&strict_original, &strict_candidate);
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let (original, candidate) = (self.original, self.candidate);
        if original.revision_for_publication()? != *self.before
            || candidate.revision_for_publication()? != *self.preview.candidate()
        {
            return Err(AutomationError::FileChanged);
        }
        self.preview
            .revalidate(original, self.before, self.selected, candidate)?;
        namespace.check_named_source(self.original)
    }
    fn facts(
        &self,
        project: Uuid,
        operation: Uuid,
        catalog_revision: u64,
        catalog_row_sha256: &str,
    ) -> Result<Facts> {
        require(
            !project.is_nil()
                && !operation.is_nil()
                && catalog_revision > 0
                && valid_hash(catalog_row_sha256),
        )?;
        let file_hash = |revision: &ProjectRevision, path: &str| -> Option<String> {
            revision.entries.iter().find_map(|entry| match entry {
                ProjectEntry::File {
                    path: p, sha256, ..
                } if p == path => Some(sha256.clone()),
                _ => None,
            })
        };
        Ok(Facts {
            project,
            operation,
            catalog_revision,
            catalog_row_sha256: catalog_row_sha256.into(),
            base: identity(&self.original.root)?,
            base_digest: self.before.digest.clone(),
            candidate_digest: self.preview.candidate().digest.clone(),
            review_digest: self.preview.review_digest().into(),
            selection_sha256: hash(&serde_json::to_vec(self.selected).map_err(|_| recovery())?),
            archive_sha256: self.selected.source().sha256.clone(),
            tree_digest: self.preview.archive().archive().tree().digest.clone(),
            old_lock: file_hash(self.before, ".consolecrypt-dependencies.json"),
            old_receipt: file_hash(self.before, ".consolecrypt-dependency-receipts.json"),
            candidate_lock: file_hash(self.preview.candidate(), ".consolecrypt-dependencies.json"),
            candidate_receipt: file_hash(
                self.preview.candidate(),
                ".consolecrypt-dependency-receipts.json",
            ),
        })
    }
}

/// Purely filesystem state. Its private prepare kernel accepts a recheck
/// closure only so unit tests exercise OS/journal boundaries using real trees;
/// production construction is exclusively through the actual Review above.
struct Prepared {
    namespace: namespace::ManagedNamespace,
    intent: Intent,
    marker: Marker,
    _permit: OwnedSemaphorePermit,
}
enum Resumed {
    Ready(Prepared),
    Published(Published),
}
pub(crate) struct PublicationStage<'a> {
    prepared: Prepared,
    review: Review<'a>,
}
pub(crate) struct FilesystemPublished {
    published: Published,
}
struct Published {
    namespace: namespace::ManagedNamespace,
    intent: Intent,
    target: ProjectRoot,
    marker: Marker,
    _permit: OwnedSemaphorePermit,
}
pub(crate) enum Recovered<'a> {
    Ready(PublicationStage<'a>),
    Published(FilesystemPublished),
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RecoveryObservation {
    IntentOnly,
    Partial,
    Ready,
    Published,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PublicationDurability {
    LinuxDirectoryFsync,
    MacosFileFullSyncDirectoryFsync,
    WindowsFileFlushProcessRecovery,
}

impl<'a> PublicationStage<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare(
        profile: &Path,
        source_kind: &str,
        project: Uuid,
        operation: Uuid,
        catalog_revision: u64,
        catalog_row_sha256: &str,
        original: &'a ProjectRoot,
        before: &'a ProjectRevision,
        selected: &'a DependencySourceSelection,
        candidate: &'a ProjectRoot,
        preview: &'a VerifiedDependencyPreview,
        cancel: &CancellationToken,
    ) -> Result<Self> {
        // Native must pass the captured CURRENT row's kind; this string is
        // not a caller-asserted authority field and no public API accepts it.
        managed_only(source_kind)?;
        let held = permit()?;
        check_cancel(cancel)?;
        let review = Review {
            original,
            before,
            selected,
            candidate,
            preview,
        };
        let facts = review.facts(project, operation, catalog_revision, catalog_row_sha256)?;
        // Strict initial checks and actual preview I/O before ANY mkdir.
        review.initial_check()?;
        let namespace = namespace::ManagedNamespace::open(profile, project, original, candidate)?;
        let prepared = Prepared::prepare(
            namespace,
            candidate,
            preview.candidate(),
            facts,
            held,
            cancel,
            |n| review.check(n),
            |_| Ok(()),
        )?;
        Ok(Self { prepared, review })
    }
    pub(crate) fn publish_new(
        self,
        expected_review: &str,
        cancel: &CancellationToken,
    ) -> Result<FilesystemPublished> {
        require(expected_review == self.review.preview.review_digest())?;
        let Self { prepared, review } = self;
        let published = prepared.publish(cancel, |n| review.check(n), |_| Ok(()))?;
        Ok(FilesystemPublished { published })
    }
    /// Restart requires a NEW actual verified preview and Native explicit
    /// acceptance. transaction is only a bounded observation, never authority.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn recover(
        profile: &Path,
        source_kind: &str,
        project: Uuid,
        original_operation: Uuid,
        transaction: Uuid,
        catalog_revision: u64,
        catalog_row_sha256: &str,
        original: &'a ProjectRoot,
        before: &'a ProjectRevision,
        selected: &'a DependencySourceSelection,
        candidate: &'a ProjectRoot,
        preview: &'a VerifiedDependencyPreview,
        cancel: &CancellationToken,
    ) -> Result<Recovered<'a>> {
        managed_only(source_kind)?;
        let held = permit()?;
        check_cancel(cancel)?;
        let review = Review {
            original,
            before,
            selected,
            candidate,
            preview,
        };
        // Original journal operation is provenance DATA. A fresh accepted
        // Native owner/TTL/cancellation scope must authorize this recovery;
        // reconstructing an old operation ID cannot resurrect that authority.
        let facts = review.facts(
            project,
            original_operation,
            catalog_revision,
            catalog_row_sha256,
        )?;
        // Open-existing only during recovery: no missing namespace recreation.
        let namespace =
            namespace::ManagedNamespace::existing(profile, project, original, candidate)?;
        match Prepared::resume(
            namespace,
            preview.candidate(),
            facts,
            transaction,
            held,
            cancel,
            |n| review.check(n),
        )? {
            Resumed::Ready(prepared) => Ok(Recovered::Ready(Self { prepared, review })),
            Resumed::Published(published) => {
                Ok(Recovered::Published(FilesystemPublished { published }))
            }
        }
    }
}
impl FilesystemPublished {
    pub(crate) fn transaction(&self) -> Uuid {
        self.published.intent.transaction
    }
    pub(crate) fn target_name(&self) -> String {
        self.published.intent.target()
    }
    pub(crate) fn candidate_digest(&self) -> &str {
        &self.published.intent.facts.candidate_digest
    }
    pub(crate) fn durability(&self) -> PublicationDurability {
        #[cfg(windows)]
        {
            PublicationDurability::WindowsFileFlushProcessRecovery
        }
        #[cfg(target_os = "macos")]
        {
            PublicationDurability::MacosFileFullSyncDirectoryFsync
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            PublicationDurability::LinuxDirectoryFsync
        }
    }
    /// Audit data AFTER Native's actual SQL acknowledgement, never selection.
    pub(crate) fn record_catalog_ack(&mut self, revision: u64, row_sha256: &str) -> Result<()> {
        self.captured_marker()?;
        record_catalog_ack(
            &self.published.namespace,
            &self.published.intent,
            revision,
            row_sha256,
        )
    }
    pub(crate) fn record_complete(&mut self, revision: u64, row_sha256: &str) -> Result<()> {
        self.captured_marker()?;
        record_complete(
            &self.published.namespace,
            &self.published.intent,
            revision,
            row_sha256,
        )
    }
    fn captured_marker(&self) -> Result<Marker> {
        let marker = published_marker(&self.published.namespace, &self.published.intent)?;
        namespace::verify_private_child(&self.published.target.root)?;
        require(
            marker == self.published.marker
                && identity(&self.published.target.root)? == marker.stage,
        )?;
        Ok(marker)
    }
    pub(crate) fn audit_fingerprint(&self) -> Result<String> {
        let marker = self.captured_marker()?;
        audit_fingerprint(&self.published.intent, &marker)
    }
    pub(crate) fn catalog_audit_observation(&self) -> Result<CatalogAuditObservation> {
        let marker = self.captured_marker()?;
        catalog_audit(&self.published.namespace, &self.published.intent, &marker)
    }
    /// The existing Native Accepted owner retains the ORIGINAL roots/witness
    /// while this owned namespace/permit crosses the async SQL boundary. This
    /// value alone cannot recreate them or authorize a catalog selection.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn recheck(
        &mut self,
        source_kind: &str,
        catalog_revision: u64,
        catalog_row_sha256: &str,
        original: &ProjectRoot,
        before: &ProjectRevision,
        selected: &DependencySourceSelection,
        candidate: &ProjectRoot,
        preview: &VerifiedDependencyPreview,
    ) -> Result<()> {
        managed_only(source_kind)?;
        let review = Review {
            original,
            before,
            selected,
            candidate,
            preview,
        };
        let facts = review.facts(
            self.published.intent.facts.project,
            self.published.intent.facts.operation,
            catalog_revision,
            catalog_row_sha256,
        )?;
        require(facts == self.published.intent.facts)?;
        review.check(&self.published.namespace)?;
        let (ready, target) = checked_ready(
            &self.published.namespace,
            &self.published.intent,
            preview.candidate(),
        )?;
        require(
            target
                && ready == self.published.marker
                && identity(&self.published.target.root)? == self.published.marker.stage
                && self.published.target.revision_for_publication()? == *preview.candidate(),
        )
    }
}
impl Prepared {
    #[allow(clippy::too_many_arguments)]
    fn resume(
        namespace: namespace::ManagedNamespace,
        expected: &ProjectRevision,
        facts: Facts,
        transaction: Uuid,
        held: OwnedSemaphorePermit,
        cancel: &CancellationToken,
        mut check: impl FnMut(&namespace::ManagedNamespace) -> Result<()>,
    ) -> Result<Resumed> {
        check_cancel(cancel)?;
        supported_tree(expected)?;
        check(&namespace)?;
        let intent: Intent = parsed(&read_small(&namespace.root, INTENT)?)?;
        require(
            intent.version == 1
                && !transaction.is_nil()
                && intent.transaction == transaction
                && intent.parent == identity(&namespace.root)?
                && intent.facts == facts,
        )?;
        require(matches!(
            observe(&namespace, &intent, expected)?,
            RecoveryObservation::Ready | RecoveryObservation::Published
        ))?;
        let (marker, target) = checked_ready(&namespace, &intent, expected)?;
        check(&namespace)?;
        check_cancel(cancel)?;
        if target {
            // No archive witness is recreated here; the production caller has
            // already supplied a NEW actual Review. Tests supply FS checks only.
            if !exists(&namespace.root, PUBLISHED)? {
                write_new(&namespace.root, PUBLISHED, &marker)?;
            }
            namespace.check()?;
            check(&namespace)?;
            check_cancel(cancel)?;
            let target_root = ProjectRoot {
                root: namespace::open_child(&namespace.root, &intent.target())?,
                #[cfg(any(target_os = "linux", target_os = "macos"))]
                publication_boundary: true,
            };
            require(identity(&target_root.root)? == marker.stage)?;
            Ok(Resumed::Published(Published {
                namespace,
                intent,
                target: target_root,
                marker,
                _permit: held,
            }))
        } else {
            Ok(Resumed::Ready(Self {
                namespace,
                intent,
                marker,
                _permit: held,
            }))
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn prepare(
        namespace: namespace::ManagedNamespace,
        candidate: &ProjectRoot,
        expected: &ProjectRevision,
        facts: Facts,
        held: OwnedSemaphorePermit,
        cancel: &CancellationToken,
        mut check: impl FnMut(&namespace::ManagedNamespace) -> Result<()>,
        mut hook: impl FnMut(Phase) -> Result<()>,
    ) -> Result<Self> {
        namespace.check()?;
        check(&namespace)?;
        supported_tree(expected)?;
        require(expected.digest == facts.candidate_digest)?;
        // ANY entry blocks; do not scan arbitrary junk to find an old UUID.
        require(namespace.root.entries()?.next().transpose()?.is_none())?;
        check_cancel(cancel)?;
        let intent = Intent {
            version: 1,
            transaction: Uuid::new_v4(),
            parent: identity(&namespace.root)?,
            facts,
        };
        // The SAME create_new name serializes competing first installs, unlike
        // different UUID intent files which can both pass an empty-dir check.
        write_new(&namespace.root, INTENT, &intent)?;
        hook(Phase::Intent)?;
        check_cancel(cancel)?;
        namespace.check()?;
        let created_stage = namespace::mkdir(&namespace.root, &intent.stage())?;
        sync_directory(&namespace.root)?;
        let staging = ProjectRoot {
            root: created_stage,
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            publication_boundary: true,
        };
        let marker = intent.marker(identity(&staging.root)?)?;
        write_new(&namespace.root, CREATED, &marker)?;
        hook(Phase::Created)?;
        check(&namespace)?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let strict_candidate = candidate.publication_root()?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let candidate = &strict_candidate;
        candidate.freeze_to(expected, &staging, cancel)?;
        hook(Phase::Frozen)?;
        require(
            candidate.revision_for_publication()? == *expected
                && staging.revision_for_publication()? == *expected,
        )?;
        check(&namespace)?;
        sync_tree(&staging, expected)?;
        check_cancel(cancel)?;
        namespace.check()?;
        write_new(&namespace.root, READY, &marker)?;
        // Only our NEW copying handle is dropped. Original/ancestors stay held.
        drop(staging);
        hook(Phase::Ready)?;
        Ok(Self {
            namespace,
            intent,
            marker,
            _permit: held,
        })
    }
    fn publish(
        self,
        cancel: &CancellationToken,
        mut check: impl FnMut(&namespace::ManagedNamespace) -> Result<()>,
        mut hook: impl FnMut(Phase) -> Result<()>,
    ) -> Result<Published> {
        self.namespace.check()?;
        check(&self.namespace)?;
        // Existing walker is used again before rename; Ready bytes are not trust.
        let expected = ProjectRoot {
            root: namespace::open_child(&self.namespace.root, &self.intent.stage())?,
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            publication_boundary: true,
        }
        .revision_for_publication()?;
        require(expected.digest == self.intent.facts.candidate_digest)?;
        supported_tree(&expected)?;
        let (marker, target) = checked_ready(&self.namespace, &self.intent, &expected)?;
        require(!target && marker == self.marker)?;
        hook(Phase::BeforeRename)?;
        self.namespace.check()?;
        check(&self.namespace)?;
        check_cancel(cancel)?;
        rename_new(
            &self.namespace.root,
            &self.intent.stage(),
            &self.intent.target(),
            &self.marker.stage,
        )?;
        // No cancellation return between commit and observation/journaling.
        sync_directory(&self.namespace.root)?;
        hook(Phase::Renamed)?;
        let (_, target) = checked_ready(&self.namespace, &self.intent, &expected)?;
        require(target)?;
        write_new(&self.namespace.root, PUBLISHED, &self.marker)?;
        hook(Phase::Published)?;
        self.namespace.check()?;
        check(&self.namespace)?;
        // Cancelled after rename leaves a retained target for explicit review;
        // the caller must NOT infer catalog installation or roll it back.
        check_cancel(cancel)?;
        let target_root = ProjectRoot {
            root: namespace::open_child(&self.namespace.root, &self.intent.target())?,
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            publication_boundary: true,
        };
        require(identity(&target_root.root)? == self.marker.stage)?;
        Ok(Published {
            namespace: self.namespace,
            intent: self.intent,
            target: target_root,
            marker: self.marker,
            _permit: self._permit,
        })
    }
}

fn supported_tree(expected: &ProjectRevision) -> Result<()> {
    require(
        expected.version == 1
            && expected.entries.len() <= 20_000
            && expected.bytes <= 100 * 1024 * 1024,
    )?;
    let mut portable = std::collections::BTreeSet::new();
    for entry in &expected.entries {
        // Reuse the archive's existing portable-name rule, not a second path
        // parser or Unicode/case normalization algorithm. Base files must obey
        // the same narrow publication subset as the reviewed dependency.
        if crate::dependency_archive::portable_path(entry.path()).is_err()
            || !portable.insert(entry.path().to_ascii_lowercase())
        {
            return Err(AutomationError::PublicationUnsupported(
                "nonportable publication names",
            ));
        }
    }
    #[cfg(windows)]
    {
        if expected
            .entries
            .iter()
            .any(|e| matches!(e, ProjectEntry::File { mode: 0o700, .. }))
        {
            return Err(AutomationError::PublicationUnsupported(
                "Windows executable publication metadata",
            ));
        }
    }
    let _ = expected;
    Ok(())
}
fn checked_entries(namespace: &namespace::ManagedNamespace, intent: &Intent) -> Result<()> {
    namespace.check()?;
    let stage = intent.stage();
    let target = intent.target();
    let allowed = [
        INTENT,
        CREATED,
        READY,
        PUBLISHED,
        CATALOG_COMMITTED,
        COMPLETE,
        stage.as_str(),
        target.as_str(),
    ];
    for (index, entry) in namespace.root.entries()?.enumerate() {
        require(index < allowed.len())?;
        let name = entry?.file_name();
        require(allowed.iter().any(|n| name == *n))?;
    }
    let current: Intent = parsed(&read_small(&namespace.root, INTENT)?)?;
    require(current == *intent)
}
/// Read-only data classification. Partial/unknown copies are never repaired or
/// deleted. Even Ready/Published cannot produce a typed archive/accept witness.
fn observe(
    namespace: &namespace::ManagedNamespace,
    intent: &Intent,
    expected: &ProjectRevision,
) -> Result<RecoveryObservation> {
    checked_entries(namespace, intent)?;
    if exists(&namespace.root, READY)? {
        let (_, target) = checked_ready(namespace, intent, expected)?;
        return Ok(if target {
            RecoveryObservation::Published
        } else {
            RecoveryObservation::Ready
        });
    }
    require(
        !exists(&namespace.root, PUBLISHED)?
            && !exists(&namespace.root, &intent.target())?
            && !exists(&namespace.root, CATALOG_COMMITTED)?
            && !exists(&namespace.root, COMPLETE)?,
    )?;
    let stage = exists(&namespace.root, &intent.stage())?;
    if exists(&namespace.root, CREATED)? {
        let marker: Marker = parsed(&read_small(&namespace.root, CREATED)?)?;
        require(stage && marker == intent.marker(marker.stage.clone())?)?;
        let opened = namespace::open_child(&namespace.root, &intent.stage())?;
        require(identity(&opened)? == marker.stage)?;
        return Ok(RecoveryObservation::Partial);
    }
    Ok(if stage {
        RecoveryObservation::Partial
    } else {
        RecoveryObservation::IntentOnly
    })
}
fn checked_ready(
    namespace: &namespace::ManagedNamespace,
    intent: &Intent,
    expected: &ProjectRevision,
) -> Result<(Marker, bool)> {
    // Counts ALL entries, bounded nine reads (ninth denies immediately).
    checked_entries(namespace, intent)?;
    let stage = intent.stage();
    let target = intent.target();
    let created: Marker = parsed(&read_small(&namespace.root, CREATED)?)?;
    let ready: Marker = parsed(&read_small(&namespace.root, READY)?)?;
    require(created == ready && ready == intent.marker(ready.stage.clone())?)?;
    let has_stage = exists(&namespace.root, &stage)?;
    let has_target = exists(&namespace.root, &target)?;
    require(has_stage != has_target)?;
    if exists(&namespace.root, PUBLISHED)? {
        let published: Marker = parsed(&read_small(&namespace.root, PUBLISHED)?)?;
        require(has_target && published == ready)?;
    }
    let root = ProjectRoot {
        root: namespace::open_child(&namespace.root, if has_target { &target } else { &stage })?,
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        publication_boundary: true,
    };
    namespace::verify_private_child(&root.root)?;
    require(
        identity(&root.root)? == ready.stage
            && expected.digest == intent.facts.candidate_digest
            && root.revision_for_publication()? == *expected,
    )?;
    if exists(&namespace.root, CATALOG_COMMITTED)? || exists(&namespace.root, COMPLETE)? {
        require(has_target && exists(&namespace.root, PUBLISHED)?)?;
        catalog_audit(namespace, intent, &ready)?;
    }
    Ok((ready, has_target))
}
fn rename_new(parent: &Dir, stage: &str, target: &str, expected: &DirectoryIdentity) -> Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let opened = namespace::open_child(parent, stage)?;
        require(identity(&opened)? == *expected)?;
        // Linux renameat2 / Darwin renameatx_np bind names, not a source inode. Exact
        // post-rename identity detects a same-UID retarget; it never overwrites
        // a target and never deletes either tree on error. See threat limits.
        rustix::fs::renameat_with(
            parent,
            stage,
            parent,
            target,
            rustix::fs::RenameFlags::NOREPLACE,
        )
        .map_err(std::io::Error::from)?;
        Ok(())
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsHandle;
        let source = windows::open_stage(parent, stage, expected)?;
        windows::rename_new(&source, parent.as_handle(), target)?;
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (parent, stage, target, expected);
        Err(AutomationError::PublicationUnsupported(
            "unsupported publication platform",
        ))
    }
}

#[cfg(all(test, windows))]
pub(crate) fn protect_owned_publication_fixture(path: &Path) {
    private_acl::tests::protect_owned_fixture(path)
}

/// Test-only checkpoint on the SAME production initial-review helper. Requires
/// a genuine verified preview, never a fabricated archive or acceptance witness.
#[cfg(all(test, target_os = "linux"))]
pub(crate) fn initial_revalidation_fixture(
    original: &ProjectRoot,
    before: &ProjectRevision,
    selected: &DependencySourceSelection,
    candidate: &ProjectRoot,
    preview: &VerifiedDependencyPreview,
    after_walks: impl FnOnce() -> Result<()>,
) -> Result<()> {
    Review {
        original,
        before,
        selected,
        candidate,
        preview,
    }
    .initial_check_after_walks(after_walks)
}

#[cfg(all(test, target_os = "linux"))]
pub(crate) fn initial_bind_revalidation_fixture(
    original: &ProjectRoot,
    before: &ProjectRevision,
    selected: &DependencySourceSelection,
    candidate: &ProjectRoot,
    preview: &VerifiedDependencyPreview,
    injected: &Path,
    holding: &Path,
) -> Result<(Result<()>, bool)> {
    // Only owned, explicit test data. No fabricated preview, mount syscall,
    // approval or public integration seam is introduced by this helper.
    require(
        injected
            .to_str()
            .is_some_and(|s| s.starts_with("/tmp/owned-publication-whole-"))
            && injected
                .file_name()
                .is_some_and(|s| s == "initial-candidate-files"),
    )?;
    let source_parent = Dir::open_ambient_dir(
        injected.parent().ok_or_else(recovery)?,
        cap_std::ambient_authority(),
    )?;
    let held_parent = Dir::open_ambient_dir(holding, cap_std::ambient_authority())?;
    let (candidate_parent, leaf) = candidate.parent("roles/demo/files", false)?;
    let mut original_moved = false;
    let mut injected_moved = false;
    let mut plain_passed = false;
    let actual =
        initial_revalidation_fixture(original, before, selected, candidate, preview, || {
            // The non-mount parent containing the pre-mounted same-byte payload
            // moves atomically on the SAME owned volume AFTER the strict walks.
            candidate_parent.rename(&leaf, &held_parent, "held-original-candidate-files")?;
            original_moved = true;
            source_parent.rename("initial-candidate-files", &candidate_parent, &leaf)?;
            injected_moved = true;
            preview.revalidate(original, before, selected, candidate)?;
            plain_passed = true;
            Ok(())
        });
    // Restore even when the old-control or actual strict check failed, before
    // an owned TempDir recursively disposes its ordinary uncommitted candidate.
    if injected_moved {
        candidate_parent.rename(&leaf, &source_parent, "initial-candidate-files")?;
    }
    if original_moved {
        held_parent.rename("held-original-candidate-files", &candidate_parent, &leaf)?;
    }
    Ok((actual, plain_passed))
}

/// Read-only original managed namespace observation before any download.
pub(crate) fn check_publication_base(
    profile: &Path,
    project: Uuid,
    original: &ProjectRoot,
) -> Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let strict = original.publication_root()?;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let original = &strict;
    let namespace = namespace::ManagedNamespace::admit_base(profile, project, original)?;
    // Same existing bounded walker and excluded-entry policy; no second parser.
    original.revision_for_publication()?;
    namespace.recheck_base(original)
}
#[cfg(windows)]
pub(super) use windows::{open_project_move_file, rename_project_move_file};
