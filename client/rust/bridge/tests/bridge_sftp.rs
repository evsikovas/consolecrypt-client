//! The core-gaps surface through the bridge functions (JSON wire format,
//! `BridgeError` code / reason / details): SFTP browser and edit sessions
//! against sftp-core's in-process server, account / reveal errors, planner
//! preview, snippet search / render, backup schedule and the attested OS
//! authenticator.
//!
//! One test per binary: the bridge holds one global core.

use cc_app_core::platform::{FileOpener, LaunchOutput, LaunchSpec, Launcher, OpenError, OsFamily};
use cc_bridge::api::app::{self, CoreConfig, KdfChoice, OsAuthKindChoice, SecureStoreChoice};
use cc_bridge::api::sftp::{self, EditStopChoice};
use cc_bridge::api::{account, backup, credentials, inventory, profiles};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

fn parse(s: &str) -> Value {
    serde_json::from_str(s).expect("valid JSON")
}

#[derive(Debug, Default)]
struct Recorder(Mutex<Vec<LaunchSpec>>);

impl Launcher for Recorder {
    fn launch(&self, spec: &LaunchSpec) -> Result<LaunchOutput, OpenError> {
        self.0.lock().unwrap().push(spec.clone());
        Ok(LaunchOutput::default())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn bridge_sftp_edit_account_plan_snippets_backup() {
    let dir = tempfile::tempdir().unwrap();
    let remote = dir.path().join("remote");
    std::fs::create_dir_all(remote.join("srv")).unwrap();
    std::fs::write(remote.join("srv/app.conf"), b"workers=2\n").unwrap();
    let passphrase = format!("bridge-{}-Violet-Anchor-Muffin", std::process::id());

    app::core_init(CoreConfig {
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
    let core = cc_bridge::core_for_tests().unwrap();
    let launches = Arc::new(Recorder::default());
    core.set_file_opener(FileOpener::new(OsFamily::current(), launches.clone()));

    let created = parse(
        &profiles::profiles_create_local("Bridge".into(), passphrase.clone().into_bytes())
            .await
            .unwrap(),
    );
    assert_eq!(created["profile"]["recovery_kit_pending"], true);
    assert!(profiles::vault_has_pending_recovery_kit().await.unwrap());
    assert!(profiles::vault_recovery_kit_available().await.unwrap());
    profiles::vault_acknowledge_recovery_kit().await.unwrap();
    assert!(!profiles::vault_has_pending_recovery_kit().await.unwrap());

    let host = parse(
        &inventory::hosts_save(
            json!({
                "id": "", "name": "files", "address": "files.example", "port": null,
                "username": "deploy", "credential_id": null, "group_id": null, "jump_chain": [],
                "jump_profile_id": null, "proxy_id": null, "proxy_command": null,
                "host_key_policy": "ask", "backend": "native", "keepalive_secs": null,
                "agent_forwarding": false, "tags": [], "notes": "", "metadata": {},
                "created_at_ms": 0, "updated_at_ms": 0, "auth_mode": "inherit"
            })
            .to_string(),
        )
        .await
        .unwrap(),
    );
    let host_id = host["id"].as_str().unwrap().to_owned();
    let client = cc_sftp_core::test_server::connect_local(remote.clone())
        .await
        .unwrap();
    let sftp_id = core
        .sftp_attach_client_for_tests(host_id.clone(), client)
        .await
        .unwrap();

    // ── browsing ────────────────────────────────────────────────────────
    let list = parse(
        &sftp::sftp_list_detailed(sftp_id.clone(), "/srv".into())
            .await
            .unwrap(),
    );
    assert_eq!(list[0]["name"], "app.conf");
    assert_eq!(list[0]["kind"], "file");
    assert_eq!(list[0]["size"], 10);
    let e = sftp::sftp_list_detailed(sftp_id.clone(), "/nope".into())
        .await
        .unwrap_err();
    assert_eq!(e.code, "not_found");
    assert_eq!(e.reason.as_deref(), Some("directory_not_found"));
    assert_eq!(e.details["path"], "/nope");
    assert_eq!(
        sftp::sftp_resolve_directory(sftp_id.clone(), "..".into(), "/srv".into())
            .await
            .unwrap(),
        "/"
    );
    sftp::sftp_create_file(sftp_id.clone(), "/srv/empty.txt".into())
        .await
        .unwrap();
    let e = sftp::sftp_create_file(sftp_id.clone(), "/srv/empty.txt".into())
        .await
        .unwrap_err();
    assert_eq!(
        (
            e.code.as_str(),
            e.reason.as_deref(),
            e.details["name"].as_str()
        ),
        ("already_exists", Some("already_exists"), "empty.txt")
    );
    sftp::sftp_duplicate(sftp_id.clone(), "/srv".into(), "/srv2".into())
        .await
        .unwrap();
    sftp::sftp_chmod(sftp_id.clone(), "/srv2/app.conf".into(), 0o640)
        .await
        .unwrap();
    let st = parse(
        &sftp::sftp_stat(sftp_id.clone(), "/srv2/app.conf".into())
            .await
            .unwrap(),
    );
    #[cfg(unix)]
    assert_eq!(st["permissions"], 0o640);
    let _ = st;
    let preview = sftp::sftp_read_preview(sftp_id.clone(), "/srv/app.conf".into(), 7)
        .await
        .unwrap();
    assert_eq!(
        (preview.data.as_slice(), preview.total_size),
        (&b"workers"[..], 10)
    );
    assert_eq!(parse(&sftp::sftp_transfers().await.unwrap()), json!([]));

    // ── edit session ────────────────────────────────────────────────────
    let s = parse(
        &sftp::edit_open(
            sftp_id.clone(),
            "/srv/app.conf".into(),
            Some(r#"{"kind":"default"}"#.into()),
        )
        .await
        .unwrap(),
    );
    assert_eq!(s["status"]["state"], "synced");
    assert_eq!(s["sftp_id"], sftp_id.as_str());
    assert!(!launches.0.lock().unwrap().is_empty());
    let session_id = s["id"].as_str().unwrap().to_owned();
    std::fs::write(s["local_path"].as_str().unwrap(), b"workers=8\n").unwrap();
    let status = parse(&sftp::edit_sync_now(session_id.clone()).await.unwrap());
    assert_eq!(status["state"], "synced");
    assert_eq!(
        std::fs::read(remote.join("srv/app.conf")).unwrap(),
        b"workers=8\n"
    );
    assert_eq!(
        parse(&sftp::edit_sessions().await.unwrap())[0]["uploads"],
        1
    );
    let out = parse(
        &sftp::edit_stop(session_id.clone(), EditStopChoice::KeepFiles)
            .await
            .unwrap(),
    );
    assert_eq!(out["outcome"], "kept_files");
    let left = parse(&sftp::edit_leftovers().await.unwrap());
    assert_eq!(left[0]["id"], session_id.as_str());
    assert_eq!(left[0]["locally_modified"], false);
    sftp::edit_discard_leftover(session_id.clone())
        .await
        .unwrap();
    assert_eq!(parse(&sftp::edit_leftovers().await.unwrap()), json!([]));
    let e = sftp::edit_open(sftp_id.clone(), "/srv".into(), None)
        .await
        .unwrap_err();
    assert_eq!(e.code, "invalid_input");

    // ── account / reveal ────────────────────────────────────────────────
    let e = account::account_logout().await.unwrap_err();
    assert_eq!(
        (e.code.as_str(), e.reason.as_deref()),
        ("local_profile", Some("synced_profile_required"))
    );
    let cred = parse(
        &credentials::credentials_add_password("db".into(), None, b"pw-123".to_vec())
            .await
            .unwrap(),
    );
    let secret = account::credentials_reveal_secret(cred["id"].as_str().unwrap().into())
        .await
        .unwrap();
    assert_eq!(secret, b"pw-123");

    // ── planner preview / snippets ──────────────────────────────────────
    let mut draft = host.clone();
    draft["jump_chain"] = json!([host_id]);
    let p = parse(
        &inventory::hosts_plan_preview(draft.to_string())
            .await
            .unwrap(),
    );
    assert_eq!(p["ok"], false);
    assert_eq!(p["username"]["source"], "host");
    assert!(p["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["code"] == "self_jump"));
    inventory::snippets_save(
        json!({
            "id": "", "name": "tail log", "description": "", "snippet_type": "bash",
            "shell": null, "template": "tail -n {{lines}} /var/log/syslog", "variables": [],
            "tags": [], "risk_level": "read_only", "source": "user",
            "created_by_device_id": null, "last_used_at_ms": null, "usage_count": 0,
            "created_at_ms": 0, "updated_at_ms": 0
        })
        .to_string(),
    )
    .await
    .unwrap();
    let hits = parse(
        &inventory::snippets_search("syslog".into(), 5)
            .await
            .unwrap(),
    );
    let snippets = parse(&inventory::snippets_list().await.unwrap());
    assert_eq!(hits[0]["snippet_id"], snippets[0]["id"]);
    let r = parse(
        &inventory::snippets_render(snippets[0].to_string(), r#"{"lines":"50"}"#.into())
            .await
            .unwrap(),
    );
    assert_eq!(r["command"], "tail -n 50 /var/log/syslog");

    // ── backup schedule ─────────────────────────────────────────────────
    let folder = dir.path().join("bk");
    std::fs::create_dir_all(&folder).unwrap();
    let e = backup::backup_schedule_set(json!({"enabled": true, "folder": null, "frequency": "weekly", "keep_last": 3, "last_run_at_ms": null, "next_run_at_ms": null, "last_error": null}).to_string())
        .await
        .unwrap_err();
    assert_eq!(e.reason.as_deref(), Some("backup_folder_required"));
    let sch = parse(
        &backup::backup_schedule_set(
            json!({"enabled": true, "folder": folder, "frequency": "weekly", "keep_last": 3,
                   "last_run_at_ms": null, "next_run_at_ms": null, "last_error": null})
            .to_string(),
        )
        .await
        .unwrap(),
    );
    assert!(sch["next_run_at_ms"].is_i64());
    let info = parse(&backup::backup_now().await.unwrap());
    assert_eq!(info["automatic"], true);
    assert_eq!(
        parse(&backup::backup_recent().await.unwrap())[0]["path"],
        info["path"]
    );

    // ── OS authenticator: report + attested unlock ──────────────────────
    // OS unlock is opt-in; enable it while unlocked with a fresh UI grant.
    app::os_auth_report(Some(OsAuthKindChoice::TouchId), false);
    profiles::vault_set_device_unlock_enabled_attested(true)
        .await
        .unwrap();
    app::os_auth_report(None, false);
    profiles::vault_lock().await.unwrap();
    let d = parse(&profiles::vault_device_unlock_info().await.unwrap());
    assert_eq!(d["available"], false);
    app::os_auth_report(Some(OsAuthKindChoice::TouchId), false);
    let d = parse(&profiles::vault_device_unlock_info().await.unwrap());
    assert_eq!(
        (d["available"].clone(), d["kind"].clone()),
        (json!(true), json!("touch_id"))
    );
    let e = profiles::vault_unlock_with_device().await.unwrap_err();
    assert_eq!(e.code, "os_auth_failed", "no grant without the UI prompt");
    profiles::vault_unlock_with_device_attested().await.unwrap();
    assert!(profiles::vault_is_unlocked().await.unwrap());
    app::os_auth_report(None, false);

    app::core_shutdown().await.unwrap();
}
