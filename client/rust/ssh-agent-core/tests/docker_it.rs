//! Built-in agent + OpenSSH fallback against the Docker testbed.
//! Run with `CC_SSH_IT=1 cargo test -p cc-ssh-agent-core --test docker_it -- --nocapture`.

use cc_models::credential::{Credential, CredentialKind};
use cc_ssh_agent_core::{
    prepare_openssh, start_agent, AgentKey, AgentListenOptions, AgentService, LaunchOptions,
};
use cc_ssh_core::keys;
use cc_ssh_core::openssh::detect_openssh;
use cc_ssh_core::russh::keys::ssh_key::Certificate;
use cc_ssh_core::testing::{self, connector, SshTestbed};
use cc_ssh_core::{ConnectionPlanner, HostKeyDecision, MemoryKnownHosts};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn docker_agent_and_openssh_fallback() {
    if !testing::enabled() {
        eprintln!("skipped: set CC_SSH_IT=1 to run Docker integration tests");
        return;
    }
    let tb = tokio::task::spawn_blocking(|| SshTestbed::start(true))
        .await
        .unwrap()
        .expect("testbed");
    let ti = tb.inventory();
    let s = &tb.secrets;

    // Built-in agent holding the plain key and the certificate-only key.
    let key = keys::load_private_key(&s.key.private_openssh, None).unwrap();
    let cert_key = keys::load_private_key(&s.cert_key.private_openssh, None).unwrap();
    let cert = Certificate::from_openssh(&s.cert_line).unwrap();
    let agent = start_agent(
        AgentService::new(vec![
            AgentKey::new(cert_key, Some(cert), "cert").unwrap(),
            AgentKey::new(key, None, "key").unwrap(),
        ]),
        &AgentListenOptions::default(),
    )
    .await
    .unwrap();

    // 1. Native connector authenticating every hop through the agent.
    let mut agent_cred = Credential::new("built-in agent", CredentialKind::ExternalAgent);
    agent_cred.agent_path = Some(agent.path().to_string_lossy().to_string());
    let agent_cred_id = ti.inventory.add_credential(agent_cred);
    for id in [ti.bastion1, ti.bastion2.unwrap(), ti.target.unwrap()] {
        ti.inventory
            .update_host(id, |h| h.credential_id = Some(agent_cred_id));
    }
    let (c, _) = connector(
        ti.resolver.clone(),
        Arc::new(MemoryKnownHosts::new()),
        HostKeyDecision::Reject,
    );
    let planner = ConnectionPlanner::new(ti.inventory.clone());
    let plan = planner.plan(ti.target.unwrap()).await.unwrap();
    let session = c.connect(&plan).await.unwrap();
    assert_eq!(
        session.exec("whoami").await.unwrap().stdout_lossy(),
        "tester\n"
    );
    session.disconnect().await.unwrap();
    println!("IT native_auth_via_builtin_agent_3_hops ... ok");
    drop(agent);

    // 2. System OpenSSH through the chain, keys served by a per-session agent.
    let Some(ssh) = detect_openssh() else {
        println!("IT openssh_fallback ... SKIPPED (no OpenSSH client found)");
        return;
    };
    println!(
        "   using {} ({})",
        ssh.path.display(),
        ssh.version.clone().unwrap_or_default()
    );
    for id in [ti.bastion1, ti.bastion2.unwrap()] {
        ti.inventory
            .update_host(id, |h| h.credential_id = Some(ti.cred_key));
    }
    // target: passphrase-protected key, passphrase supplied by the "prompt"
    ti.inventory.update_host(ti.target.unwrap(), |h| {
        h.credential_id = Some(ti.cred_encrypted)
    });
    ti.resolver
        .set_prompt_passphrase(ti.cred_encrypted, s.encrypted_passphrase.clone());
    let plan = planner.plan(ti.target.unwrap()).await.unwrap();
    let launch = prepare_openssh(
        &ssh,
        &plan,
        ti.resolver.as_ref(),
        &[],
        &LaunchOptions {
            remote_command: Some("hostname; whoami".into()),
            request_tty: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let cmd = launch.command.clone();
    let out = tokio::time::timeout(
        Duration::from_secs(60),
        tokio::process::Command::from(cmd.to_command())
            .stdin(Stdio::null())
            .output(),
    )
    .await
    .expect("ssh timed out")
    .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "ssh failed: {stderr}\nconfig:\n{}",
        cmd.config
    );
    assert_eq!(
        stdout.trim(),
        format!("{}\ntester", tb.target.as_deref().unwrap()),
        "stderr: {stderr}"
    );
    let learned = launch.learned_known_hosts();
    assert_eq!(learned.len(), 3, "one TOFU entry per hop: {learned:?}");
    let dir = launch.session_dir().to_path_buf();
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_none() && p.file_name().unwrap() != "agent.sock" {
            let text = std::fs::read_to_string(&p).unwrap_or_default();
            assert!(!text.contains("PRIVATE KEY"), "{}", p.display());
        }
    }
    drop(launch);
    assert!(!dir.exists());
    println!("IT openssh_fallback_proxyjump_builtin_agent ... ok");
    drop(tb);
}
