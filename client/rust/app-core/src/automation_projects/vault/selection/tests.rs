use super::*;
struct Fixture {
    _temp: tempfile::TempDir,
    app: AppCore,
    project: String,
    credential: String,
}
impl Fixture {
    async fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            temp.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile(
            "Vault selection test".into(),
            "synthetic profile passphrase42".into(),
        )
        .await
        .unwrap();
        let project = app
            .automation_project_create("Vault selection".into(), None)
            .await
            .unwrap()
            .id;
        let credential = app
            .add_password_credential(
                "Explicit Vault selection".into(),
                None,
                "synthetic-vault-password42".into(),
            )
            .await
            .unwrap()
            .id;
        Self {
            _temp: temp,
            app,
            project,
            credential,
        }
    }
    async fn context(&self) -> Value {
        serde_json::from_str(
            &self
                .app
                .automation_project_vault_context_json(self.project.clone())
                .await
                .unwrap(),
        )
        .unwrap()
    }
    async fn select(&self, frame: &Value) -> AppResult<String> {
        self.app
            .automation_project_vault_select_json(
                json!({"frame":frame,"credentialId":self.credential}).to_string(),
            )
            .await
    }
}
#[tokio::test]
async fn explicit_native_selection_writes_only_fixed_reference_and_requires_fresh_tree() {
    let f = Fixture::new().await;
    let before = f.context().await;
    assert!(before["referencedCredentialId"].is_null());
    let after: Value = serde_json::from_str(&f.select(&before["frame"]).await.unwrap()).unwrap();
    assert_eq!(after["referencedCredentialId"], f.credential);
    assert_ne!(
        before["frame"]["revisionDigest"],
        after["frame"]["revisionDigest"]
    );
    let source = f.app.automation_project_context(&f.project).await.unwrap();
    let text =
        std::fs::read_to_string(source.path.join(cc_automation_core::VAULT_INPUT_FILE)).unwrap();
    assert!(!text.contains("synthetic-vault-password"));
    assert!(f.select(&before["frame"]).await.is_err());
    let revision = f
        .app
        .automation_project_files(f.project.clone())
        .await
        .unwrap();
    f.app
        .automation_project_save(
            f.project.clone(),
            "changed.yml".into(),
            None,
            "---\n".into(),
        )
        .await
        .unwrap();
    assert!(f.select(&after["frame"]).await.is_err());
    assert_ne!(
        revision.digest,
        f.app
            .automation_project_files(f.project.clone())
            .await
            .unwrap()
            .digest
    );
}
#[tokio::test]
async fn original_unlock_nonce_and_same_byte_other_directory_are_not_selection_authority() {
    let f = Fixture::new().await;
    let before = f.context().await;
    f.app.lock().await.unwrap();
    f.app
        .unlock_with_passphrase("synthetic profile passphrase42".into())
        .await
        .unwrap();
    assert!(f.select(&before["frame"]).await.is_err());
    let current = f.context().await;
    assert_ne!(
        before["frame"]["notificationScopeId"],
        current["frame"]["notificationScopeId"]
    );
    let mut forged = current["frame"].clone();
    forged["directoryIdentity"] = json!("0".repeat(64));
    assert!(f.select(&forged).await.is_err());
    assert!(f
        .app
        .automation_project_file(
            f.project.clone(),
            cc_automation_core::VAULT_INPUT_FILE.into()
        )
        .await
        .is_err());
}
#[tokio::test]
async fn future_reference_schema_and_duplicate_input_never_overwrite_existing_bytes() {
    let f = Fixture::new().await;
    let before = f.context().await;
    let malformed = format!(
        "{{\"frame\":{},\"credentialId\":\"{}\",\"credentialId\":\"{}\"}}",
        before["frame"], f.credential, f.credential
    );
    assert!(f
        .app
        .automation_project_vault_select_json(malformed)
        .await
        .is_err());
    let future = "{\"version\":2,\"unknown\":true}";
    f.app
        .automation_project_save(
            f.project.clone(),
            cc_automation_core::VAULT_INPUT_FILE.into(),
            None,
            future.into(),
        )
        .await
        .unwrap();
    assert!(f
        .app
        .automation_project_vault_context_json(f.project.clone())
        .await
        .is_err());
    assert!(f.select(&before["frame"]).await.is_err());
    assert_eq!(
        f.app
            .automation_project_file(
                f.project.clone(),
                cc_automation_core::VAULT_INPUT_FILE.into()
            )
            .await
            .unwrap()
            .text,
        future
    );
}
