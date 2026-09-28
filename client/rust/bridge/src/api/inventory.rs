//! Inventory of the unlocked vault: hosts (+ Termius-style auth, ADR-0101
//! §6a), groups, jump profiles, tunnels, snippets, known hosts, vault
//! settings and AI provider configs.
//!
//! Save semantics ("upsert"): the Dart models mint ids for drafts, app-core
//! mints ids on create (empty id). A DTO whose id is not in the vault is
//! therefore created (id cleared); the stored DTO (with the core's id) is
//! returned and must be used from then on.

use crate::api::error::BridgeError;
use crate::state::{from_json, opt_secret_string, to_json, with_core};
use cc_app_core::{
    AgentKind, AiProviderDto, AppCore, AppError, GroupDto, HostAuth, HostDto, JumpProfileDto,
    SnippetDto, TunnelDto, VaultSettingsDto,
};
use zeroize::Zeroize;

/// How a host authenticates (`HostAuth` of app-core, flattened for FRB).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostAuthKind {
    /// Inherit username/credential from the group chain.
    Inherit,
    /// Link an existing (shared) credential: `credential_id`.
    Credential,
    /// The host's own password credential: `password` (`None` keeps it).
    InlinePassword,
    /// Ask at connect; nothing stored.
    PasswordPrompt,
    /// OS SSH agent.
    OsAgent,
    /// External agent socket / pipe: `agent_path`.
    ExternalAgent,
}

/// Desired authentication of a host (sent with [`hosts_save_with_auth`]).
/// Secret-bearing: redacted `Debug`; the password buffer is moved into
/// app-core's zeroizing secret (or zeroized when unused).
#[derive(Clone)]
pub struct HostAuthInput {
    pub kind: HostAuthKind,
    pub credential_id: Option<String>,
    /// Inline password (UTF-8), inbound only.
    pub password: Option<Vec<u8>>,
    pub agent_path: Option<String>,
}

impl std::fmt::Debug for HostAuthInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostAuthInput")
            .field("kind", &self.kind)
            .field("credential_id", &self.credential_id)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("agent_path", &self.agent_path)
            .finish()
    }
}

pub(crate) fn host_auth(mut input: HostAuthInput) -> Result<HostAuth, BridgeError> {
    if input.kind != HostAuthKind::InlinePassword {
        if let Some(mut unused) = input.password.take() {
            unused.zeroize();
        }
    }
    Ok(match input.kind {
        HostAuthKind::Inherit => HostAuth::Inherit,
        HostAuthKind::Credential => HostAuth::Credential {
            credential_id: input
                .credential_id
                .take()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| BridgeError::invalid("credential_id", "required"))?,
        },
        HostAuthKind::InlinePassword => HostAuth::InlinePassword {
            password: opt_secret_string("password", input.password.take())?,
        },
        HostAuthKind::PasswordPrompt => HostAuth::PasswordPrompt,
        HostAuthKind::OsAgent => HostAuth::Agent {
            kind: AgentKind::Os,
            path: None,
        },
        HostAuthKind::ExternalAgent => HostAuth::Agent {
            kind: AgentKind::External,
            path: input.agent_path.take(),
        },
    })
}

/// Clear `id` when it does not name an existing object (create instead).
pub(crate) fn upsert_id(id: &mut String, exists: bool) {
    if !exists {
        id.clear();
    }
}

async fn host_exists(c: &AppCore, id: &str) -> Result<bool, BridgeError> {
    if id.trim().is_empty() {
        return Ok(false);
    }
    match c.get_host(id.to_owned()).await {
        Ok(_) => Ok(true),
        Err(AppError::NotFound { .. }) | Err(AppError::InvalidInput { .. }) => Ok(false),
        Err(e) => Err(e.into()),
    }
}

// ---- hosts -------------------------------------------------------------------------

/// `Vec<HostDto>` sorted by name.
pub async fn hosts_list() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.list_hosts().await?) }).await
}

/// Insert or update a host (`HostDto` JSON) keeping `credential_id` as given.
pub async fn hosts_save(host_json: String) -> Result<String, BridgeError> {
    let mut host: HostDto = from_json("host", &host_json)?;
    with_core(move |c| async move {
        let exists = host_exists(&c, &host.id).await?;
        upsert_id(&mut host.id, exists);
        to_json(&c.save_host(host).await?)
    })
    .await
}

/// Insert or update a host together with its authentication, atomically.
pub async fn hosts_save_with_auth(
    host_json: String,
    auth: HostAuthInput,
) -> Result<String, BridgeError> {
    let mut host: HostDto = from_json("host", &host_json)?;
    let auth = host_auth(auth)?;
    with_core(move |c| async move {
        let exists = host_exists(&c, &host.id).await?;
        upsert_id(&mut host.id, exists);
        to_json(&c.save_host_with_auth(host, auth).await?)
    })
    .await
}

/// Delete a host, its tunnels and its inline credential.
pub async fn hosts_delete(id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.delete_host(id).await?) }).await
}

