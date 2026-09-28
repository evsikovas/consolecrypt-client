//! OpenSSH fallback launcher: resolves the plan's keys into a per-session
//! built-in agent and prepares the `ssh` invocation (config via `-F`,
//! `IdentityAgent` + `SSH_AUTH_SOCK` → built-in agent, exported known hosts).
//! No private key is ever written to disk; the private session directory
//! (0700) contains only the generated ssh_config, the known_hosts export
//! (public data) and the agent socket, and is removed on drop.

use crate::server::{start_agent, AgentHandle, AgentKey, AgentListenOptions, AgentService};
use crate::AgentError;
use cc_models::credential::{Credential, CredentialKind};
use cc_models::known_host::{KnownHost, KnownHostSource};
use cc_ssh_core::keys::{self, KeyError};
use cc_ssh_core::known_hosts::{export_known_hosts, import_known_hosts};
use cc_ssh_core::openssh::{
    build_openssh_command, write_private_file, OpenSshClient, OpenSshCommand, OpenSshOptions,
};
use cc_ssh_core::russh::keys::ssh_key::Certificate;
use cc_ssh_core::{ConnectionPlan, CredentialResolver, ResolvedCredential};
use secrecy::SecretString;
use std::path::{Path, PathBuf};

/// What to run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchOptions {
    pub remote_command: Option<String>,
    pub request_tty: Option<bool>,
    pub include_forwards: bool,
    /// Parent of the private session directory (default: OS temp dir).
    pub parent_dir: Option<PathBuf>,
}

/// A prepared OpenSSH session. Keep it alive while `ssh` runs.
pub struct OpenSshLaunch {
    pub command: OpenSshCommand,
    agent: AgentHandle,
    dir: PathBuf,
    known_hosts_path: PathBuf,
    initial_known_hosts: Vec<KnownHost>,
    /// Non-fatal notes (e.g. password hops will prompt on the TTY).
    pub warnings: Vec<String>,
}

impl std::fmt::Debug for OpenSshLaunch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenSshLaunch")
            .field("command", &self.command.program)
            .field("agent", &self.agent.path())
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

impl OpenSshLaunch {
    pub fn agent_path(&self) -> &Path {
        self.agent.path()
    }

    pub fn session_dir(&self) -> &Path {
        &self.dir
    }

    /// Host keys OpenSSH learned during the session (TOFU additions), to be
    /// stored in the vault's known hosts.
    pub fn learned_known_hosts(&self) -> Vec<KnownHost> {
        let text = std::fs::read_to_string(&self.known_hosts_path).unwrap_or_default();
        let (entries, _) = import_known_hosts(&text);
        entries
            .into_iter()
            .filter(|e| {
                !self
                    .initial_known_hosts
                    .iter()
                    .any(|k| k.host_pattern == e.host_pattern && k.public_key == e.public_key)
            })
            .map(|mut e| {
                e.source = KnownHostSource::Tofu;
                e
            })
            .collect()
    }
}

impl Drop for OpenSshLaunch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.dir.join("ssh_config"));
        let _ = std::fs::remove_file(&self.known_hosts_path);
        let _ = std::fs::remove_dir(&self.dir);
    }
}

async fn load_key(
    resolver: &dyn CredentialResolver,
    cred: &Credential,
    openssh: &SecretString,
    remembered: Option<SecretString>,
) -> Result<cc_ssh_core::russh::keys::PrivateKey, AgentError> {
    match keys::load_private_key(openssh, remembered.as_ref()) {
        Ok(k) => return Ok(k),
        Err(KeyError::PassphraseRequired | KeyError::WrongPassphrase) => {}
        Err(e) => return Err(AgentError::Key(e.to_string())),
    }
    for attempt in 0..3 {
        let Some(p) = resolver
            .prompt_passphrase(cred, attempt)
            .await
            .map_err(|e| AgentError::Credential(e.to_string()))?
        else {
            break;
        };
        match keys::load_private_key(openssh, Some(&p)) {
            Ok(k) => return Ok(k),
            Err(KeyError::WrongPassphrase) => continue,
            Err(e) => return Err(AgentError::Key(e.to_string())),
        }
    }
    Err(AgentError::Key(format!(
        "passphrase required for credential '{}'",
        cred.name
    )))
}

