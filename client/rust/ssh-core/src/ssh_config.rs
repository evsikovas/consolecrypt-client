//! `~/.ssh/config` parser and import into model objects (own minimal
//! parser; no third-party dependency).
//!
//! Semantics follow ssh_config(5) for what we support: blocks apply in file
//! order, the *first* obtained value of an option wins, except the
//! accumulating options (`IdentityFile`, `CertificateFile`, `LocalForward`,
//! `RemoteForward`, `DynamicForward`). `Host` patterns support `*`, `?` and
//! `!negation`. `Match` blocks are skipped with a warning. `Include` is
//! expanded by [`parse_ssh_config_file`].
//!
//! The importer never reads private key files: it returns the referenced
//! paths ([`IdentityFileRef`]) so app-core can import them into the vault
//! (keeping any passphrase protection).

use crate::known_hosts::glob_match;
use cc_models::credential::{Credential, CredentialKind};
use cc_models::group::Group;
use cc_models::host::{Host, HostKeyPolicy};
use cc_models::tunnel::{Tunnel, TunnelKind};
use cc_models::ObjectId;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Kind of a config block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    /// Options before the first `Host`/`Match` line.
    Global,
    Host(Vec<String>),
    /// Unsupported `Match` criteria (kept verbatim).
    Match(String),
}

/// One option line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigOption {
    /// Lower-cased keyword.
    pub keyword: String,
    pub args: Vec<String>,
    pub line_no: usize,
}

/// A `Host` / `Match` block (or the global block).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigBlock {
    pub kind: BlockKind,
    pub options: Vec<ConfigOption>,
}

/// Parsed ssh_config.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SshConfig {
    pub blocks: Vec<ConfigBlock>,
    pub warnings: Vec<String>,
}

const ACCUMULATING: &[&str] = &[
    "identityfile",
    "certificatefile",
    "localforward",
    "remoteforward",
    "dynamicforward",
];

/// Split a config line into words, honoring double quotes and `key=value`
/// / `key = value`.
fn tokenize(line: &str) -> Vec<String> {
    let line = line.trim();
    let split = line
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(line.len());
    let (keyword, rest) = line.split_at(split);
    let mut out = vec![keyword.to_string()];
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('=').unwrap_or(rest);
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut has_token = false;
    for c in rest.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                has_token = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if has_token {
                    out.push(std::mem::take(&mut cur));
                    has_token = false;
                }
            }
            c => {
                cur.push(c);
                has_token = true;
            }
        }
    }
    if has_token {
        out.push(cur);
    }
    out
}

/// Parse ssh_config text (no `Include` expansion; see [`parse_ssh_config_file`]).
pub fn parse_ssh_config(text: &str) -> SshConfig {
    let mut cfg = SshConfig::default();
    let mut current = ConfigBlock {
        kind: BlockKind::Global,
        options: Vec::new(),
    };
    for (idx, raw) in text.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = tokenize(line);
        if words.is_empty() {
            continue;
        }
        let keyword = words.remove(0).to_ascii_lowercase();
        match keyword.as_str() {
            "host" => {
                cfg.blocks.push(std::mem::replace(
                    &mut current,
                    ConfigBlock {
                        kind: BlockKind::Host(words),
                        options: Vec::new(),
                    },
                ));
            }
            "match" => {
                cfg.warnings.push(format!(
                    "line {line_no}: 'Match {}' is not supported; block ignored",
                    words.join(" ")
                ));
                cfg.blocks.push(std::mem::replace(
                    &mut current,
                    ConfigBlock {
                        kind: BlockKind::Match(words.join(" ")),
                        options: Vec::new(),
                    },
                ));
            }
            _ => current.options.push(ConfigOption {
                keyword,
                args: words,
                line_no,
            }),
        }
    }
    cfg.blocks.push(current);
    cfg.blocks
        .retain(|b| !(b.kind == BlockKind::Global && b.options.is_empty()));
    cfg
}

