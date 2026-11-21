use crate::{
    models::*,
    runtime::{parser, process},
};
use serde_json::Value;
use std::{future::Future, path::PathBuf, time::Duration};

pub trait Runtime: Send + Sync {
    fn list_containers(&self) -> impl Future<Output = AppResult<Vec<ContainerSummary>>> + Send;
    fn inspect_container(
        &self,
        id: &str,
        reveal: bool,
    ) -> impl Future<Output = AppResult<ContainerDetails>> + Send;
    fn mutate(
        &self,
        id: &str,
        action: ContainerAction,
        budget: Duration,
    ) -> impl Future<Output = AppResult<()>> + Send;
    fn remove(&self, id: &str, force: bool) -> impl Future<Output = AppResult<()>> + Send;
}

/// Lists one directory inside a container without a shell on the Windows side. The path is
/// a positional argument, never part of the script. `[ -d ]` follows links to directories.
pub const LIST_FILES_SCRIPT: &str = r#"cd "$1" || exit 2
for f in * .*; do
  case "$f" in .|..) continue;; esac
  [ -e "$f" ] || [ -L "$f" ] || continue
  if [ -d "$f" ]; then k=d; else k=f; fi
  if [ -L "$f" ]; then l=l; else l=-; fi
  printf '%s%s\t%s\n' "$k" "$l" "$f"
done"#;

#[derive(Debug, Clone)]
pub struct WslcCliAdapter {
    pub path: PathBuf,
}

/// Releases whose output was captured and checked against the parsers.
pub const VERIFIED_VERSIONS: &[&str] = &["3.0.1.0", "3.0.3.0"];
const MINIMUM_VERSION: [u32; 4] = [3, 0, 1, 0];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionSupport {
    Verified,
    /// Same major line, newer than the minimum. The strict parsers still fail closed on any
    /// changed field type, so reads and full-ID mutations remain safe.
    Compatible,
    Unsupported,
}

pub fn version_support(output: &str) -> VersionSupport {
    let Some(raw) = output.trim().strip_prefix("wslc ") else {
        return VersionSupport::Unsupported;
    };
    if VERIFIED_VERSIONS.contains(&raw) {
        return VersionSupport::Verified;
    }
    let parts: Vec<u32> = match raw.split('.').map(str::parse).collect() {
        Ok(parts) => parts,
        Err(_) => return VersionSupport::Unsupported,
    };
    if parts.len() != 4 || parts[0] != MINIMUM_VERSION[0] || parts[..] < MINIMUM_VERSION[..] {
        return VersionSupport::Unsupported;
    }
    VersionSupport::Compatible
}

pub fn validate_name(name: &str, operation: &str) -> AppResult<()> {
    if name.is_empty()
        || name.len() > 255
        || name.starts_with('-')
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c))
    {
        return Err(AppError::new(
            "invalid_name",
            operation,
            "Некорректное имя ресурса",
        ));
    }
    Ok(())
}

pub fn validate_id(id: &str) -> AppResult<()> {
    if id.len() != 64 || !id.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::new(
            "invalid_id",
            "container",
            "Требуется полный ID контейнера из runtime (64 hex-символа)",
        ));
    }
    Ok(())
}

pub fn list_args(kind: &str) -> Vec<String> {
    let args = match kind {
        "container" => vec![
            "container",
            "list",
            "--all",
            "--no-trunc",
            "--format",
            "json",
        ],
        "image" => vec!["image", "list", "--all", "--no-trunc", "--format", "json"],
        "volume" => vec!["volume", "list", "--format", "json"],
        "network" => vec!["network", "list", "--no-trunc", "--format", "json"],
        _ => unreachable!("internal list kind"),
    };
    args.into_iter().map(String::from).collect()
}

impl WslcCliAdapter {
    pub fn new(path: Option<&str>) -> AppResult<Self> {
        Ok(Self {
            path: process::resolve_cli(path)?,
        })
    }

    pub async fn version(&self) -> AppResult<String> {
        process::execute(
            &self.path,
            &["version".into()],
            Duration::from_secs(60),
            "version",
        )
        .await
    }

