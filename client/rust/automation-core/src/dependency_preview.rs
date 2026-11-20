//! Fixed prepare + checked uncommitted candidate. No approval/commit/publication.
use crate::dependency_archive::{portable_path, revision, SourceSelection, VerifiedArchive};
use crate::{
    compare_revisions, AutomationError, DependencyHelperConnection, HelperPeer, ProjectDiff,
    ProjectEntry, ProjectRevision, ProjectRoot, Result, UploadedRevision,
    VerifiedDependencyArchive,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

const LOCK: &str = ".consolecrypt-dependencies.json";
const RECEIPTS: &str = ".consolecrypt-dependency-receipts.json";
static PREVIEW_SLOTS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
fn invalid() -> AutomationError {
    AutomationError::Protocol("invalid dependency candidate evidence")
}
fn require(ok: bool) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(invalid())
    }
}
fn sha(raw: &[u8]) -> String {
    hex::encode(Sha256::digest(raw))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectionWire {
    kind: crate::DependencyKind,
    name: String,
    version: String,
    source: crate::DependencyArtifactSource,
    strip_components: u8,
}
impl SelectionWire {
    fn from_selected(selected: &SourceSelection) -> Result<Self> {
        serde_json::from_value(serde_json::to_value(selected).map_err(|_| invalid())?)
            .map_err(|_| invalid())
    }
    fn record(&self, tree_digest: String) -> Record {
        Record {
            kind: self.kind.clone(),
            name: self.name.clone(),
            version: self.version.clone(),
            source: self.source.clone(),
            tree_digest,
        }
    }
    fn request(&self) -> Value {
        json!({"kind":self.kind,"name":self.name,"version":self.version,"source":self.source})
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Record {
    kind: crate::DependencyKind,
    name: String,
    version: String,
    source: crate::DependencyArtifactSource,
    tree_digest: String,
}
impl Record {
    fn selection(&self) -> Result<SourceSelection> {
        SourceSelection::new(
            self.kind.clone(),
            self.name.clone(),
            self.version.clone(),
            self.source.clone(),
            0,
        )
        .map_err(|_| invalid())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Lock {
    version: u32,
    dependencies: Vec<Record>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptDocument {
    version: u32,
    receipts: Vec<Value>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Prepared {
    preparation_id: String,
    review_digest: String,
    file_count: usize,
    entry_count: usize,
    bytes: u64,
    record: Record,
    provenance: Value,
}

fn read_optional(
    root: &ProjectRoot,
    snapshot: &ProjectRevision,
    path: &str,
) -> Result<Option<Vec<u8>>> {
    let matches = snapshot
        .entries
        .iter()
        .filter(|entry| entry.path().eq_ignore_ascii_case(path))
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return Ok(None);
    }
    require(matches.len() == 1 && matches[0].path() == path)?;
    let ProjectEntry::File { bytes, sha256, .. } = matches[0] else {
        return Err(invalid());
    };
    require(*bytes <= 64 * 1024)?;
    let mut raw = Vec::new();
    root.open_file(path)?
        .take(64 * 1024 + 1)
        .read_to_end(&mut raw)?;
    require(raw.len() as u64 == *bytes && sha(&raw) == *sha256)?;
    Ok(Some(raw))
}
fn subtree(snapshot: &ProjectRevision, target: &str) -> Result<ProjectRevision> {
    require(
        snapshot
            .entries
            .iter()
            .any(|entry| matches!(entry,ProjectEntry::Directory{path}if path==target)),
    )?;
    let prefix = format!("{target}/");
    let mut entries = Vec::new();
    for entry in &snapshot.entries {
        if let Some(path) = entry.path().strip_prefix(&prefix) {
            entries.push(match entry {
                ProjectEntry::Directory { .. } => ProjectEntry::Directory { path: path.into() },
                ProjectEntry::File {
                    mode,
                    bytes,
                    sha256,
                    ..
                } => ProjectEntry::File {
                    path: path.into(),
                    mode: *mode,
                    bytes: *bytes,
                    sha256: sha256.clone(),
                },
            });
        }
    }
    revision(entries).map_err(|_| invalid())
}
fn parse_lock(raw: Option<&[u8]>, snapshot: &ProjectRevision) -> Result<Vec<Record>> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let value: Lock = serde_json::from_value(crate::json::parse(raw)?).map_err(|_| invalid())?;
    require(value.version == 1 && value.dependencies.len() <= 32)?;
    let mut seen = BTreeSet::new();
    for record in &value.dependencies {
        require(cc_models::automation::is_digest(&record.tree_digest))?;
        let target = record.selection()?.target();
        require(
            seen.insert(target.clone()) && subtree(snapshot, &target)?.digest == record.tree_digest,
        )?;
    }
    Ok(value.dependencies)
}
fn receipt(selected: &SourceSelection, archive: &VerifiedArchive) -> Result<Value> {
    let wire = SelectionWire::from_selected(selected)?;
    let tree = archive.tree();
    Ok(
        json!({"version":1,"target":selected.target(),"source":selected.source(),"tree_digest":tree.digest,
        "strip_components":wire.strip_components,"files":tree.entries.iter().filter(|entry|matches!(entry,ProjectEntry::File{..})).count(),
        "bytes":tree.bytes,"kind":wire.kind,"name":wire.name,"dependency_version":wire.version,
        "verification":"helper_https_sha256_v1","origin":selected.origin()}),
    )
}
fn native_base(
    root: &ProjectRoot,
    base: &ProjectRevision,
    selected: &SourceSelection,
) -> Result<(Option<Vec<u8>>, Vec<Record>)> {
    require(root.revision()? == *base)?;
    for entry in &base.entries {
        portable_path(entry.path()).map_err(|_| invalid())?;
    }
    // The Helper still refuses existing provenance: make this explicit BEFORE
    // upload/preparation/network rather than silently replace or invent a merge.
    if read_optional(root, base, RECEIPTS)?.is_some() {
        return Err(AutomationError::Invalid(
            "existing dependency receipts require the future merge workflow",
        ));
    }
    let raw = read_optional(root, base, LOCK)?;
    let records = parse_lock(raw.as_deref(), base)?;
    require(records.len() < 32)?;
    let target = selected.target();
    require(!records.iter().any(|record| {
        record
            .selection()
            .is_ok_and(|value| value.target() == target)
    }))?;
    let parts = target.split('/').collect::<Vec<_>>();
    for index in 1..=parts.len() {
        let path = parts[..index].join("/");
        for entry in &base.entries {
            if entry.path().eq_ignore_ascii_case(&path) {
                require(
                    index < parts.len()
                        && entry.path() == path
                        && matches!(entry, ProjectEntry::Directory { .. }),
                )?;
            }
        }
    }
    Ok((raw, records))
}

/// Read-only project lock metadata; never an approval, installed-version
/// claim, credential scope, helper call or dependency publication witness.
#[derive(Clone, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectCollectionPin {
    pub name: String,
    pub version: String,
}
pub fn project_collection_pins(root: &ProjectRoot, captured: &ProjectRevision) -> Result<Vec<ProjectCollectionPin>> {
    require(root.revision()?.digest == captured.digest)?;
    let bytes = read_optional(root, captured, LOCK)?;
    let records = parse_lock(bytes.as_deref(), captured)?;
    let pins = records.into_iter().filter(|r| r.kind == crate::DependencyKind::Collection)
        .map(|r| ProjectCollectionPin { name: r.name, version: r.version }).collect();
    require(root.revision()?.digest == captured.digest)?;
    Ok(pins)
}



/// Read-only captured-base data, not source/network approval or a lease. A
/// controller may store these digests in its own fenced review ticket; prepare
/// always repeats the same checks instead of trusting an old preflight result.
#[derive(Debug)]
pub struct DependencyBasePreflight {
    base_digest: String,
    lock_sha256: Option<String>,
}
impl DependencyBasePreflight {
    pub fn base_digest(&self) -> &str {
        &self.base_digest
    }
    pub fn lock_sha256(&self) -> Option<&str> {
        self.lock_sha256.as_deref()
    }
}
pub fn dependency_preflight(
    root: &ProjectRoot,
    base: &ProjectRevision,
    selected: &SourceSelection,
) -> Result<DependencyBasePreflight> {
    let (lock, _) = native_base(root, base, selected)?;
    Ok(DependencyBasePreflight {
        base_digest: base.digest.clone(),
        lock_sha256: lock.as_deref().map(sha),
    })
}

/// Checked data and complete diff, never an approval, publish token or safe-code
/// claim. The Native caller must retain private staging and rehash/fence before
/// every review/publish use; helper/candidate digests are separate from review.
pub struct VerifiedDependencyPreview {
    archive: VerifiedDependencyArchive,
    candidate: ProjectRevision,
    diff: ProjectDiff,
    base_digest: String,
    review_digest: String,
    uploaded: UploadedRevision,
}
impl std::fmt::Debug for VerifiedDependencyPreview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifiedDependencyPreview")
            .field("base_digest", &self.base_digest)
            .field("candidate_digest", &self.candidate.digest)
            .field("review_digest", &self.review_digest)
            .finish_non_exhaustive()
    }
}
impl VerifiedDependencyPreview {
    pub fn archive(&self) -> &VerifiedDependencyArchive {
        &self.archive
    }
    pub fn candidate(&self) -> &ProjectRevision {
        &self.candidate
    }
    pub fn diff(&self) -> &ProjectDiff {
        &self.diff
    }
    pub fn base_digest(&self) -> &str {
        &self.base_digest
    }
    pub fn review_digest(&self) -> &str {
        &self.review_digest
    }
    pub fn uploaded_base(&self) -> &UploadedRevision {
        &self.uploaded
    }
    /// Read-only evidence recheck. This is not publication permission: a later
    /// consumer must recheck again at its own commit boundary.
    pub fn revalidate(
        &self,
        base: &ProjectRoot,
        before: &ProjectRevision,
        selected: &SourceSelection,
        candidate: &ProjectRoot,
    ) -> Result<()> {
        require(
            before.digest == self.base_digest && selected == self.archive.archive().selection(),
        )?;
        let (old_lock, old_records) = native_base(base, before, selected)?;
        verify_candidate(
            base,
            before,
            candidate,
            &self.candidate,
            selected,
            self.archive.archive(),
            &old_records,
        )?;
        require(
            review_digest(
                selected,
                before,
                &self.candidate,
                self.archive.archive(),
                old_lock.as_deref(),
            )? == self.review_digest,
        )
    }
}

fn review_digest(
    selected: &SourceSelection,
    base: &ProjectRevision,
    candidate: &ProjectRevision,
    archive: &VerifiedArchive,
    old_lock: Option<&[u8]>,
) -> Result<String> {
    let value = crate::project::canonical(
        json!({"version":1,"source":selected,"base_digest":base.digest,
        "candidate_digest":candidate.digest,"archive_tree_digest":archive.tree().digest,"lock_before":old_lock.map(sha)}),
    );
    Ok(sha(&serde_json::to_vec(&value).map_err(|_| invalid())?))
}
struct OperationGuard<'a> {
    peer: &'a HelperPeer,
    complete: bool,
}
impl Drop for OperationGuard<'_> {
    fn drop(&mut self) {
        if !self.complete {
            self.peer.abort_dependency_exchange();
        }
    }
}

