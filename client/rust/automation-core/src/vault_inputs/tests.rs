use super::*;
use crate::{EnrolledJobGrant, SignedJobApproval};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::json;

fn document(id: Uuid) -> String {
    json!({"version":1,"ansible_vault":{"credential_id":id}}).to_string()
}
fn fixture() -> (tempfile::TempDir, ProjectRoot, ProjectRevision, Uuid) {
    let dir = tempfile::tempdir().unwrap();
    let selected = Uuid::new_v4();
    std::fs::write(dir.path().join(VAULT_INPUT_FILE), document(selected)).unwrap();
    std::fs::write(dir.path().join("playbook.yml"), "---\n- hosts: all\n").unwrap();
    let root = ProjectRoot::open(dir.path()).unwrap();
    let revision = root.revision().unwrap();
    (dir, root, revision, selected)
}
pub(super) fn verified_job(revision: &str) -> VerifiedJob {
    let keys = cc_crypto_core::DeviceSecretKeys::generate().unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let signer = Uuid::new_v4();
    let tenant = Uuid::new_v4();
    let runner = Uuid::new_v4();
    let project = Uuid::new_v4();
    let host = Uuid::new_v4();
    let manifest = serde_json::from_value(json!({"version":1,"job_id":Uuid::new_v4(),
        "tenant_id":tenant,"runner_id":runner,"project_id":project,"revision_digest":revision,
        "inventory_digest":"b".repeat(64),"environment_digest":"c".repeat(64),"parameters_digest":"d".repeat(64),
        "playbook":"playbook.yml","mode":"check","policy":{"strict_isolation":true,"cancel_on_lock":false,
        "timeout_seconds":60,"forks":1,"production":false},"targets":[{"host_id":host,"alias":"fixture",
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
fn request(job: &VerifiedJob, binding: &VaultInputBinding) -> VaultPasswordRequest {
    VaultPasswordRequest {
        version: 1,
        purpose: VaultPasswordPurpose::AnsibleVaultPassword,
        job_id: job.manifest().job_id,
        manifest_digest: job.digest().into(),
        revision_digest: binding.revision_digest().into(),
        reference_sha256: binding.reference_sha256().into(),
    }
}
fn lease(job: &VerifiedJob, binding: &VaultInputBinding) -> VaultPasswordLease {
    VaultPasswordLease::issue(
        job,
        binding,
        SecretValue::new("synthetic-vault-password-42"),
        CancellationToken::new(),
        CancellationToken::new(),
        Duration::from_secs(30),
    )
    .unwrap()
}

#[test]
fn native_selection_and_entire_current_revision_are_mandatory() {
    let (dir, root, revision, id) = fixture();
    let binding = root.bind_vault_input(&revision, id).unwrap();
    assert_eq!(binding.credential_id(), id);
    assert_eq!(binding.revision_digest(), revision.digest);
    assert_eq!(binding.reference_sha256().len(), 64);
    assert!(root.bind_vault_input(&revision, Uuid::new_v4()).is_err());
    assert!(root.bind_vault_input(&revision, Uuid::nil()).is_err());
    let mut fabricated = revision.clone();
    fabricated.digest = "a".repeat(64);
    assert!(root.bind_vault_input(&fabricated, id).is_err());
    std::fs::write(dir.path().join("playbook.yml"), "changed unrelated task").unwrap();
    assert!(root.bind_vault_input(&revision, id).is_err());
}

#[test]
fn strict_reference_schema_rejects_secret_fields_duplicates_future_and_limits() {
    let (dir, root, _, id) = fixture();
    for text in [
        document(Uuid::nil()),
        document(id).replace("\"version\":1", "\"version\":2"),
        format!(
            r#"{{"version":1,"ansible_vault":{{"credential_id":"{id}","password":"never-authority"}}}}"#
        ),
        format!(r#"{{"version":1,"version":1,"ansible_vault":{{"credential_id":"{id}"}}}}"#),
        format!(
            r#"{{"version":1,"ansible_vault":{{"credential_id":"{id}","credential_id":"{id}"}}}}"#
        ),
        format!(
            r#"{{"version":1,"ansible_vault":{{"credential_id":"{}"}}}}"#,
            format!("{{{id}}}")
        ),
        format!("{}{{}}", document(id)),
        format!("{}{}", document(id), " ".repeat(4097)),
    ] {
        std::fs::write(dir.path().join(VAULT_INPUT_FILE), text).unwrap();
        let revision = root.revision().unwrap();
        assert!(root.bind_vault_input(&revision, id).is_err());
    }
    std::fs::remove_file(dir.path().join(VAULT_INPUT_FILE)).unwrap();
    assert!(root
        .bind_vault_input(&root.revision().unwrap(), id)
        .is_err());
}

#[cfg(unix)]
#[test]
fn reference_symlink_and_hardlink_are_not_regular_project_inputs() {
    use std::os::unix::fs::symlink;
    let (dir, root, revision, id) = fixture();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("reference");
    std::fs::write(&target, document(id)).unwrap();
    std::fs::remove_file(dir.path().join(VAULT_INPUT_FILE)).unwrap();
    symlink(&target, dir.path().join(VAULT_INPUT_FILE)).unwrap();
    assert!(root.bind_vault_input(&revision, id).is_err());
    std::fs::remove_file(dir.path().join(VAULT_INPUT_FILE)).unwrap();
    std::fs::hard_link(&target, dir.path().join(VAULT_INPUT_FILE)).unwrap();
    assert!(root.bind_vault_input(&revision, id).is_err());
    assert_eq!(std::fs::read_to_string(target).unwrap(), document(id));
}

#[test]
fn password_only_reaches_private_pipe_once_and_debug_request_are_reference_only() {
    let (_dir, root, revision, id) = fixture();
    let binding = root.bind_vault_input(&revision, id).unwrap();
    let job = verified_job(&revision.digest);
    let request = request(&job, &binding);
    let mut lease = lease(&job, &binding);
    assert!(!format!("{lease:?}").contains("synthetic-vault-password-42"));
    let refs = serde_json::to_string(&request).unwrap();
    assert!(!refs.contains("synthetic-vault-password-42"));
    assert!(!refs.contains(&id.to_string())); // no caller-selected secret ID on wire
    let mut pipe = Vec::new();
    lease.write_once(&request, &mut pipe).unwrap();
    assert_eq!(pipe, b"synthetic-vault-password-42\n");
    let mut replay = Vec::new();
    assert!(lease.write_once(&request, &mut replay).is_err());
    assert!(replay.is_empty());
}

#[test]
fn every_request_scope_field_is_exact_and_failure_consumes_without_output() {
    let (_dir, root, revision, id) = fixture();
    let binding = root.bind_vault_input(&revision, id).unwrap();
    let job = verified_job(&revision.digest);
    for field in [
        "version",
        "job_id",
        "manifest_digest",
        "revision_digest",
        "reference_sha256",
    ] {
        let mut req = request(&job, &binding);
        match field {
            "version" => req.version = 2,
            "job_id" => req.job_id = Uuid::new_v4(),
            "manifest_digest" => req.manifest_digest = "f".repeat(64),
            "revision_digest" => req.revision_digest = "f".repeat(64),
            _ => req.reference_sha256 = "f".repeat(64),
        }
        let mut lease = lease(&job, &binding);
        let mut output = Vec::new();
        assert!(lease.write_once(&req, &mut output).is_err());
        assert!(output.is_empty());
        assert!(lease
            .write_once(&request(&job, &binding), &mut output)
            .is_err());
    }
    let mut wire = serde_json::to_value(request(&job, &binding)).unwrap();
    wire["credential_id"] = json!(id);
    assert!(serde_json::from_value::<VaultPasswordRequest>(wire).is_err());
    let mut wire = serde_json::to_value(request(&job, &binding)).unwrap();
    wire["purpose"] = json!("ssh_password");
    assert!(serde_json::from_value::<VaultPasswordRequest>(wire).is_err());
}

#[test]
fn invalid_material_ttl_signature_revision_and_both_cancellations_deny() {
    let (_dir, root, revision, id) = fixture();
    let binding = root.bind_vault_input(&revision, id).unwrap();
    let job = verified_job(&revision.digest);
    for text in [
        "".into(),
        "x\nsecret".into(),
        "x\rsecret".into(),
        "x\0secret".into(),
        "x".repeat(4097),
    ] {
        assert!(VaultPasswordLease::issue(
            &job,
            &binding,
            SecretValue::new(text),
            CancellationToken::new(),
            CancellationToken::new(),
            Duration::from_secs(1)
        )
        .is_err());
    }
    for ttl in [Duration::ZERO, Duration::from_secs(31)] {
        assert!(VaultPasswordLease::issue(
            &job,
            &binding,
            SecretValue::new("fixture"),
            CancellationToken::new(),
            CancellationToken::new(),
            ttl
        )
        .is_err());
    }
    assert!(VaultPasswordLease::issue(
        &verified_job(&"f".repeat(64)),
        &binding,
        SecretValue::new("fixture"),
        CancellationToken::new(),
        CancellationToken::new(),
        Duration::from_secs(1)
    )
    .is_err());
    for kind in [0, 1, 2] {
        let mut lease = lease(&job, &binding);
        match kind {
            0 => lease.revoked.cancel(),
            1 => lease.job_cancel.cancel(),
            _ => lease.deadline = Instant::now(),
        }
        let mut out = Vec::new();
        assert!(lease
            .write_once(&request(&job, &binding), &mut out)
            .is_err());
        assert!(out.is_empty());
    }
}

#[test]
fn partial_pipe_failure_is_not_retryable_and_error_does_not_echo_os_details() {
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("synthetic-vault-password-42"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let (_dir, root, revision, id) = fixture();
    let binding = root.bind_vault_input(&revision, id).unwrap();
    let job = verified_job(&revision.digest);
    let mut lease = lease(&job, &binding);
    let req = request(&job, &binding);
    let error = lease.write_once(&req, &mut Broken).unwrap_err();
    assert!(!format!("{error:?}").contains("synthetic-vault-password-42"));
    assert!(lease.write_once(&req, &mut Vec::new()).is_err());
}

#[test]
fn admitted_material_detaches_only_original_unlock_and_remains_one_use_exact_job() {
    let (_dir, root, revision, selected) = fixture();
    let binding = root.bind_vault_input(&revision, selected).unwrap();
    let job = verified_job(&revision.digest);
    let original = CancellationToken::new();
    let job_cancel = CancellationToken::new();
    let issued = VaultPasswordLease::issue(
        &job,
        &binding,
        SecretValue::new("synthetic-password42"),
        original.clone(),
        job_cancel.clone(),
        Duration::from_secs(30),
    )
    .unwrap();
    let material = CancellationToken::new();
    let mut admitted = issued.into_admitted_job(&job, material.clone()).unwrap();
    original.cancel(); // Only preissued exact-job material may continue on lock.
    let mut pipe = Vec::new();
    admitted
        .write_once(&request(&job, &binding), &mut pipe)
        .unwrap();
    assert_eq!(pipe, b"synthetic-password42\n");
    assert!(admitted
        .write_once(&request(&job, &binding), &mut Vec::new())
        .is_err());
}
#[test]
fn admitted_material_job_cancel_and_material_revoke_prevent_any_password_bytes() {
    let (_dir, root, revision, selected) = fixture();
    let binding = root.bind_vault_input(&revision, selected).unwrap();
    for cancel_job in [false, true] {
        let job = verified_job(&revision.digest);
        let job_cancel = CancellationToken::new();
        let material = CancellationToken::new();
        let issued = VaultPasswordLease::issue(
            &job,
            &binding,
            SecretValue::new("synthetic-password42"),
            CancellationToken::new(),
            job_cancel.clone(),
            Duration::from_secs(30),
        )
        .unwrap();
        let mut admitted = issued.into_admitted_job(&job, material.clone()).unwrap();
        if cancel_job {
            job_cancel.cancel();
        } else {
            material.cancel();
        }
        let mut pipe = Vec::new();
        assert!(admitted
            .write_once(&request(&job, &binding), &mut pipe)
            .is_err());
        assert!(pipe.is_empty());
    }
}
#[test]
fn reference_inspection_reuses_strict_parser_and_directory_freshness_is_not_tree_digest() {
    let (dir, root, revision, selected) = fixture();
    let observed = root.vault_input_reference(&revision).unwrap().unwrap();
    assert_eq!(observed.credential_id(), selected);
    let other = tempfile::tempdir().unwrap();
    for name in [VAULT_INPUT_FILE, "playbook.yml"] {
        std::fs::copy(dir.path().join(name), other.path().join(name)).unwrap();
    }
    let copied = ProjectRoot::open(other.path()).unwrap();
    assert_eq!(root.revision().unwrap(), copied.revision().unwrap());
    assert_ne!(
        root.directory_identity_digest().unwrap(),
        copied.directory_identity_digest().unwrap()
    );
    let file = root.read_file(VAULT_INPUT_FILE).unwrap();
    root.write_file(
        VAULT_INPUT_FILE,
        Some(&file.sha256),
        "{\"version\":1,\"version\":1}",
    )
    .unwrap();
    assert!(root
        .vault_input_reference(&root.revision().unwrap())
        .is_err());
}

#[test]
fn diagnostic_overflow_censors_without_changing_one_use_vault_material_authority(){
    let (_dir,root,revision,id)=fixture();let binding=root.bind_vault_input(&revision,id).unwrap();
    let job=verified_job(&revision.digest);let request=request(&job,&binding);let lease=lease(&job,&binding);
    let mask=cc_ssh_core::DiagnosticCapture::new();for n in 0..512{mask.remember(&format!("owned-prefill-{n}")).unwrap();}
    lease.retain_diagnostic(&mask).unwrap();assert!(mask.redact("synthetic-vault-password-42").is_err());
    let mut admitted=lease.into_admitted_job(&job,CancellationToken::new()).unwrap();
    let mut private_pipe=Vec::new();admitted.write_once(&request,&mut private_pipe).unwrap();
    assert_eq!(private_pipe,b"synthetic-vault-password-42\n");
    let mut replay=Vec::new();assert!(admitted.write_once(&request,&mut replay).is_err());assert!(replay.is_empty());
    let expired=lease_for_diagnostic_cancel(&job,&binding);
    assert!(expired.retain_diagnostic(&mask).is_err());
}
fn lease_for_diagnostic_cancel(job:&VerifiedJob,binding:&VaultInputBinding)->VaultPasswordLease{
    let mut value=lease(job,binding);value.deadline=Instant::now();value
}
