//! Profile-local path/provenance validation. Metadata is never an archive,
//! approval, filesystem identity or an instruction to follow a supplied path.
use super::*;
use sha2::{Digest, Sha256};

const FIELD: &str = "publication";
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PublicationMarker {
    pub version: u8,
    pub transaction: Uuid,
    pub operation: Uuid,
    pub installed_revision: u64,
    pub base_digest: String,
    pub candidate_digest: String,
    pub review_digest: String,
    pub previous_row_sha256: String,
    pub publication_fingerprint: String,
}
fn invalid() -> AppError {
    AppError::invalid(
        "automation_project",
        "managed project provenance or path unavailable",
    )
}
pub(super) fn marker(row: &Value) -> AppResult<Option<PublicationMarker>> {
    let Some(value) = row.get(FIELD) else {
        return Ok(None);
    };
    if row.pointer("/source/kind").and_then(Value::as_str) != Some("managed")
        || serde_json::to_vec(value).map_err(|_| invalid())?.len() > 2048
    {
        return Err(invalid());
    }
    for field in ["transaction", "operation"] {
        let raw = value
            .get(field)
            .and_then(Value::as_str)
            .ok_or_else(invalid)?;
        if Uuid::parse_str(raw).map_err(|_| invalid())?.to_string() != raw {
            return Err(invalid());
        }
    }
    let marker: PublicationMarker = serde_json::from_value(value.clone()).map_err(|_| invalid())?;
    if marker.version != 1
        || marker.transaction.is_nil()
        || marker.operation.is_nil()
        || marker.installed_revision < 2
        || row
            .get("revision")
            .and_then(Value::as_u64)
            .is_none_or(|r| r < marker.installed_revision)
        || [
            &marker.base_digest,
            &marker.candidate_digest,
            &marker.review_digest,
            &marker.previous_row_sha256,
            &marker.publication_fingerprint,
        ]
        .iter()
        .any(|s| !cc_models::automation::is_digest(s))
    {
        return Err(invalid());
    }
    Ok(Some(marker))
}
pub(super) fn validate_path(
    profile: Option<&std::path::Path>,
    id: &str,
    row: &Value,
) -> AppResult<()> {
    let provenance = marker(row)?;
    let portable=portable::import_catalog::ready_provenance(id,row)?;
    if row.pointer("/source/kind").and_then(Value::as_str) != Some("managed") {
        return Ok(());
    }
    project_id(id)?;
    let profile = profile.ok_or_else(invalid)?;
    if !profile.is_absolute() {
        return Err(invalid());
    }
    let expected = if let Some((transaction,_))=portable {
        profile.join("automation").join("project-revisions").join(id).join(format!("generation-{transaction}"))
    } else {match provenance {
        None => profile.join("automation").join("projects").join(id),
        Some(marker) => profile
            .join("automation")
            .join("project-revisions")
            .join(id)
            .join(format!("generation-{}", marker.transaction)),
    }};
    let actual = row
        .pointer("/source/path")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?;
    // OsStr equality is exact; Path equality/canonicalize must not normalize a
    // foreign or caller-supplied spelling into an accepted managed family.
    if std::ffi::OsStr::new(actual) != expected.as_os_str() {
        return Err(invalid());
    }
    Ok(())
}
pub(crate) fn supported_at(profile: Option<&std::path::Path>, id: &str, row: &Value) -> bool {
    supported(row) && validate_path(profile, id, row).is_ok()
}
/// Small Native-captured SQL metadata, not a path resolver or approval token.
#[derive(Clone)]
pub(crate) struct RootSelection {
    path: PathBuf,
    generation: Option<(PathBuf, Uuid, Uuid, String)>,
    portable_generation:Option<(PathBuf,Uuid,Uuid,String)>,
}
impl RootSelection {
    pub(super) fn capture(
        profile: Option<&std::path::Path>,
        id: &str,
        row: &Value,
    ) -> AppResult<Self> {
        validate_path(profile, id, row)?;
        let path = PathBuf::from(
            row.pointer("/source/path")
                .and_then(Value::as_str)
                .ok_or_else(invalid)?,
        );
        let generation = marker(row)?
            .map(|m| {
                Ok::<_, AppError>((
                    profile.ok_or_else(invalid)?.to_owned(),
                    Uuid::parse_str(id).map_err(|_| invalid())?,
                    m.transaction,
                    m.publication_fingerprint,
                ))
            })
            .transpose()?;
        let portable_generation=portable::import_catalog::ready_provenance(id,row)?.map(|(transaction,fingerprint)|{
            Ok::<_,AppError>((profile.ok_or_else(invalid)?.to_owned(),Uuid::parse_str(id).map_err(|_|invalid())?,transaction,fingerprint))
        }).transpose()?;
        Ok(Self { path, generation,portable_generation })
    }
    pub(crate) fn open(&self) -> AppResult<OpenedProjectRoot> {
        if let Some((profile,project,transaction,fingerprint))=&self.portable_generation {
            if self.generation.is_some(){return Err(invalid());}
            return cc_automation_core::portable_publication::open_selected_portable_project(profile,*project,*transaction,fingerprint)
                .map(OpenedProjectRoot::Portable).map_err(automation_error);
        }
        match &self.generation {
            None => ProjectRoot::open(&self.path)
                .map(OpenedProjectRoot::Plain)
                .map_err(automation_error),
            Some((profile, project, transaction, fingerprint)) => {
                cc_automation_core::open_selected_generation(
                    profile,
                    *project,
                    *transaction,
                    fingerprint,
                )
                .map(OpenedProjectRoot::Selected)
                .map_err(automation_error)
            }
        }
    }
}
pub(crate) enum OpenedProjectRoot {
    Plain(ProjectRoot),
    Selected(cc_automation_core::PinnedSelectedDependencyGeneration),
    Portable(cc_automation_core::portable_publication::PinnedSelectedPortableProject),
}
impl std::ops::Deref for OpenedProjectRoot {
    type Target = ProjectRoot;
    fn deref(&self) -> &ProjectRoot {
        match self {
            Self::Plain(root) => root,
            Self::Selected(pinned) => pinned.root(),
            Self::Portable(pinned)=>pinned.root(),
        }
    }
}
impl OpenedProjectRoot {
    pub(crate) fn recheck(&self) -> AppResult<()> {
        match self {
            Self::Plain(_) => Ok(()),
            Self::Selected(pinned) => pinned.recheck().map_err(automation_error),
            Self::Portable(pinned)=>pinned.recheck().map_err(automation_error),
        }
    }
}
pub(crate) fn row_stamp(row: &Value) -> AppResult<String> {
    let raw = serde_json::to_vec(row).map_err(|_| invalid())?;
    if raw.len() > 1024 * 1024 {
        return Err(invalid());
    }
    Ok(hex::encode(Sha256::digest(raw)))
}
pub(crate) async fn admit_io(
    app: &AppCore,
    context: &AutomationProjectContext,
) -> AppResult<Arc<tokio::sync::OwnedSemaphorePermit>> {
    let retained = Arc::new(automation_io_slot(&context.owner)?);
    context.check_after_io(app).await?;
    Ok(retained)
}
impl AutomationProjectContext {
    /// Recheck ONLY the original owner/unlock and captured row. No latest-scope
    /// fallback or recursive active mutex; never hold these guards across FS.
    pub(crate) async fn check_after_io(&self, app: &AppCore) -> AppResult<()> {
        let u = self.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = app.hold_session(&self.owner).await?;
        let _unlock = self.owner.hold_unlocked(&u).await?;
        self.check_catalog_held().await
    }

    /// Existing row fence with ORIGINAL scope guards already held. No recapture.
    pub(super) async fn check_catalog_held(&self) -> AppResult<()> {
        let catalog: Catalog = self
            .owner
            .storage()?
            .setting_get(PROJECTS)
            .await?
            .unwrap_or_default();
        let row = catalog.records.get(&self.project.id).ok_or_else(invalid)?;
        if catalog.version != 1
            || !catalog.extra.is_empty()
            || !supported_at(self.owner.dir.as_deref(), &self.project.id, row)
            || row_stamp(row)? != self.catalog_row_sha256
        {
            return Err(invalid());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
