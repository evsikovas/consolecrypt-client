//! Short-lived native kubeconfig previews. The UI receives metadata and an
//! opaque ticket, never authentication values. Tickets belong to one unlock
//! lifetime and cannot be replayed or moved to another profile.
use crate::{AppCore, AppError, AppResult};
use cc_k8s_core::{
    import::{self, ImportBatch, ImportedAuth},
    import_files::{pending_paths, resolve},
    vault_import::prepare,
};
use cc_models::kubernetes::{KubeContext, KubeRoute};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::time::Instant;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

const PREVIEW_LIFETIME: Duration = Duration::from_secs(600);
const MAX_PREVIEWS: usize = 4;

#[derive(Debug, Clone, Serialize)]
pub struct KubeImportPreview {
    pub ticket: String,
    pub expires_at_ms: i64,
    pub current_context: Option<String>,
    pub entries: Vec<KubeImportEntry>,
}

/// Only fields needed to decide which contexts to import.
#[derive(Debug, Clone, Serialize)]
pub struct KubeImportEntry {
    pub index: u32,
    pub name: String,
    pub cluster_name: Option<String>,
    pub user_name: Option<String>,
    pub server: Option<String>,
    pub namespace: Option<String>,
    pub authentication: Option<String>,
    pub pending_files: Vec<String>,
    pub requires_route: bool,
    pub insecure_tls: bool,
    pub error: Option<String>,
}

/// This is deliberately separate from ordinary preview metadata. Return it
/// only to a protected, short-lived local modal; never cache or log it.
#[derive(Serialize)]
pub struct KubeImportExecInspection {
    pub command: String,
    pub arguments: Vec<String>,
    pub environment: BTreeMap<String, String>,
    pub binary_path: Option<String>,
    pub binary_sha256: Option<String>,
    pub supported: bool,
}
impl std::fmt::Debug for KubeImportExecInspection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("KubeImportExecInspection([protected])")
    }
}
impl Drop for KubeImportExecInspection {
    fn drop(&mut self) {
        self.command.zeroize();
        self.arguments.zeroize();
        for value in self.environment.values_mut() {
            value.zeroize();
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KubeImportChoice {
    pub index: u32,
    #[serde(default = "production_default")]
    pub production: bool,
    #[serde(default)]
    pub route: KubeRoute,
    #[serde(default)]
    pub approved_files: Vec<String>,
}

fn production_default() -> bool {
    true
}

struct PendingImport {
    batch: ImportBatch,
    expires: Instant,
}

#[derive(Default)]
struct PendingState {
    closed: bool,
    tickets: BTreeMap<Uuid, PendingImport>,
}

#[derive(Default)]
pub(crate) struct ImportRuntime(Mutex<PendingState>);

impl std::fmt::Debug for ImportRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ImportRuntime([native previews])")
    }
}

fn invalid(reason: impl Into<String>) -> AppError {
    AppError::invalid("kube_import", reason)
}

impl ImportRuntime {
    pub(crate) fn close(&self) {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        state.closed = true;
        state.tickets.clear();
    }

    fn discard(&self, id: Uuid) {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .tickets
            .remove(&id);
    }

    fn inspect_exec(&self, id: Uuid, index: u32) -> AppResult<KubeImportExecInspection> {
        let state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if state.closed {
            return Err(AppError::VaultLocked);
        }
        let context = state
            .tickets
            .get(&id)
            .filter(|pending| pending.expires > Instant::now())
            .and_then(|pending| pending.batch.entries.get(index as usize))
            .and_then(|entry| entry.context.as_ref().ok())
            .ok_or_else(|| invalid("preview_unavailable"))?;
        let ImportedAuth::Exec(exec) = &context.auth else {
            return Err(invalid("not_exec_authentication"));
        };
        Ok(KubeImportExecInspection {
            command: exec.command.clone(),
            arguments: exec.args.to_vec(),
            environment: exec
                .env
                .iter()
                .map(|(name, value)| (name.clone(), value.expose_secret().into()))
                .collect(),
            binary_path: None,
            binary_sha256: None,
            supported: !cfg!(any(target_os = "android", target_os = "ios")),
        })
    }

