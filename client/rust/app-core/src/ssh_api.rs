//! Facade methods for SSH: exec, terminal tabs, tunnels, SFTP and the
//! OpenSSH fallback. Connections are always planned by ssh-core's
//! `ConnectionPlanner` from vault objects (the UI never assembles SSH
//! parameters).

use crate::app::AppCore;
use crate::dto::*;
use crate::error::{AppError, AppResult};
use crate::inventory::host_of;
use crate::session::{AppCtx, Unlocked};
use cc_terminal_core::{TerminalError, TerminalId, TerminalSize, TerminalStatus};
use std::{future::Future, path::Path};
use tokio::sync::{broadcast, mpsc};

fn parse_terminal_id(s: &str) -> AppResult<TerminalId> {
    uuid::Uuid::parse_str(s.trim()).map_err(|_| AppError::invalid("terminal_id", "not a valid id"))
}

/// The manager retains closed sessions for scrollback, including their output
/// sender. A terminal's final status, rather than broadcast closure, therefore
/// ends an attachment. Dropping the consumer also releases an idle attachment.
async fn forward_terminal_output(
    mut output: broadcast::Receiver<bytes::Bytes>,
    closed: impl Future<Output = Result<TerminalStatus, TerminalError>>,
    tx: mpsc::Sender<TerminalChunk>,
) {
    tokio::pin!(closed);
    let mut output_open = true;
    let status = loop {
        tokio::select! {
            biased;
            _ = tx.closed() => return,
            status = &mut closed => break status,
            next = output.recv(), if output_open => {
                let chunk = match next {
                    Ok(bytes) => TerminalChunk::Data(bytes.to_vec()),
                    Err(broadcast::error::RecvError::Lagged(_)) => TerminalChunk::Lagged,
                    Err(broadcast::error::RecvError::Closed) => {
                        output_open = false;
                        continue;
                    }
                };
                if tx.send(chunk).await.is_err() {
                    return;
                }
            }
        }
    };

    // The pump publishes its final status after its last output. Preserve any
    // queued bytes (and a lag notice) before sending exactly one final chunk.
    loop {
        let chunk = match output.try_recv() {
            Ok(bytes) => TerminalChunk::Data(bytes.to_vec()),
            Err(broadcast::error::TryRecvError::Lagged(_)) => TerminalChunk::Lagged,
            Err(broadcast::error::TryRecvError::Empty | broadcast::error::TryRecvError::Closed) => {
                break;
            }
        };
        if tx.send(chunk).await.is_err() {
            return;
        }
    }
    let status = status
        .map(|s| TerminalStatusDto::from(&s))
        .unwrap_or(TerminalStatusDto::Closed {
            exit_status: None,
            reason: None,
        });
    let _ = tx.send(TerminalChunk::Closed(status)).await;
}

/// A running OpenSSH invocation prepared for the caller to spawn with
/// inherited stdio (CLI interactive fallback).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenSshCommandDto {
    pub session_id: String,
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<EnvVarDto>,
}

impl AppCore {
    /// Human-readable route (`user@hop -> … -> user@target`) and warnings.
    pub async fn describe_connection(&self, host_id: String) -> AppResult<ConnectionRouteDto> {
        let prepared = self.sharing_prepare_connection(&host_id).await?;
        let plan = prepared.plan;
        Ok(ConnectionRouteDto {
            route: plan.describe(),
            warnings: plan.warnings.clone(),
        })
    }

    /// Run `command` on a host (through its jump chain) and collect output.
    pub async fn exec(&self, host_id: String, command: String) -> AppResult<ExecResultDto> {
        let prepared = self.sharing_prepare_connection(&host_id).await?;
        prepared
            .run(prepared.unlocked.ssh.exec_plan(&prepared.plan, &command))
            .await
    }

    // ---- terminals -------------------------------------------------------------

    /// Open an interactive terminal (PTY shell) to a host. Returns once
    /// connected (or with the connect error).
    pub async fn open_terminal(
        &self,
        host_id: String,
        cols: u32,
        rows: u32,
    ) -> AppResult<TerminalInfoDto> {
        let prepared = self.sharing_prepare_connection(&host_id).await?;
        let tid = prepared
            .run(async {
                Ok(prepared
                    .unlocked
                    .ssh
                    .terminals
                    .open(prepared.plan.clone(), TerminalSize { cols, rows })
                    .await?)
            })
            .await?;
        let i = prepared.unlocked.ssh.terminals.info(tid)?;
        Ok(TerminalInfoDto {
            id: i.id.to_string(),
            host_id: i.host_id.to_string(),
            title: i.title,
            status: (&i.status).into(),
            cols: i.size.cols,
            rows: i.size.rows,
        })
    }

