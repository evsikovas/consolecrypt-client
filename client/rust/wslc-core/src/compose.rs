//! Docker Compose files translated into WSLc steps. Names, labels and start order follow
//! `docker compose up`; anything WSLc cannot do is reported as a warning, never dropped
//! silently. Every resulting command still goes through the usual argument validation.
use crate::{
    models::*,
    resources::{network_create_args, volume_create_args},
    runspec::{
        command_line, quote, run_arguments, validate_container_name, validate_duration,
        validate_image,
    },
};
use serde_yaml::{Mapping, Value};
use std::collections::{BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};

pub const PROJECT_LABEL: &str = "com.docker.compose.project";
pub const SERVICE_LABEL: &str = "com.docker.compose.service";
pub const HASH_LABEL: &str = "com.docker.compose.config-hash";

/// Most ports one entry may expand to (`8000-8100:8000-8100`).
const MAX_PORTS: usize = 1000;

fn err(message: impl AsRef<str>) -> AppError {
    AppError::new("compose", "parse_compose", message.as_ref())
}

// ---------------------------------------------------------------- environment

/// `.env` syntax: `KEY=value`, `export KEY=value`, quotes, `#` comments.
pub fn dotenv(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            let (key, value) = line.split_once('=')?;
            let value = value.trim();
            let quoted = value.len() >= 2
                && ((value.starts_with('"') && value.ends_with('"'))
                    || (value.starts_with('\'') && value.ends_with('\'')));
            let value = if quoted {
                value[1..value.len() - 1].to_owned()
            } else {
                value
                    .split(" #")
                    .next()
                    .unwrap_or(value)
                    .trim_end()
                    .to_owned()
            };
            Some((key.trim().to_owned(), value))
        })
        .collect()
}

/// Variables for `${…}`: the project's `.env`, overridden by the Windows environment.
pub fn load_env(project_dir: Option<&Path>) -> HashMap<String, String> {
    let mut env = HashMap::new();
    if let Some(dir) = project_dir
        && let Ok(text) = std::fs::read_to_string(dir.join(".env"))
    {
        env.extend(dotenv(&text));
    }
    env.extend(std::env::vars());
    env
}

/// `$VAR`, `${VAR}`, `${VAR:-default}`, `${VAR-default}`, `${VAR:?error}`, `${VAR:+alt}`, `$$`.
pub fn interpolate(
    text: &str,
    env: &HashMap<String, String>,
    missing: &mut BTreeSet<String>,
) -> Result<String, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '$' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        match chars.get(i + 1) {
            Some('$') => {
                out.push('$');
                i += 2;
            }
            Some('{') => {
                let start = i + 2;
                let mut depth = 1;
                let mut j = start;
                while j < chars.len() {
                    match chars[j] {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                if j >= chars.len() {
                    return Err(format!("незакрытая подстановка в «{text}»"));
                }
                let inner: String = chars[start..j].iter().collect();
                out.push_str(&expand(&inner, env, missing)?);
                i = j + 1;
            }
            Some(&c) if c == '_' || c.is_ascii_alphabetic() => {
                let mut j = i + 1;
                while j < chars.len() && (chars[j] == '_' || chars[j].is_ascii_alphanumeric()) {
                    j += 1;
                }
                let name: String = chars[i + 1..j].iter().collect();
                match env.get(&name) {
                    Some(v) => out.push_str(v),
                    None => {
                        missing.insert(name);
                    }
                }
                i = j;
            }
            _ => {
                out.push('$');
                i += 1;
            }
        }
    }
    Ok(out)
}

fn expand(
    inner: &str,
    env: &HashMap<String, String>,
    missing: &mut BTreeSet<String>,
) -> Result<String, String> {
    let end = inner
        .find(|c: char| !(c == '_' || c.is_ascii_alphanumeric()))
        .unwrap_or(inner.len());
    let (name, rest) = inner.split_at(end);
    if name.is_empty() {
        return Err(format!("некорректная подстановка ${{{inner}}}"));
    }
    let value = env.get(name);
    let set = value.is_some();
    let filled = value.is_some_and(|v| !v.is_empty());
    let current = || value.cloned().unwrap_or_default();
    for (op, needs) in [(":-", filled), ("-", set)] {
        if let Some(default) = rest.strip_prefix(op) {
            return if needs {
                Ok(current())
            } else {
                interpolate(default, env, missing)
            };
        }
    }
    for (op, needs) in [(":?", filled), ("?", set)] {
        if let Some(message) = rest.strip_prefix(op) {
            return if needs {
                Ok(current())
            } else if message.is_empty() {
                Err(format!("переменная {name} обязательна"))
            } else {
                Err(format!("{name}: {message}"))
            };
        }
    }
    for (op, needs) in [(":+", filled), ("+", set)] {
        if let Some(alt) = rest.strip_prefix(op) {
            return if needs {
                interpolate(alt, env, missing)
            } else {
                Ok(String::new())
            };
        }
    }
    if !rest.is_empty() {
        return Err(format!("некорректная подстановка ${{{inner}}}"));
    }
    if !set {
        missing.insert(name.to_owned());
    }
    Ok(current())
}

/// Interpolates every string value (keys stay as written) and unwraps `!reset`/`!override` tags.
fn interpolate_tree(
    value: &mut Value,
    env: &HashMap<String, String>,
    missing: &mut BTreeSet<String>,
) -> Result<(), String> {
    match value {
        Value::String(s) => *s = interpolate(s, env, missing)?,
        Value::Sequence(items) => {
            for item in items {
                interpolate_tree(item, env, missing)?;
            }
        }
        Value::Mapping(map) => {
            for (_, item) in map.iter_mut() {
                interpolate_tree(item, env, missing)?;
            }
        }
        Value::Tagged(tagged) => {
            let inner = std::mem::take(&mut tagged.value);
            *value = inner;
            interpolate_tree(value, env, missing)?;
        }
        _ => {}
    }
    Ok(())
}

// ---------------------------------------------------------------- small readers

fn get<'v>(map: &'v Mapping, key: &str) -> Option<&'v Value> {
    map.get(key).filter(|v| !v.is_null())
}

fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn text(map: &Mapping, key: &str) -> Option<String> {
    get(map, key).and_then(scalar)
}

fn flag(map: &Mapping, key: &str) -> bool {
    get(map, key).is_some_and(|v| v.as_bool() == Some(true))
}

/// A string or a list of strings.
fn strings(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::Sequence(items) => items.iter().map(scalar).collect(),
        other => scalar(other).map(|s| vec![s]),
    }
}

/// `KEY: value` maps and `KEY=value` lists; a bare `KEY` has no value.
fn pairs(value: &Value) -> Option<Vec<(String, Option<String>)>> {
    match value {
        Value::Mapping(map) => map
            .iter()
            .map(|(k, v)| Some((scalar(k)?, if v.is_null() { None } else { scalar(v) })))
            .collect(),
        Value::Sequence(items) => items
            .iter()
            .map(|item| {
                let s = scalar(item)?;
                Some(match s.split_once('=') {
                    Some((k, v)) => (k.to_owned(), Some(v.to_owned())),
                    None => (s, None),
                })
            })
            .collect(),
        _ => None,
    }
}

