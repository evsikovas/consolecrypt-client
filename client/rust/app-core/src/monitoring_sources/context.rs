//! Original-unlock metadata only. No query, resolver, authorization or routing.
use crate::{AppCore, AppError, AppResult};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const MAX_REQUEST: usize = 4096;
const MAX_REVISION: u64 = 9_007_199_254_740_991;
fn invalid(rule: &'static str) -> AppError {
    AppError::invalid("monitoring_context", rule)
}
fn id(raw: &str) -> AppResult<Uuid> {
    let parsed = Uuid::parse_str(raw).map_err(|_| invalid("context_request_invalid"))?;
    if parsed.is_nil() || parsed.to_string() != raw {
        return Err(invalid("context_request_invalid"));
    }
    Ok(parsed)
}
fn revision(value: u64) -> AppResult<()> {
    if value == 0 || value > MAX_REVISION {
        return Err(invalid("context_request_invalid"));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Request {
    Capture {
        version: u32,
        profile_id: String,
        host_id: String,
        source_id: String,
        source_revision: u64,
        binding_id: String,
        binding_revision: u64,
    },
    Recheck {
        version: u32,
        profile_id: String,
        host_id: String,
        source_id: String,
        source_revision: u64,
        binding_id: String,
        binding_revision: u64,
        notification_scope_id: String,
        host_revision: u64,
    },
}
struct Fields {
    profile: String,
    host: String,
    source: String,
    source_revision: u64,
    binding: String,
    binding_revision: u64,
    expected: Option<(String, u64)>,
}
impl Request {
    fn checked(self) -> AppResult<Fields> {
        let (version, fields) = match self {
            Self::Capture {
                version,
                profile_id,
                host_id,
                source_id,
                source_revision,
                binding_id,
                binding_revision,
            } => (
                version,
                Fields {
                    profile: profile_id,
                    host: host_id,
                    source: source_id,
                    source_revision,
                    binding: binding_id,
                    binding_revision,
                    expected: None,
                },
            ),
            Self::Recheck {
                version,
                profile_id,
                host_id,
                source_id,
                source_revision,
                binding_id,
                binding_revision,
                notification_scope_id,
                host_revision,
            } => (
                version,
                Fields {
                    profile: profile_id,
                    host: host_id,
                    source: source_id,
                    source_revision,
                    binding: binding_id,
                    binding_revision,
                    expected: Some((notification_scope_id, host_revision)),
                },
            ),
        };
        if version != 1 {
            return Err(invalid("context_request_invalid"));
        }
        for raw in [
            &fields.profile,
            &fields.host,
            &fields.source,
            &fields.binding,
        ] {
            id(raw)?;
        }
        revision(fields.source_revision)?;
        revision(fields.binding_revision)?;
        if let Some((scope, host_revision)) = &fields.expected {
            id(scope)?;
            revision(*host_revision)?;
        }
        Ok(fields)
    }
}
fn request(raw: &str) -> AppResult<Fields> {
    if raw.len() > MAX_REQUEST {
        return Err(invalid("context_request_budget"));
    }
    serde_json::from_str::<Request>(raw)
        .map_err(|_| invalid("context_request_invalid"))?
        .checked()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct View {
    version: u32,
    profile_id: String,
    notification_scope_id: String,
    host_id: String,
    host_revision: u64,
    protocol: &'static str,
    source_id: String,
    source_revision: u64,
    binding_id: String,
    binding_revision: u64,
}

impl AppCore {
    /// Captures/rechecks an explicit host binding under the same original
    /// session and unlock. This is a metadata snapshot, NEVER an admission
    /// grant. UI must fence initial response by its original service/generation
    /// and recheck the RETURNED nonce/revision before deferred selection.
    pub async fn monitoring_host_context_json(&self, raw: &str) -> AppResult<String> {
        let input = request(raw)?;
        let (owner, u) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        if owner.profile_id.to_string() != input.profile {
            return Err(invalid("context_changed"));
        }
        let scope = u
            .notices
            .scope_id()
            .ok_or_else(|| invalid("context_changed"))?;
        if input
            .expected
            .as_ref()
            .is_some_and(|(expected, _)| expected != &scope)
        {
            return Err(invalid("context_changed"));
        }
        // No await after acquiring the guards. Sync readback can still mutate
        // WorkingSet, so reject a changed mutation version across all lookups.
        let working = u.working();
        let before = working.version();
        let binding = super::binding_for_request(
            &u,
            &input.binding,
            input.binding_revision,
            input.source_revision,
        )
        .map_err(|_| invalid("context_binding_unavailable"))?;
        if binding.host_id().to_string() != input.host || binding.source_id() != input.source {
            return Err(invalid("context_changed"));
        }
        let host_id = crate::dto::parse_id("monitoring_context", &input.host)?;
        let (host_revision, protocol) = working
            .correlation_host_metadata(host_id)
            .ok_or_else(|| invalid("context_host_unavailable"))?;
        let host_revision =
            u64::try_from(host_revision).map_err(|_| invalid("context_host_unavailable"))?;
        revision(host_revision)?;
        if input
            .expected
            .as_ref()
            .is_some_and(|(_, expected)| *expected != host_revision)
            || before != working.version()
        {
            return Err(invalid("context_changed"));
        }
        let view = View {
            version: 1,
            profile_id: input.profile,
            notification_scope_id: scope,
            host_id: input.host,
            host_revision,
            protocol,
            source_id: input.source,
            source_revision: input.source_revision,
            binding_id: input.binding,
            binding_revision: input.binding_revision,
        };
        serde_json::to_string(&view).map_err(|_| invalid("context_unavailable"))
    }
}

#[cfg(test)]
mod tests;
