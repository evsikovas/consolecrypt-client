use super::*;
use cc_models::{group::Group, host::Host, VaultObject};
struct Fixture {
    _dir: tempfile::TempDir,
    app: AppCore,
    project: String,
    folder: Group,
    host: Host,
}
impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            dir.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile(
            "Owned folder mapping fixture".into(),
            "synthetic profile passphrase 42".into(),
        )
        .await
        .unwrap();
        let folder = Group::new("Docker folder");
        let mut host = Host::new("Docker SSH", "192.0.2.1");
        host.group_id = Some(folder.id);
        let (_, u) = app.unlocked().await.unwrap();
        u.writer
            .put(VaultObject::Group(folder.clone()))
            .await
            .unwrap();
        u.writer.put(VaultObject::Host(host.clone())).await.unwrap();
        drop(u);
        let project = app
            .automation_project_create("Folder binding project".into(), None)
            .await
            .unwrap();
        Self {
            _dir: dir,
            app,
            project: project.id,
            folder,
            host,
        }
    }
    async fn action(&self, revision: u64, kind: &str) -> AppResult<Value> {
        let (owner, u) = self.app.unlocked().await?;
        let profile_id = owner.profile_id.to_string();
        let notification_scope_id = u.notices.scope_id().ok_or(AppError::VaultLocked)?;
        drop(u);
        self.app.automation_project_folder_binding_json(json!({"version":1,"project_id":self.project,"expected_revision":revision,"profile_id":profile_id,"notification_scope_id":notification_scope_id,"action":{"type":kind,"name":"docker_hosts","folder_id":self.folder.id}}).to_string()).await
    }
    async fn composition(&self) -> Composition {
        let context = self
            .app
            .automation_project_context(&self.project)
            .await
            .unwrap();
        capture(&self.app, &context).await.unwrap().unwrap()
    }
}
#[tokio::test]
async fn live_direct_ids_are_profile_local_cas_and_never_move_or_grant_hosts() {
    let f = Fixture::new().await;
    let changed = f.action(1, "link_direct").await.unwrap();
    assert_eq!(changed["project"]["revision"], 2);
    assert!(f.action(1, "link_direct").await.is_err());
    let captured = f.composition().await;
    let host = Uuid::parse_str(&f.host.id.to_string()).unwrap();
    assert_eq!(captured.inventory.groups["docker_hosts"], vec![host]);
    assert!(
        captured.inventory.runtime_groups_for_hosts(&[]).unwrap()["docker_hosts"]["hosts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let (_, u) = f.app.unlocked().await.unwrap();
    assert_eq!(u.working().host(f.host.id).unwrap(), f.host);
    drop(u);
    let read = f
        .app
        .automation_project_inventory(f.project.clone())
        .await
        .unwrap();
    assert_eq!(read["inventory"]["groups"]["docker_hosts"], json!([]));
    assert_eq!(
        read["folder_mapping"]["bindings"]["groups"]["docker_hosts"]["folder_id"],
        f.folder.id.to_string()
    );
    f.app
        .create_local_profile(
            "Other profile".into(),
            "synthetic profile passphrase 42".into(),
        )
        .await
        .unwrap();
    assert!(f.app.automation_project_inventory(f.project).await.is_err());
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn added_moved_and_folder_revision_changes_require_fresh_review_without_expanding_capture() {
    let f = Fixture::new().await;
    f.action(1, "link_direct").await.unwrap();
    let mut captured = f.composition().await;
    let (_, u) = f.app.unlocked().await.unwrap();
    let token = tokio_util::sync::CancellationToken::new();
    captured.watch(&u, token.clone()).unwrap();
    let mut next = Host::new("Added SSH", "192.0.2.2");
    next.group_id = Some(f.folder.id);
    u.writer.put(VaultObject::Host(next.clone())).await.unwrap();
    assert!(
        token.is_cancelled(),
        "new member must revoke before working-set mutation unlock"
    );
    assert!(captured.check(&u).is_err());
    assert_eq!(captured.inventory.groups["docker_hosts"].len(), 1);
    drop(u);
    let fresh = f.composition().await;
    assert_eq!(fresh.inventory.groups["docker_hosts"].len(), 2);
    let (_, u) = f.app.unlocked().await.unwrap();
    next.group_id = None;
    u.writer.put(VaultObject::Host(next)).await.unwrap();
    assert!(fresh.check(&u).is_err());
    drop(u);
    let fresh = f.composition().await;
    let (_, u) = f.app.unlocked().await.unwrap();
    let mut folder = f.folder.clone();
    folder.name = "Renamed same folder identity".into();
    u.writer.put(VaultObject::Group(folder)).await.unwrap();
    assert!(fresh.check(&u).is_err());
    drop(u);
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn snapshot_is_explicit_membership_and_live_legacy_edit_requires_unlink() {
    let f = Fixture::new().await;
    f.action(1, "link_direct").await.unwrap();
    assert!(f
        .app
        .automation_project_save_inventory(
            f.project.clone(),
            2,
            json!({"version":1,"groups":{}}).to_string()
        )
        .await
        .is_err());
    let result = f.action(2, "snapshot_direct").await.unwrap();
    assert_eq!(result["project"]["revision"], 3);
    let context = f.app.automation_project_context(&f.project).await.unwrap();
    assert!(capture(&f.app, &context).await.unwrap().is_none());
    let (_, u) = f.app.unlocked().await.unwrap();
    let mut moved = f.host.clone();
    moved.group_id = None;
    u.writer.put(VaultObject::Host(moved)).await.unwrap();
    drop(u);
    let view = f
        .app
        .automation_project_inventory(f.project.clone())
        .await
        .unwrap();
    assert_eq!(
        view["inventory"]["groups"]["docker_hosts"],
        json!([f.host.id])
    );
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn missing_folder_is_visible_unlinkable_and_never_falls_back() {
    let f = Fixture::new().await;
    f.action(1, "link_direct").await.unwrap();
    let (owner, _) = f.app.unlocked().await.unwrap();
    let id = f.project.clone();
    let missing = Uuid::new_v4();
    owner
        .storage()
        .unwrap()
        .write(move |tx| {
            let mut catalog: Catalog = tx.setting_get(PROJECTS)?.unwrap();
            catalog.records.get_mut(&id).unwrap()[FIELD]["groups"]["docker_hosts"]["folder_id"] =
                json!(missing);
            tx.setting_set(PROJECTS, &catalog)
        })
        .await
        .unwrap();
    let context = f.app.automation_project_context(&f.project).await.unwrap();
    assert!(capture(&f.app, &context).await.is_err());
    let view = f
        .app
        .automation_project_inventory(f.project.clone())
        .await
        .unwrap();
    assert_eq!(view["folder_mapping"]["status"], "unavailable");
    let (_, current) = f.app.unlocked().await.unwrap();
    let notification_scope_id = current.notices.scope_id().unwrap();
    drop(current);
    f.app.automation_project_folder_binding_json(json!({"version":1,"project_id":f.project,"expected_revision":2,"profile_id":owner.profile_id.to_string(),"notification_scope_id":notification_scope_id,"action":{"type":"unlink","name":"docker_hosts"}}).to_string()).await.unwrap();
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn final_sql_metadata_fence_denies_changed_link_and_future_is_raw_preserved() {
    let f = Fixture::new().await;
    f.action(1, "link_direct").await.unwrap();
    let captured = f.composition().await;
    let (owner, u) = f.app.unlocked().await.unwrap();
    confirm_metadata(&owner, &f.project, 2, Some(&captured))
        .await
        .unwrap();
    let id = f.project.clone();
    let raw = json!({"version":77,"groups":{},"opaque":{"preserve":true}});
    let saved = raw.clone();
    owner
        .storage()
        .unwrap()
        .write(move |tx| {
            let mut catalog: Catalog = tx.setting_get(PROJECTS)?.unwrap();
            catalog.records.get_mut(&id).unwrap()[FIELD] = saved;
            tx.setting_set(PROJECTS, &catalog)
        })
        .await
        .unwrap();
    assert!(confirm_metadata(&owner, &f.project, 2, Some(&captured))
        .await
        .is_err());
    assert!(f.action(2, "snapshot_direct").await.is_err());
    let catalog: Catalog = owner
        .storage()
        .unwrap()
        .setting_get(PROJECTS)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(catalog.records[&f.project][FIELD], raw);
    drop(u);
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn old_ui_frame_cannot_mutate_another_profile_even_with_cloned_project_and_folder_ids() {
    let f = Fixture::new().await;
    let (owner, u) = f.app.unlocked().await.unwrap();
    let raw=json!({"version":1,"project_id":f.project,"expected_revision":1,"profile_id":owner.profile_id.to_string(),"notification_scope_id":u.notices.scope_id().unwrap(),"action":{"type":"link_direct","name":"docker_hosts","folder_id":f.folder.id}}).to_string();
    let catalog: Catalog = owner
        .storage()
        .unwrap()
        .setting_get(PROJECTS)
        .await
        .unwrap()
        .unwrap();
    drop(u);
    f.app
        .create_local_profile(
            "Owned cloned IDs profile".into(),
            "synthetic profile passphrase 42".into(),
        )
        .await
        .unwrap();
    let (other, u) = f.app.unlocked().await.unwrap();
    u.writer
        .put(VaultObject::Group(f.folder.clone()))
        .await
        .unwrap();
    u.writer
        .put(VaultObject::Host(f.host.clone()))
        .await
        .unwrap();
    let copy = catalog.clone();
    other
        .storage()
        .unwrap()
        .write(move |tx| tx.setting_set(PROJECTS, &copy))
        .await
        .unwrap();
    assert!(f
        .app
        .automation_project_folder_binding_json(raw)
        .await
        .is_err());
    let after: Catalog = other
        .storage()
        .unwrap()
        .setting_get(PROJECTS)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(after).unwrap(),
        serde_json::to_value(catalog).unwrap()
    );
    drop(u);
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn old_unlock_frame_is_not_rebound_after_lock_unlock_same_profile() {
    let f = Fixture::new().await;
    let (owner, u) = f.app.unlocked().await.unwrap();
    let raw=json!({"version":1,"project_id":f.project,"expected_revision":1,"profile_id":owner.profile_id.to_string(),"notification_scope_id":u.notices.scope_id().unwrap(),"action":{"type":"link_direct","name":"docker_hosts","folder_id":f.folder.id}}).to_string();
    drop(u);
    f.app.lock().await.unwrap();
    f.app
        .unlock_with_passphrase("synthetic profile passphrase 42".into())
        .await
        .unwrap();
    assert!(f
        .app
        .automation_project_folder_binding_json(raw)
        .await
        .is_err());
    assert_eq!(
        f.app
            .automation_project_inventory(f.project.clone())
            .await
            .unwrap()["project_revision"],
        1
    );
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn same_profile_close_reopen_generation_collision_does_not_rebind_old_frame() {
    let f = Fixture::new().await;
    let (owner, u) = f.app.unlocked().await.unwrap();
    let profile = owner.profile_id.to_string();
    drop(u);
    // Compare TWO identical reopen paths, rather than assuming creation has the
    // same initialization sequence as loading an existing profile.
    f.app.close_profile().await.unwrap();
    f.app.open_profile(profile.clone()).await.unwrap();
    f.app
        .unlock_with_passphrase("synthetic profile passphrase 42".into())
        .await
        .unwrap();
    let (owner, u) = f.app.unlocked().await.unwrap();
    let generation = owner.automation.unlocked_generation().unwrap();
    let nonce = u.notices.scope_id().unwrap();
    let raw=json!({"version":1,"project_id":f.project,"expected_revision":1,"profile_id":profile,"notification_scope_id":nonce,"action":{"type":"link_direct","name":"docker_hosts","folder_id":f.folder.id}}).to_string();
    drop(u);
    f.app.close_profile().await.unwrap();
    f.app.open_profile(profile).await.unwrap();
    f.app
        .unlock_with_passphrase("synthetic profile passphrase 42".into())
        .await
        .unwrap();
    let (current, u) = f.app.unlocked().await.unwrap();
    assert_eq!(
        current.automation.unlocked_generation().unwrap(),
        generation,
        "real same-profile session generation collision required"
    );
    assert_ne!(u.notices.scope_id(), Some(nonce));
    drop(u);
    assert!(f
        .app
        .automation_project_folder_binding_json(raw)
        .await
        .is_err());
    assert_eq!(
        f.app
            .automation_project_inventory(f.project.clone())
            .await
            .unwrap()["project_revision"],
        1
    );
    f.app.shutdown().await.unwrap();
}

#[test]
fn strict_mapping_schema_has_no_paths_commands_descendant_or_approval_authority() {
    for action in [
        json!({"type":"link_direct","name":"docker_hosts","folder_id":Uuid::new_v4(),"path":"/outside"}),
        json!({"type":"link_direct","name":"docker_hosts","folder_id":Uuid::new_v4(),"include_descendants":true}),
        json!({"type":"exec","command":"anything"}),
    ] {
        assert!(serde_json::from_value::<Request>(
            json!({"version":1,"project_id":Uuid::new_v4(),"expected_revision":1,"profile_id":"owned-profile","notification_scope_id":Uuid::new_v4(),"action":action})
        )
        .is_err());
    }
}

#[tokio::test]
async fn duplicate_mapping_fields_are_rejected_without_changing_an_otherwise_valid_request() {
    let f = Fixture::new().await;
    let (owner, u) = f.app.unlocked().await.unwrap();
    let raw = json!({
        "version": 1,
        "project_id": f.project,
        "expected_revision": 1,
        "profile_id": owner.profile_id.to_string(),
        "notification_scope_id": u.notices.scope_id().unwrap(),
        "action": {"type": "link_direct", "name": "docker_hosts", "folder_id": f.folder.id}
    }).to_string();
    drop(u);
    assert!(serde_json::from_str::<Request>(&raw).is_ok());
    let before = f.app.automation_project_inventory(f.project.clone()).await.unwrap();
    for (field, duplicate) in [
        (r#""version":1"#, r#""version":1,"version":1"#),
        (r#""version":1"#, r#""version":1,"\u0076ersion":1"#),
        (r#""name":"docker_hosts""#, r#""name":"docker_hosts","name":"docker_hosts""#),
        (r#""type":"link_direct""#, r#""type":"link_direct","type":"link_direct""#),
    ] {
        let malformed = raw.replacen(field, duplicate, 1);
        assert_ne!(malformed, raw);
        assert!(f.app.automation_project_folder_binding_json(malformed).await.is_err());
        assert_eq!(f.app.automation_project_inventory(f.project.clone()).await.unwrap(), before);
    }
    let accepted = f.app.automation_project_folder_binding_json(raw).await.unwrap();
    assert_eq!(accepted["project"]["revision"], 2);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn direct_members_exclude_child_folder_and_non_ssh_objects() {
    let f = Fixture::new().await;
    let (_, u) = f.app.unlocked().await.unwrap();
    let mut child = Group::new("Child folder");
    child.parent_id = Some(f.folder.id);
    u.writer
        .put(VaultObject::Group(child.clone()))
        .await
        .unwrap();
    let mut child_host = Host::new("Child SSH is not direct", "192.0.2.3");
    child_host.group_id = Some(child.id);
    u.writer.put(VaultObject::Host(child_host)).await.unwrap();
    let mut rdp = cc_models::rdp::RdpHost::new("RDP is not SSH", "192.0.2.4");
    rdp.group_id = Some(f.folder.id);
    u.writer.put(VaultObject::RdpHost(rdp)).await.unwrap();
    drop(u);
    f.action(1, "link_direct").await.unwrap();
    let capture = f.composition().await;
    assert_eq!(
        capture.inventory.groups["docker_hosts"],
        vec![Uuid::parse_str(&f.host.id.to_string()).unwrap()]
    );
    assert_eq!(capture.view["descendants_included"], false);
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn locked_profile_denies_mapping_and_capture_cannot_rebind_after_unlock() {
    let f = Fixture::new().await;
    f.action(1, "link_direct").await.unwrap();
    let context = f.app.automation_project_context(&f.project).await.unwrap();
    f.app.lock().await.unwrap();
    assert!(f.action(2, "snapshot_direct").await.is_err());
    assert!(capture(&f.app, &context).await.is_err());
    f.app
        .unlock_with_passphrase("synthetic profile passphrase 42".into())
        .await
        .unwrap();
    assert!(capture(&f.app, &context).await.is_err());
    assert!(f
        .app
        .automation_project_inventory(f.project.clone())
        .await
        .is_ok());
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn queued_sql_action_tolerates_identical_and_unrelated_actual_readbacks() {
    use std::{future::Future, task::Poll};
    use cc_models::ObjectPayload;
    use cc_storage_core::LocalState;
    use cc_sync_core::DecryptedObject;
    use crate::working_set::Readback;
    // Current-thread runtime: no other task can interleave between checking
    // the Native mutex and synchronously polling this action to its SQL wait.
    for action_kind in ["link_direct", "snapshot_direct"] {
        let f=Fixture::new().await;
        let (owner,u)=f.app.unlocked().await.unwrap();
        let storage=owner.storage().unwrap();
        let (entered_tx,entered_rx)=tokio::sync::oneshot::channel();
        let (release_tx,release_rx)=std::sync::mpsc::channel();
        let blocker=tokio::spawn(async move {
            storage.write(move|_tx|->AppResult<()> {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(std::time::Duration::from_secs(10))
                    .map_err(|_|AppError::internal("owned SQL barrier expired"))?;
                Ok(())
            }).await
        });
        tokio::time::timeout(std::time::Duration::from_secs(10),entered_rx).await.unwrap().unwrap();
        let working=u.working().clone();
        let version=working.version();
        assert!(f.app.inner.active.try_lock().is_ok());
        let action=f.action(1,action_kind);
        tokio::pin!(action);
        std::future::poll_fn(|cx| match action.as_mut().poll(cx) {
            Poll::Pending=>Poll::Ready(()),
            Poll::Ready(_)=>panic!("actual action cannot finish while its SQL worker is blocked"),
        }).await;
        assert!(f.app.inner.active.try_lock().is_err(),"actual action owns original session across its queued SQL write");
        let host=working.host(f.host.id).unwrap();
        let revision=working.revision_of(host.id).unwrap();
        working.apply(host.id,Readback::Live(Box::new(DecryptedObject {
            object_id:host.id,revision,local_state:LocalState::LocalOnly,conflict_origin:None,
            payload:ObjectPayload::new(VaultObject::Host(host)),
        })));
        let outside=Host::new("Unrelated readback","192.0.2.77");
        working.apply(outside.id,Readback::Live(Box::new(DecryptedObject {
            object_id:outside.id,revision:1,local_state:LocalState::LocalOnly,conflict_origin:None,
            payload:ObjectPayload::new(VaultObject::Host(outside)),
        })));
        assert_ne!(working.version(),version,"causal coarse-version regression condition");
        release_tx.send(()).unwrap();
        let result=tokio::time::timeout(std::time::Duration::from_secs(10),&mut action).await.unwrap().unwrap();
        blocker.await.unwrap().unwrap();
        assert_eq!(result["project"]["revision"],2);
        let composition=&result["composition"]["groups"]["docker_hosts"]["hosts"];
        if action_kind=="link_direct" {
            assert_eq!(composition.as_array().unwrap().len(),1);
        } else {
            assert_eq!(result["inventory"]["groups"]["docker_hosts"],json!([f.host.id.to_string()]));
        }
        assert!(!result.to_string().contains("Unrelated readback"));
        drop(action);drop(u);
        f.app.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn snapshot_direct_retains_selected_folder_fence_after_its_live_binding_is_removed() {
    let f=Fixture::new().await;
    let (_,u)=f.app.unlocked().await.unwrap();
    let mut inventory=ProjectInventory::default();
    inventory.groups.insert("docker_hosts".into(),vec![]);
    let proposed=Document{version:1,groups:BTreeMap::from([("docker_hosts".into(),Binding{folder_id:Uuid::parse_str(&f.folder.id.to_string()).unwrap(),mode:Mode::Direct})])};
    let action=resolve(&u,&inventory,&proposed).unwrap();
    inventory.groups=action.inventory.groups.clone();
    let remaining=resolve(&u,&inventory,&Document::default()).unwrap();
    check_action_compositions(&u,Some(&action),Some(&remaining)).unwrap();
    let mut added=Host::new("After copied membership","192.0.2.88");added.group_id=Some(f.folder.id);
    u.writer.put(VaultObject::Host(added)).await.unwrap();
    // Final document alone no longer mentions this folder. The actual
    // production predicate MUST retain and repeat the selected action capture.
    assert!(check_action_compositions(&u,None,Some(&remaining)).is_ok());
    assert!(check_action_compositions(&u,Some(&action),Some(&remaining)).is_err());
    assert_eq!(action.inventory.groups["docker_hosts"].len(),1);
    drop(u);f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn oversized_snapshot_and_live_composition_never_persist_unreadable_inventory() {
    let f = Fixture::new().await;
    let (owner, u) = f.app.unlocked().await.unwrap();
    // Real typed production readbacks, no network/credentials/fabricated approval.
    // Only one SQL action is attempted; avoid thousands of irrelevant disk writes.
    for n in 0..3500 {
        let mut host = Host::new(format!("Owned budget member {n}"), "192.0.2.9");
        host.group_id = Some(f.folder.id);
        let id = host.id;
        u.working().apply(
            id,
            crate::working_set::Readback::Live(Box::new(cc_sync_core::DecryptedObject {
                object_id: id,
                revision: 1,
                local_state: cc_storage_core::LocalState::LocalOnly,
                conflict_origin: None,
                payload: cc_models::ObjectPayload::new(VaultObject::Host(host)),
            })),
        );
    }
    let before: Catalog = owner
        .storage()
        .unwrap()
        .setting_get(PROJECTS)
        .await
        .unwrap()
        .unwrap();
    assert!(f.action(1, "snapshot_direct").await.is_err());
    assert!(f.action(1, "link_direct").await.is_err());
    let after: Catalog = owner
        .storage()
        .unwrap()
        .setting_get(PROJECTS)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(after).unwrap()
    );
    assert_eq!(
        f.app
            .automation_project_inventory(f.project.clone())
            .await
            .unwrap()["project_revision"],
        1
    );
    drop(u);
    f.app.shutdown().await.unwrap();
}
