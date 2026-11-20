//! Private ANS04 source-only prerequisite. A separately encrypted FILE, never
//! a VaultObject/credential/KDBX record, job grant or dependency receipt witness.
//! No catalog, source-kind, sync, Runner, secret resolver or Bridge operation.
use crate::{AutomationError, ProjectEntry, ProjectRevision, ProjectRoot, Result};
use cc_crypto_core::{SecretString, open_portable_project, seal_portable_project};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    ops::Range,
    sync::{Arc, OnceLock},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

const MAX_BYTES: u64 = 16 * 1024 * 1024;
const MAX_FILE: u64 = 1024 * 1024;
const MAX_ENTRIES: usize = 2000;
const MAX_FILES: usize = 1000;
const MAX_MANIFEST: usize = 512 * 1024;
const LOCK: &str = ".consolecrypt-dependencies.json";
static SLOTS: OnceLock<Arc<Semaphore>> = OnceLock::new();
fn invalid() -> AutomationError {
    AutomationError::Invalid("portable project package invalid or unavailable")
}
fn require(value: bool) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err(invalid())
    }
}
fn cancel(token: &CancellationToken) -> Result<()> {
    if token.is_cancelled() {
        Err(AutomationError::Closed)
    } else {
        Ok(())
    }
}
fn sha(raw: &[u8]) -> String {
    hex::encode(Sha256::digest(raw))
}
fn permit() -> Result<OwnedSemaphorePermit> {
    SLOTS
        .get_or_init(|| Arc::new(Semaphore::new(2)))
        .clone()
        .try_acquire_owned()
        .map_err(|_| AutomationError::QueueFull)
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum CapturePolicy {
    ExplicitSnapshotSelectionV1,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ExclusionReason {
    Unselected,
    KnownControl,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Exclusion {
    path: String,
    reason: ExclusionReason,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    policy: CapturePolicy,
    /// Provenance DATA, not a signature, user permission or receipt witness.
    source_revision_digest: String,
    revision: ProjectRevision,
    playbook: Option<String>,
    exclusions: Vec<Exclusion>,
}
/// Public-to-this-crate data only. No raw plaintext or native identities.
pub(crate) struct PackageSummary {
    pub(crate) source_digest: String,
    pub(crate) portable_digest: String,
    pub(crate) files: usize,
    pub(crate) bytes: u64,
    pub(crate) exclusions: usize,
}
/// Not Clone/Deserialize/Debug. Only full AEAD+schema/tree validation constructs
/// it. The permit bounds retained plaintext as well as active decrypt/KDF work.
pub(crate) struct VerifiedPortablePackage {
    manifest: Manifest,
    // Actual authenticated input bytes, not a caller/source assertion.
    encrypted_sha256:Option<String>,
    plaintext: Zeroizing<Vec<u8>>,
    files: Vec<(String, Range<usize>)>,
    _permit: OwnedSemaphorePermit,
}
fn blocked(path: &str) -> bool {
    if path == LOCK {
        return false;
    }
    path.split('/').any(|part| {
        let name = part.to_ascii_lowercase();
        name.starts_with(".consolecrypt")
            || matches!(
                name.as_str(),
                ".git"
                    | ".ssh"
                    | ".gnupg"
                    | ".env"
                    | "id_rsa"
                    | "id_ed25519"
                    | "authorized_keys"
                    | "known_hosts"
                    | "vault_password"
                    | "vault-password"
                    | "credentials.json"
            )
            || name.starts_with(".env.")
            || name.ends_with(".key")
            || name.ends_with(".kdbx")
            || name.ends_with(".ccbackup")
    })
}
fn portable(path: &str) -> Result<()> {
    crate::dependency_archive::portable_path(path).map_err(|_| invalid())
}
fn metadata(manifest: &Manifest) -> Result<()> {
    require(
        manifest.version == 1
            && manifest.policy == CapturePolicy::ExplicitSnapshotSelectionV1
            && cc_models::automation::is_digest(&manifest.source_revision_digest)
            && manifest.exclusions.len() <= MAX_ENTRIES,
    )?;
    let mut previous = None;
    for exclusion in &manifest.exclusions {
        portable(&exclusion.path)?;
        require(previous.is_none_or(|old: &str| old < exclusion.path.as_str()))?;
        require(match exclusion.reason {
            ExclusionReason::KnownControl => blocked(&exclusion.path),
            ExclusionReason::Unselected => !blocked(&exclusion.path),
        })?;
        previous = Some(exclusion.path.as_str());
    }
    if let Some(playbook) = &manifest.playbook {
        portable(playbook)?;
        require(
            manifest
                .revision
                .entries
                .iter()
                .any(|e| matches!(e,ProjectEntry::File{path,..}if path==playbook)),
        )?;
    }
    Ok(())
}
fn tree(revision: &ProjectRevision) -> Result<()> {
    require(
        revision.version == 1
            && revision.entries.len() <= MAX_ENTRIES
            && revision.bytes <= MAX_BYTES,
    )?;
    let mut names = BTreeMap::new();
    let mut folded = BTreeSet::new();
    let mut previous = None;
    let mut files = 0usize;
    for entry in &revision.entries {
        let path = entry.path();
        portable(path)?;
        require(!blocked(path))?;
        require(
            previous.is_none_or(|old: &str| old < path) && folded.insert(path.to_ascii_lowercase()),
        )?;
        let parts = path.split('/').collect::<Vec<_>>();
        for end in 1..parts.len() {
            require(matches!(
                names.get(&parts[..end].join("/")),
                Some(ProjectEntry::Directory { .. })
            ))?;
        }
        if let ProjectEntry::File {
            mode,
            bytes,
            sha256,
            ..
        } = entry
        {
            files += 1;
            require(
                files <= MAX_FILES
                    && *mode == 0o600
                    && *bytes <= MAX_FILE
                    && cc_models::automation::is_digest(sha256),
            )?;
        } else {
            require(path != LOCK)?;
        }
        names.insert(path.to_owned(), entry.clone());
        previous = Some(path);
    }
    require(
        crate::dependency_archive::revision(revision.entries.clone()).map_err(|_| invalid())?
            == *revision,
    )
}
fn validate_lock(
    manifest: &Manifest,
    plaintext: &[u8],
    files: &[(String, Range<usize>)],
) -> Result<()> {
    let lock = files
        .iter()
        .find(|(path, _)| path == LOCK)
        .map(|(_, range)| &plaintext[range.clone()]);
    if let Some(raw) = lock {
        require(raw.len() <= 64 * 1024)?;
    }
    crate::dependency_preview::validate_portable_lock(lock, &manifest.revision)
}
fn verify_plain(
    plaintext: Zeroizing<Vec<u8>>,
    held: OwnedSemaphorePermit,
) -> Result<VerifiedPortablePackage> {
    require(plaintext.len() >= 4 && plaintext.len() <= cc_crypto_core::MAX_PORTABLE_PROJECT_PLAIN)?;
    let length = u32::from_be_bytes(plaintext[..4].try_into().map_err(|_| invalid())?) as usize;
    require(length <= MAX_MANIFEST && length <= plaintext.len() - 4)?;
    let manifest: Manifest = serde_json::from_value(crate::json::parse(&plaintext[4..4 + length])?)
        .map_err(|_| invalid())?;
    metadata(&manifest)?;
    tree(&manifest.revision)?;
    let included = manifest
        .revision
        .entries
        .iter()
        .map(|e| e.path())
        .collect::<BTreeSet<_>>();
    require(
        manifest
            .exclusions
            .iter()
            .all(|e| !included.contains(e.path.as_str())),
    )?;
    let mut offset = 4 + length;
    let mut files = Vec::new();
    for entry in &manifest.revision.entries {
        if let ProjectEntry::File {
            path,
            bytes,
            sha256,
            ..
        } = entry
        {
            let end = offset.checked_add(*bytes as usize).ok_or_else(invalid)?;
            require(end <= plaintext.len() && sha(&plaintext[offset..end]) == *sha256)?;
            files.push((path.clone(), offset..end));
            offset = end;
        }
    }
    require(offset == plaintext.len())?;
    validate_lock(&manifest, &plaintext, &files)?;
    Ok(VerifiedPortablePackage {
        manifest,
        encrypted_sha256:None,
        plaintext,
        files,
        _permit: held,
    })
}

/// Explicit selection is Native-reviewed DATA, not permission to resolve any
/// IDs. Only exact files/actually-empty directories are selected; no globs or
/// recursive folder auto-expansion. Known control/secret files cannot override
/// this exclusion policy. Arbitrary source can still contain user-written
/// secrets (hosts.ini/YAML); no universal content-secret detection is promised.
pub(crate) fn seal_project(
    root: &ProjectRoot,
    captured: &ProjectRevision,
    selected: &[String],
    playbook: Option<String>,
    passphrase: &SecretString,
    token: &CancellationToken,
) -> Result<Vec<u8>> {
    let _permit = permit()?;
    cancel(token)?;
    require(
        captured.entries.len() <= MAX_ENTRIES
            && !selected.is_empty()
            && selected.len() <= MAX_ENTRIES
            && root.revision()? == *captured,
    )?;
    let original = captured
        .entries
        .iter()
        .map(|e| (e.path(), e))
        .collect::<BTreeMap<_, _>>();
    let mut paths = BTreeSet::new();
    let mut included = BTreeMap::<String, ProjectEntry>::new();
    for path in selected {
        portable(path)?;
        require(paths.insert(path.as_str()) && !blocked(path))?;
        let entry = original.get(path.as_str()).ok_or_else(invalid)?;
        if matches!(entry, ProjectEntry::Directory { .. }) {
            let prefix = format!("{path}/");
            require(!original.keys().any(|p| p.starts_with(&prefix)))?;
        }
        included.insert(path.clone(), (**entry).clone());
        let parts = path.split('/').collect::<Vec<_>>();
        for end in 1..parts.len() {
            let parent = parts[..end].join("/");
            let entry = original.get(parent.as_str()).ok_or_else(invalid)?;
            require(matches!(entry, ProjectEntry::Directory { .. }))?;
            included.insert(parent, (**entry).clone());
        }
    }
    let entries = included.into_values().collect::<Vec<_>>();
    let revision = crate::dependency_archive::revision(entries).map_err(|_| invalid())?;
    tree(&revision)?;
    let included = revision
        .entries
        .iter()
        .map(|e| e.path())
        .collect::<BTreeSet<_>>();
    let mut exclusions = Vec::new();
    for entry in &captured.entries {
        portable(entry.path())?;
        if !included.contains(entry.path()) {
            exclusions.push(Exclusion {
                path: entry.path().into(),
                reason: if blocked(entry.path()) {
                    ExclusionReason::KnownControl
                } else {
                    ExclusionReason::Unselected
                },
            });
        }
    }
    let manifest = Manifest {
        version: 1,
        policy: CapturePolicy::ExplicitSnapshotSelectionV1,
        source_revision_digest: captured.digest.clone(),
        revision,
        playbook,
        exclusions,
    };
    metadata(&manifest)?;
    let raw = serde_json::to_vec(&manifest).map_err(|_| invalid())?;
    require(raw.len() <= MAX_MANIFEST)?;
    let mut plain = Zeroizing::new(Vec::with_capacity(
        4 + raw.len() + manifest.revision.bytes as usize,
    ));
    plain.extend_from_slice(&(raw.len() as u32).to_be_bytes());
    plain.extend_from_slice(&raw);
    for entry in &manifest.revision.entries {
        cancel(token)?;
        if let ProjectEntry::File {
            path,
            bytes,
            sha256,
            ..
        } = entry
        {
            let start = plain.len();
            root.open_file(path)?
                .take(bytes + 1)
                .read_to_end(&mut plain)?;
            require(plain.len() - start == *bytes as usize && sha(&plain[start..]) == *sha256)?;
        }
    }
    require(root.revision()? == *captured)?;
    cancel(token)?;
    // Use the same in-memory validator before encrypting; never emit an
    // unreadable lock/subtree or a payload that our reader would reject.
    // This borrows the same permit rather than acquiring a second slot.
    let mut offset = 4 + raw.len();
    let mut ranges = Vec::new();
    for entry in &manifest.revision.entries {
        if let ProjectEntry::File { path, bytes, .. } = entry {
            let end = offset + *bytes as usize;
            ranges.push((path.clone(), offset..end));
            offset = end;
        }
    }
    validate_lock(&manifest, &plain, &ranges)?;
    let encrypted = seal_portable_project(passphrase, &plain).map_err(|_| invalid())?;
    cancel(token)?;
    Ok(encrypted)
}
pub(crate) fn open_project(
    package: &[u8],
    passphrase: &SecretString,
    token: &CancellationToken,
) -> Result<VerifiedPortablePackage> {
    let held = permit()?;
    cancel(token)?;
    let plaintext = open_portable_project(passphrase, package).map_err(|_| invalid())?;
    cancel(token)?;
    let mut verified=verify_plain(plaintext, held)?;
    verified.encrypted_sha256=Some(sha(package));
    Ok(verified)
}
impl VerifiedPortablePackage {
    pub(crate) fn encrypted_sha256(&self)->Option<&str> {self.encrypted_sha256.as_deref()}
    pub(crate) fn revision(&self)->&ProjectRevision {&self.manifest.revision}
    pub(crate) fn summary(&self) -> PackageSummary {
        PackageSummary {
            source_digest: self.manifest.source_revision_digest.clone(),
            portable_digest: self.manifest.revision.digest.clone(),
            files: self.files.len(),
            bytes: self.manifest.revision.bytes,
            exclusions: self.manifest.exclusions.len(),
        }
    }
    /// ALL authentication/schema/path/hash/lock work finished before this call.
    /// The destination must be a NEW caller-owned root. Never overwrite or
    /// publish: cancellation/races may leave an explicitly uncommitted partial
    /// root. Its existing owner is responsible for joined scoped cleanup.
    pub(crate) fn write_uncommitted(
        self,
        destination: &ProjectRoot,
        token: &CancellationToken,
    ) -> Result<ProjectRevision> {
        cancel(token)?;
        destination.require_empty_import()?;
        for entry in &self.manifest.revision.entries {
            cancel(token)?;
            match entry {
                ProjectEntry::Directory { path } => destination.import_directory(path)?,
                ProjectEntry::File { path, mode, .. } => {
                    let range = &self
                        .files
                        .iter()
                        .find(|(p, _)| p == path)
                        .ok_or_else(invalid)?
                        .1;
                    let mut output = destination.import_file(path, *mode)?;
                    output.write_all(&self.plaintext[range.clone()])?;
                    destination.sync_import_file(&output)?;
                }
            }
        }
        cancel(token)?;
        let actual = destination.revision()?;
        require(actual == self.manifest.revision)?;
        Ok(actual)
    }
}

#[cfg(test)]
mod tests;

mod operation_io;
pub use operation_io::{PortableChoice, PortableImport, PortablePackageSummary, decrypt_package, encrypt_selection, selection_choices, read_encrypted_file, write_encrypted_new, write_encrypted_new_checked};

#[cfg(test)]
pub(crate) fn test_serial()->std::sync::MutexGuard<'static,()> {
    static SERIAL:std::sync::Mutex<()>=std::sync::Mutex::new(());
    SERIAL.lock().unwrap()
}
