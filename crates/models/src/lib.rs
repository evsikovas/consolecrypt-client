//! # cc-models — plaintext domain model
//!
//! These types exist only on clients: they are serialized, padded and
//! encrypted into [`cc_protocol::sync::EncryptedBody`] before leaving the
//! device. The server never depends on this crate.
//!
//! Design rules:
//! * Raw secret material (passwords, private keys, passphrases, API keys)
//!   lives **only** in [`secret::Secret`] objects, encrypted under the Secrets
//!   KEK. Everything else references secrets by [`ObjectId`]. This is what
//!   lets the AI subsystem read hosts/snippets without any path to secrets.
//! * Every object kind maps to exactly one KEK class ([`ObjectKind::kek_class`]).
//! * Payloads are versioned ([`ObjectPayload::schema`]); new optional fields
//!   use `#[serde(default)]` so older clients keep working.

pub mod ai;
pub mod credential;
pub mod group;
pub mod history;
pub mod host;
pub mod known_host;
pub mod note;
pub mod secret;
pub mod settings;
pub mod snippet;
pub mod tunnel;

pub use cc_protocol::{DeviceId, ObjectId, Timestamp, VaultId};

use serde::{Deserialize, Serialize};

/// Current payload schema version.
pub const PAYLOAD_SCHEMA_V1: u16 = 1;

/// KEK class — which HKDF-derived key wraps the object's DEK (ADR-0002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KekClass {
    Inventory,
    Secrets,
    Snippets,
    History,
    Settings,
}

impl KekClass {
    pub const ALL: [KekClass; 5] = [
        KekClass::Inventory,
        KekClass::Secrets,
        KekClass::Snippets,
        KekClass::History,
        KekClass::Settings,
    ];

    /// HKDF `info` label for this class.
    pub const fn hkdf_label(&self) -> &'static [u8] {
        use cc_protocol::canonical::labels;
        match self {
            KekClass::Inventory => labels::KEK_INVENTORY,
            KekClass::Secrets => labels::KEK_SECRETS,
            KekClass::Snippets => labels::KEK_SNIPPETS,
            KekClass::History => labels::KEK_HISTORY,
            KekClass::Settings => labels::KEK_SETTINGS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    Host,
    Group,
    JumpProfile,
    Proxy,
    Credential,
    Tunnel,
    KnownHost,
    Secret,
    Snippet,
    Note,
    HistoryEntry,
    AiConversation,
    VaultSettings,
    AiProvider,
}

impl ObjectKind {
    pub const fn kek_class(&self) -> KekClass {
        match self {
            ObjectKind::Host
            | ObjectKind::Group
            | ObjectKind::JumpProfile
            | ObjectKind::Proxy
            | ObjectKind::Credential
            | ObjectKind::Tunnel
            | ObjectKind::KnownHost => KekClass::Inventory,
            ObjectKind::Secret => KekClass::Secrets,
            ObjectKind::Snippet | ObjectKind::Note => KekClass::Snippets,
            ObjectKind::HistoryEntry | ObjectKind::AiConversation => KekClass::History,
            ObjectKind::VaultSettings | ObjectKind::AiProvider => KekClass::Settings,
        }
    }
}

/// Any object that can live in a vault.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum VaultObject {
    Host(host::Host),
    Group(group::Group),
    JumpProfile(host::JumpProfile),
    Proxy(host::Proxy),
    Credential(credential::Credential),
    Tunnel(tunnel::Tunnel),
    KnownHost(known_host::KnownHost),
    Secret(secret::Secret),
    Snippet(snippet::Snippet),
    Note(note::Note),
    HistoryEntry(history::HistoryEntry),
    AiConversation(ai::AiConversation),
    VaultSettings(settings::VaultSettings),
    AiProvider(ai::AiProviderConfig),
}

impl VaultObject {
    pub fn kind(&self) -> ObjectKind {
        match self {
            VaultObject::Host(_) => ObjectKind::Host,
            VaultObject::Group(_) => ObjectKind::Group,
            VaultObject::JumpProfile(_) => ObjectKind::JumpProfile,
            VaultObject::Proxy(_) => ObjectKind::Proxy,
            VaultObject::Credential(_) => ObjectKind::Credential,
            VaultObject::Tunnel(_) => ObjectKind::Tunnel,
            VaultObject::KnownHost(_) => ObjectKind::KnownHost,
            VaultObject::Secret(_) => ObjectKind::Secret,
            VaultObject::Snippet(_) => ObjectKind::Snippet,
            VaultObject::Note(_) => ObjectKind::Note,
            VaultObject::HistoryEntry(_) => ObjectKind::HistoryEntry,
            VaultObject::AiConversation(_) => ObjectKind::AiConversation,
            VaultObject::VaultSettings(_) => ObjectKind::VaultSettings,
            VaultObject::AiProvider(_) => ObjectKind::AiProvider,
        }
    }

    pub fn id(&self) -> ObjectId {
        match self {
            VaultObject::Host(o) => o.id,
            VaultObject::Group(o) => o.id,
            VaultObject::JumpProfile(o) => o.id,
            VaultObject::Proxy(o) => o.id,
            VaultObject::Credential(o) => o.id,
            VaultObject::Tunnel(o) => o.id,
            VaultObject::KnownHost(o) => o.id,
            VaultObject::Secret(o) => o.id,
            VaultObject::Snippet(o) => o.id,
            VaultObject::Note(o) => o.id,
            VaultObject::HistoryEntry(o) => o.id,
            VaultObject::AiConversation(o) => o.id,
            VaultObject::VaultSettings(o) => o.id,
            VaultObject::AiProvider(o) => o.id,
        }
    }
}

/// What gets serialized (JSON), padded and encrypted as an object body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectPayload {
    /// [`PAYLOAD_SCHEMA_V1`].
    pub schema: u16,
    #[serde(flatten)]
    pub object: VaultObject,
}

impl ObjectPayload {
    pub fn new(object: VaultObject) -> Self {
        Self {
            schema: PAYLOAD_SCHEMA_V1,
            object,
        }
    }
}

/// Common validation error for domain objects.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid {field}: {reason}")]
pub struct ValidationError {
    pub field: &'static str,
    pub reason: &'static str,
}

impl ValidationError {
    pub const fn new(field: &'static str, reason: &'static str) -> Self {
        Self { field, reason }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_roundtrip_and_shape() {
        let host = host::Host::new("prod-db", "10.10.10.20");
        let id = host.id;
        let p = ObjectPayload::new(VaultObject::Host(host));
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["schema"], 1);
        assert_eq!(v["kind"], "host");
        assert_eq!(v["data"]["address"], "10.10.10.20");
        let back: ObjectPayload = serde_json::from_value(v).unwrap();
        assert_eq!(back.object.id(), id);
        assert_eq!(back.object.kind().kek_class(), KekClass::Inventory);
    }

    #[test]
    fn secrets_are_isolated_in_their_own_kek_class() {
        for kind in [
            ObjectKind::Host,
            ObjectKind::Credential,
            ObjectKind::Snippet,
            ObjectKind::AiProvider,
        ] {
            assert_ne!(kind.kek_class(), KekClass::Secrets, "{kind:?}");
        }
        assert_eq!(ObjectKind::Secret.kek_class(), KekClass::Secrets);
    }
}
