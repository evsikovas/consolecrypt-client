//! Best-effort shell command tokenizer for POSIX shells, PowerShell and
//! cmd.exe.
//!
//! Used by the risk rules, the sanitizer's command-aware rules and the
//! command → snippet parameterizer. It is deliberately forgiving: malformed
//! input (unbalanced quotes etc.) never fails, it just produces the most
//! plausible words. It is **not** a security boundary on its own — the risk
//! engine treats anything it cannot understand as `Unknown`.

use serde::{Deserialize, Serialize};
use std::ops::Range;

/// Quoting / tokenization rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellDialect {
    /// sh / bash / zsh.
    #[default]
    Posix,
    PowerShell,
    Cmd,
}

impl ShellDialect {
    /// Map a free-form shell name ("bash", "pwsh", "cmd.exe", …).
    pub fn from_shell_name(name: &str) -> Option<Self> {
        let n = name.trim().to_ascii_lowercase();
        let n = n.rsplit(['/', '\\']).next().unwrap_or(&n);
        let n = n.strip_suffix(".exe").unwrap_or(n);
        match n {
            "sh" | "bash" | "zsh" | "dash" | "ash" | "ksh" | "fish" | "busybox" | "posix" => {
                Some(Self::Posix)
            }
            "pwsh" | "powershell" | "pwsh7" | "ps" | "ps1" => Some(Self::PowerShell),
            "cmd" => Some(Self::Cmd),
            _ => None,
        }
    }
}

/// A word after quote removal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    /// Value after quote removal and escape processing.
    pub value: String,
    /// Byte range of the raw word in the input.
    pub span: Range<usize>,
    /// Any part of the word was quoted.
    pub quoted: bool,
}

/// A redirection such as `> file`, `2>&1`, `>> log`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirect {
    /// Operator including a leading fd, e.g. `>`, `2>`, `>>`, `&>`, `<`.
    pub op: String,
    /// Target word (`None` for fd duplications like `2>&1`).
    pub target: Option<Word>,
}

impl Redirect {
    /// Output redirection that truncates/appends a file (not `&1`, not
    /// `/dev/null`).
    pub fn writes_file(&self) -> bool {
        let out = self.op.contains('>');
        match &self.target {
            Some(t) => out && t.value != "/dev/null" && !t.value.eq_ignore_ascii_case("nul"),
            None => false,
        }
    }
}

/// Operator that ended a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Separator {
    Pipe,
    And,
    Or,
    Semicolon,
    Background,
    Newline,
    End,
}

/// One simple command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub words: Vec<Word>,
    pub redirects: Vec<Redirect>,
    /// Bodies of `$(…)`, `` `…` `` (POSIX) and `$(…)` / `(…)` (PowerShell)
    /// found inside words; analysed recursively by callers.
    pub substitutions: Vec<String>,
    /// Byte range covered by the command.
    pub span: Range<usize>,
    /// What followed the command.
    pub separator: Separator,
}

impl Command {
    /// Word values.
    pub fn args(&self) -> Vec<&str> {
        self.words.iter().map(|w| w.value.as_str()).collect()
    }
}

struct Lexer<'a> {
    src: &'a str,
    chars: Vec<(usize, char)>,
    i: usize,
    dialect: ShellDialect,
    commands: Vec<Command>,
    cur: Command,
    word: Option<(String, usize, bool)>,
    pending_redirect: Option<String>,
}

/// Split `input` into simple commands.
pub fn parse(input: &str, dialect: ShellDialect) -> Vec<Command> {
    let mut lx = Lexer {
        src: input,
        chars: input.char_indices().collect(),
        i: 0,
        dialect,
        commands: Vec::new(),
        cur: empty_command(0),
        word: None,
        pending_redirect: None,
    };
    lx.run();
    lx.commands
}

fn empty_command(at: usize) -> Command {
    Command {
        words: Vec::new(),
        redirects: Vec::new(),
        substitutions: Vec::new(),
        span: at..at,
        separator: Separator::End,
    }
}