    fn insert(self: &Arc<Self>, batch: ImportBatch) -> AppResult<KubeImportPreview> {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if state.closed {
            return Err(AppError::VaultLocked);
        }
        state
            .tickets
            .retain(|_, pending| pending.expires > Instant::now());
        if state.tickets.len() >= MAX_PREVIEWS {
            return Err(invalid("close an existing import preview first"));
        }
        let id = Uuid::new_v4();
        let preview = preview(id, &batch);
        state.tickets.insert(
            id,
            PendingImport {
                batch,
                expires: Instant::now() + PREVIEW_LIFETIME,
            },
        );
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            tokio::time::sleep(PREVIEW_LIFETIME).await;
            if let Some(runtime) = weak.upgrade() {
                runtime.discard(id);
            }
        });
        Ok(preview)
    }

    fn take(&self, id: Uuid, choices: &[KubeImportChoice]) -> AppResult<ImportBatch> {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if state.closed {
            return Err(AppError::VaultLocked);
        }
        state
            .tickets
            .retain(|_, pending| pending.expires > Instant::now());
        let pending = state
            .tickets
            .get(&id)
            .ok_or_else(|| invalid("preview expired; import again"))?;
        if choices.is_empty() || choices.len() > 64 {
            return Err(invalid("select between 1 and 64 contexts"));
        }
        let mut seen = BTreeSet::new();
        // Validate every selection and every approval before opening any file.
        for choice in choices {
            if !seen.insert(choice.index) {
                return Err(invalid("a context was selected more than once"));
            }
            let context = pending
                .batch
                .entries
                .get(choice.index as usize)
                .and_then(|entry| entry.context.as_ref().ok())
                .ok_or_else(|| invalid("selected context is unavailable"))?;
            let approved: BTreeSet<_> = choice.approved_files.iter().map(PathBuf::from).collect();
            if approved.len() != choice.approved_files.len() || pending_paths(context) != approved {
                return Err(invalid(
                    "confirm the exact referenced files before importing",
                ));
            }
            if choice.production && context.insecure_skip_tls_verify {
                return Err(invalid(
                    "production contexts require TLS certificate verification",
                ));
            }
            if context.proxy.is_some() && choice.route == KubeRoute::Direct {
                return Err(invalid(
                    "map the imported proxy to a saved connection route",
                ));
            }
        }
        Ok(state.tickets.remove(&id).expect("validated preview").batch)
    }
}

fn preview(id: Uuid, batch: &ImportBatch) -> KubeImportPreview {
    KubeImportPreview {
        ticket: id.to_string(),
        expires_at_ms: chrono::Utc::now().timestamp_millis() + PREVIEW_LIFETIME.as_millis() as i64,
        current_context: batch.current_context.clone(),
        entries: batch
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let mut result = KubeImportEntry {
                    index: index as u32,
                    name: entry.name.clone(),
                    cluster_name: None,
                    user_name: None,
                    server: None,
                    namespace: None,
                    authentication: None,
                    pending_files: Vec::new(),
                    requires_route: false,
                    insecure_tls: false,
                    error: None,
                };
                match &entry.context {
                    Ok(context) => {
                        result.cluster_name = Some(context.cluster_name.clone());
                        result.user_name = Some(context.user_name.clone());
                        result.server = Some(context.server.to_string());
                        result.namespace = Some(context.namespace.clone());
                        result.authentication = Some(
                            match context.auth {
                                ImportedAuth::Token(_) => "token",
                                ImportedAuth::ClientCertificate { .. } => "client_certificate",
                                ImportedAuth::Oidc { .. } => "oidc",
                                ImportedAuth::Exec(_) => "exec",
                            }
                            .into(),
                        );
                        result.pending_files = pending_paths(context)
                            .iter()
                            .map(|path| path.to_string_lossy().into_owned())
                            .collect();
                        result.requires_route = context.proxy.is_some();
                        result.insecure_tls = context.insecure_skip_tls_verify;
                    }
                    Err(error) => result.error = Some(error.to_string()),
                }
                result
            })
            .collect(),
    }
}

impl AppCore {
    /// Inspect the installed executable without executing it or granting any
    /// permission. The later connect flow independently rehashes/reviews the
    /// binary and effective environment before execution.
    pub async fn kube_import_inspect_exec(
        &self,
        ticket: String,
        index: u32,
    ) -> AppResult<KubeImportExecInspection> {
        let (owner, unlocked) = self.unlocked().await?;
        let id = ticket
            .parse()
            .map_err(|_| invalid("invalid preview ticket"))?;
        let mut inspection = unlocked.kube_imports.inspect_exec(id, index)?;
        let inspection = crate::secrets::blocking(move || {
            if inspection.supported {
                let path = inspection
                    .environment
                    .get("PATH")
                    .cloned()
                    .unwrap_or_else(crate::kubernetes_auth::default_exec_path);
                if let Ok(binary) =
                    cc_k8s_core::auth::exec::resolve_binary(&inspection.command, &path)
                {
                    inspection.binary_path = Some(binary.to_string_lossy().into_owned());
                    if let Ok(hash) = cc_k8s_core::auth::exec::binary_hash(&binary) {
                        inspection.binary_sha256 =
                            Some(hash.iter().map(|byte| format!("{byte:02x}")).collect());
                    }
                }
            }
            Ok(inspection)
        })
        .await?;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        // Expiry/discard may occur while a large binary is being hashed.
        drop(unlocked.kube_imports.inspect_exec(id, index)?);
        Ok(inspection)
    }