/// Planned route (`ConnectionRouteDto`: `user@hop -> … -> user@target`).
pub async fn hosts_describe_connection(host_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.describe_connection(host_id).await?) }).await
}

/// Effective settings of a (possibly unsaved) host with provenance, the
/// expanded route and planner findings as codes → `PlanPreviewDto`
/// (host editor). Nothing is saved; no connection is made.
pub async fn hosts_plan_preview(host_json: String) -> Result<String, BridgeError> {
    let host: HostDto = from_json("host", &host_json)?;
    with_core(move |c| async move { to_json(&c.plan_preview(host).await?) }).await
}

// ---- groups ------------------------------------------------------------------------

pub async fn groups_list() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.list_groups().await?) }).await
}

pub async fn groups_save(group_json: String) -> Result<String, BridgeError> {
    let mut group: GroupDto = from_json("group", &group_json)?;
    with_core(move |c| async move {
        let exists = c.list_groups().await?.iter().any(|g| g.id == group.id);
        upsert_id(&mut group.id, exists);
        to_json(&c.save_group(group).await?)
    })
    .await
}

/// Delete a group; its child groups and hosts move to its parent first
/// (the UI contract), then the group is removed.
pub async fn groups_delete(id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move {
        let groups = c.list_groups().await?;
        let Some(target) = groups.iter().find(|g| g.id == id) else {
            return Err(AppError::NotFound {
                what: "group".into(),
                id,
            }
            .into());
        };
        let parent = target.parent_id.clone();
        for mut child in groups
            .iter()
            .filter(|g| g.parent_id.as_deref() == Some(id.as_str()))
            .cloned()
        {
            child.parent_id = parent.clone();
            c.save_group(child).await?;
        }
        for mut host in c
            .list_hosts()
            .await?
            .into_iter()
            .filter(|h| h.group_id.as_deref() == Some(id.as_str()))
        {
            host.group_id = parent.clone();
            c.save_host(host).await?;
        }
        Ok(c.delete_group(id).await?)
    })
    .await
}

// ---- jump profiles -----------------------------------------------------------------

pub async fn jump_profiles_list() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.list_jump_profiles().await?) }).await
}

pub async fn jump_profiles_save(profile_json: String) -> Result<String, BridgeError> {
    let mut p: JumpProfileDto = from_json("jump_profile", &profile_json)?;
    with_core(move |c| async move {
        let exists = c.list_jump_profiles().await?.iter().any(|x| x.id == p.id);
        upsert_id(&mut p.id, exists);
        to_json(&c.save_jump_profile(p).await?)
    })
    .await
}

pub async fn jump_profiles_delete(id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.delete_jump_profile(id).await?) }).await
}

// ---- tunnels -----------------------------------------------------------------------

pub async fn tunnels_list() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.list_tunnels().await?) }).await
}

pub async fn tunnels_save(tunnel_json: String) -> Result<String, BridgeError> {
    let mut t: TunnelDto = from_json("tunnel", &tunnel_json)?;
    with_core(move |c| async move {
        let exists = c.list_tunnels().await?.iter().any(|x| x.id == t.id);
        upsert_id(&mut t.id, exists);
        to_json(&c.save_tunnel(t).await?)
    })
    .await
}

/// Stop the tunnel if running, then delete it.
pub async fn tunnels_delete(id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move {
        let _ = c.stop_tunnel(id.clone()).await;
        Ok(c.delete_tunnel(id).await?)
    })
    .await
}

// ---- snippets ----------------------------------------------------------------------

pub async fn snippets_list() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.list_snippets().await?) }).await
}

pub async fn snippets_save(snippet_json: String) -> Result<String, BridgeError> {
    let mut s: SnippetDto = from_json("snippet", &snippet_json)?;
    with_core(move |c| async move {
        let exists = c.list_snippets().await?.iter().any(|x| x.id == s.id);
        upsert_id(&mut s.id, exists);
        to_json(&c.save_snippet(s).await?)
    })
    .await
}

pub async fn snippets_delete(id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.delete_snippet(id).await?) }).await
}

/// Snippet palette search (local FTS, semantic only with an embedding
/// provider, never an LLM) → `Vec<SnippetSearchHitDto>`. Empty query =
/// most used.
pub async fn snippets_search(query: String, limit: u32) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.search_snippets(query, limit).await?) }).await
}

/// Render a (possibly unsaved) snippet with values (`{"name":"value"}`
/// JSON) using the core's quoting policy → `SnippetRenderDto`
/// (`command` or `field_errors`).
pub async fn snippets_render(
    snippet_json: String,
    values_json: String,
) -> Result<String, BridgeError> {
    let snippet: SnippetDto = from_json("snippet", &snippet_json)?;
    let values: std::collections::HashMap<String, String> = from_json("values", &values_json)?;
    with_core(move |c| async move { to_json(&c.snippet_render_dto(snippet, values).await?) }).await
}

// ---- known hosts -------------------------------------------------------------------

pub async fn known_hosts_list() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.list_known_hosts().await?) }).await
}

