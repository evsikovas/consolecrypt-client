use super::*;
use crate::automation_runners::AutomationRunnerConfig;
use cc_storage_core::ServiceOwnerIdentity;

#[tokio::test(start_paused = true)]
async fn actual_results_original_entry_deadline_expires_while_profile_guard_is_held() {
    let f = Fixture::new().await;
    let (owner, u) = f.app.unlocked().await.unwrap();
    drop(u);
    let held = f.app.hold_session(&owner).await.unwrap();
    let call = f.app.automation_service_results_json(f.action(1));
    tokio::pin!(call);
    assert!(call.as_mut().now_or_never().is_none());
    tokio::time::advance(Duration::from_secs(61)).await;
    assert!(call.await.is_err());
    drop(held);
    drop(owner);
    assert!(f.row().await.is_none());
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn actual_results_missing_ledger_never_opens_a_helper_or_creates_owner_metadata() {
    let f = Fixture::new().await;
    assert!(
        f.app
            .automation_service_results_json(f.action(1))
            .await
            .is_err()
    );
    assert!(f.row().await.is_none());
    let (owner, u) = f.app.unlocked().await.unwrap();
    assert!(owner.automation.connections.lock().unwrap().is_empty());
    drop(u);
    drop(owner);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn actual_results_cannot_reuse_old_unlock_or_accept_caller_job_policy_paths() {
    let f = Fixture::new().await;
    let action = f.action(1);
    for extra in [
        json!({"jobId":Uuid::new_v4()}),
        json!({"path":"/tmp/foreign"}),
        json!({"policyDigest":"0".repeat(64)}),
    ] {
        let mut value: Value = serde_json::from_str(&action).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert!(
            f.app
                .automation_service_results_json(value.to_string())
                .await
                .is_err()
        );
    }
    f.app.lock().await.unwrap();
    f.app
        .unlock_with_passphrase("synthetic public owner42".into())
        .await
        .unwrap();
    let (_, u) = f.app.unlocked().await.unwrap();
    assert_ne!(u.notices.scope_id().unwrap(), f.scope);
    drop(u);
    assert!(f.app.automation_service_results_json(action).await.is_err());
    assert!(f.row().await.is_none());
    f.app.shutdown().await.unwrap();
}

struct Fixture {
    _dir: tempfile::TempDir,
    app: AppCore,
    profile: String,
    scope: String,
    runner: String,
}
impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            dir.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile(
            "Owned public service fixture".into(),
            "synthetic public owner42".into(),
        )
        .await
        .unwrap();
        let (owner, u) = app.unlocked().await.unwrap();
        let profile = owner.profile_id.to_string();
        let scope = u.notices.scope_id().unwrap();
        drop(u);
        let runner = app
            .automation_runner_create(
                "Owned offline service".into(),
                AutomationRunnerConfig::ServiceRemote {
                    host_id: Uuid::new_v4().to_string(),
                    identity_id: Uuid::new_v4().to_string(),
                },
            )
            .await
            .unwrap()
            .id;
        Self {
            _dir: dir,
            app,
            profile,
            scope,
            runner,
        }
    }
    fn action(&self, revision: u64) -> String {
        json!({"version":1,"profileId":self.profile,"notificationScopeId":self.scope,
        "runnerId":self.runner,"expectedRunnerRevision":revision})
        .to_string()
    }
    async fn owner(&self) -> (cc_storage_core::Storage, ServiceOwnerIdentity) {
        let (owner, u) = self.app.unlocked().await.unwrap();
        (
            owner.storage().unwrap(),
            ServiceOwnerIdentity {
                profile_id: owner.profile_id,
                vault_id: u.vault_id,
                device_id: owner.device_id(),
                signing_fingerprint: hex::encode(sha2::Sha256::digest(
                    owner.identity().public_keys().signing,
                )),
                personal: None,
            },
        )
    }
    async fn seed_observation(&self) {
        // DATA only: no PreparedReview, grant, signature, peer or enrollment ACK.
        let (storage, identity) = self.owner().await;
        let id = Uuid::parse_str(&self.runner).unwrap();
        storage
            .write_service_owner(move |tx| -> AppResult<()> {
                let old = tx.service_owner_snapshot(&identity)?;
                tx.service_owner_compare_exchange(
                    &old,
                    vec![ServiceOwnerRecord {
                        service_id: id,
                        node_local_profile: "cc-service-local-v1-fixture".into(),
                        owner_epoch: 1,
                        policy_revision: 1,
                        revocation_epoch: 1,
                        clock_floor: 100,
                        signer_fingerprint: "a".repeat(64),
                        recipient_fingerprint: "b".repeat(64),
                        route_trust_digest: "c".repeat(64),
                        policy_digest: "d".repeat(64),
                        status_digest: "e".repeat(64),
                        delivery: ServiceOwnerDelivery::DeliveryUnknown,
                    }],
                )?;
                Ok(())
            })
            .await
            .unwrap();
    }
    async fn row(&self) -> Option<ServiceOwnerRecord> {
        let (storage, identity) = self.owner().await;
        storage
            .read(move |tx| {
                Ok::<_, AppError>(
                    tx.service_owner_snapshot(&identity)?
                        .records()
                        .first()
                        .cloned(),
                )
            })
            .await
            .unwrap()
    }
}