    /// Read only the file selected by the user. Its authentication references
    /// remain unopened until the later, explicit import confirmation.
    pub async fn kube_import_preview_file(&self, path: String) -> AppResult<KubeImportPreview> {
        let (session, unlocked) = self.unlocked().await?;
        let batch = crate::secrets::blocking(move || {
            let path = PathBuf::from(path);
            let text = cc_k8s_core::import_files::read_config_file(&path)
                .map_err(|e| invalid(e.to_string()))?;
            import::parse(&text, Some(&path)).map_err(|e| invalid(e.to_string()))
        })
        .await?;
        let _profile = self.hold_session(&session).await?;
        let _generation = session.hold_unlocked(&unlocked).await?;
        unlocked.kube_imports.insert(batch)
    }

    /// Parse pasted or selected YAML on a blocking worker. `source_file` only
    /// resolves relative references; it does not authorize opening any file.
    pub async fn kube_import_preview(
        &self,
        yaml: Zeroizing<Vec<u8>>,
        source_file: Option<String>,
    ) -> AppResult<KubeImportPreview> {
        let (session, unlocked) = self.unlocked().await?;
        let batch = crate::secrets::blocking(move || {
            let text =
                std::str::from_utf8(&yaml).map_err(|_| invalid("kubeconfig must be UTF-8"))?;
            let path = source_file.map(PathBuf::from);
            import::parse(text, path.as_deref()).map_err(|e| invalid(e.to_string()))
        })
        .await?;
        let _profile = self.hold_session(&session).await?;
        let _generation = session.hold_unlocked(&unlocked).await?;
        unlocked.kube_imports.insert(batch)
    }

    pub async fn kube_import_discard(&self, ticket: String) -> AppResult<()> {
        let (_, unlocked) = self.unlocked().await?;
        let id = ticket
            .parse()
            .map_err(|_| invalid("invalid preview ticket"))?;
        unlocked.kube_imports.discard(id);
        Ok(())
    }

