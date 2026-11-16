//! A persisted service connection is not an ordinary manual-job producer.
use super::*;
use crate::automation_prepare::AutomationRunRequest;
use cc_automation_core::RunParameters;
use cc_models::automation::JobMode;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncWriteExt, BufReader};

#[tokio::test]
async fn ordinary_prepare_service_remote_denies_before_probe_or_target_or_enrollment() {
    let directory = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        directory.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile(
        "Owned service role fence".into(),
        "synthetic role fixture password42".into(),
    )
    .await
    .unwrap();
    let host = app
        .save_host(crate::HostDto::new("Owned undialed host", "127.0.0.1"))
        .await
        .unwrap();
    let project = app
        .automation_project_create("Owned role source".into(), None)
        .await
        .unwrap();
    app.automation_project_save_inventory(
        project.id.clone(),
        1,
        json!({"version":2,"groups":{"owned":[host.id]},"selected_hosts":[host.id]}).to_string(),
    )
    .await
    .unwrap();
    let runner = Uuid::new_v4().to_string();
    let calls = Arc::new(AtomicUsize::new(0));
    let requests = calls.clone();
    let (native, helper) = tokio::io::duplex(65536);
    let (reader, writer) = tokio::io::split(native);
    let peer = HelperPeer::connect(reader, writer);
    let fixture = tokio::spawn(async move {
        let (reader, mut writer) = tokio::io::split(helper);
        let mut reader = BufReader::new(reader);
        while let Ok(Some(raw)) = cc_automation_core::read_frame(&mut reader).await {
            requests.fetch_add(1, Ordering::SeqCst);
            let request: Value = serde_json::from_slice(&raw).unwrap();
            // Reject everything. A missing producer fence reaches this peer and
            // fails with a DIFFERENT reason/count, not a fabricated positive grant.
            let mut reply = serde_json::to_vec(&json!({"version":1,"type":"response",
                "request_id":request["request_id"],"result":null,
                "error":{"code":"fixture_denied","message":"No ordinary helper endpoint"}}))
            .unwrap();
            reply.push(b'\n');
            if writer.write_all(&reply).await.is_err() {
                break;
            }
        }
    });
    let (owner, unlock) = app.unlocked().await.unwrap();
    drop(unlock);
    owner.automation.connections.lock().unwrap().insert(
        runner.clone(),
        Arc::new(RunnerConnection {
            id: runner.clone(),
            revision: 1,
            configuration: AutomationRunnerConfig::ServiceRemote {
                host_id: host.id.clone(),
                identity_id: Uuid::new_v4().to_string(),
            },
            peer: peer.clone(),
            probe: Value::Null,
            remote: None,
            local: None,
            authorization: tokio::sync::Mutex::new(None),
        }),
    );
    let request = AutomationRunRequest {
        project_id: project.id,
        runner_id: runner,
        playbook: "playbook.yml".into(),
        host_ids: vec![host.id],
        mode: JobMode::Check,
        parameters: RunParameters {
            extra_vars: BTreeMap::new(),
            tags: vec![],
            skip_tags: vec![],
            diff: false,
            vault: None,
        },
        timeout_seconds: 60,
        forks: 1,
        cancel_on_lock: true,
        retry_of: None,
        vault_credential_id: None,
        vault_selection: None, deployment_environment:None,
    };
    let error = tokio::time::timeout(Duration::from_secs(3), app.automation_prepare_run(request))
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(error, AppError::InvalidInput { field, reason }
        if field=="automation_run" && reason=="service Runner requires separate delegation review"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(owner.automation.reviews.lock().unwrap().is_empty());
    assert!(app.automation_jobs().await.unwrap()["jobs"]
        .as_array()
        .unwrap()
        .is_empty());
    // Close this test-owned endpoint before profile cleanup; no service command
    // or vault-state broadcast is necessary to prove a pre-network denial.
    peer.close().await;
    fixture.await.unwrap();
    app.shutdown().await.unwrap();
}


#[tokio::test]
async fn actual_saved_service_reference_roundtrips_same_parser_and_future_row_stays_unchanged() {
    let directory=tempfile::tempdir().unwrap();
    let app=AppCore::new(crate::AppConfig::for_tests(directory.path().to_string_lossy().into_owned())).unwrap();
    app.create_local_profile("Owned service reference".into(),"synthetic reference pass42".into()).await.unwrap();
    let host=app.save_host(crate::HostDto::new("Owned saved service endpoint","127.0.0.1")).await.unwrap();
    let identity=Uuid::new_v4().to_string();
    let created=app.automation_runner_create("Owned service".into(),AutomationRunnerConfig::ServiceRemote {host_id:host.id.clone(),identity_id:identity.clone()}).await.unwrap();
    let (owner,unlock)=app.unlocked().await.unwrap();drop(unlock);let storage=owner.storage().unwrap();let id=created.id.clone();
    let original=storage.read(move|tx|stored_runner(tx,&id)).await.unwrap();
    assert_eq!(original.configuration.as_ref().unwrap().service_remote().unwrap(),(host.id.as_str(),identity.as_str()));
    let runner_id=created.id.clone();
    storage.write(move|tx|->AppResult<()> {let mut catalog:Value=tx.setting_get(RUNNERS)?.unwrap();catalog["records"][&runner_id]["futureMeaning"]=json!({"preserve":true});tx.setting_set(RUNNERS,&catalog)?;Ok(())}).await.unwrap();
    let before:Value=storage.setting_get(RUNNERS).await.unwrap().unwrap();let id=created.id;
    assert!(storage.read(move|tx|stored_runner(tx,&id)).await.is_err());
    let after:Value=storage.setting_get(RUNNERS).await.unwrap().unwrap();assert_eq!(before,after);
    app.shutdown().await.unwrap();
}

#[test]
fn service_remote_has_no_python_url_command_or_approval_fields() {
    let host=Uuid::new_v4().to_string();let identity=Uuid::new_v4().to_string();
    let valid=json!({"kind":"service_remote","host_id":host,"identity_id":identity});
    let config:AutomationRunnerConfig=serde_json::from_value(valid.clone()).unwrap();config.validate().unwrap();
    for field in ["python","root","url","command","approved"] {let mut value=valid.clone();value[field]=json!("caller supplied");assert!(serde_json::from_value::<AutomationRunnerConfig>(value).is_err());}
    assert!(AutomationRunnerConfig::ServiceRemote {host_id:host,identity_id:Uuid::nil().to_string()}.validate().is_err());
}


#[tokio::test]
async fn captured_service_plan_equal_revision_mutation_denies_before_probe_and_scope_reopen() {
    use crate::working_set::Readback;
    let directory=tempfile::tempdir().unwrap();let app=AppCore::new(crate::AppConfig::for_tests(directory.path().to_string_lossy().into_owned())).unwrap();
    app.create_local_profile("Owned service connect source".into(),"synthetic original source pass42".into()).await.unwrap();
    let host=app.save_host(crate::HostDto::new("Owned immutable dial source","192.0.2.11")).await.unwrap();
    let config=AutomationRunnerConfig::ServiceRemote {host_id:host.id.clone(),identity_id:Uuid::new_v4().to_string()};
    let runner=app.automation_runner_create("Owned original source".into(),config.clone()).await.unwrap();
    let (owner,u)=app.unlocked().await.unwrap();let id=crate::dto::parse_id("host_id",&host.id).unwrap();
    let Readback::Live(mut data)=u.writer.readback(id).await.unwrap() else {panic!("actual saved host")};
    let revision=data.revision;let stamp=ServiceConnectSource::capture(&u).unwrap();stamp.current(&u).unwrap();
    let watched=tokio_util::sync::CancellationToken::new();assert!(u.working().watch_revisions(BTreeMap::from([(id,revision)]),watched.clone()));
    let cc_models::VaultObject::Host(changed)=&mut data.payload.object else {panic!("actual host kind")};changed.address="192.0.2.12".into();
    u.working().apply(id,Readback::Live(data));assert_eq!(u.working().revision_of(id),Some(revision));assert!(!watched.is_cancelled(),"old revision-only guard misses coalesced body edit");
    assert!(stamp.current(&u).is_err());
    let (native,helper)=tokio::io::duplex(65536);let (read,write)=tokio::io::split(native);let peer=HelperPeer::connect(read,write);let calls=Arc::new(AtomicUsize::new(0));let count=calls.clone();
    let server=tokio::spawn(async move {let (read,_write)=tokio::io::split(helper);let mut read=BufReader::new(read);while let Ok(Some(_))=cc_automation_core::read_frame(&mut read).await {count.fetch_add(1,Ordering::SeqCst);}});
    let probe=json!({SERVICE_SOURCE_FIELD:serde_json::to_value(&stamp).unwrap()});
    let connection=Arc::new(RunnerConnection {id:runner.id.clone(),revision:1,configuration:config.clone(),peer:peer.clone(),probe,remote:None,local:None,authorization:tokio::sync::Mutex::new(None)});
    owner.automation.connections.lock().unwrap().insert(runner.id.clone(),connection);
    let result=app.automation_service_connect_original(owner.clone(),Arc::downgrade(&u),&runner.id,1,&config).await;
    assert!(matches!(result,Err(AppError::InvalidInput{field,reason}) if field=="service_owner" && reason=="service_source_changed_reconnect"));assert_eq!(calls.load(Ordering::SeqCst),0);
    peer.close().await;server.await.unwrap();drop(u);
    app.lock().await.unwrap();app.unlock_with_passphrase("synthetic original source pass42".into()).await.unwrap();let (_,fresh)=app.unlocked().await.unwrap();assert!(stamp.current(&fresh).is_err());drop(fresh);app.shutdown().await.unwrap();
}
