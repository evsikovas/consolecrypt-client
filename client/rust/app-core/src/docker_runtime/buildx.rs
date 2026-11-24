use super::invalid;
use crate::{AppResult, docker_compose::LocalComposeHost};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
};
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BuildOptions {
    directory: PathBuf,
    platforms: Vec<String>,
}

impl BuildOptions {
    pub(super) async fn publish(
        self,
        endpoint: &str,
        registry: &str,
        repository: &str,
        tag: &str,
        username: &str,
        password: &str,
    ) -> AppResult<String> {
        let repo = cc_docker_core::registry_repository(registry, repository, tag)
            .ok_or_else(|| invalid("registry_credential"))?;
        if self.platforms.is_empty()
            || self.platforms.len() > 2
            || self
                .platforms
                .iter()
                .any(|p| !matches!(p.as_str(), "linux/amd64" | "linux/arm64"))
            || self.platforms.len() == 2 && self.platforms[0] == self.platforms[1]
            || !self.directory.is_absolute()
        {
            return Err(invalid("build_context"));
        }
        let directory =
            dunce::canonicalize(&self.directory).map_err(|_| invalid("build_context"))?;
        if !directory.join("Dockerfile").is_file() {
            return Err(invalid("build_context"));
        }
        let docker =
            LocalComposeHost::program("docker").ok_or_else(|| invalid("buildx_unavailable"))?;
        let temp = tempfile::Builder::new()
            .prefix("cc-buildx-")
            .tempdir()
            .map_err(|_| invalid("build_context"))?;
        // The registry config must never be included in the build context.
        let config = dunce::canonicalize(temp.path()).map_err(|_| invalid("build_context"))?;
        if config.starts_with(&directory) {
            return Err(invalid("build_context"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| invalid("build_context"))?;
        }
        let (authority, _) = repo
            .split_once('/')
            .ok_or_else(|| invalid("registry_credential"))?;
        let authority = if authority == "docker.io" {
            "https://index.docker.io/v1/"
        } else {
            authority
        };
        let mut plugins = Vec::new();
        if let Some(settings) = std::env::var_os("DOCKER_CONFIG")
            .map(PathBuf::from)
            .or_else(|| std::env::home_dir().map(|p| p.join(".docker")))
        {
            if settings.is_absolute() {
                plugins.push(settings.join("cli-plugins"));
            }
        }
        if let Some(resources) = docker.parent().and_then(Path::parent) {
            plugins.push(resources.join("cli-plugins"));
        }
        #[cfg(target_os = "macos")]
        plugins.push(PathBuf::from(
            "/Applications/Docker.app/Contents/Resources/cli-plugins",
        ));
        // Use only the selected account. Neither argv nor the user's Docker config receives its token.
        let credentials = Zeroizing::new(format!("{username}:{password}"));
        let auth = Zeroizing::new(STANDARD.encode(credentials.as_bytes()));
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct DockerConfig<'a> {
            auths: BTreeMap<&'a str, BTreeMap<&'static str, &'a str>>,
            cli_plugins_extra_dirs: Vec<PathBuf>,
        }
        let settings = serde_json::to_vec(&DockerConfig {
            auths: BTreeMap::from([(authority, BTreeMap::from([("auth", auth.as_str())]))]),
            cli_plugins_extra_dirs: plugins,
        })
        .map(Zeroizing::new)
        .map_err(|_| invalid("build_context"))?;
        std::fs::write(config.join("config.json"), &*settings)
            .map_err(|_| invalid("build_context"))?;
        let metadata = config.join("result.json");
        let mut command = tokio::process::Command::new(docker);
        command
            .env("DOCKER_CONFIG", &config)
            .env("DOCKER_HOST", endpoint)
            .env_remove("DOCKER_CONTEXT")
            .env_remove("DOCKER_TLS_VERIFY")
            .env_remove("DOCKER_CERT_PATH")
            .env_remove("BUILDX_BUILDER")
            .env_remove("BUILDKIT_HOST")
            .env_remove("DOCKER_AUTH_CONFIG")
            .args([
                "buildx",
                "build",
                "--platform",
                &self.platforms.join(","),
                "-t",
                &format!("{repo}:{tag}"),
                "--push",
                "--metadata-file",
            ])
            .arg(&metadata)
            .arg(&directory)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let status = command
            .status()
            .await
            .map_err(|_| invalid("buildx_unavailable"))?;
        if !status.success() {
            return Err(invalid("buildx_failed"));
        }
        let bytes = std::fs::read(metadata).map_err(|_| invalid("buildx_result"))?;
        let result: Value = serde_json::from_slice(&bytes).map_err(|_| invalid("buildx_result"))?;
        let digest = result["containerimage.digest"]
            .as_str()
            .and_then(|s| s.strip_prefix("sha256:"))
            .filter(|s| {
                s.len() == 64
                    && s.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
            .ok_or_else(|| invalid("buildx_result"))?;
        Ok(format!("{repo}@sha256:{digest}"))
    }
}
