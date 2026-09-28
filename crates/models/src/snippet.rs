//! Command snippets with `{{variable}}` templates.

use crate::{DeviceId, ObjectId, Timestamp};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnippetType {
    Shell,
    Bash,
    Zsh,
    Powershell,
    Cmd,
    Sql,
    Postgresql,
    Kubectl,
    Helm,
    Docker,
    Terraform,
    Ansible,
    RedisCli,
    Cql,
    OpensearchDsl,
    HttpCurl,
}

/// Local rules decide the effective risk; an AI suggestion is only a hint.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    ReadOnly,
    Modifying,
    Destructive,
    #[default]
    Unknown,
}

impl RiskLevel {
    /// Whether running requires an explicit confirmation step.
    pub const fn requires_confirmation(&self) -> bool {
        !matches!(self, RiskLevel::ReadOnly)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnippetSource {
    #[default]
    User,
    Ai,
    Imported,
    History,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnippetVariable {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default = "default_true")]
    pub required: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snippet {
    pub id: ObjectId,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Display name of the package. Packages are derived from their members.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_name: Option<String>,
    /// Stable starter-catalog entry identifier, retained when the user edits it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_id: Option<String>,
    pub snippet_type: SnippetType,
    /// Target shell / language dialect, free-form (e.g. "bash", "pwsh7").
    #[serde(default)]
    pub shell: Option<String>,
    pub template: String,
    #[serde(default)]
    pub variables: Vec<SnippetVariable>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub risk_level: RiskLevel,
    #[serde(default)]
    pub source: SnippetSource,
    #[serde(default)]
    pub created_by: Option<DeviceId>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default)]
    pub last_used_at: Option<Timestamp>,
    #[serde(default)]
    pub usage_count: u64,
}

/// Names of `{{variable}}` placeholders in order of first appearance.
/// Names are `[A-Za-z_][A-Za-z0-9_.-]*`, surrounding whitespace allowed.
pub fn template_variables(template: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else { break };
        let name = after[..end].trim();
        let valid = name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
        if valid && !out.iter().any(|n| n == name) {
            out.push(name.to_owned());
        }
        rest = &after[end + 2..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packages_are_optional_and_round_trip_without_changing_legacy_payloads() {
        let original = serde_json::json!({
            "id": ObjectId::new(), "name": "Disk space", "description": "",
            "snippet_type": "shell", "shell": null, "template": "df -h",
            "variables": [], "tags": [], "risk_level": "read_only",
            "source": "user", "created_by": null,
            "created_at": "2026-09-28T00:00:00Z", "updated_at": "2026-09-28T00:00:00Z",
            "last_used_at": null, "usage_count": 0
        });
        let mut snippet: Snippet = serde_json::from_value(original.clone()).unwrap();
        assert!(snippet.package_name.is_none());
        assert!(snippet.catalog_id.is_none());
        assert_eq!(serde_json::to_value(&snippet).unwrap(), original);
        snippet.package_name = Some("Linux".into());
        snippet.catalog_id = Some("linux.disk-space.v1".into());
        let encoded = serde_json::to_vec(&snippet).unwrap();
        assert_eq!(
            serde_json::from_slice::<Snippet>(&encoded).unwrap(),
            snippet
        );
    }

    #[test]
    fn extracts_variables() {
        assert_eq!(
            template_variables("kubectl logs -n {{namespace}} {{ pod }} --tail={{lines}} {{pod}}"),
            vec!["namespace", "pod", "lines"]
        );
        assert!(template_variables("echo {{}} {{1bad}} {{ unterminated").is_empty());
    }

    #[test]
    fn risk_confirmation() {
        assert!(!RiskLevel::ReadOnly.requires_confirmation());
        assert!(RiskLevel::Unknown.requires_confirmation());
        assert!(RiskLevel::Destructive.requires_confirmation());
    }
}
