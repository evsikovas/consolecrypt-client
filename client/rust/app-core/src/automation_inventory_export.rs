//! Explicit static DATA export. No credential resolution, connection or job.
//! The provenance is informational, never a signature or access capability.
use crate::{AppCore, AppError, AppResult};
use cc_automation_core::{InventoryMode, ProjectInventory};
use cc_ssh_core::ConnectionPlan;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use uuid::Uuid;

const MAX_SNAPSHOT_BYTES: usize = 256 * 1024;

fn invalid(code: &'static str) -> AppError {
    AppError::invalid("automation_inventory_export", code)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    version: u32,
    project_id: String,
    expected_revision: u64,
    host_ids: Vec<String>,
    group_names: Vec<String>,
}
fn id(value: &str) -> AppResult<Uuid> {
    Uuid::parse_str(value)
        .ok()
        .filter(|id| !id.is_nil() && id.hyphenated().to_string() == value)
        .ok_or_else(|| invalid("inventory_export_invalid_request"))
}
fn name(value: &str) -> bool {
    let mut bytes = value.bytes();
    value.len() <= 96
        && bytes
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        && !matches!(value, "all" | "ungrouped" | "_meta")
}
fn parse(raw: &str) -> AppResult<Request> {
    if raw.len() > 16 * 1024 {
        return Err(invalid("inventory_export_invalid_request"));
    }
    // Deserialize directly into the strict struct: Serde rejects duplicates of
    // every known field, rather than silently keeping the final Value entry.
    let value: Request =
        serde_json::from_str(raw).map_err(|_| invalid("inventory_export_invalid_request"))?;
    if value.version != 1
        || value.expected_revision == 0
        || value.host_ids.is_empty()
        || value.host_ids.len() > 32
        || value.group_names.len() > 64
        || value.host_ids.iter().collect::<BTreeSet<_>>().len() != value.host_ids.len()
        || value.group_names.iter().collect::<BTreeSet<_>>().len() != value.group_names.len()
        || value.group_names.iter().any(|value| !name(value))
    {
        return Err(invalid("inventory_export_invalid_request"));
    }
    id(&value.project_id)?;
    for host in &value.host_ids {
        id(host)?;
    }
    Ok(value)
}
fn endpoint(address: &str, user: &str) -> bool {
    let dns = !address.is_empty()
        && address.len() <= 253
        && address
            .strip_suffix('.')
            .unwrap_or(address)
            .split('.')
            .all(|part| {
                !part.is_empty()
                    && part.len() <= 63
                    && part.as_bytes()[0].is_ascii_alphanumeric()
                    && part.as_bytes()[part.len() - 1].is_ascii_alphanumeric()
                    && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            });
    let address = address.len() <= 253 && (address.parse::<IpAddr>().is_ok() || dns);
    let user = !user.is_empty()
        && user.len() <= 128
        && !user.starts_with('-')
        && user
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.@\\-".contains(&b));
    address && user
}
fn alias(id: Uuid) -> String {
    format!("cc_{}", id.simple())
}

/// Pure projection of metadata already resolved by the existing planner.
/// Do not serialize the plan: it also contains credential references, tunnel
/// settings and other fields that do not belong in an ordinary inventory.
struct ExportHost {
    id: Uuid,
    alias: String,
    address: String,
    port: u16,
    username: String,
}
fn project(plan: &ConnectionPlan) -> AppResult<ExportHost> {
    if !plan.route.is_empty()
        || plan.proxy.is_some()
        || !plan.forwards.is_empty()
        || plan.agent_forwarding
        || !plan.warnings.is_empty()
    {
        return Err(invalid("inventory_export_unsupported_route"));
    }
    let host_id = id(&plan.host_id.to_string())?;
    if !endpoint(&plan.target.host, &plan.username) || plan.target.port == 0 {
        return Err(invalid("inventory_export_unsupported_endpoint"));
    }
    Ok(ExportHost {
        id: host_id,
        alias: alias(host_id),
        address: plan.target.host.clone(),
        port: plan.target.port,
        username: plan.username.clone(),
    })
}

