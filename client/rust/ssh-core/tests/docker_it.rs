//! Docker integration tests for the native SSH backend (CLIENT_SPEC §19).
//!
//! Run with `CC_SSH_IT=1 cargo test -p cc-ssh-core --test docker_it -- --nocapture`.
//! Without `CC_SSH_IT=1` the test returns immediately (plain `cargo test`
//! stays fast and needs no Docker).

use cc_models::host::HostKeyPolicy;
use cc_models::known_host::KnownHostSource;
use cc_ssh_core::known_hosts::new_known_host;
use cc_ssh_core::testing::{self, connector, SshTestbed, TestInventory};
use cc_ssh_core::{
    ConnectOptions, ConnectionPlanner, HostKeyDecision, KnownHostsStore, MemoryKnownHosts,
    PlannerDefaults, PtyRequest, ResolvedCredential, SessionEvent, ShellEvent, SshError,
};
use secrecy::SecretString;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

fn planner(ti: &TestInventory) -> ConnectionPlanner {
    ConnectionPlanner::new(ti.inventory.clone()).with_defaults(PlannerDefaults::default())
}

async fn scenario<F>(name: &str, failures: &mut Vec<String>, fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    let started = std::time::Instant::now();
    match tokio::spawn(fut).await {
        Ok(()) => println!("IT {name} ... ok ({:.1}s)", started.elapsed().as_secs_f32()),
        Err(e) => {
            println!("IT {name} ... FAILED: {e}");
            failures.push(name.to_string());
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn docker_ssh_core() {
    if !testing::enabled() {
        eprintln!("skipped: set CC_SSH_IT=1 to run Docker integration tests");
        return;
    }
    let tb = tokio::task::spawn_blocking(|| SshTestbed::start(true))
        .await
        .unwrap()
        .expect("testbed");
    let tb = Arc::new(tb);
    let ti = tb.inventory();
    let mut failures = Vec::new();

    // 1. key auth, direct
    {
        let ti = ti.clone();
        scenario("direct_key_auth_exec", &mut failures, async move {
            let kh = Arc::new(MemoryKnownHosts::new());
            let (c, _) = connector(ti.resolver.clone(), kh.clone(), HostKeyDecision::Reject);
            let plan = planner(&ti).plan(ti.bastion1).await.unwrap();
            let s = c.connect(&plan).await.unwrap();
            let out = s.exec("echo hello; whoami").await.unwrap();
            assert_eq!(out.stdout_lossy(), "hello\ntester\n");
            assert_eq!(out.exit_status, Some(0));
            let out = s.exec("exit 3").await.unwrap();
            assert_eq!(out.exit_status, Some(3));
            // AcceptNew recorded the key
            assert_eq!(kh.entries().len(), 1);
            assert_eq!(kh.entries()[0].source, KnownHostSource::Tofu);
            s.disconnect().await.unwrap();
        })
        .await;
    }

    // 2. password auth + wrong password
    {
        let ti = ti.clone();
        let tb = tb.clone();
        scenario("password_auth", &mut failures, async move {
            ti.inventory
                .update_host(ti.bastion1, |h| h.credential_id = Some(ti.cred_password));
            let kh = Arc::new(MemoryKnownHosts::new());
            let (c, _) = connector(ti.resolver.clone(), kh, HostKeyDecision::Reject);
            let plan = planner(&ti).plan(ti.bastion1).await.unwrap();
            let s = c.connect(&plan).await.unwrap();
            assert_eq!(s.exec("whoami").await.unwrap().stdout_lossy(), "tester\n");

            ti.resolver.insert(
                ti.cred_password,
                ResolvedCredential::Password(SecretString::from("definitely-wrong")),
            );
            let err = c.connect(&plan).await.unwrap_err();
            assert!(matches!(err, SshError::AuthFailed { .. }), "{err}");
            assert!(!err.to_string().contains("definitely-wrong"));
            ti.resolver.insert(
                ti.cred_password,
                ResolvedCredential::Password(tb.secrets.password.clone()),
            );
            ti.inventory
                .update_host(ti.bastion1, |h| h.credential_id = Some(ti.cred_key));
        })
        .await;
    }

    // 3. encrypted key: missing / prompted / remembered passphrase
    {
        let ti = ti.clone();
        let tb = tb.clone();
        scenario("encrypted_key_auth", &mut failures, async move {
            let inv = ti.inventory.clone();
            let mut h = cc_models::host::Host::new("enc", "127.0.0.1");
            h.port = Some(tb.bastion1_port);
            h.username = Some("tester".into());
            h.credential_id = Some(ti.cred_encrypted);
            h.host_key_policy = HostKeyPolicy::AcceptNew;
            let hid = inv.add_host(h);
            let kh = Arc::new(MemoryKnownHosts::new());
            let (c, _) = connector(ti.resolver.clone(), kh, HostKeyDecision::Reject);
            let plan = planner(&ti).plan(hid).await.unwrap();
            let err = c.connect(&plan).await.unwrap_err();
            assert!(matches!(err, SshError::PassphraseRequired { .. }), "{err}");

            ti.resolver
                .set_prompt_passphrase(ti.cred_encrypted, tb.secrets.encrypted_passphrase.clone());
            let s = c.connect(&plan).await.unwrap();
            assert_eq!(s.exec("whoami").await.unwrap().stdout_lossy(), "tester\n");

            ti.resolver.insert(
                ti.cred_encrypted,
                ResolvedCredential::PrivateKey {
                    openssh: tb.secrets.encrypted_key.private_openssh.clone(),
                    passphrase: Some(tb.secrets.encrypted_passphrase.clone()),
                    certificate: None,
                },
            );
            let s = c.connect(&plan).await.unwrap();
            assert_eq!(s.exec("whoami").await.unwrap().stdout_lossy(), "tester\n");
        })
        .await;
    }

    // 4. user certificate auth (key not in authorized_keys)
    {
        let ti = ti.clone();
        let tb = tb.clone();
        scenario("certificate_auth", &mut failures, async move {
            let mut h = cc_models::host::Host::new("cert", "127.0.0.1");
            h.port = Some(tb.bastion1_port);
            h.username = Some("tester".into());
            h.credential_id = Some(ti.cred_cert);
            h.host_key_policy = HostKeyPolicy::AcceptNew;
            let hid = ti.inventory.add_host(h);
            let kh = Arc::new(MemoryKnownHosts::new());
            let (c, _) = connector(ti.resolver.clone(), kh, HostKeyDecision::Reject);
            let plan = planner(&ti).plan(hid).await.unwrap();
            let s = c.connect(&plan).await.unwrap();
            assert_eq!(s.exec("whoami").await.unwrap().stdout_lossy(), "tester\n");
        })
        .await;
    }

    // 5. host key policies: strict/unknown, ask/reject, ask/accept+save
    {
        let ti = ti.clone();
        scenario("host_key_unknown_policies", &mut failures, async move {
            let plan_for = |policy| {
                let ti = ti.clone();
                async move {
                    ti.inventory
                        .update_host(ti.bastion1, |h| h.host_key_policy = policy);
                    planner(&ti).plan(ti.bastion1).await.unwrap()
                }
            };
            let kh = Arc::new(MemoryKnownHosts::new());
            let (c, _) = connector(ti.resolver.clone(), kh.clone(), HostKeyDecision::Reject);
            let err = c
                .connect(&plan_for(HostKeyPolicy::Strict).await)
                .await
                .unwrap_err();
            assert!(matches!(err, SshError::HostKeyUnknown { .. }), "{err}");
            let err = c
                .connect(&plan_for(HostKeyPolicy::Ask).await)
                .await
                .unwrap_err();
            assert!(matches!(err, SshError::HostKeyRejected { .. }), "{err}");
            assert!(kh.entries().is_empty());

            let (c, prompt) = connector(
                ti.resolver.clone(),
                kh.clone(),
                HostKeyDecision::AcceptAndSave,
            );
            let plan = plan_for(HostKeyPolicy::Ask).await;
            c.connect(&plan).await.unwrap();
            assert_eq!(prompt.prompts().len(), 1);
            let p = &prompt.prompts()[0];
            assert!(p.fingerprint_sha256.starts_with("SHA256:"));
            assert_eq!(p.hop_count, 1);
            assert_eq!(kh.entries().len(), 1);
            // known now: no second prompt, strict passes
            c.connect(&plan).await.unwrap();
            assert_eq!(prompt.prompts().len(), 1);
            c.connect(&plan_for(HostKeyPolicy::Strict).await)
                .await
                .unwrap();
            ti.inventory.update_host(ti.bastion1, |h| {
                h.host_key_policy = HostKeyPolicy::AcceptNew
            });
        })
        .await;
    }

    // 6. host certificate trusted via @cert-authority under Strict
    {
        let ti = ti.clone();
        let tb = tb.clone();
        scenario("host_certificate_authority", &mut failures, async move {
            let ca_line = &tb.secrets.host_ca_public;
            let mut parts = ca_line.split_whitespace();
            let (kt, b64) = (parts.next().unwrap(), parts.next().unwrap());
            use base64::Engine as _;
            let blob = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .unwrap();
            let mut ca = new_known_host("x", 22, kt, &blob, KnownHostSource::CertAuthority);
            ca.host_pattern = "*".into();
            let kh = Arc::new(MemoryKnownHosts::with_entries(vec![ca]));
            let (c, prompt) = connector(ti.resolver.clone(), kh.clone(), HostKeyDecision::Reject);
            ti.inventory
                .update_host(ti.bastion1, |h| h.host_key_policy = HostKeyPolicy::Strict);
            let plan = planner(&ti).plan(ti.bastion1).await.unwrap();
            let r = c.connect(&plan).await;
            ti.inventory.update_host(ti.bastion1, |h| {
                h.host_key_policy = HostKeyPolicy::AcceptNew
            });
            let s = r.unwrap();
            assert!(prompt.prompts().is_empty());
            assert_eq!(kh.entries().len(), 1, "CA trust must not add plain entries");
            s.exec("true").await.unwrap();

            // revoke the CA → hard failure
            let fp = kh.entries()[0].fingerprint_sha256.clone();
            kh.revoke(&fp).await.unwrap();
            ti.inventory
                .update_host(ti.bastion1, |h| h.host_key_policy = HostKeyPolicy::Strict);
            let err = c.connect(&plan).await.unwrap_err();
            ti.inventory.update_host(ti.bastion1, |h| {
                h.host_key_policy = HostKeyPolicy::AcceptNew
            });
            assert!(err.is_host_key_error(), "{err}");
        })
        .await;
    }

    // 7. two-hop jump chain to the internal target
    {
        let ti = ti.clone();
        let tb = tb.clone();
        scenario("two_hop_jump_chain", &mut failures, async move {
            let kh = Arc::new(MemoryKnownHosts::new());
            let (c, _) = connector(ti.resolver.clone(), kh.clone(), HostKeyDecision::Reject);
            let plan = planner(&ti).plan(ti.target.unwrap()).await.unwrap();
            assert_eq!(plan.route.len(), 2);
            let s = c.connect(&plan).await.unwrap();
            assert_eq!(s.hop_count(), 3);
            let out = s.exec("hostname").await.unwrap();
            assert_eq!(out.stdout_lossy().trim(), tb.target.as_deref().unwrap());
            // one known-host entry per hop
            assert_eq!(kh.entries().len(), 3);
            // The target is on an internal network: bastion1 cannot reach it.
            let direct = tb
                .exec_in(
                    &tb.bastion1,
                    &format!(
                        "nc -z -w 2 {} 22 && echo reachable || echo unreachable",
                        tb.target.as_deref().unwrap()
                    ),
                )
                .unwrap();
            assert_eq!(direct.trim(), "unreachable");
        })
        .await;
    }

    // 8. recursive route: target's chain = [bastion2] only, bastion2 → [bastion1]
    {
        let ti = ti.clone();
        let tb = tb.clone();
        scenario("recursive_jump_route", &mut failures, async move {
            let b1 = ti.bastion1;
            let b2 = ti.bastion2.unwrap();
            ti.inventory.update_host(b2, |h| h.jump_chain = vec![b1]);
            let mut t = cc_models::host::Host::new("target-2", tb.target.as_deref().unwrap());
            t.username = Some("tester".into());
            t.credential_id = Some(ti.cred_password);
            t.jump_chain = vec![b2];
            t.host_key_policy = HostKeyPolicy::AcceptNew;
            let tid = ti.inventory.add_host(t);
            let plan = planner(&ti).plan(tid).await.unwrap();
            ti.inventory.update_host(b2, |h| h.jump_chain = vec![]);
            assert_eq!(
                plan.route.iter().map(|h| h.host_id).collect::<Vec<_>>(),
                vec![b1, b2]
            );
            let kh = Arc::new(MemoryKnownHosts::new());
            let (c, _) = connector(ti.resolver.clone(), kh, HostKeyDecision::Reject);
            let s = c.connect(&plan).await.unwrap();
            assert_eq!(s.exec("whoami").await.unwrap().stdout_lossy(), "tester\n");
        })
        .await;
    }

    // 9. PTY shell over the jump chain: TERM, resize, exit status
    {
        let ti = ti.clone();
        scenario("pty_shell_over_jumps", &mut failures, async move {
            let kh = Arc::new(MemoryKnownHosts::new());
            let (c, _) = connector(ti.resolver.clone(), kh, HostKeyDecision::Reject);
            let plan = planner(&ti).plan(ti.target.unwrap()).await.unwrap();
            let s = c.connect(&plan).await.unwrap();
            let sh = s
                .open_shell(PtyRequest {
                    cols: 100,
                    rows: 30,
                    ..Default::default()
                })
                .await
                .unwrap();
            let (w, mut r) = sh.split();
            w.resize(120, 40).await.unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
            w.write(&b"echo TERM=$TERM; stty size; exit 7\n"[..])
                .await
                .unwrap();
            let mut out = Vec::new();
            let mut status = None;
            let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
            loop {
                match tokio::time::timeout_at(deadline, r.recv()).await {
                    Ok(Some(ShellEvent::Data(d))) => out.extend_from_slice(&d),
                    Ok(Some(ShellEvent::ExitStatus(c))) => status = Some(c),
                    Ok(Some(ShellEvent::Closed)) | Ok(None) => break,
                    Ok(Some(_)) => {}
                    Err(_) => panic!("timeout; output so far: {}", String::from_utf8_lossy(&out)),
                }
            }
            let text = String::from_utf8_lossy(&out);
            assert!(text.contains("TERM=xterm-256color"), "{text}");
            assert!(text.contains("40 120"), "{text}");
            assert_eq!(status, Some(7));
        })
        .await;
    }

    // 10. disconnect events (reconnect UX)
    {
        let ti = ti.clone();
        scenario("disconnect_events", &mut failures, async move {
            let kh = Arc::new(MemoryKnownHosts::new());
            let (c, _) = connector(ti.resolver.clone(), kh, HostKeyDecision::Reject);
            let plan = planner(&ti).plan(ti.target.unwrap()).await.unwrap();
            let s = c.connect(&plan).await.unwrap();
            let mut ev = s.events();
            assert!(!s.is_closed());
            s.disconnect().await.unwrap();
            tokio::time::timeout(Duration::from_secs(5), s.closed())
                .await
                .expect("closed");
            assert!(s.is_closed());
            let mut saw = false;
            while let Ok(e) = ev.try_recv() {
                if matches!(e, SessionEvent::Disconnected { .. }) {
                    saw = true;
                }
            }
            assert!(saw);
            assert!(matches!(s.exec("true").await, Err(SshError::Disconnected)));
        })
        .await;
    }

    // 11. host key changed = hard failure (separate container)
    {
        scenario(
            "host_key_changed_hard_failure_and_keepalive",
            &mut failures,
            async move {
                let single = tokio::task::spawn_blocking(|| SshTestbed::start(false))
                    .await
                    .unwrap()
                    .expect("testbed");
                let ti = single.inventory();
                let kh = Arc::new(MemoryKnownHosts::new());
                let (c, _) = connector(
                    ti.resolver.clone(),
                    kh.clone(),
                    HostKeyDecision::AcceptAndSave,
                );
                let plan = planner(&ti).plan(ti.bastion1).await.unwrap();
                c.connect(&plan).await.unwrap().disconnect().await.unwrap();
                assert_eq!(kh.entries().len(), 1);
                single.rotate_host_keys(&single.bastion1).unwrap();
                for policy in [
                    HostKeyPolicy::AcceptNew,
                    HostKeyPolicy::Ask,
                    HostKeyPolicy::Strict,
                ] {
                    ti.inventory
                        .update_host(ti.bastion1, |h| h.host_key_policy = policy);
                    let plan = planner(&ti).plan(ti.bastion1).await.unwrap();
                    let err = c.connect(&plan).await.unwrap_err();
                    match &err {
                        SshError::HostKeyChanged {
                            expected_fingerprints,
                            actual_fingerprint,
                            ..
                        } => {
                            assert_eq!(
                                expected_fingerprints,
                                &vec![kh.entries()[0].fingerprint_sha256.clone()]
                            );
                            assert_ne!(actual_fingerprint, &kh.entries()[0].fingerprint_sha256);
                        }
                        other => panic!("{policy:?}: expected HostKeyChanged, got {other}"),
                    }
                    assert!(err.to_string().contains("CHANGED"));
                }
                // nothing was overwritten
                assert_eq!(kh.entries().len(), 1);

                // keepalive: a frozen peer (no FIN) is detected by keepalives
                ti.inventory.update_host(ti.bastion1, |h| {
                    h.host_key_policy = HostKeyPolicy::AcceptNew
                });
                let kh2 = Arc::new(MemoryKnownHosts::new());
                let (c2, _) = connector(ti.resolver.clone(), kh2, HostKeyDecision::Reject);
                let c2 = c2.with_options(ConnectOptions {
                    default_keepalive: Some(Duration::from_secs(1)),
                    keepalive_max: 2,
                    ..Default::default()
                });
                let plan = planner(&ti).plan(ti.bastion1).await.unwrap();
                let s = c2.connect(&plan).await.unwrap();
                let mut ev = s.events();
                let name = single.bastion1.clone();
                tokio::task::spawn_blocking(move || {
                    std::process::Command::new("docker")
                        .args(["pause", &name])
                        .output()
                })
                .await
                .unwrap()
                .unwrap();
                let t0 = std::time::Instant::now();
                tokio::time::timeout(Duration::from_secs(20), s.closed())
                    .await
                    .expect("keepalive should detect the frozen peer");
                let mut reason = String::new();
                while let Ok(e) = ev.try_recv() {
                    if let SessionEvent::Disconnected {
                        reason: r, error, ..
                    } = e
                    {
                        assert!(error);
                        reason = r;
                    }
                }
                println!(
                    "   keepalive detected dead peer after {:.1}s: {reason}",
                    t0.elapsed().as_secs_f32()
                );
                let name = single.bastion1.clone();
                let _ = std::process::Command::new("docker")
                    .args(["unpause", &name])
                    .output();
                drop(single);
            },
        )
        .await;
    }

    drop(tb);
    assert!(failures.is_empty(), "failed scenarios: {failures:?}");
}
