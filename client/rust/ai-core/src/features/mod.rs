//! AI features (CLIENT_SPEC §13.2): targets, structured outputs and robust
//! parsing of model answers. Prompt construction lives in `prompts`; the
//! orchestration in [`crate::assistant`].

pub(crate) mod output;
pub(crate) mod prompts;

pub use output::parse_risk;

use crate::policy::RunProposal;
use crate::risk::{CommandDialect, RiskAssessment, RiskLevel};
use crate::sanitizer::SanitizeReport;
use crate::shell::ShellDialect;
use crate::snippet::{RenderDialect, VariableForm};
use cc_models::snippet::{Snippet, SnippetSource, SnippetType, SnippetVariable};
use cc_models::{DeviceId, ObjectId};
use serde::{Deserialize, Serialize};

/// What kind of command to generate (CLIENT_SPEC §11.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandTarget {
    #[default]
    Shell,
    Bash,
    Zsh,
    PowerShell,
    Cmd,
    Sql,
    PostgreSql,
    Kubectl,
    Helm,
    Docker,
    Terraform,
    Ansible,
    RedisCli,
    Cql,
    OpenSearchDsl,
    Curl,
}

impl CommandTarget {
    pub const ALL: [CommandTarget; 16] = [
        CommandTarget::Shell,
        CommandTarget::Bash,
        CommandTarget::Zsh,
        CommandTarget::PowerShell,
        CommandTarget::Cmd,
        CommandTarget::Sql,
        CommandTarget::PostgreSql,
        CommandTarget::Kubectl,
        CommandTarget::Helm,
        CommandTarget::Docker,
        CommandTarget::Terraform,
        CommandTarget::Ansible,
        CommandTarget::RedisCli,
        CommandTarget::Cql,
        CommandTarget::OpenSearchDsl,
        CommandTarget::Curl,
    ];

    pub const fn snippet_type(self) -> SnippetType {
        match self {
            CommandTarget::Shell => SnippetType::Shell,
            CommandTarget::Bash => SnippetType::Bash,
            CommandTarget::Zsh => SnippetType::Zsh,
            CommandTarget::PowerShell => SnippetType::Powershell,
            CommandTarget::Cmd => SnippetType::Cmd,
            CommandTarget::Sql => SnippetType::Sql,
            CommandTarget::PostgreSql => SnippetType::Postgresql,
            CommandTarget::Kubectl => SnippetType::Kubectl,
            CommandTarget::Helm => SnippetType::Helm,
            CommandTarget::Docker => SnippetType::Docker,
            CommandTarget::Terraform => SnippetType::Terraform,
            CommandTarget::Ansible => SnippetType::Ansible,
            CommandTarget::RedisCli => SnippetType::RedisCli,
            CommandTarget::Cql => SnippetType::Cql,
            CommandTarget::OpenSearchDsl => SnippetType::OpensearchDsl,
            CommandTarget::Curl => SnippetType::HttpCurl,
        }
    }

    pub fn from_snippet_type(t: SnippetType) -> Self {
        Self::ALL
            .into_iter()
            .find(|c| c.snippet_type() == t)
            .unwrap_or_default()
    }

    /// Dialect for local risk rules.
    pub const fn dialect(self) -> CommandDialect {
        match self {
            CommandTarget::PowerShell => CommandDialect::PowerShell,
            CommandTarget::Cmd => CommandDialect::Cmd,
            CommandTarget::Sql | CommandTarget::PostgreSql => CommandDialect::Sql,
            CommandTarget::Cql => CommandDialect::Cql,
            CommandTarget::OpenSearchDsl => CommandDialect::OpenSearch,
            _ => CommandDialect::Posix,
        }
    }

    /// Quoting rules for snippet rendering.
    pub fn render_dialect(self) -> RenderDialect {
        RenderDialect::for_snippet(self.snippet_type(), None)
    }

    /// Tokenizer for command parameterization.
    pub const fn shell_dialect(self) -> ShellDialect {
        match self {
            CommandTarget::PowerShell => ShellDialect::PowerShell,
            CommandTarget::Cmd => ShellDialect::Cmd,
            _ => ShellDialect::Posix,
        }
    }

