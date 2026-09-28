//! The AI security boundary (CLIENT_SPEC §14, CLIENT_ARCHITECTURE §3).
//!
//! [`AiContextProvider`] is the **only** way the AI core reads user data. It
//! exposes exactly six read operations and nothing that could return secret
//! material. app-core implements it on top of the plaintext inventory,
//! terminal sessions and the local search index.
//!
//! There is deliberately no `get_private_key`, `get_password`,
//! `get_vault_root_key`, `decrypt_secret` or `list_raw_secrets`, and
//! [`HostContext`] has no credential fields. The crate does not depend on
//! `vault-core`, `crypto-core`, `platform-core` or `ssh-core` either.
//!
//! ```compile_fail
//! # use cc_ai_core::context::AiContextProvider;
//! async fn steal(ctx: &dyn AiContextProvider) {
//!     let _ = ctx.get_private_key(cc_models::ObjectId::new()).await; // no such method
//! }
//! ```
//! ```compile_fail
//! # use cc_ai_core::context::AiContextProvider;
//! async fn steal(ctx: &dyn AiContextProvider) {
//!     let _ = ctx.get_password(cc_models::ObjectId::new()).await; // no such method
//! }
//! ```
//! ```compile_fail
//! # use cc_ai_core::context::AiContextProvider;
//! async fn steal(ctx: &dyn AiContextProvider) {
//!     let _ = ctx.get_vault_root_key().await; // no such method
//! }
//! ```
//! ```compile_fail
//! # use cc_ai_core::context::AiContextProvider;
//! async fn steal(ctx: &dyn AiContextProvider) {
//!     let _ = ctx.decrypt_secret(cc_models::ObjectId::new()).await; // no such method
//! }
//! ```
//! ```compile_fail
//! # use cc_ai_core::context::AiContextProvider;
//! async fn steal(ctx: &dyn AiContextProvider) {
//!     let _ = ctx.list_raw_secrets().await; // no such method
//! }
//! ```
//! ```compile_fail
//! # use cc_ai_core::context::HostContext;
//! let h = HostContext::default();
//! let _ = h.password; // no credential fields
//! ```
//! ```compile_fail
//! # use cc_ai_core::context::HostContext;
//! let h = HostContext::default();
//! let _ = h.credential_id; // not even credential references
//! ```

use async_trait::async_trait;
use cc_models::host::Host;
use cc_models::snippet::Snippet;
use cc_models::ObjectId;
use cc_search_core::DocKind;
use serde::{Deserialize, Serialize};

/// Non-secret facts about a host that may help the model (and that the
/// sanitizer tokenizes under the `Strict` profile).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostContext {
    pub host_id: Option<ObjectId>,
    /// Display name.
    pub name: String,
    /// Hostname or IP literal.
    pub address: String,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub tags: Vec<String>,
    /// e.g. "Ubuntu 24.04", "Windows Server 2022".
    pub os: Option<String>,
    /// Remote shell, e.g. "bash", "zsh", "pwsh".
    pub shell: Option<String>,
}

impl HostContext {
    /// Build from a [`Host`], copying only non-credential fields (the
    /// credential reference, jump chain, proxy and notes are not copied).
    pub fn from_host(host: &Host) -> Self {
        Self {
            host_id: Some(host.id),
            name: host.name.clone(),
            address: host.address.clone(),
            port: host.port,
            username: host.username.clone(),
            tags: host.tags.clone(),
            os: host.metadata.get("os").cloned(),
            shell: host.metadata.get("shell").cloned(),
        }
    }
}

/// A hit from the local knowledge base (snippets, notes, history, hosts).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KbHit {
    pub id: ObjectId,
    pub kind: DocKind,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Higher = better (implementation-defined scale).
    pub score: f64,
}

/// Allowed context interfaces — the complete list. See the module docs.
#[async_trait]
pub trait AiContextProvider: Send + Sync {
    /// Snippets matching `query` (plaintext snippet objects; snippets never
    /// hold secrets — they reference them by id).
    async fn search_snippets(&self, query: &str, limit: usize) -> Vec<Snippet>;
    /// Name/address/user/tags/OS of a host — no credentials.
    async fn get_host_context(&self, host_id: ObjectId) -> Option<HostContext>;
    /// Text the user explicitly selected in the active terminal.
    async fn get_selected_terminal_text(&self) -> Option<String>;
    /// Last command entered in the active terminal.
    async fn get_last_command(&self) -> Option<String>;
    /// Error output of the last failed command in the active terminal.
    async fn get_last_error(&self) -> Option<String>;
    /// Local knowledge-base search (FTS/semantic, never leaves the device).
    async fn search_local_kb(&self, query: &str, limit: usize) -> Vec<KbHit>;
}