    pub async fn terminal_info(&self, terminal_id: String) -> AppResult<TerminalInfoDto> {
        let (_, u) = self.unlocked().await?;
        let i = u.ssh.terminals.info(parse_terminal_id(&terminal_id)?)?;
        Ok(TerminalInfoDto {
            id: i.id.to_string(),
            host_id: i.host_id.to_string(),
            title: i.title,
            status: (&i.status).into(),
            cols: i.size.cols,
            rows: i.size.rows,
        })
    }

    pub async fn list_terminals(&self) -> AppResult<Vec<TerminalInfoDto>> {
        let (_, u) = self.unlocked().await?;
        Ok(u.ssh
            .terminals
            .list()
            .into_iter()
            .map(|i| TerminalInfoDto {
                id: i.id.to_string(),
                host_id: i.host_id.to_string(),
                title: i.title,
                status: (&i.status).into(),
                cols: i.size.cols,
                rows: i.size.rows,
            })
            .collect())
    }

    /// Send keyboard input.
    pub async fn terminal_write(&self, terminal_id: String, data: Vec<u8>) -> AppResult<()> {
        let (_, u) = self.unlocked().await?;
        Ok(u.ssh
            .terminals
            .write(parse_terminal_id(&terminal_id)?, &data)
            .await?)
    }

    pub async fn terminal_resize(
        &self,
        terminal_id: String,
        cols: u32,
        rows: u32,
    ) -> AppResult<()> {
        let (_, u) = self.unlocked().await?;
        Ok(u.ssh
            .terminals
            .resize(
                parse_terminal_id(&terminal_id)?,
                TerminalSize { cols, rows },
            )
            .await?)
    }

    /// Attach to a terminal's output: returns the local scrollback snapshot
    /// and a stream of chunks ending with [`TerminalChunk::Closed`].
    pub async fn attach_terminal(&self, terminal_id: String) -> AppResult<TerminalAttachment> {
        let (_, u) = self.unlocked().await?;
        let tid = parse_terminal_id(&terminal_id)?;
        let att = u.ssh.terminals.attach(tid)?;
        let (tx, rx) = mpsc::channel(256);
        let ssh = u.ssh.clone();
        tokio::spawn(async move {
            forward_terminal_output(att.output, ssh.terminals.wait_closed(tid), tx).await;
        });
        Ok(TerminalAttachment {
            snapshot: att.snapshot,
            output: rx,
        })
    }

    /// Wait until the terminal ends.
    pub async fn terminal_wait_closed(&self, terminal_id: String) -> AppResult<TerminalStatusDto> {
        let (_, u) = self.unlocked().await?;
        let s = u
            .ssh
            .terminals
            .wait_closed(parse_terminal_id(&terminal_id)?)
            .await?;
        Ok((&s).into())
    }

    /// Close a terminal and forget its (local-only) scrollback.
    pub async fn close_terminal(&self, terminal_id: String) -> AppResult<()> {
        let (_, u) = self.unlocked().await?;
        let tid = parse_terminal_id(&terminal_id)?;
        let _ = u.ssh.terminals.close(tid).await;
        Ok(u.ssh.terminals.remove(tid).await?)
    }

    /// Best-effort last command / error of a terminal (AI context hooks).
    pub async fn terminal_last_command(&self, terminal_id: String) -> AppResult<Option<String>> {
        let (_, u) = self.unlocked().await?;
        Ok(u.ssh
            .terminals
            .last_command(parse_terminal_id(&terminal_id)?))
    }

    // ---- tunnels ---------------------------------------------------------------

    /// Start a saved tunnel over a (shared) connection to its host.
    pub async fn start_tunnel(&self, tunnel_id: String) -> AppResult<TunnelStatusDto> {
        let (_, u) = self.unlocked().await?;
        let id = parse_id("tunnel_id", &tunnel_id)?;
        let tunnel = u
            .working()
            .tunnel(id)
            .ok_or_else(|| AppError::not_found("tunnel", id))?;
        let current = self
            .sharing_prepare_connection(&tunnel.host_id.to_string())
            .await?;
        if !std::sync::Arc::ptr_eq(&u, &current.unlocked) {
            return Err(AppError::invalid("shared_host", "profile changed"));
        }
        current
            .run(start_tunnel_plan(&current.unlocked, &tunnel, &current.plan))
            .await
    }

