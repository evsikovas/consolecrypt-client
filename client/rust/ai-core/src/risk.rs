//! Local risk rules (CLIENT_SPEC §11.3, §16).
//!
//! Commands are classified locally into `read_only`, `modifying`,
//! `destructive` or `unknown`. An AI risk suggestion is only a hint: it can
//! **raise** the level ([`combine_with_ai`]) but never lower it. Anything the
//! rules do not recognise is `unknown` and therefore requires confirmation.
//!
//! Severity order used for combining: `read_only < modifying < unknown <
//! destructive`.

use crate::shell::{self, command_name, skip_wrappers, Command, Separator, ShellDialect};
pub use cc_models::snippet::RiskLevel;
use cc_models::snippet::SnippetType;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Language of the command being classified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandDialect {
    /// sh/bash/zsh command line (also kubectl/helm/docker/terraform/… CLIs).
    #[default]
    Posix,
    PowerShell,
    Cmd,
    /// SQL / PostgreSQL statements.
    Sql,
    /// Cassandra CQL statements.
    Cql,
    /// Commands typed into `redis-cli`.
    Redis,
    /// OpenSearch/Elasticsearch Dev Tools console requests.
    OpenSearch,
}

impl CommandDialect {
    /// Dialect for a snippet type; `shell` (free-form, e.g. "pwsh") refines
    /// shell-based types.
    pub fn for_snippet(t: SnippetType, shell: Option<&str>) -> Self {
        let from_shell = shell
            .and_then(ShellDialect::from_shell_name)
            .map(Self::from);
        match t {
            SnippetType::Powershell => Self::PowerShell,
            SnippetType::Cmd => Self::Cmd,
            SnippetType::Sql | SnippetType::Postgresql => Self::Sql,
            SnippetType::Cql => Self::Cql,
            SnippetType::OpensearchDsl => Self::OpenSearch,
            SnippetType::RedisCli => Self::Posix,
            _ => from_shell.unwrap_or(Self::Posix),
        }
    }
}

impl From<ShellDialect> for CommandDialect {
    fn from(d: ShellDialect) -> Self {
        match d {
            ShellDialect::Posix => Self::Posix,
            ShellDialect::PowerShell => Self::PowerShell,
            ShellDialect::Cmd => Self::Cmd,
        }
    }
}

/// Why a level was assigned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskReason {
    pub level: RiskLevel,
    /// Stable rule id (e.g. `rm`, `kubectl-delete`, `sql-delete-no-where`).
    pub rule: String,
    /// Human-readable explanation (English; UI may localize by `rule`).
    pub detail: String,
}

/// Result of [`classify`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskAssessment {
    pub level: RiskLevel,
    pub reasons: Vec<RiskReason>,
}

impl RiskAssessment {
    /// Whether running requires an explicit confirmation.
    pub fn requires_confirmation(&self) -> bool {
        self.level.requires_confirmation()
    }
}

/// Severity rank: read_only 0 < modifying 1 < unknown 2 < destructive 3.
pub const fn severity(l: RiskLevel) -> u8 {
    match l {
        RiskLevel::ReadOnly => 0,
        RiskLevel::Modifying => 1,
        RiskLevel::Unknown => 2,
        RiskLevel::Destructive => 3,
    }
}

/// The more severe of two levels.
pub fn max_risk(a: RiskLevel, b: RiskLevel) -> RiskLevel {
    if severity(b) > severity(a) {
        b
    } else {
        a
    }
}

/// Combine the local level with an AI suggestion: the AI can only raise it.
pub fn combine_with_ai(local: RiskLevel, ai: Option<RiskLevel>) -> RiskLevel {
    match ai {
        Some(a) => max_risk(local, a),
        None => local,
    }
}

/// Classify a command (possibly multi-line / a pipeline / a script).
pub fn classify(command: &str, dialect: CommandDialect) -> RiskAssessment {
    let mut acc = Acc::default();
    if command.trim().is_empty() {
        acc.add(RiskLevel::Unknown, "empty", "empty command");
    } else {
        match dialect {
            CommandDialect::Posix => classify_shell(command, ShellDialect::Posix, 0, &mut acc),
            CommandDialect::PowerShell => {
                classify_shell(command, ShellDialect::PowerShell, 0, &mut acc)
            }
            CommandDialect::Cmd => classify_shell(command, ShellDialect::Cmd, 0, &mut acc),
            CommandDialect::Sql | CommandDialect::Cql => classify_sql(command, &mut acc),
            CommandDialect::Redis => {
                for line in command.lines().filter(|l| !l.trim().is_empty()) {
                    let words = shell::parse(line, ShellDialect::Posix);
                    for c in words {
                        classify_redis(&c.args(), &mut acc);
                    }
                }
            }
            CommandDialect::OpenSearch => classify_opensearch_console(command, &mut acc),
        }
    }
    acc.finish()
}

#[derive(Default)]
struct Acc {
    reasons: Vec<RiskReason>,
}

impl Acc {
    fn add(&mut self, level: RiskLevel, rule: &str, detail: impl Into<String>) {
        let detail = detail.into();
        if !self
            .reasons
            .iter()
            .any(|r| r.rule == rule && r.level == level && r.detail == detail)
        {
            self.reasons.push(RiskReason {
                level,
                rule: rule.to_owned(),
                detail,
            });
        }
    }
    fn ro(&mut self, rule: &str, detail: impl Into<String>) {
        self.add(RiskLevel::ReadOnly, rule, detail);
    }
    fn md(&mut self, rule: &str, detail: impl Into<String>) {
        self.add(RiskLevel::Modifying, rule, detail);
    }
    fn ds(&mut self, rule: &str, detail: impl Into<String>) {
        self.add(RiskLevel::Destructive, rule, detail);
    }
    fn unk(&mut self, rule: &str, detail: impl Into<String>) {
        self.add(RiskLevel::Unknown, rule, detail);
    }
    fn finish(mut self) -> RiskAssessment {
        let level = self
            .reasons
            .iter()
            .map(|r| r.level)
            .fold(RiskLevel::ReadOnly, max_risk);
        let level = if self.reasons.is_empty() {
            RiskLevel::Unknown
        } else {
            level
        };
        // Most severe reasons first.
        self.reasons
            .sort_by_key(|r| std::cmp::Reverse(severity(r.level)));
        RiskAssessment {
            level,
            reasons: self.reasons,
        }
    }
}

// ---------------------------------------------------------------------------
// Argument helpers
// ---------------------------------------------------------------------------

/// `-r`, `-rf`, `-Rf` (combined short flags) or `--recursive`.
fn has_flag(args: &[&str], short: &[char], long: &[&str]) -> bool {
    args.iter().any(|a| {
        if let Some(l) = a.strip_prefix("--") {
            let name = l.split('=').next().unwrap_or(l);
            long.contains(&name)
        } else if let Some(s) = a.strip_prefix('-') {
            !s.is_empty()
                && s.chars().all(|c| c.is_ascii_alphanumeric())
                && s.chars().any(|c| short.contains(&c))
        } else {
            false
        }
    })
}

fn has_word(args: &[&str], words: &[&str]) -> bool {
    args.iter().any(|a| words.contains(a))
}

fn has_prefix(args: &[&str], prefixes: &[&str]) -> bool {
    args.iter()
        .any(|a| prefixes.iter().any(|p| a.starts_with(p)))
}

/// Non-flag arguments, skipping values of the given flags (`-n ns`).
fn positionals<'a>(args: &[&'a str], flags_with_values: &[&str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut skip = false;
    let mut after_ddash = false;
    for a in args {
        if skip {
            skip = false;
            continue;
        }
        if after_ddash {
            out.push(*a);
            continue;
        }
        if *a == "--" {
            after_ddash = true;
            continue;
        }
        if a.starts_with('-') && a.len() > 1 {
            if !a.contains('=') && flags_with_values.contains(a) {
                skip = true;
            }
            continue;
        }
        out.push(*a);
    }
    out
}

/// Value of `--flag value` or `--flag=value` (also `-f value`).
fn flag_value<'a>(args: &[&'a str], names: &[&str]) -> Option<&'a str> {
    let mut it = args.iter().peekable();
    while let Some(a) = it.next() {
        for n in names {
            if a == n {
                return it.peek().copied().copied();
            }
            if let Some(v) = a.strip_prefix(n).and_then(|r| r.strip_prefix('=')) {
                return Some(v);
            }
        }
    }
    None
}

/// All values of a repeatable flag.
fn flag_values<'a>(args: &[&'a str], names: &[&str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i];
        for n in names {
            if a == *n {
                if let Some(v) = args.get(i + 1) {
                    out.push(*v);
                }
            } else if let Some(v) = a.strip_prefix(n).and_then(|r| r.strip_prefix('=')) {
                out.push(v);
            }
        }
        i += 1;
    }
    out
}

const CRITICAL_PATHS: &[&str] = &[
    "/", "/*", "/etc", "/usr", "/var", "/bin", "/sbin", "/lib", "/lib64", "/boot", "/home",
    "/root", "/opt", "/srv", "/dev", "/proc", "/sys", "~", "~/", "$HOME", "*", ".", "..", "C:\\",
];

fn is_critical_path(p: &str) -> bool {
    let t = p.trim_end_matches('/');
    CRITICAL_PATHS.contains(&p) || CRITICAL_PATHS.contains(&t) || t.is_empty() && p.starts_with('/')
}

fn is_block_device(p: &str) -> bool {
    static BLOCK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^/dev/(sd[a-z]|hd[a-z]|vd[a-z]|xvd[a-z]|nvme\d|mmcblk\d|disk\d|md\d|dm-\d|mapper/|loop\d)")
            .expect("valid regex")
    });
    BLOCK.is_match(p)
}

// ---------------------------------------------------------------------------
// Shells
// ---------------------------------------------------------------------------

static FORK_BOMB: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"([A-Za-z_:][A-Za-z0-9_:]*)\s*\(\)\s*\{[^}]*\|\s*[A-Za-z_:][A-Za-z0-9_:]*\s*&")
        .expect("valid regex")
});

const SHELLS: &[&str] = &["sh", "bash", "zsh", "dash", "ksh", "fish", "ash", "busybox"];
const INTERPRETERS: &[&str] = &[
    "python", "python3", "python2", "perl", "ruby", "node", "php", "lua", "Rscript", "deno", "bun",
];

fn classify_shell(text: &str, dialect: ShellDialect, depth: u8, acc: &mut Acc) {
    if depth > 6 {
        acc.unk("nesting", "deeply nested command");
        return;
    }
    if FORK_BOMB.is_match(text) {
        acc.ds("fork-bomb", "fork bomb");
    }
    let cmds = shell::parse(text, dialect);
    if cmds.is_empty() {
        acc.ro("noop", "no command");
        return;
    }
    for (i, c) in cmds.iter().enumerate() {
        let upstream = if i > 0 && cmds[i - 1].separator == Separator::Pipe {
            cmds[i - 1].words.first().map(|w| command_name(&w.value))
        } else {
            None
        };
        classify_simple(c, dialect, depth, upstream.as_deref(), acc);
    }
}

fn classify_simple(
    c: &Command,
    dialect: ShellDialect,
    depth: u8,
    upstream: Option<&str>,
    acc: &mut Acc,
) {
    for sub in &c.substitutions {
        classify_shell(sub, dialect, depth + 1, acc);
    }
    for r in &c.redirects {
        if let Some(t) = &r.target {
            if r.op.contains('>') {
                if is_block_device(&t.value) {
                    acc.ds(
                        "redirect-block-device",
                        format!("writes to block device {}", t.value),
                    );
                } else if r.writes_file() {
                    acc.md("redirect-write", "writes to a file via redirection");
                }
            } else if r.op.starts_with('<') && !r.op.starts_with("<<") {
                // Input from a file (e.g. `mysql < dump.sql`): content unknown.
                let name = c
                    .words
                    .first()
                    .map(|w| command_name(&w.value))
                    .unwrap_or_default();
                if matches!(
                    name.as_str(),
                    "mysql"
                        | "psql"
                        | "sqlite3"
                        | "mariadb"
                        | "cqlsh"
                        | "redis-cli"
                        | "sh"
                        | "bash"
                        | "zsh"
                        | "mongosh"
                        | "clickhouse-client"
                        | "sqlcmd"
                ) {
                    acc.unk("stdin-script", "runs statements read from a file");
                }
            }
        }
    }
    let words = c.args();
    let idx = skip_wrappers(&words);
    if idx >= words.len() {
        if words.iter().any(|w| matches!(*w, "sudo" | "doas")) {
            acc.unk("sudo-shell", "interactive root shell");
        } else if !words.is_empty() || !c.redirects.is_empty() {
            acc.ro("assignment", "shell variable assignment / redirection only");
        }
        return;
    }
    let name_raw = words[idx];
    let args = &words[idx + 1..];
    match dialect {
        ShellDialect::PowerShell => classify_powershell(name_raw, args, depth, upstream, acc),
        ShellDialect::Cmd => classify_cmd(name_raw, args, depth, upstream, acc),
        ShellDialect::Posix => {
            classify_tool(&command_name(name_raw), args, dialect, depth, upstream, acc)
        }
    }
}

