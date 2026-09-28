//! SSH wiring (CLIENT_ARCHITECTURE §3): ssh-core's consumer-side traits
//! implemented over the unlocked vault, plus the per-session runtime
//! (terminals, tunnels, SFTP, shared connections, OpenSSH fallback).
//!
//! * [`AppInventory`] — `InventoryLookup` over the decrypted working set
//!   (plaintext inventory only, no secrets).
//! * [`VaultCredentialResolver`] — the ONLY path from Secret objects to
//!   `ResolvedCredential`; secrets are decrypted on demand, never cached.
//! * [`VaultKnownHosts`] — `KnownHostsStore` persisting `KnownHost` vault
//!   objects, so trust decisions sync across devices.
//! * [`BrokerHostKeyPrompt`] — `HostKeyPrompt` bridged to the UI.

use crate::dto::{ExecResultDto, RemoteEntryDto};
use crate::error::{AppError, AppResult};
use crate::prompts::PromptBroker;
use crate::working_set::WorkingSet;
use crate::writer::VaultWriter;
use async_trait::async_trait;
use cc_models::credential::{Credential, CredentialKind};
use cc_models::group::Group;
use cc_models::host::{Host, JumpProfile, Proxy, SshBackend};
use cc_models::known_host::KnownHost;
use cc_models::tunnel::Tunnel;
use cc_models::{ObjectId, VaultObject};
use cc_sftp_core::SftpClient;
use cc_ssh_agent_core::{prepare_openssh, LaunchOptions, OpenSshLaunch};
use cc_ssh_core::known_hosts::pattern_matches;
use cc_ssh_core::openssh::detect_openssh;
use cc_ssh_core::{
    ConnectionPlan, ConnectionPlanner, CredentialResolver, HostKeyDecision, HostKeyInfo,
    HostKeyPrompt, InventoryLookup, KnownHostsError, KnownHostsStore, PlannerDefaults, PtyRequest,
    ResolveError, ResolvedCredential, ShellChannel, ShellEvent, ShellInput, SshConnector,
    SshSession,
};
use cc_terminal_core::{OpenedShell, ShellOpener, SshShellOpener, TerminalError, TerminalManager};
use cc_tunnel_core::TunnelManager;
use secrecy::SecretString;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// ---- InventoryLookup -------------------------------------------------------------

/// Plaintext inventory for the planner (no secrets).
#[derive(Debug)]
pub(crate) struct AppInventory {
    working: Arc<WorkingSet>,
}

#[async_trait]
impl InventoryLookup for AppInventory {
    async fn host(&self, id: ObjectId) -> Option<Host> {
        let mut h = self.working.host(id)?;
        if crate::host_auth::prompts_for_password(&h) {
            // Ask at connect; also stops group-credential inheritance.
            h.credential_id = Some(crate::host_auth::prompt_credential_id(h.id));
        }
        Some(h)
    }
    async fn group(&self, id: ObjectId) -> Option<Group> {
        self.working.group(id)
    }
    async fn jump_profile(&self, id: ObjectId) -> Option<JumpProfile> {
        self.working.jump_profile(id)
    }
    async fn proxy(&self, id: ObjectId) -> Option<Proxy> {
        self.working.proxy(id)
    }
    async fn credential(&self, id: ObjectId) -> Option<Credential> {
        if let Some(c) = self.working.credential(id) {
            return Some(c);
        }
        let host = self.working.host(crate::host_auth::prompt_host_of(id))?;
        crate::host_auth::prompts_for_password(&host)
            .then(|| crate::host_auth::prompt_credential(&host))
    }
    async fn tunnels_for_host(&self, host_id: ObjectId) -> Vec<Tunnel> {
        self.working
            .tunnels()
            .into_iter()
            .filter(|t| t.host_id == host_id)
            .collect()
    }
}

// ---- CredentialResolver ----------------------------------------------------------

