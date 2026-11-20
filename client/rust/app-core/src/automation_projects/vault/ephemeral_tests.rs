use super::*;
use cc_models::secret::SecretValue;

#[tokio::test]
async fn one_launch_password_is_never_a_saved_credential_or_project_file_and_is_scope_revocable() {
    let directory=tempfile::tempdir().unwrap();
    let app=AppCore::new(crate::AppConfig::for_tests(directory.path().to_string_lossy().into_owned())).unwrap();
    app.create_local_profile("Ephemeral Vault fixture".into(),"synthetic profile passphrase42".into()).await.unwrap();
    let project=app.automation_project_create("One-shot fixture".into(),None).await.unwrap();
    let context=app.automation_project_context(&project.id).await.unwrap();
    let before=app.automation_project_files(project.id.clone()).await.unwrap();
    let cancel=CancellationToken::new();
    let review=app.automation_ephemeral_vault_capture(context.clone(),before.digest.clone(),SecretValue::new("synthetic-vault-password"),&cancel).await.unwrap();
    assert!(review.metadata.is_none());assert!(review.secret.is_none());assert!(review.binding.is_ephemeral());
    assert!(app.list_credentials().await.unwrap().is_empty());
    assert_eq!(app.automation_project_files(project.id.clone()).await.unwrap(),before);
    assert!(!context.path.join(cc_automation_core::VAULT_INPUT_FILE).exists());
    let u=context.unlock.upgrade().unwrap();review.check_held(&u).await.unwrap();
    assert_eq!(review.password_value(&u).await.unwrap().expose_secret(),"synthetic-vault-password");
    cancel.cancel();assert!(review.check_held(&u).await.is_err());
}

#[tokio::test]
async fn saved_reference_and_original_context_change_cannot_be_reinterpreted_as_ephemeral_authority() {
    let directory=tempfile::tempdir().unwrap();
    let app=AppCore::new(crate::AppConfig::for_tests(directory.path().to_string_lossy().into_owned())).unwrap();
    app.create_local_profile("Vault scope fixture".into(),"synthetic profile passphrase42".into()).await.unwrap();
    let project=app.automation_project_create("Original fixture".into(),None).await.unwrap();
    let context=app.automation_project_context(&project.id).await.unwrap();
    let before=app.automation_project_files(project.id.clone()).await.unwrap();
    std::fs::write(context.path.join(cc_automation_core::VAULT_INPUT_FILE),
        json!({"version":1,"ansible_vault":{"credential_id":Uuid::new_v4()}}).to_string()).unwrap();
    let changed=app.automation_project_files(project.id.clone()).await.unwrap();
    assert!(app.automation_ephemeral_vault_capture(context.clone(),changed.digest,SecretValue::new("synthetic"),&CancellationToken::new()).await.is_err());
    std::fs::remove_file(context.path.join(cc_automation_core::VAULT_INPUT_FILE)).unwrap();
    app.lock().await.unwrap();
    assert!(app.automation_ephemeral_vault_capture(context,before.digest,SecretValue::new("synthetic"),&CancellationToken::new()).await.is_err());
}
