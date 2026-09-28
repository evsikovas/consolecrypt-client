//! Terminals (PTY shells), tunnel runtime and SFTP (+ queued transfers with
//! progress and cancel). Connections are always planned by app-core from
//! vault objects; the UI passes host ids only.

use crate::api::error::BridgeError;
use crate::frb_generated::StreamSink;
use crate::state::{self, to_json, with_core};
use cc_app_core::{
    AppCore, AppEvent, TerminalChunk, TerminalStatusDto, TransferDirectionDto, TransferJobDto,
    TransferRequestDto, TransferStateDto,
};
use tokio::sync::broadcast::error::RecvError;

// ---- terminals ---------------------------------------------------------------------

/// Kind of a [`TerminalFrame`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalFrameKind {
    /// Output bytes (the first frame is the local scrollback snapshot).
    Data,
    /// Output was dropped (slow consumer); re-attach for a fresh snapshot.
    Lagged,
    /// The session ended (`exit_status`, `message` = reason). Last frame.
    Closed,
    /// The session failed (`message`). Last frame.
    Failed,
}

/// One item of a terminal output stream (snapshot first, then live).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalFrame {
    pub kind: TerminalFrameKind,
    pub data: Vec<u8>,
    pub exit_status: Option<u32>,
    pub message: Option<String>,
}

impl TerminalFrame {
    fn data(data: Vec<u8>) -> Self {
        Self {
            kind: TerminalFrameKind::Data,
            data,
            exit_status: None,
            message: None,
        }
    }

    fn end(status: &TerminalStatusDto) -> Self {
        let (kind, exit_status, message) = match status {
            TerminalStatusDto::Failed { message } => {
                (TerminalFrameKind::Failed, None, Some(message.clone()))
            }
            TerminalStatusDto::Closed {
                exit_status,
                reason,
            } => (TerminalFrameKind::Closed, *exit_status, reason.clone()),
            // Not an end state; treat as a plain close.
            TerminalStatusDto::Connecting | TerminalStatusDto::Connected => {
                (TerminalFrameKind::Closed, None, None)
            }
        };
        Self {
            kind,
            data: Vec::new(),
            exit_status,
            message,
        }
    }

    pub(crate) fn from_chunk(chunk: TerminalChunk) -> Self {
        match chunk {
            TerminalChunk::Data(d) => Self::data(d),
            TerminalChunk::Lagged => Self {
                kind: TerminalFrameKind::Lagged,
                data: Vec::new(),
                exit_status: None,
                message: None,
            },
            TerminalChunk::Closed(s) => Self::end(&s),
        }
    }
}

/// Open a PTY shell to a host; returns once connected (host-key and
/// password prompts arrive on `core_prompts` meanwhile) → `TerminalInfoDto`.
pub async fn terminal_open(host_id: String, cols: u32, rows: u32) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.open_terminal(host_id, cols, rows).await?) }).await
}

/// Output of a terminal: the scrollback snapshot as the first `Data` frame,
/// then live output, ending with `Closed` / `Failed`.
pub async fn terminal_attach(
    terminal_id: String,
    sink: StreamSink<TerminalFrame>,
) -> Result<(), BridgeError> {
    let core = state::core()?;
    let attachment = state::run(async move {
        core.attach_terminal(terminal_id)
            .await
            .map_err(BridgeError::from)
    })
    .await?;
    let mut output = attachment.output;
    if sink.add(TerminalFrame::data(attachment.snapshot)).is_err() {
        return Ok(());
    }
    state::runtime().spawn(async move {
        while let Some(chunk) = output.recv().await {
            let end = matches!(chunk, TerminalChunk::Closed(_));
            if sink.add(TerminalFrame::from_chunk(chunk)).is_err() || end {
                break;
            }
        }
    });
    Ok(())
}

/// Keyboard / paste input (UTF-8 bytes).
pub async fn terminal_write(terminal_id: String, data: Vec<u8>) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.terminal_write(terminal_id, data).await?) }).await
}

pub async fn terminal_resize(terminal_id: String, cols: u32, rows: u32) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.terminal_resize(terminal_id, cols, rows).await?) }).await
}

/// Close the channel and forget the (local-only) scrollback.
pub async fn terminal_close(terminal_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.close_terminal(terminal_id).await?) }).await
}

/// `TerminalInfoDto`.
pub async fn terminal_info(terminal_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.terminal_info(terminal_id).await?) }).await
}

// ---- tunnels -----------------------------------------------------------------------

/// Start a saved tunnel → `TunnelStatusDto`.
pub async fn tunnel_start(tunnel_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.start_tunnel(tunnel_id).await?) }).await
}

pub async fn tunnel_stop(tunnel_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.stop_tunnel(tunnel_id).await?) }).await
}

