//! Two isolated containers: Runner != target. The private SSH key remains native.
use base64::{engine::general_purpose::STANDARD, Engine};
use cc_automation_core::{
    target_from_session, EnrolledJobGrant, HelperLaunch, InputDigestWitness, JobScopeRegistry,
    JobTargetScope, ProjectRoot, RemoteHelperConfig, RunParameters, SignedJobApproval,
    TargetHandler, TargetRequest, ValidationProof, VerifiedJob,
};
use cc_models::{
    automation::JobManifest,
    credential::{Credential, CredentialKind},
    host::{Host, HostKeyPolicy},
};
use cc_ssh_core::keys::{generate_key, KeyGenAlgorithm};
use cc_ssh_core::{
    ConnectionPlanner, FixedHostKeyPrompt, HostKeyDecision, MemoryCredentialResolver,
    MemoryInventory, MemoryKnownHosts, ResolvedCredential, SshConnector,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{self, BufRead, Write},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    port: u16,
    host_public_key: String,
    target_port: u16,
    target_address: String,
    target_public_key: String,
    project_path: std::path::PathBuf,
    #[serde(default)]
    wsl_launch: Option<HelperLaunch>,
    #[serde(default)]
    native_launch: Option<HelperLaunch>,
}
fn digest(value: Value) -> String {
    fn canonical(value: Value) -> Value {
        match value {
            Value::Object(map) => Value::Object(
                map.into_iter()
                    .collect::<BTreeMap<_, _>>()
                    .into_iter()
                    .map(|(k, v)| (k, canonical(v)))
                    .collect(),
            ),
            Value::Array(array) => Value::Array(array.into_iter().map(canonical).collect()),
            other => other,
        }
    }
    hex::encode(Sha256::digest(
        serde_json::to_vec(&canonical(value)).unwrap(),
    ))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = generate_key(
        KeyGenAlgorithm::Ed25519,
        "cc-disposable-execution-fixture",
        None,
    )?;
    let target_key = generate_key(
        KeyGenAlgorithm::Ed25519,
        "cc-disposable-target-fixture",
        None,
    )?;
    println!(
        "{}",
        json!({"public_key": key.public_openssh, "target_public_key": target_key.public_openssh})
    );
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    let fixture: Fixture = serde_json::from_str(&line)?;
    if let Some(launch) = &fixture.native_launch {
        if !cfg!(target_os = "linux") || fixture.wsl_launch.is_some()
            || !matches!(launch, HelperLaunch::Native { .. })
        {
            return Err("native-local fixture requires explicit Linux Native launch only".into());
        }
    }
    let inventory = Arc::new(MemoryInventory::new());
    let credential = Credential::new("disposable fixture", CredentialKind::SshPrivateKey);
    let target_credential = Credential::new("different target key", CredentialKind::SshPrivateKey);
    let resolver = Arc::new(MemoryCredentialResolver::new());
    resolver.insert(
        credential.id,
        ResolvedCredential::PrivateKey {
            openssh: key.private_openssh,
            passphrase: None,
            certificate: None,
        },
    );
    let mut ids = Vec::new();
    resolver.insert(
        target_credential.id,
        ResolvedCredential::PrivateKey {
            openssh: target_key.private_openssh,
            passphrase: None,
            certificate: None,
        },
    );
    for (name, address, port, credential_id) in [
        ("isolated Runner", "127.0.0.1", fixture.port, credential.id),
        (
            "separate target",
            fixture.target_address.as_str(),
            fixture.target_port,
            target_credential.id,
        ),
    ] {
        let mut host = Host::new(name, address);
        host.port = Some(port);
        host.username = Some("runner".into());
        host.credential_id = Some(credential_id);
        host.host_key_policy = HostKeyPolicy::Strict;
        ids.push(inventory.add_host(host));
    }
    inventory.add_credential(credential);
    inventory.add_credential(target_credential);
    let planner = ConnectionPlanner::new(inventory);
    let known_hosts = format!(
        "[127.0.0.1]:{} {}\n[{}]:{} {}\n",
        fixture.port,
        fixture.host_public_key.trim(),
        fixture.target_address,
        fixture.target_port,
        fixture.target_public_key.trim()
    );
    let (entries, errors) = cc_ssh_core::known_hosts::import_known_hosts(&known_hosts);
    assert!(errors.is_empty() && entries.len() == 2);
    let connector = SshConnector::new(
        resolver,
        Arc::new(MemoryKnownHosts::with_entries(entries)),
        Arc::new(FixedHostKeyPrompt::new(HostKeyDecision::Reject)),
    );
    let runner_plan = planner.plan(ids[0]).await?;
    let runner_session = if fixture.native_launch.is_some() {
        None // Local adapter never reaches the helper through SSH.
    } else {
        Some(connector.connect(&runner_plan).await?)
    };
    println!("phase: runner-origin-ready");
    let registry = Arc::new(JobScopeRegistry::default());
    let mut local_helper = None;
    let peer = if let Some(launch) = fixture.native_launch.as_ref().or(fixture.wsl_launch.as_ref()) {
        let helper = launch.spawn_with_targets(Some(registry.clone())).await?;
        let peer = helper.peer.clone();
        local_helper = Some(helper);
        peer
    } else {
        RemoteHelperConfig {
            python: "/opt/cc-runtime/bin/python".into(),
            module_dir: "/opt/consolecrypt/src".into(),
            root: "/home/runner/execution state".into(),
        }
        .connect_with_targets(runner_session.as_ref().ok_or("remote Runner required")?, Some(registry.clone()))
        .await?
    };
    let probe = peer
        .request("probe", json!({}), Duration::from_secs(30))
        .await?;
    assert_eq!(probe["dependencies_ready"], true, "{probe}");
    assert_eq!(probe["sandbox"]["network"], true);
    println!("phase: isolated-helper-ready");
    let mut target_plan = planner.plan(ids[1]).await?;
    let target_session = if fixture.native_launch.is_some() {
        // SAME local Rust process owns native-origin SSH credentials and channels;
        // the networkless Ansible helper sees only the original scoped broker.
        Arc::new(connector.connect(&target_plan).await?)
    } else if let Some(launch) = &fixture.wsl_launch {
        let stream = launch
            .wsl_origin_stream(
                &fixture.target_address,
                fixture.target_port,
                probe["environment_digest"].as_str().unwrap(),
            )
            .await?;
        let origin = stream.scope_digest();
        Arc::new(
            connector
                .connect_over_stream(&target_plan, stream, origin)
                .await?,
        )
    } else {
        // No published target port. The Runner originates the TCP connection;
        // native Rust verifies different keys on both SSH hops.
        target_plan.route.insert(0, runner_plan.target_hop());
        let session = Arc::new(connector.connect(&target_plan).await?);
        assert_eq!(session.hop_count(), 2);
        session
    };
    let target = target_from_session("fixture".into(), &target_session)?;
    println!("phase: target-ssh-ready");
    let project = ProjectRoot::open(&fixture.project_path)?;
    let revision = project.revision()?;
    let captured = project.upload(&revision, &peer).await?;
    let static_inventory = json!({"version":1,"hosts":[{"alias":target.alias,"host_id":target.host_id,
        "address":target.address,"port":target.port,"username":target.username,
        "variables":{"ansible_python_interpreter":"/usr/bin/python3.12"},"sources":["fixture"],"credential_refs":target.credential_refs}],
        "groups":{},"source_digests":{"fixture":digest(serde_json::to_value(&target)?)}});
    let parameters =
        json!({"extra_vars":{"threshold":1e-7,"large":1e20},"tags":[],"skip_tags":[],"diff":false});
    let targets = json!([target]);
    let report = peer
        .request(
            "prepare_run",
            json!({"revision_id":captured.revision_id,"playbook":"playbook.yml",
        "inventory":static_inventory,"parameters":parameters,"targets":targets}),
            Duration::from_secs(120),
        )
        .await?;
    assert_eq!(report["valid"], true, "{report}");
    assert_eq!(report["inventory_digest"], digest(static_inventory.clone()));
    for (name, expected) in [
        ("inventory", &static_inventory),
        ("parameters", &parameters),
    ] {
        let witness: InputDigestWitness =
            serde_json::from_value(report["input_witnesses"][name].clone())?;
        witness.verify(expected)?;
        assert_eq!(report[format!("{name}_digest")], witness.digest);
    }
    let parsed_parameters: RunParameters = serde_json::from_value(parameters.clone())?;
    let parsed_targets =
        serde_json::from_value::<Vec<cc_models::automation::ApprovedTarget>>(targets.clone())?;
    let verified_validation = serde_json::from_value::<ValidationProof>(report.clone())?.verify(
        &captured.digest,
        probe["environment_digest"].as_str().unwrap(),
        "playbook.yml",
        &parsed_parameters,
        &static_inventory,
        &parsed_targets,
    )?;
    assert!(verified_validation.is_valid());
    let device = cc_crypto_core::DeviceSecretKeys::generate()?;
    let signer = Uuid::new_v4();
    let tenant = Uuid::new_v4();
    let runner = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
    let grant = EnrolledJobGrant {
        signer_id: signer,
        public_key: device.public_keys().signing,
        tenant_id: tenant,
        runner_id: runner,
        projects: [project_id].into(),
        hosts: [target.host_id].into(),
        jobs_execute: true,
        expires_at: now + 3600,
        max_job_seconds: 120,
    };
    peer.request("configure_interactive", json!({"tenant_id":tenant,"runner_id":runner,"signer_id":signer,
        "public_key":STANDARD.encode(device.public_keys().signing),"hosts":[target.host_id],"projects":[project_id],
        "expires_at":now+3500,"max_job_seconds":120,"transport":"native_scoped_rpc_v1"}), Duration::from_secs(10)).await?;
    let manifest: JobManifest = serde_json::from_value(
        json!({"version":1,"job_id":Uuid::new_v4(),"tenant_id":tenant,
        "runner_id":runner,"project_id":project_id,"revision_digest":captured.digest,"inventory_digest":report["inventory_digest"],
        "environment_digest":report["environment_digest"],"parameters_digest":report["parameters_digest"],"playbook":"playbook.yml",
        "policy":{"strict_isolation":true,"cancel_on_lock":true,"timeout_seconds":120,"forks":5,"production":false},
        "mode":"apply","targets":targets,"issued_at":now,"expires_at":now+600,"access_expires_at":now+120,"nonce":Uuid::new_v4()}),
    )?;
    let approval = SignedJobApproval {
        signer_id: signer,
        signature: STANDARD.encode(device.sign_automation_manifest(&manifest)?),
        manifest,
    };
    let verified = VerifiedJob::verify(
        &approval,
        &grant,
        report["environment_digest"].as_str().unwrap(),
        now,
    )?;
    let manifest_digest = verified.digest().to_string();
    let scope = JobTargetScope::bind(verified, [(target.host_id, target_session.clone())].into())?;
    registry.register(scope.clone())?;
    let forbidden = TargetRequest {
        version: 1,
        kind: "target_request".into(),
        request_id: Uuid::new_v4(),
        job_id: approval.manifest.job_id,
        manifest_digest: manifest_digest.clone(),
        host_id: Uuid::new_v4(),
        operation: "exec".into(),
        body: json!({"command":"must not run","stdin":""}),
    };
    assert!(registry.handle(forbidden).await.is_err());
    let submission = json!({"approval":approval,"revision_id":captured.revision_id,"inventory":static_inventory,"parameters":parameters});
    let accepted = peer
        .request("accept_job", submission.clone(), Duration::from_secs(30))
        .await?;
    assert_eq!(accepted["created"], true);
    let repeated = peer
        .request("accept_job", submission, Duration::from_secs(30))
        .await?;
    assert_eq!(repeated["created"], false);
    let mut state;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(150);
    loop {
        state = peer
            .request(
                "job_get",
                json!({"job_id":approval.manifest.job_id}),
                Duration::from_secs(10),
            )
            .await?;
        if !matches!(
            state["state"].as_str(),
            Some("queued" | "running" | "cancelling")
        ) {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "job timeout {state}"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let events = peer
        .request(
            "job_events",
            json!({"job_id":approval.manifest.job_id,"after":0,"limit":100}),
            Duration::from_secs(10),
        )
        .await?;
    assert_eq!(
        state["state"], "succeeded",
        "state={state}; events={events}"
    );
    assert!(events["events"]
        .as_array()
        .is_some_and(|events| !events.is_empty()));
    let no_replay = peer
        .request(
            "job_events",
            json!({"job_id":approval.manifest.job_id,"after":events["cursor"],"limit":100}),
            Duration::from_secs(10),
        )
        .await?;
    assert_eq!(no_replay["events"], json!([]));
    let proof = target_session
        .exec("cat /home/runner/consolecrypt-proof.txt")
        .await?;
    assert_eq!(proof.stdout_lossy(), "ConsoleCrypt scoped execution\n");
    if let Some(session) = &runner_session {
        assert_eq!(session.exec("test ! -e /home/runner/consolecrypt-proof.txt").await?.exit_status, Some(0));
    } else {
        assert!(!std::path::Path::new("/home/runner/consolecrypt-proof.txt").exists());
    }
    println!(
        "{}",
        json!({"status":"PASS","transport":if fixture.native_launch.is_some() {"Linux Native HelperLaunch stdio -> networkless Ansible Runner -> original scoped native SSH/SFTP -> separate target"} else {"Windows native -> strict SSH -> networkless Runner -> scoped native SSH/SFTP -> separate target"},
        "job_id":approval.manifest.job_id,"state":state["state"],"events":events["cursor"],"replay_created":false,
        "outside_target_denied":true,"target_marker_verified":true,"runner_marker_absent":true,"private_keys_in_guest":false,
        "ssh_origin":if fixture.native_launch.is_some() {"same local Linux kernel"} else if fixture.wsl_launch.is_some() {"explicit WSL2 distribution/user"} else {"selected remote Runner"},
        "native_local_adapter":fixture.native_launch.is_some(),
        "different_hop_keys":fixture.wsl_launch.is_none() && fixture.native_launch.is_none()})
    );
    registry.cancel_all();
    if let Some(helper) = local_helper {
        helper.close().await;
    } else {
        peer.close().await;
    }
    target_session.disconnect().await?;
    if let Some(session) = runner_session { session.disconnect().await?; }
    Ok(())
}
