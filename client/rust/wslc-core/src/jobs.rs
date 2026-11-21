//! Long-running CLI jobs (pull, build, push, save, load…) with streamed output.
//! A line ended by a lone CR is a progress update: the UI replaces the previous one.
use crate::{models::*, runtime::process};
use std::{
    collections::{HashMap, VecDeque},
    path::Path,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    sync::{Mutex, watch},
};

const MAX_LINES: usize = 5000;
const MAX_LINE: usize = 16 * 1024;

#[derive(Default)]
struct Output {
    lines: VecDeque<JobLine>,
    finished: bool,
    success: bool,
    exit_code: Option<i32>,
}

struct Job {
    output: Arc<Mutex<Output>>,
    cancel: watch::Sender<bool>,
}

#[derive(Default)]
struct Registry {
    next: AtomicU64,
    jobs: Mutex<HashMap<u64, Job>>,
}

static REGISTRY: OnceLock<Registry> = OnceLock::new();
fn registry() -> &'static Registry {
    REGISTRY.get_or_init(Registry::default)
}

/// Splits CLI output into lines; LF ends a line, a lone CR ends a progress line.
#[derive(Default)]
pub struct JobLineSplitter {
    pending: Vec<u8>,
    after_cr: bool,
}

impl JobLineSplitter {
    pub fn feed(&mut self, bytes: &[u8], mut emit: impl FnMut(JobLine)) {
        for &b in bytes {
            // A CR is decided by the next byte: CRLF ends a normal line, a lone CR a
            // progress line.
            if self.after_cr {
                self.after_cr = false;
                if b == b'\n' {
                    self.flush(false, &mut emit);
                    continue;
                }
                self.flush(true, &mut emit);
            }
            match b {
                b'\n' => self.flush(false, &mut emit),
                b'\r' => self.after_cr = true,
                _ if self.pending.len() < MAX_LINE => self.pending.push(b),
                _ => {}
            }
        }
    }

    fn flush(&mut self, progress: bool, emit: &mut impl FnMut(JobLine)) {
        let text = String::from_utf8_lossy(&self.pending).trim_end().to_owned();
        self.pending.clear();
        if !text.is_empty() {
            emit(JobLine { text, progress });
        }
    }

    pub fn finish(&mut self, mut emit: impl FnMut(JobLine)) {
        let progress = std::mem::take(&mut self.after_cr);
        self.flush(progress, &mut emit);
    }
}

async fn pump(mut pipe: impl AsyncRead + Unpin, output: Arc<Mutex<Output>>) {
    let mut splitter = JobLineSplitter::default();
    let mut chunk = [0u8; 8192];
    loop {
        let n = match pipe.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let mut lines = Vec::new();
        splitter.feed(&chunk[..n], |l| lines.push(l));
        push(&output, lines).await;
    }
    let mut rest = Vec::new();
    splitter.finish(|l| rest.push(l));
    push(&output, rest).await;
}

async fn push(output: &Mutex<Output>, lines: Vec<JobLine>) {
    if lines.is_empty() {
        return;
    }
    let mut out = output.lock().await;
    out.lines.extend(lines);
    while out.lines.len() > MAX_LINES {
        out.lines.pop_front();
    }
}

/// Starts `wslc <args>`; the arguments are validated by the caller.
pub async fn start(cli: &Path, args: Vec<String>) -> AppResult<u64> {
    let mut child = process::command(cli, &args)
        .spawn()
        .map_err(|_| AppError::new("spawn", "job", "Не удалось запустить WSLc"))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let output = Arc::new(Mutex::new(Output::default()));
    let (cancel, mut cancelled) = watch::channel(false);
    let task_output = output.clone();
    tokio::spawn(async move {
        let out = tokio::spawn(pump(stdout, task_output.clone()));
        let err = tokio::spawn(pump(stderr, task_output.clone()));
        let status = tokio::select! {
            _ = cancelled.changed() => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                None
            }
            status = child.wait() => status.ok(),
        };
        let _ = out.await;
        let _ = err.await;
        let mut o = task_output.lock().await;
        o.exit_code = status.and_then(|s| s.code());
        o.success = status.is_some_and(|s| s.success());
        if status.is_none() {
            o.lines.push_back(JobLine {
                text: "Отменено".into(),
                progress: false,
            });
        }
        o.finished = true;
    });
    let id = registry().next.fetch_add(1, Ordering::Relaxed) + 1;
    registry()
        .jobs
        .lock()
        .await
        .insert(id, Job { output, cancel });
    Ok(id)
}

pub async fn take(id: u64) -> AppResult<JobBatch> {
    let output = registry()
        .jobs
        .lock()
        .await
        .get(&id)
        .map(|j| j.output.clone())
        .ok_or_else(|| AppError::new("job_missing", "job", "Задача не найдена"))?;
    let mut o = output.lock().await;
    Ok(JobBatch {
        lines: o.lines.drain(..).collect(),
        finished: o.finished,
        success: o.success,
        exit_code: o.exit_code,
    })
}

pub async fn cancel(id: u64) {
    if let Some(job) = registry().jobs.lock().await.get(&id) {
        let _ = job.cancel.send(true);
    }
}

/// Drops a finished job's record.
pub async fn forget(id: u64) {
    registry().jobs.lock().await.remove(&id);
}

pub async fn shutdown() {
    let jobs: Vec<Job> = registry()
        .jobs
        .lock()
        .await
        .drain()
        .map(|(_, j)| j)
        .collect();
    for job in jobs {
        let _ = job.cancel.send(true);
    }
}
