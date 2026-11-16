use super::*;

fn row() -> Value {
    let (job, profile, runner, project, signer, host) = (
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    let manifest:JobManifest=serde_json::from_value(json!({"version":1,"job_id":job,"tenant_id":profile,"runner_id":runner,
        "project_id":project,"revision_digest":"a".repeat(64),"inventory_digest":"b".repeat(64),"environment_digest":"c".repeat(64),
        "parameters_digest":"d".repeat(64),"playbook":"playbook.yml","mode":"apply",
        "policy":{"strict_isolation":true,"cancel_on_lock":true,"timeout_seconds":60,"forks":1,"production":false},
        "targets":[{"host_id":host,"alias":"owned","address":"secret.example.test","port":22,"username":"secretuser","route_digest":"e".repeat(64),"credential_refs":[Uuid::new_v4()]}],
        "issued_at":100,"expires_at":200,"access_expires_at":160,"nonce":Uuid::new_v4()})).unwrap();
    let mut signed = JOB_SIGNATURE_DOMAIN.to_vec();
    signed.extend(manifest.canonical_bytes().unwrap());
    let digest = hash(&signed);
    json!({"version":1,"job_id":job,"project_id":project,"runner_id":runner,"host_ids":[host],"manifest_digest":digest,"manifest":manifest,
        "correlation_provenance":{"version":1,"jobId":job,"profileId":profile,"deviceId":signer,"projectId":project,"projectRevision":7,
            "runnerId":runner,"revisionObjectId":Uuid::new_v4(),"revisionDigest":"a".repeat(64)}})
}
fn recorded(row: &Value) -> Value {
    let Expected::Original { remote, .. } = expected(row) else {
        panic!("original test admission missing")
    };
    json!({"version":1,"status":"recorded","record":remote})
}
fn evidence(row: &Value, audit: Value) -> Value {
    json!({"job_id":row["job_id"],"manifest_digest":row["manifest_digest"],"state":"unknown","result":{"exit_code":-15,"cancel_requested":true,"timed_out":false,"partial":true},"admission_audit":audit})
}

#[cfg_attr(test, test)]
pub fn exact_original_admission_is_projected_without_private_fields_or_result_change() {
    let mut r = row();
    let audit = recorded(&r);
    r["runner_result"] = evidence(&r, audit);
    let mut original = r["runner_result"].clone();
    original.as_object_mut().unwrap().remove("admission_audit");
    project_row(&mut r);
    assert_eq!(r["runner_result"], original);
    assert_eq!(r["admission_audit"]["status"], "recorded");
    assert_eq!(
        r["admission_audit"]["signerId"],
        r["correlation_provenance"]["deviceId"]
    );
    assert_eq!(
        r["admission_audit"]["targets"][0]["routeDigest"],
        "e".repeat(64)
    );
    let out = r["admission_audit"].to_string();
    for private in [
        "secret.example.test",
        "secretuser",
        "credential_refs",
        "owner_confirmation",
        "signature",
    ] {
        assert!(!out.contains(private));
    }
    let before = r.clone();
    project_row(&mut r);
    assert_eq!(r, before);
}

#[cfg_attr(test, test)]
pub fn nested_uncertain_result_is_same_outcome_and_metadata_is_removed() {
    let mut r = row();
    let audit = recorded(&r);
    r["runner_result"] =
        json!({"reason":"native_operation_not_acknowledged","runner_result":evidence(&r,audit)});
    project_row(&mut r);
    assert_eq!(
        r["runner_result"]["reason"],
        "native_operation_not_acknowledged"
    );
    assert_eq!(
        r["runner_result"]["runner_result"]["result"]["partial"],
        true
    );
    assert!(
        r["runner_result"]["runner_result"]
            .get("admission_audit")
            .is_none()
    );
    assert_eq!(r["admission_audit"]["status"], "recorded");
}

#[cfg_attr(test, test)]
pub fn foreign_signer_scope_route_or_extra_field_never_becomes_recorded() {
    for field in [
        "signer_id",
        "job_id",
        "project_id",
        "runner_id",
        "revision_digest",
        "scope",
        "extra",
    ] {
        let mut r = row();
        let mut audit = recorded(&r);
        audit["record"][field] = json!("foreign-private-material");
        r["runner_result"] = evidence(&r, audit);
        project_row(&mut r);
        assert_eq!(r["admission_audit"], unavailable(), "{field}");
        assert!(!r.to_string().contains("foreign-private-material"));
    }
    let mut r = row();
    let mut audit = recorded(&r);
    audit["record"]["targets"][0]["route_digest"] = json!("f".repeat(64));
    r["runner_result"] = evidence(&r, audit);
    project_row(&mut r);
    assert_eq!(r["admission_audit"], unavailable());
}

#[cfg_attr(test, test)]
pub fn missing_native_provenance_never_uses_the_remote_or_current_identity() {
    let mut r = row();
    let audit = recorded(&r);
    r["runner_result"] = evidence(&r, audit);
    r.as_object_mut().unwrap().remove("correlation_provenance");
    project_row(&mut r);
    assert_eq!(r["admission_audit"], missing("legacy_native_provenance"));
    assert!(r["runner_result"].get("admission_audit").is_none());
}

#[cfg_attr(test, test)]
pub fn local_manifest_provenance_or_recorded_projection_changes_are_rechecked() {
    for field in [
        "jobId",
        "profileId",
        "projectId",
        "runnerId",
        "revisionDigest",
    ] {
        let mut r = row();
        let audit = recorded(&r);
        r["runner_result"] = evidence(&r, audit);
        r["correlation_provenance"][field] = json!(Uuid::new_v4());
        project_row(&mut r);
        assert_eq!(r["admission_audit"], unavailable(), "{field}");
    }
    let mut r = row();
    r["admission_audit"] = json!({"version":1,"status":"recorded","signerId":Uuid::new_v4()});
    project_row(&mut r);
    assert_eq!(r["admission_audit"], missing("not_observed"));
}

#[cfg_attr(test, test)]
pub fn production_needs_the_actual_original_proof_digest() {
    let mut r = row();
    r["manifest"]["policy"]["production"] = json!(true);
    let m: JobManifest = serde_json::from_value(r["manifest"].clone()).unwrap();
    let mut signed = JOB_SIGNATURE_DOMAIN.to_vec();
    signed.extend(m.canonical_bytes().unwrap());
    r["manifest_digest"] = json!(hash(&signed));
    project_row(&mut r);
    assert_eq!(r["admission_audit"], unavailable());
    let key = cc_crypto_core::DeviceSecretKeys::generate().unwrap();
    let signer: Uuid =
        serde_json::from_value(r["correlation_provenance"]["deviceId"].clone()).unwrap();
    r["owner_confirmation"] =
        serde_json::to_value(key.sign_production_owner_confirmation(&m, signer).unwrap()).unwrap();
    let audit = recorded(&r);
    r["runner_result"] = evidence(&r, audit);
    project_row(&mut r);
    assert_eq!(r["admission_audit"]["status"], "recorded");
    assert!(r["admission_audit"]["ownerEvidenceDigest"].is_string());
    r.as_object_mut().unwrap().remove("owner_confirmation");
    project_row(&mut r);
    assert_eq!(r["admission_audit"], unavailable());
}

#[cfg_attr(test, test)]
pub fn old_or_unavailable_runner_audit_does_not_change_existing_outcome() {
    for wire in [
        json!({"version":1,"status":"unrecorded"}),
        json!({"version":1,"status":"unavailable"}),
        json!({"version":2,"status":"recorded"}),
    ] {
        let mut r = row();
        r["runner_result"] = evidence(&r, wire.clone());
        let outcome = r["runner_result"]["result"].clone();
        project_row(&mut r);
        assert_eq!(r["runner_result"]["result"], outcome);
        if wire == json!({"version":1,"status":"unrecorded"}) {
            assert_eq!(r["admission_audit"], missing("runner_unrecorded"));
        } else {
            assert_eq!(r["admission_audit"], unavailable());
        }
    }
}
