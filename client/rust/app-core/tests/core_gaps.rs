//! Facade additions for the UI (core-gaps): account logout / password
//! change / reset e-mail, the explicit secret reveal, planner preview with
//! provenance + diagnostic codes, error reasons / args, snippet search and
//! rendering without a provider, the persisted Recovery Kit flag, account
//! hints of closed profiles, enable-sync progress events, the backup
//! scheduler and the attested OS authenticator.

mod common;

use cc_app_core::platform::{ExternalOsAuthenticator, OsAuthAvailability, OsAuthKind};
use cc_app_core::*;
use cc_sync_core::mock::MockServer;
use common::*;
use std::sync::Arc;

fn snippet(name: &str, template: &str) -> SnippetDto {
    SnippetDto {
        package_name: None,
        catalog_id: None,
        id: String::new(),
        name: name.into(),
        description: String::new(),
        snippet_type: SnippetType::Bash,
        shell: None,
        template: template.into(),
        variables: Vec::new(),
        tags: Vec::new(),
        risk_level: RiskLevel::Modifying,
        source: SnippetSource::User,
        created_by_device_id: None,
        last_used_at_ms: None,
        usage_count: 0,
        created_at_ms: 0,
        updated_at_ms: 0,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn account_logout_password_change_and_reset() {
    let server = MockServer::start().await;
    let url = server.url().to_string();
    let tmp = tempfile::tempdir().unwrap();
    let app = common::app(tmp.path());
    let email = "gaps@example.org";
    app.create_synced_profile(
        "Work".into(),
        url.clone(),
        email.into(),
        ACCOUNT_PASSWORD.into(),
        AccountMode::Register,
    )
    .await
    .unwrap();
    app.create_vault(PASSPHRASE.into()).await.unwrap();
    let mut events = app.subscribe_events();

    // Change password: wrong current → reason; too short → reason.
    let e = app
        .change_account_password("not the password".into(), "a-brand-new-password-1".into())
        .await
        .unwrap_err();
    assert_eq!(
        (e.code(), e.reason()),
        ("invalid_credentials", Some("current_password_wrong"))
    );
    let e = app
        .change_account_password(ACCOUNT_PASSWORD.into(), "short".into())
        .await
        .unwrap_err();
    assert_eq!(e.reason(), Some("account_password_too_short"));
    let new_password = "a-brand-new-password-1";
    app.change_account_password(ACCOUNT_PASSWORD.into(), new_password.into())
        .await
        .unwrap();

    // Reset e-mail: no profile needed, always accepted; validated input.
    app.request_password_reset(url.clone(), email.into())
        .await
        .unwrap();
    app.request_password_reset(url.clone(), "nobody@example.org".into())
        .await
        .unwrap();
    let e = app
        .request_password_reset(url.clone(), "not-an-email".into())
        .await
        .unwrap_err();
    assert_eq!(e.reason(), Some("invalid_email"));

    // Logout: tokens gone → server calls need a new sign-in.
    app.logout().await.unwrap();
    let mut signed_out = false;
    while let Ok(ev) = events.try_recv() {
        signed_out |= matches!(ev, AppEvent::SignedOut { .. });
    }
    assert!(signed_out);
    assert_eq!(
        app.list_devices().await.unwrap_err().code(),
        "reauth_required"
    );
    app.reauthenticate(new_password.into(), false)
        .await
        .unwrap();
    assert!(!app.list_devices().await.unwrap().devices.is_empty());

    // Local profiles have no account.
    let local = common::app(&tmp.path().join("local"));
    local
        .create_local_profile("L".into(), PASSPHRASE.into())
        .await
        .unwrap();
    assert_eq!(local.logout().await.unwrap_err().code(), "local_profile");
    local.shutdown().await.unwrap();
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reveal_secret_plan_preview_errors_and_snippets() {
    let tmp = tempfile::tempdir().unwrap();
    let app = common::app(tmp.path());
    app.create_local_profile("P".into(), PASSPHRASE.into())
        .await
        .unwrap();

    // Reveal: password and key credentials only.
    let pw = app
        .add_password_credential("db".into(), Some("postgres".into()), "s3cr3t-pw".into())
        .await
        .unwrap();
    let mut r = app.reveal_credential_secret(pw.id.clone()).await.unwrap();
    assert_eq!(r.kind, "password");
    assert!(!format!("{r:?}").contains("s3cr3t-pw"));
    assert_eq!(r.take_bytes(), b"s3cr3t-pw");
    let key = app
        .generate_ssh_key("k".into(), None, KeyGenAlgorithm::Ed25519, None, false)
        .await
        .unwrap();
    let r = app.reveal_credential_secret(key.id.clone()).await.unwrap();
    assert_eq!(r.kind, "private_key");
    assert!(r.value.contains("OPENSSH PRIVATE KEY"));
    let agent = app
        .add_agent_credential("agent".into(), None, None)
        .await
        .unwrap();
    let e = app.reveal_credential_secret(agent.id).await.unwrap_err();
    assert_eq!(
        (e.code(), e.reason()),
        ("not_found", Some("secret_not_stored"))
    );

    // Planner preview with provenance.
    let mut g = GroupDto::new("prod");
    g.inherited_port = Some(2222);
    g.inherited_credential_id = Some(pw.id.clone());
    let g = app.save_group(g).await.unwrap();
    let mut bastion = HostDto::new("bastion", "203.0.113.1");
    bastion.username = Some("jump".into());
    let bastion = app.save_host(bastion).await.unwrap();
    let mut db = HostDto::new("db", "10.0.0.5");
    db.group_id = Some(g.id.clone());
    db.jump_chain = vec![bastion.id.clone()];
    let db = app.save_host(db).await.unwrap();
    let p = app.plan_preview_host(db.id.clone()).await.unwrap();
    assert!(p.ok, "{:?}", p.diagnostics);
    assert_eq!(p.port.value, Some(2222));
    assert_eq!(p.port.source, ValueSourceDto::Group);
    assert_eq!(p.port.source_name.as_deref(), Some("prod"));
    assert_eq!(p.credential_id.value.as_deref(), Some(pw.id.as_str()));
    assert_eq!(p.username.value.as_deref(), Some("postgres"));
    assert_eq!(p.username.source, ValueSourceDto::Credential);
    assert_eq!(p.route.len(), 1);
    assert_eq!(p.route[0].name, "bastion");
    assert_eq!(p.route_source.source, ValueSourceDto::Host);
    assert_eq!(p.group_path, vec!["prod".to_owned()]);
    assert!(p.route_description.unwrap().contains("jump@203.0.113.1"));

    // Unsaved draft: missing jump host + self jump → codes, not English.
    let mut draft = db.clone();
    draft.port = Some(22);
    draft.jump_chain = vec![db.id.clone(), uuid::Uuid::now_v7().to_string()];
    let p = app.plan_preview(draft).await.unwrap();
    assert!(!p.ok);
    let codes: Vec<&str> = p.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(codes.contains(&"self_jump"), "{codes:?}");
    assert!(codes.contains(&"jump_host_deleted"), "{codes:?}");
    assert_eq!(p.port.source, ValueSourceDto::Host);
    let mut draft = HostDto::new("new", "bad address");
    draft.id = uuid::Uuid::now_v7().to_string();
    let p = app.plan_preview(draft).await.unwrap();
    let d = p
        .diagnostics
        .iter()
        .find(|d| d.code == "invalid_host")
        .unwrap();
    assert_eq!(d.args["field"], "address");
    let mut prompt = HostDto::new("ask", "10.0.0.9");
    prompt
        .metadata
        .insert(META_AUTH_PROMPT.into(), "password".into());
    let p = app.plan_preview(prompt).await.unwrap();
    assert!(p.prompts_for_password);
    assert!(p.diagnostics.iter().any(|d| d.code == "credential_prompt"));

    // Connection-plan errors carry the diagnostic code.
    let mut orphan = HostDto::new("orphan", "10.0.0.7");
    orphan.jump_chain = vec![bastion.id.clone()];
    let orphan = app.save_host(orphan).await.unwrap();
    app.delete_host(bastion.id.clone()).await.unwrap_err(); // still referenced
    let e = app
        .describe_connection(uuid::Uuid::now_v7().to_string())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "connection_plan");
    assert_eq!(e.args()["diagnostic"], "missing_host");
    let _ = orphan;

    // Error reasons of common failures.
    let e = app
        .get_host(uuid::Uuid::now_v7().to_string())
        .await
        .unwrap_err();
    assert_eq!(e.reason(), Some("host_not_found"));
    let e = app
        .change_passphrase("wrong current passphrase".into(), "x".repeat(20))
        .await
        .unwrap_err();
    assert_eq!(
        (e.code(), e.reason()),
        ("wrong_passphrase", Some("current_passphrase_wrong"))
    );
    app.lock().await.unwrap();
    let e = app.list_hosts().await.unwrap_err();
    assert_eq!(e.reason(), Some("vault_locked"));
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();

    // Snippets: search without a provider, render with the core's quoting.
    let mut s = snippet("restart nginx", "sudo systemctl restart {{service}}");
    s.tags = vec!["web".into()];
    let saved = app.save_snippet(s).await.unwrap();
    app.save_snippet(snippet("disk usage", "df -h"))
        .await
        .unwrap();
    let hits = app.search_snippets("nginx".into(), 10).await.unwrap();
    assert_eq!(
        hits.first().map(|h| h.snippet_id.as_str()),
        Some(saved.id.as_str())
    );
    assert_eq!(
        app.search_snippets(String::new(), 10).await.unwrap().len(),
        2
    );
    let mut values = std::collections::HashMap::new();
    values.insert("service".to_owned(), "nginx; rm -rf /".to_owned());
    let r = app.snippet_render_dto(saved.clone(), values).await.unwrap();
    let cmd = r.command.unwrap_or_default();
    assert!(cmd.starts_with("sudo systemctl restart "), "{cmd}");
    assert!(cmd.contains('\''), "value is quoted: {cmd}");
    let r = app
        .snippet_render_dto(saved, std::collections::HashMap::new())
        .await
        .unwrap();
    assert!(r.command.is_none());
    assert_eq!(r.field_errors[0].name, "service");
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn recovery_flag_hints_enable_sync_progress_and_device_unlock() {
    let server = MockServer::start().await;
    let url = server.url().to_string();
    let tmp = tempfile::tempdir().unwrap();
    let store = memory_store();
    let dir = tmp.path().join("a");
    let auth = Arc::new(ExternalOsAuthenticator::new());
    let app = AppCore::with_platform(config(&dir), store.clone(), auth.clone()).unwrap();

    // Recovery Kit flag survives a restart; acknowledge clears it.
    let created = app
        .create_local_profile("Mine".into(), PASSPHRASE.into())
        .await
        .unwrap();
    assert!(created.profile.recovery_kit_pending);
    let pid = created.profile.id.clone();
    auth.set_availability(OsAuthAvailability::Available(OsAuthKind::TouchId));
    auth.grant_once();
    app.set_device_unlock_enabled(true).await.unwrap();
    auth.set_availability(OsAuthAvailability::Unsupported);
    app.shutdown().await.unwrap();
    let app = AppCore::with_platform(config(&dir), store.clone(), auth.clone()).unwrap();
    let info = app.open_profile(pid.clone()).await.unwrap();
    assert!(info.recovery_kit_pending, "persisted");
    assert!(app.recovery_kit_check_pending().await.unwrap());
    assert!(
        app.pending_recovery_kit().await.unwrap().is_none(),
        "kit itself not kept"
    );
    app.recovery_kit_acknowledge().await.unwrap();
    assert!(
        !app.active_profile()
            .await
            .unwrap()
            .unwrap()
            .recovery_kit_pending
    );

    // Device unlock: needs availability + a fresh single-use grant.
    let d = app.device_unlock_info().await.unwrap();
    assert!(d.has_device_envelope && !d.available);
    assert_eq!(
        app.unlock_with_device().await.unwrap_err().code(),
        "os_auth_failed"
    );
    auth.set_availability(OsAuthAvailability::Available(OsAuthKind::TouchId));
    let d = app.device_unlock_info().await.unwrap();
    assert!(d.available);
    assert_eq!(d.kind.as_deref(), Some("touch_id"));
    assert_eq!(
        app.unlock_with_device().await.unwrap_err().code(),
        "os_auth_failed"
    );
    auth.grant_once();
    app.unlock_with_device().await.unwrap();
    assert!(app.is_unlocked().await);

    // Enable sync: progress events, then the closed profile keeps its hint.
    app.save_host(HostDto::new("h1", "10.0.0.1")).await.unwrap();
    let mut events = app.subscribe_events();
    app.enable_sync(
        url.clone(),
        "hint@example.org".into(),
        ACCOUNT_PASSWORD.into(),
        AccountMode::Register,
    )
    .await
    .unwrap();
    let mut steps = Vec::new();
    while let Ok(ev) = events.try_recv() {
        if let AppEvent::EnableSyncProgress { step, total, .. } = ev {
            if step != "uploading" || steps.last() != Some(&step) {
                steps.push(step.clone());
            }
            if step == "uploading" {
                assert!(total > 0);
            }
        }
    }
    assert_eq!(
        steps,
        [
            "authenticating",
            "creating_remote_vault",
            "uploading",
            "finishing",
            "done"
        ]
    );
    app.create_local_profile("Other".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let all = app.list_profiles().await.unwrap();
    let closed = all.iter().find(|p| p.id == pid).unwrap();
    assert!(!closed.active);
    assert_eq!(closed.server_url.as_deref(), Some(url.as_str()));
    assert_eq!(closed.email.as_deref(), Some("hint@example.org"));
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn backup_schedule_runs_due_backups_with_retention() {
    let tmp = tempfile::tempdir().unwrap();
    let app = common::app(&tmp.path().join("data"));
    app.create_local_profile("Backed up".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let folder = tmp.path().join("backups");
    std::fs::create_dir_all(&folder).unwrap();

    assert_eq!(
        app.backup_schedule().await.unwrap(),
        BackupScheduleDto::default()
    );
    let e = app
        .set_backup_schedule(BackupScheduleDto {
            enabled: true,
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(e.reason(), Some("backup_folder_required"));
    let e = app
        .set_backup_schedule(BackupScheduleDto {
            keep_last: 0,
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(e.reason(), Some("keep_at_least_one"));
    let sch = app
        .set_backup_schedule(BackupScheduleDto {
            enabled: true,
            folder: Some(folder.to_string_lossy().into_owned()),
            frequency: BackupFrequencyDto::Daily,
            keep_last: 2,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(sch.next_run_at_ms.is_some());
    assert_eq!(app.run_due_backup().await.unwrap(), None, "not due yet");

    // Three runs, keep two.
    let mut events = app.subscribe_events();
    for _ in 0..3 {
        app.backup_now().await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    }
    let recent = app.recent_backups().await.unwrap();
    assert_eq!(recent.len(), 2);
    assert!(recent.iter().all(|b| b.automatic && b.format_version >= 1));
    let files = std::fs::read_dir(&folder).unwrap().count();
    assert_eq!(files, 2, "retention deleted the oldest file");
    let mut completed = 0;
    while let Ok(ev) = events.try_recv() {
        completed += usize::from(matches!(ev, AppEvent::BackupCompleted { .. }));
    }
    assert_eq!(completed, 3);
    let sch = app.backup_schedule().await.unwrap();
    assert!(sch.last_run_at_ms.is_some() && sch.last_error.is_none());

    // Manual exports are remembered too (not automatic, not pruned).
    let manual = tmp.path().join("manual.ccbackup");
    app.export_backup(manual.to_string_lossy().into_owned())
        .await
        .unwrap();
    assert!(app
        .recent_backups()
        .await
        .unwrap()
        .iter()
        .any(|b| !b.automatic && b.path.ends_with("manual.ccbackup")));

    // A failing run is recorded.
    app.set_backup_schedule(BackupScheduleDto {
        enabled: true,
        folder: Some(
            tmp.path()
                .join("missing/dir")
                .to_string_lossy()
                .into_owned(),
        ),
        keep_last: 2,
        ..Default::default()
    })
    .await
    .unwrap();
    assert!(app.backup_now().await.is_err());
    assert!(app.backup_schedule().await.unwrap().last_error.is_some());
    app.shutdown().await.unwrap();
}
