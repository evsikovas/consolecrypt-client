//! Genuine interrupted prepared2 fixture. No fabricated Ready/package/receipt.
//! The interruption is an explicit drop after actual durable SQL; this is NOT
//! an OS crash or power-loss simulation.
use super::*;

const PROFILE_PASSWORD: &str = "owned prepared recovery profile password42";
const PACKAGE_PASSWORD: &str = "separate owned prepared recovery package password";
const TEXT: &str = "---\n- hosts: all\n  tasks: []\n";

fn owned_root() -> PathBuf {
    let root = PathBuf::from(std::env::var_os("CC_IMPORT_ACCEPTANCE_ROOT").unwrap());
    let token = std::env::var("CC_IMPORT_ACCEPTANCE_TOKEN").unwrap();
    let id = Uuid::parse_str(&token).unwrap();
    assert!(!id.is_nil());
    assert_eq!(id.to_string(), token);
    assert!(root.is_absolute());
    assert_eq!(
        root.file_name().unwrap(),
        std::ffi::OsStr::new(&format!("owned-{token}"))
    );
    assert!(!std::fs::symlink_metadata(&root)
        .unwrap()
        .file_type()
        .is_symlink());
    let owner: Value =
        serde_json::from_slice(&std::fs::read(root.join("owner.json")).unwrap()).unwrap();
    assert_eq!(
        owner,
        json!({"token":token,"label":"portable-import-acceptance"})
    );
    let names: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, [std::ffi::OsString::from("owner.json")]);
    root
}