/// Shell-like split for string `command`/`entrypoint`, as Compose does it (no shell runs).
pub fn shell_split(s: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = s.chars();
    let unclosed = || "незакрытая кавычка в команде".to_owned();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(x) => word.push(x),
                        None => return Err(unclosed()),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(x @ ('"' | '\\' | '$' | '`')) => word.push(x),
                            Some(x) => {
                                word.push('\\');
                                word.push(x);
                            }
                            None => return Err(unclosed()),
                        },
                        Some(x) => word.push(x),
                        None => return Err(unclosed()),
                    }
                }
            }
            '\\' => {
                in_word = true;
                if let Some(x) = chars.next() {
                    word.push(x);
                }
            }
            c if c.is_whitespace() => {
                if in_word {
                    out.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        out.push(word);
    }
    Ok(out)
}

/// POSIX shell quoting for the exec form of a health check.
fn sh_quote(arg: &str) -> String {
    let safe = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_./:=@,+-%".contains(c));
    if safe {
        arg.to_owned()
    } else {
        format!("'{}'", arg.replace('\'', r"'\''"))
    }
}

/// Seconds in a Compose duration such as `1m30s`, rounded up.
fn duration_secs(value: &str) -> Option<u32> {
    validate_duration(value).ok()?;
    let mut total = 0.0;
    let mut rest = value;
    while !rest.is_empty() {
        let digits = rest.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
        let n: f64 = rest[..digits].parse().ok()?;
        rest = &rest[digits..];
        let (unit, scale) = [
            ("ns", 1e-9),
            ("us", 1e-6),
            ("µs", 1e-6),
            ("ms", 1e-3),
            ("s", 1.0),
            ("m", 60.0),
            ("h", 3600.0),
        ]
        .into_iter()
        .filter(|(u, _)| rest.starts_with(u))
        .max_by_key(|(u, _)| u.len())?;
        total += n * scale;
        rest = &rest[unit.len()..];
    }
    Some(total.ceil() as u32)
}

/// `512m`, `1gb`, `1073741824` → what `--memory`/`--shm-size` accept.
fn size(value: &Value) -> Option<String> {
    match value {
        Value::Number(n) => Some(format!("{n}b")),
        Value::String(s) => {
            let s = s.trim().to_ascii_lowercase().replace(' ', "");
            let s = s.strip_suffix("ib").map(|x| x.to_owned()).unwrap_or(s);
            let s = match s.strip_suffix('b') {
                Some(x) if x.ends_with(['k', 'm', 'g']) => x.to_owned(),
                _ => s,
            };
            Some(s)
        }
        _ => None,
    }
}

/// FNV-1a: stable across builds, unlike the standard hasher.
fn fnv1a(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Project names as Compose normalizes them: lowercase letters, digits, `_` and `-`.
pub fn normalize_project(name: &str) -> String {
    let s: String = name
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    s.trim_start_matches(['_', '-']).to_owned()
}

fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn is_drive_path(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'\\' | b'/')
}

/// `source:target[:mode]`, keeping the colon of a Windows drive in the source.
fn split_volume(s: &str) -> (Option<String>, String, String) {
    let (drive, rest) = if is_drive_path(s) {
        (&s[..2], &s[2..])
    } else {
        ("", s)
    };
    let parts: Vec<&str> = rest.split(':').collect();
    match parts.as_slice() {
        [target] if drive.is_empty() => (None, (*target).to_owned(), String::new()),
        [source, target] => (
            Some(format!("{drive}{source}")),
            (*target).to_owned(),
            String::new(),
        ),
        [source, target, mode, ..] => (
            Some(format!("{drive}{source}")),
            (*target).to_owned(),
            (*mode).to_owned(),
        ),
        _ => (None, s.to_owned(), String::new()),
    }
}

fn is_host_path(source: &str) -> bool {
    source.starts_with(['.', '/', '~', '\\']) || is_drive_path(source)
}

// ---------------------------------------------------------------- planner

struct Draft {
    service: String,
    image: String,
    build: Option<BuildSpec>,
    pull_policy: String,
    spec: RunSpec,
    extra_networks: Vec<ComposeAttach>,
    depends_on: Vec<ComposeDependency>,
}

struct Planner<'a> {
    project: String,
    dir: Option<&'a Path>,
    env: &'a HashMap<String, String>,
    networks: Option<&'a Mapping>,
    volumes: Option<&'a Mapping>,
    secrets: Option<&'a Mapping>,
    configs: Option<&'a Mapping>,
    warnings: Vec<String>,
    host_folders: BTreeSet<String>,
    used_networks: Vec<String>,
    used_volumes: Vec<String>,
    anonymous: Vec<(String, String)>,
    open_ports: bool,
}

const UNSUPPORTED: &[&str] = &[
    "privileged",
    "cap_add",
    "cap_drop",
    "devices",
    "security_opt",
    "sysctls",
    "pid",
    "ipc",
    "uts",
    "userns_mode",
    "cgroup",
    "cgroup_parent",
    "read_only",
    "group_add",
    "oom_kill_disable",
    "oom_score_adj",
    "runtime",
    "isolation",
    "storage_opt",
    "mac_address",
    "logging",
    "memswap_limit",
    "mem_reservation",
    "mem_swappiness",
    "cpu_shares",
    "cpu_quota",
    "cpu_period",
    "cpu_rt_runtime",
    "cpu_rt_period",
    "cpuset",
    "cpu_count",
    "cpu_percent",
    "pids_limit",
    "blkio_config",
    "device_cgroup_rules",
    "credential_spec",
    "external_links",
    "volumes_from",
    "annotations",
    "attach",
    "develop",
    "post_start",
    "pre_stop",
    "use_api_socket",
    "label_file",
    "models",
    "provider",
];

const HANDLED: &[&str] = &[
    "image",
    "build",
    "container_name",
    "command",
    "entrypoint",
    "environment",
    "env_file",
    "ports",
    "volumes",
    "networks",
    "network_mode",
    "depends_on",
    "working_dir",
    "user",
    "hostname",
    "domainname",
    "labels",
    "healthcheck",
    "tmpfs",
    "ulimits",
    "shm_size",
    "stop_signal",
    "stop_grace_period",
    "dns",
    "dns_search",
    "dns_opt",
    "tty",
    "stdin_open",
    "cpus",
    "mem_limit",
    "deploy",
    "gpus",
    "pull_policy",
    "secrets",
    "configs",
    "profiles",
    "expose",
    "links",
    "restart",
    "scale",
    "platform",
    "init",
    "extra_hosts",
    "extends",
];

