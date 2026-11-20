//! Static sources carry data, never host/credential authority. Call the async
//! transfer from a bounded native worker; never decode a large source in Dart.
use crate::{
    validate_project_path, AutomationError, HelperPeer, ProjectEntry, ProjectRevision, Result,
    UploadedRevision,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const MAX_BYTES: usize = 5 * 1024 * 1024;
fn invalid() -> AutomationError {
    AutomationError::Protocol("invalid or changed static inventory preview")
}
fn name(value: &str) -> bool {
    let mut bytes = value.bytes();
    value.len() <= 96
        && bytes
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InventoryFormat {
    Ini,
    Yaml,
    Directory,
    #[serde(rename = "group_vars")]
    GroupVars,
    #[serde(rename = "host_vars")]
    HostVars,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InventorySource {
    pub path: String,
    pub format: InventoryFormat,
}

/// Resolve directory selection from captured metadata, never a fresh OS walk.
/// The resulting regular files are independently hashed during upload/read.
pub fn resolve_inventory_sources(
    revision: &ProjectRevision,
    selections: &[InventorySource],
) -> Result<Vec<InventorySource>> {
    if selections.is_empty() || selections.len() > 8 {
        return Err(invalid());
    }
    let entries = revision
        .entries
        .iter()
        .map(|e| (e.path(), e))
        .collect::<BTreeMap<_, _>>();
    let mut files = BTreeMap::new();
    let mut selected = BTreeSet::new();
    for selection in selections {
        validate_project_path(&selection.path)?;
        if matches!(
            selection.format,
            InventoryFormat::GroupVars | InventoryFormat::HostVars
        ) {
            return Err(invalid());
        }
        if !selected.insert(&selection.path) {
            return Err(invalid());
        }
        if selection.format != InventoryFormat::Directory {
            if let Some(previous) = files.insert(selection.path.clone(), selection.format) {
                if previous != selection.format {
                    return Err(invalid());
                }
            }
            continue;
        }
        if !matches!(
            entries.get(selection.path.as_str()),
            Some(ProjectEntry::Directory { .. })
        ) {
            return Err(invalid());
        }
        let prefix = format!("{}/", selection.path);
        let mut found = false;
        for entry in &revision.entries {
            let Some(relative) = entry.path().strip_prefix(&prefix) else {
                continue;
            };
            let parts = relative.split('/').collect::<Vec<_>>();
            if parts.iter().any(|p| p.starts_with('.') || p.ends_with('~')) {
                continue;
            }
            let ProjectEntry::File { path, mode, .. } = entry else {
                continue;
            };
            let full_parts = path.split('/').collect::<Vec<_>>();
            let vars = full_parts
                .iter()
                .find(|p| matches!(**p, "group_vars" | "host_vars"));
            if vars.is_none()
                && parts
                    .last()
                    .is_some_and(|p| p.to_ascii_uppercase().starts_with("README"))
            {
                continue;
            }
            let extension = std::path::Path::new(path)
                .extension()
                .and_then(|v| v.to_str())
                .map(str::to_ascii_lowercase);
            let format = if let Some(scope) = vars {
                if extension
                    .as_deref()
                    .is_some_and(|v| !matches!(v, "yaml" | "yml" | "json"))
                {
                    continue;
                }
                if *scope == "group_vars" {
                    InventoryFormat::GroupVars
                } else {
                    InventoryFormat::HostVars
                }
            } else if extension.is_none() || extension.as_deref() == Some("ini") {
                InventoryFormat::Ini
            } else if matches!(extension.as_deref(), Some("yaml" | "yml" | "json")) {
                InventoryFormat::Yaml
            } else {
                continue;
            };
            if *mode != 0o600 {
                return Err(invalid());
            }
            if let Some(previous) = files.insert(path.clone(), format) {
                if previous != format {
                    return Err(invalid());
                }
            }
            found = true;
            if files.len() > 128 {
                return Err(invalid());
            }
        }
        if !found {
            return Err(invalid());
        }
    }
    if files.is_empty() || files.len() > 128 {
        return Err(invalid());
    }
    for path in files.keys() {
        if !matches!(
            entries.get(path.as_str()),
            Some(ProjectEntry::File { mode: 0o600, .. })
        ) {
            return Err(invalid());
        }
    }
    Ok(files
        .into_iter()
        .map(|(path, format)| InventorySource { path, format })
        .collect())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceInventoryHost {
    pub alias: String,
    pub host_id: Option<Uuid>,
    pub address: String,
    pub port: u16,
    pub username: Option<String>,
    pub variables: BTreeMap<String, Value>,
    pub sources: Vec<String>,
    pub credential_refs: Vec<Uuid>,
}
impl std::fmt::Debug for SourceInventoryHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceInventoryHost")
            .field("alias", &self.alias)
            .finish_non_exhaustive()
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceInventoryGroup {
    pub hosts: Vec<String>,
    pub children: Vec<String>,
    pub vars: BTreeMap<String, Value>,
}
impl std::fmt::Debug for SourceInventoryGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceInventoryGroup")
            .field("host_count", &self.hosts.len())
            .finish_non_exhaustive()
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceInventory {
    pub version: u32,
    pub hosts: Vec<SourceInventoryHost>,
    pub groups: BTreeMap<String, SourceInventoryGroup>,
    pub source_digests: BTreeMap<String, String>,
}
impl std::fmt::Debug for SourceInventory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceInventory")
            .field("host_count", &self.hosts.len())
            .field("group_count", &self.groups.len())
            .finish_non_exhaustive()
    }
}
fn variables(values: &BTreeMap<String, Value>) -> Result<()> {
    fn numbers(value: &Value) -> Result<()> {
        match value {
            Value::Number(number) if number.is_u64() && number.as_i64().is_none() => {
                return Err(invalid())
            }
            Value::Array(values) => {
                for value in values {
                    numbers(value)?;
                }
            }
            Value::Object(values) => {
                for value in values.values() {
                    numbers(value)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    for (key, value) in values {
        if key.chars().count() > 128
            || key.starts_with("consolecrypt_")
            || key.starts_with("ansible_")
                && !matches!(
                    key.as_str(),
                    "ansible_shell_type" | "ansible_python_interpreter"
                )
        {
            return Err(invalid());
        }
        numbers(value)?;
    }
    Ok(())
}
impl SourceInventory {
    fn validate_source(&self, expected: &BTreeMap<String, String>) -> Result<()> {
        if self.version != 1
            || self.hosts.len() > 10000
            || self.groups.len() > 1000
            || self.source_digests != *expected
        {
            return Err(invalid());
        }
        let mut aliases = BTreeSet::new();
        let mut previous = None;
        for host in &self.hosts {
            if !name(&host.alias)
                || matches!(host.alias.as_str(), "all" | "ungrouped" | "_meta")
                || previous.is_some_and(|value: &str| value >= host.alias.as_str())
                || host.host_id.is_some()
                || !host.credential_refs.is_empty()
                || host.address.is_empty()
                || host.address.chars().count() > 253
                || host.address.starts_with('-')
                || host.port == 0
                || host
                    .address
                    .chars()
                    .any(|c| !c.is_alphanumeric() && !"_.:[]%-".contains(c))
                || host.username.as_ref().is_some_and(|user| {
                    user.is_empty()
                        || user.len() > 128
                        || user.starts_with('-')
                        || !user
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"_.@\\-".contains(&b))
                })
                || host.sources.is_empty()
                || host.sources.len() > 64
                || host
                    .sources
                    .iter()
                    .any(|source| !expected.contains_key(source))
                || host.sources.iter().collect::<BTreeSet<_>>().len() != host.sources.len()
            {
                return Err(invalid());
            }
            variables(&host.variables)?;
            aliases.insert(host.alias.as_str());
            previous = Some(host.alias.as_str());
        }
        for (group, value) in &self.groups {
            if !name(group)
                || group == "_meta"
                || aliases.contains(group.as_str())
                || value.hosts.len() > 10000
                || value.children.len() > 1000
                || value
                    .hosts
                    .iter()
                    .any(|alias| !aliases.contains(alias.as_str()))
                || value.hosts.iter().collect::<BTreeSet<_>>().len() != value.hosts.len()
                || value
                    .children
                    .iter()
                    .any(|child| child == "all" || !self.groups.contains_key(child))
                || value.children.iter().collect::<BTreeSet<_>>().len() != value.children.len()
            {
                return Err(invalid());
            }
            variables(&value.vars)?;
        }
        fn height<'a>(
            name: &'a str,
            groups: &'a BTreeMap<String, SourceInventoryGroup>,
            active: &mut BTreeSet<&'a str>,
            known: &mut BTreeMap<&'a str, usize>,
        ) -> Result<usize> {
            if let Some(value) = known.get(name) {
                return Ok(*value);
            }
            if !active.insert(name) || active.len() > 33 {
                return Err(invalid());
            }
            let mut longest = 0;
            for child in &groups[name].children {
                longest = longest.max(1 + height(child, groups, active, known)?);
                if longest > 32 {
                    return Err(invalid());
                }
            }
            active.remove(name);
            known.insert(name, longest);
            Ok(longest)
        }
        let mut known = BTreeMap::new();
        for group in self.groups.keys() {
            height(group, &self.groups, &mut BTreeSet::new(), &mut known)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct CapturedInventory {
    pub inventory: SourceInventory,
    pub digest: String,
    pub sources: Vec<InventorySource>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u32,
    revision_id: Uuid,
    revision_digest: String,
    key: String,
    digest: String,
    bytes: usize,
    host_count: usize,
    group_count: usize,
    sources: Vec<InventorySource>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Chunk {
    revision_digest: String,
    key: String,
    offset: usize,
    data: String,
    eof: bool,
}

impl CapturedInventory {
    pub async fn fetch(
        peer: &Arc<HelperPeer>,
        capture: &UploadedRevision,
        revision: &ProjectRevision,
        sources: &[InventorySource],
        cancel: &CancellationToken,
    ) -> Result<Self> {
        if capture.revision_id.is_nil()
            || capture.digest != revision.digest
            || !cc_models::automation::is_digest(&capture.digest)
            || sources.is_empty()
            || sources.len() > 8
        {
            return Err(invalid());
        }
        let sources = resolve_inventory_sources(revision, sources)?;
        let mut expected = BTreeMap::new();
        let mut input_bytes = 0u64;
        for source in &sources {
            validate_project_path(&source.path)?;
            let extension = std::path::Path::new(&source.path)
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if matches!(
                extension.as_str(),
                "py" | "sh" | "ps1" | "exe" | "bat" | "cmd"
            ) {
                return Err(invalid());
            }
            let Some(ProjectEntry::File {
                mode: 0o600,
                bytes,
                sha256,
                ..
            }) = revision
                .entries
                .iter()
                .find(|entry| entry.path() == source.path)
            else {
                return Err(invalid());
            };
            if !cc_models::automation::is_digest(sha256)
                || expected
                    .insert(source.path.clone(), sha256.clone())
                    .is_some()
            {
                return Err(invalid());
            }
            input_bytes = input_bytes.checked_add(*bytes).ok_or_else(invalid)?;
        }
        if input_bytes > MAX_BYTES as u64 {
            return Err(invalid());
        }
        let raw = tokio::select! {biased;_=cancel.cancelled()=>return Err(AutomationError::Closed),
        result=peer.request("inventory_capture",json!({"revision_id":capture.revision_id,"sources":sources}),Duration::from_secs(90))=>result?,};
        let receipt: Receipt = serde_json::from_value(raw).map_err(|_| invalid())?;
        let key = hex::encode(Sha256::digest(
            serde_json::to_vec(&json!({"revision_digest":revision.digest,"sources":sources}))
                .map_err(|_| invalid())?,
        ));
        if receipt.version != 1
            || receipt.revision_id != capture.revision_id
            || receipt.revision_digest != revision.digest
            || receipt.key != key
            || !cc_models::automation::is_digest(&receipt.digest)
            || receipt.bytes == 0
            || receipt.bytes > MAX_BYTES
            || receipt.host_count > 10000
            || receipt.group_count > 1000
            || receipt.sources != sources
        {
            return Err(invalid());
        }
        let deadline = Instant::now() + Duration::from_secs(120);
        let mut body = Vec::with_capacity(receipt.bytes);
        while body.len() < receipt.bytes {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(AutomationError::Timeout);
            }
            let raw = tokio::select! {biased;_=cancel.cancelled()=>return Err(AutomationError::Closed),
            result=peer.request("inventory_chunk",json!({"revision_id":capture.revision_id,"key":key,"offset":body.len(),"limit":128*1024}),
                remaining.min(Duration::from_secs(15)))=>result?,};
            let chunk: Chunk = serde_json::from_value(raw).map_err(|_| invalid())?;
            if chunk.revision_digest != revision.digest
                || chunk.key != key
                || chunk.offset != body.len()
                || chunk.data.len() > 175000
            {
                return Err(invalid());
            }
            let part = STANDARD.decode(chunk.data).map_err(|_| invalid())?;
            if part.is_empty()
                || part.len() > 128 * 1024
                || part.len() > receipt.bytes - body.len()
                || chunk.eof != (body.len() + part.len() == receipt.bytes)
            {
                return Err(invalid());
            }
            body.extend_from_slice(&part);
        }
        if cancel.is_cancelled() {
            return Err(AutomationError::Closed);
        }
        if hex::encode(Sha256::digest(&body)) != receipt.digest {
            return Err(invalid());
        }
        let inventory: SourceInventory =
            serde_json::from_value(crate::json::parse(&body)?).map_err(|_| invalid())?;
        inventory.validate_source(&expected)?;
        if cancel.is_cancelled() {
            return Err(AutomationError::Closed);
        }
        if inventory.hosts.len() != receipt.host_count
            || inventory.groups.len() != receipt.group_count
        {
            return Err(invalid());
        }
        Ok(Self {
            inventory,
            digest: receipt.digest,
            sources,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directories_are_expanded_from_the_exact_snapshot_without_scripts_or_later_files() {
        let folder = tempfile::tempdir().unwrap();
        let directory = folder.path().join("inventory");
        std::fs::create_dir(&directory).unwrap();
        for (path, text) in [
            ("a.ini", "web"),
            ("b.yml", "web: {}"),
            ("ignored.py", "raise Exception()"),
            (".hidden.yml", "ignored"),
            ("backup.yml~", "ignored"),
            ("production", "web"),
            ("README", "not inventory"),
        ] {
            std::fs::write(directory.join(path), text).unwrap();
        }
        let root = crate::ProjectRoot::open(folder.path()).unwrap();
        let revision = root.revision().unwrap();
        std::fs::write(directory.join("later.yml"), "later: {}").unwrap();
        let selections = vec![
            InventorySource {
                path: "inventory".into(),
                format: InventoryFormat::Directory,
            },
            InventorySource {
                path: "inventory/a.ini".into(),
                format: InventoryFormat::Ini,
            },
        ];
        let resolved = resolve_inventory_sources(&revision, &selections).unwrap();
        assert_eq!(
            resolved,
            vec![
                InventorySource {
                    path: "inventory/a.ini".into(),
                    format: InventoryFormat::Ini
                },
                InventorySource {
                    path: "inventory/b.yml".into(),
                    format: InventoryFormat::Yaml
                },
                InventorySource {
                    path: "inventory/production".into(),
                    format: InventoryFormat::Ini
                }
            ]
        );
        let mut executable = revision.clone();
        for entry in &mut executable.entries {
            if let ProjectEntry::File { path, mode, .. } = entry {
                if path == "inventory/a.ini" {
                    *mode = 0o700;
                }
            }
        }
        assert!(resolve_inventory_sources(&executable, &selections).is_err());
        assert!(resolve_inventory_sources(
            &revision,
            &[InventorySource {
                path: "missing".into(),
                format: InventoryFormat::Directory
            }]
        )
        .is_err());
    }
    #[test]
    fn directory_limits_are_bounded_and_variable_files_are_explicit_sources() {
        let mut revision = ProjectRevision {
            version: 1,
            digest: "a".repeat(64),
            bytes: 0,
            entries: vec![ProjectEntry::Directory {
                path: "inventory".into(),
            }],
        };
        for i in 0..129 {
            revision.entries.push(ProjectEntry::File {
                path: format!("inventory/{i}.ini"),
                mode: 0o600,
                bytes: 0,
                sha256: "a".repeat(64),
            });
        }
        let source = InventorySource {
            path: "inventory".into(),
            format: InventoryFormat::Directory,
        };
        assert!(resolve_inventory_sources(&revision, std::slice::from_ref(&source)).is_err());
        revision.entries.truncate(2);
        revision.entries.push(ProjectEntry::File {
            path: "inventory/group_vars/web.yml".into(),
            mode: 0o600,
            bytes: 0,
            sha256: "a".repeat(64),
        });
        let files = resolve_inventory_sources(&revision, &[source]).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[1].format, InventoryFormat::GroupVars);
    }
    fn value() -> Value {
        json!({"version":1,"hosts":[{"alias":"service_a","host_id":null,"address":"192.0.2.1","port":2222,
            "username":"deployer","variables":{"threshold":1e-7},"sources":["inventory.ini"],"credential_refs":[]}],
            "groups":{"web":{"hosts":["service_a"],"children":[],"vars":{}}},"source_digests":{"inventory.ini":"a".repeat(64)}})
    }
    fn validate(value: Value) -> bool {
        let parsed = serde_json::from_value::<SourceInventory>(value);
        parsed.is_ok_and(|value| {
            value
                .validate_source(&BTreeMap::from([("inventory.ini".into(), "a".repeat(64))]))
                .is_ok()
        })
    }
    #[test]
    fn a_source_cannot_assign_native_authority_retarget_groups_or_replace_provenance() {
        assert!(validate(value()));
        for (pointer, replacement) in [
            ("/hosts/0/host_id", json!(Uuid::new_v4())),
            ("/hosts/0/credential_refs", json!([Uuid::new_v4()])),
            ("/hosts/0/variables/ansible_connection", json!("ssh")),
            ("/groups/web/vars/ansible_host", json!("elsewhere")),
            ("/hosts/0/sources", json!(["another.ini"])),
            ("/hosts/0/alias", json!("web")),
            ("/source_digests/inventory.ini", json!("b".repeat(64))),
            ("/version", json!(2)),
        ] {
            let mut document = value();
            // Existing fields are replaced; extra variable keys are inserted.
            if let Some(destination) = document.pointer_mut(pointer) {
                *destination = replacement;
            } else {
                let (parent, key) = pointer.rsplit_once('/').unwrap();
                document.pointer_mut(parent).unwrap()[key] = replacement;
            }
            assert!(!validate(document), "accepted {pointer}");
        }
    }
    #[test]
    fn unknown_fields_cycles_and_port_overflow_are_not_valid_inventory() {
        let mut document = value();
        document["extra_authority"] = json!(true);
        assert!(!validate(document));
        let mut document = value();
        document["groups"]["web"]["children"] = json!(["web"]);
        assert!(!validate(document));
        let mut document = value();
        document["hosts"][0]["port"] = json!(65536);
        assert!(!validate(document));
        let mut document = value();
        document["hosts"][0]["variables"]["id"] = json!(9223372036854775808u64);
        assert!(!validate(document));
    }
}