    /// Consume a native preview once. Every selected connection and credential
    /// is committed in one SQLCipher transaction through the E2EE outbox.
    pub async fn kube_import_commit(
        &self,
        ticket: String,
        choices: Vec<KubeImportChoice>,
    ) -> AppResult<Vec<KubeContext>> {
        let (session, unlocked) = self.unlocked().await?;
        let id = ticket
            .parse()
            .map_err(|_| invalid("invalid preview ticket"))?;
        let batch = unlocked.kube_imports.take(id, &choices)?;
        let connections = crate::secrets::blocking(move || {
            let mut entries: Vec<_> = batch.entries.into_iter().map(Some).collect();
            choices
                .into_iter()
                .map(|choice| {
                    let context = entries[choice.index as usize]
                        .take()
                        .ok_or_else(|| invalid("duplicate context"))?
                        .context
                        .map_err(|e| invalid(e.to_string()))?;
                    let approved = choice.approved_files.iter().map(PathBuf::from).collect();
                    let context =
                        resolve(context, &approved).map_err(|e| invalid(e.to_string()))?;
                    let connection = prepare(context, choice.production, choice.route)
                        .map_err(|e| invalid(e.to_string()))?;
                    Ok((connection.cluster, connection.context, connection.secrets))
                })
                .collect::<AppResult<Vec<_>>>()
        })
        .await?;
        let _profile = self.hold_session(&session).await?;
        let _generation = session.hold_unlocked(&unlocked).await?;
        crate::kubernetes::persist_imports(&unlocked, connections).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppConfig;

    #[cfg(unix)]
    #[tokio::test]
    async fn protected_exec_inspection_never_runs_or_approves_imported_command() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("must-not-run");
        let executable = temp.path().join("provider");
        std::fs::write(
            &executable,
            format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let app = app(&temp.path().join("data")).await;
        let yaml = serde_json::json!({
            "apiVersion":"v1","kind":"Config",
            "clusters":[{"name":"cluster","cluster":{"server":"https://cluster.test"}}],
            "users":[{"name":"user","user":{"exec":{
                "command":executable,"args":["--private=fixture-argument"],
                "env":[{"name":"PRIVATE_VALUE","value":"fixture-env"}],
                "apiVersion":"client.authentication.k8s.io/v1","interactiveMode":"Never"
            }}}],
            "contexts":[{"name":"context","context":{"cluster":"cluster","user":"user"}}]
        })
        .to_string();
        let preview = app
            .kube_import_preview(Zeroizing::new(yaml.into_bytes()), None)
            .await
            .unwrap();
        let metadata = serde_json::to_string(&preview).unwrap();
        assert!(!metadata.contains("fixture-argument"));
        assert!(!metadata.contains("fixture-env"));
        let review = app
            .kube_import_inspect_exec(preview.ticket.clone(), 0)
            .await
            .unwrap();
        assert_eq!(review.arguments, ["--private=fixture-argument"]);
        assert_eq!(review.environment["PRIVATE_VALUE"], "fixture-env");
        assert_eq!(review.binary_sha256.as_ref().unwrap().len(), 64);
        assert!(!format!("{review:?}").contains("fixture-env"));
        assert!(!marker.exists());
        let contexts = app
            .kube_import_commit(
                preview.ticket.clone(),
                vec![choice(&preview, "context", false)],
            )
            .await
            .unwrap();
        assert!(app
            .kube_import_inspect_exec(preview.ticket, 0)
            .await
            .is_err());
        let error = app
            .kube_connect(contexts[0].id.to_string())
            .await
            .unwrap_err();
        assert!(
            matches!(error,AppError::InvalidInput {reason,..} if reason=="exec_approval_required")
        );
        assert!(
            !marker.exists(),
            "inspection and import never grant execution consent"
        );
    }

    fn yaml() -> Zeroizing<Vec<u8>> {
        Zeroizing::new(
            b"apiVersion: v1
kind: Config
current-context: inline
clusters:
- name: test
  cluster:
    server: https://cluster.example.test
users:
- name: inline
  user:
    token: synthetic-inline-token
- name: file
  user:
    tokenFile: token
contexts:
- name: inline
  context: {cluster: test, user: inline}
- name: file
  context: {cluster: test, user: file}
"
            .to_vec(),
        )
    }

    fn choice(preview: &KubeImportPreview, name: &str, approve: bool) -> KubeImportChoice {
        let entry = preview
            .entries
            .iter()
            .find(|entry| entry.name == name)
            .unwrap();
        KubeImportChoice {
            index: entry.index,
            production: true,
            route: KubeRoute::Direct,
            approved_files: if approve {
                entry.pending_files.clone()
            } else {
                Vec::new()
            },
        }
    }

    async fn app(path: &std::path::Path) -> AppCore {
        let app = AppCore::new(AppConfig::for_tests(path.to_string_lossy().into_owned())).unwrap();
        app.create_local_profile("test".into(), "kube-import-local-passphrase".into())
            .await
            .unwrap();
        app
    }

    #[tokio::test]
    async fn preview_is_redacted_and_no_partial_import_survives_a_file_failure() {
        let dir = tempfile::tempdir().unwrap();
        let app = app(&dir.path().join("app")).await;
        let source = Some(dir.path().join("config").to_string_lossy().into_owned());
        std::fs::write(dir.path().join("config"), &*yaml()).unwrap();
        let preview = app
            .kube_import_preview_file(source.clone().unwrap())
            .await
            .unwrap();
        assert!(!serde_json::to_string(&preview)
            .unwrap()
            .contains("synthetic-inline-token"));
        assert!(!format!("{preview:?}").contains("synthetic-inline-token"));
        assert_eq!(preview.current_context.as_deref(), Some("inline"));
        assert!(app.list_kube_contexts().await.unwrap().is_empty());

        // Missing approval must be reported before even trying the missing file.
        let error = app
            .kube_import_commit(
                preview.ticket.clone(),
                vec![
                    choice(&preview, "inline", false),
                    choice(&preview, "file", false),
                ],
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("confirm the exact"));
        assert!(app.list_kube_contexts().await.unwrap().is_empty());
        let error = app
            .kube_import_commit(
                preview.ticket.clone(),
                vec![
                    choice(&preview, "inline", false),
                    choice(&preview, "file", true),
                ],
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("unavailable"));
        assert!(app.list_kube_clusters().await.unwrap().is_empty());
        assert!(app.list_kube_contexts().await.unwrap().is_empty());

        std::fs::write(dir.path().join("token"), "synthetic-file-token\n").unwrap();
        let preview = app.kube_import_preview(yaml(), source).await.unwrap();
        let choices = vec![
            choice(&preview, "inline", false),
            choice(&preview, "file", true),
        ];
        let saved = app
            .kube_import_commit(preview.ticket.clone(), choices.clone())
            .await
            .unwrap();
        assert_eq!(saved.len(), 2);
        assert!(saved.iter().all(|context| context.production));
        let metadata = serde_json::to_string(&(
            app.list_kube_clusters().await.unwrap(),
            app.list_kube_contexts().await.unwrap(),
        ))
        .unwrap();
        assert!(!metadata.contains("synthetic-inline-token"));
        assert!(!metadata.contains("synthetic-file-token"));
        let (_, unlocked) = app.unlocked().await.unwrap();
        for context in &saved {
            let secret = unlocked
                .writer
                .read_secret(context.auth.secret_ids()[0])
                .await
                .unwrap();
            assert_eq!(
                secret.value.expose_secret(),
                if context.name == "file" {
                    "synthetic-file-token"
                } else {
                    "synthetic-inline-token"
                }
            );
        }
        assert!(
            app.kube_import_commit(preview.ticket, choices)
                .await
                .is_err(),
            "ticket must be single use"
        );
        assert_eq!(app.list_kube_contexts().await.unwrap().len(), 2);
        app.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn cancel_lock_and_profile_switch_invalidate_native_previews() {
        let dir = tempfile::tempdir().unwrap();
        let app = app(dir.path()).await;
        let preview = app.kube_import_preview(yaml(), None).await.unwrap();
        let choices = vec![choice(&preview, "inline", false)];
        app.kube_import_discard(preview.ticket.clone())
            .await
            .unwrap();
        assert!(app
            .kube_import_commit(preview.ticket, choices)
            .await
            .is_err());
        let preview = app.kube_import_preview(yaml(), None).await.unwrap();
        let choices = vec![choice(&preview, "inline", false)];
        let (_, old) = app.unlocked().await.unwrap();
        app.lock().await.unwrap();
        assert!(old.kube_imports.0.lock().unwrap().tickets.is_empty());
        app.unlock_with_passphrase("kube-import-local-passphrase".into())
            .await
            .unwrap();
        assert!(app
            .kube_import_commit(preview.ticket, choices)
            .await
            .is_err());
        let preview = app.kube_import_preview(yaml(), None).await.unwrap();
        let choices = vec![choice(&preview, "inline", false)];
        app.create_local_profile("second".into(), "second-kube-local-passphrase".into())
            .await
            .unwrap();
        assert!(app
            .kube_import_commit(preview.ticket, choices)
            .await
            .is_err());
        assert!(app.list_kube_contexts().await.unwrap().is_empty());
        app.shutdown().await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn previews_expire_without_another_call_and_capacity_is_bounded() {
        let runtime = Arc::new(ImportRuntime::default());
        let batch = || import::parse(std::str::from_utf8(&yaml()).unwrap(), None).unwrap();
        for _ in 0..MAX_PREVIEWS {
            runtime.insert(batch()).unwrap();
        }
        assert!(runtime.insert(batch()).is_err());
        // Start the expiry tasks before advancing the paused clock.
        tokio::task::yield_now().await;
        tokio::time::advance(PREVIEW_LIFETIME).await;
        tokio::task::yield_now().await;
        assert!(runtime.0.lock().unwrap().tickets.is_empty());
        runtime.insert(batch()).unwrap();
        runtime.close();
        assert!(matches!(
            runtime.insert(batch()),
            Err(AppError::VaultLocked)
        ));
    }

    #[tokio::test]
    async fn altered_selections_and_unapproved_paths_do_not_consume_a_preview() {
        let runtime = Arc::new(ImportRuntime::default());
        let preview = runtime
            .insert(import::parse(std::str::from_utf8(&yaml()).unwrap(), None).unwrap())
            .unwrap();
        let id = preview.ticket.parse().unwrap();
        let selected = choice(&preview, "inline", false);
        assert!(runtime.take(id, &[]).is_err());
        assert!(runtime
            .take(id, &[selected.clone(), selected.clone()])
            .is_err());
        let mut unexpected = selected.clone();
        unexpected.approved_files = vec!["/unrelated/secret".into()];
        assert!(runtime.take(id, &[unexpected]).is_err());
        let mut invalid_index = selected.clone();
        invalid_index.index = u32::MAX;
        assert!(runtime.take(id, &[invalid_index]).is_err());
        assert!(runtime.take(id, &[selected]).is_ok());
        let defaults: KubeImportChoice = serde_json::from_str(r#"{"index":0}"#).unwrap();
        assert!(defaults.production);
        assert_eq!(defaults.route, KubeRoute::Direct);
    }
}
