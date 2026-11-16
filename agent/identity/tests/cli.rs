#[path = "support/fixture.rs"]
mod fixture;
use cc_agent_identity_policy::*;
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(1);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "cc-agent-identity-test-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
        }
        #[cfg(not(unix))]
        std::fs::create_dir(&dir).unwrap();
        Self(dir)
    }
    fn fixture() -> Self {
        let temp = Self::new();
        fixture::create(
            &temp.0,
            1000,
            AgentId::new("44444444-4444-4444-8444-444444444444").unwrap(),
            ResourceId::new("66666666-6666-4666-8666-666666666666").unwrap(),
            ResourceId::new("77777777-7777-4777-8777-777777777777").unwrap(),
        );
        temp
    }
    fn value(&self, name: &str) -> Value {
        serde_json::from_slice(&std::fs::read(self.0.join(name)).unwrap()).unwrap()
    }
    fn cli(&self, command: &str, request: &Value) -> Output {
        self.cli_raw(command, &serde_json::to_vec(request).unwrap())
    }
    fn cli_raw(&self, command: &str, request: &[u8]) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_cc-agent-identity"))
            .arg(command)
            .arg("--store")
            .arg(self.0.join("store"))
            .arg("--anchor")
            .arg(self.0.join("anchor"))
            .arg("--owner-pin")
            .arg(self.0.join("owner-pin.json"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let _ = child.stdin.take().unwrap().write_all(request);
        child.wait_with_output().unwrap()
    }
    fn open(&self) -> Result<FileStore> {
        FileStore::open(
            self.0.join("store"),
            self.0.join("anchor"),
            serde_json::from_value(self.value("owner-pin.json")).unwrap(),
        )
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let expected = std::env::temp_dir();
        assert_eq!(self.0.parent(), Some(expected.as_path()));
        assert!(self
            .0
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("cc-agent-identity-test-"));
        assert!(!std::fs::symlink_metadata(&self.0)
            .unwrap()
            .file_type()
            .is_symlink());
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn cli_real_signed_admission_replay_resource_binding_restart_and_revoke() {
    let temp = Temp::fixture();
    let heartbeat = temp.value("heartbeat-request.json");
    let mut wrong = heartbeat.clone();
    wrong["operation"] =
        json!({"kind":"telemetryRead","source":"77777777-7777-4777-8777-777777777777"});
    let denied = temp.cli("verify-request", &wrong);
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("ScopeDenied"));
    let accepted = temp.cli("verify-request", &heartbeat);
    assert!(accepted.status.success(), "{:?}", accepted);
    let response: Value = serde_json::from_slice(&accepted.stdout).unwrap();
    assert_eq!(response["agentId"], "44444444-4444-4444-8444-444444444444");
    assert_eq!(response["expiresAt"], 1300);
    assert_eq!(response["operation"], heartbeat["operation"]);
    let replay = temp.cli("verify-request", &heartbeat);
    assert!(!replay.status.success());
    assert!(String::from_utf8_lossy(&replay.stderr).contains("Replay"));
    assert!(temp
        .cli("verify-request", &temp.value("history-request.json"))
        .status
        .success());
    assert!(temp
        .cli("mutate", &temp.value("revoke-request.json"))
        .status
        .success());
    assert!(temp
        .open()
        .unwrap()
        .load()
        .unwrap()
        .revoked(AgentId::new("44444444-4444-4444-8444-444444444444").unwrap()));
    let revoked = temp.cli(
        "verify-request",
        &temp.value("heartbeat-after-revoke-request.json"),
    );
    assert!(!revoked.status.success());
    assert!(String::from_utf8_lossy(&revoked.stderr).contains("WrongCheckpoint"));
}
#[test]
fn cli_bad_signature_and_privileged_scope_fail_without_consuming_lease() {
    let temp = Temp::fixture();
    let request = temp.value("heartbeat-request.json");
    let mut forged = request.clone();
    forged["proof"]["signature"] = json!("00".repeat(64));
    assert!(!temp.cli("verify-request", &forged).status.success());
    let mut privileged = request.clone();
    privileged["operation"] = json!({"kind":"jobsExecute","manifest":"11".repeat(32)});
    assert!(!temp.cli("verify-request", &privileged).status.success());
    assert!(temp.cli("verify-request", &request).status.success());
}
#[test]
fn journal_restore_missing_anchor_torn_commit_corruption_and_wrong_pin_fail_closed() {
    let temp = Temp::fixture();
    let newest = temp.0.join("store").join("00000000000000000003.json");
    let saved = std::fs::read(&newest).unwrap();
    std::fs::remove_file(&newest).unwrap();
    assert!(matches!(temp.open(), Err(Error::WrongCheckpoint)));
    std::fs::write(&newest, &saved).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&newest, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    // Different bytes under an independently retained high-water digest fail.
    std::fs::write(&newest, b"{}").unwrap();
    assert!(matches!(temp.open(), Err(Error::WrongCheckpoint)));
    std::fs::write(&newest, &saved).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&newest, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let mut pin: OwnerPin = serde_json::from_value(temp.value("owner-pin.json")).unwrap();
    pin.epoch = 2;
    assert!(matches!(
        FileStore::open(temp.0.join("store"), temp.0.join("anchor"), pin),
        Err(Error::WrongContext)
    ));
    // A data record without the second-phase independent anchor never recovers
    // by trusting an unacknowledged transition or auto-resetting enrollment.
    std::fs::write(
        temp.0.join("store").join("00000000000000000004.json"),
        b"{}",
    )
    .unwrap();
    assert!(matches!(temp.open(), Err(Error::WrongCheckpoint)));
}
#[test]
fn concurrent_processes_never_both_admit_same_lease() {
    let temp = Temp::fixture();
    let request = temp.value("heartbeat-request.json");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            barrier.wait();
            temp.cli("verify-request", &request)
        });
        let second = scope.spawn(|| {
            barrier.wait();
            temp.cli("verify-request", &request)
        });
        let a = first.join().unwrap();
        let b = second.join().unwrap();
        assert_eq!(
            usize::from(a.status.success()) + usize::from(b.status.success()),
            1,
            "a={a:?} b={b:?}"
        );
    });
    assert_eq!(temp.open().unwrap().load().unwrap().revision(), 4);
}
#[test]
fn stale_writer_lock_is_never_automatically_deleted_and_relative_paths_denied() {
    let temp = Temp::fixture();
    std::fs::write(temp.0.join("store").join("writer.lock"), b"stale").unwrap();
    assert!(matches!(temp.open(), Err(Error::Conflict)));
    assert!(temp.0.join("store").join("writer.lock").exists());
    let pin = serde_json::from_value(temp.value("owner-pin.json")).unwrap();
    assert!(matches!(
        FileStore::open("relative", temp.0.join("anchor"), pin),
        Err(Error::Storage)
    ));
}

fn release_fixture(
    temp: &Temp,
    version: u64,
) -> (Signed<ReleaseManifest>, ReleaseSignerPin, PathBuf) {
    use ed25519_dalek::{Signer, SigningKey};
    use sha2::{Digest, Sha256};
    let artifact = temp.0.join(format!("artifact-{version}.zip"));
    let bytes = format!("isolated-release-fixture-{version}").into_bytes();
    std::fs::write(&artifact, &bytes).unwrap();
    let key = SigningKey::from_bytes(&[77; 32]);
    let body = ReleaseManifest {
        format: 1,
        version,
        display_version: format!("0.1.{version}"),
        protocol: 1,
        capabilities: Capabilities {
            version: 1,
            scopes: Scopes(vec![Scope::TelemetryRead]),
        },
        platform: "linux-x86_64".into(),
        artifact_sha256: Hash::from_bytes(Sha256::digest(&bytes).into()),
        artifact_bytes: bytes.len() as u64,
        issued_at: Timestamp::new(1000).unwrap(),
        expires_at: Timestamp::new(2000).unwrap(),
    };
    let signature = SignatureBytes::from_bytes(key.sign(&body.signing_bytes().unwrap()).to_bytes());
    (
        Signed { body, signature },
        ReleaseSignerPin {
            format: 1,
            public_key: PublicKey::from_bytes(key.verifying_key().to_bytes()),
        },
        artifact,
    )
}
#[test]
fn release_real_signature_digest_expiry_platform_protocol_and_separate_signer_are_checked() {
    let temp = Temp::new();
    let (signed, pin, artifact) = release_fixture(&temp, 1);
    let now = Now(Timestamp::new(1001).unwrap());
    assert_eq!(
        verify_release(signed.clone(), &pin, &artifact, "linux-x86_64", 1, 0, now)
            .unwrap()
            .manifest()
            .version,
        1
    );
    assert!(matches!(
        verify_release(signed.clone(), &pin, &artifact, "linux-aarch64", 1, 0, now),
        Err(Error::CapabilityDenied)
    ));
    assert!(matches!(
        verify_release(signed.clone(), &pin, &artifact, "linux-x86_64", 2, 0, now),
        Err(Error::CapabilityDenied)
    ));
    assert!(matches!(
        verify_release(signed.clone(), &pin, &artifact, "linux-x86_64", 1, 1, now),
        Err(Error::WrongEpoch)
    ));
    assert!(matches!(
        verify_release(
            signed.clone(),
            &pin,
            &artifact,
            "linux-x86_64",
            1,
            0,
            Now(Timestamp::new(2000).unwrap())
        ),
        Err(Error::Expired)
    ));
    assert!(matches!(
        verify_release(
            signed.clone(),
            &pin,
            &artifact,
            "linux-x86_64",
            1,
            0,
            Now(Timestamp::new(999).unwrap())
        ),
        Err(Error::InvalidTime)
    ));
    use ed25519_dalek::SigningKey;
    let wrong = ReleaseSignerPin {
        format: 1,
        public_key: PublicKey::from_bytes(
            SigningKey::from_bytes(&[11; 32]).verifying_key().to_bytes(),
        ),
    };
    assert!(matches!(
        verify_release(signed.clone(), &wrong, &artifact, "linux-x86_64", 1, 0, now),
        Err(Error::InvalidSignature)
    ));
    let mut forged = signed.clone();
    forged.body.display_version = "99.9.9".into();
    assert!(matches!(
        verify_release(forged, &pin, &artifact, "linux-x86_64", 1, 0, now),
        Err(Error::InvalidSignature)
    ));
    let mut bytes = std::fs::read(&artifact).unwrap();
    bytes[0] ^= 1;
    std::fs::write(&artifact, &bytes).unwrap();
    assert!(matches!(
        verify_release(signed.clone(), &pin, &artifact, "linux-x86_64", 1, 0, now),
        Err(Error::InvalidSignature)
    ));
    bytes.push(0);
    std::fs::write(&artifact, &bytes).unwrap();
    assert!(matches!(
        verify_release(signed, &pin, &artifact, "linux-x86_64", 1, 0, now),
        Err(Error::InvalidWire)
    ));
}
#[test]
fn release_rollback_retains_highwater_and_never_reaccepts_downgrade_as_update() {
    let temp = Temp::new();
    let (one, pin, a) = release_fixture(&temp, 1);
    let (two, _, b) = release_fixture(&temp, 2);
    let now = Now(Timestamp::new(1001).unwrap());
    let mut slots = ReleaseSlots::default();
    assert!(matches!(slots.rollback(), Err(Error::NotFound)));
    slots
        .activate(verify_release(one.clone(), &pin, &a, "linux-x86_64", 1, 0, now).unwrap())
        .unwrap();
    slots
        .activate(
            verify_release(
                two.clone(),
                &pin,
                &b,
                "linux-x86_64",
                1,
                slots.highest_version(),
                now,
            )
            .unwrap(),
        )
        .unwrap();
    slots.rollback().unwrap();
    assert_eq!(slots.current().unwrap().version, 1);
    assert_eq!(slots.highest_version(), 2);
    assert!(matches!(
        verify_release(
            one,
            &pin,
            &a,
            "linux-x86_64",
            1,
            slots.highest_version(),
            now
        ),
        Err(Error::WrongEpoch)
    ));
    assert!(matches!(
        verify_release(
            two,
            &pin,
            &b,
            "linux-x86_64",
            1,
            slots.highest_version(),
            now
        ),
        Err(Error::WrongEpoch)
    ));
    assert!(matches!(slots.rollback(), Err(Error::NotFound)));
}
#[test]
fn release_cli_verifies_exact_artifact_and_rejects_replay_sequence() {
    let temp = Temp::new();
    let (mut signed, pin, artifact) = release_fixture(&temp, 1);
    use ed25519_dalek::{Signer, SigningKey};
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    signed.body.issued_at = Timestamp::new(seconds.saturating_sub(1)).unwrap();
    signed.body.expires_at = Timestamp::new(seconds + 600).unwrap();
    signed.signature = SignatureBytes::from_bytes(
        SigningKey::from_bytes(&[77; 32])
            .sign(&signed.body.signing_bytes().unwrap())
            .to_bytes(),
    );
    let pin_path = temp.0.join("release-pin.json");
    let manifest = temp.0.join("release.json");
    std::fs::write(&pin_path, serde_json::to_vec(&pin).unwrap()).unwrap();
    std::fs::write(&manifest, serde_json::to_vec(&signed).unwrap()).unwrap();
    let call = |current: &str| {
        Command::new(env!("CARGO_BIN_EXE_cc-agent-identity"))
            .args(["verify-release", "--public-key"])
            .arg(&pin_path)
            .arg("--manifest")
            .arg(&manifest)
            .arg("--artifact")
            .arg(&artifact)
            .args([
                "--platform",
                "linux-x86_64",
                "--protocol",
                "1",
                "--current-version",
                current,
            ])
            .output()
            .unwrap()
    };
    let accepted = call("0");
    assert!(accepted.status.success(), "{accepted:?}");
    let response: Value = serde_json::from_slice(&accepted.stdout).unwrap();
    assert_eq!(response["version"], 1);
    assert_eq!(
        response["artifactSha256"],
        serde_json::to_value(signed.body.artifact_sha256).unwrap()
    );
    assert!(!call("1").status.success());
    std::fs::write(&artifact, b"tampered").unwrap();
    assert!(!call("0").status.success());
}