fn standard_inventory(
    inventory: &ProjectInventory,
    names: &[String],
    targets: &[ExportHost],
) -> AppResult<Value> {
    let requested = names.iter().cloned().collect::<BTreeSet<_>>();
    if requested
        .iter()
        .any(|name| !inventory.groups.contains_key(name))
    {
        return Err(invalid("inventory_export_unknown_group"));
    }
    let filtered = ProjectInventory {
        groups: inventory
            .groups
            .iter()
            .filter(|(name, _)| requested.contains(*name))
            .map(|(name, members)| (name.clone(), members.clone()))
            .collect::<BTreeMap<_, _>>(),
        ..inventory.clone()
    };
    let selected = targets
        .iter()
        .map(|host| (host.id, host.alias.as_str()))
        .collect::<Vec<_>>();
    let groups = filtered
        .runtime_groups_for_hosts(&selected)
        .map_err(|_| invalid("inventory_export_invalid_groups"))?;
    let mut result = groups
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("inventory_export_invalid_groups"))?;
    let mut hostvars = Map::new();
    for target in targets {
        if result.contains_key(&target.alias) || hostvars.contains_key(&target.alias) {
            return Err(invalid("inventory_export_alias_conflict"));
        }
        hostvars.insert(target.alias.clone(), json!({
            "ansible_host":target.address,"ansible_port":target.port,"ansible_user":target.username
        }));
    }
    let hosts = hostvars.keys().cloned().collect::<Vec<_>>();
    let grouped = result
        .values()
        .filter_map(|row| row["hosts"].as_array())
        .flatten()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    let ungrouped = hosts
        .iter()
        .filter(|host| !grouped.contains(host.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    result.insert(
        "all".into(),
        json!({"hosts":hosts,"children":requested,"vars":{}}),
    );
    result.insert(
        "ungrouped".into(),
        json!({"hosts":ungrouped,"children":[],"vars":{}}),
    );
    result.insert("_meta".into(), json!({"hostvars":hostvars}));
    Ok(Value::Object(result))
}

impl AppCore {
    /// Issue an explicitly selected static snapshot. The caller may save these
    /// credential-free bytes as an ordinary file; this does not authorize any
    /// external execution, credential access, native SSH route or job.
    pub async fn automation_inventory_export_issue(
        &self,
        request_json: String,
    ) -> AppResult<Value> {
        let request = parse(&request_json)?;
        let context = self.automation_project_context(&request.project_id).await?;
        let original = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let version = {
            let _profile = self.hold_session(&context.owner).await?;
            let _epoch = context.owner.hold_unlocked(&original).await?;
            if context.project.revision != Some(request.expected_revision)
                || context
                    .external_inventory
                    .as_ref()
                    .is_some_and(|config| config.mode == InventoryMode::ExternalOnly)
            {
                return Err(invalid("inventory_export_context_changed"));
            }
            original.working().version()
        };
        let mut targets = Vec::with_capacity(request.host_ids.len());
        for value in &request.host_ids {
            let host = id(value)?;
            if !context
                .inventory
                .permits(host, context.external_inventory.as_ref())
            {
                return Err(invalid("inventory_export_host_not_selected"));
            }
            // Only metadata lookup; no connector, VaultCredentialResolver,
            // secret reader, SSH pin prompt, helper process or network request.
            let native_id = crate::dto::parse_id("host_id", value)?;
            let metadata = crate::inventory::host_of(&original, native_id)?;
            // Imported ProxyCommand metadata is not represented in the plan.
            if metadata.proxy_command.is_some()
                || metadata.metadata.contains_key("ssh_config.proxy_command")
            {
                return Err(invalid("inventory_export_unsupported_route"));
            }
            let plan = original.ssh.plan(native_id).await?;
            targets.push(project(&plan)?);
        }
        targets.sort_by(|left, right| left.alias.cmp(&right.alias));
        let inventory = standard_inventory(&context.inventory, &request.group_names, &targets)?;
        self.finish_inventory_export(context, original, request, version, inventory)
            .await
    }

    async fn finish_inventory_export(
        &self,
        context: crate::automation_projects::AutomationProjectContext,
        original: std::sync::Arc<crate::session::Unlocked>,
        request: Request,
        version: u64,
        inventory: Value,
    ) -> AppResult<Value> {
        let _profile = self.hold_session(&context.owner).await?;
        let _epoch = context.owner.hold_unlocked(&original).await?;
        if !context
            .matches_inventory_export(request.expected_revision)
            .await?
            || original.working().version() != version
        {
            return Err(invalid("inventory_export_context_changed"));
        }
        let mut host_ids = request.host_ids;
        let mut group_names = request.group_names;
        host_ids.sort();
        group_names.sort();
        let result = json!({
            "format":"consolecrypt.inventory.snapshot","version":1,"snapshotId":Uuid::new_v4(),
            "issuedAtUnixSeconds":chrono::Utc::now().timestamp(),
            "origin":{"kind":"native","projectId":request.project_id,
                "projectRevision":request.expected_revision,"workingVersion":version},
            "selection":{"hostIds":host_ids,"groupNames":group_names},"inventory":inventory
        });
        if serde_json::to_vec(&result)
            .map_err(AppError::internal)?
            .len()
            > MAX_SNAPSHOT_BYTES
        {
            return Err(invalid("inventory_export_size"));
        }
        Ok(result)
    }
}

#[cfg(test)]
#[path = "automation_inventory_export_tests.rs"]
mod tests;
