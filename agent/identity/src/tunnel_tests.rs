//! DRAFT data/wire tests; no grant is inferred from a parsed policy.
use super::*;
use serde_json::{json, Value};

fn policy() -> TunnelRoutePolicy {
    serde_json::from_str(include_str!("tunnel_policy_vector.json")).unwrap()
}

#[test]
fn tunnel_policy_shared_vector_and_bound_operation_match() {
    let policy = policy();
    assert_eq!(
        serde_json::to_value(policy.digest().unwrap()).unwrap(),
        json!("4b3315b5dd76ab0f8ecfd687c920918bea3fc8e62f50610a2499cfbfc369b07d")
    );
    assert_eq!(
        serde_json::to_value(policy.operation().unwrap()).unwrap(),
        json!({
        "kind":"tunnelOpenBound","route":"44444444-4444-4444-8444-444444444444",
        "policyHash":"4b3315b5dd76ab0f8ecfd687c920918bea3fc8e62f50610a2499cfbfc369b07d"})
    );
    assert_eq!(policy.operation().unwrap().scope(), Some(Scope::TunnelOpen));
    assert_eq!(
        serde_json::to_value(Operation::TunnelOpen {
            route: policy.route_id
        })
        .unwrap(),
        json!({"kind":"tunnelOpen","route":"44444444-4444-4444-8444-444444444444"})
    );
}

#[test]
fn tunnel_policy_changes_both_tls_pins_destination_and_budget_commitment() {
    let original = serde_json::to_value(policy()).unwrap();
    for (field, value) in [
        ("clientTlsCertificateSha256", json!("44".repeat(32))),
        ("agentTlsCertificateSha256", json!("55".repeat(32))),
        (
            "clientDeviceId",
            json!("77777777-7777-4777-8777-777777777777"),
        ),
    ] {
        let mut changed = original.clone();
        changed[field] = value;
        let changed: TunnelRoutePolicy = serde_json::from_value(changed).unwrap();
        assert_ne!(changed.digest().unwrap(), policy().digest().unwrap());
    }
    let mut changed = policy();
    changed.destination.port += 1;
    assert_ne!(changed.digest().unwrap(), policy().digest().unwrap());
    changed = policy();
    changed.limits.max_bytes_total -= 1;
    assert_ne!(changed.digest().unwrap(), policy().digest().unwrap());
}

#[test]
fn tunnel_policy_refuses_external_dns_unknown_fields_and_wrong_numeric_types() {
    let original = serde_json::to_value(policy()).unwrap();
    for ip in [
        "localhost",
        "0.0.0.0",
        "127.0.0.2",
        "::ffff:127.0.0.1",
        "::1%lo",
        "8.8.8.8",
    ] {
        let mut changed = original.clone();
        changed["destination"]["ip"] = json!(ip);
        assert!(serde_json::from_value::<TunnelRoutePolicy>(changed)
            .unwrap()
            .validate()
            .is_err());
    }
    for value in [json!(true), json!(1.0), json!("1"), Value::Null, json!(-1)] {
        let mut changed = original.clone();
        changed["limits"]["maxChannels"] = value;
        assert!(serde_json::from_value::<TunnelRoutePolicy>(changed).is_err());
    }
    let mut changed = original.clone();
    changed["callback"] = json!(true);
    assert!(serde_json::from_value::<TunnelRoutePolicy>(changed).is_err());
    let mut changed = original;
    changed["clientSubject"]["kind"] = json!("team");
    assert!(serde_json::from_value::<TunnelRoutePolicy>(changed).is_err());
}

#[test]
fn tunnel_policy_quotas_and_noncanonical_subject_fail_closed() {
    for channels in [0, 5] {
        let mut value = policy();
        value.limits.max_channels = channels;
        assert!(value.validate().is_err());
    }
    let mut value = policy();
    value.limits.idle_timeout_seconds = 61;
    assert!(value.validate().is_err());
    let mut value = policy();
    value.client_device_id = "00000000-0000-0000-0000-000000000000".into();
    assert!(value.validate().is_err());
    let mut value = policy();
    value.client_tls_certificate_sha256 = Hash::from_bytes([0; 32]);
    assert!(value.validate().is_err());
}
