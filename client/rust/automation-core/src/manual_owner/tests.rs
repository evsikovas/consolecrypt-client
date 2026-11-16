use super::*;
use crate::{SignedJobApproval, VerifiedJob};
use cc_crypto_core::DeviceSecretKeys;
use cc_models::automation::{ApprovedTarget, JobManifest, JobMode, JobPolicy};
use tokio::io::{AsyncWriteExt, BufReader};
use uuid::Uuid;

fn fixture(production: bool) -> (DeviceSecretKeys, SignedJobApproval, EnrolledJobGrant) {
    let keys = DeviceSecretKeys::generate().unwrap();
    let signer = Uuid::new_v4();
    let m = JobManifest {
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
            production,
        },
        mode: JobMode::Apply,
        targets: vec![ApprovedTarget {
            host_id: Uuid::new_v4(),
            alias: "owned".into(),
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
    };
    let grant = EnrolledJobGrant {
        signer_id: signer,
        public_key: keys.public_keys().signing,
        tenant_id: m.tenant_id,
        runner_id: m.runner_id,
        projects: [m.project_id].into_iter().collect(),
        hosts: [m.targets[0].host_id].into_iter().collect(),
        jobs_execute: true,
        expires_at: 1_800_003_600,
        max_job_seconds: 3600,
    };
    let signature = STANDARD.encode(keys.sign_automation_manifest(&m).unwrap());
    (
        keys,
        SignedJobApproval {
            signer_id: signer,
            manifest: m,
            signature,
        },
        grant,
    )
}
fn peer(
    response: serde_json::Value,
    error: bool,
) -> (Arc<HelperPeer>, tokio::task::JoinHandle<()>) {
    let (client, server) = tokio::io::duplex(8192);
    let (reader, writer) = tokio::io::split(client);
    let peer = HelperPeer::connect(reader, writer);
    let task = tokio::spawn(async move {
        let (read, mut write) = tokio::io::split(server);
        let mut read = BufReader::new(read);
        while let Some(raw) = crate::read_frame(&mut read).await.unwrap() {
            let request: serde_json::Value = serde_json::from_slice(&raw).unwrap();
            assert_eq!(request["operation"], "configure_manual_owner");
            let mut row = json!({"version":1,"type":"response","request_id":request["request_id"],"result":response,"error":null});
            if error {
                row["result"] = serde_json::Value::Null;
                row["error"] =
                    json!({"code":"service_purpose","message":"Explicit service purpose required"});
            }
            let mut raw = serde_json::to_vec(&row).unwrap();
            raw.push(b'\n');
            if write.write_all(&raw).await.is_err() {
                break;
            }
        }
    });
    (peer, task)
}
fn ack() -> serde_json::Value {
    json!({"configured":true,"persistent":false,"owner_policy":"ownerConfirmationVaultPassphraseV1"})
}

#[test]
fn nonproduction_public_struct_wire_and_verifier_stay_three_fields() {
    let (_, approval, grant) = fixture(false);
    let value = serde_json::to_value(&approval).unwrap();
    assert_eq!(
        value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["manifest", "signature", "signer_id"]
    );
    let verified = VerifiedJob::verify(&approval, &grant, &"c".repeat(64), 1_800_000_000).unwrap();
    assert!(verified.owner_confirmation().is_none());
}

#[tokio::test]
async fn valid_node_extra_signature_and_service_configuration_cannot_create_manual_grant() {
    let (node, approval, grant) = fixture(true);
    let extra = node
        .sign_production_owner_confirmation(&approval.manifest, grant.signer_id)
        .unwrap();
    extra
        .verify(&approval.manifest, grant.signer_id, &grant.public_key)
        .unwrap();
    assert!(VerifiedJob::verify(&approval, &grant, &"c".repeat(64), 1_800_000_000).is_err());
    let (service, worker) = peer(ack(), true);
    assert!(ManualOwnerGrant::configure(&service, grant).await.is_err());
    service.close().await;
    worker.abort();
}

#[tokio::test]
async fn original_manual_owner_exact_manifest_verifies_then_peer_close_retires_held_result() {
    let (owner, approval, grant) = fixture(true);
    let (original, worker) = peer(ack(), false);
    let manual = ManualOwnerGrant::configure(&original, grant.clone())
        .await
        .unwrap();
    let extra = owner
        .sign_production_owner_confirmation(&approval.manifest, grant.signer_id)
        .unwrap();
    let verified = VerifiedJob::verify_owner_confirmed(
        &approval,
        &manual,
        &extra,
        &"c".repeat(64),
        1_800_000_000,
    )
    .unwrap();
    assert!(verified.owner_confirmation().is_some());
    assert!(!verified.is_access_expired());
    original.close().await;
    assert!(verified.is_access_expired());
    assert!(verified.is_submission_expired());
    assert_eq!(verified.dispatch_remaining(), Duration::ZERO);
    assert!(!manual.matches_enrolled(&grant));
    worker.abort();
}

#[tokio::test]
async fn owner_evidence_cannot_follow_reminted_or_changed_manifest_even_with_fresh_job_signature() {
    let (owner, approval, grant) = fixture(true);
    let (original, worker) = peer(ack(), false);
    let manual = ManualOwnerGrant::configure(&original, grant.clone())
        .await
        .unwrap();
    let extra = owner
        .sign_production_owner_confirmation(&approval.manifest, grant.signer_id)
        .unwrap();
    for which in 0..4 {
        let mut changed = approval.clone();
        match which {
            0 => changed.manifest.nonce = Uuid::new_v4(),
            1 => changed.manifest.job_id = Uuid::new_v4(),
            2 => changed.manifest.expires_at -= 1,
            _ => changed.manifest.targets[0].username = "root".into(),
        }
        changed.signature =
            STANDARD.encode(owner.sign_automation_manifest(&changed.manifest).unwrap());
        assert!(VerifiedJob::verify_owner_confirmed(
            &changed,
            &manual,
            &extra,
            &"c".repeat(64),
            1_800_000_000
        )
        .is_err());
    }
    original.close().await;
    worker.abort();
}

#[tokio::test]
async fn future_or_self_reported_configuration_flag_does_not_issue_manual_scope() {
    let (_, _, grant) = fixture(true);
    for response in [
        json!({"configured":true}),
        json!({"configured":true,"persistent":false,"owner_policy":"future"}),
        json!({"configured":true,"persistent":false,"owner_policy":"ownerConfirmationVaultPassphraseV1","approved":true}),
    ] {
        let (peer, worker) = peer(response, false);
        assert!(ManualOwnerGrant::configure(&peer, grant.clone())
            .await
            .is_err());
        peer.close().await;
        worker.abort();
    }
}
