//! Execution policy (CLIENT_SPEC §16, MVP).
//!
//! The AI core never executes anything. It produces a [`RunProposal`] for the
//! UI (command, host, risk, whether confirmation is required). Only
//! [`ExecutionGate::approve`] turns a proposal into an [`ApprovedCommand`],
//! and it refuses when a required confirmation is missing or the command
//! still contains unresolved placeholders. app-core must call it only in
//! response to an explicit user "Run" action.

use crate::risk::{classify, combine_with_ai, CommandDialect, RiskLevel, RiskReason};
use cc_models::snippet::template_variables;
use cc_models::ObjectId;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Where a command came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalOrigin {
    User,
    Snippet,
    Ai,
    History,
}

/// Target host shown to the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostRef {
    pub id: Option<ObjectId>,
    /// Display name (e.g. host name / address).
    pub display: String,
}

/// What the UI shows before "Run".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunProposal {
    pub command: String,
    pub host: Option<HostRef>,
    pub dialect: CommandDialect,
    /// Effective risk (local rules, raised by the AI hint if any).
    pub risk: RiskLevel,
    /// Local rule level before considering the AI hint.
    pub local_risk: RiskLevel,
    pub ai_risk_suggestion: Option<RiskLevel>,
    pub reasons: Vec<RiskReason>,
    pub requires_confirmation: bool,
    pub origin: ProposalOrigin,
    /// Placeholders that must be filled before running (`{{var}}`,
    /// `<PASSWORD_1>`, …). Non-empty → the gate refuses.
    pub unresolved_placeholders: Vec<String>,
}

static SECRET_PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<(?:PRIVATE_KEY|PEM|PASSWORD|SECRET|IP|HOST|USER|DB)_\d+>").expect("valid regex")
});

impl RunProposal {
    /// Build a proposal: classify locally, let the AI hint only raise the
    /// level, detect unresolved placeholders.
    pub fn new(
        command: impl Into<String>,
        dialect: CommandDialect,
        host: Option<HostRef>,
        origin: ProposalOrigin,
        ai_risk_suggestion: Option<RiskLevel>,
    ) -> Self {
        let command = command.into();
        let assessment = classify(&command, dialect);
        let risk = combine_with_ai(assessment.level, ai_risk_suggestion);
        let mut unresolved: Vec<String> = template_variables(&command)
            .into_iter()
            .map(|v| format!("{{{{{v}}}}}"))
            .collect();
        for m in SECRET_PLACEHOLDER.find_iter(&command) {
            if !unresolved.iter().any(|u| u == m.as_str()) {
                unresolved.push(m.as_str().to_owned());
            }
        }
        let multiline = command.trim().contains('\n');
        Self {
            requires_confirmation: risk.requires_confirmation() || multiline,
            command,
            host,
            dialect,
            risk,
            local_risk: assessment.level,
            ai_risk_suggestion,
            reasons: assessment.reasons,
            origin,
            unresolved_placeholders: unresolved,
        }
    }

    /// Whether the gate would currently refuse regardless of confirmation.
    pub fn is_blocked(&self) -> bool {
        !self.unresolved_placeholders.is_empty() || self.command.trim().is_empty()
    }
}

/// Why the gate refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
pub enum PolicyError {
    #[error("this command is {0:?} and must be confirmed by the user")]
    ConfirmationRequired(RiskLevel),
    #[error("fill in the placeholders first: {0:?}")]
    Unresolved(Vec<String>),
    #[error("empty command")]
    Empty,
}

/// A command the user explicitly approved. Can only be created by
/// [`ExecutionGate::approve`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedCommand {
    command: String,
    host: Option<HostRef>,
    risk: RiskLevel,
}

impl ApprovedCommand {
    pub fn command(&self) -> &str {
        &self.command
    }
    pub fn host(&self) -> Option<&HostRef> {
        self.host.as_ref()
    }
    pub fn risk(&self) -> RiskLevel {
        self.risk
    }
    pub fn into_parts(self) -> (String, Option<HostRef>) {
        (self.command, self.host)
    }
}

/// The only path from a proposal to something executable.
#[derive(Debug, Clone, Copy, Default)]
pub struct ExecutionGate;

impl ExecutionGate {
    /// Approve after an explicit user "Run" action. `user_confirmed` is the
    /// answer to the confirmation dialog (required for modifying,
    /// destructive, unknown and multi-line commands).
    pub fn approve(
        &self,
        proposal: &RunProposal,
        user_confirmed: bool,
    ) -> Result<ApprovedCommand, PolicyError> {
        if proposal.command.trim().is_empty() {
            return Err(PolicyError::Empty);
        }
        // Re-derive from the command text so a tampered proposal (e.g. risk
        // edited to read_only) cannot skip confirmation.
        let fresh = RunProposal::new(
            proposal.command.clone(),
            proposal.dialect,
            proposal.host.clone(),
            proposal.origin,
            proposal.ai_risk_suggestion,
        );
        if !fresh.unresolved_placeholders.is_empty() {
            return Err(PolicyError::Unresolved(fresh.unresolved_placeholders));
        }
        if (fresh.requires_confirmation || proposal.requires_confirmation) && !user_confirmed {
            return Err(PolicyError::ConfirmationRequired(fresh.risk));
        }
        Ok(ApprovedCommand {
            command: fresh.command,
            host: fresh.host,
            risk: fresh.risk,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_needs_no_confirmation_but_others_do() {
        let gate = ExecutionGate;
        let p = RunProposal::new(
            "ls -la",
            CommandDialect::Posix,
            None,
            ProposalOrigin::Ai,
            None,
        );
        assert!(!p.requires_confirmation);
        assert!(gate.approve(&p, false).is_ok());

        for cmd in ["rm -rf /tmp/x", "kubectl apply -f a.yaml", "frob"] {
            let p = RunProposal::new(cmd, CommandDialect::Posix, None, ProposalOrigin::Ai, None);
            assert!(p.requires_confirmation, "{cmd}");
            assert!(matches!(
                gate.approve(&p, false),
                Err(PolicyError::ConfirmationRequired(_))
            ));
            assert_eq!(gate.approve(&p, true).unwrap().command(), cmd);
        }
    }

    #[test]
    fn ai_hint_raises_and_tampering_is_ignored() {
        let p = RunProposal::new(
            "ls",
            CommandDialect::Posix,
            None,
            ProposalOrigin::Ai,
            Some(RiskLevel::Destructive),
        );
        assert_eq!(p.risk, RiskLevel::Destructive);
        assert!(p.requires_confirmation);
        let mut tampered = RunProposal::new(
            "rm -rf /",
            CommandDialect::Posix,
            None,
            ProposalOrigin::Ai,
            None,
        );
        tampered.risk = RiskLevel::ReadOnly;
        tampered.requires_confirmation = false;
        assert!(ExecutionGate.approve(&tampered, false).is_err());
    }

    #[test]
    fn placeholders_block() {
        let p = RunProposal::new(
            "mysql -p<PASSWORD_1> -h {{host}}",
            CommandDialect::Posix,
            None,
            ProposalOrigin::Ai,
            None,
        );
        assert!(p.is_blocked());
        assert_eq!(p.unresolved_placeholders, vec!["{{host}}", "<PASSWORD_1>"]);
        assert!(matches!(
            ExecutionGate.approve(&p, true),
            Err(PolicyError::Unresolved(_))
        ));
        let multi = RunProposal::new(
            "ls\npwd",
            CommandDialect::Posix,
            None,
            ProposalOrigin::User,
            None,
        );
        assert!(multi.requires_confirmation);
    }
}