/// Recurse into a command given as separate words (xargs, find -exec, …).
fn classify_words(words: &[&str], dialect: ShellDialect, depth: u8, acc: &mut Acc) {
    if words.is_empty() {
        return;
    }
    let idx = skip_wrappers(words);
    if idx >= words.len() {
        return;
    }
    classify_tool(
        &command_name(words[idx]),
        &words[idx + 1..],
        dialect,
        depth + 1,
        None,
        acc,
    );
}

const READ_ONLY_TOOLS: &[&str] = &[
    "ls",
    "ll",
    "la",
    "dir",
    "vdir",
    "cat",
    "less",
    "more",
    "most",
    "head",
    "tail",
    "grep",
    "egrep",
    "fgrep",
    "zgrep",
    "rg",
    "ag",
    "ack",
    "locate",
    "which",
    "whereis",
    "type",
    "file",
    "stat",
    "wc",
    "cut",
    "tr",
    "jq",
    "yq",
    "echo",
    "printf",
    "pwd",
    "whoami",
    "id",
    "groups",
    "uname",
    "uptime",
    "cal",
    "df",
    "du",
    "free",
    "top",
    "htop",
    "btop",
    "atop",
    "ps",
    "pgrep",
    "pstree",
    "lsof",
    "netstat",
    "ss",
    "ping",
    "ping6",
    "traceroute",
    "traceroute6",
    "tracepath",
    "mtr",
    "dig",
    "nslookup",
    "host",
    "whois",
    "printenv",
    "lsblk",
    "blkid",
    "lscpu",
    "lsmem",
    "lspci",
    "lsusb",
    "lsmod",
    "lshw",
    "dmidecode",
    "findmnt",
    "tree",
    "diff",
    "colordiff",
    "cmp",
    "comm",
    "md5sum",
    "sha1sum",
    "sha224sum",
    "sha256sum",
    "sha384sum",
    "sha512sum",
    "b2sum",
    "cksum",
    "base64",
    "base32",
    "xxd",
    "hexdump",
    "od",
    "strings",
    "man",
    "help",
    "info",
    "tldr",
    "test",
    "[",
    "true",
    "false",
    "sleep",
    "readlink",
    "realpath",
    "basename",
    "dirname",
    "last",
    "lastlog",
    "w",
    "who",
    "users",
    "getent",
    "nproc",
    "vmstat",
    "iostat",
    "mpstat",
    "pidstat",
    "sar",
    "iotop",
    "iftop",
    "nethogs",
    "nvidia-smi",
    "zcat",
    "zless",
    "bzcat",
    "xzcat",
    "zstdcat",
    "column",
    "nl",
    "fold",
    "rev",
    "tac",
    "seq",
    "expr",
    "bc",
    "cd",
    "pushd",
    "popd",
    "clear",
    "exit",
    "logout",
    "arch",
    "lsb_release",
    "getconf",
    "locale",
    "tty",
    "ulimit",
    "numfmt",
    "join",
    "paste",
    "fmt",
    "look",
    "apropos",
    "whatis",
    "systemd-analyze",
    "loginctl",
    "resolvectl",
    "iwconfig",
    "ethtool",
    "arp",
    "k9s",
    "stern",
    "kubetail",
    "lazydocker",
    "ctop",
    "glances",
    "neofetch",
    "fastfetch",
    "screenfetch",
    "ncdu",
    "duf",
    "dust",
    "bat",
    "exa",
    "eza",
    "lsd",
    "fd",
    "fzf",
    "tokei",
    "cloc",
    "hash",
    "jobs",
    "fg",
    "bg",
    "wait",
    "yes",
    "ldd",
    "nm",
    "objdump",
    "readelf",
    "size",
    "pmap",
    "sensors",
    "upower",
    "acpi",
];

