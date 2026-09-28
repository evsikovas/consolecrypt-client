//! Test harness (feature `test-harness`): a Docker SSH testbed shared by the
//! integration tests of ssh-core, tunnel-core and sftp-core.
//!
//! Topology (target reachable **only** through the chain):
//!
//! ```text
//!  test process ──127.0.0.1:<mapped>──► bastion1 ─(net A)─► bastion2 ─(net B, internal)─► target
//! ```
//!
//! All secrets (user password, client keys, user/host CA keys) are generated
//! at runtime and passed via environment variables of the `docker` process
//! (never on its command line). Containers and networks are removed on drop.
//! Enable with `CC_SSH_IT=1`.

use crate::keys::{self, CertificateKind, GeneratedKey, KeyGenAlgorithm};
use crate::memory::{
    FixedHostKeyPrompt, MemoryCredentialResolver, MemoryInventory, MemoryKnownHosts,
};
use crate::traits::{HostKeyDecision, ResolvedCredential};
use crate::SshConnector;
use cc_models::credential::{Credential, CredentialKind};
use cc_models::host::{Host, HostKeyPolicy};
use cc_models::ObjectId;
use secrecy::{ExposeSecret, SecretString};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

/// Image tag built from `ssh-core/tests/docker`.
pub const IMAGE: &str = "cc-ssh-it:latest";

/// `true` when Docker integration tests are enabled (`CC_SSH_IT=1`).
pub fn enabled() -> bool {
    std::env::var("CC_SSH_IT").is_ok_and(|v| v == "1")
}

fn docker(args: &[&str]) -> Result<String, String> {
    docker_env(args, &[])
}

fn docker_env(args: &[&str], env: &[(&str, &str)]) -> Result<String, String> {
    let mut c = Command::new("docker");
    c.args(args).stdin(Stdio::null());
    for (k, v) in env {
        c.env(k, v);
    }
    let out = c
        .output()
        .map_err(|e| format!("docker not available: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(format!(
            "docker {} failed: {}",
            args.first().copied().unwrap_or(""),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Build the test image once per process.
pub fn build_image() -> Result<(), String> {
    static BUILT: OnceLock<Result<(), String>> = OnceLock::new();
    BUILT
        .get_or_init(|| {
            let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("docker");
            docker(&["build", "-q", "-t", IMAGE, &dir.to_string_lossy()]).map(|_| ())
        })
        .clone()
}

/// Runtime-generated credentials of the testbed.
pub struct TestSecrets {
    pub user: String,
    pub password: SecretString,
    /// Ed25519 key listed in `authorized_keys`.
    pub key: GeneratedKey,
    /// Passphrase-protected Ed25519 key listed in `authorized_keys`.
    pub encrypted_key: GeneratedKey,
    pub encrypted_passphrase: SecretString,
    /// Key NOT in `authorized_keys`; accepted only via `cert_line`.
    pub cert_key: GeneratedKey,
    /// User certificate for `cert_key` signed by the user CA (principal `tester`).
    pub cert_line: String,
    /// Host CA public key line (`@cert-authority` tests).
    pub host_ca_public: String,
    /// Host CA private key (test-only; signs the containers' host keys).
    pub host_ca_private: SecretString,
    /// User CA public key (`TrustedUserCAKeys`).
    pub user_ca_public: String,
}

impl std::fmt::Debug for TestSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestSecrets")
            .field("user", &self.user)
            .finish_non_exhaustive()
    }
}

impl TestSecrets {
    pub fn generate() -> Self {
        let key = keys::generate_key(KeyGenAlgorithm::Ed25519, "it-key", None).expect("keygen");
        let encrypted_passphrase = SecretString::from(format!("pp-{}", uuid::Uuid::new_v4()));
        let encrypted_key = keys::generate_key(
            KeyGenAlgorithm::Ed25519,
            "it-enc",
            Some(&encrypted_passphrase),
        )
        .expect("keygen");
        let user_ca =
            keys::generate_key(KeyGenAlgorithm::Ed25519, "user-ca", None).expect("keygen");
        let cert_key =
            keys::generate_key(KeyGenAlgorithm::Ed25519, "it-cert", None).expect("keygen");
        let ca_priv = keys::load_private_key(&user_ca.private_openssh, None).expect("ca");
        let subject = keys::load_private_key(&cert_key.private_openssh, None).expect("subject");
        let cert_line = keys::sign_certificate(
            &ca_priv,
            subject.public_key(),
            CertificateKind::User,
            "it-cert",
            &["tester"],
            0,
            u64::MAX,
            true,
        )
        .expect("sign");
        let host_ca =
            keys::generate_key(KeyGenAlgorithm::Ed25519, "host-ca", None).expect("keygen");
        Self {
            user: "tester".into(),
            password: SecretString::from(format!("pw-{}", uuid::Uuid::new_v4())),
            key,
            encrypted_key,
            encrypted_passphrase,
            cert_key,
            cert_line,
            host_ca_public: host_ca.public_openssh.clone(),
            host_ca_private: host_ca.private_openssh.clone(),
            user_ca_public: user_ca.public_openssh.clone(),
        }
    }
}

/// Inventory objects describing the testbed.
#[derive(Debug, Clone)]
pub struct TestInventory {
    pub inventory: Arc<MemoryInventory>,
    pub resolver: Arc<MemoryCredentialResolver>,
    pub bastion1: ObjectId,
    pub bastion2: Option<ObjectId>,
    pub target: Option<ObjectId>,
    pub cred_key: ObjectId,
    pub cred_password: ObjectId,
    pub cred_encrypted: ObjectId,
    pub cred_cert: ObjectId,
}

/// Running Docker testbed. Removed on drop.
pub struct SshTestbed {
    pub id: String,
    pub secrets: TestSecrets,
    pub bastion1: String,
    pub bastion1_port: u16,
    pub bastion2: Option<String>,
    pub target: Option<String>,
    containers: Vec<String>,
    networks: Vec<String>,
}

impl std::fmt::Debug for SshTestbed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshTestbed")
            .field("id", &self.id)
            .field("bastion1_port", &self.bastion1_port)
            .field("containers", &self.containers)
            .finish_non_exhaustive()
    }
}