/// Read and parse a config file, expanding `Include` (relative paths are
/// resolved against `~/.ssh`; `*`/`?` globs in the file name are supported;
/// depth-limited).
pub fn parse_ssh_config_file(path: &Path, home: Option<&Path>) -> std::io::Result<SshConfig> {
    let mut out = SshConfig::default();
    include_file(path, home, 0, &mut out)?;
    Ok(out)
}

fn include_file(
    path: &Path,
    home: Option<&Path>,
    depth: usize,
    out: &mut SshConfig,
) -> std::io::Result<()> {
    if depth > 16 {
        out.warnings
            .push(format!("{}: Include nesting too deep", path.display()));
        return Ok(());
    }
    let text = std::fs::read_to_string(path)?;
    let parsed = parse_ssh_config(&text);
    out.warnings.extend(parsed.warnings);
    for mut block in parsed.blocks {
        let mut kept = Vec::new();
        for opt in std::mem::take(&mut block.options) {
            if opt.keyword != "include" {
                kept.push(opt);
                continue;
            }
            // Flush options gathered so far, then splice included blocks.
            if !kept.is_empty() {
                out.blocks.push(ConfigBlock {
                    kind: block.kind.clone(),
                    options: std::mem::take(&mut kept),
                });
            }
            for pattern in &opt.args {
                for inc in expand_include(pattern, home) {
                    if let Err(e) = include_file(&inc, home, depth + 1, out) {
                        out.warnings.push(format!("Include {}: {e}", inc.display()));
                    }
                }
            }
        }
        if !kept.is_empty() || block.options.is_empty() {
            block.options = kept;
            out.blocks.push(block);
        }
    }
    Ok(())
}

fn expand_include(pattern: &str, home: Option<&Path>) -> Vec<PathBuf> {
    let expanded = expand_tilde(pattern, home);
    let p = if expanded.is_absolute() {
        expanded
    } else {
        match home {
            Some(h) => h.join(".ssh").join(expanded),
            None => expanded,
        }
    };
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if !name.contains('*') && !name.contains('?') {
        return vec![p];
    }
    let dir = p.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut v: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|f| {
                    f.file_name()
                        .is_some_and(|n| glob_match(&name, &n.to_string_lossy()))
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn expand_tilde(s: &str, home: Option<&Path>) -> PathBuf {
    match (s.strip_prefix("~/"), home) {
        (Some(rest), Some(h)) => h.join(rest),
        _ if s == "~" => home
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(s)),
        _ => PathBuf::from(s),
    }
}

/// Do `Host` patterns match `alias`? (negation wins)
pub fn host_patterns_match(patterns: &[String], alias: &str) -> bool {
    let mut positive = false;
    for p in patterns {
        if let Some(neg) = p.strip_prefix('!') {
            if glob_match(neg, alias) {
                return false;
            }
        } else if glob_match(p, alias) {
            positive = true;
        }
    }
    positive
}

impl SshConfig {
    /// Effective options for `alias` (lower-cased keyword → values; for
    /// accumulating options every occurrence in order).
    pub fn resolve(&self, alias: &str) -> HashMap<String, Vec<Vec<String>>> {
        let mut out: HashMap<String, Vec<Vec<String>>> = HashMap::new();
        for b in &self.blocks {
            let applies = match &b.kind {
                BlockKind::Global => true,
                BlockKind::Host(p) => host_patterns_match(p, alias),
                BlockKind::Match(_) => false,
            };
            if !applies {
                continue;
            }
            for o in &b.options {
                let entry = out.entry(o.keyword.clone()).or_default();
                if ACCUMULATING.contains(&o.keyword.as_str()) || entry.is_empty() {
                    entry.push(o.args.clone());
                }
            }
        }
        out
    }

    /// Concrete aliases (Host patterns without wildcards / negation), in order.
    pub fn concrete_aliases(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for b in &self.blocks {
            if let BlockKind::Host(patterns) = &b.kind {
                for p in patterns {
                    if !p.contains(['*', '?', '!']) && !v.contains(p) {
                        v.push(p.clone());
                    }
                }
            }
        }
        v
    }
}

/// Import options.
#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    /// Home directory for `~` / `%d` expansion.
    pub home_dir: Option<PathBuf>,
    /// Local user name for `%u` expansion.
    pub local_user: Option<String>,
    /// Name of the container group (default "Imported from ~/.ssh/config").
    pub group_name: Option<String>,
}