#[allow(clippy::too_many_lines)]
fn classify_tool(
    name: &str,
    args: &[&str],
    dialect: ShellDialect,
    depth: u8,
    upstream: Option<&str>,
    acc: &mut Acc,
) {
    // `curl … | sh`: executing downloaded code.
    if (SHELLS.contains(&name) || INTERPRETERS.contains(&name))
        && upstream.is_some()
        && positionals(args, &[]).is_empty()
    {
        if matches!(
            upstream,
            Some("curl" | "wget" | "fetch" | "iwr" | "invoke-webrequest")
        ) {
            acc.ds("pipe-to-shell", "executes code downloaded from the network");
        } else {
            acc.unk("pipe-to-shell", "executes code read from a pipe");
        }
        return;
    }
    match name {
        // ------------------------------------------------ files & disks
        "rm" => {
            let targets = positionals(args, &[]);
            let rec = has_flag(args, &['r', 'R'], &["recursive"]);
            let force = has_flag(args, &['f'], &["force"]);
            if targets.iter().any(|t| is_critical_path(t))
                || has_word(args, &["--no-preserve-root"])
            {
                acc.ds("rm-critical", "recursive delete of a system/home path");
            } else if rec || force {
                acc.ds("rm-recursive", "deletes files (recursive/forced)");
            } else {
                acc.ds("rm", "deletes files");
            }
        }
        "unlink" | "shred" | "wipefs" | "blkdiscard" | "truncate" | "srm" => {
            acc.ds(name, format!("{name} irreversibly removes data"));
        }
        "rmdir" => acc.md("rmdir", "removes empty directories"),
        "dd" => {
            if args.iter().any(|a| a.starts_with("of=")) {
                acc.ds("dd-of", "dd writes raw data to a file or device");
            } else {
                acc.md("dd", "dd copies raw data");
            }
        }
        n if n.starts_with("mkfs") || matches!(n, "mke2fs" | "mkswap" | "mkdosfs" | "newfs") => {
            acc.ds("mkfs", "creates a file system (erases the device)");
        }
        "fdisk" | "sfdisk" | "gdisk" | "sgdisk" | "cfdisk" | "parted" => {
            if has_flag(args, &['l'], &["list"]) || has_word(args, &["print", "-d", "--dump"]) {
                acc.ro("partition-list", "lists partitions");
            } else {
                acc.ds("partition-edit", "edits the partition table");
            }
        }
        "lvremove" | "vgremove" | "pvremove" | "lvreduce" => {
            acc.ds("lvm-remove", "removes/reduces LVM volumes");
        }
        "zpool" | "zfs" => match args.first().copied() {
            Some("destroy" | "rollback" | "labelclear") => {
                acc.ds("zfs-destroy", "destroys ZFS data")
            }
            Some("list" | "status" | "get" | "iostat" | "history") | None => {
                acc.ro("zfs-read", "reads ZFS status");
            }
            _ => acc.md("zfs", "changes ZFS configuration"),
        },
        "mdadm" => {
            if has_word(args, &["--zero-superblock", "--stop", "--remove", "--fail"]) {
                acc.ds("mdadm", "stops/removes RAID members");
            } else if has_word(args, &["--detail", "--examine", "-D", "-E", "--query"]) {
                acc.ro("mdadm-read", "reads RAID status");
            } else {
                acc.md("mdadm", "changes RAID configuration");
            }
        }
        "mv" => {
            if positionals(args, &["-t", "--target-directory"]).last() == Some(&"/dev/null") {
                acc.ds("mv-devnull", "moves files to /dev/null (deletes them)");
            } else {
                acc.md("mv", "moves/renames files");
            }
        }
        "cp" | "ln" | "install" | "touch" | "mkdir" | "mktemp" | "scp" | "sftp" | "patch"
        | "split" | "csplit" | "sponge" => acc.md(name, format!("{name} writes files")),
        "rsync" => {
            if has_flag(args, &['n'], &["dry-run"]) {
                acc.ro("rsync-dry-run", "rsync dry run");
            } else if args
                .iter()
                .any(|a| a.starts_with("--delete") || *a == "--remove-source-files")
            {
                acc.ds(
                    "rsync-delete",
                    "rsync deletes files at the destination/source",
                );
            } else {
                acc.md("rsync", "copies files");
            }
        }
        "chmod" | "chown" | "chgrp" | "setfacl" | "chattr" => {
            let rec = has_flag(args, &['R'], &["recursive"]);
            let targets = positionals(args, &[]);
            if rec && targets.iter().skip(1).any(|t| is_critical_path(t)) {
                acc.ds(
                    "chmod-recursive-root",
                    "recursive permission/owner change on a system path",
                );
            } else {
                acc.md(name, "changes permissions/ownership");
            }
        }
        "sed" => {
            if has_flag(args, &['i'], &["in-place"]) || args.iter().any(|a| a.starts_with("-i")) {
                acc.md("sed-in-place", "edits files in place");
            } else {
                acc.ro("sed", "stream editing to stdout");
            }
        }
        "perl" | "ruby" if has_flag(args, &['i'], &[]) => {
            acc.md("in-place", "edits files in place")
        }
        "awk" | "gawk" | "mawk" | "nawk" => {
            let prog = args
                .iter()
                .find(|a| !a.starts_with('-'))
                .copied()
                .unwrap_or("");
            if prog.contains("system(")
                || prog.contains("print >")
                || prog.contains("printf >")
                || prog.contains("| \"")
                || prog.contains("|\"")
                || has_flag(args, &['i'], &[])
            {
                acc.unk(
                    "awk-side-effects",
                    "awk program may write files or run commands",
                );
            } else {
                acc.ro("awk", "text processing");
            }
        }
        "sort" => {
            if has_flag(args, &['o'], &["output"]) {
                acc.md("sort-output", "writes sorted output to a file");
            } else {
                acc.ro("sort", "sorts text");
            }
        }
        "uniq" => {
            if positionals(args, &["-f", "-s", "-w"]).len() >= 2 {
                acc.md("uniq-output", "writes to an output file");
            } else {
                acc.ro("uniq", "filters text");
            }
        }
        "tee" => {
            if positionals(args, &[]).iter().any(|p| *p != "/dev/null") {
                acc.md("tee", "writes to files");
            } else {
                acc.ro("tee", "copies stdin to stdout");
            }
        }
        "tar" | "bsdtar" => {
            let first = args.first().copied().unwrap_or("");
            if has_flag(args, &['t'], &["list"]) || (!first.starts_with('-') && first.contains('t'))
            {
                acc.ro("tar-list", "lists an archive");
            } else {
                acc.md("tar", "creates/extracts an archive");
            }
        }
        "unzip" => {
            if has_flag(args, &['l', 'v', 't'], &[]) {
                acc.ro("unzip-list", "lists an archive");
            } else {
                acc.md("unzip", "extracts an archive");
            }
        }
        "zip" | "gzip" | "gunzip" | "bzip2" | "bunzip2" | "xz" | "unxz" | "zstd" | "unzstd"
        | "7z" | "7za" | "rar" | "unrar" | "compress" | "uncompress" => {
            if has_flag(args, &['c', 'l', 't'], &["stdout", "list", "test"])
                && !has_flag(args, &['k'], &[])
                && name != "zip"
            {
                acc.ro("compress-stdout", "writes to stdout / lists");
            } else {
                acc.md("compress", "creates/replaces compressed files");
            }
        }
        "find" => classify_find(args, dialect, depth, acc),
        "xargs" => {
            // Skip xargs options (some take values).
            let mut i = 0;
            while i < args.len() && args[i].starts_with('-') {
                let takes = matches!(
                    args[i],
                    "-I" | "-n" | "-P" | "-L" | "-d" | "-s" | "-E" | "-a"
                );
                i += if takes { 2 } else { 1 };
            }
            if i >= args.len() {
                acc.ro("xargs-echo", "xargs with the default echo");
            } else {
                classify_words(&args[i..], dialect, depth, acc);
            }
        }
        "watch" => {
            let mut i = 0;
            while i < args.len() && args[i].starts_with('-') {
                let takes = matches!(args[i], "-n" | "--interval" | "-d" | "-q");
                i += if takes { 2 } else { 1 };
            }
            if args.len() - i == 1 {
                classify_shell(args[i], dialect, depth + 1, acc);
            } else {
                classify_words(&args[i..], dialect, depth, acc);
            }
        }
        "crontab" => {
            if has_flag(args, &['r'], &[]) {
                acc.ds("crontab-remove", "removes the whole crontab");
            } else if has_flag(args, &['l'], &[]) {
                acc.ro("crontab-list", "lists the crontab");
            } else {
                acc.md("crontab", "edits the crontab");
            }
        }
        // ------------------------------------------------ system state
        "shutdown" | "reboot" | "poweroff" | "halt" | "kexec" => {
            if has_flag(args, &['c'], &[]) && name == "shutdown" {
                acc.md("shutdown-cancel", "cancels a scheduled shutdown");
            } else {
                acc.ds("power", "shuts down / reboots the machine");
            }
        }
        "init" | "telinit" => {
            if has_word(args, &["0", "6", "1", "s", "S"]) {
                acc.ds("power", "changes runlevel (halt/reboot/single-user)");
            } else {
                acc.md("init", "changes runlevel");
            }
        }
        "kill" | "pkill" | "killall" | "skill" => {
            if (name == "kill"
                && positionals(args, &["-s", "-n"])
                    .iter()
                    .any(|p| *p == "1" || *p == "-1"))
                || has_word(args, &["-1"]) && name == "kill"
            {
                acc.ds("kill-all", "signals init / all processes");
            } else {
                acc.md("kill", "terminates processes");
            }
        }
        "killall5" => acc.ds("kill-all", "signals all processes"),
        "systemctl" => classify_systemctl(args, acc),
        "service" => {
            let action = positionals(args, &[]).get(1).copied().unwrap_or("");
            match action {
                "stop" => acc.ds("service-stop", "stops a service"),
                "status" | "" => acc.ro("service-status", "service status"),
                _ => acc.md("service", format!("service {action}")),
            }
        }
        "journalctl" => {
            if args.iter().any(|a| {
                a.starts_with("--vacuum")
                    || matches!(*a, "--rotate" | "--flush" | "--relinquish-var")
            }) {
                acc.md("journalctl-maintenance", "rotates/vacuums the journal");
            } else {
                acc.ro("journalctl", "reads logs");
            }
        }
        "dmesg" => {
            if has_flag(args, &['c', 'C'], &["clear", "read-clear"]) {
                acc.md("dmesg-clear", "clears the kernel ring buffer");
            } else {
                acc.ro("dmesg", "reads kernel messages");
            }
        }
        "sysctl" => {
            if has_flag(args, &['w', 'p'], &["write", "load", "system"])
                || args.iter().any(|a| a.contains('='))
            {
                acc.md("sysctl-write", "changes kernel parameters");
            } else {
                acc.ro("sysctl", "reads kernel parameters");
            }
        }
        "hostname" => {
            if positionals(args, &[]).is_empty() {
                acc.ro("hostname", "prints the host name");
            } else {
                acc.md("hostname-set", "changes the host name");
            }
        }
        "hostnamectl" | "timedatectl" | "localectl" => {
            if args
                .first()
                .is_some_and(|a| a.starts_with("set") || *a == "hostname" && args.len() > 1)
            {
                acc.md(name, "changes system settings");
            } else {
                acc.ro(name, "reads system settings");
            }
        }
        "date" => {
            if has_flag(args, &['s'], &["set"]) {
                acc.md("date-set", "changes the system clock");
            } else {
                acc.ro("date", "prints the date");
            }
        }
        "history" => {
            if has_flag(args, &['c', 'd', 'w'], &[]) {
                acc.md("history-edit", "modifies shell history");
            } else {
                acc.ro("history", "prints shell history");
            }
        }
        "export" | "unset" | "alias" | "unalias" | "set" | "setopt" | "shopt" | "umask"
        | "declare" | "typeset" | "readonly" | "local" => {
            if args.is_empty() {
                acc.ro(name, "prints shell state");
            } else {
                acc.md(name, "changes shell state");
            }
        }
        "source" | "." | "eval" => acc.unk("eval", "runs code that is not visible here"),
        "ip" => classify_ip(args, acc),
        "ifconfig" => {
            if positionals(args, &[]).len() > 1 {
                acc.md("ifconfig-set", "changes a network interface");
            } else {
                acc.ro("ifconfig", "shows interfaces");
            }
        }
        "ifdown" | "ifup" | "nmcli" | "netplan" | "dhclient" | "brctl" | "tc" | "wg"
        | "wg-quick" => {
            if name == "nmcli" && has_word(args, &["show", "status", "list"])
                || name == "wg" && args.first().is_some_and(|a| *a == "show")
            {
                acc.ro(name, "shows network state");
            } else {
                acc.md(name, "changes network configuration");
            }
        }
        "route" => {
            if has_word(args, &["add", "del", "delete", "flush", "change"]) {
                acc.md("route-edit", "changes routes");
            } else {
                acc.ro("route", "shows routes");
            }
        }
        "iptables" | "ip6tables" | "iptables-legacy" | "iptables-nft" | "ebtables"
        | "arptables" => {
            if has_flag(args, &['F', 'X', 'Z'], &["flush", "delete-chain", "zero"])
                || (has_flag(args, &['P'], &["policy"]) && has_word(args, &["DROP", "REJECT"]))
            {
                acc.ds(
                    "iptables-flush",
                    "flushes firewall rules / sets a drop policy",
                );
            } else if has_flag(args, &['L', 'S'], &["list", "list-rules"]) {
                acc.ro("iptables-list", "lists firewall rules");
            } else {
                acc.md("iptables", "changes firewall rules");
            }
        }
        "iptables-restore" | "ip6tables-restore" | "nft" => {
            if name == "nft" && has_word(args, &["list"]) {
                acc.ro("nft-list", "lists firewall rules");
            } else if has_word(args, &["flush", "delete"]) {
                acc.ds("nft-flush", "flushes firewall rules");
            } else {
                acc.md(name, "changes firewall rules");
            }
        }
        "ufw" => match args.first().copied() {
            Some("disable" | "reset") => acc.ds("ufw-disable", "disables/resets the firewall"),
            Some("status" | "show" | "app") | None => acc.ro("ufw-status", "firewall status"),
            _ => acc.md("ufw", "changes firewall rules"),
        },
        "firewall-cmd" => {
            if args.iter().any(|a| {
                a.starts_with("--list")
                    || a.starts_with("--get")
                    || *a == "--state"
                    || a.starts_with("--query")
            }) {
                acc.ro("firewalld-read", "reads firewall state");
            } else if has_word(args, &["--panic-on", "--complete-reload"]) {
                acc.ds("firewalld", "drops all traffic / resets state");
            } else {
                acc.md("firewalld", "changes firewall rules");
            }
        }
        "mount" => {
            if positionals(args, &["-t", "-o", "-L", "-U"]).is_empty()
                && !has_flag(args, &['a'], &["all"])
            {
                acc.ro("mount-list", "lists mounts");
            } else {
                acc.md("mount", "mounts a file system");
            }
        }
        "umount" | "swapoff" | "swapon" | "losetup" | "cryptsetup" | "modprobe" | "insmod"
        | "rmmod" | "depmod" | "update-grub" | "grub-install" | "update-initramfs" | "dracut"
        | "mkinitcpio" => {
            acc.md(name, format!("{name} changes system state"));
        }
        "useradd" | "usermod" | "groupadd" | "groupmod" | "passwd" | "chpasswd" | "adduser"
        | "addgroup" | "gpasswd" | "chage" | "chsh" | "chfn" | "visudo" | "newusers" => {
            if name == "passwd" && has_flag(args, &['S'], &["status"]) {
                acc.ro("passwd-status", "shows password status");
            } else {
                acc.md(name, "changes users/groups");
            }
        }
        "userdel" | "groupdel" | "deluser" | "delgroup" => acc.ds(name, "deletes users/groups"),
        // ------------------------------------------------ packages
        "apt" | "apt-get" | "aptitude" | "yum" | "dnf" | "microdnf" | "zypper" | "apk" | "brew"
        | "snap" | "flatpak" | "port" | "choco" | "winget" | "scoop" | "pip" | "pip3" | "pipx"
        | "npm" | "pnpm" | "yarn" | "gem" | "cargo" | "conda" | "mamba" | "uv" | "poetry"
        | "composer" | "go" | "nix-env" | "emerge" | "xbps-install" | "pkg" => {
            classify_package_manager(name, args, acc);
        }
        "pacman" | "yay" | "paru" => {
            let first = args.first().copied().unwrap_or("");
            if first.starts_with("-R") {
                acc.ds("pkg-remove", "removes packages");
            } else if first.starts_with("-Q")
                || first.starts_with("-Ss")
                || first.starts_with("-Si")
                || first.starts_with("-F")
            {
                acc.ro("pkg-query", "queries packages");
            } else {
                acc.md("pkg-install", "installs/updates packages");
            }
        }
        "rpm" => {
            if has_flag(args, &['e'], &["erase"]) {
                acc.ds("pkg-remove", "removes packages");
            } else if has_flag(args, &['q', 'V'], &["query", "verify"]) {
                acc.ro("pkg-query", "queries packages");
            } else {
                acc.md("pkg-install", "installs packages");
            }
        }
        "dpkg" => {
            if has_flag(args, &['r', 'P'], &["remove", "purge"]) {
                acc.ds("pkg-remove", "removes packages");
            } else if has_flag(
                args,
                &['l', 'L', 's', 'S', 'p'],
                &[
                    "list",
                    "listfiles",
                    "status",
                    "search",
                    "print-avail",
                    "get-selections",
                ],
            ) {
                acc.ro("pkg-query", "queries packages");
            } else {
                acc.md("pkg-install", "installs/configures packages");
            }
        }
        // ------------------------------------------------ dev & ops tools
        "git" => classify_git(args, acc),
        "kubectl" | "oc" | "k" | "microk8s.kubectl" | "k3s" => {
            classify_kubectl(args, dialect, depth, acc)
        }
        "helm" => classify_helm(args, acc),
        "terraform" | "tofu" | "terragrunt" => classify_terraform(args, acc),
        "docker" | "podman" | "nerdctl" => classify_docker(args, dialect, depth, acc),
        "docker-compose" | "podman-compose" => classify_compose(args, dialect, depth, acc),
        "ansible" => classify_ansible(args, dialect, depth, acc),
        "ansible-playbook" => {
            if has_flag(
                args,
                &['C'],
                &[
                    "check",
                    "syntax-check",
                    "list-tasks",
                    "list-hosts",
                    "list-tags",
                ],
            ) {
                acc.ro("ansible-check", "ansible check/list mode");
            } else {
                acc.md("ansible-playbook", "runs a playbook (changes hosts)");
            }
        }
        "ansible-inventory" | "ansible-doc" | "ansible-config" | "ansible-lint" => {
            acc.ro(name, "reads ansible metadata");
        }
        "ansible-vault" => match args.first().copied() {
            Some("view") => acc.ro("ansible-vault-view", "views a vault file"),
            _ => acc.md("ansible-vault", "changes a vault file"),
        },
        "ansible-galaxy" => {
            if has_word(args, &["list", "info", "search"]) {
                acc.ro("ansible-galaxy-read", "reads roles/collections");
            } else {
                acc.md("ansible-galaxy", "installs roles/collections");
            }
        }
        "ssh" | "slogin" | "autossh" => {
            let pos = positionals(
                args,
                &[
                    "-p", "-i", "-l", "-o", "-J", "-F", "-L", "-R", "-D", "-W", "-b", "-c", "-E",
                    "-e", "-I", "-m", "-O", "-Q", "-S", "-w", "-B", "-M",
                ],
            );
            if pos.len() > 1 {
                let remote = pos[1..].join(" ");
                classify_shell(&remote, ShellDialect::Posix, depth + 1, acc);
            } else if has_flag(args, &['L', 'R', 'D'], &[]) {
                acc.md("ssh-tunnel", "opens a port forward");
            } else {
                acc.ro("ssh", "opens an interactive SSH session");
            }
        }
        "mosh" | "telnet" => acc.ro(name, "opens an interactive session"),
        "su" => {
            if let Some(cmd) = flag_value(args, &["-c", "--command"]) {
                classify_shell(cmd, ShellDialect::Posix, depth + 1, acc);
            } else {
                acc.unk("su", "switches user (interactive root shell)");
            }
        }
        s if SHELLS.contains(&s) => {
            if let Some(cmd) = flag_value(args, &["-c"]) {
                classify_shell(cmd, ShellDialect::Posix, depth + 1, acc);
            } else if positionals(args, &[]).is_empty() {
                acc.unk("shell", "starts an interactive shell");
            } else {
                acc.unk("script", "runs a script whose content is not visible");
            }
        }
        "pwsh" | "powershell" => {
            if let Some(cmd) = flag_value(args, &["-c", "-Command", "-command"]) {
                classify_shell(cmd, ShellDialect::PowerShell, depth + 1, acc);
            } else {
                acc.unk("powershell", "starts PowerShell / runs a script");
            }
        }
        i if INTERPRETERS.contains(&i) => {
            if has_word(args, &["--version", "-V"]) {
                acc.ro("interpreter-version", "prints a version");
            } else {
                acc.unk("interpreter", format!("runs {i} code"));
            }
        }
        "make" | "just" | "task" | "ninja" | "cmake" | "gradle" | "gradlew" | "mvn" | "npx"
        | "bunx" => {
            acc.unk(
                "build-tool",
                format!("{name} runs project-defined commands"),
            );
        }
        "nc" | "ncat" | "netcat" | "socat" => {
            if has_flag(args, &['z'], &[]) {
                acc.ro("port-scan", "checks whether ports are open");
            } else {
                acc.unk("netcat", "raw network connection/listener");
            }
        }
        "nmap" => acc.ro("nmap", "network scan"),
        "curl" | "curlie" => classify_curl(args, acc),
        "wget" => {
            if flag_value(args, &["-O", "--output-document"]) == Some("-")
                || has_word(args, &["-qO-", "-O-", "--spider"])
            {
                acc.ro("wget-stdout", "fetches to stdout");
            } else {
                acc.md("wget", "downloads files");
            }
        }
        "http" | "https" | "xh" => {
            let method = args
                .iter()
                .find(|a| !a.starts_with('-'))
                .map(|m| m.to_ascii_uppercase())
                .unwrap_or_default();
            let url = args
                .iter()
                .find(|a| a.contains('/') || a.contains(':'))
                .copied()
                .unwrap_or("");
            let method = if matches!(
                method.as_str(),
                "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD"
            ) {
                method
            } else if args.iter().any(|a| a.contains('=') && !a.starts_with('-')) {
                "POST".into()
            } else {
                "GET".into()
            };
            http_risk(&method, url, "", acc);
        }
        "openssl" => {
            if has_word(args, &["-out", "-keyout"])
                || matches!(args.first().copied(), Some("genrsa" | "genpkey" | "gendsa"))
            {
                acc.md("openssl-write", "writes keys/certificates");
            } else {
                acc.ro("openssl", "inspects keys/certificates/TLS");
            }
        }
        "ssh-keygen" => {
            if has_flag(args, &['l', 'F', 'y', 'L', 'B'], &[]) {
                acc.ro("ssh-keygen-read", "reads key information");
            } else if has_flag(args, &['R'], &[]) {
                acc.md("ssh-keygen-remove-host", "removes a known_hosts entry");
            } else {
                acc.md("ssh-keygen", "generates/changes keys");
            }
        }
        "ssh-add" => {
            if has_flag(args, &['l', 'L'], &[]) {
                acc.ro("ssh-add-list", "lists agent keys");
            } else if has_flag(args, &['D', 'd'], &[]) {
                acc.md("ssh-add-delete", "removes agent keys");
            } else {
                acc.md("ssh-add", "adds agent keys");
            }
        }
        "ssh-copy-id" => acc.md("ssh-copy-id", "installs a public key on a host"),
        // ------------------------------------------------ databases
        "psql" => {
            let cmds = flag_values(args, &["-c", "--command"]);
            if !cmds.is_empty() {
                for c in cmds {
                    classify_sql(c, acc);
                }
            } else if flag_value(args, &["-f", "--file"]).is_some() {
                acc.unk("sql-file", "runs statements from a file");
            } else if has_flag(args, &['l'], &["list", "version"]) {
                acc.ro("psql-list", "lists databases");
            } else {
                acc.unk("sql-interactive", "opens an interactive database session");
            }
        }
        "mysql" | "mariadb" => {
            if let Some(q) = flag_value(args, &["-e", "--execute"]) {
                classify_sql(q, acc);
            } else if has_word(args, &["--version", "-V"]) {
                acc.ro("mysql-version", "prints a version");
            } else {
                acc.unk("sql-interactive", "opens an interactive database session");
            }
        }
        "sqlite3" => {
            let pos = positionals(args, &["-cmd", "-separator", "-newline", "-nullvalue"]);
            if pos.len() >= 2 {
                classify_sql(&pos[1..].join(" "), acc);
            } else {
                acc.unk("sql-interactive", "opens an interactive database session");
            }
        }
        "cqlsh" => match flag_value(args, &["-e", "--execute"]) {
            Some(q) => classify_sql(q, acc),
            None => acc.unk("sql-interactive", "opens an interactive CQL session"),
        },
        "clickhouse-client" | "clickhouse" => match flag_value(args, &["-q", "--query"]) {
            Some(q) => classify_sql(q, acc),
            None => acc.unk("sql-interactive", "opens an interactive database session"),
        },
        "sqlcmd" | "osql" => match flag_value(args, &["-Q", "-q"]) {
            Some(q) => classify_sql(q, acc),
            None => acc.unk("sql-interactive", "opens an interactive database session"),
        },
        "mongosh" | "mongo" => acc.unk("mongo", "runs MongoDB shell code"),
        "pg_dump" | "pg_dumpall" | "mysqldump" | "mariadb-dump" | "mongodump" | "mongoexport"
        | "pg_isready" | "pg_controldata" | "mysqlshow" => {
            if has_word(args, &["-f", "--file", "-o", "--out"])
                || args.iter().any(|a| a.starts_with("--file="))
            {
                acc.md("db-dump-file", "writes a dump file");
            } else {
                acc.ro("db-dump", "reads the database");
            }
        }
        "pg_restore" | "mongorestore" | "mongoimport" | "mysqlimport" => {
            if has_flag(args, &['c'], &["clean", "drop"]) {
                acc.ds("db-restore-clean", "restores with DROP of existing objects");
            } else {
                acc.md("db-restore", "restores data into a database");
            }
        }
        "dropdb" | "dropuser" => acc.ds(name, "drops a database/role"),
        "createdb" | "createuser" | "vacuumdb" | "reindexdb" | "pgbench" => {
            acc.md(name, "changes the database")
        }
        "mysqladmin" => {
            if has_word(args, &["drop", "shutdown", "flush-hosts", "kill"]) {
                acc.ds("mysqladmin", "drops databases / shuts down the server");
            } else if has_word(
                args,
                &[
                    "status",
                    "processlist",
                    "variables",
                    "version",
                    "ping",
                    "extended-status",
                ],
            ) {
                acc.ro("mysqladmin-status", "reads server status");
            } else {
                acc.md("mysqladmin", "administers the server");
            }
        }
        "redis-cli" | "valkey-cli" | "keydb-cli" => classify_redis_cli(args, acc),
        _ if READ_ONLY_TOOLS.contains(&name) => acc.ro("read-only", format!("{name} only reads")),
        ":" => acc.ro("noop", "no-op"),
        "env" => acc.ro("env", "prints the environment"),
        "sudo" | "doas" => acc.unk("sudo-shell", "interactive root shell"),
        _ => acc.unk("unknown-command", format!("no local rule for `{name}`")),
    }
}

