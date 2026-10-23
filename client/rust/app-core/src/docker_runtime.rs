//! Docker engines of the unlocked vault: this computer's socket or a saved SSH
//! host's `/var/run/docker.sock` (via `direct-streamlocal`, falling back to
//! `docker system dial-stdio`). Container shells reuse terminal sessions, and
//! port forwards tunnel through the same SSH session. Everything is dropped on
//! lock.
use crate::dto::TerminalInfoDto;
use crate::{AppCore, AppError, AppResult};
use async_trait::async_trait;
use cc_docker_core::{
    ContainerAction, ContainerSummary, DockerClient, DockerConnector, DockerError, DockerIo,
    EngineInfo, LogLine,
};
use cc_models::ObjectId;
use cc_ssh_core::{ShellChannel, ShellEvent, ShellInput, SshSession};
use cc_terminal_core::{OpenedShell, ShellLifetime, TerminalSize, TerminalTarget};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const MAX_SESSIONS: usize = 16;
const MAX_LOG_STREAMS: usize = 32;
const MAX_FORWARDS: usize = 64;
pub const LOCAL_HOST: &str = "local";
const AUTO_SHELL: &str = "/__consolecrypt_auto_shell__";

/// Finds the engine socket the SSH user can use and reports why not.
const SOCKET_PROBE: &str = "for s in /var/run/docker.sock /run/docker.sock \"/run/user/$(id -u)/docker.sock\"; do \
    if [ -S \"$s\" ]; then if [ -r \"$s\" ] && [ -w \"$s\" ]; then echo \"ok $s\"; else \
    echo \"denied $s $(stat -c %G \"$s\" 2>/dev/null || stat -f %Sg \"$s\" 2>/dev/null)\"; fi; exit 0; fi; done; echo missing";

#[derive(Debug)]
struct SshSocket {
    session: Arc<SshSession>,
    path: String,
}

#[async_trait]
impl DockerConnector for SshSocket {
    async fn connect(&self) -> Result<DockerIo, DockerError> {
        let stream = self
            .session
            .open_direct_streamlocal(&self.path)
            .await
            .map_err(|_| DockerError::Connection)?;
        Ok(Box::new(stream))
    }
}

/// `docker system dial-stdio` on the host: for servers that disable Unix
/// socket forwarding but have the Docker CLI.
#[derive(Debug)]
struct SshDialStdio {
    session: Arc<SshSession>,
}

