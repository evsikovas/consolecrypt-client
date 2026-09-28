//! End-to-end tests of the `cc` binary: local-only profile (init → hosts →
//! list → backup export/import) and synced profiles against the in-process
//! mock server (register + create vault, second device joins with the
//! passphrase, third device joins by approval with the verification code).
//! A live-server smoke test runs when `CC_E2E_SERVER` is set.

use cc_sync_core::mock::MockServer;
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

const PASSPHRASE: &str = "cli vault passphrase 42";
const PASSWORD: &str = "cli-account-password-1234";

fn cc(dir: &Path, envs: &[(&str, &str)], args: &[&str]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_consolecrypt"));
    c.arg("--data-dir")
        .arg(dir)
        .args(["--secure-store", "file"])
        .args(args)
        .env("CC_TEST_FAST_KDF", "1")
        .env("CC_PASSPHRASE", PASSPHRASE)
        .env("CC_PASSWORD", PASSWORD)
        .env_remove("CONSOLECRYPT_DATA_DIR")
        .env_remove("CC_PROFILE")
        .env_remove("RUST_LOG");
    for (k, v) in envs {
        c.env(k, v);
    }
    c.output().expect("run cc")
}

#[track_caller]
fn ok(o: Output) -> String {
    let stdout = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(
        o.status.success(),
        "cc failed ({:?})\nstdout: {stdout}\nstderr: {}",
        o.status.code(),
        String::from_utf8_lossy(&o.stderr)
    );
    stdout
}

#[track_caller]
fn json(o: Output) -> Value {
    let s = ok(o);
    serde_json::from_str(&s).unwrap_or_else(|e| panic!("not JSON ({e}): {s}"))
}

fn names(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|h| h["name"].as_str().unwrap().to_owned())
        .collect()
}

async fn run(dir: &Path, envs: &[(&str, &str)], args: &[&str]) -> Output {
    let dir = dir.to_path_buf();
    let envs: Vec<(String, String)> = envs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
    tokio::task::spawn_blocking(move || {
        let e: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        cc(&dir, &e, &a)
    })
    .await
    .unwrap()
}