fn classify_find(args: &[&str], dialect: ShellDialect, depth: u8, acc: &mut Acc) {
    let mut risky = false;
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "-delete" => {
                acc.ds("find-delete", "find deletes matching files");
                risky = true;
            }
            "-exec" | "-execdir" | "-ok" | "-okdir" => {
                let end = args[i + 1..]
                    .iter()
                    .position(|a| *a == ";" || *a == "+" || *a == "\\;")
                    .map_or(args.len(), |p| i + 1 + p);
                let cmd: Vec<&str> = args[i + 1..end]
                    .iter()
                    .copied()
                    .filter(|a| *a != "{}")
                    .collect();
                classify_words(&cmd, dialect, depth, acc);
                risky = true;
                i = end;
            }
            "-fprint" | "-fprint0" | "-fprintf" | "-fls" => {
                acc.md("find-write", "find writes to a file");
                risky = true;
            }
            _ => {}
        }
        i += 1;
    }
    if !risky {
        acc.ro("find", "searches files");
    }
}

fn classify_systemctl(args: &[&str], acc: &mut Acc) {
    let pos = positionals(
        args,
        &[
            "-H",
            "--host",
            "-M",
            "--machine",
            "-t",
            "--type",
            "-p",
            "--property",
            "-n",
            "--lines",
            "-o",
            "--output",
            "--state",
        ],
    );
    match pos.first().copied() {
        None => acc.ro("systemctl-list", "lists units"),
        Some(
            "stop" | "disable" | "mask" | "kill" | "isolate" | "poweroff" | "reboot" | "halt"
            | "kexec" | "rescue" | "emergency" | "suspend" | "hibernate" | "soft-reboot",
        ) => acc.ds(
            "systemctl-stop",
            format!("systemctl {} (service outage / power)", pos[0]),
        ),
        Some(
            "status" | "show" | "cat" | "is-active" | "is-enabled" | "is-failed"
            | "is-system-running" | "help" | "get-default" | "show-environment"
            | "list-dependencies" | "--version",
        ) => acc.ro("systemctl-read", "reads unit state"),
        Some(s) if s.starts_with("list-") => acc.ro("systemctl-read", "lists units"),
        Some(s) => acc.md("systemctl", format!("systemctl {s}")),
    }
}

fn classify_ip(args: &[&str], acc: &mut Acc) {
    let pos = positionals(
        args,
        &["-n", "-netns", "-b", "-batch", "-f", "-family", "-rc"],
    );
    let action = pos.get(1).copied().unwrap_or("show");
    match action {
        "flush" => acc.ds("ip-flush", "flushes addresses/routes/neighbours"),
        "add" | "del" | "delete" | "change" | "replace" | "set" | "append" | "prepend" | "up"
        | "down" | "attach" | "detach" | "exec" => {
            acc.md("ip-change", format!("ip {action} changes networking"))
        }
        _ => acc.ro("ip-show", "shows network state"),
    }
}

fn classify_package_manager(name: &str, args: &[&str], acc: &mut Acc) {
    let pos = positionals(
        args,
        &[
            "-C",
            "--prefix",
            "-r",
            "--requirement",
            "--cache-dir",
            "--index-url",
            "-i",
        ],
    );
    let sub = pos.first().copied().unwrap_or("");
    let remove = [
        "remove",
        "purge",
        "autoremove",
        "uninstall",
        "erase",
        "rm",
        "del",
        "delete",
        "un",
        "prune",
        "clean-all",
    ];
    let read = [
        "list",
        "ls",
        "search",
        "show",
        "info",
        "policy",
        "outdated",
        "why",
        "view",
        "depends",
        "rdepends",
        "madison",
        "check",
        "doctor",
        "config-list",
        "freeze",
        "whatprovides",
        "provides",
        "repolist",
        "history",
        "audit",
        "env",
        "version",
        "--version",
        "-v",
        "help",
        "tree",
        "metadata",
    ];
    if remove.contains(&sub) {
        acc.ds("pkg-remove", format!("{name} {sub} removes packages"));
    } else if read.contains(&sub)
        || (sub.is_empty()
            && !args.is_empty()
            && args.iter().all(|a| a.starts_with('-'))
            && has_word(args, &["--version", "-v", "-h", "--help"]))
    {
        acc.ro("pkg-query", format!("{name} {sub} only reads"));
    } else if name == "go"
        && !matches!(
            sub,
            "install" | "get" | "mod" | "generate" | "run" | "build" | "clean"
        )
    {
        acc.ro("go-read", "go tooling");
    } else {
        acc.md(
            "pkg-install",
            format!("{name} {sub} installs/changes packages"),
        );
    }
}

