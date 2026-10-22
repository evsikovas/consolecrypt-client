//! Decrypted in-memory working set of the unlocked vault
//! (CLIENT_ARCHITECTURE §4). Kept consistent with the store by the facade
//! (read-back after every local mutation) and by an event task following
//! sync-core's [`cc_sync_core::SyncEvent`]s (remote changes, conflict
//! resolution, snapshots).
//!
//! Teams (protocol 1.9): objects of the profile's team vaults live in the
//! same set, tagged with their vault ([`WorkingSet::vault_of`]); untagged
//! entries belong to the personal vault. A team's `VaultSettings` never
//! enters the set (the team runtime keeps it), so personal settings
//! lookups stay unambiguous.
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
use cc_protocol::VaultId;
use cc_sync_core::DecryptedObject;
use std::collections::{BTreeMap, HashMap};
use std::sync::RwLock;
use tokio_util::sync::CancellationToken;

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
    /// Team vault of an entry (absent = personal vault).
    origin: HashMap<ObjectId, VaultId>,
    /// Teams whose policy forbids sending their resources to AI providers.
    ai_blocked: std::collections::HashSet<VaultId>,
    /// Bumped by every mutation (derived views such as the AI search index
    /// compare it to know they are stale).
    version: u64,
    guards: Vec<RevisionGuard>,
}

#[derive(Debug)]
struct RevisionGuard {
    expected: BTreeMap<ObjectId, i64>,
    kube_connections: BTreeMap<ObjectId, VaultObject>,
    cancel: CancellationToken,
}

// Only decorative fields are excluded. Authentication, TLS, routes, namespace,
// policy and context names continue to revoke the connection immediately.
fn kube_connection_identity(object: &VaultObject) -> Option<VaultObject> {
    match object {
        VaultObject::KubeContext(context) => {
            let mut context = context.clone();
            context.favourite_namespaces.clear();
            context.favourite_kinds.clear();
            context.updated_at = context.created_at;
            Some(VaultObject::KubeContext(context))
        }
        VaultObject::KubeCluster(cluster) => {
            let mut cluster = cluster.clone();
            cluster.tags.clear();
            cluster.color = None;
            cluster.updated_at = cluster.created_at;
            Some(VaultObject::KubeCluster(cluster))
        }
        _ => None,
    }
}

impl Inner {
    fn revoke_stale(&mut self) {
        let entries = &self.entries;
        self.guards.retain(|guard| {
            if guard.expected.iter().any(|(id, revision)| {
                let Some(entry) = entries.get(id) else {
                    return true;
                };
                if entry.revision == *revision {
                    return false;
                }
                let Some(expected) = guard.kube_connections.get(id) else {
                    return true;
                };
                let Item::Object(object) = &entry.item else {
                    return true;
                };
                kube_connection_identity(object).as_ref() != Some(expected)
            }) {
                guard.cancel.cancel();
            }
            !guard.cancel.is_cancelled()
        });
    }
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

    /// One coherent view used to resolve a connection and its dependencies.
    /// Secret entries contain only kind/revision; values stay encrypted.
    pub(crate) fn snapshot(&self) -> BTreeMap<ObjectId, Entry> {
        self.read()
            .entries
            .iter()
            .map(|(id, entry)| (*id, entry.clone()))
            .collect()
    }

    /// Install the fence atomically with checking the captured revisions.
    /// Changes revoke the native scope before their working-set lock is released.
    pub(crate) fn watch_revisions(
        &self,
        expected: BTreeMap<ObjectId, i64>,
        cancel: CancellationToken,
    ) -> bool {
        self.watch_dependencies(expected, cancel, false)
    }

    pub(crate) fn watch_kube_revisions(
        &self,
        expected: BTreeMap<ObjectId, i64>,
        cancel: CancellationToken,
    ) -> bool {
        self.watch_dependencies(expected, cancel, true)
    }

    fn watch_dependencies(
        &self,
        expected: BTreeMap<ObjectId, i64>,
        cancel: CancellationToken,
        decorative_kube_fields: bool,
    ) -> bool {
        let mut g = self.inner.write().unwrap_or_else(|p| p.into_inner());
        if cancel.is_cancelled()
            || expected
                .iter()
                .any(|(id, revision)| g.entries.get(id).map(|e| e.revision) != Some(*revision))
        {
            cancel.cancel();
            return false;
        }
        let kube_connections = if decorative_kube_fields {
            expected
                .keys()
                .filter_map(|id| {
                    let Item::Object(object) = &g.entries.get(id)?.item else {
                        return None;
                    };
                    Some((*id, kube_connection_identity(object)?))
                })
                .collect()
        } else {
            BTreeMap::new()
        };
        g.guards.retain(|guard| !guard.cancel.is_cancelled());
        g.guards.push(RevisionGuard {
            expected,
            kube_connections,
            cancel,
        });
        true
    }