pub async fn known_hosts_remove(id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.remove_known_host(id).await?) }).await
}

// ---- vault settings ----------------------------------------------------------------

/// `VaultSettingsDto` (singleton).
pub async fn vault_settings_get() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.get_vault_settings().await?) }).await
}

pub async fn vault_settings_save(settings_json: String) -> Result<String, BridgeError> {
    let s: VaultSettingsDto = from_json("vault_settings", &settings_json)?;
    with_core(move |c| async move { to_json(&c.save_vault_settings(s).await?) }).await
}

// ---- AI provider configs -----------------------------------------------------------

pub async fn ai_providers_list() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.list_ai_providers().await?) }).await
}

/// Save a provider config; `api_key` replaces the stored key (a Secret
/// object), `clear_api_key` removes it. The key is never returned.
pub async fn ai_providers_save(
    provider_json: String,
    api_key: Option<Vec<u8>>,
    clear_api_key: bool,
) -> Result<String, BridgeError> {
    let mut p: AiProviderDto = from_json("ai_provider", &provider_json)?;
    let api_key = opt_secret_string("api_key", api_key)?;
    with_core(move |c| async move {
        let exists = c.list_ai_providers().await?.iter().any(|x| x.id == p.id);
        upsert_id(&mut p.id, exists);
        let mut saved = c.save_ai_provider(p).await?;
        if api_key.is_some() || clear_api_key {
            saved = c.set_ai_provider_api_key(saved.id.clone(), api_key).await?;
        }
        to_json(&saved)
    })
    .await
}

pub async fn ai_providers_delete(id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.delete_ai_provider(id).await?) }).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_auth_maps_every_kind() {
        let base = HostAuthInput {
            kind: HostAuthKind::Inherit,
            credential_id: None,
            password: None,
            agent_path: None,
        };
        assert!(matches!(
            host_auth(base.clone()).unwrap(),
            HostAuth::Inherit
        ));
        let e = host_auth(HostAuthInput {
            kind: HostAuthKind::Credential,
            ..base.clone()
        })
        .unwrap_err();
        assert_eq!(e.code, "invalid_input");
        assert!(matches!(
            host_auth(HostAuthInput {
                kind: HostAuthKind::Credential,
                credential_id: Some("c1".into()),
                ..base.clone()
            })
            .unwrap(),
            HostAuth::Credential { ref credential_id } if credential_id == "c1"
        ));
        match host_auth(HostAuthInput {
            kind: HostAuthKind::InlinePassword,
            password: Some(b"hunter2".to_vec()),
            ..base.clone()
        })
        .unwrap()
        {
            HostAuth::InlinePassword { ref password } => {
                assert_eq!(password.as_deref(), Some("hunter2"))
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(
            host_auth(HostAuthInput {
                kind: HostAuthKind::InlinePassword,
                ..base.clone()
            })
            .unwrap(),
            HostAuth::InlinePassword { password: None }
        ));
        assert!(matches!(
            host_auth(HostAuthInput {
                kind: HostAuthKind::PasswordPrompt,
                ..base.clone()
            })
            .unwrap(),
            HostAuth::PasswordPrompt
        ));
        assert!(matches!(
            host_auth(HostAuthInput {
                kind: HostAuthKind::OsAgent,
                agent_path: Some("ignored".into()),
                ..base.clone()
            })
            .unwrap(),
            HostAuth::Agent {
                kind: AgentKind::Os,
                path: None
            }
        ));
        assert!(matches!(
            host_auth(HostAuthInput {
                kind: HostAuthKind::ExternalAgent,
                agent_path: Some("/tmp/agent.sock".into()),
                ..base
            })
            .unwrap(),
            HostAuth::Agent {
                kind: AgentKind::External,
                path: Some(_)
            }
        ));
    }

    #[test]
    fn host_auth_debug_never_shows_the_password() {
        let auth = host_auth(HostAuthInput {
            kind: HostAuthKind::InlinePassword,
            credential_id: None,
            password: Some(b"s3cr3t-value".to_vec()),
            agent_path: None,
        })
        .unwrap();
        assert!(!format!("{auth:?}").contains("s3cr3t-value"));
    }

    #[test]
    fn upsert_clears_unknown_ids_only() {
        let mut id = "draft".to_owned();
        upsert_id(&mut id, false);
        assert!(id.is_empty());
        let mut id = "known".to_owned();
        upsert_id(&mut id, true);
        assert_eq!(id, "known");
    }

    #[test]
    fn host_dto_json_is_the_documented_wire_format() {
        let dto = HostDto::new("db", "10.0.0.5");
        let json = to_json(&dto).unwrap();
        for key in [
            "\"host_key_policy\":\"ask\"",
            "\"backend\":\"native\"",
            "\"auth_mode\":\"inherit\"",
        ] {
            assert!(json.contains(key), "{key} missing in {json}");
        }
        let back: HostDto = from_json("host", &json).unwrap();
        assert_eq!(back, dto);
    }
}