fn classify_git(args: &[&str], acc: &mut Acc) {
    let pos = positionals(
        args,
        &["-C", "-c", "--git-dir", "--work-tree", "--namespace"],
    );
    let Some(&sub) = pos.first() else {
        acc.ro("git", "git help");
        return;
    };
    let rest: Vec<&str> = {
        let i = args
            .iter()
            .position(|a| *a == sub)
            .map_or(args.len(), |i| i + 1);
        args[i..].to_vec()
    };
    let rpos = positionals(&rest, &["-m", "-C", "-F", "--author", "-u", "--upstream"]);
    match sub {
        "push" => {
            if has_flag(
                &rest,
                &['f', 'd'],
                &[
                    "force",
                    "force-with-lease",
                    "force-if-includes",
                    "mirror",
                    "delete",
                    "prune",
                ],
            ) || rpos
                .iter()
                .skip(1)
                .any(|r| r.starts_with('+') || r.starts_with(':'))
            {
                acc.ds(
                    "git-push-force",
                    "force-push / remote branch deletion rewrites remote history",
                );
            } else {
                acc.md("git-push", "pushes commits");
            }
        }
        "reset" => {
            if has_word(&rest, &["--hard", "--merge", "--keep"]) {
                acc.ds("git-reset-hard", "discards local changes");
            } else {
                acc.md("git-reset", "moves HEAD / unstages");
            }
        }
        "clean" => {
            if has_flag(&rest, &['n'], &["dry-run"]) {
                acc.ro("git-clean-dry-run", "lists untracked files");
            } else {
                acc.ds("git-clean", "deletes untracked files");
            }
        }
        "checkout" => {
            if has_flag(&rest, &['f'], &["force"]) || rest.contains(&"--") || rpos == ["."] {
                acc.ds("git-checkout-discard", "discards working-tree changes");
            } else {
                acc.md("git-checkout", "switches branches");
            }
        }
        "restore" => {
            if has_word(&rest, &["--staged", "-S"]) && !has_word(&rest, &["--worktree", "-W"]) {
                acc.md("git-restore-staged", "unstages changes");
            } else {
                acc.ds("git-restore", "discards working-tree changes");
            }
        }
        "branch" => {
            if has_word(&rest, &["-D"])
                || (has_flag(&rest, &['d'], &["delete"]) && has_flag(&rest, &['f'], &["force"]))
            {
                acc.ds(
                    "git-branch-force-delete",
                    "force-deletes a branch (unmerged commits lost)",
                );
            } else if has_flag(
                &rest,
                &['d', 'm', 'M', 'c', 'C', 'u'],
                &[
                    "delete",
                    "move",
                    "copy",
                    "set-upstream-to",
                    "unset-upstream",
                ],
            ) || !rpos.is_empty()
                && !has_word(
                    &rest,
                    &["--list", "-l", "--contains", "--merged", "--no-merged"],
                )
            {
                acc.md("git-branch", "changes branches");
            } else {
                acc.ro("git-branch-list", "lists branches");
            }
        }
        "stash" => match rpos.first().copied() {
            Some("drop" | "clear") => acc.ds("git-stash-drop", "deletes stashed changes"),
            Some("list" | "show") => acc.ro("git-stash-list", "lists stashes"),
            _ => acc.md("git-stash", "stashes/applies changes"),
        },
        "reflog" => match rpos.first().copied() {
            Some("expire" | "delete") => acc.ds("git-reflog-expire", "expires reflog entries"),
            _ => acc.ro("git-reflog", "shows the reflog"),
        },
        "filter-branch" | "filter-repo" => acc.ds("git-rewrite", "rewrites repository history"),
        "update-ref" if has_flag(&rest, &['d'], &[]) => {
            acc.ds("git-update-ref-delete", "deletes a ref")
        }
        "config" => {
            if has_word(
                &rest,
                &[
                    "--get",
                    "--get-all",
                    "--list",
                    "-l",
                    "--get-regexp",
                    "--show-origin",
                ],
            ) || rpos.len() == 1
            {
                acc.ro("git-config-read", "reads git config");
            } else {
                acc.md("git-config", "changes git config");
            }
        }
        "remote" => match rpos.first().copied() {
            None | Some("show" | "get-url" | "-v") => acc.ro("git-remote-read", "lists remotes"),
            _ => acc.md("git-remote", "changes remotes"),
        },
        "tag" => {
            if has_flag(&rest, &['d'], &["delete"]) {
                acc.md("git-tag-delete", "deletes a tag");
            } else if rpos.is_empty() || has_flag(&rest, &['l'], &["list"]) {
                acc.ro("git-tag-list", "lists tags");
            } else {
                acc.md("git-tag", "creates a tag");
            }
        }
        "status" | "log" | "diff" | "show" | "blame" | "grep" | "ls-files" | "ls-tree"
        | "ls-remote" | "rev-parse" | "describe" | "shortlog" | "cat-file" | "show-ref"
        | "for-each-ref" | "whatchanged" | "version" | "help" | "rev-list" | "name-rev"
        | "merge-base" | "count-objects" | "fsck" | "check-ignore" | "var" | "range-diff" => {
            acc.ro("git-read", format!("git {sub} only reads"));
        }
        _ => acc.md("git", format!("git {sub} changes the repository")),
    }
}

fn is_dry_run(args: &[&str]) -> bool {
    args.iter().any(|a| {
        *a == "--dry-run"
            || a.starts_with("--dry-run=") && !a.ends_with("=none")
            || *a == "--server-dry-run"
    })
}

const KUBECTL_GLOBAL_VALUE_FLAGS: &[&str] = &[
    "-n",
    "--namespace",
    "--context",
    "--kubeconfig",
    "--cluster",
    "--user",
    "-s",
    "--server",
    "--token",
    "--as",
    "--as-group",
    "--request-timeout",
    "--certificate-authority",
    "--client-certificate",
    "--client-key",
    "-v",
    "--v",
];

fn classify_kubectl(args: &[&str], dialect: ShellDialect, depth: u8, acc: &mut Acc) {
    let pos = positionals(args, KUBECTL_GLOBAL_VALUE_FLAGS);
    let Some(&sub) = pos.first() else {
        acc.ro("kubectl-help", "kubectl help");
        return;
    };
    let dry = is_dry_run(args);
    match sub {
        "get" | "describe" | "logs" | "top" | "explain" | "api-resources" | "api-versions"
        | "version" | "cluster-info" | "events" | "diff" | "wait" | "completion" | "plugin"
        | "kustomize" | "options" | "help" | "can-i" => {
            acc.ro("kubectl-read", format!("kubectl {sub} only reads"))
        }
        "auth" => {
            if pos.get(1).is_some_and(|s| *s == "reconcile") {
                acc.md("kubectl-auth", "reconciles RBAC");
            } else {
                acc.ro("kubectl-read", "checks permissions");
            }
        }
        "config" => match pos.get(1).copied() {
            Some("view" | "get-contexts" | "current-context" | "get-clusters" | "get-users")
            | None => {
                acc.ro("kubectl-config-read", "reads kubeconfig");
            }
            _ => acc.md("kubectl-config", "changes kubeconfig"),
        },
        "rollout" => match pos.get(1).copied() {
            Some("status" | "history") => acc.ro("kubectl-rollout-read", "reads rollout state"),
            _ => acc.md("kubectl-rollout", "restarts/undoes a rollout"),
        },
        "delete" if dry => acc.ro("kubectl-dry-run", "dry run"),
        "delete" => acc.ds("kubectl-delete", "deletes cluster resources"),
        "drain" if dry => acc.ro("kubectl-dry-run", "dry run"),
        "drain" => acc.ds("kubectl-drain", "evicts all pods from a node"),
        "scale" => {
            let replicas = flag_value(args, &["--replicas"]);
            if dry {
                acc.ro("kubectl-dry-run", "dry run");
            } else if replicas == Some("0") {
                acc.ds("kubectl-scale-zero", "scales a workload to zero replicas");
            } else {
                acc.md("kubectl-scale", "scales a workload");
            }
        }
        "exec" => {
            if let Some(i) = args.iter().position(|a| *a == "--") {
                let cmd = &args[i + 1..];
                if cmd.len() == 1 && (SHELLS.contains(&command_name(cmd[0]).as_str())) {
                    acc.unk(
                        "kubectl-exec-shell",
                        "opens an interactive shell in a container",
                    );
                } else if cmd.len() >= 2
                    && SHELLS.contains(&command_name(cmd[0]).as_str())
                    && cmd[1] == "-c"
                {
                    classify_shell(
                        cmd.get(2).copied().unwrap_or(""),
                        ShellDialect::Posix,
                        depth + 1,
                        acc,
                    );
                } else {
                    classify_words(cmd, dialect, depth, acc);
                }
            } else {
                acc.unk("kubectl-exec", "runs a command in a container");
            }
        }
        "attach" | "debug" | "run" => acc.unk(
            "kubectl-run",
            format!("kubectl {sub} runs code in the cluster"),
        ),
        "port-forward" | "proxy" | "cp" => acc.md("kubectl-forward", format!("kubectl {sub}")),
        "apply" | "create" | "replace" | "patch" | "edit" | "label" | "annotate" | "set"
        | "expose" | "autoscale" | "cordon" | "uncordon" | "taint" | "certificate" => {
            if dry {
                acc.ro("kubectl-dry-run", "dry run");
            } else if sub == "replace" && has_word(args, &["--force"]) {
                acc.ds("kubectl-replace-force", "deletes and recreates resources");
            } else {
                acc.md(
                    "kubectl-change",
                    format!("kubectl {sub} changes cluster state"),
                );
            }
        }
        _ => acc.unk("kubectl-unknown", format!("no rule for kubectl {sub}")),
    }
}

fn classify_helm(args: &[&str], acc: &mut Acc) {
    let pos = positionals(
        args,
        &[
            "-n",
            "--namespace",
            "--kube-context",
            "--kubeconfig",
            "-f",
            "--values",
            "--set",
            "--set-string",
            "--version",
            "--repo",
        ],
    );
    let sub = pos.first().copied().unwrap_or("");
    let dry = is_dry_run(args);
    match sub {
        "uninstall" | "delete" | "del" | "un" => {
            if dry {
                acc.ro("helm-dry-run", "dry run");
            } else {
                acc.ds("helm-uninstall", "uninstalls a release");
            }
        }
        "list" | "ls" | "status" | "get" | "history" | "hist" | "show" | "inspect" | "template"
        | "search" | "lint" | "version" | "env" | "verify" | "diff" | "completion" | "help" => {
            acc.ro("helm-read", format!("helm {sub} only reads"));
        }
        "repo" => match pos.get(1).copied() {
            Some("list" | "ls") | None => acc.ro("helm-repo-list", "lists repositories"),
            _ => acc.md("helm-repo", "changes repositories"),
        },
        "install" | "upgrade" if dry => acc.ro("helm-dry-run", "dry run"),
        "" => acc.ro("helm-help", "helm help"),
        _ => acc.md("helm-change", format!("helm {sub} changes releases/config")),
    }
}

fn classify_terraform(args: &[&str], acc: &mut Acc) {
    let pos = positionals(args, &["-chdir"]);
    let sub = pos.first().copied().unwrap_or("");
    match sub {
        "apply" => acc.ds(
            "terraform-apply",
            "applies infrastructure changes (may destroy resources)",
        ),
        "destroy" => acc.ds("terraform-destroy", "destroys infrastructure"),
        "plan" => {
            if has_word(args, &["-destroy"]) {
                acc.ro("terraform-plan", "plans a destroy (no changes)");
            } else {
                acc.ro("terraform-plan", "plans changes (no changes applied)");
            }
        }
        "validate" | "show" | "output" | "graph" | "providers" | "version" | "console" | "help"
        | "metadata" | "test" => acc.ro("terraform-read", format!("terraform {sub} only reads")),
        "fmt" => {
            if has_word(args, &["-check", "-diff"]) {
                acc.ro("terraform-fmt-check", "checks formatting");
            } else {
                acc.md("terraform-fmt", "rewrites files");
            }
        }
        "state" => match pos.get(1).copied() {
            Some("list" | "show" | "pull") => acc.ro("terraform-state-read", "reads state"),
            _ => acc.md("terraform-state", "changes state"),
        },
        "workspace" => match pos.get(1).copied() {
            Some("list" | "show") | None => acc.ro("terraform-workspace-read", "lists workspaces"),
            Some("delete") => acc.ds("terraform-workspace-delete", "deletes a workspace"),
            _ => acc.md("terraform-workspace", "changes workspace"),
        },
        "" => acc.ro("terraform-help", "terraform help"),
        _ => acc.md(
            "terraform-change",
            format!("terraform {sub} changes state/config"),
        ),
    }
}

const DOCKER_GLOBAL_VALUE_FLAGS: &[&str] = &[
    "-H",
    "--host",
    "--context",
    "-c",
    "--config",
    "-l",
    "--log-level",
];