    /// Replace everything of the personal vault (initial load, snapshot,
    /// lagged events). Team entries stay.
    pub(crate) fn replace_all(&self, objects: Vec<DecryptedObject>) {
        self.replace_vault(None, objects);
    }

    /// Replace every entry of one vault (`None` = personal).
    pub(crate) fn replace_vault(&self, vault: Option<VaultId>, objects: Vec<DecryptedObject>) {
        let mut g = self.write();
        let Inner {
            entries,
            tombstones,
            origin,
            ..
        } = &mut *g;
        entries.retain(|id, _| origin.get(id).copied() != vault);
        tombstones.retain(|id, _| origin.get(id).copied() != vault);
        origin.retain(|_, v| Some(*v) != vault);
        for d in objects {
            if vault.is_some() && d.payload.object.kind() == ObjectKind::VaultSettings {
                continue;
            }
            let id = d.object_id;
            entries.insert(id, Entry::from_decrypted(d));
            if let Some(v) = vault {
                origin.insert(id, v);
            }
        }
        g.revoke_stale();
    }

    /// Forget a team vault's entries (team closed or left).
    pub(crate) fn drop_vault(&self, vault: VaultId) {
        self.replace_vault(Some(vault), Vec::new());
    }

    /// Apply a team's "no AI context" policy (ТР-25).
    pub(crate) fn set_ai_blocked(&self, vault: VaultId, blocked: bool) {
        let changed = {
            let g = self.read();
            g.ai_blocked.contains(&vault) != blocked
        };
        if changed {
            let mut g = self.write();
            if blocked {
                g.ai_blocked.insert(vault);
            } else {
                g.ai_blocked.remove(&vault);
            }
        }
    }

    /// The object belongs to a team whose policy keeps it away from AI.
    pub(crate) fn is_ai_blocked(&self, id: ObjectId) -> bool {
        let g = self.read();
        g.origin.get(&id).is_some_and(|v| g.ai_blocked.contains(v))
    }

    /// The team vault of an object (`None` = personal or unknown).
    pub(crate) fn vault_of(&self, id: ObjectId) -> Option<VaultId> {
        self.read().origin.get(&id).copied()
    }

    /// Ids of the live objects of a team vault.
    pub(crate) fn ids_in(&self, vault: VaultId) -> Vec<ObjectId> {
        let g = self.read();
        let mut v: Vec<ObjectId> = g
            .origin
            .iter()
            .filter(|(id, o)| **o == vault && g.entries.contains_key(id))
            .map(|(id, _)| *id)
            .collect();
        v.sort();
        v
    }

    /// Every team-tagged live object id with its vault.
    pub(crate) fn team_origins(&self) -> Vec<(ObjectId, VaultId)> {
        let g = self.read();
        let mut v: Vec<(ObjectId, VaultId)> = g
            .origin
            .iter()
            .filter(|(id, _)| g.entries.contains_key(id))
            .map(|(id, o)| (*id, *o))
            .collect();
        v.sort();
        v
    }

    /// Apply a read-back of `id`. Returns the kind of the affected object
    /// (before or after), if any.
    pub(crate) fn apply(&self, id: ObjectId, r: Readback) -> Option<ObjectKind> {
        self.apply_in(None, id, r)
    }

    /// [`WorkingSet::apply`] for an object of `vault` (`None` = personal).
    pub(crate) fn apply_in(
        &self,
        vault: Option<VaultId>,
        id: ObjectId,
        r: Readback,
    ) -> Option<ObjectKind> {
        if vault.is_some() {
            if let Readback::Live(d) = &r {
                if d.payload.object.kind() == ObjectKind::VaultSettings {
                    return None;
                }
            }
        }
        let mut g = self.write();
        match vault {
            Some(v) => {
                g.origin.insert(id, v);
            }
            None => {
                g.origin.remove(&id);
            }
        }
        let current_rev = g
            .entries
            .get(&id)
            .map(|e| e.revision)
            .or_else(|| g.tombstones.get(&id).copied());
        let affected = match r {
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
        };
        g.revoke_stale();
        affected
    }

    pub(crate) fn clear(&self) {
        let mut g = self.write();
        g.entries.clear();
        g.tombstones.clear();
        g.origin.clear();
        g.ai_blocked.clear();
        g.revoke_stale();
    }

