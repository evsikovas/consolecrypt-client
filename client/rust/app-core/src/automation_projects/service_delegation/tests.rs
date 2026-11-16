use super::*;
fn terms() -> NativeServiceTerms {
    let keys = cc_crypto_core::DeviceSecretKeys::generate()
        .unwrap()
        .public_keys();
    let public = |raw: &[u8]| raw.iter().map(|b| format!("{b:02x}")).collect::<String>();
    NativeServiceTerms {
        service_id: Uuid::new_v4(),
        signer: ServicePolicyKey {
            kind: "ed25519".into(),
            public_key: public(&keys.signing),
            key_epoch: 1,
            certificate_epoch: 1,
        },
        recipient: ServicePolicyKey {
            kind: "x25519".into(),
            public_key: public(&keys.encryption),
            key_epoch: 1,
            certificate_epoch: 1,
        },
        owner_epoch: 1,
        policy_revision: 1,
        revocation_epoch: 1,
        signer_not_after: now().unwrap() + 86400,
        recipient_not_after: now().unwrap() + 86400,
        schedule_id: Uuid::new_v4(),
        schedule_revision: 1,
        definition_digest: "a".repeat(64),
        validity_seconds: 3600,
        access_mode: ServiceAccessMode::FreshOwnerHead,
        quotas: ServicePolicyQuotas {
            max_jobs: 16,
            max_per_utc_day: 4,
            max_concurrent: 1,
            max_job_seconds: 120,
            max_forks: 1,
        },
    }
}
#[tokio::test]
async fn absent_real_prepared_review_cannot_sign_or_touch_source_or_grant_service() {
    let directory = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        directory.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile(
        "Owned service delegation negative fixture".into(),
        "synthetic policy owner42".into(),
    )
    .await
    .unwrap();
    let owner = app.session().await.unwrap();
    let profile = owner.profile_id.to_string();
    let before = owner.automation.io_slots.available_permits();
    assert!(
        app.service_delegation_preview(profile, Uuid::new_v4().to_string(), terms())
            .await
            .is_err()
    );
    assert_eq!(owner.automation.io_slots.available_permits(), before);
    assert!(owner.automation.reviews.lock().unwrap().is_empty());
    app.shutdown().await.unwrap();
}
#[tokio::test]
async fn original_profile_and_canonical_review_id_are_required_without_recapture() {
    let directory = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        directory.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile(
        "Original service profile".into(),
        "synthetic policy owner42".into(),
    )
    .await
    .unwrap();
    let owner = app.session().await.unwrap();
    let profile = owner.profile_id.to_string();
    assert!(
        app.service_delegation_preview(
            Uuid::new_v4().to_string(),
            Uuid::new_v4().to_string(),
            terms()
        )
        .await
        .is_err()
    );
    assert!(
        app.service_delegation_preview(
            profile,
            "00000000-0000-0000-0000-000000000000".into(),
            terms()
        )
        .await
        .is_err()
    );
    assert_eq!(owner.automation.io_slots.available_permits(), 4);
    app.shutdown().await.unwrap();
}

async fn metadata_fixture() -> (
    tempfile::TempDir,
    AppCore,
    ObjectId,
    BTreeMap<ObjectId, i64>,
    BTreeMap<ObjectId, cc_models::ObjectKind>,
    BTreeMap<ObjectId, i64>,
) {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let directory = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        directory.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile(
        "Owned route metadata fixture".into(),
        "synthetic route owner42".into(),
    )
    .await
    .unwrap();
    let (_, u) = app.unlocked().await.unwrap();
    let host = cc_models::host::Host::new("Exact native target", "192.0.2.18");
    let id = host.id;
    u.writer
        .put(cc_models::VaultObject::Host(host))
        .await
        .unwrap();
    let key = cc_crypto_core::DeviceSecretKeys::generate()
        .unwrap()
        .public_keys()
        .signing;
    let mut blob = Vec::new();
    blob.extend_from_slice(&11u32.to_be_bytes());
    blob.extend_from_slice(b"ssh-ed25519");
    blob.extend_from_slice(&32u32.to_be_bytes());
    blob.extend_from_slice(&key);
    app.add_known_host(
        "192.0.2.18".into(),
        22,
        format!("ssh-ed25519 {}", STANDARD.encode(blob)),
    )
    .await
    .unwrap();
    let deps = BTreeMap::from([(id, u.working().revision_of(id).unwrap())]);
    let kinds = BTreeMap::from([(id, cc_models::ObjectKind::Host)]);
    let trust = u.working().service_key_trust_revisions().unwrap();
    drop(u);
    (directory, app, id, deps, kinds, trust)
}

