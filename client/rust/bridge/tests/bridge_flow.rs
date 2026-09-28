//! End-to-end flow through the bridge functions (the Dart-facing surface)
//! against a real app-core with an in-memory secure store: JSON wire
//! format, upsert ids, inline host auth, group re-parenting, lock/unlock,
//! backup export/inspect/import, the UI store and shutdown.
//!
//! One test per binary: the bridge holds one global core.

use cc_bridge::api::app::{self, CoreConfig, KdfChoice, SecureStoreChoice};
use cc_bridge::api::inventory::{self, HostAuthInput, HostAuthKind};
use cc_bridge::api::{backup, credentials, profiles};
use serde_json::{json, Value};

fn parse(s: &str) -> Value {
    serde_json::from_str(s).expect("valid JSON")
}

#[tokio::test(flavor = "multi_thread")]
async fn bridge_flow_local_profile_hosts_lock_backup() {
    let dir = tempfile::tempdir().unwrap();
    let passphrase = format!("bridge-{}-Violet-Anchor-Muffin", std::process::id());

    let info = app::core_init(CoreConfig {
        data_dir: Some(dir.path().join("data").to_string_lossy().into_owned()),
        secure_store: SecureStoreChoice::Memory,
        keychain_service: None,
        kdf: KdfChoice::Floor,
        client_version: "0.0.0-test".into(),
        device_name: Some("bridge test".into()),
        background_sync: false,
        auto_start_tunnels: false,
        log_directives: None,
    })
    .await
    .unwrap();
    assert!(!info.already_initialized);
    assert!(app::core_is_initialized());
    // Idempotent (Flutter hot restart).
    let again = app::core_init(CoreConfig {
        data_dir: None,
        secure_store: SecureStoreChoice::Memory,
        keychain_service: None,
        kdf: KdfChoice::Floor,
        client_version: String::new(),
        device_name: None,
        background_sync: false,
        auto_start_tunnels: false,
        log_directives: None,
    })
    .await
    .unwrap();
    assert!(again.already_initialized);
    assert_eq!(again.data_dir, info.data_dir);

    // Profile + vault; the Recovery Kit is returned once.
    let created = parse(
        &profiles::profiles_create_local("Bridge".into(), passphrase.clone().into_bytes())
            .await
            .unwrap(),
    );
    assert_eq!(
        created["recovery_kit"]["words"].as_array().unwrap().len(),
        24
    );
    let profile_id = created["profile"]["id"].as_str().unwrap().to_owned();
    assert_eq!(created["profile"]["vault_state"], "unlocked");
    assert!(profiles::vault_has_pending_recovery_kit().await.unwrap());
    profiles::vault_acknowledge_recovery_kit().await.unwrap();
    assert!(!profiles::vault_has_pending_recovery_kit().await.unwrap());

    // Upsert: a draft id minted by Dart is replaced by the core's id.
    let draft_id = "0190e3c5-0000-7000-8000-000000000001";
    let mut host = json!({
        "id": draft_id, "name": "db", "address": "10.0.0.5", "port": 2222,
        "username": "ops", "credential_id": null, "group_id": null, "jump_chain": [],
        "jump_profile_id": null, "proxy_id": null, "proxy_command": null,
        "host_key_policy": "ask", "backend": "native", "keepalive_secs": null,
        "agent_forwarding": false, "tags": ["prod"], "notes": "", "metadata": {},
        "created_at_ms": 0, "updated_at_ms": 0, "auth_mode": "inherit"
    });
    let saved = parse(
        &inventory::hosts_save_with_auth(
            host.to_string(),
            HostAuthInput {
                kind: HostAuthKind::InlinePassword,
                credential_id: None,
                password: Some(b"hunter2".to_vec()),
                agent_path: None,
            },
        )
        .await
        .unwrap(),
    );
    let host_id = saved["id"].as_str().unwrap().to_owned();
    assert_ne!(host_id, draft_id);
    assert_eq!(saved["auth_mode"], "inline_password");
    let cred_id = saved["credential_id"].as_str().unwrap().to_owned();
    let creds = parse(&credentials::credentials_list().await.unwrap());
    let inline = creds
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == cred_id.as_str())
        .unwrap();
    assert_eq!(inline["has_secret"], true);
    assert_eq!(inline["owner_host_id"], host_id.as_str());
    assert!(!creds.to_string().contains("hunter2"), "no secret in DTOs");

    // Update keeps the id; unknown fields round-trip.
    host["id"] = json!(host_id);
    host["name"] = json!("db-renamed");
    host["credential_id"] = json!(cred_id);
    let updated = parse(&inventory::hosts_save(host.to_string()).await.unwrap());
    assert_eq!(updated["id"], host_id.as_str());
    assert_eq!(updated["port"], 2222);

    // Deleting a group moves its hosts / children to its parent.
    let group_json = |name: &str, parent: Option<&str>| {
        json!({"id": "", "name": name, "parent_id": parent, "inherited_username": null,
               "inherited_port": null, "inherited_credential_id": null,
               "inherited_jump_profile_id": null, "tags": [], "created_at_ms": 0, "updated_at_ms": 0})
        .to_string()
    };
    let parent = parse(
        &inventory::groups_save(group_json("parent", None))
            .await
            .unwrap(),
    );
    let parent_id = parent["id"].as_str().unwrap().to_owned();
    let child = parse(
        &inventory::groups_save(group_json("child", Some(&parent_id)))
            .await
            .unwrap(),
    );
    let child_id = child["id"].as_str().unwrap().to_owned();
    let mut in_child = updated.clone();
    in_child["group_id"] = json!(child_id);
    inventory::hosts_save(in_child.to_string()).await.unwrap();
    inventory::groups_delete(child_id.clone()).await.unwrap();
    let hosts = parse(&inventory::hosts_list().await.unwrap());
    assert_eq!(hosts[0]["group_id"], parent_id.as_str());

    // Lock: nothing readable; wrong passphrase refused with its code.
    profiles::vault_lock().await.unwrap();
    assert_eq!(
        inventory::hosts_list().await.unwrap_err().code,
        "vault_locked"
    );
    let e = profiles::vault_unlock_with_passphrase(b"wrong passphrase".to_vec())
        .await
        .unwrap_err();
    assert_eq!(e.code, "wrong_passphrase");
    profiles::vault_unlock_with_passphrase(passphrase.clone().into_bytes())
        .await
        .unwrap();
    assert_eq!(
        parse(&inventory::hosts_list().await.unwrap())[0]["name"],
        "db-renamed"
    );

    // Backup → header → restore into a new local profile.
    let path = dir.path().join("b.ccbackup").to_string_lossy().into_owned();
    let summary = parse(&backup::backup_export(path.clone()).await.unwrap());
    let header = backup::backup_inspect(path.clone()).await.unwrap();
    assert_eq!(header.vault_id, summary["vault_id"].as_str().unwrap());
    assert!(header.objects >= 3);
    let restored = parse(
        &backup::backup_import(
            path,
            "Restored".into(),
            backup::BackupUnlockKind::Passphrase,
            passphrase.clone().into_bytes(),
            None,
        )
        .await
        .unwrap(),
    );
    assert_ne!(restored["profile"]["id"], profile_id.as_str());
    assert_eq!(restored["profile"]["vault_state"], "unlocked");
    assert_eq!(
        parse(&inventory::hosts_list().await.unwrap())[0]["name"],
        "db-renamed"
    );
    let all = parse(&profiles::profiles_list().await.unwrap());
    assert_eq!(all.as_array().unwrap().len(), 2);

    // Device-local UI store.
    app::ui_store_set(
        "local_settings".into(),
        Some(r#"{"app_locale":"ru"}"#.into()),
    )
    .unwrap();
    assert_eq!(
        app::ui_store_get("local_settings".into())
            .unwrap()
            .as_deref(),
        Some(r#"{"app_locale":"ru"}"#)
    );
    app::ui_store_set("local_settings".into(), None).unwrap();
    assert!(app::ui_store_get("local_settings".into())
        .unwrap()
        .is_none());

    // Switch back: the other profile opens locked.
    let reopened = parse(&profiles::profiles_open(profile_id.clone()).await.unwrap());
    assert_eq!(reopened["vault_state"], "locked");
    profiles::profiles_remove(profile_id).await.unwrap();

    app::core_shutdown().await.unwrap();
    assert!(!app::core_is_initialized());
    assert_eq!(
        profiles::profiles_list().await.unwrap_err().code,
        "not_initialized"
    );
}
