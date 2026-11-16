//! Staged additive wire tests. Syntactic UserId types do not authenticate an account.
//! No keys, issuer initialization, storage, network or authority are used here.
use super::*;
use serde_json::{Value, json};

fn vector() -> Value {
    serde_json::from_str(include_str!("personal_account_vectors.json")).unwrap()
}

#[test]
fn personal_account_id_uses_typed_user_id_without_native_profile_cast() {
    let user: cc_protocol::UserId = "11111111-1111-4111-8111-111111111111".parse().unwrap();
    let account = AgentReaderPersonalAccountId::from_user_id(user).unwrap();
    assert_eq!(account.as_uuid().to_string(), user.to_string());
    assert!(AgentReaderPersonalAccountId::from_user_id(cc_protocol::UserId::NIL).is_err());
    let tenant = AgentReaderTenantV1::PersonalAccount { id: account };
    assert_eq!(
        serde_json::to_value(&tenant).unwrap(),
        json!({"kind":"personalAccount","id":user.to_string()})
    );
    let native_profile =
        AgentReaderNativeProfileId::new("77777777-7777-4777-8777-777777777777").unwrap();
    assert_ne!(account.as_uuid(), native_profile.as_uuid());
    // Public UUID parsing remains syntax only. The authority layer must use
    // the opaque authenticated subject, never this constructor as permission.
    assert!(AgentReaderPersonalAccountId::new(&native_profile.as_uuid().to_string()).is_ok());
}

#[test]
fn personal_account_context_and_association_match_unsigned_public_bytes() {
    let v = vector();
    let context: AgentReaderContextV1 =
        serde_json::from_value(v["context"]["body"].clone()).unwrap();
    assert_eq!(
        wire::canonical(&context).unwrap(),
        v["context"]["canonical_utf8"].as_str().unwrap().as_bytes()
    );
    let association: AgentReaderAssociationV1 =
        serde_json::from_value(v["association"]["body"].clone()).unwrap();
    assert_eq!(association.context, context);
    assert_ne!(
        association.native_profile_id.as_uuid(),
        match &context.tenant {
            AgentReaderTenantV1::PersonalAccount { id } => id.as_uuid(),
            _ => panic!("personal account vector required"),
        }
    );
    assert_eq!(
        association.canonical_bytes().unwrap(),
        v["association"]["canonical_utf8"]
            .as_str()
            .unwrap()
            .as_bytes()
    );
    assert_eq!(
        hex::encode(association.signing_bytes().unwrap()),
        v["association"]["signing_bytes_hex"].as_str().unwrap()
    );
    assert_eq!(
        hex::encode(association.body_digest().unwrap().as_bytes()),
        v["association"]["body_digest_hex"].as_str().unwrap()
    );
}

#[test]
fn personal_account_tenant_is_strict_and_never_relabels_local_or_team() {
    let account: AgentReaderTenantV1 = serde_json::from_value(
        json!({"kind":"personalAccount","id":"11111111-1111-4111-8111-111111111111"}),
    )
    .unwrap();
    let local: AgentReaderTenantV1 = serde_json::from_value(
        json!({"kind":"localProfile","id":"11111111-1111-4111-8111-111111111111"}),
    )
    .unwrap();
    assert_ne!(account, local);
    for bad in [
        json!({"kind":"team","id":"11111111-1111-4111-8111-111111111111"}),
        json!({"kind":"personalAccount","id":"00000000-0000-0000-0000-000000000000"}),
        json!({"kind":"personalAccount","id":"AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA"}),
        json!({"kind":"personalAccount","id":"11111111-1111-4111-8111-111111111111","profileId":"77777777-7777-4777-8777-777777777777"}),
        json!({"kind":"personalAccount","id":1}),
    ] {
        assert!(serde_json::from_value::<AgentReaderTenantV1>(bad).is_err());
    }
}

#[test]
fn additive_personal_variant_keeps_existing_local_association_bytes_exact() {
    let old: Value = serde_json::from_str(include_str!("vectors.json")).unwrap();
    let association: AgentReaderAssociationV1 =
        serde_json::from_value(old["statements"][0]["body"].clone()).unwrap();
    assert!(matches!(
        &association.context.tenant,
        AgentReaderTenantV1::LocalProfile { .. }
    ));
    assert_eq!(
        hex::encode(association.signing_bytes().unwrap()),
        old["statements"][0]["signing_bytes_hex"].as_str().unwrap()
    );
    assert_eq!(
        hex::encode(association.body_digest().unwrap().as_bytes()),
        old["statements"][0]["body_digest_hex"].as_str().unwrap()
    );
}
