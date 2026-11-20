//! Actual private owner acceptance. Ignored until the UUID-labelled coordinator
//! supplies strict SSH pins and synthetic setup via PRIVATE stdin (not files).
use super::*;
use crate::{automation_runners::AutomationRunnerConfig, HostDto};
use cc_automation_core::{ProjectRoot, RemoteHelperConfig, VAULT_INPUT_FILE};
use cc_models::host::HostKeyPolicy;
use cc_ssh_core::keys::{generate_key, KeyGenAlgorithm};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::{io::{Read, Write}, path::{Path, PathBuf}, time::Duration};
use zeroize::Zeroizing;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Setup { version:u8, token:String, password:String, ciphertext:String }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pins { version:u8, token:String, ssh_port:u16, host_public_key:String, egress_blocked:bool }
fn publish(root:&Path,name:&str,value:Value) {
    let raw=serde_json::to_vec(&value).unwrap(); assert!(raw.len()<=16384);
    let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(root.join(name)).unwrap();
    file.write_all(&raw).unwrap(); file.sync_all().unwrap();
}
async fn bounded_json(root:&Path,name:&str)->Value {
    tokio::time::timeout(Duration::from_secs(60),async {
        loop {
            if let Ok(metadata)=std::fs::symlink_metadata(root.join(name)) {
                assert!(metadata.is_file()&&!metadata.file_type().is_symlink()&&metadata.len()<=16384);
                let bytes=std::fs::read(root.join(name)).unwrap();
                if let Ok(value)=serde_json::from_slice(&bytes) {return value;}
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("owned fixture control deadline")
}
fn make_input(project:&str,digest:&str,credential:&str,runner:&str)->VaultValidationInput {
    let host=Uuid::new_v4();
    VaultValidationInput {
        project_id:project.into(),project_revision:1,revision_digest:digest.into(),
        credential_id:credential.into(),runner_id:runner.into(),runner_revision:1,
        playbook:"playbook.yml".into(),
        parameters:RunParameters{extra_vars:BTreeMap::new(),tags:vec![],skip_tags:vec![],diff:false,vault:None},
        inventory:json!({"version":1,"hosts":[{"host_id":host,"alias":"fixture","address":"192.0.2.1","port":22,"username":"fixture","variables":{},"sources":["owned"],"credential_refs":[]}],"groups":{},"source_digests":{"owned":"f".repeat(64)}}),
        targets:vec![ApprovedTarget{host_id:host,alias:"fixture".into(),address:"192.0.2.1".into(),port:22,username:"fixture".into(),route_digest:"e".repeat(64),credential_refs:vec![]}],
    }
}
async fn drained(registry:&VaultValidationOperations) {
    tokio::time::timeout(Duration::from_secs(30),async {
        loop {
            let done={let state=registry.state.lock().unwrap();
                state.entries.is_empty()&&state.workers.iter().all(|task|task.is_finished())&&state.cleanups.iter().all(|task|task.is_finished())};
            if done {break;}
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("owned validation cleanup/admission deadline");
    assert_eq!(ACTIVE.get().unwrap().available_permits(),2);
}
#[tokio::test(flavor="multi_thread",worker_threads=2)]
#[ignore="requires explicitly owned pinned Remote SSH coordinator and PRIVATE synthetic stdin"]
async fn remote_appcore_vault_private_owner_four_loaders_and_context_fences() {
    let token=std::env::var("CC_VAULT_OWNER_TOKEN").expect("owned UUID required");
    let id=Uuid::parse_str(&token).unwrap();assert!(!id.is_nil()&&id.to_string()==token);
    let root=PathBuf::from(std::env::var_os("CC_VAULT_OWNER_ROOT").expect("owned control root required"));
    assert!(root.is_absolute()&&root.file_name().unwrap().to_str().unwrap()==format!("owned-{token}"));
    assert!(!std::fs::symlink_metadata(&root).unwrap().file_type().is_symlink());
    let workspace=Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(4).unwrap();
    let allowed=workspace.join("automation-e1-next/ansible-vault-v1/validation-owner-v1/control").canonicalize().unwrap();
    assert_eq!(root.canonicalize().unwrap().parent().unwrap(),allowed);
    let keys=generate_key(KeyGenAlgorithm::Ed25519,"owned Vault validation fixture",None).unwrap();
    publish(&root,"native-public.json",json!({"version":1,"token":token,"public_key":keys.public_openssh}));
    // Only bounded private pipe bytes contain the synthetic password. Never a
    // control file/env/argv/log/assertion diff; zeroize both JSON and password.
    let mut raw=Zeroizing::new(Vec::new());
    std::io::stdin().take(16385).read_to_end(&mut raw).unwrap();assert!(raw.len()<=16384);
    let setup:Setup=serde_json::from_slice(&raw).expect("invalid private synthetic setup");
    assert!(setup.version==1&&setup.token==token&&setup.password.len()>=32&&setup.password.len()<=128);
    assert!(setup.ciphertext.len()<=8192&&setup.ciphertext.starts_with("$ANSIBLE_VAULT;1.1;AES256\n"));
    let password=SecretString::from(setup.password);drop(raw);
    let pins:Pins=serde_json::from_value(bounded_json(&root,"fixture.json").await).unwrap();
    assert!(pins.version==1&&pins.token==token&&pins.ssh_port>0);
    assert!(pins.host_public_key.starts_with("ssh-ed25519 ")&&pins.host_public_key.len()<8192&&!pins.host_public_key.chars().any(char::is_control));
    let profile=tempfile::Builder::new().prefix("native-profile-").tempdir_in(&root).unwrap();
    let app=AppCore::new(crate::AppConfig::for_tests(profile.path().to_string_lossy().into_owned())).unwrap();
    app.create_local_profile("Owned Vault validation fixture".into(),"synthetic profile password 42".into()).await.unwrap();
    let ssh=app.import_ssh_key("Own ephemeral fixture key".into(),Some("runner".into()),keys.private_openssh.expose_secret().to_owned(),None,false,None).await.unwrap();
    app.import_known_hosts(format!("[127.0.0.1]:{} {}\n",pins.ssh_port,pins.host_public_key)).await.unwrap();
    let mut host=HostDto::new("Owned Vault validation fixture","127.0.0.1");host.port=Some(pins.ssh_port);host.username=Some("runner".into());host.credential_id=Some(ssh.id);host.host_key_policy=HostKeyPolicy::Strict;
    let host=app.save_host(host).await.unwrap();
    let runner=app.automation_runner_create("Dedicated Remote SSH Vault validation".into(),AutomationRunnerConfig::Remote{host_id:host.id,helper:RemoteHelperConfig{python:format!("/home/runner/proof-{token}/python"),module_dir:"/opt/consolecrypt/src".into(),root:format!("/home/runner/proof-{token}/jobs")}}).await.unwrap();
    let credential=app.add_password_credential("Explicit Vault validation password".into(),None,password.expose_secret().to_owned()).await.unwrap();
    let project=app.automation_project_create("Owned Vault validation source".into(),None).await.unwrap();
    let encrypted=setup.ciphertext.lines().map(|line|format!("      {line}\n")).collect::<String>();
    let playbook=format!("---\n- name: Owned private Vault validation\n  hosts: \"{{{{ selected_group }}}}\"\n  gather_facts: false\n  vars:\n    selected_group: !vault |\n{encrypted}  tasks:\n    - name: No task execution in syntax check\n      ansible.builtin.debug:\n        msg: synthetic\n");
    for (path,text) in [
        (VAULT_INPUT_FILE.to_owned(),json!({"version":1,"ansible_vault":{"credential_id":credential.id}}).to_string()),
        ("playbook.yml".into(),playbook),
        ("ansible.cfg".into(),"[defaults]\nvault_password_file=/tool/python/consolecrypt_runner/vault_password_client.py\nvault_identity_list=evil@/work/project/evil-client\nask_vault_pass=True\n".into()),
        ("consolecrypt_runner/__init__.py".into(),String::new()),
        ("consolecrypt_runner/validation_worker.py".into(),"raise RuntimeError('PROJECT_WORKER_MUST_NOT_RUN')\n".into()),
    ] {let expected=app.automation_project_file(project.id.clone(),path.clone()).await.ok().map(|file|file.sha256);app.automation_project_save(project.id.clone(),path,expected,text).await.unwrap();}
    let context=app.automation_project_context(&project.id).await.unwrap();let source=ProjectRoot::open(&context.path).unwrap();let before=source.revision().unwrap();
    let (session,unlock)=app.unlocked().await.unwrap();drop(unlock);let registry=session.automation.vault_validation.clone();
    let outcome=tokio::time::timeout(Duration::from_secs(180),app.automation_vault_validate_private(make_input(&project.id,&before.digest,&credential.id,&runner.id),&CancellationToken::new())).await.expect("actual private owner deadline").expect("actual private owner failed");
    assert!(outcome.validation.is_valid());assert!(matches!(outcome.cleanup,ValidationCleanup::RemoteStopUnconfirmed));
    assert_eq!(outcome.validation.checks().len(),2);
    for check in outcome.validation.checks(){assert_eq!(check.exit_code,0);assert_eq!(check.output,"Vault-sensitive diagnostics withheld");}
    let safe=serde_json::to_string(&outcome.validation.ai_summary()).unwrap();assert!(!safe.contains(password.expose_secret())&&!safe.contains("PROJECT_WORKER_MUST_NOT_RUN"));
    drained(&registry).await;assert_eq!(source.revision().unwrap(),before);
    publish(&root,"positive.json",json!({"version":1,"token":token,"ok":true}));
    // Test wrapper gates only startup of operations 2/3, never an RPC/result or
    // credential response. Native context is already captured before launch.
    for (ordinal,cause) in [(2,"credential"),(3,"lock")] {
        // Independent lock negative: restore correct material BEFORE this call
        // captures its original context; a stale wrong password cannot explain
        // its failure. Coordinator also requires ZERO child requests in both.
        if ordinal==3 {app.set_credential_password(credential.id.clone(),password.expose_secret().to_owned()).await.unwrap();}
        let input=make_input(&project.id,&before.digest,&credential.id,&runner.id);let called=app.clone();
        let task=tokio::spawn(async move{called.automation_vault_validate_private(input,&CancellationToken::new()).await});
        let started=bounded_json(&root,&format!("started-{ordinal}.json")).await;assert_eq!(started["token"],token);
        if cause=="credential" {app.set_credential_password(credential.id.clone(),"synthetic replacement unavailable".into()).await.unwrap();}
        else {app.lock().await.unwrap();}
        publish(&root,&format!("release-{ordinal}.json"),json!({"version":1,"token":token}));
        assert!(tokio::time::timeout(Duration::from_secs(60),task).await.unwrap().unwrap().is_err());
        drained(&registry).await;assert_eq!(source.revision().unwrap(),before);
    }
    registry.close_and_wait().await;
    publish(&root,"native-result.json",json!({"version":1,"token":token,"ok":true,"actualRemoteFactory":true,"originalUnchanged":true,"nativeOwnedCloseAwaited":true,"remoteStop":"unconfirmed","positive":1,"contextDenials":2,"egressBlocked":pins.egress_blocked,"publicCapability":false,"executedTasks":false}));
    app.shutdown().await.unwrap();
}

#[cfg(test)]
#[path = "live_remote/normal_job.rs"]
mod normal_job;
