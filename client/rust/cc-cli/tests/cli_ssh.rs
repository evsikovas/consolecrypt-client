//! `cc ssh` / `cc sftp` against the Docker testbed through the 2-hop jump
//! chain (password credential; host keys trusted on first use and stored in
//! the vault). Gated by `CC_SSH_IT=1`.

use cc_ssh_core::testing::{self, SshTestbed};
use secrecy::ExposeSecret;
use std::path::Path;
use std::process::{Command, Output};

const PASSPHRASE: &str = "cli ssh passphrase 42";

fn cc(dir: &Path, envs: &[(&str, &str)], args: &[&str]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_consolecrypt"));
    c.arg("--data-dir")
        .arg(dir)
        .args(["--secure-store", "file"])
        .args(args)
        .env("CC_TEST_FAST_KDF", "1")
        .env("CC_PASSPHRASE", PASSPHRASE)
        .env_remove("CONSOLECRYPT_DATA_DIR")
        .env_remove("CC_PROFILE");
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

#[test]
fn cli_ssh_exec_and_sftp_through_jump_chain() {
    if !testing::enabled() {
        eprintln!("skipped: set CC_SSH_IT=1 to run Docker integration tests");
        return;
    }
    let tb = SshTestbed::start(true).expect("ssh testbed");
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("data");
    let user = tb.secrets.user.clone();
    let pw = tb.secrets.password.expose_secret().to_owned();
    let port = tb.bastion1_port.to_string();

    ok(cc(&dir, &[], &["--json", "init", "--local"]));
    ok(cc(
        &dir,
        &[("CC_SECRET", &pw)],
        &["cred", "add-password", "it", "--user", &user],
    ));
    ok(cc(
        &dir,
        &[],
        &[
            "host",
            "add",
            "b1",
            "127.0.0.1",
            "--port",
            &port,
            "--cred",
            "it",
        ],
    ));
    ok(cc(
        &dir,
        &[],
        &[
            "host",
            "add",
            "b2",
            tb.bastion2.as_deref().unwrap(),
            "--cred",
            "it",
        ],
    ));
    ok(cc(
        &dir,
        &[],
        &[
            "host",
            "add",
            "target",
            tb.target.as_deref().unwrap(),
            "--cred",
            "it",
            "--jump",
            "b1,b2",
        ],
    ));

    // Unknown host keys are refused non-interactively without the flag …
    let refused = cc(&dir, &[], &["ssh", "target", "--", "true"]);
    assert!(!refused.status.success());
    // … and trusted (stored in the vault) with it.
    let out = ok(cc(
        &dir,
        &[],
        &[
            "ssh",
            "--accept-host-key",
            "target",
            "--",
            "echo",
            "cli-ssh-ok",
        ],
    ));
    assert!(out.contains("cli-ssh-ok"), "{out}");
    // Now known: no flag needed; the remote exit status is propagated.
    let st = cc(&dir, &[], &["ssh", "target", "--", "exit", "7"]);
    assert_eq!(st.status.code(), Some(7));

    let local = tmp.path().join("up.txt");
    std::fs::write(&local, b"cc sftp roundtrip").unwrap();
    let l = local.to_string_lossy().to_string();
    ok(cc(
        &dir,
        &[],
        &["sftp", "put", "target", &l, "cc-cli-it.txt"],
    ));
    let ls = ok(cc(&dir, &[], &["sftp", "ls", "target"]));
    assert!(ls.contains("cc-cli-it.txt"), "{ls}");
    let back = tmp.path().join("down.txt");
    ok(cc(
        &dir,
        &[],
        &[
            "sftp",
            "get",
            "target",
            "cc-cli-it.txt",
            &back.to_string_lossy(),
        ],
    ));
    assert_eq!(std::fs::read(&back).unwrap(), b"cc sftp roundtrip");
    println!("IT cc ssh/sftp via 2 jump hosts ... ok");
}
