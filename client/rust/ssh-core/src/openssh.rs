//! System OpenSSH fallback (CLIENT_SPEC §6.5).
//!
//! * Detects `/usr/bin/ssh` (macOS/Unix) or the Windows OpenSSH Client
//!   (`%SystemRoot%\System32\OpenSSH\ssh.exe`). Cygwin/MSYS builds are
//!   refused.
//! * Builds the command line from a [`ConnectionPlan`]: a per-session
//!   `ssh_config` (passed with `-F`, which OpenSSH also forwards to the
//!   `ProxyJump` child processes) with one `Host` alias per hop and
//!   `ProxyJump` between them.
//! * Authentication uses `IdentityAgent` / `SSH_AUTH_SOCK` pointing at the
//!   built-in agent (`cc-ssh-agent-core`) — never an `IdentityFile` with a
//!   decrypted key. The generated config contains no secrets.
//! * Host keys: `StrictHostKeyChecking` follows each hop's policy and
//!   `UserKnownHostsFile` can point at an export of the vault's known hosts.

use crate::error::SshError;
use crate::planner::{ConnectionPlan, Hop};
use cc_models::host::{HostKeyPolicy, ProxyKind};
use cc_models::tunnel::{Tunnel, TunnelKind};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A detected OpenSSH client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenSshClient {
    pub path: PathBuf,
    /// `ssh -V` banner, e.g. `OpenSSH_9.8p1, LibreSSL 3.3.6`.
    pub version: Option<String>,
}

/// Paths that look like Cygwin / MSYS / Git-for-Windows builds (not
/// supported: no Cygwin dependency).
pub fn is_cygwin_like(path: &Path) -> bool {
    let p = path
        .to_string_lossy()
        .to_ascii_lowercase()
        .replace('\\', "/");
    p.contains("cygwin") || p.contains("msys") || p.contains("/git/usr/bin") || p.contains("mingw")
}

fn candidate_paths() -> Vec<PathBuf> {
    if cfg!(any(target_os = "ios", target_os = "android")) {
        return Vec::new();
    }
    let mut v = Vec::new();
    if cfg!(windows) {
        if let Some(root) = std::env::var_os("SystemRoot") {
            v.push(
                PathBuf::from(root)
                    .join("System32")
                    .join("OpenSSH")
                    .join("ssh.exe"),
            );
        }
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            v.push(PathBuf::from(pf).join("OpenSSH").join("ssh.exe"));
        }
    } else {
        v.push(PathBuf::from("/usr/bin/ssh"));
        v.push(PathBuf::from("/usr/local/bin/ssh"));
        v.push(PathBuf::from("/opt/homebrew/bin/ssh"));
    }
    if let Some(path) = std::env::var_os("PATH") {
        let exe = if cfg!(windows) { "ssh.exe" } else { "ssh" };
        for dir in std::env::split_paths(&path) {
            v.push(dir.join(exe));
        }
    }
    v
}

/// Run `ssh -V` and return the banner (printed on stderr).
pub fn openssh_version(path: &Path) -> Option<String> {
    if cfg!(any(target_os = "ios", target_os = "android")) {
        return None;
    }
    let out = Command::new(path).arg("-V").output().ok()?;
    let text = if out.stderr.is_empty() {
        out.stdout
    } else {
        out.stderr
    };
    let s = String::from_utf8_lossy(&text).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Find a usable OpenSSH client (no Cygwin/MSYS builds).
pub fn detect_openssh() -> Option<OpenSshClient> {
    candidate_paths()
        .into_iter()
        .filter(|p| p.is_file() && !is_cygwin_like(p))
        .find_map(|path| {
            let version = openssh_version(&path);
            match &version {
                Some(v) if !v.contains("OpenSSH") => None,
                _ => Some(OpenSshClient { path, version }),
            }
        })
}

/// Options for [`build_openssh_command`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenSshOptions {
    /// Socket / pipe of the built-in agent holding the plan's keys.
    pub agent_socket: PathBuf,
    /// Where the caller will write [`OpenSshCommand::config`] (`-F`).
    pub config_path: PathBuf,
    /// Known hosts file (e.g. an export of the vault). `None` = OpenSSH default.
    pub known_hosts_file: Option<PathBuf>,
    /// Remote command; `None` = interactive shell.
    pub remote_command: Option<String>,
    /// Force (`true`) or disable (`false`) PTY allocation; `None` = OpenSSH decides.
    pub request_tty: Option<bool>,
    /// Emit `LocalForward` / `RemoteForward` / `DynamicForward` for the plan's tunnels.
    pub include_forwards: bool,
    /// Keepalive (seconds) when the hop does not specify one.
    pub default_keepalive_secs: Option<u32>,
}

