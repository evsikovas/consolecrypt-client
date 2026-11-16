//! Draft negative lifecycle tests; no fake Permit, TLS peer or Actor acceptance.
use super::*;
use ed25519_dalek::{Signer, SigningKey};
use std::io::{BufRead, BufReader};
use std::process::ChildStdout;
use std::sync::atomic::Ordering;

#[test]
fn supervised_stop_empty_struct_rejects_caller_join_claims_and_parent_eof() {
    assert!(pipe::stop_frame(br#"{"action":"stop"}"#).is_ok());
    assert_eq!(
        pipe::stop_frame(br#"{"action":"stop","joined":true}"#),
        Err(Error::InvalidWire)
    );
    assert_eq!(
        pipe::stop_frame(br#"{"action":"stop","drainRecorded":true}"#),
        Err(Error::InvalidWire)
    );
    assert!(pipe::stop_frame(b"").is_err());
}

#[test]
fn supervised_pipe_session_sequence_and_nested_duplicate_fail_closed() {
    let frame = json!({"format":"cc-tunnel-supervised/v1","session":"11111111111111111111111111111111","seq":1,"kind":"idle"});
    assert_eq!(
        pipe::response(
            &frame,
            "22222222222222222222222222222222",
            1,
            &[("idle", &[])]
        ),
        Err(Error::Replay)
    );
    assert_eq!(
        pipe::response(
            &frame,
            "11111111111111111111111111111111",
            2,
            &[("idle", &[])]
        ),
        Err(Error::Replay)
    );
    let raw = b"{\"binding\":{\"nonce\":1,\"nonce\":2}}\n";
    assert_eq!(pipe::parse(raw), Err(Error::InvalidWire));
    let claimed = json!({"format":"cc-tunnel-supervised/v1","session":"11111111111111111111111111111111","seq":1,"kind":"stopped","joined":true});
    assert_eq!(
        pipe::response(
            &claimed,
            "11111111111111111111111111111111",
            1,
            &[("stopped", &[])]
        ),
        Err(Error::InvalidWire)
    );
}

#[test]
fn supervised_pending_signed_nonce_policy_device_and_peer_must_all_match() {
    let route: TunnelRoutePolicy =
        serde_json::from_str(include_str!("../tunnel_policy_vector.json")).unwrap();
    let body = AgentRequest {
        header: Header {
            version: 1,
            context: route.context.clone(),
            owner_epoch: 1,
            checkpoint: 2,
        },
        agent_id: route.agent_id,
        key_epoch: 1,
        lease_hash: Hash::from_bytes([9; 32]),
        nonce: Nonce::from_bytes([3; 16]),
        issued_at: Timestamp::new(1000).unwrap(),
    };
    let key = SigningKey::from_bytes(&[23; 32]);
    let signature = SignatureBytes::from_bytes(key.sign(&body.signing_bytes().unwrap()).to_bytes());
    let proof = Signed { body, signature };
    let hash = Hash::from_bytes([8; 32]);
    let peer = serde_json::to_value(route.client_tls_certificate_sha256).unwrap();
    let exact = json!({"nonce":proof.body.nonce,"policyHash":hash,"leaseHash":proof.body.lease_hash,
        "deviceId":route.client_device_id,"peerSha256":peer});
    assert!(verify_echo(&exact, &proof, hash, &route.client_device_id, &peer).is_ok());
    let mut requested = exact.clone();
    requested["format"] = json!("cc-tunnel-supervised/v1");
    requested["session"] = json!("11111111111111111111111111111111");
    requested["seq"] = json!(1);
    requested["kind"] = json!("stop_requested");
    let replies = [("stop_requested", &["nonce", "policyHash", "leaseHash", "deviceId", "peerSha256"][..])];
    assert!(pipe::response(&requested, "11111111111111111111111111111111", 1, &replies).is_ok());
    requested["drainRecorded"] = json!(true);
    assert_eq!(pipe::response(&requested, "11111111111111111111111111111111", 1, &replies), Err(Error::InvalidWire));
    for name in ["nonce", "policyHash", "leaseHash", "deviceId", "peerSha256"] {
        let mut wrong = exact.clone();
        wrong[name] = json!("different");
        assert_eq!(
            verify_echo(&wrong, &proof, hash, &route.client_device_id, &peer),
            Err(Error::WrongClaim)
        );
    }
}

fn owned_fixture(mode: &str) -> (ChildOwner, BufReader<ChildStdout>) {
    // TEST-ONLY owned actual Rust test child. Production never accepts this
    // executable or argv; InstalledChild derives its one fixed Python command.
    let mut child = ChildOwner(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "tunnel_supervisor::tests::owned_pipe_child_fixture",
                "--nocapture",
            ])
            .env_clear()
            .env("CC_SUPERVISED_PRIVATE_TEST", mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    if matches!(mode,"remote-partial"|"network-partial") {
        child.stdin.as_mut().unwrap().write_all(b"G{\"action\":").unwrap();
    } else if matches!(mode,"remote-malformed"|"network-malformed") {
        child.stdin.as_mut().unwrap().write_all(b"G{\"action\":\"stop\",\"joined\":true}\n").unwrap();
    }
    let stdout = child.stdout.take().unwrap();
    let reader = thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut total = 0;
        loop {
            let mut line = String::new();
            let n = reader.read_line(&mut line).unwrap();
            total += n;
            assert!(n > 0 && total <= 4096);
            if line.trim() == "CC_PRIVATE_CHILD_READY" {
                return reader;
            }
        }
    });
    let end = Instant::now() + Duration::from_secs(3);
    while !reader.is_finished() && Instant::now() < end {
        thread::sleep(Duration::from_millis(5));
    }
    if !reader.is_finished() {
        kill_wait(&mut child);
    }
    let output = reader.join().unwrap();
    (child, output)
}

#[test]
fn supervised_actual_killed_child_is_not_successful_wait_or_drain() {
    let (mut child, _output) = owned_fixture("block");
    child.kill().unwrap();
    assert!(!wait_child(&mut child, Instant::now() + Duration::from_secs(2)).unwrap());
    // This negative does not manufacture a Permit/clean retirement. Actor08ee
    // independently tests unknown-generation restore remains blocked forever.
}

#[test]
fn supervised_actual_stuck_reader_requires_owned_child_death_before_join() {
    let (mut child, mut output) = owned_fixture("block");
    let handle = thread::spawn(move || {
        let mut raw = Vec::new();
        output.read_to_end(&mut raw).map_err(|_| Error::Storage)?;
        Ok(())
    });
    let mut handle = Some(handle);
    // READY barrier +held stdin prove child is blocked; no timing hope/fake pipe.
    assert!(!pipe::join(
        &mut handle,
        Instant::now() + Duration::from_millis(20)
    ));
    child.kill().unwrap();
    assert!(!wait_child(&mut child, Instant::now() + Duration::from_secs(2)).unwrap());
    assert!(pipe::join(
        &mut handle,
        Instant::now() + Duration::from_secs(2)
    ));
    // A forced kill can permit physical joins, but NEVER a clean-drain marker.
}

#[test]
fn supervised_actual_parent_pipe_eof_is_not_a_typed_stop_even_when_child_exits_zero() {
    let (mut child, _output) = owned_fixture("control-eof");
    drop(child.stdin.take()); // Actual owned parent pipe EOF, not an input bool.
    assert!(wait_child(&mut child, Instant::now() + Duration::from_secs(3)).unwrap());
    // Child fixture asserted actual ParentControl flag2/notStopped. Exit0 alone
    // is explicitly insufficient in the production cleanup/drain conjunction.
}

#[test]
fn product_mode_is_closed_and_cannot_reuse_admit_as_resume() {
    assert!(PreparedTransportMode::from_product_verb("startTunnelTransport").unwrap().matches("admit"));
    assert!(!PreparedTransportMode::from_product_verb("startTunnelTransport").unwrap().matches("resume"));
    assert!(PreparedTransportMode::from_product_verb("resumeTunnelTransport").unwrap().matches("resume"));
    for bad in ["start", "resume", "ResumeTunnelTransport", "resumeTunnelTransport "] {
        assert!(PreparedTransportMode::from_product_verb(bad).is_err());
    }
}

#[test]
fn physical_join_and_successful_child_exit_are_separate_from_kill_or_thread_failure() {
    let (mut child,_output)=owned_fixture("block");child.kill().unwrap();
    assert!(retirement::child(&mut child,Instant::now()+Duration::from_secs(2)).is_err());
    for panic in [false,true] {
        let (tx,rx)=std::sync::mpsc::sync_channel(0);
        let mut handle=Some(thread::spawn(move || ->Result<()> {
            tx.send(()).unwrap();
            if panic {panic!("owned negative join fixture")}else{Err(Error::WrongState)}
        }));
        rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(!pipe::join(&mut handle,Instant::now()+Duration::from_secs(1)));
        assert!(handle.is_none()); // Actually joined Err/panic, not a success token.
    }
}

#[test]
fn product_control_retirement_is_actual_join_and_cannot_hide_partial_control() {
    for mode in ["network-open","network-partial","network-malformed"] {
        let (mut child,_output)=owned_fixture(mode);
        assert!(child.stdin.is_some());
        assert!(wait_child(&mut child,Instant::now()+Duration::from_secs(2)).unwrap());
    }
}


#[test]
fn supervised_remote_retirement_joins_real_control_reader_with_stdin_still_open() {
    let (mut child, _output) = owned_fixture("remote-open");
    assert!(child.stdin.is_some()); // Owned parent retains write end throughout join.
    assert!(wait_child(&mut child, Instant::now() + Duration::from_secs(2)).unwrap());
    // This exercises private reader retirement ONLY, not TLS authentication/drain.
}

#[test]
fn supervised_remote_retirement_cannot_hide_actual_partial_or_malformed_control() {
    for mode in ["remote-partial", "remote-malformed"] {
        let (mut child, _output) = owned_fixture(mode);
        assert!(wait_child(&mut child, Instant::now() + Duration::from_secs(2)).unwrap());
    }
}

#[test]
#[ignore = "private owned child fixture only; not a standalone acceptance test"]
fn owned_pipe_child_fixture() {
    let Ok(mode) = std::env::var("CC_SUPERVISED_PRIVATE_TEST") else {
        return;
    };
    if matches!(mode.as_str(), "remote-open" | "remote-partial" | "remote-malformed" | "network-open" | "network-partial" | "network-malformed") {
        if !matches!(mode.as_str(),"remote-open"|"network-open") {
            let mut gate = [0u8; 1];
            // File reads exactly one kernel byte; Stdin could prefetch bad control.
            let mut gate_input = fs::File::open("/proc/self/fd/0").unwrap();
            gate_input.read_exact(&mut gate).unwrap();
            drop(gate_input);
            assert_eq!(gate, [b'G']); // Remaining partial/malformed bytes already in pipe.
        }
        let mut control = ParentControl::start().unwrap();
        println!("\nCC_PRIVATE_CHILD_READY");
        std::io::stdout().flush().unwrap();
        let network=mode.starts_with("network-");
        let retired = if network {control.retire_for_transport_loss()}else{control.retire_for_remote()};
        let joined = if network {control.join_transport_loss(Instant::now()+Duration::from_secs(1)).is_ok()}
            else{control.join(Instant::now() + Duration::from_secs(1))};
        if mode == "network-open" {
            assert!(retired.is_ok() && joined);
            assert_eq!(control.flag.load(Ordering::SeqCst),4);
            assert!(!control.stopped() && !control.remote_retired());
        } else if mode == "remote-open" {
            assert!(retired.is_ok() && joined && control.remote_retired());
            assert!(!control.stopped()); // No forged original stdin Stop.
        } else {
            assert!(!joined && !control.remote_retired());
            assert_eq!(control.flag.load(Ordering::SeqCst), 2);
        }
    } else if mode == "control-eof" {
        let mut control = ParentControl::start().unwrap();
        println!("\nCC_PRIVATE_CHILD_READY");
        std::io::stdout().flush().unwrap();
        let end = Instant::now() + Duration::from_secs(2);
        while control.flag.load(Ordering::SeqCst) == 0 && Instant::now() < end {
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(control.flag.load(Ordering::SeqCst), 2);
        assert!(!control.stopped());
        assert!(control.retire_for_remote().is_err()); // UNKNOWN cannot revive.
        assert!(!control.join(Instant::now() + Duration::from_secs(1)));
    } else {
        assert_eq!(mode, "block");
        println!("\nCC_PRIVATE_CHILD_READY");
        std::io::stdout().flush().unwrap();
        let mut byte = [0u8; 1];
        std::io::stdin().read_exact(&mut byte).unwrap();
        panic!("owned test child must be killed while its private stdin remains open");
    }
}
