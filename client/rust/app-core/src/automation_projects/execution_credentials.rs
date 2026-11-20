//! Profile/project-local SSH reference selection. No secret copies or resolver.
use super::*;
use cc_models::credential::CredentialKind;
use cc_models::{ObjectId, ObjectKind, VaultObject};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};

const FIELD: &str = "execution_credentials";
const MAX_BYTES: usize = 64 * 1024;
fn invalid() -> AppError {
    AppError::invalid(
        "automation_credentials",
        "project_environment_selection_changed",
    )
}
fn id(raw: &str) -> AppResult<Uuid> {
    let id = Uuid::parse_str(raw).map_err(|_| invalid())?;
    if id.is_nil() || id.to_string() != raw {
        return Err(invalid());
    }
    Ok(id)
}
fn hash(value: &Value) -> AppResult<String> {
    Ok(
        Sha256::digest(serde_json::to_vec(value).map_err(|_| invalid())?)
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect(),
    )
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CredentialOverride {
    pub host_id: String,
    pub credential_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Environment {
    pub id: String,
    pub name: String,
    pub production: bool,
    pub overrides: Vec<CredentialOverride>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Policy {
    pub version: u8,
    pub environments: Vec<Environment>,
}
impl Policy {
    fn validate(&self) -> AppResult<()> {
        if self.version != 1 || self.environments.is_empty() || self.environments.len() > 16 {
            return Err(invalid());
        }
        if serde_json::to_vec(self).map_err(|_| invalid())?.len() > MAX_BYTES {
            return Err(invalid());
        }
        let mut environments = BTreeSet::new();
        for environment in &self.environments {
            id(&environment.id)?;
            if !environments.insert(&environment.id)
                || valid_name(&environment.name)? != environment.name
                || environment.overrides.is_empty()
                || environment.overrides.len() > 256
            {
                return Err(invalid());
            }
            let mut hosts = BTreeSet::new();
            for selected in &environment.overrides {
                id(&selected.host_id)?;
                id(&selected.credential_id)?;
                if !hosts.insert(&selected.host_id) {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }
}
pub(crate) fn from_row(row: &Value) -> AppResult<Option<Policy>> {
    if row
        .pointer("/source/status")
        .is_some_and(|value| value.as_str() != Some("ready"))
    {
        return Err(AppError::invalid(
            "automation_credentials",
            "project_environment_schema_read_only",
        ));
    }
    match (row.get("version").and_then(Value::as_u64), row.get(FIELD)) {
        (Some(1), None) => Ok(None),
        (Some(2), Some(raw)) => {
            if serde_json::to_vec(raw).map_err(|_| invalid())?.len() > MAX_BYTES {
                return Err(invalid());
            }
            let policy: Policy = serde_json::from_value(raw.clone()).map_err(|_| invalid())?;
            policy.validate()?;
            if serde_json::to_value(&policy).map_err(|_| invalid())? != *raw {
                return Err(invalid());
            }
            Ok(Some(policy))
        }
        _ => Err(AppError::invalid(
            "automation_credentials",
            "project_environment_schema_read_only",
        )),
    }
}
pub(crate) fn supported_row_version(row: &Value) -> bool {
    from_row(row).is_ok()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub version: u8,
    pub profile_id: String,
    pub notification_scope_id: String,
    pub project_id: String,
    pub project_revision: u64,
    pub policy_sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub version: u8,
    pub frame: Frame,
    pub environment_id: String,
}
impl Selection {
    pub(crate) fn validate(&self) -> AppResult<()> {
        self.frame.validate()?;
        id(&self.environment_id)?;
        if self.version != 1 {
            return Err(invalid());
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Save {
    version: u8,
    frame: Frame,
    policy: Option<Policy>,
}
impl Frame {
    fn validate(&self) -> AppResult<()> {
        id(&self.profile_id)?;
        id(&self.project_id)?;
        if self.notification_scope_id.is_empty()
            || self.notification_scope_id.len() > 256
            || self.notification_scope_id.chars().any(char::is_control)
        {
            return Err(invalid());
        }
        if self.version != 1
            || !(1..=9_007_199_254_740_991).contains(&self.project_revision)
            || !cc_models::automation::is_digest(&self.policy_sha256)
        {
            return Err(invalid());
        }
        Ok(())
    }
    fn check(
        &self,
        context: &AutomationProjectContext,
        u: &crate::session::Unlocked,
        row: &Value,
    ) -> AppResult<()> {
        self.validate()?;
        if self.profile_id != context.owner.profile_id.to_string()
            || u.notices.scope_id().as_deref() != Some(self.notification_scope_id.as_str())
            || self.project_id != context.project.id
            || Some(self.project_revision) != context.project.revision
            || row["revision"] != json!(self.project_revision)
            || self.policy_sha256 != hash(row.get(FIELD).unwrap_or(&Value::Null))?
        {
            return Err(invalid());
        }
        Ok(())
    }
}
async fn original_row(context: &AutomationProjectContext) -> AppResult<Value> {
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
        || !catalog.extra.is_empty()
        || !managed::supported_at(context.owner.dir.as_deref(), &context.project.id, row)
        || managed::row_stamp(row)? != context.catalog_row_sha256
    {
        return Err(invalid());
    }
    Ok(row.clone())
}
fn frame(
    context: &AutomationProjectContext,
    u: &crate::session::Unlocked,
    row: &Value,
) -> AppResult<Frame> {
    Ok(Frame {
        version: 1,
        profile_id: context.owner.profile_id.to_string(),
        notification_scope_id: u.notices.scope_id().ok_or(AppError::VaultLocked)?,
        project_id: context.project.id.clone(),
        project_revision: context.project.revision.ok_or_else(invalid)?,
        policy_sha256: hash(row.get(FIELD).unwrap_or(&Value::Null))?,
    })
}
fn validate_refs(u: &crate::session::Unlocked, policy: &Policy) -> AppResult<()> {
    for environment in &policy.environments {
        for reference in &environment.overrides {
            let host = crate::dto::parse_id("host_id", &reference.host_id)?;
            let host_value = crate::inventory::host_of(u, host)?;
            let credential = crate::dto::parse_id("credential_id", &reference.credential_id)?;
            let value = u.working().credential(credential).ok_or_else(invalid)?;
            if u.working().vault_of(host).is_some()
                || u.working().vault_of(credential).is_some()
                || crate::sharing_bindings::host_sharing_binding(&host_value)?.is_some()
            {
                return Err(AppError::invalid(
                    "automation_credentials",
                    "project_environment_shared_override_unsupported",
                ));
            }
            if !matches!(
                value.kind,
                CredentialKind::Password
                    | CredentialKind::SshPrivateKey
                    | CredentialKind::SshCertificate
            ) {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

impl AppCore {
    pub async fn automation_project_credentials_context(&self, id: String) -> AppResult<Value> {
        let context = self.automation_project_context(&id).await?;
        let row = original_row(&context).await?;
        let policy = from_row(&row)?;
        let u = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = self.hold_session(&context.owner).await?;
        let _epoch = context.owner.hold_unlocked(&u).await?;
        Ok(
            json!({"version":1,"frame":frame(&context,&u,&row)?,"policy":policy,
            "supported_credential_kinds":["password","ssh_private_key","ssh_certificate"]}),
        )
    }
    pub async fn automation_project_credentials_save(&self, input: String) -> AppResult<Value> {
        if input.len() > MAX_BYTES + 2048 {
            return Err(invalid());
        }
        let request: Save = serde_json::from_str(&input).map_err(|_| invalid())?;
        request.frame.validate()?;
        if request.version != 1 {
            return Err(invalid());
        }
        if let Some(policy) = &request.policy {
            policy.validate()?;
        }
        let (original_owner, original_unlock) = self.unlocked().await?;
        if request.frame.profile_id != original_owner.profile_id.to_string()
            || original_unlock.notices.scope_id().as_deref()
                != Some(request.frame.notification_scope_id.as_str())
        {
            return Err(AppError::VaultLocked);
        }
        let original = Arc::downgrade(&original_unlock);
        drop(original_unlock);
        let context = self
            .automation_project_context_original(
                &request.frame.project_id,
                original_owner,
                original,
            )
            .await?;
        // SAME existing prepare semaphore, not a snapshot of available slots.
        let _prepares = context
            .owner
            .automation
            .prepare_slots
            .clone()
            .try_acquire_many_owned(crate::automation_runners::PREPARE_LIMIT)
            .map_err(|_| {
                AppError::invalid("automation_credentials", "project_environment_prepare_busy")
            })?;
        let row = original_row(&context).await?;
        let u = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        request.frame.check(&context, &u, &row)?;
        if let Some(policy) = &request.policy {
            validate_refs(&u, policy)?;
        }
        let _profile = self.hold_session(&context.owner).await?;
        let _epoch = context.owner.hold_unlocked(&u).await?;
        let owner = context.owner.clone();
        let path = owner.dir.clone();
        let project = context.project.id.clone();
        let expected_stamp = context.catalog_row_sha256.clone();
        let expected_revision = request.frame.project_revision;
        let policy = request
            .policy
            .map(serde_json::to_value)
            .transpose()
            .map_err(|_| invalid())?;
        let saved = owner
            .storage()?
            .write(move |tx| {
                let mut catalog: Catalog = tx.setting_get(PROJECTS)?.unwrap_or_default();
                if catalog.version != 1 || !catalog.extra.is_empty() {
                    return Err(invalid());
                }
                let row = catalog.records.get_mut(&project).ok_or_else(invalid)?;
                if !managed::supported_at(path.as_deref(), &project, row)
                    || managed::row_stamp(row)? != expected_stamp
                    || row["revision"] != json!(expected_revision)
                    || expected_revision >= 9_007_199_254_740_991
                {
                    return Err(invalid());
                }
                if let Some(policy) = policy {
                    row[FIELD] = policy;
                    row["version"] = json!(2);
                } else {
                    row.as_object_mut().ok_or_else(invalid)?.remove(FIELD);
                    row["version"] = json!(1);
                }
                row["revision"] = json!(expected_revision + 1);
                row["updated_at"] = json!(chrono::Utc::now().timestamp());
                let result = row.clone();
                tx.setting_set(PROJECTS, &catalog)?;
                Ok::<_, AppError>(result)
            })
            .await?;
        // Retire only this original owner's prepared reviews. Already admitted
        // jobs have their separate original signed TTL; no authority is reset.
        let dropped = {
            let mut reviews = owner
                .automation
                .reviews
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let ids = reviews
                .iter()
                .filter_map(|(id, r)| (r.context.project_id == context.project.id).then_some(*id))
                .collect::<Vec<_>>();
            ids.into_iter()
                .filter_map(|id| reviews.remove(&id))
                .collect::<Vec<_>>()
        };
        drop(dropped);
        let next_frame = Frame {
            project_revision: expected_revision + 1,
            policy_sha256: hash(saved.get(FIELD).unwrap_or(&Value::Null))?,
            ..request.frame
        };
        Ok(
            json!({"version":1,"project":summary(&context.project.id,&saved,false),"frame":next_frame,"policy":from_row(&saved)?}),
        )
    }
}

#[derive(Clone)]
pub(crate) struct Frozen {
    pub selection: Selection,
    pub environment: Environment,
    pub credentials: HashMap<ObjectId, ObjectId>,
    pub revisions: BTreeMap<ObjectId, i64>,
    pub mutation: u64,
    pub plans: HashMap<ObjectId, cc_ssh_core::ConnectionPlan>,
}
impl Frozen {
    pub(crate) fn current(&self, u: &crate::session::Unlocked) -> bool {
        u.notices.scope_id().as_deref() == Some(self.selection.frame.notification_scope_id.as_str())
            && u.working().mutation_stamp() == self.mutation
    }
}
pub(crate) async fn freeze(
    app: &AppCore,
    context: &AutomationProjectContext,
    selection: Option<&Selection>,
    host_ids: &[String],
) -> AppResult<Option<Frozen>> {
    let row = original_row(context).await?;
    let policy = from_row(&row)?;
    let Some(policy) = policy else {
        if selection.is_some() {
            return Err(invalid());
        }
        return Ok(None);
    };
    let selection = selection.ok_or_else(|| {
        AppError::invalid(
            "automation_credentials",
            "project_environment_selection_required",
        )
    })?;
    selection.validate()?;
    let u = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
    let _profile = app.hold_session(&context.owner).await?;
    let _epoch = context.owner.hold_unlocked(&u).await?;
    selection.frame.check(context, &u, &row)?;
    let mut environment = policy
        .environments
        .into_iter()
        .find(|v| v.id == selection.environment_id)
        .ok_or_else(invalid)?;
    let mutation = u.working().mutation_stamp();
    let snapshot = u.working().snapshot();
    let all = environment
        .overrides
        .iter()
        .map(|v| {
            Ok((
                crate::dto::parse_id("host_id", &v.host_id)?,
                crate::dto::parse_id("credential_id", &v.credential_id)?,
            ))
        })
        .collect::<AppResult<HashMap<_, _>>>()?;
    let mut credentials = HashMap::new();
    let mut plans = HashMap::new();
    for raw in host_ids {
        let host = crate::dto::parse_id("host_id", raw)?;
        // Same original planner, before any auth/material/target network I/O.
        let plan = u
            .ssh
            .planner
            .plan_with_credentials(host, &all)
            .await
            .map_err(|error| match error {
                cc_ssh_core::PlanError::MissingProjectCredential(_)
                | cc_ssh_core::PlanError::MissingCredential(_) => AppError::invalid(
                    "automation_credentials",
                    "project_environment_credential_missing",
                ),
                other => AppError::from(other),
            })?;
        for hop in plan.all_hops() {
            let selected = *all.get(&hop.host_id).ok_or_else(invalid)?;
            let host_value = crate::inventory::host_of(&u, hop.host_id)?;
            let value = u.working().credential(selected).ok_or_else(invalid)?;
            if u.working().vault_of(hop.host_id).is_some()
                || u.working().vault_of(selected).is_some()
                || crate::sharing_bindings::host_sharing_binding(&host_value)?.is_some()
            {
                return Err(AppError::invalid(
                    "automation_credentials",
                    "project_environment_shared_override_unsupported",
                ));
            }
            if !matches!(
                value.kind,
                CredentialKind::Password
                    | CredentialKind::SshPrivateKey
                    | CredentialKind::SshCertificate
            ) || hop.credential.as_ref().map(|v| v.id) != Some(selected)
            {
                return Err(invalid());
            }
            credentials.insert(hop.host_id, selected);
        }
        plans.insert(host, plan);
    }
    // Reuse the existing dependency walker over a metadata-only projection.
    // Global Host/group credential fallback is irrelevant with explicit refs;
    // original proxy refs and route/group/jump metadata remain unchanged.
    let mut projected = snapshot.clone();
    for entry in projected.values_mut() {
        if let crate::working_set::Item::Object(object) = &mut entry.item {
            match object.as_mut() {
                VaultObject::Host(host) => host.credential_id = credentials.get(&host.id).copied(),
                VaultObject::Group(group) => group.inherited_credential_id = None,
                _ => {}
            }
        }
    }
    let mut revisions = BTreeMap::new();
    for raw in host_ids {
        revisions.extend(crate::kubernetes_runtime::dependencies(
            &projected,
            crate::dto::parse_id("host_id", raw)?,
        )?);
    }
    for id in credentials.values() {
        revisions.extend(crate::kubernetes_runtime::dependencies(&snapshot, *id)?);
    }
    if u.working().mutation_stamp() != mutation {
        return Err(invalid());
    }
    environment.overrides.retain(|v| {
        crate::dto::parse_id("host_id", &v.host_id).is_ok_and(|id| credentials.contains_key(&id))
    });
    Ok(Some(Frozen {
        selection: selection.clone(),
        environment,
        credentials,
        revisions,
        mutation,
        plans,
    }))
}

/// The only new risk input is original saved environment metadata. Existing
/// Native target/jump/group classification is ORed, never replaced by caller data.
pub(crate) fn production(
    selected: Option<&Frozen>,
    snapshot: &BTreeMap<ObjectId, crate::working_set::Entry>,
    watched: &BTreeMap<ObjectId, i64>,
) -> bool {
    selected.is_some_and(|v| v.environment.production)
        || watched.keys().any(|id| {
            snapshot.get(id).is_some_and(|entry| {
                let crate::working_set::Item::Object(object) = &entry.item else {
                    return false;
                };
                let tags = match object.as_ref() {
                    VaultObject::Host(v) => Some(&v.tags),
                    VaultObject::Group(v) => Some(&v.tags),
                    _ => None,
                };
                tags.is_some_and(|tags| {
                    tags.iter()
                        .any(|v| matches!(v.to_ascii_lowercase().as_str(), "prod" | "production"))
                })
            })
        })
}

#[cfg(test)]
mod tests;