#[tokio::test]
async fn actual_original_metadata_projection_keeps_typed_revisions_and_no_material() {
    let (_dir, app, id, deps, kinds, trust) = metadata_fixture().await;
    let (_, u) = app.unlocked().await.unwrap();
    let projected = original_route_inputs(&u, &deps, &kinds, &trust).unwrap();
    assert_eq!(projected.dependencies.len(), 1);
    assert_eq!(projected.dependencies[0].id, id.to_string());
    assert_eq!(projected.dependencies[0].kind, cc_models::ObjectKind::Host);
    assert_eq!(projected.key_trust.len(), 1);
    assert_eq!(
        projected.key_trust[0].kind,
        cc_models::ObjectKind::KnownHost
    );
    let value = serde_json::to_value(projected).unwrap();
    assert_eq!(value["key_trust"][0].as_object().unwrap().len(), 3);
    drop(u);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn actual_new_known_host_after_capture_does_not_fit_old_trust_scope() {
    let (_dir, app, _id, deps, kinds, trust) = metadata_fixture().await;
    let (_, u) = app.unlocked().await.unwrap();
    let mut second = u
        .working()
        .known_host(*trust.keys().next().unwrap())
        .unwrap();
    second.id = ObjectId::new();
    second.host_pattern = "192.0.2.19".into();
    u.writer
        .put(cc_models::VaultObject::KnownHost(second))
        .await
        .unwrap();
    assert_eq!(u.working().service_key_trust_revisions().unwrap().len(), 2);
    assert!(original_route_inputs(&u, &deps, &kinds, &trust).is_err());
    drop(u);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn original_proxy_dependency_revision_is_checked_without_resolving_routes_again() {
    let (_dir, app, _id, mut deps, mut kinds, trust) = metadata_fixture().await;
    let (_, u) = app.unlocked().await.unwrap();
    let stamp = chrono::Utc::now();
    let mut proxy = cc_models::host::Proxy {
        id: ObjectId::new(),
        name: "Native original proxy".into(),
        kind: cc_models::host::ProxyKind::Socks5,
        address: "192.0.2.20".into(),
        port: 1080,
        username: None,
        password_secret_id: None,
        created_at: stamp,
        updated_at: stamp,
    };
    u.writer
        .put(cc_models::VaultObject::Proxy(proxy.clone()))
        .await
        .unwrap();
    deps.insert(proxy.id, u.working().revision_of(proxy.id).unwrap());
    kinds.insert(proxy.id, cc_models::ObjectKind::Proxy);
    assert!(original_route_inputs(&u, &deps, &kinds, &trust).is_ok());
    proxy.address = "192.0.2.21".into();
    u.writer
        .put(cc_models::VaultObject::Proxy(proxy))
        .await
        .unwrap();
    assert!(original_route_inputs(&u, &deps, &kinds, &trust).is_err());
    drop(u);
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn same_revision_key_trust_retyped_as_other_object_cannot_pass_original_scope() {
    use crate::working_set::Readback;
    use cc_models::{
        ObjectPayload, VaultObject,
        credential::{Credential, CredentialKind},
    };
    let (_dir, app, _id, deps, kinds, trust) = metadata_fixture().await;
    let (_, u) = app.unlocked().await.unwrap();
    let id = *trust.keys().next().unwrap();
    let revision = trust[&id];
    let mut other = Credential::new("Different actual metadata type", CredentialKind::Password);
    other.id = id;
    u.working().apply(
        id,
        Readback::Live(Box::new(cc_sync_core::DecryptedObject {
            object_id: id,
            revision,
            local_state: cc_storage_core::LocalState::LocalOnly,
            conflict_origin: None,
            payload: ObjectPayload::new(VaultObject::Credential(other)),
        })),
    );
    assert_eq!(u.working().revision_of(id), Some(revision));
    assert!(original_route_inputs(&u, &deps, &kinds, &trust).is_err());
    drop(u);
    app.shutdown().await.unwrap();
}

#[cfg(test)]
mod live_remote;

#[cfg(test)]
mod live_execution;