impl Planner<'_> {
    fn warn(&mut self, text: String) {
        if !self.warnings.contains(&text) {
            self.warnings.push(text);
        }
    }

    /// An absolute Windows path for a bind source; `None` (with a warning) for Linux host paths.
    fn host_path(&mut self, service: &str, raw: &str) -> AppResult<Option<String>> {
        if raw.starts_with('/') && !raw.starts_with("//") {
            self.warn(format!(
                "{service}: путь {raw} — это путь Linux-хоста, на Windows его нет; подключение пропущено"
            ));
            return Ok(None);
        }
        let path = if let Some(rest) = raw.strip_prefix('~') {
            let home = std::env::var("USERPROFILE").map_err(|_| err("Не найдена папка профиля"))?;
            PathBuf::from(format!("{home}{}", rest.replace('/', "\\")))
        } else if is_drive_path(raw) || raw.starts_with(r"\\") {
            PathBuf::from(raw.replace('/', "\\"))
        } else {
            let dir = self.dir.ok_or_else(|| {
                err(format!(
                    "{service}: относительный путь {raw} — укажите папку проекта"
                ))
            })?;
            dir.join(raw.replace('/', "\\"))
        };
        Ok(Some(lexical(&path).to_string_lossy().into_owned()))
    }

    fn network_name(&self, key: &str) -> String {
        let decl = self
            .networks
            .and_then(|n| get(n, key))
            .and_then(Value::as_mapping);
        if let Some(name) = decl.and_then(|d| text(d, "name")) {
            return name;
        }
        if let Some(Value::Mapping(ext)) = decl.and_then(|d| get(d, "external"))
            && let Some(name) = text(ext, "name")
        {
            return name;
        }
        if decl.is_some_and(|d| get(d, "external").is_some_and(|e| e.as_bool() != Some(false))) {
            return key.to_owned();
        }
        format!("{}_{key}", self.project)
    }

    fn volume_name(&self, key: &str) -> String {
        let decl = self
            .volumes
            .and_then(|v| get(v, key))
            .and_then(Value::as_mapping);
        if let Some(name) = decl.and_then(|d| text(d, "name")) {
            return name;
        }
        if decl.is_some_and(|d| get(d, "external").is_some_and(|e| e.as_bool() != Some(false))) {
            return key.to_owned();
        }
        format!("{}_{key}", self.project)
    }

    fn use_volume(&mut self, key: &str) {
        if !self.used_volumes.iter().any(|v| v == key) {
            self.used_volumes.push(key.to_owned());
        }
    }

    fn mount(
        &mut self,
        service: &str,
        item: &Value,
        mounts: &mut Vec<MountSpec>,
        tmpfs: &mut Vec<String>,
    ) -> AppResult<()> {
        let (kind, source, target, read_only, create) = match item {
            Value::String(s) => {
                let (source, target, mode) = split_volume(s);
                let ro = mode.split(',').any(|m| m == "ro");
                let kind = match &source {
                    Some(src) if is_host_path(src) => "bind",
                    _ => "volume",
                };
                (kind.to_owned(), source, target, ro, true)
            }
            Value::Mapping(m) => {
                let kind = text(m, "type").unwrap_or_else(|| "volume".into());
                let target = text(m, "target")
                    .ok_or_else(|| err(format!("{service}: у подключения нет target")))?;
                let create = get(m, "bind")
                    .and_then(Value::as_mapping)
                    .is_some_and(|b| flag(b, "create_host_path"));
                if kind == "tmpfs" {
                    let limit = get(m, "tmpfs")
                        .and_then(Value::as_mapping)
                        .and_then(|t| get(t, "size"))
                        .and_then(size);
                    tmpfs.push(match limit {
                        Some(s) => format!("{target}:size={s}"),
                        None => target,
                    });
                    return Ok(());
                }
                (
                    kind,
                    text(m, "source"),
                    target,
                    flag(m, "read_only"),
                    create,
                )
            }
            _ => return Err(err(format!("{service}: непонятная запись в volumes"))),
        };
        match kind.as_str() {
            "bind" => {
                let source = source.ok_or_else(|| err(format!("{service}: у bind нет source")))?;
                let Some(path) = self.host_path(service, &source)? else {
                    return Ok(());
                };
                if create && !Path::new(&path).exists() {
                    self.host_folders.insert(path.clone());
                }
                mounts.push(MountSpec {
                    volume: false,
                    source: path,
                    target,
                    read_only,
                });
            }
            "volume" => {
                let source = match source {
                    Some(key) => {
                        if self.volumes.and_then(|v| v.get(key.as_str())).is_none() {
                            return Err(err(format!(
                                "{service}: том «{key}» не объявлен в разделе volumes"
                            )));
                        }
                        self.use_volume(&key);
                        self.volume_name(&key)
                    }
                    None => {
                        // Docker would make an anonymous volume; a stable name keeps data across runs.
                        let name = format!("{}_{service}_{}", self.project, &fnv1a(&target)[..8]);
                        if !self.anonymous.iter().any(|(n, _)| n == &name) {
                            self.anonymous.push((name.clone(), service.to_owned()));
                        }
                        self.warn(format!(
                            "{service}: анонимный том {target} создаётся как именованный {name}"
                        ));
                        name
                    }
                };
                mounts.push(MountSpec {
                    volume: true,
                    source,
                    target,
                    read_only,
                });
            }
            other => self.warn(format!(
                "{service}: подключение типа {other} не поддерживается — пропущено"
            )),
        }
        Ok(())
    }

    /// File-based secrets and configs become read-only bind mounts, as Compose does.
    fn file_mount(
        &mut self,
        service: &str,
        secret: bool,
        item: &Value,
        mounts: &mut Vec<MountSpec>,
    ) -> AppResult<()> {
        let what = if secret {
            "секрет"
        } else {
            "конфиг"
        };
        let (source, target) = match item {
            Value::Mapping(m) => (text(m, "source"), text(m, "target")),
            other => (scalar(other), None),
        };
        let source = source.ok_or_else(|| err(format!("{service}: {what} без имени")))?;
        let decls = if secret { self.secrets } else { self.configs };
        let decl = decls
            .and_then(|d| get(d, &source))
            .and_then(Value::as_mapping)
            .ok_or_else(|| err(format!("{service}: {what} «{source}» не объявлен")))?;
        let Some(file) = text(decl, "file") else {
            self.warn(format!(
                "{service}: {what} «{source}» не из файла — поддерживаются только file:"
            ));
            return Ok(());
        };
        let Some(path) = self.host_path(service, &file)? else {
            return Ok(());
        };
        let target = match (secret, target) {
            (_, Some(t)) if t.starts_with('/') => t,
            (true, Some(t)) => format!("/run/secrets/{t}"),
            (true, None) => format!("/run/secrets/{source}"),
            (false, Some(t)) => format!("/{t}"),
            (false, None) => format!("/{source}"),
        };
        mounts.push(MountSpec {
            volume: false,
            source: path,
            target,
            read_only: true,
        });
        Ok(())
    }

    fn ports(&mut self, service: &str, value: &Value) -> AppResult<Vec<PortSpec>> {
        let items = match value {
            Value::Sequence(items) => items.as_slice(),
            _ => return Err(err(format!("{service}: ports должен быть списком"))),
        };
        let mut out = Vec::new();
        for item in items {
            let (ip, host, container, proto) = match item {
                Value::Number(n) => (None, String::new(), n.to_string(), "tcp".to_owned()),
                Value::String(s) => {
                    short_port(s).ok_or_else(|| err(format!("{service}: не понял порт «{s}»")))?
                }
                Value::Mapping(m) => (
                    text(m, "host_ip"),
                    text(m, "published").unwrap_or_default(),
                    text(m, "target")
                        .ok_or_else(|| err(format!("{service}: у порта нет target")))?,
                    text(m, "protocol").unwrap_or_else(|| "tcp".into()),
                ),
                _ => return Err(err(format!("{service}: непонятная запись в ports"))),
            };
            let udp = match proto.as_str() {
                "tcp" => false,
                "udp" => true,
                other => {
                    self.warn(format!(
                        "{service}: протокол {other} не поддерживается — порт пропущен"
                    ));
                    continue;
                }
            };
            if ip.is_none() {
                self.open_ports = true;
            }
            let containers = port_range(&container)
                .ok_or_else(|| err(format!("{service}: неверный порт {container}")))?;
            let hosts = if host.is_empty() {
                vec![0; containers.len()]
            } else {
                port_range(&host).ok_or_else(|| err(format!("{service}: неверный порт {host}")))?
            };
            if hosts.len() != containers.len() {
                return Err(err(format!(
                    "{service}: диапазоны {host} и {container} разной длины"
                )));
            }
            for (h, c) in hosts.into_iter().zip(containers) {
                out.push(PortSpec {
                    // Docker publishes on every interface unless told otherwise.
                    host_ip: Some(ip.clone().unwrap_or_else(|| "0.0.0.0".into())),
                    host_port: h,
                    container_port: c,
                    udp,
                });
            }
            if out.len() > MAX_PORTS {
                return Err(err(format!("{service}: слишком много портов")));
            }
        }
        Ok(out)
    }

    fn healthcheck(&mut self, service: &str, m: &Mapping, extras: &mut RunExtras) {
        if flag(m, "disable") {
            extras.no_healthcheck = true;
            return;
        }
        let cmd = match get(m, "test") {
            Some(Value::Sequence(items)) => {
                let parts: Vec<String> = items.iter().filter_map(scalar).collect();
                match parts.split_first() {
                    Some((first, _)) if first == "NONE" => {
                        extras.no_healthcheck = true;
                        return;
                    }
                    Some((first, rest)) if first == "CMD" => rest
                        .iter()
                        .map(|a| sh_quote(a))
                        .collect::<Vec<_>>()
                        .join(" "),
                    Some((first, rest)) if first == "CMD-SHELL" => rest.join(" "),
                    _ => {
                        self.warn(format!(
                            "{service}: healthcheck.test не распознан — пропущен"
                        ));
                        return;
                    }
                }
            }
            Some(other) => match scalar(other) {
                Some(s) => s,
                None => return,
            },
            // Only timings: the image's own check keeps running.
            None => return,
        };
        let duration = |key: &str| {
            let v = text(m, key)?;
            if validate_duration(&v).is_ok() {
                Some(v)
            } else {
                None
            }
        };
        let interval = duration("interval");
        let timeout = duration("timeout");
        let start_period = duration("start_period");
        if get(m, "start_interval").is_some() {
            self.warn(format!(
                "{service}: healthcheck.start_interval не поддерживается"
            ));
        }
        extras.health = Some(HealthSpec {
            cmd,
            interval,
            timeout,
            retries: get(m, "retries").and_then(Value::as_u64).map(|n| n as u32),
            start_period,
        });
    }

    fn build(&mut self, service: &str, value: &Value) -> AppResult<BuildSpec> {
        let (context, m) = match value {
            Value::Mapping(m) => (text(m, "context").unwrap_or_else(|| ".".into()), Some(m)),
            other => (
                scalar(other).ok_or_else(|| err(format!("{service}: непонятный build")))?,
                None,
            ),
        };
        if context.contains("://") || context.starts_with("git@") {
            return Err(err(format!(
                "{service}: сборка из удалённого репозитория не поддерживается"
            )));
        }
        let context = self
            .host_path(service, &context)?
            .ok_or_else(|| err(format!("{service}: контекст сборки недоступен")))?;
        let mut spec = BuildSpec {
            context: context.clone(),
            dockerfile: None,
            tags: vec![],
            build_args: vec![],
            target: None,
            no_cache: false,
            pull: false,
        };
        if let Some(m) = m {
            if let Some(file) = text(m, "dockerfile") {
                let path = if is_drive_path(&file) {
                    PathBuf::from(file.replace('/', "\\"))
                } else {
                    Path::new(&context).join(file.replace('/', "\\"))
                };
                spec.dockerfile = Some(lexical(&path).to_string_lossy().into_owned());
            }
            if let Some(args) = get(m, "args").and_then(pairs) {
                for (key, value) in args {
                    if let Some(value) = value.or_else(|| self.env.get(&key).cloned()) {
                        spec.build_args.push(EnvVar { key, value });
                    }
                }
            }
            spec.target = text(m, "target");
            for (k, _) in m.iter() {
                let key = k.as_str().unwrap_or_default();
                if !matches!(key, "context" | "dockerfile" | "args" | "target" | "labels")
                    && !key.starts_with("x-")
                {
                    self.warn(format!(
                        "{service}: build.{key} не поддерживается — пропущено"
                    ));
                }
            }
        }
        Ok(spec)
    }

    fn service(&mut self, name: &str, def: &Mapping, all: &BTreeSet<String>) -> AppResult<Draft> {
        for (k, _) in def.iter() {
            let key = k.as_str().unwrap_or_default();
            if key.starts_with("x-") || HANDLED.contains(&key) {
                continue;
            }
            if UNSUPPORTED.contains(&key) {
                self.warn(format!(
                    "{name}: {key} — WSLc этого не поддерживает, параметр пропущен"
                ));
            } else {
                self.warn(format!("{name}: неизвестный ключ {key} — пропущен"));
            }
        }
        if let Some(policy) = text(def, "restart").filter(|p| p != "no" && p != "false") {
            self.warn(format!(
                "{name}: restart: {policy} — у WSLc нет политик перезапуска, контейнер сам не перезапустится"
            ));
        }
        if get(def, "extra_hosts").is_some() {
            self.warn(format!(
                "{name}: extra_hosts — WSLc не умеет дописывать /etc/hosts, пропущено"
            ));
        }
        if get(def, "extends").is_some() {
            self.warn(format!(
                "{name}: extends не поддерживается — используются только ключи самого сервиса"
            ));
        }
        if flag(def, "init") {
            self.warn(format!(
                "{name}: init — WSLc этого не поддерживает, параметр пропущен"
            ));
        }
        if let Some(platform) =
            text(def, "platform").filter(|p| !matches!(p.as_str(), "linux/amd64" | "linux/x86_64"))
        {
            self.warn(format!(
                "{name}: platform {platform} — WSLc запускает образы linux/amd64"
            ));
        }

        let build = get(def, "build").map(|b| self.build(name, b)).transpose()?;
        let image = match (text(def, "image"), &build) {
            (Some(image), _) => image,
            (None, Some(_)) => format!("{}-{name}", self.project),
            (None, None) => return Err(err(format!("{name}: нет ни image, ни build"))),
        };
        validate_image(&image).map_err(|_| err(format!("{name}: некорректный образ «{image}»")))?;
        let build = build.map(|mut b| {
            b.tags = vec![image.clone()];
            b
        });
        let pull_policy = match text(def, "pull_policy").as_deref() {
            None | Some("missing" | "if_not_present") => "missing",
            Some("always") => "always",
            Some("never") => "never",
            Some("build") => "build",
            Some(other) => {
                self.warn(format!(
                    "{name}: pull_policy {other} — используется missing"
                ));
                "missing"
            }
        }
        .to_owned();

        let mut command = match get(def, "command") {
            None => vec![],
            Some(Value::Sequence(items)) => items.iter().filter_map(scalar).collect(),
            Some(other) => shell_split(&scalar(other).unwrap_or_default())
                .map_err(|e| err(format!("{name}: {e}")))?,
        };
        let entry = match get(def, "entrypoint") {
            None => vec![],
            Some(Value::Sequence(items)) => items.iter().filter_map(scalar).collect(),
            Some(other) => shell_split(&scalar(other).unwrap_or_default())
                .map_err(|e| err(format!("{name}: {e}")))?,
        };
        let entrypoint = match entry.split_first() {
            Some((first, rest)) => {
                command = rest.iter().cloned().chain(command).collect();
                Some(first.clone())
            }
            None => {
                if get(def, "entrypoint").is_some() {
                    self.warn(format!(
                        "{name}: пустой entrypoint не поддерживается — оставлен entrypoint образа"
                    ));
                }
                None
            }
        };

        let mut env: Vec<EnvVar> = Vec::new();
        let set = |key: String, value: String, env: &mut Vec<EnvVar>| match env
            .iter_mut()
            .find(|e| e.key == key)
        {
            Some(e) => e.value = value,
            None => env.push(EnvVar { key, value }),
        };
        if let Some(files) = get(def, "env_file") {
            let entries: Vec<(String, bool)> = match files {
                Value::Sequence(items) => items
                    .iter()
                    .filter_map(|i| match i {
                        Value::Mapping(m) => Some((
                            text(m, "path")?,
                            get(m, "required").and_then(Value::as_bool).unwrap_or(true),
                        )),
                        other => scalar(other).map(|s| (s, true)),
                    })
                    .collect(),
                other => scalar(other).map(|s| vec![(s, true)]).unwrap_or_default(),
            };
            for (file, required) in entries {
                let Some(path) = self.host_path(name, &file)? else {
                    continue;
                };
                match std::fs::read_to_string(&path) {
                    Ok(body) => {
                        for (k, v) in dotenv(&body) {
                            set(k, v, &mut env);
                        }
                    }
                    Err(_) if !required => {}
                    Err(_) => return Err(err(format!("{name}: не найден env_file {path}"))),
                }
            }
        }
        if let Some(list) = get(def, "environment") {
            let list = pairs(list).ok_or_else(|| err(format!("{name}: непонятный environment")))?;
            for (key, value) in list {
                if let Some(value) = value.or_else(|| self.env.get(&key).cloned()) {
                    set(key, value, &mut env);
                }
            }
        }

        let ports = match get(def, "ports") {
            Some(v) => self.ports(name, v)?,
            None => vec![],
        };

        let mut extras = RunExtras::default();
        let mut mounts = Vec::new();
        if let Some(Value::Sequence(items)) = get(def, "volumes") {
            for item in items {
                self.mount(name, item, &mut mounts, &mut extras.tmpfs)?;
            }
        }
        for (key, secret) in [("secrets", true), ("configs", false)] {
            if let Some(Value::Sequence(items)) = get(def, key) {
                for item in items {
                    self.file_mount(name, secret, item, &mut mounts)?;
                }
            }
        }

        // Networks: the first one at create time, the rest connected before start.
        let mode = text(def, "network_mode");
        let mut attaches: Vec<(i64, ComposeAttach)> = Vec::new();
        let mut network = None;
        match mode.as_deref() {
            Some(m @ ("host" | "none" | "bridge")) => network = Some(m.to_owned()),
            Some(other) if other != "default" => {
                self.warn(format!(
                    "{name}: network_mode {other} не поддерживается — используется сеть проекта"
                ));
            }
            _ => {}
        }
        if network.is_none() {
            let entries: Vec<(String, Option<&Mapping>)> = match get(def, "networks") {
                None => vec![("default".into(), None)],
                Some(Value::Sequence(items)) => {
                    items.iter().filter_map(scalar).map(|n| (n, None)).collect()
                }
                Some(Value::Mapping(m)) => m
                    .iter()
                    .filter_map(|(k, v)| Some((scalar(k)?, v.as_mapping())))
                    .collect(),
                Some(_) => return Err(err(format!("{name}: непонятный networks"))),
            };
            for (key, opts) in entries {
                if key != "default" && self.networks.and_then(|n| n.get(key.as_str())).is_none() {
                    return Err(err(format!(
                        "{name}: сеть «{key}» не объявлена в разделе networks"
                    )));
                }
                if !self.used_networks.contains(&key) {
                    self.used_networks.push(key.clone());
                }
                let actual = self.network_name(&key);
                let mut aliases = Vec::new();
                if !crate::resources::BUILTIN_NETWORKS.contains(&actual.as_str()) {
                    // Every service is reachable by its name on its networks.
                    aliases.push(name.to_owned());
                    for a in opts
                        .and_then(|o| get(o, "aliases"))
                        .and_then(strings)
                        .unwrap_or_default()
                    {
                        if !aliases.contains(&a) {
                            aliases.push(a);
                        }
                    }
                }
                let ip = opts.and_then(|o| text(o, "ipv4_address"));
                if opts.is_some_and(|o| get(o, "ipv6_address").is_some()) {
                    self.warn(format!("{name}: ipv6_address не поддерживается"));
                }
                let priority = opts
                    .and_then(|o| get(o, "priority"))
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                attaches.push((
                    priority,
                    ComposeAttach {
                        network: actual,
                        aliases,
                        ip,
                    },
                ));
            }
            attaches.sort_by_key(|(p, _)| -p);
        }
        let mut attaches = attaches.into_iter().map(|(_, a)| a);
        let first_attach = attaches.next();
        let extra_networks: Vec<ComposeAttach> = attaches.collect();
        if let Some(first) = &first_attach {
            network = Some(first.network.clone());
            extras.network_aliases = first.aliases.clone();
            extras.ip = first.ip.clone();
        }

        let mut depends_on = Vec::new();
        match get(def, "depends_on") {
            Some(Value::Sequence(items)) => {
                for s in items.iter().filter_map(scalar) {
                    depends_on.push(ComposeDependency {
                        service: s,
                        condition: "started".into(),
                    });
                }
            }
            Some(Value::Mapping(m)) => {
                for (k, v) in m.iter() {
                    let Some(service) = scalar(k) else { continue };
                    let condition =
                        match v.as_mapping().and_then(|o| text(o, "condition")).as_deref() {
                            Some("service_healthy") => "healthy",
                            Some("service_completed_successfully") => "completed",
                            _ => "started",
                        };
                    depends_on.push(ComposeDependency {
                        service,
                        condition: condition.into(),
                    });
                }
            }
            _ => {}
        }
        if let Some(links) = get(def, "links").and_then(strings) {
            for link in links {
                let service = link.split(':').next().unwrap_or_default().to_owned();
                if !depends_on.iter().any(|d| d.service == service) {
                    depends_on.push(ComposeDependency {
                        service,
                        condition: "started".into(),
                    });
                }
            }
            self.warn(format!(
                "{name}: links — порядок запуска учтён, а псевдонимы не нужны: сервисы видят друг друга по имени"
            ));
        }
        for d in &depends_on {
            if !all.contains(&d.service) {
                return Err(err(format!(
                    "{name}: зависит от неизвестного сервиса «{}»",
                    d.service
                )));
            }
        }

        // Plain options.
        extras.hostname = text(def, "hostname");
        extras.domainname = text(def, "domainname");
        extras.stop_signal = text(def, "stop_signal");
        extras.stop_timeout = text(def, "stop_grace_period").and_then(|d| duration_secs(&d));
        extras.shm_size = get(def, "shm_size").and_then(size);
        extras.tty = flag(def, "tty");
        extras.interactive = flag(def, "stdin_open");
        extras.dns = get(def, "dns").and_then(strings).unwrap_or_default();
        extras.dns_search = get(def, "dns_search").and_then(strings).unwrap_or_default();
        extras.dns_options = get(def, "dns_opt").and_then(strings).unwrap_or_default();
        if let Some(t) = get(def, "tmpfs").and_then(strings) {
            extras.tmpfs.extend(t);
        }
        if let Some(Value::Mapping(limits)) = get(def, "ulimits") {
            for (k, v) in limits.iter() {
                let Some(key) = scalar(k) else { continue };
                let value = match v {
                    Value::Mapping(m) => match (text(m, "soft"), text(m, "hard")) {
                        (Some(s), Some(h)) => format!("{s}:{h}"),
                        (Some(s), None) | (None, Some(s)) => s,
                        _ => continue,
                    },
                    other => match scalar(other) {
                        Some(s) => s,
                        None => continue,
                    },
                };
                extras.ulimits.push(format!("{key}={value}"));
            }
        }
        if let Some(Value::Mapping(h)) = get(def, "healthcheck") {
            self.healthcheck(name, h, &mut extras);
        }
        if let Some(labels) = get(def, "labels").and_then(pairs) {
            extras.labels = labels
                .into_iter()
                .map(|(key, value)| EnvVar {
                    key,
                    value: value.unwrap_or_default(),
                })
                .collect();
        }

        let deploy = get(def, "deploy").and_then(Value::as_mapping);
        let limits = deploy
            .and_then(|d| get(d, "resources"))
            .and_then(Value::as_mapping)
            .and_then(|r| get(r, "limits"))
            .and_then(Value::as_mapping);
        let cpus = text(def, "cpus").or_else(|| limits.and_then(|l| text(l, "cpus")));
        let memory = get(def, "mem_limit")
            .or_else(|| limits.and_then(|l| get(l, "memory")))
            .and_then(size);
        let gpu_devices = deploy
            .and_then(|d| get(d, "resources"))
            .and_then(Value::as_mapping)
            .and_then(|r| get(r, "reservations"))
            .and_then(Value::as_mapping)
            .and_then(|r| get(r, "devices"))
            .and_then(Value::as_sequence)
            .and_then(|devices| {
                devices.iter().filter_map(Value::as_mapping).find(|d| {
                    get(d, "capabilities")
                        .and_then(strings)
                        .is_some_and(|c| c.iter().any(|x| x == "gpu"))
                })
            });
        extras.gpus = match (get(def, "gpus"), gpu_devices) {
            (Some(_), _) => Some("all".into()),
            (None, Some(d)) => Some(
                text(d, "count")
                    .filter(|c| c != "all")
                    .unwrap_or_else(|| "all".into()),
            ),
            _ => None,
        };
        let replicas = deploy
            .and_then(|d| get(d, "replicas"))
            .or_else(|| get(def, "scale"))
            .and_then(Value::as_u64);
        if replicas.is_some_and(|r| r > 1) {
            self.warn(format!(
                "{name}: несколько реплик не поддерживается — создаётся один контейнер"
            ));
        }
        if deploy.is_some_and(|d| get(d, "restart_policy").is_some()) {
            self.warn(format!(
                "{name}: deploy.restart_policy — у WSLc нет политик перезапуска"
            ));
        }

        let spec = RunSpec {
            image: image.clone(),
            name: None,
            ports,
            mounts,
            env,
            command,
            entrypoint,
            workdir: text(def, "working_dir"),
            user: text(def, "user"),
            network,
            cpus,
            memory,
            auto_remove: false,
            start: false,
            extras: Some(extras),
        };
        Ok(Draft {
            service: name.to_owned(),
            image,
            build,
            pull_policy,
            spec,
            extra_networks,
            depends_on,
        })
    }
}

