//! Convert a terminal command into a parameterized snippet template
//! (CLIENT_SPEC §13.2 "Convert terminal command into parameterized snippet").
//!
//! Variable parts (namespaces, pod/container names, hosts, IPs, users,
//! ports, databases, files, branches, releases, …) become `{{var}}`
//! placeholders whose default is the original value. **Secrets never survive**:
//! passwords/tokens/keys are replaced by variables without defaults.

use crate::sanitizer::{identity_spans, opt_takes_value, secret_spans, Category};
use crate::shell::{self, command_name, skip_wrappers, ShellDialect, Word};
use cc_models::snippet::{template_variables, SnippetVariable};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ops::Range;

/// Result of [`parameterize`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParameterizedCommand {
    pub template: String,
    /// In order of first appearance in `template`.
    pub variables: Vec<SnippetVariable>,
    /// How many secret values were replaced (their values are not kept).
    pub secrets_removed: usize,
}

#[derive(Debug, Clone)]
enum Part {
    Lit(String),
    /// `value == None` → secret (no default).
    Var {
        base: &'static str,
        value: Option<String>,
    },
}

#[derive(Debug, Clone)]
struct Rep {
    span: Range<usize>,
    parts: Vec<Part>,
    priority: u8,
}

fn var(base: &'static str, value: &str) -> Part {
    Part::Var {
        base,
        value: Some(value.to_owned()),
    }
}

/// Parameterize `command` for the given shell dialect.
pub fn parameterize(command: &str, dialect: ShellDialect) -> ParameterizedCommand {
    let mut reps: Vec<Rep> = Vec::new();

    // 1. Secrets (highest priority, no defaults).
    for (span, cat) in secret_spans(command) {
        let base = match cat {
            Category::Password => "password",
            Category::PrivateKey => "private_key",
            Category::Pem => "certificate",
            _ => "token",
        };
        reps.push(Rep {
            span,
            parts: vec![Part::Var { base, value: None }],
            priority: 3,
        });
    }

    // 2. Command-aware words.
    for cmd in shell::parse(command, dialect) {
        let values: Vec<&str> = cmd.words.iter().map(|w| w.value.as_str()).collect();
        let idx = skip_wrappers(&values);
        if idx >= cmd.words.len() {
            continue;
        }
        let tool = command_name(&cmd.words[idx].value);
        word_rules(&tool, &cmd.words[idx + 1..], &mut reps);
    }

    // 3. Remaining hosts / IPs / users anywhere (URLs, config strings).
    for (span, cat) in identity_spans(command) {
        let base = match cat {
            Category::User => "user",
            _ => "host",
        };
        let value = command[span.clone()].to_owned();
        reps.push(Rep {
            span,
            parts: vec![Part::Var {
                base,
                value: Some(value),
            }],
            priority: 1,
        });
    }

    build(command, reps)
}

/// Replace secret values and sanitizer secret placeholders (`<PASSWORD_1>`)
/// in a template with variables that have no default.
pub(crate) fn scrub_secrets(template: &str) -> ParameterizedCommand {
    static SECRET_PH: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"<(PASSWORD|SECRET|PRIVATE_KEY|PEM)_\d+>").expect("valid regex")
    });
    let mut reps: Vec<Rep> = Vec::new();
    for (span, cat) in secret_spans(template) {
        let base = match cat {
            Category::Password => "password",
            Category::PrivateKey => "private_key",
            Category::Pem => "certificate",
            _ => "token",
        };
        reps.push(Rep {
            span,
            parts: vec![Part::Var { base, value: None }],
            priority: 3,
        });
    }
    for c in SECRET_PH.captures_iter(template) {
        let (Some(m), Some(kind)) = (c.get(0), c.get(1)) else {
            continue;
        };
        let base = match kind.as_str() {
            "PASSWORD" => "password",
            "PRIVATE_KEY" => "private_key",
            "PEM" => "certificate",
            _ => "token",
        };
        reps.push(Rep {
            span: m.range(),
            parts: vec![Part::Var { base, value: None }],
            priority: 4,
        });
    }
    build(template, reps)
}