impl Lexer<'_> {
    fn peek(&self, off: usize) -> Option<char> {
        self.chars.get(self.i + off).map(|c| c.1)
    }

    fn pos(&self) -> usize {
        self.chars.get(self.i).map_or(self.src.len(), |c| c.0)
    }

    fn push_char(&mut self, c: char, quoted: bool) {
        let pos = self.pos();
        match &mut self.word {
            Some((s, _, q)) => {
                s.push(c);
                *q |= quoted;
            }
            None => self.word = Some((c.to_string(), pos, quoted)),
        }
    }

    fn start_word_if_needed(&mut self, quoted: bool) {
        if self.word.is_none() {
            self.word = Some((String::new(), self.pos(), quoted));
        } else if let Some((_, _, q)) = &mut self.word {
            *q |= quoted;
        }
    }

    fn finish_word(&mut self) {
        if let Some((value, start, quoted)) = self.word.take() {
            let end = self.pos();
            let w = Word {
                value,
                span: start..end,
                quoted,
            };
            if self.cur.words.is_empty() && self.cur.redirects.is_empty() {
                self.cur.span.start = start;
            }
            if let Some(op) = self.pending_redirect.take() {
                self.cur.redirects.push(Redirect {
                    op,
                    target: Some(w),
                });
            } else {
                self.cur.words.push(w);
            }
            self.cur.span.end = end;
        }
    }

    fn finish_command(&mut self, sep: Separator) {
        self.finish_word();
        if let Some(op) = self.pending_redirect.take() {
            self.cur.redirects.push(Redirect { op, target: None });
        }
        let at = self.pos();
        let mut cmd = std::mem::replace(&mut self.cur, empty_command(at));
        if !cmd.words.is_empty() || !cmd.redirects.is_empty() {
            cmd.separator = sep;
            self.commands.push(cmd);
        } else if let Some(last) = self.commands.last_mut() {
            if last.separator == Separator::End {
                last.separator = sep;
            }
        }
    }

    /// Read a balanced `(...)` starting at the current `(`; returns the body.
    fn read_balanced(&mut self, open: char, close: char) -> String {
        let mut depth = 0usize;
        let mut body = String::new();
        let mut quote: Option<char> = None;
        while let Some(c) = self.peek(0) {
            self.i += 1;
            if let Some(q) = quote {
                if c == q {
                    quote = None;
                }
                body.push(c);
                continue;
            }
            if c == open {
                depth += 1;
                if depth == 1 {
                    continue;
                }
            } else if c == close {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return body;
                }
            } else if c == '\'' || c == '"' {
                quote = Some(c);
            }
            body.push(c);
        }
        body
    }

    fn run(&mut self) {
        while let Some(c) = self.peek(0) {
            match self.dialect {
                ShellDialect::Posix => self.step_posix(c),
                ShellDialect::PowerShell => self.step_powershell(c),
                ShellDialect::Cmd => self.step_cmd(c),
            }
        }
        self.finish_command(Separator::End);
    }

    fn operator(&mut self, c: char) -> bool {
        if matches!(c, '\n' | ';' | '|' | '&') {
            // End the current word before consuming the operator so its span
            // does not include the operator.
            self.finish_word();
        }
        match c {
            '\n' => {
                self.i += 1;
                self.finish_command(Separator::Newline);
            }
            ';' => {
                self.i += 1;
                self.finish_command(Separator::Semicolon);
            }
            '|' => {
                if self.peek(1) == Some('|') {
                    self.i += 2;
                    self.finish_command(Separator::Or);
                } else {
                    self.i += 1;
                    if self.peek(0) == Some('&') && self.dialect == ShellDialect::Posix {
                        self.i += 1;
                    }
                    self.finish_command(Separator::Pipe);
                }
            }
            '&' => {
                if self.peek(1) == Some('&') {
                    self.i += 2;
                    self.finish_command(Separator::And);
                } else if self.peek(1) == Some('>') && self.dialect == ShellDialect::Posix {
                    self.finish_word();
                    self.i += 2;
                    let mut op = String::from("&>");
                    if self.peek(0) == Some('>') {
                        self.i += 1;
                        op.push('>');
                    }
                    self.pending_redirect = Some(op);
                } else {
                    self.i += 1;
                    self.finish_command(Separator::Background);
                }
            }
            '>' | '<' => self.redirect(c),
            _ => return false,
        }
        true
    }

    fn redirect(&mut self, c: char) {
        // A preceding all-digit word is an fd number (`2>`).
        let mut op = String::new();
        if let Some((w, _, false)) = &self.word {
            if !w.is_empty() && w.chars().all(|d| d.is_ascii_digit()) || w == "*" {
                op.push_str(w);
                self.word = None;
            }
        }
        self.finish_word();
        op.push(c);
        self.i += 1;
        while let Some(n) = self.peek(0) {
            if n == '>' || n == '<' || n == '|' && op.ends_with('>') {
                op.push(n);
                self.i += 1;
            } else {
                break;
            }
        }
        if self.peek(0) == Some('&') {
            // fd duplication: 2>&1, >&-, <&3
            op.push('&');
            self.i += 1;
            while let Some(d) = self.peek(0) {
                if d.is_ascii_digit() || d == '-' {
                    op.push(d);
                    self.i += 1;
                } else {
                    break;
                }
            }
            self.cur.redirects.push(Redirect { op, target: None });
            return;
        }
        self.pending_redirect = Some(op);
    }

    fn step_posix(&mut self, c: char) {
        if self.word.is_none() && (c == ' ' || c == '\t' || c == '\r') {
            self.i += 1;
            return;
        }
        if self.operator(c) {
            return;
        }
        match c {
            ' ' | '\t' | '\r' => {
                self.finish_word();
                self.i += 1;
            }
            '#' if self.word.is_none() => {
                while let Some(n) = self.peek(0) {
                    if n == '\n' {
                        break;
                    }
                    self.i += 1;
                }
            }
            '(' | ')' if self.word.is_none() => {
                self.i += 1;
                self.finish_command(Separator::Semicolon);
            }
            '\\' => {
                self.i += 1;
                match self.peek(0) {
                    Some('\n') => self.i += 1,
                    Some(n) => {
                        self.push_char(n, true);
                        self.i += 1;
                    }
                    None => self.push_char('\\', false),
                }
            }
            '\'' => {
                self.start_word_if_needed(true);
                self.i += 1;
                while let Some(n) = self.peek(0) {
                    self.i += 1;
                    if n == '\'' {
                        break;
                    }
                    self.push_char(n, true);
                }
            }
            '"' => {
                self.start_word_if_needed(true);
                self.i += 1;
                while let Some(n) = self.peek(0) {
                    if n == '"' {
                        self.i += 1;
                        break;
                    }
                    if n == '\\' && matches!(self.peek(1), Some('"' | '\\' | '$' | '`' | '\n')) {
                        let e = self.peek(1).unwrap_or('\\');
                        self.i += 2;
                        if e != '\n' {
                            self.push_char(e, true);
                        }
                        continue;
                    }
                    if n == '$' && self.peek(1) == Some('(') {
                        self.substitution_dollar();
                        continue;
                    }
                    if n == '`' {
                        self.substitution_backtick();
                        continue;
                    }
                    self.push_char(n, true);
                    self.i += 1;
                }
            }
            '$' if self.peek(1) == Some('(') => self.substitution_dollar(),
            '`' => self.substitution_backtick(),
            _ => {
                self.push_char(c, false);
                self.i += 1;
            }
        }
    }

    fn substitution_dollar(&mut self) {
        self.start_word_if_needed(false);
        self.i += 1; // '$'
        let body = self.read_balanced('(', ')');
        if let Some((s, _, _)) = &mut self.word {
            s.push_str("$(");
            s.push_str(&body);
            s.push(')');
        }
        let body = body
            .trim_start_matches('(')
            .trim_end_matches(')')
            .to_owned();
        self.cur.substitutions.push(body);
    }

    fn substitution_backtick(&mut self) {
        self.start_word_if_needed(false);
        self.i += 1;
        let mut body = String::new();
        while let Some(n) = self.peek(0) {
            self.i += 1;
            if n == '`' {
                break;
            }
            body.push(n);
        }
        if let Some((s, _, _)) = &mut self.word {
            s.push('`');
            s.push_str(&body);
            s.push('`');
        }
        self.cur.substitutions.push(body);
    }

    fn step_powershell(&mut self, c: char) {
        if self.word.is_none() && (c == ' ' || c == '\t' || c == '\r') {
            self.i += 1;
            return;
        }
        if self.operator(c) {
            return;
        }
        match c {
            ' ' | '\t' | '\r' => {
                self.finish_word();
                self.i += 1;
            }
            '#' if self.word.is_none() => {
                while let Some(n) = self.peek(0) {
                    if n == '\n' {
                        break;
                    }
                    self.i += 1;
                }
            }
            '`' => {
                self.i += 1;
                match self.peek(0) {
                    Some('\n') => self.i += 1,
                    Some(n) => {
                        self.push_char(n, true);
                        self.i += 1;
                    }
                    None => {}
                }
            }
            '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => {
                self.start_word_if_needed(true);
                self.i += 1;
                while let Some(n) = self.peek(0) {
                    self.i += 1;
                    if is_ps_single_quote(n) {
                        if self.peek(0).is_some_and(is_ps_single_quote) {
                            self.push_char(n, true);
                            self.i += 1;
                            continue;
                        }
                        break;
                    }
                    self.push_char(n, true);
                }
            }
            '"' | '\u{201C}' | '\u{201D}' | '\u{201E}' => {
                self.start_word_if_needed(true);
                self.i += 1;
                while let Some(n) = self.peek(0) {
                    if is_ps_double_quote(n) {
                        self.i += 1;
                        if self.peek(0).is_some_and(is_ps_double_quote) {
                            self.push_char(n, true);
                            self.i += 1;
                            continue;
                        }
                        break;
                    }
                    if n == '`' {
                        self.i += 1;
                        if let Some(e) = self.peek(0) {
                            self.push_char(e, true);
                            self.i += 1;
                        }
                        continue;
                    }
                    if n == '$' && self.peek(1) == Some('(') {
                        self.substitution_dollar();
                        continue;
                    }
                    self.push_char(n, true);
                    self.i += 1;
                }
            }
            '$' if self.peek(1) == Some('(') => self.substitution_dollar(),
            '(' => {
                self.start_word_if_needed(false);
                let body = self.read_balanced('(', ')');
                if let Some((s, _, _)) = &mut self.word {
                    s.push('(');
                    s.push_str(&body);
                    s.push(')');
                }
                self.cur.substitutions.push(body);
            }
            '{' if self.word.is_none() => {
                // Script block: analyse its body as a nested command.
                self.start_word_if_needed(false);
                let body = self.read_balanced('{', '}');
                if let Some((s, _, _)) = &mut self.word {
                    s.push('{');
                    s.push_str(&body);
                    s.push('}');
                }
                self.cur.substitutions.push(body);
            }
            _ => {
                self.push_char(c, false);
                self.i += 1;
            }
        }
    }

    fn step_cmd(&mut self, c: char) {
        if self.word.is_none() && (c == ' ' || c == '\t' || c == '\r') {
            self.i += 1;
            return;
        }
        if c == '&' && self.peek(1) != Some('&') {
            self.i += 1;
            self.finish_command(Separator::Semicolon);
            return;
        }
        if c != ';' && self.operator(c) {
            return;
        }
        match c {
            ' ' | '\t' | '\r' => {
                self.finish_word();
                self.i += 1;
            }
            '^' => {
                self.i += 1;
                match self.peek(0) {
                    Some('\n') => self.i += 1,
                    Some(n) => {
                        self.push_char(n, true);
                        self.i += 1;
                    }
                    None => {}
                }
            }
            '"' => {
                self.start_word_if_needed(true);
                self.i += 1;
                while let Some(n) = self.peek(0) {
                    self.i += 1;
                    if n == '"' {
                        break;
                    }
                    self.push_char(n, true);
                }
            }
            _ => {
                self.push_char(c, false);
                self.i += 1;
            }
        }
    }
}

