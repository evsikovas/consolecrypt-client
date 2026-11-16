use super::*;
use cc_models::{
    credential::{Credential, CredentialKind},
    group::Group,
    host::Host,
    VaultObject,
};
use sha2::{Digest, Sha256};

fn request(project: &str, revision: u64, hosts: &[String], groups: &[&str]) -> String {
    json!({"version":1,"projectId":project,"expectedRevision":revision,
        "hostIds":hosts,"groupNames":groups})
    .to_string()
}
fn rule(error: &AppError) -> String {
    error.args().get("rule").cloned().unwrap_or_default()
}
const PASSWORD: &str = "own inventory export fixture passphrase 42";
async fn setup() -> (tempfile::TempDir, AppCore) {
    let directory = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        directory.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("Own static inventory fixture".into(), PASSWORD.into())
        .await
        .unwrap();
    (directory, app)
}

#[test]
fn strict_request_rejects_duplicates_unknown_fields_future_and_implicit_host_selection() {
    let project = Uuid::new_v4().to_string();
    let host = Uuid::new_v4().to_string();
    let valid = request(&project, 1, std::slice::from_ref(&host), &["web"]);
    assert!(parse(&valid).is_ok());
    for text in [
        valid.replacen("\"version\":1", "\"version\":1,\"version\":1", 1),
        valid.replacen("\"version\":1", "\"version\":2", 1),
        valid.replace(&host, &host.replace('-', "")),
        request(&project, 1, &[], &["web"]),
        request(&project, 1, &[host.clone(), host.clone()], &[]),
        request(&project, 1, std::slice::from_ref(&host), &["all"]),
        request(&project, 1, std::slice::from_ref(&host), &["web", "web"]),
        format!("{} ", " ".repeat(16 * 1024)),
    ] {
        assert!(parse(&text).is_err());
    }
    let mut value: Value = serde_json::from_str(&valid).unwrap();
    value["credentialResolver"] = json!(true);
    assert!(parse(&value.to_string()).is_err());
}

#[test]
fn endpoint_allowlist_rejects_templates_controls_shell_urls_and_zone_semantics() {
    for (host, user) in [
        ("192.0.2.1", "runner"),
        ("2001:db8::1", "domain\\runner"),
        ("db.example.invalid.", "runner@realm"),
        ("localhost", "_runner"),
    ] {
        assert!(endpoint(host, user));
    }
    for host in [
        "",
        "{{host}}",
        "db\n",
        "-db",
        "https://db",
        "db;id",
        "$(id)",
        "fe80::1%eth0",
        "a..b",
    ] {
        assert!(!endpoint(host, "runner"), "{host:?}");
    }
    for user in ["", "{{user}}", "runner\n", "-option", "$(id)", "runner;id"] {
        assert!(!endpoint("192.0.2.1", user));
    }
}

