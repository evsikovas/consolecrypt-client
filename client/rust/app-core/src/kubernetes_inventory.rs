//! Explicit inventory edits and filesystem import/export boundaries.
use crate::{dto::parse_id, AppCore, AppError, AppResult};
use base64::{engine::general_purpose::STANDARD, Engine};
use cc_models::kubernetes::{KubeAuth, KubeContext};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};
use zeroize::{Zeroize, Zeroizing};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KubeContextUpdate {
    pub name: String,
    pub namespace: String,
    pub production: bool,
    pub allow_exec_in_prod: bool,
    pub allow_port_forward_in_prod: bool,
    pub record_events: bool,
}
fn invalid(code: &str) -> AppError {
    AppError::invalid("kubernetes_inventory", code)
}

struct ProtectedJson(Value);
impl Drop for ProtectedJson {
    fn drop(&mut self) {
        wipe_json(&mut self.0);
    }
}
pub(crate) fn wipe_json(value: &mut Value) {
    match value {
        Value::String(value) => value.zeroize(),
        Value::Array(values) => values.iter_mut().for_each(wipe_json),
        Value::Object(values) => values.values_mut().for_each(wipe_json),
        _ => {}
    }
}

impl AppCore {
    pub async fn kube_context_favourites(
        &self,
        id: String,
        namespaces: Vec<String>,
        kinds: Vec<String>,
    ) -> AppResult<()> {
        let (_, unlocked) = self.unlocked().await?;
        let id = parse_id("context_id", &id)?;
        let mut context = unlocked
            .working()
            .kube_context(id)
            .ok_or_else(|| AppError::not_found("kube_context", id))?;
        context.favourite_namespaces = namespaces;
        context.favourite_kinds = kinds;
        context.validate()?;
        context.updated_at = chrono::Utc::now();
        unlocked
            .writer
            .put(cc_models::VaultObject::KubeContext(context))
            .await?;
        Ok(())
    }

    pub async fn kube_cluster_labels(
        &self,
        context_id: String,
        tags: Vec<String>,
        color: Option<String>,
    ) -> AppResult<()> {
        let (_, unlocked) = self.unlocked().await?;
        let id = parse_id("context_id", &context_id)?;
        let context = unlocked
            .working()
            .kube_context(id)
            .ok_or_else(|| AppError::not_found("kube_context", id))?;
        let mut cluster = unlocked
            .working()
            .kube_cluster(context.cluster_id)
            .ok_or_else(|| invalid("cluster_unavailable"))?;
        cluster.tags = tags;
        cluster.color = color;
        cluster.validate()?;
        cluster.updated_at = chrono::Utc::now();
        unlocked
            .writer
            .put(cc_models::VaultObject::KubeCluster(cluster))
            .await?;
        Ok(())
    }

    pub async fn kube_context_update(
        &self,
        id: String,
        update: KubeContextUpdate,
    ) -> AppResult<KubeContext> {
        let (_, unlocked) = self.unlocked().await?;
        let id = parse_id("context_id", &id)?;
        let mut context = unlocked
            .working()
            .kube_context(id)
            .ok_or_else(|| AppError::not_found("kube_context", id))?;
        context.name = update.name;
        context.namespace = update.namespace;
        context.production = update.production;
        context.allow_exec_in_prod = update.allow_exec_in_prod;
        context.allow_port_forward_in_prod = update.allow_port_forward_in_prod;
        context.event_history_enabled = update.record_events;
        context.updated_at = chrono::Utc::now();
        let cluster = unlocked
            .working()
            .kube_cluster(context.cluster_id)
            .ok_or_else(|| invalid("cluster_unavailable"))?;
        context.validate_cluster(&cluster)?;
        unlocked
            .writer
            .put(cc_models::VaultObject::KubeContext(context.clone()))
            .await?;
        Ok(context)
    }

    pub async fn kube_scan_config_files(&self, directory: String) -> AppResult<Vec<String>> {
        let (owner, unlocked) = self.unlocked().await?;
        let paths = tokio::task::spawn_blocking(move || scan(Path::new(&directory)))
            .await
            .map_err(|_| invalid("scan_failed"))??;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        Ok(paths)
    }

