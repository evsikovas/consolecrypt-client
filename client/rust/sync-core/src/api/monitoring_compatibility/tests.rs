use super::*;
use serde_json::{json, Value};
fn fixture() -> (VaultId, Url, Value) {
    let vault = VaultId::new();
    let origin = Url::parse("https://owned.example.test/prefix").unwrap();
    let data = json!({"version":1,"scope":"personalAccount","accountId":UserId::new(),"revision":0,
        "state":"off","minimumClientVersion":null,"offerMinimumClientVersion":"2.0.0",
        "rosterDigest":"a".repeat(64),"checkpoint":{"vaultId":vault,"vaultEpoch":Uuid::new_v4(),"vaultSequence":0}});
    (vault, origin, data)
}
fn raw(body: Vec<u8>) -> Raw {
    Raw {
        status: StatusCode::OK,
        headers: HeaderMap::new(),
        body: body.into(),
    }
}
fn parse(
    data: &Value,
    vault: VaultId,
    origin: &Url,
) -> Result<MonitoringAccountPolicy, MonitoringCompatibilityError> {
    parse_policy(&raw(serde_json::to_vec(data).unwrap()), vault, origin)
}
#[test]
fn policy_data_is_exactly_bound_and_never_a_deserialized_writer_grant() {
    let (vault, origin, data) = fixture();
    let p = parse(&data, vault, &origin).unwrap();
    assert_eq!(p.state(), MonitoringPolicyState::Off);
    assert_eq!(p.revision(), 0);
    assert_eq!(p.checkpoint().vault_id(), vault);
    assert_eq!(p.checkpoint().sequence(), 0);
    assert_eq!(p.minimum_client_version(), None);
    assert_eq!(p.offer_minimum_client_version(), Some("2.0.0"));
    assert!(parse(&data, VaultId::new(), &origin).is_err());
    // Epoch/sequence are data to be checked against the REAL applied cursor.
    // There is deliberately no ready/admission Boolean or writer constructor.
}
#[test]
fn unknown_missing_duplicate_and_future_wire_fields_are_refused() {
    let (vault, origin, data) = fixture();
    for (field, value) in [
        ("version", json!(2)),
        ("scope", json!("team")),
        ("approved", json!(true)),
        ("sourceUrl", json!("https://guest.invalid")),
    ] {
        let mut d = data.clone();
        d[field] = value;
        assert!(parse(&d, vault, &origin).is_err(), "{field}");
    }
    for key in [
        "minimumClientVersion",
        "offerMinimumClientVersion",
        "accountId",
        "checkpoint",
    ] {
        let mut d = data.clone();
        d.as_object_mut().unwrap().remove(key);
        assert!(parse(&d, vault, &origin).is_err(), "{key}");
    }
    let valid = serde_json::to_string(&data).unwrap();
    let duplicate = valid.replacen('{', "{\"version\":1,", 1);
    assert!(parse_policy(&raw(duplicate.into_bytes()), vault, &origin).is_err());
    let mut d = data.clone();
    d["checkpoint"]["approved"] = json!(true);
    assert!(parse(&d, vault, &origin).is_err());
}
#[test]
fn stable_floor_and_state_revision_rules_fail_closed() {
    let (vault, origin, data) = fixture();
    for bad in [
        json!("2.0.0-beta"),
        json!("2.0.0+build"),
        json!(" 2.0.0"),
        json!("x".repeat(65)),
        json!({}),
        json!(3),
    ] {
        let mut d = data.clone();
        d["offerMinimumClientVersion"] = bad;
        assert!(parse(&d, vault, &origin).is_err());
    }
    for state in ["enabled", "quarantined"] {
        let mut d = data.clone();
        d["state"] = json!(state);
        assert!(parse(&d, vault, &origin).is_err());
        d["revision"] = json!(1);
        d["minimumClientVersion"] = json!("2.0.0");
        assert!(parse(&d, vault, &origin).is_ok());
        d["revision"] = json!(u64::MAX);
        assert!(parse(&d, vault, &origin).is_err());
    }
    let mut d = data.clone();
    d["state"] = json!("future");
    assert!(parse(&d, vault, &origin).is_err());
    d = data.clone();
    d["minimumClientVersion"] = json!("2.0.0");
    assert!(parse(&d, vault, &origin).is_err());
}
#[test]
fn checkpoint_digest_identity_and_encoding_budgets_are_enforced() {
    let (vault, origin, data) = fixture();
    for digest in [
        "a".repeat(63),
        "A".repeat(64),
        "a".repeat(65),
        "\n".repeat(64),
    ] {
        let mut d = data.clone();
        d["rosterDigest"] = json!(digest);
        assert!(parse(&d, vault, &origin).is_err());
    }
    let mut d = data.clone();
    d["accountId"] = json!(UserId::NIL);
    assert!(parse(&d, vault, &origin).is_err());
    d = data.clone();
    d["checkpoint"]["vaultEpoch"] = json!(Uuid::nil());
    assert!(parse(&d, vault, &origin).is_err());
    d = data.clone();
    d["checkpoint"]["vaultSequence"] = json!(-1);
    assert!(parse(&d, vault, &origin).is_err());
    let mut bytes = serde_json::to_vec(&data).unwrap();
    bytes.extend(vec![b' '; MAX_POLICY_BYTES]);
    assert!(parse_policy(&raw(bytes), vault, &origin).is_err());
}
#[test]
fn refusal_never_retains_or_echoes_remote_prose() {
    let mut response=raw(serde_json::to_vec(&json!({"code":"upgrade_required","message":"untrusted-private-marker","details":{"reason":"monitoring_compatibility_quarantined"}})).unwrap());
    response.status = StatusCode::UPGRADE_REQUIRED;
    let error = refusal(&response);
    assert_eq!(error, MonitoringCompatibilityError::Quarantined);
    assert!(!format!("{error:?} {error}").contains("untrusted-private-marker"));
    response.status = StatusCode::CONFLICT;
    assert_eq!(
        refusal(&response),
        MonitoringCompatibilityError::ContextChanged
    );
    response.status = StatusCode::NOT_FOUND;
    assert_eq!(
        refusal(&response),
        MonitoringCompatibilityError::Unsupported
    );
}
