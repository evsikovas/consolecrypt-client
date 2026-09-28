//! Best-effort extraction of the last typed command and the last error
//! output for the AI layer (`AiContextProvider::get_last_command` /
//! `get_last_error`). Local only; the AI layer must still run everything
//! through its sanitizer.
//!
//! * Typed input is tracked line by line (backspace, Ctrl-U/W/C handled).
//!   When the line was edited in ways we cannot follow (history recall,
//!   cursor movement, tab completion) the command is taken from the echoed
//!   output line instead, with a common prompt prefix stripped.
//! * Input typed right after a password / passphrase prompt is never
//!   recorded.
//! * After a command is submitted, its output (ANSI-stripped, capped) is
//!   scanned for error-looking lines.

use zeroize::Zeroize;

const MAX_LINE: usize = 4096;
const MAX_CAPTURE: usize = 64 * 1024;
const MAX_ERROR: usize = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnsiState {
    None,
    Esc,
    Csi,
    Ss3,
    Osc,
    OscEsc,
}

/// ANSI escape stripper (stateful across chunks).
#[derive(Debug, Clone)]
pub struct AnsiStripper {
    state: AnsiState,
}

impl Default for AnsiStripper {
    fn default() -> Self {
        Self {
            state: AnsiState::None,
        }
    }
}

impl AnsiStripper {
    /// Strip escape sequences; keeps printable text, `\n`, `\r`, `\t`, `\x08`.
    pub fn strip(&mut self, input: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(input.len());
        for &b in input {
            self.state = match self.state {
                AnsiState::None => match b {
                    0x1b => AnsiState::Esc,
                    b'\n' | b'\r' | b'\t' | 0x08 => {
                        out.push(b);
                        AnsiState::None
                    }
                    b if b < 0x20 || b == 0x7f => AnsiState::None,
                    b => {
                        out.push(b);
                        AnsiState::None
                    }
                },
                AnsiState::Esc => match b {
                    b'[' => AnsiState::Csi,
                    b']' => AnsiState::Osc,
                    b'O' => AnsiState::Ss3,
                    _ => AnsiState::None,
                },
                AnsiState::Csi => {
                    if (0x40..=0x7e).contains(&b) {
                        AnsiState::None
                    } else {
                        AnsiState::Csi
                    }
                }
                AnsiState::Ss3 => AnsiState::None,
                AnsiState::Osc => match b {
                    0x07 => AnsiState::None,
                    0x1b => AnsiState::OscEsc,
                    _ => AnsiState::Osc,
                },
                AnsiState::OscEsc => {
                    if b == b'\\' {
                        AnsiState::None
                    } else {
                        AnsiState::Osc
                    }
                }
            };
        }
        out
    }
}

/// Is this (lower-cased, trimmed) line a secret prompt?
pub fn looks_like_secret_prompt(line: &str) -> bool {
    let l = line.trim().to_ascii_lowercase();
    if l.is_empty() {
        return false;
    }
    let ends_prompt = l.ends_with(':') || l.ends_with(": ") || l.ends_with('?');
    ends_prompt
        && (l.contains("password")
            || l.contains("passphrase")
            || l.contains("passwort")
            || l.contains("пароль")
            || l.contains("verification code")
            || l.contains("otp")
            || l.contains("pin"))
}

/// Does this output line look like an error message?
pub fn looks_like_error(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    const PATTERNS: &[&str] = &[
        "error",
        "command not found",
        "no such file or directory",
        "permission denied",
        "not found",
        "fatal:",
        "failed",
        "failure",
        "cannot ",
        "can't ",
        "unable to",
        "segmentation fault",
        "traceback (most recent call last)",
        "exception",
        "refused",
        "invalid",
        "denied",
        "panic",
        "timed out",
        "usage:",
    ];
    PATTERNS.iter().any(|p| l.contains(p))
}

/// Strip a shell prompt prefix from an echoed line (`user@h:~$ cmd` → `cmd`).
pub fn strip_prompt(line: &str) -> &str {
    let mut best: Option<usize> = None;
    for marker in ["$ ", "# ", "> ", "% ", "❯ ", "➜ "] {
        if let Some(pos) = line.rfind(marker) {
            let end = pos + marker.len();
            if best.is_none_or(|b| end > b) {
                best = Some(end);
            }
        }
    }
    match best {
        Some(end) if end <= line.len() => line[end..].trim(),
        _ => line.trim(),
    }
}