/// A context provider that knows nothing (e.g. AI chat without a terminal).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoContext;

#[async_trait]
impl AiContextProvider for NoContext {
    async fn search_snippets(&self, _query: &str, _limit: usize) -> Vec<Snippet> {
        Vec::new()
    }
    async fn get_host_context(&self, _host_id: ObjectId) -> Option<HostContext> {
        None
    }
    async fn get_selected_terminal_text(&self) -> Option<String> {
        None
    }
    async fn get_last_command(&self) -> Option<String> {
        None
    }
    async fn get_last_error(&self) -> Option<String> {
        None
    }
    async fn search_local_kb(&self, _query: &str, _limit: usize) -> Vec<KbHit> {
        Vec::new()
    }
}

/// Fixed in-memory context (tests, CLI, previews). Snippet/KB search is a
/// simple case-insensitive substring match.
#[derive(Debug, Clone, Default)]
pub struct StaticContext {
    pub snippets: Vec<Snippet>,
    pub hosts: Vec<HostContext>,
    pub selected_text: Option<String>,
    pub last_command: Option<String>,
    pub last_error: Option<String>,
    pub kb: Vec<KbHit>,
}

fn matches_query(q: &str, fields: &[&str]) -> bool {
    let q = q.to_lowercase();
    let terms: Vec<&str> = q.split_whitespace().collect();
    if terms.is_empty() {
        return true;
    }
    let hay = fields.join(" ").to_lowercase();
    terms.iter().any(|t| hay.contains(t))
}

#[async_trait]
impl AiContextProvider for StaticContext {
    async fn search_snippets(&self, query: &str, limit: usize) -> Vec<Snippet> {
        self.snippets
            .iter()
            .filter(|s| matches_query(query, &[&s.name, &s.description, &s.template]))
            .take(limit)
            .cloned()
            .collect()
    }
    async fn get_host_context(&self, host_id: ObjectId) -> Option<HostContext> {
        self.hosts
            .iter()
            .find(|h| h.host_id == Some(host_id))
            .cloned()
    }
    async fn get_selected_terminal_text(&self) -> Option<String> {
        self.selected_text.clone()
    }
    async fn get_last_command(&self) -> Option<String> {
        self.last_command.clone()
    }
    async fn get_last_error(&self) -> Option<String> {
        self.last_error.clone()
    }
    async fn search_local_kb(&self, query: &str, limit: usize) -> Vec<KbHit> {
        self.kb
            .iter()
            .filter(|h| matches_query(query, &[&h.title, &h.body]))
            .take(limit)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guard: any new field on `HostContext` must be reviewed against the
    /// "no credentials" rule — update this list consciously.
    #[test]
    fn host_context_fields_are_reviewed() {
        let v = serde_json::to_value(HostContext::default()).unwrap();
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["address", "host_id", "name", "os", "port", "shell", "tags", "username"]
        );
        for k in keys {
            for bad in [
                "pass",
                "secret",
                "key",
                "token",
                "credential",
                "cert",
                "identity",
            ] {
                assert!(!k.contains(bad), "{k}");
            }
        }
    }

    /// The crate must not depend on anything that can reach secret material.
    #[test]
    fn no_dependency_on_secret_bearing_crates() {
        let manifest = include_str!("../Cargo.toml");
        let deps = manifest
            .split("[dependencies]")
            .nth(1)
            .and_then(|s| s.split("\n[").next())
            .unwrap();
        for forbidden in [
            "cc-vault-core",
            "cc-crypto-core",
            "cc-platform-core",
            "cc-ssh-core",
            "cc-ssh-agent-core",
            "cc-storage-core",
            "cc-sync-core",
            "cc-app-core",
            "keyring",
        ] {
            assert!(
                !deps.contains(forbidden),
                "ai-core must not depend on {forbidden}"
            );
        }
    }

    #[tokio::test]
    async fn from_host_copies_only_safe_fields() {
        let mut h = Host::new("prod-db", "10.0.0.5");
        h.username = Some("alice".into());
        h.credential_id = Some(ObjectId::new());
        h.notes = "vpn password is hunter2".into();
        h.metadata.insert("os".into(), "Ubuntu 24.04".into());
        let c = HostContext::from_host(&h);
        assert_eq!(c.os.as_deref(), Some("Ubuntu 24.04"));
        let json = serde_json::to_string(&c).unwrap();
        assert!(!json.contains("hunter2"));
        assert!(!json.contains(&h.credential_id.unwrap().to_string()));
        let ctx = StaticContext {
            hosts: vec![c.clone()],
            ..Default::default()
        };
        assert_eq!(ctx.get_host_context(h.id).await, Some(c));
        assert!(NoContext.get_last_error().await.is_none());
    }
}
