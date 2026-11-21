//! One long-running `wslc events --format json` reader with a bounded queue.
//! The child is owned by the reader task: aborting the task drops it, and
//! `kill_on_drop` ends only our CLI process.
use crate::{models::*, runtime::process};
use serde_json::Value;
use std::{
    collections::VecDeque,
    path::Path,
    sync::{Arc, OnceLock},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    sync::Mutex,
    task::JoinHandle,
};

pub const MAX_EVENTS: usize = 1000;
const MAX_LINE: usize = 64 * 1024;

#[derive(Default)]
struct State {
    queue: VecDeque<DeskEvent>,
    task: Option<JoinHandle<()>>,
    running: bool,
    closing: bool,
}

static STATE: OnceLock<Arc<Mutex<State>>> = OnceLock::new();
fn state() -> Arc<Mutex<State>> {
    STATE.get_or_init(Default::default).clone()
}

/// Docker-compatible event line; unknown shapes are skipped rather than failing the stream.
pub fn parse_event(line: &str) -> Option<DeskEvent> {
    if line.len() > MAX_LINE {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    let attrs = &v["Actor"]["Attributes"];
    let text = |x: &Value| x.as_str().filter(|s| !s.is_empty()).map(str::to_owned);
    Some(DeskEvent {
        kind: text(&v["Type"])?,
        action: text(&v["Action"]).or_else(|| text(&v["status"]))?,
        id: text(&v["Actor"]["ID"]).or_else(|| text(&v["id"]))?,
        name: text(&attrs["name"]),
        image: text(&attrs["image"]).or_else(|| text(&v["from"])),
        exit_code: text(&attrs["exitCode"]).and_then(|c| c.parse().ok()),
        time: v["time"].as_i64().unwrap_or_default(),
    })
}

pub async fn start(path: &Path) -> AppResult<()> {
    let shared = state();
    let mut s = shared.lock().await;
    if s.closing {
        return Err(AppError::new("closing", "events", "Приложение закрывается"));
    }
    if s.running {
        return Ok(());
    }
    let args = ["events", "--format", "json"].map(String::from);
    let mut child = process::command(path, &args)
        .spawn()
        .map_err(|_| AppError::new("spawn", "events", "Не удалось подписаться на события WSLc"))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    s.running = true;
    let task_state = shared.clone();
    s.task = Some(tokio::spawn(async move {
        // Drain stderr so a chatty CLI can never block on a full pipe.
        let drain = tokio::spawn(async move {
            let mut sink = [0u8; 1024];
            while matches!(stderr.read(&mut sink).await, Ok(n) if n > 0) {}
        });
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(event) = parse_event(&line) {
                let mut s = task_state.lock().await;
                s.queue.push_back(event);
                while s.queue.len() > MAX_EVENTS {
                    s.queue.pop_front();
                }
            }
        }
        let _ = child.kill().await;
        let _ = child.wait().await;
        drain.abort();
        task_state.lock().await.running = false;
    }));
    Ok(())
}

pub async fn take() -> EventBatch {
    let shared = state();
    let mut s = shared.lock().await;
    EventBatch {
        events: s.queue.drain(..).collect(),
        running: s.running,
    }
}

pub async fn stop(closing: bool) {
    let shared = state();
    let task = {
        let mut s = shared.lock().await;
        s.closing |= closing;
        s.running = false;
        s.queue.clear();
        s.task.take()
    };
    if let Some(task) = task {
        task.abort();
        let _ = task.await;
    }
}