    pub async fn probe(&self) -> AppResult<RuntimeInfo> {
        // The first runtime read may have to start the WSLc session (several seconds).
        // Overlap it with the version query; the list is read-only and discarded if unsupported.
        let (version, containers) = tokio::join!(
            self.version(),
            self.read_containers(Duration::from_secs(60))
        );
        let version = version?;
        let support = version_support(&version);
        let result = if support == VersionSupport::Unsupported {
            Err(AppError::new(
                "unsupported_version",
                "probe",
                "Эта версия WSLc не поддерживается: нужна линейка 3.x не ниже 3.0.1",
            ))
        } else {
            containers.map(|_| ())
        };
        Ok(RuntimeInfo {
            cli_path: self.path.to_string_lossy().into(),
            cli_version: version.trim().into(),
            version_supported: support != VersionSupport::Unsupported,
            version_verified: support == VersionSupport::Verified,
            readable: result.is_ok(),
            error: result.err(),
        })
    }

    async fn read_containers(&self, budget: Duration) -> AppResult<Vec<ContainerSummary>> {
        parser::containers(
            &process::execute(
                &self.path,
                &list_args("container"),
                budget,
                "list_containers",
            )
            .await?,
        )
    }

    pub async fn list_images(&self) -> AppResult<Vec<ImageSummary>> {
        parser::images(
            &process::execute(
                &self.path,
                &list_args("image"),
                Duration::from_secs(15),
                "list_images",
            )
            .await?,
        )
    }

    pub async fn list_volumes(&self) -> AppResult<Vec<VolumeSummary>> {
        parser::volumes(
            &process::execute(
                &self.path,
                &list_args("volume"),
                Duration::from_secs(15),
                "list_volumes",
            )
            .await?,
        )
    }

    pub async fn list_networks(&self) -> AppResult<Vec<NetworkSummary>> {
        parser::networks(
            &process::execute(
                &self.path,
                &list_args("network"),
                Duration::from_secs(15),
                "list_networks",
            )
            .await?,
        )
    }

    pub async fn inspect_network(&self, name: &str) -> AppResult<String> {
        // Since 3.0.3 the argument is a network name. The default bridge network is also
        // recreated with a new ID whenever the WSLc session restarts, so IDs go stale.
        validate_name(name, "inspect_network")?;
        let text = process::execute(
            &self.path,
            &["network".into(), "inspect".into(), name.into()],
            Duration::from_secs(15),
            "inspect_network",
        )
        .await?;
        let value = parser::inspect_document(&text, "inspect_network")?;
        if value.get("Name").and_then(|n| n.as_str()) != Some(name) {
            return Err(AppError::new(
                "identity_mismatch",
                "inspect_network",
                "Runtime вернул другую сеть",
            ));
        }
        serde_json::to_string_pretty(&value).map_err(|_| {
            AppError::new(
                "invalid_output",
                "inspect_network",
                "Повреждён Inspect сети",
            )
        })
    }

    pub async fn inspect_readonly(&self, kind: &str, id: &str, reveal: bool) -> AppResult<String> {
        if kind == "image" {
            validate_id(id.strip_prefix("sha256:").unwrap_or(id))?;
        }
        if kind == "volume" {
            validate_name(id, "inspect_volume")?;
        }
        if !matches!(kind, "image" | "volume") {
            return Err(AppError::new(
                "unsupported",
                "inspect",
                "Операция недоступна",
            ));
        }
        let text = process::execute(
            &self.path,
            &[kind.into(), "inspect".into(), id.into()],
            Duration::from_secs(15),
            "inspect",
        )
        .await?;
        let mut v = parser::inspect_document(&text, "inspect")?;
        if !reveal {
            parser::redact_environment(&mut v);
        }
        serde_json::to_string_pretty(&v)
            .map_err(|_| AppError::new("invalid_output", "inspect", "Не удалось прочитать Inspect"))
    }
}

impl WslcCliAdapter {
    pub async fn container_stats(&self) -> AppResult<Vec<ContainerStats>> {
        // A snapshot samples CPU for about two seconds.
        let args = ["stats", "--no-trunc", "--format", "json"].map(String::from);
        parser::stats(
            &process::execute(
                &self.path,
                &args,
                Duration::from_secs(20),
                "container_stats",
            )
            .await?,
        )
    }