fn port_range(s: &str) -> Option<Vec<u16>> {
    match s.split_once('-') {
        Some((a, b)) => {
            let (a, b): (u16, u16) = (a.parse().ok()?, b.parse().ok()?);
            (a <= b && a > 0 && usize::from(b - a) < MAX_PORTS).then(|| (a..=b).collect())
        }
        None => s.parse().ok().filter(|p| *p > 0).map(|p| vec![p]),
    }
}

/// `80`, `8080:80`, `127.0.0.1:8080:80`, `127.0.0.1::80`, `[::1]:8080:80`, `8080:80/udp`.
fn short_port(s: &str) -> Option<(Option<String>, String, String, String)> {
    let (rest, proto) = match s.rsplit_once('/') {
        Some((r, p)) => (r, p.to_owned()),
        None => (s, "tcp".to_owned()),
    };
    let (ip, rest) = if let Some(after) = rest.strip_prefix('[') {
        let (ip, tail) = after.split_once("]:")?;
        (Some(ip.to_owned()), tail)
    } else {
        (None, rest)
    };
    let parts: Vec<&str> = rest.split(':').collect();
    let (ip, host, container) = match (ip, parts.as_slice()) {
        (None, [c]) => (None, "", *c),
        (None, [h, c]) => (None, *h, *c),
        (None, [ip, h, c]) => (Some((*ip).to_owned()), *h, *c),
        (Some(ip), [h, c]) => (Some(ip), *h, *c),
        _ => return None,
    };
    Some((ip, host.to_owned(), container.to_owned(), proto))
}

