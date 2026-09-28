//! `consolecrypt ai …`: local knowledge-base search through the per-profile
//! index (rebuilt on every unlock of a fresh process) and the error paths
//! without a configured AI provider.

use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

const PASSPHRASE: &str = "cli vault passphrase 42";

fn cc(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_consolecrypt"))
        .arg("--data-dir")
        .arg(dir)
        .args(["--secure-store", "file"])
        .args(args)
        .env("CC_TEST_FAST_KDF", "1")
        .env("CC_PASSPHRASE", PASSPHRASE)
        .env_remove("CONSOLECRYPT_DATA_DIR")
        .env_remove("CC_PROFILE")
        .env_remove("RUST_LOG")
        .output()
        .expect("run consolecrypt")
}

#[track_caller]
fn json_ok(o: Output) -> Value {
    let stdout = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(
        o.status.success(),
        "failed ({:?})\nstdout: {stdout}\nstderr: {}",
        o.status.code(),
        String::from_utf8_lossy(&o.stderr)
    );
    serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("not JSON ({e}): {stdout}"))
}

#[test]
fn ai_search_and_errors_without_provider() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("data");
    json_ok(cc(&dir, &["--json", "init", "--local", "--name", "AI"]));
    json_ok(cc(
        &dir,
        &[
            "--json",
            "host",
            "add",
            "billing-db",
            "10.0.0.5",
            "--tag",
            "postgres",
        ],
    ));

    // Each CLI run is a new process: the index is opened and reconciled on
    // unlock, so the host added by the previous run is found.
    let r = json_ok(cc(&dir, &["--json", "ai", "search", "billing"]));
    let hits = r["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1, "{r}");
    assert_eq!(hits[0]["kind"], "host");
    assert_eq!(hits[0]["title"], "billing-db");
    assert!(r["answer"].is_null());
    let r = json_ok(cc(
        &dir,
        &["--json", "ai", "search", "billing", "--kind", "snippet"],
    ));
    assert!(r["hits"].as_array().unwrap().is_empty());

    // No provider configured: a clean `ai_not_configured` error (exit 1).
    let o = cc(&dir, &["--json", "ai", "gen", "list", "files"]);
    assert_eq!(o.status.code(), Some(1));
    let err: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(err["error"]["code"], "ai_not_configured");

    // Bad enum values are usage errors (exit 2); `--run` needs `--host`.
    assert_eq!(
        cc(&dir, &["ai", "search", "x", "--kind", "bogus"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        cc(&dir, &["ai", "gen", "x", "--run"]).status.code(),
        Some(2)
    );
}
