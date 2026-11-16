//! Actual existing Native secret-store/explicit-selection issuer, not a mocked
//! exporter or a claim that the fixed SSH/service wiring already exists.
use super::*;
use cc_crypto_core::{DeviceSecretKeys, open_service_credential};

struct Fixture {
    _directory: tempfile::TempDir,
    app: AppCore,
    profile: String,
    credential: String,
    recipient: DeviceSecretKeys,
}
impl Fixture {
    async fn new(password: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(directory.path().to_string_lossy().into_owned())).unwrap();
        app.create_local_profile("Collector selected secret fixture".into(), "synthetic passphrase42".into()).await.unwrap();
        let profile = app.session().await.unwrap().profile_id.to_string();
        let credential = app.add_password_credential("Only selected collector credential".into(),
            Some("saved-exporter-user".into()), password.to_owned()).await.unwrap().id;
        Self { _directory: directory, app, profile, credential, recipient: DeviceSecretKeys::generate().unwrap() }
    }
    async fn review(&self) -> ServiceCredentialReview {
        self.app.service_credential_review(self.profile.clone(), self.credential.clone(), ServiceRecipientSelection {
            service_id: Uuid::new_v4(), recipient_public_key: self.recipient.public_keys().encryption,
            recipient_key_epoch: 1, recipient_certificate_epoch: 1,
            delegation_digest: "a".repeat(64), policy_revision: 1, revocation_epoch: 1,
            delegation_expires_at: now().unwrap() + 3600,
        }, &CancellationToken::new()).await.unwrap()
    }
}

#[tokio::test]
async fn collector_issuer_uses_exact_saved_username_and_exports_only_supported_password_bytes() {
    let f = Fixture::new("synthetic internal \u{00a0} password42").await;
    let review = f.review().await; let binding = review.binding().clone();
    let owner = f.app.session().await.unwrap().identity().public_keys().signing;
    let capsule = review.approve_and_seal_collector_basic_password(&f.app, "saved-exporter-user".into()).await.unwrap();
    let encoded = capsule.to_json().unwrap();
    assert!(encoded.len() <= 8192);
    assert!(!String::from_utf8(encoded).unwrap().contains("synthetic internal"));
    let opened = open_service_credential(&capsule, &binding, &owner, &f.recipient, now().unwrap()).unwrap();
    assert!(opened.with_secret(&binding, now().unwrap(), |value|
        value == "synthetic internal \u{00a0} password42".as_bytes()).unwrap());
    assert!(f.review().await.approve_and_seal_collector_basic_password(&f.app, "caller-substituted-user".into()).await.is_err());
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn collector_issuer_rejects_actual_unsupported_selected_material_before_capsule_return() {
    let f = Fixture::new("synthetic-valid42").await;
    for password in [" leading".to_owned(), "trailing ".to_owned(), "\u{00a0}leading".to_owned(),
        "trailing\u{0085}".to_owned(), "embedded\nline".to_owned(), "embedded\rline".to_owned(),
        "x".repeat(4097)] {
        f.app.set_credential_password(f.credential.clone(), password).await.unwrap();
        assert!(f.review().await.approve_and_seal_collector_basic_password(&f.app, "saved-exporter-user".into()).await.is_err());
    }
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn existing_service_issuer_preserves_its_preexisting_exact_whitespace_password_behavior() {
    let f = Fixture::new(" synthetic-generic-service42 ").await;
    let review = f.review().await; let binding = review.binding().clone();
    let owner = f.app.session().await.unwrap().identity().public_keys().signing;
    let capsule = review.approve_and_seal(&f.app).await.unwrap();
    let opened = open_service_credential(&capsule, &binding, &owner, &f.recipient, now().unwrap()).unwrap();
    assert!(opened.with_secret(&binding, now().unwrap(), |value| value == b" synthetic-generic-service42 ").unwrap());
    f.app.shutdown().await.unwrap();
}
