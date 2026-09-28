//! Inventory CRUD over the unlocked vault: hosts, groups, jump profiles,
//! proxies, tunnels, snippets, notes, known hosts, vault settings and AI
//! provider configs. Identical in Local and Synced profiles (the writer
//! picks local revisions or the outbox). Validation: cc-models validators
//! plus referential checks against the working set.

use crate::app::AppCore;
use crate::dto::*;
use crate::error::{AppError, AppResult};
use crate::session::Unlocked;
use crate::working_set::WorkingSet;
use cc_models::host::Host;
use cc_models::known_host::KnownHostSource;
use cc_models::secret::{Secret, SecretKind, SecretValue};
use cc_models::{ObjectId, ObjectKind, VaultObject};
use std::collections::HashSet;

fn exists(ws: &WorkingSet, id: ObjectId, kind: ObjectKind, field: &str) -> AppResult<()> {
    match ws.kind_of(id) {
        Some(k) if k == kind => Ok(()),
        _ => Err(AppError::invalid(
            field.to_owned(),
            format!("references a missing {kind:?} ({id})"),
        )),
    }
}

/// Referential integrity of an object about to be saved.
pub(crate) fn validate_refs(ws: &WorkingSet, obj: &VaultObject) -> AppResult<()> {
    match obj {
        VaultObject::Host(h) => {
            if let Some(c) = h.credential_id {
                exists(ws, c, ObjectKind::Credential, "credential_id")?;
            }
            if let Some(g) = h.group_id {
                exists(ws, g, ObjectKind::Group, "group_id")?;
            }
            if let Some(p) = h.jump_profile_id {
                exists(ws, p, ObjectKind::JumpProfile, "jump_profile_id")?;
            }
            if let Some(p) = h.proxy_id {
                exists(ws, p, ObjectKind::Proxy, "proxy_id")?;
            }
            let mut seen = HashSet::new();
            for hop in &h.jump_chain {
                if !seen.insert(*hop) {
                    return Err(AppError::invalid("jump_chain", "a host appears twice"));
                }
                exists(ws, *hop, ObjectKind::Host, "jump_chain")?;
            }
        }
        VaultObject::Group(g) => {
            if let Some(c) = g.inherited_credential_id {
                exists(ws, c, ObjectKind::Credential, "inherited_credential_id")?;
            }
            if let Some(p) = g.inherited_jump_profile_id {
                exists(ws, p, ObjectKind::JumpProfile, "inherited_jump_profile_id")?;
            }
            // Parent chain must exist and must not loop back to this group.
            let mut cur = g.parent_id;
            let mut depth = 0;
            while let Some(pid) = cur {
                if pid == g.id {
                    return Err(AppError::invalid("parent_id", "group parents form a cycle"));
                }
                depth += 1;
                if depth > cc_ssh_core::planner::MAX_GROUP_DEPTH {
                    return Err(AppError::invalid("parent_id", "group nesting too deep"));
                }
                let parent = ws
                    .group(pid)
                    .ok_or_else(|| AppError::invalid("parent_id", "references a missing group"))?;
                cur = parent.parent_id;
            }
        }
        VaultObject::JumpProfile(p) => {
            let mut seen = HashSet::new();
            for hop in &p.chain {
                if !seen.insert(*hop) {
                    return Err(AppError::invalid("chain", "a host appears twice"));
                }
                exists(ws, *hop, ObjectKind::Host, "chain")?;
            }
        }
        VaultObject::Tunnel(t) => exists(ws, t.host_id, ObjectKind::Host, "host_id")?,
        VaultObject::Credential(c) => {
            if let Some(s) = c.secret_id {
                let ok = matches!(
                    ws.secret_kind(s),
                    Some(SecretKind::Password | SecretKind::SshPrivateKey)
                );
                if !ok {
                    return Err(AppError::invalid(
                        "secret_id",
                        "references a missing secret",
                    ));
                }
            }
            if let Some(s) = c.passphrase_secret_id {
                if ws.secret_kind(s) != Some(SecretKind::SshKeyPassphrase) {
                    return Err(AppError::invalid(
                        "passphrase_secret_id",
                        "references a missing secret",
                    ));
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Names of objects that reference `id` (blocking deletion).
fn referrers(ws: &WorkingSet, id: ObjectId) -> Vec<String> {
    let mut out = Vec::new();
    for o in ws.objects() {
        let hit = match &o {
            VaultObject::Host(h) => {
                h.credential_id == Some(id)
                    || h.group_id == Some(id)
                    || h.jump_profile_id == Some(id)
                    || h.proxy_id == Some(id)
                    || h.jump_chain.contains(&id)
            }
            VaultObject::Group(g) => {
                g.parent_id == Some(id)
                    || g.inherited_credential_id == Some(id)
                    || g.inherited_jump_profile_id == Some(id)
            }
            VaultObject::JumpProfile(p) => p.chain.contains(&id),
            _ => false,
        };
        if hit {
            out.push(match &o {
                VaultObject::Host(h) => format!("host '{}'", h.name),
                VaultObject::Group(g) => format!("group '{}'", g.name),
                VaultObject::JumpProfile(p) => format!("jump profile '{}'", p.name),
                other => format!("{:?} {}", other.kind(), other.id()),
            });
        }
    }
    out.sort();
    out
}

fn ensure_unreferenced(ws: &WorkingSet, id: ObjectId, what: &str) -> AppResult<()> {
    let refs = referrers(ws, id);
    if refs.is_empty() {
        Ok(())
    } else {
        Err(AppError::InUse {
            what: what.to_owned(),
            id: id.to_string(),
            used_by: refs.join(", "),
        })
    }
}

/// Extract a typed model from a `VaultObject`.
type Getter<M> = fn(&WorkingSet, ObjectId) -> Option<M>;

impl AppCore {
    /// Generic save: create (empty id) or update, validate, write, return
    /// the stored DTO.
    pub(crate) async fn save_dto<D: EditableDto>(
        &self,
        dto: D,
        get: Getter<D::Model>,
        wrap: fn(D::Model) -> VaultObject,
    ) -> AppResult<D> {
        let (_, u) = self.unlocked().await?;
        let ws = u.working();
        let (id, existing) = if dto.id_str().trim().is_empty() {
            (ObjectId::new(), None)
        } else {
            let id = parse_id("id", dto.id_str())?;
            let existing =
                get(ws, id).ok_or_else(|| AppError::not_found(format!("{:?}", D::KIND), id))?;
            (id, Some(existing))
        };
        let model = dto.to_model(id, existing.as_ref())?;
        let obj = wrap(model);
        validate_refs(ws, &obj)?;
        u.writer.put(obj).await?;
        let stored = get(ws, id).ok_or_else(|| AppError::internal("object vanished after save"))?;
        Ok(D::from_model(&stored))
    }

    pub(crate) async fn delete_object(
        &self,
        id: &str,
        kind: ObjectKind,
    ) -> AppResult<(ObjectId, std::sync::Arc<Unlocked>)> {
        let (_, u) = self.unlocked().await?;
        let id = parse_id("id", id)?;
        match u.working().kind_of(id) {
            Some(k) if k == kind => Ok((id, u)),
            _ => Err(AppError::not_found(format!("{kind:?}"), id)),
        }
    }

    // ---- hosts -----------------------------------------------------------------

    pub async fn list_hosts(&self) -> AppResult<Vec<HostDto>> {
        let (_, u) = self.unlocked().await?;
        let mut v: Vec<HostDto> = u
            .working()
            .hosts()
            .iter()
            .map(HostDto::from_model)
            .collect();
        v.sort_by_key(|a| a.name.to_lowercase());
        Ok(v)
    }

    pub async fn get_host(&self, id: String) -> AppResult<HostDto> {
        let (_, u) = self.unlocked().await?;
        let id = parse_id("id", &id)?;
        u.working()
            .host(id)
            .map(|h| HostDto::from_model(&h))
            .ok_or_else(|| AppError::not_found("host", id))
    }

    /// Resolve a host by id or (unique, case-insensitive) name.
    pub async fn find_host(&self, id_or_name: String) -> AppResult<HostDto> {
        let hosts = self.list_hosts().await?;
        if let Some(h) = hosts.iter().find(|h| h.id == id_or_name.trim()) {
            return Ok(h.clone());
        }
        let m: Vec<&HostDto> = hosts
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case(id_or_name.trim()))
            .collect();
        match m.as_slice() {
            [one] => Ok((*one).clone()),
            [] => Err(AppError::not_found("host", id_or_name)),
            _ => Err(AppError::invalid("host", "name is ambiguous; use the id")),
        }
    }

    pub async fn save_host(&self, host: HostDto) -> AppResult<HostDto> {
        self.save_dto(host, WorkingSet::host, VaultObject::Host)
            .await
    }

    /// Delete a host and its tunnels; refused while other hosts or jump
    /// profiles route through it.
    pub async fn delete_host(&self, id: String) -> AppResult<()> {
        let (id, u) = self.delete_object(&id, ObjectKind::Host).await?;
        ensure_unreferenced(u.working(), id, "host")?;
        for t in u
            .working()
            .tunnels()
            .into_iter()
            .filter(|t| t.host_id == id)
        {
            let _ = u.ssh.tunnels.stop(t.id).await;
            u.writer.delete(t.id).await?;
        }
        let inline = u
            .working()
            .host(id)
            .and_then(|h| crate::host_auth::inline_credential(&h));
        u.writer.delete(id).await?;
        if let Some(c) = inline {
            crate::host_auth::delete_credential_if_unused(&u, c).await?;
        }
        Ok(())
    }

    // ---- groups ----------------------------------------------------------------

    pub async fn list_groups(&self) -> AppResult<Vec<GroupDto>> {
        let (_, u) = self.unlocked().await?;
        Ok(u.working()
            .groups()
            .iter()
            .map(GroupDto::from_model)
            .collect())
    }

    pub async fn save_group(&self, group: GroupDto) -> AppResult<GroupDto> {
        self.save_dto(group, WorkingSet::group, VaultObject::Group)
            .await
    }

    pub async fn delete_group(&self, id: String) -> AppResult<()> {
        let (id, u) = self.delete_object(&id, ObjectKind::Group).await?;
        ensure_unreferenced(u.working(), id, "group")?;
        u.writer.delete(id).await
    }

    // ---- jump profiles ---------------------------------------------------------

    pub async fn list_jump_profiles(&self) -> AppResult<Vec<JumpProfileDto>> {
        let (_, u) = self.unlocked().await?;
        Ok(u.working()
            .jump_profiles()
            .iter()
            .map(JumpProfileDto::from_model)
            .collect())
    }

    pub async fn save_jump_profile(&self, profile: JumpProfileDto) -> AppResult<JumpProfileDto> {
        self.save_dto(profile, WorkingSet::jump_profile, VaultObject::JumpProfile)
            .await
    }

    pub async fn delete_jump_profile(&self, id: String) -> AppResult<()> {
        let (id, u) = self.delete_object(&id, ObjectKind::JumpProfile).await?;
        ensure_unreferenced(u.working(), id, "jump profile")?;
        u.writer.delete(id).await
    }

    // ---- proxies ---------------------------------------------------------------

    pub async fn list_proxies(&self) -> AppResult<Vec<ProxyDto>> {
        let (_, u) = self.unlocked().await?;
        Ok(u.working()
            .proxies()
            .iter()
            .map(ProxyDto::from_model)
            .collect())
    }

    pub async fn save_proxy(&self, proxy: ProxyDto) -> AppResult<ProxyDto> {
        self.save_dto(proxy, WorkingSet::proxy, VaultObject::Proxy)
            .await
    }

    /// Set (`Some`) or clear (`None`) a proxy's password (Secret object).
    pub async fn set_proxy_password(
        &self,
        id: String,
        password: Option<String>,
    ) -> AppResult<ProxyDto> {
        let (_, u) = self.unlocked().await?;
        let id = parse_id("id", &id)?;
        let mut p = u
            .working()
            .proxy(id)
            .ok_or_else(|| AppError::not_found("proxy", id))?;
        let old = p.password_secret_id;
        p.password_secret_id = replace_secret(&u, old, password, SecretKind::Password).await?;
        p.updated_at = chrono::Utc::now();
        let new = p.password_secret_id;
        u.writer.put(VaultObject::Proxy(p)).await?;
        if let Some(o) = old.filter(|o| Some(*o) != new) {
            u.writer.delete(o).await?;
        }
        Ok(ProxyDto::from_model(
            &u.working()
                .proxy(id)
                .ok_or_else(|| AppError::internal("proxy vanished"))?,
        ))
    }

    pub async fn delete_proxy(&self, id: String) -> AppResult<()> {
        let (id, u) = self.delete_object(&id, ObjectKind::Proxy).await?;
        ensure_unreferenced(u.working(), id, "proxy")?;
        let secret = u.working().proxy(id).and_then(|p| p.password_secret_id);
        u.writer.delete(id).await?;
        if let Some(s) = secret {
            u.writer.delete(s).await?;
        }
        Ok(())
    }

    // ---- tunnels ---------------------------------------------------------------

    pub async fn list_tunnels(&self) -> AppResult<Vec<TunnelDto>> {
        let (_, u) = self.unlocked().await?;
        Ok(u.working()
            .tunnels()
            .iter()
            .map(TunnelDto::from_model)
            .collect())
    }

    pub async fn save_tunnel(&self, tunnel: TunnelDto) -> AppResult<TunnelDto> {
        self.save_dto(tunnel, WorkingSet::tunnel, VaultObject::Tunnel)
            .await
    }

    pub async fn delete_tunnel(&self, id: String) -> AppResult<()> {
        let (id, u) = self.delete_object(&id, ObjectKind::Tunnel).await?;
        let _ = u.ssh.tunnels.stop(id).await;
        u.writer.delete(id).await
    }

    // ---- snippets & notes ------------------------------------------------------

    pub async fn list_snippets(&self) -> AppResult<Vec<SnippetDto>> {
        let (_, u) = self.unlocked().await?;
        Ok(u.working()
            .snippets()
            .iter()
            .map(SnippetDto::from_model)
            .collect())
    }

    pub async fn save_snippet(&self, snippet: SnippetDto) -> AppResult<SnippetDto> {
        self.save_dto(snippet, WorkingSet::snippet, VaultObject::Snippet)
            .await
    }

    pub async fn delete_snippet(&self, id: String) -> AppResult<()> {
        let (id, u) = self.delete_object(&id, ObjectKind::Snippet).await?;
        u.writer.delete(id).await
    }

    pub async fn list_notes(&self) -> AppResult<Vec<NoteDto>> {
        let (_, u) = self.unlocked().await?;
        Ok(u.working()
            .notes()
            .iter()
            .map(NoteDto::from_model)
            .collect())
    }

    pub async fn save_note(&self, note: NoteDto) -> AppResult<NoteDto> {
        self.save_dto(note, WorkingSet::note, VaultObject::Note)
            .await
    }

    pub async fn delete_note(&self, id: String) -> AppResult<()> {
        let (id, u) = self.delete_object(&id, ObjectKind::Note).await?;
        u.writer.delete(id).await
    }

    // ---- known hosts -----------------------------------------------------------

    pub async fn list_known_hosts(&self) -> AppResult<Vec<KnownHostDto>> {
        let (_, u) = self.unlocked().await?;
        Ok(u.working()
            .known_hosts()
            .iter()
            .map(KnownHostDto::from_model)
            .collect())
    }

    /// Import `known_hosts` text (OpenSSH format incl. hashed names,
    /// `@cert-authority`, `@revoked`). Returns (imported, skipped lines).
    pub async fn import_known_hosts(&self, text: String) -> AppResult<KnownHostsImportDto> {
        let (_, u) = self.unlocked().await?;
        let (entries, errors) = cc_ssh_core::known_hosts::import_known_hosts(&text);
        let existing = u.working().known_hosts();
        let mut added = 0u32;
        for e in entries {
            let dup = existing
                .iter()
                .any(|k| k.host_pattern == e.host_pattern && k.public_key == e.public_key);
            if !dup {
                u.writer.put(VaultObject::KnownHost(e)).await?;
                added += 1;
            }
        }
        Ok(KnownHostsImportDto {
            imported: added,
            skipped: errors.len() as u32,
        })
    }

    /// Trust a host key manually (`key` = `ssh-ed25519 AAAA…`).
    pub async fn add_known_host(
        &self,
        host: String,
        port: u16,
        key: String,
    ) -> AppResult<KnownHostDto> {
        let host = host.trim().to_owned();
        if host.is_empty() || host.contains(char::is_whitespace) || port == 0 {
            return Err(AppError::invalid("host", "must be a host name and port"));
        }
        let pattern = cc_models::known_host::host_pattern(&host, port);
        let line = format!("{pattern} {}", key.trim());
        let (mut entries, errors) = cc_ssh_core::known_hosts::import_known_hosts(&line);
        let mut e = match (entries.pop(), errors.is_empty()) {
            (Some(e), true) => e,
            _ => return Err(AppError::invalid("key", "not an OpenSSH public key line")),
        };
        e.source = KnownHostSource::Manual;
        let (_, u) = self.unlocked().await?;
        let id = e.id;
        u.writer.put(VaultObject::KnownHost(e)).await?;
        u.working()
            .known_host(id)
            .map(|k| KnownHostDto::from_model(&k))
            .ok_or_else(|| AppError::internal("known host vanished"))
    }

    /// Delete a known host entry (e.g. after verifying a changed key).
    pub async fn remove_known_host(&self, id: String) -> AppResult<()> {
        let (id, u) = self.delete_object(&id, ObjectKind::KnownHost).await?;
        u.writer.delete(id).await
    }

    /// Mark every entry with this fingerprint as `@revoked`.
    pub async fn revoke_known_host_key(&self, fingerprint_sha256: String) -> AppResult<u32> {
        let (_, u) = self.unlocked().await?;
        let mut n = 0;
        for mut k in u.working().known_hosts() {
            if k.fingerprint_sha256 == fingerprint_sha256.trim() && !k.revoked {
                k.revoked = true;
                k.updated_at = chrono::Utc::now();
                u.writer.put(VaultObject::KnownHost(k)).await?;
                n += 1;
            }
        }
        Ok(n)
    }

    /// Export the vault's known hosts in OpenSSH format.
    pub async fn export_known_hosts(&self) -> AppResult<String> {
        let (_, u) = self.unlocked().await?;
        Ok(cc_ssh_core::known_hosts::export_known_hosts(
            &u.working().known_hosts(),
        ))
    }

    // ---- vault settings --------------------------------------------------------

    /// The vault's settings (newest wins if a conflict produced two).
    pub async fn get_vault_settings(&self) -> AppResult<VaultSettingsDto> {
        let (_, u) = self.unlocked().await?;
        u.working()
            .all_vault_settings()
            .into_iter()
            .max_by_key(|s| s.updated_at)
            .map(|s| VaultSettingsDto::from_model(&s))
            .ok_or_else(|| AppError::not_found("vault settings", "-"))
    }

    pub async fn save_vault_settings(&self, s: VaultSettingsDto) -> AppResult<VaultSettingsDto> {
        let mut s = s;
        if s.id.trim().is_empty() {
            if let Ok(existing) = self.get_vault_settings().await {
                s.id = existing.id;
            }
        }
        self.save_dto(s, WorkingSet::vault_settings, VaultObject::VaultSettings)
            .await
    }

    // ---- AI provider configs ---------------------------------------------------
    // The AI runtime (providers built from these configs, search index,
    // context provider, execution gate) lives in `crate::ai`.

    pub async fn list_ai_providers(&self) -> AppResult<Vec<AiProviderDto>> {
        let (_, u) = self.unlocked().await?;
        Ok(u.working()
            .ai_providers()
            .iter()
            .map(AiProviderDto::from_model)
            .collect())
    }

    /// Save a provider config; `is_default = true` clears the flag on the
    /// others.
    pub async fn save_ai_provider(&self, p: AiProviderDto) -> AppResult<AiProviderDto> {
        let make_default = p.is_default;
        let saved = self
            .save_dto(p, WorkingSet::ai_provider, VaultObject::AiProvider)
            .await?;
        if make_default {
            let (_, u) = self.unlocked().await?;
            for mut other in u.working().ai_providers() {
                if other.is_default && other.id.to_string() != saved.id {
                    other.is_default = false;
                    other.updated_at = chrono::Utc::now();
                    u.writer.put(VaultObject::AiProvider(other)).await?;
                }
            }
        }
        Ok(saved)
    }

    /// Set (`Some`) or clear (`None`) a provider's API key (Secret object;
    /// never returned by the facade).
    pub async fn set_ai_provider_api_key(
        &self,
        id: String,
        api_key: Option<String>,
    ) -> AppResult<AiProviderDto> {
        let (_, u) = self.unlocked().await?;
        let id = parse_id("id", &id)?;
        let mut p = u
            .working()
            .ai_provider(id)
            .ok_or_else(|| AppError::not_found("ai provider", id))?;
        let old = p.api_key_secret_id;
        p.api_key_secret_id = replace_secret(&u, old, api_key, SecretKind::ApiKey).await?;
        p.updated_at = chrono::Utc::now();
        let new = p.api_key_secret_id;
        u.writer.put(VaultObject::AiProvider(p)).await?;
        if let Some(o) = old.filter(|o| Some(*o) != new) {
            u.writer.delete(o).await?;
        }
        Ok(AiProviderDto::from_model(
            &u.working()
                .ai_provider(id)
                .ok_or_else(|| AppError::internal("provider vanished"))?,
        ))
    }

    pub async fn delete_ai_provider(&self, id: String) -> AppResult<()> {
        let (id, u) = self.delete_object(&id, ObjectKind::AiProvider).await?;
        let secret = u
            .working()
            .ai_provider(id)
            .and_then(|p| p.api_key_secret_id);
        u.writer.delete(id).await?;
        if let Some(s) = secret {
            u.writer.delete(s).await?;
        }
        Ok(())
    }

    // ---- metadata --------------------------------------------------------------

    /// Revision / sync state / conflict origin of an object.
    pub async fn object_meta(&self, id: String) -> AppResult<ObjectMetaDto> {
        let (_, u) = self.unlocked().await?;
        let oid = parse_id("id", &id)?;
        let kind = u
            .working()
            .kind_of(oid)
            .ok_or_else(|| AppError::not_found("object", oid))?;
        // Sync state changes (push confirmations) are not object events:
        // read the stored row, not the working set.
        let stored = u
            .store()
            .stored(oid)
            .await?
            .ok_or_else(|| AppError::not_found("object", oid))?;
        Ok(ObjectMetaDto {
            id: oid.to_string(),
            kind,
            revision: stored.revision,
            local_state: stored.local_state,
            conflict_origin: stored.conflict_origin.map(|c| c.to_string()),
        })
    }

    /// Number of live objects in the open vault (incl. secrets).
    pub async fn object_count(&self) -> AppResult<u64> {
        let (_, u) = self.unlocked().await?;
        Ok(u.working().len() as u64)
    }
}

/// Write a new Secret for `value` (or none) and return its id. The caller
/// deletes the old secret after the referencing object was updated.
pub(crate) async fn replace_secret(
    u: &Unlocked,
    _old: Option<ObjectId>,
    value: Option<String>,
    kind: SecretKind,
) -> AppResult<Option<ObjectId>> {
    match value {
        Some(v) => {
            if v.is_empty() {
                return Err(AppError::invalid("secret", "must not be empty"));
            }
            let s = Secret::new(kind, SecretValue::new(v));
            let id = s.id;
            u.writer.put(VaultObject::Secret(s)).await?;
            Ok(Some(id))
        }
        None => Ok(None),
    }
}

/// Host lookup used by other modules.
pub(crate) fn host_of(u: &Unlocked, id: ObjectId) -> AppResult<Host> {
    u.working()
        .host(id)
        .ok_or_else(|| AppError::not_found("host", id))
}
