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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn port_check_runs_on_the_selected_ssh_host() {
    if !testing::enabled() {
        return;
    }
    let tb = tokio::task::spawn_blocking(|| SshTestbed::start(false)).await.unwrap().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let app = app_with(tmp.path(), memory_store(), false);
    app.create_local_profile("Port check IT".into(), PASSPHRASE.into()).await.unwrap();
    let mut source = host("source", "127.0.0.1", None);
    source.port = Some(tb.bastion1_port);
    source.username = Some(tb.secrets.user.clone());
    source.host_key_policy = HostKeyPolicy::AcceptNew;
    let saved = app.save_host_with_auth(source, HostAuth::InlinePassword {
        password: Some(tb.secrets.password.expose_secret().to_owned()),
    }).await.unwrap();
    let check = |port, udp| app.port_check(Some(saved.id.clone()), "127.0.0.1".into(), port, udp);
    if tb.exec_in(&tb.bastion1, "command -v python3").is_err() {
        assert_eq!(check(22, false).await.unwrap(), "no_python");
        tb.exec_in(&tb.bastion1, "apk add --no-cache python3").unwrap();
    }
    assert_eq!(check(22, false).await.unwrap(), "connected");
    assert_eq!(check(1, false).await.unwrap(), "refused");
    let port = tb.exec_in(&tb.bastion1, r#"python3 -c 'import socket; s=socket.socket(type=socket.SOCK_DGRAM); s.bind(("127.0.0.1",0)); print(s.getsockname()[1],flush=True); s.settimeout(15); data,peer=s.recvfrom(1); s.sendto(data,peer)' </dev/null >/tmp/cc-port-check-test 2>/dev/null &
for i in 1 2 3 4 5; do test -s /tmp/cc-port-check-test && break; sleep 1; done
cat /tmp/cc-port-check-test"#).unwrap();
    assert_eq!(check(port.trim().parse().unwrap(), true).await.unwrap(), "reply");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_metrics_sample_on_the_connected_ssh_transport_leaves_pty_untouched() {
    if !testing::enabled() {
        return;
    }
    let tb = tokio::task::spawn_blocking(|| SshTestbed::start(false))
        .await
        .unwrap()
        .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let app = app_with(tmp.path(), memory_store(), false);
    app.create_local_profile("Metrics IT".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let mut host = host("testbed", "127.0.0.1", None);
    host.port = Some(tb.bastion1_port);
    host.username = Some(tb.secrets.user.clone());
    host.host_key_policy = HostKeyPolicy::AcceptNew;
    let host = app
        .save_host_with_auth(
            host,
            HostAuth::InlinePassword {
                password: Some(tb.secrets.password.expose_secret().to_owned()),
            },
        )
        .await
        .unwrap();
    let term = app.open_terminal(host.id.clone(), 80, 24).await.unwrap();
    let scope = app.list_profiles().await.unwrap().pop().unwrap();
    let request = serde_json::json!({
        "operationId": uuid::Uuid::new_v4(), "hostId": host.id, "terminalId": term.id,
        "hostUpdatedAtMs": host.updated_at_ms, "profileId": scope.id, "notificationScopeId": scope.notification_scope_id.unwrap()
    }).to_string();
    let mut samples = Vec::new();
    tokio::time::timeout(
        Duration::from_secs(20),
        app.host_metrics_watch(&request, |sample| {
            samples.push(sample);
            samples.len() < 2
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(samples[0]["cpuPercent"].is_null());
    assert!((0.0..=100.0).contains(&samples[1]["cpuPercent"].as_f64().unwrap()));
    assert!(samples[1]["memoryUsed"].as_u64().unwrap() > 0);
    assert!(samples[1]["diskTotal"].as_u64().unwrap() > 0);
    assert!(samples[1]["os"].is_string());
    assert!(samples[1]["receivedPerSecond"].is_number());
    let pty = app.attach_terminal(term.id.clone()).await.unwrap();
    assert!(!String::from_utf8_lossy(&pty.snapshot).contains("/proc/stat"));
    assert_eq!(
        app.terminal_info(term.id.clone()).await.unwrap().status,
        TerminalStatusDto::Connected
    );
    app.close_terminal(term.id).await.unwrap();
}

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
                PromptRequest::AutomationTransition(p) => {
                    let _ = responder
                        .automation_answer_transition(p.request_id, "abort".into())
                        .await;
                }
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
    // A lock (the idle auto-lock) keeps the tunnel forwarding (BUG-01).
    app.lock().await.unwrap();
    let mut sock = tokio::net::TcpStream::connect(&status.listen)
        .await
        .unwrap();
    sock.write_all(b"SSH-2.0-cc-tunnel-probe\r\n")
        .await
        .unwrap();
    let n = tokio::time::timeout(Duration::from_secs(10), sock.read(&mut banner))
        .await
        .unwrap()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&banner[..n]).starts_with("SSH-2.0-"),
        "banner through the tunnel while locked"
    );
    drop(sock);
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert!(app
        .tunnel_statuses()
        .await
        .unwrap()
        .iter()
        .any(|s| s.id == tunnel.id && s.state == TunnelStateDto::Running));
    app.stop_tunnel(tunnel.id.clone()).await.unwrap();
    println!("IT local tunnel roundtrip, also across a lock ... ok");

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
    // The browser's own session stays open after its edit session ends.
    app.sftp_home(sftp.clone()).await.unwrap();

    // Edit sessions on a host the browser is not showing share one pooled
    // session (TODO-03), closed with the last of them.
    let pooled = app.edit_connect(target.id.clone()).await.unwrap();
    assert_ne!(pooled, sftp);
    assert_eq!(app.edit_connect(target.id.clone()).await.unwrap(), pooled);
    let first = app
        .edit_open(
            pooled.clone(),
            format!("{base}/dir/a.txt"),
            OpenWithDto::Default,
        )
        .await
        .unwrap();
    let second = app
        .edit_open(
            pooled.clone(),
            format!("{base}/copy/a.txt"),
            OpenWithDto::Default,
        )
        .await
        .unwrap();
    app.edit_stop(first.id, EditStopModeDto::Upload)
        .await
        .unwrap();
    app.sftp_home(pooled.clone())
        .await
        .expect("still used by the second");
    app.edit_stop(second.id, EditStopModeDto::Upload)
        .await
        .unwrap();
    assert!(
        app.sftp_home(pooled.clone()).await.is_err(),
        "closed with its last edit session"
    );
    let again = app.edit_connect(target.id.clone()).await.unwrap();
    assert_ne!(again, pooled, "a closed session is not handed out");
    app.sftp_close(again).await.unwrap();
    println!("IT pooled sftp session for edit sessions ... ok");
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

/// Exercise the same facade, snapshot and streaming channel as the desktop client.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tcpdump_check_denied_capture_and_stop_over_real_ssh() {
    if !testing::enabled() {
        return;
    }
    let tb = tokio::task::spawn_blocking(|| SshTestbed::start(false))
        .await
        .unwrap()
        .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let app = app_with(tmp.path(), memory_store(), false);
    app.create_local_profile("Capture IT".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let mut source = host("source", "127.0.0.1", None);
    source.port = Some(tb.bastion1_port);
    source.username = Some(tb.secrets.user.clone());
    source.host_key_policy = HostKeyPolicy::AcceptNew;
    let source = app
        .save_host_with_auth(
            source,
            HostAuth::InlinePassword {
                password: Some(tb.secrets.password.expose_secret().to_owned()),
            },
        )
        .await
        .unwrap();
    let peer = app
        .save_host(host("peer", "127.0.0.2", None))
        .await
        .unwrap();

    async fn finished(app: &AppCore, id: String) -> String {
        tokio::time::timeout(Duration::from_secs(8), app.terminal_wait_closed(id.clone()))
            .await
            .unwrap()
            .unwrap();
        let mut attachment = app.attach_terminal(id.clone()).await.unwrap();
        let mut data = attachment.snapshot;
        while let Some(chunk) = attachment.output.recv().await {
            if let TerminalChunk::Data(bytes) = chunk {
                data.extend(bytes);
            }
        }
        app.close_terminal(id).await.unwrap();
        String::from_utf8(data).unwrap()
    }

    let check = app
        .open_tcpdump(source.id.clone(), None, false)
        .await
        .unwrap();
    assert!(finished(&app, check.id).await.contains("CC_MISSING apk"));
    tb.exec_in(&tb.bastion1, "apk add --no-cache tcpdump iproute2 sudo")
        .unwrap();
    let check = app
        .open_tcpdump(source.id.clone(), None, false)
        .await
        .unwrap();
    assert!(finished(&app, check.id).await.contains("CC_TCPDUMP_READY"));
    let denied = app
        .open_tcpdump(source.id.clone(), Some(peer.id.clone()), false)
        .await
        .unwrap();
    assert!(
        finished(&app, denied.id)
            .await
            .to_lowercase()
            .contains("permission")
    );
    async fn read_until(stream: &mut TerminalAttachment, text: &mut String, marker: &str) {
        tokio::time::timeout(Duration::from_secs(12), async {
            while !text.contains(marker) {
                match stream.output.recv().await {
                    Some(TerminalChunk::Data(bytes)) => {
                        text.push_str(&String::from_utf8_lossy(&bytes))
                    }
                    _ => panic!("capture ended before the expected prompt or status"),
                }
            }
        })
        .await
        .expect("capture prompt or status timed out");
    }

    tb.exec_in(&tb.bastion1, "printf 'tester ALL=(root) PASSWD: %s\\n' \"$(command -v tcpdump)\" >/etc/sudoers.d/capture-it && chmod 440 /etc/sudoers.d/capture-it").unwrap();
    for cancel in [true, false] {
        let capture = app
            .open_tcpdump(source.id.clone(), Some(peer.id.clone()), true)
            .await
            .unwrap();
        let mut stream = app.attach_terminal(capture.id.clone()).await.unwrap();
        let mut text = String::from_utf8(std::mem::take(&mut stream.snapshot)).unwrap();
        read_until(&mut stream, &mut text, "CC_TCPDUMP_SUDO_PASSWORD\r\n").await;
        if !cancel {
            text.clear();
            app.terminal_write(capture.id.clone(), b"test-incorrect-password\n".to_vec())
                .await
                .unwrap();
            read_until(&mut stream, &mut text, "CC_TCPDUMP_SUDO_PASSWORD\r\n").await;
            assert!(
                !text.contains("test-incorrect-password"),
                "sudo echoed its input"
            );
            let secret = tb.secrets.password.expose_secret();
            let mut input = secret.as_bytes().to_vec();
            input.push(b'\n');
            app.terminal_write(capture.id.clone(), input).await.unwrap();
            read_until(&mut stream, &mut text, "listening on").await;
            assert!(!text.contains(secret), "sudo echoed its input");
        }
        app.close_terminal(capture.id).await.unwrap();
    }
    tb.exec_in(&tb.bastion1, "printf 'tester ALL=(root) NOPASSWD: %s\\n' \"$(command -v tcpdump)\" >/etc/sudoers.d/capture-it && chmod 440 /etc/sudoers.d/capture-it").unwrap();
    let capture = app
        .open_tcpdump(source.id, Some(peer.id), true)
        .await
        .unwrap();
    assert!(app.list_terminals().await.unwrap().is_empty());
    let mut stream = app.attach_terminal(capture.id.clone()).await.unwrap();
    let mut text = String::from_utf8(stream.snapshot).unwrap();
    tokio::time::timeout(Duration::from_secs(8), async {
        let mut generated = false;
        loop {
            if !generated && text.contains("listening on") {
                tb.exec_in(
                    &tb.bastion1,
                    "ping -qc 1 -I 127.0.0.1 127.0.0.3 && ping -qc 2 -I 127.0.0.1 127.0.0.2",
                )
                .unwrap();
                generated = true;
            }
            if text.contains("127.0.0.2 > 127.0.0.1: ICMP echo reply") {
                break;
            }
            match stream.output.recv().await {
                Some(TerminalChunk::Data(bytes)) => text.push_str(&String::from_utf8_lossy(&bytes)),
                other => panic!("unexpected capture end {other:?}: {text}"),
            }
        }
    })
    .await
    .unwrap();
    assert!(text.contains("127.0.0.1 > 127.0.0.2: ICMP echo request"));
    assert!(!text.contains("127.0.0.3"));
    assert!(text.contains("ethertype IPv4") && text.contains("0x0000:"));
    app.close_terminal(capture.id).await.unwrap();
    tb.exec_in(
        &tb.bastion1,
        "for i in 1 2 3 4 5; do pgrep -x tcpdump >/dev/null || exit 0; sleep 1; done; exit 1",
    )
    .unwrap();
}
