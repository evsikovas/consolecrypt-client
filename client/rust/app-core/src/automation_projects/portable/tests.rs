use super::*;
use std::sync::OnceLock;
fn serial() -> &'static tokio::sync::Mutex<()> {
    static S: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    S.get_or_init(Default::default)
}
struct Fixture {
    directory: tempfile::TempDir,
    app: AppCore,
    project: String,
}
impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            directory.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile(
            "Owned portable package fixture".into(),
            "synthetic profile password42".into(),
        )
        .await
        .unwrap();
        let project = app
            .automation_project_create("Portable fixture".into(), None)
            .await
            .unwrap()
            .id;
        app.automation_project_save(
            project.clone(),
            "selected.yml".into(),
            None,
            "---\n- hosts: all\n  tasks: []\n".into(),
        )
        .await
        .unwrap();
        app.automation_project_save(
            project.clone(),
            ".env".into(),
            None,
            "SYNTHETIC_EXCLUDED_VALUE=do_not_transfer\n".into(),
        )
        .await
        .unwrap();
        Self {
            directory,
            app,
            project,
        }
    }
    async fn context(&self) -> PortableContext {
        let scope = self.app.automation_portable_scope().await.unwrap();
        let project = self
            .app
            .automation_projects()
            .await
            .unwrap()
            .projects
            .into_iter()
            .find(|p| p.id == self.project)
            .unwrap();
        self.app
            .automation_portable_context(
                scope,
                self.project.clone(),
                project.revision.unwrap(),
                &CancellationToken::new(),
            )
            .await
            .unwrap()
    }
    fn request(&self, context: &PortableContext) -> String {
        json!({"scope":context.scope,"projectId":self.project,"projectRevision":context.project_revision,"revisionDigest":context.revision_digest,"projectStamp":context.project_stamp,"selectedPaths":["selected.yml"],"playbook":"selected.yml"}).to_string()
    }
    fn destination(&self) -> PathBuf {
        self.directory.path().join("owned-export.ccproject")
    }
}
#[tokio::test]
async fn actual_native_export_inspection_no_overwrite_no_catalog_or_source_mutation() {
    let _serial = serial().lock().await;
    let f = Fixture::new().await;
    let context = f.context().await;
    let before = f
        .app
        .automation_project_files(f.project.clone())
        .await
        .unwrap();
    let catalog = f.app.automation_projects().await.unwrap();
    assert!(
        context
            .choices
            .iter()
            .any(|c| c.path == ".env" && !c.selectable)
    );
    let result = f
        .app
        .automation_portable_export(
            f.request(&context),
            SecretString::from("explicit separate package passphrase"),
            f.destination(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    let bytes = std::fs::read(f.destination()).unwrap();
    assert_eq!(result.bytes, bytes.len());
    assert_eq!(result.encrypted_sha256, digest(&bytes));
    assert!(
        !bytes
            .windows(b"SYNTHETIC_EXCLUDED_VALUE".len())
            .any(|v| v == b"SYNTHETIC_EXCLUDED_VALUE")
    );
    let inspected = f
        .app
        .automation_portable_inspect(
            context.scope.clone(),
            f.destination(),
            SecretString::from("explicit separate package passphrase"),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(inspected.summary.files, 1);
    assert_eq!(inspected.encrypted_sha256, result.encrypted_sha256);
    assert!(!inspected.publication_supported);
    assert_eq!(inspected.reason, "managed_publication_required");
    assert!(
        f.app
            .automation_portable_inspect(
                context.scope.clone(),
                f.destination(),
                SecretString::from("wrong separate passphrase"),
                &CancellationToken::new()
            )
            .await
            .is_err()
    );
    assert!(
        f.app
            .automation_portable_export(
                f.request(&context),
                SecretString::from("explicit separate package passphrase"),
                f.destination(),
                &CancellationToken::new()
            )
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(f.destination()).unwrap(), bytes);
    assert_eq!(
        f.app
            .automation_project_files(f.project.clone())
            .await
            .unwrap(),
        before
    );
    assert_eq!(
        f.app.automation_projects().await.unwrap().projects.len(),
        catalog.projects.len()
    );
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn actual_sql_only_project_change_denies_original_review_without_destination_write() {
    let f = Fixture::new().await;
    let context = f.context().await;
    let (owner, u) = f.app.unlocked().await.unwrap();
    drop(u);
    let project = f.project.clone();
    owner
        .storage()
        .unwrap()
        .write(move |tx| -> AppResult<()> {
            let mut catalog: Catalog = tx.setting_get(PROJECTS)?.unwrap();
            catalog.records.get_mut(&project).unwrap()["name"] =
                json!("same revision new SQL-only name");
            tx.setting_set(PROJECTS, &catalog)?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(
        f.app
            .automation_portable_export(
                f.request(&context),
                SecretString::from("explicit separate package passphrase"),
                f.destination(),
                &CancellationToken::new()
            )
            .await
            .is_err()
    );
    assert!(!f.destination().exists());
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn cancelled_operation_and_changed_actual_source_preserve_destination_absence() {
    let f = Fixture::new().await;
    let context = f.context().await;
    let token = CancellationToken::new();
    token.cancel();
    assert!(
        f.app
            .automation_portable_export(
                f.request(&context),
                SecretString::from("explicit separate package passphrase"),
                f.destination(),
                &token
            )
            .await
            .is_err()
    );
    assert!(!f.destination().exists());
    let source = f.app.automation_project_context(&f.project).await.unwrap();
    std::fs::write(
        source.path.join("selected.yml"),
        b"---\n# actual external content mutation\n",
    )
    .unwrap();
    assert!(
        f.app
            .automation_portable_export(
                f.request(&context),
                SecretString::from("explicit separate package passphrase"),
                f.destination(),
                &CancellationToken::new()
            )
            .await
            .is_err()
    );
    assert!(!f.destination().exists());
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn original_scope_lock_and_worker_admission_closed_deny_without_file() {
    let f = Fixture::new().await;
    let context = f.context().await;
    f.app.lock().await.unwrap();
    assert!(
        f.app
            .automation_portable_export(
                f.request(&context),
                SecretString::from("explicit separate package passphrase"),
                f.destination(),
                &CancellationToken::new()
            )
            .await
            .is_err()
    );
    assert!(!f.destination().exists());
    f.app.shutdown().await.unwrap();
}
#[test]
fn strict_duplicate_authority_field_or_oversized_request_rejected() {
    let scope =
        json!({"version":1,"profileId":Uuid::new_v4(),"notificationScopeId":Uuid::new_v4()});
    let request = json!({"scope":scope,"projectId":Uuid::new_v4(),"projectRevision":1,"revisionDigest":"a".repeat(64),"projectStamp":"b".repeat(64),"selectedPaths":["play.yml"],"playbook":null});
    let raw = request.to_string();
    assert!(PortableExportRequest::parse(&raw).is_ok());
    for raw in [
        raw.replacen(
            "\"projectRevision\":1",
            "\"projectRevision\":1,\"projectRevision\":1",
            1,
        ),
        raw.replacen(
            "\"version\":1",
            "\"version\":1,\"credentialId\":\"never-authority\"",
            1,
        ),
        " ".repeat(256 * 1024 + 1),
    ] {
        assert!(PortableExportRequest::parse(&raw).is_err());
    }
}

#[test]
fn actual_lock_fences_registered_queued_worker_before_any_file_handle_or_write() {
    std::thread::spawn(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()
            .unwrap();
        runtime.block_on(async {
            use std::{
                future::Future,
                task::Poll,
                sync::atomic::{AtomicBool, Ordering},
                time::Duration,
            };
            let f = Fixture::new().await;
            let scope = f.app.automation_portable_scope().await.unwrap();
            let cancel = CancellationToken::new();
            let subject = Subject::capture(&f.app, &scope, &cancel).await.unwrap();
            let (entered_tx, entered_rx) = oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let blocker = tokio::task::spawn_blocking(move || {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            });
            tokio::time::timeout(Duration::from_secs(10), entered_rx)
                .await
                .unwrap()
                .unwrap();
            let entered = Arc::new(AtomicBool::new(false));
            let entered_worker = entered.clone();
            let marker = f.directory.path().join("must-not-exist-after-native-lock");
            let worker_marker = marker.clone();
            let mut worker = Box::pin(work(&subject, move |_fence| {
                entered_worker.store(true, Ordering::SeqCst);
                std::fs::write(worker_marker, b"own fixture only").map_err(|_| invalid())?;
                Ok(())
            }));
            std::future::poll_fn(|cx| match worker.as_mut().poll(cx) {
                Poll::Pending => Poll::Ready(()),
                Poll::Ready(_) => {
                    panic!("worker cannot complete through occupied single-slot blocking pool")
                }
            })
            .await;
            assert_eq!(
                subject.owner.automation.io_slots.available_permits(),
                3,
                "actual registered worker retains IOpermit while queued"
            );
            let mut locking = Box::pin(f.app.lock());
            let mut completed = None;
            std::future::poll_fn(|cx| {
                if let Poll::Ready(result) = locking.as_mut().poll(cx) {
                    completed = Some(result);
                }
                assert!(
                    subject.fence.registry_epoch.is_cancelled(),
                    "the actual native lock reached synchronous registryclose before release"
                );
                Poll::Ready(())
            })
            .await;
            release_tx.send(()).unwrap();
            blocker.await.unwrap();
            assert!(
                tokio::time::timeout(Duration::from_secs(10), &mut worker)
                    .await
                    .unwrap()
                    .is_err()
            );
            if let Some(result) = completed {
                result.unwrap();
            } else {
                tokio::time::timeout(Duration::from_secs(10), &mut locking)
                    .await
                    .unwrap()
                    .unwrap();
            }
            assert!(
                !entered.load(Ordering::SeqCst),
                "post-lock queued callback never begins; final reply fencing alone is insufficient"
            );
            assert!(!marker.exists());
            assert_eq!(subject.owner.automation.io_slots.available_permits(), 4);
            assert!(subject.fence.check().is_err());
            drop(worker);
            drop(locking);
            drop(subject);
            f.app.shutdown().await.unwrap();
        });
    })
    .join()
    .unwrap();
}
#[test]
fn registry_close_is_synchronous_and_reopen_never_revives_old_native_fs_epoch() {
    let registry = dependencies::DependencyRegistry::default();
    let original = registry.local_filesystem_epoch().unwrap();
    assert!(!original.is_cancelled());
    registry.close();
    assert!(original.is_cancelled());
    assert!(registry.local_filesystem_epoch().is_err());
    registry.reopen();
    assert!(original.is_cancelled());
    let fresh = registry.local_filesystem_epoch().unwrap();
    assert!(!fresh.is_cancelled());
    fresh.cancel();
    assert!(original.is_cancelled());
    assert!(
        !registry.local_filesystem_epoch().unwrap().is_cancelled(),
        "cancelling one local worker never closes siblings or normal jobs"
    );
}

/// The UI consumes this actual Native serialization projection, never a regex
/// interpretation of Rust source or values from a user profile. Only keys,
/// JSON types, fixed supported-state enums and published byte caps leave the test.
#[tokio::test]
async fn actual_native_serde_wire_shapes_and_bounds_for_portable_ui() {
    fn shape(value: &Value) -> Value {
        match value {
            Value::Null => json!("null"),
            Value::Bool(_) => json!("boolean"),
            Value::Number(number) => {
                assert!(number.is_u64() || number.is_i64());
                json!("integer")
            }
            Value::String(_) => json!("string"),
            Value::Array(values) => {
                let mut distinct = Vec::new();
                for value in values {
                    let next = shape(value);
                    if !distinct.contains(&next) {
                        distinct.push(next);
                    }
                }
                json!({"type":"array","itemShapes":distinct})
            }
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(key, value)| (key.clone(), shape(value)))
                    .collect(),
            ),
        }
    }
    let _serial = serial().lock().await;
    let f = Fixture::new().await;
    let context = f.context().await;
    let before = f
        .app
        .automation_project_files(f.project.clone())
        .await
        .unwrap();
    let before_catalog = serde_json::to_value(f.app.automation_projects().await.unwrap()).unwrap();
    let context_wire = serde_json::to_value(&context).unwrap();
    assert!(serde_json::to_vec(&context_wire).unwrap().len() <= 512 * 1024);
    let request = f.request(&context);
    assert!(PortableExportRequest::parse(&request).is_ok());
    // Trailing whitespace is VALID JSON. Without the Native byte-cap check,
    // this exact payload would still deserialize to the same supported request.
    let oversized = format!("{request}{}", " ".repeat(256 * 1024 + 1 - request.len()));
    assert!(serde_json::from_str::<Value>(&oversized).is_ok());
    assert!(PortableExportRequest::parse(&oversized).is_err());
    assert!(!f.destination().exists());
    let outcome = f
        .app
        .automation_portable_export(
            request,
            SecretString::from("separate synthetic wire fixture passphrase"),
            f.destination(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    let inspected = f
        .app
        .automation_portable_inspect(
            context.scope.clone(),
            f.destination(),
            SecretString::from("separate synthetic wire fixture passphrase"),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    let outcome_wire = serde_json::to_value(&outcome).unwrap();
    let inspected_wire = serde_json::to_value(&inspected).unwrap();
    assert!(outcome.bytes <= cc_crypto_core::MAX_PORTABLE_PROJECT_BYTES);
    assert_eq!(outcome.encrypted_sha256, inspected.encrypted_sha256);
    assert_eq!(context_wire["scope"]["version"], 1);
    assert_eq!(outcome_wire["version"], 1);
    assert_eq!(inspected_wire["version"], 1);
    assert_eq!(inspected_wire["publicationSupported"], false);
    assert_eq!(inspected_wire["reason"], "managed_publication_required");
    let shapes = json!({
        "context":shape(&context_wire),
        "exportOutcome":shape(&outcome_wire),
        "importPreview":shape(&inspected_wire),
    });
    assert_eq!(
        shapes,
        json!({
            "context":{
                "scope":{"version":"integer","profileId":"string","notificationScopeId":"string"},
                "projectId":"string","projectRevision":"integer","revisionDigest":"string","projectStamp":"string",
                "choices":{"type":"array","itemShapes":[{"path":"string","bytes":"integer","directory":"boolean","selectable":"boolean"}]}
            },
            "exportOutcome":{"version":"integer","encryptedSha256":"string","bytes":"integer"},
            "importPreview":{
                "version":"integer","encryptedSha256":"string","publicationSupported":"boolean","reason":"string",
                "summary":{"sourceDigest":"string","portableDigest":"string","files":"integer","bytes":"integer","exclusions":"integer"}
            }
        })
    );
    let witness = json!({
        "version":1,
        "origin":"actualNativeOwnedFixtureSerde",
        "shapes":shapes,
        "supportedValues":{
            "scopeVersion":context_wire["scope"]["version"],
            "exportVersion":outcome_wire["version"],
            "importVersion":inspected_wire["version"],
            "publicationSupported":inspected_wire["publicationSupported"],
            "inspectionReason":inspected_wire["reason"],
        },
        "byteLimits":{
            "contextJson":512 * 1024,
            "exportRequestJson":256 * 1024,
            "encryptedPackage":cc_crypto_core::MAX_PORTABLE_PROJECT_BYTES,
        },
    });
    let encoded = serde_json::to_string(&witness).unwrap();
    assert!(encoded.len() <= 4096);
    // A changed projector must not accidentally emit identifiers, chosen paths,
    // synthetic plaintext or password, even from this private test fixture.
    for omitted in [
        f.project.as_str(),
        context.scope.profile_id.as_str(),
        context.scope.notification_scope_id.as_str(),
        "selected.yml",
        "SYNTHETIC_EXCLUDED_VALUE",
        "separate synthetic wire fixture passphrase",
    ] {
        assert!(!encoded.contains(omitted));
    }
    assert_eq!(
        f.app
            .automation_project_files(f.project.clone())
            .await
            .unwrap(),
        before
    );
    assert_eq!(
        serde_json::to_value(f.app.automation_projects().await.unwrap()).unwrap(),
        before_catalog
    );
    f.app.shutdown().await.unwrap();
    println!("PORTABLE_NATIVE_WIRE_V1 {encoded}");
}