    pub async fn kube_export_context(
        &self,
        id: String,
        path: String,
        include_credentials: bool,
    ) -> AppResult<()> {
        let (owner, unlocked) = self.unlocked().await?;
        let id = parse_id("context_id", &id)?;
        let context = unlocked
            .working()
            .kube_context(id)
            .ok_or_else(|| AppError::not_found("kube_context", id))?;
        let cluster = unlocked
            .working()
            .kube_cluster(context.cluster_id)
            .ok_or_else(|| invalid("cluster_unavailable"))?;
        let destination = export_destination(Path::new(&path))?;
        let mut user = ProtectedJson(json!({}));
        if include_credentials {
            match &context.auth {
                KubeAuth::None => {}
                KubeAuth::Token { secret_id } => {
                    let token = unlocked.writer.read_secret(*secret_id).await?;
                    user.0["token"] = token.value.expose_secret().into();
                }
                KubeAuth::ClientCertificate {
                    certificate_pem,
                    key_secret_id,
                } => {
                    let key = unlocked.writer.read_secret(*key_secret_id).await?;
                    user.0["client-certificate-data"] = STANDARD.encode(certificate_pem).into();
                    user.0["client-key-data"] = STANDARD.encode(key.value.expose_secret()).into();
                }
                KubeAuth::Exec {
                    command,
                    args_secret_id,
                    env,
                    api_version,
                    provide_cluster_info,
                    interactive_mode,
                } => {
                    let args = unlocked.writer.read_secret(*args_secret_id).await?;
                    let args: Value = serde_json::from_str(args.value.expose_secret())
                        .map_err(|_| invalid("invalid_exec"))?;
                    user.0["exec"] = json!({
                        "command":command,"args":args,"apiVersion":api_version,
                        "provideClusterInfo":provide_cluster_info,
                        "interactiveMode":match interactive_mode {
                            cc_models::kubernetes::ExecInteractiveMode::Never => "Never",
                            cc_models::kubernetes::ExecInteractiveMode::IfAvailable => "IfAvailable",
                            cc_models::kubernetes::ExecInteractiveMode::Always => "Always",
                        },"env":[],
                    });
                    for (name, id) in env {
                        let value = unlocked.writer.read_secret(*id).await?;
                        user.0["exec"]["env"]
                            .as_array_mut()
                            .ok_or_else(|| invalid("invalid_exec"))?
                            .push(json!({"name":name,"value":value.value.expose_secret()}));
                    }
                }
                KubeAuth::Oidc {
                    issuer,
                    client_id,
                    refresh_secret_id,
                    client_secret_id,
                    certificate_authority_pem,
                    scopes,
                } => {
                    user.0["auth-provider"] = json!({"name":"oidc","config":{
                        "idp-issuer-url":issuer,"client-id":client_id,"extra-scopes":scopes.join(","),
                    }});
                    let config = &mut user.0["auth-provider"]["config"];
                    for (key, id) in [
                        ("refresh-token", refresh_secret_id),
                        ("client-secret", client_secret_id),
                    ] {
                        if let Some(id) = id {
                            let secret = unlocked.writer.read_secret(*id).await?;
                            config[key] = secret.value.expose_secret().into();
                        }
                    }
                    if let Some(ca) = certificate_authority_pem {
                        config["idp-certificate-authority-data"] = STANDARD.encode(ca).into();
                    }
                }
            }
        }
        let mut document = ProtectedJson(json!({
            "apiVersion":"v1","kind":"Config","current-context":context.name,
            "clusters":[{"name":cluster.name,"cluster":{"server":cluster.server}}],
            "contexts":[{"name":context.name,"context":{"cluster":cluster.name,"user":context.name,"namespace":context.namespace}}],
            "users":[{"name":context.name,"user":user.0.take()}],
        }));
        let config = &mut document.0["clusters"][0]["cluster"];
        if let Some(ca) = &cluster.certificate_authority_pem {
            config["certificate-authority-data"] = STANDARD.encode(ca).into();
        }
        if let Some(name) = &cluster.tls_server_name {
            config["tls-server-name"] = name.as_str().into();
        }
        if cluster.insecure_skip_tls_verify {
            config["insecure-skip-tls-verify"] = true.into();
        }
        let bytes = Zeroizing::new(
            serde_json::to_vec_pretty(&document.0).map_err(|_| invalid("export_failed"))?,
        );
        // A restrictive staging directory gives Windows an inherited private
        // ACL before the first plaintext byte is written.
        let parent = destination
            .parent()
            .ok_or_else(|| invalid("invalid_path"))?;
        let directory = tempfile::Builder::new()
            .prefix(".consolecrypt-export-")
            .tempdir_in(parent)
            .map_err(|_| invalid("export_failed"))?;
        crate::kubernetes_proxy::restrict_directory(directory.path()).await?;
        let mut file = tempfile::NamedTempFile::new_in(directory.path())
            .map_err(|_| invalid("export_failed"))?;
        file.write_all(&bytes)
            .and_then(|_| file.as_file().sync_all())
            .map_err(|_| invalid("export_failed"))?;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        // Never truncate or replace another file (including symlinks).
        file.persist_noclobber(destination)
            .map_err(|_| invalid("destination_exists_or_unwritable"))?;
        Ok(())
    }
}

