//! A durable source import is separate from Ansible execution. Only a fully
//! verified local tree can become a ready project; interruption never runs it.
use super::*;
use crate::automation_prepare::{CancelOnDrop, RevisionLease};
use crate::automation_runners::{AutomationRuntime, RunnerConnection};
use crate::session::{Session, Unlocked};
use cc_automation_core::{GitProjectSource, UploadedRevision};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

fn invalid(reason: &str) -> AppError {
    AppError::invalid("automation_git", reason)
}
fn validate_ca(certificate: Option<&str>) -> AppResult<()> {
    let Some(certificate) = certificate else {
        return Ok(());
    };
    if certificate.is_empty() || certificate.len() > 64 * 1024 || !certificate.is_ascii() {
        return Err(invalid("public CA certificate required"));
    }
    let mut remaining = certificate.as_bytes();
    let mut count = 0;
    loop {
        while remaining.first().is_some_and(u8::is_ascii_whitespace) {
            remaining = &remaining[1..];
        }
        if remaining.is_empty() {
            break;
        }
        if !remaining.starts_with(b"-----BEGIN CERTIFICATE-----") {
            return Err(invalid(
                "private keys and non-certificate data are not accepted",
            ));
        }
        let (tail, pem) = x509_parser::pem::parse_x509_pem(remaining)
            .map_err(|_| invalid("invalid public CA certificate"))?;
        if pem.label != "CERTIFICATE" || tail.len() >= remaining.len() {
            return Err(invalid("invalid public CA certificate"));
        }
        let (rest, _) = x509_parser::parse_x509_certificate(&pem.contents)
            .map_err(|_| invalid("invalid public CA certificate"))?;
        if !rest.is_empty() {
            return Err(invalid("unexpected data after certificate"));
        }
        remaining = tail;
        count += 1;
    }
    if count == 0 {
        return Err(invalid("public CA certificate required"));
    }
    Ok(())
}

struct ActiveImport {
    runtime: Arc<AutomationRuntime>,
    id: String,
    _cancellation: CancelOnDrop,
    _slot: tokio::sync::OwnedSemaphorePermit,
}
impl Drop for ActiveImport {
    fn drop(&mut self) {
        self.runtime
            .git_imports
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}

impl AppCore {
    pub(crate) async fn automation_recover_git_imports(
        &self,
        owner: &Arc<Session>,
    ) -> AppResult<()> {
        let runtime = owner.automation.clone();
        owner
            .storage()?
            .write(move |transaction| {
                let active: std::collections::BTreeSet<_> = runtime
                    .git_imports
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .keys()
                    .cloned()
                    .collect();
                let mut catalog: Catalog = transaction.setting_get(PROJECTS)?.unwrap_or_default();
                if catalog.version != 1 {
                    return Ok::<_, AppError>(());
                }
                let mut changed = false;
                for (id, row) in &mut catalog.records {
                    if managed::marker(row).is_ok()
                        && row.get("version").and_then(Value::as_u64) == Some(1)
                        && row.pointer("/source/kind").and_then(Value::as_str) == Some("git")
                        && row.pointer("/source/status").and_then(Value::as_str)
                            == Some("importing")
                        && !active.contains(id)
                    {
                        row["source"]["status"] = json!("interrupted");
                        row["source"]["error"] = json!("git_import_interrupted");
                        changed = true;
                    }
                }
                if changed {
                    transaction.setting_set(PROJECTS, &catalog)?;
                }
                Ok(())
            })
            .await
    }