#[tokio::test]
async fn sql_selection_mutation_in_final_guard_gap_rejects_obsolete_context_without_working_version_change(
) {
    let (_directory, app) = setup().await;
    let project = app
        .automation_project_create("Own export fence".into(), None)
        .await
        .unwrap();
    let (owner, u) = app.unlocked().await.unwrap();
    let context = app.automation_project_context(&project.id).await.unwrap();
    let version = u.working().version();
    let raw = request(&project.id, 1, &[Uuid::new_v4().to_string()], &[]);
    let request = parse(&raw).unwrap();
    // Deterministic scheduling boundary: the production finish method receives
    // the captured context, after a real SQL-only save has completed. No timer,
    // global hook, altered lock implementation or network stand-in is used.
    app.automation_project_save_inventory(
        project.id,
        1,
        json!({"version":2,"selected_hosts":[],"groups":{}}).to_string(),
    )
    .await
    .unwrap();
    assert_eq!(
        u.working().version(),
        version,
        "this is specifically the SQL-only gap"
    );
    let error = app
        .finish_inventory_export(context, u.clone(), request, version, json!({}))
        .await
        .unwrap_err();
    assert_eq!(rule(&error), "inventory_export_context_changed");
    drop(u);
    drop(owner);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn sql_source_binding_clear_during_issuance_rejects_captured_context() {
    let (_directory, app) = setup().await;
    let project = app
        .automation_project_create("Own source export fence".into(), None)
        .await
        .unwrap();
    let (owner, u) = app.unlocked().await.unwrap();
    // Seed only the exact persisted source-binding document, no source file,
    // preview helper or process. Then mutate through the real public clear API.
    let config=cc_automation_core::ExternalInventoryConfig::parse(br#"{"version":1,"sources":[{"path":"hosts.ini","format":"ini"}],"mode":"mixed","bindings":{}}"#).unwrap();
    let value = serde_json::to_value(config).unwrap();
    let digest = hex::encode(Sha256::digest(serde_json::to_vec(&value).unwrap()));
    let project_id = project.id.clone();
    owner
        .storage()
        .unwrap()
        .write(move |tx| {
            let mut catalog: Value = tx.setting_get("automation.projects.v1")?.unwrap();
            catalog["records"][&project_id]["external_inventory"] =
                json!({"version":1,"sha256":digest});
            catalog["records"][&project_id]["revision"] = json!(2);
            tx.setting_set(
                &format!("automation.inventory.sources.v1.{project_id}"),
                &value,
            )?;
            tx.setting_set("automation.projects.v1", &catalog)?;
            Ok::<_, AppError>(())
        })
        .await
        .unwrap();
    let context = app.automation_project_context(&project.id).await.unwrap();
    assert!(context.external_inventory.is_some());
    let version = u.working().version();
    let request = parse(&request(&project.id, 2, &[Uuid::new_v4().to_string()], &[])).unwrap();
    assert_eq!(
        app.automation_inventory_clear(project.id, 2)
            .await
            .unwrap()
            .revision,
        Some(3)
    );
    assert_eq!(u.working().version(), version);
    let error = app
        .finish_inventory_export(context, u.clone(), request, version, json!({}))
        .await
        .unwrap_err();
    assert_eq!(rule(&error), "inventory_export_context_changed");
    drop(u);
    drop(owner);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn lock_and_profile_switch_after_capture_during_issuance_reject_finish() {
    for transition in ["lock", "profile"] {
        let (_directory, app) = setup().await;
        let project = app
            .automation_project_create("Own lifecycle export fence".into(), None)
            .await
            .unwrap();
        let context = app.automation_project_context(&project.id).await.unwrap();
        let original = context.unlock.upgrade().unwrap();
        let version = original.working().version();
        let request = parse(&request(&project.id, 1, &[Uuid::new_v4().to_string()], &[])).unwrap();
        if transition == "lock" {
            app.lock().await.unwrap();
        } else {
            app.create_local_profile("Own switched fixture".into(), PASSWORD.into())
                .await
                .unwrap();
        }
        assert!(matches!(
            app.finish_inventory_export(context, original, request, version, json!({}))
                .await,
            Err(AppError::VaultLocked)
        ));
        app.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn future_inventory_is_rejected_and_its_exact_stored_json_is_preserved() {
    let (_directory, app) = setup().await;
    let project = app
        .automation_project_create("Own future export".into(), None)
        .await
        .unwrap();
    let (owner, _) = app.unlocked().await.unwrap();
    let project_id = project.id.clone();
    let future = json!({"version":99,"opaque":{"must":"remain unchanged"}});
    let saved = future.clone();
    owner
        .storage()
        .unwrap()
        .write(move |tx| {
            let mut value: Value = tx.setting_get("automation.projects.v1")?.unwrap();
            value["records"][&project_id]["inventory"] = saved;
            tx.setting_set("automation.projects.v1", &value)?;
            Ok::<_, AppError>(())
        })
        .await
        .unwrap();
    assert!(app
        .automation_inventory_export_issue(request(
            &project.id,
            1,
            &[Uuid::new_v4().to_string()],
            &[]
        ))
        .await
        .is_err());
    let unchanged: Value = owner
        .storage()
        .unwrap()
        .setting_get("automation.projects.v1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unchanged["records"][&project.id]["inventory"], future);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn issued_data_reuses_inheritance_and_projects_only_explicit_hosts_groups_and_three_fields() {
    let (_directory, app) = setup().await;
    let (owner, u) = app.unlocked().await.unwrap();
    let mut credential = Credential::new(
        "SYNTHETIC_NEVER_EXPORT_CREDENTIAL",
        CredentialKind::Password,
    );
    credential.username = Some("credential_runner".into());
    // Deliberately no secret object. A resolver/connection call would fail;
    // pure metadata planning must still produce the correct inherited user.
    let missing_secret = cc_models::ObjectId::new();
    credential.secret_id = Some(missing_secret);
    let credential_id = credential.id;
    u.writer
        .put(VaultObject::Credential(credential))
        .await
        .unwrap();
    let mut folder = Group::new("SYNTHETIC_NOT_AN_ANSIBLE_GROUP");
    folder.inherited_username = Some("group_runner".into());
    folder.inherited_port = Some(2205);
    folder.inherited_credential_id = Some(credential_id);
    let group_id = folder.id;
    u.writer.put(VaultObject::Group(folder)).await.unwrap();
    let mut selected = Host::new("SYNTHETIC_NEVER_EXPORT_HOST_PROSE", "192.0.2.7");
    selected.group_id = Some(group_id);
    selected.notes = "SYNTHETIC_PRIVATE_NOTE_NOT_EXPORTED".into();
    let selected_id: Uuid = selected.id.to_string().parse().unwrap();
    u.writer.put(VaultObject::Host(selected)).await.unwrap();
    let mut outside = Host::new("Outside explicit approval", "192.0.2.8");
    outside.username = Some("outside".into());
    let outside_id: Uuid = outside.id.to_string().parse().unwrap();
    u.writer.put(VaultObject::Host(outside)).await.unwrap();
    let project = app
        .automation_project_create("Own export".into(), None)
        .await
        .unwrap();
    app.automation_project_save_inventory(
        project.id.clone(),
        1,
        json!({
            "version":2,"selected_hosts":[selected_id],
            "groups":{"web":[selected_id,outside_id],"empty":[],"hidden":[selected_id]}
        })
        .to_string(),
    )
    .await
    .unwrap();
    let result = app
        .automation_inventory_export_issue(request(
            &project.id,
            2,
            &[selected_id.to_string()],
            &["web", "empty"],
        ))
        .await
        .unwrap();
    let host_alias = alias(selected_id);
    let inventory = &result["inventory"];
    assert_eq!(inventory["web"]["hosts"], json!([host_alias]));
    assert_eq!(inventory["empty"]["hosts"], json!([]));
    assert_eq!(inventory["ungrouped"]["hosts"], json!([]));
    assert!(inventory.get("hidden").is_none());
    assert_eq!(
        inventory["_meta"]["hostvars"][&host_alias],
        json!({
            "ansible_host":"192.0.2.7","ansible_port":2205,"ansible_user":"credential_runner"
        })
    );
    let serialized = serde_json::to_string(&result).unwrap();
    for forbidden in [
        "SYNTHETIC_",
        "credential_refs",
        "secret_id",
        "private_key",
        "consolecrypt_rpc",
        &credential_id.to_string(),
        &missing_secret.to_string(),
        &outside_id.to_string(),
    ] {
        assert!(
            !serialized.contains(forbidden),
            "unexpected field/content category"
        );
    }
    assert!(serialized.len() <= MAX_SNAPSHOT_BYTES);
    assert_eq!(result["origin"]["projectRevision"], 2);
    assert_eq!(
        rule(
            &app.automation_inventory_export_issue(request(
                &project.id,
                2,
                &[outside_id.to_string()],
                &[]
            ))
            .await
            .unwrap_err()
        ),
        "inventory_export_host_not_selected"
    );
    drop(u);
    drop(owner);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn stale_missing_groups_lock_and_other_profile_fail_closed_without_execution() {
    let (_directory, app) = setup().await;
    let (_, u) = app.unlocked().await.unwrap();
    let mut host = Host::new("Own offline export", "example.invalid");
    host.username = Some("runner".into());
    let host_id = host.id.to_string();
    u.writer.put(VaultObject::Host(host)).await.unwrap();
    let project = app
        .automation_project_create("Own export".into(), None)
        .await
        .unwrap();
    assert_eq!(
        rule(
            &app.automation_inventory_export_issue(request(
                &project.id,
                2,
                std::slice::from_ref(&host_id),
                &[]
            ))
            .await
            .unwrap_err()
        ),
        "inventory_export_context_changed"
    );
    assert_eq!(
        rule(
            &app.automation_inventory_export_issue(request(
                &project.id,
                1,
                std::slice::from_ref(&host_id),
                &["missing"]
            ))
            .await
            .unwrap_err()
        ),
        "inventory_export_unknown_group"
    );
    let no_groups = app
        .automation_inventory_export_issue(request(
            &project.id,
            1,
            std::slice::from_ref(&host_id),
            &[],
        ))
        .await
        .unwrap();
    assert_eq!(
        no_groups["inventory"]["all"]["hosts"],
        no_groups["inventory"]["ungrouped"]["hosts"]
    );
    app.lock().await.unwrap();
    assert!(matches!(
        app.automation_inventory_export_issue(request(
            &project.id,
            1,
            std::slice::from_ref(&host_id),
            &[]
        ))
        .await,
        Err(AppError::VaultLocked)
    ));
    app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    app.create_local_profile("Different own profile".into(), PASSWORD.into())
        .await
        .unwrap();
    assert!(app
        .automation_inventory_export_issue(request(
            &project.id,
            1,
            std::slice::from_ref(&host_id),
            &[]
        ))
        .await
        .is_err());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn configured_proxy_command_jump_and_agent_forwarding_never_become_direct_exports() {
    let (_directory, app) = setup().await;
    let (_, u) = app.unlocked().await.unwrap();
    let project = app
        .automation_project_create("Own unsupported route export".into(), None)
        .await
        .unwrap();
    let mut bastion = Host::new("Own metadata bastion", "192.0.2.10");
    bastion.username = Some("runner".into());
    let bastion_id = bastion.id;
    u.writer.put(VaultObject::Host(bastion)).await.unwrap();
    for kind in [
        "proxy_command",
        "imported_proxy_command",
        "jump",
        "agent_forwarding",
    ] {
        let mut host = Host::new("Own unsupported metadata route", "192.0.2.9");
        host.username = Some("runner".into());
        match kind {
            "proxy_command" => host.proxy_command = Some("SYNTHETIC_NEVER_EXECUTED".into()),
            "imported_proxy_command" => {
                host.metadata.insert(
                    "ssh_config.proxy_command".into(),
                    "SYNTHETIC_NEVER_EXECUTED".into(),
                );
            }
            "jump" => host.jump_chain = vec![bastion_id],
            _ => host.agent_forwarding = true,
        }
        let host_id = host.id.to_string();
        u.writer.put(VaultObject::Host(host)).await.unwrap();
        let error = app
            .automation_inventory_export_issue(request(&project.id, 1, &[host_id], &[]))
            .await
            .unwrap_err();
        assert_eq!(rule(&error), "inventory_export_unsupported_route");
    }
    app.shutdown().await.unwrap();
}
