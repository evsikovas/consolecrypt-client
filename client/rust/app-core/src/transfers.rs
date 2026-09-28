//! SFTP transfer jobs (SFTP browser): uploads / downloads of files **or
//! folders** (recursive) under caller-supplied transfer ids, run in the
//! background with a parallelism limit, progress + state as
//! [`AppEvent::TransferUpdate`] (and the older per-chunk
//! [`AppEvent::TransferProgress`]), cooperative cancel (partial files are
//! removed) and retry of failed / cancelled jobs. Jobs belong to the
//! unlocked vault: lock cancels them.

use crate::dto::{ms, AppEvent};
use crate::error::{AppError, AppResult};
use crate::session::AppCtx;
use cc_sftp_core::{CancelToken, EntryKind, SftpClient, SftpError, TransferOptions};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

/// Direction of a transfer job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferDirectionDto {
    /// `local_path` → `remote_path`.
    Upload,
    /// `remote_path` → `local_path`.
    Download,
}

/// State of a transfer job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferStateDto {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl TransferStateDto {
    /// Queued or running.
    pub fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
}

/// Machine-readable error of a failed operation (code / reason / args as
/// [`AppError::code`] / [`AppError::reason`] / [`AppError::args`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorInfoDto {
    pub code: String,
    pub message: String,
    pub reason: Option<String>,
    pub args: BTreeMap<String, String>,
}

impl From<&AppError> for ErrorInfoDto {
    fn from(e: &AppError) -> Self {
        Self {
            code: e.code().to_owned(),
            message: e.message(),
            reason: e.reason().map(str::to_owned),
            args: e.args(),
        }
    }
}

/// What a job copies (kept for retries).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferRequestDto {
    /// Caller-supplied, unique among active jobs.
    pub transfer_id: String,
    pub sftp_id: String,
    pub direction: TransferDirectionDto,
    /// Source (upload) or destination (download) on this machine.
    pub local_path: String,
    /// Destination (upload) or source (download) on the server.
    pub remote_path: String,
}

/// Snapshot of a transfer job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferJobDto {
    pub transfer_id: String,
    pub sftp_id: String,
    pub direction: TransferDirectionDto,
    pub local_path: String,
    pub remote_path: String,
    pub state: TransferStateDto,
    /// Bytes copied so far (all files of a folder job).
    pub transferred: u64,
    /// Total bytes when known (a folder job counts everything below it).
    pub total: Option<u64>,
    pub files_done: u32,
    pub files_total: u32,
    /// The source is a folder (copied recursively).
    pub is_directory: bool,
    /// File being copied right now.
    pub current_path: Option<String>,
    pub created_at_ms: i64,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
    /// Running time (updated with progress).
    pub elapsed_ms: u64,
    pub error: Option<ErrorInfoDto>,
}

/// Minimum interval between progress events of one job.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(150);
/// Finished jobs kept for the transfer list / retries.
const MAX_FINISHED: usize = 200;

struct Job {
    seq: u64,
    request: TransferRequestDto,
    dto: TransferJobDto,
    cancel: CancelToken,
    task: Option<tokio::task::AbortHandle>,
}

/// Transfer jobs of one unlocked vault.
pub(crate) struct TransferManager {
    ctx: Arc<AppCtx>,
    jobs: Mutex<HashMap<String, Job>>,
    seq: std::sync::atomic::AtomicU64,
    permits: Arc<Semaphore>,
}

impl std::fmt::Debug for TransferManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransferManager").finish_non_exhaustive()
    }
}

fn now_ms() -> i64 {
    ms(&chrono::Utc::now())
}

/// One file of a (folder) job.
struct Item {
    /// Local path.
    local: PathBuf,
    /// Remote path.
    remote: String,
    size: u64,
}

/// A planned job: directories to create (parents first) and files.
struct Plan {
    dirs: Vec<(PathBuf, String)>,
    files: Vec<Item>,
    is_directory: bool,
}