    pub async fn stop_tunnel(&self, tunnel_id: String) -> AppResult<()> {
        let (_, u) = self.unlocked().await?;
        Ok(u.ssh
            .tunnels
            .stop(parse_id("tunnel_id", &tunnel_id)?)
            .await?)
    }

    pub async fn tunnel_statuses(&self) -> AppResult<Vec<TunnelStatusDto>> {
        let (_, u) = self.unlocked().await?;
        Ok(u.ssh.tunnels.list().iter().map(Into::into).collect())
    }

    /// Start every tunnel with `auto_start` (errors are reported per tunnel
    /// via [`AppEvent::TunnelFailed`] and returned as (id, message) pairs).
    pub async fn start_auto_tunnels(&self) -> AppResult<Vec<TunnelFailureDto>> {
        let (_, u) = self.unlocked().await?;
        Ok(auto_start_tunnels(&u, &self.inner.ctx).await)
    }

    // ---- sftp ------------------------------------------------------------------

    /// Open an SFTP session to a host; returns its id.
    pub async fn sftp_open(&self, host_id: String) -> AppResult<String> {
        let prepared = self.sharing_prepare_connection(&host_id).await?;
        prepared
            .run(prepared.unlocked.ssh.sftp_open_plan(&prepared.plan))
            .await
    }

    /// Close an SFTP session: its edit sessions are stopped first (final
    /// upload; on conflict / failure the working copy is kept as a
    /// leftover) and its transfer jobs cancelled.
    pub async fn sftp_close(&self, sftp_id: String) -> AppResult<()> {
        let (_, u) = self.unlocked().await?;
        u.edit.stop_for_sftp(&sftp_id).await;
        u.transfers.cancel_for_session(&sftp_id);
        u.ssh.sftp_close(&sftp_id).await
    }

    pub async fn sftp_home(&self, sftp_id: String) -> AppResult<String> {
        let (_, u) = self.unlocked().await?;
        Ok(u.ssh.sftp(&sftp_id).await?.home_dir().await?)
    }

    pub async fn sftp_list(&self, sftp_id: String, path: String) -> AppResult<Vec<RemoteEntryDto>> {
        let (_, u) = self.unlocked().await?;
        u.ssh.sftp_list(&sftp_id, &path).await
    }

    pub async fn sftp_mkdir(&self, sftp_id: String, path: String) -> AppResult<()> {
        let (_, u) = self.unlocked().await?;
        Ok(u.ssh.sftp(&sftp_id).await?.mkdir_all(&path).await?)
    }

    pub async fn sftp_rename(&self, sftp_id: String, from: String, to: String) -> AppResult<()> {
        let (_, u) = self.unlocked().await?;
        Ok(u.ssh.sftp(&sftp_id).await?.rename(&from, &to).await?)
    }

    pub async fn sftp_remove(
        &self,
        sftp_id: String,
        path: String,
        recursive: bool,
    ) -> AppResult<()> {
        let (_, u) = self.unlocked().await?;
        let c = u.ssh.sftp(&sftp_id).await?;
        let e = c.lstat(&path).await?;
        if e.is_dir() {
            Ok(c.remove_dir(&path, recursive).await?)
        } else {
            Ok(c.remove_file(&path).await?)
        }
    }

    pub async fn sftp_chmod(&self, sftp_id: String, path: String, mode: u32) -> AppResult<()> {
        let (_, u) = self.unlocked().await?;
        Ok(u.ssh.sftp(&sftp_id).await?.chmod(&path, mode).await?)
    }

    fn progress(&self, transfer_id: String) -> impl FnMut(cc_sftp_core::TransferProgress) + Send {
        let ctx = self.inner.ctx.clone();
        move |p| {
            ctx.emit(AppEvent::TransferProgress {
                transfer_id: transfer_id.clone(),
                transferred: p.transferred,
                total: p.total,
            })
        }
    }

