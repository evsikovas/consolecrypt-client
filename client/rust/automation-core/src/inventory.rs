//! Project-only Ansible groups. Membership never edits ConsoleCrypt folders
//! and a group name never grants a new target to a job.
use crate::{AutomationError, Result};
use cc_models::automation::ApprovedTarget;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProjectInventory {
    pub version: u32,
    pub groups: BTreeMap<String, Vec<Uuid>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_hosts: Option<Vec<Uuid>>,
}
impl Default for ProjectInventory {
    fn default() -> Self {
        Self {
            version: 1,
            groups: BTreeMap::new(),
            selected_hosts: None,
        }
    }
}
fn invalid() -> AutomationError {
    AutomationError::Invalid("invalid or newer project inventory groups")
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventorySelection {
    pub version: u32,
    pub project_id: String,
    pub expected_revision: u64,
    pub saved_hosts: Vec<Uuid>,
    pub configuration: Option<crate::ExternalInventoryConfig>,
    pub preview_id: Option<String>,
}
impl InventorySelection {
    pub fn parse(raw: &[u8]) -> Result<Self> {
        if raw.len() > 3 * 1024 * 1024 {
            return Err(invalid());
        }
        let value: Self =
            serde_json::from_value(crate::json::parse(raw)?).map_err(|_| invalid())?;
        if value.version != 1 || value.expected_revision == 0 {
            return Err(invalid());
        }
        ProjectInventory {
            version: 2,
            groups: BTreeMap::new(),
            selected_hosts: Some(value.saved_hosts.clone()),
        }
        .validate()?;
        if let Some(config) = &value.configuration {
            config.validate()?;
        }
        Ok(value)
    }
}
impl ProjectInventory {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 128 * 1024 {
            return Err(invalid());
        }
        let value: Self =
            serde_json::from_value(crate::json::parse(bytes)?).map_err(|_| invalid())?;
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<()> {
        if !matches!(
            (self.version, self.selected_hosts.is_some()),
            (1, false) | (2, true)
        ) || self.groups.len() > 64
        {
            return Err(invalid());
        }
        if let Some(hosts) = &self.selected_hosts {
            if hosts.len() > 10000
                || hosts.iter().any(Uuid::is_nil)
                || hosts.iter().collect::<BTreeSet<_>>().len() != hosts.len()
            {
                return Err(invalid());
            }
        }
        let mut count = 0usize;
        for (name, members) in &self.groups {
            let mut bytes = name.bytes();
            if name.len() > 96
                || !bytes
                    .next()
                    .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
                || !bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                || matches!(name.as_str(), "all" | "ungrouped" | "_meta")
                || members.iter().any(Uuid::is_nil)
                || members.iter().collect::<BTreeSet<_>>().len() != members.len()
            {
                return Err(invalid());
            }
            count = count.checked_add(members.len()).ok_or_else(invalid)?;
            if count > 10000 {
                return Err(invalid());
            }
        }
        Ok(())
    }
    pub fn normalize(&mut self) -> Result<()> {
        self.validate()?;
        for members in self.groups.values_mut() {
            members.sort();
        }
        if let Some(hosts) = &mut self.selected_hosts {
            hosts.sort();
        }
        Ok(())
    }
    /// Project inventory selection is an additional fence. The final native
    /// manifest still explicitly approves a subset of these existing Host IDs.
    pub fn permits(&self, id: Uuid, external: Option<&crate::ExternalInventoryConfig>) -> bool {
        let bound = external.is_some_and(|config| {
            config
                .bindings
                .values()
                .any(|binding| binding.host_id == id)
        });
        if external.is_some_and(|config| config.mode == crate::InventoryMode::ExternalOnly) {
            return bound;
        }
        self.selected_hosts
            .as_ref()
            .is_none_or(|hosts| hosts.contains(&id))
            || bound
    }
    /// Only aliases already bound to approved native SSH sessions are emitted.
    pub fn runtime_groups(&self, targets: &[ApprovedTarget]) -> Result<Value> {
        let hosts = targets
            .iter()
            .map(|target| (target.host_id, target.alias.as_str()))
            .collect::<Vec<_>>();
        self.runtime_groups_for_hosts(&hosts)
    }
    /// Pure membership projection for explicitly selected native Host IDs.
    /// An alias/group snapshot is data and does not grant execution authority.
    pub fn runtime_groups_for_hosts(&self, targets: &[(Uuid, &str)]) -> Result<Value> {
        self.validate()?;
        let mut groups = serde_json::Map::new();
        for (name, members) in &self.groups {
            let mut hosts = targets
                .iter()
                .filter(|(id, _)| members.contains(id))
                .map(|(_, alias)| (*alias).to_owned())
                .collect::<Vec<_>>();
            hosts.sort();
            hosts.dedup();
            groups.insert(name.clone(), json!({"hosts":hosts,"children":[],"vars":{}}));
        }
        Ok(Value::Object(groups))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn data_group_projection_matches_existing_runtime_for_empty_duplicate_and_selected_targets() {
        let selected = Uuid::new_v4();
        let outside = Uuid::new_v4();
        let groups = ProjectInventory {
            version: 1,
            selected_hosts: None,
            groups: BTreeMap::from([
                ("empty".into(), vec![]),
                ("web".into(), vec![selected, outside]),
            ]),
        };
        let target = ApprovedTarget {
            host_id: selected,
            alias: "cc_selected".into(),
            address: "192.0.2.1".into(),
            port: 22,
            username: "runner".into(),
            route_digest: "a".repeat(64),
            credential_refs: vec![],
        };
        for targets in [
            vec![],
            vec![target.clone()],
            vec![target.clone(), target.clone()],
        ] {
            let aliases = targets
                .iter()
                .map(|host| (host.host_id, host.alias.as_str()))
                .collect::<Vec<_>>();
            let data = groups.runtime_groups_for_hosts(&aliases).unwrap();
            assert_eq!(data, groups.runtime_groups(&targets).unwrap());
            assert_eq!(data["empty"]["hosts"], json!([]));
            assert_eq!(
                data["web"]["hosts"],
                if targets.is_empty() {
                    json!([])
                } else {
                    json!(["cc_selected"])
                }
            );
        }
        let invalid = ProjectInventory {
            groups: BTreeMap::from([("web".into(), vec![selected, selected])]),
            ..groups
        };
        assert!(invalid.runtime_groups_for_hosts(&[]).is_err());
        assert!(invalid.runtime_groups(&[]).is_err());
    }
    #[test]
    fn selected_hosts_are_versioned_unique_and_fence_unselected_targets() {
        let allowed = Uuid::new_v4();
        let outside = Uuid::new_v4();
        let inventory = ProjectInventory::parse(
            serde_json::to_string(&json!({"version":2,"groups":{},"selected_hosts":[allowed]}))
                .unwrap()
                .as_bytes(),
        )
        .unwrap();
        assert!(inventory.permits(allowed, None));
        assert!(!inventory.permits(outside, None));
        for value in [
            json!({"version":1,"groups":{},"selected_hosts":[allowed]}),
            json!({"version":2,"groups":{},"selected_hosts":[allowed,allowed]}),
            json!({"version":3,"groups":{},"selected_hosts":[]}),
        ] {
            assert!(ProjectInventory::parse(&serde_json::to_vec(&value).unwrap()).is_err());
        }
        assert!(InventorySelection::parse(br#"{"version":1,"version":1}"#).is_err());
    }
    #[test]
    fn a_host_can_belong_to_multiple_groups_without_adding_unselected_targets() {
        let host = Uuid::new_v4();
        let outside = Uuid::new_v4();
        let groups = ProjectInventory {
            version: 1,
            selected_hosts: None,
            groups: BTreeMap::from([
                ("web".into(), vec![host, outside]),
                ("production_readonly".into(), vec![host]),
            ]),
        };
        let target = ApprovedTarget {
            host_id: host,
            alias: "cc_fixture".into(),
            address: "192.0.2.1".into(),
            port: 22,
            username: "runner".into(),
            route_digest: "a".repeat(64),
            credential_refs: vec![],
        };
        let runtime = groups.runtime_groups(&[target]).unwrap();
        assert_eq!(runtime["web"]["hosts"], json!(["cc_fixture"]));
        assert_eq!(
            runtime["production_readonly"]["hosts"],
            json!(["cc_fixture"])
        );
        assert!(groups.runtime_groups(&[]).unwrap()["web"]["hosts"]
            .as_array()
            .unwrap()
            .is_empty());
    }
    #[test]
    fn duplicate_future_oversized_or_reserved_group_documents_are_rejected() {
        let id = Uuid::new_v4();
        for text in [
            format!(r#"{{"version":1,"groups":{{"web":["{id}","{id}"]}}}}"#),
            r#"{"version":1,"groups":{"web":[],"web":[]}}"#.into(),
            r#"{"version":2,"groups":{}}"#.into(),
            r#"{"version":1,"groups":{"all":[]}}"#.into(),
            r#"{"version":1,"groups":{"../web":[]}}"#.into(),
            r#"{"version":1,"groups":{},"credentials":"forbidden"}"#.into(),
            " ".repeat(128 * 1024 + 1),
        ] {
            assert!(
                ProjectInventory::parse(text.as_bytes()).is_err(),
                "{text:.120}"
            );
        }
    }
}