/// Tracks input/output of one terminal session.
#[derive(Debug, Default)]
pub struct CommandTracker {
    line: Vec<u8>,
    uncertain: bool,
    input_esc: InputState,
    secret_input: bool,
    output_line: String,
    stripper: AnsiStripper,
    /// Output of the current command is being scanned.
    capturing: bool,
    captured: usize,
    echo_skipped: bool,
    current_errors: Vec<String>,
    last_command: Option<String>,
    last_error: Option<String>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum InputState {
    #[default]
    None,
    Esc,
    Csi,
}

impl CommandTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn last_command(&self) -> Option<&str> {
        self.last_command.as_deref()
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Bytes typed by the user (sent to the remote shell).
    pub fn on_input(&mut self, data: &[u8]) {
        for &b in data {
            match self.input_esc {
                InputState::Esc => {
                    self.input_esc = if b == b'[' || b == b'O' {
                        InputState::Csi
                    } else {
                        InputState::None
                    };
                    self.uncertain = true;
                    continue;
                }
                InputState::Csi => {
                    if (0x40..=0x7e).contains(&b) {
                        self.input_esc = InputState::None;
                    }
                    continue;
                }
                InputState::None => {}
            }
            match b {
                0x1b => self.input_esc = InputState::Esc,
                b'\r' | b'\n' => self.commit(),
                0x7f | 0x08 => {
                    // pop one UTF-8 character
                    while let Some(c) = self.line.pop() {
                        if c & 0xC0 != 0x80 {
                            break;
                        }
                    }
                }
                0x15 => self.clear_line(),
                0x17 => {
                    while self.line.last() == Some(&b' ') {
                        self.line.pop();
                    }
                    while self.line.last().is_some_and(|c| *c != b' ') {
                        self.line.pop();
                    }
                }
                0x03 => {
                    self.clear_line();
                    self.secret_input = false;
                }
                b'\t' => self.uncertain = true,
                b if b < 0x20 => {}
                b => {
                    if self.line.len() < MAX_LINE {
                        self.line.push(b);
                    }
                }
            }
        }
    }

    fn clear_line(&mut self) {
        self.line.zeroize();
        self.line.clear();
        self.uncertain = false;
    }

    fn commit(&mut self) {
        if self.secret_input {
            // Never record what was typed at a password prompt.
            self.secret_input = false;
            self.clear_line();
            return;
        }
        let typed = String::from_utf8_lossy(&self.line).trim().to_string();
        let cmd = if self.uncertain {
            let echoed = strip_prompt(&self.output_line).to_string();
            if echoed.is_empty() {
                typed
            } else {
                echoed
            }
        } else {
            typed
        };
        self.clear_line();
        if !cmd.is_empty() {
            self.last_command = Some(cmd);
            self.capturing = true;
            self.captured = 0;
            self.echo_skipped = false;
            self.current_errors.clear();
        }
    }

    /// Bytes received from the remote side.
    pub fn on_output(&mut self, data: &[u8]) {
        let text = self.stripper.strip(data);
        let text = String::from_utf8_lossy(&text);
        for ch in text.chars() {
            match ch {
                '\n' => {
                    let line = std::mem::take(&mut self.output_line);
                    self.finish_output_line(&line);
                }
                '\r' => {
                    // carriage return without newline: line will be redrawn
                }
                '\u{8}' => {
                    self.output_line.pop();
                }
                c => {
                    if self.output_line.len() < MAX_LINE {
                        self.output_line.push(c);
                    }
                }
            }
        }
        if looks_like_secret_prompt(&self.output_line) {
            self.secret_input = true;
        }
    }

    fn finish_output_line(&mut self, line: &str) {
        if !self.capturing {
            return;
        }
        let cmd = self.last_command.clone().unwrap_or_default();
        let l = line.trim();
        if !self.echo_skipped {
            self.echo_skipped = true;
            if !cmd.is_empty() && l.ends_with(cmd.as_str()) {
                return; // echo of the command line itself
            }
        }
        self.captured += l.len();
        if self.captured > MAX_CAPTURE {
            self.capturing = false;
            return;
        }
        if looks_like_error(l) && self.current_errors.len() < 10 {
            self.current_errors.push(l.to_string());
            self.last_error = Some(truncate(
                format!("$ {cmd}\n{}", self.current_errors.join("\n")),
                MAX_ERROR,
            ));
        }
    }

