use super::*;
use cc_models::automation::{ApprovedTarget, JobMode, JobPolicy};

pub(super) fn manifest() -> JobManifest {
    JobManifest {
        version: 1,
        job_id: Uuid::new_v4(),
        tenant_id: Uuid::new_v4(),
        runner_id: Uuid::new_v4(),
        project_id: Uuid::new_v4(),
        revision_digest: "a".repeat(64),
        inventory_digest: "b".repeat(64),
        environment_digest: "c".repeat(64),
        parameters_digest: "d".repeat(64),
        playbook: "site.yml".into(),
        policy: JobPolicy {
            strict_isolation: true,
            cancel_on_lock: true,
            timeout_seconds: 300,
            forks: 1,
            production: true,
        },
        mode: JobMode::Apply,
        targets: vec![ApprovedTarget {
            host_id: Uuid::new_v4(),
            alias: "owned_fixture".into(),
            address: "127.0.0.1".into(),
            port: 22,
            username: "runner".into(),
            route_digest: "e".repeat(64),
            credential_refs: vec![],
        }],
        issued_at: 1_800_000_000,
        expires_at: 1_800_000_300,
        access_expires_at: 1_800_000_300,
        nonce: Uuid::new_v4(),
    }
}

#[test]
fn signature_binds_exact_frozen_manifest_and_original_signer() {
    let keys = DeviceSecretKeys::generate().unwrap();
    let signer = Uuid::new_v4();
    let m = manifest();
    let proof = keys.sign_production_owner_confirmation(&m, signer).unwrap();
    proof
        .verify(&m, signer, &keys.public_keys().signing)
        .unwrap();
    assert!(proof
        .verify(&m, Uuid::new_v4(), &keys.public_keys().signing)
        .is_err());
    assert!(proof
        .verify(
            &m,
            signer,
            &DeviceSecretKeys::generate().unwrap().public_keys().signing
        )
        .is_err());
    for kind in 0..9 {
        let mut changed = m.clone();
        match kind {
            0 => changed.job_id = Uuid::new_v4(),
            1 => changed.nonce = Uuid::new_v4(),
            2 => changed.issued_at += 1,
            3 => changed.expires_at -= 1,
            4 => changed.parameters_digest = "0".repeat(64),
            5 => changed.targets[0].username = "root".into(),
            6 => changed.mode = JobMode::Check,
            7 => changed.environment_digest = "0".repeat(64),
            _ => changed.policy.production = false,
        }
        assert!(proof
            .verify(&changed, signer, &keys.public_keys().signing)
            .is_err());
    }
}

#[test]
fn cannot_reuse_job_signature_or_future_policy_and_closed_schema() {
    let keys = DeviceSecretKeys::generate().unwrap();
    let signer = Uuid::new_v4();
    let m = manifest();
    let proof = keys.sign_production_owner_confirmation(&m, signer).unwrap();
    let mut value = serde_json::to_value(&proof).unwrap();
    value["signature"] =
        serde_json::json!(STANDARD.encode(keys.sign_automation_manifest(&m).unwrap()));
    let reused: OwnerConfirmation = serde_json::from_value(value).unwrap();
    assert!(reused
        .verify(&m, signer, &keys.public_keys().signing)
        .is_err());
    let raw = serde_json::to_string(&proof).unwrap();
    let duplicate = raw.replacen("\"version\":1", "\"version\":1,\"version\":1", 1);
    assert!(serde_json::from_str::<OwnerConfirmation>(&duplicate).is_err());
    let mut future = serde_json::to_value(&proof).unwrap();
    future["policy"] = serde_json::json!("unknownPolicy");
    assert!(serde_json::from_value::<OwnerConfirmation>(future).is_err());
    let mut extra = serde_json::to_value(&proof).unwrap();
    extra["approved"] = serde_json::json!(true);
    assert!(serde_json::from_value::<OwnerConfirmation>(extra).is_err());
    let mut nonprod = m;
    nonprod.policy.production = false;
    assert!(keys
        .sign_production_owner_confirmation(&nonprod, signer)
        .is_err());
}