fn join_remote(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// Walk a local folder (symlinked files are followed, symlinked folders
/// skipped so cycles cannot occur).
fn plan_upload(local: &Path, remote: &str) -> AppResult<Plan> {
    let meta =
        std::fs::metadata(local).map_err(|e| AppError::Io(format!("{}: {e}", local.display())))?;
    if !meta.is_dir() {
        return Ok(Plan {
            dirs: Vec::new(),
            files: vec![Item {
                local: local.to_path_buf(),
                remote: remote.to_owned(),
                size: meta.len(),
            }],
            is_directory: false,
        });
    }
    let mut plan = Plan {
        dirs: vec![(local.to_path_buf(), remote.to_owned())],
        files: Vec::new(),
        is_directory: true,
    };
    let mut stack = vec![(local.to_path_buf(), remote.to_owned())];
    while let Some((dir, rdir)) = stack.pop() {
        let rd =
            std::fs::read_dir(&dir).map_err(|e| AppError::Io(format!("{}: {e}", dir.display())))?;
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            let path = e.path();
            let Ok(lmeta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            let meta = if lmeta.file_type().is_symlink() {
                match std::fs::metadata(&path) {
                    Ok(m) if m.is_file() => m,
                    _ => continue,
                }
            } else {
                lmeta
            };
            let rpath = join_remote(&rdir, &name);
            if meta.is_dir() {
                plan.dirs.push((path.clone(), rpath.clone()));
                stack.push((path, rpath));
            } else if meta.is_file() {
                plan.files.push(Item {
                    local: path,
                    remote: rpath,
                    size: meta.len(),
                });
            }
        }
    }
    Ok(plan)
}

/// Walk a remote folder (symlinks to files are downloaded, symlinks to
/// folders skipped).
async fn plan_download(c: &SftpClient, remote: &str, local: &Path) -> AppResult<Plan> {
    let st = c.stat(remote).await?;
    if st.kind != EntryKind::Dir {
        if st.kind != EntryKind::File {
            return Err(AppError::invalid(
                "remote_path",
                "not a regular file or folder",
            ));
        }
        return Ok(Plan {
            dirs: Vec::new(),
            files: vec![Item {
                local: local.to_path_buf(),
                remote: remote.to_owned(),
                size: st.size,
            }],
            is_directory: false,
        });
    }
    let mut plan = Plan {
        dirs: vec![(local.to_path_buf(), remote.to_owned())],
        files: Vec::new(),
        is_directory: true,
    };
    let mut stack = vec![(remote.to_owned(), local.to_path_buf())];
    while let Some((rdir, ldir)) = stack.pop() {
        for e in c.list(&rdir).await? {
            let lpath = ldir.join(crate::sftp_browser::safe_local_name(&e.name)?);
            match e.kind {
                EntryKind::Dir => {
                    plan.dirs.push((lpath.clone(), e.path.clone()));
                    stack.push((e.path, lpath));
                }
                EntryKind::File => plan.files.push(Item {
                    local: lpath,
                    remote: e.path,
                    size: e.size,
                }),
                EntryKind::Symlink => {
                    if let Ok(t) = c.stat(&e.path).await {
                        if t.kind == EntryKind::File {
                            plan.files.push(Item {
                                local: lpath,
                                remote: e.path,
                                size: t.size,
                            });
                        }
                    }
                }
                EntryKind::Other => {}
            }
        }
    }
    Ok(plan)
}

impl TransferManager {
    pub(crate) fn new(ctx: Arc<AppCtx>) -> Arc<Self> {
        let n = ctx.config.max_parallel_transfers.max(1);
        Arc::new(Self {
            ctx,
            jobs: Mutex::new(HashMap::new()),
            seq: std::sync::atomic::AtomicU64::new(0),
            permits: Arc::new(Semaphore::new(n)),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Job>> {
        self.jobs.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn emit(&self, dto: &TransferJobDto) {
        self.ctx.emit(AppEvent::TransferUpdate(dto.clone()));
    }

    /// Update a job's snapshot (ignored once it is final) and emit it.
    fn update(&self, id: &str, f: impl FnOnce(&mut TransferJobDto)) {
        let dto = {
            let mut jobs = self.lock();
            let Some(j) = jobs.get_mut(id) else { return };
            if !j.dto.state.is_active() {
                return;
            }
            f(&mut j.dto);
            j.dto.clone()
        };
        self.emit(&dto);
    }

    /// All jobs, most recent first.
    pub(crate) fn list(&self) -> Vec<TransferJobDto> {
        let jobs = self.lock();
        let mut v: Vec<(u64, TransferJobDto)> =
            jobs.values().map(|j| (j.seq, j.dto.clone())).collect();
        v.sort_by_key(|(seq, _)| std::cmp::Reverse(*seq));
        v.into_iter().map(|(_, d)| d).collect()
    }

    pub(crate) fn get(&self, id: &str) -> Option<TransferJobDto> {
        self.lock().get(id).map(|j| j.dto.clone())
    }

    /// Queue a job on `client` (the SFTP session of `request.sftp_id`).
    pub(crate) fn start(
        self: &Arc<Self>,
        request: TransferRequestDto,
        client: Arc<SftpClient>,
    ) -> AppResult<TransferJobDto> {
        let id = request.transfer_id.trim().to_owned();
        if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
            return Err(AppError::invalid(
                "transfer_id",
                "must be a short non-empty id",
            ));
        }
        crate::validate::file_path("local_path", &request.local_path)?;
        if request.remote_path.trim().is_empty() || request.remote_path.contains('\0') {
            return Err(AppError::invalid("remote_path", "must not be empty"));
        }
        let dto = TransferJobDto {
            transfer_id: id.clone(),
            sftp_id: request.sftp_id.clone(),
            direction: request.direction,
            local_path: request.local_path.clone(),
            remote_path: request.remote_path.clone(),
            state: TransferStateDto::Queued,
            transferred: 0,
            total: None,
            files_done: 0,
            files_total: 0,
            is_directory: false,
            current_path: None,
            created_at_ms: now_ms(),
            started_at_ms: None,
            finished_at_ms: None,
            elapsed_ms: 0,
            error: None,
        };
        let cancel = CancelToken::new();
        {
            let mut jobs = self.lock();
            if jobs.get(&id).is_some_and(|j| j.dto.state.is_active()) {
                return Err(AppError::invalid(
                    "transfer_id",
                    "a job with this id is running",
                ));
            }
            let seq = self.seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            jobs.insert(
                id.clone(),
                Job {
                    seq,
                    request: TransferRequestDto {
                        transfer_id: id.clone(),
                        ..request.clone()
                    },
                    dto: dto.clone(),
                    cancel: cancel.clone(),
                    task: None,
                },
            );
            Self::prune(&mut jobs);
        }
        self.emit(&dto);
        let me = self.clone();
        let task_id = id.clone();
        let handle = tokio::spawn(async move {
            let Ok(_permit) = me.permits.clone().acquire_owned().await else {
                return;
            };
            if cancel.is_cancelled() {
                return;
            }
            let started = Instant::now();
            me.update(&task_id, |d| {
                d.state = TransferStateDto::Running;
                d.started_at_ms = Some(now_ms());
            });
            let r = me.run(&task_id, &request, &client, &cancel, started).await;
            let elapsed = started.elapsed().as_millis() as u64;
            me.update(&task_id, |d| {
                d.elapsed_ms = elapsed;
                d.finished_at_ms = Some(now_ms());
                d.current_path = None;
                match r {
                    Ok(()) => {
                        d.state = TransferStateDto::Completed;
                        d.total = Some(d.transferred);
                    }
                    Err(AppError::Cancelled) => d.state = TransferStateDto::Cancelled,
                    Err(e) => {
                        tracing::warn!(transfer_id = %d.transfer_id, code = e.code(), "transfer failed");
                        d.state = TransferStateDto::Failed;
                        d.error = Some(ErrorInfoDto::from(&e));
                    }
                }
            });
            if let Some(j) = me.lock().get_mut(&task_id) {
                j.task = None;
            }
        });
        if let Some(j) = self.lock().get_mut(&id) {
            if j.dto.state.is_active() {
                j.task = Some(handle.abort_handle());
            }
        }
        Ok(dto)
    }

    /// Drop the oldest finished jobs beyond [`MAX_FINISHED`].
    fn prune(jobs: &mut HashMap<String, Job>) {
        let mut finished: Vec<(u64, String)> = jobs
            .iter()
            .filter(|(_, j)| !j.dto.state.is_active())
            .map(|(k, j)| (j.seq, k.clone()))
            .collect();
        if finished.len() <= MAX_FINISHED {
            return;
        }
        finished.sort();
        let excess = finished.len() - MAX_FINISHED;
        for (_, k) in finished.into_iter().take(excess) {
            jobs.remove(&k);
        }
    }

    async fn run(
        &self,
        id: &str,
        request: &TransferRequestDto,
        c: &SftpClient,
        cancel: &CancelToken,
        started: Instant,
    ) -> AppResult<()> {
        let local = PathBuf::from(&request.local_path);
        let plan = match request.direction {
            TransferDirectionDto::Upload => {
                let l = local.clone();
                let r = request.remote_path.clone();
                tokio::task::spawn_blocking(move || plan_upload(&l, &r))
                    .await
                    .map_err(AppError::internal)??
            }
            TransferDirectionDto::Download => {
                plan_download(c, &request.remote_path, &local).await?
            }
        };
        let total: u64 = plan.files.iter().map(|f| f.size).sum();
        let files_total = u32::try_from(plan.files.len()).unwrap_or(u32::MAX);
        self.update(id, |d| {
            d.total = Some(total);
            d.files_total = files_total;
            d.is_directory = plan.is_directory;
        });
        if cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        for (ldir, rdir) in &plan.dirs {
            match request.direction {
                TransferDirectionDto::Upload => c.mkdir_all(rdir).await?,
                TransferDirectionDto::Download => tokio::fs::create_dir_all(ldir)
                    .await
                    .map_err(|e| AppError::Io(format!("{}: {e}", ldir.display())))?,
            }
        }
        let opts = TransferOptions::default();
        let mut done_bytes = 0u64;
        for (index, item) in plan.files.iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(AppError::Cancelled);
            }
            let shown = match request.direction {
                TransferDirectionDto::Upload => item.local.display().to_string(),
                TransferDirectionDto::Download => item.remote.clone(),
            };
            self.update(id, |d| d.current_path = Some(shown));
            let mut last_emit = Instant::now() - PROGRESS_INTERVAL;
            let ctx = self.ctx.clone();
            let base = done_bytes;
            let tid = id.to_owned();
            let mut progress = |p: cc_sftp_core::TransferProgress| {
                if last_emit.elapsed() < PROGRESS_INTERVAL {
                    return;
                }
                last_emit = Instant::now();
                let transferred = base + p.transferred;
                ctx.emit(AppEvent::TransferProgress {
                    transfer_id: tid.clone(),
                    transferred,
                    total: Some(total),
                });
                self.update(&tid, |d| {
                    d.transferred = transferred;
                    d.elapsed_ms = started.elapsed().as_millis() as u64;
                });
            };
            let r = match request.direction {
                TransferDirectionDto::Upload => {
                    c.upload(&item.local, &item.remote, &opts, &mut progress, cancel)
                        .await
                }
                TransferDirectionDto::Download => {
                    c.download(&item.remote, &item.local, &opts, &mut progress, cancel)
                        .await
                }
            };
            match r {
                Ok(s) => done_bytes += s.bytes,
                Err(SftpError::Cancelled) => return Err(AppError::Cancelled),
                Err(e) => return Err(e.into()),
            }
            let files_done = u32::try_from(index + 1).unwrap_or(u32::MAX);
            let transferred = done_bytes;
            self.update(id, |d| {
                d.files_done = files_done;
                d.transferred = transferred;
                d.elapsed_ms = started.elapsed().as_millis() as u64;
            });
        }
        Ok(())
    }

    /// Cancel a queued or running job; `false` if it is unknown / finished.
    pub(crate) fn cancel(&self, id: &str) -> bool {
        let dto = {
            let mut jobs = self.lock();
            let Some(j) = jobs.get_mut(id) else {
                return false;
            };
            if !j.dto.state.is_active() {
                return false;
            }
            j.cancel.cancel();
            if j.dto.state == TransferStateDto::Queued {
                if let Some(t) = j.task.take() {
                    t.abort();
                }
            }
            j.dto.state = TransferStateDto::Cancelled;
            j.dto.finished_at_ms = Some(now_ms());
            j.dto.current_path = None;
            j.dto.clone()
        };
        self.emit(&dto);
        true
    }

    /// Cancel every job of an SFTP session (it is being closed).
    pub(crate) fn cancel_for_session(&self, sftp_id: &str) {
        let ids: Vec<String> = self
            .lock()
            .values()
            .filter(|j| j.dto.state.is_active() && j.request.sftp_id == sftp_id)
            .map(|j| j.dto.transfer_id.clone())
            .collect();
        for id in ids {
            self.cancel(&id);
        }
    }

    /// Cancel everything (vault lock).
    pub(crate) fn cancel_all(&self) {
        let ids: Vec<String> = self
            .lock()
            .values()
            .filter(|j| j.dto.state.is_active())
            .map(|j| j.dto.transfer_id.clone())
            .collect();
        for id in ids {
            self.cancel(&id);
        }
    }

    /// The request of a finished job (for retries), removing the job.
    pub(crate) fn take_for_retry(&self, id: &str) -> AppResult<TransferRequestDto> {
        let mut jobs = self.lock();
        let j = jobs
            .remove(id)
            .ok_or_else(|| AppError::not_found("transfer", id))?;
        if j.dto.state.is_active() {
            jobs.insert(id.to_owned(), j);
            return Err(AppError::invalid("transfer_id", "the job is still running"));
        }
        Ok(j.request)
    }

    /// Remove completed, failed and cancelled jobs.
    pub(crate) fn clear_finished(&self) {
        self.lock().retain(|_, j| j.dto.state.is_active());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_folder_plan_is_recursive_and_skips_dir_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("site");
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::write(root.join("index.html"), b"12345").unwrap();
        std::fs::write(root.join("a/b/deep.txt"), b"xy").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&root, root.join("a/loop")).unwrap();
        let plan = plan_upload(&root, "/srv/site").unwrap();
        assert!(plan.is_directory);
        let dirs: Vec<_> = plan.dirs.iter().map(|(_, r)| r.as_str()).collect();
        assert!(dirs.contains(&"/srv/site") && dirs.contains(&"/srv/site/a/b"));
        assert!(!dirs.iter().any(|d| d.contains("loop")));
        let total: u64 = plan.files.iter().map(|f| f.size).sum();
        assert_eq!(total, 7);
        assert_eq!(plan.files.len(), 2);
        let single = plan_upload(&root.join("index.html"), "/srv/i.html").unwrap();
        assert!(!single.is_directory);
        assert_eq!(single.files[0].remote, "/srv/i.html");
        assert!(plan_upload(&root.join("missing"), "/x").is_err());
    }
}
