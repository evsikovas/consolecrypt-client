use super::*;
mod legacy;
const PASSWORD: &str = "owned-project-environment-test42";
struct Fixture {
    _dir: tempfile::TempDir,
    app: AppCore,
    project: String,
    host: String,
    global: String,
    selected: String,
}
impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            dir.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile("Owned project credential policy".into(), PASSWORD.into())
            .await
            .unwrap();
        let global = app
            .add_password_credential(
                "Global metadata".into(),
                Some("globaluser".into()),
                "synthetic-global-secret42".into(),
            )
            .await
            .unwrap()
            .id;
        let selected = app
            .add_password_credential(
                "Project metadata".into(),
                Some("projectuser".into()),
                "synthetic-selected-secret42".into(),
            )
            .await
            .unwrap()
            .id;
        let mut host = crate::HostDto::new("Owned undialed host", "127.0.0.1");
        host.credential_id = Some(global.clone());
        host.port = Some(2222);
        let host = app.save_host(host).await.unwrap().id;
        let project = app
            .automation_project_create("Owned environment source".into(), None)
            .await
            .unwrap()
            .id;
        app.automation_project_save(
            project.clone(),
            "site.yml".into(),
            None,
            "---\n- hosts: all\n  tasks: []\n".into(),
        )
        .await
        .unwrap();
        app.automation_project_save_inventory(
            project.clone(),
            1,
            json!({"version":2,"groups":{"owned":[host]},"selected_hosts":[host]}).to_string(),
        )
        .await
        .unwrap();
        Self {
            _dir: dir,
            app,
            project,
            host,
            global,
            selected,
        }
    }
    async fn row(&self) -> Value {
        let (owner, u) = self.app.unlocked().await.unwrap();
        drop(u);
        let catalog: Catalog = owner
            .storage()
            .unwrap()
            .setting_get(PROJECTS)
            .await
            .unwrap()
            .unwrap();
        catalog.records[&self.project].clone()
    }
    fn policy(&self, production: bool, credential: &str) -> Value {
        json!({"version":1,"environments":[{"id":Uuid::new_v4(),"name":"Owned environment","production":production,
            "overrides":[{"host_id":self.host,"credential_id":credential}]}]})
    }
    async fn save(&self, policy: Value) -> Value {
        let context = self
            .app
            .automation_project_credentials_context(self.project.clone())
            .await
            .unwrap();
        self.app
            .automation_project_credentials_save(
                json!({"version":1,"frame":context["frame"],"policy":policy}).to_string(),
            )
            .await
            .unwrap()
    }
    fn selection(saved: &Value) -> Selection {
        serde_json::from_value(json!({"version":1,"frame":saved["frame"],"environment_id":saved["policy"]["environments"][0]["id"]})).unwrap()
    }
}