/// A private key file referenced by `IdentityFile`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityFileRef {
    pub credential_id: ObjectId,
    pub path: PathBuf,
    /// Matching `CertificateFile`, if any.
    pub certificate_path: Option<PathBuf>,
}

/// Result of an import. Nothing is persisted; app-core decides.
#[derive(Debug, Clone)]
pub struct SshConfigImport {
    pub group: Group,
    pub hosts: Vec<Host>,
    pub credentials: Vec<Credential>,
    pub identity_files: Vec<IdentityFileRef>,
    pub tunnels: Vec<Tunnel>,
    pub warnings: Vec<String>,
}

fn first<'a>(opts: &'a HashMap<String, Vec<Vec<String>>>, key: &str) -> Option<&'a Vec<String>> {
    opts.get(key).and_then(|v| v.first())
}

fn first_str(opts: &HashMap<String, Vec<Vec<String>>>, key: &str) -> Option<String> {
    first(opts, key).and_then(|a| a.first().cloned())
}

fn expand_tokens(
    s: &str,
    alias: &str,
    host: &str,
    user: Option<&str>,
    o: &ImportOptions,
) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('%') => out.push('%'),
            Some('h') => out.push_str(host),
            Some('n') => out.push_str(alias),
            Some('r') => out.push_str(user.unwrap_or("")),
            Some('u') => out.push_str(o.local_user.as_deref().unwrap_or("")),
            Some('d') => out.push_str(
                &o.home_dir
                    .as_deref()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default(),
            ),
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    out
}

/// Parse `[user@]host[:port]` (IPv6 as `[addr]:port`).
fn parse_jump_spec(spec: &str) -> (Option<String>, String, Option<u16>) {
    let spec = spec.strip_prefix("ssh://").unwrap_or(spec);
    let (user, rest) = match spec.rsplit_once('@') {
        Some((u, r)) => (Some(u.to_string()), r),
        None => (None, spec),
    };
    if let Some(inner) = rest.strip_prefix('[') {
        if let Some((addr, tail)) = inner.split_once(']') {
            let port = tail.strip_prefix(':').and_then(|p| p.parse().ok());
            return (user, addr.to_string(), port);
        }
    }
    match rest.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') => (user, h.to_string(), p.parse().ok()),
        _ => (user, rest.to_string(), None),
    }
}

/// Parse a forward spec (`[bind:]port`) → (bind_host, port).
fn parse_bind(spec: &str) -> Option<(String, u16)> {
    if let Some(inner) = spec.strip_prefix('[') {
        let (addr, tail) = inner.split_once(']')?;
        return Some((addr.to_string(), tail.strip_prefix(':')?.parse().ok()?));
    }
    match spec.rsplit_once(':') {
        Some((h, p)) => Some((
            if h.is_empty() || h == "*" {
                "0.0.0.0".to_string()
            } else {
                h.to_string()
            },
            p.parse().ok()?,
        )),
        None => Some(("127.0.0.1".to_string(), spec.parse().ok()?)),
    }
}

