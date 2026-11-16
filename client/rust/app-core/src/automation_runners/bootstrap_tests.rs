//! Actual AppCore preflight: an old helper cannot trigger upload/validation,
//! target connection, enrollment or job admission merely by reporting 0.1.0.
use super::*;
use crate::automation_prepare::AutomationRunRequest;
use cc_automation_core::RunParameters;
use cc_models::automation::JobMode;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn old_helper_missing_or_false_bootstrap_denies_before_upload_target_or_job() {
    for compatibility in [None,Some(false)] {
        let directory=tempfile::tempdir().unwrap();
        let app=AppCore::new(crate::AppConfig::for_tests(directory.path().to_string_lossy().into_owned())).unwrap();
        app.create_local_profile("Owned old-helper fixture".into(),"synthetic fixture password 42".into()).await.unwrap();
        let project=app.automation_project_create("Owned source".into(),None).await.unwrap();
        let runner=Uuid::new_v4().to_string();
        let (native,helper)=tokio::io::duplex(65536);
        let (reader,writer)=tokio::io::split(native);
        let peer=HelperPeer::connect(reader,writer);
        let fixture=tokio::spawn(async move{
            let (reader,mut writer)=tokio::io::split(helper);
            let mut lines=BufReader::new(reader).lines();
            let line=lines.next_line().await.unwrap().unwrap();
            let request:Value=serde_json::from_str(&line).unwrap();assert_eq!(request["operation"],"probe");
            let mut probe:Value=serde_json::from_str(include_str!("../../../automation-core/src/vault_validation/runtime-pin-v1.json")).unwrap();
            probe["capabilities"]["target_match_v1"]=json!(true);
            probe["capabilities"].as_object_mut().unwrap().remove("trusted_worker_bootstrap_v1");
            if let Some(value)=compatibility{probe["capabilities"]["trusted_worker_bootstrap_v1"]=json!(value);}
            assert_eq!(probe["helper"],"0.1.0");
            let response=json!({"version":1,"type":"response","request_id":request["request_id"],"result":probe,"error":null});
            let mut bytes=serde_json::to_vec(&response).unwrap();bytes.push(b'\n');writer.write_all(&bytes).await.unwrap();writer.flush().await.unwrap();
            // Retain reader until explicit normal shutdown. ANY extra protocol
            // dispatch (upload/validate/enroll/job/cancel) is a regression.
            assert!(tokio::time::timeout(Duration::from_secs(10),lines.next_line()).await.unwrap().unwrap().is_none());
        });
        let (owner,unlock)=app.unlocked().await.unwrap();drop(unlock);
        owner.automation.connections.lock().unwrap().insert(runner.clone(),Arc::new(RunnerConnection{
            id:runner.clone(),revision:1,configuration:AutomationRunnerConfig::Remote{
                host_id:Uuid::new_v4().to_string(),helper:cc_automation_core::RemoteHelperConfig{python:"/owned/no-exec/python".into(),module_dir:"/owned/no-exec/src".into(),root:"/owned/no-exec/state".into()}},
            peer:peer.clone(),probe:Value::Null,remote:None,local:None,authorization:tokio::sync::Mutex::new(None),
        }));
        let request=AutomationRunRequest{project_id:project.id,runner_id:runner,playbook:"playbook.yml".into(),host_ids:vec![Uuid::new_v4().to_string()],mode:JobMode::Check,
            parameters:RunParameters{extra_vars:BTreeMap::new(),tags:vec![],skip_tags:vec![],diff:false,vault:None},timeout_seconds:60,forks:1,cancel_on_lock:true,retry_of:None,vault_credential_id:None,vault_selection:None,deployment_environment:None};
        let error=tokio::time::timeout(Duration::from_secs(5),app.automation_prepare_run(request)).await.unwrap().unwrap_err();
        assert!(matches!(error,AppError::InvalidInput{field,..}if field=="automation_runner_update"));
        assert!(owner.automation.reviews.lock().unwrap().is_empty());
        assert!(app.automation_jobs().await.unwrap()["jobs"].as_array().unwrap().is_empty());
        peer.close().await;fixture.await.unwrap();app.shutdown().await.unwrap();
    }
}