#[test]
fn local_profile_hosts_and_backup_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("data");

    let created = json(cc(
        &dir,
        &[],
        &["--json", "init", "--local", "--name", "Personal"],
    ));
    assert_eq!(created["profile"]["kind"], "local");
    let words = created["recovery_kit"]["words"].as_array().unwrap();
    assert_eq!(words.len(), 24);
    let phrase = created["recovery_kit"]["phrase"]
        .as_str()
        .unwrap()
        .to_owned();

    ok(cc(
        &dir,
        &[("CC_SECRET", "db-secret")],
        &["cred", "add-password", "dbpw", "--user", "postgres"],
    ));
    let key = json(cc(
        &dir,
        &[],
        &["--json", "cred", "gen-key", "deploy", "--user", "alex"],
    ));
    assert!(key["public_key"]
        .as_str()
        .unwrap()
        .starts_with("ssh-ed25519 "));
    ok(cc(
        &dir,
        &[],
        &["host", "add", "bastion", "203.0.113.5", "--cred", "deploy"],
    ));
    ok(cc(
        &dir,
        &[],
        &[
            "host", "add", "db", "10.0.0.5", "--port", "2222", "--cred", "dbpw", "--jump",
            "bastion", "--tag", "prod",
        ],
    ));
    let prompt = json(cc(
        &dir,
        &[],
        &[
            "--json",
            "host",
            "add",
            "asks",
            "10.0.0.7",
            "--ask-password",
        ],
    ));
    assert_eq!(prompt["auth_mode"], "password_prompt");
    let inline = json(cc(
        &dir,
        &[("CC_SECRET", "inline-secret")],
        &[
            "--json",
            "host",
            "add",
            "own",
            "10.0.0.8",
            "--password",
            "--user",
            "root",
        ],
    ));
    assert_eq!(inline["auth_mode"], "inline_password");
    ok(cc(&dir, &[], &["host", "rm", "asks"]));
    ok(cc(&dir, &[], &["host", "rm", "own"]));
    let hosts = json(cc(&dir, &[], &["--json", "host", "list"]));
    assert_eq!(names(&hosts), ["bastion", "db"]);
    let human = ok(cc(&dir, &[], &["host", "show", "db"]));
    assert!(
        human.contains("alex@203.0.113.5:22 -> postgres@10.0.0.5:2222"),
        "{human}"
    );
    ok(cc(
        &dir,
        &[],
        &[
            "tunnel",
            "add",
            "pg",
            "db",
            "--local",
            "15432",
            "--to",
            "127.0.0.1:5432",
        ],
    ));
    let tunnels = json(cc(&dir, &[], &["--json", "tunnel", "list"]));
    assert_eq!(tunnels[0]["bind_port"], 15432);

    // Wrong passphrase → stable error code, exit 1.
    let bad = cc(
        &dir,
        &[("CC_PASSPHRASE", "wrong wrong")],
        &["--json", "unlock-check"],
    );
    assert_eq!(bad.status.code(), Some(1));
    let err: Value = serde_json::from_slice(&bad.stdout).unwrap();
    assert_eq!(err["error"]["code"], "wrong_passphrase");
    ok(cc(
        &dir,
        &[("CC_RECOVERY_KEY", &phrase)],
        &["unlock-check", "--recovery"],
    ));

    // Backup export → import into a new profile of the same installation.
    let file = tmp.path().join("vault.ccbackup");
    let f = file.to_string_lossy().to_string();
    let exported = json(cc(&dir, &[], &["--json", "backup", "export", &f]));
    assert_eq!(
        exported["objects"], 8,
        "settings, 2 secrets, 2 creds, 2 hosts, tunnel → {exported}"
    );
    let raw = std::fs::read_to_string(&file).unwrap();
    assert!(!raw.contains("db-secret") && !raw.contains("10.0.0.5"));
    let restored = json(cc(
        &dir,
        &[],
        &["--json", "backup", "import", &f, "--name", "Restored"],
    ));
    assert_eq!(restored["profile"]["display_name"], "Restored");
    let profiles = json(cc(&dir, &[], &["--json", "profiles"]));
    assert_eq!(profiles.as_array().unwrap().len(), 2);
    let restored_hosts = json(cc(
        &dir,
        &[],
        &["--json", "--profile", "Restored", "host", "list"],
    ));
    assert_eq!(restored_hosts, hosts);

    // Into a fresh installation, with the Recovery Key and a new passphrase.
    let dir2 = tmp.path().join("data2");
    json(cc(
        &dir2,
        &[
            ("CC_RECOVERY_KEY", &phrase),
            ("CC_NEW_PASSPHRASE", "fresh passphrase 99"),
        ],
        &["--json", "backup", "import", &f, "--recovery"],
    ));
    let h2 = json(cc(
        &dir2,
        &[("CC_PASSPHRASE", "fresh passphrase 99")],
        &["--json", "host", "list"],
    ));
    assert_eq!(h2, hosts);

    // Passphrase change.
    ok(cc(
        &dir,
        &[("CC_NEW_PASSPHRASE", "changed passphrase 1")],
        &["--profile", "Personal", "passphrase", "change"],
    ));
    ok(cc(
        &dir,
        &[("CC_PASSPHRASE", "changed passphrase 1")],
        &["--profile", "Personal", "unlock-check"],
    ));

    // Input validation reaches the user as a stable code.
    let e = cc(&dir, &[], &["--json", "host", "add", "bad", "has space"]);
    assert_eq!(e.status.code(), Some(1));
    let e = cc(
        &dir,
        &[],
        &["--json", "--profile", "Restored", "host", "rm", "nope"],
    );
    let v: Value = serde_json::from_slice(&e.stdout).unwrap();
    assert_eq!(v["error"]["code"], "not_found");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn synced_profiles_against_mock_server() {
    let server = MockServer::start().await;
    let url = server.url().to_string();
    let tmp = tempfile::tempdir().unwrap();
    let (a, b, c) = (
        tmp.path().join("a"),
        tmp.path().join("b"),
        tmp.path().join("c"),
    );
    let email = "cli@example.org";

    // Device A: register + create the vault, add a host (auto-synced).
    let init = run(
        &a,
        &[],
        &[
            "--json",
            "init",
            "--server",
            &url,
            "--register",
            "--email",
            email,
        ],
    )
    .await;
    let kit = json(init);
    let vault_id = kit["vault_id"].as_str().unwrap().to_owned();
    ok(run(
        &a,
        &[],
        &["host", "add", "web", "192.0.2.80", "--user", "deploy"],
    )
    .await);
    let status = json(run(&a, &[], &["--json", "sync", "status"]).await);
    assert_eq!(status["pending"], 0, "{status}");

    // Device B: log in and join with the passphrase.
    let joined = json(
        run(
            &b,
            &[],
            &[
                "--json", "init", "--server", &url, "--login", "--email", email,
            ],
        )
        .await,
    );
    assert_eq!(joined["vault_id"], vault_id.as_str());
    let hosts = json(run(&b, &[], &["--json", "host", "list"]).await);
    assert_eq!(names(&hosts), ["web"]);

    // Device C: request approval, A approves with C's code, C finishes.
    let req = json(
        run(
            &c,
            &[],
            &[
                "--json",
                "init",
                "--server",
                &url,
                "--login",
                "--email",
                email,
                "--request-approval",
            ],
        )
        .await,
    );
    let request_id = req["request_id"].as_str().unwrap().to_owned();
    let code = req["verification_code"].as_str().unwrap().to_owned();
    let not_yet = run(&c, &[], &["--json", "device", "finish"]).await;
    assert!(!not_yet.status.success());
    // A wrong code is refused (nothing is approved).
    let wrong = run(
        &a,
        &[],
        &[
            "device",
            "approve",
            &request_id,
            "--code",
            "00000 00000 00000 00000 00000 00000",
        ],
    )
    .await;
    assert_eq!(wrong.status.code(), Some(2));
    ok(run(
        &a,
        &[],
        &["device", "approve", &request_id, "--code", &code],
    )
    .await);
    let fin = json(run(&c, &[], &["--json", "device", "finish"]).await);
    assert_eq!(fin["joined"], true);
    let hosts_c = json(run(&c, &[], &["--json", "host", "list"]).await);
    assert_eq!(names(&hosts_c), ["web"]);

    // C adds a host; A sees it after its automatic pull.
    ok(run(&c, &[], &["host", "add", "db", "192.0.2.81"]).await);
    let hosts_a = json(run(&a, &[], &["--json", "host", "list"]).await);
    assert_eq!(names(&hosts_a), ["db", "web"]);

    // Device list from A shows three devices; revoke C.
    let devices = json(run(&a, &[], &["--json", "device", "list"]).await);
    let list = devices["devices"].as_array().unwrap();
    assert_eq!(list.len(), 3);
    let c_id = json(run(&c, &[], &["--json", "profiles"]).await)[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let c_profile = json(run(&c, &[], &["--json", "profiles", "use", &c_id]).await);
    let c_device = c_profile["device_id"].as_str().unwrap().to_owned();
    ok(run(&a, &[], &["device", "revoke", &c_device]).await);
    let after = run(&c, &[], &["--json", "sync", "now"]).await;
    let v: Value = serde_json::from_slice(&after.stdout).unwrap();
    assert_eq!(v["error"]["code"], "device_revoked", "{v}");

    // A local profile enables sync.
    let d = tmp.path().join("d");
    json(run(&d, &[], &["--json", "init", "--local", "--name", "Home"]).await);
    ok(run(&d, &[], &["host", "add", "nas", "192.168.1.2"]).await);
    let p = json(
        run(
            &d,
            &[],
            &[
                "--json",
                "sync",
                "enable",
                "--server",
                &url,
                "--register",
                "--email",
                "home@example.org",
            ],
        )
        .await,
    );
    assert_eq!(p["kind"], "synced");
    let vid: cc_protocol::VaultId = p["vault_id"].as_str().unwrap().parse().unwrap();
    assert!(!server.live_objects(vid).is_empty());
    let disc = json(run(&d, &[], &["--json", "sync", "disconnect"]).await);
    assert_eq!(disc["kind"], "local");
}

/// Live-server smoke test (`CC_E2E_SERVER=http://localhost:8080`).
#[test]
fn live_server_smoke() {
    let Ok(url) = std::env::var("CC_E2E_SERVER") else {
        eprintln!("skipped: set CC_E2E_SERVER=http://localhost:8080 to run against a live server");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let email = format!("cli-e2e-{}@example.test", uuid::Uuid::new_v4().simple());
    let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
    let kit = json(cc(
        &a,
        &[],
        &[
            "--json",
            "init",
            "--server",
            &url,
            "--register",
            "--email",
            &email,
        ],
    ));
    assert!(kit["vault_id"].is_string());
    ok(cc(
        &a,
        &[],
        &["host", "add", "live-web", "192.0.2.90", "--user", "ops"],
    ));
    let joined = json(cc(
        &b,
        &[],
        &[
            "--json", "init", "--server", &url, "--login", "--email", &email,
        ],
    ));
    assert_eq!(joined["vault_id"], kit["vault_id"]);
    let hosts = json(cc(&b, &[], &["--json", "host", "list"]));
    assert_eq!(names(&hosts), ["live-web"]);
    let devices = json(cc(&b, &[], &["--json", "device", "list"]));
    assert_eq!(devices["devices"].as_array().unwrap().len(), 2);
    println!("live server CLI smoke against {url}: OK");
}
