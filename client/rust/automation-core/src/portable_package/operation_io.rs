//! Narrow operation adapter over the existing A7 parser, cipher and copier.
//! No catalog, secret resolver, Runner/network or approval authority.
use super::*;
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortableChoice {
    pub path: String,
    pub bytes: u64,
    pub directory: bool,
    pub selectable: bool,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PortablePackageSummary {
    pub source_digest: String,
    pub portable_digest: String,
    pub files: usize,
    pub bytes: u64,
    pub exclusions: usize,
}
/// Full authentication/tree/lock witness. Non-Clone/non-Serde; retains A7's
/// process slot and zeroizing plaintext until consumed/dropped.
pub struct PortableImport {
    inner: VerifiedPortablePackage,
}
impl PortableImport {
    pub fn encrypted_sha256(&self)->Option<&str> {self.inner.encrypted_sha256()}
    pub(crate) fn revision(&self)->&ProjectRevision {self.inner.revision()}
    pub(crate) fn into_verified(self)->VerifiedPortablePackage {self.inner}
    pub fn summary(&self) -> PortablePackageSummary {
        let v = self.inner.summary();
        PortablePackageSummary {
            source_digest: v.source_digest,
            portable_digest: v.portable_digest,
            files: v.files,
            bytes: v.bytes,
            exclusions: v.exclusions,
        }
    }
    /// Only a NEW empty caller-owned root. The Native caller supplies its own
    /// managed staging root, never a UI/helper path. No catalog publication.
    pub fn write_uncommitted(
        self,
        destination: &ProjectRoot,
        cancel: &CancellationToken,
    ) -> Result<ProjectRevision> {
        self.inner.write_uncommitted(destination, cancel)
    }
}
pub fn selection_choices(captured: &ProjectRevision) -> Result<Vec<PortableChoice>> {
    require(captured.entries.len() <= MAX_ENTRIES)?;
    let mut out = Vec::with_capacity(captured.entries.len());
    let names = captured
        .entries
        .iter()
        .map(|e| e.path().to_owned())
        .collect::<BTreeSet<_>>();
    for entry in &captured.entries {
        portable(entry.path())?;
        let (directory, bytes) = match entry {
            ProjectEntry::File { bytes, .. } => (false, *bytes),
            ProjectEntry::Directory { .. } => (true, 0),
        };
        let prefix = format!("{}/", entry.path());
        let has_children = names
            .range(prefix.clone()..)
            .next()
            .is_some_and(|name| name.starts_with(&prefix));
        let selectable = !blocked(entry.path()) && (!directory || !has_children);
        out.push(PortableChoice {
            path: entry.path().into(),
            bytes,
            directory,
            selectable,
        });
    }
    Ok(out)
}
pub fn encrypt_selection(
    root: &ProjectRoot,
    captured: &ProjectRevision,
    selected: &[String],
    playbook: Option<String>,
    password: &SecretString,
    cancel: &CancellationToken,
) -> Result<Vec<u8>> {
    seal_project(root, captured, selected, playbook, password, cancel)
}
pub fn decrypt_package(
    encrypted: &[u8],
    password: &SecretString,
    cancel: &CancellationToken,
) -> Result<PortableImport> {
    Ok(PortableImport {
        inner: open_project(encrypted, password, cancel)?,
    })
}
fn leaf(name: &str) -> Result<()> {
    portable(name)?;
    require(!name.contains('/') && name.ends_with(".ccproject") && name.len() > 10)
}
/// Existing ProjectRoot bounded/nofollow/nlink-checked file opener. Encrypted
/// bytes only; header/version/KDF/AEAD still checked by decrypt_package.
pub fn read_encrypted_file(parent: &ProjectRoot, name: &str) -> Result<Vec<u8>> {
    leaf(name)?;
    let mut raw = Vec::new();
    parent
        .open_file(name)?
        .take(cc_crypto_core::MAX_PORTABLE_PROJECT_BYTES as u64 + 1)
        .read_to_end(&mut raw)?;
    require(raw.len() <= cc_crypto_core::MAX_PORTABLE_PROJECT_BYTES)?;
    Ok(raw)
}
/// No-replace encrypted FILE output using the existing create_new/nofollow
/// import_file primitive. This is not directory publication. A failed/cancelled
/// write can leave this newly created file incomplete; never remove by pathname
/// or overwrite any existing file. Contents sync is not a power-loss claim.
pub fn write_encrypted_new(
    parent: &ProjectRoot,
    name: &str,
    encrypted: &[u8],
    cancel_token: &CancellationToken,
) -> Result<()> {
    write_encrypted_new_checked(parent, name, encrypted, cancel_token, || Ok(()))
}
/// The embedding Native operation supplies its original shutdown/cancel-only
/// fence. A callback is Rust-only, never deserialized from a package/UI/model.
/// Checks define timepoints, not atomic rollback of an OS write already begun.
pub fn write_encrypted_new_checked(
    parent: &ProjectRoot,
    name: &str,
    encrypted: &[u8],
    cancel_token: &CancellationToken,
    mut fence: impl FnMut() -> Result<()>,
) -> Result<()> {
    leaf(name)?;
    require(encrypted.len() <= cc_crypto_core::MAX_PORTABLE_PROJECT_BYTES)?;
    fence()?;
    cancel(cancel_token)?;
    let mut file = parent.import_file(name, 0o600)?;
    for chunk in encrypted.chunks(64 * 1024) {
        fence()?;
        cancel(cancel_token)?;
        file.write_all(chunk)?;
    }
    fence()?;
    cancel(cancel_token)?;
    file.sync_all()?;
    fence()?;
    cancel(cancel_token)
}
