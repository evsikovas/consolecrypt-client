use super::*;
use cc_sync_core::mock::{FaultAction, MockServer};
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_original_owner_scope_cancel_projection_and_final_publication_deny_logout() {
    let f = Fixture::new().await;
    let (owner, weak) = f.subject().await;
    let original = f.scope().await;
    let mut retired = Box::pin(original.cancelled());
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), &mut retired)
            .await
            .is_err()
    );
    f.app.logout().await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(1), retired)
        .await
        .unwrap();
    let u = weak.upgrade().unwrap();
    assert!(original.check(&owner, &u).is_err());
    let produced = std::sync::atomic::AtomicBool::new(false);
    assert!(
        original
            .with_current(&owner, || {
                produced.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            })
            .is_err()
    );
    assert!(!produced.load(std::sync::atomic::Ordering::SeqCst));
    drop(u);
    f.app.shutdown().await.unwrap();
}

pub(crate) const PASSWORD: &str = "owned personal service vault42";
pub(crate) struct Fixture {
    _root: tempfile::TempDir,
    pub(crate) app: AppCore,
    pub(crate) server: MockServer,
    pub(crate) credential: String,
}
impl Fixture {
    pub(crate) async fn new() -> Self {
        let server = MockServer::start().await;
        let root = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            root.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_synced_profile(
            "Owned personal service".into(),
            server.url().to_string(),
            "owned-service@example.org".into(),
            "owned account password42".into(),
            crate::AccountMode::Register,
        )
        .await
        .unwrap();
        app.create_vault(PASSWORD.into()).await.unwrap();
        let credential = app
            .add_password_credential(
                "Selected owned service secret".into(),
                None,
                "synthetic personal service value42".into(),
            )
            .await
            .unwrap()
            .id;
        app.sync_now().await.unwrap();
        app.session()
            .await
            .unwrap()
            .unlocked()
            .await
            .unwrap()
            .stop_engine()
            .await;
        Self {
            _root: root,
            app,
            server,
            credential,
        }
    }
    pub(crate) async fn subject(&self) -> (Arc<Session>, Weak<Unlocked>) {
        let (owner, u) = self.app.unlocked().await.unwrap();
        let weak = Arc::downgrade(&u);
        drop(u);
        (owner, weak)
    }
    pub(crate) async fn scope(&self) -> Arc<OwnerScope> {
        let (owner, weak) = self.subject().await;
        OwnerScope::capture(&self.app, &owner, weak).await.unwrap()
    }
    pub(crate) fn recipient() -> super::super::ServiceRecipientSelection {
        let keys = cc_crypto_core::DeviceSecretKeys::generate().unwrap();
        super::super::ServiceRecipientSelection {
            service_id: uuid::Uuid::new_v4(),
            recipient_public_key: keys.public_keys().encryption,
            recipient_key_epoch: 1,
            recipient_certificate_epoch: 1,
            delegation_digest: "a".repeat(64),
            policy_revision: 1,
            revocation_epoch: 1,
            delegation_expires_at: chrono::Utc::now().timestamp() as u64 + 3600,
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_personal_owner_observation_and_selected_capsule_bind_actual_user_not_profile() {
    let f = Fixture::new().await;
    let (owner, weak) = f.subject().await;
    let actual = owner.api().unwrap().me().await.unwrap();
    let before = f.server.hits(cc_protocol::paths::AUTH_ME);
    let scope = f.scope().await;
    assert!(f.server.hits(cc_protocol::paths::AUTH_ME) > before);
    assert!(scope.tenant().kind == ServiceTenantKind::PersonalAccount);
    assert_eq!(scope.tenant().id, actual.user_id.to_string());
    assert_ne!(scope.tenant().id, owner.profile_id.to_string());
    scope.verify(&owner, weak).await.unwrap();
    let recipient = cc_crypto_core::DeviceSecretKeys::generate().unwrap();
    let mut selection = Fixture::recipient();
    selection.recipient_public_key = recipient.public_keys().encryption;
    let review = f
        .app
        .service_credential_review(
            owner.profile_id.to_string(),
            f.credential.clone(),
            selection,
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
    let binding = review.binding().clone();
    assert!(binding.tenant_kind == ServiceTenantKind::PersonalAccount);
    assert_eq!(binding.tenant_id, actual.user_id.to_string());
    let capsule = review.approve_and_seal(&f.app).await.unwrap();
    let opened = cc_crypto_core::open_service_credential(
        &capsule,
        &binding,
        &owner.identity().public_keys().signing,
        &recipient,
        chrono::Utc::now().timestamp() as u64,
    )
    .unwrap();
    opened
        .with_secret(&binding, chrono::Utc::now().timestamp() as u64, |value| {
            assert_eq!(value, b"synthetic personal service value42");
        })
        .unwrap();
    assert!(
        !String::from_utf8(capsule.to_json().unwrap())
            .unwrap()
            .contains("synthetic personal service value42")
    );
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_account_owner_role_trust_and_personal_list_fail_closed_without_repair() {
    let f = Fixture::new().await;
    for changed in ["role", "trust", "state", "owner", "not_personal"] {
        f.server.clear_faults();
        f.server.inject(
            "/v1/vaults",
            2,
            FaultAction::Rewrite(Arc::new(move |value| {
                let rewrite = |v: &mut serde_json::Value| match changed {
                    "role" => v["role"] = json!("viewer"),
                    "trust" => v["caller_trusted"] = json!(false),
                    "state" => v["state"] = json!("pending_deletion"),
                    "owner" => v["owner_user_id"] = json!(cc_protocol::UserId::new()),
                    _ => {}
                };
                if changed == "not_personal" {
                    value["vaults"] = json!([]);
                } else if let Some(vaults) = value.get_mut("vaults").and_then(|v| v.as_array_mut())
                {
                    for v in vaults {
                        rewrite(v);
                    }
                } else {
                    rewrite(value);
                }
            })),
        );
        let (owner, weak) = f.subject().await;
        assert!(
            OwnerScope::capture(&f.app, &owner, weak).await.is_err(),
            "{changed}"
        );
        let row: Option<serde_json::Value> = owner
            .storage()
            .unwrap()
            .setting_get("cc.native.service.owner.v1")
            .await
            .unwrap();
        assert!(row.is_none());
    }
    f.server.clear_faults();
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_sql_account_origin_change_and_key_conflict_deny_personal_capture() {
    let f = Fixture::new().await;
    let (owner, weak) = f.subject().await;
    let original = owner.profile().await.unwrap();
    let mut changed = original.clone();
    changed.user_id = Some(cc_protocol::UserId::new());
    owner.put_profile(changed).await.unwrap();
    assert!(
        OwnerScope::capture(&f.app, &owner, weak.clone())
            .await
            .is_err()
    );
    let other = MockServer::start().await;
    let mut changed = original.clone();
    changed.server_url = Some(other.url().to_string());
    owner.put_profile(changed).await.unwrap();
    assert!(
        OwnerScope::capture(&f.app, &owner, weak.clone())
            .await
            .is_err()
    );
    owner.put_profile(original).await.unwrap();
    weak.upgrade()
        .unwrap()
        .set_key_conflicts(vec![crate::dto::VaultKeyConflictDto {
            device_id: cc_protocol::DeviceId::new().to_string(),
            device_name: "Owned conflict".into(),
            fingerprint: "0102030405060708".into(),
        }]);
    assert!(OwnerScope::capture(&f.app, &owner, weak).await.is_err());
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_completed_logout_keeps_vault_unlocked_but_denies_original_and_new_consent() {
    let f = Fixture::new().await;
    let (owner, weak) = f.subject().await;
    let scope = f.scope().await;
    let original = weak.upgrade().unwrap();
    f.app.logout().await.unwrap();
    assert!(Arc::ptr_eq(&original, &owner.unlocked().await.unwrap()));
    assert!(scope.check(&owner, &original).is_err());
    drop(original);
    assert!(OwnerScope::capture(&f.app, &owner, weak).await.is_err());
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_blocked_me_request_cancels_on_lock_without_retaining_unlocked_vault() {
    let f = Fixture::new().await;
    let (owner, weak) = f.subject().await;
    let old = weak.clone();
    let (entered, wait_entered) = tokio::sync::oneshot::channel();
    let entered = Arc::new(std::sync::Mutex::new(Some(entered)));
    let (release, wait_release) = std::sync::mpsc::channel();
    let wait_release = Arc::new(std::sync::Mutex::new(wait_release));
    f.server.inject(
        cc_protocol::paths::AUTH_ME,
        1,
        FaultAction::Rewrite(Arc::new(move |_| {
            entered.lock().unwrap().take().unwrap().send(()).unwrap();
            wait_release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
        })),
    );
    let app = f.app.clone();
    let capture = tokio::spawn(async move { OwnerScope::capture(&app, &owner, weak).await });
    tokio::time::timeout(Duration::from_secs(10), wait_entered)
        .await
        .unwrap()
        .unwrap();
    let locked = tokio::time::timeout(Duration::from_secs(5), f.app.lock()).await;
    let captured = tokio::time::timeout(Duration::from_secs(5), capture).await;
    release.send(()).unwrap();
    locked.unwrap().unwrap();
    assert!(captured.unwrap().unwrap().is_err());
    assert!(
        old.upgrade().is_none(),
        "account capture must not retain original Unlocked across HTTP"
    );
    f.app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    f.app.shutdown().await.unwrap();
}
