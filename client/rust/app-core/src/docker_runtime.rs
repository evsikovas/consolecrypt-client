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
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

mod removal;

const MAX_SESSIONS: usize = 16;
const MAX_LOG_STREAMS: usize = 32;
const MAX_FORWARDS: usize = 64;
pub const LOCAL_HOST: &str = "local";
const AUTO_SHELL: &str = "/__consolecrypt_auto_shell__";

#[cfg(test)]
mod forward_tests;

#[cfg(test)]
mod local_engine_tests {
    use super::*;

    #[derive(Debug)]
    struct NeverConnect;
    #[async_trait]
    impl DockerConnector for NeverConnect {
        async fn connect(&self) -> Result<DockerIo, DockerError> {
            panic!("session selection must not connect to any engine")
        }
    }

    #[test]
    fn restored_forward_uses_current_engine_network_bindings() {
        use serde_json::json;
        let mut inspect = json!({
            "State":{"Running":true},
            "NetworkSettings":{"Ports":{"5432/tcp":[{"HostIp":"192.0.2.4","HostPort":"15432"}]},
                "Networks":{"db":{"IPAddress":"172.18.0.7"}}}
        });
        assert_eq!(
            forward_target(&inspect, 5432).unwrap(),
            "192.0.2.4:15432".parse().unwrap()
        );
        inspect["NetworkSettings"]["Ports"]["5432/tcp"][0]["HostIp"] = json!("::");
        assert_eq!(
            forward_target(&inspect, 5432).unwrap(),
            "[::1]:15432".parse().unwrap()
        );
        inspect["NetworkSettings"]["Ports"] = json!({});
        assert_eq!(
            forward_target(&inspect, 5432).unwrap(),
            "172.18.0.7:5432".parse().unwrap()
        );
        inspect["NetworkSettings"]["Networks"] = json!({"v6":{"GlobalIPv6Address":"2001:db8::2"}});
        assert_eq!(
            forward_target(&inspect, 5432).unwrap(),
            "[2001:db8::2]:5432".parse().unwrap()
        );
        inspect["NetworkSettings"]["Networks"] = json!({});
        assert!(forward_target(&inspect, 5432).is_err());
        inspect["HostConfig"] = json!({"NetworkMode":"host"});
        assert_eq!(
            forward_target(&inspect, 5432).unwrap(),
            "127.0.0.1:5432".parse().unwrap()
        );
        inspect["State"]["Running"] = json!(false);
        assert!(forward_target(&inspect, 5432).is_err());
    }

    #[test]
    fn named_engines_and_legacy_local_sessions_are_never_interchanged() {
        let runtime = DockerRuntime::default();
        let host = ObjectId::new();
        for (id, host_id, ssh_host) in [
            ("legacy", "local", None),
            ("orb", "local/orbstack", None),
            ("colima", "local/colima/work", None),
            ("ssh", "remote", Some(host)),
        ] {
            runtime.sessions.lock().unwrap().insert(
                id.into(),
                Arc::new(LiveDocker {
                    info: DockerSessionDto {
                        id: id.into(),
                        host_id: host_id.into(),
                        host_name: id.into(),
                        engine_version: "fixture".into(),
                        os: "linux".into(),
                        arch: "arm64".into(),
                        containers_running: 0,
                        containers_total: 0,
                        images: 0,
                    },
                    client: DockerClient::new(Arc::new(NeverConnect)),
                    ssh: None,
                    local_endpoint: None,
                    host_id: ssh_host,
                    cancel: CancellationToken::new(),
                }),
            );
        }
        assert_eq!(runtime.session_ids(None), ["legacy"]);
        assert_eq!(runtime.session_ids(Some(host)), ["ssh"]);
        assert_eq!(runtime.local_session_ids("local/orbstack"), ["orb"]);
        assert_eq!(runtime.local_session_ids("local/colima/work"), ["colima"]);
        assert!(runtime.local_session_ids("local/colima/missing").is_empty());
        runtime.sessions.lock().unwrap()["orb"].cancel.cancel();
        assert!(runtime.local_session_ids("local/orbstack").is_empty());
        assert_eq!(runtime.local_session_ids("local/colima/work"), ["colima"]);
    }
}

