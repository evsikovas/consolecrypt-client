//! Bounded device-local original trees. Like ordinary project files these are
//! not vault-encrypted packages; portable encrypted snapshots are a later scope.
use super::*;
use tokio_util::sync::CancellationToken;

const MAX_CACHE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_BASELINES: usize = 256;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Baseline {
    version: u32,
    directory: String,
    pub digest: String,
    pub(super) bytes: u64,
}
fn invalid() -> AppError {
    AppError::invalid(
        "automation_baseline",
        "baseline cache is changed, full or unavailable",
    )
}
fn valid_directory(name: &str) -> bool {
    (10..=80).contains(&name.len())
        && name.starts_with("baseline-")
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
impl Baseline {
    fn validate(&self) -> AppResult<()> {
        if self.version != 1
            || !valid_directory(&self.directory)
            || !cc_models::automation::is_digest(&self.digest)
            || self.bytes > 100 * 1024 * 1024
        {
            return Err(invalid());
        }
        Ok(())
    }
}
pub(super) fn from_row(row: &Value) -> AppResult<Option<Baseline>> {
    let Some(value) = row.get("baseline") else {
        return Ok(None);
    };
    let baseline: Baseline = serde_json::from_value(value.clone()).map_err(|_| invalid())?;
    baseline.validate()?;
    Ok(Some(baseline))
}
pub(super) fn base(profile: &std::path::Path) -> PathBuf {
    profile.join("automation").join("baselines")
}

/// Called with the profile's single baseline writer permit and an I/O permit.
pub(super) fn capture(
    cache: &std::path::Path,
    source: &ProjectRoot,
    revision: &ProjectRevision,
    cancel: &CancellationToken,
) -> AppResult<Baseline> {
    capture_bounded(
        cache,
        source,
        revision,
        cancel,
        MAX_CACHE_BYTES,
        MAX_BASELINES,
    )
}
fn capture_bounded(
    cache: &std::path::Path,
    source: &ProjectRoot,
    revision: &ProjectRevision,
    cancel: &CancellationToken,
    max_bytes: u64,
    max_count: usize,
) -> AppResult<Baseline> {
    std::fs::create_dir_all(cache)?;
    if std::fs::symlink_metadata(cache)?.file_type().is_symlink() {
        return Err(invalid());
    }
    let mut count = 0usize;
    let mut used = 0u64;
    for entry in std::fs::read_dir(cache)? {
        if cancel.is_cancelled() {
            return Err(AppError::VaultLocked);
        }
        let entry = entry?;
        count += 1;
        if count >= max_count
            || !entry.file_type()?.is_dir()
            || !entry.file_name().to_str().is_some_and(valid_directory)
        {
            return Err(invalid());
        }
        let tree = entry.path().join("tree");
        // An interrupted, still empty staging directory has no tree yet. It
        // consumes the count budget; complete orphaned trees consume bytes too.
        if tree.try_exists()? {
            used = used
                .checked_add(
                    ProjectRoot::open(&tree)
                        .map_err(automation_error)?
                        .revision()
                        .map_err(automation_error)?
                        .bytes,
                )
                .ok_or_else(invalid)?;
        }
        if used > max_bytes {
            return Err(invalid());
        }
    }
    if revision.bytes > max_bytes.saturating_sub(used) || cancel.is_cancelled() {
        return Err(invalid());
    }
    if cc_platform_core::available_disk_space(cache)?
        < revision.bytes.saturating_add(128 * 1024 * 1024)
    {
        return Err(invalid());
    }
    let staging = tempfile::Builder::new()
        .prefix("baseline-")
        .tempdir_in(cache)?;
    let tree = staging.path().join("tree");
    std::fs::create_dir(&tree)?;
    let destination = ProjectRoot::open(&tree).map_err(automation_error)?;
    source
        .freeze_to(revision, &destination, cancel)
        .map_err(automation_error)?;
    let directory = staging
        .path()
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or_else(invalid)?
        .to_owned();
    let baseline = Baseline {
        version: 1,
        directory,
        digest: revision.digest.clone(),
        bytes: revision.bytes,
    };
    baseline.validate()?;
    if cancel.is_cancelled() {
        return Err(AppError::VaultLocked);
    }
    // Keep before ready metadata. Failures after this are accounted as orphaned
    // managed cache entries; a ready row never depends on a dropped TempDir.
    let _path = staging.keep();
    Ok(baseline)
}
pub(super) fn open_verified(
    profile: &std::path::Path,
    baseline: &Baseline,
) -> AppResult<(ProjectRoot, ProjectRevision)> {
    baseline.validate()?;
    let directory = base(profile).join(&baseline.directory);
    if std::fs::symlink_metadata(&directory)?
        .file_type()
        .is_symlink()
    {
        return Err(invalid());
    }
    let root = ProjectRoot::open(&directory.join("tree")).map_err(automation_error)?;
    let revision = root.revision().map_err(automation_error)?;
    if revision.digest != baseline.digest || revision.bytes != baseline.bytes {
        return Err(invalid());
    }
    Ok((root, revision))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn baseline_is_immutable_bounded_and_cannot_appoint_an_external_path() {
        let profile = tempfile::tempdir().unwrap();
        let files = tempfile::tempdir().unwrap();
        std::fs::write(files.path().join("playbook.yml"), "---\n# original\n").unwrap();
        let root = ProjectRoot::open(files.path()).unwrap();
        let revision = root.revision().unwrap();
        let cache = base(profile.path());
        let baseline = capture_bounded(
            &cache,
            &root,
            &revision,
            &CancellationToken::new(),
            revision.bytes,
            4,
        )
        .unwrap();
        std::fs::write(files.path().join("playbook.yml"), "# edited\n").unwrap();
        let (saved, original) = open_verified(profile.path(), &baseline).unwrap();
        assert_eq!(original, revision);
        assert_eq!(
            saved.read_text("playbook.yml").unwrap(),
            "---\n# original\n"
        );
        assert!(
            capture_bounded(
                &cache,
                &root,
                &root.revision().unwrap(),
                &CancellationToken::new(),
                revision.bytes,
                4
            )
            .is_err()
        );
        let mut outside = baseline.clone();
        outside.directory = "../outside".into();
        assert!(open_verified(profile.path(), &outside).is_err());
        std::fs::write(
            cache.join(&baseline.directory).join("tree/playbook.yml"),
            "# tampered\n",
        )
        .unwrap();
        assert!(open_verified(profile.path(), &baseline).is_err());
        let future = json!({"baseline":{"version":2,"directory":"baseline-future","digest":"a".repeat(64),"bytes":0}});
        assert!(from_row(&future).is_err());
    }
}