/// `Vec<TunnelStatusDto>` of running / failed tunnels (empty while locked).
pub async fn tunnel_statuses() -> Result<String, BridgeError> {
    with_core(move |c| async move {
        match c.tunnel_statuses().await {
            Ok(v) => to_json(&v),
            Err(cc_app_core::AppError::VaultLocked)
            | Err(cc_app_core::AppError::NoActiveProfile) => Ok("[]".to_owned()),
            Err(e) => Err(e.into()),
        }
    })
    .await
}

// ---- sftp --------------------------------------------------------------------------

/// Open an SFTP session → its id.
pub async fn sftp_open(host_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { Ok(c.sftp_open(host_id).await?) }).await
}

pub async fn sftp_close(sftp_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.sftp_close(sftp_id).await?) }).await
}

pub async fn sftp_home(sftp_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { Ok(c.sftp_home(sftp_id).await?) }).await
}

/// `Vec<RemoteEntryDto>`.
pub async fn sftp_list(sftp_id: String, path: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.sftp_list(sftp_id, path).await?) }).await
}

pub async fn sftp_mkdir(sftp_id: String, path: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.sftp_mkdir(sftp_id, path).await?) }).await
}

pub async fn sftp_rename(sftp_id: String, from: String, to: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.sftp_rename(sftp_id, from, to).await?) }).await
}

pub async fn sftp_remove(
    sftp_id: String,
    path: String,
    recursive: bool,
) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.sftp_remove(sftp_id, path, recursive).await?) }).await
}

/// Direction of a transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferDirection {
    Upload,
    Download,
}

/// Phase of a transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferPhase {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Progress item of [`sftp_transfer`] / [`sftp_retry_transfer`]. The stream
/// ends after `Completed`, `Failed` or `Cancelled`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferUpdate {
    pub phase: TransferPhase,
    pub transferred: u64,
    pub total: Option<u64>,
    pub elapsed_ms: u64,
    pub error: Option<BridgeError>,
    /// Files copied so far / in total (a folder job counts every file).
    pub files_done: u32,
    pub files_total: u32,
    /// The source is a folder (copied recursively).
    pub is_directory: bool,
    /// File being copied right now.
    pub current_path: Option<String>,
}

impl TransferUpdate {
    pub(crate) fn from_job(j: &TransferJobDto) -> Self {
        Self {
            phase: match j.state {
                TransferStateDto::Queued => TransferPhase::Queued,
                TransferStateDto::Running => TransferPhase::Running,
                TransferStateDto::Completed => TransferPhase::Completed,
                TransferStateDto::Failed => TransferPhase::Failed,
                TransferStateDto::Cancelled => TransferPhase::Cancelled,
            },
            transferred: j.transferred,
            total: j.total,
            elapsed_ms: j.elapsed_ms,
            error: j.error.as_ref().map(BridgeError::from_info),
            files_done: j.files_done,
            files_total: j.files_total,
            is_directory: j.is_directory,
            current_path: j.current_path.clone(),
        }
    }

    fn failed(e: BridgeError) -> Self {
        Self {
            phase: TransferPhase::Failed,
            transferred: 0,
            total: None,
            elapsed_ms: 0,
            error: Some(e),
            files_done: 0,
            files_total: 0,
            is_directory: false,
            current_path: None,
        }
    }
}

