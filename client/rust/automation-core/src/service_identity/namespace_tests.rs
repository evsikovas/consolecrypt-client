use super::*;
#[test]
fn public_namespace_is_exact_domain_separated_metadata_and_changes_with_identity_or_keys() {
    let id = Uuid::new_v4();
    let keys = DeviceSecretKeys::generate().unwrap().public_keys();
    let n = ServiceLocalNamespace::derive(id, &keys).unwrap();
    assert!(n.namespace.starts_with("cc-service-local-v1-"));
    assert_eq!(n.namespace.len(), 84);
    assert_ne!(n.namespace, id.to_string());
    n.validate_public(&keys).unwrap();
    let mut changed = n.clone();
    changed.key_epoch = 2;
    assert!(changed.validate_public(&keys).is_err());
    changed = n.clone();
    changed.identity_id = Uuid::new_v4();
    assert!(changed.validate_public(&keys).is_err());
    assert!(n
        .validate_public(&DeviceSecretKeys::generate().unwrap().public_keys())
        .is_err());
    assert!(ServiceLocalNamespace::derive(Uuid::nil(), &keys).is_err());
    let mut wire = serde_json::to_value(&n).unwrap();
    wire["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ServiceLocalNamespace>(wire).is_err());
}
