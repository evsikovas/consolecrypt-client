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
async fn actual_public_service_reopen_status_revoke_before_due_and_restart_tombstone() {
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
    // Fixture platform backing persists across Desktop facade instances, like
    // an OS store. No owner keys are exported or supplied to the Node.
    let config = crate::AppConfig::for_tests(profile.path().to_string_lossy().into_owned());
    let secure: Arc<dyn cc_platform_core::SecureStore> =
        Arc::new(cc_platform_core::InMemorySecureStore::new());
    let app = AppCore::with_platform(
        config.clone(),
        secure.clone(),
        Arc::new(cc_platform_core::UnsupportedDeviceKeys),
    )
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
    let owner_signing = owner.identity().public_keys().signing;
    let owner_device = owner.device_id().to_string();
    drop(u);
    let current = app.automation_project_context(&project.id).await.unwrap();
    let definition = &fixture.schedule_definition;
    let input=json!({"version":1,"profileId":profile_id,"notificationScopeId":scope,
        "expectedRunnerRevision":runner.revision.unwrap(),"expectedProjectRevision":current.project.revision.unwrap(),
        "run":request,"schedule":{"zone":definition["zone"],"hour":definition["hour"],"minute":definition["minute"],
            "weekdays":definition["weekdays"]},"validitySeconds":900,"maxJobs":2,"maxPerUtcDay":2}).to_string();
    let prepared: Value = serde_json::from_str(
        &app.automation_service_prepare_json(input.clone())
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
    assert!(
        app.automation_service_confirm_json(confirm.clone())
            .await
            .is_err()
    );
    let action=json!({"version":1,"profileId":profile_id,"notificationScopeId":scope,"runnerId":runner.id,"expectedRunnerRevision":runner.revision.unwrap()}).to_string();
    let status: Value = serde_json::from_str(
        &app.automation_service_status_json(action.clone())
            .await
            .unwrap(),
    )
    .unwrap();
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
        "nativeClosed":true,"originalDigest":before.digest,"publicApi":true,"advancedRevoke":true,
        "ownerSigning":hex::encode(owner_signing),"ownerDeviceId":owner_device,
        "scheduleId":prepared["policy"]["schedule_id"],"definitionDigest":prepared["policy"]["definition_digest"]}),
    );
    drop(app);
    let restored = receive(&root, "service-restored.json").await;
    assert_eq!(restored["token"], token);
    assert_eq!(restored["sameIdentity"], true);
    assert_eq!(restored["sameOriginalPolicy"], true);
    assert_eq!(restored["restoredBeforeOriginalDue"], true);
    assert_eq!(restored["jobCount"], 0);
    let reopened = reopen_original(&config, secure.clone(), &profile_id).await;
    let (owner2, u2) = reopened.unlocked().await.unwrap();
    let scope2 = u2.notices.scope_id().unwrap();
    assert_ne!(scope2, scope);
    assert_eq!(owner2.identity().public_keys().signing, owner_signing);
    drop(u2);
    // Old nonce and old revision both deny BEFORE the original connector.
    assert!(
        reopened
            .automation_service_status_json(action.clone())
            .await
            .is_err()
    );
    assert!(owner2.automation.connections.lock().unwrap().is_empty());
    let mut stale: Value = serde_json::from_str(&action).unwrap();
    stale["notificationScopeId"] = json!(scope2);
    stale["expectedRunnerRevision"] = json!(runner.revision.unwrap() + 1);
    assert!(
        reopened
            .automation_service_status_json(stale.to_string())
            .await
            .is_err()
    );
    assert!(owner2.automation.connections.lock().unwrap().is_empty());
    let action2 = json!({"version":1,"profileId":profile_id,"notificationScopeId":scope2,
        "runnerId":runner.id,"expectedRunnerRevision":runner.revision.unwrap()})
    .to_string();
    let status2: Value = serde_json::from_str(
        &reopened
            .automation_service_status_json(action2.clone())
            .await
            .expect("actual reopened original status"),
    )
    .unwrap();
    assert_eq!(status2["localState"], "acknowledged");
    assert_eq!(status2["remoteState"], "restored");
    assert_eq!(status2["expiresUtc"], result["expiresUtc"]);
    // Explicit metadata disconnect leaves enrolled/restored daemon alive. Revoke
    // must independently reconnect AFTER its local tombstone, not need Status.
    reopened
        .automation_runner_disconnect(runner.id.clone())
        .await
        .unwrap();
    assert!(owner2.automation.connections.lock().unwrap().is_empty());
    let revoked: Value = serde_json::from_str(
        &reopened
            .automation_service_revoke_json(action2)
            .await
            .expect("actual public denial"),
    )
    .unwrap();
    assert_eq!(revoked["localState"], "localBlocked");
    assert_eq!(revoked["remoteState"], "revocationRecorded");
    assert_eq!(revoked["policyRevision"], result["policyRevision"]);
    assert_eq!(
        revoked["revocationEpoch"].as_u64().unwrap(),
        result["revocationEpoch"].as_u64().unwrap() + 1
    );
    reopened.shutdown().await.unwrap();
    drop(owner2);
    drop(reopened);
    publish(
        &root,
        "native-revoked.json",
        json!({"version":1,"token":token,"localBlocked":true,
        "actualRemoteAck":true,"revocationEpoch":revoked["revocationEpoch"],"policyDigest":digest}),
    );
    let observed = receive_final(&root, "service-revocation-restored.json").await;
    assert_eq!(observed["token"], token);
    assert_eq!(observed["sameIdentity"], true);
    assert_eq!(observed["knownRevocationRetained"], true);
    assert_eq!(observed["originalPolicyOtherwiseEligible"], true);
    assert_eq!(observed["originalDueAndGraceObserved"], true);
    assert_eq!(observed["newDispatchDenied"], true);
    assert_eq!(observed["jobCount"], 0);
    assert_eq!(observed["actualTaskOkCount"], 0);
    let third = reopen_original(&config, secure, &profile_id).await;
    let (owner3, u3) = third.unlocked().await.unwrap();
    let scope3 = u3.notices.scope_id().unwrap();
    assert_ne!(scope3, scope2);
    assert_eq!(owner3.identity().public_keys().signing, owner_signing);
    drop(u3);
    let action3 = json!({"version":1,"profileId":profile_id,"notificationScopeId":scope3,
        "runnerId":runner.id,"expectedRunnerRevision":runner.revision.unwrap()})
    .to_string();
    let status3: Value =
        serde_json::from_str(&third.automation_service_status_json(action3).await.unwrap())
            .unwrap();
    assert_eq!(status3["localState"], "localBlocked");
    let mut new_prepare: Value = serde_json::from_str(&input).unwrap();
    new_prepare["notificationScopeId"] = json!(scope3);
    let count = owner3.automation.connections.lock().unwrap().len();
    assert!(
        third
            .automation_service_prepare_json(new_prepare.to_string())
            .await
            .is_err()
    );
    assert_eq!(owner3.automation.connections.lock().unwrap().len(), count);
    assert!(
        third
            .automation_service_confirm_json(confirm)
            .await
            .is_err()
    );
    assert_eq!(
        third
            .automation_project_files(project.id.clone())
            .await
            .unwrap(),
        before
    );
    third.shutdown().await.unwrap();
    drop(owner3);
    drop(third);
    publish(
        &root,
        "native-advanced-result.json",
        json!({"version":1,"token":token,"ok":true,
        "publicReopenedStatus":true,"sameOriginalOwner":true,"oldNonceAndRevisionDeniedBeforeConnect":true,
        "stickyLocalTombstone":true,"actualRemoteRevocationAck":true,"noRegrantAfterRestart":true,
        "originalEligibleDueDenied":true,"originalUnchanged":true}),
    );
}

async fn reopen_original(
    config: &crate::AppConfig,
    secure: Arc<dyn cc_platform_core::SecureStore>,
    profile: &str,
) -> AppCore {
    let app = AppCore::with_platform(
        config.clone(),
        secure,
        Arc::new(cc_platform_core::UnsupportedDeviceKeys),
    )
    .unwrap();
    app.open_profile(profile.into()).await.unwrap();
    app.unlock_with_passphrase("synthetic owner passphrase42".into())
        .await
        .unwrap();
    app
}
async fn receive_final(root: &Path, name: &str) -> Value {
    // This phase observes the REAL original daily due+grace. No test clock,
    // expired-policy negative or forged scheduler slot may substitute for it.
    tokio::time::timeout(Duration::from_secs(600), async {
        loop {
            let path = root.join(name);
            if let Ok(info) = std::fs::symlink_metadata(&path) {
                assert!(info.is_file() && !info.file_type().is_symlink() && info.len() <= 16384);
                return serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("owned original due/revocation observation deadline")
}