pub(crate) fn is_ps_single_quote(c: char) -> bool {
    matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}')
}

pub(crate) fn is_ps_double_quote(c: char) -> bool {
    matches!(c, '"' | '\u{201C}' | '\u{201D}' | '\u{201E}')
}

/// Leading words that only wrap another command (`sudo`, `env`, `nohup`, …).
/// Returns the index of the first word of the wrapped command, skipping
/// wrapper options and `VAR=value` assignments.
pub(crate) fn skip_wrappers(words: &[&str]) -> usize {
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        if is_assignment(w) {
            i += 1;
            continue;
        }
        match w {
            "sudo" | "doas" => {
                i += 1;
                while i < words.len() && words[i].starts_with('-') {
                    let takes_value = matches!(
                        words[i],
                        "-u" | "-g" | "-U" | "-C" | "-D" | "-h" | "-p" | "-r" | "-t" | "-T"
                    );
                    i += if takes_value { 2 } else { 1 };
                }
            }
            "env" => {
                i += 1;
                while i < words.len() && (words[i].starts_with('-') || is_assignment(words[i])) {
                    let takes_value = matches!(words[i], "-u" | "-C" | "-S");
                    i += if takes_value { 2 } else { 1 };
                }
            }
            "nohup" | "exec" | "command" | "builtin" | "time" | "nice" | "ionice" | "stdbuf"
            | "chrt" | "taskset" | "unbuffer" | "caffeinate" | "!" | "then" | "do" | "else"
            | "{" => {
                i += 1;
                while i < words.len() && words[i].starts_with('-') {
                    // nice -n 10, ionice -c 3, taskset -c 0
                    let takes_value = matches!(words[i], "-n" | "-c" | "-p" | "-o" | "-e" | "-i");
                    i += if takes_value { 2 } else { 1 };
                }
            }
            "timeout" => {
                i += 1;
                while i < words.len() && words[i].starts_with('-') {
                    let takes_value = matches!(words[i], "-s" | "-k" | "--signal" | "--kill-after");
                    i += if takes_value { 2 } else { 1 };
                }
                i += 1; // duration
            }
            _ => break,
        }
    }
    i
}