/// Dependencies first; among independent services, file order.
fn order(drafts: Vec<Draft>) -> AppResult<Vec<Draft>> {
    let mut pending = drafts;
    let mut done: Vec<Draft> = Vec::new();
    while !pending.is_empty() {
        let ready = pending.iter().position(|d| {
            d.depends_on
                .iter()
                .all(|dep| done.iter().any(|x| x.service == dep.service))
        });
        match ready {
            Some(i) => done.push(pending.remove(i)),
            None => {
                let names: Vec<_> = pending.iter().map(|d| d.service.as_str()).collect();
                return Err(err(format!(
                    "Циклическая зависимость между сервисами: {}",
                    names.join(", ")
                )));
            }
        }
    }
    Ok(done)
}

fn compose_labels(project: &str, extra: &[(&str, &str)]) -> Vec<EnvVar> {
    std::iter::once((PROJECT_LABEL, project))
        .chain(extra.iter().copied())
        .map(|(k, v)| EnvVar {
            key: k.into(),
            value: v.into(),
        })
        .collect()
}

pub fn plan(
    yaml: &str,
    project_dir: Option<&Path>,
    project_name: Option<&str>,
    env: &HashMap<String, String>,
) -> AppResult<ComposePlan> {
    let mut root: Value =
        serde_yaml::from_str(yaml).map_err(|e| err(format!("Ошибка YAML: {e}")))?;
    root.apply_merge()
        .map_err(|e| err(format!("Ошибка YAML: {e}")))?;
    let mut missing = BTreeSet::new();
    interpolate_tree(&mut root, env, &mut missing).map_err(err)?;
    let root = root
        .as_mapping()
        .ok_or_else(|| err("Ожидается файл Compose с разделом services"))?;

    let mut warnings = Vec::new();
    for name in &missing {
        warnings.push(format!(
            "Переменная {name} не задана — подставлена пустая строка"
        ));
    }
    for (k, _) in root.iter() {
        let key = k.as_str().unwrap_or_default();
        match key {
            "version" | "name" | "services" | "networks" | "volumes" | "secrets" | "configs" => {}
            k if k.starts_with("x-") => {}
            other => warnings.push(format!(
                "{other} верхнего уровня не поддерживается — пропущено"
            )),
        }
    }

    let raw_name = project_name
        .filter(|n| !n.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| text(root, "name"))
        .or_else(|| {
            project_dir
                .and_then(|d| d.file_name())
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "compose".into());
    let project = normalize_project(&raw_name);
    if project.is_empty() {
        return Err(err(format!("Не получилось имя проекта из «{raw_name}»")));
    }

    let services = get(root, "services")
        .and_then(Value::as_mapping)
        .ok_or_else(|| err("В файле нет раздела services"))?;
    let section = |key: &str| get(root, key).and_then(Value::as_mapping);
    let mut planner = Planner {
        project: project.clone(),
        dir: project_dir,
        env,
        networks: section("networks"),
        volumes: section("volumes"),
        secrets: section("secrets"),
        configs: section("configs"),
        warnings,
        host_folders: BTreeSet::new(),
        used_networks: vec![],
        used_volumes: vec![],
        anonymous: vec![],
        open_ports: false,
    };

    let mut active = Vec::new();
    let empty = Mapping::new();
    for (k, v) in services.iter() {
        let name = scalar(k).ok_or_else(|| err("Имя сервиса должно быть строкой"))?;
        let def = match v {
            Value::Mapping(m) => m,
            Value::Null => &empty,
            _ => {
                return Err(err(format!(
                    "{name}: описание сервиса должно быть словарём"
                )));
            }
        };
        if let Some(profiles) = get(def, "profiles")
            .and_then(strings)
            .filter(|p| !p.is_empty())
        {
            planner.warn(format!(
                "{name}: сервис из профиля {} — пропущен, как без --profile",
                profiles.join(", ")
            ));
            continue;
        }
        active.push((name, def));
    }
    if active.is_empty() {
        return Err(err("Нет сервисов для запуска"));
    }
    let names: BTreeSet<String> = active.iter().map(|(n, _)| n.clone()).collect();
    let mut drafts = Vec::new();
    for (name, def) in &active {
        drafts.push(planner.service(name, def, &names)?);
    }
    let drafts = order(drafts)?;
    if planner.open_ports {
        planner.warn(
            "Порты без IP опубликованы на всех интерфейсах (0.0.0.0), как в Docker; \
             для доступа только с этого компьютера укажите 127.0.0.1:порт:порт"
                .into(),
        );
    }

    // Networks and volumes the services use.
    let mut commands = Vec::new();
    let mut networks = Vec::new();
    for key in planner.used_networks.clone() {
        let decl = planner
            .networks
            .and_then(|n| get(n, &key))
            .and_then(Value::as_mapping);
        let name = planner.network_name(&key);
        let external = decl
            .is_some_and(|d| get(d, "external").is_some_and(|e| e.as_bool() != Some(false)))
            || crate::resources::BUILTIN_NETWORKS.contains(&name.as_str());
        let ipam = decl
            .and_then(|d| get(d, "ipam"))
            .and_then(Value::as_mapping)
            .and_then(|i| get(i, "config"))
            .and_then(Value::as_sequence)
            .and_then(|c| c.first())
            .and_then(Value::as_mapping);
        if let Some(driver) = decl
            .and_then(|d| text(d, "driver"))
            .filter(|d| d != "bridge")
        {
            planner.warn(format!(
                "Сеть {key}: драйвер {driver} не поддерживается — используется bridge"
            ));
        }
        if decl.is_some_and(|d| flag(d, "enable_ipv6")) {
            planner.warn(format!("Сеть {key}: enable_ipv6 не поддерживается"));
        }
        let mut labels = compose_labels(&project, &[("com.docker.compose.network", key.as_str())]);
        if let Some(extra) = decl.and_then(|d| get(d, "labels")).and_then(pairs) {
            labels.extend(extra.into_iter().map(|(k, v)| EnvVar {
                key: k,
                value: v.unwrap_or_default(),
            }));
        }
        let spec = NetworkSpec {
            name,
            driver: None,
            subnet: ipam.and_then(|i| text(i, "subnet")),
            gateway: ipam.and_then(|i| text(i, "gateway")),
            ip_range: ipam.and_then(|i| text(i, "ip_range")),
            internal: decl.is_some_and(|d| flag(d, "internal")),
            labels,
        };
        if !external {
            commands.push(command_line(&network_create_args(&spec)?));
        }
        networks.push(ComposeNetwork {
            key,
            is_external: external,
            spec,
        });
    }
    let mut volumes = Vec::new();
    let anonymous = planner.anonymous.clone();
    let declared = planner.used_volumes.clone().into_iter().map(|k| (k, false));
    for (key, anon) in declared.chain(anonymous.into_iter().map(|(n, _)| (n, true))) {
        let decl = if anon {
            None
        } else {
            planner
                .volumes
                .and_then(|v| get(v, &key))
                .and_then(Value::as_mapping)
        };
        let name = if anon {
            key.clone()
        } else {
            planner.volume_name(&key)
        };
        let external =
            decl.is_some_and(|d| get(d, "external").is_some_and(|e| e.as_bool() != Some(false)));
        let driver = match decl.and_then(|d| text(d, "driver")) {
            None => None,
            Some(d) if d == "local" => None,
            Some(d) if d == "guest" || d == "vhd" => Some(d),
            Some(d) => {
                planner.warn(format!(
                    "Том {key}: драйвер {d} не поддерживается — используется guest"
                ));
                None
            }
        };
        let mut options = Vec::new();
        if let Some(opts) = decl.and_then(|d| get(d, "driver_opts")).and_then(pairs) {
            if driver.is_some() {
                options = opts
                    .into_iter()
                    .map(|(k, v)| EnvVar {
                        key: k,
                        value: v.unwrap_or_default(),
                    })
                    .collect();
            } else {
                planner.warn(format!(
                    "Том {key}: driver_opts для local не поддерживаются — пропущены"
                ));
            }
        }
        let mut labels = compose_labels(&project, &[("com.docker.compose.volume", key.as_str())]);
        if let Some(extra) = decl.and_then(|d| get(d, "labels")).and_then(pairs) {
            labels.extend(extra.into_iter().map(|(k, v)| EnvVar {
                key: k,
                value: v.unwrap_or_default(),
            }));
        }
        let spec = VolumeSpec {
            name,
            driver,
            options,
            labels,
        };
        if !external {
            commands.push(command_line(&volume_create_args(&spec)?));
        }
        volumes.push(ComposeVolume {
            key,
            is_external: external,
            spec,
        });
    }

    // Services: labels, hash, commands.
    let working_dir = project_dir.map(|d| d.to_string_lossy().into_owned());
    let mut out = Vec::new();
    for d in drafts {
        let container_name = format!("{project}-{}-1", d.service);
        let mut spec = d.spec;
        validate_container_name(&container_name).map_err(|_| {
            err(format!(
                "{}: некорректное имя контейнера {container_name}",
                d.service
            ))
        })?;
        spec.name = Some(container_name.clone());
        let mut extras = spec.extras.take().unwrap_or_default();
        let mut labels = compose_labels(
            &project,
            &[
                (SERVICE_LABEL, d.service.as_str()),
                ("com.docker.compose.container-number", "1"),
                ("com.docker.compose.oneoff", "False"),
            ],
        );
        if let Some(dir) = &working_dir {
            labels.push(EnvVar {
                key: "com.docker.compose.project.working_dir".into(),
                value: dir.clone(),
            });
        }
        labels.append(&mut extras.labels);
        extras.labels = labels;
        spec.extras = Some(extras);
        let hash = fnv1a(
            &serde_json::to_string(&(&spec, &d.extra_networks, &d.build)).unwrap_or_default(),
        );
        if let Some(x) = spec.extras.as_mut() {
            x.labels.push(EnvVar {
                key: HASH_LABEL.into(),
                value: hash.clone(),
            });
        }

        let mut lines = Vec::new();
        if let Some(b) = &d.build {
            let mut args = vec!["build".to_owned(), "--tag".into(), d.image.clone()];
            if let Some(f) = &b.dockerfile {
                args.extend(["--file".into(), f.clone()]);
            }
            for a in &b.build_args {
                args.extend(["--build-arg".into(), format!("{}=…", a.key)]);
            }
            if let Some(t) = &b.target {
                args.extend(["--target".into(), t.clone()]);
            }
            args.push(b.context.clone());
            lines.push(command_line(&args));
        } else if d.pull_policy == "always" {
            lines.push(command_line(&["pull".into(), d.image.clone()]));
        }
        // Env values go through a temporary file at run time; the preview never shows them.
        let env_file = (!spec.env.is_empty()).then_some("<файл-окружения>");
        let create = run_arguments(&spec, env_file, false)
            .map_err(|e| err(format!("{}: {}", d.service, e.message)))?;
        lines.push(command_line(&create));
        for a in &d.extra_networks {
            let mut args = vec!["network".to_owned(), "connect".into()];
            for alias in &a.aliases {
                args.extend(["--network-alias".into(), alias.clone()]);
            }
            if let Some(ip) = &a.ip {
                args.extend(["--ip".into(), ip.clone()]);
            }
            args.extend([a.network.clone(), container_name.clone()]);
            lines.push(command_line(&args));
        }
        lines.push(format!("wslc start {}", quote(&container_name)));
        out.push(ComposeService {
            service: d.service,
            container_name,
            image: d.image,
            build: d.build,
            pull_policy: d.pull_policy,
            spec,
            extra_networks: d.extra_networks,
            depends_on: d.depends_on,
            config_hash: hash,
            commands: lines,
        });
    }

    Ok(ComposePlan {
        project,
        working_dir,
        networks,
        volumes,
        services: out,
        host_folders: planner.host_folders.into_iter().collect(),
        commands,
        warnings: planner.warnings,
    })
}

/// Containers of `project` among batch-inspect documents, read from their labels.
pub fn containers(documents: &[serde_json::Value], project: &str) -> Vec<ComposeContainer> {
    documents
        .iter()
        .filter_map(|c| {
            let labels = &c["Config"]["Labels"];
            if labels[PROJECT_LABEL].as_str() != Some(project) {
                return None;
            }
            let label = |key: &str| labels[key].as_str().map(str::to_owned);
            Some(ComposeContainer {
                id: c["Id"].as_str()?.to_owned(),
                name: c["Name"]
                    .as_str()
                    .unwrap_or_default()
                    .trim_start_matches('/')
                    .to_owned(),
                service: label(SERVICE_LABEL),
                state: ContainerState::from_raw(c["State"]["Status"].as_str()),
                config_hash: label(HASH_LABEL),
                health: c["State"]["Health"]["Status"].as_str().map(str::to_owned),
                exit_code: c["State"]["ExitCode"].as_i64(),
            })
        })
        .collect()
}
