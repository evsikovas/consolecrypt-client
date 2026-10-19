//! Device-local shell discovery and a real PTY/ConPTY transport. The caller
//! supplies only a discovered profile id, never a command line or environment.

use crate::{OpenedShell, ShellLifetime, TerminalError, TerminalSize};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalShellKind {
    Zsh,
    Powershell,
    Cmd,
    Wsl,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalShellProfile {
    pub id: String,
    pub kind: LocalShellKind,
    pub label: String,
    pub shell: String,
    pub distribution: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalShellProfiles {
    pub profiles: Vec<LocalShellProfile>,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug)]
struct LaunchProfile {
    info: LocalShellProfile,
    program: std::path::PathBuf,
    args: Vec<String>,
}

impl LaunchProfile {
    fn new(
        id: &str,
        kind: LocalShellKind,
        label: &str,
        shell: &str,
        program: impl Into<std::path::PathBuf>,
        args: &[&str],
    ) -> Self {
        Self {
            info: LocalShellProfile {
                id: id.into(),
                kind,
                label: label.into(),
                shell: shell.into(),
                distribution: None,
            },
            program: program.into(),
            args: args.iter().map(|s| (*s).into()).collect(),
        }
    }
}

async fn discover() -> Result<Vec<LaunchProfile>, TerminalError> {
    #[cfg(target_os = "macos")]
    {
        Ok(vec![LaunchProfile::new(
            "macos:zsh",
            LocalShellKind::Zsh,
            "zsh",
            "zsh",
            "/bin/zsh",
            &["-il"],
        )])
    }
    #[cfg(target_os = "windows")]
    {
        use tokio::io::AsyncReadExt;
        let system = std::env::var_os("SystemRoot")
            .map(std::path::PathBuf::from)
            .ok_or_else(|| TerminalError::Open("Windows system directory is unavailable".into()))?
            .join("System32");
        let mut profiles = vec![
            LaunchProfile::new(
                "windows:powershell",
                LocalShellKind::Powershell,
                "Windows PowerShell",
                "powershell",
                system.join("WindowsPowerShell/v1.0/powershell.exe"),
                &["-NoLogo"],
            ),
            LaunchProfile::new(
                "windows:cmd",
                LocalShellKind::Cmd,
                "Command Prompt",
                "cmd",
                system.join("cmd.exe"),
                &[],
            ),
        ];
        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            let pwsh = std::path::PathBuf::from(program_files).join("PowerShell/7/pwsh.exe");
            if pwsh.is_file() {
                profiles.insert(
                    0,
                    LaunchProfile::new(
                        "windows:pwsh",
                        LocalShellKind::Powershell,
                        "PowerShell 7",
                        "powershell",
                        pwsh,
                        &["-NoLogo"],
                    ),
                );
            }
        }
        profiles.retain(|p| p.program.is_file());
        let wsl = system.join("wsl.exe");
        if wsl.is_file() {
            use std::os::windows::process::CommandExt;
            let mut command = std::process::Command::new(&wsl);
            command
                .args(["--list", "--quiet"])
                .creation_flags(0x08000000);
            let mut command = tokio::process::Command::from(command);
            command
                .kill_on_drop(true)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null());
            if let Ok(mut child) = command.spawn() {
                let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    let mut bytes = Vec::new();
                    child
                        .stdout
                        .take()
                        .expect("piped stdout")
                        .take(128 * 1024 + 1)
                        .read_to_end(&mut bytes)
                        .await?;
                    if bytes.len() > 128 * 1024 {
                        return Err(std::io::Error::other("WSL profile list too large"));
                    }
                    let status = child.wait().await?;
                    Ok::<_, std::io::Error>((status, bytes))
                })
                .await;
                if let Ok(Ok((status, bytes))) = result {
                    if status.success() {
                        for name in decode_wsl_names(&bytes) {
                            let mut profile = LaunchProfile::new(
                                &format!("wsl:{name}"),
                                LocalShellKind::Wsl,
                                &format!("WSL · {name}"),
                                "sh",
                                &wsl,
                                &["--distribution", &name, "--cd", "~"],
                            );
                            profile.info.distribution = Some(name);
                            profiles.push(profile);
                        }
                    }
                } else {
                    let _ = child.kill().await;
                }
            }
        }
        Ok(profiles)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    Ok(Vec::new())
}