#[tokio::test]
async fn actual_project_selection_changes_only_native_target_credential_and_username() {
    let f = Fixture::new().await;
    let saved = f.save(f.policy(false, &f.selected)).await;
    let context = f.app.automation_project_context(&f.project).await.unwrap();
    let frozen = freeze(
        &f.app,
        &context,
        Some(&Fixture::selection(&saved)),
        &[f.host.clone()],
    )
    .await
    .unwrap()
    .unwrap();
    let host = crate::dto::parse_id("host_id", &f.host).unwrap();
    let selected = crate::dto::parse_id("credential_id", &f.selected).unwrap();
    assert_eq!(
        frozen.plans[&host].credential.as_ref().unwrap().id,
        selected
    );
    assert_eq!(frozen.plans[&host].username, "projectuser");
    let (owner, u) = f.app.unlocked().await.unwrap();
    assert_eq!(
        crate::inventory::host_of(&u, host)
            .unwrap()
            .credential_id
            .unwrap()
            .to_string(),
        f.global
    );
    assert_eq!(u.ssh.plan(host).await.unwrap().username, "globaluser");
    assert!(frozen.current(&u));
    assert!(frozen.revisions.contains_key(&selected));
    assert!(owner.automation.reviews.lock().unwrap().is_empty());
    drop(u);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn intentional_same_credential_reuse_is_allowed_and_legacy_without_policy_is_unchanged() {
    let f = Fixture::new().await;
    let context = f.app.automation_project_context(&f.project).await.unwrap();
    assert!(
        freeze(&f.app, &context, None, &[f.host.clone()])
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(f.row().await["version"], 1);
    let saved = f.save(f.policy(false, &f.global)).await;
    let context = f.app.automation_project_context(&f.project).await.unwrap();
    let frozen = freeze(
        &f.app,
        &context,
        Some(&Fixture::selection(&saved)),
        &[f.host.clone()],
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(frozen.environment.overrides[0].credential_id, f.global);
    f.app.shutdown().await.unwrap();
}

#[test]
fn native_notice_nonce_is_opaque_exact_utf8_not_a_uuid_or_trimmed_token() {
    let base = Frame {
        version: 1,
        profile_id: Uuid::new_v4().to_string(),
        notification_scope_id: " native/epoch:opaque 🚦 ".into(),
        project_id: Uuid::new_v4().to_string(),
        project_revision: 1,
        policy_sha256: "a".repeat(64),
    };
    assert!(base.validate().is_ok());
    for nonce in [
        "".to_string(),
        "x".repeat(257),
        "bad\nnonce".into(),
        "🚦".repeat(65),
    ] {
        assert!(
            Frame {
                notification_scope_id: nonce,
                ..base.clone()
            }
            .validate()
            .is_err()
        );
    }
}

#[tokio::test]
async fn real_lock_unlock_retires_old_setting_and_run_frames_without_any_catalog_write() {
    let f = Fixture::new().await;
    let saved = f.save(f.policy(false, &f.selected)).await;
    let selected = Fixture::selection(&saved);
    let before = f.row().await;
    f.app.lock().await.unwrap();
    f.app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    assert!(
        f.app
            .automation_project_credentials_save(
                json!({"version":1,"frame":saved["frame"],"policy":saved["policy"]}).to_string()
            )
            .await
            .is_err()
    );
    let context = f.app.automation_project_context(&f.project).await.unwrap();
    assert!(
        freeze(&f.app, &context, Some(&selected), &[f.host.clone()])
            .await
            .is_err()
    );
    assert_eq!(f.row().await, before);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn row2_old_real_rename_and_file_mutators_deny_before_writes_new_mutators_preserve_refs() {
    let f = Fixture::new().await;
    let legacy = f.row().await;
    legacy::rename(
        &f.app,
        f.project.clone(),
        legacy["revision"].as_u64().unwrap(),
        "Old supported baseline".into(),
    )
    .await
    .unwrap();
    let old_file = f
        .app
        .automation_project_file(f.project.clone(), "site.yml".into())
        .await
        .unwrap();
    legacy::save(
        &f.app,
        f.project.clone(),
        "site.yml".into(),
        Some(old_file.sha256),
        "---\n- hosts: all\n  tasks: []\n# old supported baseline\n".into(),
    )
    .await
    .unwrap();
    let saved = f.save(f.policy(false, &f.selected)).await;
    let before = f.row().await;
    assert_eq!(before["version"], 2);
    let file = f
        .app
        .automation_project_file(f.project.clone(), "site.yml".into())
        .await
        .unwrap();
    assert!(
        legacy::rename(
            &f.app,
            f.project.clone(),
            before["revision"].as_u64().unwrap(),
            "Must not write".into()
        )
        .await
        .is_err()
    );
    assert!(
        legacy::save(
            &f.app,
            f.project.clone(),
            "site.yml".into(),
            Some(file.sha256.clone()),
            "must not write".into()
        )
        .await
        .is_err()
    );
    assert_eq!(f.row().await, before);
    assert_eq!(
        f.app
            .automation_project_file(f.project.clone(), "site.yml".into())
            .await
            .unwrap()
            .sha256,
        file.sha256
    );
    f.app
        .automation_project_rename(
            f.project.clone(),
            before["revision"].as_u64().unwrap(),
            "New supported rename".into(),
        )
        .await
        .unwrap();
    f.app
        .automation_project_save(
            f.project.clone(),
            "site.yml".into(),
            Some(file.sha256),
            "---\n- hosts: all\n  tasks: []\n# current editor\n".into(),
        )
        .await
        .unwrap();
    let current = f.row().await;
    f.app
        .automation_project_save_inventory(
            f.project.clone(),
            current["revision"].as_u64().unwrap(),
            json!({"version":2,"groups":{"owned":[f.host]},"selected_hosts":[f.host]}).to_string(),
        )
        .await
        .unwrap();
    // Another project/catalog insertion preserves the original row's complete
    // bindings; package import uses the same bounded Catalog Value semantics.
    f.app
        .automation_project_create("Other owned project".into(), None)
        .await
        .unwrap();
    let after = f.row().await;
    assert_eq!(after["version"], 2);
    assert_eq!(after[FIELD], saved["policy"]);
    assert!(
        !f.app
            .automation_projects()
            .await
            .unwrap()
            .projects
            .iter()
            .find(|p| p.id == f.project)
            .unwrap()
            .read_only
    );
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn stale_row_or_busy_existing_prepare_slots_refuse_policy_update_without_clearing_refs() {
    let f = Fixture::new().await;
    let context = f
        .app
        .automation_project_credentials_context(f.project.clone())
        .await
        .unwrap();
    let before = f.row().await;
    let (owner, u) = f.app.unlocked().await.unwrap();
    drop(u);
    let slot = owner
        .automation
        .prepare_slots
        .clone()
        .try_acquire_owned()
        .unwrap();
    let request =
        json!({"version":1,"frame":context["frame"],"policy":f.policy(false,&f.selected)})
            .to_string();
    let error = f
        .app
        .automation_project_credentials_save(request.clone())
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("project_environment_prepare_busy")
    );
    assert_eq!(f.row().await, before);
    drop(slot);
    let saved = f
        .app
        .automation_project_credentials_save(request.clone())
        .await
        .unwrap();
    assert!(
        f.app
            .automation_project_credentials_save(request)
            .await
            .is_err()
    );
    assert_eq!(f.row().await[FIELD], saved["policy"]);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn future_policy_is_readonly_and_raw_metadata_is_preserved() {
    let f = Fixture::new().await;
    let saved = f.save(f.policy(false, &f.selected)).await;
    let (owner, u) = f.app.unlocked().await.unwrap();
    drop(u);
    let project = f.project.clone();
    owner
        .storage()
        .unwrap()
        .write(move |tx| {
            let mut c: Catalog = tx.setting_get(PROJECTS)?.unwrap();
            c.records.get_mut(&project).unwrap()[FIELD] = json!({"version":99,"opaque":"keep"});
            tx.setting_set(PROJECTS, &c)
        })
        .await
        .unwrap();
    let before = f.row().await;
    assert!(f.app.automation_projects().await.unwrap().projects[0].read_only);
    assert!(
        f.app
            .automation_project_credentials_save(
                json!({"version":1,"frame":saved["frame"],"policy":null}).to_string()
            )
            .await
            .is_err()
    );
    assert_eq!(f.row().await, before);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn configured_shared_ref_is_denied_without_dial_or_new_issuer() {
    let f = Fixture::new().await;
    let (owner, u) = f.app.unlocked().await.unwrap();
    let mut host =
        crate::inventory::host_of(&u, crate::dto::parse_id("host_id", &f.host).unwrap()).unwrap();
    host.metadata
        .insert("cc.shared.instance".into(), Uuid::new_v4().to_string());
    host.metadata
        .insert("cc.shared.share".into(), Uuid::new_v4().to_string());
    u.writer.put(VaultObject::Host(host)).await.unwrap();
    drop(u);
    let context = f
        .app
        .automation_project_credentials_context(f.project.clone())
        .await
        .unwrap();
    let before = f.row().await;
    let error = f
        .app
        .automation_project_credentials_save(
            json!({"version":1,"frame":context["frame"],"policy":f.policy(false,&f.selected)})
                .to_string(),
        )
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("project_environment_shared_override_unsupported")
    );
    assert_eq!(f.row().await, before);
    assert!(owner.automation.connections.lock().unwrap().is_empty());
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn selected_environment_never_lowers_native_production_host_or_group_tags() {
    let f = Fixture::new().await;
    let saved = f.save(f.policy(false, &f.selected)).await;
    let mut host = f.app.get_host(f.host.clone()).await.unwrap();
    host.tags = vec!["PRODUCTION".into()];
    f.app.save_host(host).await.unwrap();
    let context = f.app.automation_project_context(&f.project).await.unwrap();
    let frozen = freeze(
        &f.app,
        &context,
        Some(&Fixture::selection(&saved)),
        &[f.host.clone()],
    )
    .await
    .unwrap()
    .unwrap();
    let (_, u) = f.app.unlocked().await.unwrap();
    let snapshot = u.working().snapshot();
    assert!(!frozen.environment.production);
    assert!(production(Some(&frozen), &snapshot, &frozen.revisions));
    let mut dev = snapshot.clone();
    for entry in dev.values_mut() {
        if let crate::working_set::Item::Object(o) = &mut entry.item {
            if let VaultObject::Host(h) = o.as_mut() {
                h.tags.clear();
            }
        }
    }
    assert!(!production(Some(&frozen), &dev, &frozen.revisions));
    let mut group = cc_models::group::Group::new("Owned production group");
    group.tags = vec!["prod".into()];
    let group_id = group.id;
    dev.insert(
        group_id,
        crate::working_set::Entry {
            revision: 1,
            item: crate::working_set::Item::Object(Box::new(VaultObject::Group(group))),
        },
    );
    let mut watched = frozen.revisions.clone();
    watched.insert(group_id, 1);
    assert!(production(Some(&frozen), &dev, &watched));
    let mut enforced = frozen.clone();
    enforced.environment.production = true;
    assert!(production(Some(&enforced), &dev, &enforced.revisions));
    drop(u);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn configured_policy_requires_explicit_selection_before_any_helper_or_target_access() {
    let f = Fixture::new().await;
    f.save(f.policy(true, &f.selected)).await;
    let request:crate::automation_prepare::AutomationRunRequest=serde_json::from_value(json!({
        "project_id":f.project,"runner_id":Uuid::new_v4(),"playbook":"site.yml","host_ids":[f.host],
        "mode":"apply","parameters":{"extra_vars":{"production":false},"tags":[],"skip_tags":[],"diff":false},
        "timeout_seconds":60,"forks":1,"cancel_on_lock":true})).unwrap();
    let error = f.app.automation_prepare_run(request).await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("project_environment_selection_required")
    );
    let (owner, u) = f.app.unlocked().await.unwrap();
    drop(u);
    assert!(owner.automation.connections.lock().unwrap().is_empty());
    assert!(owner.automation.reviews.lock().unwrap().is_empty());
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn actual_selected_credential_mutation_retires_the_frozen_original_selection() {
    let f = Fixture::new().await;
    let saved = f.save(f.policy(false, &f.selected)).await;
    let context = f.app.automation_project_context(&f.project).await.unwrap();
    let frozen = freeze(
        &f.app,
        &context,
        Some(&Fixture::selection(&saved)),
        &[f.host.clone()],
    )
    .await
    .unwrap()
    .unwrap();
    f.app
        .set_credential_password(f.selected.clone(), "synthetic-rotated-secret42".into())
        .await
        .unwrap();
    let (_, u) = f.app.unlocked().await.unwrap();
    assert!(!frozen.current(&u));
    drop(u);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn pending_or_readonly_source_cannot_gain_a_credential_policy() {
    let f = Fixture::new().await;
    let context = f
        .app
        .automation_project_credentials_context(f.project.clone())
        .await
        .unwrap();
    let (owner, u) = f.app.unlocked().await.unwrap();
    drop(u);
    let project = f.project.clone();
    owner
        .storage()
        .unwrap()
        .write(move |tx| {
            let mut c: Catalog = tx.setting_get(PROJECTS)?.unwrap();
            c.records.get_mut(&project).unwrap()["source"]["status"] = json!("importPreparing");
            tx.setting_set(PROJECTS, &c)
        })
        .await
        .unwrap();
    let before = f.row().await;
    assert!(
        f.app
            .automation_project_credentials_save(
                json!({"version":1,"frame":context["frame"],"policy":f.policy(false,&f.selected)})
                    .to_string()
            )
            .await
            .is_err()
    );
    assert_eq!(f.row().await, before);
    assert!(f.app.automation_projects().await.unwrap().projects[0].read_only);
    f.app.shutdown().await.unwrap();
}

mod live;
