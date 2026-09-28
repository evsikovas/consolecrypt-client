//! Facade methods for SSH: exec, terminal tabs, tunnels, SFTP and the
//! OpenSSH fallback. Connections are always planned by ssh-core's
//! `ConnectionPlanner` from vault objects (the UI never assembles SSH
//! parameters).

use crate::app::AppCore;
use crate::dto::*;
use crate::error::{AppError, AppResult};
use crate::inventory::host_of;
use crate::session::{AppCtx, Unlocked};
use cc_models::ObjectId;
use cc_terminal_core::{TerminalId, TerminalSize};
use std::path::Path;
use tokio::sync::{broadcast, mpsc};

fn parse_terminal_id(s: &str) -> AppResult<TerminalId> {
    uuid::Uuid::parse_str(s.trim()).map_err(|_| AppError::invalid("terminal_id", "not a valid id"))
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
        let (_, u) = self.unlocked().await?;
        let plan = u.ssh.plan(parse_id("host_id", &host_id)?).await?;
        Ok(ConnectionRouteDto {
            route: plan.describe(),
            warnings: plan.warnings.clone(),
        })
    }

    /// Run `command` on a host (through its jump chain) and collect output.
    pub async fn exec(&self, host_id: String, command: String) -> AppResult<ExecResultDto> {
        let (_, u) = self.unlocked().await?;
        let id = parse_id("host_id", &host_id)?;
        host_of(&u, id)?;
        u.ssh.exec(id, &command).await
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
        let (_, u) = self.unlocked().await?;
        let id = parse_id("host_id", &host_id)?;
        let plan = u.ssh.plan(id).await?;
        let tid = u
            .ssh
            .terminals
            .open(plan, TerminalSize { cols, rows })
            .await?;
        self.terminal_info(tid.to_string()).await
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
        let mut output = att.output;
        tokio::spawn(async move {
            loop {
                match output.recv().await {
                    Ok(b) => {
                        if tx.send(TerminalChunk::Data(b.to_vec())).await.is_err() {
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if tx.send(TerminalChunk::Lagged).await.is_err() {
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            let status = ssh
                .terminals
                .wait_closed(tid)
                .await
                .map(|s| TerminalStatusDto::from(&s))
                .unwrap_or(TerminalStatusDto::Closed {
                    exit_status: None,
                    reason: None,
                });
            let _ = tx.send(TerminalChunk::Closed(status)).await;
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
        start_tunnel_on(&u, parse_id("tunnel_id", &tunnel_id)?).await
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
        let (_, u) = self.unlocked().await?;
        u.ssh.sftp_open(parse_id("host_id", &host_id)?).await
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
        let (_, u) = self.unlocked().await?;
        let (session_id, program, args, env) = u
            .ssh
            .prepare_openssh(parse_id("host_id", &host_id)?, command, tty)
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
pub(crate) async fn start_tunnel_on(u: &Unlocked, id: ObjectId) -> AppResult<TunnelStatusDto> {
    let t = u
        .working()
        .tunnel(id)
        .ok_or_else(|| AppError::not_found("tunnel", id))?;
    let session = u.ssh.shared_session(t.host_id).await?;
    let status = u.ssh.tunnels.start(&t, session).await?;
    Ok((&status).into())
}

/// Start every `auto_start` tunnel that is not running yet.
pub(crate) async fn auto_start_tunnels(u: &Unlocked, ctx: &AppCtx) -> Vec<TunnelFailureDto> {
    let mut failures = Vec::new();
    for t in u.working().tunnels().into_iter().filter(|t| t.auto_start) {
        if u.ssh.tunnels.status(t.id).is_some() {
            continue;
        }
        if let Err(e) = start_tunnel_on(u, t.id).await {
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