#[test]
fn trusted_local_status_reports_hard_capacity_without_private_material() {
    let temp = Temp::fixture();
    let response = temp.cli("status", &serde_json::json!({}));
    assert!(response.status.success(), "{response:?}");
    let status: Value = serde_json::from_slice(&response.stdout).unwrap();
    assert_eq!(status["revision"], 3);
    assert_eq!(status["journalRecords"], 4);
    assert_eq!(status["policy"]["consumedNonces"], 1);
    assert_eq!(status["policy"]["remainingAdmissionsUpperBound"], 4095);
    assert_eq!(status["maxJournalDataBytes"], 32 * 1024 * 1024);
    let output = String::from_utf8(response.stdout).unwrap();
    assert!(
        !output.contains("publicKey") && !output.contains("signature") && !output.contains("token")
    );
    let mut request = temp.value("heartbeat-request.json");
    request["proof"]["signature"] = serde_json::json!("00".repeat(64));
    let denied = temp.cli("verify-request", &request);
    let event: Value = serde_json::from_slice(&denied.stderr).unwrap();
    assert_eq!(
        event,
        serde_json::json!({"event":"identityDenied","code":"InvalidSignature"})
    );
    assert!(denied.stderr.len() < 128);
}

#[test]
fn admission_rejects_oversized_signed_subframe_before_consuming_lease() {
    let temp = Temp::fixture();
    let request = temp.value("heartbeat-request.json");
    let raw = serde_json::to_string(&request).unwrap();
    let padded = raw.replacen(
        "\"lease\":{",
        &format!("\"lease\":{{{}", " ".repeat(MAX_WIRE_BYTES + 1)),
        1,
    );
    assert!(padded.len() > MAX_WIRE_BYTES && padded.len() < 64 * 1024);
    let denied = temp.cli_raw("verify-request", padded.as_bytes());
    assert!(
        !denied.status.success(),
        "Oversized nested Signed frame was accepted: {denied:?}"
    );
    assert!(denied.stdout.is_empty());
    let event: Value = serde_json::from_slice(&denied.stderr).unwrap();
    assert_eq!(event["code"], "Quota");
    assert_eq!(temp.open().unwrap().load().unwrap().revision(), 3);
    assert!(temp.cli("verify-request", &request).status.success());
}

#[test]
fn raw_admission_regression_corpus_rejects_ambiguous_nested_and_wrong_purpose_inputs() {
    let temp = Temp::fixture();
    let request = temp.value("heartbeat-request.json");
    let raw = serde_json::to_string(&request).unwrap();
    let mut corpus: Vec<(&str, Vec<u8>, &str)> = vec![
        (
            "duplicate-observation",
            raw.replacen("\"now\":1000", "\"now\":1000,\"now\":1000", 1)
                .into_bytes(),
            "InvalidWire",
        ),
        (
            "float-observation",
            raw.replacen("\"now\":1000", "\"now\":1000.0", 1)
                .into_bytes(),
            "InvalidWire",
        ),
        ("invalid-utf8", vec![0xff], "InvalidWire"),
        (
            "truncated",
            raw.as_bytes()[..raw.len() - 1].to_vec(),
            "InvalidWire",
        ),
        (
            "trailing-document",
            format!("{raw}{{}}").into_bytes(),
            "InvalidWire",
        ),
        (
            "trailing-nul",
            [raw.as_bytes(), &[0]].concat(),
            "InvalidWire",
        ),
        (
            "unknown-field",
            raw.replacen('{', "{\"sentinel-secret\":\"NEVER_ECHO_TEST_SENTINEL\",", 1)
                .into_bytes(),
            "InvalidWire",
        ),
    ];
    let duplicate = serde_json::to_string(&request["proof"]["body"]).unwrap();
    corpus.push((
        "duplicate-signed-body",
        raw.replacen(
            &format!("\"proof\":{{\"body\":{duplicate}"),
            &format!("\"proof\":{{\"body\":{duplicate},\"body\":{duplicate}"),
            1,
        )
        .into_bytes(),
        "InvalidWire",
    ));
    for (name, pointer, value, code) in [
        (
            "unknown-operation",
            "/operation/kind",
            json!("futureOperation"),
            "InvalidWire",
        ),
        (
            "unknown-protocol",
            "/lease/body/header/version",
            json!(2),
            "InvalidWire",
        ),
        (
            "float-issued-time",
            "/proof/body/issuedAt",
            json!(1000.0),
            "InvalidWire",
        ),
        (
            "negative-issued-time",
            "/proof/body/issuedAt",
            json!(-1),
            "InvalidWire",
        ),
        (
            "out-of-range-now",
            "/now",
            json!(253_402_300_800u64),
            "InvalidTime",
        ),
        (
            "invalid-hex-key",
            "/proof/signature",
            json!("GG".repeat(64)),
            "InvalidWire",
        ),
    ] {
        let mut changed = request.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        corpus.push((name, serde_json::to_vec(&changed).unwrap(), code));
    }
    let nested = format!("{}0{}", "[".repeat(200), "]".repeat(200));
    let operation = serde_json::to_string(&request["operation"]).unwrap();
    let deep_operation = format!(
        "{},\"nested\":{nested}}}",
        &operation[..operation.len() - 1]
    );
    corpus.push((
        "deep-tagged-enum",
        raw.replacen(
            &format!("\"operation\":{operation}"),
            &format!("\"operation\":{deep_operation}"),
            1,
        )
        .into_bytes(),
        "InvalidWire",
    ));
    use ed25519_dalek::{Signer, SigningKey};
    let freshness: Freshness =
        serde_json::from_value(request["freshness"]["body"].clone()).unwrap();
    let mut bytes = freshness.signing_bytes().unwrap();
    let prefix = b"consolecrypt.agent.identity.personal-prototype\0v1\0".len();
    bytes.splice(
        prefix..prefix + Freshness::DOMAIN.len(),
        ReleaseManifest::DOMAIN.bytes(),
    );
    let mut cross = request.clone();
    cross["freshness"]["signature"] = json!(hex::encode(
        SigningKey::from_bytes(&[11; 32]).sign(&bytes).to_bytes()
    ));
    corpus.push((
        "same-owner-cross-purpose-signature",
        serde_json::to_vec(&cross).unwrap(),
        "InvalidSignature",
    ));
    assert!(corpus.len() <= 20);
    for (name, bytes, code) in corpus {
        assert!(bytes.len() < MAX_WIRE_BYTES, "unbounded corpus item {name}");
        let denied = temp.cli_raw("verify-request", &bytes);
        assert!(!denied.status.success(), "accepted {name}: {denied:?}");
        assert!(denied.stdout.is_empty(), "stdout in {name}");
        assert!(denied.stderr.len() < 128, "unbounded stderr {name}");
        let event: Value = serde_json::from_slice(&denied.stderr).unwrap();
        assert_eq!(event["code"], code, "wrong denial in {name}");
        assert!(!String::from_utf8_lossy(&denied.stderr).contains("NEVER_ECHO_TEST_SENTINEL"));
    }
    assert_eq!(temp.open().unwrap().load().unwrap().revision(), 3);
    assert!(temp.cli("verify-request", &request).status.success());
}

// Recompute only public hash anchors in the corruption tests. This deliberately
// models an attacker able to rewrite untrusted local bytes, proving replay still
// requires valid owner/agent signatures rather than trusting a checksum alone.
fn reanchor_synthetic_journal(temp: &Temp) {
    use sha2::{Digest, Sha256};
    let mut previous = [0u8; 32];
    for n in 0..=3 {
        let name = format!("{n:020}.json");
        let bytes = std::fs::read(temp.0.join("store").join(&name)).unwrap();
        let mut digest = Sha256::new();
        digest.update(b"consolecrypt.agent.durable-journal.v1");
        digest.update([0]);
        digest.update(previous);
        digest.update(bytes);
        previous = digest.finalize().into();
        std::fs::write(
            temp.0.join("anchor").join(name),
            serde_json::to_vec(&json!({"revision":n,"digest":hex::encode(previous)})).unwrap(),
        )
        .unwrap();
    }
}
#[test]
fn corrupt_journal_corpus_cannot_gain_authority_by_replacing_public_hash_anchors() {
    let cases = [
        "duplicate-time",
        "forged-owner-signature",
        "foreign-context",
        "unknown-transition",
        "clock-rollback",
        "duplicate-genesis-version",
        "substituted-transition",
    ];
    for case in cases {
        let temp = Temp::fixture();
        let n = if case == "duplicate-genesis-version" {
            0
        } else if case == "clock-rollback" {
            3
        } else {
            1
        };
        let path = temp.0.join("store").join(format!("{n:020}.json"));
        let raw = std::fs::read_to_string(&path).unwrap();
        let mut value: Value = serde_json::from_str(&raw).unwrap();
        let (changed, expected) = match case {
            "duplicate-time" => (
                raw.replacen("\"at\":1000", "\"at\":1000,\"at\":1000", 1),
                Error::Storage,
            ),
            "forged-owner-signature" => {
                value["transition"]["value"]["signature"] = json!("00".repeat(64));
                (value.to_string(), Error::InvalidSignature)
            }
            "foreign-context" => {
                value["transition"]["value"]["body"]["header"]["context"]["deploymentId"] =
                    json!("99999999-9999-4999-8999-999999999999");
                (value.to_string(), Error::WrongContext)
            }
            "unknown-transition" => {
                value["transition"]["kind"] = json!("FutureTrustEverything");
                (value.to_string(), Error::Storage)
            }
            "clock-rollback" => {
                value["at"] = json!(999);
                (value.to_string(), Error::InvalidTime)
            }
            "duplicate-genesis-version" => (
                raw.replacen("\"version\":1", "\"version\":1,\"version\":1", 1),
                Error::Storage,
            ),
            "substituted-transition" => (
                std::fs::read_to_string(temp.0.join("store").join("00000000000000000002.json"))
                    .unwrap(),
                Error::NotFound,
            ),
            _ => unreachable!(),
        };
        std::fs::write(&path, changed).unwrap();
        reanchor_synthetic_journal(&temp);
        assert!(
            matches!(temp.open(),Err(error) if error==expected),
            "accepted or wrong error for {case}"
        );
        // Never repair/reset a corrupted enrollment or invent a new checkpoint.
        assert_eq!(std::fs::read_dir(temp.0.join("store")).unwrap().count(), 4);
    }
}
#[test]
fn anchor_duplicate_keys_partial_and_swapped_records_fail_closed() {
    for case in ["duplicate-revision", "partial-json", "swap-anchor"] {
        let temp = Temp::fixture();
        let path = temp.0.join("anchor").join("00000000000000000002.json");
        let raw = std::fs::read_to_string(&path).unwrap();
        let (changed, expected) = match case {
            "duplicate-revision" => (
                raw.replacen("\"revision\":2", "\"revision\":2,\"revision\":2", 1),
                Error::Storage,
            ),
            "partial-json" => (raw[..raw.len() / 2].to_string(), Error::Storage),
            "swap-anchor" => (
                std::fs::read_to_string(temp.0.join("anchor").join("00000000000000000003.json"))
                    .unwrap(),
                Error::WrongCheckpoint,
            ),
            _ => unreachable!(),
        };
        std::fs::write(path, changed).unwrap();
        assert!(
            matches!(temp.open(),Err(error) if error==expected),
            "accepted or wrong error for {case}"
        );
    }
}
#[test]
fn raw_release_manifest_corpus_rejects_duplicate_float_enum_nested_and_cross_purpose() {
    let temp = Temp::new();
    let (signed, pin, artifact) = release_fixture(&temp, 1);
    let pin_path = temp.0.join("release-pin.json");
    let manifest_path = temp.0.join("release.json");
    std::fs::write(&pin_path, serde_json::to_vec(&pin).unwrap()).unwrap();
    let raw = serde_json::to_string(&signed).unwrap();
    let body = serde_json::to_string(&signed.body).unwrap();
    let mut corpus = vec![
        (
            "duplicate-body",
            format!(
                "{{\"body\":{body},\"body\":{body},\"signature\":{}}}",
                serde_json::to_string(&signed.signature).unwrap()
            ),
            "InvalidWire",
        ),
        (
            "duplicate-version",
            raw.replacen("\"version\":1", "\"version\":1,\"version\":1", 1),
            "InvalidWire",
        ),
        (
            "float-issued-time",
            raw.replacen("\"issuedAt\":1000", "\"issuedAt\":1000.0", 1),
            "InvalidWire",
        ),
        (
            "unknown-field",
            raw.replacen('{', "{\"sentinel-secret\":\"NEVER_ECHO_TEST_SENTINEL\",", 1),
            "InvalidWire",
        ),
    ];
    let mut unknown = serde_json::to_value(&signed).unwrap();
    unknown["body"]["capabilities"]["scopes"] = json!(["root"]);
    corpus.push(("unknown-scope", unknown.to_string(), "InvalidWire"));
    let deep = format!(
        "{{\"body\":{}0{},\"signature\":\"{}\"}}",
        "[".repeat(200),
        "]".repeat(200),
        "00".repeat(64)
    );
    corpus.push(("deep-body", deep, "InvalidWire"));
    use ed25519_dalek::{Signer, SigningKey};
    let mut bytes = signed.body.signing_bytes().unwrap();
    let prefix = b"consolecrypt.agent.identity.personal-prototype\0v1\0".len();
    bytes.splice(
        prefix..prefix + ReleaseManifest::DOMAIN.len(),
        Freshness::DOMAIN.bytes(),
    );
    let mut cross = serde_json::to_value(&signed).unwrap();
    cross["signature"] = json!(hex::encode(
        SigningKey::from_bytes(&[77; 32]).sign(&bytes).to_bytes()
    ));
    corpus.push((
        "same-release-key-cross-purpose",
        cross.to_string(),
        "InvalidSignature",
    ));
    assert!(corpus.len() <= 10);
    for (name, raw, code) in corpus {
        assert!(raw.len() < MAX_WIRE_BYTES);
        std::fs::write(&manifest_path, raw).unwrap();
        let denied = Command::new(env!("CARGO_BIN_EXE_cc-agent-identity"))
            .args(["verify-release", "--public-key"])
            .arg(&pin_path)
            .arg("--manifest")
            .arg(&manifest_path)
            .arg("--artifact")
            .arg(&artifact)
            .args([
                "--platform",
                "linux-x86_64",
                "--protocol",
                "1",
                "--current-version",
                "0",
            ])
            .output()
            .unwrap();
        assert!(!denied.status.success(), "accepted {name}");
        assert!(denied.stdout.is_empty());
        assert!(denied.stderr.len() < 128);
        let event: Value = serde_json::from_slice(&denied.stderr).unwrap();
        assert_eq!(event["code"], code, "wrong error in {name}");
        assert!(!String::from_utf8_lossy(&denied.stderr).contains("NEVER_ECHO_TEST_SENTINEL"));
    }
}

