use crate::{
    logs::{LineDecoder, LogRecord, MAX_BYTES, MAX_LINES},
    models::*,
    runtime::{cli::validate_id, process},
};
use std::{
    collections::{HashMap, VecDeque},
    path::Path,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    sync::{Mutex, watch},
    task::JoinHandle,
};

#[derive(Default)]
struct Queue {
    entries: VecDeque<LogEntry>,
    bytes: usize,
    sequence: u64,
    dropped: u64,
    truncated: u64,
    status: String,
}
impl Queue {
    fn push(&mut self, stream: &str, record: LogRecord) {
        self.sequence += 1;
        self.truncated += u64::from(record.truncated);
        self.bytes += record.text.len() + 1;
        self.entries.push_back(LogEntry {
            sequence: self.sequence,
            stream: stream.into(),
            text: record.text,
            truncated: record.truncated,
        });
        while self.entries.len() > MAX_LINES || self.bytes > MAX_BYTES {
            if let Some(entry) = self.entries.pop_front() {
                self.bytes -= entry.text.len() + 1;
                self.dropped += 1;
            }
        }
    }
    fn drain(&mut self) -> LogBatch {
        let mut entries = Vec::new();
        let mut bytes = 0;
        while entries.len() < 256 {
            let Some(front) = self.entries.front() else {
                break;
            };
            if bytes + front.text.len() > 512 * 1024 {
                break;
            }
            let entry = self.entries.pop_front().expect("observed entry");
            bytes += entry.text.len();
            self.bytes -= entry.text.len() + 1;
            entries.push(entry);
        }
        LogBatch {
            entries,
            status: self.status.clone(),
            dropped_total: self.dropped,
            truncated_total: self.truncated,
        }
    }
}
struct Reader {
    cancel: watch::Sender<bool>,
    task: JoinHandle<()>,
    queue: Arc<Mutex<Queue>>,
}
#[derive(Default)]
struct Readers {
    next: AtomicU64,
    closing: AtomicBool,
    readers: Mutex<HashMap<u64, Reader>>,
}
static READERS: OnceLock<Readers> = OnceLock::new();
fn registry() -> &'static Readers {
    READERS.get_or_init(Readers::default)
}

async fn pump(mut pipe: impl AsyncRead + Unpin, stream: &str, queue: Arc<Mutex<Queue>>) -> bool {
    let mut decoder = LineDecoder::default();
    let mut bytes = [0u8; 8192];
    loop {
        let n = match pipe.read(&mut bytes).await {
            Ok(0) => break,
            Err(_) => return false,
            Ok(n) => n,
        };
        let mut records = Vec::new();
        decoder.feed(&bytes[..n], |record| records.push(record));
        let mut queue = queue.lock().await;
        for record in records {
            queue.push(stream, record);
        }
    }
    if let Some(record) = decoder.finish() {
        queue.lock().await.push(stream, record);
    }
    true
}

/// `container logs` arguments; `since` must look like Unix seconds or RFC 3339.
pub fn log_args(id: &str, options: &LogOptions) -> AppResult<Vec<String>> {
    validate_id(id)?;
    let mut args: Vec<String> = vec!["container".into(), "logs".into()];
    if let Some(since) = &options.since {
        let ok = !since.is_empty()
            && since.len() <= 40
            && since.starts_with(|c: char| c.is_ascii_digit())
            && since
                .chars()
                .all(|c| c.is_ascii_digit() || "TZ:.+-".contains(c));
        if !ok {
            return Err(AppError::new(
                "invalid_since",
                "subscribe_logs",
                "Некорректная отметка времени для логов",
            ));
        }
        args.extend(["--since".into(), since.clone()]);
    }
    match options.tail {
        Some(n) if n > MAX_LINES as u32 => {
            return Err(AppError::new(
                "invalid_tail",
                "subscribe_logs",
                "Слишком много строк истории",
            ));
        }
        Some(n) => args.extend(["--tail".into(), n.to_string()]),
        None if options.since.is_none() => args.extend(["--tail".into(), "1000".into()]),
        None => {}
    }
    if options.timestamps {
        args.push("--timestamps".into());
    }
    args.extend(["--follow".into(), id.into()]);
    Ok(args)
}

