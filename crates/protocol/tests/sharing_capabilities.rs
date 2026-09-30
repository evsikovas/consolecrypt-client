use cc_protocol::sharing::SharingCapabilities;

#[test]
fn old_capabilities_fail_closed_for_independent_extensions() {
    let old = serde_json::json!({"enabled":true,
        "server_instance_id":"00000000-0000-0000-0000-000000000001",
        "format":1,"max_members":64,"max_ciphertext_bytes":1048592});
    let decoded: SharingCapabilities = serde_json::from_value(old).unwrap();
    assert!(!decoded.supports_groups);
    assert!(!decoded.supports_secrets);
    assert!(!decoded.supports_owner_online_enrollment_v1);
}

#[test]
fn extension_capabilities_remain_independent_and_typed() {
    let mut value = serde_json::json!({"enabled":true,
        "server_instance_id":"00000000-0000-0000-0000-000000000001",
        "format":1,"max_members":64,"max_ciphertext_bytes":1048592,
        "supports_groups":true,"supports_secrets":false,
        "supports_owner_online_enrollment_v1":false});
    let decoded: SharingCapabilities = serde_json::from_value(value.clone()).unwrap();
    assert!(decoded.supports_groups);
    assert!(!decoded.supports_secrets);
    assert!(!decoded.supports_owner_online_enrollment_v1);
    value["supports_secrets"] = serde_json::json!("true");
    assert!(serde_json::from_value::<SharingCapabilities>(value).is_err());
}
