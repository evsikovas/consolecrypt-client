//! Public AppCore-only fixture issuer. Own temporary local profile, no server,
//! secret material, resolver calls, external commands or target connections.
use cc_app_core::{AppConfig, AppCore, GroupDto, HostDto};
use serde_json::{json, Value};

fn request(project: &str, revision: u64, hosts: &[String], groups: &[&str]) -> String {
    json!({"version":1,"projectId":project,"expectedRevision":revision,
        "hostIds":hosts,"groupNames":groups})
    .to_string()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let app = AppCore::new(AppConfig::for_tests(
        directory.path().to_string_lossy().into_owned(),
    ))?;
    app.create_local_profile(
        "Own export integration fixture".into(),
        "own temporary export fixture passphrase 42".into(),
    )
    .await?;

    // This is metadata only; no password/key or installed/running agent needed.
    let credential = app
        .add_agent_credential(
            "SYNTHETIC_CREDENTIAL_METADATA_DO_NOT_EXPORT".into(),
            Some("credential_runner".into()),
            None,
        )
        .await?;
    assert!(!credential.has_secret && !credential.has_remembered_passphrase);
    let mut folder = GroupDto::new("SYNTHETIC_FOLDER_NOT_ANSIBLE_GROUP");
    folder.inherited_port = Some(2205);
    folder.inherited_username = Some("group_runner".into());
    folder.inherited_credential_id = Some(credential.id.clone());
    let folder = app.save_group(folder).await?;
    let mut first = HostDto::new("SYNTHETIC_DISPLAY_DO_NOT_EXPORT", "192.0.2.101");
    first.group_id = Some(folder.id.clone());
    first.notes = "SYNTHETIC_NOTE_DO_NOT_EXPORT".into();
    first.tags = vec!["SYNTHETIC_TAG_DO_NOT_EXPORT".into()];
    let first = app.save_host(first).await?;
    let mut second = HostDto::new("Own direct selected fixture", "2001:db8::102");
    second.username = Some("direct_runner".into());
    second.port = Some(2222);
    let second = app.save_host(second).await?;
    let mut outside = HostDto::new("Own outside approval fixture", "192.0.2.103");
    outside.username = Some("outside_runner".into());
    let outside = app.save_host(outside).await?;
    let project = app
        .automation_project_create("Own explicit export integration".into(), None)
        .await?;
    app.automation_project_save_inventory(
        project.id.clone(),
        1,
        json!({
            "version":2,"selected_hosts":[first.id,second.id],
            "groups":{"web":[first.id,outside.id],"empty":[],"not_requested":[second.id]}
        })
        .to_string(),
    )
    .await?;
    let hosts = [first.id.clone(), second.id.clone()];
    let snapshot = app
        .automation_inventory_export_issue(request(&project.id, 2, &hosts, &["web", "empty"]))
        .await?;
    assert_eq!(snapshot["format"], "consolecrypt.inventory.snapshot");
    assert_eq!(snapshot["origin"]["projectRevision"], 2);
    let first_alias = format!("cc_{}", first.id.replace('-', ""));
    let second_alias = format!("cc_{}", second.id.replace('-', ""));
    assert_eq!(
        snapshot["inventory"]["_meta"]["hostvars"][&first_alias],
        json!({
            "ansible_host":"192.0.2.101","ansible_port":2205,"ansible_user":"credential_runner"
        })
    );
    assert_eq!(
        snapshot["inventory"]["_meta"]["hostvars"][&second_alias],
        json!({
            "ansible_host":"2001:db8::102","ansible_port":2222,"ansible_user":"direct_runner"
        })
    );
    assert_eq!(snapshot["inventory"]["web"]["hosts"], json!([first_alias]));
    assert_eq!(snapshot["inventory"]["empty"]["hosts"], json!([]));
    assert_eq!(
        snapshot["inventory"]["ungrouped"]["hosts"],
        json!([second_alias])
    );
    assert!(snapshot["inventory"].get("not_requested").is_none());
    for rejected in [
        request(&project.id, 1, &hosts, &[]),
        request(&project.id, 2, &[outside.id.clone()], &[]),
        request(&project.id, 2, &hosts, &["missing"]),
    ] {
        assert!(app
            .automation_inventory_export_issue(rejected)
            .await
            .is_err());
    }
    let mut imported = HostDto::new("Own metadata-only unsupported route", "192.0.2.104");
    imported.username = Some("runner".into());
    imported.metadata.insert(
        "ssh_config.proxy_command".into(),
        "SYNTHETIC_NEVER_EXECUTED".into(),
    );
    let imported = app.save_host(imported).await?;
    let unsupported_project = app
        .automation_project_create("Own unsupported route metadata fixture".into(), None)
        .await?;
    let rejected = app
        .automation_inventory_export_issue(request(&unsupported_project.id, 1, &[imported.id], &[]))
        .await
        .unwrap_err();
    assert_eq!(
        rejected.args().get("rule").map(String::as_str),
        Some("inventory_export_unsupported_route")
    );
    let raw = serde_json::to_vec(&snapshot)?;
    assert!(raw.len() <= 256 * 1024);
    for forbidden in [
        "SYNTHETIC_",
        &credential.id,
        &folder.id,
        &outside.id,
        "credential_refs",
        "secret_id",
        "private_key",
        "consolecrypt_rpc",
    ] {
        assert!(!String::from_utf8_lossy(&raw).contains(forbidden));
    }
    app.lock().await?;
    assert!(app
        .automation_inventory_export_issue(request(&project.id, 2, &hosts, &[]))
        .await
        .is_err());
    app.shutdown().await?;
    // The only stdout document is the exact Native-issued envelope. The driver
    // persists these bytes unchanged for both real external consumers.
    let _: Value = serde_json::from_slice(&raw)?;
    println!("{}", String::from_utf8(raw)?);
    Ok(())
}
