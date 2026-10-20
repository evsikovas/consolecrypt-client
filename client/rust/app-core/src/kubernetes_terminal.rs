//! Pod shells reuse ConsoleCrypt's terminal sessions and never spawn a local
//! shell or materialize Kubernetes credentials.
use crate::dto::TerminalInfoDto;
use crate::{AppCore, AppError, AppResult};
use cc_k8s_core::resources::{
    PodTerminalInput, PodTerminalOutput, PodTerminalRequest, ResourceApi, ResourceError,
};
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
        mut request: PodTerminalRequest,
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
        if request.command.as_deref() == Some(&["/__consolecrypt_auto_shell__".into()]) {
            request.command = Some(vec![detect_shell(&live.resources, &request, &cancel)
                .await
                .map_err(super::kubernetes_runtime::resource_error)?]);
        }
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

/// Only these literal argv vectors reach the remote exec endpoint. Importing a
/// context cannot trigger discovery, and every probe traverses the same native
/// authorization/production policy as the final interactive terminal.
async fn detect_shell(
    api: &ResourceApi,
    request: &PodTerminalRequest,
    cancel: &CancellationToken,
) -> Result<String, ResourceError> {
    for shell in ["/bin/bash", "/bin/ash", "/bin/sh"] {
        let mut probe = request.clone();
        probe.tty = false;
        probe.command = Some(vec![
            shell.into(),
            "-c".into(),
            "printf CC_SHELL_READY".into(),
        ]);
        let (input, input_rx) = mpsc::channel(1);
        // A probe needs no input and must never leave a remote stdin open.
        drop(input);
        let (output_tx, output) = mpsc::channel(8);
        let scope = cancel.child_token();
        let api = api.clone();
        let transport_scope = scope.clone();
        let mut run = tokio::spawn(async move {
            api.terminal(probe, input_rx, output_tx, transport_scope)
                .await
        });
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(ResourceError::Closed),
            result = tokio::time::timeout(std::time::Duration::from_secs(5), collect_probe(output)) =>
                result.map_err(|_| ResourceError::Timeout).and_then(|value|value),
        };
        scope.cancel();
        // No unobserved task may retain an authorization/session after return.
        let transport =
            match tokio::time::timeout(std::time::Duration::from_secs(1), &mut run).await {
                Ok(result) => result.ok(),
                Err(_) => {
                    run.abort();
                    let _ = run.await;
                    None
                }
            };
        if cancel.is_cancelled() {
            return Err(ResourceError::Closed);
        }
        if matches!(
            transport,
            Some(Err(ResourceError::Forbidden
                | ResourceError::Unauthorized
                | ResourceError::Closed))
        ) {
            return transport
                .expect("matched transport error")
                .map(|()| String::new());
        }
        match result {
            Ok(true) => return Ok(shell.into()),
            Err(ResourceError::Closed) => return Err(ResourceError::Closed),
            _ => {}
        }
    }
    Err(ResourceError::Unavailable)
}

async fn collect_probe(
    mut output: mpsc::Receiver<PodTerminalOutput>,
) -> Result<bool, ResourceError> {
    let mut text = zeroize::Zeroizing::new(Vec::new());
    let mut total = 0usize;
    while let Some(event) = output.recv().await {
        match event {
            PodTerminalOutput::Connected => {}
            PodTerminalOutput::Stdout(bytes) => {
                let bytes = zeroize::Zeroizing::new(bytes);
                total = total.saturating_add(bytes.len());
                if total > 8192 {
                    return Err(ResourceError::TooLarge);
                }
                text.extend_from_slice(&bytes);
            }
            PodTerminalOutput::Stderr(bytes) => {
                let bytes = zeroize::Zeroizing::new(bytes);
                total = total.saturating_add(bytes.len());
                if total > 8192 {
                    return Err(ResourceError::TooLarge);
                }
            }
            PodTerminalOutput::Exited { code } => {
                return Ok(code == Some(0) && text.as_slice() == b"CC_SHELL_READY");
            }
        }
    }
    Ok(false)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn auto_shell_requires_exact_marker_and_successful_remote_exit() {
        for (stdout, exit, expected) in [
            ("CC_SHELL_READY", Some(0), true),
            ("CC_SHELL_READY", Some(127), false),
            ("CC_SHELL_READY", None, false),
            ("CC_SHELL_READY injected", Some(0), false),
        ] {
            let (tx, rx) = mpsc::channel(8);
            tx.send(PodTerminalOutput::Connected).await.unwrap();
            tx.send(PodTerminalOutput::Stdout(stdout.as_bytes().to_vec()))
                .await
                .unwrap();
            tx.send(PodTerminalOutput::Exited { code: exit })
                .await
                .unwrap();
            assert_eq!(collect_probe(rx).await.unwrap(), expected);
        }
        let (tx, rx) = mpsc::channel(8);
        tx.send(PodTerminalOutput::Connected).await.unwrap();
        drop(tx);
        assert!(!collect_probe(rx).await.unwrap());
        let (tx, rx) = mpsc::channel(8);
        tx.send(PodTerminalOutput::Stderr(vec![b'x'; 8193]))
            .await
            .unwrap();
        assert!(matches!(
            collect_probe(rx).await,
            Err(ResourceError::TooLarge)
        ));
    }
}
