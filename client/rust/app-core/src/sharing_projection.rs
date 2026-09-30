//! Explicit plaintext allow-list for sharing (ADR-0008).
//!
//! Never serialize a personal VaultObject/HostDto directly into a shared
//! revision. This projection intentionally has no credential, personal IDs,
//! metadata, local commands, trust policy, history or AI-provider fields.
//! Free-form commands/notes still need a human preview before publication.

use crate::dto::EditableDto;
use crate::{AppCore, AppError, AppResult, HostDto, PlanPreviewDto};
use cc_models::group::Group;
use cc_models::host::Host;
use cc_models::secret::{Secret, SecretKind, SecretValue};
use cc_models::snippet::{Snippet, SnippetType};
use cc_protocol::sharing::SharedItemKind;
use cc_protocol::{ObjectId, ShareId};
use serde::{Deserialize, Serialize};
use std::fmt;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
pub struct SharedHostProjection {
    pub name: String,
    pub address: String,
    /// Resolved before preview; receiving users do not inherit private groups.
    pub port: u16,
    pub username: Option<String>,
    pub keepalive_secs: Option<u32>,
    pub tags: Vec<String>,
    /// Excluded by default. May contain sensitive text; preview explicitly.
    pub notes: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
pub struct SharedSnippetVariable {
    pub name: String,
    pub description: String,
    pub required: bool,
    // Personal default values are deliberately not part of this format.
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
pub struct SharedSnippetProjection {
    pub name: String,
    pub description: String,
    #[zeroize(skip)]
    pub snippet_type: SnippetType,
    pub shell: Option<String>,
    pub template: String,
    pub variables: Vec<SharedSnippetVariable>,
    pub tags: Vec<String>,
}

/// References are meaningful only in the enclosing signed server instance.
/// A collection does not confer access to any referenced item.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
pub struct SharedChildReference {
    #[zeroize(skip)]
    pub share_id: ShareId,
    #[zeroize(skip)]
    pub item_id: ObjectId,
    #[zeroize(skip)]
    pub kind: SharedItemKind,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
pub struct SharedGroupProjection {
    pub name: String,
    pub tags: Vec<String>,
    pub children: Vec<SharedChildReference>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
pub struct SharedSecretProjection {
    pub name: String,
    #[zeroize(skip)]
    pub secret_kind: SecretKind,
    pub value: SecretValue,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SharedProjection {
    Host(SharedHostProjection),
    Snippet(SharedSnippetProjection),
    Group(SharedGroupProjection),
    Secret(SharedSecretProjection),
}

macro_rules! opaque_debug {
    ($($t:ty),+ $(,)?) => { $(
        impl fmt::Debug for $t {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($t), "(<redacted>)"))
            }
        }
    )+ };
}
opaque_debug!(
    SharedHostProjection,
    SharedSnippetVariable,
    SharedSnippetProjection,
    SharedChildReference,
    SharedGroupProjection,
    SharedSecretProjection,
    SharedProjection
);

pub struct SharedProjectionBytes(Zeroizing<Vec<u8>>);
opaque_debug!(SharedProjectionBytes);

impl SharedProjectionBytes {
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

impl SharedProjection {
    pub fn kind(&self) -> SharedItemKind {
        match self {
            Self::Host(_) => SharedItemKind::Host,
            Self::Snippet(_) => SharedItemKind::Snippet,
            Self::Group(_) => SharedItemKind::Group,
            Self::Secret(_) => SharedItemKind::Secret,
        }
    }

    pub fn encode(&self) -> AppResult<SharedProjectionBytes> {
        let mut output = Zeroizing::new(Vec::new());
        serde_json::to_writer(&mut *output, self).map_err(AppError::internal)?;
        Ok(SharedProjectionBytes(output))
    }

