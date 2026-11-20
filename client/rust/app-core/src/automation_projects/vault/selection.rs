//! Original-source metadata for explicit public selection. No password lookup,
//! approval token, holder map, arbitrary path or source-kind conversion.
use super::*;
use crate::automation_prepare::CancelOnDrop;
use tokio::sync::oneshot;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VaultSelectionFrame {
    version: u8,
    profile_id: String,
    notification_scope_id: String,
    project_id: String,
    project_revision: u64,
    catalog_sha256: String,
    revision_digest: String,
    directory_identity: String,
    reference_sha256: Option<String>,
}
impl VaultSelectionFrame {
    pub(crate) fn valid(&self) -> bool {
        self.version == 1
            && self.project_revision > 0
            && [
                &self.catalog_sha256,
                &self.revision_digest,
                &self.directory_identity,
            ]
            .iter()
            .all(|v| cc_models::automation::is_digest(v))
            && self
                .reference_sha256
                .as_ref()
                .is_none_or(|v| cc_models::automation::is_digest(v))
            && [
                &self.profile_id,
                &self.notification_scope_id,
                &self.project_id,
            ]
            .iter()
            .all(|v| Uuid::parse_str(v).is_ok_and(|id| !id.is_nil() && id.to_string() == **v))
    }
    fn original(&self, context: &AutomationProjectContext, u: &crate::session::Unlocked) -> bool {
        self.valid()
            && self.profile_id == context.owner.profile_id.to_string()
            && u.notices.scope_id().as_deref() == Some(self.notification_scope_id.as_str())
            && self.project_id == context.project.id
            && Some(self.project_revision) == context.project.revision
            && self.catalog_sha256 == context.catalog_row_sha256
    }
    pub(crate) fn matches_tree(
        &self,
        root: &ProjectRoot,
        revision: &ProjectRevision,
    ) -> AppResult<()> {
        if revision.digest != self.revision_digest
            || root.directory_identity_digest().map_err(|_| invalid())? != self.directory_identity
        {
            return Err(invalid());
        }
        Ok(())
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SelectionContext {
    version: u8,
    frame: VaultSelectionFrame,
    // Reference data is NEVER automatic credential selection.
    referenced_credential_id: Option<String>,
    diff_supported: bool,
}
struct Scan {
    revision: ProjectRevision,
    identity: String,
    reference: Option<(String, String)>,
}

async fn scan(
    app: &AppCore,
    context: &AutomationProjectContext,
    write: Option<(VaultSelectionFrame, Uuid)>,
) -> AppResult<(Scan, Arc<OwnedSemaphorePermit>)> {
    let retained = managed::admit_io(app, context).await?;
    let io = retained.clone();
    let epoch = context
        .owner
        .automation
        .dependencies
        .local_filesystem_epoch()?;
    let u = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
    let shutdown = u.sharing_shutdown.subscribe();
    drop(u);
    let cancelled = CancelOnDrop(CancellationToken::new());
    let cancel = cancelled.0.clone();
    let deadline = Instant::now() + Duration::from_secs(60);
    let source = context.root_selection.clone();
    let path = context.path.clone();
    let (send, receive) = oneshot::channel();
    context
        .owner
        .automation
        .dependencies
        .launch_worker(move || {
            let check = || {
                if epoch.is_cancelled()
                    || cancel.is_cancelled()
                    || *shutdown.borrow()
                    || Instant::now() >= deadline
                {
                    Err(invalid())
                } else {
                    Ok(())
                }
            };
            let result = (|| {
                check()?;
                let root = source.open()?;
                root.reopen_same_directory(&path).map_err(|_| invalid())?;
                let mut revision = root.revision().map_err(|_| invalid())?;
                let mut reference = root
                    .vault_input_reference(&revision)
                    .map_err(|_| invalid())?
                    .map(|v| {
                        (
                            v.credential_id().to_string(),
                            v.reference_sha256().to_owned(),
                        )
                    });
                let identity = root.directory_identity_digest().map_err(|_| invalid())?;
                if let Some((expected, credential)) = write {
                    expected.matches_tree(&root, &revision)?;
                    if reference.as_ref().map(|v| &v.1) != expected.reference_sha256.as_ref() {
                        return Err(invalid());
                    }
                    // Unknown/future files have already failed through Core's ONE
                    // strict parser. A missing ref uses write_file's no-overwrite CAS.
                    let text = json!({"version":1,"ansible_vault":{"credential_id":credential}})
                        .to_string();
                    check()?;
                    root.write_file(
                        cc_automation_core::VAULT_INPUT_FILE,
                        expected.reference_sha256.as_deref(),
                        &text,
                    )
                    .map_err(|_| invalid())?;
                    revision = root.revision().map_err(|_| invalid())?;
                    reference = root
                        .vault_input_reference(&revision)
                        .map_err(|_| invalid())?
                        .map(|v| {
                            (
                                v.credential_id().to_string(),
                                v.reference_sha256().to_owned(),
                            )
                        });
                }
                root.recheck()?;
                root.reopen_same_directory(&path).map_err(|_| invalid())?;
                check()?;
                Ok(Scan {
                    revision,
                    identity,
                    reference,
                })
            })();
            drop(send.send(result));
            drop(io);
        })?;
    let result = tokio::time::timeout(Duration::from_secs(60), receive)
        .await
        .map_err(|_| invalid())?
        .map_err(|_| invalid())??;
    context.check_after_io(app).await?;
    Ok((result, retained))
}
fn reply(
    context: &AutomationProjectContext,
    u: &crate::session::Unlocked,
    scan: Scan,
) -> AppResult<Value> {
    serde_json::to_value(SelectionContext {
        version: 1,
        frame: VaultSelectionFrame {
            version: 1,
            profile_id: context.owner.profile_id.to_string(),
            notification_scope_id: u.notices.scope_id().ok_or(AppError::VaultLocked)?,
            project_id: context.project.id.clone(),
            project_revision: context.project.revision.ok_or_else(invalid)?,
            catalog_sha256: context.catalog_row_sha256.clone(),
            revision_digest: scan.revision.digest,
            directory_identity: scan.identity,
            reference_sha256: scan.reference.as_ref().map(|v| v.1.clone()),
        },
        referenced_credential_id: scan.reference.map(|v| v.0),
        diff_supported: false,
    })
    .map_err(|_| invalid())
}
impl AppCore {
    pub async fn automation_project_vault_context_json(
        &self,
        project_id: String,
    ) -> AppResult<String> {
        let context = self.automation_project_context(&project_id).await?;
        let (scan, _io) = scan(self, &context, None).await?;
        let u = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = self.hold_session(&context.owner).await?;
        let _unlock = context.owner.hold_unlocked(&u).await?;
        // SQL first; capture nonce from the SAME original Unlocked afterward.
        if stamp(&context).await? != context.catalog_row_sha256 {
            return Err(invalid());
        }
        serde_json::to_string(&reply(&context, &u, scan)?).map_err(|_| invalid())
    }
    /// Fixed reference-file CAS. Reference metadata never grants secret access.
    pub async fn automation_project_vault_select_json(&self, payload: String) -> AppResult<String> {
        if payload.len() > 8192 {
            return Err(invalid());
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Request {
            frame: VaultSelectionFrame,
            credential_id: String,
        }
        let request: Request = serde_json::from_str(&payload).map_err(|_| invalid())?;
        let credential = Uuid::parse_str(&request.credential_id).map_err(|_| invalid())?;
        if !request.frame.valid()
            || credential.is_nil()
            || credential.to_string() != request.credential_id
        {
            return Err(invalid());
        }
        let context = self
            .automation_project_context(&request.frame.project_id)
            .await?;
        if context.project.read_only {
            return Err(invalid());
        }
        let metadata = {
            let u = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
            let _profile = self.hold_session(&context.owner).await?;
            let _unlock = context.owner.hold_unlocked(&u).await?;
            if !request.frame.original(&context, &u) {
                return Err(invalid());
            }
            u.working()
                .vault_credential_metadata(crate::dto::parse_id(
                    "credential_id",
                    &request.credential_id,
                )?)
                .ok_or_else(invalid)?
        };
        let (scan, _io) = scan(self, &context, Some((request.frame.clone(), credential))).await?;
        let u = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = self.hold_session(&context.owner).await?;
        let _unlock = context.owner.hold_unlocked(&u).await?;
        if stamp(&context).await? != request.frame.catalog_sha256
            || !request.frame.original(&context, &u)
            || u.working()
                .vault_credential_metadata(metadata.credential())
                .as_ref()
                != Some(&metadata)
        {
            return Err(invalid());
        }
        serde_json::to_string(&reply(&context, &u, scan)?).map_err(|_| invalid())
    }
}
/// Called before any Runner/target network, then full identity/tree is repeated
/// on the actual source root used by preparation. UI metadata grants nothing.
pub(crate) async fn check_run_frame(
    app: &AppCore,
    context: &AutomationProjectContext,
    frame: &VaultSelectionFrame,
    selected: &str,
) -> AppResult<()> {
    if !frame.valid() {
        return Err(invalid());
    }
    {
        let u = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = app.hold_session(&context.owner).await?;
        let _unlock = context.owner.hold_unlocked(&u).await?;
        if !frame.original(context, &u) {
            return Err(invalid());
        }
    }
    let (scan, _io) = scan(app, context, None).await?;
    if scan.revision.digest != frame.revision_digest
        || scan.identity != frame.directory_identity
        || scan.reference.as_ref().map(|v| v.1.as_str()) != frame.reference_sha256.as_deref()
        || scan.reference.as_ref().map(|v| v.0.as_str()) != Some(selected)
    {
        return Err(invalid());
    }
    let u = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
    let _profile = app.hold_session(&context.owner).await?;
    let _unlock = context.owner.hold_unlocked(&u).await?;
    if stamp(context).await? != frame.catalog_sha256 || !frame.original(context, &u) {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
