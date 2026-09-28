//! Edit-session integration test against a real OpenSSH server over the
//! 2-hop jump chain (Docker): download, save → atomic replace via
//! `posix-rename@openssh.com` (new inode, mode kept), conflict detection,
//! stop + cleanup.
//! Run with `CC_SSH_IT=1 cargo test -p cc-sftp-core --test edit_docker_it -- --nocapture`.

use cc_sftp_core::edit::{
    ChooseOutcome, ConflictResolution, EditConfig, EditManager, EditOpener, EditStatus, OpenError,
    OpenWith, StopMode, StopOutcome,
};
use cc_sftp_core::SftpClient;
use cc_ssh_core::testing::{self, connector, SshTestbed};
use cc_ssh_core::{ConnectionPlanner, HostKeyDecision, MemoryKnownHosts, SshSession};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Records opens; never launches an application.
#[derive(Debug, Default)]
struct RecordingOpener(Mutex<Vec<PathBuf>>);

impl EditOpener for RecordingOpener {
    fn open(&self, path: &Path, _with: &OpenWith) -> Result<ChooseOutcome, OpenError> {
        self.0.lock().unwrap().push(path.to_path_buf());
        Ok(ChooseOutcome::Opened { app: None })
    }
}

async fn eventually(what: &str, f: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {what}");
}

/// `mode inode` of a remote file.
async fn stat(session: &SshSession, path: &str) -> String {
    session
        .exec(&format!("stat -c '%a %i' {path}"))
        .await
        .unwrap()
        .stdout_lossy()
        .trim()
        .to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn docker_edit_session_roundtrip() {
    if !testing::enabled() {
        eprintln!("skipped: set CC_SSH_IT=1 to run Docker integration tests");
        return;
    }
    let tb = tokio::task::spawn_blocking(|| SshTestbed::start(true))
        .await
        .unwrap()
        .expect("testbed");
    let ti = tb.inventory();
    let (c, _) = connector(
        ti.resolver.clone(),
        Arc::new(MemoryKnownHosts::new()),
        HostKeyDecision::Reject,
    );
    let plan = ConnectionPlanner::new(ti.inventory.clone())
        .plan(ti.target.unwrap())
        .await
        .unwrap();
    let session = c.connect(&plan).await.unwrap();
    let sftp = Arc::new(SftpClient::open(&session).await.unwrap());
    assert!(
        sftp.supports_posix_rename(),
        "OpenSSH announces posix-rename@openssh.com"
    );
    println!("IT sftp_posix_rename_announced ... ok");

    let home = sftp.home_dir().await.unwrap();
    let dir = format!("{home}/edit-it");
    let file = format!("{dir}/wp-config.php");
    sftp.mkdir_all(&dir).await.unwrap();
    sftp.write(&file, b"<?php define('DB', 'v1');\n")
        .await
        .unwrap();
    sftp.chmod(&file, 0o640).await.unwrap();
    let before = stat(&session, &file).await;
    assert!(before.starts_with("640 "), "{before}");

    let local = tempfile::tempdir().unwrap();
    let opener = Arc::new(RecordingOpener::default());
    let cfg = EditConfig {
        debounce: Duration::from_millis(200),
        poll_interval: Some(Duration::from_secs(1)),
        exclude_from_backup: false,
        ..EditConfig::new(local.path().join("edit"))
    };
    let mgr = EditManager::new(cfg, opener.clone()).unwrap();
    let s = mgr
        .open(sftp.clone(), "target", &file, OpenWith::Default)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(s.local_path()).unwrap(),
        b"<?php define('DB', 'v1');\n"
    );
    assert_eq!(opener.0.lock().unwrap().len(), 1);
    println!("IT edit_open_download ... ok");

    // Save → atomic replace (posix-rename: new inode, same mode).
    let v2 = b"<?php define('DB', 'v2 edited locally');\n";
    std::fs::write(s.local_path(), v2).unwrap();
    eventually("upload", || s.info().uploads == 1).await;
    assert_eq!(s.status(), EditStatus::Synced);
    assert_eq!(sftp.read(&file).await.unwrap(), v2);
    let after = stat(&session, &file).await;
    let (mode_b, ino_b) = before.split_once(' ').unwrap();
    let (mode_a, ino_a) = after.split_once(' ').unwrap();
    assert_eq!(mode_a, mode_b, "mode preserved");
    assert_ne!(ino_a, ino_b, "replaced via rename (new inode)");
    let names: Vec<_> = sftp
        .list(&dir)
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(names, vec!["wp-config.php"], "no temp leftovers");
    println!("IT edit_save_atomic_replace ... ok");

    // Change on the server → conflict, nothing overwritten.
    session
        .exec(&format!("echo '// hotfix' >> {file}"))
        .await
        .unwrap();
    std::fs::write(s.local_path(), b"<?php define('DB', 'v3');\n").unwrap();
    eventually("conflict", || {
        matches!(s.status(), EditStatus::Conflict { .. })
    })
    .await;
    let remote_now = sftp.read(&file).await.unwrap();
    assert!(remote_now.ends_with(b"// hotfix\n"));
    let copy = s
        .resolve(ConflictResolution::KeepRemoteCopyLocally)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(std::fs::read(&copy).unwrap(), remote_now);
    assert_eq!(s.status(), EditStatus::Modified);
    println!("IT edit_conflict_detected ... ok");

    // Stop editing uploads the pending (merged) change and cleans up.
    let merged = b"<?php define('DB', 'v3'); // hotfix merged\n";
    std::fs::write(s.local_path(), merged).unwrap();
    let work_dir = s.local_path().parent().unwrap().to_path_buf();
    let out = s.stop(StopMode::Upload).await.unwrap();
    assert!(matches!(out, StopOutcome::Closed { .. }), "{out:?}");
    assert_eq!(sftp.read(&file).await.unwrap(), merged);
    assert!(!work_dir.exists());
    assert!(mgr.leftovers().unwrap().is_empty());
    assert!(stat(&session, &file).await.starts_with("640 "));
    println!("IT edit_stop_cleanup ... ok");

    sftp.remove_dir(&dir, true).await.unwrap();
    sftp.close().await.unwrap();
    session.disconnect().await.unwrap();
    drop(tb);
}