impl Drop for SshTestbed {
    fn drop(&mut self) {
        for c in &self.containers {
            let _ = docker(&["rm", "-f", c]);
        }
        for n in &self.networks {
            let _ = docker(&["network", "rm", n]);
        }
    }
}

impl SshTestbed {
    /// Start the testbed. `chain = false` starts only bastion1 (for tests
    /// that mutate server state, e.g. host key rotation).
    pub fn start(chain: bool) -> Result<Self, String> {
        build_image()?;
        let id = uuid::Uuid::new_v4().simple().to_string()[..10].to_string();
        let secrets = TestSecrets::generate();
        let mut tb = SshTestbed {
            bastion1: format!("cc-it-{id}-bastion1"),
            bastion1_port: 0,
            bastion2: None,
            target: None,
            containers: Vec::new(),
            networks: Vec::new(),
            id,
            secrets,
        };
        let net_a = format!("cc-it-{}-a", tb.id);
        let net_b = format!("cc-it-{}-b", tb.id);
        docker(&["network", "create", &net_a])?;
        tb.networks.push(net_a.clone());
        if chain {
            docker(&["network", "create", "--internal", &net_b])?;
            tb.networks.push(net_b.clone());
        }
        let b1 = tb.bastion1.clone();
        tb.run_container(&b1, &net_a, true, &format!("127.0.0.1,localhost,{b1}"))?;
        let port = docker(&["port", &b1, "22/tcp"])?;
        tb.bastion1_port = port
            .lines()
            .find_map(|l| l.rsplit(':').next()?.trim().parse().ok())
            .ok_or_else(|| format!("cannot parse mapped port from {port:?}"))?;
        if chain {
            let b2 = format!("cc-it-{}-bastion2", tb.id);
            tb.run_container(&b2, &net_a, false, &b2)?;
            docker(&["network", "connect", &net_b, &b2])?;
            let t = format!("cc-it-{}-target", tb.id);
            tb.run_container(&t, &net_b, false, &t)?;
            tb.bastion2 = Some(b2);
            tb.target = Some(t);
        }
        for c in tb.containers.clone() {
            tb.wait_ready(&c)?;
        }
        Ok(tb)
    }

    fn run_container(
        &mut self,
        name: &str,
        network: &str,
        publish: bool,
        principals: &str,
    ) -> Result<(), String> {
        let authorized = format!(
            "{}\n{}",
            self.secrets.key.public_openssh, self.secrets.encrypted_key.public_openssh
        );
        let user_ca = self.secrets.user_ca_public.clone();
        let host_ca_text = self.secrets.host_ca_private.expose_secret().to_string();
        let mut args = vec![
            "run",
            "-d",
            "--name",
            name,
            "--hostname",
            name,
            "--network",
            network,
            "-e",
            "CC_IT_PASSWORD",
            "-e",
            "CC_IT_AUTHORIZED_KEYS",
            "-e",
            "CC_IT_USER_CA",
            "-e",
            "CC_IT_HOST_CA_KEY",
            "-e",
            "CC_IT_HOST_PRINCIPALS",
        ];
        if publish {
            args.extend(["-p", "127.0.0.1::22"]);
        }
        args.push(IMAGE);
        docker_env(
            &args,
            &[
                ("CC_IT_PASSWORD", self.secrets.password.expose_secret()),
                ("CC_IT_AUTHORIZED_KEYS", &authorized),
                ("CC_IT_USER_CA", &user_ca),
                ("CC_IT_HOST_CA_KEY", &host_ca_text),
                ("CC_IT_HOST_PRINCIPALS", principals),
            ],
        )?;
        self.containers.push(name.to_string());
        Ok(())
    }