#[tokio::test]
#[ignore = "explicit new private root supplied by reviewed prepared-recovery-v1 driver"]
async fn genuine_prepared2_interruption_recovers_only_after_fresh_native_review_and_acceptance() {
    let control = owned_root();
    let data = control.join("profile-data");
    std::fs::create_dir(&data).unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        data.to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("Owned prepared2 recovery".into(), PROFILE_PASSWORD.into())
        .await
        .unwrap();
    let (owner, u) = app.unlocked().await.unwrap();
    drop(u);
    let scope = app.automation_portable_scope().await.unwrap();
    let original = app
        .automation_project_create("Original source stays untouched".into(), None)
        .await
        .unwrap();
    app.automation_project_save(
        original.id.clone(),
        "selected.yml".into(),
        None,
        TEXT.into(),
    )
    .await
    .unwrap();
    let original_files = app
        .automation_project_files(original.id.clone())
        .await
        .unwrap();
    let storage = owner.storage().unwrap();
    let before: Value = storage
        .read(|tx| tx.setting_get::<Value>(PROJECTS))
        .await
        .unwrap()
        .unwrap();
    let original_revision = before["records"][&original.id]["revision"]
        .as_u64()
        .unwrap();
    let context = app
        .automation_portable_context(
            scope.clone(),
            original.id.clone(),
            original_revision,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    let request = json!({"scope":context.scope,"projectId":original.id,"projectRevision":context.project_revision,
        "revisionDigest":context.revision_digest,"projectStamp":context.project_stamp,
        "selectedPaths":["selected.yml"],"playbook":"selected.yml"}).to_string();
    let source_file = control.join("owned-recovery.ccproject");
    let exported = app
        .automation_portable_export(
            request,
            SecretString::from(PACKAGE_PASSWORD),
            source_file.clone(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    let encrypted = std::fs::read(&source_file).unwrap();
    assert_eq!(digest(&encrypted), exported.encrypted_sha256);
    let package = portable_package::decrypt_package(
        &encrypted,
        &SecretString::from(PACKAGE_PASSWORD),
        &CancellationToken::new(),
    )
    .unwrap();
    let summary = package.summary();
    let project = Uuid::new_v4();
    let operation = Uuid::new_v4();
    let transaction = Uuid::new_v4();
    let id = project.to_string();
    let profile = owner.dir.clone().unwrap();
    // Establish the real process-interruption precondition using the SAME
    // import catalog constructors, FULL storage worker and genuine Core copier.
    // No production fault hook or journal-to-approval constructor is introduced.
    let row = import_catalog::reservation(
        &profile,
        project,
        operation,
        transaction,
        "Interrupted owned import",
        &exported.encrypted_sha256,
        &summary,
        chrono::Utc::now().timestamp(),
    )
    .unwrap();
    let reserve_sha = managed::row_stamp(&row).unwrap();
    let row_id = id.clone();
    storage
        .write_project_publication(move |tx| {
            let mut catalog: Catalog = tx.setting_get(PROJECTS)?.unwrap_or_default();
            assert_eq!(catalog.version, 1);
            assert!(
                catalog.extra.is_empty()
                    && catalog.records.len() < 256
                    && !catalog.records.contains_key(&row_id)
            );
            catalog.records.insert(row_id, row);
            tx.setting_set(PROJECTS, &catalog)?;
            Ok::<_, AppError>(())
        })
        .await
        .unwrap();
    let source = cc_automation_core::portable_publication::PortablePublicationSource::new(
        &profile,
        project,
        operation,
        transaction,
        &reserve_sha,
    )
    .unwrap();
    let prepared = tokio::task::spawn_blocking(move || {
        cc_automation_core::portable_publication::prepare_portable_project(
            source,
            package,
            &CancellationToken::new(),
        )
    })
    .await
    .unwrap()
    .unwrap();
    let fingerprint = prepared.fingerprint().unwrap();
    let expected_reserve = reserve_sha.clone();
    let row_id = id.clone();
    let expected_fp = fingerprint.clone();
    let prepared_sha = storage
        .write_project_publication(move |tx| {
            let mut catalog: Catalog = tx.setting_get(PROJECTS)?.unwrap();
            let row = catalog.records.get(&row_id).unwrap();
            let row = import_catalog::prepared(
                row,
                &expected_reserve,
                project,
                operation,
                transaction,
                &expected_fp,
                chrono::Utc::now().timestamp(),
            )?;
            let sha = managed::row_stamp(&row)?;
            catalog.records.insert(row_id, row);
            tx.setting_set(PROJECTS, &catalog)?;
            Ok::<_, AppError>(sha)
        })
        .await
        .unwrap();
    drop(prepared); // explicit interrupted pre-rename point, no install claim
    let family = profile
        .join("automation")
        .join("project-revisions")
        .join(&id);
    assert!(family.join(format!("stage-{transaction}")).is_dir());
    assert!(!family.join(format!("generation-{transaction}")).exists());
    let pending: Value = storage
        .read(|tx| tx.setting_get::<Value>(PROJECTS))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending["records"][&id]["revision"], 2);
    assert_eq!(
        managed::row_stamp(&pending["records"][&id]).unwrap(),
        prepared_sha
    );
    assert!(
        app.automation_project_files(id.clone()).await.is_err(),
        "prepared is never an ordinary project source"
    );

    app.lock().await.unwrap();
    app.unlock_with_passphrase(PROFILE_PASSWORD.into())
        .await
        .unwrap();
    // Lock correctly closed the original sealed-profile connection. Observe
    // the newly opened connection, without retaining StrongUnlocked over I/O.
    let storage = {
        let (current_owner, unlocked) = app.unlocked().await.unwrap();
        let current = current_owner.storage().unwrap();
        drop(unlocked);
        current
    };
    let fresh_scope = app.automation_portable_scope().await.unwrap();
    assert_ne!(
        fresh_scope.notification_scope_id,
        scope.notification_scope_id
    );
    let recovery = app
        .portable_register_import_recovery(
            serde_json::to_string(&fresh_scope).unwrap(),
            source_file.to_string_lossy().into_owned(),
            id.clone(),
            PortablePasswordTransfer::from_bytes(PACKAGE_PASSWORD.as_bytes().to_vec()).unwrap(),
        )
        .await
        .unwrap();
    let ready: Value = serde_json::from_str(&recovery.start().await.unwrap()).unwrap();
    assert_eq!(ready["scope"], serde_json::to_value(&fresh_scope).unwrap());
    assert_eq!(ready["projectId"], id);
    assert_eq!(ready["publicationSupported"], true);
    assert_eq!(ready["encryptedSha256"], exported.encrypted_sha256);
    assert!(!recovery.control.joined.is_cancelled());
    let after_preview: Value = storage
        .read(|tx| tx.setting_get::<Value>(PROJECTS))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after_preview, pending, "recovery review is read-only");
    assert!(family.join(format!("stage-{transaction}")).exists());
    assert!(!family.join(format!("generation-{transaction}")).exists());
    let installed: Value = serde_json::from_str(
        &recovery
            .accept_import(
                ready["reviewId"].as_str().unwrap().into(),
                ready["reviewDigest"].as_str().unwrap().into(),
            )
            .await
            .unwrap(),
    )
    .unwrap();
    tokio::time::timeout(Duration::from_secs(10), recovery.joined())
        .await
        .unwrap();
    assert_eq!(installed["state"], "installed");
    assert_eq!(installed["projectId"], id);
    assert_eq!(installed["projectRevision"], 3);
    assert!(!family.join(format!("stage-{transaction}")).exists());
    assert!(family.join(format!("generation-{transaction}")).is_dir());
    let final_catalog: Value = storage
        .read(|tx| tx.setting_get::<Value>(PROJECTS))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        final_catalog["records"].as_object().unwrap().len(),
        pending["records"].as_object().unwrap().len()
    );
    assert_eq!(final_catalog["records"][&id]["revision"], 3);
    assert_eq!(
        final_catalog["records"][&id]["portableImport"]["publicationFingerprint"],
        fingerprint
    );
    assert_eq!(
        final_catalog["records"][&original.id],
        before["records"][&original.id]
    );
    assert_eq!(
        app.automation_project_file(id.clone(), "selected.yml".into())
            .await
            .unwrap()
            .text,
        TEXT
    );
    assert_eq!(
        app.automation_project_files(original.id.clone())
            .await
            .unwrap(),
        original_files
    );
    assert_eq!(std::fs::read(&source_file).unwrap(), encrypted);
    assert_eq!(owned_setup::test_count(&owner), 0);
    assert_eq!(
        owner.automation.io_slots.available_permits(),
        crate::automation_runners::AUTOMATION_IO_CAPACITY as usize
    );
    let selected = app.automation_project_context(&id).await.unwrap();
    let pin = selected.root_selection.open().unwrap();
    assert!(matches!(
        &pin,
        crate::automation_projects::managed::OpenedProjectRoot::Portable(_)
    ));
    pin.recheck().unwrap();
    drop(pin);
    drop(selected);
    drop(recovery);
    app.shutdown().await.unwrap();
    println!("portable_import_prepared_recovery: genuinePackage=true durablePrepared2=true freshScope=true explicitAccept=true ready3=true originalUnchanged=true simulatedInterruptionOnly=true");
}
