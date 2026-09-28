//! SSH through app-core against the Docker testbed (bastion1 → bastion2 →
//! target). Host and key live in the vault; ssh-core plans the route from
//! vault objects; secrets come only through the credential resolver; host
//! keys are recorded as vault objects. Gated by `CC_SSH_IT=1`:
//!
//! ```sh
//! CC_SSH_IT=1 cargo test -p cc-app-core --test ssh_docker -- --nocapture
//! ```

mod common;

use cc_app_core::*;
use cc_ssh_core::testing::{self, SshTestbed};
use common::*;
use secrecy::ExposeSecret;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Launcher that starts nothing (edit sessions "open" the working copy).
#[derive(Debug)]
struct NoLaunch;

impl platform::Launcher for NoLaunch {
    fn launch(
        &self,
        _spec: &platform::LaunchSpec,
    ) -> Result<platform::LaunchOutput, platform::OpenError> {
        Ok(platform::LaunchOutput::default())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ssh_terminal_tunnel_sftp_through_two_jump_hosts() {
    if !testing::enabled() {
        eprintln!("skipped: set CC_SSH_IT=1 to run Docker integration tests");
        return;
    }
    let tb = tokio::task::spawn_blocking(|| SshTestbed::start(true))
        .await
        .unwrap()
        .expect("ssh testbed");
    let tmp = tempfile::tempdir().unwrap();
    let app = common::app(&tmp.path().join("app"));
    // Edit sessions must not start real applications.
    app.set_file_opener(platform::FileOpener::new(
        platform::OsFamily::current(),
        std::sync::Arc::new(NoLaunch),
    ));
    app.create_local_profile("SSH IT".into(), PASSPHRASE.into())
        .await
        .unwrap();

    // Unknown host keys of the target are asked (policy Ask) — answer
    // "accept and save" from a UI stand-in; the bastions use AcceptNew.
    let mut prompts = app.subscribe_prompts();
    let responder = app.clone();
    let tb_password = tb.secrets.password.expose_secret().to_owned();
    let asked = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let asked2 = asked.clone();
    let prompt_task = tokio::spawn(async move {
        while let Ok(p) = prompts.recv().await {
            match p {
                PromptRequest::HostKey(h) => {
                    asked2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    assert!(h.fingerprint_sha256.starts_with("SHA256:"));
                    responder
                        .answer_host_key_prompt(h.request_id, HostKeyDecision::AcceptAndSave)
                        .unwrap();
                }
                PromptRequest::Password(pw) => {
                    assert_eq!(pw.host_name, "b1-prompt");
                    responder
                        .answer_password_prompt(pw.request_id, Some(tb_password.clone()))
                        .unwrap();
                }
                PromptRequest::Passphrase(_) => {}
            }
        }
    });

    let s = &tb.secrets;
    let cred = app
        .import_ssh_key(
            "it key".into(),
            Some(s.user.clone()),
            s.key.private_openssh.expose_secret().to_owned(),
            None,
            false,
            None,
        )
        .await
        .unwrap();
    let mk = |name: &str, addr: &str, port: u16, policy: HostKeyPolicy| {
        let mut h = host(name, addr, Some(&cred.id));
        h.port = Some(port);
        h.host_key_policy = policy;
        h
    };
    let b1 = app
        .save_host(mk(
            "bastion1",
            "127.0.0.1",
            tb.bastion1_port,
            HostKeyPolicy::AcceptNew,
        ))
        .await
        .unwrap();
    let b2 = app
        .save_host(mk(
            "bastion2",
            tb.bastion2.as_deref().unwrap(),
            22,
            HostKeyPolicy::AcceptNew,
        ))
        .await
        .unwrap();
    let mut t = mk(
        "target",
        tb.target.as_deref().unwrap(),
        22,
        HostKeyPolicy::Ask,
    );
    t.jump_chain = vec![b1.id.clone(), b2.id.clone()];
    let target = app.save_host(t).await.unwrap();

    // ── exec over the 2-hop chain ───────────────────────────────────────
    let out = app
        .exec(target.id.clone(), "echo app-core-exec-ok; hostname".into())
        .await
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("app-core-exec-ok"), "{stdout}");
    assert_eq!(out.exit_status, Some(0));
    println!("IT exec via bastion1 -> bastion2 -> target ... ok");

    // Host keys are vault objects now (synced across devices).
    let known = app.list_known_hosts().await.unwrap();
    for pattern in [
        format!("[127.0.0.1]:{}", tb.bastion1_port),
        tb.bastion2.clone().unwrap(),
        tb.target.clone().unwrap(),
    ] {
        assert!(
            known
                .iter()
                .any(|k| k.host_pattern == pattern && k.source == KnownHostSource::Tofu),
            "{pattern} recorded: {known:?}"
        );
    }
    // A second connect is silent (keys known) and still works.
    app.exec(target.id.clone(), "true".into()).await.unwrap();

    // ── password auth: asked at connect, and host-owned inline ──────────
    let mut hp = host("b1-prompt", "127.0.0.1", None);
    hp.port = Some(tb.bastion1_port);
    hp.username = Some(s.user.clone());
    hp.host_key_policy = HostKeyPolicy::AcceptNew;
    let hp = app
        .save_host_with_auth(hp, HostAuth::PasswordPrompt)
        .await
        .unwrap();
    let out = app
        .exec(hp.id.clone(), "echo prompt-auth-ok".into())
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("prompt-auth-ok"));
    let hi = app
        .save_host_with_auth(
            hp,
            HostAuth::InlinePassword {
                password: Some(s.password.expose_secret().to_owned()),
            },
        )
        .await
        .unwrap();
    let out = app
        .exec(hi.id.clone(), "echo inline-auth-ok".into())
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("inline-auth-ok"));
    println!("IT password auth (prompt + inline) ... ok");

    // ── interactive terminal ────────────────────────────────────────────
    let term = app.open_terminal(target.id.clone(), 100, 30).await.unwrap();
    assert_eq!(term.status, TerminalStatusDto::Connected);
    let mut output = app.attach_terminal(term.id.clone()).await.unwrap().output;
    app.terminal_write(term.id.clone(), b"echo term-$((40+2))-ok\n".to_vec())
        .await
        .unwrap();
    let mut seen = Vec::new();
    let found = tokio::time::timeout(Duration::from_secs(15), async {
        while let Some(chunk) = output.recv().await {
            if let TerminalChunk::Data(d) = chunk {
                seen.extend_from_slice(&d);
                if String::from_utf8_lossy(&seen).contains("term-42-ok") {
                    return true;
                }
            }
        }
        false
    })
    .await
    .unwrap_or(false);
    assert!(found, "terminal output: {}", String::from_utf8_lossy(&seen));
    app.terminal_resize(term.id.clone(), 120, 40).await.unwrap();
    app.terminal_write(term.id.clone(), b"exit\n".to_vec())
        .await
        .unwrap();
    let st = tokio::time::timeout(
        Duration::from_secs(15),
        app.terminal_wait_closed(term.id.clone()),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(matches!(st, TerminalStatusDto::Closed { .. }), "{st:?}");
    app.close_terminal(term.id).await.unwrap();
    println!("IT terminal over the chain ... ok");

    // ── local tunnel roundtrip: 127.0.0.1:0 -> target -> its own sshd ───
    let tunnel = app
        .save_tunnel(TunnelDto {
            id: String::new(),
            name: "to-target-sshd".into(),
            kind: TunnelKind::Local,
            host_id: target.id.clone(),
            bind_host: "127.0.0.1".into(),
            bind_port: 0,
            target_host: Some("127.0.0.1".into()),
            target_port: Some(22),
            auto_start: false,
            binds_publicly: false,
            created_at_ms: 0,
            updated_at_ms: 0,
        })
        .await
        .unwrap();
    let status = app.start_tunnel(tunnel.id.clone()).await.unwrap();
    assert_eq!(status.state, TunnelStateDto::Running);
    let mut sock = tokio::net::TcpStream::connect(&status.listen)
        .await
        .unwrap();
    sock.write_all(b"SSH-2.0-cc-tunnel-probe\r\n")
        .await
        .unwrap();
    let mut banner = vec![0u8; 64];
    let n = tokio::time::timeout(Duration::from_secs(10), sock.read(&mut banner))
        .await
        .unwrap()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&banner[..n]).starts_with("SSH-2.0-"),
        "banner through the tunnel"
    );
    drop(sock);
    let stats = app.tunnel_statuses().await.unwrap();
    assert!(stats
        .iter()
        .any(|s| s.id == tunnel.id && s.total_connections >= 1));
    app.stop_tunnel(tunnel.id.clone()).await.unwrap();
    println!("IT local tunnel roundtrip ... ok");

    // ── SFTP put / get / ls ─────────────────────────────────────────────
    let sftp = app.sftp_open(target.id.clone()).await.unwrap();
    let home = app.sftp_home(sftp.clone()).await.unwrap();
    let local_src = tmp.path().join("upload.bin");
    let payload: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(&local_src, &payload).unwrap();
    let remote = format!("{home}/cc-app-core-it.bin");
    let up = app
        .sftp_upload(
            sftp.clone(),
            local_src.to_string_lossy().to_string(),
            remote.clone(),
        )
        .await
        .unwrap();
    assert_eq!(up.bytes, payload.len() as u64);
    let listing = app.sftp_list(sftp.clone(), home.clone()).await.unwrap();
    assert!(listing
        .iter()
        .any(|e| e.name == "cc-app-core-it.bin" && !e.is_dir));
    let local_dst = tmp.path().join("download.bin");
    app.sftp_download(
        sftp.clone(),
        remote.clone(),
        local_dst.to_string_lossy().to_string(),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&local_dst).unwrap(), payload);
    app.sftp_remove(sftp.clone(), remote, false).await.unwrap();
    println!("IT sftp put/get over the chain ... ok");

    // ── SFTP browser + edit session against OpenSSH (ADR-0108) ──────────
    let base = format!("{home}/cc-browser-it");
    app.sftp_mkdir(sftp.clone(), format!("{base}/dir"))
        .await
        .unwrap();
    app.sftp_create_file(sftp.clone(), format!("{base}/dir/a.txt"))
        .await
        .unwrap();
    let e = app
        .sftp_create_file(sftp.clone(), format!("{base}/dir/a.txt"))
        .await
        .unwrap_err();
    assert_eq!(e.code(), "already_exists");
    app.exec(
        target.id.clone(),
        format!("printf 'hello openssh' > {base}/dir/a.txt && ln -s a.txt {base}/dir/link"),
    )
    .await
    .unwrap();
    let list = app
        .sftp_list_detailed(sftp.clone(), format!("{base}/dir"))
        .await
        .unwrap();
    let a = list.iter().find(|e| e.name == "a.txt").unwrap();
    assert!(a.owner.is_some(), "owner name from the OpenSSH longname");
    let link = list.iter().find(|e| e.name == "link").unwrap();
    assert_eq!(link.kind, "symlink");
    assert_eq!(link.link_target.as_deref(), Some("a.txt"));
    assert_eq!(link.link_target_kind.as_deref(), Some("file"));
    app.sftp_duplicate(sftp.clone(), format!("{base}/dir"), format!("{base}/copy"))
        .await
        .unwrap();
    let copied = app
        .sftp_stat(sftp.clone(), format!("{base}/copy/link"))
        .await
        .unwrap();
    assert_eq!(
        copied.link_target.as_deref(),
        Some("a.txt"),
        "OpenSSH symlink order"
    );
    let p = app
        .sftp_read_preview(sftp.clone(), format!("{base}/copy/a.txt"), 5)
        .await
        .unwrap();
    assert_eq!(p.data, b"hello");
    assert_eq!(
        app.sftp_resolve_directory(sftp.clone(), "~/cc-browser-it/copy/..".into(), "/".into())
            .await
            .unwrap(),
        base
    );
    let s = app
        .edit_open(
            sftp.clone(),
            format!("{base}/dir/link"),
            OpenWithDto::Default,
        )
        .await
        .unwrap();
    assert!(s.target_path.ends_with("/dir/a.txt"), "symlink resolved");
    std::fs::write(&s.local_path, b"edited via app-core").unwrap();
    app.edit_sync_now(s.id.clone()).await.unwrap();
    let out = app
        .exec(target.id.clone(), format!("cat {base}/dir/a.txt"))
        .await
        .unwrap();
    assert_eq!(out.stdout, b"edited via app-core");
    let stopped = app
        .edit_stop(s.id.clone(), EditStopModeDto::Upload)
        .await
        .unwrap();
    assert_eq!(stopped, EditStopOutcomeDto::Closed { uploaded: false });
    app.sftp_remove(sftp.clone(), base.clone(), true)
        .await
        .unwrap();
    app.sftp_close(sftp).await.unwrap();
    println!("IT sftp browser + edit session over the chain ... ok");

    // ── OpenSSH fallback (system ssh + built-in agent), if available ────
    if cc_ssh_core::openssh::detect_openssh().is_some() {
        let mut t2 = app.get_host(target.id.clone()).await.unwrap();
        t2.backend = SshBackend::OpenSsh;
        app.save_host(t2).await.unwrap();
        let out = app
            .exec(target.id.clone(), "echo openssh-fallback-ok".into())
            .await
            .unwrap();
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("openssh-fallback-ok"),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        println!("IT OpenSSH fallback exec ... ok");
    } else {
        println!("IT OpenSSH fallback ... SKIPPED (no OpenSSH client)");
    }

    app.shutdown().await.unwrap();
    prompt_task.abort();
    assert!(
        asked.load(std::sync::atomic::Ordering::SeqCst) >= 1,
        "the target's unknown key was asked through the UI prompt"
    );
}
