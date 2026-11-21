//! Interactive `wslc exec -it` sessions on a Windows pseudo console (ConPTY).
//! A reader thread fills a bounded buffer that the UI drains; input and resizes go
//! straight to the pseudo console. Closing a session ends only our CLI process.
use crate::{desktop::terminal_args, models::*};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    path::Path,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

/// Output kept while the UI is not reading; older bytes are dropped first.
pub const OUTPUT_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Default)]
struct Output {
    data: VecDeque<u8>,
    eof: bool,
}

struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    output: Arc<Mutex<Output>>,
}

#[derive(Default)]
struct Registry {
    next: AtomicU64,
    sessions: Mutex<HashMap<u64, Session>>,
    closing: Mutex<bool>,
}

static REGISTRY: OnceLock<Registry> = OnceLock::new();
fn registry() -> &'static Registry {
    REGISTRY.get_or_init(Registry::default)
}

fn failed(operation: &str, message: &str, e: impl std::fmt::Display) -> AppError {
    let mut error = AppError::new("terminal", operation, message);
    error.diagnostics = Some(e.to_string());
    error
}

fn missing() -> AppError {
    AppError::new("session_missing", "terminal", "Сессия терминала закрыта")
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.clamp(2, 500),
        cols: cols.clamp(2, 1000),
        pixel_width: 0,
        pixel_height: 0,
    }
}

pub fn open(cli: &Path, id: &str, workdir: Option<&str>, cols: u16, rows: u16) -> AppResult<u64> {
    let args = terminal_args(id, workdir)?;
    if *registry().closing.lock().unwrap() {
        return Err(AppError::new(
            "closing",
            "terminal",
            "Приложение закрывается",
        ));
    }
    let pair = native_pty_system()
        .openpty(size(cols, rows))
        .map_err(|e| failed("open_terminal", "Не удалось создать псевдоконсоль", e))?;
    let mut command = CommandBuilder::new(cli);
    command.args(&args);
    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|e| failed("open_terminal", "Не удалось запустить wslc exec", e))?;
    drop(pair.slave);
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| failed("open_terminal", "Не удалось открыть вывод терминала", e))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| failed("open_terminal", "Не удалось открыть ввод терминала", e))?;
    let output = Arc::new(Mutex::new(Output::default()));
    let sink = output.clone();
    std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let mut out = sink.lock().unwrap();
                    out.data.extend(&chunk[..n]);
                    let excess = out.data.len().saturating_sub(OUTPUT_LIMIT);
                    out.data.drain(..excess);
                }
            }
        }
        sink.lock().unwrap().eof = true;
    });
    let session_id = registry().next.fetch_add(1, Ordering::Relaxed) + 1;
    registry().sessions.lock().unwrap().insert(
        session_id,
        Session {
            master: pair.master,
            writer,
            child,
            output,
        },
    );
    Ok(session_id)
}

/// New output since the last call; `alive` turns false once the shell has exited.
pub fn take(session: u64) -> AppResult<TerminalOutput> {
    let mut sessions = registry().sessions.lock().unwrap();
    let s = sessions.get_mut(&session).ok_or_else(missing)?;
    let (data, eof) = {
        let mut out = s.output.lock().unwrap();
        (out.data.drain(..).collect::<Vec<u8>>(), out.eof)
    };
    // ConPTY may keep the pipe open after the child exits, so ask the process too.
    let exited = matches!(s.child.try_wait(), Ok(Some(_)));
    Ok(TerminalOutput {
        data,
        alive: !(eof || exited),
    })
}

pub fn write(session: u64, data: &[u8]) -> AppResult<()> {
    let mut sessions = registry().sessions.lock().unwrap();
    let s = sessions.get_mut(&session).ok_or_else(missing)?;
    s.writer
        .write_all(data)
        .and_then(|()| s.writer.flush())
        .map_err(|e| failed("write_terminal", "Терминал не принимает ввод", e))
}

pub fn resize(session: u64, cols: u16, rows: u16) -> AppResult<()> {
    let sessions = registry().sessions.lock().unwrap();
    let s = sessions.get(&session).ok_or_else(missing)?;
    s.master
        .resize(size(cols, rows))
        .map_err(|e| failed("resize_terminal", "Не удалось изменить размер терминала", e))
}

pub fn close(session: u64) {
    let removed = registry().sessions.lock().unwrap().remove(&session);
    if let Some(mut s) = removed {
        let _ = s.child.kill();
        // Closing a pseudo console can wait for conhost; never hold the caller for it.
        std::thread::spawn(move || {
            let _ = s.child.wait();
            drop(s);
        });
    }
}

pub fn shutdown() {
    *registry().closing.lock().unwrap() = true;
    let ids: Vec<u64> = registry()
        .sessions
        .lock()
        .unwrap()
        .keys()
        .copied()
        .collect();
    for id in ids {
        close(id);
    }
}