    /// Decrypted projection only; not an instruction to import or execute it.
    pub fn decode(expected_kind: SharedItemKind, bytes: &[u8]) -> AppResult<Self> {
        if bytes.len() > 1024 * 1024 {
            return Err(AppError::invalid(
                "shared_projection",
                "payload is too large",
            ));
        }
        let projection: Self = serde_json::from_slice(bytes)
            .map_err(|_| AppError::invalid("shared_projection", "invalid payload"))?;
        if projection.kind() != expected_kind {
            return Err(AppError::invalid(
                "shared_projection",
                "content kind differs from signed context",
            ));
        }
        match &projection {
            Self::Host(host) => {
                let mut check = Host::new(&host.name, &host.address);
                check.port = Some(host.port);
                check
                    .validate()
                    .map_err(|_| AppError::invalid("shared_projection", "invalid host"))?;
            }
            Self::Snippet(s) if s.name.trim().is_empty() || s.template.trim().is_empty() => {
                return Err(AppError::invalid("shared_projection", "empty snippet"));
            }
            Self::Group(g) => validate_group(g)?,
            Self::Secret(s) if s.name.trim().is_empty() || s.value.is_empty() => {
                return Err(AppError::invalid("shared_projection", "empty secret"));
            }
            _ => {}
        }
        Ok(projection)
    }
}

fn validate_group(group: &SharedGroupProjection) -> AppResult<()> {
    if group.name.trim().is_empty() || group.children.len() > 256 {
        return Err(AppError::invalid("shared_group", "invalid collection"));
    }
    let mut seen = std::collections::BTreeSet::new();
    for child in &group.children {
        if child.share_id == ShareId::NIL
            || child.item_id == ObjectId::NIL
            || !seen.insert(child.share_id)
        {
            return Err(AppError::invalid(
                "shared_group",
                "invalid or duplicate child reference",
            ));
        }
    }
    Ok(())
}

pub fn project_group(
    group: &Group,
    children: Vec<SharedChildReference>,
) -> AppResult<SharedProjection> {
    let projection = SharedGroupProjection {
        name: group.name.clone(),
        tags: group.tags.clone(),
        children,
    };
    validate_group(&projection)?;
    Ok(SharedProjection::Group(projection))
}

pub fn project_secret(name: &str, secret: &Secret) -> AppResult<SharedProjection> {
    if name.trim().is_empty() || secret.value.is_empty() {
        return Err(AppError::invalid("shared_secret", "empty secret"));
    }
    Ok(SharedProjection::Secret(SharedSecretProjection {
        name: name.to_owned(),
        secret_kind: secret.kind,
        value: secret.value.clone(),
    }))
}

/// Plan identity must match the source object. Dependencies require a separate
/// explicit selection flow; silently turning a jump/proxy host into a direct
/// connection would change its meaning and is rejected in this first stage.
pub fn project_host(
    host: &Host,
    resolved: &PlanPreviewDto,
    include_notes: bool,
) -> AppResult<SharedProjection> {
    host.validate()
        .map_err(|_| AppError::invalid("shared_host", "invalid host"))?;
    if resolved.host_id != host.id.to_string() {
        return Err(AppError::invalid(
            "shared_host",
            "resolved plan belongs to another host",
        ));
    }
    if !host.jump_chain.is_empty()
        || host.jump_profile_id.is_some()
        || !resolved.route.is_empty()
        || resolved.route_source.value.is_some()
        || host.proxy_id.is_some()
        || resolved.proxy_id.is_some()
        || host.proxy_command.is_some()
        || resolved.diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.code.as_str(),
                "missing_group"
                    | "group_cycle"
                    | "group_too_deep"
                    | "missing_jump_profile"
                    | "jump_host_deleted"
                    | "missing_proxy"
                    | "jump_cycle"
                    | "self_jump"
                    | "too_many_hops"
            )
        })
    {
        return Err(AppError::invalid(
            "shared_dependencies",
            "jump hosts and proxies require explicit shared dependency selection",
        ));
    }
    let port = resolved
        .port
        .value
        .filter(|p| *p > 0)
        .ok_or_else(|| AppError::invalid("shared_host", "effective port is unavailable"))?;
    Ok(SharedProjection::Host(SharedHostProjection {
        name: host.name.clone(),
        address: host.address.clone(),
        port,
        username: resolved.username.value.clone(),
        keepalive_secs: host.keepalive_secs,
        tags: host.tags.clone(),
        notes: include_notes.then(|| host.notes.clone()),
    }))
}

pub fn project_snippet(snippet: &Snippet) -> AppResult<SharedProjection> {
    if snippet.name.trim().is_empty() || snippet.template.trim().is_empty() {
        return Err(AppError::invalid("shared_snippet", "empty snippet"));
    }
    Ok(SharedProjection::Snippet(SharedSnippetProjection {
        name: snippet.name.clone(),
        description: snippet.description.clone(),
        snippet_type: snippet.snippet_type,
        shell: snippet.shell.clone(),
        template: snippet.template.clone(),
        variables: snippet
            .variables
            .iter()
            .map(|v| SharedSnippetVariable {
                name: v.name.clone(),
                description: v.description.clone(),
                required: v.required,
            })
            .collect(),
        tags: snippet.tags.clone(),
    }))
}

impl AppCore {
    pub async fn sharing_preview_group(
        &self,
        group_id: String,
        children: Vec<SharedChildReference>,
    ) -> AppResult<SharedProjection> {
        let (_, unlocked) = self.unlocked().await?;
        let id = crate::dto::parse_id("group_id", &group_id)?;
        let group = unlocked
            .working()
            .groups()
            .into_iter()
            .find(|g| g.id == id)
            .ok_or_else(|| AppError::not_found("group", id))?;
        // The caller must explicitly select independently verified shared IDs.
        // No personal children, inheritance or credentials are copied here.
        project_group(&group, children)
    }

    /// Rust-only plaintext preparation. FFI previews expose metadata alone;
    /// publication encrypts this value without returning it to the UI or AI.
    pub async fn sharing_prepare_credential(
        &self,
        credential_id: String,
        passphrase: bool,
    ) -> AppResult<SharedProjection> {
        let (_, unlocked) = self.unlocked().await?;
        let id = crate::dto::parse_id("credential_id", &credential_id)?;
        let credential = unlocked
            .working()
            .credential(id)
            .ok_or_else(|| AppError::not_found("credential", id))?;
        let secret_id = if passphrase {
            credential.passphrase_secret_id
        } else {
            credential.secret_id
        }
        .ok_or_else(|| {
            AppError::invalid("shared_secret", "credential has no selected stored secret")
        })?;
        let secret = unlocked.writer.read_secret(secret_id).await?;
        // Original private-key/passphrase protection remains untouched. The
        // ancillary passphrase is never automatically bundled with a key.
        project_secret(&credential.name, &secret)
    }

    /// Prepared locally, never sent automatically or made available to an LLM.
    pub async fn sharing_preview_host(
        &self,
        host_id: String,
        include_notes: bool,
    ) -> AppResult<SharedProjection> {
        let (_, unlocked) = self.unlocked().await?;
        let id = crate::dto::parse_id("host_id", &host_id)?;
        let host = crate::inventory::host_of(&unlocked, id)?;
        let preview = self.plan_preview(HostDto::from_model(&host)).await?;
        project_host(&host, &preview, include_notes)
    }

    pub async fn sharing_preview_snippet(&self, snippet_id: String) -> AppResult<SharedProjection> {
        let (_, unlocked) = self.unlocked().await?;
        let id = crate::dto::parse_id("snippet_id", &snippet_id)?;
        let snippet = unlocked
            .working()
            .snippets()
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| AppError::not_found("snippet", id))?;
        project_snippet(&snippet)
    }
}