fn build(command: &str, mut reps: Vec<Rep>) -> ParameterizedCommand {
    // Never touch existing `{{var}}` placeholders.
    let existing: Vec<Range<usize>> = {
        let mut v = Vec::new();
        let mut from = 0;
        while let Some(s) = command[from..].find("{{") {
            let s = from + s;
            match command[s..].find("}}") {
                Some(e) => {
                    v.push(s..s + e + 2);
                    from = s + e + 2;
                }
                None => break,
            }
        }
        v
    };
    reps.retain(|r| {
        !existing
            .iter()
            .any(|e| r.span.start < e.end && e.start < r.span.end)
    });
    // Resolve overlaps: priority, then longer span.
    reps.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then((b.span.end - b.span.start).cmp(&(a.span.end - a.span.start)))
            .then(a.span.start.cmp(&b.span.start))
    });
    let mut accepted: Vec<Rep> = Vec::new();
    for r in reps {
        if r.span.end > r.span.start
            && accepted
                .iter()
                .all(|a| r.span.end <= a.span.start || r.span.start >= a.span.end)
        {
            accepted.push(r);
        }
    }
    accepted.sort_by_key(|r| r.span.start);

    let mut names: HashMap<(&'static str, String), String> = HashMap::new();
    let mut used: HashMap<&'static str, usize> = HashMap::new();
    let mut meta: HashMap<String, SnippetVariable> = HashMap::new();
    let mut secrets_removed = 0;
    let mut template = String::with_capacity(command.len());
    let mut last = 0;
    for r in &accepted {
        template.push_str(&command[last..r.span.start]);
        for p in &r.parts {
            match p {
                Part::Lit(l) => template.push_str(l),
                Part::Var { base, value } => {
                    let key = match value {
                        Some(v) => v.clone(),
                        None => {
                            secrets_removed += 1;
                            format!("\u{0}secret{secrets_removed}")
                        }
                    };
                    let name = names
                        .entry((base, key))
                        .or_insert_with(|| {
                            let n = used.entry(base).or_default();
                            *n += 1;
                            if *n == 1 {
                                (*base).to_owned()
                            } else {
                                format!("{base}_{n}")
                            }
                        })
                        .clone();
                    meta.entry(name.clone()).or_insert_with(|| SnippetVariable {
                        name: name.clone(),
                        description: describe(base).to_owned(),
                        default: value.clone(),
                        required: true,
                    });
                    template.push_str("{{");
                    template.push_str(&name);
                    template.push_str("}}");
                }
            }
        }
        last = r.span.end;
    }
    template.push_str(&command[last..]);
    let variables = template_variables(&template)
        .into_iter()
        .map(|n| {
            meta.remove(&n).unwrap_or(SnippetVariable {
                name: n,
                description: String::new(),
                default: None,
                required: true,
            })
        })
        .collect();
    ParameterizedCommand {
        template,
        variables,
        secrets_removed,
    }
}

fn describe(base: &str) -> &'static str {
    match base {
        "namespace" => "Kubernetes namespace",
        "context" => "Kubernetes context",
        "container" => "Container name",
        "pod" => "Pod name",
        "deployment" => "Deployment name",
        "service" => "Service / unit name",
        "node" => "Node name",
        "name" => "Resource name",
        "selector" => "Label selector",
        "replicas" => "Replica count",
        "lines" => "Number of lines",
        "since" => "Time window (e.g. 1h)",
        "host" => "Host name or IP address",
        "jump_host" => "Jump host",
        "port" => "Port",
        "user" => "User name",
        "database" => "Database name",
        "db" => "Database number",
        "file" => "File path",
        "path" => "Path",
        "identity_file" => "SSH identity file",
        "image" => "Container image",
        "branch" => "Git branch",
        "release" => "Helm release name",
        "chart" => "Helm chart",
        "revision" => "Revision",
        "values_file" => "Values file",
        "chart_version" => "Chart version",
        "region" => "Region",
        "profile" => "Profile",
        "id" => "Identifier",
        "pid" => "Process id",
        "seconds" => "Seconds",
        "password" => "Password (never stored in the snippet)",
        "token" => "Secret token (never stored in the snippet)",
        "private_key" => "Private key (never stored in the snippet)",
        "certificate" => "Certificate",
        "port_mapping" => "Port mapping (host:container)",
        "volume" => "Volume mapping",
        _ => "",
    }
}

