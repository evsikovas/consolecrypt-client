use super::*;
use async_trait::async_trait;
use cc_models::credential::{Credential, CredentialKind};
use cc_models::host::{HostKeyPolicy, SshBackend};
use cc_models::known_host::KnownHostSource;
use cc_models::ObjectId;
use cc_ssh_core::keys::{self, CertificateKind, KeyGenAlgorithm};
use cc_ssh_core::known_hosts::new_known_host;
use cc_ssh_core::openssh::OpenSshClient;
use cc_ssh_core::russh::keys::agent::AgentIdentity;
use cc_ssh_core::russh::keys::signature::Verifier;
use cc_ssh_core::russh::keys::ssh_encoding::Decode;
use cc_ssh_core::russh::keys::ssh_key::{Certificate, HashAlg, Signature};
use cc_ssh_core::{ConnectionPlan, Endpoint, Hop, MemoryCredentialResolver, ResolvedCredential};
use secrecy::{ExposeSecret, SecretString};
use std::sync::Arc;

fn gen(
    alg: KeyGenAlgorithm,
    comment: &str,
) -> (keys::GeneratedKey, cc_ssh_core::russh::keys::PrivateKey) {
    let g = keys::generate_key(alg, comment, None).unwrap();
    let k = keys::load_private_key(&g.private_openssh, None).unwrap();
    (g, k)
}

/// russh's `sign_request` returns `data || string(signature)`.
fn verify(identity: &AgentIdentity, data: &[u8], out: &[u8]) -> Signature {
    assert!(out.starts_with(data));
    let mut rest = &out[data.len() + 4..];
    let sig = Signature::decode(&mut rest).unwrap();
    identity
        .public_key()
        .key_data()
        .verify(data, &sig)
        .expect("valid signature");
    sig
}

#[tokio::test]
async fn serves_identities_and_signs_ed25519_rsa_and_certificates() {
    let dir = tempfile::tempdir().unwrap();
    let (_, ed) = gen(KeyGenAlgorithm::Ed25519, "ed");
    let (_, rsa) = gen(KeyGenAlgorithm::Rsa3072, "rsa");
    let (_, ca) = gen(KeyGenAlgorithm::Ed25519, "ca");
    let (_, certified) = gen(KeyGenAlgorithm::Ed25519, "certified");
    let cert_line = keys::sign_certificate(
        &ca,
        certified.public_key(),
        CertificateKind::User,
        "id",
        &["alice"],
        0,
        u64::MAX,
        true,
    )
    .unwrap();
    let cert = Certificate::from_openssh(&cert_line).unwrap();
    let service = AgentService::new(vec![
        AgentKey::new(ed, None, "ed-key").unwrap(),
        AgentKey::new(rsa, None, "rsa-key").unwrap(),
        AgentKey::new(certified, Some(cert), "cert-key").unwrap(),
    ]);
    assert_eq!(service.identity_count(), 4);
    let agent = start_agent(
        service,
        &AgentListenOptions {
            parent_dir: Some(dir.path().to_path_buf()),
        },
    )
    .await
    .unwrap();
    let path = agent.path().to_path_buf();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let sock = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let d = std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(sock, 0o600);
        assert_eq!(d, 0o700);
    }

    let infos = os_agent::list_agent_identities(Some(&path)).await.unwrap();
    assert_eq!(infos.len(), 4);
    assert_eq!(infos.iter().filter(|i| i.is_certificate).count(), 1);
    assert!(infos
        .iter()
        .any(|i| i.comment == "rsa-key" && i.algorithm_name == "ssh-rsa"));

    let mut client = os_agent::connect_agent(Some(&path)).await.unwrap();
    let ids = client.request_identities().await.unwrap();
    let data = b"session-id-and-userauth-request".to_vec();
    for id in &ids {
        let is_rsa = id.public_key().algorithm().is_rsa();
        let hash = if is_rsa { Some(HashAlg::Sha512) } else { None };
        let sig = client.sign_request(id, hash, data.clone()).await.unwrap();
        let sig = verify(id, &data, &sig);
        if is_rsa {
            assert_eq!(sig.algorithm().as_str(), "rsa-sha2-512");
        }
    }
    // RSA with SHA-256 flag
    let rsa_id = ids
        .iter()
        .find(|i| i.public_key().algorithm().is_rsa())
        .unwrap();
    let sig = client
        .sign_request(rsa_id, Some(HashAlg::Sha256), data.clone())
        .await
        .unwrap();
    assert_eq!(
        verify(rsa_id, &data, &sig).algorithm().as_str(),
        "rsa-sha2-256"
    );

    // unknown key → failure; add identity refused (read-only agent)
    let (_, other) = gen(KeyGenAlgorithm::Ed25519, "other");
    let unknown = AgentIdentity::from(other.public_key().clone());
    assert!(client
        .sign_request(&unknown, None, data.clone())
        .await
        .is_err());
    assert!(client.add_identity(&other, &[]).await.is_err());

    drop(client);
    drop(agent);
    assert!(!path.exists(), "socket removed on drop");
    assert!(
        !path.parent().unwrap().exists(),
        "private dir removed on drop"
    );
}

struct Deny;
#[async_trait]
impl SignConfirm for Deny {
    async fn confirm(&self, _fp: &str, _c: &str) -> bool {
        false
    }
}