fn verify_candidate(
    base: &ProjectRoot,
    before: &ProjectRevision,
    candidate: &ProjectRoot,
    after: &ProjectRevision,
    selected: &SourceSelection,
    archive: &VerifiedArchive,
    old_records: &[Record],
) -> Result<ProjectDiff> {
    require(base.revision()? == *before && candidate.revision()? == *after)?;
    let mut folded = BTreeSet::new();
    for entry in &after.entries {
        portable_path(entry.path()).map_err(|_| invalid())?;
        require(folded.insert(entry.path().to_ascii_lowercase()))?;
    }
    let target = selected.target();
    require(subtree(after, &target)? == *archive.tree())?;
    for contract in [LOCK, RECEIPTS] {
        require(after.entries.iter().any(
            |entry| matches!(entry,ProjectEntry::File{path,mode:0o600,..} if path==contract),
        ))?;
    }
    let lock = read_optional(candidate, after, LOCK)?.ok_or_else(invalid)?;
    let mut expected = old_records.to_vec();
    expected.push(SelectionWire::from_selected(selected)?.record(archive.tree().digest.clone()));
    require(parse_lock(Some(&lock), after)? == expected)?;
    let raw = read_optional(candidate, after, RECEIPTS)?.ok_or_else(invalid)?;
    let receipts: ReceiptDocument =
        serde_json::from_value(crate::json::parse(&raw)?).map_err(|_| invalid())?;
    require(receipts.version == 1 && receipts.receipts == vec![receipt(selected, archive)?])?;
    let old = before
        .entries
        .iter()
        .map(|entry| (entry.path(), entry))
        .collect::<BTreeMap<_, _>>();
    let new = after
        .entries
        .iter()
        .map(|entry| (entry.path(), entry))
        .collect::<BTreeMap<_, _>>();
    for (path, entry) in &old {
        if *path != LOCK {
            require(new.get(path) == Some(entry))?;
        }
    }
    for (path, entry) in &new {
        if !old.contains_key(path) {
            require(
                *path == LOCK
                    || *path == RECEIPTS
                    || *path == target
                    || path.starts_with(&format!("{target}/"))
                    || (target.starts_with(&format!("{path}/"))
                        && matches!(entry, ProjectEntry::Directory { .. })),
            )?;
        }
    }
    require(base.revision()? == *before && candidate.revision()? == *after)?;
    Ok(compare_revisions(before, after))
}

