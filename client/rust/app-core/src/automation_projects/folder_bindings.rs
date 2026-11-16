//! Profile-local folder proposals. A folder never grants targets or credentials.
use super::*;
use crate::{
    session::Unlocked,
    working_set::{FolderSnapshot, FolderWatch},
};
use cc_automation_core::ProjectInventory;
use cc_models::ObjectId;
use sha2::{Digest, Sha256};
const FIELD: &str = "group_folder_bindings";
fn invalid() -> AppError {
    AppError::invalid(
        "automation_folder_binding",
        "folder mapping changed, unavailable or unsupported; review again",
    )
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Binding {
    folder_id: Uuid,
    mode: Mode,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Direct,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    groups: BTreeMap<String, Binding>,
}
impl Default for Document {
    fn default() -> Self {
        Self {
            version: 1,
            groups: BTreeMap::new(),
        }
    }
}
fn document(row: &Value, inventory: &ProjectInventory) -> AppResult<Document> {
    let Some(raw) = row.get(FIELD) else {
        return Ok(Document::default());
    };
    if serde_json::to_vec(raw).map_err(AppError::internal)?.len() > 16 * 1024 {
        return Err(invalid());
    }
    let doc: Document = serde_json::from_value(raw.clone()).map_err(|_| invalid())?;
    if doc.version != 1
        || doc.groups.len() > 64
        || doc.groups.iter().any(|(name, b)| {
            b.folder_id.is_nil()
                || inventory
                    .groups
                    .get(name)
                    .is_none_or(|members| !members.is_empty())
        })
    {
        return Err(invalid());
    }
    Ok(doc)
}
pub(super) fn validate_row(row: &Value, inventory: &ProjectInventory) -> AppResult<()> {
    document(row, inventory).map(|_| ())
}
fn hash(value: &impl Serialize) -> AppResult<String> {
    Ok(hex::encode(Sha256::digest(
        serde_json::to_vec(value).map_err(AppError::internal)?,
    )))
}
/// Original profile/weak-unlock is held by AutomationReviewContext. This opaque
/// capture contains no resolver, SSH endpoint, password or execution authority.
#[derive(Clone, Debug)]
pub(crate) struct Composition {
    doc: Document,
    original: ProjectInventory,
    pub(crate) inventory: ProjectInventory,
    stamp: String,
    pub(crate) view: Value,
    metadata: FolderSnapshot,
    watch: Option<FolderWatch>,
}
fn resolve(u: &Unlocked, original: &ProjectInventory, doc: &Document) -> AppResult<Composition> {
    let ids = doc
        .groups
        .values()
        .map(|binding| crate::dto::parse_id("folder_id", &binding.folder_id.to_string()))
        .collect::<AppResult<std::collections::BTreeSet<_>>>()?;
    let metadata = u
        .working()
        .direct_folder_snapshot(&ids)
        .ok_or_else(invalid)?;
    let mut inventory = original.clone();
    let mut rows = BTreeMap::new();
    let mut names_by_folder: BTreeMap<ObjectId, Vec<String>> = BTreeMap::new();
    for (name, binding) in &doc.groups {
        let id = crate::dto::parse_id("folder_id", &binding.folder_id.to_string())?;
        let group = &metadata.groups[&id];
        names_by_folder.entry(id).or_default().push(name.clone());
        rows.insert(name.clone(),json!({"folder_id":binding.folder_id,"folder_name":group.name,"parent_id":group.parent_id,"folder_revision":group.revision,"mode":"direct","hosts":[]}));
        inventory.groups.insert(name.clone(), vec![]);
    }
    let mut members = 0usize;
    for (id, host) in &metadata.hosts {
        let uuid = Uuid::parse_str(&id.to_string()).map_err(|_| invalid())?;
        for name in &names_by_folder[&host.group_id] {
            members += 1;
            if members > 10_000 {
                return Err(invalid());
            }
            inventory.groups.get_mut(name).unwrap().push(uuid);
            rows.get_mut(name).unwrap()["hosts"]
                .as_array_mut()
                .unwrap()
                .push(json!({"host_id":uuid,"name":host.name,"revision":host.revision}));
        }
    }
    inventory.normalize().map_err(automation_error)?;
    inventory =
        ProjectInventory::parse(&serde_json::to_vec(&inventory).map_err(AppError::internal)?)
            .map_err(automation_error)?;
    let view =
        json!({"version":1,"groups":rows,"authority":"proposal_only","descendants_included":false});
    if serde_json::to_vec(&view).map_err(AppError::internal)?.len() > 3 * 1024 * 1024 {
        return Err(invalid());
    }
    let stamp = hash(&view)?;
    Ok(Composition {
        doc: doc.clone(),
        original: original.clone(),
        inventory,
        stamp,
        view,
        metadata,
        watch: None,
    })
}

impl Composition {
    pub(crate) fn check(&self, u: &Unlocked) -> AppResult<()> {
        let current = resolve(u, &self.original, &self.doc)?;
        if current.stamp != self.stamp || current.metadata != self.metadata {
            return Err(invalid());
        }
        Ok(())
    }
    pub(crate) fn watch(
        &mut self,
        u: &Unlocked,
        cancel: tokio_util::sync::CancellationToken,
    ) -> AppResult<()> {
        self.watch = Some(
            u.working()
                .watch_folder_snapshot(&self.metadata, cancel)
                .ok_or_else(invalid)?,
        );
        self.check(u)
    }
    pub(crate) fn finish(&self, u: &Unlocked) -> AppResult<()> {
        if !self
            .watch
            .as_ref()
            .is_some_and(|watch| u.working().finish_folder_review(watch))
        {
            return Err(invalid());
        }
        Ok(())
    }
}

/// Repeat both the selected action and available remaining live bindings.
/// SnapshotDirect removes its binding, so final composition alone is not a
/// fence for the folder whose membership was just copied.
fn check_action_compositions(u: &Unlocked, action: Option<&Composition>, remaining: Option<&Composition>) -> AppResult<()> {
    if let Some(action) = action { action.check(u)?; }
    if let Some(remaining) = remaining { remaining.check(u)?; }
    Ok(())
}

async fn load_document(app: &AppCore, context: &AutomationProjectContext) -> AppResult<Document> {
    let u = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
    let _profile = app.hold_session(&context.owner).await?;
    let _epoch = context.owner.hold_unlocked(&u).await?;
    let catalog: Catalog = context
        .owner
        .storage()?
        .setting_get(PROJECTS)
        .await?
        .unwrap_or_default();
    let row = catalog
        .records
        .get(&context.project.id)
        .ok_or_else(invalid)?;
    if catalog.version != 1
        || !managed::supported_at(context.owner.dir.as_deref(), &context.project.id, row)
        || row["revision"].as_u64() != context.project.revision
        || groups::from_row(row)? != context.inventory
    {
        return Err(invalid());
    }
    document(row, &context.inventory)
}
pub(crate) async fn capture(
    app: &AppCore,
    context: &AutomationProjectContext,
) -> AppResult<Option<Composition>> {
    let doc = load_document(app, context).await?;
    if doc.groups.is_empty() {
        return Ok(None);
    }
    let u = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
    let _profile = app.hold_session(&context.owner).await?;
    let _epoch = context.owner.hold_unlocked(&u).await?;
    Ok(Some(resolve(&u, &context.inventory, &doc)?))
}
pub(super) async fn describe(
    app: &AppCore,
    context: &AutomationProjectContext,
) -> AppResult<Value> {
    let doc = load_document(app, context).await?;
    let u = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
    let _profile = app.hold_session(&context.owner).await?;
    let _epoch = context.owner.hold_unlocked(&u).await?;
    // Missing folders can be explicitly unlinked; an unavailable composition is
    // NEVER a fallback for prepare/confirm, which use capture/check and fail.
    let composition = resolve(&u, &context.inventory, &doc).ok();
    Ok(
        json!({"bindings":doc,"composition":composition.as_ref().map(|c|&c.view),"status":if composition.is_some(){"available"}else{"unavailable"}}),
    )
}
/// Caller ALREADY holds original session/unlock guards. Repeat SQL after any
/// probe/auth await; WorkingSet watches alone do not cover project metadata.
pub(crate) async fn confirm_metadata(
    owner: &crate::session::Session,
    project: &str,
    revision: u64,
    composition: Option<&Composition>,
) -> AppResult<()> {
    let catalog: Catalog = owner
        .storage()?
        .setting_get(PROJECTS)
        .await?
        .unwrap_or_default();
    let row = catalog.records.get(project).ok_or_else(invalid)?;
    if catalog.version != 1 || !managed::supported_at(owner.dir.as_deref(), project, row) || row["revision"] != json!(revision) {
        return Err(invalid());
    }
    let inventory = groups::from_row(row)?;
    let current = document(row, &inventory)?;
    match composition {
        Some(c) if current == c.doc && inventory == c.original => Ok(()),
        None if current.groups.is_empty() => Ok(()),
        _ => Err(invalid()),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    version: u32,
    project_id: String,
    expected_revision: u64,
    profile_id: String,
    notification_scope_id: Uuid,
    action: Action,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Action {
    LinkDirect { name: String, folder_id: Uuid },
    SnapshotDirect { name: String, folder_id: Uuid },
    Unlink { name: String },
}
impl AppCore {
    /// Typed explicit metadata action. Never contacts a Runner/host or resolves a
    /// credential. Snapshot is a copied membership, live link stays a proposal.
    pub async fn automation_project_folder_binding_json(
        &self,
        payload: String,
    ) -> AppResult<Value> {
        if payload.len() > 16 * 1024 {
            return Err(invalid());
        }
        // Deserialize the strict typed schema directly: an intermediate Value
        // would discard duplicate object keys before the Request visitor.
        let request: Request = serde_json::from_str(&payload).map_err(|_| invalid())?;
        if request.version != 1
            || request.expected_revision == 0
            || request.expected_revision == u64::MAX
        {
            return Err(invalid());
        }
        project_id(&request.project_id)?;
        let (owner, u) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _epoch = owner.hold_unlocked(&u).await?;
        if owner.profile_id.to_string() != request.profile_id
            || u.notices.scope_id() != Some(request.notification_scope_id.to_string())
        {
            return Err(invalid());
        }
        let captured = u.clone();
        let profile_path = owner.dir.clone();
        owner.storage()?.write(move|tx| {
   let mut catalog:Catalog=tx.setting_get(PROJECTS)?.unwrap_or_default();
   if catalog.version!=1||!catalog.extra.is_empty(){return Err(invalid());}
   let row=catalog.records.get_mut(&request.project_id).ok_or_else(invalid)?;
   if !managed::supported_at(profile_path.as_deref(), &request.project_id, row)||row["revision"]!=json!(request.expected_revision){return Err(invalid());}
   let mut inventory=groups::from_row(row)?;let mut doc=document(row,&inventory)?;
   let snapshot_action=matches!(&request.action,Action::SnapshotDirect{..});
   let mut action_composition=None;
   match request.action {
    Action::Unlink{name}=>{if doc.groups.remove(&name).is_none(){return Err(invalid());}}
    Action::LinkDirect{name,folder_id}|Action::SnapshotDirect{name,folder_id}=>{
     inventory.groups.insert(name.clone(),vec![]);inventory.normalize().map_err(automation_error)?;
     let binding=Binding{folder_id,mode:Mode::Direct};
     let proposed=Document{version:1,groups:BTreeMap::from([(name.clone(),binding.clone())])};
     let resolved=resolve(&captured,&inventory,&proposed)?;
     if snapshot_action {inventory.groups.insert(name.clone(),resolved.inventory.groups[&name].clone());doc.groups.remove(&name);}
     else {doc.groups.insert(name,binding);}
     action_composition=Some(resolved);

    }
   }
   inventory.normalize().map_err(automation_error)?;
   inventory=ProjectInventory::parse(&serde_json::to_vec(&inventory).map_err(AppError::internal)?).map_err(automation_error)?;
   document(&json!({"group_folder_bindings":doc}),&inventory)?;
   let composition=resolve(&captured,&inventory,&doc).ok();
   check_action_compositions(&captured,action_composition.as_ref(),composition.as_ref())?;
   if doc.groups.is_empty(){row.as_object_mut().ok_or_else(invalid)?.remove(FIELD);}else{row[FIELD]=serde_json::to_value(&doc).map_err(AppError::internal)?;}
   row["inventory"]=serde_json::to_value(inventory).map_err(AppError::internal)?;
   row["revision"]=json!(request.expected_revision+1);row["updated_at"]=json!(chrono::Utc::now().timestamp());
   let result=json!({"version":1,"project":summary(&request.project_id,row,false),"inventory":row["inventory"],"folder_bindings":doc,"composition":composition.as_ref().map(|c|&c.view),"composition_status":if composition.is_some(){"available"}else{"unavailable"}});
   tx.setting_set(PROJECTS,&catalog)?;Ok(result)
  }).await
    }
}
#[cfg(test)]
mod tests;