/// Forward the core's `transfer_update` events of `id` into `sink` until
/// the job is final.
async fn forward_transfer(
    core: AppCore,
    mut events: tokio::sync::broadcast::Receiver<AppEvent>,
    id: String,
    sink: StreamSink<TransferUpdate>,
) {
    loop {
        match events.recv().await {
            Ok(AppEvent::TransferUpdate(j)) if j.transfer_id == id => {
                let end = !j.state.is_active();
                if sink.add(TransferUpdate::from_job(&j)).is_err() || end {
                    break;
                }
            }
            Ok(_) => {}
            Err(RecvError::Lagged(_)) => {
                // Missed updates: take the current snapshot.
                match core.sftp_transfer(id.clone()).await {
                    Ok(j) => {
                        let end = !j.state.is_active();
                        if sink.add(TransferUpdate::from_job(&j)).is_err() || end {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = sink.add(TransferUpdate::failed(e.into()));
                        break;
                    }
                }
            }
            Err(RecvError::Closed) => break,
        }
    }
}

/// Start a core job and stream its updates.
fn start_streamed(
    sink: StreamSink<TransferUpdate>,
    start: impl FnOnce(AppCore) -> BoxedJob + Send + 'static,
) -> Result<(), BridgeError> {
    let core = state::core()?;
    // Subscribe before starting so no update is missed.
    let events = core.subscribe_events();
    state::runtime().spawn(async move {
        match start(core.clone()).await {
            Ok(job) => {
                let end = !job.state.is_active();
                if sink.add(TransferUpdate::from_job(&job)).is_err() || end {
                    return;
                }
                forward_transfer(core, events, job.transfer_id, sink).await;
            }
            Err(e) => {
                let _ = sink.add(TransferUpdate::failed(e.into()));
            }
        }
    });
    Ok(())
}

type BoxedJob = std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<TransferJobDto, cc_app_core::AppError>> + Send>,
>;

/// Queue an upload (`local_path` → `remote_path`) or download
/// (`remote_path` → `local_path`) of a file **or folder** (recursive) under
/// the caller's `transfer_id`. Jobs run in the core (in parallel, up to a
/// limit) and keep running while the window is hidden.
pub fn sftp_transfer(
    transfer_id: String,
    sftp_id: String,
    direction: TransferDirection,
    local_path: String,
    remote_path: String,
    sink: StreamSink<TransferUpdate>,
) -> Result<(), BridgeError> {
    let request = TransferRequestDto {
        transfer_id: transfer_id.clone(),
        sftp_id,
        direction: match direction {
            TransferDirection::Upload => TransferDirectionDto::Upload,
            TransferDirection::Download => TransferDirectionDto::Download,
        },
        local_path,
        remote_path,
    };
    let _ = transfer_id;
    start_streamed(sink, move |c| {
        Box::pin(async move { c.sftp_start_transfer(request).await })
    })
}

/// Re-queue a failed / cancelled job with the same source and destination
/// under `new_transfer_id` (the old job is removed) and stream it.
pub fn sftp_retry_transfer(
    transfer_id: String,
    new_transfer_id: String,
    sink: StreamSink<TransferUpdate>,
) -> Result<(), BridgeError> {
    start_streamed(sink, move |c| {
        Box::pin(async move { c.sftp_retry_transfer(transfer_id, new_transfer_id).await })
    })
}

/// Cancel a queued or running transfer (partial files are removed).
/// `false` if it already finished (or the vault is locked).
pub async fn sftp_cancel_transfer(transfer_id: String) -> Result<bool, BridgeError> {
    with_core(move |c| async move {
        match c.sftp_cancel_transfer(transfer_id).await {
            Ok(b) => Ok(b),
            Err(cc_app_core::AppError::VaultLocked)
            | Err(cc_app_core::AppError::NoActiveProfile)
            | Err(cc_app_core::AppError::NoVault) => Ok(false),
            Err(e) => Err(e.into()),
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_map_every_chunk() {
        let f = TerminalFrame::from_chunk(TerminalChunk::Data(b"hi".to_vec()));
        assert_eq!(
            (f.kind, f.data.as_slice()),
            (TerminalFrameKind::Data, &b"hi"[..])
        );
        assert_eq!(
            TerminalFrame::from_chunk(TerminalChunk::Lagged).kind,
            TerminalFrameKind::Lagged
        );
        let f = TerminalFrame::from_chunk(TerminalChunk::Closed(TerminalStatusDto::Closed {
            exit_status: Some(3),
            reason: Some("bye".into()),
        }));
        assert_eq!(f.kind, TerminalFrameKind::Closed);
        assert_eq!(f.exit_status, Some(3));
        assert_eq!(f.message.as_deref(), Some("bye"));
        let f = TerminalFrame::from_chunk(TerminalChunk::Closed(TerminalStatusDto::Failed {
            message: "auth".into(),
        }));
        assert_eq!(f.kind, TerminalFrameKind::Failed);
        assert_eq!(f.message.as_deref(), Some("auth"));
    }

    #[test]
    fn job_snapshots_map_to_updates() {
        let mut j = TransferJobDto {
            transfer_id: "t".into(),
            sftp_id: "s".into(),
            direction: TransferDirectionDto::Upload,
            local_path: "/l".into(),
            remote_path: "/r".into(),
            state: TransferStateDto::Running,
            transferred: 5,
            total: Some(10),
            files_done: 1,
            files_total: 2,
            is_directory: true,
            current_path: Some("/l/b".into()),
            created_at_ms: 0,
            started_at_ms: Some(0),
            finished_at_ms: None,
            elapsed_ms: 7,
            error: None,
        };
        let u = TransferUpdate::from_job(&j);
        assert_eq!(u.phase, TransferPhase::Running);
        assert_eq!((u.transferred, u.total, u.files_total), (5, Some(10), 2));
        j.state = TransferStateDto::Failed;
        j.error = Some(cc_app_core::ErrorInfoDto {
            code: "not_found".into(),
            message: "no such remote file or directory: /r".into(),
            reason: None,
            args: [("path".to_owned(), "/r".to_owned())].into(),
        });
        let u = TransferUpdate::from_job(&j);
        assert_eq!(u.phase, TransferPhase::Failed);
        let e = u.error.unwrap();
        assert_eq!(e.code, "not_found");
        assert_eq!(e.details["path"], "/r");
    }
}