impl DependencyHelperConnection {
    /// Call only after Native's explicit exact-source/network review, from a
    /// bounded filesystem worker. `staging` is a Native-owned empty temp root,
    /// never a Helper/UI destination. This method never approves or commits.
    pub async fn prepare_preview(
        &self,
        base_root: &ProjectRoot,
        base: &ProjectRevision,
        selected: &SourceSelection,
        staging: &ProjectRoot,
        cancel: &CancellationToken,
    ) -> Result<VerifiedDependencyPreview> {
        if cancel.is_cancelled() {
            return Err(AutomationError::Closed);
        }
        let deadline = Instant::now() + Duration::from_secs(300);
        let _slot = PREVIEW_SLOTS
            .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
            .clone()
            .try_acquire_owned()
            .map_err(|_| AutomationError::QueueFull)?;
        let (old_lock, old_records) = native_base(base_root, base, selected)?;
        require(staging.revision()?.entries.is_empty())?;
        let wire = SelectionWire::from_selected(selected)?;
        if Instant::now() >= deadline {
            return Err(AutomationError::Timeout);
        }
        let peer = self.private_control_peer();
        let mut guard = OperationGuard {
            peer,
            complete: false,
        };
        let operation = async {
            let uploaded = base_root.upload_dependency(base, peer).await?;
            require(
                !uploaded.revision_id.is_nil()
                    && uploaded.file_count
                        == base
                            .entries
                            .iter()
                            .filter(|entry| matches!(entry, ProjectEntry::File { .. }))
                            .count(),
            )?;
            let body = json!({"revision_id":uploaded.revision_id,"revision_digest":base.digest,
                "lock_sha256":old_lock.as_deref().map(sha),"selection":wire.request(),
                "strip_components":wire.strip_components,"network_origin":selected.origin()});
            let reply: Prepared = serde_json::from_value(
                peer.request_dependency_bounded(
                    "dependency_prepare",
                    body,
                    Duration::from_secs(60),
                    32 * 1024,
                )
                .await?,
            )
            .map_err(|_| invalid())?;
            let id = uuid::Uuid::parse_str(&reply.preparation_id).map_err(|_| invalid())?;
            require(
                !id.is_nil()
                    && id.to_string() == reply.preparation_id
                    && cc_models::automation::is_digest(&reply.review_digest)
                    && reply.file_count <= 10_000
                    && reply.entry_count <= 20_000
                    && reply.entry_count >= reply.file_count
                    && reply.bytes <= 100 * 1024 * 1024,
            )?;
            let archive =
                crate::receive_dependency_archive(peer, selected, id, &reply.review_digest, cancel)
                    .await?;
            require(
                reply.record == wire.record(archive.archive().tree().digest.clone())
                    && reply.provenance == receipt(selected, archive.archive())?,
            )?;
            let capture = UploadedRevision {
                revision_id: id,
                digest: reply.review_digest,
                file_count: reply.file_count,
                entry_count: reply.entry_count,
                bytes: reply.bytes,
            };
            let candidate = staging
                .download_dependency_candidate(&capture, peer, cancel)
                .await?;
            let diff = verify_candidate(
                base_root,
                base,
                staging,
                &candidate,
                selected,
                archive.archive(),
                &old_records,
            )?;
            let review_digest = review_digest(
                selected,
                base,
                &candidate,
                archive.archive(),
                old_lock.as_deref(),
            )?;
            require(!peer.is_closed() && !cancel.is_cancelled())?;
            Ok(VerifiedDependencyPreview {
                archive,
                candidate,
                diff,
                base_digest: base.digest.clone(),
                review_digest,
                uploaded,
            })
        };
        let result = tokio::select! {biased;
            _=cancel.cancelled()=>return Err(AutomationError::Closed),
            result=tokio::time::timeout(deadline.saturating_duration_since(Instant::now()),operation)=>result.map_err(|_|AutomationError::Timeout)?,
        };
        // A synchronous existing filesystem call can complete after a timer's
        // poll. Never admit late evidence even when that inner future is ready.
        if Instant::now() >= deadline {
            return Err(AutomationError::Timeout);
        }
        if result.is_ok() {
            guard.complete = true;
        }
        result
    }
}

#[cfg(test)]
#[path = "dependency_preview_tests.rs"]
mod tests;

/// Reuse the existing supported lock schema/subtree checks for portable data.
/// This does not recreate archive/source verification or an approval witness.
pub(crate) fn validate_portable_lock(raw: Option<&[u8]>, snapshot: &ProjectRevision) -> Result<()> {
    parse_lock(raw, snapshot).map(|_| ())
}
