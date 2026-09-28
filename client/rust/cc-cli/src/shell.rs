//! Interactive PTY session on the local terminal (raw mode) over an
//! app-core terminal, and the OpenSSH-fallback variant (system `ssh` with
//! inherited stdio, keys served by the built-in agent).

use crate::CliError;
use cc_app_core::{AppCore, TerminalChunk, TerminalStatusDto};
use std::io::{Read, Write};
use std::time::Duration;
use tokio::sync::mpsc;

struct RawMode;

impl RawMode {
    fn enable() -> Result<Self, CliError> {
        crossterm::terminal::enable_raw_mode()
            .map_err(|e| CliError::Usage(format!("cannot switch the terminal to raw mode: {e}")))?;
        Ok(Self)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

fn exit_code(st: &TerminalStatusDto) -> i32 {
    match st {
        TerminalStatusDto::Closed {
            exit_status: Some(c),
            ..
        } => *c as i32,
        TerminalStatusDto::Closed {
            reason: Some(_), ..
        }
        | TerminalStatusDto::Failed { .. } => 255,
        _ => 0,
    }
}

/// Native interactive shell. Returns the remote exit status.
pub async fn native(app: &AppCore, host_id: String) -> Result<i32, CliError> {
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let term = app
        .open_terminal(host_id, u32::from(cols), u32::from(rows))
        .await?;
    let id = term.id.clone();
    let att = app.attach_terminal(id.clone()).await?;
    let (snapshot, mut output) = (att.snapshot, att.output);
    let raw = RawMode::enable()?;
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(&snapshot);
    let _ = stdout.flush();

    let (tx, mut input) = mpsc::channel::<Vec<u8>>(64);
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin();
        let mut buf = [0u8; 4096];
        loop {
            match stdin.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let mut size = (cols, rows);
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let mut stdin_open = true;
    let status = loop {
        tokio::select! {
            chunk = output.recv() => match chunk {
                Some(TerminalChunk::Data(d)) => {
                    let _ = stdout.write_all(&d);
                    let _ = stdout.flush();
                }
                Some(TerminalChunk::Lagged) => {}
                Some(TerminalChunk::Closed(st)) => break st,
                None => break TerminalStatusDto::Closed { exit_status: None, reason: None },
            },
            data = input.recv(), if stdin_open => match data {
                Some(d) => {
                    if app.terminal_write(id.clone(), d).await.is_err() {
                        stdin_open = false;
                    }
                }
                None => stdin_open = false,
            },
            _ = tick.tick() => {
                if let Ok(s) = crossterm::terminal::size() {
                    if s != size {
                        size = s;
                        let _ = app.terminal_resize(id.clone(), u32::from(s.0), u32::from(s.1)).await;
                    }
                }
            }
        }
    };
    drop(raw);
    let _ = app.close_terminal(id).await;
    if let TerminalStatusDto::Failed { message } = &status {
        eprintln!("cc: session failed: {message}");
    }
    Ok(exit_code(&status))
}

/// OpenSSH fallback: run the prepared `ssh` with inherited stdio.
pub async fn openssh(
    app: &AppCore,
    host_id: String,
    command: Option<String>,
    tty: bool,
) -> Result<i32, CliError> {
    let prepared = app.prepare_openssh_session(host_id, command, tty).await?;
    let mut cmd = tokio::process::Command::new(&prepared.program);
    cmd.args(&prepared.args);
    for e in &prepared.env {
        cmd.env(&e.name, &e.value);
    }
    let status = cmd
        .status()
        .await
        .map_err(|e| CliError::Usage(format!("cannot run {}: {e}", prepared.program)))?;
    app.finish_openssh_session(prepared.session_id).await?;
    Ok(status.code().unwrap_or(255))
}