/// Collect every hop's key into agent keys.
pub async fn collect_agent_keys(
    plan: &ConnectionPlan,
    resolver: &dyn CredentialResolver,
) -> Result<(Vec<AgentKey>, Vec<String>), AgentError> {
    let mut keys_out: Vec<AgentKey> = Vec::new();
    let mut warnings = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for hop in plan.all_hops() {
        let Some(cred) = &hop.credential else {
            warnings.push(format!(
                "{}: no credential; OpenSSH uses its defaults",
                hop.name
            ));
            continue;
        };
        if !seen.insert(cred.id) {
            continue;
        }
        match cred.kind {
            CredentialKind::Password => {
                warnings.push(format!(
                    "{}: password authentication — OpenSSH will prompt on the terminal",
                    hop.name
                ));
                continue;
            }
            CredentialKind::OsSshAgent | CredentialKind::ExternalAgent | CredentialKind::Fido2 => {
                warnings.push(format!(
                    "{}: {:?} credentials are not served by the built-in agent",
                    hop.name, cred.kind
                ));
                continue;
            }
            CredentialKind::SshPrivateKey | CredentialKind::SshCertificate => {}
        }
        let resolved = resolver
            .resolve(cred)
            .await
            .map_err(|e| AgentError::Credential(e.to_string()))?;
        let ResolvedCredential::PrivateKey {
            openssh,
            passphrase,
            certificate,
        } = resolved
        else {
            warnings.push(format!(
                "{}: credential did not resolve to a private key",
                hop.name
            ));
            continue;
        };
        let key = load_key(resolver, cred, &openssh, passphrase).await?;
        let cert = match certificate.or_else(|| cred.certificate.clone()) {
            Some(line) => Some(
                Certificate::from_openssh(line.trim())
                    .map_err(|e| AgentError::Key(format!("invalid certificate: {e}")))?,
            ),
            None => None,
        };
        keys_out.push(AgentKey::new(key, cert, cred.name.clone())?);
    }
    Ok((keys_out, warnings))
}

/// Prepare an OpenSSH session for `plan`.
pub async fn prepare_openssh(
    client: &OpenSshClient,
    plan: &ConnectionPlan,
    resolver: &dyn CredentialResolver,
    known_hosts: &[KnownHost],
    opts: &LaunchOptions,
) -> Result<OpenSshLaunch, AgentError> {
    let (keys, warnings) = collect_agent_keys(plan, resolver).await?;
    let agent = start_agent(
        AgentService::new(keys),
        &AgentListenOptions {
            parent_dir: opts.parent_dir.clone(),
        },
    )
    .await?;
    // The agent's private directory doubles as the session directory
    // (Unix). On Windows the pipe has no directory: use a private temp dir.
    let dir = match agent.path().parent() {
        Some(p) if cfg!(unix) => p.to_path_buf(),
        _ => {
            let d = opts
                .parent_dir
                .clone()
                .unwrap_or_else(std::env::temp_dir)
                .join(format!(
                    "cc-ssh-{}",
                    &uuid::Uuid::new_v4().simple().to_string()[..12]
                ));
            std::fs::create_dir_all(&d)?;
            d
        }
    };
    let known_hosts_path = dir.join("known_hosts");
    write_private_file(&known_hosts_path, &export_known_hosts(known_hosts))?;
    let config_path = dir.join("ssh_config");
    let mut o = OpenSshOptions::new(agent.path(), &config_path);
    o.known_hosts_file = Some(known_hosts_path.clone());
    o.remote_command = opts.remote_command.clone();
    o.request_tty = opts.request_tty;
    o.include_forwards = opts.include_forwards;
    let command = build_openssh_command(client, plan, &o)
        .map_err(|e| AgentError::Unsupported(e.to_string()))?;
    write_private_file(&config_path, &command.config)?;
    Ok(OpenSshLaunch {
        command,
        agent,
        dir,
        known_hosts_path,
        initial_known_hosts: known_hosts.to_vec(),
        warnings,
    })
}
