//! Explicit aliases are configuration, never target authority. Resolution is
//! repeated against captured source data and native authenticated targets.
use crate::{AutomationError, InventorySource, Result, SourceInventory, SourceInventoryHost};
use cc_models::automation::ApprovedTarget;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

const MAX_BYTES: usize = 2 * 1024 * 1024;
fn invalid() -> AutomationError {
    AutomationError::Invalid("invalid, changed or newer external inventory bindings")
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InventoryMode {
    ExternalOnly,
    Mixed,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BindingResolution {
    Match,
    UseSavedHost,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceEndpoint {
    pub address: String,
    pub port: u16,
    pub username: Option<String>,
}
impl SourceEndpoint {
    pub fn from_host(host: &SourceInventoryHost) -> Self {
        Self {
            address: host.address.clone(),
            port: host.port,
            username: host.username.clone(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InventoryBinding {
    pub host_id: Uuid,
    pub resolution: BindingResolution,
    /// A changed source route requires a new explicit binding, including when
    /// the user previously chose to keep the saved ConsoleCrypt route.
    pub source: SourceEndpoint,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExternalInventoryConfig {
    pub version: u32,
    pub sources: Vec<InventorySource>,
    pub mode: InventoryMode,
    pub bindings: BTreeMap<String, InventoryBinding>,
}
impl ExternalInventoryConfig {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_BYTES {
            return Err(invalid());
        }
        let value: Self =
            serde_json::from_value(crate::json::parse(bytes)?).map_err(|_| invalid())?;
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<()> {
        if self.version != 1
            || !(1..=8).contains(&self.sources.len())
            || self.bindings.len() > 10000
        {
            return Err(invalid());
        }
        let mut paths = BTreeSet::new();
        for source in &self.sources {
            crate::validate_project_path(&source.path)?;
            let extension = std::path::Path::new(&source.path)
                .extension()
                .and_then(|v| v.to_str())
                .map(str::to_ascii_lowercase);
            if !paths.insert(&source.path)
                || !match source.format {
                    crate::InventoryFormat::Ini => {
                        extension.is_none() || extension.as_deref() == Some("ini")
                    }
                    crate::InventoryFormat::Yaml => {
                        matches!(extension.as_deref(), Some("yaml" | "yml" | "json"))
                    }
                    crate::InventoryFormat::Directory => true,
                    crate::InventoryFormat::GroupVars | crate::InventoryFormat::HostVars => false,
                }
            {
                return Err(invalid());
            }
        }
        let mut ids = BTreeSet::new();
        for (alias, binding) in &self.bindings {
            let mut chars = alias.bytes();
            if alias.len() > 96
                || !chars
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
                || !chars.all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
                || matches!(alias.as_str(), "all" | "ungrouped" | "_meta")
                || binding.host_id.is_nil()
                || !ids.insert(binding.host_id)
                || binding.source.port == 0
                || binding.source.address.is_empty()
                || binding.source.address.chars().count() > 253
                || binding.source.address.starts_with('-')
                || binding
                    .source
                    .address
                    .chars()
                    .any(|c| !c.is_alphanumeric() && !"_.:[]%-".contains(c))
                || binding.source.username.as_ref().is_some_and(|user| {
                    user.is_empty()
                        || user.len() > 128
                        || user.starts_with('-')
                        || !user
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"_.@\\-".contains(&c))
                })
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
    /// Every stored binding refers to a visible source alias with the exact
    /// route that was displayed when the user chose the resolution.
    pub fn validate_capture(&self, inventory: &SourceInventory) -> Result<()> {
        self.validate()?;
        let covers = |source: &InventorySource, path: &str| {
            source.path == path && source.format != crate::InventoryFormat::Directory
                || source.format == crate::InventoryFormat::Directory
                    && path.starts_with(&format!("{}/", source.path))
        };
        if self
            .sources
            .iter()
            .any(|s| !inventory.source_digests.keys().any(|path| covers(s, path)))
            || inventory
                .source_digests
                .keys()
                .any(|path| !self.sources.iter().any(|s| covers(s, path)))
        {
            return Err(invalid());
        }
        let hosts = inventory
            .hosts
            .iter()
            .map(|host| (host.alias.as_str(), host))
            .collect::<BTreeMap<_, _>>();
        for (alias, binding) in &self.bindings {
            let host = hosts.get(alias.as_str()).ok_or_else(invalid)?;
            if binding.source != SourceEndpoint::from_host(host) {
                return Err(invalid());
            }
        }
        Ok(())
    }
    /// Does not add targets, change endpoints, routes, credential references or
    /// Host IDs. A source alias only labels an already approved native target.
    pub fn bind_targets(
        &self,
        inventory: &SourceInventory,
        targets: &mut [ApprovedTarget],
    ) -> Result<()> {
        self.validate_capture(inventory)?;
        let by_id = self
            .bindings
            .iter()
            .map(|(alias, b)| (b.host_id, (alias, b)))
            .collect::<BTreeMap<_, _>>();
        let source_aliases = inventory
            .hosts
            .iter()
            .map(|h| h.alias.as_str())
            .collect::<BTreeSet<_>>();
        let mut aliases = BTreeSet::new();
        let mut changes = Vec::with_capacity(targets.len());
        let mut ids = BTreeSet::new();
        for target in targets.iter() {
            if !ids.insert(target.host_id) {
                return Err(invalid());
            }
            let alias = match by_id.get(&target.host_id) {
                Some((alias, binding)) => {
                    if binding.resolution == BindingResolution::Match
                        && (binding.source.address != target.address
                            || binding.source.port != target.port
                            || binding
                                .source
                                .username
                                .as_ref()
                                .is_some_and(|u| *u != target.username))
                    {
                        return Err(invalid());
                    }
                    (*alias).clone()
                }
                None if self.mode == InventoryMode::Mixed
                    && !source_aliases.contains(target.alias.as_str()) =>
                {
                    target.alias.clone()
                }
                _ => return Err(invalid()),
            };
            if inventory.groups.contains_key(&alias) || !aliases.insert(alias.clone()) {
                return Err(invalid());
            }
            changes.push(alias);
        }
        // Apply atomically only after every selected target passed validation.
        for (target, alias) in targets.iter_mut().zip(changes) {
            target.alias = alias;
        }
        Ok(())
    }

    /// Produces metadata for the ordinary signed gate. Unselected source hosts
    /// retain their data but never receive a native identity or credentials.
    pub fn runtime_snapshot(
        &self,
        source: &SourceInventory,
        project_groups: &crate::ProjectInventory,
        targets: &mut [ApprovedTarget],
    ) -> Result<SourceInventory> {
        let mut approved = targets.to_vec();
        self.bind_targets(source, &mut approved)?;
        let mut snapshot = source.clone();
        let mut hashes = BTreeMap::new();
        for (name, value) in [
            ("consolecrypt:saved_hosts", serde_json::to_value(&approved)),
            (
                "consolecrypt:project_groups",
                serde_json::to_value(project_groups),
            ),
            ("consolecrypt:bindings", serde_json::to_value(self)),
        ] {
            use sha2::{Digest, Sha256};
            let bytes =
                serde_json::to_vec(&value.map_err(|_| invalid())?).map_err(|_| invalid())?;
            if snapshot.source_digests.contains_key(name) {
                return Err(invalid());
            }
            hashes.insert(name.to_string(), hex::encode(Sha256::digest(bytes)));
        }
        for target in &approved {
            if let Some(host) = snapshot
                .hosts
                .iter_mut()
                .find(|host| host.alias == target.alias)
            {
                host.host_id = Some(target.host_id);
                host.address = target.address.clone();
                host.port = target.port;
                host.username = Some(target.username.clone());
                host.credential_refs = target.credential_refs.clone();
                host.sources.push("consolecrypt:saved_hosts".into());
                host.sources.sort();
                host.sources.dedup();
            } else {
                snapshot.hosts.push(SourceInventoryHost {
                    alias: target.alias.clone(),
                    host_id: Some(target.host_id),
                    address: target.address.clone(),
                    port: target.port,
                    username: Some(target.username.clone()),
                    variables: BTreeMap::new(),
                    sources: vec!["consolecrypt:saved_hosts".into()],
                    credential_refs: target.credential_refs.clone(),
                });
            }
        }
        snapshot.hosts.sort_by(|a, b| a.alias.cmp(&b.alias));
        let native = project_groups.runtime_groups(&approved)?;
        let native: BTreeMap<String, crate::SourceInventoryGroup> =
            serde_json::from_value(native).map_err(|_| invalid())?;
        for (name, group) in native {
            if snapshot.hosts.iter().any(|h| h.alias == name) {
                return Err(invalid());
            }
            if let Some(existing) = snapshot.groups.get_mut(&name) {
                existing.hosts.extend(group.hosts);
                existing.hosts.sort();
                existing.hosts.dedup();
            } else {
                snapshot.groups.insert(name, group);
            }
        }
        snapshot.source_digests.extend(hashes);
        if snapshot.hosts.len() > 10000
            || snapshot.groups.len() > 1000
            || serde_json::to_vec(&snapshot).map_err(|_| invalid())?.len() > 5 * 1024 * 1024
        {
            return Err(invalid());
        }
        targets.clone_from_slice(&approved);
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directory_binding_provenance_is_component_scoped_and_requires_each_selection() {
        let (mut config, mut source, _) = fixture();
        config.sources = vec![InventorySource {
            path: "inventory".into(),
            format: InventoryFormat::Directory,
        }];
        source.source_digests = BTreeMap::from([("inventory/hosts.ini".into(), "a".repeat(64))]);
        config.validate_capture(&source).unwrap();
        source.source_digests =
            BTreeMap::from([("inventory-other/hosts.ini".into(), "a".repeat(64))]);
        assert!(config.validate_capture(&source).is_err());
        source.source_digests.clear();
        assert!(config.validate_capture(&source).is_err());
    }
    use crate::InventoryFormat;
    use serde_json::json;
    fn fixture() -> (ExternalInventoryConfig, SourceInventory, ApprovedTarget) {
        let id = Uuid::new_v4();
        let host = SourceInventoryHost {
            alias: "web".into(),
            host_id: None,
            address: "192.0.2.1".into(),
            port: 22,
            username: Some("operator".into()),
            variables: BTreeMap::new(),
            sources: vec!["hosts.ini".into()],
            credential_refs: vec![],
        };
        let config = ExternalInventoryConfig {
            version: 1,
            sources: vec![InventorySource {
                path: "hosts.ini".into(),
                format: InventoryFormat::Ini,
            }],
            mode: InventoryMode::ExternalOnly,
            bindings: BTreeMap::from([(
                "web".into(),
                InventoryBinding {
                    host_id: id,
                    resolution: BindingResolution::Match,
                    source: SourceEndpoint::from_host(&host),
                },
            )]),
        };
        let source = SourceInventory {
            version: 1,
            hosts: vec![host],
            groups: BTreeMap::new(),
            source_digests: BTreeMap::from([("hosts.ini".into(), "a".repeat(64))]),
        };
        let target = ApprovedTarget {
            host_id: id,
            alias: format!("cc_{}", id.simple()),
            address: "192.0.2.1".into(),
            port: 22,
            username: "operator".into(),
            route_digest: "b".repeat(64),
            credential_refs: vec![Uuid::new_v4()],
        };
        (config, source, target)
    }
    #[test]
    fn binding_only_renames_an_approved_target_and_never_grants_another() {
        let (config, source, target) = fixture();
        let mut targets = vec![target.clone()];
        config.bind_targets(&source, &mut targets).unwrap();
        let mut expected = target.clone();
        expected.alias = "web".into();
        assert_eq!(
            serde_json::to_value(&targets[0]).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        let mut unknown = target;
        unknown.host_id = Uuid::new_v4();
        let original = serde_json::to_value(&unknown).unwrap();
        assert!(config
            .bind_targets(&source, std::slice::from_mut(&mut unknown))
            .is_err());
        assert_eq!(serde_json::to_value(unknown).unwrap(), original);
    }
    #[test]
    fn conflicts_require_explicit_resolution_and_source_changes_require_rebinding() {
        let (mut config, mut source, mut target) = fixture();
        target.address = "198.51.100.9".into();
        assert!(config
            .bind_targets(&source, std::slice::from_mut(&mut target))
            .is_err());
        config.bindings.get_mut("web").unwrap().resolution = BindingResolution::UseSavedHost;
        config
            .bind_targets(&source, std::slice::from_mut(&mut target))
            .unwrap();
        assert_eq!(target.address, "198.51.100.9");
        source.hosts[0].address = "203.0.113.5".into();
        assert!(config
            .bind_targets(&source, std::slice::from_mut(&mut target))
            .is_err());
    }
    #[test]
    fn mixed_mode_does_not_implicitly_bind_colliding_aliases_and_errors_are_atomic() {
        let (mut config, source, target) = fixture();
        config.mode = InventoryMode::Mixed;
        let mut other = target.clone();
        other.host_id = Uuid::new_v4();
        other.alias = "web".into();
        let mut targets = vec![target, other];
        let original = serde_json::to_value(&targets).unwrap();
        assert!(config.bind_targets(&source, &mut targets).is_err());
        assert_eq!(serde_json::to_value(&targets).unwrap(), original);
        targets[1].alias = "cc_another".into();
        config.bind_targets(&source, &mut targets).unwrap();
        assert_eq!(targets[0].alias, "web");
        assert_eq!(targets[1].alias, "cc_another");
    }
    #[test]
    fn duplicate_ids_future_schemas_oversized_and_hidden_authority_are_rejected() {
        let (config, _, _) = fixture();
        let value = serde_json::to_value(config).unwrap();
        let mut duplicate = value.clone();
        duplicate["bindings"]["other"] = duplicate["bindings"]["web"].clone();
        let mut future = value.clone();
        future["version"] = json!(2);
        let mut unknown = value.clone();
        unknown["bindings"]["web"]["credential_refs"] = json!([Uuid::new_v4()]);
        for value in [duplicate, future, unknown] {
            assert!(ExternalInventoryConfig::parse(value.to_string().as_bytes()).is_err());
        }
        assert!(ExternalInventoryConfig::parse(&vec![b' '; MAX_BYTES + 1]).is_err());
        assert!(ExternalInventoryConfig::parse(b"{\"version\":1,\"version\":1}").is_err());
    }

    #[test]
    fn merged_snapshot_keeps_variables_but_only_selected_hosts_get_native_authority() {
        let (config, mut source, target) = fixture();
        source.hosts[0]
            .variables
            .insert("threshold".into(), json!(1e-7));
        let mut unselected = source.hosts[0].clone();
        unselected.alias = "other".into();
        source.hosts.push(unselected);
        let groups = crate::ProjectInventory {
            version: 1,
            selected_hosts: None,
            groups: BTreeMap::from([("readers".into(), vec![target.host_id, Uuid::new_v4()])]),
        };
        let mut targets = vec![target.clone()];
        let snapshot = config
            .runtime_snapshot(&source, &groups, &mut targets)
            .unwrap();
        let selected = snapshot.hosts.iter().find(|h| h.alias == "web").unwrap();
        assert_eq!(selected.host_id, Some(target.host_id));
        assert_eq!(selected.credential_refs, target.credential_refs);
        assert_eq!(selected.variables["threshold"], json!(1e-7));
        let outside = snapshot.hosts.iter().find(|h| h.alias == "other").unwrap();
        assert!(outside.host_id.is_none() && outside.credential_refs.is_empty());
        assert_eq!(snapshot.groups["readers"].hosts, vec!["web"]);
        assert_eq!(snapshot.source_digests.len(), 4);
    }
}
