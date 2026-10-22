//! Device-local database connections and the SQLite file routes.
//!
//! Connections of engines added after 0.9.1 ([`DbEngine::local_only`], today
//! SQLite) are kept in this profile's encrypted local store, never in the
//! synced vault: clients up to 0.9.1 cannot decode such a vault object, and a
//! file path on this computer means nothing on another device. They are not
//! shared, synced or part of vault backups; the UI says "this device only".
//!
//! SQLite files open on this computer, or through the `sqlite3` program over
//! the saved SSH host's exec channel, a Docker exec or a Kubernetes Pod exec.
use crate::{AppCore, AppError, AppResult};
use async_trait::async_trait;
use cc_db_core::{
    sqlite_cli::{shell_command, CliOutput, SqliteCliRunner},
    DbError, SqliteTarget,
};
use cc_models::{
    database::{DbConnection, DbEngine, DbKubeTargetKind, DbRoute},
    ObjectId, VaultObject,
};
use std::{fmt, sync::Arc};
use tokio_util::sync::CancellationToken;

/// Local store key: a list of connection objects.
const LOCAL_CONNECTIONS: &str = "database.local_connections.v1";
const MAX_LOCAL_CONNECTIONS: usize = 1000;

fn invalid(reason: &'static str) -> AppError {
    AppError::invalid("database", reason)
}

/// Decodes stored connections one by one; an entry this version cannot read
/// (a later engine) is kept as stored and skipped.
fn decode(values: &[serde_json::Value]) -> Vec<DbConnection> {
    values
        .iter()
        .filter_map(|value| serde_json::from_value::<DbConnection>(value.clone()).ok())
        .filter(|connection| connection.engine.local_only() && connection.validate().is_ok())
        .collect()
}

fn same_id(value: &serde_json::Value, id: ObjectId) -> bool {
    value.get("id").and_then(|id| id.as_str()) == Some(id.to_string().as_str())
}

impl AppCore {
    /// Connections kept only on this device.
    pub(crate) async fn db_local_connections(&self) -> AppResult<Vec<DbConnection>> {
        let (owner, _) = self.unlocked().await?;
        let values: Vec<serde_json::Value> = owner
            .storage()?
            .setting_get(LOCAL_CONNECTIONS)
            .await?
            .unwrap_or_default();
        Ok(decode(&values))
    }

    /// A saved connection from the vault or from this device.
    pub(crate) async fn db_saved_connection(&self, id: ObjectId) -> AppResult<Option<DbConnection>> {
        let (_, unlocked) = self.unlocked().await?;
        if let Some(connection) = unlocked.working().db_connection(id) {
            return Ok(Some(connection));
        }
        Ok(self
            .db_local_connections()
            .await?
            .into_iter()
            .find(|connection| connection.id == id))
    }

    /// Creates or replaces a device-local connection. It keeps no secrets;
    /// its route references (SSH host, Kubernetes context) must exist.
    pub(crate) async fn db_save_local_connection(
        &self,
        mut connection: DbConnection,
        creating: bool,
    ) -> AppResult<DbConnection> {
        let (owner, unlocked) = self.unlocked().await?;
        if !connection.engine.local_only() || connection.engine == DbEngine::Unknown {
            return Err(invalid("engine_unavailable"));
        }
        let existing = if creating {
            None
        } else {
            Some(
                self.db_local_connections()
                    .await?
                    .into_iter()
                    .find(|old| old.id == connection.id)
                    .ok_or_else(|| invalid("connection_unavailable"))?,
            )
        };
        let now = chrono::Utc::now();
        connection.created_at = existing.as_ref().map_or(now, |old| old.created_at);
        connection.updated_at = now;
        crate::inventory::validate_refs(
            unlocked.working(),
            &VaultObject::DbConnection(connection.clone()),
        )?;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        let stored = serde_json::to_value(&connection).map_err(|_| invalid("invalid_connection"))?;
        let id = connection.id;
        owner
            .storage()?
            .write(move |tx| {
                let mut values: Vec<serde_json::Value> =
                    tx.setting_get(LOCAL_CONNECTIONS)?.unwrap_or_default();
                values.retain(|value| !same_id(value, id));
                values.push(stored);
                values.truncate(MAX_LOCAL_CONNECTIONS);
                tx.setting_set(LOCAL_CONNECTIONS, &values)
            })
            .await?;
        Ok(connection)
    }