fn classify_docker(args: &[&str], dialect: ShellDialect, depth: u8, acc: &mut Acc) {
    let pos = positionals(args, DOCKER_GLOBAL_VALUE_FLAGS);
    let sub = pos.first().copied().unwrap_or("");
    let second = pos.get(1).copied().unwrap_or("");
    let sub_args: Vec<&str> = {
        let i = args
            .iter()
            .position(|a| *a == sub)
            .map_or(args.len(), |i| i + 1);
        args[i..].to_vec()
    };
    let force = has_flag(&sub_args, &['f'], &["force"]);
    match (sub, second) {
        (_, "prune") => acc.ds(
            "docker-prune",
            "deletes unused containers/images/volumes/networks",
        ),
        ("volume", "rm" | "remove") => acc.ds("docker-volume-rm", "deletes volumes (data loss)"),
        ("rm" | "container", _) if sub == "rm" || second == "rm" => {
            acc.ds("docker-rm", "removes containers");
        }
        ("rmi", _) | ("image", "rm" | "remove") => {
            if force {
                acc.ds("docker-rmi-force", "force-removes images");
            } else {
                acc.md("docker-rmi", "removes images");
            }
        }
        ("stop" | "kill", _) | ("container", "stop" | "kill") => {
            acc.ds("docker-stop", "stops running containers (service outage)")
        }
        ("compose", _) => {
            let i = args
                .iter()
                .position(|a| *a == "compose")
                .map_or(args.len(), |i| i + 1);
            classify_compose(&args[i..], dialect, depth, acc);
        }
        ("exec", _) | ("container", "exec") => {
            let exec_args: Vec<&str> = {
                let i = sub_args
                    .iter()
                    .position(|a| *a == "exec")
                    .map_or(0, |i| i + 1);
                if sub == "exec" {
                    sub_args.clone()
                } else {
                    sub_args[i..].to_vec()
                }
            };
            let p = positionals(
                &exec_args,
                &[
                    "-e",
                    "--env",
                    "-u",
                    "--user",
                    "-w",
                    "--workdir",
                    "--env-file",
                    "--detach-keys",
                ],
            );
            if p.len() <= 1 {
                acc.unk("docker-exec", "runs a command in a container");
            } else if p.len() == 2 && SHELLS.contains(&command_name(p[1]).as_str()) {
                acc.unk(
                    "docker-exec-shell",
                    "opens an interactive shell in a container",
                );
            } else if p.len() >= 4 && SHELLS.contains(&command_name(p[1]).as_str()) && p[2] == "-c"
            {
                classify_shell(p[3], ShellDialect::Posix, depth + 1, acc);
            } else {
                classify_words(&p[1..], dialect, depth, acc);
            }
        }
        (
            "ps" | "images" | "logs" | "inspect" | "stats" | "top" | "version" | "info" | "history"
            | "port" | "diff" | "events" | "search" | "help",
            _,
        )
        | (
            _,
            "ls" | "list" | "inspect" | "logs" | "history" | "df" | "info" | "events" | "show"
            | "port" | "top" | "stats",
        ) => {
            acc.ro("docker-read", format!("docker {sub} only reads"));
        }
        ("", _) => acc.ro("docker-help", "docker help"),
        _ => acc.md(
            "docker-change",
            format!("docker {sub} changes containers/images"),
        ),
    }
}

fn classify_compose(args: &[&str], dialect: ShellDialect, depth: u8, acc: &mut Acc) {
    let pos = positionals(
        args,
        &[
            "-f",
            "--file",
            "-p",
            "--project-name",
            "--env-file",
            "--profile",
            "--project-directory",
        ],
    );
    let sub = pos.first().copied().unwrap_or("");
    match sub {
        "down" => {
            if has_flag(args, &['v'], &["volumes", "rmi"]) {
                acc.ds("compose-down-volumes", "removes containers and volumes");
            } else {
                acc.md("compose-down", "stops and removes containers");
            }
        }
        "rm" | "kill" | "stop" => acc.ds(
            "compose-stop",
            format!("compose {sub} stops/removes services"),
        ),
        "ps" | "logs" | "config" | "ls" | "images" | "top" | "version" | "port" | "events"
        | "help" => {
            acc.ro("compose-read", format!("compose {sub} only reads"));
        }
        "exec" => {
            let p = positionals(
                &args[1..],
                &["-e", "--env", "-u", "--user", "-w", "--workdir", "--index"],
            );
            if p.len() >= 2 {
                classify_words(&p[1..], dialect, depth, acc);
            } else {
                acc.unk("compose-exec", "runs a command in a service");
            }
        }
        _ => acc.md("compose-change", format!("compose {sub} changes services")),
    }
}

fn classify_ansible(args: &[&str], dialect: ShellDialect, depth: u8, acc: &mut Acc) {
    if has_flag(args, &['C'], &["check", "list-hosts"]) {
        acc.ro("ansible-check", "ansible check mode");
        return;
    }
    let module = flag_value(args, &["-m", "--module-name"]).unwrap_or("command");
    let margs = flag_value(args, &["-a", "--args"]);
    match module {
        "ping"
        | "setup"
        | "gather_facts"
        | "debug"
        | "stat"
        | "slurp"
        | "find"
        | "service_facts"
        | "package_facts"
        | "ansible.builtin.ping"
        | "ansible.builtin.setup" => {
            acc.ro("ansible-read", "read-only module");
        }
        "shell"
        | "command"
        | "raw"
        | "ansible.builtin.shell"
        | "ansible.builtin.command"
        | "ansible.builtin.raw" => match margs {
            Some(a) => classify_shell(a, dialect, depth + 1, acc),
            None => acc.unk("ansible-command", "runs a command on hosts"),
        },
        "script" => acc.unk("ansible-script", "runs a script on hosts"),
        _ => acc.md("ansible-module", format!("module {module} changes hosts")),
    }
}

fn classify_curl(args: &[&str], acc: &mut Acc) {
    let method = flag_value(args, &["-X", "--request"]).map(str::to_ascii_uppercase);
    let data = flag_value(
        args,
        &[
            "-d",
            "--data",
            "--data-raw",
            "--data-binary",
            "--data-urlencode",
            "--json",
            "-F",
            "--form",
        ],
    );
    let upload = flag_value(args, &["-T", "--upload-file"]).is_some();
    let head = has_flag(args, &['I'], &["head"]);
    let method = method.unwrap_or_else(|| {
        if upload {
            "PUT".into()
        } else if data.is_some() {
            "POST".into()
        } else if head {
            "HEAD".into()
        } else {
            "GET".into()
        }
    });
    let url = positionals(
        args,
        &[
            "-X",
            "--request",
            "-d",
            "--data",
            "--data-raw",
            "--data-binary",
            "--data-urlencode",
            "--json",
            "-F",
            "--form",
            "-H",
            "--header",
            "-u",
            "--user",
            "-o",
            "--output",
            "-T",
            "--upload-file",
            "-b",
            "--cookie",
            "-c",
            "--cookie-jar",
            "-A",
            "--user-agent",
            "-e",
            "--referer",
            "--connect-timeout",
            "-m",
            "--max-time",
            "-w",
            "--write-out",
            "--cacert",
            "--cert",
            "--key",
            "-x",
            "--proxy",
            "--resolve",
            "-r",
            "--range",
            "--retry",
        ],
    )
    .into_iter()
    .find(|p| p.contains('/') || p.contains('.') || p.contains(':'))
    .unwrap_or("");
    http_risk(&method, url, data.unwrap_or(""), acc);
    if flag_value(args, &["-o", "--output"]).is_some_and(|o| o != "-" && o != "/dev/null")
        || has_flag(args, &['O'], &["remote-name", "remote-name-all"])
    {
        acc.md("curl-output", "writes the response to a file");
    }
}

/// Risk of an HTTP request (curl/httpie or OpenSearch console line).
fn http_risk(method: &str, url: &str, body: &str, acc: &mut Acc) {
    let path = url.split('?').next().unwrap_or(url);
    let lower = path.to_ascii_lowercase();
    let read_endpoints = [
        "_search",
        "_msearch",
        "_count",
        "_mget",
        "_explain",
        "_validate",
        "_analyze",
        "_field_caps",
        "_rank_eval",
        "_termvectors",
        "_mtermvectors",
        "_async_search",
        "_sql",
        "_ppl",
        "_cat",
        "_mapping",
        "_settings",
        "_stats",
        "_health",
        "_nodes",
        "_graphql",
        "graphql",
        "_render",
    ];
    match method {
        "DELETE" => acc.ds("http-delete", "HTTP DELETE removes a resource"),
        _ if lower.contains("_delete_by_query") => {
            acc.ds("http-delete-by-query", "deletes documents by query")
        }
        "GET" | "HEAD" | "OPTIONS" => acc.ro("http-get", "HTTP read"),
        "POST"
            if read_endpoints.iter().any(|e| lower.contains(e)) && !lower.contains("_update") =>
        {
            acc.ro("http-post-read", "read-only search/query endpoint");
        }
        "POST" | "PUT" | "PATCH" => {
            if lower.contains("_bulk") && body.contains("\"delete\"") {
                acc.ds("http-bulk-delete", "bulk request contains deletes");
            } else {
                acc.md("http-write", format!("HTTP {method} changes data"));
            }
        }
        _ => acc.unk("http-method", format!("HTTP {method}")),
    }
}

// ---------------------------------------------------------------------------
// PowerShell & cmd
// ---------------------------------------------------------------------------

fn ps_alias(name: &str) -> Option<&'static str> {
    Some(match name {
        "ls" | "dir" | "gci" => "get-childitem",
        "cat" | "gc" | "type" => "get-content",
        "rm" | "del" | "erase" | "rd" | "rmdir" | "ri" => "remove-item",
        "cp" | "copy" | "cpi" => "copy-item",
        "mv" | "move" | "mi" => "move-item",
        "ren" | "rni" => "rename-item",
        "echo" | "write" => "write-output",
        "ps" | "gps" => "get-process",
        "kill" | "spps" => "stop-process",
        "gsv" => "get-service",
        "sasv" => "start-service",
        "spsv" => "stop-service",
        "iex" => "invoke-expression",
        "iwr" | "curl" | "wget" => "invoke-webrequest",
        "irm" => "invoke-restmethod",
        "ni" | "md" | "mkdir" => "new-item",
        "sc" => "set-content",
        "ac" => "add-content",
        "cls" | "clear" => "clear-host",
        "select" => "select-object",
        "where" | "?" => "where-object",
        "%" | "foreach" => "foreach-object",
        "sort" => "sort-object",
        "ft" => "format-table",
        "fl" => "format-list",
        "measure" => "measure-object",
        "cd" | "sl" | "chdir" => "set-location",
        "pwd" | "gl" => "get-location",
        "h" | "history" | "ghy" => "get-history",
        "gm" => "get-member",
        "gcm" => "get-command",
        "sal" => "set-alias",
        "saps" | "start" => "start-process",
        "icm" => "invoke-command",
        _ => return None,
    })
}

fn classify_powershell(
    name_raw: &str,
    args: &[&str],
    depth: u8,
    upstream: Option<&str>,
    acc: &mut Acc,
) {
    if name_raw.starts_with('$') {
        // An expression (`$x = 1`, `$_.Status -eq 'Running'`), unless it
        // invokes methods.
        if name_raw.contains('(') || args.iter().any(|a| a.contains('(')) {
            acc.unk("ps-method-call", "expression invokes methods");
        } else {
            acc.ro("ps-expression", "PowerShell expression");
        }
        return;
    }
    let lower = name_raw.to_ascii_lowercase();
    let cmdlet = ps_alias(&lower).map(str::to_owned).unwrap_or(lower.clone());
    if !cmdlet.contains('-') || cmdlet.starts_with('-') {
        // Native executable (git, kubectl, …).
        classify_tool(
            &command_name(name_raw),
            args,
            ShellDialect::PowerShell,
            depth,
            upstream,
            acc,
        );
        return;
    }
    let lower_args: Vec<String> = args.iter().map(|a| a.to_ascii_lowercase()).collect();
    let largs: Vec<&str> = lower_args.iter().map(String::as_str).collect();
    if has_word(&largs, &["-whatif"]) {
        acc.ro("ps-whatif", "-WhatIf simulation");
        return;
    }
    let (verb, noun) = cmdlet.split_once('-').unwrap_or((&cmdlet, ""));
    match (verb, noun) {
        ("invoke", "expression") | ("start", "process") | ("invoke", "item") => {
            acc.unk("ps-invoke", format!("{cmdlet} runs arbitrary code"));
        }
        ("invoke", "command") => {
            acc.unk("ps-invoke-command", "runs a script block (possibly remote)")
        }
        ("invoke", "webrequest" | "restmethod") => {
            let method = flag_value(&largs, &["-method"])
                .unwrap_or("get")
                .to_ascii_uppercase();
            let url = args
                .iter()
                .find(|a| a.contains("://"))
                .copied()
                .unwrap_or("");
            http_risk(&method, url, "", acc);
            if has_word(&largs, &["-outfile"]) {
                acc.md("ps-outfile", "writes the response to a file");
            }
        }
        ("remove" | "uninstall" | "clear" | "format" | "reset", _) if cmdlet != "clear-host" => {
            if cmdlet == "remove-item" && has_word(&largs, &["-recurse", "-r"]) {
                acc.ds("ps-remove-recurse", "recursively deletes items");
            } else {
                acc.ds("ps-remove", format!("{cmdlet} deletes/clears data"));
            }
        }
        ("stop", "computer")
        | ("restart", "computer")
        | ("stop", "service")
        | ("stop", "vm")
        | ("initialize", "disk")
        | ("clear", "disk")
        | ("disable", "netadapter") => {
            acc.ds(
                "ps-disruptive",
                format!("{cmdlet} causes an outage or data loss"),
            );
        }
        (
            "get" | "test" | "find" | "select" | "where" | "measure" | "format" | "sort" | "group"
            | "compare" | "convertto" | "convertfrom" | "show" | "resolve" | "read" | "search"
            | "wait" | "split" | "join" | "trace" | "debug" | "foreach",
            _,
        )
        | (
            "write",
            "output" | "host" | "verbose" | "debug" | "information" | "warning" | "error"
            | "progress",
        )
        | ("out", "string" | "host" | "null" | "gridview" | "default")
        | ("set" | "push" | "pop", "location")
        | ("clear", "host") => acc.ro("ps-read", format!("{cmdlet} only reads")),
        _ => acc.md("ps-change", format!("{cmdlet} changes state")),
    }
}