#[test]
fn fixed_choice_parsers_reject_authority_fields_unsafe_numbers_and_unknown_schema() {
    assert!(
        parse::<Action>(
            &json!({"version":1,"profileId":"p","notificationScopeId":"s","runnerId":"r"})
                .to_string(),
            4096
        )
        .is_err()
    );
    assert!(parse::<Action>(&json!({"version":1,"profileId":"p","notificationScopeId":"s","runnerId":"r","expectedRunnerRevision":1,"approved":true}).to_string(),4096).is_err());
    assert!(parse::<Action>(&"x".repeat(4097), 4096).is_err());
    let valid = ScheduleChoice {
        zone: "Europe/Moscow".into(),
        hour: 9,
        minute: 30,
        weekdays: vec![4, 0],
    }
    .definition()
    .unwrap();
    assert_eq!(valid["weekdays"], json!([0, 4]));
    assert_eq!(valid["gapPolicy"], "skip");
    assert_eq!(valid["foldPolicy"], "first");
    for zone in ["", "../UTC", "/UTC", "UTC\\bad", "$(command)"] {
        assert!(
            ScheduleChoice {
                zone: zone.into(),
                hour: 9,
                minute: 30,
                weekdays: vec![0]
            }
            .definition()
            .is_err()
        );
    }
    assert!(
        ScheduleChoice {
            zone: "UTC".into(),
            hour: 9,
            minute: 30,
            weekdays: vec![0, 0]
        }
        .definition()
        .is_err()
    );
}

