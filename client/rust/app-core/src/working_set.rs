//! Decrypted in-memory working set of the unlocked vault
//! (CLIENT_ARCHITECTURE §4). Kept consistent with the store by the facade
//! (read-back after every local mutation) and by an event task following
//! sync-core's [`cc_sync_core::SyncEvent`]s (remote changes, conflict
//! resolution, snapshots).
//!
//! Secret objects are **not** kept decrypted: only their id/kind/revision
//! is tracked. Secret values are decrypted on demand by the credential
//! resolver and dropped (zeroized) right after use.

use cc_models::credential::Credential;
use cc_models::group::Group;
use cc_models::host::{Host, JumpProfile, Proxy};
use cc_models::known_host::KnownHost;
use cc_models::secret::SecretKind;
use cc_models::tunnel::Tunnel;
use cc_models::{ObjectId, ObjectKind, VaultObject};
use cc_sync_core::DecryptedObject;
use std::collections::HashMap;
use std::sync::RwLock;

/// What is kept for one object.
#[derive(Debug, Clone)]
pub(crate) enum Item {
    Object(Box<VaultObject>),
    /// Secret metadata only (the value is never cached).
    Secret {
        kind: SecretKind,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub revision: i64,
    pub item: Item,
}

impl Entry {
    pub(crate) fn kind(&self) -> ObjectKind {
        match &self.item {
            Item::Object(o) => o.kind(),
            Item::Secret { .. } => ObjectKind::Secret,
        }
    }

    fn from_decrypted(d: DecryptedObject) -> Self {
        let item = match d.payload.object {
            VaultObject::Secret(s) => Item::Secret { kind: s.kind },
            other => Item::Object(Box::new(other)),
        };
        Self {
            revision: d.revision,
            item,
        }
    }
}

#[derive(Debug, Default)]
struct Inner {
    entries: HashMap<ObjectId, Entry>,
    /// Revisions of objects known to be deleted (guards against a stale
    /// read resurrecting them).
    tombstones: HashMap<ObjectId, i64>,
    /// Bumped by every mutation (derived views such as the AI search index
    /// compare it to know they are stale).
    version: u64,
}

/// The working set. Cheap reads; writers hold the lock briefly.
#[derive(Debug, Default)]
pub(crate) struct WorkingSet {
    inner: RwLock<Inner>,
}

/// Result of reading one object back from the store.
#[derive(Debug)]
pub(crate) enum Readback {
    /// Row missing (never existed or removed locally).
    Missing,
    /// Tombstone at `revision`.
    Deleted {
        revision: i64,
    },
    Live(Box<DecryptedObject>),
}

macro_rules! typed {
    ($one:ident, $all:ident, $variant:ident, $ty:ty) => {
        pub(crate) fn $one(&self, id: ObjectId) -> Option<$ty> {
            match self.read().entries.get(&id).map(|e| &e.item) {
                Some(Item::Object(o)) => match o.as_ref() {
                    VaultObject::$variant(x) => Some(x.clone()),
                    _ => None,
                },
                _ => None,
            }
        }
        pub(crate) fn $all(&self) -> Vec<$ty> {
            let g = self.read();
            let mut v: Vec<$ty> = g
                .entries
                .values()
                .filter_map(|e| match &e.item {
                    Item::Object(o) => match o.as_ref() {
                        VaultObject::$variant(x) => Some(x.clone()),
                        _ => None,
                    },
                    _ => None,
                })
                .collect();
            v.sort_by_key(|x| x.id);
            v
        }
    };
}

impl WorkingSet {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Inner> {
        self.inner.read().unwrap_or_else(|p| p.into_inner())
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, Inner> {
        let mut g = self.inner.write().unwrap_or_else(|p| p.into_inner());
        g.version = g.version.wrapping_add(1);
        g
    }

    /// Mutation counter (changes whenever the content may have changed).
    pub(crate) fn version(&self) -> u64 {
        self.read().version
    }

    /// Replace everything (initial load, snapshot, lagged events).
    pub(crate) fn replace_all(&self, objects: Vec<DecryptedObject>) {
        let mut g = self.write();
        g.entries = objects
            .into_iter()
            .map(|d| (d.object_id, Entry::from_decrypted(d)))
            .collect();
        g.tombstones.clear();
    }