    /// Pick a target from a free-form remote shell name.
    pub fn for_shell_name(shell: Option<&str>) -> Self {
        match shell.and_then(ShellDialect::from_shell_name) {
            Some(ShellDialect::PowerShell) => CommandTarget::PowerShell,
            Some(ShellDialect::Cmd) => CommandTarget::Cmd,
            _ => match shell.map(|s| s.to_ascii_lowercase()) {
                Some(s) if s.ends_with("zsh") => CommandTarget::Zsh,
                Some(s) if s.ends_with("bash") => CommandTarget::Bash,
                _ => CommandTarget::Shell,
            },
        }
    }
}

/// How well the model followed the requested structured format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParseQuality {
    /// Valid JSON as requested.
    Json,
    /// JSON recovered (code fences, surrounding prose, trailing commas).
    Repaired,
    /// No JSON: heuristics (code block / single line).
    Fallback,
}

/// A variable the model suggests for a command/snippet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestedVariable {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub default: Option<String>,
}

/// Parsed "command" answer (generate / fix / RAG).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandSuggestion {
    pub command: String,
    pub explanation: String,
    /// Model's risk opinion (only ever raises the local level).
    pub risk_suggestion: Option<RiskLevel>,
    pub variables: Vec<SuggestedVariable>,
    pub alternatives: Vec<String>,
    /// For "fix last error": what went wrong.
    pub diagnosis: Option<String>,
    /// RAG: indexes (1-based) of the knowledge-base entries used.
    pub sources: Vec<usize>,
    pub parse_quality: ParseQuality,
}

/// A generated command ready for the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandProposal {
    pub target: CommandTarget,
    /// Re-hydrated suggestion (non-secret placeholders restored locally).
    pub suggestion: CommandSuggestion,
    /// Run proposal for the (re-hydrated) command. Never auto-executed.
    pub run: RunProposal,
    /// Present when the command contains `{{variables}}` to fill in.
    pub form: Option<VariableForm>,
    /// Secret placeholders the user must replace (e.g. `<PASSWORD_1>`).
    pub unresolved_secrets: Vec<String>,
    /// What was redacted from the prompt (counts only).
    pub redactions: SanitizeReport,
}

/// One explained piece of a command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplainedPart {
    pub text: String,
    pub meaning: String,
}

/// Parsed "explain" answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Explanation {
    pub summary: String,
    pub parts: Vec<ExplainedPart>,
    pub risk_suggestion: Option<RiskLevel>,
    pub warnings: Vec<String>,
    pub parse_quality: ParseQuality,
}

/// Explain result with the local risk assessment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExplainResult {
    pub explanation: Explanation,
    pub local_risk: RiskAssessment,
    /// Local level raised by the model's opinion.
    pub effective_risk: RiskLevel,
    pub redactions: SanitizeReport,
}

/// A snippet proposal (create / convert). Not yet persisted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnippetDraft {
    pub name: String,
    pub description: String,
    pub snippet_type: SnippetType,
    pub shell: Option<String>,
    pub template: String,
    /// In order of appearance in `template`; secrets have no default.
    pub variables: Vec<SnippetVariable>,
    pub tags: Vec<String>,
    /// Effective risk (local rules on the template, raised by the AI hint).
    pub risk: RiskLevel,
    pub local_risk: RiskLevel,
    pub ai_risk_suggestion: Option<RiskLevel>,
    pub source: SnippetSource,
    /// Secret values removed from the template/defaults.
    pub secrets_removed: usize,
    pub parse_quality: ParseQuality,
}

impl SnippetDraft {
    /// Materialize as a new [`Snippet`] (to be encrypted & synced by app-core).
    pub fn into_snippet(self, created_by: Option<DeviceId>) -> Snippet {
        let now = chrono::Utc::now();
        Snippet {
            package_name: None,
            catalog_id: None,
            id: ObjectId::new(),
            name: self.name,
            description: self.description,
            snippet_type: self.snippet_type,
            shell: self.shell,
            template: self.template,
            variables: self.variables,
            tags: self.tags,
            risk_level: self.risk,
            source: self.source,
            created_by,
            created_at: now,
            updated_at: now,
            last_used_at: None,
            usage_count: 0,
        }
    }
}