/// `NAME=value` shell assignment.
pub(crate) fn is_assignment(w: &str) -> bool {
    match w.find('=') {
        Some(eq) if eq > 0 => {
            let name = &w[..eq];
            name.chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        _ => false,
    }
}

/// Base name of a command word (`/usr/bin/rm` → `rm`, `C:\x\git.exe` → `git`).
pub(crate) fn command_name(w: &str) -> String {
    let base = w.rsplit(['/', '\\']).next().unwrap_or(w);
    let lower = base.to_ascii_lowercase();
    lower
        .strip_suffix(".exe")
        .map(str::to_owned)
        .unwrap_or(lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(c: &Command) -> Vec<&str> {
        c.words.iter().map(|w| w.value.as_str()).collect()
    }

    #[test]
    fn posix_quotes_and_separators() {
        let cmds = parse(
            r#"echo "a b" 'c d' e\ f && ls -la | grep x; rm -rf /tmp/y & true"#,
            ShellDialect::Posix,
        );
        assert_eq!(cmds.len(), 5);
        assert_eq!(words(&cmds[0]), vec!["echo", "a b", "c d", "e f"]);
        assert_eq!(cmds[0].separator, Separator::And);
        assert_eq!(words(&cmds[1]), vec!["ls", "-la"]);
        assert_eq!(cmds[1].separator, Separator::Pipe);
        assert_eq!(words(&cmds[3]), vec!["rm", "-rf", "/tmp/y"]);
        assert_eq!(cmds[3].separator, Separator::Background);
    }

    #[test]
    fn posix_spans_point_at_raw_words() {
        let src = r#"kubectl logs -n "prod ns" pod-1"#;
        let cmds = parse(src, ShellDialect::Posix);
        let w = &cmds[0].words[3];
        assert_eq!(w.value, "prod ns");
        assert_eq!(&src[w.span.clone()], "\"prod ns\"");
        assert!(w.quoted);
        assert_eq!(&src[cmds[0].words[4].span.clone()], "pod-1");
    }

    #[test]
    fn posix_redirects_and_substitutions() {
        let cmds = parse("echo $(rm -rf /) `id` > out.txt 2>&1", ShellDialect::Posix);
        assert_eq!(cmds.len(), 1);
        let c = &cmds[0];
        assert_eq!(c.substitutions, vec!["rm -rf /", "id"]);
        assert_eq!(c.redirects.len(), 2);
        assert_eq!(c.redirects[0].op, ">");
        assert_eq!(c.redirects[0].target.as_ref().unwrap().value, "out.txt");
        assert!(c.redirects[0].writes_file());
        assert_eq!(c.redirects[1].op, "2>&1");
        assert!(!c.redirects[1].writes_file());
        let c = &parse("cat x >/dev/null", ShellDialect::Posix)[0];
        assert!(!c.redirects[0].writes_file());
    }

    #[test]
    fn posix_comments_and_continuations() {
        let cmds = parse("ls \\\n -la # rm -rf /\npwd", ShellDialect::Posix);
        assert_eq!(cmds.len(), 2);
        assert_eq!(words(&cmds[0]), vec!["ls", "-la"]);
        assert_eq!(words(&cmds[1]), vec!["pwd"]);
    }

    #[test]
    fn powershell_quotes() {
        let cmds = parse(
            "Remove-Item 'C:\\it''s' -Recurse; Get-ChildItem \"a`\"b\" | Select-Object Name",
            ShellDialect::PowerShell,
        );
        assert_eq!(cmds.len(), 3);
        assert_eq!(words(&cmds[0]), vec!["Remove-Item", "C:\\it's", "-Recurse"]);
        assert_eq!(words(&cmds[1]), vec!["Get-ChildItem", "a\"b"]);
    }

    #[test]
    fn cmd_quotes_and_ampersand() {
        let cmds = parse("del /q \"C:\\a b\\*\" & dir ^& echo", ShellDialect::Cmd);
        assert_eq!(cmds.len(), 2);
        assert_eq!(words(&cmds[0]), vec!["del", "/q", "C:\\a b\\*"]);
        assert_eq!(words(&cmds[1]), vec!["dir", "&", "echo"]);
    }

    #[test]
    fn wrappers_are_skipped() {
        let w = [
            "sudo", "-u", "postgres", "env", "A=1", "nice", "-n", "5", "psql",
        ];
        assert_eq!(skip_wrappers(&w), 8);
        assert_eq!(skip_wrappers(&["timeout", "5s", "curl"]), 2);
        assert_eq!(skip_wrappers(&["FOO=bar", "rm"]), 1);
        assert_eq!(command_name("/usr/bin/RM"), "rm");
        assert_eq!(command_name("C:\\Git\\git.exe"), "git");
    }

    #[test]
    fn dialect_names() {
        assert_eq!(
            ShellDialect::from_shell_name("/bin/zsh"),
            Some(ShellDialect::Posix)
        );
        assert_eq!(
            ShellDialect::from_shell_name("pwsh.exe"),
            Some(ShellDialect::PowerShell)
        );
        assert_eq!(
            ShellDialect::from_shell_name("CMD.EXE"),
            Some(ShellDialect::Cmd)
        );
        assert_eq!(ShellDialect::from_shell_name("psql"), None);
    }
}