#[cfg(unix)]
#[test]
fn linux_private_modes_and_insecure_identity_storage_permissions_are_enforced() {
    use std::os::unix::fs::PermissionsExt;
    let temp = Temp::fixture();
    for dir in ["store", "anchor"] {
        let path = temp.0.join(dir);
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        for item in std::fs::read_dir(&path).unwrap() {
            assert_eq!(
                item.unwrap().metadata().unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
    let data = temp.0.join("store");
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(temp.open(), Err(Error::Storage)));
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(temp.open().is_ok());
    let record = data.join("00000000000000000003.json");
    std::fs::set_permissions(&record, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(temp.open(), Err(Error::Storage)));
    std::fs::set_permissions(&record, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(temp.open().is_ok());
}
#[cfg(unix)]
#[test]
fn linux_identity_state_symlinks_and_hardlinks_cannot_replace_owned_records() {
    use std::os::unix::fs::symlink;
    let temp = Temp::fixture();
    let pin: OwnerPin = serde_json::from_value(temp.value("owner-pin.json")).unwrap();
    let alias = temp.0.join("alias");
    symlink(temp.0.join("store"), &alias).unwrap();
    assert!(matches!(
        FileStore::open(&alias, temp.0.join("anchor"), pin),
        Err(Error::Storage)
    ));
    let record = temp.0.join("store").join("00000000000000000003.json");
    let saved = temp.0.join("saved-owned-record");
    std::fs::rename(&record, &saved).unwrap();
    symlink(&saved, &record).unwrap();
    assert!(matches!(temp.open(), Err(Error::Storage)));
    std::fs::remove_file(&record).unwrap();
    std::fs::rename(&saved, &record).unwrap();
    let linked = temp.0.join("extra-owned-link");
    std::fs::hard_link(&record, &linked).unwrap();
    assert!(matches!(temp.open(), Err(Error::Storage)));
    std::fs::remove_file(&linked).unwrap();
    assert!(temp.open().is_ok());
}
#[cfg(unix)]
#[test]
fn linux_release_verification_rejects_symlink_and_hardlinked_artifacts() {
    use std::os::unix::fs::symlink;
    let temp = Temp::new();
    let (signed, pin, artifact) = release_fixture(&temp, 1);
    let now = Now(Timestamp::new(1001).unwrap());
    let alias = temp.0.join("artifact-alias");
    symlink(&artifact, &alias).unwrap();
    assert!(matches!(
        verify_release(signed.clone(), &pin, &alias, "linux-x86_64", 1, 0, now),
        Err(Error::Storage)
    ));
    let extra = temp.0.join("artifact-extra-link");
    std::fs::hard_link(&artifact, &extra).unwrap();
    assert!(matches!(
        verify_release(signed.clone(), &pin, &artifact, "linux-x86_64", 1, 0, now),
        Err(Error::Storage)
    ));
    std::fs::remove_file(&extra).unwrap();
    assert!(verify_release(signed, &pin, &artifact, "linux-x86_64", 1, 0, now).is_ok());
}

#[cfg(target_os = "linux")]
fn provision_cli(temp: &Temp, name: &str, agent: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cc-agent-identity"))
        .args(["provision-node", "--state-dir"])
        .arg(temp.0.join(name))
        .args(["--agent-id", agent])
        .output()
        .unwrap()
}
#[cfg(target_os = "linux")]
#[test]
fn linux_node_local_provisioning_has_fresh_per_host_keys_and_idempotent_public_hello() {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::PermissionsExt;
    let temp = Temp::new();
    let id_a = "44444444-4444-4444-8444-444444444444";
    let id_b = "55555555-5555-4555-8555-555555555555";
    let a = provision_cli(&temp, "node-a", id_a);
    let b = provision_cli(&temp, "node-b", id_b);
    assert!(a.status.success(), "{a:?}");
    assert!(b.status.success(), "{b:?}");
    let public_a: Value = serde_json::from_slice(&a.stdout).unwrap();
    let public_b: Value = serde_json::from_slice(&b.stdout).unwrap();
    assert_eq!(public_a["agentId"], id_a);
    assert_eq!(public_b["agentId"], id_b);
    assert_eq!(public_a["keyEpoch"], 1);
    assert_ne!(public_a["publicKey"], public_b["publicKey"]);
    assert_ne!(public_a["keyFingerprint"], public_b["keyFingerprint"]);
    let key: PublicKey = serde_json::from_value(public_a["publicKey"].clone()).unwrap();
    assert_eq!(
        serde_json::to_value(key.fingerprint()).unwrap(),
        public_a["keyFingerprint"]
    );
    let path = temp.0.join("node-a/node-identity.key");
    let initial = Sha256::digest(std::fs::read(&path).unwrap());
    assert_eq!(
        std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let repeated = provision_cli(&temp, "node-a", id_a);
    assert!(repeated.status.success());
    assert_eq!(repeated.stdout, a.stdout);
    assert_eq!(Sha256::digest(std::fs::read(&path).unwrap()), initial);
    let wrong = provision_cli(&temp, "node-a", id_b);
    assert!(!wrong.status.success());
    let event: Value = serde_json::from_slice(&wrong.stderr).unwrap();
    assert_eq!(event["code"], "WrongContext");
    assert_eq!(Sha256::digest(std::fs::read(&path).unwrap()), initial);
    for public in [&a.stdout, &b.stdout] {
        let text = String::from_utf8_lossy(public);
        assert!(
            !text.contains("seed")
                && !text.contains("private")
                && !text.contains("vault")
                && !text.contains("owner")
                && !text.contains("scopes")
        );
        assert!(public.len() < 512);
    }
}
#[cfg(target_os = "linux")]
#[test]
fn linux_node_provisioning_never_silently_regenerates_missing_partial_or_corrupt_keys() {
    use sha2::{Digest, Sha256};
    let id = "44444444-4444-4444-8444-444444444444";
    for state in ["missing", "partial", "corrupt", "incomplete-initialization"] {
        let temp = Temp::new();
        let root = temp.0.join("node");
        let path = root.join("node-identity.key");
        if state == "incomplete-initialization" {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(&root)
                .unwrap();
        } else {
            assert!(provision_cli(&temp, "node", id).status.success());
            if state == "missing" {
                std::fs::remove_file(&path).unwrap();
            } else if state == "partial" {
                std::fs::write(&path, b"partial").unwrap();
            } else {
                let mut bytes = zeroize::Zeroizing::new(std::fs::read(&path).unwrap());
                bytes[52] ^= 1;
                std::fs::write(&path, &bytes).unwrap();
            }
        }
        let before = if path.exists() {
            Some(Sha256::digest(std::fs::read(&path).unwrap()))
        } else {
            None
        };
        let denied = provision_cli(&temp, "node", id);
        assert!(!denied.status.success(), "accepted {state}");
        assert!(denied.stdout.is_empty());
        let event: Value = serde_json::from_slice(&denied.stderr).unwrap();
        assert_eq!(event["code"], "Storage");
        let after = if path.exists() {
            Some(Sha256::digest(std::fs::read(&path).unwrap()))
        } else {
            None
        };
        assert_eq!(after, before, "rewrote state in {state}");
    }
}
#[cfg(target_os = "linux")]
#[test]
fn linux_node_private_key_permission_and_link_substitution_are_denied() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let temp = Temp::new();
    let id = "44444444-4444-4444-8444-444444444444";
    let accepted = provision_cli(&temp, "node", id);
    assert!(accepted.status.success());
    let root = temp.0.join("node");
    let path = root.join("node-identity.key");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(!provision_cli(&temp, "node", id).status.success());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!provision_cli(&temp, "node", id).status.success());
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let extra = temp.0.join("extra-private-key-link");
    std::fs::hard_link(&path, &extra).unwrap();
    assert!(!provision_cli(&temp, "node", id).status.success());
    std::fs::remove_file(&extra).unwrap();
    let saved = temp.0.join("saved-owned-private-key");
    std::fs::rename(&path, &saved).unwrap();
    symlink(&saved, &path).unwrap();
    assert!(!provision_cli(&temp, "node", id).status.success());
    std::fs::remove_file(&path).unwrap();
    std::fs::rename(&saved, &path).unwrap();
    assert_eq!(provision_cli(&temp, "node", id).stdout, accepted.stdout);
}
#[cfg(target_os = "linux")]
#[test]
fn linux_concurrent_initial_provisioning_never_creates_two_keys_for_one_node() {
    let temp = Temp::new();
    let id = "44444444-4444-4444-8444-444444444444";
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    std::thread::scope(|scope| {
        let a = scope.spawn(|| {
            barrier.wait();
            provision_cli(&temp, "node", id)
        });
        let b = scope.spawn(|| {
            barrier.wait();
            provision_cli(&temp, "node", id)
        });
        let a = a.join().unwrap();
        let b = b.join().unwrap();
        assert!(a.status.success() || b.status.success());
        if a.status.success() && b.status.success() {
            assert_eq!(a.stdout, b.stdout);
        }
        let stable = provision_cli(&temp, "node", id);
        assert!(stable.status.success());
        for result in [a, b] {
            if result.status.success() {
                assert_eq!(result.stdout, stable.stdout);
            } else {
                assert!(result.stdout.is_empty());
            }
        }
    });
    let mut names: Vec<_> = std::fs::read_dir(temp.0.join("node"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, vec![".node-keys.lock", "node-identity.key"]);
}
#[cfg(not(target_os = "linux"))]
#[test]
fn node_provisioning_is_denied_on_uncertified_non_linux_platforms() {
    let temp = Temp::new();
    let id = AgentId::new("44444444-4444-4444-8444-444444444444").unwrap();
    assert!(matches!(
        provision_node(&temp.0.join("node"), id),
        Err(Error::CapabilityDenied)
    ));
    assert!(!temp.0.join("node").exists());
}

#[cfg(target_os = "linux")]
fn node_offer(public: &Value, seconds: u64) -> (OwnerPin, Signed<Offer>) {
    use ed25519_dalek::{Signer, SigningKey};
    let owner = SigningKey::from_bytes(&[11; 32]);
    let now = Timestamp::new(seconds).unwrap();
    let context = Context {
        tenant: Tenant::LocalProfile {
            id: LocalProfileId::new("11111111-1111-4111-8111-111111111111").unwrap(),
        },
        deployment_id: DeploymentId::new("22222222-2222-4222-8222-222222222222").unwrap(),
    };
    let pin = OwnerPin {
        context: context.clone(),
        key: PublicKey::from_bytes(owner.verifying_key().to_bytes()),
        epoch: 1,
        checkpoint: 0,
        expires_at: now.checked_add(3600).unwrap(),
    };
    let body = Offer {
        header: Header {
            version: 1,
            context,
            owner_epoch: 1,
            checkpoint: 1,
        },
        invitation_id: InvitationId::new("33333333-3333-4333-8333-333333333333").unwrap(),
        agent_id: serde_json::from_value(public["agentId"].clone()).unwrap(),
        agent_key: serde_json::from_value(public["publicKey"].clone()).unwrap(),
        token_hash: InvitationToken::from_bytes([33; 32]).unwrap().digest(),
        scopes: Scopes(vec![Scope::TelemetryRead]),
        issued_at: now,
        expires_at: now.checked_add(600).unwrap(),
    };
    let signature =
        SignatureBytes::from_bytes(owner.sign(&body.signing_bytes().unwrap()).to_bytes());
    (pin, Signed { body, signature })
}
#[cfg(target_os = "linux")]
fn sign_synthetic_offer(body: Offer) -> Signed<Offer> {
    use ed25519_dalek::{Signer, SigningKey};
    let signature = SignatureBytes::from_bytes(
        SigningKey::from_bytes(&[11; 32])
            .sign(&body.signing_bytes().unwrap())
            .to_bytes(),
    );
    Signed { body, signature }
}
#[cfg(target_os = "linux")]
fn claim_cli(temp: &Temp, id: &str, request: &Value) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cc-agent-identity"))
        .args(["claim-offer", "--state-dir"])
        .arg(temp.0.join("node"))
        .args(["--agent-id", id, "--owner-pin"])
        .arg(temp.0.join("claim-owner-pin.json"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(request).unwrap());
    child.wait_with_output().unwrap()
}
#[cfg(target_os = "linux")]
#[test]
fn actual_node_claim_requires_pinned_offer_and_separate_durable_owner_approval() {
    use ed25519_dalek::{Signer, SigningKey};
    let temp = Temp::new();
    let id = "44444444-4444-4444-8444-444444444444";
    let provisioned = provision_cli(&temp, "node", id);
    assert!(provisioned.status.success());
    let public: Value = serde_json::from_slice(&provisioned.stdout).unwrap();
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let (pin, offer) = node_offer(&public, seconds);
    std::fs::write(
        temp.0.join("claim-owner-pin.json"),
        serde_json::to_vec(&pin).unwrap(),
    )
    .unwrap();
    let first = claim_cli(&temp, id, &json!({"offer":offer}));
    let second = claim_cli(&temp, id, &json!({"offer":offer}));
    assert!(first.status.success(), "{first:?}");
    assert!(second.status.success());
    let claim = Signed::<Claim>::from_json(&first.stdout).unwrap();
    let another = Signed::<Claim>::from_json(&second.stdout).unwrap();
    assert_ne!(claim.body.nonce, another.body.nonce);
    assert_eq!(
        claim.body.capabilities,
        Capabilities {
            version: 1,
            scopes: Scopes(vec![Scope::TelemetryRead])
        }
    );
    let observed = Now(Timestamp::new(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap());
    let mut owner = Actor::new(
        FileStore::create(
            temp.0.join("claim-store"),
            temp.0.join("claim-anchor"),
            pin.clone(),
            Now(Timestamp::new(seconds).unwrap()),
        )
        .unwrap(),
    )
    .unwrap();
    owner.offer(offer.clone(), observed).unwrap();
    let token = InvitationToken::from_bytes([33; 32]).unwrap();
    owner.consume(&token, claim.clone(), observed).unwrap();
    assert!(owner
        .snapshot()
        .unwrap()
        .certificate(offer.body.agent_id)
        .is_none());
    assert!(matches!(
        owner.consume(&token, another, observed),
        Err(Error::Replay)
    ));
    let body = Certificate {
        header: Header {
            checkpoint: 2,
            ..offer.body.header.clone()
        },
        agent_id: offer.body.agent_id,
        key_epoch: 1,
        agent_key: offer.body.agent_key,
        scopes: Scopes(vec![Scope::TelemetryRead]),
        capabilities: claim.body.capabilities.clone(),
        origin: CertificateOrigin::Enrollment {
            invitation_id: offer.body.invitation_id,
            claim_hash: claim.body.digest().unwrap(),
            key_fingerprint: offer.body.agent_key.fingerprint(),
        },
        not_before: observed.0,
        expires_at: observed.0.checked_add(600).unwrap(),
    };
    let signature = SignatureBytes::from_bytes(
        SigningKey::from_bytes(&[11; 32])
            .sign(&body.signing_bytes().unwrap())
            .to_bytes(),
    );
    owner.approve(Signed { body, signature }, observed).unwrap();
    assert_eq!(
        owner
            .snapshot()
            .unwrap()
            .certificate(offer.body.agent_id)
            .unwrap()
            .agent_key,
        serde_json::from_value(public["publicKey"].clone()).unwrap()
    );
}
#[cfg(target_os = "linux")]
#[test]
fn typed_claim_refuses_foreign_identity_key_context_epoch_scope_and_expiry_without_rekeying() {
    use ed25519_dalek::SigningKey;
    use sha2::{Digest, Sha256};
    let temp = Temp::new();
    let id = "44444444-4444-4444-8444-444444444444";
    let agent = AgentId::new(id).unwrap();
    let public: Value = serde_json::from_slice(&provision_cli(&temp, "node", id).stdout).unwrap();
    let (pin, offer) = node_offer(&public, 1000);
    let now = Now(Timestamp::new(1000).unwrap());
    let key_path = temp.0.join("node/node-identity.key");
    let before = Sha256::digest(std::fs::read(&key_path).unwrap());
    for case in [
        "foreign-id",
        "foreign-key",
        "foreign-context",
        "wrong-epoch",
        "wrong-checkpoint",
        "unsupported-scope",
        "expired",
        "future",
    ] {
        let mut body = offer.body.clone();
        let expected = match case {
            "foreign-id" => {
                body.agent_id = AgentId::new("55555555-5555-4555-8555-555555555555").unwrap();
                Error::WrongClaim
            }
            "foreign-key" => {
                body.agent_key = PublicKey::from_bytes(
                    SigningKey::from_bytes(&[22; 32]).verifying_key().to_bytes(),
                );
                Error::WrongClaim
            }
            "foreign-context" => {
                body.header.context.deployment_id =
                    DeploymentId::new("99999999-9999-4999-8999-999999999999").unwrap();
                Error::WrongContext
            }
            "wrong-epoch" => {
                body.header.owner_epoch = 2;
                Error::WrongEpoch
            }
            "wrong-checkpoint" => {
                body.header.checkpoint = 2;
                Error::WrongCheckpoint
            }
            "unsupported-scope" => {
                body.scopes = Scopes(vec![Scope::TelemetryRead, Scope::JobsExecute]);
                Error::CapabilityDenied
            }
            "expired" => {
                body.issued_at = Timestamp::new(400).unwrap();
                body.expires_at = Timestamp::new(1000).unwrap();
                Error::Expired
            }
            "future" => {
                body.issued_at = Timestamp::new(1001).unwrap();
                body.expires_at = Timestamp::new(1601).unwrap();
                Error::InvalidTime
            }
            _ => unreachable!(),
        };
        assert!(
            matches!(create_node_claim(&temp.0.join("node"),agent,&pin,sign_synthetic_offer(body),now),Err(error) if error==expected),
            "accepted {case}"
        );
    }
    let mut bad = offer.clone();
    bad.signature = SignatureBytes::from_bytes([0; 64]);
    assert!(matches!(
        create_node_claim(&temp.0.join("node"), agent, &pin, bad, now),
        Err(Error::InvalidSignature)
    ));
    assert!(matches!(
        create_node_claim(&temp.0.join("absent"), agent, &pin, offer.clone(), now),
        Err(Error::Storage)
    ));
    assert!(!temp.0.join("absent").exists());
    assert_eq!(Sha256::digest(std::fs::read(&key_path).unwrap()), before);
    assert!(create_node_claim(&temp.0.join("node"), agent, &pin, offer, now).is_ok());
}
#[cfg(target_os = "linux")]
#[test]
fn node_claim_cli_uses_local_clock_and_rejects_caller_time_or_expired_offer() {
    let temp = Temp::new();
    let id = "44444444-4444-4444-8444-444444444444";
    let public: Value = serde_json::from_slice(&provision_cli(&temp, "node", id).stdout).unwrap();
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let (pin, mut offer) = node_offer(&public, seconds);
    std::fs::write(
        temp.0.join("claim-owner-pin.json"),
        serde_json::to_vec(&pin).unwrap(),
    )
    .unwrap();
    offer.body.issued_at = Timestamp::new(seconds - 601).unwrap();
    offer.body.expires_at = Timestamp::new(seconds - 1).unwrap();
    offer = sign_synthetic_offer(offer.body);
    let denied = claim_cli(&temp, id, &json!({"offer":offer}));
    assert!(!denied.status.success());
    assert!(denied.stdout.is_empty());
    let event: Value = serde_json::from_slice(&denied.stderr).unwrap();
    assert_eq!(event["code"], "Expired");
    let spoof = claim_cli(&temp, id, &json!({"offer":offer,"now":seconds-600}));
    assert!(!spoof.status.success());
    let event: Value = serde_json::from_slice(&spoof.stderr).unwrap();
    assert_eq!(event["code"], "InvalidWire");
    assert!(denied.stderr.len() < 128 && spoof.stderr.len() < 128);
}

#[cfg(target_os = "linux")]
fn test_owner_sign<T: Statement>(body: T) -> Signed<T> {
    use ed25519_dalek::{Signer, SigningKey};
    let signature = SignatureBytes::from_bytes(
        SigningKey::from_bytes(&[11; 32])
            .sign(&body.signing_bytes().unwrap())
            .to_bytes(),
    );
    Signed { body, signature }
}
#[cfg(target_os = "linux")]
fn enrolled_real_node(
    temp: &Temp,
    seconds: u64,
) -> (Actor<FileStore>, OwnerPin, OwnerPin, Signed<Certificate>) {
    enrolled_real_node_in(temp, seconds, |pin, now| {
        FileStore::create(
            temp.0.join("real-authority-store"),
            temp.0.join("real-authority-anchor"),
            pin,
            now,
        )
        .unwrap()
    })
}
#[cfg(target_os = "linux")]
fn enrolled_real_node_in<S: CompareAndSwap, F: FnOnce(OwnerPin, Now) -> S>(
    temp: &Temp,
    seconds: u64,
    make_store: F,
) -> (Actor<S>, OwnerPin, OwnerPin, Signed<Certificate>) {
    let id = AgentId::new("44444444-4444-4444-8444-444444444444").unwrap();
    let now = Now(Timestamp::new(seconds).unwrap());
    let public = provision_node(&temp.0.join("real-node"), id).unwrap();
    let value = serde_json::to_value(public).unwrap();
    let (pin, offer) = node_offer(&value, seconds);
    let claim = create_node_claim(&temp.0.join("real-node"), id, &pin, offer.clone(), now).unwrap();
    let mut authority = Actor::new(make_store(pin.clone(), now)).unwrap();
    authority.offer(offer.clone(), now).unwrap();
    authority
        .consume(
            &InvitationToken::from_bytes([33; 32]).unwrap(),
            claim.clone(),
            now,
        )
        .unwrap();
    let certificate = test_owner_sign(Certificate {
        header: Header {
            checkpoint: 2,
            ..offer.body.header.clone()
        },
        agent_id: id,
        key_epoch: 1,
        agent_key: offer.body.agent_key,
        scopes: Scopes(vec![Scope::TelemetryRead]),
        capabilities: claim.body.capabilities.clone(),
        origin: CertificateOrigin::Enrollment {
            invitation_id: offer.body.invitation_id,
            claim_hash: claim.body.digest().unwrap(),
            key_fingerprint: offer.body.agent_key.fingerprint(),
        },
        not_before: now.0,
        expires_at: now.0.checked_add(600).unwrap(),
    });
    authority.approve(certificate.clone(), now).unwrap();
    let current = authority.snapshot().unwrap().owner().clone();
    (authority, pin, current, certificate)
}
#[cfg(target_os = "linux")]
fn real_request_binding(
    pin: &OwnerPin,
    cert: &Signed<Certificate>,
    seconds: u64,
    n: u64,
) -> NodeRequestBinding {
    let now = Timestamp::new(seconds).unwrap();
    let header = Header {
        version: 1,
        context: pin.context.clone(),
        owner_epoch: pin.epoch,
        checkpoint: pin.checkpoint,
    };
    let operation = Operation::TelemetryRead {
        source: ResourceId::new("66666666-6666-4666-8666-666666666666").unwrap(),
    };
    let lease = test_owner_sign(Lease {
        header: header.clone(),
        id: LeaseId::new(&format!("88888888-8888-4888-8888-{n:012x}")).unwrap(),
        agent_id: cert.body.agent_id,
        key_epoch: 1,
        certificate_hash: cert.body.digest().unwrap(),
        operation: operation.clone(),
        issued_at: now,
        expires_at: now.checked_add(300).unwrap(),
    });
    let freshness = test_owner_sign(Freshness {
        header,
        issued_at: now,
        expires_at: now.checked_add(300).unwrap(),
    });
    NodeRequestBinding {
        certificate: cert.clone(),
        lease,
        freshness,
        operation,
    }
}
#[cfg(target_os = "linux")]
#[test]
fn actual_node_request_proof_admits_once_and_revocation_persists_across_authority_restart() {
    let temp = Temp::new();
    let (mut authority, original, current, cert) = enrolled_real_node(&temp, 1000);
    let now = Now(Timestamp::new(1000).unwrap());
    let binding = real_request_binding(&current, &cert, 1000, 1);
    let revision = authority.snapshot().unwrap().revision();
    let proof = create_node_request(
        &temp.0.join("real-node"),
        cert.body.agent_id,
        &current,
        binding.clone(),
        now,
    )
    .unwrap();
    assert_eq!(
        authority.snapshot().unwrap().revision(),
        revision,
        "signing itself must not admit or register"
    );
    assert_eq!(proof.body.lease_hash, binding.lease.body.digest().unwrap());
    let permit = authority
        .admit(
            binding.lease.clone(),
            proof.clone(),
            Some(&binding.freshness),
            &binding.operation,
            now,
        )
        .unwrap();
    authority.check_permit(&permit, now).unwrap();
    let another = create_node_request(
        &temp.0.join("real-node"),
        cert.body.agent_id,
        &current,
        binding.clone(),
        now,
    )
    .unwrap();
    assert_ne!(proof.body.nonce, another.body.nonce);
    assert!(matches!(
        authority.admit(
            binding.lease,
            another,
            Some(&binding.freshness),
            &binding.operation,
            now
        ),
        Err(Error::Replay)
    ));
    let unused = real_request_binding(&current, &cert, 1000, 2);
    let proof = create_node_request(
        &temp.0.join("real-node"),
        cert.body.agent_id,
        &current,
        unused.clone(),
        now,
    )
    .unwrap();
    authority
        .revoke(
            test_owner_sign(Revocation {
                header: Header {
                    version: 1,
                    context: current.context.clone(),
                    owner_epoch: 1,
                    checkpoint: 3,
                },
                agent_id: cert.body.agent_id,
                key_epoch: 1,
                certificate_hash: cert.body.digest().unwrap(),
                issued_at: now.0,
            }),
            now,
        )
        .unwrap();
    assert!(matches!(
        authority.check_permit(&permit, now),
        Err(Error::WrongCheckpoint)
    ));
    drop(authority);
    let mut authority = Actor::new(
        FileStore::open(
            temp.0.join("real-authority-store"),
            temp.0.join("real-authority-anchor"),
            original,
        )
        .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        authority.admit(
            unused.lease,
            proof,
            Some(&unused.freshness),
            &unused.operation,
            now
        ),
        Err(Error::WrongCheckpoint)
    ));
}
#[cfg(target_os = "linux")]
#[test]
fn typed_node_request_rejects_foreign_certificate_resource_scope_context_and_expiry() {
    use ed25519_dalek::SigningKey;
    let temp = Temp::new();
    let (_authority, _original, current, cert) = enrolled_real_node(&temp, 1000);
    let now = Now(Timestamp::new(1000).unwrap());
    let original = real_request_binding(&current, &cert, 1000, 1);
    for case in [
        "resource",
        "certificate-key",
        "certificate-hash",
        "scope",
        "epoch",
        "freshness-checkpoint",
        "context",
        "forged-signature",
    ] {
        let mut binding = original.clone();
        let expected = match case {
            "resource" => {
                binding.operation = Operation::TelemetryRead {
                    source: ResourceId::new("77777777-7777-4777-8777-777777777777").unwrap(),
                };
                Error::ScopeDenied
            }
            "certificate-key" => {
                binding.certificate.body.agent_key = PublicKey::from_bytes(
                    SigningKey::from_bytes(&[22; 32]).verifying_key().to_bytes(),
                );
                binding.certificate = test_owner_sign(binding.certificate.body);
                Error::WrongClaim
            }
            "certificate-hash" => {
                binding.lease.body.certificate_hash = Hash::from_bytes([99; 32]);
                binding.lease = test_owner_sign(binding.lease.body);
                Error::WrongClaim
            }
            "scope" => {
                binding.lease.body.operation = Operation::JobsExecute {
                    manifest: Hash::from_bytes([99; 32]),
                };
                binding.operation = binding.lease.body.operation.clone();
                binding.lease = test_owner_sign(binding.lease.body);
                Error::ScopeDenied
            }
            "epoch" => {
                binding.lease.body.key_epoch = 2;
                binding.lease = test_owner_sign(binding.lease.body);
                Error::WrongEpoch
            }
            "freshness-checkpoint" => {
                binding.freshness.body.header.checkpoint = 1;
                binding.freshness = test_owner_sign(binding.freshness.body);
                Error::WrongCheckpoint
            }
            "context" => {
                binding.lease.body.header.context.deployment_id =
                    DeploymentId::new("99999999-9999-4999-8999-999999999999").unwrap();
                binding.lease = test_owner_sign(binding.lease.body);
                Error::WrongContext
            }
            "forged-signature" => {
                binding.lease.signature = SignatureBytes::from_bytes([0; 64]);
                Error::InvalidSignature
            }
            _ => unreachable!(),
        };
        assert!(
            matches!(create_node_request(&temp.0.join("real-node"),cert.body.agent_id,&current,binding,now),Err(error) if error==expected),
            "accepted or wrong error {case}"
        );
    }
    assert!(matches!(
        create_node_request(
            &temp.0.join("real-node"),
            cert.body.agent_id,
            &current,
            original.clone(),
            Now(Timestamp::new(1300).unwrap())
        ),
        Err(Error::Expired)
    ));
    assert!(matches!(
        create_node_request(
            &temp.0.join("absent"),
            cert.body.agent_id,
            &current,
            original,
            now
        ),
        Err(Error::Storage)
    ));
    assert!(!temp.0.join("absent").exists());
}
#[cfg(target_os = "linux")]
#[test]
fn node_request_cli_uses_local_clock_and_produces_only_typed_node_principal_proof() {
    let temp = Temp::new();
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let (mut authority, _original, current, cert) = enrolled_real_node(&temp, seconds);
    let binding = real_request_binding(&current, &cert, seconds, 1);
    let pin_path = temp.0.join("request-owner-pin.json");
    std::fs::write(&pin_path, serde_json::to_vec(&current).unwrap()).unwrap();
    let call = |request: &Value| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_cc-agent-identity"))
            .args(["sign-request", "--state-dir"])
            .arg(temp.0.join("real-node"))
            .args([
                "--agent-id",
                "44444444-4444-4444-8444-444444444444",
                "--owner-pin",
            ])
            .arg(&pin_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let _ = child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(request).unwrap());
        child.wait_with_output().unwrap()
    };
    let request = serde_json::to_value(&binding).unwrap();
    let signed = call(&request);
    assert!(signed.status.success(), "{signed:?}");
    let proof = Signed::<AgentRequest>::from_json(&signed.stdout).unwrap();
    let observed = Now(Timestamp::new(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap());
    authority
        .admit(
            binding.lease,
            proof,
            Some(&binding.freshness),
            &binding.operation,
            observed,
        )
        .unwrap();
    let mut spoof = request;
    spoof["now"] = json!(seconds - 600);
    let denied = call(&spoof);
    assert!(!denied.status.success());
    let event: Value = serde_json::from_slice(&denied.stderr).unwrap();
    assert_eq!(event["code"], "InvalidWire");
    let text = String::from_utf8(signed.stdout).unwrap();
    assert!(!text.contains("private") && !text.contains("seed") && !text.contains("token"));
}

#[cfg(target_os = "linux")]
#[test]
fn actual_two_os_keys_rotation_policy_works_but_v1_node_signer_has_no_epoch2_activation() {
    use ed25519_dalek::{Signer, SigningKey};
    let temp = Temp::new();
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let now = Now(Timestamp::new(seconds).unwrap());
    let (mut authority, original, current, first_cert) =
        enrolled_real_node_in(&temp, seconds, |pin, observed| {
            SqliteJournalStore::create(
                temp.0.join("rotation-db"),
                temp.0.join("rotation-anchor"),
                pin,
                observed,
                SqliteStorageBudget::default(),
            )
            .unwrap()
        });
    let agent_id = first_cert.body.agent_id;
    let standby_dir = temp.0.join("standby-node");
    let second = provision_node(&standby_dir, agent_id).unwrap();
    assert_ne!(second.public_key, first_cert.body.agent_key);
    assert_eq!(
        second.key_epoch, 1,
        "another v1 directory is not an activated epoch2"
    );
    let initial = real_request_binding(&current, &first_cert, seconds, 201);
    let initial_proof = create_node_request(
        &temp.0.join("real-node"),
        agent_id,
        &current,
        initial.clone(),
        now,
    )
    .unwrap();
    let old_permit = authority
        .admit(
            initial.lease,
            initial_proof,
            Some(&initial.freshness),
            &initial.operation,
            now,
        )
        .unwrap();
    let next_header = Header {
        checkpoint: current.checkpoint + 1,
        version: 1,
        context: current.context.clone(),
        owner_epoch: current.epoch,
    };
    let rotated = test_owner_sign(Certificate {
        header: next_header.clone(),
        agent_id,
        key_epoch: 2,
        agent_key: second.public_key,
        scopes: first_cert.body.scopes.clone(),
        capabilities: first_cert.body.capabilities.clone(),
        origin: CertificateOrigin::Rotation {
            previous_certificate: first_cert.body.digest().unwrap(),
            previous_key_epoch: 1,
        },
        not_before: now.0,
        expires_at: first_cert.body.expires_at,
    });
    // Closed test oracle only: two private records generated by this test in
    // its own temporary directory. Buffers zeroize; keys never leave the fixture
    // or get logged. No production signer/import/export API is added.
    let keys: Vec<SigningKey> = [temp.0.join("real-node"), standby_dir.clone()]
        .into_iter()
        .map(|directory| {
            let record = zeroize::Zeroizing::new(
                std::fs::read(directory.join("node-identity.key")).unwrap(),
            );
            assert_eq!(record.len(), 148);
            let mut seed = zeroize::Zeroizing::new([0u8; 32]);
            seed.copy_from_slice(&record[52..84]);
            SigningKey::from_bytes(&seed)
        })
        .collect();
    assert_eq!(
        keys[0].verifying_key().to_bytes(),
        *first_cert.body.agent_key.as_bytes()
    );
    assert_eq!(
        keys[1].verifying_key().to_bytes(),
        *second.public_key.as_bytes()
    );
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).unwrap();
    let body = RotationProof {
        header: next_header,
        agent_id,
        certificate_hash: rotated.body.digest().unwrap(),
        nonce: Nonce::from_bytes(nonce),
        issued_at: now.0,
    };
    let old = Signed {
        signature: SignatureBytes::from_bytes(
            keys[0].sign(&body.signing_bytes().unwrap()).to_bytes(),
        ),
        body: body.clone(),
    };
    let new_body = NewKeyProof(body.clone());
    let new = Signed {
        signature: SignatureBytes::from_bytes(
            keys[1].sign(&new_body.signing_bytes().unwrap()).to_bytes(),
        ),
        body: new_body,
    };
    let revision = authority.snapshot().unwrap().revision();
    let bad_new = Signed {
        signature: old.signature,
        body: NewKeyProof(body),
    };
    assert_eq!(
        authority.rotate(rotated.clone(), old.clone(), bad_new, now),
        Err(Error::InvalidSignature)
    );
    assert_eq!(authority.snapshot().unwrap().revision(), revision);
    authority.rotate(rotated.clone(), old, new, now).unwrap();
    assert!(matches!(
        authority.check_permit(&old_permit, now),
        Err(Error::WrongCheckpoint)
    ));
    let current = authority.snapshot().unwrap().owner().clone();
    let mut binding = real_request_binding(&current, &rotated, seconds, 202);
    binding.lease.body.key_epoch = 2;
    binding.lease = test_owner_sign(binding.lease.body);
    assert!(matches!(
        create_node_request(&standby_dir, agent_id, &current, binding.clone(), now),
        Err(Error::WrongEpoch)
    ));
    let pin_path = temp.0.join("rotation-owner-pin.json");
    std::fs::write(&pin_path, serde_json::to_vec(&current).unwrap()).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_cc-agent-identity"));
    command
        .args(["sign-request", "--state-dir"])
        .arg(&standby_dir)
        .args([
            "--agent-id",
            "44444444-4444-4444-8444-444444444444",
            "--owner-pin",
        ])
        .arg(&pin_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&binding).unwrap())
        .unwrap();
    let denied = child.wait_with_output().unwrap();
    assert!(!denied.status.success() && denied.stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&denied.stderr).unwrap()["code"],
        "WrongEpoch"
    );
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).unwrap();
    let body = AgentRequest {
        header: binding.lease.body.header.clone(),
        agent_id,
        key_epoch: 2,
        lease_hash: binding.lease.body.digest().unwrap(),
        nonce: Nonce::from_bytes(nonce),
        issued_at: now.0,
    };
    let proof = Signed {
        signature: SignatureBytes::from_bytes(
            keys[1].sign(&body.signing_bytes().unwrap()).to_bytes(),
        ),
        body,
    };
    // Current signed epoch1 request can still be constructed locally, but the
    // retained verified authority head rejects it: no old-key resurrection.
    let obsolete = real_request_binding(&current, &first_cert, seconds, 203);
    let obsolete_proof = create_node_request(
        &temp.0.join("real-node"),
        agent_id,
        &current,
        obsolete.clone(),
        now,
    )
    .unwrap();
    assert!(matches!(
        authority.admit(
            obsolete.lease,
            obsolete_proof,
            Some(&obsolete.freshness),
            &obsolete.operation,
            now
        ),
        Err(Error::WrongEpoch)
    ));
    drop(authority);
    let mut restored = Actor::new(
        SqliteJournalStore::open(
            temp.0.join("rotation-db"),
            temp.0.join("rotation-anchor"),
            original.clone(),
            SqliteStorageBudget::default(),
        )
        .unwrap(),
    )
    .unwrap();
    let permit = restored
        .admit(
            binding.lease.clone(),
            proof.clone(),
            Some(&binding.freshness),
            &binding.operation,
            now,
        )
        .unwrap();
    restored.check_permit(&permit, now).unwrap();
    drop(restored);
    let mut restored = Actor::new(
        SqliteJournalStore::open(
            temp.0.join("rotation-db"),
            temp.0.join("rotation-anchor"),
            original,
            SqliteStorageBudget::default(),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        restored.admit(
            binding.lease,
            proof,
            Some(&binding.freshness),
            &binding.operation,
            now
        ),
        Err(Error::Replay)
    ));
    let original_public = provision_node(&temp.0.join("real-node"), agent_id).unwrap();
    assert_eq!(original_public.public_key, first_cert.body.agent_key);
    assert_eq!(original_public.key_epoch, 1);
    assert_eq!(provision_node(&standby_dir, agent_id).unwrap(), second);
    println!("node rotation boundary: two actual OS keys / same AgentId; signed admin+old+new proof -> SQLite epoch2/restart/replay PASS; real v1 CLI WrongEpoch; original key preserved; no activation API or raw signer added");
}

