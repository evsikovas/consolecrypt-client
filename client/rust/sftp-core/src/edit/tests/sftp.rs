//! Integration tests against the local-filesystem SFTP server (real
//! [`SftpClient`], russh-sftp protocol, extension shim).

use super::fake::FakeOpener;
use super::{cfg, eventually};
use crate::edit::*;
use crate::test_server::LocalFsServer;
use crate::{SftpClient, SftpError};
use std::path::Path;
use std::sync::Arc;

async fn client(root: &Path, posix_rename: bool) -> Arc<SftpClient> {
    let (a, b) = tokio::io::duplex(1 << 20);
    let mut server = LocalFsServer::new(root.to_path_buf());
    if !posix_rename {
        server = server.without_posix_rename();
    }
    tokio::spawn(russh_sftp::server::run(b, server));
    Arc::new(SftpClient::from_stream(a).await.unwrap())
}

#[cfg(unix)]
fn ino_mode(p: &Path) -> (u64, u32) {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let m = std::fs::metadata(p).unwrap();
    (m.ino(), m.permissions().mode() & 0o7777)
}

fn no_temp_files(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .all(|e| !e.file_name().to_string_lossy().contains(".cc-upload-"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn extension_shim_posix_rename() {
    let srv = tempfile::tempdir().unwrap();
    let c = client(srv.path(), true).await;
    assert!(c.supports_posix_rename());
    assert_eq!(
        c.server_extensions()
            .get("posix-rename@openssh.com")
            .map(String::as_str),
        Some("1")
    );
    c.write("/a", b"new").await.unwrap();
    c.write("/b", b"old").await.unwrap();
    // plain RENAME refuses to overwrite (OpenSSH semantics) …
    assert!(matches!(
        c.rename("/a", "/b").await,
        Err(SftpError::Remote { .. })
    ));
    // … posix-rename replaces atomically
    c.posix_rename("/a", "/b").await.unwrap();
    assert_eq!(std::fs::read(srv.path().join("b")).unwrap(), b"new");
    assert!(!srv.path().join("a").exists());
    assert!(matches!(
        c.posix_rename("/missing", "/b").await,
        Err(SftpError::NotFound(_))
    ));

    // Injected requests interleave safely with russh-sftp's own traffic.
    let payload: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(srv.path().join("big"), &payload).unwrap();
    let mut tasks = Vec::new();
    for i in 0..16 {
        let c = c.clone();
        let payload = payload.clone();
        tasks.push(tokio::spawn(async move {
            let from = format!("/t{i}");
            let to = format!("/u{i}");
            c.write(&from, format!("x{i}").as_bytes()).await.unwrap();
            c.write(&to, b"old").await.unwrap();
            let (read, renamed) = tokio::join!(c.read("/big"), c.posix_rename(&from, &to));
            renamed.unwrap();
            assert_eq!(read.unwrap(), payload);
            assert_eq!(c.read(&to).await.unwrap(), format!("x{i}").as_bytes());
        }));
    }
    for t in tasks {
        t.await.unwrap();
    }
    assert_eq!(c.list("/").await.unwrap().len(), 18);

    // A server without the extension.
    let srv2 = tempfile::tempdir().unwrap();
    let c2 = client(srv2.path(), false).await;
    assert!(!c2.supports_posix_rename());
    c2.write("/a", b"1").await.unwrap();
    assert!(matches!(
        c2.posix_rename("/a", "/b").await,
        Err(SftpError::Unsupported(_))
    ));
    assert_eq!(c2.read("/a").await.unwrap(), b"1", "session still healthy");
}

async fn roundtrip(posix_rename: bool) {
    let srv = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(srv.path().join("www")).unwrap();
    let remote_file = srv.path().join("www/index.php");
    std::fs::write(&remote_file, b"<?php echo 'v1';\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&remote_file, std::fs::Permissions::from_mode(0o640)).unwrap();
    }
    #[cfg(unix)]
    let (ino_before, _) = ino_mode(&remote_file);

    let c = client(srv.path(), posix_rename).await;
    let opener = FakeOpener::new();
    let mgr = EditManager::new(cfg(&local.path().join("edit")), opener.clone()).unwrap();
    let s = mgr
        .open(c.clone(), "srv", "/www/index.php", OpenWith::Default)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(s.local_path()).unwrap(),
        b"<?php echo 'v1';\n"
    );

    for (i, body) in ["<?php echo 'v2';\n", "<?php echo 'version 3';\n"]
        .iter()
        .enumerate()
    {
        std::fs::write(s.local_path(), body).unwrap();
        eventually("upload", || s.info().uploads == i as u64 + 1).await;
        assert_eq!(std::fs::read(&remote_file).unwrap(), body.as_bytes());
        assert_eq!(s.status(), EditStatus::Synced);
    }
    #[cfg(unix)]
    {
        let (ino_after, mode_after) = ino_mode(&remote_file);
        assert_ne!(ino_before, ino_after, "replaced atomically (new inode)");
        assert_eq!(mode_after, 0o640, "permissions preserved");
    }
    assert!(no_temp_files(&srv.path().join("www")));

    // Someone edits the file on the server → conflict, no overwrite.
    std::fs::write(&remote_file, b"<?php echo 'hotfix on the server';\n").unwrap();
    std::fs::write(s.local_path(), b"<?php echo 'mine';\n").unwrap();
    eventually("conflict", || {
        matches!(s.status(), EditStatus::Conflict { .. })
    })
    .await;
    assert_eq!(
        std::fs::read(&remote_file).unwrap(),
        b"<?php echo 'hotfix on the server';\n"
    );
    s.resolve(ConflictResolution::DiscardLocal).await.unwrap();
    assert_eq!(
        std::fs::read(s.local_path()).unwrap(),
        b"<?php echo 'hotfix on the server';\n"
    );
    let dir = s.local_path().parent().unwrap().to_path_buf();
    assert_eq!(
        s.stop(StopMode::Upload).await.unwrap(),
        StopOutcome::Closed { uploaded: false }
    );
    assert!(!dir.exists());
    assert!(no_temp_files(&srv.path().join("www")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sftp_edit_roundtrip_with_posix_rename() {
    roundtrip(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sftp_edit_roundtrip_remove_rename_fallback() {
    roundtrip(false).await;
}