/// Resolves credentials from Secret objects of the unlocked vault.
pub(crate) struct VaultCredentialResolver {
    writer: VaultWriter,
    prompts: Arc<PromptBroker>,
}

impl std::fmt::Debug for VaultCredentialResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VaultCredentialResolver")
    }
}

fn resolve_err(id: ObjectId, e: AppError) -> ResolveError {
    match e {
        AppError::VaultLocked => ResolveError::Locked,
        AppError::NotFound { .. } => ResolveError::SecretNotFound(id),
        other => ResolveError::Other(other.to_string()),
    }
}

impl VaultCredentialResolver {
    async fn secret(&self, id: ObjectId) -> Result<SecretString, ResolveError> {
        let s = self
            .writer
            .read_secret(id)
            .await
            .map_err(|e| resolve_err(id, e))?;
        Ok(SecretString::from(s.value.expose_secret()))
    }
}

#[async_trait]
impl CredentialResolver for VaultCredentialResolver {
    async fn resolve(&self, credential: &Credential) -> Result<ResolvedCredential, ResolveError> {
        match credential.kind {
            CredentialKind::Password => match credential.secret_id {
                Some(id) => Ok(ResolvedCredential::Password(self.secret(id).await?)),
                None => {
                    // Auth mode `PasswordPrompt`: ask the UI, use once.
                    let host = self
                        .writer
                        .working
                        .host(crate::host_auth::prompt_host_of(credential.id));
                    let (host_id, host_name) = match &host {
                        Some(h) => (Some(h.id.to_string()), h.name.clone()),
                        None => (None, credential.name.clone()),
                    };
                    self.prompts
                        .password(host_id, host_name)
                        .await
                        .map(ResolvedCredential::Password)
                        .ok_or(ResolveError::Cancelled)
                }
            },
            CredentialKind::SshPrivateKey | CredentialKind::SshCertificate => {
                let id = credential
                    .secret_id
                    .ok_or(ResolveError::NoSecret(credential.id))?;
                let openssh = self.secret(id).await?;
                let passphrase = match credential.passphrase_secret_id {
                    Some(p) => Some(self.secret(p).await?),
                    None => None,
                };
                Ok(ResolvedCredential::PrivateKey {
                    openssh,
                    passphrase,
                    certificate: credential.certificate.clone(),
                })
            }
            CredentialKind::OsSshAgent | CredentialKind::ExternalAgent => {
                ResolvedCredential::agent_for(credential)
                    .ok_or(ResolveError::Unsupported(credential.kind))
            }
            CredentialKind::Fido2 => Err(ResolveError::Unsupported(credential.kind)),
        }
    }

    async fn prompt_passphrase(
        &self,
        credential: &Credential,
        attempt: u32,
    ) -> Result<Option<SecretString>, ResolveError> {
        Ok(self
            .prompts
            .passphrase(credential.id.to_string(), credential.name.clone(), attempt)
            .await)
    }

    async fn resolve_proxy_password(
        &self,
        proxy: &Proxy,
    ) -> Result<Option<SecretString>, ResolveError> {
        match proxy.password_secret_id {
            Some(id) => self.secret(id).await.map(Some),
            None => Ok(None),
        }
    }
}

// ---- KnownHostsStore -------------------------------------------------------------

/// Known hosts as synced vault objects.
#[derive(Debug)]
pub(crate) struct VaultKnownHosts {
    writer: VaultWriter,
}

fn kh_err(e: AppError) -> KnownHostsError {
    KnownHostsError(e.to_string())
}

#[async_trait]
impl KnownHostsStore for VaultKnownHosts {
    async fn lookup(&self, host: &str, port: u16) -> Result<Vec<KnownHost>, KnownHostsError> {
        Ok(self
            .writer
            .working
            .known_hosts()
            .into_iter()
            .filter(|e| pattern_matches(&e.host_pattern, host, port))
            .collect())
    }

