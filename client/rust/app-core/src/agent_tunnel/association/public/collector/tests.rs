//! DRAFT/UNRUN public intent boundaries, not a substituted Actor or unit witness.
use super::*;
include!("disk_tests.rs");
fn intent() -> Value {
    json!({"version":1,"profileId":Uuid::new_v4().to_string(),"notificationScopeId":"original-notice",
        "associationId":Uuid::new_v4().to_string(),"durationSeconds":300})
}
#[test]
fn collection_intent_cannot_carry_authority_catalog_process_or_replacement_scope() {
    let original = intent();
    for name in ["approved", "grant", "certificate", "pin", "manifest", "collectorId", "receiver",
        "targets", "executable", "env", "path", "clock", "freshness", "renew", "lease"] {
        let mut value = original.clone(); value[name] = json!(true);
        assert!(serde_json::from_value::<Request>(value).is_err(), "{name}");
    }
    let duplicate = r#"{"version":1,"profileId":"p","notificationScopeId":"n","associationId":"00000000-0000-4000-8000-000000000001","durationSeconds":30,"durationSeconds":300}"#;
    assert!(serde_json::from_str::<Request>(duplicate).is_err());
}
#[test]
fn original_collection_intent_is_bounded_and_never_accepts_implicit_or_noncanonical_identity() {
    for duration in [30, 300] {
        let mut value = intent(); value["durationSeconds"] = json!(duration);
        serde_json::from_value::<Request>(value).unwrap().validate().unwrap();
    }
    for duration in [0, 29, 301, 7200] {
        let mut value = intent(); value["durationSeconds"] = json!(duration);
        assert!(serde_json::from_value::<Request>(value).unwrap().validate().is_err());
    }
    for identity in ["00000000-0000-0000-0000-000000000000", "{00000000-0000-4000-8000-000000000001}", "inherited"] {
        let mut value = intent(); value["associationId"] = json!(identity);
        assert!(serde_json::from_value::<Request>(value).unwrap().validate().is_err());
    }
    let mut value = intent(); value["durationSeconds"] = json!("300");
    assert!(serde_json::from_value::<Request>(value).is_err());
    // Actual original profile/notice binding is performed by capture_expected;
    // these parser tests cannot establish that bound owner or authorize a unit.
}
