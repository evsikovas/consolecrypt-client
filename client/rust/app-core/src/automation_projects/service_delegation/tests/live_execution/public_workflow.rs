//! Real PreparedReview/default validation/SSH, not a reconstructed review.
//! Requires the bounded owned password-SSH/Node-custody coordinator.
use super::*;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicFixture {
    version: u8,
    token: String,
    runner_port: u16,
    runner_key: String,
    target_address: String,
    target_port: u16,
    target_key: String,
    #[serde(deserialize_with = "secret_input")]
    target_password: SecretString,
    service_identity_id: String,
    service_signing: String,
    service_recipient: String,
    schedule_id: Uuid,
    definition_digest: String,
    schedule_definition: Value,
    egress_blocked: bool,
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned actual Rust service supervisor/password-SSH fixture"]
async fn actual_public_service_prepare_confirm_status_and_closed_desktop_execution() {
    let token = std::env::var("CC_SERVICE_PROOF_TOKEN").unwrap();
    let id = Uuid::parse_str(&token).unwrap();
    assert!(!id.is_nil() && id.to_string() == token);
    let root = PathBuf::from(std::env::var_os("CC_SERVICE_PROOF_ROOT").unwrap());
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let allowed = workspace
        .join("Tasks/service-execution/control")
        .canonicalize()
        .unwrap();
    assert_eq!(
        root.canonicalize().unwrap().parent(),
        Some(allowed.as_path())
    );
    assert_eq!(
        root.file_name().unwrap().to_str().unwrap(),
        format!("owned-{token}")
    );
    assert!(
        !std::fs::symlink_metadata(&root)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let key = generate_key(KeyGenAlgorithm::Ed25519, "owned service Runner", None).unwrap();
    publish(
        &root,
        "native-public.json",
        json!({"version":1,"token":token,"publicKey":key.public_openssh}),
    );
    let mut raw = Zeroizing::new(Vec::new());
    std::io::stdin().take(65537).read_to_end(&mut raw).unwrap();
    assert!(raw.len() <= 65536);
    let fixture: PublicFixture = serde_json::from_slice(&raw).unwrap();
    drop(raw);
    assert_eq!(fixture.version, 1);
    assert_eq!(fixture.token, token);
    assert!(fixture.runner_port > 0 && fixture.target_port == 2222);
    assert!(
        fixture
            .target_address
            .parse::<std::net::Ipv4Addr>()
            .unwrap()
            .is_private()
    );
    assert!((32..=128).contains(&fixture.target_password.expose_secret().len()));
    assert!(
        !fixture.schedule_id.is_nil()
            && cc_models::automation::is_digest(&fixture.definition_digest)
    );
    let profile = tempfile::Builder::new()
        .prefix("native-")
        .tempdir_in(&root)
        .unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        profile.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile(
        "Owned service consent".into(),
        "synthetic owner passphrase42".into(),
    )
    .await
    .unwrap();
    let ssh = app
        .import_ssh_key(
            "Owned Runner key".into(),
            Some("runner".into()),
            key.private_openssh.expose_secret().to_owned(),
            None,
            false,
            None,
        )
        .await
        .unwrap();
    let password = app
        .add_password_credential(
            "Explicit owned target password".into(),
            Some("runner".into()),
            fixture.target_password.expose_secret().to_owned(),
        )
        .await
        .unwrap();
    let mut hosts = Vec::new();
    for (address, port, host_key, credential) in [
        (
            "127.0.0.1",
            fixture.runner_port,
            fixture.runner_key.as_str(),
            ssh.id.as_str(),
        ),
        (
            fixture.target_address.as_str(),
            fixture.target_port,
            fixture.target_key.as_str(),
            password.id.as_str(),
        ),
    ] {
        assert!(
            host_key.starts_with("ssh-ed25519 ")
                && host_key.len() < 8192
                && !host_key.chars().any(char::is_control)
        );
        app.import_known_hosts(format!("[{address}]:{port} {host_key}\n"))
            .await
            .unwrap();
        let mut host = HostDto::new("Owned service proof node", address);
        host.port = Some(port);
        host.username = Some("runner".into());
        host.credential_id = Some(credential.into());
        host.host_key_policy = HostKeyPolicy::Strict;
        hosts.push(app.save_host(host).await.unwrap());
    }
    let runner = app
        .automation_runner_create(
            "Owned public service".into(),
            AutomationRunnerConfig::ServiceRemote {
                host_id: hosts[0].id.clone(),
                identity_id: fixture.service_identity_id.clone(),
            },
        )
        .await
        .unwrap();
    let project = app
        .automation_project_create("Owned service source".into(), None)
        .await
        .unwrap();
    app.automation_project_save_inventory(
        project.id.clone(),
        1,
        json!({"version":2,"groups":{"fixture":[hosts[1].id]},"selected_hosts":[hosts[1].id]})
            .to_string(),
    )
    .await
    .unwrap();
    let script = format!(
        r#"import os,pathlib,socket
assert os.geteuid()!=0
assert os.environ['HOME']=='/work/home'
assert not pathlib.Path('/home/runner').exists()
assert not pathlib.Path('/var/run/docker.sock').exists()
assert not pathlib.Path('/run/docker.sock').exists()
status=pathlib.Path('/proc/self/status').read_text()
assert int(next(line.split()[1] for line in status.splitlines() if line.startswith('CapEff:')),16)==0
s=socket.socket();s.settimeout(1)
try:assert s.connect_ex(('{}',2222))!=0
finally:s.close()
try:pathlib.Path('/project/forbidden-service-write').write_text('deny')
except OSError:pass
else:raise AssertionError('project mount must be readonly')
print('SERVICE_CONTROLLER_ISOLATED')
"#,
        fixture.target_address
    );
    app.automation_project_save(
        project.id.clone(),
        "controller_probe.py".into(),
        None,
        script,
    )
    .await
    .unwrap();
    let playbook = app
        .automation_project_file(project.id.clone(), "playbook.yml".into())
        .await
        .unwrap();
    app.automation_project_save(project.id.clone(),"playbook.yml".into(),Some(playbook.sha256),r#"---
- name: Owned delegated service execution
  hosts: all
  gather_facts: false
  tasks:
    - name: Verify actual isolated controller
      ansible.builtin.assert:
        that:
          - lookup('ansible.builtin.pipe', '/opt/cc-runtime/bin/python /project/controller_probe.py') == 'SERVICE_CONTROLLER_ISOLATED'
    - name: Execute actual approved SSH task
      ansible.builtin.raw: printf 'SERVICE_CAPSULE_SSH_TASK\n'
      changed_when: false
"#.into()).await.unwrap();
    let before = app
        .automation_project_files(project.id.clone())
        .await
        .unwrap();
    let request = AutomationRunRequest {
        project_id: project.id.clone(),
        runner_id: runner.id.clone(),
        playbook: "playbook.yml".into(),
        host_ids: vec![hosts[1].id.clone()],
        mode: JobMode::Apply,
        timeout_seconds: 30,
        forks: 1,
        cancel_on_lock: false,
        parameters: RunParameters {
            extra_vars: BTreeMap::new(),
            tags: vec![],
            skip_tags: vec![],
            diff: false,
            vault: None,
        },
        retry_of: None,
        vault_credential_id: None,
        vault_selection: None, deployment_environment:None,
    };
    let (owner, u) = app.unlocked().await.unwrap();
    let profile_id = owner.profile_id.to_string();
    let scope = u.notices.scope_id().unwrap();
    drop(u);
    let current = app.automation_project_context(&project.id).await.unwrap();
    let definition = &fixture.schedule_definition;
    let input=json!({"version":1,"profileId":profile_id,"notificationScopeId":scope,
        "expectedRunnerRevision":runner.revision.unwrap(),"expectedProjectRevision":current.project.revision.unwrap(),
        "run":request,"schedule":{"zone":definition["zone"],"hour":definition["hour"],"minute":definition["minute"],
            "weekdays":definition["weekdays"]},"validitySeconds":600,"maxJobs":2,"maxPerUtcDay":2}).to_string();
    let prepared: Value = serde_json::from_str(
        &app.automation_service_prepare_json(input)
            .await
            .expect("actual public prepare"),
    )
    .unwrap();
    assert_eq!(prepared["review"]["validation"]["valid"], true);
    assert_eq!(
        prepared["service"]["identityId"],
        fixture.service_identity_id
    );
    assert_eq!(
        prepared["service"]["signingFingerprint"],
        hex::encode(sha2::Sha256::digest(
            hex::decode(&fixture.service_signing).unwrap()
        ))
    );
    assert_eq!(
        prepared["service"]["recipientFingerprint"],
        hex::encode(sha2::Sha256::digest(
            hex::decode(&fixture.service_recipient).unwrap()
        ))
    );
    assert!((1..=60000).contains(&prepared["consentRemainingMs"].as_u64().unwrap()));
    let review = prepared["review"]["review_id"].as_str().unwrap().to_owned();
    let confirm=json!({"version":1,"profileId":profile_id,"notificationScopeId":scope,"reviewId":review,"policyId":prepared["policy"]["id"]}).to_string();
    let result: Value = serde_json::from_str(
        &app.automation_service_confirm_json(confirm.clone())
            .await
            .expect("actual public confirm"),
    )
    .unwrap();
    assert_eq!(result.as_object().unwrap().len(), 10);
    assert_eq!(result["localState"], "acknowledged");
    assert_eq!(result["remoteState"], "enrolled");
    assert!(app.automation_service_confirm_json(confirm).await.is_err());
    let action=json!({"version":1,"profileId":profile_id,"notificationScopeId":scope,"runnerId":runner.id,"expectedRunnerRevision":runner.revision.unwrap()}).to_string();
    let status: Value =
        serde_json::from_str(&app.automation_service_status_json(action).await.unwrap()).unwrap();
    assert_eq!(status["localState"], "acknowledged");
    assert_eq!(status["remoteState"], "enrolled");
    assert_eq!(status["expiresUtc"], result["expiresUtc"]);
    let ledger = super::super::super::owner_ledger::OwnerLedger::capture_stale(
        &app,
        owner.clone(),
        current.unlock.clone(),
    )
    .await
    .unwrap();
    let ledger = Arc::new(ledger);
    let snapshot = ledger.read_stale_metadata(&app).await.unwrap();
    let row = snapshot
        .records()
        .iter()
        .find(|r| r.service_id.to_string() == runner.id)
        .unwrap();
    let digest = row.policy_digest.clone();
    drop(ledger);
    drop(current);
    assert_eq!(
        app.automation_project_files(project.id.clone())
            .await
            .unwrap(),
        before
    );
    let original_path = app
        .automation_project_context(&project.id)
        .await
        .unwrap()
        .path;
    assert!(
        app.automation_jobs().await.unwrap()["jobs"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    app.shutdown().await.unwrap();
    drop(owner);
    publish(
        &root,
        "native-closed.json",
        json!({"version":1,"token":token,"delegationDigest":digest,
        "nativeClosed":true,"originalDigest":before.digest,"publicApi":true,
        "scheduleId":prepared["policy"]["schedule_id"],"definitionDigest":prepared["policy"]["definition_digest"]}),
    );
    let executed = receive(&root, "service-executed.json").await;
    assert_eq!(executed["token"], token);
    assert_eq!(executed["nativeClosedBeforeExecution"], true);
    assert_eq!(executed["state"], "succeeded");
    assert_eq!(executed["targetOutcome"], "known");
    assert_eq!(executed["sameJobStore"], true);
    assert_eq!(executed["originalSlotResolved"], true);
    assert_eq!(executed["controllerIsolated"], true);
    assert_eq!(executed["actualSshTaskObserved"], true);
    assert_eq!(executed["nodeExited"], true);
    assert_eq!(
        cc_automation_core::ProjectRoot::open(&original_path)
            .unwrap()
            .revision()
            .unwrap(),
        before
    );
    publish(
        &root,
        "native-result.json",
        json!({"version":1,"token":token,"ok":true,
        "genuinePreparedReview":true,"publicFixedApi":true,"capsuleOnlySsh":true,"nativeClosedBeforeExecution":true,
        "sameJobStore":true,"originalSlotResolved":true,"controllerIsolated":true,"actualSshTaskObserved":true,
        "originalUnchanged":true,"egressBlocked":fixture.egress_blocked}),
    );
}