impl OpenSshOptions {
    pub fn new(agent_socket: impl Into<PathBuf>, config_path: impl Into<PathBuf>) -> Self {
        Self {
            agent_socket: agent_socket.into(),
            config_path: config_path.into(),
            known_hosts_file: None,
            remote_command: None,
            request_tty: None,
            include_forwards: false,
            default_keepalive_secs: Some(30),
        }
    }
}

/// A ready-to-spawn OpenSSH invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenSshCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Extra environment (`SSH_AUTH_SOCK`).
    pub env: Vec<(String, String)>,
    /// `ssh_config` text to write to `OpenSshOptions::config_path` (0600)
    /// before spawning. Contains no secrets.
    pub config: String,
    /// Alias of the target inside `config`.
    pub target_alias: String,
}

impl OpenSshCommand {
    /// Build a `std::process::Command` (the config file must already exist).
    pub fn to_command(&self) -> Command {
        let mut c = Command::new(&self.program);
        c.args(&self.args);
        for (k, v) in &self.env {
            c.env(k, v);
        }
        c
    }
}

fn quote(value: &str) -> String {
    if value.is_empty()
        || value
            .chars()
            .any(|c| c.is_whitespace() || c == '#' || c == '"')
    {
        format!("\"{}\"", value.replace('"', ""))
    } else {
        value.to_string()
    }
}

fn check_token(what: &str, value: &str) -> Result<(), SshError> {
    if value.is_empty()
        || value
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '"')
    {
        return Err(SshError::Unsupported(format!(
            "{what} {value:?} cannot be expressed in an ssh_config file"
        )));
    }
    Ok(())
}

fn strict_value(p: HostKeyPolicy) -> &'static str {
    match p {
        HostKeyPolicy::Ask => "ask",
        HostKeyPolicy::Strict => "yes",
        HostKeyPolicy::AcceptNew => "accept-new",
    }
}