#[cfg(target_os = "linux")]
struct SqlWorker {
    child: std::process::Child,
    input: Option<std::process::ChildStdin>,
    output: std::io::BufReader<std::process::ChildStdout>,
}

#[cfg(not(target_os = "linux"))]
#[test]
fn prepared_rotation_signer_is_unsupported_before_pin_or_private_state_access() {
    let temp = Temp::new();
    let result = Command::new(env!("CARGO_BIN_EXE_cc-agent-identity"))
        .args(["sign-prepared-rotation", "--state-dir"])
        .arg(temp.0.join("missing-node"))
        .args([
            "--agent-id",
            "44444444-4444-4444-8444-444444444444",
            "--rotation-id",
            &"11".repeat(16),
            "--owner-pin",
        ])
        .arg(temp.0.join("missing-pin.json"))
        .output()
        .unwrap();
    assert!(!result.status.success() && result.stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stderr).unwrap()["code"],
        "CapabilityDenied"
    );
    assert_eq!(std::fs::read_dir(&temp.0).unwrap().count(), 0);
}

#[cfg(not(target_os = "linux"))]
#[test]
fn activation_is_unsupported_before_pin_sqlite_or_private_state_access() {
    let temp = Temp::new();
    let result = Command::new(env!("CARGO_BIN_EXE_cc-agent-identity"))
        .args([
            "activate-prepared-rotation",
            "--provider",
            "sqlite",
            "--state-dir",
        ])
        .arg(temp.0.join("missing-node"))
        .args([
            "--agent-id",
            "44444444-4444-4444-8444-444444444444",
            "--rotation-id",
        ])
        .arg("11".repeat(16))
        .arg("--store")
        .arg(temp.0.join("missing-db"))
        .arg("--anchor")
        .arg(temp.0.join("missing-anchor"))
        .arg("--owner-pin")
        .arg(temp.0.join("missing-pin"))
        .output()
        .unwrap();
    assert!(!result.status.success() && result.stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stderr).unwrap()["code"],
        "CapabilityDenied"
    );
    assert_eq!(std::fs::read_dir(&temp.0).unwrap().count(), 0);
}

