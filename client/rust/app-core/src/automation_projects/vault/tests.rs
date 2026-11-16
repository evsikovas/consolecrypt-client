use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use cc_automation_core::{
    EnrolledJobGrant, SignedJobApproval, VaultPasswordPurpose, VaultPasswordRequest,
    VAULT_INPUT_FILE,
};

struct Fixture {
    _directory: tempfile::TempDir,
    app: AppCore,
    project: String,
    credential: String,
    digest: String,
}
impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            directory.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile(
            "Owned Ansible Vault prerequisite fixture".into(),
            "synthetic profile passphrase 42".into(),
        )
        .await
        .unwrap();
        let project = app
            .automation_project_create("Vault ref fixture".into(), None)
            .await
            .unwrap()
            .id;
        let credential = app
            .add_password_credential(
                "Explicit Ansible Vault input".into(),
                None,
                "synthetic-vault-password-42".into(),
            )
            .await
            .unwrap()
            .id;
        app.automation_project_save(
            project.clone(),
            VAULT_INPUT_FILE.into(),
            None,
            json!({"version":1,"ansible_vault":{"credential_id":credential}}).to_string(),
        )
        .await
        .unwrap();
        let digest = app
            .automation_project_files(project.clone())
            .await
            .unwrap()
            .digest;
        Self {
            _directory: directory,
            app,
            project,
            credential,
            digest,
        }
    }
    async fn capture(&self) -> VaultCredentialReview {
        self.app
            .automation_vault_capture(
                self.project.clone(),
                1,
                self.digest.clone(),
                self.credential.clone(),
                &CancellationToken::new(),
            )
            .await
            .unwrap()
    }
    fn job(&self, project: Uuid, revision: &str) -> VerifiedJob {
        let keys = cc_crypto_core::DeviceSecretKeys::generate().unwrap();
        let now = chrono::Utc::now().timestamp();
        let signer = Uuid::new_v4();
        let tenant = Uuid::new_v4();
        let runner = Uuid::new_v4();
        let host = Uuid::new_v4();
        let manifest=serde_json::from_value(json!({"version":1,"job_id":Uuid::new_v4(),"tenant_id":tenant,"runner_id":runner,
            "project_id":project,"revision_digest":revision,"inventory_digest":"b".repeat(64),"environment_digest":"c".repeat(64),
            "parameters_digest":"d".repeat(64),"playbook":"playbook.yml","mode":"check","policy":{"strict_isolation":true,
            "cancel_on_lock":false,"timeout_seconds":60,"forks":1,"production":false},"targets":[{"host_id":host,"alias":"fixture",
            "address":"127.0.0.1","port":22,"username":"fixture","route_digest":"e".repeat(64),"credential_refs":[]}],
            "issued_at":now,"expires_at":now+60,"access_expires_at":now+60,"nonce":Uuid::new_v4()})).unwrap();
        let approval = SignedJobApproval {
            signer_id: signer,
            signature: STANDARD.encode(keys.sign_automation_manifest(&manifest).unwrap()),
            manifest,
        };
        let grant = EnrolledJobGrant {
            signer_id: signer,
            public_key: keys.public_keys().signing,
            tenant_id: tenant,
            runner_id: runner,
            projects: [project].into(),
            hosts: [host].into(),
            jobs_execute: true,
            expires_at: now + 60,
            max_job_seconds: 60,
        };
        VerifiedJob::verify(&approval, &grant, &"c".repeat(64), now).unwrap()
    }
    fn verified(&self) -> VerifiedJob {
        self.job(self.project.parse().unwrap(), &self.digest)
    }
}

