mod common;

use common::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn project_catalog_is_profile_local_unreadable_when_locked_and_does_not_need_ansible() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("проект с пробелами");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("playbook.yml"), "---\r\n- hosts: all\r\n").unwrap();
    let app = common::app(&temporary.path().join("app"));
    app.create_local_profile("A".into(), PASSPHRASE.into()).await.unwrap();
    let project = app.automation_project_create("Первый".into(), Some(source.to_string_lossy().into())).await.unwrap();
    assert!(!project.read_only);
    assert_eq!(app.automation_projects().await.unwrap().projects.len(), 1);
    let file = app.automation_project_file(project.id.clone(), "playbook.yml".into()).await.unwrap();
    assert_eq!(file.line_endings, "crlf");
    let revision = app.automation_project_files(project.id.clone()).await.unwrap();
    assert_eq!(revision.entries.len(), 1);
    app.lock().await.unwrap();
    assert!(app.automation_projects().await.is_err());
    assert!(app.automation_project_file(project.id.clone(), "playbook.yml".into()).await.is_err());
    assert!(app.automation_project_save(project.id.clone(), "playbook.yml".into(), Some(file.sha256.clone()), "locked".into()).await.is_err());
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert_eq!(app.automation_projects().await.unwrap().projects[0].name.as_deref(), Some("Первый"));
    app.create_local_profile("B".into(), PASSPHRASE.into()).await.unwrap();
    assert!(app.automation_projects().await.unwrap().projects.is_empty());
    assert!(app.automation_project_file(project.id, "playbook.yml".into()).await.is_err());
    assert_eq!(std::fs::read(source.join("playbook.yml")).unwrap(), b"---\r\n- hosts: all\r\n");
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn managed_project_edit_and_metadata_conflicts_preserve_manual_work() {
    let temporary = tempfile::tempdir().unwrap();
    let app = common::app(&temporary.path().join("app"));
    app.create_local_profile("Workspace".into(), PASSPHRASE.into()).await.unwrap();
    let project = app.automation_project_create("Managed".into(), None).await.unwrap();
    let original = app.automation_project_file(project.id.clone(), "playbook.yml".into()).await.unwrap();
    let updated = app.automation_project_save(project.id.clone(), "playbook.yml".into(), Some(original.sha256.clone()), "---\nmanual\n".into()).await.unwrap();
    assert_ne!(original.sha256, updated.sha256);
    assert!(app.automation_project_save(project.id.clone(), "playbook.yml".into(), Some(original.sha256), "stale".into()).await.is_err());
    let renamed = app.automation_project_rename(project.id.clone(), 1, "Renamed".into()).await.unwrap();
    assert_eq!(renamed.revision, Some(2));
    assert!(app.automation_project_rename(project.id.clone(), 1, "Stale name".into()).await.is_err());
    assert!(app.automation_project_file(project.id.clone(), "../outside".into()).await.is_err());
    assert_eq!(app.automation_project_file(project.id, "playbook.yml".into()).await.unwrap().text, "---\nmanual\n");
    app.shutdown().await.unwrap();
}