    /// Forget everything (e.g. when the session is closed).
    pub fn clear(&mut self) {
        self.clear_line();
        self.output_line.zeroize();
        self.capturing = false;
        self.current_errors.clear();
        self.last_command = None;
        self.last_error = None;
    }
}

fn truncate(mut s: String, max: usize) -> String {
    if s.len() > max {
        let mut cut = max;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push('…');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_ansi() {
        let mut s = AnsiStripper::default();
        let out = s.strip(b"\x1b[1;32mgreen\x1b[0m \x1b]0;title\x07text\x1bOA!");
        assert_eq!(out, b"green text!");
        // split across chunks
        let mut s = AnsiStripper::default();
        let mut out = s.strip(b"a\x1b[3");
        out.extend(s.strip(b"1mb"));
        assert_eq!(out, b"ab");
    }

    #[test]
    fn typed_commands_with_editing() {
        let mut t = CommandTracker::new();
        t.on_input(b"lss\x7f -la\r");
        assert_eq!(t.last_command(), Some("ls -la"));
        t.on_input(b"rm -rf /\x15echo hi\r");
        assert_eq!(t.last_command(), Some("echo hi"));
        t.on_input(b"git push origin\x17main\r");
        assert_eq!(t.last_command(), Some("git push main"));
        t.on_input(b"sleep 100\x03");
        assert_eq!(t.last_command(), Some("git push main"));
        t.on_input(b"\r");
        assert_eq!(
            t.last_command(),
            Some("git push main"),
            "empty line ignored"
        );
        t.on_input("echo привет\x7f\r".as_bytes());
        assert_eq!(t.last_command(), Some("echo приве"));
    }

    #[test]
    fn history_recall_uses_echoed_line() {
        let mut t = CommandTracker::new();
        t.on_output(b"alice@host:~$ ");
        t.on_input(b"\x1b[A");
        t.on_output(b"docker ps -a");
        t.on_input(b"\r");
        assert_eq!(t.last_command(), Some("docker ps -a"));
        // tab completion
        t.on_output(b"\r\nalice@host:~$ ");
        t.on_input(b"systemctl sta\t");
        t.on_output(b"systemctl status");
        t.on_input(b" nginx\r");
        t.on_output(b" nginx");
        // echo arrives after Enter in reality; we used the echo seen so far
        assert!(t.last_command().unwrap().starts_with("systemctl status"));
    }

    #[test]
    fn password_input_is_never_recorded() {
        let mut t = CommandTracker::new();
        t.on_input(b"sudo ls\r");
        t.on_output(b"sudo ls\r\n[sudo] password for alice: ");
        t.on_input(b"hunter2\r");
        assert_eq!(t.last_command(), Some("sudo ls"));
        t.on_output(b"\r\nEnter passphrase for key '/home/a/.ssh/id': ");
        t.on_input(b"secret phrase\r");
        assert_eq!(t.last_command(), Some("sudo ls"));
        assert!(!format!("{t:?}").contains("hunter2"));
    }

    #[test]
    fn last_error_extraction() {
        let mut t = CommandTracker::new();
        t.on_input(b"cat /nope\r");
        t.on_output(b"cat /nope\r\ncat: /nope: No such file or directory\r\nalice@h:~$ ");
        assert_eq!(
            t.last_error(),
            Some("$ cat /nope\ncat: /nope: No such file or directory")
        );
        t.on_input(b"ls\r");
        t.on_output(b"ls\r\nfile1 file2\r\n");
        assert_eq!(t.last_command(), Some("ls"));
        // last error stays from the previous failing command
        assert!(t.last_error().unwrap().contains("/nope"));
        t.on_input(b"make\r");
        t.on_output(
            b"make\r\n\x1b[31merror: missing separator\x1b[0m\r\nmake: *** [all] Error 2\r\n",
        );
        let e = t.last_error().unwrap();
        assert!(e.starts_with("$ make\n"), "{e}");
        assert!(e.contains("missing separator"), "{e}");
        assert!(e.contains("Error 2"), "{e}");
        t.clear();
        assert!(t.last_error().is_none());
    }

    #[test]
    fn prompt_stripping() {
        assert_eq!(strip_prompt("alice@h:~$ ls -la"), "ls -la");
        assert_eq!(strip_prompt("root@h:/# apt update"), "apt update");
        assert_eq!(strip_prompt("PS C:\\> dir"), "dir");
        assert_eq!(strip_prompt("plain"), "plain");
        assert!(looks_like_secret_prompt("Password: "));
        assert!(looks_like_secret_prompt("[sudo] password for bob:"));
        assert!(!looks_like_secret_prompt("password reset done"));
    }
}