    /// Upload a local file (progress via [`AppEvent::TransferProgress`]).
    pub async fn sftp_upload(
        &self,
        sftp_id: String,
        local_path: String,
        remote_path: String,
    ) -> AppResult<TransferSummaryDto> {
        let (_, u) = self.unlocked().await?;
        let c = u.ssh.sftp(&sftp_id).await?;
        let transfer_id = uuid::Uuid::new_v4().to_string();
        let mut progress = self.progress(transfer_id.clone());
        let s = c
            .upload(
                Path::new(&local_path),
                &remote_path,
                &cc_sftp_core::TransferOptions::default(),
                &mut progress,
                &cc_sftp_core::CancelToken::new(),
            )
            .await?;
        Ok(TransferSummaryDto {
            transfer_id,
            bytes: s.bytes,
            elapsed_ms: s.elapsed_ms,
        })
    }

    /// Download a remote file to a local path.
    pub async fn sftp_download(
        &self,
        sftp_id: String,
        remote_path: String,
        local_path: String,
    ) -> AppResult<TransferSummaryDto> {
        let (_, u) = self.unlocked().await?;
        let c = u.ssh.sftp(&sftp_id).await?;
        let transfer_id = uuid::Uuid::new_v4().to_string();
        let mut progress = self.progress(transfer_id.clone());
        let s = c
            .download(
                &remote_path,
                Path::new(&local_path),
                &cc_sftp_core::TransferOptions::default(),
                &mut progress,
                &cc_sftp_core::CancelToken::new(),
            )
            .await?;
        Ok(TransferSummaryDto {
            transfer_id,
            bytes: s.bytes,
            elapsed_ms: s.elapsed_ms,
        })
    }

    // ---- OpenSSH fallback (CLI) ------------------------------------------------

    /// Prepare a system-OpenSSH invocation for a host (keys served by a
    /// per-session built-in agent; never written to disk). Spawn it with
    /// inherited stdio, then call [`AppCore::finish_openssh_session`].
    pub async fn prepare_openssh_session(
        &self,
        host_id: String,
        command: Option<String>,
        tty: bool,
    ) -> AppResult<OpenSshCommandDto> {
        let prepared = self.sharing_prepare_connection(&host_id).await?;
        let (session_id, program, args, env) = prepared
            .run(
                prepared
                    .unlocked
                    .ssh
                    .prepare_openssh_plan(&prepared.plan, command, tty),
            )
            .await?;
        Ok(OpenSshCommandDto {
            session_id,
            program,
            args,
            env: env
                .into_iter()
                .map(|(name, value)| EnvVarDto { name, value })
                .collect(),
        })
    }

    /// Store host keys OpenSSH learned and stop the session's agent.
    pub async fn finish_openssh_session(&self, session_id: String) -> AppResult<()> {
        let (_, u) = self.unlocked().await?;
        u.ssh.finish_openssh(&session_id).await
    }

    /// Whether a host uses the OpenSSH backend.
    pub async fn host_uses_openssh(&self, host_id: String) -> AppResult<bool> {
        let (_, u) = self.unlocked().await?;
        let h = host_of(&u, parse_id("host_id", &host_id)?)?;
        Ok(h.backend == SshBackend::OpenSsh)
    }
}

/// Start one saved tunnel of the unlocked vault.
pub(crate) async fn start_tunnel_plan(
    u: &Unlocked,
    t: &cc_models::tunnel::Tunnel,
    plan: &cc_ssh_core::ConnectionPlan,
) -> AppResult<TunnelStatusDto> {
    if t.host_id != plan.host_id {
        return Err(AppError::invalid(
            "tunnel",
            "host changed during verification",
        ));
    }
    let session = u.ssh.shared_session_plan(plan).await?;
    let status = u.ssh.tunnels.start(t, session).await?;
    Ok((&status).into())
}

/// Start every `auto_start` tunnel that is not running yet.
pub(crate) async fn auto_start_tunnels(
    u: &std::sync::Arc<Unlocked>,
    ctx: &AppCtx,
) -> Vec<TunnelFailureDto> {
    let mut failures = Vec::new();
    for t in u.working().tunnels().into_iter().filter(|t| t.auto_start) {
        if u.ssh.tunnels.status(t.id).is_some() {
            continue;
        }
        // Session startup has no trusted sharing transcript yet. Require the
        // explicit start path which verifies current ACL/history before SSH.
        let result = async {
            let plan = u.ssh.plan(t.host_id).await?;
            for hop in plan.all_hops() {
                let host = host_of(u, hop.host_id)?;
                if crate::sharing_bindings::host_sharing_binding(&host)?.is_some() {
                    return Err(AppError::invalid(
                        "auto_start",
                        "shared_host_requires_verification",
                    ));
                }
            }
            let prepared = crate::sharing_bindings::PreparedSharingConnection {
                unlocked: u.clone(),
                plan,
            };
            prepared.run(start_tunnel_plan(u, &t, &prepared.plan)).await
        }
        .await;
        if let Err(e) = result {
            tracing::warn!(tunnel_id = %t.id, error = %e, "auto-start tunnel failed");
            ctx.emit(AppEvent::TunnelFailed {
                tunnel_id: t.id.to_string(),
                reason: e.to_string(),
            });
            failures.push(TunnelFailureDto {
                tunnel_id: t.id.to_string(),
                message: e.to_string(),
            });
        }
    }
    failures
}