/// Import a parsed config into model objects.
pub fn import_ssh_config(config: &SshConfig, options: &ImportOptions) -> SshConfigImport {
    let mut warnings = config.warnings.clone();
    let group = Group::new(
        options
            .group_name
            .clone()
            .unwrap_or_else(|| "Imported from ~/.ssh/config".into()),
    );
    let aliases = config.concrete_aliases();
    // Pre-assign ids so ProxyJump can reference aliases defined later.
    let alias_ids: HashMap<String, ObjectId> = aliases
        .iter()
        .map(|a| (a.clone(), ObjectId::new()))
        .collect();

    let mut hosts: Vec<Host> = Vec::new();
    let mut credentials: Vec<Credential> = Vec::new();
    let mut identity_files: Vec<IdentityFileRef> = Vec::new();
    let mut cred_by_key: HashMap<String, ObjectId> = HashMap::new();
    let mut jump_hosts: HashMap<String, ObjectId> = HashMap::new();
    let mut extra_hosts: Vec<Host> = Vec::new();
    let mut tunnels: Vec<Tunnel> = Vec::new();

    for alias in &aliases {
        let opts = config.resolve(alias);
        let hostname = first_str(&opts, "hostname")
            .map(|h| expand_tokens(&h, alias, alias, None, options))
            .unwrap_or_else(|| alias.clone());
        let mut host = Host::new(alias.clone(), hostname.clone());
        host.id = alias_ids[alias];
        host.group_id = Some(group.id);
        host.username = first_str(&opts, "user");
        if let Some(p) = first_str(&opts, "port") {
            match p.parse::<u16>() {
                Ok(p) if p > 0 => host.port = Some(p),
                _ => warnings.push(format!("{alias}: invalid Port {p:?}")),
            }
        }
        let user = host.username.clone();

        // Credentials: IdentityAgent / IdentityFile (+ CertificateFile).
        let cert = first_str(&opts, "certificatefile").map(|c| {
            expand_tilde(
                &expand_tokens(&c, alias, &hostname, user.as_deref(), options),
                options.home_dir.as_deref(),
            )
        });
        if let Some(agent) = first_str(&opts, "identityagent") {
            let (kind, path) = match agent.as_str() {
                "none" => (None, None),
                "SSH_AUTH_SOCK" => (Some(CredentialKind::OsSshAgent), None),
                p => (
                    Some(CredentialKind::ExternalAgent),
                    Some(
                        expand_tilde(
                            &expand_tokens(p, alias, &hostname, user.as_deref(), options),
                            options.home_dir.as_deref(),
                        )
                        .to_string_lossy()
                        .to_string(),
                    ),
                ),
            };
            if let Some(kind) = kind {
                let key = format!("agent:{}", path.clone().unwrap_or_default());
                let id = *cred_by_key.entry(key).or_insert_with(|| {
                    let mut c = Credential::new(
                        match &path {
                            Some(p) => format!("Agent {p}"),
                            None => "OS SSH agent".into(),
                        },
                        kind,
                    );
                    c.agent_path = path.clone();
                    let id = c.id;
                    credentials.push(c);
                    id
                });
                host.credential_id = Some(id);
            }
        }
        if host.credential_id.is_none() {
            if let Some(files) = opts.get("identityfile") {
                let paths: Vec<PathBuf> = files
                    .iter()
                    .filter_map(|a| a.first())
                    .filter(|f| f.as_str() != "none")
                    .map(|f| {
                        expand_tilde(
                            &expand_tokens(f, alias, &hostname, user.as_deref(), options),
                            options.home_dir.as_deref(),
                        )
                    })
                    .collect();
                if paths.len() > 1 {
                    warnings.push(format!(
                        "{alias}: {} IdentityFile entries; only the first is imported",
                        paths.len()
                    ));
                }
                if let Some(path) = paths.into_iter().next() {
                    let key = format!("file:{}", path.display());
                    let id = match cred_by_key.get(&key) {
                        Some(id) => *id,
                        None => {
                            let name = path
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_else(|| "key".into());
                            let kind = if cert.is_some() {
                                CredentialKind::SshCertificate
                            } else {
                                CredentialKind::SshPrivateKey
                            };
                            let c = Credential::new(name, kind);
                            let id = c.id;
                            credentials.push(c);
                            identity_files.push(IdentityFileRef {
                                credential_id: id,
                                path: path.clone(),
                                certificate_path: cert.clone(),
                            });
                            cred_by_key.insert(key, id);
                            id
                        }
                    };
                    host.credential_id = Some(id);
                }
            }
        }

        // ProxyJump.
        if let Some(pj) = first_str(&opts, "proxyjump") {
            if pj != "none" {
                for spec in pj.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    let (u, h, p) = parse_jump_spec(spec);
                    let id = if u.is_none() && p.is_none() && alias_ids.contains_key(&h) {
                        alias_ids[&h]
                    } else {
                        *jump_hosts.entry(spec.to_string()).or_insert_with(|| {
                            let mut jh = Host::new(spec.to_string(), h.clone());
                            jh.username = u.clone();
                            jh.port = p;
                            jh.group_id = Some(group.id);
                            let id = jh.id;
                            extra_hosts.push(jh);
                            id
                        })
                    };
                    if id == host.id {
                        warnings.push(format!("{alias}: ProxyJump through itself ignored"));
                        continue;
                    }
                    host.jump_chain.push(id);
                }
            }
        }
        if let Some(pc) = first(&opts, "proxycommand") {
            if pc.first().map(String::as_str) != Some("none") {
                warnings.push(format!(
                    "{alias}: ProxyCommand is not supported by the native backend; stored in host metadata"
                ));
                host.metadata
                    .insert("ssh_config.proxy_command".into(), pc.join(" "));
            }
        }

        if let Some(v) = first_str(&opts, "stricthostkeychecking") {
            host.host_key_policy = match v.to_ascii_lowercase().as_str() {
                "yes" => HostKeyPolicy::Strict,
                "accept-new" => HostKeyPolicy::AcceptNew,
                "ask" => HostKeyPolicy::Ask,
                "no" | "off" => {
                    warnings.push(format!(
                        "{alias}: StrictHostKeyChecking {v} mapped to accept-new (changed keys still fail)"
                    ));
                    HostKeyPolicy::AcceptNew
                }
                _ => HostKeyPolicy::Ask,
            };
        }
        if let Some(v) = first_str(&opts, "serveraliveinterval") {
            host.keepalive_secs = v.parse().ok();
        }
        if let Some(v) = first_str(&opts, "forwardagent") {
            host.agent_forwarding = v.eq_ignore_ascii_case("yes");
        }

        // Forwards.
        let now = chrono::Utc::now();
        let mut add_tunnel = |kind: TunnelKind, args: &Vec<String>| {
            let bind = args.first().and_then(|b| parse_bind(b));
            let target = args.get(1).map(|t| parse_jump_spec(t));
            let Some((bind_host, bind_port)) = bind else {
                warnings.push(format!("{alias}: cannot parse forward {args:?}"));
                return;
            };
            let (target_host, target_port) = match (kind, target) {
                (TunnelKind::Dynamic, _) => (None, None),
                (_, Some((_, h, Some(p)))) => (Some(h), Some(p)),
                _ => {
                    warnings.push(format!("{alias}: cannot parse forward target {args:?}"));
                    return;
                }
            };
            tunnels.push(Tunnel {
                id: ObjectId::new(),
                name: format!("{alias} {:?} {bind_port}", kind).to_lowercase(),
                kind,
                host_id: host.id,
                bind_host,
                bind_port,
                target_host,
                target_port,
                auto_start: false,
                created_at: now,
                updated_at: now,
            });
        };
        for (kw, kind) in [
            ("localforward", TunnelKind::Local),
            ("remoteforward", TunnelKind::Remote),
            ("dynamicforward", TunnelKind::Dynamic),
        ] {
            if let Some(list) = opts.get(kw) {
                for args in list {
                    add_tunnel(kind, args);
                }
            }
        }

        for kw in [
            "localcommand",
            "pkcs11provider",
            "securitykeyprovider",
            "remotecommand",
        ] {
            if opts.contains_key(kw) {
                warnings.push(format!("{alias}: option '{kw}' is not imported"));
            }
        }
        hosts.push(host);
    }
    hosts.extend(extra_hosts);
    SshConfigImport {
        group,
        hosts,
        credentials,
        identity_files,
        tunnels,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
# global
ServerAliveInterval 20

Host bastion
    HostName bastion.example.com
    User jump
    Port 2222
    IdentityFile ~/.ssh/id_ed25519

Host db db-alias
    HostName=10.10.10.20
    User alex
    ProxyJump bastion,ops@inner.example:2200
    IdentityFile ~/.ssh/id_ed25519
    LocalForward 15432 localhost:5432
    DynamicForward 127.0.0.1:1080
    RemoteForward [::1]:9000 localhost:3000
    StrictHostKeyChecking accept-new

Host agent-host
    HostName "agent.example.com"
    IdentityAgent SSH_AUTH_SOCK

Host web-*
    User www

Host web-1
    HostName %h.internal
    CertificateFile ~/.ssh/id_rsa-cert.pub
    IdentityFile ~/.ssh/id_rsa
    ProxyCommand ssh -W %h:%p bastion

Match host foo
    User matched

Host *
    User fallback
    Port 22
"#;

    fn opts() -> ImportOptions {
        ImportOptions {
            home_dir: Some(PathBuf::from("/home/me")),
            local_user: Some("me".into()),
            group_name: None,
        }
    }

    #[test]
    fn parse_and_resolve_first_wins() {
        let cfg = parse_ssh_config(SAMPLE);
        assert_eq!(cfg.warnings.len(), 1, "{:?}", cfg.warnings);
        let r = cfg.resolve("web-1");
        assert_eq!(r["user"], vec![vec!["www".to_string()]]);
        assert_eq!(r["serveraliveinterval"], vec![vec!["20".to_string()]]);
        let r = cfg.resolve("agent-host");
        assert_eq!(r["user"], vec![vec!["fallback".to_string()]]);
        assert_eq!(r["hostname"], vec![vec!["agent.example.com".to_string()]]);
        assert_eq!(
            cfg.concrete_aliases(),
            vec!["bastion", "db", "db-alias", "agent-host", "web-1"]
        );
    }

    #[test]
    fn tokenizer() {
        assert_eq!(tokenize("HostName=a"), vec!["HostName", "a"]);
        assert_eq!(tokenize("HostName = a"), vec!["HostName", "a"]);
        assert_eq!(
            tokenize("IdentityFile \"/a b/c\""),
            vec!["IdentityFile", "/a b/c"]
        );
        assert_eq!(
            tokenize("LocalForward 1 h:2"),
            vec!["LocalForward", "1", "h:2"]
        );
    }

    #[test]
    fn import_maps_hosts_credentials_jumps_and_tunnels() {
        let cfg = parse_ssh_config(SAMPLE);
        let imp = import_ssh_config(&cfg, &opts());
        let by_name = |n: &str| imp.hosts.iter().find(|h| h.name == n).unwrap();

        let bastion = by_name("bastion");
        assert_eq!(bastion.address, "bastion.example.com");
        assert_eq!(bastion.port, Some(2222));
        assert_eq!(bastion.username.as_deref(), Some("jump"));
        assert_eq!(bastion.keepalive_secs, Some(20));
        assert_eq!(bastion.group_id, Some(imp.group.id));

        let db = by_name("db");
        assert_eq!(db.address, "10.10.10.20");
        assert_eq!(db.jump_chain.len(), 2);
        assert_eq!(db.jump_chain[0], bastion.id);
        let inner = imp.hosts.iter().find(|h| h.id == db.jump_chain[1]).unwrap();
        assert_eq!(inner.address, "inner.example");
        assert_eq!(inner.port, Some(2200));
        assert_eq!(inner.username.as_deref(), Some("ops"));
        assert_eq!(db.host_key_policy, HostKeyPolicy::AcceptNew);
        // same IdentityFile → one credential
        assert_eq!(db.credential_id, bastion.credential_id);
        assert!(by_name("db-alias").jump_chain.len() == 2);

        let agent = by_name("agent-host");
        let agent_cred = imp
            .credentials
            .iter()
            .find(|c| Some(c.id) == agent.credential_id)
            .unwrap();
        assert_eq!(agent_cred.kind, CredentialKind::OsSshAgent);

        let web = by_name("web-1");
        assert_eq!(web.address, "web-1.internal");
        assert_eq!(web.username.as_deref(), Some("www"));
        assert!(web.metadata.contains_key("ssh_config.proxy_command"));
        let wc = imp
            .credentials
            .iter()
            .find(|c| Some(c.id) == web.credential_id)
            .unwrap();
        assert_eq!(wc.kind, CredentialKind::SshCertificate);

        assert_eq!(imp.identity_files.len(), 2);
        let f = imp
            .identity_files
            .iter()
            .find(|f| f.path == Path::new("/home/me/.ssh/id_rsa"))
            .unwrap();
        assert_eq!(
            f.certificate_path.as_deref(),
            Some(Path::new("/home/me/.ssh/id_rsa-cert.pub"))
        );

        // tunnels: db and db-alias each get 3
        let db_tunnels: Vec<_> = imp.tunnels.iter().filter(|t| t.host_id == db.id).collect();
        assert_eq!(db_tunnels.len(), 3);
        let local = db_tunnels
            .iter()
            .find(|t| t.kind == TunnelKind::Local)
            .unwrap();
        assert_eq!(local.bind_host, "127.0.0.1");
        assert_eq!(local.bind_port, 15432);
        assert_eq!(local.target_host.as_deref(), Some("localhost"));
        assert_eq!(local.target_port, Some(5432));
        let remote = db_tunnels
            .iter()
            .find(|t| t.kind == TunnelKind::Remote)
            .unwrap();
        assert_eq!(remote.bind_host, "::1");
        for t in &imp.tunnels {
            t.validate().unwrap();
        }
        for h in &imp.hosts {
            h.validate().unwrap();
        }
        assert!(imp.warnings.iter().any(|w| w.contains("ProxyCommand")));
    }

    #[test]
    fn jump_spec_parsing() {
        assert_eq!(parse_jump_spec("h"), (None, "h".into(), None));
        assert_eq!(
            parse_jump_spec("u@h:22"),
            (Some("u".into()), "h".into(), Some(22))
        );
        assert_eq!(
            parse_jump_spec("[::1]:2222"),
            (None, "::1".into(), Some(2222))
        );
        assert_eq!(
            parse_jump_spec("ssh://u@h"),
            (Some("u".into()), "h".into(), None)
        );
    }

    #[test]
    fn include_expansion() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        std::fs::create_dir_all(home.join(".ssh/conf.d")).unwrap();
        std::fs::write(
            home.join(".ssh/config"),
            "Include conf.d/*.conf\nHost main\n  HostName main.example\n",
        )
        .unwrap();
        std::fs::write(
            home.join(".ssh/conf.d/a.conf"),
            "Host a\n  HostName a.example\n",
        )
        .unwrap();
        std::fs::write(
            home.join(".ssh/conf.d/b.conf"),
            "Host b\n  HostName b.example\n",
        )
        .unwrap();
        let cfg = parse_ssh_config_file(&home.join(".ssh/config"), Some(home)).unwrap();
        assert_eq!(cfg.concrete_aliases(), vec!["a", "b", "main"]);
        assert_eq!(
            cfg.resolve("b")["hostname"],
            vec![vec!["b.example".to_string()]]
        );
    }
}