#[cfg(not(target_os = "linux"))]
#[test]
fn readonly_inspector_is_unsupported_without_any_path_or_guard_creation() {
    let temp = Temp::new();
    let path = temp.0.join("missing-node");
    assert!(matches!(
        inspect_node_state(
            &path,
            AgentId::new("44444444-4444-4444-8444-444444444444").unwrap()
        ),
        Err(Error::CapabilityDenied)
    ));
    let result = Command::new(env!("CARGO_BIN_EXE_cc-agent-identity"))
        .args(["inspect-node-state", "--state-dir"])
        .arg(&path)
        .args(["--agent-id", "44444444-4444-4444-8444-444444444444"])
        .output()
        .unwrap();
    assert!(!result.status.success() && result.stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stderr).unwrap()["code"],
        "CapabilityDenied"
    );
    assert_eq!(std::fs::read_dir(&temp.0).unwrap().count(), 0);
}

#[cfg(target_os = "linux")]
fn private_record_digests(path: &std::path::Path) -> std::collections::BTreeMap<String, [u8; 32]> {
    use sha2::Digest;
    std::fs::read_dir(path)
        .unwrap()
        .map(|v| {
            let p = v.unwrap().path();
            let bytes = zeroize::Zeroizing::new(std::fs::read(&p).unwrap());
            (
                p.file_name().unwrap().to_str().unwrap().to_owned(),
                sha2::Sha256::digest(bytes.as_slice()).into(),
            )
        })
        .collect()
}
#[cfg(target_os = "linux")]
fn assert_inspector_cli(
    node: &std::path::Path,
    agent: AgentId,
    layout: &str,
    state: &str,
    reader: u32,
    epoch: Option<u64>,
) {
    use std::os::unix::fs::MetadataExt;
    let before = private_record_digests(node);
    let metadata = std::fs::metadata(node.join(".node-keys.lock")).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_cc-agent-identity"))
        .args(["inspect-node-state", "--state-dir"])
        .arg(node)
        .arg("--agent-id")
        .arg(serde_json::to_value(agent).unwrap().as_str().unwrap())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "inspection denial: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stdout).unwrap(),
        json!({"format":1,"agentId":agent,
        "privateLayout":layout,"requiredReaderSchema":reader,"actualSelectedEpoch":epoch,"selectionState":state,
        "guard":{"dev":metadata.dev(),"ino":metadata.ino()}})
    );
    assert_eq!(private_record_digests(node), before);
}

