mod common;

use cc_app_core::sharing_projection::{project_snippet, SharedProjection};
use cc_app_core::*;
use cc_models::snippet::{RiskLevel, Snippet, SnippetSource, SnippetType, SnippetVariable};
use cc_protocol::sharing::SharedItemKind;
use cc_protocol::{DeviceId, ObjectId};
use common::*;

#[tokio::test]
async fn real_host_projection_resolves_inheritance_without_private_references() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let passphrase = format!("vault-{}-{}", ObjectId::new(), ObjectId::new());
    app.create_local_profile("Personal".into(), passphrase)
        .await
        .unwrap();
    let secret_marker = ObjectId::new().to_string();
    let credential = app
        .add_password_credential(
            "private login".into(),
            Some("operator".into()),
            secret_marker.clone(),
        )
        .await
        .unwrap();
    let group = app
        .save_group(GroupDto {
            id: String::new(),
            name: "private group".into(),
            parent_id: None,
            inherited_username: Some("group-user".into()),
            inherited_port: Some(2222),
            inherited_credential_id: Some(credential.id.clone()),
            inherited_jump_profile_id: None,
            tags: vec![],
            created_at_ms: 0,
            updated_at_ms: 0,
        })
        .await
        .unwrap();
    let mut host = HostDto::new("shared endpoint", "example.test");
    host.group_id = Some(group.id.clone());
    host.notes = secret_marker.clone();
    host.metadata
        .insert("private_value".into(), secret_marker.clone());
    host.agent_forwarding = true;
    host.host_key_policy = cc_models::host::HostKeyPolicy::AcceptNew;
    let host = app.save_host(host).await.unwrap();
    let preview = app
        .sharing_preview_host(host.id.clone(), false)
        .await
        .unwrap();
    let encoded = preview.encode().unwrap();
    let json: serde_json::Value = serde_json::from_slice(encoded.as_slice()).unwrap();
    assert_eq!(json["kind"], "host");
    assert_eq!(json["data"]["port"], 2222);
    assert_eq!(json["data"]["username"], "operator");
    let text = String::from_utf8(encoded.as_slice().to_vec()).unwrap();
    for excluded in [
        &secret_marker,
        &host.id,
        &credential.id,
        &group.id,
        "metadata",
        "agent_forwarding",
        "host_key_policy",
        "credential_id",
    ] {
        assert!(!text.contains(excluded), "private field leaked");
    }
    assert!(!format!("{preview:?}{encoded:?}").contains("shared endpoint"));
    assert_eq!(
        SharedProjection::decode(SharedItemKind::Host, encoded.as_slice()).unwrap(),
        preview
    );
    assert!(SharedProjection::decode(SharedItemKind::Snippet, encoded.as_slice()).is_err());
    let explicit_notes = app
        .sharing_preview_host(host.id, true)
        .await
        .unwrap()
        .encode()
        .unwrap();
    assert!(std::str::from_utf8(explicit_notes.as_slice())
        .unwrap()
        .contains(&secret_marker));
    app.lock().await.unwrap();
    assert!(matches!(
        app.sharing_preview_host(ObjectId::new().to_string(), false)
            .await,
        Err(AppError::VaultLocked)
    ));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn jump_dependencies_cannot_be_silently_removed_from_a_shared_host() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    app.create_local_profile(
        "Personal".into(),
        format!("vault-{}-{}", ObjectId::new(), ObjectId::new()),
    )
    .await
    .unwrap();
    let mut jump = HostDto::new("bastion", "bastion.test");
    jump.username = Some("jump-user".into());
    let jump = app.save_host(jump).await.unwrap();
    let mut host = HostDto::new("db", "db.internal");
    host.username = Some("db-user".into());
    host.jump_chain.push(jump.id);
    let host = app.save_host(host).await.unwrap();
    assert!(
        matches!(app.sharing_preview_host(host.id, false).await, Err(AppError::InvalidInput { field, .. }) if field == "shared_dependencies")
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn missing_inherited_jump_profile_does_not_become_a_direct_shared_connection() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    app.create_local_profile(
        "Personal".into(),
        format!("vault-{}-{}", ObjectId::new(), ObjectId::new()),
    )
    .await
    .unwrap();
    let mut host = HostDto::new("internal", "internal.test");
    host.username = Some("operator".into());
    let host = app.save_host(host).await.unwrap();
    let mut resolved = app.plan_preview_host(host.id.clone()).await.unwrap();
    // An incomplete restored/imported group chain can produce an empty route
    // plus this diagnostic. Empty route alone must not authorize direct sharing.
    resolved.diagnostics.push(PlanDiagnosticDto {
        code: "missing_jump_profile".into(),
        severity: PlanSeverityDto::Error,
        args: Default::default(),
        message: "inherited profile is unavailable".into(),
    });
    let mut model = cc_models::host::Host::new(&host.name, &host.address);
    model.id = host.id.parse().unwrap();
    assert!(
        matches!(cc_app_core::sharing_projection::project_host(&model, &resolved, false), Err(AppError::InvalidInput { field, .. }) if field == "shared_dependencies")
    );
    app.shutdown().await.unwrap();
}