fn classify_cmd(name_raw: &str, args: &[&str], depth: u8, upstream: Option<&str>, acc: &mut Acc) {
    let name = command_name(name_raw);
    let largs_owned: Vec<String> = args.iter().map(|a| a.to_ascii_lowercase()).collect();
    let largs: Vec<&str> = largs_owned.iter().map(String::as_str).collect();
    match name.as_str() {
        "dir" | "type" | "echo" | "cd" | "chdir" | "cls" | "ver" | "vol" | "where" | "whoami"
        | "hostname" | "ping" | "tracert" | "pathping" | "nslookup" | "netstat" | "tasklist"
        | "systeminfo" | "tree" | "findstr" | "find" | "more" | "fc" | "comp" | "title"
        | "color" | "pause" | "rem" | "getmac" | "arp" | "qwinsta" | "query" | "driverquery"
        | "gpresult" => acc.ro("cmd-read", format!("{name} only reads")),
        "set" | "path" => {
            if largs.iter().any(|a| a.contains('=')) || name == "path" && !largs.is_empty() {
                acc.md("cmd-set", "changes environment variables");
            } else {
                acc.ro("cmd-set-list", "lists environment variables");
            }
        }
        "ipconfig" => {
            if has_prefix(&largs, &["/release", "/renew", "/flushdns", "/registerdns"]) {
                acc.md("ipconfig-change", "changes network configuration");
            } else {
                acc.ro("ipconfig", "shows network configuration");
            }
        }
        "del" | "erase" => acc.ds("cmd-del", "deletes files"),
        "rd" | "rmdir" => {
            if largs.contains(&"/s") {
                acc.ds("cmd-rd-s", "recursively deletes a directory tree");
            } else {
                acc.md("cmd-rd", "removes an empty directory");
            }
        }
        "format" | "diskpart" | "cipher"
            if name != "cipher" || largs.iter().any(|a| a.starts_with("/w")) =>
        {
            acc.ds("cmd-disk", format!("{name} can erase disks/data"));
        }
        "shutdown" => {
            if largs.contains(&"/a") {
                acc.md("shutdown-abort", "aborts a shutdown");
            } else {
                acc.ds("power", "shuts down / restarts the machine");
            }
        }
        "reg" => match largs.first().copied() {
            Some("query" | "export" | "compare") => acc.ro("reg-read", "reads the registry"),
            Some("delete") => acc.ds("reg-delete", "deletes registry keys"),
            _ => acc.md("reg-change", "changes the registry"),
        },
        "sc" => match largs.first().copied() {
            Some("query" | "qc" | "queryex" | "qdescription" | "enumdepend") => {
                acc.ro("sc-read", "reads services")
            }
            Some("delete" | "stop") => acc.ds("sc-stop", "stops/deletes a service"),
            _ => acc.md("sc-change", "changes services"),
        },
        "net" => {
            let sub = largs.first().copied().unwrap_or("");
            match sub {
                "stop" => acc.ds("net-stop", "stops a service"),
                "start" if largs.len() == 1 => acc.ro("net-start-list", "lists services"),
                "user" | "localgroup" | "group" => {
                    if largs.iter().any(|a| *a == "/delete" || *a == "/del") {
                        acc.ds("net-user-delete", "deletes a user/group");
                    } else if largs.len() <= 2 && !largs.iter().any(|a| a.starts_with('/')) {
                        acc.ro("net-user-read", "shows users");
                    } else {
                        acc.md("net-user", "changes users/groups");
                    }
                }
                "view" | "config" | "statistics" | "accounts" if largs.len() <= 2 => {
                    acc.ro("net-read", "reads network info")
                }
                "share" | "use" if largs.len() == 1 => acc.ro("net-read", "lists shares"),
                _ => acc.md("net", format!("net {sub}")),
            }
        }
        "taskkill" => acc.md("taskkill", "terminates processes"),
        "bcdedit" => {
            if largs.is_empty() || largs.contains(&"/enum") {
                acc.ro("bcdedit-read", "reads boot configuration");
            } else {
                acc.ds("bcdedit", "changes boot configuration");
            }
        }
        "vssadmin" | "wbadmin" => {
            if largs.first().is_some_and(|a| *a == "delete") {
                acc.ds("shadow-delete", "deletes shadow copies/backups");
            } else if largs.first().is_some_and(|a| *a == "list" || *a == "get") {
                acc.ro("shadow-read", "lists shadow copies/backups");
            } else {
                acc.md(&name, "changes backups");
            }
        }
        "schtasks" => {
            if largs.contains(&"/delete") {
                acc.ds("schtasks-delete", "deletes scheduled tasks");
            } else if largs.contains(&"/query") || largs.is_empty() {
                acc.ro("schtasks-read", "lists scheduled tasks");
            } else {
                acc.md("schtasks", "changes scheduled tasks");
            }
        }
        "robocopy" => {
            if largs.iter().any(|a| *a == "/mir" || *a == "/purge") {
                acc.ds(
                    "robocopy-mirror",
                    "mirrors and deletes extra files at the destination",
                );
            } else if largs.contains(&"/l") {
                acc.ro("robocopy-list", "lists only");
            } else {
                acc.md("robocopy", "copies files");
            }
        }
        "wmic" => {
            if largs.contains(&"delete") {
                acc.ds("wmic-delete", "deletes WMI objects");
            } else if largs
                .iter()
                .any(|a| *a == "call" || *a == "set" || *a == "create")
            {
                acc.md("wmic-change", "changes WMI objects");
            } else {
                acc.ro("wmic-read", "reads WMI data");
            }
        }
        "cmd" => match largs.iter().position(|a| *a == "/c" || *a == "/k") {
            Some(i) => classify_shell(&args[i + 1..].join(" "), ShellDialect::Cmd, depth + 1, acc),
            None => acc.unk("cmd-shell", "starts an interactive shell"),
        },
        "copy" | "xcopy" | "move" | "ren" | "rename" | "mkdir" | "md" | "attrib" | "icacls"
        | "takeown" | "mklink" | "setx" | "cacls" | "compact" | "expand" | "subst" | "assoc"
        | "ftype" | "netsh" | "pnputil" | "dism" | "sfc" | "chkdsk" => {
            if name == "netsh" && largs.contains(&"show")
                || name == "chkdsk"
                    && largs
                        .iter()
                        .all(|a| !a.starts_with("/f") && !a.starts_with("/r"))
            {
                acc.ro(&name, format!("{name} only reads"));
            } else {
                acc.md(&name, format!("{name} changes files/system"));
            }
        }
        "call" | "start" => acc.unk("cmd-run", "runs another program/script"),
        _ => classify_tool(&name, args, ShellDialect::Cmd, depth, upstream, acc),
    }
}

// ---------------------------------------------------------------------------
// SQL / CQL
// ---------------------------------------------------------------------------

/// Split on `;` outside quotes, dollar-quotes and comments; comments removed.
fn split_sql(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut quote: Option<char> = None;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            cur.push(c);
            if c == q {
                if chars.get(i + 1) == Some(&q) {
                    cur.push(q);
                    i += 1;
                } else {
                    quote = None;
                }
            }
            i += 1;
            continue;
        }
        match c {
            '\'' | '"' | '`' => {
                quote = Some(c);
                cur.push(c);
            }
            '-' if chars.get(i + 1) == Some(&'-') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                cur.push(' ');
                continue;
            }
            '#' if cur.trim().is_empty() => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    i += 1;
                }
                i += 2;
                cur.push(' ');
                continue;
            }
            '$' if chars.get(i + 1) == Some(&'$') => {
                // $$ … $$ body
                cur.push_str("$$");
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '$' && chars[i + 1] == '$') {
                    cur.push(chars[i]);
                    i += 1;
                }
                cur.push_str("$$");
                i += 2;
                continue;
            }
            ';' => {
                if !cur.trim().is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
            }
            '\n' if cur.trim_start().starts_with('\\') => {
                // psql meta-command ends at end of line.
                out.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
        i += 1;
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// Uppercased words outside quotes/parentheses (top level only).
fn sql_top_words(stmt: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    for c in stmt.chars() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' | '`' => quote = Some(c),
            '(' => {
                depth += 1;
                if !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
            }
            ')' => depth -= 1,
            c if c.is_alphanumeric() || c == '_' || c == '\\' => {
                if depth == 0 {
                    cur.push(c.to_ascii_uppercase());
                }
            }
            _ => {
                if !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
            }
        }
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words
}

/// Uppercased words outside quotes (including inside parentheses).
fn sql_all_words(stmt: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in stmt.chars() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' | '`' => quote = Some(c),
            c if c.is_alphanumeric() || c == '_' => cur.push(c.to_ascii_uppercase()),
            _ => {
                if !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
            }
        }
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words
}

static TRIVIAL_WHERE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bwhere\s+(?:1\s*=\s*1|true|1|'1'\s*=\s*'1')\s*(?:;|$)").expect("valid regex")
});

fn classify_sql(text: &str, acc: &mut Acc) {
    let stmts = split_sql(text);
    if stmts.is_empty() {
        acc.ro("sql-empty", "no statements");
        return;
    }
    for stmt in stmts {
        classify_sql_statement(stmt.trim(), acc);
    }
}