fn bind_spec(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

fn forward_line(t: &Tunnel) -> Option<String> {
    let bind = bind_spec(&t.bind_host, t.bind_port);
    match t.kind {
        TunnelKind::Local | TunnelKind::Remote => {
            let target = bind_spec(t.target_host.as_deref()?, t.target_port?);
            let kw = if t.kind == TunnelKind::Local {
                "LocalForward"
            } else {
                "RemoteForward"
            };
            Some(format!("  {kw} {bind} {target}\n"))
        }
        TunnelKind::Dynamic => Some(format!("  DynamicForward {bind}\n")),
    }
}

fn hop_block(
    alias: &str,
    hop: &Hop,
    proxy_jump: Option<&str>,
    extra: &str,
    default_keepalive: Option<u32>,
) -> Result<String, SshError> {
    check_token("host", &hop.endpoint.host)?;
    check_token("user", &hop.username)?;
    let mut s = format!("Host {alias}\n");
    s.push_str(&format!("  HostName {}\n", hop.endpoint.host));
    s.push_str(&format!("  Port {}\n", hop.endpoint.port));
    s.push_str(&format!("  User {}\n", hop.username));
    s.push_str(&format!(
        "  StrictHostKeyChecking {}\n",
        strict_value(hop.host_key_policy)
    ));
    if let Some(k) = hop.keepalive_secs.or(default_keepalive).filter(|k| *k > 0) {
        s.push_str(&format!(
            "  ServerAliveInterval {k}\n  ServerAliveCountMax 3\n"
        ));
    }
    if let Some(j) = proxy_jump {
        s.push_str(&format!("  ProxyJump {j}\n"));
    }
    s.push_str(extra);
    Ok(s)
}

/// Build the OpenSSH invocation for `plan`.
pub fn build_openssh_command(
    client: &OpenSshClient,
    plan: &ConnectionPlan,
    opts: &OpenSshOptions,
) -> Result<OpenSshCommand, SshError> {
    if cfg!(any(target_os = "ios", target_os = "android")) {
        return Err(SshError::Unsupported(
            "the system OpenSSH backend is unavailable on mobile; use native SSH".into(),
        ));
    }
    let hops = plan.all_hops();
    let agent = opts.agent_socket.to_string_lossy().to_string();
    let mut config =
        String::from("# Generated by ConsoleCrypt for a single session. Contains no secrets.\n");

    let mut prev_alias: Option<String> = None;
    let mut target_alias = String::new();
    for (i, hop) in hops.iter().enumerate() {
        let is_target = i + 1 == hops.len();
        let alias = if is_target {
            "cc-target".to_string()
        } else {
            format!("cc-hop-{i}")
        };
        let mut extra = String::new();
        if i == 0 {
            if let Some(proxy) = &plan.proxy {
                extra.push_str(&proxy_command(proxy)?);
            }
        }
        if is_target {
            extra.push_str(&format!(
                "  ForwardAgent {}\n",
                if plan.agent_forwarding { "yes" } else { "no" }
            ));
            if opts.include_forwards {
                for t in &plan.forwards {
                    if let Some(l) = forward_line(t) {
                        extra.push_str(&l);
                    }
                }
            }
        } else {
            extra.push_str("  ForwardAgent no\n");
        }
        config.push_str(&hop_block(
            &alias,
            hop,
            prev_alias.as_deref(),
            &extra,
            opts.default_keepalive_secs,
        )?);
        prev_alias = Some(alias.clone());
        if is_target {
            target_alias = alias;
        }
    }

    config.push_str("Host *\n");
    config.push_str(&format!("  IdentityAgent {}\n", quote(&agent)));
    if let Some(kh) = &opts.known_hosts_file {
        config.push_str(&format!(
            "  UserKnownHostsFile {}\n",
            quote(&kh.to_string_lossy())
        ));
    }
    config.push_str("  HashKnownHosts no\n  UpdateHostKeys no\n  BatchMode no\n");

    let mut args = vec![
        "-F".to_string(),
        opts.config_path.to_string_lossy().to_string(),
    ];
    match opts.request_tty {
        Some(true) => args.push("-tt".into()),
        Some(false) => args.push("-T".into()),
        None => {}
    }
    args.push(target_alias.clone());
    if let Some(cmd) = &opts.remote_command {
        args.push("--".into());
        args.push(cmd.clone());
    }
    Ok(OpenSshCommand {
        program: client.path.clone(),
        args,
        env: vec![("SSH_AUTH_SOCK".into(), agent)],
        config,
        target_alias,
    })
}

fn proxy_command(proxy: &cc_models::host::Proxy) -> Result<String, SshError> {
    if cfg!(windows) {
        return Err(SshError::Unsupported(
            "network proxies are not supported by the OpenSSH fallback on Windows".into(),
        ));
    }
    if proxy.username.is_some() || proxy.password_secret_id.is_some() {
        return Err(SshError::Unsupported(
            "authenticated proxies are not supported by the OpenSSH fallback".into(),
        ));
    }
    check_token("proxy address", &proxy.address)?;
    let mode = match proxy.kind {
        ProxyKind::Socks5 => "5",
        ProxyKind::HttpConnect => "connect",
    };
    Ok(format!(
        "  ProxyCommand /usr/bin/nc -X {mode} -x {} %h %p\n",
        bind_spec(&proxy.address, proxy.port)
    ))
}

/// Write a file readable only by the current user (0600 on Unix).
pub fn write_private_file(path: &Path, contents: &str) -> std::io::Result<()> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let mut f = o.open(path)?;
    f.write_all(contents.as_bytes())?;
    f.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::Endpoint;
    use cc_models::host::{Proxy, SshBackend};
    use cc_models::ObjectId;

    fn hop(name: &str, host: &str, port: u16, user: &str, policy: HostKeyPolicy) -> Hop {
        Hop {
            host_id: ObjectId::new(),
            name: name.into(),
            endpoint: Endpoint::new(host, port),
            username: user.into(),
            credential: None,
            host_key_policy: policy,
            keepalive_secs: None,
        }
    }

    fn plan(route: Vec<Hop>) -> ConnectionPlan {
        ConnectionPlan {
            host_id: ObjectId::new(),
            name: "db".into(),
            target: Endpoint::new("10.10.10.20", 22),
            username: "alex".into(),
            credential: None,
            host_key_policy: HostKeyPolicy::Strict,
            route,
            proxy: None,
            forwards: vec![],
            backend: SshBackend::OpenSsh,
            keepalive_secs: Some(15),
            agent_forwarding: false,
            warnings: vec![],
        }
    }

    fn client() -> OpenSshClient {
        OpenSshClient {
            path: PathBuf::from("/usr/bin/ssh"),
            version: None,
        }
    }

    #[test]
    fn multi_hop_command_uses_agent_and_proxyjump() {
        let p = plan(vec![
            hop("b1", "bastion-a", 22, "jump", HostKeyPolicy::Ask),
            hop("b2", "bastion-b", 2222, "jump2", HostKeyPolicy::AcceptNew),
        ]);
        let mut o = OpenSshOptions::new("/tmp/cc agent/agent.sock", "/tmp/cc/ssh_config");
        o.known_hosts_file = Some(PathBuf::from("/tmp/cc/known_hosts"));
        o.remote_command = Some("uptime".into());
        let cmd = build_openssh_command(&client(), &p, &o).unwrap();
        assert_eq!(
            cmd.args,
            vec!["-F", "/tmp/cc/ssh_config", "cc-target", "--", "uptime"]
        );
        assert_eq!(
            cmd.env,
            vec![(
                "SSH_AUTH_SOCK".to_string(),
                "/tmp/cc agent/agent.sock".to_string()
            )]
        );
        let c = &cmd.config;
        assert!(c.contains("Host cc-hop-0\n  HostName bastion-a\n  Port 22\n  User jump\n  StrictHostKeyChecking ask\n"), "{c}");
        assert!(c.contains("Host cc-hop-1\n  HostName bastion-b\n  Port 2222\n  User jump2\n  StrictHostKeyChecking accept-new\n"), "{c}");
        assert!(c.contains("  ProxyJump cc-hop-0\n"), "{c}");
        assert!(c.contains("Host cc-target\n  HostName 10.10.10.20\n  Port 22\n  User alex\n  StrictHostKeyChecking yes\n  ServerAliveInterval 15\n"), "{c}");
        assert!(c.contains("  ProxyJump cc-hop-1\n"), "{c}");
        assert!(
            c.contains("  IdentityAgent \"/tmp/cc agent/agent.sock\"\n"),
            "{c}"
        );
        assert!(
            c.contains("  UserKnownHostsFile /tmp/cc/known_hosts\n"),
            "{c}"
        );
        assert!(
            !c.contains("IdentityFile"),
            "never a temp IdentityFile: {c}"
        );
    }

    #[test]
    fn forwards_and_proxy() {
        let mut p = plan(vec![]);
        let now = chrono::Utc::now();
        let mk = |kind, bind: &str, th: Option<&str>, tp: Option<u16>| Tunnel {
            id: ObjectId::new(),
            name: "t".into(),
            kind,
            host_id: p.host_id,
            bind_host: bind.into(),
            bind_port: 15432,
            target_host: th.map(Into::into),
            target_port: tp,
            auto_start: false,
            created_at: now,
            updated_at: now,
        };
        p.forwards = vec![
            mk(
                TunnelKind::Local,
                "127.0.0.1",
                Some("db.internal"),
                Some(5432),
            ),
            mk(TunnelKind::Remote, "::1", Some("localhost"), Some(3000)),
            mk(TunnelKind::Dynamic, "127.0.0.1", None, None),
        ];
        p.proxy = Some(Proxy {
            id: ObjectId::new(),
            name: "p".into(),
            kind: ProxyKind::Socks5,
            address: "proxy.local".into(),
            port: 1080,
            username: None,
            password_secret_id: None,
            created_at: now,
            updated_at: now,
        });
        let mut o = OpenSshOptions::new("/a", "/c");
        o.include_forwards = true;
        o.request_tty = Some(true);
        #[cfg(windows)]
        {
            assert!(matches!(
                build_openssh_command(&client(), &p, &o),
                Err(SshError::Unsupported(_))
            ));
            // Windows has no nc-based ProxyCommand; forwards still work.
            p.proxy = None;
        }
        let cmd = build_openssh_command(&client(), &p, &o).unwrap();
        let c = &cmd.config;
        assert!(
            c.contains("  LocalForward 127.0.0.1:15432 db.internal:5432\n"),
            "{c}"
        );
        assert!(
            c.contains("  RemoteForward [::1]:15432 localhost:3000\n"),
            "{c}"
        );
        assert!(c.contains("  DynamicForward 127.0.0.1:15432\n"), "{c}");
        if !cfg!(windows) {
            assert!(
                c.contains("ProxyCommand /usr/bin/nc -X 5 -x proxy.local:1080 %h %p"),
                "{c}"
            );
        }
        assert!(cmd.args.contains(&"-tt".to_string()));
    }

    #[test]
    fn rejects_injection_in_values() {
        let mut p = plan(vec![]);
        p.username = "alex\n  ProxyCommand evil".into();
        let o = OpenSshOptions::new("/a", "/c");
        assert!(build_openssh_command(&client(), &p, &o).is_err());
    }

    #[test]
    fn cygwin_detection() {
        assert!(is_cygwin_like(Path::new(r"C:\cygwin64\bin\ssh.exe")));
        assert!(is_cygwin_like(Path::new(
            r"C:\Program Files\Git\usr\bin\ssh.exe"
        )));
        assert!(is_cygwin_like(Path::new("/c/msys64/usr/bin/ssh")));
        assert!(!is_cygwin_like(Path::new(
            r"C:\Windows\System32\OpenSSH\ssh.exe"
        )));
        assert!(!is_cygwin_like(Path::new("/usr/bin/ssh")));
    }

    #[test]
    fn detects_system_ssh_when_present() {
        if Path::new("/usr/bin/ssh").exists() {
            let c = detect_openssh().expect("ssh present");
            assert!(!is_cygwin_like(&c.path));
        }
    }

    #[test]
    fn private_file_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("cfg");
        write_private_file(&p, "Host x\n").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "Host x\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }
}
