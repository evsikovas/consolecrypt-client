//! Actual profile-scoped preview lifecycle; never connects inventory targets.
use cc_app_core::{automation_runners::AutomationRunnerConfig, AppConfig, AppCore, HostDto};
use cc_automation_core::HelperLaunch;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    io::{self, BufRead},
    time::Duration,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    launch: HelperLaunch,
}
async fn ready(app: &AppCore, id: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
    loop {
        let page = app
            .automation_inventory_page(id.into(), String::new(), 0, 50)
            .await?;
        match page["state"].as_str() {
            Some("ready") => return Ok(page),
            Some("preparing") => {}
            _ => return Err("native source preview failed".into()),
        }
        if tokio::time::Instant::now() > deadline {
            return Err("source preview deadline exceeded".into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    if line.len() > 128 * 1024 {
        return Err("fixture exceeds limit".into());
    }
    let fixture: Fixture = serde_json::from_str(&line)?;
    let folder = tempfile::tempdir()?;
    let app = AppCore::new(AppConfig::for_tests(
        folder.path().to_string_lossy().into_owned(),
    ))?;
    let password = "correct horse inventory preview fixture 42";
    app.create_local_profile("Inventory source fixture".into(), password.into())
        .await?;
    let result=async {
        let project=app.automation_project_create("Inventory source".into(),None).await?;
        app.automation_project_save(project.id.clone(),"defaults.ini".into(),None,
            "[all:vars]\nansible_port=2222\nansible_user=deployer\n".into()).await?;
        app.automation_project_save(project.id.clone(),"inventory/hosts.yml".into(),None,
            "\u{feff}web:\n  hosts:\n    service_a:\n      ansible_host: 192.0.2.10\n      private_fixture_value: fixture-hidden-from-pages\n".into()).await?;
        app.automation_project_save(project.id.clone(),"inventory/group_vars/web.yml".into(),None,"ansible_user: deployer\nansible_port: 2222\nenabled: true\n".into()).await?;
        let runner=app.automation_runner_create("Owned source Runner".into(),AutomationRunnerConfig::Local{launch:fixture.launch}).await?;
        app.automation_runner_connect(runner.id.clone()).await?;
        let sources=json!([{"path":"defaults.ini","format":"ini"},{"path":"inventory","format":"directory"}]).to_string();
        let started=app.automation_inventory_start(project.id.clone(),runner.id.clone(),sources.clone()).await?;
        assert_eq!(started["state"],"preparing");
        let id=started["preview_id"].as_str().unwrap().to_owned();
        let page=ready(&app,&id).await?;
        assert_eq!(page["host_count"],1);assert_eq!(page["total"],1);assert_eq!(page["cursor"],1);
        assert_eq!(page["hosts"][0]["alias"],"service_a");assert_eq!(page["hosts"][0]["username"],"deployer");
        assert_eq!(page["hosts"][0]["port"],2222);
        assert!(!page.to_string().contains("fixture-hidden-from-pages"));
        assert!(page["hosts"][0].get("variables").is_none());assert!(page["hosts"][0].get("credential_refs").is_none());
        assert_eq!(app.automation_inventory_page(id.clone(),"no-match".into(),0,50).await?["total"],0);
        assert!(app.automation_runner_disconnect(runner.id.clone()).await.is_err());
        app.automation_inventory_discard(id.clone()).await?;
        assert!(app.automation_inventory_page(id.clone(),String::new(),0,50).await.is_err());
        assert!(app.list_hosts().await?.is_empty(),"preview must not create target hosts");
        // The user separately saves one host. The preview never creates it and
        // committing this mapping never attempts any network connection to it.
        let mut host=HostDto::new("Explicit saved target","192.0.2.10");
        host.port=Some(2222);host.username=Some("deployer".into());
        let host=app.save_host(host).await?;
        let start=app.automation_inventory_start(project.id.clone(),runner.id.clone(),sources.clone()).await?;
        let binding_id=start["preview_id"].as_str().unwrap().to_owned();ready(&app,&binding_id).await?;
        let configuration=json!({"version":1,"sources":serde_json::from_str::<Value>(&sources)?,"mode":"external_only",
            "bindings":{"service_a":{"host_id":host.id,"resolution":"match","source":{"address":"192.0.2.10","port":2222,"username":"deployer"}}}});
        let mut unknown=configuration.clone();unknown["bindings"]["service_a"]["host_id"]=json!(uuid::Uuid::new_v4());
        let report=app.automation_inventory_check(binding_id.clone(),unknown.to_string()).await?;
        assert_eq!(report["valid"],false);assert_eq!(report["conflicts"][0]["kind"],"saved_host_missing");
        assert!(app.automation_inventory_commit(binding_id.clone(),unknown.to_string()).await.is_err());
        assert_eq!(app.automation_inventory_check(binding_id.clone(),configuration.to_string()).await?["valid"],true);
        let mut changed_host=host.clone();changed_host.port=Some(3333);app.save_host(changed_host).await?;
        let report=app.automation_inventory_check(binding_id.clone(),configuration.to_string()).await?;
        assert_eq!(report["valid"],false);assert_eq!(report["conflicts"][0]["kind"],"endpoint_mismatch");
        assert_eq!(report["conflicts"][0]["saved"]["port"],3333);
        let mut override_config=configuration.clone();override_config["bindings"]["service_a"]["resolution"]=json!("use_saved_host");
        assert_eq!(app.automation_inventory_check(binding_id.clone(),override_config.to_string()).await?["valid"],true);
        app.save_host(host.clone()).await?;
        let mut stale=configuration.clone();stale["bindings"]["service_a"]["source"]["port"]=json!(1111);
        assert_eq!(app.automation_inventory_check(binding_id.clone(),stale.to_string()).await?["conflicts"][0]["kind"],"source_changed");
        let mut many=configuration.clone();let mut entries=serde_json::Map::new();
        for index in 0..100 {
            let mut binding=configuration["bindings"]["service_a"].clone();binding["host_id"]=json!(uuid::Uuid::new_v4());entries.insert(format!("missing_{index}"),binding);
        }
        many["bindings"]=Value::Object(entries);
        let report=app.automation_inventory_check(binding_id.clone(),many.to_string()).await?;
        assert_eq!(report["conflict_count"],100);assert_eq!(report["conflicts"].as_array().unwrap().len(),50);
        assert_eq!(report["conflicts"][0]["kind"],"source_missing");
        assert!(!report.to_string().contains("fixture-hidden-from-pages"));
        let defaults=app.automation_project_file(project.id.clone(),"defaults.ini".into()).await?;
        let changed=app.automation_project_save(project.id.clone(),"defaults.ini".into(),Some(defaults.sha256),
            "[all:vars]\nansible_port=2223\nansible_user=deployer\n".into()).await?;
        assert!(app.automation_inventory_commit(binding_id.clone(),configuration.to_string()).await.is_err());
        app.automation_project_save(project.id.clone(),"defaults.ini".into(),Some(changed.sha256),
            "[all:vars]\nansible_port=2222\nansible_user=deployer\n".into()).await?;
        let (first,second)=tokio::join!(app.automation_inventory_commit(binding_id.clone(),configuration.to_string()),
            app.automation_inventory_commit(binding_id.clone(),configuration.to_string()));
        assert_ne!(first.is_ok(),second.is_ok(),"one preview cannot be committed twice");
        assert!(app.automation_inventory_page(binding_id,String::new(),0,50).await.is_err());
        let saved=app.automation_inventory_configuration(project.id.clone()).await?;
        assert_eq!(saved["configuration"],configuration);assert_eq!(saved["project_revision"],2);
        assert!(!saved.to_string().contains("fixture-hidden-from-pages"));
        app.automation_inventory_clear(project.id.clone(),2).await?;
        let mut retained=Vec::new();
        for _ in 0..4 {
            let start=app.automation_inventory_start(project.id.clone(),runner.id.clone(),sources.clone()).await?;
            let id=start["preview_id"].as_str().unwrap().to_owned();ready(&app,&id).await?;retained.push(id);
        }
        assert!(app.automation_inventory_start(project.id.clone(),runner.id.clone(),sources.clone()).await.is_err());
        app.lock().await?;
        assert!(app.automation_inventory_page(retained[0].clone(),String::new(),0,50).await.is_err());
        app.unlock_with_passphrase(password.into()).await?;
        assert!(app.automation_inventory_page(retained[0].clone(),String::new(),0,50).await.is_err());
        let pending=app.automation_inventory_start(project.id,runner.id.clone(),sources).await?;
        let pending=pending["preview_id"].as_str().unwrap().to_owned();
        app.automation_inventory_discard(pending.clone()).await?;
        assert!(app.automation_inventory_page(pending,String::new(),0,50).await.is_err());
        let hosts=app.list_hosts().await?;assert_eq!(hosts.len(),1);assert_eq!(hosts[0].id,host.id,"no implicit target creation");
        app.automation_runner_disconnect(runner.id).await?;
        Ok::<_,Box<dyn std::error::Error>>(())
    }.await;
    app.shutdown().await?;
    result?;
    println!(
        "{}",
        json!({"status":"PASS","native_appcore_inventory_preview":true,"no_target_creation":true,
        "public_pages_only":true,"bounded_retained_previews":true,"lock_fences_old_ids":true,"discard_cancels_only_preview":true})
    );
    Ok(())
}
