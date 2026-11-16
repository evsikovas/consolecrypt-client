use base64::{engine::general_purpose::STANDARD, Engine};
use cc_automation_core::{EnrolledJobGrant, SignedJobApproval, VerifiedJob};
use cc_models::automation::JobManifest;
use serde_json::json;
use uuid::Uuid;

fn fixture() -> (SignedJobApproval, EnrolledJobGrant) {
    let key = cc_crypto_core::DeviceSecretKeys::generate().unwrap();
    let signer = Uuid::new_v4();
    let tenant = Uuid::new_v4();
    let runner = Uuid::new_v4();
    let project = Uuid::new_v4();
    let host = Uuid::new_v4();
    let manifest: JobManifest = serde_json::from_value(json!({"version": 1, "job_id": Uuid::new_v4(),
        "tenant_id": tenant, "runner_id": runner, "project_id": project,
        "revision_digest": "a".repeat(64), "inventory_digest": "b".repeat(64), "environment_digest": "c".repeat(64),
        "parameters_digest": "d".repeat(64), "playbook": "папка с пробелами/playbook.yml", "mode": "apply",
        "policy": {"strict_isolation": true, "cancel_on_lock": true, "timeout_seconds": 3600, "forks": 5, "production": false},
        "targets": [{"host_id": host, "alias": "fixture", "address": "127.0.0.1", "port": 22,
            "username": "runner", "route_digest": "e".repeat(64), "credential_refs": [Uuid::new_v4()]}],
        "issued_at": 1800000000i64, "expires_at": 1800000600i64, "access_expires_at": 1800003600i64, "nonce": Uuid::new_v4()})).unwrap();
    let signature = STANDARD.encode(key.sign_automation_manifest(&manifest).unwrap());
    (
        SignedJobApproval {
            signer_id: signer,
            manifest,
            signature,
        },
        EnrolledJobGrant {
            signer_id: signer,
            public_key: key.public_keys().signing,
            tenant_id: tenant,
            runner_id: runner,
            projects: [project].into(),
            hosts: [host].into(),
            jobs_execute: true,
            expires_at: 1800003600,
            max_job_seconds: 3600,
        },
    )
}

#[test]
fn time_spent_waiting_after_signature_verification_does_not_extend_access() {
    let (mut approval, mut grant) = fixture();
    let key = cc_crypto_core::DeviceSecretKeys::generate().unwrap();
    grant.public_key = key.public_keys().signing;
    approval.manifest.access_expires_at = approval.manifest.issued_at + 1;
    approval.signature = STANDARD.encode(key.sign_automation_manifest(&approval.manifest).unwrap());
    let verified = VerifiedJob::verify(
        &approval,
        &grant,
        &"c".repeat(64),
        approval.manifest.issued_at,
    )
    .unwrap();
    assert!(!verified.is_access_expired());
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert!(verified.is_access_expired());
}

#[test]
fn submission_window_expires_in_the_queue_even_with_a_longer_access_lease() {
    let (mut approval, mut grant) = fixture();
    let key = cc_crypto_core::DeviceSecretKeys::generate().unwrap();
    grant.public_key = key.public_keys().signing;
    approval.manifest.expires_at = approval.manifest.issued_at + 1;
    approval.signature = STANDARD.encode(key.sign_automation_manifest(&approval.manifest).unwrap());
    let verified = VerifiedJob::verify(
        &approval,
        &grant,
        &"c".repeat(64),
        approval.manifest.issued_at,
    )
    .unwrap();
    assert!(!verified.is_submission_expired());
    assert!(!verified.dispatch_remaining().is_zero());
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert!(verified.is_submission_expired());
    assert!(verified.dispatch_remaining().is_zero());
    assert!(!verified.is_access_expired());
}

#[test]
fn domain_login_and_a_full_jump_chain_remain_data_in_the_signed_scope() {
    let (mut approval,mut grant)=fixture();
    let key=cc_crypto_core::DeviceSecretKeys::generate().unwrap();
    grant.public_key=key.public_keys().signing;
    approval.manifest.targets[0].username=r"CORP\alice@example.test".into();
    approval.manifest.targets[0].credential_refs=(0..34).map(|_|Uuid::new_v4()).collect();
    approval.signature=STANDARD.encode(key.sign_automation_manifest(&approval.manifest).unwrap());
    assert!(VerifiedJob::verify(&approval,&grant,&"c".repeat(64),approval.manifest.issued_at).is_ok());
    approval.manifest.targets[0].username="user\ninjected".into();
    assert!(key.sign_automation_manifest(&approval.manifest).is_err());
}

#[test]
fn native_verifier_binds_the_frozen_manifest_and_enrolled_scope() {
    let (mut approval, grant) = fixture();
    let verified = VerifiedJob::verify(&approval, &grant, &"c".repeat(64), 1800000000).unwrap();
    assert_eq!(verified.digest().len(), 64);
    approval.manifest.playbook = "another.yml".into();
    assert!(VerifiedJob::verify(&approval, &grant, &"c".repeat(64), 1800000000).is_err());
    assert_eq!(
        verified.manifest().playbook,
        "папка с пробелами/playbook.yml"
    );
    let (approval, mut grant) = fixture();
    assert!(VerifiedJob::verify(&approval, &grant, &"f".repeat(64), 1800000000).is_err());
    assert!(VerifiedJob::verify(&approval, &grant, &"c".repeat(64), 1800000601).is_err());
    grant.hosts.clear();
    assert!(VerifiedJob::verify(&approval, &grant, &"c".repeat(64), 1800000000).is_err());
}

#[test]
fn job_signature_cannot_be_reused_as_another_device_operation() {
    let key = cc_crypto_core::DeviceSecretKeys::generate().unwrap();
    let (approval, _) = fixture();
    let signature = key.sign_automation_manifest(&approval.manifest).unwrap();
    cc_crypto_core::verify_automation_manifest(
        &key.public_keys().signing,
        &approval.manifest,
        &signature,
    )
    .unwrap();
    let other = cc_crypto_core::DeviceSecretKeys::generate().unwrap();
    assert!(cc_crypto_core::verify_automation_manifest(
        &other.public_keys().signing,
        &approval.manifest,
        &signature
    )
    .is_err());
    // Raw/cross-domain messages do not reach a generic public signing method.
    let mut malformed = approval.manifest;
    malformed.targets[0].alias = "--arbitrary-option".into();
    assert!(key.sign_automation_manifest(&malformed).is_err());
}