#[cfg(target_os = "linux")]
#[test]
fn readonly_inspector_cli_denials_have_no_facts_or_repair_side_effects() {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let id = AgentId::new("44444444-4444-4444-8444-444444444444").unwrap();
    for case in [
        "missing-directory",
        "missing-key",
        "missing-guard",
        "guard-mode",
        "directory-mode",
        "unknown",
        "foreign-agent",
        "relative",
    ] {
        let temp = Temp::new();
        let node = temp.0.join("node");
        if case != "missing-directory" {
            provision_node(&node, id).unwrap();
        }
        match case {
            "directory-mode" => {
                std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o500)).unwrap()
            }
            "missing-key" => std::fs::remove_file(node.join("node-identity.key")).unwrap(),
            "missing-guard" => std::fs::remove_file(node.join(".node-keys.lock")).unwrap(),
            "guard-mode" => std::fs::set_permissions(
                node.join(".node-keys.lock"),
                std::fs::Permissions::from_mode(0o644),
            )
            .unwrap(),
            "unknown" => {
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(node.join("future-state"))
                    .unwrap();
            }
            _ => (),
        }
        let before = if node.exists() {
            Some(private_record_digests(&node))
        } else {
            None
        };
        let result = Command::new(env!("CARGO_BIN_EXE_cc-agent-identity"))
            .args(["inspect-node-state", "--state-dir"])
            .arg(if case == "relative" {
                PathBuf::from("relative-node-inspector-deny")
            } else {
                node.clone()
            })
            .args([
                "--agent-id",
                if case == "foreign-agent" {
                    "99999999-9999-4999-8999-999999999999"
                } else {
                    "44444444-4444-4444-8444-444444444444"
                },
            ])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            !result.status.success() && result.stdout.is_empty(),
            "{case}"
        );
        let event: Value = serde_json::from_slice(&result.stderr).unwrap();
        assert_eq!(event["event"], "identityDenied");
        assert!(result.stderr.len() < 512);
        assert_eq!(
            if node.exists() {
                Some(private_record_digests(&node))
            } else {
                None
            },
            before
        );
        if case == "missing-guard" {
            assert!(!node.join(".node-keys.lock").exists());
        }
        if case == "directory-mode" {
            assert_eq!(
                std::fs::metadata(&node).unwrap().permissions().mode() & 0o777,
                0o500
            );
            std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o700)).unwrap();
            // owned fixture cleanup
        }
    }
}
#[cfg(target_os = "linux")]
#[test]
fn actual_prepared_rotation_signer_value_commits_once_and_new_epoch2_oracle_is_not_activation() {
    run_prepared_rotation(false);
}
#[cfg(target_os = "linux")]
#[test]
fn actual_activation_cli_selects_epoch2_typed_cli_request_and_restart_without_seed_oracle() {
    run_prepared_rotation(true);
}
#[cfg(target_os = "linux")]
fn run_prepared_rotation(activation: bool) {
    use ed25519_dalek::{Signer, SigningKey};
    for owner_rotated in [false, true] {
        let temp = Temp::new();
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let initial = seconds - 120;
        let (mut authority, original, mut current, previous) =
            enrolled_real_node_in(&temp, initial, |pin, now| {
                SqliteJournalStore::create(
                    temp.0.join("worker-db"),
                    temp.0.join("worker-anchor"),
                    pin,
                    now,
                    SqliteStorageBudget::default(),
                )
                .unwrap()
            });
        let node = temp.0.join("real-node");
        if activation {
            assert_inspector_cli(
                &node,
                previous.body.agent_id,
                "legacyV1",
                "legacy1",
                1,
                Some(1),
            );
        }
        let prepared = prepare_node_rotation(
            &node,
            previous.body.agent_id,
            previous.body.agent_key.fingerprint(),
        )
        .unwrap();
        if activation {
            assert_inspector_cli(
                &node,
                previous.body.agent_id,
                "preparedV2",
                "prepared1",
                1,
                Some(1),
            );
        }
        if owner_rotated {
            let change = OwnerRotation {
                header: Header {
                    checkpoint: 3,
                    ..previous.body.header.clone()
                },
                new_epoch: 2,
                new_key: PublicKey::from_bytes(
                    SigningKey::from_bytes(&[55; 32]).verifying_key().to_bytes(),
                ),
                issued_at: Timestamp::new(initial).unwrap(),
                expires_at: Timestamp::new(initial + 500).unwrap(),
            };
            authority
                .rotate_owner(
                    test_owner_sign(change.clone()),
                    rotated_owner_sign(NewOwnerProof(change)),
                    Now(Timestamp::new(initial).unwrap()),
                )
                .unwrap();
            current = authority.snapshot().unwrap().owner().clone();
        }
        let proposed = Certificate {
            header: Header {
                checkpoint: current.checkpoint + 1,
                version: 1,
                context: current.context.clone(),
                owner_epoch: current.epoch,
            },
            agent_id: previous.body.agent_id,
            key_epoch: 2,
            agent_key: prepared.new_public_key,
            scopes: previous.body.scopes.clone(),
            capabilities: previous.body.capabilities.clone(),
            origin: CertificateOrigin::Rotation {
                previous_certificate: previous.body.digest().unwrap(),
                previous_key_epoch: 1,
            },
            not_before: Timestamp::new(seconds).unwrap(),
            expires_at: previous.body.expires_at.min(current.expires_at),
        };
        let proposed = if owner_rotated {
            rotated_owner_sign(proposed)
        } else {
            test_owner_sign(proposed)
        };
        let mut input = PreparedRotationInput {
            previous_certificate: previous.clone(),
            certificate: proposed.clone(),
        };
        if owner_rotated {
            input.previous_certificate.signature = SignatureBytes::from_bytes([0; 64]);
        }
        let before = private_record_digests(&node);
        let revision = authority.snapshot().unwrap().revision();
        let first = sign_prepared_node_rotation(
            &node,
            previous.body.agent_id,
            &prepared.rotation_id,
            &current,
            input.clone(),
            Now(Timestamp::new(seconds).unwrap()),
        )
        .unwrap();
        assert_eq!(private_record_digests(&node), before);
        assert_eq!(authority.snapshot().unwrap().revision(), revision);
        let pin_path = temp.0.join("signer-current-pin.json");
        std::fs::write(&pin_path, serde_json::to_vec(&current).unwrap()).unwrap();
        std::fs::write(
            temp.0.join("worker-owner-pin.json"),
            serde_json::to_vec(&original).unwrap(),
        )
        .unwrap();
        drop(authority);
        let call = |bytes: &[u8]| {
            let mut child = Command::new(env!("CARGO_BIN_EXE_cc-agent-identity"))
                .args(["sign-prepared-rotation", "--state-dir"])
                .arg(&node)
                .args([
                    "--agent-id",
                    "44444444-4444-4444-8444-444444444444",
                    "--rotation-id",
                ])
                .arg(&prepared.rotation_id)
                .arg("--owner-pin")
                .arg(&pin_path)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(bytes).unwrap();
            child.wait_with_output().unwrap()
        };
        let raw = serde_json::to_vec(&input).unwrap();
        let result = call(&raw);
        assert!(result.status.success(), "{result:?}");
        let proofs: PreparedRotationProofs = serde_json::from_slice(&result.stdout).unwrap();
        assert_ne!(first.old.body.nonce, proofs.old.body.nonce);
        assert_eq!(proofs.old.body, proofs.new.body.0);
        for payload in [
            format!(
                "{{\"now\":{seconds},{}",
                std::str::from_utf8(&raw).unwrap().trim_start_matches('{')
            ),
            format!(
                "{{\"certificate\":{},{}",
                serde_json::to_string(&input.certificate).unwrap(),
                std::str::from_utf8(&raw).unwrap().trim_start_matches('{')
            ),
        ] {
            let denied = call(payload.as_bytes());
            assert!(!denied.status.success() && denied.stdout.is_empty());
        }
        assert_eq!(private_record_digests(&node), before);
        assert!(matches!(
            sign_prepared_node_rotation(
                &node,
                previous.body.agent_id,
                &prepared.rotation_id,
                &current,
                input,
                Now(Timestamp::new(seconds - 61).unwrap()),
            ),
            Err(Error::InvalidTime)
        ));
        let mut worker = SqlWorker::new(&temp);
        let accepted =
            worker.send(&json!({"action":"mutate","mutation":{"kind":"Rotate","value":proofs}}));
        assert_eq!(
            accepted["ok"], true,
            "rotation denial code: {}",
            accepted["code"]
        );
        assert_eq!(
            worker.send(&json!({"action":"mutate","mutation":{"kind":"Rotate","value":first}}))
                ["ok"],
            false
        );
        worker.finish();
        let authority = Actor::new(
            SqliteJournalStore::open(
                temp.0.join("worker-db"),
                temp.0.join("worker-anchor"),
                original.clone(),
                SqliteStorageBudget::default(),
            )
            .unwrap(),
        )
        .unwrap();
        let new_pin = authority.snapshot().unwrap().owner().clone();
        assert_eq!(
            authority
                .snapshot()
                .unwrap()
                .certificate(previous.body.agent_id)
                .unwrap(),
            &proposed.body
        );
        drop(authority);
        // Closed test oracle for ONLY epoch2 AgentRequest. Product selection is
        // still old1: this does not implement activation or a raw signer API.
        let mut binding = real_request_binding(&new_pin, &proposed, seconds, 221);
        binding.lease.body.key_epoch = 2;
        if owner_rotated {
            binding.lease = rotated_owner_sign(binding.lease.body);
            binding.freshness = rotated_owner_sign(binding.freshness.body);
        } else {
            binding.lease = test_owner_sign(binding.lease.body);
        }
        let proof = if activation {
            let assertions = LocalActivationInput {
                previous_certificate: previous.body.clone(),
                committed_certificate: proposed.body.clone(),
            };
            let activation_call = |raw: &[u8]| {
                let mut child = Command::new(env!("CARGO_BIN_EXE_cc-agent-identity"))
                    .args([
                        "activate-prepared-rotation",
                        "--provider",
                        "sqlite",
                        "--state-dir",
                    ])
                    .arg(&node)
                    .args([
                        "--agent-id",
                        "44444444-4444-4444-8444-444444444444",
                        "--rotation-id",
                    ])
                    .arg(&prepared.rotation_id)
                    .arg("--store")
                    .arg(temp.0.join("worker-db"))
                    .arg("--anchor")
                    .arg(temp.0.join("worker-anchor"))
                    .arg("--owner-pin")
                    .arg(temp.0.join("worker-owner-pin.json"))
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                child.stdin.take().unwrap().write_all(raw).unwrap();
                child.wait_with_output().unwrap()
            };
            let raw = serde_json::to_vec(&assertions).unwrap();
            for bad in [
                format!(
                    "{{\"now\":{seconds},{}",
                    std::str::from_utf8(&raw).unwrap().trim_start_matches('{')
                ),
                serde_json::to_string(
                    &json!({"previousCertificate":previous,"committedCertificate":proposed}),
                )
                .unwrap(),
            ] {
                let denied = activation_call(bad.as_bytes());
                assert!(!denied.status.success() && denied.stdout.is_empty());
            }
            assert_eq!(private_record_digests(&node), before);
            let result = activation_call(&raw);
            assert!(
                result.status.success(),
                "activation error: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            let observed: Value = serde_json::from_slice(&result.stdout).unwrap();
            assert_eq!(
                observed,
                json!({"format":3,"agentId":previous.body.agent_id,"rotationId":prepared.rotation_id,
                "selectedKeyEpoch":2,"keyFingerprint":prepared.new_key_fingerprint,"certificateHash":proposed.body.digest().unwrap(),"localOnly":true})
            );
            let repeated = activation_call(&raw);
            assert!(repeated.status.success());
            assert_eq!(repeated.stdout, result.stdout);
            assert_inspector_cli(
                &node,
                previous.body.agent_id,
                "activationV3",
                "selected2",
                3,
                Some(2),
            );
            // Explicit own-fixture simulation of the documented resumable class.
            // This public bookkeeping leaf contains no private seed material.
            std::fs::remove_file(node.join("node-activation.state.v3")).unwrap();
            assert_inspector_cli(
                &node,
                previous.body.agent_id,
                "activationV3",
                "activationPending",
                3,
                None,
            );
            assert!(!node.join("node-activation.state.v3").exists());
            let resumed = activation_call(&raw);
            assert!(resumed.status.success());
            assert_eq!(resumed.stdout, result.stdout);
            assert!(prepare_node_rotation(
                &node,
                previous.body.agent_id,
                previous.body.agent_key.fingerprint()
            )
            .is_err());
            assert!(sign_prepared_node_rotation(
                &node,
                previous.body.agent_id,
                &prepared.rotation_id,
                &new_pin,
                PreparedRotationInput {
                    previous_certificate: previous.clone(),
                    certificate: proposed.clone()
                },
                Now(Timestamp::new(seconds).unwrap())
            )
            .is_err());
            std::fs::write(&pin_path, serde_json::to_vec(&new_pin).unwrap()).unwrap();
            let mut child = Command::new(env!("CARGO_BIN_EXE_cc-agent-identity"))
                .args(["sign-request", "--state-dir"])
                .arg(&node)
                .args([
                    "--agent-id",
                    "44444444-4444-4444-8444-444444444444",
                    "--owner-pin",
                ])
                .arg(&pin_path)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            // Current pin is unchanged by local activation and already describes
            // the authority checkpoint after the explicit Rotate mutation.
            child
                .stdin
                .take()
                .unwrap()
                .write_all(&serde_json::to_vec(&binding).unwrap())
                .unwrap();
            let signed = child.wait_with_output().unwrap();
            assert!(
                signed.status.success(),
                "epoch2 sign error: {}",
                String::from_utf8_lossy(&signed.stderr)
            );
            serde_json::from_slice::<Signed<AgentRequest>>(&signed.stdout).unwrap()
        } else {
            let record = zeroize::Zeroizing::new(
                std::fs::read(node.join("node-rotation.pending.v2")).unwrap(),
            );
            let mut seed = zeroize::Zeroizing::new([0u8; 32]);
            seed.copy_from_slice(&record[116..148]);
            let key = SigningKey::from_bytes(&seed);
            assert_eq!(
                key.verifying_key().to_bytes(),
                *prepared.new_public_key.as_bytes()
            );
            let mut nonce = [0u8; 16];
            getrandom::fill(&mut nonce).unwrap();
            let body = AgentRequest {
                header: binding.lease.body.header.clone(),
                agent_id: previous.body.agent_id,
                key_epoch: 2,
                lease_hash: binding.lease.body.digest().unwrap(),
                nonce: Nonce::from_bytes(nonce),
                issued_at: Timestamp::new(seconds).unwrap(),
            };
            Signed {
                signature: SignatureBytes::from_bytes(
                    key.sign(&body.signing_bytes().unwrap()).to_bytes(),
                ),
                body,
            }
        };
        let mut worker = SqlWorker::new(&temp);
        let frame = json!({"action":"admit","lease":binding.lease,"proof":proof,"freshness":binding.freshness,"operation":binding.operation});
        assert_eq!(worker.send(&frame)["ok"], true);
        worker.finish();
        let mut worker = SqlWorker::new(&temp);
        assert_eq!(worker.send(&frame), json!({"ok":false,"code":"Replay"}));
        worker.finish();
        let after = private_record_digests(&node);
        for (name, hash) in &before {
            assert_eq!(after.get(name), Some(hash));
        }
        if !activation {
            assert_eq!(after, before);
        }
        let public = provision_node(&node, previous.body.agent_id).unwrap();
        assert_eq!(public.key_epoch, if activation { 2 } else { 1 });
        assert_eq!(
            public.public_key,
            if activation {
                prepared.new_public_key
            } else {
                previous.body.agent_key
            }
        );
        if activation {
            let (pin, offer) = node_offer(&serde_json::to_value(&public).unwrap(), seconds);
            assert!(matches!(
                create_node_claim(
                    &node,
                    previous.body.agent_id,
                    &pin,
                    offer,
                    Now(Timestamp::new(seconds).unwrap())
                ),
                Err(Error::WrongState)
            ));
        }
        println!("typed signer actual OS keys: ownerHistorical={owner_rotated}; exact Rotate.value accepted, duplicate denied, epoch2 admit/restart replay denied, activation={activation}/immutable old and v2 files unchanged");
    }
}

#[cfg(not(target_os = "linux"))]
#[test]
fn prepare_is_unsupported_without_loading_or_creating_linux_private_state() {
    let temp = Temp::new();
    let path = temp.0.join("must-not-create");
    assert_eq!(
        prepare_node_rotation(
            &path,
            AgentId::new("44444444-4444-4444-8444-444444444444").unwrap(),
            Hash::from_bytes([11; 32])
        ),
        Err(Error::CapabilityDenied)
    );
    let result = Command::new(env!("CARGO_BIN_EXE_cc-agent-identity"))
        .args(["prepare-node-rotation", "--state-dir"])
        .arg(&path)
        .args([
            "--agent-id",
            "44444444-4444-4444-8444-444444444444",
            "--expected-active-fingerprint",
            &"11".repeat(32),
        ])
        .output()
        .unwrap();
    assert!(!result.status.success() && result.stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stderr).unwrap()["code"],
        "CapabilityDenied"
    );
    assert!(!path.exists());
}

