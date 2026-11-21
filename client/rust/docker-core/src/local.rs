//! Device-local Docker-compatible engines. Discovery never executes a CLI or
//! connects to a daemon, and never treats an SSH/TCP context as a local engine.
#[cfg(windows)]
use crate::{DockerConnector, DockerIo};
use crate::{DockerError, SharedConnector};
#[cfg(windows)]
use async_trait::async_trait;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LocalEndpoint {
    Unix(PathBuf),
    NamedPipe(String),
}

impl LocalEndpoint {
    fn stable_id(&self) -> String {
        // Context names, their priority and DOCKER_HOST may change. A saved
        // route must retain the actual endpoint and never follow another
        // daemon merely because it acquired the same context/environment slot.
        let kind = match self {
            Self::Unix(_) => "unix:",
            Self::NamedPipe(_) => "npipe:",
        };
        format!(
            "local/endpoint/{:x}",
            Sha256::digest(format!("{kind}{}", self.key()).as_bytes())
        )
    }

    pub fn uri(&self) -> String {
        match self {
            Self::Unix(path) => format!("unix://{}", path.display()),
            Self::NamedPipe(path) => format!("npipe://{}", path.replace('\\', "/")),
        }
    }

    fn key(&self) -> String {
        match self {
            Self::Unix(path) => std::fs::canonicalize(path)
                .unwrap_or_else(|_| path.clone())
                .to_string_lossy()
                .into_owned(),
            Self::NamedPipe(path) => path.to_ascii_lowercase(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LocalEngine {
    pub id: String,
    pub name: String,
    pub endpoint: LocalEndpoint,
}

pub fn is_local_host(id: &str) -> bool {
    id == "local" || id.starts_with("local/")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Platform {
    Mac,
    Windows,
    Linux,
    Unsupported,
}

fn platform() -> Platform {
    if cfg!(target_os = "macos") {
        Platform::Mac
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else if cfg!(target_os = "linux") {
        Platform::Linux
    } else {
        Platform::Unsupported
    }
}

#[derive(Debug)]
struct Discovery {
    platform: Platform,
    home: Option<PathBuf>,
    config: Option<PathBuf>,
    runtime: Option<PathBuf>,
    context: Option<String>,
    host: Option<String>,
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

fn json(path: &Path) -> Option<Value> {
    // Configuration is local input, not an unbounded filesystem crawl.
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > 256 * 1024 {
        return None;
    }
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(256 * 1024 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 256 * 1024 {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

fn endpoint(uri: &str, platform: Platform) -> Option<LocalEndpoint> {
    if uri.contains('\0') || uri.len() > 4096 {
        return None;
    }
    if platform == Platform::Windows {
        let path = uri.strip_prefix("npipe://")?.replace('/', "\\");
        let path = path.strip_prefix(r"\\.\pipe\")?;
        if path.is_empty()
            || path.len() > 256
            || !path
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-.$".contains(&b))
        {
            return None;
        }
        return Some(LocalEndpoint::NamedPipe(format!(r"\\.\pipe\{path}")));
    }
    if !matches!(platform, Platform::Mac | Platform::Linux) {
        return None;
    }
    let path = uri.strip_prefix("unix://")?;
    // URI syntax is POSIX even when a configuration is inspected on Windows.
    path.starts_with('/')
        .then(|| LocalEndpoint::Unix(PathBuf::from(path)))
}

fn context_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}

fn provider(name: &str) -> &str {
    if name == "orbstack" {
        "OrbStack"
    } else if name.starts_with("desktop-") {
        "Docker Desktop"
    } else if name.starts_with("colima") {
        "Colima"
    } else {
        "Docker"
    }
}

impl Discovery {
    fn read(&self) -> Vec<LocalEngine> {
        if self.platform == Platform::Unsupported {
            return vec![];
        }
        let mut contexts = Vec::new();
        let configured = self
            .config
            .as_ref()
            .and_then(|dir| json(&dir.join("config.json")))
            .and_then(|config| config["currentContext"].as_str().map(str::to_owned));
        let active = self.context.clone().or(configured);
        if let Some(config) = &self.config {
            if let Ok(entries) = std::fs::read_dir(config.join("contexts/meta")) {
                for entry in entries.take(1024).flatten() {
                    let Some(data) = json(&entry.path().join("meta.json")) else {
                        continue;
                    };
                    let Some(name) = data["Name"].as_str().filter(|name| context_name(name)) else {
                        continue;
                    };
                    let Some(uri) = data["Endpoints"]["docker"]["Host"].as_str() else {
                        continue;
                    };
                    let Some(endpoint) = endpoint(uri, self.platform) else {
                        continue;
                    };
                    let label = provider(name);
                    let name_label = if label == "OrbStack" || label == "Docker Desktop" {
                        label.to_owned()
                    } else {
                        format!("{label} · {name}")
                    };
                    contexts.push((
                        name.to_owned(),
                        LocalEngine {
                            id: format!("local/context/{name}"),
                            name: name_label,
                            endpoint,
                        },
                    ));
                }
            }
        }
        contexts.sort_by(|(a, _), (b, _)| {
            (Some(a) != active.as_ref())
                .cmp(&(Some(b) != active.as_ref()))
                .then(a.cmp(b))
        });
        let mut result = Vec::new();
        // DOCKER_CONTEXT takes precedence over DOCKER_HOST, like Docker CLI.
        if self.context.is_none() {
            if let Some(selected) = self
                .host
                .as_deref()
                .and_then(|host| endpoint(host, self.platform))
            {
                result.push(LocalEngine {
                    id: "local/environment".into(),
                    name: "Docker · DOCKER_HOST".into(),
                    endpoint: selected,
                });
            }
        }
        result.extend(contexts.into_iter().map(|(_, engine)| engine));
        let mut socket = |id: &str, name: &str, path: PathBuf| {
            if path.exists() {
                result.push(LocalEngine {
                    id: format!("local/{id}"),
                    name: name.into(),
                    endpoint: LocalEndpoint::Unix(path),
                });
            }
        };
        if self.platform != Platform::Windows {
            if let Some(home) = &self.home {
                socket(
                    "docker-desktop",
                    "Docker Desktop",
                    home.join(".docker/run/docker.sock"),
                );
                socket(
                    "orbstack",
                    "OrbStack",
                    home.join(".orbstack/run/docker.sock"),
                );
                if let Ok(profiles) = std::fs::read_dir(home.join(".colima")) {
                    let mut profiles: Vec<_> = profiles.take(256).flatten().collect();
                    profiles.sort_by_key(|entry| entry.file_name());
                    for profile in profiles {
                        let name = profile.file_name().to_string_lossy().into_owned();
                        if context_name(&name) {
                            socket(
                                &format!("colima/{name}"),
                                &format!("Colima · {name}"),
                                profile.path().join("docker.sock"),
                            );
                        }
                    }
                }
                socket("rancher", "Rancher Desktop", home.join(".rd/docker.sock"));
                socket(
                    "podman",
                    "Podman",
                    home.join(".local/share/containers/podman/machine/podman.sock"),
                );
            }
            if let Some(runtime) = &self.runtime {
                socket("rootless", "Docker · rootless", runtime.join("docker.sock"));
                socket(
                    "podman-runtime",
                    "Podman",
                    runtime.join("podman/podman.sock"),
                );
            }
            socket("system", "Docker", PathBuf::from("/var/run/docker.sock"));
        } else if result.is_empty() {
            result.push(LocalEngine {
                id: "local/docker-desktop".into(),
                name: "Docker Desktop".into(),
                endpoint: LocalEndpoint::NamedPipe(r"\\.\pipe\docker_engine".into()),
            });
        }
        let mut seen = HashSet::new();
        // Keep a diagnostic connection available even if the daemon is stopped.
        if result.is_empty() && self.platform != Platform::Windows {
            result.push(LocalEngine {
                id: "local/system".into(),
                name: "Docker".into(),
                endpoint: LocalEndpoint::Unix(PathBuf::from("/var/run/docker.sock")),
            });
        }
        for engine in &mut result {
            engine.id = engine.endpoint.stable_id();
        }
        result.retain(|engine| seen.insert(engine.id.clone()));
        result
    }
}

pub fn local_engines() -> Vec<LocalEngine> {
    let platform = platform();
    let home = if platform == Platform::Windows {
        std::env::var_os("USERPROFILE")
    } else {
        std::env::var_os("HOME")
    }
    .map(PathBuf::from);
    Discovery {
        platform,
        config: std::env::var_os("DOCKER_CONFIG")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| home.as_ref().map(|home| home.join(".docker"))),
        home,
        runtime: std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from),
        context: nonempty(std::env::var("DOCKER_CONTEXT").ok()),
        host: nonempty(std::env::var("DOCKER_HOST").ok()),
    }
    .read()
}

pub fn connector(endpoint: &LocalEndpoint) -> Result<SharedConnector, DockerError> {
    match endpoint {
        #[cfg(unix)]
        LocalEndpoint::Unix(path) => Ok(Arc::new(crate::UnixConnector { path: path.clone() })),
        #[cfg(windows)]
        LocalEndpoint::NamedPipe(path) => Ok(Arc::new(PipeConnector { path: path.clone() })),
        #[allow(unreachable_patterns)]
        _ => Err(DockerError::NotRunning),
    }
}

#[cfg(windows)]
#[derive(Debug)]
struct PipeConnector {
    path: String,
}

#[cfg(windows)]
#[async_trait]
impl DockerConnector for PipeConnector {
    async fn connect(&self) -> Result<DockerIo, DockerError> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            match tokio::net::windows::named_pipe::ClientOptions::new().open(&self.path) {
                Ok(pipe) => return Ok(Box::new(pipe)),
                Err(error)
                    if error.raw_os_error() == Some(231)
                        && tokio::time::Instant::now() < deadline =>
                {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                Err(error) => {
                    return Err(match error.kind() {
                        std::io::ErrorKind::PermissionDenied => DockerError::PermissionDenied,
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
                            DockerError::NotRunning
                        }
                        _ => DockerError::Connection,
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[tokio::test]
    async fn native_pipe_transports_http_and_opens_a_fresh_connection_per_request() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::windows::named_pipe::ServerOptions;
        let name = format!(
            r"\\.\pipe\consolecrypt-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let first = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&name)
            .unwrap();
        let server_name = name.clone();
        let server = tokio::spawn(async move {
            let mut next = Some(first);
            for request in 0..2 {
                let mut pipe = next.take().unwrap();
                pipe.connect().await.unwrap();
                if request == 0 {
                    next = Some(ServerOptions::new().create(&server_name).unwrap());
                }
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    let byte = pipe.read_u8().await.unwrap();
                    header.push(byte);
                    assert!(header.len() < 8192);
                }
                let header = String::from_utf8(header).unwrap();
                let expected = if request == 0 {
                    "GET /containers/json?all=1 HTTP/1.1\r\n"
                } else {
                    "GET /containers/json HTTP/1.1\r\n"
                };
                assert!(header.starts_with(expected), "{header}");
                pipe.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]").await.unwrap();
                pipe.flush().await.unwrap();
            }
        });
        let client = crate::DockerClient::new(connector(&LocalEndpoint::NamedPipe(name)).unwrap());
        for all in [true, false] {
            let containers =
                tokio::time::timeout(std::time::Duration::from_secs(5), client.containers(all))
                    .await
                    .unwrap()
                    .unwrap();
            assert!(containers.is_empty());
        }
        server.await.unwrap();
    }

    fn settings(root: &Path, platform: Platform) -> Discovery {
        Discovery {
            platform,
            home: Some(root.into()),
            config: Some(root.join(".docker")),
            runtime: None,
            context: None,
            host: None,
        }
    }

    fn context(root: &Path, key: &str, name: &str, host: &str) {
        let dir = root.join(".docker/contexts/meta").join(key);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("meta.json"),
            serde_json::to_vec(&serde_json::json!({
                "Name": name, "Endpoints": {"docker": {"Host": host}}
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn windows_pipe_is_local_only_and_round_trips() {
        let pipe = endpoint(
            "npipe:////./pipe/dockerDesktopLinuxEngine",
            Platform::Windows,
        )
        .unwrap();
        assert_eq!(pipe.uri(), "npipe:////./pipe/dockerDesktopLinuxEngine");
        for value in [
            "npipe:////server/pipe/docker",
            "npipe:////./pipe/../other",
            "tcp://localhost:2375",
            "ssh://prod",
            "unix:///tmp/docker.sock",
        ] {
            assert!(endpoint(value, Platform::Windows).is_none(), "{value}");
        }
        assert!(endpoint("unix://relative/docker.sock", Platform::Mac).is_none());
    }

    #[test]
    fn active_context_is_first_and_remote_contexts_never_appear_local() {
        let root = tempfile::tempdir().unwrap();
        context(
            root.path(),
            "one",
            "desktop-linux",
            "unix:///test/desktop.sock",
        );
        context(root.path(), "two", "orbstack", "unix:///test/orbstack.sock");
        context(root.path(), "three", "production", "ssh://root@production");
        context(root.path(), "four", "tcp", "tcp://127.0.0.1:2375");
        std::fs::write(
            root.path().join(".docker/config.json"),
            r#"{"currentContext":"orbstack"}"#,
        )
        .unwrap();
        let engines = settings(root.path(), Platform::Mac).read();
        assert_eq!(engines[0].name, "OrbStack");
        assert_eq!(engines[1].name, "Docker Desktop");
        assert!(!engines
            .iter()
            .any(|engine| engine.id.contains("production") || engine.id.ends_with("/tcp")));
    }

    #[test]
    fn context_environment_overrides_host_and_duplicates_are_removed() {
        let root = tempfile::tempdir().unwrap();
        context(
            root.path(),
            "one",
            "desktop-linux",
            "unix:///test/desktop.sock",
        );
        context(root.path(), "two", "orbstack", "unix:///test/orbstack.sock");
        context(root.path(), "three", "alias", "unix:///test/orbstack.sock");
        let mut config = settings(root.path(), Platform::Mac);
        config.context = Some("orbstack".into());
        config.host = Some("unix:///test/ignored.sock".into());
        let engines = config.read();
        assert_eq!(engines[0].name, "OrbStack");
        assert_eq!(
            engines
                .iter()
                .filter(|engine| engine.endpoint.uri() == "unix:///test/orbstack.sock")
                .count(),
            1
        );
        assert!(!engines
            .iter()
            .any(|engine| engine.name.contains("DOCKER_HOST")));
    }

    #[test]
    fn windows_desktop_pipe_remains_available_for_stopped_daemon_diagnostics() {
        let root = tempfile::tempdir().unwrap();
        let engines = settings(root.path(), Platform::Windows).read();
        assert_eq!(engines.len(), 1);
        assert_eq!(engines[0].name, "Docker Desktop");
        assert_eq!(engines[0].endpoint.uri(), "npipe:////./pipe/docker_engine");
        assert!(settings(root.path(), Platform::Unsupported)
            .read()
            .is_empty());
    }

    #[test]
    fn all_colima_profiles_are_distinct_engines() {
        let root = tempfile::tempdir().unwrap();
        for profile in ["default", "work"] {
            let path = root.path().join(".colima").join(profile);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("docker.sock"), []).unwrap();
        }
        let engines = settings(root.path(), Platform::Mac).read();
        for profile in ["default", "work"] {
            let endpoint = LocalEndpoint::Unix(
                root.path()
                    .join(".colima")
                    .join(profile)
                    .join("docker.sock"),
            );
            assert!(engines
                .iter()
                .any(|engine| engine.id == endpoint.stable_id()));
        }
        assert_ne!(engines[0].id, engines[1].id);
    }

    #[test]
    fn endpoint_identity_survives_context_alias_changes_but_not_host_retargeting() {
        let root = tempfile::tempdir().unwrap();
        context(root.path(), "one", "orbstack", "unix:///test/orbstack.sock");
        context(root.path(), "two", "alias", "unix:///test/orbstack.sock");
        let mut config = settings(root.path(), Platform::Mac);
        config.context = Some("orbstack".into());
        let original = config.read()[0].id.clone();
        config.context = Some("alias".into());
        assert_eq!(config.read()[0].id, original);
        config.context = None;
        config.host = Some("unix:///test/orbstack.sock".into());
        assert_eq!(config.read()[0].id, original);
        config.host = Some("unix:///test/another-engine.sock".into());
        assert_ne!(config.read()[0].id, original);
    }
}