const PG_TOOLS: &[&str] = &[
    "psql",
    "pg_dump",
    "pg_restore",
    "createdb",
    "dropdb",
    "pg_isready",
    "vacuumdb",
];
const MY_TOOLS: &[&str] = &[
    "mysql",
    "mysqldump",
    "mariadb",
    "mysqladmin",
    "mariadb-dump",
];
const K8S: &[&str] = &["kubectl", "oc", "k"];

fn flag_base(tool: &str, flag: &str) -> Option<&'static str> {
    let k8s = K8S.contains(&tool);
    let pg = PG_TOOLS.contains(&tool);
    let my = MY_TOOLS.contains(&tool);
    let ssh = matches!(tool, "ssh" | "scp" | "sftp" | "mosh");
    Some(match flag {
        "-n" | "--namespace" if k8s || tool == "helm" => "namespace",
        "--context" | "--kube-context" if k8s || tool == "helm" => "context",
        "-c" | "--container" if k8s => "container",
        "-l" | "--selector" if k8s => "selector",
        "--replicas" if k8s => "replicas",
        "-f" | "--filename" if k8s => "file",
        "--tail" => "lines",
        "--since" => "since",
        "-n" | "--lines" if matches!(tool, "head" | "tail" | "journalctl") => "lines",
        "-u" | "--unit" if matches!(tool, "journalctl" | "systemctl") => "service",
        "-p" if ssh && tool == "ssh" => "port",
        "-P" if matches!(tool, "scp" | "sftp") => "port",
        "-i" if ssh => "identity_file",
        "-l" if tool == "ssh" => "user",
        "-J" if tool == "ssh" => "jump_host",
        "-h" if pg || my || tool == "redis-cli" => "host",
        "-p" if pg || tool == "redis-cli" => "port",
        "-P" if my => "port",
        "-U" if pg => "user",
        "-u" if my => "user",
        "-d" if pg => "database",
        "-D" if my => "database",
        "-n" if tool == "redis-cli" => "db",
        "--name" if matches!(tool, "docker" | "podman") => "name",
        "-p" | "--publish" if matches!(tool, "docker" | "podman") => "port_mapping",
        "-v" | "--volume" if matches!(tool, "docker" | "podman") => "volume",
        "--version" if tool == "helm" => "chart_version",
        "-f" | "--values" if tool == "helm" => "values_file",
        "-b" | "-c" if tool == "git" => "branch",
        "--host" | "--hostname" | "--server" => "host",
        "--port" => "port",
        "--user" | "--username" => "user",
        "--namespace" => "namespace",
        "--region" => "region",
        "--profile" => "profile",
        "--database" | "--dbname" | "--db" => "database",
        _ => return None,
    })
}

fn k8s_type_base(t: &str) -> &'static str {
    match t.to_ascii_lowercase().as_str() {
        "pod" | "pods" | "po" => "pod",
        "deployment" | "deployments" | "deploy" => "deployment",
        "service" | "services" | "svc" => "service",
        "namespace" | "namespaces" | "ns" => "namespace",
        "node" | "nodes" | "no" => "node",
        "statefulset" | "statefulsets" | "sts" => "name",
        _ => "name",
    }
}

const FILE_EXTENSIONS: &[&str] = &[
    "log", "conf", "yaml", "yml", "json", "txt", "sql", "csv", "ini", "toml", "sh", "py", "xml",
    "env", "gz", "tgz", "zip", "tar", "pem", "crt", "key", "cfg", "service",
];

fn has_file_extension(v: &str) -> bool {
    v.rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && FILE_EXTENSIONS.contains(&ext))
}

fn looks_like_path(v: &str) -> bool {
    v.starts_with('/')
        || v.starts_with("./")
        || v.starts_with("~/")
        || v.starts_with("../")
        || (v.contains('/') && !v.contains("://"))
        || has_file_extension(v)
}