    /// Apply a read-back of `id`. Returns the kind of the affected object
    /// (before or after), if any.
    pub(crate) fn apply(&self, id: ObjectId, r: Readback) -> Option<ObjectKind> {
        let mut g = self.write();
        let current_rev = g
            .entries
            .get(&id)
            .map(|e| e.revision)
            .or_else(|| g.tombstones.get(&id).copied());
        match r {
            Readback::Missing => {
                g.tombstones.remove(&id);
                g.entries.remove(&id).map(|e| e.kind())
            }
            Readback::Deleted { revision } => {
                if current_rev.is_some_and(|c| c > revision) {
                    return None;
                }
                g.tombstones.insert(id, revision);
                g.entries.remove(&id).map(|e| e.kind())
            }
            Readback::Live(d) => {
                if current_rev.is_some_and(|c| c > d.revision) {
                    return None;
                }
                g.tombstones.remove(&id);
                let e = Entry::from_decrypted(*d);
                let kind = e.kind();
                g.entries.insert(id, e);
                Some(kind)
            }
        }
    }

    pub(crate) fn clear(&self) {
        let mut g = self.write();
        g.entries.clear();
        g.tombstones.clear();
    }

    pub(crate) fn kind_of(&self, id: ObjectId) -> Option<ObjectKind> {
        self.read().entries.get(&id).map(Entry::kind)
    }

    pub(crate) fn secret_kind(&self, id: ObjectId) -> Option<SecretKind> {
        match self.read().entries.get(&id).map(|e| &e.item) {
            Some(Item::Secret { kind }) => Some(*kind),
            _ => None,
        }
    }

    /// All live object ids of a kind.
    pub(crate) fn ids_of(&self, kind: ObjectKind) -> Vec<ObjectId> {
        let mut v: Vec<ObjectId> = self
            .read()
            .entries
            .iter()
            .filter(|(_, e)| e.kind() == kind)
            .map(|(id, _)| *id)
            .collect();
        v.sort();
        v
    }

    /// Every non-secret object (for scans such as reference checks).
    pub(crate) fn objects(&self) -> Vec<VaultObject> {
        self.read()
            .entries
            .values()
            .filter_map(|e| match &e.item {
                Item::Object(o) => Some(o.as_ref().clone()),
                Item::Secret { .. } => None,
            })
            .collect()
    }

    pub(crate) fn len(&self) -> usize {
        self.read().entries.len()
    }

    typed!(host, hosts, Host, Host);
    typed!(group, groups, Group, Group);
    typed!(jump_profile, jump_profiles, JumpProfile, JumpProfile);
    typed!(proxy, proxies, Proxy, Proxy);
    typed!(credential, credentials, Credential, Credential);
    typed!(tunnel, tunnels, Tunnel, Tunnel);
    typed!(known_host, known_hosts, KnownHost, KnownHost);
    typed!(snippet, snippets, Snippet, cc_models::snippet::Snippet);
    typed!(note, notes, Note, cc_models::note::Note);
    typed!(
        vault_settings,
        all_vault_settings,
        VaultSettings,
        cc_models::settings::VaultSettings
    );
    typed!(
        ai_provider,
        ai_providers,
        AiProvider,
        cc_models::ai::AiProviderConfig
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_models::ObjectPayload;
    use cc_storage_core::LocalState;

    fn dec(h: &Host, revision: i64) -> DecryptedObject {
        DecryptedObject {
            object_id: h.id,
            revision,
            local_state: LocalState::LocalOnly,
            conflict_origin: None,
            payload: ObjectPayload::new(VaultObject::Host(h.clone())),
        }
    }

    #[test]
    fn stale_readbacks_do_not_regress() {
        let ws = WorkingSet::new();
        let mut h = Host::new("a", "192.0.2.1");
        ws.apply(h.id, Readback::Live(Box::new(dec(&h, 1))));
        h.name = "b".into();
        ws.apply(h.id, Readback::Live(Box::new(dec(&h, 2))));
        let mut old = h.clone();
        old.name = "a".into();
        assert!(ws
            .apply(h.id, Readback::Live(Box::new(dec(&old, 1))))
            .is_none());
        assert_eq!(ws.host(h.id).unwrap().name, "b");
        ws.apply(h.id, Readback::Deleted { revision: 3 });
        assert!(ws.host(h.id).is_none());
        assert!(ws
            .apply(h.id, Readback::Live(Box::new(dec(&h, 2))))
            .is_none());
        assert!(ws.host(h.id).is_none());
        assert_eq!(ws.hosts().len(), 0);
    }
}
