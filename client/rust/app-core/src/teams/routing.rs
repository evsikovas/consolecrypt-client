//! Which vault an object is written to.
//!
//! * Existing objects stay in their vault (moving is an explicit copy +
//!   delete, see `teams_api`).
//! * A new object of a team-capable kind goes to the team its references
//!   point to (a host created in a team group lands in that team); with no
//!   team reference it goes to the personal vault.
//! * Personal-only kinds (history, AI conversations and providers, vault
//!   settings, known hosts, memberships) never enter a team, but may point
//!   at team objects.
//! * A team object may reference only objects of the same team: other
//!   members could not resolve anything else.
//!
//! References are found generically: every string in the serialized
//! object that is the id of another live working-set object.

use crate::error::{AppError, AppResult};
use crate::working_set::WorkingSet;
use cc_models::{ObjectId, ObjectKind, VaultObject};
use cc_protocol::VaultId;
use std::collections::BTreeSet;

/// Kinds that may live in a team vault.
pub(crate) fn team_capable(kind: ObjectKind) -> bool {
    matches!(
        kind,
        ObjectKind::Host
            | ObjectKind::RdpHost
            | ObjectKind::KubeCluster
            | ObjectKind::KubeContext
            | ObjectKind::DbConnection
            | ObjectKind::Group
            | ObjectKind::JumpProfile
            | ObjectKind::Proxy
            | ObjectKind::Credential
            | ObjectKind::Tunnel
            | ObjectKind::Secret
            | ObjectKind::Snippet
            | ObjectKind::Note
    )
}

fn collect(v: &serde_json::Value, out: &mut BTreeSet<ObjectId>) {
    match v {
        serde_json::Value::String(s) if s.len() == 36 => {
            if let Ok(id) = s.parse::<ObjectId>() {
                out.insert(id);
            }
        }
        serde_json::Value::Array(a) => a.iter().for_each(|x| collect(x, out)),
        serde_json::Value::Object(m) => m.values().for_each(|x| collect(x, out)),
        _ => {}
    }
}

/// Ids of other live working-set objects `object` points to.
pub(crate) fn references(ws: &WorkingSet, object: &VaultObject) -> BTreeSet<ObjectId> {
    let mut out = BTreeSet::new();
    if let Ok(v) = serde_json::to_value(object) {
        collect(&v, &mut out);
    }
    out.remove(&object.id());
    out.retain(|id| ws.kind_of(*id).is_some());
    out
}

fn cross_vault() -> AppError {
    AppError::invalid("vault", "cross_vault")
}

/// Target vault of a **new** object (`None` = personal).
pub(crate) fn infer_vault(ws: &WorkingSet, object: &VaultObject) -> AppResult<Option<VaultId>> {
    if !team_capable(object.kind()) {
        return Ok(None);
    }
    let teams: BTreeSet<VaultId> = references(ws, object)
        .into_iter()
        .filter_map(|id| ws.vault_of(id))
        .collect();
    match teams.len() {
        0 => Ok(None),
        1 => Ok(teams.into_iter().next()),
        _ => Err(cross_vault()),
    }
}

/// Validate that `object`, written to `vault`, references nothing a member
/// of that vault could not resolve.
pub(crate) fn check_references(
    ws: &WorkingSet,
    vault: Option<VaultId>,
    object: &VaultObject,
) -> AppResult<()> {
    let Some(vault) = vault else {
        return Ok(());
    };
    if !team_capable(object.kind()) {
        return Err(AppError::invalid("vault", "personal_only"));
    }
    for id in references(ws, object) {
        let kind = ws.kind_of(id);
        if ws.vault_of(id) != Some(vault) && kind.is_some_and(team_capable) {
            return Err(cross_vault());
        }
    }
    Ok(())
}

/// Error of an AI request about a resource whose team forbids AI context.
pub(crate) fn ai_blocked() -> AppError {
    AppError::invalid("ai_context", "blocked_by_team_policy")
}

/// Refuse AI requests about `id` (a string object id) when its team
/// forbids AI context (ТР-25).
pub(crate) fn check_ai_allowed(ws: &WorkingSet, id: &str) -> AppResult<()> {
    match id.parse::<ObjectId>() {
        Ok(id) if ws.is_ai_blocked(id) => Err(ai_blocked()),
        _ => Ok(()),
    }
}
