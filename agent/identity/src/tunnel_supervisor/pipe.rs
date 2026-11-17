//! Actual uniquely owned Child stdio and joined bounded reader/writer owners.
use super::*;
use std::io::{BufRead, BufReader};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::process::{ChildStdin, ChildStdout};
use std::sync::{
    atomic::{AtomicU8, Ordering},
    mpsc, Arc,
};
use std::thread::{self, JoinHandle};

pub(super) const LINE: usize = 24576;
pub(super) const DATA: usize = 8192;
const FORMAT: &str = "cc-tunnel-supervised/v1";
pub(super) fn fifo(fd: i32) -> Result<(u64, u64)> {
    let m = fs::metadata(format!("/proc/self/fd/{fd}")).map_err(|_| Error::Storage)?;
    if !m.file_type().is_fifo() || m.ino() == 0 {
        return Err(Error::Storage);
    }
    Ok((m.dev(), m.ino()))
}
pub(super) fn audit_inherited_fds() -> Result<()> {
    // Fail closed instead of unsafe close-range/pre_exec hooks. Every nonstdio
    // inherited descriptor MUST be CLOEXEC before starting any helper thread.
    for item in fs::read_dir("/proc/self/fd").map_err(|_| Error::Storage)? {
        let name = item.map_err(|_| Error::Storage)?.file_name();
        let Some(name) = name.to_str() else {
            return Err(Error::Storage);
        };
        let fd: u32 = name.parse().map_err(|_| Error::Storage)?;
        if fd <= 2 {
            continue;
        }
        let info = match fs::read_to_string(format!("/proc/self/fdinfo/{fd}")) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(Error::Storage),
        };
        let flags = info
            .lines()
            .find_map(|l| l.strip_prefix("flags:\t"))
            .ok_or(Error::Storage)?;
        let flags = u64::from_str_radix(flags.trim(), 8).map_err(|_| Error::Storage)?;
        if flags & (rustix::fs::OFlags::CLOEXEC.bits() as u64) == 0 {
            return Err(Error::Storage);
        }
    }
    Ok(())
}
pub(super) fn canonical(value: &Value) -> Result<Vec<u8>> {
    let mut raw = Vec::new();
    crate::wire::canonical_json(value, &mut raw)?;
    if !raw.is_ascii() || raw.len() + 1 > LINE {
        return Err(Error::Quota);
    }
    raw.push(b'\n');
    Ok(raw)
}
pub(super) fn parse(raw: &[u8]) -> Result<Value> {
    if raw.is_empty() || raw.len() > LINE || !raw.ends_with(b"\n") || !raw.is_ascii() {
        return Err(Error::InvalidWire);
    }
    let value: Value = serde_json::from_slice(raw).map_err(|_| Error::InvalidWire)?;
    // Canonical byte equality rejects duplicate keys at ALL depths, trailing
    // frames and alternate representations without adding a new codec/parser.
    if canonical(&value)?.as_slice() != raw {
        return Err(Error::InvalidWire);
    }
    Ok(value)
}
pub(super) fn fields(value: &Value, names: &[&str]) -> Result<()> {
    let object = value.as_object().ok_or(Error::InvalidWire)?;
    if object.len() != names.len() || names.iter().any(|n| !object.contains_key(*n)) {
        return Err(Error::InvalidWire);
    }
    Ok(())
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
enum Control {
    Stop {},
}
pub(super) fn stop_frame(raw: &[u8]) -> Result<()> {
    let command: Control = serde_json::from_slice(raw).map_err(|_| Error::InvalidWire)?;
    match command {
        Control::Stop {} => Ok(()),
    }
}
pub(super) struct ParentControl {
    pub flag: Arc<AtomicU8>, // 0 waiting, 1 original Stop, 2 unknown, 3 remote stop, 4 transport retirement
    retirement: Arc<AtomicU8>, // private: 0 waiting, 1 verified remote stop, 2 abort, 3 verified product loss
    join: Option<JoinHandle<Result<()>>>,
}
impl ParentControl {
    pub fn start() -> Result<Self> {
        let identity = fifo(0)?;
        // Fixed Linux proc alias ONLY. Reopening gives this reader its own
        // nonblocking file description; never alters caller stdin flags.
        let input = fs::OpenOptions::new()
            .read(true)
            .custom_flags((rustix::fs::OFlags::NONBLOCK | rustix::fs::OFlags::CLOEXEC).bits() as i32) // O_NONBLOCK | O_CLOEXEC
            .open("/proc/self/fd/0")
            .map_err(|_| Error::Storage)?;
        let meta = input.metadata().map_err(|_| Error::Storage)?;
        if !meta.file_type().is_fifo() || meta.uid() != uid()?
            || (meta.dev(), meta.ino()) != identity || fifo(0)? != identity
        {
            return Err(Error::Storage);
        }
        let flag = Arc::new(AtomicU8::new(0));
        let state = flag.clone();
        let retirement = Arc::new(AtomicU8::new(0));
        let retire = retirement.clone();
        let join = thread::Builder::new()
            .name("tunnel-parent-control".into())
            .spawn(move || {
                let mut input = input;
                let mut raw = Vec::new();
                let result = (|| -> Result<u8> {
                    let mut block = [0u8; 4097];
                    loop {
                        let meta = input.metadata().map_err(|_| Error::Storage)?;
                        if fifo(0)? != identity || !meta.file_type().is_fifo()
                            || (meta.dev(), meta.ino()) != identity || meta.uid() != uid()?
                        {
                            return Err(Error::Storage);
                        }
                        // Read BEFORE private retirement: EOF, malformed bytes,
                        // or any partial input wins UNKNOWN. No caller EOF needed.
                        match input.read(&mut block[..4097 - raw.len()]) {
                            Ok(0) => return Err(Error::WrongState),
                            Ok(n) => {
                                raw.extend_from_slice(&block[..n]);
                                if raw.len() > 4096 { return Err(Error::Quota); }
                                if raw.contains(&b'\n') {
                                    stop_frame(&raw)?;
                                    if !raw.ends_with(b"\n") || fifo(0)? != identity {
                                        return Err(Error::InvalidWire);
                                    }
                                    return Ok(1);
                                }
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                match retire.load(Ordering::SeqCst) {
                                    1 if raw.is_empty() => return Ok(3),
                                    3 if raw.is_empty() => return Ok(4),
                                    1 | 2 | 3 => return Err(Error::WrongState),
                                    _ => thread::sleep(Duration::from_millis(5)),
                                }
                            }
                            Err(_) => return Err(Error::Storage),
                        }
                        if retire.load(Ordering::SeqCst) == 2 {
                            return Err(Error::WrongState);
                        }
                    }
                })();
                state.store(result.as_ref().copied().unwrap_or(2), Ordering::SeqCst);
                result.map(|_| ())
            })
            .map_err(|_| Error::Storage)?;
        Ok(Self { flag, retirement, join: Some(join) })
    }
    // Called ONLY after current active nonce/grant/peer verification by parent.
    // This is control-reader retirement, NEVER physical drain evidence.
    pub fn retire_for_remote(&self) -> Result<()> {
        if self.flag.load(Ordering::SeqCst) == 2 {
            return Err(Error::WrongState);
        }
        self.retirement.compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| Error::WrongState)?;
        Ok(())
    }
    // Only SAME prepared product parent after verified active loss_request.
    // Reading actual pending input first preserves EOF/malformed/Stop priority.
    pub fn retire_for_transport_loss(&self) -> Result<()> {
        if self.flag.load(Ordering::SeqCst) != 0 { return Err(Error::WrongState); }
        self.retirement.compare_exchange(0,3,Ordering::SeqCst,Ordering::SeqCst)
            .map_err(|_|Error::WrongState)?;
        Ok(())
    }
    pub fn join_transport_loss(&mut self,end:Instant)->Result<ControlRetired> {
        if !self.join(end) || self.flag.load(Ordering::SeqCst)!=4 {return Err(Error::WrongState)}
        Ok(ControlRetired { _private:() })
    }
    pub fn abort(&self) {
        self.retirement.store(2, Ordering::SeqCst);
    }
    pub fn join(&mut self, end: Instant) -> bool {
        join(&mut self.join, end)
    }
    pub fn stopped(&self) -> bool {
        self.flag.load(Ordering::SeqCst) == 1
    }
    pub fn remote_retired(&self) -> bool {
        self.flag.load(Ordering::SeqCst) == 3
    }
}
impl Drop for ParentControl {
    fn drop(&mut self) {
        self.abort();
        let _ = self.join(Instant::now() + Duration::from_secs(1));
    }
}
pub(super) fn join(handle: &mut Option<JoinHandle<Result<()>>>, end: Instant) -> bool {
    while handle.as_ref().is_some_and(|h| !h.is_finished()) && Instant::now() < end {
        thread::sleep(Duration::from_millis(5));
    }
    if handle.as_ref().is_some_and(|h| h.is_finished()) {
        return handle
            .take()
            .is_some_and(|h| matches!(h.join(), Ok(Ok(()))));
    }
    false
}
// No serde/bool/PID constructor. Successful actual joins alone create tokens.
pub(super) struct ControlRetired { _private:() }
pub(super) struct PipesRetired { _private:() }
pub(super) struct Pipes {
    tx: Option<mpsc::SyncSender<Vec<u8>>>,
    wrote: mpsc::Receiver<Result<()>>,
    received: Option<mpsc::Receiver<Vec<u8>>>,
    reader: Option<JoinHandle<Result<()>>>,
    writer: Option<JoinHandle<Result<()>>>,
    pub input_inode: u64,
    pub output_inode: u64,
    session: String,
    seq: u64,
}
impl Pipes {
    pub fn new(mut input: ChildStdin, output: ChildStdout, session: String) -> Result<Self> {
        let input_id = fifo(input.as_raw_fd())?;
        let output_id = fifo(output.as_raw_fd())?;
        if input_id == output_id {
            return Err(Error::Storage);
        }
        let (tx, requests) = mpsc::sync_channel::<Vec<u8>>(1);
        let (done, wrote) = mpsc::sync_channel(1);
        let writer = thread::Builder::new()
            .name("tunnel-child-writer".into())
            .spawn(move || {
                while let Ok(raw) = requests.recv() {
                    if raw.len() > LINE || fifo(input.as_raw_fd())? != input_id {
                        return Err(Error::Storage);
                    }
                    let value = input
                        .write_all(&raw)
                        .and_then(|_| input.flush())
                        .map_err(|_| Error::Storage);
                    if done.send(value).is_err() {
                        return Err(Error::Storage);
                    }
                    value?;
                }
                Ok(()) // drops actual ChildStdin, not a claimed caller close.
            })
            .map_err(|_| Error::Storage)?;
        let (frames, received) = mpsc::sync_channel(0);
        let reader = thread::Builder::new()
            .name("tunnel-child-reader".into())
            .spawn(move || {
                let fd = output.as_raw_fd();
                let mut output = BufReader::with_capacity(1024, output);
                loop {
                    if fifo(fd)? != output_id {
                        return Err(Error::Storage);
                    }
                    let mut raw = Vec::new();
                    let n = output
                        .by_ref()
                        .take((LINE + 1) as u64)
                        .read_until(b'\n', &mut raw)
                        .map_err(|_| Error::Storage)?;
                    if n == 0 {
                        return Ok(());
                    }
                    if raw.len() > LINE || !raw.ends_with(b"\n") {
                        return Err(Error::Quota);
                    }
                    if frames.send(raw).is_err() {
                        return Err(Error::Storage);
                    }
                }
            });
        let reader = match reader {
            Ok(v) => v,
            Err(_) => {
                drop(tx);
                let mut writer = Some(writer);
                let _ = join(&mut writer, Instant::now() + Duration::from_secs(2));
                return Err(Error::Storage);
            }
        };
        Ok(Self {
            tx: Some(tx),
            wrote,
            received: Some(received),
            reader: Some(reader),
            writer: Some(writer),
            input_inode: input_id.1,
            output_inode: output_id.1,
            session,
            seq: 0,
        })
    }
    pub fn call(
        &mut self,
        kind: &str,
        payload: Value,
        replies: &[(&str, &[&str])],
        end: Instant,
        control: &ParentControl,
    ) -> Result<Value> {
        self.seq = self.seq.checked_add(1).ok_or(Error::Quota)?;
        let mut request =
            json!({"format":FORMAT,"session":self.session,"seq":self.seq,"kind":kind});
        for (name, value) in payload.as_object().ok_or(Error::InvalidWire)? {
            if request.get(name).is_some() {
                return Err(Error::InvalidWire);
            }
            request[name] = value.clone();
        }
        self.tx
            .as_ref()
            .ok_or(Error::WrongState)?
            .try_send(canonical(&request)?)
            .map_err(|_| Error::Quota)?;
        wait(&self.wrote, end, control)??;
        let value = parse(&wait(
            self.received.as_ref().ok_or(Error::WrongState)?,
            end,
            control,
        )?)?;
        response(&value, &self.session, self.seq, replies)?;
        Ok(value)
    }
    pub fn close_writer(&mut self) {
        self.tx.take();
    }
    pub fn retire_after_transport_loss(&mut self,end:Instant)->Result<PipesRetired> {
        self.close_writer();
        // Err/panic/still-running are not recast as successful physical joins.
        if !self.joins(end) {return Err(Error::WrongState)}
        Ok(PipesRetired { _private:() })
    }
    pub fn joins(&mut self, end: Instant) -> bool {
        let writer = join(&mut self.writer, end);
        // Child must already have actually exited; expected final EOF permits
        // the reader to join. Drop receiver only AFTER the success join check.
        let reader = join(&mut self.reader, end);
        self.received.take();
        writer && reader
    }
}
fn wait<T>(rx: &mpsc::Receiver<T>, end: Instant, control: &ParentControl) -> Result<T> {
    loop {
        // Typed stop waits for the current bounded exchange to finish; it never
        // abandons a response then mistakes it for a stopped acknowledgement.
        if control.flag.load(Ordering::SeqCst) == 2 || Instant::now() >= end {
            return Err(Error::WrongState);
        }
        match rx.recv_timeout(
            end.saturating_duration_since(Instant::now())
                .min(Duration::from_millis(20)),
        ) {
            Ok(v) => return Ok(v),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => return Err(Error::WrongState),
        }
    }
}

pub(super) fn response(
    value: &Value,
    session: &str,
    seq: u64,
    replies: &[(&str, &[&str])],
) -> Result<()> {
    if value["format"] != FORMAT
        || value["session"] != session
        || value["seq"].as_u64() != Some(seq)
    {
        return Err(Error::Replay);
    }
    let kind = value["kind"].as_str().ok_or(Error::InvalidWire)?;
    let allowed = replies
        .iter()
        .find(|v| v.0 == kind)
        .ok_or(Error::WrongState)?;
    let mut names = vec!["format", "session", "seq", "kind"];
    names.extend_from_slice(allowed.1);
    fields(value, &names)
}
