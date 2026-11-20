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
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            directory.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile(
            "Owned scoped service fixture".into(),
            "synthetic owner passphrase42".into(),
        )
        .await
        .unwrap();
        let profile = app.session().await.unwrap().profile_id.to_string();
        let credential = app
            .add_password_credential(
                "Explicit service password".into(),
                None,
                "synthetic-selected-value42".into(),
            )
            .await
            .unwrap()
            .id;
        // A second actual secret must never be included in the selected capsule.
        app.add_password_credential(
            "Not selected".into(),
            None,
            "synthetic-unselected-value43".into(),
        )
        .await
        .unwrap();
        Self {
            _directory: directory,
            app,
            profile,
            credential,
            recipient: DeviceSecretKeys::generate().unwrap(),
        }
    }
    fn selection(&self) -> ServiceRecipientSelection {
        ServiceRecipientSelection {
            service_id: Uuid::new_v4(),
            recipient_public_key: self.recipient.public_keys().encryption,
            recipient_key_epoch: 2,
            recipient_certificate_epoch: 3,
            delegation_digest: "a".repeat(64),
            policy_revision: 4,
            revocation_epoch: 5,
            delegation_expires_at: now().unwrap() + 3600,
        }
    }
    async fn review(&self) -> ServiceCredentialReview {
        self.app
            .service_credential_review(
                self.profile.clone(),
                self.credential.clone(),
                self.selection(),
                &CancellationToken::new(),
            )
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn real_selected_secret_is_only_sealed_to_exact_recipient_and_original_owner() {
    let f = Fixture::new().await;
    let review = f.review().await;
    let binding = review.binding().clone();
    assert!(binding.tenant_kind == ServiceTenantKind::LocalProfile);
    assert_eq!(binding.tenant_id, f.profile);
    assert_eq!(binding.credential_id, f.credential);
    let owner = f
        .app
        .session()
        .await
        .unwrap()
        .identity()
        .public_keys()
        .signing;
    let capsule = review.approve_and_seal(&f.app).await.unwrap();
    let text = String::from_utf8(capsule.to_json().unwrap()).unwrap();
    assert!(!text.contains("synthetic-selected-value42"));
    assert!(!text.contains("synthetic-unselected-value43"));
    assert!(
        open_service_credential(
            &capsule,
            &binding,
            &owner,
            &DeviceSecretKeys::generate().unwrap(),
            now().unwrap()
        )
        .is_err()
    );
    let opened =
        open_service_credential(&capsule, &binding, &owner, &f.recipient, now().unwrap()).unwrap();
    opened
        .with_secret(&binding, now().unwrap(), |value| {
            assert_eq!(value, b"synthetic-selected-value42")
        })
        .unwrap();
    assert_eq!(
        f.app
            .session()
            .await
            .unwrap()
            .automation
            .review_slots
            .available_permits(),
        4
    );
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn changing_actual_selected_password_invalidates_original_review() {
    let f = Fixture::new().await;
    let review = f.review().await;
    f.app
        .set_credential_password(f.credential.clone(), "synthetic-replacement44".into())
        .await
        .unwrap();
    assert!(review.approve_and_seal(&f.app).await.is_err());
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn changed_device_identity_cannot_sign_an_old_owner_review() {
    let f = Fixture::new().await;
    let review = f.review().await;
    f.app
        .session()
        .await
        .unwrap()
        .set_identity(Arc::new(cc_vault_core::DeviceIdentity::generate().unwrap()));
    assert!(review.approve_and_seal(&f.app).await.is_err());
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn synced_profile_is_not_relabelled_as_local_tenant() {
    let f = Fixture::new().await;
    let owner = f.app.session().await.unwrap();
    let mut profile = owner.profile().await.unwrap();
    profile.kind = ProfileKind::Synced;
    profile.server_url = Some("https://owned-unused.invalid".into());
    owner.storage().unwrap().put_profile(profile).await.unwrap();
    assert!(
        f.app
            .service_credential_review(
                f.profile.clone(),
                f.credential.clone(),
                f.selection(),
                &CancellationToken::new()
            )
            .await
            .is_err()
    );
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn selected_secret_over_capsule_budget_never_gets_exported() {
    let f = Fixture::new().await;
    f.app
        .set_credential_password(f.credential.clone(), "x".repeat(16 * 1024 + 1))
        .await
        .unwrap();
    let review = f.review().await;
    assert!(review.approve_and_seal(&f.app).await.is_err());
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn original_unlock_cannot_be_replaced_by_same_profile_reunlock() {
    let f = Fixture::new().await;
    let review = f.review().await;
    let scope = review.scope.clone();
    f.app.lock().await.unwrap();
    f.app
        .unlock_with_passphrase("synthetic owner passphrase42".into())
        .await
        .unwrap();
    assert_ne!(
        f.app
            .session()
            .await
            .unwrap()
            .unlocked()
            .await
            .unwrap()
            .notices
            .scope_id(),
        Some(scope)
    );
    assert!(review.approve_and_seal(&f.app).await.is_err());
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn profile_switch_does_not_use_latest_identity_or_vault() {
    let f = Fixture::new().await;
    let review = f.review().await;
    f.app
        .create_local_profile(
            "Different owner".into(),
            "synthetic different owner43".into(),
        )
        .await
        .unwrap();
    assert!(review.approve_and_seal(&f.app).await.is_err());
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancellation_and_expiry_deny_without_renewing_review_or_leaking_slots() {
    let f = Fixture::new().await;
    let cancel = CancellationToken::new();
    let review = f
        .app
        .service_credential_review(
            f.profile.clone(),
            f.credential.clone(),
            f.selection(),
            &cancel,
        )
        .await
        .unwrap();
    cancel.cancel();
    assert!(review.approve_and_seal(&f.app).await.is_err());
    let mut review = f.review().await;
    review.deadline = Instant::now();
    assert!(review.approve_and_seal(&f.app).await.is_err());
    assert_eq!(
        f.app
            .session()
            .await
            .unwrap()
            .automation
            .review_slots
            .available_permits(),
        4
    );
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn invalid_recipient_scope_and_review_admission_fail_closed() {
    let f = Fixture::new().await;
    assert!(
        f.app
            .service_credential_review(
                Uuid::new_v4().to_string(),
                f.credential.clone(),
                f.selection(),
                &CancellationToken::new()
            )
            .await
            .is_err()
    );
    let mut selection = f.selection();
    selection.recipient_public_key = [0; 32];
    assert!(
        f.app
            .service_credential_review(
                f.profile.clone(),
                f.credential.clone(),
                selection,
                &CancellationToken::new()
            )
            .await
            .is_err()
    );
    let mut selection = f.selection();
    selection.recipient_public_key = f
        .app
        .session()
        .await
        .unwrap()
        .identity()
        .public_keys()
        .encryption;
    assert!(
        f.app
            .service_credential_review(
                f.profile.clone(),
                f.credential.clone(),
                selection,
                &CancellationToken::new()
            )
            .await
            .is_err()
    );
    let mut selection = f.selection();
    selection.delegation_expires_at = now().unwrap() + 86401;
    assert!(
        f.app
            .service_credential_review(
                f.profile.clone(),
                f.credential.clone(),
                selection,
                &CancellationToken::new()
            )
            .await
            .is_err()
    );
    let mut reviews = Vec::new();
    for _ in 0..4 {
        reviews.push(f.review().await);
    }
    assert!(
        f.app
            .service_credential_review(
                f.profile.clone(),
                f.credential.clone(),
                f.selection(),
                &CancellationToken::new()
            )
            .await
            .is_err()
    );
    drop(reviews);
    assert_eq!(
        f.app
            .session()
            .await
            .unwrap()
            .automation
            .review_slots
            .available_permits(),
        4
    );
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn actual_queued_secret_read_cannot_return_after_original_lock() {
    use std::{future::Future, task::Poll};
    let f = Fixture::new().await;
    let review = f.review().await;
    let owner = f.app.session().await.unwrap();
    let storage = owner.storage().unwrap();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let blocker = tokio::spawn(async move {
        storage
            .write(move |_| -> AppResult<()> {
                entered_tx.send(()).unwrap();
                release_rx
                    .recv_timeout(Duration::from_secs(10))
                    .map_err(|_| invalid())?;
                Ok(())
            })
            .await
    });
    tokio::time::timeout(Duration::from_secs(10), entered_rx)
        .await
        .unwrap()
        .unwrap();
    let mut issue = Box::pin(review.approve_and_seal(&f.app));
    std::future::poll_fn(|cx| match issue.as_mut().poll(cx) {
        Poll::Pending => Poll::Ready(()),
        Poll::Ready(_) => panic!("actual secret store read must wait for SQL barrier"),
    })
    .await;
    assert!(
        f.app.inner.active.try_lock().is_ok(),
        "no profile guard held over secret store await"
    );
    // Revoke original unlock synchronously, without waiting on the blocked SQL
    // worker. This is the same actual first step used by Session::lock.
    let u = owner.unlocked().await.unwrap();
    u.sharing_shutdown.send_replace(true);
    assert!(
        tokio::time::timeout(Duration::from_secs(1), &mut issue)
            .await
            .unwrap()
            .is_err()
    );
    drop(issue);
    release_tx.send(()).unwrap();
    blocker.await.unwrap().unwrap();
    f.app.shutdown().await.unwrap();
}
