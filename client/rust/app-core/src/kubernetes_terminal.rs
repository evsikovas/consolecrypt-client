//! Pod shells reuse ConsoleCrypt's terminal sessions and never spawn a local
//! shell or materialize Kubernetes credentials.
use crate::dto::TerminalInfoDto;
use crate::{AppCore, AppError, AppResult};
use cc_k8s_core::resources::{PodTerminalInput, PodTerminalOutput, PodTerminalRequest};
use cc_ssh_core::{ShellChannel, ShellEvent, ShellInput};
use cc_terminal_core::{OpenedShell, ShellLifetime, TerminalSize, TerminalTarget};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

struct NativeShellLifetime(CancellationToken);
impl ShellLifetime for NativeShellLifetime {
    fn close(&self) {
        self.0.cancel();
    }
}
impl Drop for NativeShellLifetime {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

impl AppCore {
    pub async fn kube_terminal_open(
        &self,
        session_id: String,
        request: PodTerminalRequest,
        cols: u32,
        rows: u32,
    ) -> AppResult<TerminalInfoDto> {
        if cols == 0 || rows == 0 || cols > 65535 || rows > 65535 {
            return Err(AppError::invalid("terminal_size", "invalid_size"));
        }
        let (owner, unlocked) = self.unlocked().await?;
        let live = unlocked.kube.get(&session_id)?;
        let permit = live
            .terminal_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| AppError::invalid("kubernetes", "too_many_terminals"))?;
        let cancel = live.cancel.child_token();
        let lifetime = Arc::new(NativeShellLifetime(cancel.clone()));
        let target = TerminalTarget::Kubernetes {
            context_id: crate::dto::parse_id("context_id", &live.info.context_id)?,
            namespace: request.namespace.clone(),
            pod: request.pod.clone(),
            container: request.container.clone(),
        };
        let title = format!("{}/{}", request.namespace, request.pod);
        let api = live.resources.clone();
        let (channel, mut remote) = ShellChannel::pair(8);
        let (input, input_rx) = mpsc::channel(8);
        let (output_tx, mut output) = mpsc::channel(8);
        let (ready_tx, ready_rx) = oneshot::channel();
        let worker_cancel = cancel.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let mut ready = Some(ready_tx);
            let transport_cancel = worker_cancel.child_token();
            let transport_scope = transport_cancel.clone();
            let mut run = tokio::spawn(async move {
                api.terminal(request, input_rx, output_tx, transport_scope)
                    .await
            });
            loop {
                tokio::select! {
                    biased;
                    _ = worker_cancel.cancelled() => break,
                    event = output.recv() => match event {
                        Some(PodTerminalOutput::Connected) => {
                            if let Some(ready) = ready.take() { let _ = ready.send(Ok(())); }
                        },
                        Some(event) => {
                            if let Some(event) = shell_event(event) {
                                tokio::select! {
                                    biased;
                                    _ = worker_cancel.cancelled() => break,
                                    result = remote.events.send(event) => if result.is_err() { break; },
                                }
                            }
                        },
                        None => break,
                    },
                    result = &mut run => {
                        if let Some(ready) = ready.take() {
                            let error = result.ok().and_then(Result::err).unwrap_or(cc_k8s_core::resources::ResourceError::Closed);
                            let _ = ready.send(Err(error));
                        }
                        // Any queued output is drained below before closing.
                        break;
                    },
                    event = remote.inputs.recv() => {
                        let command = match event {
                            Some(ShellInput::Data(bytes)) => PodTerminalInput::Stdin(zeroize::Zeroizing::new(bytes.to_vec())),
                            Some(ShellInput::Resize { cols, rows }) => PodTerminalInput::Resize {
                                columns: cols.clamp(1,65535) as u16, rows: rows.clamp(1,65535) as u16,
                            },
                            Some(ShellInput::Eof) => PodTerminalInput::Eof,
                            Some(ShellInput::Close) | None => break,
                        };
                        tokio::select! {
                            biased;
                            _ = worker_cancel.cancelled() => break,
                            result = input.send(command) => if result.is_err() { break; },
                        }
                    }
                }
            }
            if !worker_cancel.is_cancelled() {
                while let Ok(event) = output.try_recv() {
                    if let Some(event) = shell_event(event) {
                        tokio::select! {
                            biased;
                            _ = worker_cancel.cancelled() => break,
                            result = remote.events.send(event) => if result.is_err() { break; },
                        }
                    }
                }
            }
            transport_cancel.cancel();
            run.abort();
            // Dropping the output sender also closes an idle terminal reader.
        });
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(AppError::invalid("kubernetes", "terminal_closed")),
            result = ready_rx => result.map_err(|_| AppError::invalid("kubernetes", "terminal_closed"))?
                .map_err(super::kubernetes_runtime::resource_error)?,
        }
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        if cancel.is_cancelled() {
            return Err(AppError::VaultLocked);
        }
        let id = unlocked
            .ssh
            .terminals
            .open_channel(
                target,
                title,
                TerminalSize { cols, rows },
                OpenedShell {
                    channel,
                    session: None,
                    lifetime: Some(lifetime),
                },
            )
            .await?;
        Ok(unlocked.ssh.terminals.info(id)?.into())
    }
}

fn shell_event(event: PodTerminalOutput) -> Option<ShellEvent> {
    match event {
        PodTerminalOutput::Connected => None,
        PodTerminalOutput::Stdout(bytes) => Some(ShellEvent::Data(bytes.into())),
        PodTerminalOutput::Stderr(bytes) => Some(ShellEvent::Stderr(bytes.into())),
        PodTerminalOutput::Exited { code } => {
            code.map(|code| ShellEvent::ExitStatus(code.max(0) as u32))
        }
    }
}
