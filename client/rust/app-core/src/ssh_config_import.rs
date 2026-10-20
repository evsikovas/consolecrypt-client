//! Preview of `~/.ssh/config` for the "Import from ~/.ssh/config" dialog.
//! Parsing only: nothing is stored and private key files are not read here
//! (only their paths are returned).

use crate::{AppError, AppResult};
use cc_ssh_core::ssh_config::{import_ssh_config, parse_ssh_config_file, ImportOptions};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct SshConfigHostPreview {
    pub alias: String,
    pub address: String,
    pub port: Option<u16>,
    pub username: Option<String>,
    /// First `IdentityFile`, expanded (`~`, `%d`, …).
    pub identity_file: Option<String>,
    /// Names of the hops (other entries of this preview), closest first.
    pub jump: Vec<String>,
    /// `ProxyCommand` is not supported natively; the host is still imported.
    pub proxy_command: bool,
    /// Created only as a `ProxyJump` hop (`user@host:port` spec).
    pub jump_only: bool,
    pub agent_forwarding: bool,
    pub keepalive_secs: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SshConfigPreview {
    pub path: String,
    pub found: bool,
    pub hosts: Vec<SshConfigHostPreview>,
    pub warnings: Vec<String>,
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Parses `path` (default `~/.ssh/config`, `Include` expanded).
pub fn ssh_config_preview(path: Option<String>) -> AppResult<SshConfigPreview> {
    let home = home_dir();
    let path = match path.filter(|p| !p.trim().is_empty()) {
        Some(p) => PathBuf::from(p),
        None => home
            .as_ref()
            .map(|h| h.join(".ssh").join("config"))
            .ok_or_else(|| AppError::invalid("ssh_config", "home directory is unknown"))?,
    };
    let shown = path.to_string_lossy().to_string();
    if !path.is_file() {
        return Ok(SshConfigPreview {
            path: shown,
            found: false,
            hosts: Vec::new(),
            warnings: Vec::new(),
        });
    }
    let config = parse_ssh_config_file(&path, home.as_deref())
        .map_err(|_| AppError::invalid("ssh_config", "cannot read the file"))?;
    let aliases: BTreeSet<String> = config.concrete_aliases().into_iter().collect();
    let options = ImportOptions {
        home_dir: home,
        local_user: std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .ok(),
        group_name: None,
    };
    let import = import_ssh_config(&config, &options);
    let names: HashMap<_, _> = import
        .hosts
        .iter()
        .map(|h| (h.id, h.name.clone()))
        .collect();
    let files: HashMap<_, _> = import
        .identity_files
        .iter()
        .map(|f| (f.credential_id, f.path.to_string_lossy().to_string()))
        .collect();
    let hosts = import
        .hosts
        .iter()
        .map(|host| SshConfigHostPreview {
            alias: host.name.clone(),
            address: host.address.clone(),
            port: host.port,
            username: host.username.clone(),
            identity_file: host.credential_id.and_then(|id| files.get(&id).cloned()),
            jump: host
                .jump_chain
                .iter()
                .filter_map(|id| names.get(id).cloned())
                .collect(),
            proxy_command: host.proxy_command.is_some()
                || host.metadata.contains_key("ssh_config.proxy_command"),
            jump_only: !aliases.contains(&host.name),
            agent_forwarding: host.agent_forwarding,
            keepalive_secs: host.keepalive_secs,
        })
        .collect();
    Ok(SshConfigPreview {
        path: shown,
        found: true,
        hosts,
        warnings: import.warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_hosts_jumps_and_keys() {
        let dir = std::env::temp_dir().join(format!("cc-ssh-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config");
        std::fs::write(
            &path,
            "Host bastion\n  HostName 10.0.0.1\n  User ops\n\nHost nethtrap\n  HostName 192.0.2.7\n  User root\n  Port 2222\n  IdentityFile /keys/id_ed25519\n  ProxyJump bastion\n\nHost *\n  ServerAliveInterval 30\n",
        )
        .unwrap();
        let preview = ssh_config_preview(Some(path.to_string_lossy().to_string())).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert!(preview.found);
        let host = preview
            .hosts
            .iter()
            .find(|h| h.alias == "nethtrap")
            .unwrap();
        assert_eq!(host.address, "192.0.2.7");
        assert_eq!(host.port, Some(2222));
        assert_eq!(host.username.as_deref(), Some("root"));
        assert_eq!(host.identity_file.as_deref(), Some("/keys/id_ed25519"));
        assert_eq!(host.jump, ["bastion"]);
        assert_eq!(host.keepalive_secs, Some(30));
        assert!(!host.jump_only);
        assert!(preview.hosts.iter().all(|h| h.alias != "*"));
    }

    #[test]
    fn missing_file_is_not_an_error() {
        let preview = ssh_config_preview(Some("/nonexistent/cc/ssh_config".into())).unwrap();
        assert!(!preview.found);
        assert!(preview.hosts.is_empty());
    }
}