#[tokio::test]
async fn actual_offline_status_is_metadata_and_cannot_create_a_ledger_record() {
    let f = Fixture::new().await;
    let value: Value = serde_json::from_str(
        &f.app
            .automation_service_status_json(f.action(1))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(value.as_object().unwrap().len(), 10);
    assert_eq!(value["localState"], "absent");
    assert_eq!(value["remoteState"], "unknown");
    assert!(value["expiresUtc"].is_null());
    assert!(f.row().await.is_none());
    assert!(
        f.app
            .automation_service_revoke_json(f.action(1))
            .await
            .is_err()
    );
    assert!(f.row().await.is_none());
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn actual_offline_revoke_commits_only_existing_sticky_block_without_peer_or_signature() {
    let f = Fixture::new().await;
    f.seed_observation().await;
    let before = f.row().await.unwrap();
    let value: Value = serde_json::from_str(
        &f.app
            .automation_service_revoke_json(f.action(1))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(value["localState"], "localBlocked");
    assert_eq!(value["remoteState"], "unknown");
    assert!(value["expiresUtc"].is_null());
    let after = f.row().await.unwrap();
    assert!(after.delivery == ServiceOwnerDelivery::RevokedLocally);
    assert_eq!(after.revocation_epoch, before.revocation_epoch + 1);
    assert_eq!(after.policy_digest, before.policy_digest);
    assert_eq!(after.node_local_profile, before.node_local_profile);
    assert_eq!(after.policy_revision, before.policy_revision);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn actual_saved_runner_revision_and_old_unlock_intent_cannot_retarget_revoke() {
    let f = Fixture::new().await;
    f.seed_observation().await;
    let before = f.row().await.unwrap();
    f.app
        .automation_runner_update(
            f.runner.clone(),
            1,
            "Changed service".into(),
            AutomationRunnerConfig::ServiceRemote {
                host_id: Uuid::new_v4().to_string(),
                identity_id: Uuid::new_v4().to_string(),
            },
        )
        .await
        .unwrap();
    assert!(
        f.app
            .automation_service_revoke_json(f.action(1))
            .await
            .is_err()
    );
    assert!(f.row().await.unwrap() == before);
    f.app.lock().await.unwrap();
    f.app
        .unlock_with_passphrase("synthetic public owner42".into())
        .await
        .unwrap();
    assert!(
        f.app
            .automation_service_revoke_json(f.action(2))
            .await
            .is_err()
    );
    assert!(f.row().await.unwrap() == before);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn actual_original_issuer_never_recaptures_reopened_unlock_for_selected_material() {
    let f = Fixture::new().await;
    let credential = f
        .app
        .add_password_credential(
            "Owned selected password".into(),
            None,
            "synthetic selected value42".into(),
        )
        .await
        .unwrap()
        .id;
    let recipient_key = cc_crypto_core::DeviceSecretKeys::generate()
        .unwrap()
        .public_keys()
        .encryption;
    let selection = || crate::service_credential_issuer::ServiceRecipientSelection {
        service_id: Uuid::new_v4(),
        recipient_public_key: recipient_key,
        recipient_key_epoch: 1,
        recipient_certificate_epoch: 1,
        delegation_digest: "a".repeat(64),
        policy_revision: 1,
        revocation_epoch: 1,
        delegation_expires_at: now().unwrap() + 300,
    };
    let (owner, u) = f.app.unlocked().await.unwrap();
    let weak = Arc::downgrade(&u);
    drop(u);
    let cancel = tokio_util::sync::CancellationToken::new();
    let valid = f
        .app
        .service_credential_review_original(
            f.profile.clone(),
            credential.clone(),
            selection(),
            &cancel,
            owner.clone(),
            weak.clone(),
        )
        .await
        .unwrap();
    drop(valid);
    f.app.lock().await.unwrap();
    f.app
        .unlock_with_passphrase("synthetic public owner42".into())
        .await
        .unwrap();
    assert!(
        f.app
            .service_credential_review_original(
                f.profile.clone(),
                credential,
                selection(),
                &cancel,
                owner,
                weak
            )
            .await
            .is_err()
    );
    assert!(f.row().await.is_none());
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn actual_catalog_edit_after_intent_read_is_rejected_inside_tombstone_cas() {
    let f = Fixture::new().await;
    f.seed_observation().await;
    let before = f.row().await.unwrap();
    let frame = Frame::capture(&f.app, 1, &f.profile, &f.scope)
        .await
        .unwrap();
    saved(&f.app, &frame, &f.runner, Some(1)).await.unwrap();
    let ledger = Arc::new(
        owner_ledger::OwnerLedger::capture_stale(&f.app, frame.owner.clone(), frame.unlock.clone())
            .await
            .unwrap(),
    );
    f.app
        .automation_runner_update(
            f.runner.clone(),
            1,
            "Changed after saved read".into(),
            AutomationRunnerConfig::ServiceRemote {
                host_id: Uuid::new_v4().to_string(),
                identity_id: Uuid::new_v4().to_string(),
            },
        )
        .await
        .unwrap();
    assert!(
        ledger
            .block_original_runner(&f.app, Uuid::parse_str(&f.runner).unwrap(), Some(1))
            .await
            .is_err()
    );
    assert!(f.row().await.unwrap() == before);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn actual_explicit_status_and_revoke_stale_nonce_or_revision_never_dial_owned_tcp() {
    use std::io::ErrorKind;
    let f = Fixture::new().await;
    f.seed_observation().await;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let credential = f
        .app
        .add_password_credential(
            "Owned status TCP password".into(),
            Some("runner".into()),
            "synthetic status tcp value42".into(),
        )
        .await
        .unwrap();
    let mut host = crate::HostDto::new("Owned explicit status endpoint", "127.0.0.1");
    host.port = Some(listener.local_addr().unwrap().port());
    host.username = Some("runner".into());
    host.credential_id = Some(credential.id);
    let host = f.app.save_host(host).await.unwrap();
    f.app
        .automation_runner_update(
            f.runner.clone(),
            1,
            "Owned actual status endpoint".into(),
            AutomationRunnerConfig::ServiceRemote {
                host_id: host.id,
                identity_id: Uuid::new_v4().to_string(),
            },
        )
        .await
        .unwrap();
    let before = f.row().await.unwrap();
    let mut stale: Value = serde_json::from_str(&f.action(2)).unwrap();
    stale["notificationScopeId"] = json!("opaque-stale-scope");
    assert!(
        f.app
            .automation_service_status_json(stale.to_string())
            .await
            .is_err()
    );
    assert!(
        f.app
            .automation_service_revoke_json(stale.to_string())
            .await
            .is_err()
    );
    assert!(matches!(listener.accept(),Err(error) if error.kind()==ErrorKind::WouldBlock));
    assert!(
        f.app
            .automation_service_status_json(f.action(1))
            .await
            .is_err()
    );
    assert!(
        f.app
            .automation_service_revoke_json(f.action(1))
            .await
            .is_err()
    );
    assert!(matches!(listener.accept(),Err(error) if error.kind()==ErrorKind::WouldBlock));
    assert!(f.row().await.unwrap() == before);
    // Positive transport control: a valid CURRENT explicit Status really reaches
    // this same owned TCP endpoint. It has no SSH banner/authority and is closed
    // immediately, so there is no fake service witness or credential exchange.
    let listener = tokio::net::TcpListener::from_std(listener).unwrap();
    let valid = f.app.automation_service_status_json(f.action(2));
    tokio::pin!(valid);
    let (stream,_)=tokio::time::timeout(Duration::from_secs(3),async {
        tokio::select! {biased;
            accepted=listener.accept()=>accepted.unwrap(),
            _=&mut valid=>panic!("valid original endpoint was rejected before positive TCP control"),
        }
    }).await.unwrap();
    drop(stream);
    let value: Value = serde_json::from_str(
        &tokio::time::timeout(Duration::from_secs(3), valid)
            .await
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(value["remoteState"], "unknown");
    assert!(f.row().await.unwrap() == before);
    f.app.shutdown().await.unwrap();
}