#[cfg(target_os = "linux")]
#[test]
fn valid_prepared_candidate_keeps_actual_typed_cli_worker_admission_on_old_key_only() {
    let temp = Temp::new();
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let (_, current, cert) = sqlite_real_node(&temp, seconds);
    let node = temp.0.join("real-node");
    let before = zeroize::Zeroizing::new(std::fs::read(node.join("node-identity.key")).unwrap());
    let call_prepare = || {
        Command::new(env!("CARGO_BIN_EXE_cc-agent-identity"))
            .args(["prepare-node-rotation", "--state-dir"])
            .arg(&node)
            .args([
                "--agent-id",
                "44444444-4444-4444-8444-444444444444",
                "--expected-active-fingerprint",
            ])
            .arg(hex::encode(cert.body.agent_key.fingerprint().as_bytes()))
            .output()
            .unwrap()
    };
    let first = call_prepare();
    assert!(first.status.success(), "{first:?}");
    let prepared: PreparedNodeKeyPublic = serde_json::from_slice(&first.stdout).unwrap();
    let second = call_prepare();
    assert!(second.status.success());
    assert_eq!(first.stdout, second.stdout);
    assert_ne!(prepared.new_public_key, cert.body.agent_key);
    assert_eq!(
        provision_node(&node, cert.body.agent_id)
            .unwrap()
            .public_key,
        cert.body.agent_key
    );
    assert_eq!(
        *zeroize::Zeroizing::new(std::fs::read(node.join("node-identity.key")).unwrap()),
        *before
    );
    let pin = temp.0.join("prepared-owner-pin.json");
    std::fs::write(&pin, serde_json::to_vec(&current).unwrap()).unwrap();
    let binding = real_request_binding(&current, &cert, seconds, 211);
    let sign = |binding: &NodeRequestBinding| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_cc-agent-identity"))
            .args(["sign-request", "--state-dir"])
            .arg(&node)
            .args([
                "--agent-id",
                "44444444-4444-4444-8444-444444444444",
                "--owner-pin",
            ])
            .arg(&pin)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(binding).unwrap())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    let signed = sign(&binding);
    assert!(signed.status.success(), "{signed:?}");
    let proof: Signed<AgentRequest> = serde_json::from_slice(&signed.stdout).unwrap();
    assert_eq!(proof.body.key_epoch, 1);
    let mut worker = SqlWorker::new(&temp);
    let frame = json!({"action":"admit","lease":binding.lease,"proof":proof,"freshness":binding.freshness,"operation":binding.operation});
    assert_eq!(worker.send(&frame)["ok"], true);
    assert_eq!(worker.send(&frame), json!({"ok":false,"code":"Replay"}));
    worker.finish();
    let selector = node.join("node-rotation.state.v2");
    let mut corrupted: Value = serde_json::from_slice(&std::fs::read(&selector).unwrap()).unwrap();
    corrupted["phase"] = json!("Active2");
    std::fs::write(selector, serde_json::to_vec(&corrupted).unwrap()).unwrap();
    let next = real_request_binding(&current, &cert, seconds, 212);
    let denied = sign(&next);
    assert!(!denied.status.success() && denied.stdout.is_empty());
    assert_eq!(
        *zeroize::Zeroizing::new(std::fs::read(node.join("node-identity.key")).unwrap()),
        *before
    );
}
#[cfg(target_os = "linux")]
impl SqlWorker {
    fn new(temp: &Temp) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_cc-agent-identity"))
            .args(["worker-sqlite", "--db-dir"])
            .arg(temp.0.join("worker-db"))
            .arg("--anchor-dir")
            .arg(temp.0.join("worker-anchor"))
            .arg("--owner-pin")
            .arg(temp.0.join("worker-owner-pin.json"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let output = std::io::BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            input,
            output,
        }
    }
    fn raw(&mut self, bytes: &[u8]) -> Value {
        use std::io::{BufRead, Read};
        let _ = self.input.as_mut().unwrap().write_all(bytes);
        let _ = self.input.as_mut().unwrap().flush();
        let mut output = Vec::new();
        self.output
            .by_ref()
            .take(4097)
            .read_until(b'\n', &mut output)
            .unwrap();
        assert!(
            !output.is_empty() && output.len() <= 4096 && output.ends_with(b"\n"),
            "invalid bounded worker frame"
        );
        serde_json::from_slice(&output).unwrap()
    }
    fn send(&mut self, value: &Value) -> Value {
        let mut bytes = serde_json::to_vec(value).unwrap();
        bytes.push(b'\n');
        self.raw(&bytes)
    }
    fn exited(&mut self) -> std::process::ExitStatus {
        let began = std::time::Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(
                began.elapsed() < std::time::Duration::from_secs(5),
                "own worker exit deadline"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    fn finish(&mut self) {
        drop(self.input.take());
        assert!(self.exited().success());
    }
}
#[cfg(target_os = "linux")]
impl Drop for SqlWorker {
    fn drop(&mut self) {
        drop(self.input.take());
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
#[cfg(target_os = "linux")]
fn sqlite_real_node(temp: &Temp, seconds: u64) -> (OwnerPin, OwnerPin, Signed<Certificate>) {
    let (actor, pin, current, cert) = enrolled_real_node_in(temp, seconds, |pin, now| {
        SqliteJournalStore::create(
            temp.0.join("worker-db"),
            temp.0.join("worker-anchor"),
            pin,
            now,
            SqliteStorageBudget::default(),
        )
        .unwrap()
    });
    drop(actor);
    std::fs::write(
        temp.0.join("worker-owner-pin.json"),
        serde_json::to_vec(&pin).unwrap(),
    )
    .unwrap();
    (pin, current, cert)
}
#[cfg(target_os = "linux")]
fn rotated_real_node() -> (
    Temp,
    OwnerPin,
    OwnerPin,
    Signed<Certificate>,
    NodeRequestBinding,
) {
    use ed25519_dalek::{Signer, SigningKey};
    let temp = Temp::new();
    let now = Now(Timestamp::new(1000).unwrap());
    let (mut authority, original, _, cert) = enrolled_real_node_in(&temp, 1000, |pin, observed| {
        SqliteJournalStore::create(
            temp.0.join("continuity-db"),
            temp.0.join("continuity-anchor"),
            pin,
            observed,
            SqliteStorageBudget::default(),
        )
        .unwrap()
    });
    // Synthetic owners are test-only; the node above was provisioned from OS
    // entropy and its private file remains in this owning test directory.
    let replacement_owner = SigningKey::from_bytes(&[55; 32]);
    let change = OwnerRotation {
        header: Header {
            version: 1,
            context: original.context.clone(),
            owner_epoch: 1,
            checkpoint: 3,
        },
        new_epoch: 2,
        new_key: PublicKey::from_bytes(replacement_owner.verifying_key().to_bytes()),
        issued_at: now.0,
        // A rotation may narrow owner expiry without changing the already
        // verified certificate. The new lease must fit both live ceilings.
        expires_at: Timestamp::new(1500).unwrap(),
    };
    let new_body = NewOwnerProof(change.clone());
    let new_proof = Signed {
        signature: SignatureBytes::from_bytes(
            replacement_owner
                .sign(&new_body.signing_bytes().unwrap())
                .to_bytes(),
        ),
        body: new_body,
    };
    authority
        .rotate_owner(test_owner_sign(change), new_proof, now)
        .unwrap();
    let current = authority.snapshot().unwrap().owner().clone();
    assert_eq!(current.epoch, 2);
    assert!(cert.body.expires_at > current.expires_at);
    let mut binding = real_request_binding(&current, &cert, 1000, 91);
    binding.lease.signature = SignatureBytes::from_bytes(
        replacement_owner
            .sign(&binding.lease.body.signing_bytes().unwrap())
            .to_bytes(),
    );
    binding.freshness.signature = SignatureBytes::from_bytes(
        replacement_owner
            .sign(&binding.freshness.body.signing_bytes().unwrap())
            .to_bytes(),
    );
    drop(authority);
    (temp, original, current, cert, binding)
}
#[cfg(target_os = "linux")]
fn continuity_actor(temp: &Temp, original: &OwnerPin) -> Actor<SqliteJournalStore> {
    Actor::new(
        SqliteJournalStore::open(
            temp.0.join("continuity-db"),
            temp.0.join("continuity-anchor"),
            original.clone(),
            SqliteStorageBudget::default(),
        )
        .unwrap(),
    )
    .unwrap()
}
#[cfg(target_os = "linux")]
fn rotated_owner_sign<T: Statement>(body: T) -> Signed<T> {
    use ed25519_dalek::{Signer, SigningKey};
    Signed {
        signature: SignatureBytes::from_bytes(
            SigningKey::from_bytes(&[55; 32])
                .sign(&body.signing_bytes().unwrap())
                .to_bytes(),
        ),
        body,
    }
}
#[cfg(target_os = "linux")]
#[test]
fn sqlite_owner_rotation_typed_actual_node_continuity_survives_restart_and_replay_denial() {
    let (temp, original, current, cert, binding) = rotated_real_node();
    let now = Now(Timestamp::new(1000).unwrap());
    let proof = create_node_request(
        &temp.0.join("real-node"),
        cert.body.agent_id,
        &current,
        binding.clone(),
        now,
    )
    .unwrap();
    // Restart from the original pin forces signed rotation/certificate replay.
    let mut authority = continuity_actor(&temp, &original);
    assert!(matches!(
        authority.admit(
            test_owner_sign(binding.lease.body.clone()),
            proof.clone(),
            Some(&binding.freshness),
            &binding.operation,
            now,
        ),
        Err(Error::InvalidSignature)
    ));
    assert!(matches!(
        authority.admit(
            binding.lease.clone(),
            proof.clone(),
            Some(&test_owner_sign(binding.freshness.body.clone())),
            &binding.operation,
            now,
        ),
        Err(Error::InvalidSignature)
    ));
    let permit = authority
        .admit(
            binding.lease.clone(),
            proof.clone(),
            Some(&binding.freshness),
            &binding.operation,
            now,
        )
        .unwrap();
    authority.check_permit(&permit, now).unwrap();
    drop(authority);
    let mut restarted = continuity_actor(&temp, &original);
    assert!(matches!(
        restarted.admit(
            binding.lease,
            proof,
            Some(&binding.freshness),
            &binding.operation,
            now,
        ),
        Err(Error::Replay)
    ));
    println!("owner continuity fixed: typed actual OS-node proof admitted after signed SQLite restart; old owner signatures denied; replay denied after second restart; narrowed owner expiry retained");
}

#[cfg(target_os = "linux")]
#[test]
fn owner_continuity_endorsement_rejects_mismatches_without_promoting_wrapper_or_actor_authority() {
    use ed25519_dalek::SigningKey;
    let (temp, original, current, cert, binding) = rotated_real_node();
    let now = Now(Timestamp::new(1000).unwrap());
    let sign = |binding: NodeRequestBinding, pin: &OwnerPin, observed| {
        create_node_request(
            &temp.0.join("real-node"),
            cert.body.agent_id,
            pin,
            binding,
            observed,
        )
    };
    type Mutation = fn(&mut NodeRequestBinding);
    let cases: &[(&str, Mutation, Error)] = &[
        (
            "future owner epoch",
            |b| b.certificate.body.header.owner_epoch = 3,
            Error::WrongEpoch,
        ),
        (
            "future checkpoint",
            |b| b.certificate.body.header.checkpoint = 4,
            Error::WrongCheckpoint,
        ),
        (
            "foreign context",
            |b| {
                b.certificate.body.header.context.deployment_id =
                    DeploymentId::new("99999999-9999-4999-8999-999999999999").unwrap()
            },
            Error::WrongContext,
        ),
        (
            "foreign node",
            |b| {
                b.certificate.body.agent_id =
                    AgentId::new("99999999-9999-4999-8999-999999999999").unwrap()
            },
            Error::WrongClaim,
        ),
        (
            "foreign key",
            |b| {
                b.certificate.body.agent_key = PublicKey::from_bytes(
                    SigningKey::from_bytes(&[66; 32]).verifying_key().to_bytes(),
                )
            },
            Error::WrongClaim,
        ),
        (
            "unsupported key epoch",
            |b| {
                b.certificate.body.key_epoch = 2;
                b.lease.body.key_epoch = 2;
            },
            Error::WrongEpoch,
        ),
        (
            "capability expansion",
            |b| {
                b.certificate.body.capabilities.scopes =
                    Scopes(vec![Scope::TelemetryRead, Scope::MetricsCollect])
            },
            Error::CapabilityDenied,
        ),
        (
            "scope expansion",
            |b| {
                b.certificate.body.scopes =
                    Scopes(vec![Scope::TelemetryRead, Scope::MetricsCollect])
            },
            Error::CapabilityDenied,
        ),
        (
            "foreign operation resource",
            |b| {
                b.operation = Operation::TelemetryRead {
                    source: ResourceId::new("99999999-9999-4999-8999-999999999999").unwrap(),
                }
            },
            Error::ScopeDenied,
        ),
        (
            "expired historical body",
            |b| {
                b.certificate.body.not_before = Timestamp::new(900).unwrap();
                b.certificate.body.expires_at = Timestamp::new(999).unwrap();
            },
            Error::Expired,
        ),
        (
            "lease exceeds historical expiry",
            |b| b.certificate.body.expires_at = Timestamp::new(1200).unwrap(),
            Error::Expired,
        ),
    ];
    for (name, mutate, expected) in cases {
        let mut candidate = binding.clone();
        mutate(&mut candidate);
        // Even a real current owner endorsement cannot override node/type/time
        // constraints. It is not historical wrapper signature verification.
        candidate.lease.body.certificate_hash = candidate.certificate.body.digest().unwrap();
        candidate.lease = rotated_owner_sign(candidate.lease.body);
        assert!(
            matches!(sign(candidate, &current, now), Err(error) if error == *expected),
            "{name}"
        );
    }
    for (name, candidate) in [
        (
            "old owner lease",
            NodeRequestBinding {
                lease: test_owner_sign(binding.lease.body.clone()),
                ..binding.clone()
            },
        ),
        (
            "old owner freshness",
            NodeRequestBinding {
                freshness: test_owner_sign(binding.freshness.body.clone()),
                ..binding.clone()
            },
        ),
        (
            "forged current lease",
            NodeRequestBinding {
                lease: Signed {
                    signature: SignatureBytes::from_bytes([0; 64]),
                    ..binding.lease.clone()
                },
                ..binding.clone()
            },
        ),
        (
            "forged current freshness",
            NodeRequestBinding {
                freshness: Signed {
                    signature: SignatureBytes::from_bytes([0; 64]),
                    ..binding.freshness.clone()
                },
                ..binding.clone()
            },
        ),
    ] {
        assert!(
            matches!(sign(candidate, &current, now), Err(Error::InvalidSignature)),
            "{name}"
        );
    }
    let mut changed_without_endorsement = binding.clone();
    changed_without_endorsement.certificate.body.expires_at = Timestamp::new(1599).unwrap();
    assert!(matches!(
        sign(changed_without_endorsement, &current, now),
        Err(Error::WrongClaim)
    ));
    let mut narrowed_pin = current.clone();
    narrowed_pin.expires_at = Timestamp::new(1200).unwrap();
    assert!(matches!(
        sign(binding.clone(), &narrowed_pin, now),
        Err(Error::Expired)
    ));
    assert!(matches!(
        sign(
            binding.clone(),
            &current,
            Now(Timestamp::new(1300).unwrap())
        ),
        Err(Error::Expired)
    ));

    // Wrapper bytes are outside Certificate::digest(): old wrapper mutation is
    // deliberately NOT claimed verified. Exact current owner body endorsement
    // and the fully replayed Actor certificate are the two authority boundaries.
    let mut historical_wrapper = binding.clone();
    historical_wrapper.certificate.signature = SignatureBytes::from_bytes([0; 64]);
    let proof = sign(historical_wrapper.clone(), &current, now).unwrap();
    let mut actor = continuity_actor(&temp, &original);
    let mut changed_endorsed_body = binding.clone();
    changed_endorsed_body.certificate.body.expires_at = Timestamp::new(1599).unwrap();
    changed_endorsed_body.lease.body.certificate_hash =
        changed_endorsed_body.certificate.body.digest().unwrap();
    changed_endorsed_body.lease = rotated_owner_sign(changed_endorsed_body.lease.body);
    let changed_proof = sign(changed_endorsed_body.clone(), &current, now).unwrap();
    assert!(matches!(
        actor.admit(
            changed_endorsed_body.lease,
            changed_proof,
            Some(&changed_endorsed_body.freshness),
            &changed_endorsed_body.operation,
            now,
        ),
        Err(Error::WrongEpoch)
    ));
    // Rejected endorsed-but-unregistered body did not consume the valid lease.
    actor
        .admit(
            historical_wrapper.lease,
            proof,
            Some(&historical_wrapper.freshness),
            &historical_wrapper.operation,
            now,
        )
        .unwrap();

    let same_epoch_temp = Temp::new();
    let (_, _, same_pin, same_cert) = enrolled_real_node(&same_epoch_temp, 1000);
    let mut same_epoch = real_request_binding(&same_pin, &same_cert, 1000, 92);
    same_epoch.certificate.signature = SignatureBytes::from_bytes([0; 64]);
    assert!(matches!(
        create_node_request(
            &same_epoch_temp.0.join("real-node"),
            same_cert.body.agent_id,
            &same_pin,
            same_epoch,
            now,
        ),
        Err(Error::InvalidSignature)
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn sqlite_worker_actual_node_proof_continues_after_bad_inner_and_refreshes_external_signed_revoke()
{
    let temp = Temp::new();
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let (pin, current, cert) = sqlite_real_node(&temp, seconds);
    let binding = real_request_binding(&current, &cert, seconds, 1);
    let proof = create_node_request(
        &temp.0.join("real-node"),
        cert.body.agent_id,
        &current,
        binding.clone(),
        Now(Timestamp::new(seconds).unwrap()),
    )
    .unwrap();
    let mut worker = SqlWorker::new(&temp);
    let status = worker.send(&json!({"action":"status"}));
    assert_eq!(status["ok"], true);
    assert_eq!(status["metadata"]["provider"], "sqlite-journal-v1");
    let malformed=worker.send(&json!({"action":"admit","lease":{},"proof":{},"freshness":{},"operation":binding.operation}));
    assert_eq!(malformed, json!({"ok":false,"code":"InvalidWire"}));
    let mut frame = json!({"action":"admit","lease":binding.lease,"proof":proof,"freshness":binding.freshness,"operation":binding.operation});
    let mut forged = frame.clone();
    forged["proof"]["signature"] = json!("00".repeat(64));
    assert_eq!(
        worker.send(&forged),
        json!({"ok":false,"code":"InvalidSignature"})
    );
    let accepted = worker.send(&frame);
    assert_eq!(accepted["ok"], true);
    assert_eq!(
        accepted["metadata"]["agentId"],
        "44444444-4444-4444-8444-444444444444"
    );
    assert_eq!(worker.send(&frame), json!({"ok":false,"code":"Replay"}));
    let next = real_request_binding(&current, &cert, seconds, 2);
    let proof = create_node_request(
        &temp.0.join("real-node"),
        cert.body.agent_id,
        &current,
        next.clone(),
        Now(Timestamp::new(seconds).unwrap()),
    )
    .unwrap();
    frame = json!({"action":"admit","lease":next.lease,"proof":proof,"freshness":next.freshness,"operation":next.operation});
    let mut authority = Actor::new(
        SqliteJournalStore::open(
            temp.0.join("worker-db"),
            temp.0.join("worker-anchor"),
            pin.clone(),
            SqliteStorageBudget::default(),
        )
        .unwrap(),
    )
    .unwrap();
    let observed = Now(Timestamp::new(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap());
    authority
        .revoke(
            test_owner_sign(Revocation {
                header: Header {
                    checkpoint: 3,
                    ..binding.lease.body.header
                },
                agent_id: cert.body.agent_id,
                key_epoch: 1,
                certificate_hash: cert.body.digest().unwrap(),
                issued_at: observed.0,
            }),
            observed,
        )
        .unwrap();
    drop(authority);
    assert_eq!(
        worker.send(&frame),
        json!({"ok":false,"code":"WrongCheckpoint"})
    );
    assert_eq!(
        worker.send(&json!({"action":"status"}))["metadata"]["revision"],
        5
    );
    worker.finish();
    let mut restarted = SqlWorker::new(&temp);
    assert_eq!(
        restarted.send(&frame),
        json!({"ok":false,"code":"WrongCheckpoint"})
    );
    restarted.finish();
}
#[cfg(target_os = "linux")]
#[test]
fn sqlite_worker_fatal_private_framing_is_bounded_but_deep_signed_data_is_ordinary_denial() {
    let temp = Temp::new();
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    sqlite_real_node(&temp, seconds);
    let mut worker = SqlWorker::new(&temp);
    let deep=format!("{{\"action\":\"admit\",\"lease\":{}0{},\"proof\":{{}},\"freshness\":{{}},\"operation\":{{\"kind\":\"telemetryRead\",\"source\":\"66666666-6666-4666-8666-666666666666\"}}}}\n","[".repeat(200),"]".repeat(200));
    assert_eq!(
        worker.raw(deep.as_bytes()),
        json!({"ok":false,"code":"InvalidWire"})
    );
    assert_eq!(worker.send(&json!({"action":"status"}))["ok"], true);
    worker.finish();
    for frame in [
        b"{\"action\":\"status\",\"action\":\"status\"}\n".to_vec(),
        b"{\"action\":\"genericSign\"}\n".to_vec(),
        vec![b' '; MAX_WIRE_BYTES + 1],
    ] {
        let mut worker = SqlWorker::new(&temp);
        let response = worker.raw(&frame);
        assert_eq!(response["ok"], false);
        assert!(matches!(
            response["code"].as_str(),
            Some("InvalidWire" | "Quota")
        ));
        drop(worker.input.take());
        assert!(!worker.exited().success());
    }
}