/// Positional argument → base name.
fn positional_base(tool: &str, pos: &[&str], value: &str) -> Option<&'static str> {
    let i = pos.len(); // index of this positional
    let sub = pos.first().copied().unwrap_or("");
    if value.contains("://") || value == "-" || value == "." || value == "--" {
        return None;
    }
    if K8S.contains(&tool) {
        const RESOURCE_VERBS: &[&str] = &[
            "get",
            "describe",
            "delete",
            "edit",
            "label",
            "annotate",
            "scale",
            "rollout",
            "logs",
            "exec",
            "port-forward",
            "patch",
            "expose",
            "autoscale",
            "set",
            "attach",
            "top",
        ];
        // `type/name` anywhere after the verb (value_parts splits it).
        if i >= 1 && value.contains('/') && RESOURCE_VERBS.contains(&sub) {
            return Some("name");
        }
        return match (sub, i) {
            (_, 0) => None,
            ("logs" | "exec" | "attach" | "port-forward", 1) => Some("pod"),
            (
                "get" | "describe" | "delete" | "edit" | "label" | "annotate" | "scale" | "patch",
                2,
            ) => Some(k8s_type_base(pos[1])),
            ("cordon" | "uncordon" | "drain", 1) => Some("node"),
            ("rollout", 3) => Some(k8s_type_base(pos[2])),
            _ => None,
        };
    }
    match tool {
        "docker" | "podman" | "nerdctl" => match (sub, i) {
            (
                "logs" | "exec" | "stop" | "start" | "restart" | "rm" | "inspect" | "attach"
                | "kill" | "top" | "stats" | "port" | "pause" | "unpause",
                1,
            ) => Some("container"),
            ("run" | "pull" | "push", 1) => Some("image"),
            _ => None,
        },
        "systemctl" if i == 1 && sub != "daemon-reload" => Some("service"),
        "service" if i == 0 => Some("service"),
        "ssh" | "mosh" | "ping" | "ping6" | "dig" | "nslookup" | "traceroute" | "telnet"
        | "mtr" | "host"
            if i == 0 =>
        {
            Some("host")
        }
        "helm" => match (sub, i) {
            (
                "install" | "upgrade" | "template" | "uninstall" | "status" | "history"
                | "rollback" | "get" | "test",
                1,
            ) => Some("release"),
            ("install" | "upgrade" | "template", 2) => Some("chart"),
            ("rollback", 2) => Some("revision"),
            ("get", 2) => Some("release"),
            _ => None,
        },
        "git" => match (sub, i) {
            // A slash is common in branch names (`feature/x`); files usually
            // start with `.`/`/`/`~` or carry an extension.
            ("checkout" | "switch" | "merge" | "rebase", 1)
                if !value.starts_with(['.', '/', '~']) && !has_file_extension(value) =>
            {
                Some("branch")
            }
            ("push" | "pull", 2) => Some("branch"),
            ("branch", 1..) => Some("branch"),
            _ => None,
        },
        t if (PG_TOOLS.contains(&t) || MY_TOOLS.contains(&t)) && i == 0 => Some("database"),
        "tail" | "cat" | "less" | "more" | "head" | "vim" | "vi" | "nano" | "stat" | "tee"
        | "bat"
            if looks_like_path(value) =>
        {
            Some("file")
        }
        "ls" | "du" | "cd" | "find" | "tree" | "rm" | "cp" | "mv" | "chmod" | "chown" | "mkdir"
            if looks_like_path(value) =>
        {
            Some("path")
        }
        "grep" | "rg" if i >= 1 && looks_like_path(value) => Some("path"),
        "kill" if value.chars().all(|c| c.is_ascii_digit()) => Some("pid"),
        "sleep" if value.chars().all(|c| c.is_ascii_digit()) => Some("seconds"),
        _ => None,
    }
}

/// Parts for a value: `user@host` split, `type/name` split.
fn value_parts(base: &'static str, value: &str) -> Vec<Part> {
    if matches!(base, "host" | "jump_host") {
        if let Some((u, h)) = value.split_once('@') {
            if !u.is_empty() && !h.is_empty() {
                return vec![var("user", u), Part::Lit("@".into()), var(base, h)];
            }
        }
    }
    if matches!(
        base,
        "pod" | "name" | "deployment" | "service" | "node" | "namespace"
    ) {
        if let Some((t, n)) = value.split_once('/') {
            if !t.is_empty() && !n.is_empty() {
                return vec![Part::Lit(format!("{t}/")), var(k8s_type_base(t), n)];
            }
        }
    }
    vec![var(base, value)]
}

