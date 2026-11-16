//! Explicit owned-fixture gate: genuine Native export -> retained review ->
//! explicit acceptance -> durable ready catalog -> ordinary pinned read/edit.
//! No manufactured package, imported identity, credential lookup or repair.
use super::*;

const PROFILE_PASSWORD: &str = "owned native import fixture password42";
const PACKAGE_PASSWORD: &str = "separate synthetic portable package passphrase";
const ORIGINAL_TEXT: &str = "---\n- hosts: all\n  tasks: []\n";
const SECOND_EDITED_TEXT: &str =
    "---\n- hosts: all\n  gather_facts: false\n  tasks: []\n# legitimate second local edit\n";
const EDITED_TEXT: &str = "---\n- hosts: all\n  gather_facts: false\n  tasks: []\n";

fn owned_root() -> PathBuf {
    let root = PathBuf::from(
        std::env::var_os("CC_IMPORT_ACCEPTANCE_ROOT")
            .expect("run only through the reviewed owned acceptance driver"),
    );
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
    let entries: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(entries, [std::ffi::OsString::from("owner.json")]);
    root
}

async fn raw_catalog(owner: &Session) -> Value {
    owner
        .storage()
        .unwrap()
        .read(|tx| tx.setting_get::<Value>(PROJECTS))
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
#[ignore = "explicit new private owned Windows/Linux root, supplied by reviewed acceptance-v1 driver"]
async fn genuine_native_import_accepts_then_reopens_edits_and_preserves_original() {
    let control = owned_root();
    // The driver's private DACL/mode precedes this creation. No existing profile
    // or ancestor ACL is changed by Native or by this test.
    let data = control.join("profile-data");
    std::fs::create_dir(&data).unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        data.to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("Owned positive import".into(), PROFILE_PASSWORD.into())
        .await
        .unwrap();
    let (owner, unlocked) = app.unlocked().await.unwrap();
    drop(unlocked);
    let scope = app.automation_portable_scope().await.unwrap();
    let original = app
        .automation_project_create("Original project stays unchanged".into(), None)
        .await
        .unwrap();
    app.automation_project_save(
        original.id.clone(),
        "selected.yml".into(),
        None,
        ORIGINAL_TEXT.into(),
    )
    .await
    .unwrap();
    app.automation_project_save(
        original.id.clone(),
        ".env".into(),
        None,
        "OWNED_EXCLUDED_VALUE=never_import\n".into(),
    )
    .await
    .unwrap();
    let before_files = app
        .automation_project_files(original.id.clone())
        .await
        .unwrap();
    let before_catalog = raw_catalog(&owner).await;
    let original_revision = before_catalog["records"][&original.id]["revision"]
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
    let package_path = control.join("owned-package.ccproject");
    let exported = app
        .automation_portable_export(
            request,
            SecretString::from(PACKAGE_PASSWORD),
            package_path.clone(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    let encrypted_before = std::fs::read(&package_path).unwrap();
    assert_eq!(digest(&encrypted_before), exported.encrypted_sha256);
    assert_eq!(raw_catalog(&owner).await, before_catalog);

    // Same actual package review, then an actual caller-owned ciphertext edit.
    // Accept must detect its captured source change BEFORE reservation SQL.
    let changed = app
        .automation_portable_register_import_json(
            serde_json::to_string(&scope).unwrap(),
            package_path.to_string_lossy().into_owned(),
            "Changed source must not install".into(),
            PortablePasswordTransfer::from_bytes(PACKAGE_PASSWORD.as_bytes().to_vec()).unwrap(),
        )
        .await
        .unwrap();
    let changed_ready: Value = serde_json::from_str(&changed.start().await.unwrap()).unwrap();
    assert_eq!(changed_ready["publicationSupported"], true);
    let mut modified = encrypted_before.clone();
    let last = modified.len() - 1;
    modified[last] ^= 1;
    std::fs::write(&package_path, &modified).unwrap();
    assert!(changed
        .accept_import(
            changed_ready["reviewId"].as_str().unwrap().into(),
            changed_ready["reviewDigest"].as_str().unwrap().into()
        )
        .await
        .is_err());
    tokio::time::timeout(Duration::from_secs(10), changed.joined())
        .await
        .unwrap();
    assert!(!changed
        .control
        .import_durable
        .load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(raw_catalog(&owner).await, before_catalog);
    assert_eq!(owned_setup::test_count(&owner), 0);
    drop(changed);
    // Restoration is explicit fixture setup, never a silent operation retry.
    std::fs::write(&package_path, &encrypted_before).unwrap();

    let operation = app
        .automation_portable_register_import_json(
            serde_json::to_string(&scope).unwrap(),
            package_path.to_string_lossy().into_owned(),
            "  Imported selection  ".into(),
            PortablePasswordTransfer::from_bytes(PACKAGE_PASSWORD.as_bytes().to_vec()).unwrap(),
        )
        .await
        .unwrap();
    let ready: Value = serde_json::from_str(&operation.start().await.unwrap()).unwrap();
    assert_eq!(ready["scope"], serde_json::to_value(&scope).unwrap());
    assert_eq!(ready["name"], "Imported selection");
    assert_eq!(ready["requestedName"], "  Imported selection  ");
    assert_eq!(ready["encryptedSha256"], exported.encrypted_sha256);
    assert_eq!(
        ready["publicationSupported"], true,
        "actual owned private root must pass unchanged production policy: {}",
        ready["publicationReason"]
    );
    assert_eq!(ready["summary"]["files"], 1);
    let imported = ready["projectId"].as_str().unwrap().to_owned();
    assert_ne!(imported, original.id, "Native issues a fresh catalog ID");
    assert_eq!(
        raw_catalog(&owner).await,
        before_catalog,
        "ready preview is not catalog selection"
    );
    assert!(!operation.control.joined.is_cancelled());
    assert_eq!(owned_setup::test_count(&owner), 1);
    assert!(operation
        .accept_import(ready["reviewId"].as_str().unwrap().into(), "0".repeat(64))
        .await
        .is_err());
    assert_eq!(raw_catalog(&owner).await, before_catalog);

    let installed: Value = serde_json::from_str(
        &operation
            .accept_import(
                ready["reviewId"].as_str().unwrap().into(),
                ready["reviewDigest"].as_str().unwrap().into(),
            )
            .await
            .unwrap(),
    )
    .unwrap();
    tokio::time::timeout(Duration::from_secs(10), operation.joined())
        .await
        .unwrap();
    assert_eq!(installed["state"], "installed");
    assert_eq!(installed["projectId"], imported);
    assert_eq!(installed["projectRevision"], 3);
    assert_eq!(installed["scope"], serde_json::to_value(&scope).unwrap());
    assert_eq!(owned_setup::test_count(&owner), 0);
    assert_eq!(owner.automation.prepare_slots.available_permits(), 2);
    assert_eq!(
        owner.automation.io_slots.available_permits(),
        crate::automation_runners::AUTOMATION_IO_CAPACITY as usize
    );
    assert!(operation
        .accept_import(
            ready["reviewId"].as_str().unwrap().into(),
            ready["reviewDigest"].as_str().unwrap().into()
        )
        .await
        .is_err());
    let selected_catalog = raw_catalog(&owner).await;
    assert_eq!(
        selected_catalog["records"].as_object().unwrap().len(),
        before_catalog["records"].as_object().unwrap().len() + 1
    );
    assert_eq!(
        selected_catalog["records"][&original.id],
        before_catalog["records"][&original.id]
    );
    let row = &selected_catalog["records"][&imported];
    assert_eq!(row["revision"], 3);
    assert_eq!(row["source"]["kind"], "managed");
    assert_eq!(row["source"]["status"], "ready");
    assert_eq!(row["portableImport"]["state"], "ready");
    assert_eq!(
        row["portableImport"]["encryptedSha256"],
        exported.encrypted_sha256
    );
    for forbidden in [
        "inventory",
        "runner_id",
        "folder_bindings",
        "external_inventory",
        "publication",
    ] {
        assert!(
            row.get(forbidden).is_none(),
            "package metadata must not import authority: {forbidden}"
        );
    }
    // A NEW original-issued review and fresh genuine authenticated package,
    // not a journal-created Ready. Current-ready3 recovery changes no SQL.
    let recovery = app
        .portable_register_import_recovery(
            serde_json::to_string(&scope).unwrap(),
            package_path.to_string_lossy().into_owned(),
            imported.clone(),
            PortablePasswordTransfer::from_bytes(PACKAGE_PASSWORD.as_bytes().to_vec()).unwrap(),
        )
        .await
        .unwrap();
    let recovery_ready: Value = serde_json::from_str(&recovery.start().await.unwrap()).unwrap();
    assert_eq!(recovery_ready["projectId"], imported);
    assert_eq!(recovery_ready["publicationSupported"], true);
    assert!(!recovery.control.joined.is_cancelled());
    let recovered: Value = serde_json::from_str(
        &recovery
            .accept_import(
                recovery_ready["reviewId"].as_str().unwrap().into(),
                recovery_ready["reviewDigest"].as_str().unwrap().into(),
            )
            .await
            .unwrap(),
    )
    .unwrap();
    tokio::time::timeout(Duration::from_secs(10), recovery.joined())
        .await
        .unwrap();
    assert_eq!(recovered["state"], "installed");
    assert_eq!(recovered["projectId"], imported);
    assert_eq!(raw_catalog(&owner).await, selected_catalog);
    assert_eq!(owned_setup::test_count(&owner), 0);
    drop(recovery);

    let pinned = app.automation_project_context(&imported).await.unwrap();
    assert_eq!(pinned.project.revision, Some(3));
    assert!(pinned.inventory.groups.is_empty());
    assert!(pinned.inventory.selected_hosts.is_none());
    let opened = pinned.root_selection.open().unwrap();
    assert!(matches!(
        &opened,
        crate::automation_projects::managed::OpenedProjectRoot::Portable(_)
    ));
    opened.recheck().unwrap();
    drop(opened);
    drop(pinned);
    let imported_files = app
        .automation_project_files(imported.clone())
        .await
        .unwrap();
    assert_eq!(
        imported_files.digest,
        installed["revisionDigest"].as_str().unwrap()
    );
    assert_eq!(imported_files.entries.len(), 1);
    assert_eq!(imported_files.entries[0].path(), "selected.yml");
    assert!(app
        .automation_project_file(imported.clone(), ".env".into())
        .await
        .is_err());
    let file = app
        .automation_project_file(imported.clone(), "selected.yml".into())
        .await
        .unwrap();
    assert_eq!(file.text, ORIGINAL_TEXT);
    let edited = app
        .automation_project_save(
            imported.clone(),
            "selected.yml".into(),
            Some(file.sha256),
            EDITED_TEXT.into(),
        )
        .await
        .unwrap();
    assert_eq!(edited.text, EDITED_TEXT);
    // Historical metadata recovery must fail after a legitimate content edit;
    // the ordinary selected-root pin continues to permit normal local editing.
    let edited_recovery = app
        .portable_register_import_recovery(
            serde_json::to_string(&scope).unwrap(),
            package_path.to_string_lossy().into_owned(),
            imported.clone(),
            PortablePasswordTransfer::from_bytes(PACKAGE_PASSWORD.as_bytes().to_vec()).unwrap(),
        )
        .await
        .unwrap();
    assert!(edited_recovery.start().await.is_err());
    edited_recovery.cancel();
    tokio::time::timeout(Duration::from_secs(10), edited_recovery.joined())
        .await
        .unwrap();
    drop(edited_recovery);
    let second = app
        .automation_project_save(
            imported.clone(),
            "selected.yml".into(),
            Some(edited.sha256),
            SECOND_EDITED_TEXT.into(),
        )
        .await
        .unwrap();
    assert_eq!(second.text, SECOND_EDITED_TEXT);

    assert_ne!(
        app.automation_project_files(imported.clone())
            .await
            .unwrap()
            .digest,
        imported_files.digest,
        "selected-root pin allows legitimate edits instead of comparing historical content"
    );
    assert_eq!(
        app.automation_project_files(original.id.clone())
            .await
            .unwrap(),
        before_files
    );
    assert_eq!(
        raw_catalog(&owner).await["records"][&original.id],
        before_catalog["records"][&original.id]
    );
    assert_eq!(std::fs::read(&package_path).unwrap(), encrypted_before);
    drop(operation);
    app.lock().await.unwrap();
    app.unlock_with_passphrase(PROFILE_PASSWORD.into())
        .await
        .unwrap();
    assert_ne!(
        app.automation_portable_scope()
            .await
            .unwrap()
            .notification_scope_id,
        scope.notification_scope_id
    );
    assert_eq!(
        app.automation_project_file(imported.clone(), "selected.yml".into())
            .await
            .unwrap()
            .text,
        SECOND_EDITED_TEXT
    );
    app.shutdown().await.unwrap();
    drop(owner);
    drop(app);

    // Actual SQLCipher reopen from the original new test profile directory.
    let reopened = AppCore::new(crate::AppConfig::for_tests(
        data.to_string_lossy().into_owned(),
    ))
    .unwrap();
    let profiles = reopened.list_profiles().await.unwrap();
    assert_eq!(profiles.len(), 1);
    reopened.open_profile(profiles[0].id.clone()).await.unwrap();
    reopened
        .unlock_with_passphrase(PROFILE_PASSWORD.into())
        .await
        .unwrap();
    assert_eq!(
        reopened
            .automation_project_file(imported, "selected.yml".into())
            .await
            .unwrap()
            .text,
        SECOND_EDITED_TEXT
    );
    reopened.shutdown().await.unwrap();
    println!("portable_import_acceptance: genuineExport=true explicitAccept=true readyRevision=3 pinnedEdit=true readyRecovery=true sourceChangeDenied=true editedHistoryDenied=true reopen=true originalUnchanged=true");
}
