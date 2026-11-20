//! Bounded informational guest declarations. Never approval or effective rights.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DeclaredPrivileges {
    version: u8,
    source: Source,
    records: Vec<Declaration>,
    unresolved: Vec<Unresolved>,
    truncated: bool,
}
#[derive(Clone, Serialize, Deserialize)]
enum Source {
    #[serde(rename = "ansible_declared_v1")]
    AnsibleDeclaredV1,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Declaration {
    play: u16,
    area: Area,
    indices: Vec<u16>,
    branches: Vec<Branch>,
    #[serde(rename = "become")]
    escalation: Become,
    user: Field,
    method: Field,
    #[serde(rename = "knownMethod")]
    known_method: Option<Method>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Area {
    Play,
    PreTasks,
    Tasks,
    PostTasks,
    Handlers,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Branch {
    Block,
    Rescue,
    Always,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Become {
    Absent,
    Enabled,
    Disabled,
    Dynamic,
    Unknown,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Field {
    Absent,
    LiteralHidden,
    Dynamic,
    Unknown,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Method {
    Sudo,
    Su,
    Doas,
    Runas,
    Pfexec,
    Ksu,
    Machinectl,
    Pbrun,
    Pmrun,
    Dzdo,
}
#[derive(Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Unresolved {
    RuntimeOverrides,
    Roles,
    TaskIncludes,
    Shape,
}

impl DeclaredPrivileges {
    pub(super) fn parse(value: Value) -> Option<Self> {
        // Bound vectors before Deserialize allocates their typed forms. The
        // entire helper response has the existing transport budget as well.
        let object = value.as_object()?;
        if object.len() != 5 {
            return None;
        }
        let rows = object.get("records")?.as_array()?;
        if rows.len() > 64 {
            return None;
        }
        for row in rows {
            let row = row.as_object()?;
            if row.len() != 8
                || row.get("indices")?.as_array()?.len() > 16
                || row.get("branches")?.as_array()?.len() > 15
            {
                return None;
            }
        }
        if object.get("unresolved")?.as_array()?.len() > 4 {
            return None;
        }
        let summary: Self = serde_json::from_value(value).ok()?;
        if summary.version != 1
            || !summary.unresolved.contains(&Unresolved::RuntimeOverrides)
            || summary.unresolved.iter().collect::<HashSet<_>>().len() != summary.unresolved.len()
            || summary.records.iter().any(|row| {
                row.play == 0
                    || row.play > 4096
                    || row.indices.iter().any(|index| *index == 0 || *index > 4096)
                    || row.branches.len() != row.indices.len().saturating_sub(1)
                    || (row.area == Area::Play) != row.indices.is_empty()
                    || row.known_method.is_some() && row.method != Field::LiteralHidden
            })
            || serde_json::to_vec(&summary).ok()?.len() > 32768
        {
            return None;
        }
        Some(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn summary() -> Value {
        json!({"version":1,"source":"ansible_declared_v1","records":[{
        "play":1,"area":"tasks","indices":[1],"branches":[],"become":"enabled",
        "user":"literal_hidden","method":"literal_hidden","knownMethod":"sudo"}],
        "unresolved":["runtime_overrides"],"truncated":false})
    }
    #[test]
    fn exact_declaration_metadata_has_no_private_literals_or_authority() {
        let value = summary();
        let parsed = DeclaredPrivileges::parse(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), value);
        for (pointer, invalid) in [
            ("/version", json!(2)),
            ("/source", json!("future")),
            ("/unresolved", json!([])),
            ("/records/0/user", json!("PRIVATE_USER")),
            ("/records/0/knownMethod", json!("PRIVATE_PLUGIN")),
            ("/records/0/indices", json!([0])),
            ("/records/0/branches", json!(["block"])),
            ("/records/0/play", json!(true)),
        ] {
            let mut changed = value.clone();
            *changed.pointer_mut(pointer).unwrap() = invalid;
            assert!(DeclaredPrivileges::parse(changed).is_none(), "{pointer}");
        }
        let mut extra = value;
        extra["records"][0]["secret_ref"] = json!("PRIVATE_REF");
        assert!(DeclaredPrivileges::parse(extra).is_none());
    }
    #[test]
    fn row_depth_bounds_and_explicit_unknown_truncation_remain_honest() {
        let mut value = summary();
        value["records"] = json!(vec![value["records"][0].clone(); 65]);
        assert!(DeclaredPrivileges::parse(value).is_none());
        let mut value = summary();
        value["records"][0]["indices"] = json!(vec![1; 17]);
        value["records"][0]["branches"] = json!(vec!["block"; 16]);
        assert!(DeclaredPrivileges::parse(value).is_none());
        let mut value = summary();
        value["records"] = json!([]);
        value["truncated"] = json!(true);
        assert!(DeclaredPrivileges::parse(value).is_some());
    }
}