fn word_rules(tool: &str, words: &[Word], reps: &mut Vec<Rep>) {
    let mut pos: Vec<&str> = Vec::new();
    let mut i = 0;
    let mut after_ddash = false;
    while i < words.len() {
        let w = &words[i];
        let v = w.value.as_str();
        if v.contains("{{") {
            i += 1;
            continue;
        }
        if v == "--" {
            after_ddash = true;
            i += 1;
            continue;
        }
        if after_ddash {
            // `kubectl exec pod -- cmd`: the inner command stays literal.
            i += 1;
            continue;
        }
        if v.starts_with('-') && v.len() > 1 {
            if let Some((flag, val)) = v.split_once('=') {
                if let Some(base) = flag_base(tool, flag) {
                    if !val.is_empty() {
                        let mut parts = vec![Part::Lit(format!("{flag}="))];
                        parts.extend(value_parts(base, val));
                        reps.push(Rep {
                            span: w.span.clone(),
                            parts,
                            priority: 2,
                        });
                    }
                }
                i += 1;
                continue;
            }
            if let Some(base) = flag_base(tool, v) {
                if let Some(next) = words.get(i + 1) {
                    if !next.value.starts_with('-') && !next.value.contains("{{") {
                        reps.push(Rep {
                            span: next.span.clone(),
                            parts: value_parts(base, &next.value),
                            priority: 2,
                        });
                        i += 2;
                        continue;
                    }
                }
            }
            // Options known to take a value: skip it so positional counting
            // stays right (`ping -c 1 host`).
            i += if opt_takes_value(tool, v) { 2 } else { 1 };
            continue;
        }
        // scp/rsync `[user@]host:path`.
        if matches!(tool, "scp" | "rsync") {
            if let Some((hostpart, path)) = v.split_once(':') {
                if !hostpart.is_empty() && !path.starts_with("//") && hostpart.len() > 1 {
                    let mut parts = value_parts("host", hostpart);
                    parts.push(Part::Lit(format!(":{path}")));
                    reps.push(Rep {
                        span: w.span.clone(),
                        parts,
                        priority: 2,
                    });
                }
            }
            pos.push(v);
            i += 1;
            continue;
        }
        if let Some(base) = positional_base(tool, &pos, v) {
            reps.push(Rep {
                span: w.span.clone(),
                parts: value_parts(base, v),
                priority: 2,
            });
        } else if is_uuid(v) {
            reps.push(Rep {
                span: w.span.clone(),
                parts: vec![var("id", v)],
                priority: 2,
            });
        }
        pos.push(v);
        i += 1;
    }
}

fn is_uuid(v: &str) -> bool {
    v.len() == 36
        && v.chars().enumerate().all(|(i, c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(cmd: &str) -> ParameterizedCommand {
        parameterize(cmd, ShellDialect::Posix)
    }

    #[test]
    fn kubectl_logs() {
        let r = p("kubectl logs -n prod api-7d9f-x2x4z --tail=200 -c app");
        assert_eq!(
            r.template,
            "kubectl logs -n {{namespace}} {{pod}} --tail={{lines}} -c {{container}}"
        );
        let names: Vec<_> = r.variables.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["namespace", "pod", "lines", "container"]);
        assert_eq!(r.variables[0].default.as_deref(), Some("prod"));
        assert_eq!(r.variables[2].default.as_deref(), Some("200"));
    }

    #[test]
    fn secrets_become_variables_without_defaults() {
        let r = p("PGPASSWORD=Hunter2Secret psql -h db.internal -U app -d billing");
        assert!(!r.template.contains("Hunter2Secret"));
        assert!(
            r.template.starts_with(
                "PGPASSWORD={{password}} psql -h {{host}} -U {{user}} -d {{database}}"
            ),
            "{}",
            r.template
        );
        assert_eq!(r.secrets_removed, 1);
        let pw = r.variables.iter().find(|v| v.name == "password").unwrap();
        assert!(pw.default.is_none());
        assert!(!serde_json::to_string(&r).unwrap().contains("Hunter2Secret"));
    }
}