#[tokio::test]
async fn actual_encrypted_password_lookup_issues_sealed_non_ai_non_json_owner() {
    let f = Fixture::new().await;
    let review = f.capture().await;
    let verified = f.verified();
    let mut native = review
        .resolve_for_job(&f.app, &verified, &CancellationToken::new())
        .await
        .unwrap();
    assert!(!format!("{native:?}").contains("synthetic-vault-password-42"));
    assert!(!format!("{:?}", native.lease).contains("synthetic-vault-password-42"));
    let request = VaultPasswordRequest {
        version: 1,
        purpose: VaultPasswordPurpose::AnsibleVaultPassword,
        job_id: verified.manifest().job_id,
        manifest_digest: verified.digest().into(),
        revision_digest: f.digest.clone(),
        reference_sha256: native.review.binding.reference_sha256().into(),
    };
    // Test-only inspection: production Native owner has no extraction method.
    let mut pipe = Vec::new();
    native.lease.write_once(&request, &mut pipe).unwrap();
    assert_eq!(pipe, b"synthetic-vault-password-42\n");
    assert!(!serde_json::to_string(&request)
        .unwrap()
        .contains("synthetic-vault-password-42"));
    drop(native);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn exact_selection_ref_digest_and_real_signed_job_are_separate_gates() {
    let f = Fixture::new().await;
    for (rev, digest, selected) in [
        (2, f.digest.clone(), f.credential.clone()),
        (1, "f".repeat(64), f.credential.clone()),
        (1, f.digest.clone(), Uuid::new_v4().to_string()),
        (1, f.digest.clone(), Uuid::nil().to_string()),
    ] {
        assert!(f
            .app
            .automation_vault_capture(
                f.project.clone(),
                rev,
                digest,
                selected,
                &CancellationToken::new()
            )
            .await
            .is_err());
    }
    assert!(f
        .capture()
        .await
        .resolve_for_job(
            &f.app,
            &f.job(Uuid::new_v4(), &f.digest),
            &CancellationToken::new()
        )
        .await
        .is_err());
    assert!(f
        .capture()
        .await
        .resolve_for_job(
            &f.app,
            &f.job(f.project.parse().unwrap(), &"f".repeat(64)),
            &CancellationToken::new()
        )
        .await
        .is_err());
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn non_password_credential_or_secret_metadata_never_becomes_vault_material() {
    for wrong_secret in [false, true] {
        let f = Fixture::new().await;
        let (_, u) = f.app.unlocked().await.unwrap();
        let id = crate::dto::parse_id("credential_id", &f.credential).unwrap();
        let mut credential = u.working().credential(id).unwrap();
        if wrong_secret {
            let secret = cc_models::secret::Secret::new(
                SecretKind::Token,
                cc_models::secret::SecretValue::new("synthetic non-Vault token"),
            );
            credential.secret_id = Some(secret.id);
            u.writer
                .put(cc_models::VaultObject::Secret(secret))
                .await
                .unwrap();
        } else {
            credential.kind = CredentialKind::ExternalAgent;
        }
        u.writer
            .put(cc_models::VaultObject::Credential(credential))
            .await
            .unwrap();
        drop(u);
        assert!(f
            .app
            .automation_vault_capture(
                f.project.clone(),
                1,
                f.digest.clone(),
                f.credential.clone(),
                &CancellationToken::new()
            )
            .await
            .is_err());
        f.app.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn credential_password_mutation_and_delete_invalidate_captured_metadata() {
    let f = Fixture::new().await;
    let review = f.capture().await;
    let verified = f.verified();
    f.app
        .set_credential_password(
            f.credential.clone(),
            "synthetic replacement password".into(),
        )
        .await
        .unwrap();
    assert!(review
        .resolve_for_job(&f.app, &verified, &CancellationToken::new())
        .await
        .is_err());
    let review = f.capture().await;
    f.app.delete_credential(f.credential.clone()).await.unwrap();
    assert!(review
        .resolve_for_job(&f.app, &verified, &CancellationToken::new())
        .await
        .is_err());
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn real_sql_only_project_change_rejects_without_working_set_change() {
    let f = Fixture::new().await;
    let review = f.capture().await;
    let verified = f.verified();
    let (_, u) = f.app.unlocked().await.unwrap();
    let version = u.working().version();
    drop(u);
    f.app
        .automation_project_rename(f.project.clone(), 1, "Renamed by another review".into())
        .await
        .unwrap();
    let (_, u) = f.app.unlocked().await.unwrap();
    assert_eq!(version, u.working().version());
    drop(u);
    assert!(review
        .resolve_for_job(&f.app, &verified, &CancellationToken::new())
        .await
        .is_err());
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn filesystem_edit_after_capture_rejects_original_revision_and_preserves_edit() {
    let f = Fixture::new().await;
    let review = f.capture().await;
    let verified = f.verified();
    let path = review.project.path.join("playbook.yml");
    std::fs::write(&path, b"explicit later user edit\n").unwrap();
    assert!(review
        .resolve_for_job(&f.app, &verified, &CancellationToken::new())
        .await
        .is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"explicit later user edit\n");
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn lock_and_profile_close_deny_even_when_target_policy_keeps_running() {
    for close in [false, true] {
        let f = Fixture::new().await;
        let review = f.capture().await;
        let verified = f.verified();
        assert!(!verified.manifest().policy.cancel_on_lock);
        if close {
            f.app.close_profile().await.unwrap();
        } else {
            f.app.lock().await.unwrap();
        }
        assert!(review
            .resolve_for_job(&f.app, &verified, &CancellationToken::new())
            .await
            .is_err());
        f.app.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn cancellation_expiry_and_existing_review_quota_fail_closed_and_release_slots() {
    let f = Fixture::new().await;
    let verified = f.verified();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(f
        .app
        .automation_vault_capture(
            f.project.clone(),
            1,
            f.digest.clone(),
            f.credential.clone(),
            &cancel
        )
        .await
        .is_err());
    assert!(f
        .capture()
        .await
        .resolve_for_job(&f.app, &verified, &cancel)
        .await
        .is_err());
    let mut review = f.capture().await;
    review.deadline = Instant::now();
    assert!(review
        .resolve_for_job(&f.app, &verified, &CancellationToken::new())
        .await
        .is_err());
    let (owner, u) = f.app.unlocked().await.unwrap();
    drop(u);
    let all = owner
        .automation
        .review_slots
        .clone()
        .try_acquire_many_owned(owner.automation.review_slots.available_permits() as u32)
        .unwrap();
    assert!(f
        .app
        .automation_vault_capture(
            f.project.clone(),
            1,
            f.digest.clone(),
            f.credential.clone(),
            &CancellationToken::new()
        )
        .await
        .is_err());
    drop(all);
    drop(f.capture().await);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn future_reference_is_preserved_and_never_repaired_or_followed() {
    let f = Fixture::new().await;
    let context = f.app.automation_project_context(&f.project).await.unwrap();
    let text =
        json!({"version":7,"ansible_vault":{"credential_id":f.credential},"future":"opaque"})
            .to_string();
    std::fs::write(context.path.join(VAULT_INPUT_FILE), &text).unwrap();
    let revision = f
        .app
        .automation_project_files(f.project.clone())
        .await
        .unwrap();
    assert!(f
        .app
        .automation_vault_capture(
            f.project.clone(),
            1,
            revision.digest,
            f.credential.clone(),
            &CancellationToken::new()
        )
        .await
        .is_err());
    assert_eq!(
        std::fs::read_to_string(context.path.join(VAULT_INPUT_FILE)).unwrap(),
        text
    );
    f.app.shutdown().await.unwrap();
}

// A real queued SQL writer creates a deterministic post-capture await window;
// no sleeps/retry/admission hook and no fabricated validation witness.
#[tokio::test(flavor = "current_thread")]
async fn benign_current_readback_and_old_secret_removal_during_sql_preserve_fresh_metadata() {
    use std::{future::Future, task::Poll, time::Duration};
    use crate::working_set::Readback;
    let f = Fixture::new().await;
    let (_, initial) = f.app.unlocked().await.unwrap();
    let selected = crate::dto::parse_id("credential", &f.credential).unwrap();
    let old_secret = initial.working().credential(selected).unwrap().secret_id.unwrap();
    drop(initial);
    f.app.set_credential_password(f.credential.clone(), "synthetic replacement password".into()).await.unwrap();
    let review = f.capture().await;
    let (owner, u) = f.app.unlocked().await.unwrap();
    let identical = u.writer.readback(selected).await.unwrap(); // Actual current SQL payload.
    let expected = u.working().vault_credential_metadata(selected).unwrap();
    let storage = owner.storage().unwrap();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let blocker = tokio::spawn(async move {
        storage.write(move |_tx| -> AppResult<()> {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(10)).map_err(|_| AppError::internal("owned Vault SQL barrier expired"))?;
            Ok(())
        }).await
    });
    tokio::time::timeout(Duration::from_secs(10), entered_rx).await.unwrap().unwrap();
    let runtime = cc_automation_core::VaultValidationRuntime::parse_pinned_probe(serde_json::from_str(include_str!("../../../../automation-core/src/vault_validation/runtime-pin-v1.json")).unwrap()).unwrap();
    let begin = review.begin_validation(&f.app, "playbook.yml".into(), runtime);
    tokio::pin!(begin);
    std::future::poll_fn(|cx| match begin.as_mut().poll(cx) {
        Poll::Pending => Poll::Ready(()),
        Poll::Ready(_) => panic!("validation cannot pass the real blocked SQL worker"),
    }).await;
    assert!(f.app.inner.active.try_lock().is_err(), "original active guard proves the held check reached SQL");
    let version = u.working().version();
    u.working().apply(selected, identical);
    u.working().apply(old_secret, Readback::Missing);
    assert_ne!(u.working().version(), version, "old coarse-version predicate must reject this benign schedule");
    assert!(u.working().vault_credential_metadata(selected).as_ref() == Some(&expected));
    release_tx.send(()).unwrap();
    let scope = tokio::time::timeout(Duration::from_secs(10), &mut begin).await.unwrap().unwrap();
    blocker.await.unwrap().unwrap();
    drop(scope);drop(begin);drop(u);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn same_revision_credential_payload_origin_and_inline_owner_still_revoke_metadata() {
    use crate::working_set::Readback;
    use cc_models::{ObjectPayload,VaultObject};
    use cc_sync_core::DecryptedObject;
    use cc_storage_core::LocalState;
    for change in ["credential", "secret_kind", "origin", "inline_owner"] {
        let f = Fixture::new().await;
        let review = f.capture().await;
        let (_,u) = f.app.unlocked().await.unwrap();
        let selected = crate::dto::parse_id("credential",&f.credential).unwrap();
        let snapshot = u.working().vault_credential_metadata(selected).unwrap();
        match change {
            "credential" => {
                let mut credential = u.working().credential(selected).unwrap();
                credential.name = "Actual same-revision metadata changed".into();
                u.working().apply(selected,Readback::Live(Box::new(DecryptedObject{object_id:selected,revision:u.working().revision_of(selected).unwrap(),local_state:LocalState::LocalOnly,conflict_origin:None,payload:ObjectPayload::new(VaultObject::Credential(credential))})));
            }
            "secret_kind" => {
                let id = snapshot.secret();
                let mut secret = cc_models::secret::Secret::new(SecretKind::ApiKey,cc_models::secret::SecretValue::new("synthetic other kind"));secret.id=id;
                u.working().apply(id,Readback::Live(Box::new(DecryptedObject{object_id:id,revision:u.working().revision_of(id).unwrap(),local_state:LocalState::LocalOnly,conflict_origin:None,payload:ObjectPayload::new(VaultObject::Secret(secret))})));
            }
            "origin" => {
                let current = u.writer.readback(selected).await.unwrap();
                u.working().apply_in(Some(cc_protocol::VaultId::new()),selected,current);
            }
            "inline_owner" => {
                let mut host = cc_models::host::Host::new("New actual inline owner","192.0.2.19");
                host.metadata.insert(crate::host_auth::META_INLINE_CREDENTIAL.into(),selected.to_string());
                assert_eq!(crate::host_auth::inline_credential(&host),Some(selected));
                u.writer.put(VaultObject::Host(host)).await.unwrap();
            }
            _ => unreachable!(),
        }
        assert!(u.working().vault_credential_metadata(selected).as_ref()!=Some(&snapshot),"{change}");
        {let _profile=f.app.hold_session(&review.project.owner).await.unwrap();let _unlock=review.project.owner.hold_unlocked(&u).await.unwrap();assert!(review.check_held(&u).await.is_err(),"{change}");}
        drop(u);drop(review);f.app.shutdown().await.unwrap();
    }
}