    pub(crate) fn kind_of(&self, id: ObjectId) -> Option<ObjectKind> {
        self.read().entries.get(&id).map(Entry::kind)
    }
    pub(crate) fn revision_of(&self, id: ObjectId) -> Option<i64> {
        self.read().entries.get(&id).map(|e| e.revision)
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
    typed!(
        db_history_entry,
        db_history_entries,
        DbHistoryEntry,
        cc_models::history::DbHistoryEntry
    );
    typed!(
        db_connection,
        db_connections,
        DbConnection,
        cc_models::database::DbConnection
    );
    typed!(
        kube_cluster,
        kube_clusters,
        KubeCluster,
        cc_models::kubernetes::KubeCluster
    );
    typed!(
        kube_context,
        kube_contexts,
        KubeContext,
        cc_models::kubernetes::KubeContext
    );
    typed!(rdp_host, rdp_hosts, RdpHost, cc_models::rdp::RdpHost);
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

    #[test]
    fn revision_fences_revoke_on_updates_deletion_and_reload() {
        for action in 0..5 {
            let ws = WorkingSet::new();
            let host = Host::new("route", "192.0.2.1");
            ws.apply(host.id, Readback::Live(Box::new(dec(&host, 3))));
            let cancel = CancellationToken::new();
            let version = ws.version();
            assert!(ws.watch_revisions(BTreeMap::from([(host.id, 3)]), cancel.clone()));
            assert_eq!(
                ws.version(),
                version,
                "registering a reader is not a mutation"
            );
            ws.apply(host.id, Readback::Live(Box::new(dec(&host, 2))));
            ws.apply(host.id, Readback::Deleted { revision: 2 });
            ws.replace_all(vec![dec(&host, 3)]);
            assert!(
                !cancel.is_cancelled(),
                "stale/no-op readback must preserve scope"
            );
            match action {
                0 => {
                    ws.apply(host.id, Readback::Live(Box::new(dec(&host, 4))));
                }
                1 => {
                    ws.apply(host.id, Readback::Deleted { revision: 4 });
                }
                2 => {
                    ws.apply(host.id, Readback::Missing);
                }
                3 => ws.replace_all(vec![]),
                _ => ws.clear(),
            }
            assert!(cancel.is_cancelled(), "action {action}");
            assert!(ws.read().guards.is_empty());
        }
    }

    #[test]
    fn changed_snapshot_cannot_register_a_live_scope() {
        let ws = WorkingSet::new();
        let host = Host::new("route", "192.0.2.1");
        ws.apply(host.id, Readback::Live(Box::new(dec(&host, 1))));
        let snapshot = ws.snapshot();
        ws.apply(host.id, Readback::Live(Box::new(dec(&host, 2))));
        let cancelled = CancellationToken::new();
        assert!(!ws.watch_revisions(
            snapshot
                .into_iter()
                .map(|(id, entry)| (id, entry.revision))
                .collect(),
            cancelled.clone(),
        ));
        assert!(cancelled.is_cancelled());
        assert!(ws.read().guards.is_empty());
    }

    #[test]
    fn kube_decorations_preserve_live_connections_but_auth_and_policy_revoke() {
        use cc_models::kubernetes::{KubeAuth, KubeCluster, KubeContext};
        fn entry(object: VaultObject, revision: i64) -> Readback {
            Readback::Live(Box::new(DecryptedObject {
                object_id: object.id(),
                revision,
                local_state: LocalState::LocalOnly,
                conflict_origin: None,
                payload: ObjectPayload::new(object),
            }))
        }
        for change in 0..5 {
            let ws = WorkingSet::new();
            let mut cluster = KubeCluster::new("cluster", "https://kube.example.test");
            let mut context = KubeContext::new("production", cluster.id);
            ws.apply(
                cluster.id,
                entry(VaultObject::KubeCluster(cluster.clone()), 1),
            );
            ws.apply(
                context.id,
                entry(VaultObject::KubeContext(context.clone()), 1),
            );
            let token = CancellationToken::new();
            assert!(ws.watch_kube_revisions(
                BTreeMap::from([(cluster.id, 1), (context.id, 1)]),
                token.clone()
            ));
            cluster.tags = vec!["team".into()];
            cluster.color = Some("#123456".into());
            cluster.updated_at += chrono::Duration::seconds(1);
            context.favourite_namespaces = vec!["default".into()];
            context.favourite_kinds = vec!["/v1/pods".into()];
            context.updated_at += chrono::Duration::seconds(1);
            ws.apply(
                cluster.id,
                entry(VaultObject::KubeCluster(cluster.clone()), 2),
            );
            ws.apply(
                context.id,
                entry(VaultObject::KubeContext(context.clone()), 2),
            );
            assert!(!token.is_cancelled());
            match change {
                0 => context.production = false,
                1 => {
                    context.auth = KubeAuth::Token {
                        secret_id: ObjectId::new(),
                    }
                }
                2 => context.allow_exec_in_prod = true,
                3 => cluster.server = "https://different.example.test".into(),
                _ => cluster.insecure_skip_tls_verify = true,
            }
            ws.apply(cluster.id, entry(VaultObject::KubeCluster(cluster), 3));
            ws.apply(context.id, entry(VaultObject::KubeContext(context), 3));
            assert!(token.is_cancelled(), "connection change {change}");
        }
    }
}