    pub async fn list_files(&self, id: &str, path: &str) -> AppResult<Vec<FileEntry>> {
        validate_id(id)?;
        crate::desktop::validate_container_path(path)?;
        let args = [
            "exec",
            id,
            "sh",
            "-c",
            LIST_FILES_SCRIPT,
            "wslc-desk-ls",
            path,
        ]
        .map(String::from);
        match process::execute(&self.path, &args, Duration::from_secs(15), "list_files").await {
            Ok(text) => Ok(parser::file_entries(&text)),
            Err(mut e) if e.code == "runtime" => {
                e.message =
                    "Каталог недоступен: контейнер должен быть запущен, а путь — существовать"
                        .into();
                Err(e)
            }
            Err(e) => Err(e),
        }
    }

    /// Starts (or only creates) a container; returns its full ID. A missing image is pulled,
    /// so the budget is generous. Environment values travel in a temporary file.
    pub async fn run_container(&self, spec: &RunSpec) -> AppResult<String> {
        crate::runspec::run_args(spec, None)?;
        let env_file = if spec.env.is_empty() {
            None
        } else {
            let body = zeroize::Zeroizing::new(
                spec.env
                    .iter()
                    .map(|e| format!("{}={}\n", e.key, e.value))
                    .collect::<String>(),
            );
            Some(super::env_file::create(body.as_bytes())?)
        };
        let args = crate::runspec::run_args(
            spec,
            env_file.as_ref().map(|p| p.to_string_lossy()).as_deref(),
        );
        let result = match args {
            Ok(args) => process::execute(&self.path, &args, Duration::from_secs(600), "run").await,
            Err(e) => Err(e),
        };
        drop(env_file);
        crate::runspec::created_id(&result?)
    }

    /// Container and image Inspect documents; the image is optional context.
    async fn inspect_with_image(&self, id: &str) -> AppResult<(Value, Option<Value>)> {
        validate_id(id)?;
        let text = process::execute(
            &self.path,
            &["container".into(), "inspect".into(), id.into()],
            Duration::from_secs(15),
            "inspect_with_image",
        )
        .await?;
        let container = parser::inspect_document(&text, "inspect_with_image")?;
        let image = match container["Image"].as_str() {
            Some(image_id) if validate_id(image_id.trim_start_matches("sha256:")).is_ok() => {
                process::execute(
                    &self.path,
                    &["image".into(), "inspect".into(), image_id.into()],
                    Duration::from_secs(15),
                    "inspect_with_image",
                )
                .await
                .ok()
                .and_then(|t| parser::inspect_document(&t, "inspect_with_image").ok())
            }
            _ => None,
        };
        Ok((container, image))
    }

    /// Run spec of an existing container; image defaults are subtracted when available.
    pub async fn container_run_spec(&self, id: &str, include_env: bool) -> AppResult<RunSpec> {
        let (container, image) = self.inspect_with_image(id).await?;
        Ok(crate::runspec::spec_from_inspect(
            &container,
            image.as_ref(),
            include_env,
        ))
    }

    /// Environment with values; read only when the user opens the tab.
    pub async fn container_env(&self, id: &str) -> AppResult<Vec<EnvEntry>> {
        let (container, image) = self.inspect_with_image(id).await?;
        Ok(crate::runspec::env_entries(&container, image.as_ref()))
    }

    pub async fn remove_image(&self, id: &str) -> AppResult<()> {
        crate::runspec::image_ref(id)?;
        let args = ["image", "remove", id].map(String::from);
        process::execute(&self.path, &args, Duration::from_secs(60), "remove_image").await?;
        Ok(())
    }

    pub async fn tag_image(&self, source: &str, target: &str) -> AppResult<()> {
        let args = crate::runspec::tag_args(source, target)?;
        process::execute(&self.path, &args, Duration::from_secs(30), "tag_image").await?;
        Ok(())
    }