/// `wsl --list --quiet` emits UTF-16LE on many Windows releases, UTF-8 on
/// others. Preserve exact distro names and pass them as one argv element.
#[cfg(any(target_os = "windows", test))]
fn decode_wsl_names(bytes: &[u8]) -> Vec<String> {
    let text =
        if bytes.starts_with(&[0xff, 0xfe]) || bytes.iter().skip(1).step_by(2).any(|b| *b == 0) {
            let bytes = bytes.strip_prefix(&[0xff, 0xfe]).unwrap_or(bytes);
            String::from_utf16_lossy(
                &bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect::<Vec<_>>(),
            )
        } else {
            String::from_utf8_lossy(bytes).into_owned()
        };
    let mut names = Vec::new();
    for name in text.trim_start_matches('\u{feff}').lines().map(str::trim) {
        if !name.is_empty()
            && !name.chars().any(char::is_control)
            && !names.iter().any(|s| s == name)
        {
            names.push(name.to_owned());
        }
    }
    names
}

pub async fn profiles() -> Result<LocalShellProfiles, TerminalError> {
    let profiles: Vec<_> = discover().await?.into_iter().map(|p| p.info).collect();
    let unavailable_reason = if profiles.is_empty() {
        Some("unsupported_platform".into())
    } else {
        None
    };
    Ok(LocalShellProfiles {
        profiles,
        unavailable_reason,
    })
}