fn classify_sql_statement(stmt: &str, acc: &mut Acc) {
    if let Some(meta) = stmt.strip_prefix('\\') {
        let cmd = meta.split_whitespace().next().unwrap_or("");
        match cmd {
            "i" | "ir" | "include" | "include_relative" => {
                acc.unk("psql-include", "runs statements from a file")
            }
            "!" => acc.unk("psql-shell", "runs a shell command"),
            "copy" if meta.to_ascii_lowercase().contains(" from ") => {
                acc.md("psql-copy-from", "loads data into a table")
            }
            "g" | "gexec" => acc.unk("psql-gexec", "executes generated statements"),
            _ => acc.ro("psql-meta", format!("psql \\{cmd}")),
        }
        return;
    }
    let words = sql_top_words(stmt);
    let Some(first) = words.first().map(String::as_str) else {
        acc.ro("sql-empty", "empty statement");
        return;
    };
    let has = |w: &str| words.iter().any(|x| x == w);
    let trivial_where = TRIVIAL_WHERE.is_match(stmt);
    match first {
        "SELECT" => {
            let into = words.iter().position(|w| w == "INTO");
            let from = words.iter().position(|w| w == "FROM");
            match into {
                Some(i) if from.is_none_or(|f| i < f) => {
                    if words
                        .get(i + 1)
                        .is_some_and(|w| w == "OUTFILE" || w == "DUMPFILE")
                    {
                        acc.md("sql-into-outfile", "writes a file on the server");
                    } else {
                        acc.md(
                            "sql-select-into",
                            "SELECT INTO creates a table / sets variables",
                        );
                    }
                }
                _ if words
                    .windows(2)
                    .any(|w| w[0] == "FOR" && (w[1] == "UPDATE" || w[1] == "SHARE")) =>
                {
                    acc.md(
                        "sql-select-for-update",
                        "SELECT … FOR UPDATE takes row locks",
                    );
                }
                _ => acc.ro("sql-select", "SELECT only reads"),
            }
        }
        "SHOW" | "DESCRIBE" | "DESC" | "VALUES" | "TABLE" | "USE" | "LIST" | "HELP"
        | "CONSISTENCY" | "TRACING" | "PAGING" | "EXPAND" => {
            acc.ro("sql-read", format!("{first} only reads"))
        }
        "EXPLAIN" => {
            if words
                .get(1)
                .is_some_and(|w| w == "ANALYZE" || w == "ANALYSE")
            {
                // EXPLAIN ANALYZE executes the statement.
                let rest = stmt
                    .split_once(char::is_whitespace)
                    .and_then(|(_, r)| r.trim_start().split_once(char::is_whitespace))
                    .map_or("", |(_, r)| r);
                classify_sql_statement(rest.trim(), acc);
            } else {
                acc.ro("sql-explain", "EXPLAIN only plans");
            }
        }
        "WITH" => {
            let dml = ["DELETE", "UPDATE", "INSERT", "MERGE"];
            let all = sql_all_words(stmt);
            let pos_dml = all.iter().position(|w| dml.contains(&w.as_str()));
            let words = all;
            match pos_dml {
                Some(p) => {
                    let tail = words[p..].join(" ");
                    classify_sql_statement(&tail, acc);
                }
                None => acc.ro("sql-select", "CTE query only reads"),
            }
        }
        "DROP" => acc.ds(
            "sql-drop",
            format!("DROP {}", words.get(1).map_or("", String::as_str)),
        ),
        "TRUNCATE" => acc.ds("sql-truncate", "TRUNCATE deletes all rows"),
        "DELETE" => {
            if !has("WHERE") || trivial_where {
                acc.ds(
                    "sql-delete-no-where",
                    "DELETE without a (meaningful) WHERE deletes all rows",
                );
            } else {
                acc.md("sql-delete", "DELETE with WHERE");
            }
        }
        "UPDATE" => {
            if !has("WHERE") || trivial_where {
                acc.ds(
                    "sql-update-no-where",
                    "UPDATE without a (meaningful) WHERE changes all rows",
                );
            } else {
                acc.md("sql-update", "UPDATE with WHERE");
            }
        }
        "ALTER" => {
            if has("DROP") {
                acc.ds(
                    "sql-alter-drop",
                    "ALTER … DROP removes columns/constraints/partitions",
                );
            } else {
                acc.md("sql-alter", "ALTER changes schema");
            }
        }
        "COPY" => {
            if has("TO") && !has("FROM")
                || words.windows(2).any(|w| w[0] == "TO" && w[1] == "STDOUT")
            {
                acc.ro("sql-copy-to", "COPY … TO exports data");
            } else {
                acc.md("sql-copy-from", "COPY … FROM loads data");
            }
        }
        "INSERT" | "UPSERT" | "MERGE" | "REPLACE" | "LOAD" | "CALL" | "DO" | "EXEC" | "EXECUTE"
        | "SET" | "RESET" | "BEGIN" | "START" | "COMMIT" | "END" | "ROLLBACK" | "SAVEPOINT"
        | "RELEASE" | "LOCK" | "UNLOCK" | "GRANT" | "REVOKE" | "COMMENT" | "REINDEX" | "VACUUM"
        | "ANALYZE" | "ANALYSE" | "CLUSTER" | "REFRESH" | "CREATE" | "RENAME" | "NOTIFY"
        | "LISTEN" | "UNLISTEN" | "PREPARE" | "DEALLOCATE" | "DISCARD" | "CHECKPOINT"
        | "IMPORT" | "SECURITY" | "BATCH" | "APPLY" | "OPTIMIZE" | "REPAIR" | "FLUSH" | "KILL"
        | "INSTALL" | "UNINSTALL" | "PURGE" | "HANDLER" | "SOURCE" => {
            if matches!(first, "PURGE" | "KILL") {
                acc.ds("sql-purge", format!("{first} removes data/sessions"));
            } else if first == "SOURCE" {
                acc.unk("sql-source", "runs statements from a file");
            } else {
                acc.md("sql-change", format!("{first} changes the database"));
            }
        }
        "PRAGMA" => {
            if stmt.contains('=') {
                acc.md("sql-pragma-set", "changes a PRAGMA");
            } else {
                acc.ro("sql-pragma", "reads a PRAGMA");
            }
        }
        _ => acc.unk("sql-unknown", format!("no rule for {first}")),
    }
}

// ---------------------------------------------------------------------------
// Redis
// ---------------------------------------------------------------------------

fn classify_redis_cli(args: &[&str], acc: &mut Acc) {
    let value_flags = [
        "-h",
        "-p",
        "-a",
        "-n",
        "-u",
        "--user",
        "--pass",
        "-r",
        "-i",
        "-s",
        "--sni",
        "--cacert",
        "--cert",
        "--key",
        "-d",
        "--pattern",
        "--count",
        "--eval",
        "--rdb",
    ];
    if has_word(
        args,
        &[
            "--scan",
            "--bigkeys",
            "--memkeys",
            "--hotkeys",
            "--latency",
            "--latency-history",
            "--stat",
            "--version",
            "--help",
        ],
    ) {
        acc.ro("redis-cli-read", "redis-cli diagnostic mode");
        return;
    }
    if args.iter().any(|a| a.starts_with("--eval")) {
        acc.unk("redis-eval", "runs a Lua script");
        return;
    }
    let pos = positionals(args, &value_flags);
    if pos.is_empty() {
        acc.unk(
            "redis-interactive",
            "opens an interactive redis-cli session",
        );
    } else {
        classify_redis(&pos, acc);
    }
}

fn classify_redis(words: &[&str], acc: &mut Acc) {
    let Some(first) = words.first() else { return };
    let cmd = first.to_ascii_uppercase();
    let sub = words
        .get(1)
        .map(|s| s.to_ascii_uppercase())
        .unwrap_or_default();
    let ds = [
        "FLUSHALL",
        "FLUSHDB",
        "SHUTDOWN",
        "SWAPDB",
        "REPLICAOF",
        "SLAVEOF",
        "FAILOVER",
    ];
    let ro = [
        "GET",
        "MGET",
        "HGET",
        "HGETALL",
        "HKEYS",
        "HVALS",
        "HLEN",
        "HEXISTS",
        "HMGET",
        "HSCAN",
        "HSTRLEN",
        "HRANDFIELD",
        "KEYS",
        "SCAN",
        "SSCAN",
        "ZSCAN",
        "EXISTS",
        "TYPE",
        "TTL",
        "PTTL",
        "EXPIRETIME",
        "PEXPIRETIME",
        "STRLEN",
        "GETRANGE",
        "SUBSTR",
        "LRANGE",
        "LLEN",
        "LINDEX",
        "LPOS",
        "SMEMBERS",
        "SISMEMBER",
        "SMISMEMBER",
        "SCARD",
        "SRANDMEMBER",
        "SINTER",
        "SINTERCARD",
        "SUNION",
        "SDIFF",
        "ZRANGE",
        "ZRANGEBYSCORE",
        "ZRANGEBYLEX",
        "ZREVRANGE",
        "ZREVRANGEBYSCORE",
        "ZREVRANGEBYLEX",
        "ZRANK",
        "ZREVRANK",
        "ZSCORE",
        "ZMSCORE",
        "ZCARD",
        "ZCOUNT",
        "ZLEXCOUNT",
        "ZRANDMEMBER",
        "ZDIFF",
        "ZINTER",
        "ZUNION",
        "INFO",
        "PING",
        "ECHO",
        "DBSIZE",
        "TIME",
        "LASTSAVE",
        "MONITOR",
        "XRANGE",
        "XREVRANGE",
        "XLEN",
        "XINFO",
        "XREAD",
        "XPENDING",
        "PFCOUNT",
        "GEODIST",
        "GEOPOS",
        "GEOHASH",
        "GEOSEARCH",
        "GEORADIUS_RO",
        "BITCOUNT",
        "BITPOS",
        "GETBIT",
        "BITFIELD_RO",
        "OBJECT",
        "COMMAND",
        "ROLE",
        "LATENCY",
        "RANDOMKEY",
        "DUMP",
        "SUBSCRIBE",
        "PSUBSCRIBE",
        "SSUBSCRIBE",
        "UNSUBSCRIBE",
        "SELECT",
        "AUTH",
        "HELLO",
        "QUIT",
        "WAIT",
        "LCS",
        "TOUCH",
        "READONLY",
        "READWRITE",
        "SORT_RO",
        "JSON.GET",
        "JSON.MGET",
        "JSON.TYPE",
        "FT.SEARCH",
        "FT.INFO",
        "FT.AGGREGATE",
        "FT._LIST",
        "TS.GET",
        "TS.RANGE",
        "TS.MRANGE",
        "TS.INFO",
    ];
    match cmd.as_str() {
        c if ds.contains(&c) => acc.ds(
            "redis-destructive",
            format!("{c} wipes/stops/re-points the server"),
        ),
        "DEBUG" => acc.ds("redis-debug", "DEBUG can crash or block the server"),
        "SCRIPT" | "FUNCTION" if matches!(sub.as_str(), "FLUSH" | "DELETE" | "KILL") => {
            acc.ds("redis-script-flush", "removes scripts/functions");
        }
        "CLUSTER" => match sub.as_str() {
            "INFO" | "NODES" | "SLOTS" | "SHARDS" | "MYID" | "KEYSLOT" | "COUNTKEYSINSLOT"
            | "GETKEYSINSLOT" | "LINKS" => acc.ro("redis-cluster-read", "reads cluster state"),
            "RESET" | "FLUSHSLOTS" | "FORGET" | "DELSLOTS" | "DELSLOTSRANGE" => {
                acc.ds("redis-cluster-reset", "resets/removes cluster membership")
            }
            _ => acc.md("redis-cluster", "changes cluster configuration"),
        },
        "CONFIG" => match sub.as_str() {
            "GET" => acc.ro("redis-config-get", "reads configuration"),
            _ => acc.md("redis-config", "changes configuration"),
        },
        "CLIENT" => match sub.as_str() {
            "LIST" | "INFO" | "GETNAME" | "ID" | "TRACKINGINFO" | "GETREDIR" => {
                acc.ro("redis-client-read", "reads clients")
            }
            _ => acc.md("redis-client", "changes client state"),
        },
        "SLOWLOG" | "MEMORY" | "ACL" | "MODULE" => match sub.as_str() {
            "GET" | "LEN" | "USAGE" | "STATS" | "DOCTOR" | "LIST" | "WHOAMI" | "USERS"
            | "GETUSER" | "CAT" | "LOG" | "MALLOC-STATS" => {
                acc.ro("redis-read", format!("{cmd} {sub}"))
            }
            "DELUSER" => acc.ds("redis-acl-deluser", "deletes users"),
            _ => acc.md("redis-admin", format!("{cmd} {sub}")),
        },
        "EVAL" | "EVALSHA" | "EVAL_RO" | "EVALSHA_RO" | "FCALL" | "FCALL_RO" => {
            acc.unk("redis-eval", "runs a script")
        }
        c if ro.contains(&c) => acc.ro("redis-read", format!("{c} only reads")),
        c if c
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '_') =>
        {
            acc.md("redis-write", format!("{c} changes data"));
        }
        _ => acc.unk("redis-unknown", format!("no rule for {cmd}")),
    }
}

// ---------------------------------------------------------------------------
// OpenSearch console
// ---------------------------------------------------------------------------

static OS_REQUEST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?im)^[ \t]*(GET|POST|PUT|DELETE|HEAD|PATCH)[ \t]+(\S+)").expect("valid regex")
});

fn classify_opensearch_console(text: &str, acc: &mut Acc) {
    let reqs: Vec<(String, String, usize, usize)> = OS_REQUEST
        .captures_iter(text)
        .filter_map(|c| {
            let m = c.get(0)?;
            Some((
                c[1].to_ascii_uppercase(),
                c[2].to_owned(),
                m.start(),
                m.end(),
            ))
        })
        .collect();
    if reqs.is_empty() {
        acc.unk("opensearch-unknown", "no request line (METHOD path)");
        return;
    }
    for (i, (method, path, _, end)) in reqs.iter().enumerate() {
        let body_end = reqs.get(i + 1).map_or(text.len(), |n| n.2);
        let body = &text[*end..body_end];
        http_risk(method, path, body, acc);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lvl(cmd: &str) -> RiskLevel {
        classify(cmd, CommandDialect::Posix).level
    }

    #[test]
    fn combine_only_raises() {
        use RiskLevel::*;
        assert_eq!(combine_with_ai(ReadOnly, Some(Destructive)), Destructive);
        assert_eq!(combine_with_ai(Destructive, Some(ReadOnly)), Destructive);
        assert_eq!(combine_with_ai(Unknown, Some(ReadOnly)), Unknown);
        assert_eq!(combine_with_ai(Unknown, Some(Modifying)), Unknown);
        assert_eq!(combine_with_ai(Modifying, Some(Unknown)), Unknown);
        assert_eq!(combine_with_ai(ReadOnly, None), ReadOnly);
    }

    #[test]
    fn basic_posix() {
        use RiskLevel::*;
        assert_eq!(lvl("ls -la /var/log | grep err"), ReadOnly);
        assert_eq!(lvl("rm -rf /"), Destructive);
        assert_eq!(lvl("sudo rm -rf /var/lib/docker"), Destructive);
        assert_eq!(lvl("echo hi > /tmp/x"), Modifying);
        assert_eq!(lvl("frobnicate --all"), Unknown);
        assert_eq!(lvl(""), Unknown);
        assert_eq!(lvl("ls; rm x"), Destructive);
        assert_eq!(lvl("echo $(rm -rf ~)"), Destructive);
    }
}