    async fn add(&self, entry: KnownHost) -> Result<(), KnownHostsError> {
        let dup = self.writer.working.known_hosts().into_iter().any(|e| {
            e.host_pattern == entry.host_pattern
                && e.public_key == entry.public_key
                && e.is_revoked() == entry.is_revoked()
                && e.is_cert_authority() == entry.is_cert_authority()
        });
        if dup {
            return Ok(());
        }
        tracing::info!(
            host = %entry.host_pattern,
            fingerprint = %entry.fingerprint_sha256,
            "known host recorded"
        );
        self.writer
            .put(VaultObject::KnownHost(entry))
            .await
            .map_err(kh_err)
    }

    async fn revoke(&self, fingerprint_sha256: &str) -> Result<(), KnownHostsError> {
        for mut e in self.writer.working.known_hosts() {
            if e.fingerprint_sha256 == fingerprint_sha256 && !e.revoked {
                e.revoked = true;
                e.updated_at = chrono::Utc::now();
                self.writer
                    .put(VaultObject::KnownHost(e))
                    .await
                    .map_err(kh_err)?;
            }
        }
        Ok(())
    }

    async fn remove(&self, id: ObjectId) -> Result<(), KnownHostsError> {
        self.writer.delete(id).await.map_err(kh_err)
    }
}

// ---- HostKeyPrompt ---------------------------------------------------------------

/// Unknown host keys are asked through the UI prompt broker.
#[derive(Debug)]
pub(crate) struct BrokerHostKeyPrompt {
    prompts: Arc<PromptBroker>,
}

#[async_trait]
impl HostKeyPrompt for BrokerHostKeyPrompt {
    async fn confirm_unknown(&self, info: HostKeyInfo) -> HostKeyDecision {
        self.prompts.host_key(info).await
    }
}

// ---- terminal opener (native / OpenSSH fallback) ---------------------------------

/// Opens terminals with the native backend, or with system OpenSSH when the
/// host asks for it (`backend = OpenSsh`); keys reach OpenSSH only through
/// the built-in agent (ssh-agent-core), never a file.
struct RoutingShellOpener {
    native: SshShellOpener,
    resolver: Arc<VaultCredentialResolver>,
    known_hosts: Arc<VaultKnownHosts>,
}

#[async_trait]
impl ShellOpener for RoutingShellOpener {
    async fn open_shell(
        &self,
        plan: &ConnectionPlan,
        pty: PtyRequest,
    ) -> Result<OpenedShell, TerminalError> {
        match plan.backend {
            SshBackend::Native => self.native.open_shell(plan, pty).await,
            SshBackend::OpenSsh => open_openssh_shell(plan, &self.resolver, &self.known_hosts)
                .await
                .map_err(|e| TerminalError::Open(e.to_string())),
        }
    }
}

async fn launch_openssh(
    plan: &ConnectionPlan,
    resolver: &VaultCredentialResolver,
    known_hosts: &VaultKnownHosts,
    remote_command: Option<String>,
    tty: bool,
) -> AppResult<OpenSshLaunch> {
    let client = detect_openssh()
        .ok_or_else(|| AppError::Unsupported("no system OpenSSH client found".into()))?;
    let entries = known_hosts.writer.working.known_hosts();
    let launch = prepare_openssh(
        &client,
        plan,
        resolver,
        &entries,
        &LaunchOptions {
            remote_command,
            request_tty: Some(tty),
            include_forwards: false,
            parent_dir: None,
        },
    )
    .await?;
    for w in &launch.warnings {
        tracing::warn!("openssh fallback: {w}");
    }
    Ok(launch)
}

/// Record host keys OpenSSH learned (TOFU) as vault objects.
async fn persist_learned(launch: &OpenSshLaunch, known_hosts: &VaultKnownHosts) {
    for k in launch.learned_known_hosts() {
        if let Err(e) = known_hosts.add(k).await {
            tracing::warn!(error = %e, "could not store a host key learned by OpenSSH");
        }
    }
}