/// Finds the engine socket the SSH user can use and reports why not.
pub(crate) const SOCKET_PROBE: &str = "for s in /var/run/docker.sock /run/docker.sock \"/run/user/$(id -u)/docker.sock\"; do \
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
    pub host_id: String,
    pub container_name: String,
    pub remote_port: u16,
    pub requested_local_port: u16,
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
    local_endpoint: Option<String>,
    host_id: Option<ObjectId>,
    cancel: CancellationToken,
}

/// What compose file access needs from a session.
pub(crate) struct ComposeTarget {
    pub client: DockerClient,
    pub ssh: Option<Arc<SshSession>>,
    pub local_endpoint: Option<String>,
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
    listener_closed: CancellationToken,
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
    removals: removal::PendingRemovals,
    logs: Mutex<HashMap<String, Arc<LogStream>>>,
    forwards: Mutex<HashMap<String, Arc<Forward>>>,
    pub(crate) compose: crate::docker_compose::ComposeState,
}

impl DockerRuntime {
    pub(crate) fn close(&self) {
        self.shutdown.cancel();
        self.compose.close();
        self.removals.clear();
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
            .filter(|(_, live)| {
                live.host_id == host
                    && !live.cancel.is_cancelled()
                    && (host.is_some() || live.info.host_id == LOCAL_HOST)
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub(crate) fn local_session_ids(&self, engine: &str) -> Vec<String> {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(_, live)| {
                live.host_id.is_none() && live.info.host_id == engine && !live.cancel.is_cancelled()
            })
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
            local_endpoint: live.local_endpoint.clone(),
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

/// Resolve the upstream on the engine host from the container's current inspect
/// response. The host side of a published port can be bound to a specific IP.
fn forward_target(inspect: &serde_json::Value, remote_port: u16) -> AppResult<SocketAddr> {
    if inspect["State"]["Running"].as_bool() != Some(true) {
        return Err(invalid("container_unreachable"));
    }
    let network = &inspect["NetworkSettings"];
    let bindings = &network["Ports"][format!("{remote_port}/tcp")];
    for binding in bindings.as_array().into_iter().flatten() {
        let Some(port) = binding["HostPort"]
            .as_str()
            .and_then(|p| p.parse::<u16>().ok())
            .filter(|p| *p != 0)
        else {
            continue;
        };
        let host = match binding["HostIp"].as_str().unwrap_or_default() {
            "" | "0.0.0.0" => IpAddr::V4(Ipv4Addr::LOCALHOST),
            "::" => IpAddr::V6(Ipv6Addr::LOCALHOST),
            other => match other.parse::<IpAddr>() {
                Ok(ip) => ip,
                Err(_) => continue,
            },
        };
        return Ok(SocketAddr::new(host, port));
    }
    if inspect["HostConfig"]["NetworkMode"].as_str() == Some("host") {
        return Ok(SocketAddr::from((Ipv4Addr::LOCALHOST, remote_port)));
    }
    let address = network["Networks"]
        .as_object()
        .into_iter()
        .flat_map(|n| n.values())
        .flat_map(|endpoint| [&endpoint["IPAddress"], &endpoint["GlobalIPv6Address"]])
        .chain([&network["IPAddress"], &network["GlobalIPv6Address"]])
        .filter_map(|value| value.as_str()?.parse::<IpAddr>().ok())
        .find(|ip| !ip.is_unspecified())
        .ok_or_else(|| invalid("container_unreachable"))?;
    Ok(SocketAddr::new(address, remote_port))
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
        let engines = tokio::task::spawn_blocking(cc_docker_core::local::local_engines)
            .await
            .map_err(|_| invalid("local_discovery_failed"))?;
        hosts.extend(engines.into_iter().map(|engine| DockerHostDto {
            id: engine.id,
            name: engine.name,
            kind: "local",
            address: engine.endpoint.uri(),
        }));
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
        let (client, ssh, host, name, local_endpoint) =
            if cc_docker_core::local::is_local_host(&host_id) {
                let engines = tokio::task::spawn_blocking(cc_docker_core::local::local_engines)
                    .await
                    .map_err(|_| invalid("local_discovery_failed"))?;
                let engine = if host_id == LOCAL_HOST {
                    engines.into_iter().next()
                } else {
                    engines.into_iter().find(|engine| engine.id == host_id)
                };
                let Some(engine) = engine else {
                    return failed("not_running", String::new());
                };
                let endpoint = engine.endpoint.uri();
                let connector = cc_docker_core::local::connector(&engine.endpoint)
                    .map_err(|e| invalid(e.code()))?;
                let client = DockerClient::new(connector);
                (client, None, None, engine.name, Some(endpoint))
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
                return failed("permission_denied", String::new());
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
                    local_endpoint,
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
        unlocked.docker.removals.forget_session(&session_id);
        let mut closing = Vec::new();
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
                    closing.push(forward.listener_closed.clone());
                }
                keep
            });
        for closed in closing {
            closed.cancelled().await;
        }
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

    pub async fn docker_image_repo_digests(
        &self,
        session_id: String,
        image_id: String,
        source_image: String,
    ) -> AppResult<Vec<String>> {
        let (_, unlocked) = self.unlocked().await?;
        let live = unlocked.docker.session(&session_id)?;
        tokio::select! {
            biased;
            _ = live.cancel.cancelled() => Err(invalid("session_closed")),
            digests = live.client.image_repo_digests(&image_id, &source_image) => digests.map_err(docker_error),
        }
    }

    pub async fn docker_networks(&self, session_id: String) -> AppResult<Vec<serde_json::Value>> {
        let (_, unlocked) = self.unlocked().await?;
        let live = unlocked.docker.session(&session_id)?;
        tokio::select! { biased; _ = live.cancel.cancelled() => Err(invalid("session_closed")),
        result = live.client.networks() => result.map_err(docker_error) }
    }

    pub async fn docker_images(&self, session_id: String) -> AppResult<Vec<serde_json::Value>> {
        let (_, unlocked) = self.unlocked().await?;
        let live = unlocked.docker.session(&session_id)?;
        tokio::select! { biased; _ = live.cancel.cancelled() => Err(invalid("session_closed")),
        result = live.client.images() => result.map_err(docker_error) }
    }

    pub async fn docker_remove_image(
        &self,
        session_id: String,
        image_id: String,
        tag: Option<String>,
    ) -> AppResult<bool> {
        let (_, unlocked) = self.unlocked().await?;
        let live = unlocked.docker.session(&session_id)?;
        tokio::select! { biased; _ = live.cancel.cancelled() => Err(invalid("session_closed")),
        result = live.client.remove_image(&image_id, tag.as_deref()) => result.map_err(docker_error) }
    }
    pub async fn docker_telemetry(
        &self,
        session_id: String,
        container: String,
    ) -> AppResult<serde_json::Value> {
        let (_, unlocked) = self.unlocked().await?;
        let live = unlocked.docker.session(&session_id)?;
        tokio::select! { biased; _ = live.cancel.cancelled() => Err(invalid("session_closed")),
        result = live.client.telemetry(&container) => result.map_err(docker_error) }
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
        let inspect = live
            .client
            .inspect(&container)
            .await
            .map_err(|error| match error {
                DockerError::NotFound => invalid("container_not_found"),
                other => docker_error(other),
            })?;
        let target = forward_target(&inspect, remote_port)?;
        let container_name = inspect["Name"]
            .as_str()
            .unwrap_or(&container)
            .trim_start_matches('/')
            .to_owned();
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
                host_id: live.info.host_id.clone(),
                container_name,
                remote_port,
                requested_local_port: local_port,
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
            listener_closed: CancellationToken::new(),
        });
        {
            let mut handles = unlocked
                .docker
                .forwards
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if live.cancel.is_cancelled() {
                return Err(invalid("session_closed"));
            }
            handles.insert(id, forward.clone());
        }
        let started = forward.snapshot();
        let ssh = live.ssh.clone();
        let closed = forward.listener_closed.clone().drop_guard();
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
                    (ssh.clone(), target, forward.clone(), cancel.clone());
                tokio::spawn(async move {
                    let upstream: Result<DockerIo, ()> = match &ssh {
                        Some(ssh) => ssh
                            .open_direct_tcpip(&target.ip().to_string(), target.port(), None)
                            .await
                            .map(|stream| Box::new(stream) as DockerIo)
                            .map_err(|_| ()),
                        None => tokio::net::TcpStream::connect(target)
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
            drop(listener);
            drop(closed);
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
        let forward = unlocked
            .docker
            .forwards
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&forward_id);
        if let Some(forward) = forward {
            forward.cancel.cancel();
            forward.listener_closed.cancelled().await;
        }
        Ok(())
    }
}