#[async_trait]
impl DockerConnector for SshDialStdio {
    async fn connect(&self) -> Result<DockerIo, DockerError> {
        let channel = self
            .session
            .open_exec("docker system dial-stdio")
            .await
            .map_err(|_| DockerError::Connection)?;
        Ok(Box::new(channel.into_stream()))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DockerSessionDto {
    pub id: String,
    pub host_id: String,
    pub host_name: String,
    pub engine_version: String,
    pub os: String,
    pub arch: String,
    pub containers_running: u64,
    pub containers_total: u64,
    pub images: u64,
}

/// A connect result: the session, or why the engine is unreachable.
#[derive(Debug, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum DockerConnectOutcome {
    Connected(DockerSessionDto),
    Failed {
        failure: &'static str,
        detail: String,
    },
}

#[derive(Debug, Serialize)]
pub struct DockerHostDto {
    pub id: String,
    pub name: String,
    pub kind: &'static str,
    pub address: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DockerForwardDto {
    pub id: String,
    pub session_id: String,
    pub container_name: String,
    pub remote_port: u16,
    pub address: String,
    pub state: &'static str,
    pub active_connections: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub error_code: Option<&'static str>,
}

struct LiveDocker {
    info: DockerSessionDto,
    client: DockerClient,
    ssh: Option<Arc<SshSession>>,
    /// The engine socket of this computer (None over SSH).
    local_socket: Option<std::path::PathBuf>,
    host_id: Option<ObjectId>,
    cancel: CancellationToken,
}

/// What compose file access needs from a session.
pub(crate) struct ComposeTarget {
    pub client: DockerClient,
    pub ssh: Option<Arc<SshSession>>,
    pub local_socket: Option<std::path::PathBuf>,
    pub cancel: CancellationToken,
}

struct LogStream {
    lines: tokio::sync::Mutex<mpsc::Receiver<Vec<LogLine>>>,
    cancel: CancellationToken,
}

struct Forward {
    info: Mutex<DockerForwardDto>,
    active: AtomicU64,
    sent: AtomicU64,
    received: AtomicU64,
    cancel: CancellationToken,
}

impl Forward {
    fn snapshot(&self) -> DockerForwardDto {
        let mut info = self.info.lock().unwrap_or_else(|e| e.into_inner()).clone();
        info.active_connections = self.active.load(Ordering::Relaxed);
        info.bytes_sent = self.sent.load(Ordering::Relaxed);
        info.bytes_received = self.received.load(Ordering::Relaxed);
        info
    }
}

#[derive(Default)]
pub(crate) struct DockerRuntime {
    shutdown: CancellationToken,
    sessions: Mutex<HashMap<String, Arc<LiveDocker>>>,
    logs: Mutex<HashMap<String, Arc<LogStream>>>,
    forwards: Mutex<HashMap<String, Arc<Forward>>>,
    pub(crate) compose: crate::docker_compose::ComposeState,
}

impl DockerRuntime {
    pub(crate) fn close(&self) {
        self.shutdown.cancel();
        self.compose.close();
        self.logs.lock().unwrap_or_else(|e| e.into_inner()).clear();
        self.forwards
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        for (_, live) in self
            .sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain()
        {
            live.cancel.cancel();
        }
    }

    /// Live sessions to an engine (None: this computer).
    pub(crate) fn session_ids(&self, host: Option<ObjectId>) -> Vec<String> {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(_, live)| live.host_id == host && !live.cancel.is_cancelled())
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// The address of a running forward of one of `sessions` to a port of
    /// the named container.
    pub(crate) fn running_forward(
        &self,
        sessions: &[String],
        container: &str,
        remote_port: u16,
    ) -> Option<String> {
        self.forwards
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .filter(|forward| !forward.cancel.is_cancelled())
            .map(|forward| forward.snapshot())
            .find(|info| {
                info.state == "running"
                    && sessions.contains(&info.session_id)
                    && info.container_name == container
                    && info.remote_port == remote_port
            })
            .map(|info| info.address)
    }

    pub(crate) fn compose_target(&self, id: &str) -> AppResult<ComposeTarget> {
        let live = self.session(id)?;
        Ok(ComposeTarget {
            client: live.client.clone(),
            ssh: live.ssh.clone(),
            local_socket: live.local_socket.clone(),
            cancel: live.cancel.clone(),
        })
    }