    /// Removes a device-local connection; false when it is not one.
    pub(crate) async fn db_delete_local_connection(&self, id: ObjectId) -> AppResult<bool> {
        let (owner, unlocked) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        Ok(owner
            .storage()?
            .write(move |tx| {
                let mut values: Vec<serde_json::Value> =
                    tx.setting_get(LOCAL_CONNECTIONS)?.unwrap_or_default();
                let before = values.len();
                values.retain(|value| !same_id(value, id));
                let removed = values.len() != before;
                if removed {
                    tx.setting_set(LOCAL_CONNECTIONS, &values)?;
                }
                Ok::<_, cc_storage_core::StorageError>(removed)
            })
            .await?)
    }

    /// Creates a new, empty SQLite database file on this computer. Nothing
    /// else creates one: connecting to a missing file fails.
    pub async fn db_sqlite_create_file(&self, path: String) -> AppResult<()> {
        self.unlocked().await?;
        let path = std::path::PathBuf::from(path);
        tokio::task::spawn_blocking(move || cc_db_core::sqlite_create_file(&path))
            .await
            .map_err(|_| invalid("connection_failed"))?
            .map_err(|error| invalid(cc_db_core::streaming::error_code(&error)))
    }

    /// Where an SQLite connection's file opens: this computer, or the
    /// `sqlite3` program through the route.
    pub(crate) async fn db_sqlite_target(
        &self,
        connection: &DbConnection,
        cancel: &CancellationToken,
    ) -> AppResult<SqliteTarget> {
        let (_, unlocked) = self.unlocked().await?;
        let runner: Arc<dyn SqliteCliRunner> = match &connection.route {
            DbRoute::Direct => {
                if cfg!(any(target_os = "android", target_os = "ios")) {
                    // Phones hand out document URIs, not file paths.
                    return Err(invalid("sqlite_local_unavailable"));
                }
                return Ok(SqliteTarget::Local);
            }
            DbRoute::Ssh { host_id } => {
                let prepared = self
                    .sharing_prepare_connection(&host_id.to_string())
                    .await?;
                if !Arc::ptr_eq(&unlocked, &prepared.unlocked) {
                    return Err(AppError::VaultLocked);
                }
                let session = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return Err(AppError::VaultLocked),
                    result = prepared.run(async {
                        Ok(unlocked.ssh.connector.connect(&prepared.plan).await?)
                    }) => result?,
                };
                Arc::new(SshProgram {
                    session: Arc::new(session),
                    scope: cancel.clone(),
                })
            }
            DbRoute::Docker { host_id, container } => {
                let mut sessions = unlocked.docker.session_ids(*host_id);
                if sessions.is_empty() {
                    let host = host_id.map_or_else(
                        || crate::docker_runtime::LOCAL_HOST.to_owned(),
                        |id| id.to_string(),
                    );
                    match self.docker_connect(host).await {
                        Ok(crate::docker_runtime::DockerConnectOutcome::Connected(session)) => {
                            sessions.push(session.id)
                        }
                        Ok(_) => return Err(invalid("docker_unreachable")),
                        Err(AppError::VaultLocked) => return Err(AppError::VaultLocked),
                        Err(_) => return Err(invalid("docker_unreachable")),
                    }
                }
                let target = unlocked.docker.compose_target(&sessions[0])?;
                Arc::new(DockerProgram {
                    client: target.client,
                    container: container.clone(),
                    scope: target.cancel,
                })
            }
            DbRoute::Kubernetes {
                context_id,
                namespace,
                target_kind,
                target_name,
            } => {
                let (resources, pod, scope) = self
                    .db_kube_exec_target(*context_id, namespace, *target_kind, target_name)
                    .await?;
                Arc::new(KubeProgram {
                    resources,
                    namespace: namespace.clone(),
                    pod,
                    scope,
                })
            }
            _ => return Err(invalid("route_unavailable")),
        };
        Ok(SqliteTarget::Cli(runner))
    }

    /// The Pod a Kubernetes route runs `sqlite3` in (a ready Pod of a
    /// Service, Deployment or StatefulSet). Production contexts allow it
    /// only when the context allows exec in production.
    async fn db_kube_exec_target(
        &self,
        context_id: ObjectId,
        namespace: &str,
        kind: DbKubeTargetKind,
        name: &str,
    ) -> AppResult<(
        cc_k8s_core::resources::ResourceApi,
        String,
        CancellationToken,
    )> {
        let (_, unlocked) = self.unlocked().await?;
        let context = context_id.to_string();
        let live = match unlocked
            .kube
            .connections()
            .into_iter()
            .find(|live| live.info.context_id == context && !live.cancel.is_cancelled())
        {
            Some(live) => live,
            None => {
                let info = self.kube_connect(context.clone()).await.map_err(|error| match error {
                    AppError::VaultLocked => error,
                    _ => invalid("kube_unreachable"),
                })?;
                self.kube_live(&info.id).await?
            }
        };
        if live.info.production && !live.info.allow_exec_in_prod {
            return Err(invalid("kube_exec_blocked"));
        }
        if kind == DbKubeTargetKind::Pod {
            return Ok((live.resources.clone(), name.to_owned(), live.cancel.clone()));
        }
        if live
            .resources
            .kinds()
            .map_err(|_| invalid("kube_unreachable"))?
            .is_empty()
        {
            tokio::time::timeout(std::time::Duration::from_secs(15), live.resources.discover())
                .await
                .map_err(|_| invalid("kube_unreachable"))?
                .map_err(|_| invalid("kube_unreachable"))?;
        }
        let (group, resource) = match kind {
            DbKubeTargetKind::Service => ("", "services"),
            DbKubeTargetKind::Deployment => ("apps", "deployments"),
            _ => ("apps", "statefulsets"),
        };
        let reference = cc_k8s_core::resources::ResourceRef {
            api: live
                .resources
                .resolve(group, "v1", resource)
                .map_err(|_| invalid("kube_target_missing"))?,
            namespace: Some(namespace.to_owned()),
            name: name.to_owned(),
        };
        let targets = live
            .resources
            .forward_targets(&reference)
            .await
            .map_err(|_| invalid("kube_target_missing"))?;
        let pod = targets
            .items
            .iter()
            .find(|target| target.ready)
            .or(targets.items.first())
            .map(|target| target.pod.name.clone())
            .ok_or_else(|| invalid("kube_target_missing"))?;
        Ok((live.resources.clone(), pod, live.cancel.clone()))
    }
}