/// OpenSSH with `-tt` over pipes: the remote side allocates the PTY; local
/// window-size changes cannot be propagated (no local PTY).
async fn open_openssh_shell(
    plan: &ConnectionPlan,
    resolver: &Arc<VaultCredentialResolver>,
    known_hosts: &Arc<VaultKnownHosts>,
) -> AppResult<OpenedShell> {
    let launch = launch_openssh(plan, resolver, known_hosts, None, true).await?;
    let mut cmd = tokio::process::Command::from(launch.command.to_command());
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take();
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::internal("ssh stdout"))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| AppError::internal("ssh stderr"))?;
    let (channel, mut remote) = ShellChannel::pair(256);
    let known_hosts = known_hosts.clone();
    tokio::spawn(async move {
        let events = remote.events.clone();
        let out = tokio::spawn({
            let events = events.clone();
            async move {
                let mut buf = vec![0u8; 16 * 1024];
                while let Ok(n) = stdout.read(&mut buf).await {
                    if n == 0
                        || events
                            .send(ShellEvent::Data(buf[..n].to_vec().into()))
                            .await
                            .is_err()
                    {
                        break;
                    }
                }
            }
        });
        let err = tokio::spawn({
            let events = events.clone();
            async move {
                let mut buf = vec![0u8; 8 * 1024];
                while let Ok(n) = stderr.read(&mut buf).await {
                    if n == 0
                        || events
                            .send(ShellEvent::Stderr(buf[..n].to_vec().into()))
                            .await
                            .is_err()
                    {
                        break;
                    }
                }
            }
        });
        let status = loop {
            tokio::select! {
                s = child.wait() => break s.ok(),
                input = remote.inputs.recv() => match input {
                    Some(ShellInput::Data(d)) => {
                        if let Some(w) = stdin.as_mut() {
                            if w.write_all(&d).await.is_err() {
                                stdin = None;
                            }
                        }
                    }
                    Some(ShellInput::Resize { .. }) => {}
                    Some(ShellInput::Eof) => stdin = None,
                    Some(ShellInput::Close) | None => {
                        let _ = child.start_kill();
                        stdin = None;
                    }
                },
            }
        };
        let _ = out.await;
        let _ = err.await;
        if let Some(code) = status.and_then(|s| s.code()) {
            let _ = events.send(ShellEvent::ExitStatus(code as u32)).await;
        }
        persist_learned(&launch, &known_hosts).await;
        let _ = events.send(ShellEvent::Closed).await;
        drop(launch);
    });
    Ok(OpenedShell {
        channel,
        session: None,
    })
}

// ---- runtime ---------------------------------------------------------------------

struct SftpEntry {
    client: Arc<SftpClient>,
    /// Host the session is connected to (edit sessions, transfers).
    host_id: ObjectId,
    /// Keeps the shared SSH connection alive (`None` for test clients).
    _session: Option<Arc<SshSession>>,
}

/// A prepared interactive OpenSSH session (CLI): run `program args` with
/// inherited stdio while this is alive, then finish it.
pub(crate) struct OpenSshPrepared {
    launch: OpenSshLaunch,
}

/// SSH runtime of an unlocked vault.
pub(crate) struct SshRuntime {
    pub resolver: Arc<VaultCredentialResolver>,
    pub known_hosts: Arc<VaultKnownHosts>,
    pub connector: SshConnector,
    pub planner: ConnectionPlanner,
    pub terminals: TerminalManager,
    pub tunnels: TunnelManager,
    pool: tokio::sync::Mutex<HashMap<ObjectId, Arc<SshSession>>>,
    sftp: tokio::sync::Mutex<HashMap<String, SftpEntry>>,
    openssh: std::sync::Mutex<HashMap<String, OpenSshPrepared>>,
}