#[tokio::test]
async fn confirmation_hook_can_deny() {
    let (_, ed) = gen(KeyGenAlgorithm::Ed25519, "ed");
    let service = AgentService::new(vec![AgentKey::new(ed, None, "ed").unwrap()])
        .with_confirm(Arc::new(Deny));
    let agent = start_agent(service, &AgentListenOptions::default())
        .await
        .unwrap();
    let mut client = os_agent::connect_agent(Some(agent.path())).await.unwrap();
    let ids = client.request_identities().await.unwrap();
    assert!(client
        .sign_request(&ids[0], None, b"x".to_vec())
        .await
        .is_err());
}

#[tokio::test]
async fn protocol_rejects_garbage() {
    let svc = AgentService::new(vec![]);
    assert_eq!(svc.handle(&[]).await, vec![5]);
    assert_eq!(svc.handle(&[13, 0, 0]).await, vec![5]);
    assert_eq!(svc.handle(&[27]).await, vec![5], "extensions unsupported");
    assert_eq!(svc.handle(&[11]).await, vec![12, 0, 0, 0, 0]);
    let (mut a, b) = tokio::io::duplex(64);
    let t = tokio::spawn(async move { svc.serve_connection(b).await });
    use tokio::io::AsyncWriteExt;
    a.write_all(&[0xff, 0xff, 0xff, 0xff]).await.unwrap();
    assert!(t.await.unwrap().is_err());
}

#[test]
fn encrypted_keys_are_rejected() {
    let g = keys::generate_key(
        KeyGenAlgorithm::Ed25519,
        "e",
        Some(&SecretString::from("pw")),
    )
    .unwrap();
    let k = cc_ssh_core::russh::keys::PrivateKey::from_openssh(g.private_openssh.expose_secret())
        .unwrap();
    assert!(AgentKey::new(k, None, "e").is_err());
}

fn hop(name: &str, cred: Option<Credential>) -> Hop {
    Hop {
        host_id: ObjectId::new(),
        name: name.into(),
        endpoint: Endpoint::new(format!("{name}.example"), 22),
        username: "u".into(),
        credential: cred,
        host_key_policy: HostKeyPolicy::AcceptNew,
        keepalive_secs: None,
    }
}

#[tokio::test]
async fn prepare_openssh_writes_no_secrets() {
    let res = MemoryCredentialResolver::new();
    let pass = SecretString::from("launch-pass");
    let enc = keys::generate_key(KeyGenAlgorithm::Ed25519, "enc", Some(&pass)).unwrap();
    let mut enc_cred = Credential::new("enc", CredentialKind::SshPrivateKey);
    enc_cred.key_encrypted = true;
    res.insert(
        enc_cred.id,
        ResolvedCredential::PrivateKey {
            openssh: enc.private_openssh.clone(),
            passphrase: None,
            certificate: None,
        },
    );
    res.set_prompt_passphrase(enc_cred.id, pass.clone());
    let pw_cred = Credential::new("pw", CredentialKind::Password);
    res.insert(
        pw_cred.id,
        ResolvedCredential::Password(SecretString::from("topsecret")),
    );

    let target = hop("target", Some(enc_cred.clone()));
    let plan = ConnectionPlan {
        host_id: target.host_id,
        name: target.name.clone(),
        target: target.endpoint.clone(),
        username: "u".into(),
        credential: target.credential.clone(),
        host_key_policy: HostKeyPolicy::AcceptNew,
        route: vec![hop("jump", Some(pw_cred)), hop("jump2", Some(enc_cred))],
        proxy: None,
        forwards: vec![],
        backend: SshBackend::OpenSsh,
        keepalive_secs: None,
        agent_forwarding: false,
        warnings: vec![],
    };
    let known = vec![new_known_host(
        "jump.example",
        22,
        "ssh-ed25519",
        b"blob",
        KnownHostSource::Tofu,
    )];
    let dir = tempfile::tempdir().unwrap();
    let client = OpenSshClient {
        path: "/usr/bin/ssh".into(),
        version: None,
    };
    let launch = prepare_openssh(
        &client,
        &plan,
        &res,
        &known,
        &LaunchOptions {
            remote_command: Some("hostname".into()),
            parent_dir: Some(dir.path().to_path_buf()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(launch.warnings.iter().any(|w| w.contains("password")));
    let infos = os_agent::list_agent_identities(Some(launch.agent_path()))
        .await
        .unwrap();
    assert_eq!(infos.len(), 1, "same credential deduplicated");

    let session = launch.session_dir().to_path_buf();
    let mut names: Vec<String> = std::fs::read_dir(&session)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    assert_eq!(names, vec!["agent.sock", "known_hosts", "ssh_config"]);
    for f in ["known_hosts", "ssh_config"] {
        let text = std::fs::read_to_string(session.join(f)).unwrap();
        assert!(!text.contains("PRIVATE KEY"), "{f}");
        assert!(
            !text.contains("topsecret") && !text.contains("launch-pass"),
            "{f}"
        );
    }
    let cfg = std::fs::read_to_string(session.join("ssh_config")).unwrap();
    assert!(cfg.contains(&format!("IdentityAgent {}", launch.agent_path().display())));
    assert!(cfg.contains("ProxyJump cc-hop-1"));
    assert!(launch.command.args.ends_with(&[
        "cc-target".to_string(),
        "--".to_string(),
        "hostname".to_string()
    ]));

    // TOFU additions made by OpenSSH are reported back.
    let mut kh = std::fs::read_to_string(session.join("known_hosts")).unwrap();
    kh.push_str("target.example ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKZ5\n");
    std::fs::write(session.join("known_hosts"), kh).unwrap();
    let learned = launch.learned_known_hosts();
    assert_eq!(learned.len(), 1);
    assert_eq!(learned[0].host_pattern, "target.example");

    drop(launch);
    assert!(!session.exists(), "session dir removed");
}