#[cfg(test)]
mod attachment_tests {
    use super::*;
    use async_trait::async_trait;
    use bytes::Bytes;
    use cc_models::{host::HostKeyPolicy, ObjectId};
    use cc_ssh_core::{ConnectionPlan, Endpoint, PtyRequest, ShellChannel, ShellEvent, ShellInput};
    use cc_terminal_core::{OpenedShell, ShellOpener, TerminalManager};
    use std::{sync::Arc, time::Duration};
    use tokio::{task::JoinHandle, time::timeout};

    struct MockShellOpener {
        fail: bool,
    }

    #[async_trait]
    impl ShellOpener for MockShellOpener {
        async fn open_shell(
            &self,
            _plan: &ConnectionPlan,
            _pty: PtyRequest,
        ) -> Result<OpenedShell, TerminalError> {
            if self.fail {
                return Err(TerminalError::Open("mock connection failed".into()));
            }
            let (channel, mut remote) = ShellChannel::pair(64);
            tokio::spawn(async move {
                while let Some(input) = remote.inputs.recv().await {
                    match input {
                        ShellInput::Data(bytes) if bytes.as_ref() == b"finish" => {
                            for bytes in
                                [b"first\r\n".as_slice(), "последняя строка\r\n".as_bytes()]
                            {
                                if remote
                                    .events
                                    .send(ShellEvent::Data(Bytes::copy_from_slice(bytes)))
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                            }
                            let _ = remote.events.send(ShellEvent::ExitStatus(0)).await;
                            let _ = remote.events.send(ShellEvent::Closed).await;
                            return;
                        }
                        ShellInput::Data(bytes) => {
                            if remote.events.send(ShellEvent::Data(bytes)).await.is_err() {
                                return;
                            }
                        }
                        ShellInput::Close => {
                            let _ = remote.events.send(ShellEvent::Closed).await;
                            return;
                        }
                        ShellInput::Resize { .. } | ShellInput::Eof => {}
                    }
                }
            });
            Ok(OpenedShell {
                channel,
                session: None,
            })
        }
    }

    fn manager(fail: bool) -> Arc<TerminalManager> {
        Arc::new(TerminalManager::new(Arc::new(MockShellOpener { fail })))
    }

    fn plan() -> ConnectionPlan {
        ConnectionPlan {
            host_id: ObjectId::new(),
            name: "generated attachment test".into(),
            target: Endpoint::new("127.0.0.1", 22),
            username: "test".into(),
            credential: None,
            host_key_policy: HostKeyPolicy::Ask,
            route: vec![],
            proxy: None,
            forwards: vec![],
            backend: cc_models::host::SshBackend::Native,
            keepalive_secs: Some(2),
            agent_forwarding: false,
            warnings: vec![],
        }
    }

    fn forward(
        manager: Arc<TerminalManager>,
        id: TerminalId,
        output: broadcast::Receiver<Bytes>,
        capacity: usize,
    ) -> (mpsc::Receiver<TerminalChunk>, JoinHandle<()>) {
        let (tx, rx) = mpsc::channel(capacity);
        let task = tokio::spawn(async move {
            forward_terminal_output(output, manager.wait_closed(id), tx).await;
        });
        (rx, task)
    }

    async fn collect(
        mut rx: mpsc::Receiver<TerminalChunk>,
        task: JoinHandle<()>,
    ) -> Vec<TerminalChunk> {
        timeout(Duration::from_secs(2), async {
            let mut chunks = Vec::new();
            while let Some(chunk) = rx.recv().await {
                chunks.push(chunk);
            }
            task.await.unwrap();
            chunks
        })
        .await
        .expect("attachment must finish while the manager retains the session")
    }