impl std::fmt::Debug for SshRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshRuntime").finish_non_exhaustive()
    }
}

/// OS user name — the planner's last-resort username.
pub(crate) fn os_user() -> Option<String> {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .ok()
        .filter(|u| !u.trim().is_empty())
}

impl SshRuntime {
    pub(crate) fn new(writer: VaultWriter, prompts: Arc<PromptBroker>) -> Self {
        let working = writer.working.clone();
        let inventory = Arc::new(AppInventory {
            working: working.clone(),
        });
        let resolver = Arc::new(VaultCredentialResolver {
            writer: writer.clone(),
            prompts: prompts.clone(),
        });
        let known_hosts = Arc::new(VaultKnownHosts {
            writer: writer.clone(),
        });
        let prompt = Arc::new(BrokerHostKeyPrompt { prompts });
        let connector = SshConnector::new(resolver.clone(), known_hosts.clone(), prompt);
        let planner = ConnectionPlanner::new(inventory).with_defaults(PlannerDefaults {
            username: os_user(),
            ..PlannerDefaults::default()
        });
        let opener = Arc::new(RoutingShellOpener {
            native: SshShellOpener::new(connector.clone()),
            resolver: resolver.clone(),
            known_hosts: known_hosts.clone(),
        });
        Self {
            resolver,
            known_hosts,
            connector,
            planner,
            terminals: TerminalManager::new(opener),
            tunnels: TunnelManager::new(),
            pool: tokio::sync::Mutex::new(HashMap::new()),
            sftp: tokio::sync::Mutex::new(HashMap::new()),
            openssh: std::sync::Mutex::new(HashMap::new()),
        }
    }

    pub(crate) async fn plan(&self, host_id: ObjectId) -> AppResult<ConnectionPlan> {
        Ok(self.planner.plan(host_id).await?)
    }

    /// A fresh native connection to `host_id` (through its jump chain).
    pub(crate) async fn connect(&self, host_id: ObjectId) -> AppResult<SshSession> {
        let plan = self.plan(host_id).await?;
        tracing::info!(host_id = %host_id, route = %plan.describe(), "ssh connect");
        Ok(self.connector.connect(&plan).await?)
    }

    /// A shared native connection (tunnels, SFTP), reconnecting if closed.
    pub(crate) async fn shared_session(&self, host_id: ObjectId) -> AppResult<Arc<SshSession>> {
        let mut pool = self.pool.lock().await;
        if let Some(s) = pool.get(&host_id) {
            if !s.is_closed() {
                return Ok(s.clone());
            }
        }
        let s = Arc::new(self.connect(host_id).await?);
        pool.insert(host_id, s.clone());
        Ok(s)
    }

    /// Run a command (native, or OpenSSH for `backend = OpenSsh` hosts).
    pub(crate) async fn exec(&self, host_id: ObjectId, command: &str) -> AppResult<ExecResultDto> {
        let plan = self.plan(host_id).await?;
        match plan.backend {
            SshBackend::Native => {
                let session = self.connector.connect(&plan).await?;
                let out = session.exec(command).await;
                let _ = session.disconnect().await;
                let out = out?;
                Ok(ExecResultDto {
                    stdout: out.stdout,
                    stderr: out.stderr,
                    exit_status: out.exit_status,
                    exit_signal: out.exit_signal,
                })
            }
            SshBackend::OpenSsh => {
                let launch = launch_openssh(
                    &plan,
                    &self.resolver,
                    &self.known_hosts,
                    Some(command.to_owned()),
                    false,
                )
                .await?;
                let out = tokio::process::Command::from(launch.command.to_command())
                    .stdin(Stdio::null())
                    .output()
                    .await?;
                persist_learned(&launch, &self.known_hosts).await;
                Ok(ExecResultDto {
                    stdout: out.stdout,
                    stderr: out.stderr,
                    exit_status: out.status.code().map(|c| c as u32),
                    exit_signal: None,
                })
            }
        }
    }

