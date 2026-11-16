//! A WSL-origin TCP stream, not an SSH agent bridge or a listener. The selected
//! helper opens one endpoint; native ssh-core still authenticates every SSH hop.
use crate::{AutomationError, HelperLaunch, Result};
use sha2::{Digest, Sha256};
use std::{
    io,
    pin::Pin,
    process::Stdio,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadBuf};

pub struct OriginStream {
    input: Option<tokio::process::ChildStdin>,
    output: tokio::process::ChildStdout,
    child: tokio::process::Child,
    scope_digest: [u8; 32],
}

impl std::fmt::Debug for OriginStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OriginStream")
            .field("process", &self.child.id())
            .finish_non_exhaustive()
    }
}

impl OriginStream {
    pub fn scope_digest(&self) -> [u8; 32] {
        self.scope_digest
    }
}

impl AsyncRead for OriginStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.output).poll_read(cx, buffer)
    }
}

impl AsyncWrite for OriginStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.input.as_mut() {
            Some(input) => Pin::new(input).poll_write(cx, data),
            None => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "origin input closed",
            ))),
        }
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.input.as_mut() {
            Some(input) => Pin::new(input).poll_flush(cx),
            None => Poll::Ready(Ok(())),
        }
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let result = match self.input.as_mut() {
            Some(input) => Pin::new(input).poll_shutdown(cx),
            None => return Poll::Ready(Ok(())),
        };
        if result.is_ready() {
            // Windows anonymous-pipe shutdown alone does not close the handle.
            // Drop this writer now, while retaining stdout for the final EOF.
            self.input.take();
        }
        result
    }
}

impl HelperLaunch {
    pub async fn wsl_origin_stream(
        &self,
        host: &str,
        port: u16,
        environment_digest: &str,
    ) -> Result<OriginStream> {
        if !matches!(self, Self::Wsl { .. })
            || !cc_models::automation::is_digest(environment_digest)
            || host.is_empty()
            || host.len() > 253
            || host.starts_with('-')
            || port == 0
            || host
                .chars()
                .any(|c| c.is_control() || c.is_whitespace() || c == '/' || c == '@')
        {
            return Err(AutomationError::Invalid(
                "invalid explicit WSL origin or endpoint",
            ));
        }
        let mut command = self.base_command()?;
        command.args(["--tcp-stdio", "--host", host, "--port", &port.to_string()]);
        let identity = crate::project::canonical(
            serde_json::json!({"launch": self, "environment_digest": environment_digest}),
        );
        let scope_digest: [u8; 32] = Sha256::digest(
            serde_json::to_vec(&identity)
                .map_err(|_| AutomationError::Invalid("network origin serialization"))?,
        )
        .into();
        let mut command = tokio::process::Command::from(command);
        command
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        let input = child.stdin.take().ok_or(AutomationError::Closed)?;
        let output = child.stdout.take().ok_or(AutomationError::Closed)?;
        if let Some(mut errors) = child.stderr.take() {
            tokio::spawn(async move {
                let mut buffer = [0u8; 4096];
                while let Ok(count) = errors.read(&mut buffer).await {
                    if count == 0 {
                        break;
                    }
                }
            });
        }
        Ok(OriginStream {
            input: Some(input),
            output,
            child,
            scope_digest,
        })
    }
}