    fn session(&self, id: &str) -> AppResult<Arc<LiveDocker>> {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
            .ok_or_else(|| invalid("session_closed"))
    }
}

fn invalid(reason: &'static str) -> AppError {
    AppError::invalid("docker", reason)
}

fn docker_error(error: DockerError) -> AppError {
    invalid(error.code())
}

struct ShellLife(CancellationToken);
impl ShellLifetime for ShellLife {
    fn close(&self) {
        self.0.cancel();
    }
}
impl Drop for ShellLife {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// Whether `text` holds something the Docker views would mask (copied log
/// lines go to the clipboard as a secret then).
pub fn docker_contains_secret(text: &str) -> bool {
    cc_docker_core::contains_secret(text)
}

fn session_dto(id: String, host_id: &str, host_name: String, info: EngineInfo) -> DockerSessionDto {
    DockerSessionDto {
        id,
        host_id: host_id.to_owned(),
        host_name,
        engine_version: info.engine_version,
        os: info.os,
        arch: info.arch,
        containers_running: info.containers_running,
        containers_total: info.containers_total,
        images: info.images,
    }
}

impl AppCore {
    /// This computer (when it has an engine socket) and the saved SSH hosts.
    pub async fn docker_hosts(&self) -> AppResult<Vec<DockerHostDto>> {
        let (_, unlocked) = self.unlocked().await?;
        let mut hosts = Vec::new();
        #[cfg(unix)]
        if !cfg!(any(target_os = "android", target_os = "ios"))
            && cc_docker_core::local_socket().is_some()
        {
            hosts.push(DockerHostDto {
                id: LOCAL_HOST.into(),
                name: String::new(),
                kind: "local",
                address: String::new(),
            });
        }
        let mut saved: Vec<_> = unlocked.working().hosts().to_vec();
        saved.sort_by_key(|host| host.name.to_lowercase());
        hosts.extend(saved.into_iter().map(|host| DockerHostDto {
            id: host.id.to_string(),
            name: host.name,
            kind: "ssh",
            address: host.address,
        }));
        Ok(hosts)
    }

    pub async fn docker_connect(&self, host_id: String) -> AppResult<DockerConnectOutcome> {
        let (_, unlocked) = self.unlocked().await?;
        if unlocked
            .docker
            .sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
            >= MAX_SESSIONS
        {
            return Err(invalid("too_many_sessions"));
        }
        let cancel = unlocked.docker.shutdown.child_token();
        let id = uuid::Uuid::new_v4().to_string();
        let failed = |failure: &'static str, detail: String| {
            Ok(DockerConnectOutcome::Failed { failure, detail })
        };
        let (client, ssh, host, name, local_socket) = if host_id == LOCAL_HOST {
            #[cfg(unix)]
            {
                let Some(path) = cc_docker_core::local_socket() else {
                    return failed("not_running", String::new());
                };
                let client = DockerClient::new(Arc::new(cc_docker_core::UnixConnector {
                    path: path.clone(),
                }));
                (client, None, None, String::new(), Some(path))
            }
            #[cfg(not(unix))]
            {
                return failed("not_running", String::new());
            }
        } else {
            let host = crate::dto::parse_id("host_id", &host_id)?;
            let name = unlocked
                .working()
                .host(host)
                .map(|host| host.name)
                .ok_or_else(|| invalid("host_unavailable"))?;
            let prepared = self.sharing_prepare_connection(&host_id).await?;
            if !Arc::ptr_eq(&unlocked, &prepared.unlocked) {
                return Err(AppError::VaultLocked);
            }
            let session = match prepared
                .run(async { Ok(unlocked.ssh.connector.connect(&prepared.plan).await?) })
                .await
            {
                Ok(session) => Arc::new(session),
                Err(AppError::VaultLocked) => return Err(AppError::VaultLocked),
                Err(_) => return failed("connection_failed", String::new()),
            };
            let probe = session
                .exec(SOCKET_PROBE)
                .await
                .map(|output| output.stdout_lossy())
                .unwrap_or_default();
            let probe = probe.trim();
            let socket = match probe.split_whitespace().collect::<Vec<_>>().as_slice() {
                ["ok", path] => Some((*path).to_owned()),
                ["denied", _, group @ ..] => {
                    let _ = session.disconnect().await;
                    return failed("permission_denied", group.join(" "));
                }
                _ => None,
            };
            // Socket forwarding first; the CLI's dial-stdio when the server
            // forbids it or uses another socket path.
            let mut chosen = None;
            if let Some(path) = socket {
                let client = DockerClient::new(Arc::new(SshSocket {
                    session: session.clone(),
                    path,
                }));
                if client.info().await.is_ok() {
                    chosen = Some(client);
                }
            }
            if chosen.is_none() {
                let client = DockerClient::new(Arc::new(SshDialStdio {
                    session: session.clone(),
                }));
                if tokio::time::timeout(Duration::from_secs(10), client.info())
                    .await
                    .is_ok_and(|r| r.is_ok())
                {
                    chosen = Some(client);
                }
            }
            let Some(client) = chosen else {
                let _ = session.disconnect().await;
                return match probe {
                    "missing" => failed("not_running", String::new()),
                    // The socket is usable but the server refuses to forward
                    // it and has no Docker CLI for dial-stdio.
                    _ if probe.starts_with("ok ") => {
                        failed("connection_failed", "socket_forwarding_disabled".into())
                    }
                    _ => failed("connection_failed", String::new()),
                };
            };
            (client, Some(session), Some(host), name, None)
        };
        let info = match tokio::time::timeout(Duration::from_secs(15), client.info()).await {
            Ok(Ok(info)) => info,
            Ok(Err(DockerError::PermissionDenied)) => {
                return failed("permission_denied", String::new())
            }
            Ok(Err(DockerError::NotRunning)) => return failed("not_running", String::new()),
            _ => return failed("connection_failed", String::new()),
        };
        if let Some(ssh) = &ssh {
            let ssh = ssh.clone();
            let scope = cancel.clone();
            tokio::spawn(async move {
                scope.cancelled().await;
                let _ = ssh.disconnect().await;
            });
        }
        let dto = session_dto(id.clone(), &host_id, name, info);
        unlocked
            .docker
            .sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                id,
                Arc::new(LiveDocker {
                    info: dto.clone(),
                    client,
                    ssh,
                    local_socket,
                    host_id: host,
                    cancel,
                }),
            );
        Ok(DockerConnectOutcome::Connected(dto))
    }