    /// Prepare an interactive/exec OpenSSH invocation to be spawned by the
    /// caller with inherited stdio. Returns (id, program, args, env).
    pub(crate) async fn prepare_openssh(
        &self,
        host_id: ObjectId,
        command: Option<String>,
        tty: bool,
    ) -> AppResult<(String, String, Vec<String>, Vec<(String, String)>)> {
        let plan = self.plan(host_id).await?;
        let launch = launch_openssh(&plan, &self.resolver, &self.known_hosts, command, tty).await?;
        let id = uuid::Uuid::new_v4().to_string();
        let program = launch.command.program.to_string_lossy().to_string();
        let args = launch.command.args.clone();
        let env = launch.command.env.clone();
        self.openssh
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id.clone(), OpenSshPrepared { launch });
        Ok((id, program, args, env))
    }

    /// Persist learned host keys and tear down a prepared OpenSSH session.
    pub(crate) async fn finish_openssh(&self, id: &str) -> AppResult<()> {
        let prepared = self
            .openssh
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(id)
            .ok_or_else(|| AppError::not_found("openssh session", id))?;
        persist_learned(&prepared.launch, &self.known_hosts).await;
        Ok(())
    }

    // ---- sftp ------------------------------------------------------------------

    pub(crate) async fn sftp_open(&self, host_id: ObjectId) -> AppResult<String> {
        let session = self.shared_session(host_id).await?;
        let client = SftpClient::open(&session).await?;
        Ok(self.sftp_register(host_id, client, Some(session)).await)
    }

    /// Register an SFTP client under a new session id.
    pub(crate) async fn sftp_register(
        &self,
        host_id: ObjectId,
        client: SftpClient,
        session: Option<Arc<SshSession>>,
    ) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        self.sftp.lock().await.insert(
            id.clone(),
            SftpEntry {
                client: Arc::new(client),
                host_id,
                _session: session,
            },
        );
        id
    }

    /// Host of an SFTP session.
    pub(crate) async fn sftp_host(&self, id: &str) -> AppResult<ObjectId> {
        self.sftp
            .lock()
            .await
            .get(id)
            .map(|e| e.host_id)
            .ok_or_else(|| AppError::not_found("sftp session", id))
    }

    pub(crate) async fn sftp(&self, id: &str) -> AppResult<Arc<SftpClient>> {
        self.sftp
            .lock()
            .await
            .get(id)
            .map(|e| e.client.clone())
            .ok_or_else(|| AppError::not_found("sftp session", id))
    }

    pub(crate) async fn sftp_close(&self, id: &str) -> AppResult<()> {
        let e = self
            .sftp
            .lock()
            .await
            .remove(id)
            .ok_or_else(|| AppError::not_found("sftp session", id))?;
        let _ = e.client.close().await;
        Ok(())
    }

    pub(crate) async fn sftp_list(&self, id: &str, path: &str) -> AppResult<Vec<RemoteEntryDto>> {
        let c = self.sftp(id).await?;
        Ok(c.list(path)
            .await?
            .iter()
            .map(RemoteEntryDto::from)
            .collect())
    }

    /// Stop tunnels, close terminals, SFTP and pooled connections.
    pub(crate) async fn shutdown(&self) {
        self.tunnels.stop_all().await;
        for t in self.terminals.list() {
            let _ = self.terminals.close(t.id).await;
            let _ = self.terminals.remove(t.id).await;
        }
        let sftp: Vec<SftpEntry> = self.sftp.lock().await.drain().map(|(_, e)| e).collect();
        for e in sftp {
            let _ = e.client.close().await;
        }
        let pool: Vec<Arc<SshSession>> = self.pool.lock().await.drain().map(|(_, s)| s).collect();
        for s in pool {
            let _ = s.disconnect().await;
        }
        self.openssh
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }
}