    pub async fn registry_login(
        &self,
        server: Option<&str>,
        username: &str,
        password: &str,
    ) -> AppResult<()> {
        let args = crate::runspec::login_args(server, username)?;
        if password.is_empty() || password.contains(['\n', '\r', '\0']) {
            return Err(AppError::new(
                "invalid_password",
                "login",
                "Введите пароль или токен доступа",
            ));
        }
        process::run_bytes(
            &self.path,
            &args,
            Some(password.as_bytes()),
            Duration::from_secs(60),
            "login",
            64 * 1024,
        )
        .await
        .map(drop)
    }

    pub async fn registry_logout(&self, server: Option<&str>) -> AppResult<()> {
        let args = crate::runspec::logout_args(server)?;
        process::execute(&self.path, &args, Duration::from_secs(30), "logout").await?;
        Ok(())
    }

    /// Relations of every container, inspected in batches.
    /// Inspect output of every container, a batch of IDs per call.
    async fn inspect_all(&self, operation: &str) -> AppResult<Vec<String>> {
        // IDs become arguments, so each one is checked like any other user-facing ID.
        let ids = self
            .read_containers(Duration::from_secs(15))
            .await?
            .into_iter()
            .map(|c| validate_id(&c.id).map(|()| c.id))
            .collect::<AppResult<Vec<_>>>()?;
        let mut texts = Vec::new();
        for batch in ids.chunks(crate::topology::BATCH) {
            let mut args = vec!["container".to_owned(), "inspect".to_owned()];
            args.extend(batch.iter().cloned());
            texts.push(
                process::execute(&self.path, &args, Duration::from_secs(30), operation).await?,
            );
        }
        Ok(texts)
    }

    pub async fn container_links(&self) -> AppResult<Vec<ContainerLinks>> {
        let mut links = Vec::new();
        for text in self.inspect_all("topology").await? {
            links.extend(crate::topology::parse(&text)?);
        }
        Ok(links)
    }

    pub async fn compose_containers(&self, project: &str) -> AppResult<Vec<ComposeContainer>> {
        let mut found = Vec::new();
        for text in self.inspect_all("compose").await? {
            let documents = crate::topology::inspect_many(&text)?;
            found.extend(crate::compose::containers(&documents, project));
        }
        Ok(found)
    }

    /// Short management commands that print nothing useful on success.
    pub async fn run_simple(&self, args: AppResult<Vec<String>>, operation: &str) -> AppResult<()> {
        process::execute(&self.path, &args?, Duration::from_secs(60), operation).await?;
        Ok(())
    }

    pub async fn remove_volume(&self, name: &str) -> AppResult<()> {
        validate_name(name, "remove_volume")?;
        let args = ["volume", "remove", name].map(String::from);
        process::execute(&self.path, &args, Duration::from_secs(60), "remove_volume").await?;
        Ok(())
    }
}

impl Runtime for WslcCliAdapter {
    async fn list_containers(&self) -> AppResult<Vec<ContainerSummary>> {
        self.read_containers(Duration::from_secs(15)).await
    }

    async fn inspect_container(&self, id: &str, reveal: bool) -> AppResult<ContainerDetails> {
        validate_id(id)?;
        let text = process::execute(
            &self.path,
            &["container".into(), "inspect".into(), id.into()],
            Duration::from_secs(15),
            "inspect_container",
        )
        .await?;
        let details = parser::inspect_container(&text, reveal)?;
        if details.id != id {
            return Err(AppError::new(
                "identity_mismatch",
                "inspect_container",
                "Runtime вернул другой ID",
            ));
        }
        Ok(details)
    }

    async fn mutate(&self, id: &str, action: ContainerAction, budget: Duration) -> AppResult<()> {
        validate_id(id)?;
        process::execute(
            &self.path,
            &["container".into(), action.command().into(), id.into()],
            budget,
            action.command(),
        )
        .await?;
        Ok(())
    }

    async fn remove(&self, id: &str, force: bool) -> AppResult<()> {
        validate_id(id)?;
        let mut args = vec!["container".to_owned(), "remove".to_owned()];
        if force {
            args.push("--force".into());
        }
        args.push(id.into());
        process::execute(&self.path, &args, Duration::from_secs(90), "remove").await?;
        Ok(())
    }
}