pub async fn open(
    profile_id: &str,
    size: TerminalSize,
) -> Result<(LocalShellProfile, OpenedShell), TerminalError> {
    let profile = discover()
        .await?
        .into_iter()
        .find(|p| p.info.id == profile_id)
        .ok_or_else(|| TerminalError::Open("local shell profile is unavailable".into()))?;
    let info = profile.info.clone();
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        let opened = tokio::task::spawn_blocking(move || transport::spawn(profile, size))
            .await
            .map_err(|e| TerminalError::Open(e.to_string()))??;
        Ok((info, opened))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = (profile, info, size);
        Err(TerminalError::Open(
            "local shells are unavailable on this platform".into(),
        ))
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
mod transport {
    use super::*;
    use bytes::Bytes;
    use cc_ssh_core::{ShellChannel, ShellEvent, ShellInput};
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    use std::io::{Read, Write};
    use std::sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    };
    use std::time::{Duration, Instant};

    const IO_POLL: Duration = Duration::from_millis(10);
    const EXIT_DRAIN: Duration = Duration::from_millis(250);
    const MAX_EXIT_DRAIN: Duration = Duration::from_secs(5);

    // portable-pty exposes the owned master fd but not cancellable Unix I/O.
    // Keep the small syscall boundary here; no external PID enumeration or
    // signalling outside the session created by spawn_command/setsid.
    #[cfg(unix)]
    #[allow(unsafe_code)]
    mod unix {
        use portable_pty::MasterPty;
        use std::io;

        pub(super) fn nonblocking(master: &dyn MasterPty) -> io::Result<()> {
            let fd = master
                .as_raw_fd()
                .ok_or_else(|| io::Error::other("PTY master fd unavailable"))?;
            // SAFETY: master owns this open descriptor for the entire call.
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags == -1
                || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1
            {
                return Err(io::Error::last_os_error());
            }
            // dup'd reader/writer descriptors share these file status flags.
            Ok(())
        }

        pub(super) fn owned_foreground(
            master: &dyn MasterPty,
            session: Option<u32>,
        ) -> Option<libc::pid_t> {
            let group = master.process_group_leader()?;
            in_session(group, session).then_some(group)
        }

        fn in_session(group: libc::pid_t, session: Option<u32>) -> bool {
            let Some(session) = session.and_then(|pid| libc::pid_t::try_from(pid).ok()) else {
                return false;
            };
            // SAFETY: getpgrp/getsid only query process metadata. Checking the
            // session again before signalling prevents targeting an unrelated
            // group, including the application/runner's own process group.
            group > 1
                && session > 1
                && unsafe { libc::getpgrp() != group && libc::getsid(group) == session }
        }

        pub(super) fn signal_group(group: Option<libc::pid_t>, session: Option<u32>, signal: i32) {
            if let Some(group) = group.filter(|group| in_session(*group, session)) {
                // SAFETY: this is a positive, verified PGID from our own PTY,
                // not an arbitrary PID or process group supplied by the UI.
                unsafe { libc::kill(-group, signal) };
            }
        }
    }

    #[derive(Debug)]
    struct State {
        closing: AtomicBool,
        read_stop: AtomicBool,
        output_progress: AtomicU64,
        size: Mutex<Option<TerminalSize>>,
    }
    #[derive(Debug)]
    struct Lifetime(Arc<State>);
    impl ShellLifetime for Lifetime {
        fn close(&self) {
            self.0.closing.store(true, Ordering::Release);
        }
    }
    impl Drop for Lifetime {
        fn drop(&mut self) {
            self.close();
        }
    }

    fn pty_size(size: TerminalSize) -> PtySize {
        // ConPTY uses signed 16-bit cells; zero is invalid on both platforms.
        PtySize {
            cols: size.cols.clamp(1, 32767) as u16,
            rows: size.rows.clamp(1, 32767) as u16,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    fn forward_output(
        events: &tokio::sync::mpsc::Sender<ShellEvent>,
        state: &State,
        mut event: ShellEvent,
    ) {
        loop {
            if state.closing.load(Ordering::Acquire) {
                return; // Continue draining ConPTY without filling a closed UI queue.
            }
            match events.try_send(event) {
                Ok(()) => {
                    state.output_progress.fetch_add(1, Ordering::Release);
                    return;
                }
                Err(tokio::sync::mpsc::error::TrySendError::Full(pending)) => {
                    event = pending;
                    std::thread::sleep(IO_POLL);
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    state.closing.store(true, Ordering::Release);
                    return;
                }
            }
        }
    }

    fn final_event(events: &tokio::sync::mpsc::Sender<ShellEvent>, mut event: ShellEvent) {
        let deadline = Instant::now() + EXIT_DRAIN;
        loop {
            match events.try_send(event) {
                Ok(()) | Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => return,
                Err(tokio::sync::mpsc::error::TrySendError::Full(pending)) => {
                    if Instant::now() >= deadline {
                        // Dropping the last sender also ends a receiver that
                        // resumes after close; never retain a worker for it.
                        return;
                    }
                    event = pending;
                    std::thread::sleep(IO_POLL);
                }
            }
        }
    }

    async fn queue_write(
        tx: &tokio::sync::mpsc::Sender<Bytes>,
        state: &State,
        bytes: Bytes,
    ) -> bool {
        loop {
            if state.closing.load(Ordering::Acquire) {
                return false;
            }
            tokio::select! {
                result = tx.send(bytes.clone()) => return result.is_ok(),
                _ = tokio::time::sleep(IO_POLL) => {}
            }
        }
    }

    pub(super) fn spawn(
        profile: LaunchProfile,
        size: TerminalSize,
    ) -> Result<OpenedShell, TerminalError> {
        let error = |e: anyhow::Error| TerminalError::Open(e.to_string());
        let pair = native_pty_system().openpty(pty_size(size)).map_err(error)?;
        #[cfg(unix)]
        unix::nonblocking(pair.master.as_ref()).map_err(|e| TerminalError::Open(e.to_string()))?;
        let mut command = CommandBuilder::new(&profile.program);
        command.args(&profile.args);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        #[cfg(unix)]
        {
            if let Some(home) = std::env::var_os("HOME") {
                command.cwd(home);
            }
            if std::env::var_os("LANG").is_none() && std::env::var_os("LC_ALL").is_none() {
                command.env("LANG", "en_US.UTF-8");
            }
        }
        #[cfg(windows)]
        if let Some(home) = std::env::var_os("USERPROFILE") {
            command.cwd(home);
        }
        let mut reader = pair.master.try_clone_reader().map_err(error)?;
        let mut writer = pair.master.take_writer().map_err(error)?;
        let mut child = pair.slave.spawn_command(command).map_err(error)?;
        drop(pair.slave);
        let state = Arc::new(State {
            closing: AtomicBool::new(false),
            read_stop: AtomicBool::new(false),
            output_progress: AtomicU64::new(0),
            size: Mutex::new(None),
        });
        let (channel, mut remote) = ShellChannel::pair(64);
        let (write_tx, mut write_rx) = tokio::sync::mpsc::channel::<Bytes>(16);

        // Blocking OS reads and writes run independently. In particular the
        // ConPTY close path must keep draining output while closing its handle.
        let read_events = remote.events.clone();
        let read_state = state.clone();
        let reader_thread = std::thread::spawn(move || {
            let mut buffer = [0_u8; 16 * 1024];
            loop {
                #[cfg(unix)]
                if read_state.read_stop.load(Ordering::Acquire) {
                    break;
                }
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => {
                        forward_output(
                            &read_events,
                            &read_state,
                            ShellEvent::Data(Bytes::copy_from_slice(&buffer[..n])),
                        );
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(IO_POLL)
                    }
                    Err(_) => break, // Unix PTY EIO is normal after the slave exits.
                }
            }
        });
        let write_state = state.clone();
        let writer_thread = std::thread::spawn(move || {
            while let Some(bytes) = write_rx.blocking_recv() {
                let mut remaining = &bytes[..];
                while !remaining.is_empty() {
                    if write_state.closing.load(Ordering::Acquire) {
                        return;
                    }
                    match writer.write(remaining) {
                        Ok(0) => {
                            write_state.closing.store(true, Ordering::Release);
                            return;
                        }
                        Ok(n) => remaining = &remaining[n..],
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(IO_POLL)
                        }
                        Err(_) => {
                            write_state.closing.store(true, Ordering::Release);
                            return;
                        }
                    }
                }
                if writer.flush().is_err() {
                    write_state.closing.store(true, Ordering::Release);
                    return;
                }
            }
        });
        let input_state = state.clone();
        // Use the runtime captured by spawn_blocking, never a blocking recv on
        // an async worker. The close flag bypasses a full data queue.
        tokio::runtime::Handle::current().spawn(async move {
            loop {
                if input_state.closing.load(Ordering::Acquire) {
                    break;
                }
                let input = tokio::select! {
                    input = remote.inputs.recv() => input,
                    _ = tokio::time::sleep(Duration::from_millis(25)) => {
                        if input_state.closing.load(Ordering::Acquire) { break; }
                        continue;
                    }
                };
                match input {
                    Some(ShellInput::Data(data)) => {
                        // Bound individual writes as well as the queue length.
                        for chunk in data.chunks(16 * 1024) {
                            if !queue_write(&write_tx, &input_state, Bytes::copy_from_slice(chunk))
                                .await
                            {
                                return;
                            }
                        }
                    }
                    Some(ShellInput::Resize { cols, rows }) => {
                        *input_state.size.lock().unwrap_or_else(|p| p.into_inner()) =
                            Some(TerminalSize { cols, rows });
                    }
                    Some(ShellInput::Eof) => {
                        if !queue_write(&write_tx, &input_state, Bytes::from_static(b"\x04")).await
                        {
                            break;
                        }
                    }
                    Some(ShellInput::Close) | None => {
                        input_state.closing.store(true, Ordering::Release);
                        break;
                    }
                }
            }
        });
        let supervise_state = state.clone();
        std::thread::spawn(move || {
            let mut killed = false;
            let status = loop {
                if supervise_state.closing.load(Ordering::Acquire) && !killed {
                    #[cfg(unix)]
                    let foreground =
                        unix::owned_foreground(pair.master.as_ref(), child.process_id());
                    #[cfg(unix)]
                    // portable-pty kills only the shell PID. A foreground job
                    // may ignore HUP and keep the slave open after its parent
                    // exits. Kill its verified group before reaping the shell
                    // so its session identity cannot be recycled in between.
                    unix::signal_group(foreground, child.process_id(), libc::SIGKILL);
                    let _ = child.kill();
                    killed = true;
                }
                if let Some(size) = supervise_state
                    .size
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .take()
                {
                    let _ = pair.master.resize(pty_size(size));
                }
                match child.try_wait() {
                    Ok(Some(status)) => break Some(status),
                    Err(_) => break None,
                    Ok(None) => std::thread::sleep(Duration::from_millis(25)),
                }
            };
            // Preserve final output on normal exit, but never wait forever for
            // a background process retaining the slave or an unread UI queue.
            let maximum_drain = Instant::now() + MAX_EXIT_DRAIN;
            let mut idle_deadline = Instant::now() + EXIT_DRAIN;
            let mut progress = supervise_state.output_progress.load(Ordering::Acquire);
            while !supervise_state.closing.load(Ordering::Acquire)
                && !reader_thread.is_finished()
                && Instant::now() < idle_deadline
                && Instant::now() < maximum_drain
            {
                std::thread::sleep(IO_POLL);
                let current = supervise_state.output_progress.load(Ordering::Acquire);
                if current != progress {
                    progress = current;
                    idle_deadline = Instant::now() + EXIT_DRAIN;
                }
            }
            supervise_state.closing.store(true, Ordering::Release);
            // Keep draining even during forced Unix termination: closing a
            // slave with pending output can otherwise delay process exit.
            supervise_state.read_stop.store(true, Ordering::Release);
            drop(pair.master);
            let deadline = Instant::now() + Duration::from_secs(1);
            while (!reader_thread.is_finished() || !writer_thread.is_finished())
                && Instant::now() < deadline
            {
                std::thread::sleep(IO_POLL);
            }
            for worker in [reader_thread, writer_thread] {
                if worker.is_finished() {
                    let _ = worker.join();
                } else {
                    tracing::warn!("PTY worker did not stop after native close");
                }
            }
            if let Some(status) = status {
                final_event(&remote.events, ShellEvent::ExitStatus(status.exit_code()));
            }
            final_event(&remote.events, ShellEvent::Closed);
        });
        Ok(OpenedShell {
            channel,
            session: None,
            lifetime: Some(Arc::new(Lifetime(state))),
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn full_output_queue_cancels_without_waiting_for_a_consumer() {
            let (events, _unread) = tokio::sync::mpsc::channel(1);
            events
                .try_send(ShellEvent::Data(Bytes::from_static(b"full")))
                .unwrap();
            let state = Arc::new(State {
                closing: AtomicBool::new(false),
                read_stop: AtomicBool::new(false),
                output_progress: AtomicU64::new(0),
                size: Mutex::new(None),
            });
            let producer_state = state.clone();
            let producer = std::thread::spawn(move || {
                forward_output(
                    &events,
                    &producer_state,
                    ShellEvent::Data(Bytes::from_static(b"pending")),
                );
                final_event(&events, ShellEvent::Closed);
            });
            std::thread::sleep(IO_POLL * 2);
            assert!(
                !producer.is_finished(),
                "fixture must actually exert backpressure"
            );
            state.closing.store(true, Ordering::Release);
            let deadline = Instant::now() + Duration::from_secs(1);
            while !producer.is_finished() && Instant::now() < deadline {
                std::thread::sleep(IO_POLL);
            }
            assert!(
                producer.is_finished(),
                "close must release a full output queue without draining it"
            );
            producer.join().unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_wsl_names_from_utf16_and_utf8() {
        let text = "Ubuntu\r\nDebian\r\nDistro with space\r\nUbuntu\r\n";
        let utf16: Vec<_> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(
            decode_wsl_names(&utf16),
            vec!["Ubuntu", "Debian", "Distro with space"]
        );
        assert_eq!(decode_wsl_names(text.as_bytes()), decode_wsl_names(&utf16));
    }

    #[cfg(unix)]
    async fn native_shell() -> OpenedShell {
        tokio::task::spawn_blocking(|| {
            transport::spawn(
                LaunchProfile::new(
                    "test:sh",
                    LocalShellKind::Zsh,
                    "test",
                    "sh",
                    "/bin/sh",
                    &["-i"],
                ),
                TerminalSize { cols: 80, rows: 24 },
            )
        })
        .await
        .unwrap()
        .unwrap()
    }

    #[cfg(unix)]
    async fn read_until(reader: &mut cc_ssh_core::ShellReader, needle: &str) -> String {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let mut output = String::new();
            loop {
                match reader.recv().await {
                    Some(cc_ssh_core::ShellEvent::Data(bytes)) => {
                        output.push_str(&String::from_utf8_lossy(&bytes));
                        if output.contains(needle) {
                            return output;
                        }
                    }
                    event => panic!("shell ended before expected output: {event:?}, {output:?}"),
                }
            }
        })
        .await
        .expect("native PTY response timed out")
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn native_pty_is_interactive_resizes_preserves_unicode_and_exit_output() {
        let mut opened = native_shell().await;
        opened
            .channel
            .writer
            .write("stty -echo; test -t 0 && test -t 1 && printf 'PTY_%s\\n' READY\r")
            .await
            .unwrap();
        read_until(&mut opened.channel.reader, "PTY_READY").await;
        opened.channel.writer.resize(117, 39).await.unwrap();
        // Resize is asynchronous; ask the slave until it observes the new size.
        opened.channel.writer.write("while [ \"$(stty size)\" != '39 117' ]; do sleep 0.02; done; printf 'SIZE_%s\\n' OK\r").await.unwrap();
        read_until(&mut opened.channel.reader, "SIZE_OK").await;
        opened
            .channel
            .writer
            .write("printf '\\033[31mПривет 世界\\033[0m\\n'; exit 7\r")
            .await
            .unwrap();
        let mut output = Vec::new();
        let mut exit = None;
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while let Some(event) = opened.channel.reader.recv().await {
                match event {
                    cc_ssh_core::ShellEvent::Data(bytes) => output.extend_from_slice(&bytes),
                    cc_ssh_core::ShellEvent::ExitStatus(code) => exit = Some(code),
                    cc_ssh_core::ShellEvent::Closed => break,
                    _ => {}
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(exit, Some(7));
        assert!(String::from_utf8(output)
            .unwrap()
            .contains("\x1b[31mПривет 世界\x1b[0m"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn normal_exit_drains_large_final_output_for_a_slow_consumer() {
        let mut opened = native_shell().await;
        opened
            .channel
            .writer
            .write("stty -echo; printf 'DRAIN_%s\\n' READY\r")
            .await
            .unwrap();
        read_until(&mut opened.channel.reader, "DRAIN_READY").await;
        opened.channel.writer.write(
            "awk 'BEGIN {for (i = 0; i < 524288; i++) printf \"@\"}'; printf '\\nFINAL_TAIL\\n'; exit 9\r",
        ).await.unwrap();
        // Let the reader fill its bounded queue before the consumer resumes.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let mut output = Vec::new();
        let mut exit = None;
        let mut chunks = 0;
        let started = tokio::time::Instant::now();
        let drained = tokio::time::timeout(std::time::Duration::from_secs(20), async {
            while let Some(event) = opened.channel.reader.recv().await {
                match event {
                    cc_ssh_core::ShellEvent::Data(bytes) => {
                        output.extend_from_slice(&bytes);
                        chunks += 1;
                        // PTY reads can fragment identical output differently
                        // across machines. Pace bytes (400 KiB/s), not events:
                        // a 10 ms delay per tiny chunk can itself exceed 20 s.
                        let elapsed_budget =
                            std::time::Duration::from_micros(output.len() as u64 * 10_000 / 4096);
                        let next = started + elapsed_budget;
                        if next > tokio::time::Instant::now() {
                            tokio::time::sleep_until(next).await;
                        }
                    }
                    cc_ssh_core::ShellEvent::ExitStatus(code) => exit = Some(code),
                    cc_ssh_core::ShellEvent::Closed => break,
                    _ => {}
                }
            }
        })
        .await;
        assert!(
            drained.is_ok(),
            "normal exit drain timed out: {} bytes in {chunks} chunks, exit={exit:?}",
            output.len()
        );
        eprintln!(
            "PTY drain: {} bytes in {chunks} chunks, elapsed={:?}",
            output.len(),
            started.elapsed()
        );
        assert_eq!(output.iter().filter(|byte| **byte == b'@').count(), 524288);
        assert!(String::from_utf8_lossy(&output).contains("FINAL_TAIL"));
        assert_eq!(exit, Some(9));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn native_pty_ctrl_c_and_owned_lifetime_close() {
        let mut opened = native_shell().await;
        opened.channel.writer.write("stty -echo; PS1='CC_PROMPT> '; /bin/sh -c 'printf \"WAIT_%s\\n\" READY; exec sleep 30'\r").await.unwrap();
        read_until(&mut opened.channel.reader, "WAIT_READY").await;
        opened.channel.writer.write(vec![3]).await.unwrap();
        read_until(&mut opened.channel.reader, "CC_PROMPT> ").await;
        opened
            .channel
            .writer
            .write("printf 'INPUT_%s\\n' WORKS\r")
            .await
            .unwrap();
        read_until(&mut opened.channel.reader, "INPUT_WORKS").await;
        opened.lifetime.as_ref().unwrap().close();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while let Some(event) = opened.channel.reader.recv().await {
                if matches!(event, cc_ssh_core::ShellEvent::Closed) {
                    return;
                }
            }
        })
        .await
        .expect("closing the owned process must end the output stream");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn closing_a_foreground_job_does_not_leave_pty_reader_alive() {
        let mut opened = native_shell().await;
        opened
            .channel
            .writer
            .write("stty -echo; printf 'CLOSE_%s\\n' READY; sleep 30\r")
            .await
            .unwrap();
        read_until(&mut opened.channel.reader, "CLOSE_READY").await;
        opened.lifetime.as_ref().unwrap().close();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while let Some(event) = opened.channel.reader.recv().await {
                if matches!(event, cc_ssh_core::ShellEvent::Closed) {
                    return;
                }
            }
        })
        .await
        .expect("owned foreground jobs must be closed with their terminal");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn closing_kills_the_owned_hup_ignoring_foreground_group() {
        let mut opened = native_shell().await;
        opened.channel.writer.write(
            "stty -echo; /bin/sh -c 'trap \"\" HUP; printf \"HUP_PID=%s\\nHUP_READY\\n\" \"$$\"; exec sleep 30'\r",
        ).await.unwrap();
        let output = read_until(&mut opened.channel.reader, "HUP_READY\r\n").await;
        let pid = output
            .lines()
            .find_map(|line| line.strip_prefix("HUP_PID="))
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();
        opened.lifetime.as_ref().unwrap().close();
        let closed = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while let Some(event) = opened.channel.reader.recv().await {
                if matches!(event, cc_ssh_core::ShellEvent::Closed) {
                    break;
                }
            }
        })
        .await;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let alive = || {
            std::process::Command::new("/bin/kill")
                .args(["-0", &pid.to_string()])
                .output()
                .unwrap()
                .status
                .success()
        };
        while alive() && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let leaked = alive();
        if leaked {
            // This PID came directly from this test's own foreground child.
            // Clean up a failed regression without leaving its 30-second sleep.
            let _ = std::process::Command::new("/bin/kill")
                .args(["-KILL", &pid.to_string()])
                .output();
        }
        assert!(
            !leaked,
            "HUP-ignoring foreground process survived terminal close"
        );
        closed.expect("foreground process must not retain the PTY reader");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn close_cancels_a_blocked_large_paste_and_eof_queue() {
        let mut opened = native_shell().await;
        opened
            .channel
            .writer
            .write("stty -echo; printf 'PASTE_%s\\n' READY; sleep 30\r")
            .await
            .unwrap();
        read_until(&mut opened.channel.reader, "PASTE_READY").await;
        opened
            .channel
            .writer
            .write(vec![b'x'; 4 * 1024 * 1024])
            .await
            .unwrap();
        opened.channel.writer.eof().await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        opened.lifetime.as_ref().unwrap().close();
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while let Some(event) = opened.channel.reader.recv().await {
                if matches!(event, cc_ssh_core::ShellEvent::Closed) {
                    break;
                }
            }
        })
        .await
        .expect("full input and EOF queues must not delay native close");
    }

    #[cfg(windows)]
    async fn windows_roundtrip(id: &str, command: &'static str) {
        let mut profile = discover()
            .await
            .unwrap()
            .into_iter()
            .find(|p| p.info.id == id)
            .unwrap();
        if profile.info.kind == LocalShellKind::Powershell {
            profile.args.push("-NoProfile".into());
        }
        let mut opened = tokio::task::spawn_blocking(move || {
            transport::spawn(
                profile,
                TerminalSize {
                    cols: 117,
                    rows: 39,
                },
            )
        })
        .await
        .unwrap()
        .unwrap();
        opened.channel.writer.resize(132, 43).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        opened.channel.writer.write(command).await.unwrap();
        let mut output = Vec::new();
        let mut exit = None;
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            while let Some(event) = opened.channel.reader.recv().await {
                match event {
                    cc_ssh_core::ShellEvent::Data(bytes) => output.extend_from_slice(&bytes),
                    cc_ssh_core::ShellEvent::ExitStatus(code) => exit = Some(code),
                    cc_ssh_core::ShellEvent::Closed => break,
                    _ => {}
                }
            }
        })
        .await
        .expect("ConPTY must drain output and close without deadlock");
        assert_eq!(exit, Some(7));
        let output = String::from_utf8(output).expect("ConPTY output must remain valid UTF-8");
        assert!(output.contains("CC_WINDOWS"));
        assert!(output.contains("Привет 世界"));
        if id == "windows:powershell" {
            assert!(output.contains("SIZE_132x43"));
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn native_conpty_powershell_output_and_exit() {
        windows_roundtrip(
            "windows:powershell",
            "Write-Output ('CC_' + 'WINDOWS'); Write-Output (-join ([char[]]@(1055,1088,1080,1074,1077,1090,32,19990,30028))); Write-Output ('SIZE_' + [Console]::WindowWidth + 'x' + [Console]::WindowHeight); exit 7\r",
        )
        .await;
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn native_conpty_cmd_output_and_exit() {
        windows_roundtrip(
            "windows:cmd",
            "@echo off\rchcp 65001>nul\recho CC_WINDOWS\rset CC_UNICODE=Привет\recho %CC_UNICODE% 世界\rexit 7\r",
        ).await;
    }
}
