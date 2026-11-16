//! Bounded credential-free direct-folder metadata, plus the EXISTING revision
//! guard's pre-confirmation membership interest. No task/service/global scan.
use super::*;
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FolderGroup {
    pub name: String,
    pub parent_id: Option<ObjectId>,
    pub revision: i64,
    pub origin: Option<VaultId>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FolderHost {
    pub name: String,
    pub group_id: ObjectId,
    pub revision: i64,
    pub origin: Option<VaultId>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FolderSnapshot {
    pub groups: BTreeMap<ObjectId, FolderGroup>,
    pub hosts: BTreeMap<ObjectId, FolderHost>,
}
#[derive(Debug)]
pub(super) struct FolderWatchState {
    pub folders: BTreeSet<ObjectId>,
    pub active: AtomicBool,
}
#[derive(Clone, Debug)]
pub(crate) struct FolderWatch(Arc<FolderWatchState>);
/// Borrowed changed-object projection, never clones credentials/routes or scans
/// the complete set. Revision/origin/type changes are significant even when a
/// readback reuses a revision; an IDENTICAL accepted replay is not a mutation.
#[derive(PartialEq, Eq)]
pub(super) enum FolderProjection<'a> {
    Host { name: &'a str, group: Option<ObjectId>, revision: i64, origin: Option<VaultId> },
    Group { id: ObjectId, name: &'a str, parent: Option<ObjectId>, revision: i64, origin: Option<VaultId> },
}
fn projection(object: &VaultObject, revision: i64, origin: Option<VaultId>) -> Option<FolderProjection<'_>> {
    match object {
        VaultObject::Host(host) => Some(FolderProjection::Host { name: &host.name, group: host.group_id, revision, origin }),
        VaultObject::Group(group) => Some(FolderProjection::Group { id: group.id, name: &group.name, parent: group.parent_id, revision, origin }),
        _ => None,
    }
}
pub(super) fn entry_projection(entry: Option<&Entry>, origin: Option<VaultId>) -> Option<FolderProjection<'_>> {
    let entry = entry?;
    match &entry.item { Item::Object(object) => projection(object, entry.revision, origin), _ => None }
}
pub(super) fn readback_projection(readback: &Readback, origin: Option<VaultId>) -> Option<FolderProjection<'_>> {
    match readback { Readback::Live(d) => projection(&d.payload.object, d.revision, origin), _ => None }
}
pub(super) fn touch(object: &VaultObject, touched: &mut BTreeSet<ObjectId>) {
    match object {
        VaultObject::Host(host) => {
            if let Some(id) = host.group_id {
                touched.insert(id);
            }
        }
        VaultObject::Group(group) => {
            touched.insert(group.id);
        }
        _ => {}
    }
}
fn snapshot(g: &Inner, folders: &BTreeSet<ObjectId>) -> Option<FolderSnapshot> {
    if folders.is_empty() {
        return Some(FolderSnapshot {
            groups: BTreeMap::new(),
            hosts: BTreeMap::new(),
        });
    }
    if folders.len() > 64 || g.entries.len() > 50_000 {
        return None;
    }
    let mut groups = BTreeMap::new();
    let mut hosts = BTreeMap::new();
    for id in folders {
        let entry = g.entries.get(id)?;
        let Item::Object(object) = &entry.item else {
            return None;
        };
        let VaultObject::Group(group) = object.as_ref() else {
            return None;
        };
        if group.name.len() > 512 {
            return None;
        }
        groups.insert(
            *id,
            FolderGroup {
                name: group.name.clone(),
                parent_id: group.parent_id,
                revision: entry.revision,
                origin: g.origin.get(id).copied(),
            },
        );
    }
    for (id, entry) in &g.entries {
        let Item::Object(object) = &entry.item else {
            continue;
        };
        let VaultObject::Host(host) = object.as_ref() else {
            continue;
        };
        let Some(folder) = host.group_id.filter(|f| folders.contains(f)) else {
            continue;
        };
        if hosts.len() >= 10_000
            || host.name.len() > 512
            || g.origin.get(id).copied() != groups[&folder].origin
        {
            return None;
        }
        hosts.insert(
            *id,
            FolderHost {
                name: host.name.clone(),
                group_id: folder,
                revision: entry.revision,
                origin: g.origin.get(id).copied(),
            },
        );
    }
    Some(FolderSnapshot { groups, hosts })
}
impl WorkingSet {
    pub(crate) fn direct_folder_snapshot(
        &self,
        folders: &BTreeSet<ObjectId>,
    ) -> Option<FolderSnapshot> {
        snapshot(&self.read(), folders)
    }
    pub(crate) fn watch_folder_snapshot(
        &self,
        expected: &FolderSnapshot,
        cancel: CancellationToken,
    ) -> Option<FolderWatch> {
        let mut g = self.inner.write().unwrap_or_else(|p| p.into_inner());
        g.guards.retain(|guard| {
            !guard.cancel.is_cancelled()
                && guard.folder_watch.as_ref().is_none_or(|watch| {
                    watch
                        .upgrade()
                        .is_some_and(|s| s.active.load(Ordering::Acquire))
                })
        });
        let folders: BTreeSet<ObjectId> = expected.groups.keys().copied().collect();
        if folders.is_empty()
            || cancel.is_cancelled()
            || g.guards
                .iter()
                .filter(|guard| guard.folder_watch.is_some())
                .count()
                >= 16
            || snapshot(&g, &folders).as_ref() != Some(expected)
        {
            cancel.cancel();
            return None;
        }
        let state = Arc::new(FolderWatchState {
            folders,
            active: AtomicBool::new(true),
        });
        let revisions = expected
            .groups
            .iter()
            .map(|(id, row)| (*id, row.revision))
            .chain(expected.hosts.iter().map(|(id, row)| (*id, row.revision)))
            .collect();
        g.guards.push(RevisionGuard {
            expected: revisions,
            kube_connections: BTreeMap::new(),
            cancel,
            folder_watch: Some(Arc::downgrade(&state)),
        });
        Some(FolderWatch(state))
    }
    /// Exact pre-signature snapshot boundary, under the SAME mutation lock. Once
    /// consumed, later folder changes cannot expand/cancel immutable job targets;
    /// ordinary selected-route guards remain independently active.
    pub(crate) fn finish_folder_review(&self, watch: &FolderWatch) -> bool {
        let mut g = self.inner.write().unwrap_or_else(|p| p.into_inner());
        let found = g.guards.iter().any(|guard| {
            !guard.cancel.is_cancelled()
                && guard
                    .folder_watch
                    .as_ref()
                    .and_then(std::sync::Weak::upgrade)
                    .is_some_and(|state| {
                        Arc::ptr_eq(&state, &watch.0) && state.active.load(Ordering::Acquire)
                    })
        });
        if !found {
            return false;
        }
        watch.0.active.store(false, Ordering::Release);
        g.guards.retain(|guard| {
            guard
                .folder_watch
                .as_ref()
                .and_then(std::sync::Weak::upgrade)
                .is_none_or(|state| !Arc::ptr_eq(&state, &watch.0))
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_models::ObjectPayload;
    use cc_storage_core::LocalState;
    fn put(ws: &WorkingSet, object: VaultObject, revision: i64) {
        let id = object.id();
        ws.apply(
            id,
            Readback::Live(Box::new(DecryptedObject {
                object_id: id,
                revision,
                local_state: LocalState::LocalOnly,
                conflict_origin: None,
                payload: ObjectPayload::new(object),
            })),
        );
    }
    fn setup() -> (WorkingSet, Group, Host) {
        let ws = WorkingSet::new();
        let group = Group::new("Owned folder");
        let mut host = Host::new("Member", "192.0.2.1");
        host.group_id = Some(group.id);
        put(&ws, VaultObject::Group(group.clone()), 1);
        put(&ws, VaultObject::Host(host.clone()), 1);
        (ws, group, host)
    }
    #[test]
    fn new_member_revokes_only_folder_guard_and_existing_route_guard_is_unchanged() {
        let (ws, group, host) = setup();
        let snapshot = ws
            .direct_folder_snapshot(&BTreeSet::from([group.id]))
            .unwrap();
        let route = CancellationToken::new();
        assert!(ws.watch_revisions(BTreeMap::from([(host.id, 1)]), route.clone()));
        let folder = CancellationToken::new();
        let _watch = ws.watch_folder_snapshot(&snapshot, folder.clone()).unwrap();
        let mut added = Host::new("Added", "192.0.2.2");
        added.group_id = Some(group.id);
        put(&ws, VaultObject::Host(added), 1);
        assert!(folder.is_cancelled());
        assert!(!route.is_cancelled());
        assert_eq!(snapshot.hosts.len(), 1);
    }
    #[test]
    fn unrelated_changes_do_not_revoke_and_consumed_folder_guard_cannot_change_approved_targets() {
        let (ws, group, host) = setup();
        let snapshot = ws
            .direct_folder_snapshot(&BTreeSet::from([group.id]))
            .unwrap();
        let route = CancellationToken::new();
        assert!(ws.watch_revisions(BTreeMap::from([(host.id, 1)]), route.clone()));
        let folder = CancellationToken::new();
        let watch = ws.watch_folder_snapshot(&snapshot, folder.clone()).unwrap();
        put(&ws, VaultObject::Host(Host::new("Outside", "192.0.2.9")), 1);
        assert!(!folder.is_cancelled());
        assert!(ws.finish_folder_review(&watch));
        assert!(!ws.finish_folder_review(&watch));
        let mut added = Host::new("Post approval", "192.0.2.3");
        added.group_id = Some(group.id);
        put(&ws, VaultObject::Host(added), 1);
        assert!(!folder.is_cancelled());
        assert!(!route.is_cancelled());
        assert_eq!(snapshot.hosts.len(), 1);
    }
    #[test]
    fn capture_projects_only_bounded_credential_free_metadata_and_registration_is_cas() {
        let (ws, group, mut host) = setup();
        host.notes = "SYNTHETIC_NOT_FOR_PROJECTION".into();
        host.metadata
            .insert("private".into(), "SYNTHETIC_NOT_FOR_PROJECTION".into());
        put(&ws, VaultObject::Host(host.clone()), 2);
        let snapshot = ws
            .direct_folder_snapshot(&BTreeSet::from([group.id]))
            .unwrap();
        assert!(!format!("{snapshot:?}").contains("SYNTHETIC_NOT_FOR_PROJECTION"));
        host.group_id = None;
        put(&ws, VaultObject::Host(host), 3);
        let token = CancellationToken::new();
        assert!(ws.watch_folder_snapshot(&snapshot, token.clone()).is_none());
        assert!(token.is_cancelled());
    }
    #[test]
    fn folder_guard_admission_is_bounded_and_ordinary_route_revision_still_revokes() {
        let (ws, group, mut host) = setup();
        let snapshot = ws
            .direct_folder_snapshot(&BTreeSet::from([group.id]))
            .unwrap();
        let watches = (0..16)
            .map(|_| {
                ws.watch_folder_snapshot(&snapshot, CancellationToken::new())
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(ws
            .watch_folder_snapshot(&snapshot, CancellationToken::new())
            .is_none());
        for watch in watches {
            assert!(ws.finish_folder_review(&watch));
        }
        let route = CancellationToken::new();
        assert!(ws.watch_revisions(BTreeMap::from([(host.id, 1)]), route.clone()));
        host.address = "192.0.2.8".into();
        put(&ws, VaultObject::Host(host), 2);
        assert!(route.is_cancelled());
    }
    #[test]
    fn bulk_reload_revokes_only_preapproval_folder_interest_preserving_old_route_semantics() {
        let (ws, group, host) = setup();
        let captured = ws
            .direct_folder_snapshot(&BTreeSet::from([group.id]))
            .unwrap();
        let route = CancellationToken::new();
        assert!(ws.watch_revisions(BTreeMap::from([(host.id, 1)]), route.clone()));
        let folder = CancellationToken::new();
        let _watch = ws.watch_folder_snapshot(&captured, folder.clone()).unwrap();
        let objects = [VaultObject::Group(group), VaultObject::Host(host)]
            .into_iter()
            .map(|object| DecryptedObject {
                object_id: object.id(),
                revision: 1,
                local_state: LocalState::LocalOnly,
                conflict_origin: None,
                payload: ObjectPayload::new(object),
            })
            .collect();
        ws.replace_all(objects);
        assert!(folder.is_cancelled());
        assert!(!route.is_cancelled());
    }

    #[test]
    fn identical_and_stale_readbacks_keep_folder_watch_and_original_origin() {
        let (ws,group,host)=setup();
        put(&ws,VaultObject::Host(host.clone()),2);
        let expected=ws.direct_folder_snapshot(&BTreeSet::from([group.id])).unwrap();
        let token=CancellationToken::new();let watch=ws.watch_folder_snapshot(&expected,token.clone()).unwrap();
        let route=CancellationToken::new();assert!(ws.watch_revisions(BTreeMap::from([(host.id,2)]),route.clone()));
        put(&ws,VaultObject::Host(host.clone()),2);
        put(&ws,VaultObject::Group(group.clone()),1);
        let foreign=VaultId::new();
        let mut stale=host.clone();stale.group_id=None;stale.name="stale foreign payload".into();
        assert!(ws.apply_in(Some(foreign),host.id,Readback::Live(Box::new(DecryptedObject {
            object_id:host.id,revision:1,local_state:LocalState::LocalOnly,conflict_origin:None,
            payload:ObjectPayload::new(VaultObject::Host(stale)),
        }))).is_none());
        assert!(ws.apply_in(Some(foreign),host.id,Readback::Deleted{revision:1}).is_none());
        assert_eq!(ws.vault_of(host.id),None);
        assert_eq!(ws.host(host.id).unwrap(),host);
        assert_eq!(ws.direct_folder_snapshot(&BTreeSet::from([group.id])).unwrap(),expected);
        assert!(!token.is_cancelled());assert!(!route.is_cancelled());assert!(ws.finish_folder_review(&watch));
    }

    #[test]
    fn same_revision_changed_name_parent_membership_type_or_origin_still_revokes() {
        for change in 0..6 {
            let (ws,mut group,mut host)=setup();
            let expected=ws.direct_folder_snapshot(&BTreeSet::from([group.id])).unwrap();
            let token=CancellationToken::new();let _watch=ws.watch_folder_snapshot(&expected,token.clone()).unwrap();
            let route=CancellationToken::new();assert!(ws.watch_revisions(BTreeMap::from([(host.id,1)]),route.clone()));
            match change {
                0=>{host.name="new metadata at same revision".into();put(&ws,VaultObject::Host(host),1);},
                1=>{host.group_id=None;put(&ws,VaultObject::Host(host),1);},
                2=>{group.parent_id=Some(Group::new("owned parent ID").id);put(&ws,VaultObject::Group(group),1);},
                3=>{group.name="changed folder name".into();put(&ws,VaultObject::Group(group),1);},
                4=>{let mut rdp=cc_models::rdp::RdpHost::new("changed variant","192.0.2.5");rdp.id=host.id;rdp.group_id=Some(group.id);put(&ws,VaultObject::RdpHost(rdp),1);},
                _=>{ws.apply_in(Some(VaultId::new()),host.id,Readback::Live(Box::new(DecryptedObject {
                    object_id:host.id,revision:1,local_state:LocalState::LocalOnly,conflict_origin:None,
                    payload:ObjectPayload::new(VaultObject::Host(host)),
                })));},
            }
            assert!(token.is_cancelled(),"actual projected change {change}");
            assert!(!route.is_cancelled(),"ordinary equal-revision semantics unchanged {change}");
        }
    }

    #[test]
    fn actual_delete_revokes_but_stale_live_cannot_resurrect_or_change_origin() {
        let (ws,group,host)=setup();
        let snapshot=ws.direct_folder_snapshot(&BTreeSet::from([group.id])).unwrap();
        let first=CancellationToken::new();let _watch=ws.watch_folder_snapshot(&snapshot,first.clone()).unwrap();
        let route=CancellationToken::new();assert!(ws.watch_revisions(BTreeMap::from([(host.id,1)]),route.clone()));
        ws.apply(host.id,Readback::Deleted{revision:3});assert!(first.is_cancelled());assert!(route.is_cancelled());
        let empty=ws.direct_folder_snapshot(&BTreeSet::from([group.id])).unwrap();
        let second=CancellationToken::new();let watch=ws.watch_folder_snapshot(&empty,second.clone()).unwrap();
        assert!(ws.apply_in(Some(VaultId::new()),host.id,Readback::Live(Box::new(DecryptedObject {
            object_id:host.id,revision:2,local_state:LocalState::LocalOnly,conflict_origin:None,
            payload:ObjectPayload::new(VaultObject::Host(host.clone())),
        }))).is_none());
        assert!(ws.host(host.id).is_none());assert_eq!(ws.vault_of(host.id),None);assert!(!second.is_cancelled());assert!(ws.finish_folder_review(&watch));
    }

    #[test]
    fn route_only_same_revision_readback_keeps_folder_metadata_projection() {
        let (ws,group,mut host)=setup();
        let captured=ws.direct_folder_snapshot(&BTreeSet::from([group.id])).unwrap();
        let token=CancellationToken::new();let watch=ws.watch_folder_snapshot(&captured,token.clone()).unwrap();
        host.address="192.0.2.222".into();host.notes="not folder metadata".into();
        put(&ws,VaultObject::Host(host.clone()),1);
        assert_eq!(ws.host(host.id).unwrap(),host,"equal-revision payload replacement preserved");
        assert_eq!(ws.direct_folder_snapshot(&BTreeSet::from([group.id])).unwrap(),captured);
        assert!(!token.is_cancelled());assert!(ws.finish_folder_review(&watch));
    }
}
