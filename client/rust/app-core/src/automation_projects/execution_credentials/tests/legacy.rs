//! Exact pre-feature function bodies; only receiver/name and legacy guard
//! routing are adapted for this private causal test module. Not product code.
use super::super::super::*;
fn legacy_supported(row: &Value) -> bool {
    managed::marker(row).is_ok()
        && portable::import_catalog::ordinary_supported(row)
        && groups::from_row(row).is_ok()
        && baseline::from_row(row).is_ok()
        && source_bindings::supported_marker(row)
        && row.get("version").and_then(Value::as_u64) == Some(1)
        && matches!(
            row.pointer("/source/kind").and_then(Value::as_str),
            Some("directory" | "managed" | "git")
        )
        && row
            .pointer("/source/path")
            .and_then(Value::as_str)
            .is_some()
        && row
            .get("revision")
            .and_then(Value::as_u64)
            .is_some_and(|version| version > 0)
        && (row.pointer("/source/kind").and_then(Value::as_str) != Some("git")
            || row.pointer("/source/status").and_then(Value::as_str) == Some("ready")
                && row
                    .pointer("/source/git/metadata_sha256")
                    .and_then(Value::as_str)
                    .is_some_and(cc_models::automation::is_digest))
}

fn legacy_supported_at(profile: Option<&std::path::Path>, id: &str, row: &Value) -> bool {
    legacy_supported(row) && managed::validate_path(profile, id, row).is_ok()
}
pub(super) async fn rename(
    app: &AppCore,
    id: String,
    expected_revision: u64,
    name: String,
) -> AppResult<AutomationProjectSummary> {
    project_id(&id)?;
    let name = valid_name(&name)?;
    let (owner, unlocked) = app.unlocked().await?;
    let profile_path = owner.dir.clone();
    let _profile = app.hold_session(&owner).await?;
    let _generation = owner.hold_unlocked(&unlocked).await?;
    owner
        .storage()?
        .write(move |transaction| {
            let mut document: Catalog = transaction.setting_get(PROJECTS)?.unwrap_or_default();
            if document.version != 1 {
                return Err(AppError::invalid(
                    "automation_projects",
                    "newer catalog is read-only",
                ));
            }
            let row = document
                .records
                .get_mut(&id)
                .ok_or_else(|| AppError::not_found("automation project", &id))?;
            if !legacy_supported_at(profile_path.as_deref(), &id, row)
                || row.get("revision").and_then(Value::as_u64) != Some(expected_revision)
                || expected_revision == u64::MAX
            {
                return Err(AppError::invalid(
                    "automation_project",
                    "project changed or uses a newer schema",
                ));
            }
            row["name"] = Value::String(name);
            row["revision"] = Value::from(expected_revision + 1);
            row["updated_at"] = Value::from(chrono::Utc::now().timestamp());
            let result = summary(&id, row, false);
            transaction.setting_set(PROJECTS, &document)?;
            Ok::<_, AppError>(result)
        })
        .await
}
pub(super) async fn context_original(
    app: &AppCore,
    id: &str,
    owner: Arc<crate::session::Session>,
    unlock: Weak<crate::session::Unlocked>,
) -> AppResult<AutomationProjectContext> {
    project_id(id)?;
    let unlocked = unlock.upgrade().ok_or(AppError::VaultLocked)?;
    {
        let _profile = app.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&unlocked).await?;
    }
    drop(unlocked);
    let document: Catalog = owner
        .storage()?
        .setting_get(PROJECTS)
        .await?
        .unwrap_or_default();
    let row = document
        .records
        .get(id)
        .ok_or_else(|| AppError::not_found("automation project", id))?;
    if document.version != 1 || !legacy_supported(row) {
        return Err(AppError::invalid(
            "automation_project",
            "unsupported project schema",
        ));
    }
    managed::validate_path(owner.dir.as_deref(), id, row)?;
    let catalog_row_sha256 = managed::row_stamp(row)?;
    let root_selection = managed::RootSelection::capture(owner.dir.as_deref(), id, row)?;
    let path = PathBuf::from(
        row.pointer("/source/path")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    let project = summary(id, row, false);
    let inventory = groups::from_row(row)?;
    let baseline = baseline::from_row(row)?;
    let external_inventory = source_bindings::load(&owner, id, row).await?;
    let git_metadata_sha256 = row
        .pointer("/source/git/metadata_sha256")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let unlocked = unlock.upgrade().ok_or(AppError::VaultLocked)?;
    let _profile = app.hold_session(&owner).await?;
    let _generation = owner.hold_unlocked(&unlocked).await?;
    drop(_generation);
    drop(_profile);
    Ok(AutomationProjectContext {
        owner,
        unlock,
        path,
        catalog_row_sha256,
        root_selection,
        project,
        git_metadata_sha256,
        inventory,
        external_inventory,
        baseline,
    })
}

async fn legacy_context(app: &AppCore, id: &str) -> AppResult<AutomationProjectContext> {
    let (owner, u) = app.unlocked().await?;
    let weak = Arc::downgrade(&u);
    drop(u);
    context_original(app, id, owner, weak).await
}
pub(super) async fn save(
    app: &AppCore,
    id: String,
    path: String,
    expected_sha256: Option<String>,
    text: String,
) -> AppResult<ProjectFile> {
    let context = legacy_context(app, &id).await?;
    // This one-file capability has already been authorized. Never hold a
    // vault transition guard across disk I/O: even a stalled filesystem
    // must not block locking. The result is fenced to the original profile.
    let retained_io = managed::admit_io(app, &context).await?;
    let io_slot = retained_io.clone();
    let source = context.root_selection.clone();
    let result = tokio::task::spawn_blocking(move || -> AppResult<_> {
        let _io_slot = io_slot;
        let root = source.open()?;
        let value = root
            .write_file(&path, expected_sha256.as_deref(), &text)
            .map_err(automation_error)?;
        root.recheck()?;
        Ok(value)
    })
    .await
    .map_err(|_| AppError::internal("project editor stopped"))??;
    context.check_after_io(app).await?;
    drop(retained_io);
    Ok(result)
}