    pub async fn automation_project_git_start(
        &self,
        name: String,
        runner_id: String,
        url: String,
        reference: String,
        ca_pem: Option<String>,
    ) -> AppResult<AutomationProjectSummary> {
        let name = valid_name(&name)?;
        if !runner_id.is_empty() {
            crate::dto::parse_id("runner_id", &runner_id)?;
        }
        validate_ca(ca_pem.as_deref())?;
        let ca_digest = ca_pem
            .as_ref()
            .map(|ca| hex::encode(Sha256::digest(ca.as_bytes())));
        GitProjectSource {
            version: 1,
            kind: "git".into(),
            url: url.clone(),
            reference: reference.clone(),
            commit: "0".repeat(40),
            ca_sha256: ca_digest.clone(),
            executable_paths: vec![],
        }
        .validate()
        .map_err(automation_error)?;
        let (owner, unlocked) = self.unlocked().await?;
        let generation = owner.automation.unlocked_generation()?;
        let mut shutdown = unlocked.sharing_shutdown.subscribe();
        if *shutdown.borrow() {
            return Err(AppError::VaultLocked);
        }
        let runtime = owner.automation.clone();
        if runner_id.is_empty() && !cc_automation_core::git_workstation_available().await {
            return Err(invalid("git_not_installed"));
        }
        let runner = if runner_id.is_empty() {
            None
        } else {
            Some(
                runtime
                    .connections
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&runner_id)
                    .cloned()
                    .filter(|runner| !runner.peer.is_closed())
                    .ok_or_else(|| invalid("connect the selected Runner first"))?,
            )
        };
        if runner.as_ref().is_some_and(|runner| {
            runner.probe.pointer("/tools/git").and_then(Value::as_bool) != Some(true)
                || runner
                    .probe
                    .pointer("/sandbox/filesystem")
                    .and_then(Value::as_bool)
                    != Some(true)
                || runner
                    .probe
                    .pointer("/sandbox/pid")
                    .and_then(Value::as_bool)
                    != Some(true)
        }) {
            return Err(invalid("isolated Runner with Git is required"));
        }
        let slot = runtime
            .git_import_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| invalid("source import limit reached"))?;
        let id = Uuid::new_v4().to_string();
        let cancel = CancellationToken::new();
        let guard = ActiveImport {
            runtime: runtime.clone(),
            id: id.clone(),
            _cancellation: CancelOnDrop(cancel.clone()),
            _slot: slot,
        };
        runtime
            .git_imports
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                id.clone(),
                GitImportActive {
                    runner_id: runner_id.clone(),
                    cancel: cancel.clone(),
                },
            );
        let base = owner
            .dir
            .as_ref()
            .ok_or_else(|| invalid("persistent profile required for a Git workspace"))?
            .join("automation")
            .join("projects");
        let io_slot = automation_io_slot(&owner)?;
        let temporary = tokio::task::spawn_blocking(move || -> AppResult<tempfile::TempDir> {
            let _io_slot = io_slot;
            std::fs::create_dir_all(&base)?;
            Ok(tempfile::Builder::new()
                .prefix("source-")
                .tempdir_in(base)?)
        })
        .await
        .map_err(|_| invalid("workspace worker stopped"))??;
        let row = json!({"version":1,"name":name,"revision":1,
            "source":{"kind":"git","path":temporary.path(),"status":"importing","runner_id":runner_id,
                "import_location":if runner.is_none(){"workstation"}else{"runner"},
                "url":url,"reference":reference,"ca_sha256":ca_digest},
            "created_at":chrono::Utc::now().timestamp(),"updated_at":chrono::Utc::now().timestamp()});
        let summary = summary(&id, &row, false);
        let weak = Arc::downgrade(&unlocked);
        let _profile = self.hold_session(&owner).await?;
        let _epoch = owner.hold_unlocked(&unlocked).await?;
        if runtime.unlocked_generation()? != generation {
            return Err(AppError::VaultLocked);
        }
        let stored_id = id.clone();
        owner
            .storage()?
            .write(move |transaction| {
                let mut catalog: Catalog = transaction.setting_get(PROJECTS)?.unwrap_or_default();
                if catalog.version != 1 || catalog.records.len() >= 256 {
                    return Err(invalid("project catalog is read-only or full"));
                }
                catalog.records.insert(stored_id, row);
                transaction.setting_set(PROJECTS, &catalog)?;
                Ok::<_, AppError>(())
            })
            .await?;
        drop(_epoch);
        drop(_profile);
        drop(unlocked);
        let task_owner = owner.clone();
        tokio::spawn(async move {
            let result = tokio::select! {biased;
                _=shutdown.changed()=>Err(AppError::VaultLocked),
                _=cancel.cancelled()=>Err(invalid("source import cancelled")),
                _=runtime.stopped.cancelled()=>Err(AppError::VaultLocked),
                result=import_tree(&task_owner,runner,temporary,url,reference,ca_digest,ca_pem,cancel.clone())=>result,
            };
            let state = if result.is_ok() {
                "ready"
            } else if cancel.is_cancelled() {
                "cancelled"
            } else {
                "failed"
            };
            if result.is_err() {
                cancel.cancel();
            }
            let source = result.ok();
            let _ = persist_result(&task_owner, &weak, &id, generation, state, source).await;
            drop(guard);
        });
        Ok(summary)
    }

    pub async fn automation_project_git_cancel(&self, id: String) -> AppResult<()> {
        project_id(&id)?;
        let (owner, _) = self.unlocked().await?;
        if let Some(import) = owner
            .automation
            .git_imports
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
        {
            import.cancel.cancel();
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceReceipt {
    url: String,
    reference: String,
    commit: String,
    ca_sha256: Option<String>,
    metadata_sha256: String,
}

struct ImportedTree {
    source: Value,
    baseline: super::baseline::Baseline,
}

async fn import_tree(
    owner: &Arc<Session>,
    runner: Option<Arc<RunnerConnection>>,
    temporary: tempfile::TempDir,
    url: String,
    reference: String,
    ca_digest: Option<String>,
    ca_pem: Option<String>,
    cancel: CancellationToken,
) -> AppResult<ImportedTree> {
    let Some(runner) = runner else {
        return import_workstation_tree(owner, temporary, url, reference, ca_pem, cancel).await;
    };
    let mut response = runner
        .peer
        .request(
            "git_capture",
            json!({"url":url,"reference":reference,"ca_pem":ca_pem}),
            Duration::from_secs(200),
        )
        .await
        .map_err(automation_error)?;
    let source = response
        .as_object_mut()
        .and_then(|value| value.remove("source"))
        .ok_or_else(|| invalid("missing source provenance"))?;
    let source: SourceReceipt =
        serde_json::from_value(source).map_err(|_| invalid("invalid source provenance"))?;
    let capture: UploadedRevision =
        serde_json::from_value(response).map_err(|_| invalid("invalid source snapshot"))?;
    let lease = RevisionLease::new(capture, &runner.peer);
    let baseline_slot = tokio::select! {biased;
        _=cancel.cancelled()=>return Err(invalid("source import cancelled")),
        permit=owner.automation.baseline_slots.clone().acquire_owned()=>permit.map_err(|_|AppError::VaultLocked)?,
    };
    let cache = super::baseline::base(
        owner
            .dir
            .as_ref()
            .ok_or_else(|| invalid("persistent profile required"))?,
    );
    let io_slot = automation_io_slot(owner)?;
    let peer = runner.peer.clone();
    let handle = tokio::runtime::Handle::current();
    let source = tokio::task::spawn_blocking(move || -> AppResult<ImportedTree> {
        let _io_slot = io_slot;
        let _baseline_slot = baseline_slot;
        let lease = lease;
        if cancel.is_cancelled() {
            return Err(invalid("source import cancelled"));
        }
        let root = ProjectRoot::open(temporary.path()).map_err(automation_error)?;
        let revision = handle
            .block_on(root.download(&lease.uploaded, &peer, &cancel))
            .map_err(automation_error)?;
        let (metadata, stamp) = root
            .git_source()
            .map_err(automation_error)?
            .ok_or_else(|| invalid("missing source provenance"))?;
        if metadata.url != url
            || metadata.reference != reference
            || metadata.ca_sha256 != ca_digest
            || source.metadata_sha256 != stamp
            || source.commit != metadata.commit
            || source.url != metadata.url
            || source.reference != metadata.reference
            || source.ca_sha256 != metadata.ca_sha256
        {
            return Err(invalid("source differs from requested repository"));
        }
        if cancel.is_cancelled() {
            return Err(invalid("source import cancelled"));
        }
        let baseline = super::baseline::capture(&cache, &root, &revision, &cancel)?;
        // Persist files before ready state. A crash/lock in between leaves a
        // durable interrupted record, never a ready project with deleted files.
        let _path = temporary.keep();
        let mut value = serde_json::to_value(source).map_err(AppError::internal)?;
        value["snapshot_digest"] = json!(lease.uploaded.digest);
        Ok(ImportedTree {
            source: value,
            baseline,
        })
    })
    .await
    .map_err(|_| invalid("source receiver stopped"))??;
    Ok(source)
}

async fn import_workstation_tree(
    owner: &Arc<Session>,
    temporary: tempfile::TempDir,
    url: String,
    reference: String,
    ca_pem: Option<String>,
    cancel: CancellationToken,
) -> AppResult<ImportedTree> {
    let workspace = tempfile::Builder::new().prefix("git-data-").tempdir_in(
        temporary
            .path()
            .parent()
            .ok_or_else(|| invalid("workspace unavailable"))?,
    )?;
    let metadata = cc_automation_core::capture_git_workstation(
        temporary.path(),
        workspace.path(),
        url,
        reference,
        ca_pem,
        cancel.clone(),
    )
    .await
    .map_err(automation_error)?;
    let baseline_slot = tokio::select! {biased;
        _=cancel.cancelled()=>return Err(invalid("source import cancelled")),
        permit=owner.automation.baseline_slots.clone().acquire_owned()=>permit.map_err(|_|AppError::VaultLocked)?,
    };
    let cache = super::baseline::base(
        owner
            .dir
            .as_ref()
            .ok_or_else(|| invalid("persistent profile required"))?,
    );
    let io_slot = automation_io_slot(owner)?;
    tokio::task::spawn_blocking(move || ->AppResult<ImportedTree>{
        let _io_slot=io_slot;let _baseline_slot=baseline_slot;
        if cancel.is_cancelled(){return Err(invalid("source import cancelled"))}
        let root=ProjectRoot::open(temporary.path()).map_err(automation_error)?;
        let revision=root.revision().map_err(automation_error)?;
        let (actual,stamp)=root.git_source().map_err(automation_error)?.ok_or_else(||invalid("missing source provenance"))?;
        if actual.url!=metadata.url || actual.reference!=metadata.reference || actual.commit!=metadata.commit
            || actual.ca_sha256!=metadata.ca_sha256{return Err(invalid("source provenance changed"))}
        let baseline=super::baseline::capture(&cache,&root,&revision,&cancel)?;
        if cancel.is_cancelled(){return Err(invalid("source import cancelled"))}
        let source=json!({"url":actual.url,"reference":actual.reference,"commit":actual.commit,
            "ca_sha256":actual.ca_sha256,"metadata_sha256":stamp,"snapshot_digest":revision.digest});
        let _path=temporary.keep();
        Ok(ImportedTree{source,baseline})
    }).await.map_err(|_|invalid("workspace worker stopped"))?
}

async fn persist_result(
    owner: &Arc<Session>,
    weak: &Weak<Unlocked>,
    id: &str,
    generation: u64,
    state: &str,
    source: Option<ImportedTree>,
) -> AppResult<()> {
    let unlocked = weak.upgrade().ok_or(AppError::VaultLocked)?;
    let _epoch = owner.hold_unlocked(&unlocked).await?;
    if owner.automation.unlocked_generation()? != generation
        || owner.automation.stopped.is_cancelled()
    {
        return Err(AppError::VaultLocked);
    }
    let id = id.to_owned();
    let state = state.to_owned();
    owner
        .storage()?
        .write(move |transaction| {
            let mut catalog: Catalog = transaction.setting_get(PROJECTS)?.unwrap_or_default();
            let row = catalog
                .records
                .get_mut(&id)
                .ok_or_else(|| invalid("import record disappeared"))?;
            if catalog.version != 1
                || managed::marker(row).is_err()
                || row.pointer("/source/status").and_then(Value::as_str) != Some("importing")
            {
                return Err(invalid("import record changed"));
            }
            row["source"]["status"] = json!(state);
            if let Some(source) = source {
                row["source"]["git"] = source.source;
                row["baseline"] =
                    serde_json::to_value(source.baseline).map_err(AppError::internal)?;
            } else {
                row["source"]["error"] = json!("git_import_failed");
            }
            row["revision"] = json!(2);
            row["updated_at"] = json!(chrono::Utc::now().timestamp());
            transaction.setting_set(PROJECTS, &catalog)?;
            Ok::<_, AppError>(())
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn interrupted_imports_stay_read_only_without_replay_and_future_rows_are_preserved() {
        let folder = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            folder.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile(
            "Git recovery".into(),
            "correct horse source fixture passphrase".into(),
        )
        .await
        .unwrap();
        let (owner, _) = app.unlocked().await.unwrap();
        let old = Uuid::new_v4().to_string();
        let future = Uuid::new_v4().to_string();
        let old_id = old.clone();
        let future_id = future.clone();
        owner.storage().unwrap().write(move|transaction| {
            let mut catalog=Catalog::default();
            catalog.records.insert(old_id,json!({"version":1,"name":"Interrupted fixture","revision":1,
                "source":{"kind":"git","status":"importing","path":"unused"}}));
            catalog.records.insert(future_id,json!({"version":2,"name":"Future fixture","revision":1,
                "source":{"kind":"git","status":"importing","path":"unused","future_field":"retain"}}));
            transaction.setting_set(PROJECTS,&catalog)?;Ok::<_,AppError>(())
        }).await.unwrap();
        let projects = app.automation_projects().await.unwrap();
        let interrupted = projects
            .projects
            .iter()
            .find(|project| project.id == old)
            .unwrap();
        assert!(interrupted.read_only);
        assert_eq!(interrupted.import_state.as_deref(), Some("interrupted"));
        let retained = projects
            .projects
            .iter()
            .find(|project| project.id == future)
            .unwrap();
        assert!(retained.read_only);
        assert_eq!(retained.import_state.as_deref(), Some("importing"));
        assert!(owner.automation.git_imports.lock().unwrap().is_empty());
        assert!(app.automation_project_files(old).await.is_err());
        app.shutdown().await.unwrap();
    }
    #[test]
    fn a_private_key_is_rejected_before_it_can_be_forwarded_to_a_runner() {
        assert!(
            validate_ca(Some(
                "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----"
            ))
            .is_err()
        );
        assert!(
            validate_ca(Some(
                "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----"
            ))
            .is_err()
        );
        assert!(validate_ca(None).is_ok());
    }
}
