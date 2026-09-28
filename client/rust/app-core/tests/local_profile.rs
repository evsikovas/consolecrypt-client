//! Local-only profiles (ADR-0106) through the facade: create → inventory →
//! lock → unlock (passphrase, recovery key, device) → restart → backup
//! export/import → recovery flows. No server is involved at any point.

mod common;

use cc_app_core::*;
use common::*;
use secrecy::ExposeSecret;

type Snapshot = (
    Vec<HostDto>,
    Vec<GroupDto>,
    Vec<CredentialDto>,
    Vec<TunnelDto>,
    Vec<SnippetDto>,
    Vec<NoteDto>,
    VaultSettingsDto,
);

async fn snapshot(app: &AppCore) -> Snapshot {
    (
        app.list_hosts().await.unwrap(),
        app.list_groups().await.unwrap(),
        app.list_credentials().await.unwrap(),
        app.list_tunnels().await.unwrap(),
        app.list_snippets().await.unwrap(),
        app.list_notes().await.unwrap(),
        app.get_vault_settings().await.unwrap(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn local_profile_lifecycle_backup_and_recovery() {
    let tmp = tempfile::tempdir().unwrap();
    let store = memory_store();
    let dir_a = tmp.path().join("a");
    let app = app_with(&dir_a, store.clone(), true);

    // ── create: vault + Recovery Kit, onboarding check ─────────────────
    let created = app
        .create_local_profile("Personal".into(), PASSPHRASE.into())
        .await
        .unwrap();
    assert_eq!(created.profile.kind, ProfileKind::Local);
    assert_eq!(created.profile.vault_state, VaultStateDto::Unlocked);
    assert!(created.profile.active);
    let pid = created.profile.id.clone();
    let kit = created.recovery_kit.clone().unwrap();
    assert_eq!(kit.words.len(), 24);
    assert_eq!(kit.phrase, kit.words.join(" "));
    assert!(kit.server_url.is_none());
    assert!(kit.qr_payload.starts_with("consolecrypt-recovery:v1:"));
    let dbg = format!("{kit:?}");
    assert!(
        !dbg.contains(&kit.phrase) && !dbg.contains(&kit.qr_payload),
        "{dbg}"
    );

    let check = app.recovery_kit_start_check().await.unwrap();
    assert_eq!(check.positions.len(), 3);
    let wrong = app
        .recovery_kit_verify(vec!["x".into(), "y".into(), "z".into()])
        .await
        .unwrap();
    assert_eq!(wrong, check.positions);
    let answers: Vec<String> = check
        .positions
        .iter()
        .map(|p| kit.words[*p as usize - 1].to_uppercase())
        .collect();
    assert!(app.recovery_kit_verify(answers).await.unwrap().is_empty());
    assert!(app.pending_recovery_kit().await.unwrap().is_none());
    assert_eq!(
        app.get_vault_settings().await.unwrap().vault_name,
        "Personal"
    );

    // ── inventory: credentials (secrets only in Secret objects) ─────────
    let pw = app
        .add_password_credential(
            "db password".into(),
            Some("postgres".into()),
            "s3cr3t-pw-marker".into(),
        )
        .await
        .unwrap();
    assert!(pw.has_secret && pw.kind == CredentialKind::Password);
    let key = app
        .generate_ssh_key(
            "ed key".into(),
            Some("alex".into()),
            KeyGenAlgorithm::Ed25519,
            None,
            false,
        )
        .await
        .unwrap();
    assert!(key
        .public_key
        .as_deref()
        .unwrap()
        .starts_with("ssh-ed25519 "));
    assert_eq!(key.key_algorithm, Some(KeyAlgorithm::Ed25519));
    let enc = app
        .generate_ssh_key(
            "enc key".into(),
            None,
            KeyGenAlgorithm::Ed25519,
            Some("key pass".into()),
            true,
        )
        .await
        .unwrap();
    assert!(enc.key_encrypted && enc.has_remembered_passphrase);
    let ext = cc_ssh_core::keys::generate_key(
        KeyGenAlgorithm::Ed25519,
        "ext",
        Some(&secrecy::SecretString::from("ext-pass")),
    )
    .unwrap();
    let err = app
        .import_ssh_key(
            "imported".into(),
            None,
            ext.private_openssh.expose_secret().to_owned(),
            Some("wrong".into()),
            false,
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "invalid_input");
    let imported = app
        .import_ssh_key(
            "imported".into(),
            Some("deploy".into()),
            ext.private_openssh.expose_secret().to_owned(),
            Some("ext-pass".into()),
            false,
            None,
        )
        .await
        .unwrap();
    assert!(imported.key_encrypted && !imported.has_remembered_passphrase);
    assert_eq!(
        imported.fingerprint.as_deref(),
        Some(ext.fingerprint_sha256.as_str())
    );
    app.set_key_passphrase(imported.id.clone(), Some("ext-pass".into()))
        .await
        .unwrap();
    assert!(
        app.get_credential(imported.id.clone())
            .await
            .unwrap()
            .has_remembered_passphrase
    );
    assert_eq!(
        app.set_key_passphrase(imported.id.clone(), Some("bad".into()))
            .await
            .unwrap_err()
            .code(),
        "invalid_input"
    );
    let agent = app
        .add_agent_credential("os agent".into(), None, None)
        .await
        .unwrap();
    assert_eq!(agent.kind, CredentialKind::OsSshAgent);

    // ── hosts, groups, jump chain, tunnel, snippet, note ────────────────
    let mut group = GroupDto::new("prod");
    group.inherited_username = Some("ops".into());
    let group = app.save_group(group).await.unwrap();
    let bastion = app
        .save_host(host("bastion", "203.0.113.10", Some(&key.id)))
        .await
        .unwrap();
    let mut db = host("db", "10.0.0.5", Some(&pw.id));
    db.group_id = Some(group.id.clone());
    db.jump_chain = vec![bastion.id.clone()];
    db.port = Some(2222);
    let db = app.save_host(db).await.unwrap();
    let mut db2 = db.clone();
    db2.notes = "primary".into();
    let db2 = app.save_host(db2).await.unwrap();
    assert_eq!(db2.notes, "primary");
    assert_eq!(db2.created_at_ms, db.created_at_ms);
    let route = app.describe_connection(db.id.clone()).await.unwrap().route;
    assert!(route.contains("alex@203.0.113.10:22"), "{route}");
    assert!(route.contains("postgres@10.0.0.5:2222"), "{route}");

    let tunnel = app
        .save_tunnel(TunnelDto {
            id: String::new(),
            name: "pg".into(),
            kind: TunnelKind::Local,
            host_id: db.id.clone(),
            bind_host: "127.0.0.1".into(),
            bind_port: 15432,
            target_host: Some("127.0.0.1".into()),
            target_port: Some(5432),
            auto_start: false,
            binds_publicly: false,
            created_at_ms: 0,
            updated_at_ms: 0,
        })
        .await
        .unwrap();
    assert!(!tunnel.binds_publicly);
    let snippet = app
        .save_snippet(SnippetDto {
            package_name: None,
            catalog_id: None,
            id: String::new(),
            name: "pod logs".into(),
            description: String::new(),
            snippet_type: SnippetType::Kubectl,
            shell: None,
            template: "kubectl logs -n {{ns}} {{pod}}".into(),
            variables: vec![],
            tags: vec!["k8s".into()],
            risk_level: RiskLevel::ReadOnly,
            source: SnippetSource::User,
            created_by_device_id: None,
            last_used_at_ms: None,
            usage_count: 0,
            created_at_ms: 0,
            updated_at_ms: 0,
        })
        .await
        .unwrap();
    let vars: Vec<&str> = snippet.variables.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(vars, ["ns", "pod"]);
    app.save_note(NoteDto {
        id: String::new(),
        title: "runbook".into(),
        body: "restart order".into(),
        tags: vec![],
        created_at_ms: 0,
        updated_at_ms: 0,
    })
    .await
    .unwrap();
    let kh = app
        .add_known_host("203.0.113.10".into(), 22, key.public_key.clone().unwrap())
        .await
        .unwrap();
    assert_eq!(kh.source, KnownHostSource::Manual);
    assert!(app
        .export_known_hosts()
        .await
        .unwrap()
        .contains("203.0.113.10 ssh-ed25519"));

    // ── validation & referential integrity ──────────────────────────────
    let e = app
        .save_host(host("bad", "has space", None))
        .await
        .unwrap_err();
    assert_eq!(e.code(), "invalid_input");
    let dangling = uuid::Uuid::now_v7().to_string();
    let e = app
        .save_host(host("dangling", "10.0.0.9", Some(&dangling)))
        .await
        .unwrap_err();
    assert_eq!(e.code(), "invalid_input");
    assert_eq!(
        app.delete_credential(key.id.clone())
            .await
            .unwrap_err()
            .code(),
        "in_use"
    );
    assert_eq!(
        app.delete_host(bastion.id.clone())
            .await
            .unwrap_err()
            .code(),
        "in_use"
    );
    assert_eq!(
        app.delete_group(group.id.clone()).await.unwrap_err().code(),
        "in_use"
    );

    // ── no network in a local profile ───────────────────────────────────
    assert_eq!(
        app.sync_status().await.unwrap().phase,
        SyncPhaseDto::LocalOnly
    );
    assert_eq!(app.sync_now().await.unwrap_err().code(), "local_profile");
    assert_eq!(
        app.list_devices().await.unwrap_err().code(),
        "local_profile"
    );
    let meta = app.object_meta(db.id.clone()).await.unwrap();
    assert_eq!(meta.local_state, LocalState::LocalOnly);
    assert_eq!(meta.revision, 2);

    let snap = snapshot(&app).await;
    let objects = app.object_count().await.unwrap();

    // ── lock / unlock by passphrase, recovery key, device ───────────────
    app.lock().await.unwrap();
    assert!(!app.is_unlocked().await);
    assert_eq!(app.list_hosts().await.unwrap_err().code(), "vault_locked");
    assert_eq!(
        app.unlock_with_passphrase("wrong passphrase".into())
            .await
            .unwrap_err()
            .code(),
        "wrong_passphrase"
    );
    assert!(!app.verify_passphrase("nope nope".into()).await.unwrap());
    assert!(app.verify_passphrase(PASSPHRASE.into()).await.unwrap());
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert_eq!(snapshot(&app).await, snap);

    app.lock().await.unwrap();
    assert_eq!(
        app.unlock_with_recovery_key("abandon ".repeat(24))
            .await
            .unwrap_err()
            .code(),
        "wrong_recovery_key"
    );
    app.unlock_with_recovery_key(kit.phrase.clone())
        .await
        .unwrap();
    assert_eq!(snapshot(&app).await, snap);
    app.lock().await.unwrap();
    app.unlock_with_recovery_key(kit.qr_payload.clone())
        .await
        .unwrap();
    app.set_device_unlock_enabled(true).await.unwrap();
    app.lock().await.unwrap();
    app.unlock_with_device().await.unwrap();
    assert_eq!(snapshot(&app).await, snap);

    // ── restart the installation ────────────────────────────────────────
    app.close_profile().await.unwrap();
    let app = app_with(&dir_a, store.clone(), false);
    let listed = app.list_profiles().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].vault_state, VaultStateDto::Closed);
    assert_eq!(
        app.last_active_profile_id().await.unwrap(),
        Some(pid.clone())
    );
    let info = app.open_profile(pid.clone()).await.unwrap();
    assert_eq!(info.vault_state, VaultStateDto::Locked);
    assert_eq!(
        app.unlock_with_device().await.unwrap_err().code(),
        "os_auth_failed"
    );
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert_eq!(snapshot(&app).await, snap);
    assert_eq!(app.object_count().await.unwrap(), objects);

    // ── backup export → import into a fresh installation ────────────────
    let backup = tmp.path().join("vault.ccbackup");
    let path = backup.to_string_lossy().to_string();
    let summary = app.export_backup(path.clone()).await.unwrap();
    assert_eq!(summary.objects, objects);
    let raw = std::fs::read_to_string(&backup).unwrap();
    for needle in [
        "s3cr3t-pw-marker",
        "203.0.113.10",
        "PRIVATE KEY",
        "postgres",
        PASSPHRASE,
    ] {
        assert!(!raw.contains(needle), "backup leaks {needle:?}");
    }
    let app2 = common::app(&tmp.path().join("b"));
    let restored = app2
        .import_backup(
            path.clone(),
            "Restored".into(),
            BackupUnlock::Passphrase(PASSPHRASE.into()),
            None,
        )
        .await
        .unwrap();
    assert_eq!(restored.profile.kind, ProfileKind::Local);
    assert_eq!(restored.profile.vault_id, info.vault_id);
    assert_eq!(snapshot(&app2).await, snap);
    assert_eq!(app2.object_count().await.unwrap(), objects);
    app2.lock().await.unwrap();
    app2.unlock_with_passphrase(PASSPHRASE.into())
        .await
        .unwrap();

    let app3 = common::app(&tmp.path().join("c"));
    let e = app3
        .import_backup(
            path.clone(),
            "x".into(),
            BackupUnlock::Passphrase("nope nope nope".into()),
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "wrong_passphrase");
    assert!(
        app3.list_profiles().await.unwrap().is_empty(),
        "no half-created profile"
    );
    app3.import_backup(
        path.clone(),
        "Recovered".into(),
        BackupUnlock::RecoveryKey(kit.phrase.clone()),
        Some("brand new passphrase".into()),
    )
    .await
    .unwrap();
    app3.lock().await.unwrap();
    assert_eq!(
        app3.unlock_with_passphrase(PASSPHRASE.into())
            .await
            .unwrap_err()
            .code(),
        "wrong_passphrase"
    );
    app3.unlock_with_passphrase("brand new passphrase".into())
        .await
        .unwrap();
    assert_eq!(snapshot(&app3).await, snap);

    // ── change passphrase, regenerate kit, forgot-passphrase ────────────
    assert_eq!(
        app.change_passphrase("wrong".into(), "second passphrase!".into())
            .await
            .unwrap_err()
            .code(),
        "wrong_passphrase"
    );
    assert_eq!(
        app.change_passphrase(PASSPHRASE.into(), "short".into())
            .await
            .unwrap_err()
            .code(),
        "weak_passphrase"
    );
    app.change_passphrase(PASSPHRASE.into(), "second passphrase!".into())
        .await
        .unwrap();
    let new_kit = app.regenerate_recovery_kit().await.unwrap();
    assert_ne!(new_kit.phrase, kit.phrase);
    app.recovery_kit_acknowledge().await.unwrap();
    app.lock().await.unwrap();
    assert_eq!(
        app.unlock_with_passphrase(PASSPHRASE.into())
            .await
            .unwrap_err()
            .code(),
        "wrong_passphrase"
    );
    app.unlock_with_passphrase("second passphrase!".into())
        .await
        .unwrap();
    app.lock().await.unwrap();
    assert_eq!(
        app.unlock_with_recovery_key(kit.phrase.clone())
            .await
            .unwrap_err()
            .code(),
        "wrong_recovery_key"
    );
    app.reset_passphrase_with_recovery_key(new_kit.phrase.clone(), "third passphrase!!".into())
        .await
        .unwrap();
    app.lock().await.unwrap();
    app.unlock_with_passphrase("third passphrase!!".into())
        .await
        .unwrap();
    assert_eq!(snapshot(&app).await, snap);

    // ── deletes & profile removal ───────────────────────────────────────
    app.delete_host(db.id.clone()).await.unwrap();
    assert!(
        app.list_tunnels().await.unwrap().is_empty(),
        "tunnels cascade"
    );
    app.delete_credential(pw.id.clone()).await.unwrap();
    assert_eq!(app.list_credentials().await.unwrap().len(), 4);
    app.remove_profile(pid.clone()).await.unwrap();
    assert!(app.list_profiles().await.unwrap().is_empty());
    assert_eq!(
        app.list_hosts().await.unwrap_err().code(),
        "no_active_profile"
    );
    for a in [&app, &app2, &app3] {
        a.shutdown().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_nul_and_control_characters() {
    let tmp = tempfile::tempdir().unwrap();
    let app = common::app(tmp.path());
    for name in ["bad\0name", "bell\u{7}", "new\nline", "\u{1b}[31mred"] {
        let e = app
            .create_local_profile(name.into(), PASSPHRASE.into())
            .await
            .unwrap_err();
        assert_eq!(e.code(), "invalid_input", "{name:?}");
    }
    let e = app
        .create_local_profile("ok".into(), "pass\0phrase-long".into())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "invalid_input");
    for (url, email) in [
        ("http://localhost:1", "a\0@example.org"),
        ("http://localhost:1", "a@exa\u{7}mple.org"),
        ("http://local\0host:1", "a@example.org"),
        ("http://localhost:1/a\nb", "a@example.org"),
        ("ftp://localhost:1", "a@example.org"),
    ] {
        let e = app
            .create_synced_profile(
                "x".into(),
                url.into(),
                email.into(),
                ACCOUNT_PASSWORD.into(),
                AccountMode::Login,
            )
            .await
            .unwrap_err();
        assert_eq!(e.code(), "invalid_input", "{url:?} {email:?}");
    }
    let e = app
        .create_synced_profile(
            "x".into(),
            "http://localhost:1".into(),
            "a@example.org".into(),
            "pw\0with-nul-inside".into(),
            AccountMode::Login,
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "invalid_input");
    assert!(app.list_profiles().await.unwrap().is_empty());

    let created = app
        .create_local_profile("ok".into(), PASSPHRASE.into())
        .await
        .unwrap();
    for p in ["a\0b.ccbackup", "a\nb.ccbackup", ""] {
        assert_eq!(
            app.export_backup(p.into()).await.unwrap_err().code(),
            "invalid_input"
        );
        let e = app
            .import_backup(
                p.into(),
                "n".into(),
                BackupUnlock::Passphrase(PASSPHRASE.into()),
                None,
            )
            .await
            .unwrap_err();
        assert_eq!(e.code(), "invalid_input");
    }
    assert_eq!(
        app.rename_profile(created.profile.id.clone(), "a\u{1b}b".into())
            .await
            .unwrap_err()
            .code(),
        "invalid_input"
    );
    let renamed = app
        .rename_profile(created.profile.id.clone(), "  Home  ".into())
        .await
        .unwrap();
    assert_eq!(renamed.display_name, "Home");
    assert_eq!(app.list_profiles().await.unwrap().len(), 1);
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_auth_modes_adr_0101_6a() {
    let tmp = tempfile::tempdir().unwrap();
    let app = common::app(tmp.path());
    app.create_local_profile("Auth".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let key = app
        .generate_ssh_key(
            "shared".into(),
            Some("keyuser".into()),
            KeyGenAlgorithm::Ed25519,
            None,
            false,
        )
        .await
        .unwrap();
    let mut g = GroupDto::new("prod");
    g.inherited_credential_id = Some(key.id.clone());
    g.inherited_username = Some("groupuser".into());
    let g = app.save_group(g).await.unwrap();
    let in_group = |name: &str| {
        let mut h = host(name, "10.1.0.1", None);
        h.group_id = Some(g.id.clone());
        h
    };

    // Inherit: group credential (its username wins over the group's).
    let h = app
        .save_host_with_auth(in_group("inherit"), HostAuth::Inherit)
        .await
        .unwrap();
    assert_eq!(h.auth_mode, HostAuthMode::Inherit);
    assert!(app
        .describe_connection(h.id.clone())
        .await
        .unwrap()
        .route
        .starts_with("keyuser@"));

    // PasswordPrompt: nothing stored, stops inheritance (group username).
    let mut p = app
        .save_host_with_auth(in_group("prompt"), HostAuth::PasswordPrompt)
        .await
        .unwrap();
    assert_eq!(p.auth_mode, HostAuthMode::PasswordPrompt);
    assert!(p.credential_id.is_none());
    assert_eq!(
        p.metadata.get(META_AUTH_PROMPT).map(String::as_str),
        Some("password")
    );
    let route = app.describe_connection(p.id.clone()).await.unwrap().route;
    assert!(route.starts_with("groupuser@"), "{route}");
    // Without a UI subscribed the prompt is declined → connect is cancelled
    // before any network I/O would be needed for auth.
    p.notes = "still prompt".into();
    let p = app.save_host(p).await.unwrap();
    assert_eq!(p.auth_mode, HostAuthMode::PasswordPrompt);

    // InlinePassword: host-owned credential, hidden from shared pickers.
    let before = app.list_credentials().await.unwrap().len();
    let e = app
        .save_host_with_auth(
            in_group("inline"),
            HostAuth::InlinePassword { password: None },
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "invalid_input");
    assert_eq!(
        app.list_credentials().await.unwrap().len(),
        before,
        "nothing left behind"
    );
    let i = app
        .save_host_with_auth(
            in_group("inline"),
            HostAuth::InlinePassword {
                password: Some("inline-pw".into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(i.auth_mode, HostAuthMode::InlinePassword);
    let inline_id = i.credential_id.clone().unwrap();
    let creds = app.list_credentials().await.unwrap();
    assert_eq!(creds.len(), before + 1);
    let inline = creds.iter().find(|c| c.id == inline_id).unwrap();
    assert_eq!(inline.owner_host_id.as_deref(), Some(i.id.as_str()));
    assert!(inline.has_secret);
    // Keep the stored secret / change it: same credential id.
    let i = app
        .save_host_with_auth(i, HostAuth::InlinePassword { password: None })
        .await
        .unwrap();
    assert_eq!(i.credential_id.as_deref(), Some(inline_id.as_str()));
    let i = app
        .save_host_with_auth(
            i,
            HostAuth::InlinePassword {
                password: Some("new-pw".into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(i.credential_id.as_deref(), Some(inline_id.as_str()));
    // Another host cannot borrow the inline credential.
    let e = app
        .save_host_with_auth(
            in_group("thief"),
            HostAuth::Credential {
                credential_id: inline_id.clone(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "invalid_input");
    // Replacing it with a shared credential deletes the inline one.
    let i = app
        .save_host_with_auth(
            i,
            HostAuth::Credential {
                credential_id: key.id.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(i.auth_mode, HostAuthMode::Credential);
    assert!(!i.metadata.contains_key(META_INLINE_CREDENTIAL));
    assert_eq!(app.list_credentials().await.unwrap().len(), before);

    // Agent: created once, reused afterwards.
    let a1 = app
        .save_host_with_auth(
            in_group("agent1"),
            HostAuth::Agent {
                kind: AgentKind::Os,
                path: None,
            },
        )
        .await
        .unwrap();
    let a2 = app
        .save_host_with_auth(
            in_group("agent2"),
            HostAuth::Agent {
                kind: AgentKind::Os,
                path: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(a1.credential_id, a2.credential_id);
    let e = app
        .save_host_with_auth(
            in_group("agent3"),
            HostAuth::Agent {
                kind: AgentKind::External,
                path: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "invalid_input");

    // Deleting a host deletes its inline credential.
    let d = app
        .save_host_with_auth(
            in_group("doomed"),
            HostAuth::InlinePassword {
                password: Some("x-pw".into()),
            },
        )
        .await
        .unwrap();
    let n = app.list_credentials().await.unwrap().len();
    app.delete_host(d.id).await.unwrap();
    assert_eq!(app.list_credentials().await.unwrap().len(), n - 1);

    // Key passphrase remember / forget.
    let enc = app
        .generate_ssh_key(
            "enc".into(),
            None,
            KeyGenAlgorithm::Ed25519,
            Some("kp-123".into()),
            false,
        )
        .await
        .unwrap();
    assert!(!enc.has_remembered_passphrase);
    assert!(
        app.remember_key_passphrase(enc.id.clone(), "kp-123".into())
            .await
            .unwrap()
            .has_remembered_passphrase
    );
    assert!(
        !app.forget_key_passphrase(enc.id.clone())
            .await
            .unwrap()
            .has_remembered_passphrase
    );
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forgot_passphrase_with_device_auth() {
    let tmp = tempfile::tempdir().unwrap();
    let store = memory_store();
    let app = app_with(tmp.path(), store.clone(), true);
    app.create_local_profile("Dev".into(), PASSPHRASE.into())
        .await
        .unwrap();
    app.set_device_unlock_enabled(true).await.unwrap();
    app.save_host(host("h", "192.0.2.7", None)).await.unwrap();
    app.lock().await.unwrap();
    app.reset_passphrase_with_device("reset via touch id".into())
        .await
        .unwrap();
    assert!(app.is_unlocked().await);
    app.lock().await.unwrap();
    assert_eq!(
        app.unlock_with_passphrase(PASSPHRASE.into())
            .await
            .unwrap_err()
            .code(),
        "wrong_passphrase"
    );
    app.unlock_with_passphrase("reset via touch id".into())
        .await
        .unwrap();
    assert_eq!(app.list_hosts().await.unwrap().len(), 1);
    app.shutdown().await.unwrap();

    // Without an OS authenticator the device path is refused.
    let app2 = app_with(tmp.path(), store, false);
    let pid = app2.last_active_profile_id().await.unwrap().unwrap();
    app2.open_profile(pid).await.unwrap();
    assert_eq!(
        app2.reset_passphrase_with_device("whatever passphrase".into())
            .await
            .unwrap_err()
            .code(),
        "os_auth_failed"
    );
    app2.shutdown().await.unwrap();
}