    fn wait_ready(&self, container: &str) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let logs = Command::new("docker")
                .args(["logs", container])
                .output()
                .map(|o| {
                    let mut s = String::from_utf8_lossy(&o.stdout).to_string();
                    s.push_str(&String::from_utf8_lossy(&o.stderr));
                    s
                })
                .unwrap_or_default();
            if logs.contains("Server listening on") {
                return Ok(());
            }
            if Instant::now() > deadline {
                return Err(format!("{container} did not start: {logs}"));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Run a shell command inside a container.
    pub fn exec_in(&self, container: &str, script: &str) -> Result<String, String> {
        docker(&["exec", container, "sh", "-c", script])
    }

    /// Replace the host keys of a container (simulates a changed host key)
    /// and make sshd reload them.
    pub fn rotate_host_keys(&self, container: &str) -> Result<(), String> {
        self.exec_in(
            container,
            "rm -f /etc/ssh/ssh_host_* /etc/ssh/sshd_config.d/hostcert.conf && \
             ssh-keygen -q -t ed25519 -N '' -f /etc/ssh/ssh_host_ed25519_key && \
             ssh-keygen -q -t rsa -b 3072 -N '' -f /etc/ssh/ssh_host_rsa_key && kill -HUP 1",
        )?;
        std::thread::sleep(Duration::from_millis(800));
        Ok(())
    }

    /// Inventory with hosts bastion1 (→ bastion2 → target when chained) and
    /// the four credentials. Hosts use `AcceptNew` and the plain key.
    pub fn inventory(&self) -> TestInventory {
        let inv = Arc::new(MemoryInventory::new());
        let res = Arc::new(MemoryCredentialResolver::new());
        let s = &self.secrets;

        let mut key = Credential::new("it-key", CredentialKind::SshPrivateKey);
        key.username = Some(s.user.clone());
        let cred_key = inv.add_credential(key);
        res.insert(
            cred_key,
            ResolvedCredential::PrivateKey {
                openssh: s.key.private_openssh.clone(),
                passphrase: None,
                certificate: None,
            },
        );
        let cred_password =
            inv.add_credential(Credential::new("it-password", CredentialKind::Password));
        res.insert(
            cred_password,
            ResolvedCredential::Password(s.password.clone()),
        );
        let mut enc = Credential::new("it-encrypted", CredentialKind::SshPrivateKey);
        enc.key_encrypted = true;
        let cred_encrypted = inv.add_credential(enc);
        res.insert(
            cred_encrypted,
            ResolvedCredential::PrivateKey {
                openssh: s.encrypted_key.private_openssh.clone(),
                passphrase: None,
                certificate: None,
            },
        );
        let mut cert = Credential::new("it-cert", CredentialKind::SshCertificate);
        cert.certificate = Some(s.cert_line.clone());
        let cred_cert = inv.add_credential(cert);
        res.insert(
            cred_cert,
            ResolvedCredential::PrivateKey {
                openssh: s.cert_key.private_openssh.clone(),
                passphrase: None,
                certificate: None,
            },
        );

        let host = |name: &str, addr: &str, port: u16| {
            let mut h = Host::new(name, addr);
            h.port = Some(port);
            h.username = Some(s.user.clone());
            h.credential_id = Some(cred_key);
            h.host_key_policy = HostKeyPolicy::AcceptNew;
            h
        };
        let b1 = inv.add_host(host("bastion1", "127.0.0.1", self.bastion1_port));
        let (b2, t) = match (&self.bastion2, &self.target) {
            (Some(b2n), Some(tn)) => {
                let b2 = inv.add_host(host("bastion2", b2n, 22));
                let mut th = host("target", tn, 22);
                th.jump_chain = vec![b1, b2];
                let t = inv.add_host(th);
                (Some(b2), Some(t))
            }
            _ => (None, None),
        };
        TestInventory {
            inventory: inv,
            resolver: res,
            bastion1: b1,
            bastion2: b2,
            target: t,
            cred_key,
            cred_password,
            cred_encrypted,
            cred_cert,
        }
    }
}

/// Connector with an in-memory known-hosts store and a fixed prompt answer.
pub fn connector(
    resolver: Arc<MemoryCredentialResolver>,
    known_hosts: Arc<MemoryKnownHosts>,
    decision: HostKeyDecision,
) -> (SshConnector, FixedHostKeyPrompt) {
    let prompt = FixedHostKeyPrompt::new(decision);
    (
        SshConnector::new(resolver, known_hosts, Arc::new(prompt.clone())),
        prompt,
    )
}