    fn finished_output() -> Vec<TerminalChunk> {
        vec![
            TerminalChunk::Data(b"first\r\n".to_vec()),
            TerminalChunk::Data("последняя строка\r\n".as_bytes().to_vec()),
            TerminalChunk::Closed(TerminalStatusDto::Closed {
                exit_status: Some(0),
                reason: None,
            }),
        ]
    }

    #[tokio::test]
    async fn attachment_reports_live_close_without_removing_the_session() {
        let manager = manager(false);
        let id = manager.open(plan(), TerminalSize::default()).await.unwrap();
        let attachment = manager.attach(id).unwrap();
        let (rx, task) = forward(manager.clone(), id, attachment.output, 1);
        manager.write(id, b"finish").await.unwrap();

        assert_eq!(collect(rx, task).await, finished_output());
        assert_eq!(manager.list().len(), 1);
        assert!(manager.status(id).unwrap().is_terminal());
        let mut retained = manager.attach(id).unwrap();
        assert_eq!(
            retained.output.try_recv(),
            Err(broadcast::error::TryRecvError::Empty),
            "the retained sender is still open"
        );
    }

    #[tokio::test]
    async fn attachment_drains_queued_bytes_before_one_final_chunk() {
        let manager = manager(false);
        let id = manager.open(plan(), TerminalSize::default()).await.unwrap();
        let attachment = manager.attach(id).unwrap();
        manager.write(id, b"finish").await.unwrap();
        timeout(Duration::from_secs(2), manager.wait_closed(id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(attachment.output.len(), 2);

        let (rx, task) = forward(manager.clone(), id, attachment.output, 1);
        assert_eq!(collect(rx, task).await, finished_output());
        assert_eq!(manager.list().len(), 1);
    }

    #[tokio::test]
    async fn attachment_reports_retained_failed_connection() {
        let manager = manager(true);
        // spawn_open preserves a failed tab so it can offer reconnect.
        let id = manager.spawn_open(plan(), TerminalSize::default());
        let attachment = manager.attach(id).unwrap();
        let status = timeout(Duration::from_secs(2), manager.wait_closed(id))
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(status, TerminalStatus::Failed(_)));
        let (rx, task) = forward(manager.clone(), id, attachment.output, 1);

        assert_eq!(
            collect(rx, task).await,
            vec![TerminalChunk::Closed((&status).into())]
        );
        assert_eq!(manager.list().len(), 1);
    }

    #[tokio::test]
    async fn late_attachment_reports_close_without_duplicating_its_snapshot() {
        let manager = manager(false);
        let id = manager.open(plan(), TerminalSize::default()).await.unwrap();
        manager.write(id, b"finish").await.unwrap();
        let status = timeout(Duration::from_secs(2), manager.wait_closed(id))
            .await
            .unwrap()
            .unwrap();
        let attachment = manager.attach(id).unwrap();
        assert_eq!(
            attachment.snapshot,
            "first\r\nпоследняя строка\r\n".as_bytes()
        );
        let (rx, task) = forward(manager.clone(), id, attachment.output, 1);

        assert_eq!(
            collect(rx, task).await,
            vec![TerminalChunk::Closed((&status).into())]
        );
    }

    #[tokio::test]
    async fn dropping_idle_attachment_stops_its_task_without_closing_the_shell() {
        let manager = manager(false);
        let id = manager.open(plan(), TerminalSize::default()).await.unwrap();
        let attachment = manager.attach(id).unwrap();
        let (rx, task) = forward(manager.clone(), id, attachment.output, 1);
        tokio::task::yield_now().await;
        drop(rx);
        timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(manager.status(id).unwrap(), TerminalStatus::Connected);
        manager.close(id).await.unwrap();
        timeout(Duration::from_secs(2), manager.wait_closed(id))
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn dropping_backpressured_attachment_stops_its_task() {
        let manager = manager(false);
        let id = manager.open(plan(), TerminalSize::default()).await.unwrap();
        let attachment = manager.attach(id).unwrap();
        let (rx, task) = forward(manager.clone(), id, attachment.output, 1);
        manager.write(id, b"echo").await.unwrap();
        manager.write(id, b"echo").await.unwrap();
        timeout(Duration::from_secs(2), async {
            while rx.len() != 1 || manager.scrollback_tail(id, 8).unwrap().len() != 8 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        drop(rx);
        timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(manager.status(id).unwrap(), TerminalStatus::Connected);
        manager.close(id).await.unwrap();
        timeout(Duration::from_secs(2), manager.wait_closed(id))
            .await
            .unwrap()
            .unwrap();
    }
}