/// `sqlite3` over the SSH host's exec channel (one channel per statement).
/// The connection closes with the database session.
struct SshProgram {
    session: Arc<cc_ssh_core::SshSession>,
    scope: CancellationToken,
}

impl fmt::Debug for SshProgram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SshProgram([SCOPED])")
    }
}

impl Drop for SshProgram {
    fn drop(&mut self) {
        let session = Arc::clone(&self.session);
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = session.disconnect().await;
            });
        }
    }
}

#[async_trait]
impl SqliteCliRunner for SshProgram {
    async fn run(
        &self,
        argv: Vec<String>,
        stdin: Vec<u8>,
        stdout_limit: usize,
        cancel: CancellationToken,
    ) -> Result<CliOutput, DbError> {
        use cc_ssh_core::russh::ChannelMsg;
        let command = shell_command(&argv);
        let work = async {
            let mut channel = self
                .session
                .open_exec(&command)
                .await
                .map_err(|_| DbError::ConnectionFailed)?;
            channel
                .data(&stdin[..])
                .await
                .map_err(|_| DbError::ConnectionFailed)?;
            channel.eof().await.map_err(|_| DbError::ConnectionFailed)?;
            let mut output = CliOutput::default();
            while let Some(message) = channel.wait().await {
                match message {
                    ChannelMsg::Data { data } => {
                        let room = stdout_limit.saturating_sub(output.stdout.len());
                        output.stdout.extend_from_slice(&data[..data.len().min(room)]);
                        if data.len() > room {
                            output.truncated = true;
                            let _ = channel.close().await;
                            break;
                        }
                    }
                    ChannelMsg::ExtendedData { data, .. } if output.stderr.len() < 64 * 1024 => {
                        output.stderr.extend_from_slice(&data)
                    }
                    ChannelMsg::ExitStatus { exit_status } => {
                        output.status = Some(i32::try_from(exit_status).unwrap_or(i32::MAX))
                    }
                    ChannelMsg::Failure => return Err(DbError::ConnectionFailed),
                    ChannelMsg::Close => break,
                    _ => {}
                }
            }
            if output.truncated {
                output.status = Some(0);
            }
            Ok(output)
        };
        tokio::select! {
            biased;
            _ = self.scope.cancelled() => Err(DbError::SessionClosed),
            _ = cancel.cancelled() => Err(DbError::Cancelled),
            output = work => output,
        }
    }
}

/// `sqlite3` through a Docker exec without a terminal.
struct DockerProgram {
    client: cc_docker_core::DockerClient,
    container: String,
    scope: CancellationToken,
}

impl fmt::Debug for DockerProgram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DockerProgram([SCOPED])")
    }
}