    pub async fn docker_disconnect(&self, session_id: String) -> AppResult<()> {
        let (_, unlocked) = self.unlocked().await?;
        if let Some(live) = unlocked
            .docker
            .sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&session_id)
        {
            live.cancel.cancel();
        }
        unlocked.docker.compose.forget_session(&session_id);
        unlocked
            .docker
            .forwards
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|_, forward| {
                let keep = forward
                    .info
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .session_id
                    != session_id;
                if !keep {
                    forward.cancel.cancel();
                }
                keep
            });
        Ok(())
    }

    /// Refreshes the engine counters together with the list.
    pub async fn docker_containers(
        &self,
        session_id: String,
        all: bool,
    ) -> AppResult<Vec<ContainerSummary>> {
        let (_, unlocked) = self.unlocked().await?;
        let live = unlocked.docker.session(&session_id)?;
        live.client.containers(all).await.map_err(docker_error)
    }

    pub async fn docker_info(&self, session_id: String) -> AppResult<DockerSessionDto> {
        let (_, unlocked) = self.unlocked().await?;
        let live = unlocked.docker.session(&session_id)?;
        let info = live.client.info().await.map_err(docker_error)?;
        Ok(session_dto(
            session_id,
            &live.info.host_id,
            live.info.host_name.clone(),
            info,
        ))
    }

    pub async fn docker_volumes(
        &self,
        session_id: String,
    ) -> AppResult<Vec<cc_docker_core::VolumeSummary>> {
        let (_, unlocked) = self.unlocked().await?;
        let live = unlocked.docker.session(&session_id)?;
        live.client.volumes().await.map_err(docker_error)
    }

    /// The container's environment with secret values, only after the vault
    /// passphrase checks out (a step-up like the production write window)
    /// or with a presence token for `docker_reveal` on
    /// `<session>/<container>`.
    pub async fn docker_reveal_environment(
        &self,
        session_id: String,
        container: String,
        proof: crate::StepUp,
    ) -> AppResult<Vec<String>> {
        let (_, unlocked) = self.unlocked().await?;
        let live = unlocked.docker.session(&session_id)?;
        self.check_step_up(
            crate::StepUpAction::DockerReveal,
            &format!("{session_id}/{container}"),
            proof,
        )
        .await?;
        live.client
            .environment(&container)
            .await
            .map_err(docker_error)
    }

    /// `docker inspect` with every secret hidden (environment, command
    /// line, labels, URL passwords, keys, tokens).
    pub async fn docker_inspect(
        &self,
        session_id: String,
        container: String,
    ) -> AppResult<serde_json::Value> {
        let (_, unlocked) = self.unlocked().await?;
        let live = unlocked.docker.session(&session_id)?;
        live.client.inspect(&container).await.map_err(docker_error)
    }

    /// `docker inspect` as the engine returns it, only after the vault
    /// passphrase checks out (the same step-up as the environment).
    pub async fn docker_inspect_reveal(
        &self,
        session_id: String,
        container: String,
        proof: crate::StepUp,
    ) -> AppResult<serde_json::Value> {
        let (_, unlocked) = self.unlocked().await?;
        let live = unlocked.docker.session(&session_id)?;
        self.check_step_up(
            crate::StepUpAction::DockerReveal,
            &format!("{session_id}/{container}"),
            proof,
        )
        .await?;
        live.client
            .inspect_unmasked(&container)
            .await
            .map_err(docker_error)
    }

    pub async fn docker_action(
        &self,
        session_id: String,
        container: String,
        action: String,
    ) -> AppResult<()> {
        let (_, unlocked) = self.unlocked().await?;
        let live = unlocked.docker.session(&session_id)?;
        let action = ContainerAction::parse(&action).ok_or_else(|| invalid("invalid_action"))?;
        live.client
            .action(&container, action)
            .await
            .map_err(docker_error)
    }

    pub async fn docker_logs_start(
        &self,
        session_id: String,
        container: String,
        tail: u32,
        follow: bool,
        timestamps: bool,
    ) -> AppResult<String> {
        let (_, unlocked) = self.unlocked().await?;
        let live = unlocked.docker.session(&session_id)?;
        let mut streams = unlocked
            .docker
            .logs
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if streams.len() >= MAX_LOG_STREAMS {
            return Err(invalid("too_many_log_streams"));
        }
        let (tx, rx) = mpsc::channel(64);
        let cancel = live.cancel.child_token();
        let id = uuid::Uuid::new_v4().to_string();
        streams.insert(
            id.clone(),
            Arc::new(LogStream {
                lines: tokio::sync::Mutex::new(rx),
                cancel: cancel.clone(),
            }),
        );
        let client = live.client.clone();
        tokio::spawn(async move {
            let _ = client
                .logs(&container, tail, follow, timestamps, tx, cancel)
                .await;
        });
        Ok(id)
    }

    /// Next log lines; `None` when the stream ended (the container stopped or
    /// the follow was not requested).
    pub async fn docker_logs_poll(
        &self,
        stream_id: String,
        wait_ms: u64,
    ) -> AppResult<Option<Vec<LogLine>>> {
        let (_, unlocked) = self.unlocked().await?;
        let stream = unlocked
            .docker
            .logs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&stream_id)
            .cloned()
            .ok_or_else(|| invalid("log_stream_closed"))?;
        let mut receiver = stream.lines.lock().await;
        let mut lines = Vec::new();
        match tokio::time::timeout(Duration::from_millis(wait_ms.min(2000)), receiver.recv()).await
        {
            Ok(Some(batch)) => lines.extend(batch),
            Ok(None) => {
                unlocked
                    .docker
                    .logs
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&stream_id);
                return Ok(None);
            }
            Err(_) => return Ok(Some(lines)),
        }
        while lines.len() < 2000 {
            match receiver.try_recv() {
                Ok(batch) => lines.extend(batch),
                Err(_) => break,
            }
        }
        Ok(Some(lines))
    }

    pub async fn docker_logs_stop(&self, stream_id: String) -> AppResult<()> {
        let (_, unlocked) = self.unlocked().await?;
        if let Some(stream) = unlocked
            .docker
            .logs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&stream_id)
        {
            stream.cancel.cancel();
        }
        Ok(())
    }

    /// A shell in the container as a regular terminal tab.
    pub async fn docker_terminal_open(
        &self,
        session_id: String,
        container: String,
        name: String,
        command: Vec<String>,
        cols: u32,
        rows: u32,
    ) -> AppResult<TerminalInfoDto> {
        if cols == 0 || rows == 0 || cols > 65535 || rows > 65535 {
            return Err(AppError::invalid("terminal_size", "invalid_size"));
        }
        let (owner, unlocked) = self.unlocked().await?;
        let live = unlocked.docker.session(&session_id)?;
        let command = if command.first().map(String::as_str) == Some(AUTO_SHELL)
            || command.is_empty()
        {
            vec![
                "/bin/sh".to_owned(),
                "-c".to_owned(),
                "if [ -x /bin/bash ]; then exec /bin/bash; elif [ -x /bin/ash ]; then exec /bin/ash; else exec /bin/sh; fi"
                    .to_owned(),
            ]
        } else {
            command
        };
        let exec = live
            .client
            .exec(
                &container,
                &command,
                cols.min(65535) as u16,
                rows.min(65535) as u16,
            )
            .await
            .map_err(docker_error)?;
        let cancel = live.cancel.child_token();
        let (channel, mut remote) = ShellChannel::pair(8);
        let (mut reader, mut writer) = tokio::io::split(exec.io);
        let client = live.client.clone();
        let exec_id = exec.exec_id.clone();
        let worker = cancel.clone();
        tokio::spawn(async move {
            let mut buffer = vec![0u8; 16 * 1024];
            loop {
                tokio::select! {
                    biased;
                    _ = worker.cancelled() => break,
                    read = reader.read(&mut buffer) => match read {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if remote.events.send(ShellEvent::Data(bytes::Bytes::copy_from_slice(&buffer[..n]))).await.is_err() {
                                break;
                            }
                        }
                    },
                    input = remote.inputs.recv() => match input {
                        Some(ShellInput::Data(bytes)) => {
                            if writer.write_all(&bytes).await.is_err() { break; }
                        }
                        Some(ShellInput::Resize { cols, rows }) => {
                            let _ = client.resize_exec(&exec_id, cols.clamp(1, 65535) as u16, rows.clamp(1, 65535) as u16).await;
                        }
                        Some(ShellInput::Eof) => { let _ = writer.shutdown().await; }
                        Some(ShellInput::Close) | None => break,
                    },
                }
            }
            let _ = remote.events.send(ShellEvent::Closed).await;
        });
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        let id = unlocked
            .ssh
            .terminals
            .open_channel(
                TerminalTarget::Docker {
                    host_id: live.host_id,
                    container: container.clone(),
                },
                name,
                TerminalSize { cols, rows },
                OpenedShell {
                    channel,
                    session: None,
                    lifetime: Some(Arc::new(ShellLife(cancel))),
                },
            )
            .await?;
        Ok(unlocked.ssh.terminals.info(id)?.into())
    }

    /// Forwards `127.0.0.1:local_port` (0 = any) to a container port: through
    /// the SSH host to the published port or the container's IP.
    pub async fn docker_forward_start(
        &self,
        session_id: String,
        container: String,
        remote_port: u16,
        local_port: u16,
    ) -> AppResult<DockerForwardDto> {
        let (_, unlocked) = self.unlocked().await?;
        let live = unlocked.docker.session(&session_id)?;
        if remote_port == 0 {
            return Err(invalid("invalid_port"));
        }
        if unlocked
            .docker
            .forwards
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
            >= MAX_FORWARDS
        {
            return Err(invalid("too_many_forwards"));
        }
        let summary = live
            .client
            .containers(true)
            .await
            .map_err(docker_error)?
            .into_iter()
            .find(|c| c.id == container || c.name == container)
            .ok_or_else(|| invalid("container_not_found"))?;
        // A published port is reachable on the engine host; otherwise the
        // container IP on its network (Linux engines route it from the host).
        let target = summary
            .ports
            .iter()
            .find(|p| {
                p.private_port == remote_port && p.public_port.is_some() && p.protocol == "tcp"
            })
            .map(|p| ("127.0.0.1".to_owned(), p.public_port.unwrap_or(remote_port)))
            .or_else(|| {
                summary
                    .networks
                    .values()
                    .next()
                    .map(|ip| (ip.clone(), remote_port))
            })
            .ok_or_else(|| invalid("container_unreachable"))?;
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", local_port))
            .await
            .map_err(|_| invalid("local_port_busy"))?;
        let address = listener
            .local_addr()
            .map_err(|_| invalid("local_port_busy"))?
            .to_string();
        let id = uuid::Uuid::new_v4().to_string();
        let cancel = live.cancel.child_token();
        let forward = Arc::new(Forward {
            info: Mutex::new(DockerForwardDto {
                id: id.clone(),
                session_id,
                container_name: summary.name.clone(),
                remote_port,
                address,
                state: "running",
                active_connections: 0,
                bytes_sent: 0,
                bytes_received: 0,
                error_code: None,
            }),
            active: AtomicU64::new(0),
            sent: AtomicU64::new(0),
            received: AtomicU64::new(0),
            cancel: cancel.clone(),
        });
        unlocked
            .docker
            .forwards
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, forward.clone());
        let started = forward.snapshot();
        let ssh = live.ssh.clone();
        tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => break,
                    accepted = listener.accept() => accepted,
                };
                let Ok((mut local, _)) = accepted else {
                    continue;
                };
                let (ssh, target, forward, cancel) =
                    (ssh.clone(), target.clone(), forward.clone(), cancel.clone());
                tokio::spawn(async move {
                    let upstream: Result<DockerIo, ()> = match &ssh {
                        Some(ssh) => ssh
                            .open_direct_tcpip(&target.0, target.1, None)
                            .await
                            .map(|stream| Box::new(stream) as DockerIo)
                            .map_err(|_| ()),
                        None => tokio::net::TcpStream::connect((target.0.as_str(), target.1))
                            .await
                            .map(|stream| Box::new(stream) as DockerIo)
                            .map_err(|_| ()),
                    };
                    let Ok(mut upstream) = upstream else {
                        forward
                            .info
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .error_code = Some("target_unreachable");
                        return;
                    };
                    forward.active.fetch_add(1, Ordering::Relaxed);
                    tokio::select! {
                        _ = cancel.cancelled() => {}
                        result = tokio::io::copy_bidirectional(&mut local, &mut upstream) => {
                            if let Ok((sent, received)) = result {
                                forward.sent.fetch_add(sent, Ordering::Relaxed);
                                forward.received.fetch_add(received, Ordering::Relaxed);
                            }
                        }
                    }
                    forward.active.fetch_sub(1, Ordering::Relaxed);
                });
            }
            forward.info.lock().unwrap_or_else(|e| e.into_inner()).state = "stopped";
        });
        Ok(started)
    }

    pub async fn docker_forwards(&self) -> AppResult<Vec<DockerForwardDto>> {
        let (_, unlocked) = self.unlocked().await?;
        let forwards = unlocked
            .docker
            .forwards
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut list: Vec<_> = forwards.values().map(|f| f.snapshot()).collect();
        list.sort_by(|a, b| a.address.cmp(&b.address));
        Ok(list)
    }

    pub async fn docker_forward_stop(&self, forward_id: String) -> AppResult<()> {
        let (_, unlocked) = self.unlocked().await?;
        if let Some(forward) = unlocked
            .docker
            .forwards
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&forward_id)
        {
            forward.cancel.cancel();
        }
        Ok(())
    }
}