#[test]
fn snippet_projection_excludes_personal_defaults_identity_and_history() {
    let now = chrono::Utc::now();
    let secret_marker = ObjectId::new().to_string();
    let snippet = Snippet {
        id: ObjectId::new(),
        name: "disk usage".into(),
        description: "Inspect files".into(),
        package_name: Some("private package".into()),
        catalog_id: Some("private catalog".into()),
        snippet_type: SnippetType::Shell,
        shell: Some("bash".into()),
        template: "du -sh {{path}}".into(),
        variables: vec![SnippetVariable {
            name: "path".into(),
            description: "Directory".into(),
            default: Some(secret_marker.clone()),
            required: true,
        }],
        tags: vec!["linux".into()],
        risk_level: RiskLevel::ReadOnly,
        source: SnippetSource::User,
        created_by: Some(DeviceId::new()),
        created_at: now,
        updated_at: now,
        last_used_at: Some(now),
        usage_count: 12,
    };
    let preview = project_snippet(&snippet).unwrap();
    let encoded = preview.encode().unwrap();
    let text = std::str::from_utf8(encoded.as_slice()).unwrap();
    for excluded in [
        &secret_marker,
        &snippet.id.to_string(),
        &snippet.created_by.unwrap().to_string(),
        "private package",
        "private catalog",
        "last_used_at",
        "usage_count",
        "default",
    ] {
        assert!(!text.contains(excluded), "private field leaked");
    }
    assert_eq!(
        SharedProjection::decode(SharedItemKind::Snippet, encoded.as_slice()).unwrap(),
        preview
    );
    assert!(!format!("{preview:?}{encoded:?}").contains("du -sh"));
}

#[test]
fn received_projection_rejects_hidden_credential_fields_and_invalid_content() {
    let base = serde_json::json!({ "kind": "host", "data": {
        "name": "host", "address": "example.test", "port": 22, "username": null,
        "keepalive_secs": null, "tags": [], "notes": null
    }});
    assert!(
        SharedProjection::decode(SharedItemKind::Host, &serde_json::to_vec(&base).unwrap()).is_ok()
    );
    let mut extra = base.clone();
    extra["data"]["credential_id"] = serde_json::json!(ObjectId::new());
    assert!(
        SharedProjection::decode(SharedItemKind::Host, &serde_json::to_vec(&extra).unwrap())
            .is_err()
    );
    let mut zero_port = base;
    zero_port["data"]["port"] = serde_json::json!(0);
    assert!(SharedProjection::decode(
        SharedItemKind::Host,
        &serde_json::to_vec(&zero_port).unwrap()
    )
    .is_err());
    assert!(SharedProjection::decode(SharedItemKind::Secret, b"{}").is_err());
}

#[test]
fn collection_projection_contains_only_explicit_shared_references() {
    use cc_app_core::sharing_projection::{project_group, SharedChildReference};
    use cc_models::group::Group;
    use cc_protocol::ShareId;
    let mut group = Group::new("Team endpoints");
    let private_id = ObjectId::new();
    group.inherited_credential_id = Some(private_id);
    group.parent_id = Some(ObjectId::new());
    group.inherited_username = Some("private-default".into());
    let child = SharedChildReference {
        share_id: ShareId::new(),
        item_id: ObjectId::new(),
        kind: SharedItemKind::Host,
    };
    let projection = project_group(&group, vec![child.clone()]).unwrap();
    let encoded = projection.encode().unwrap();
    let text = std::str::from_utf8(encoded.as_slice()).unwrap();
    assert!(!text.contains(&private_id.to_string()));
    assert!(!text.contains("inherited"));
    assert!(!text.contains("private-default"));
    assert!(SharedProjection::decode(SharedItemKind::Group, encoded.as_slice()).is_ok());
    assert!(project_group(&group, vec![child.clone(), child]).is_err());
    assert!(project_group(
        &group,
        vec![SharedChildReference {
            share_id: ShareId::NIL,
            item_id: ObjectId::new(),
            kind: SharedItemKind::Host
        }]
    )
    .is_err());
}

#[tokio::test]
async fn explicit_secret_projection_preserves_raw_protection_and_excludes_personal_ids() {
    use cc_app_core::sharing_projection::project_secret;
    use cc_models::secret::{Secret, SecretKind, SecretValue};
    let marker = format!("runtime-secret-{}", ObjectId::new());
    let secret = Secret::new(SecretKind::SshPrivateKey, SecretValue::new(&marker));
    let projection = project_secret("Explicit key", &secret).unwrap();
    assert!(!format!("{projection:?}").contains(&marker));
    let bytes = projection.encode().unwrap();
    let value: serde_json::Value = serde_json::from_slice(bytes.as_slice()).unwrap();
    assert_eq!(value["data"]["value"].as_str(), Some(marker.as_str()));
    assert_eq!(value["data"]["secret_kind"], "ssh_private_key");
    assert!(value["data"].get("id").is_none());
    assert!(value["data"].get("passphrase").is_none());
    assert!(SharedProjection::decode(SharedItemKind::Secret, bytes.as_slice()).is_ok());
    let dir = tempfile::tempdir().unwrap();
    let core = app(dir.path());
    core.create_local_profile("local".into(), format!("vault-{}", ObjectId::new()))
        .await
        .unwrap();
    let credential = core
        .add_password_credential("Selected password".into(), None, marker.clone())
        .await
        .unwrap();
    let prepared = core
        .sharing_prepare_credential(credential.id.clone(), false)
        .await
        .unwrap();
    if let SharedProjection::Secret(ref selected) = prepared {
        assert_eq!(selected.value.expose_secret(), marker)
    } else {
        panic!("expected secret projection")
    }
    assert!(core
        .sharing_prepare_credential(credential.id.clone(), true)
        .await
        .is_err());
    core.lock().await.unwrap();
    assert!(core
        .sharing_prepare_credential(credential.id, false)
        .await
        .is_err());
}