#[async_trait]
impl SqliteCliRunner for DockerProgram {
    async fn run(
        &self,
        argv: Vec<String>,
        stdin: Vec<u8>,
        stdout_limit: usize,
        cancel: CancellationToken,
    ) -> Result<CliOutput, DbError> {
        let stop = cancel.child_token();
        let run = self
            .client
            .exec_collect(&self.container, &argv, &stdin, stdout_limit, stop.clone());
        let output = tokio::select! {
            biased;
            _ = self.scope.cancelled() => return Err(DbError::SessionClosed),
            output = run => output.map_err(|error| match error {
                cc_docker_core::DockerError::NotFound => DbError::ConnectionFailed,
                _ if cancel.is_cancelled() => DbError::Cancelled,
                _ => DbError::ConnectionFailed,
            })?,
        };
        Ok(CliOutput {
            status: if output.truncated {
                Some(0)
            } else {
                output.exit_code.and_then(|code| i32::try_from(code).ok())
            },
            truncated: output.truncated,
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

/// `sqlite3` through a Kubernetes Pod exec without a terminal.
struct KubeProgram {
    resources: cc_k8s_core::resources::ResourceApi,
    namespace: String,
    pod: String,
    scope: CancellationToken,
}

impl fmt::Debug for KubeProgram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("KubeProgram([SCOPED])")
    }
}

#[async_trait]
impl SqliteCliRunner for KubeProgram {
    async fn run(
        &self,
        argv: Vec<String>,
        stdin: Vec<u8>,
        stdout_limit: usize,
        cancel: CancellationToken,
    ) -> Result<CliOutput, DbError> {
        use cc_k8s_core::resources::{PodTerminalInput, PodTerminalOutput, PodTerminalRequest};
        let request: PodTerminalRequest = serde_json::from_value(serde_json::json!({
            "namespace": self.namespace,
            "pod": self.pod,
            "container": null,
            "command": argv,
            "tty": false,
        }))
        .map_err(|_| DbError::InvalidRequest)?;
        let (input, input_rx) = tokio::sync::mpsc::channel(4);
        let (output_tx, mut events) = tokio::sync::mpsc::channel(16);
        let stop = cancel.child_token();
        let api = self.resources.clone();
        let scope = stop.clone();
        let task = tokio::spawn(async move { api.terminal(request, input_rx, output_tx, scope).await });
        let collect = async {
            let mut output = CliOutput::default();
            let mut fed = false;
            while let Some(event) = events.recv().await {
                match event {
                    PodTerminalOutput::Connected if !fed => {
                        fed = true;
                        for chunk in stdin.chunks(32 * 1024) {
                            input
                                .send(PodTerminalInput::Stdin(zeroize::Zeroizing::new(chunk.to_vec())))
                                .await
                                .map_err(|_| DbError::ConnectionFailed)?;
                        }
                        input
                            .send(PodTerminalInput::Eof)
                            .await
                            .map_err(|_| DbError::ConnectionFailed)?;
                    }
                    PodTerminalOutput::Stdout(data) => {
                        let room = stdout_limit.saturating_sub(output.stdout.len());
                        output.stdout.extend_from_slice(&data[..data.len().min(room)]);
                        if data.len() > room {
                            output.truncated = true;
                            output.status = Some(0);
                            break;
                        }
                    }
                    PodTerminalOutput::Stderr(data) if output.stderr.len() < 64 * 1024 => {
                        output.stderr.extend_from_slice(&data)
                    }
                    PodTerminalOutput::Exited { code } => {
                        output.status = code;
                        break;
                    }
                    _ => {}
                }
            }
            Ok::<_, DbError>(output)
        };
        let result = tokio::select! {
            biased;
            _ = self.scope.cancelled() => Err(DbError::SessionClosed),
            _ = cancel.cancelled() => Err(DbError::Cancelled),
            output = collect => output,
        };
        stop.cancel();
        let joined = task.await;
        let mut output = result?;
        if output.status.is_none() && !output.truncated {
            // The exec could not start (no such program or Pod).
            output.status = Some(match joined {
                Ok(Ok(())) => 1,
                _ => 127,
            });
            if output.stderr.is_empty() {
                output.stderr = b"exec: sqlite3: executable file not found".to_vec();
            }
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_entries_of_unknown_engines_are_skipped_not_fatal() {
        let mut sqlite = DbConnection::new("file", "localhost", DbEngine::Sqlite);
        sqlite.database = "/data/app.db".into();
        let mut future = serde_json::to_value(&sqlite).unwrap();
        future["id"] = serde_json::json!(ObjectId::new().to_string());
        future["engine"] = serde_json::json!("graphdb");
        future["options"] = serde_json::json!({"kind": "graphdb"});
        let synced = DbConnection::new("pg", "db.example.test", DbEngine::Postgresql);
        let values = vec![
            serde_json::to_value(&sqlite).unwrap(),
            future,
            serde_json::json!({"broken": true}),
            serde_json::to_value(&synced).unwrap(),
        ];
        assert_eq!(decode(&values), vec![sqlite.clone()]);
        assert!(same_id(&values[0], sqlite.id));
    }
}