pub async fn subscribe(path: &Path, id: &str, options: &LogOptions) -> AppResult<u64> {
    let args = log_args(id, options)?;
    let mut readers = registry().readers.lock().await;
    if registry().closing.load(Ordering::Acquire) {
        return Err(AppError::new(
            "closing",
            "subscribe_logs",
            "Приложение закрывается",
        ));
    }
    let mut child = process::command(path, &args).spawn().map_err(|_| {
        AppError::new(
            "spawn",
            "subscribe_logs",
            "Не удалось запустить чтение логов",
        )
    })?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let queue = Arc::new(Mutex::new(Queue {
        status: "Чтение".into(),
        ..Queue::default()
    }));
    let (cancel, mut cancelled) = watch::channel(false);
    let task_queue = queue.clone();
    let task = tokio::spawn(async move {
        let mut out = tokio::spawn(pump(stdout, "stdout", task_queue.clone()));
        let mut err = tokio::spawn(pump(stderr, "stderr", task_queue.clone()));
        let mut status = tokio::select! {
            _ = cancelled.changed() => { let _ = child.kill().await; let _ = child.wait().await; "Чтение отменено".to_string() },
            result = child.wait() => match result {
                Ok(exit) if exit.success() => "Поток завершён".into(),
                Ok(exit) => format!("Ошибка CLI (код {})", exit.code().unwrap_or(-1)),
                Err(_) => "Ошибка чтения CLI".into(),
            },
        };
        for handle in [&mut out, &mut err] {
            match tokio::time::timeout(Duration::from_secs(2), &mut *handle).await {
                Ok(Ok(true)) => {}
                Ok(_) => status = "Ошибка чтения потока".into(),
                Err(_) => {
                    handle.abort();
                    status = "Поток чтения не завершился вовремя".into();
                }
            }
        }
        task_queue.lock().await.status = status;
    });
    let id = registry().next.fetch_add(1, Ordering::Relaxed) + 1;
    readers.insert(
        id,
        Reader {
            cancel,
            task,
            queue,
        },
    );
    Ok(id)
}
pub async fn take(id: u64) -> AppResult<LogBatch> {
    let queue = registry()
        .readers
        .lock()
        .await
        .get(&id)
        .map(|r| r.queue.clone())
        .ok_or_else(|| AppError::new("subscription_missing", "logs", "Подписка на логи закрыта"))?;
    Ok(queue.lock().await.drain())
}
pub async fn unsubscribe(id: u64) {
    let reader = registry().readers.lock().await.remove(&id);
    if let Some(reader) = reader {
        let _ = reader.cancel.send(true);
        let _ = reader.task.await;
    }
}
pub async fn shutdown() {
    let readers = {
        let mut readers = registry().readers.lock().await;
        registry().closing.store(true, Ordering::Release);
        std::mem::take(&mut *readers)
    };
    for reader in readers.values() {
        let _ = reader.cancel.send(true);
    }
    for (_, reader) in readers {
        let _ = reader.task.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queue_eviction_and_batches_are_bounded() {
        let mut queue = Queue::default();
        for _ in 0..10_050 {
            queue.push(
                "stdout",
                LogRecord {
                    text: "строка".into(),
                    truncated: false,
                },
            );
        }
        assert_eq!(queue.entries.len(), MAX_LINES);
        assert_eq!(queue.dropped, 50);
        assert_eq!(queue.drain().entries.len(), 256);
        for _ in 0..200 {
            queue.push(
                "stdout",
                LogRecord {
                    text: "a".repeat(64 * 1024),
                    truncated: true,
                },
            );
        }
        assert!(queue.bytes <= MAX_BYTES);
        assert_eq!(queue.truncated, 200);
        let batch = queue.drain();
        assert!(batch.entries.iter().map(|e| e.text.len()).sum::<usize>() <= 512 * 1024);
    }
}
