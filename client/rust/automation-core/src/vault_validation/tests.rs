use super::*;
use crate::{RunParameters, ValidationProof, VerifiedValidation, VAULT_INPUT_FILE};
use cc_models::automation::ApprovedTarget;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

fn runtime() -> VaultValidationRuntime {
    VaultValidationRuntime::parse_pinned_probe(
        serde_json::from_str(include_str!("runtime-pin-v1.json")).unwrap(),
    )
    .unwrap()
}

fn fixture() -> (tempfile::TempDir, ProjectRoot, ProjectRevision, Uuid) {
    let dir = tempfile::tempdir().unwrap();
    let id = Uuid::new_v4();
    std::fs::write(
        dir.path().join(VAULT_INPUT_FILE),
        json!({"version":1,"ansible_vault":{"credential_id":id}}).to_string(),
    )
    .unwrap();
    std::fs::write(dir.path().join("playbook.yml"), "---\n- hosts: all\n").unwrap();
    let root = ProjectRoot::open(dir.path()).unwrap();
    let revision = root.revision().unwrap();
    (dir, root, revision, id)
}
fn binding(
    root: &ProjectRoot,
    revision: &ProjectRevision,
    id: Uuid,
    phase: VaultValidationPhase,
) -> VaultValidationBinding {
    root.bind_vault_validation(
        revision,
        id,
        Uuid::new_v4(),
        phase,
        Uuid::new_v4(),
        "playbook.yml",
        &runtime(),
        0,
    )
    .unwrap()
}
fn lease(binding: &VaultValidationBinding) -> VaultValidationPasswordLease {
    VaultValidationPasswordLease::issue(
        binding,
        SecretValue::new("synthetic-password-42"),
        CancellationToken::new(),
    )
    .unwrap()
}
fn verified(revision: &str, output: &str) -> VerifiedValidation {
    let parameters = RunParameters {
        extra_vars: BTreeMap::new(),
        tags: vec![],
        skip_tags: vec![],
        diff: false,
        vault: None,
    };
    let inventory = json!({"fixture":"native owned"});
    let targets:Vec<ApprovedTarget>=serde_json::from_value(json!([{"host_id":Uuid::new_v4(),"alias":"fixture","address":"192.0.2.1","port":22,"username":"fixture","route_digest":"a".repeat(64),"credential_refs":[]}])).unwrap();
    let witness = |value: Value| {
        let raw = serde_json::to_string(&crate::project::canonical(value)).unwrap();
        json!({"digest":hex::encode(Sha256::digest(raw.as_bytes())),"canonical_json":raw})
    };
    let pw = witness(serde_json::to_value(&parameters).unwrap());
    let iw = witness(inventory.clone());
    let td = hex::encode(Sha256::digest(
        serde_json::to_vec(&crate::project::canonical(
            serde_json::to_value(&targets).unwrap(),
        ))
        .unwrap(),
    ));
    let checks=["syntax","lint"].map(|name|json!({"name":name,"exit_code":1,"output":output,"truncated":false,"cancelled":false,"timed_out":false,"elapsed_ms":1}));
    serde_json::from_value::<ValidationProof>(json!({"revision_digest":revision,"environment_digest":runtime().environment_digest(),"playbook":"playbook.yml","parameters_digest":pw["digest"],"inventory_digest":iw["digest"],"targets_digest":td,"valid":false,"checks":checks,"input_witnesses":{"parameters":pw,"inventory":iw}})).unwrap()
        .verify(revision,runtime().environment_digest(),"playbook.yml",&parameters,&inventory,&targets).unwrap()
}
#[test]
fn actual_reference_native_selection_and_whole_revision_precede_phase_binding() {
    let (dir, root, rev, id) = fixture();
    let b = binding(&root, &rev, id, VaultValidationPhase::Syntax);
    assert_eq!(b.credential_id(), id);
    assert!(root
        .bind_vault_validation(
            &rev,
            Uuid::new_v4(),
            Uuid::new_v4(),
            VaultValidationPhase::Syntax,
            Uuid::new_v4(),
            "playbook.yml",
            &runtime(),
            0
        )
        .is_err());
    std::fs::write(dir.path().join("playbook.yml"), "changed user source").unwrap();
    assert!(root
        .bind_vault_validation(
            &rev,
            id,
            Uuid::new_v4(),
            VaultValidationPhase::Syntax,
            Uuid::new_v4(),
            "playbook.yml",
            &runtime(),
            0
        )
        .is_err());
}
#[test]
fn exact_utf8_single_line_and_separate_phase_leases() {
    let (_dir, root, rev, id) = fixture();
    let parent = CancellationToken::new();
    for phase in [VaultValidationPhase::Syntax, VaultValidationPhase::Lint] {
        let b = binding(&root, &rev, id, phase);
        let mut l = VaultValidationPasswordLease::issue(
            &b,
            SecretValue::new("пароль🔒42"),
            parent.child_token(),
        )
        .unwrap();
        assert!(!format!("{l:?}").contains("пароль"));
        let mut output = vec![];
        l.write_once(&b.request(), &mut output).unwrap();
        assert_eq!(output, "пароль🔒42\n".as_bytes());
        assert!(!parent.is_cancelled());
        assert!(l.write_once(&b.request(), &mut output).is_err());
        assert_eq!(output, "пароль🔒42\n".as_bytes());
    }
}
#[test]
fn unsupported_passwords_rejected_before_issue_without_echo_or_trimming() {
    let (_dir, root, rev, id) = fixture();
    let b = binding(&root, &rev, id, VaultValidationPhase::Syntax);
    for value in [
        "".to_string(),
        "line\nother".into(),
        "line\rother".into(),
        "nul\0value".into(),
        " leading".into(),
        "trailing ".into(),
        "x\tvalue".into(),
        "x\u{2028}y".into(),
        "я".repeat(2049),
    ] {
        let error = VaultValidationPasswordLease::issue(
            &b,
            SecretValue::new(value.clone()),
            CancellationToken::new(),
        )
        .unwrap_err();
        if !value.is_empty() {
            assert!(!error.to_string().contains(&value));
        }
    }
}
#[test]
fn mismatched_phase_scope_nonce_revision_reference_consumes_without_output() {
    let (_dir, root, rev, id) = fixture();
    let b = binding(&root, &rev, id, VaultValidationPhase::Syntax);
    for field in [
        "version",
        "scope_id",
        "phase",
        "phase_nonce",
        "revision_digest",
        "reference_sha256",
    ] {
        let mut value = serde_json::to_value(b.request()).unwrap();
        value[field] = match field {
            "version" => json!(2),
            "phase" => json!("lint"),
            "scope_id" | "phase_nonce" => json!(Uuid::new_v4()),
            _ => json!("d".repeat(64)),
        };
        let request = serde_json::from_value(value).unwrap();
        let mut l = lease(&b);
        let mut output = vec![];
        assert!(l.write_once(&request, &mut output).is_err());
        assert!(output.is_empty());
        assert!(l.write_once(&b.request(), &mut output).is_err());
        assert!(output.is_empty());
    }
    let mut extra = serde_json::to_value(b.request()).unwrap();
    extra["approved"] = json!(true);
    assert!(serde_json::from_value::<VaultValidationPasswordRequest>(extra).is_err());
}
#[test]
fn cancelled_expired_and_partial_io_are_non_retryable() {
    let (_dir, root, rev, id) = fixture();
    let mut b = binding(&root, &rev, id, VaultValidationPhase::Syntax);
    let mut l = lease(&b);
    l.revoked.cancel();
    let mut output = vec![];
    assert!(l.write_once(&b.request(), &mut output).is_err());
    assert!(output.is_empty());
    b.deadline = Instant::now();
    assert!(VaultValidationPasswordLease::issue(
        &b,
        SecretValue::new("synthetic"),
        CancellationToken::new()
    )
    .is_err());
    struct Broken(bool);
    impl Write for Broken {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if !self.0 {
                self.0 = true;
                Ok(bytes.len().min(3))
            } else {
                Err(std::io::Error::other("synthetic-password-42 must not echo"))
            }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let b = binding(&root, &rev, id, VaultValidationPhase::Lint);
    let mut l = lease(&b);
    let error = l.write_once(&b.request(), &mut Broken(false)).unwrap_err();
    assert!(!error.to_string().contains("synthetic-password-42"));
    assert!(l.write_once(&b.request(), &mut vec![]).is_err());
}
#[test]
fn potential_vault_marker_removes_decrypted_values_distinct_from_password() {
    let (_dir, root, rev, id) = fixture();
    let b = binding(&root, &rev, id, VaultValidationPhase::Syntax);
    let marker = "DECRYPTED_PRIVATE_DATABASE_VALUE_917";
    let raw = verified(&rev.digest, marker);
    assert!(serde_json::to_string(&raw).unwrap().contains(marker));
    let sealed = raw.with_vault_context(&b.diagnostics_marker()).unwrap();
    assert!(!serde_json::to_string(&sealed).unwrap().contains(marker));
    assert!(sealed
        .checks()
        .iter()
        .all(|check| !check.output.contains(marker)));
    let summary = sealed.ai_summary().to_string();
    assert!(!summary.contains(marker));
    assert!(summary.contains("vault_sensitive_withheld"));
    assert!(!summary.contains("output"));
}
#[test]
fn marker_context_mismatch_fails_and_non_vault_summary_also_omits_raw_diagnostics() {
    let (_dir, root, rev, id) = fixture();
    let b = binding(&root, &rev, id, VaultValidationPhase::Syntax);
    assert!(verified(&"d".repeat(64), "decrypted marker")
        .with_vault_context(&b.diagnostics_marker())
        .is_err());
    let raw = verified(&rev.digest, "not automatically safe text");
    assert!(!serde_json::to_string(&raw)
        .unwrap()
        .contains("diagnostics_policy"));
    assert!(!raw
        .ai_summary()
        .to_string()
        .contains("not automatically safe text"));
}

#[test]
fn only_exact_censused_helper_and_entire_environment_identity_get_loader_budget() {
    let pin: Value = serde_json::from_str(include_str!("runtime-pin-v1.json")).unwrap();
    assert!(VaultValidationRuntime::parse_pinned_probe(pin.clone()).is_ok());
    for (environment, helper) in [
        (
            "bae8a4ae4e6b78bcbcac5ca6174c50e188e022dc969476f1466ad7fe3e1d7331",
            "7ae56c6de89564d6d4420c679b4a9e377a56fe731971e4c217a0b562d5e5ad6b",
        ),
        (
            "b25d68ca5967c0bf17bbdf5b0482d8e9554033ce0d4855d1ebad6eddf5a5157e",
            "95706aa1cfd2c80a77cb8e0b2a0e1d1b0f953e8bdcefdeaec6343748b7029a6a",
        ),
    ] {
        let mut previous = pin.clone();
        previous["environment_digest"] = json!(environment);
        previous["helper_digest"] = json!(helper);
        assert!(VaultValidationRuntime::parse_pinned_probe(previous).is_ok());
    }
    for (pointer, value) in [
        ("/helper_digest", json!("d".repeat(64))),
        ("/environment_digest", json!("d".repeat(64))),
        ("/python", json!("3.12.4")),
        ("/installed/pip", json!("24.1")),
        ("/dependency_files_digest", json!("d".repeat(64))),
        ("/dependency_lock", json!("d".repeat(64))),
        ("/non_root", json!(false)),
        ("/sandbox/network", json!(false)),
        ("/capabilities/target_match_v1", json!(false)),
        ("/capabilities/trusted_worker_bootstrap_v1", json!(false)),
        ("/capabilities/validate", json!(false)),
    ] {
        let mut changed = pin.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert!(
            VaultValidationRuntime::parse_pinned_probe(changed).is_err(),
            "{pointer}"
        );
    }
    let mut oversized = pin.clone();
    oversized["untrusted_extra"] = json!("X".repeat(65536));
    assert!(VaultValidationRuntime::parse_pinned_probe(oversized).is_err());
}
#[test]
fn expired_runtime_and_third_child_fail_before_reference_reader() {
    let (_dir, root, rev, id) = fixture();
    let mut expired = runtime();
    expired.deadline = Instant::now();
    assert!(root
        .bind_vault_validation(
            &rev,
            id,
            Uuid::new_v4(),
            VaultValidationPhase::Syntax,
            Uuid::new_v4(),
            "playbook.yml",
            &expired,
            0
        )
        .is_err());
    assert!(root
        .bind_vault_validation(
            &rev,
            id,
            Uuid::new_v4(),
            VaultValidationPhase::Syntax,
            Uuid::new_v4(),
            "playbook.yml",
            &runtime(),
            2
        )
        .is_err());
}
#[test]
fn lease_cannot_cross_loader_call_or_runtime_fingerprints() {
    let (_dir, root, rev, id) = fixture();
    let b = binding(&root, &rev, id, VaultValidationPhase::Syntax);
    for key in ["call_index", "environment_digest", "helper_digest"] {
        let mut v = serde_json::to_value(b.request()).unwrap();
        v[key] = if key == "call_index" {
            json!(1)
        } else {
            json!("d".repeat(64))
        };
        let request = serde_json::from_value(v).unwrap();
        let mut l = lease(&b);
        let mut pipe = vec![];
        assert!(l.write_once(&request, &mut pipe).is_err());
        assert!(pipe.is_empty());
        assert!(l.write_once(&b.request(), &mut pipe).is_err());
    }
}
#[test]
fn reviewed_current_helper_pin_matches_the_platform_normalized_embedded_package() {
    let pin:serde_json::Value=serde_json::from_str(include_str!("runtime-pin-v1.json")).unwrap();
    let expected=crate::setup::embedded_helper_digest();
    assert_eq!(pin["helper_digest"].as_str(),Some(expected.as_str()));
}