fn export_destination(path: &Path) -> AppResult<PathBuf> {
    if !path.is_absolute() || path.file_name().is_none() {
        return Err(invalid("invalid_path"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid("invalid_path"))?
        .canonicalize()
        .map_err(|_| invalid("invalid_path"))?;
    let destination = parent.join(path.file_name().ok_or_else(|| invalid("invalid_path"))?);
    if destination
        .try_exists()
        .map_err(|_| invalid("invalid_path"))?
    {
        return Err(invalid("destination_exists_or_unwritable"));
    }
    // Keep the system client's active configuration untouched.
    if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        let active = PathBuf::from(home).join(".kube");
        if active
            .canonicalize()
            .ok()
            .is_some_and(|p| p.join("config") == destination)
        {
            return Err(invalid("active_kubeconfig"));
        }
    }
    Ok(destination)
}

fn scan(root: &Path) -> AppResult<Vec<String>> {
    if !root.is_absolute() {
        return Err(invalid("invalid_directory"));
    }
    let root = root
        .canonicalize()
        .map_err(|_| invalid("invalid_directory"))?;
    if !root.is_dir() {
        return Err(invalid("invalid_directory"));
    }
    let mut queue = vec![(root, 0)];
    let mut entries = 0;
    let mut bytes = 0;
    let mut found = Vec::new();
    while let Some((directory, depth)) = queue.pop() {
        let Ok(children) = std::fs::read_dir(directory) else {
            continue;
        };
        for child in children {
            entries += 1;
            if entries > 4096 || bytes > 64 * 1024 * 1024 || found.len() >= 128 {
                found.sort();
                return Ok(found);
            }
            let Ok(child) = child else {
                continue;
            };
            let name = child.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let Ok(kind) = child.file_type() else {
                continue;
            };
            if kind.is_dir() && depth < 2 {
                queue.push((child.path(), depth + 1));
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            let candidate = name == "config"
                || name.contains("kubeconfig")
                || name.ends_with(".yaml")
                || name.ends_with(".yml")
                || name.ends_with(".json");
            if !candidate {
                continue;
            }
            let Ok(metadata) = child.metadata() else {
                continue;
            };
            if metadata.len() > cc_k8s_core::import::MAX_CONFIG_BYTES as u64 {
                continue;
            }
            let Ok(file) = std::fs::File::open(child.path()) else {
                continue;
            };
            let mut content = Zeroizing::new(String::new());
            if file
                .take(cc_k8s_core::import::MAX_CONFIG_BYTES as u64 + 1)
                .read_to_string(&mut content)
                .is_err()
            {
                continue;
            }
            bytes += content.len();
            if cc_k8s_core::import::parse(&content, Some(&child.path())).is_ok() {
                found.push(child.path().to_string_lossy().into_owned());
            }
        }
    }
    found.sort();
    Ok(found)
}
