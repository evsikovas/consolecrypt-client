use super::*;
use crate::test_server::LocalFsServer;
use std::sync::Mutex;

async fn client(root: &Path) -> SftpClient {
    let (a, b) = tokio::io::duplex(1 << 20);
    let server = LocalFsServer::new(root.to_path_buf());
    tokio::spawn(russh_sftp::server::run(b, server));
    SftpClient::from_stream(a).await.unwrap()
}

fn data(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 7 % 251) as u8).collect()
}

#[tokio::test]
async fn browse_mkdir_rename_remove_chmod() {
    let dir = tempfile::tempdir().unwrap();
    let c = client(dir.path()).await;
    assert_eq!(c.home_dir().await.unwrap(), "/");
    c.mkdir("/a").await.unwrap();
    c.mkdir_all("/a/b/c").await.unwrap();
    c.write("/a/file.txt", b"hello").await.unwrap();
    c.write("/a/b/inner.txt", b"x").await.unwrap();

    let list = c.list("/a").await.unwrap();
    let names: Vec<_> = list.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["b", "file.txt"], "dirs first");
    assert_eq!(list[0].kind, EntryKind::Dir);
    assert_eq!(list[1].size, 5);
    assert_eq!(list[1].path, "/a/file.txt");
    assert!(list[0].mode_string().starts_with('d'));

    let st = c.stat("/a/file.txt").await.unwrap();
    assert_eq!(st.name, "file.txt");
    assert!(st.modified.is_some());
    assert!(c.exists("/a/file.txt").await.unwrap());
    assert!(!c.exists("/nope").await.unwrap());
    assert!(matches!(c.stat("/nope").await, Err(SftpError::NotFound(_))));

    c.chmod("/a/file.txt", 0o640).await.unwrap();
    #[cfg(unix)]
    {
        assert_eq!(
            c.stat("/a/file.txt").await.unwrap().permissions,
            Some(0o640)
        );
        assert_eq!(
            c.stat("/a/file.txt").await.unwrap().mode_string(),
            "-rw-r-----"
        );
    }

    c.rename("/a/file.txt", "/a/renamed.txt").await.unwrap();
    assert_eq!(c.read("/a/renamed.txt").await.unwrap(), b"hello");
    c.remove_file("/a/renamed.txt").await.unwrap();
    assert!(c.remove_dir("/a", false).await.is_err(), "not empty");
    c.remove_dir("/a", true).await.unwrap();
    assert!(!c.exists("/a").await.unwrap());
    assert!(c.mkdir_all("/x/y").await.is_ok());
}

#[tokio::test]
async fn upload_download_roundtrip_with_progress() {
    let dir = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();
    let c = client(dir.path()).await;
    let payload = data(1_000_003);
    let src = local.path().join("src.bin");
    std::fs::write(&src, &payload).unwrap();

    let seen = Arc::new(Mutex::new(Vec::new()));
    let s2 = seen.clone();
    let mut progress = move |p: TransferProgress| s2.lock().unwrap().push(p);
    let opts = TransferOptions {
        chunk_size: 64 * 1024,
        ..Default::default()
    };
    let sum = c
        .upload(&src, "/up.bin", &opts, &mut progress, &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(sum.bytes, payload.len() as u64);
    assert_eq!(std::fs::read(dir.path().join("up.bin")).unwrap(), payload);
    let seen = seen.lock().unwrap().clone();
    assert!(seen.len() > 10);
    assert_eq!(seen.first().unwrap().transferred, 0);
    assert_eq!(seen.last().unwrap().transferred, payload.len() as u64);
    assert!(seen.iter().all(|p| p.total == Some(payload.len() as u64)));
    assert!(seen
        .windows(2)
        .all(|w| w[0].transferred <= w[1].transferred));

    let dst = local.path().join("dst.bin");
    let mut last = TransferProgress {
        transferred: 0,
        total: None,
    };
    let sum = c
        .download(
            "/up.bin",
            &dst,
            &opts,
            &mut |p| last = p,
            &CancelToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(sum.bytes, payload.len() as u64);
    assert_eq!(last.total, Some(payload.len() as u64));
    assert_eq!(std::fs::read(&dst).unwrap(), payload);

    // no-overwrite
    let no = TransferOptions {
        overwrite: false,
        ..Default::default()
    };
    assert!(matches!(
        c.upload(&src, "/up.bin", &no, &mut |_| {}, &CancelToken::new())
            .await,
        Err(SftpError::AlreadyExists(_))
    ));
    assert!(matches!(
        c.download("/up.bin", &dst, &no, &mut |_| {}, &CancelToken::new())
            .await,
        Err(SftpError::AlreadyExists(_))
    ));
    // missing remote source does not create the local file
    let missing = local.path().join("missing.bin");
    assert!(matches!(
        c.download(
            "/missing",
            &missing,
            &opts,
            &mut |_| {},
            &CancelToken::new()
        )
        .await,
        Err(SftpError::NotFound(_))
    ));
    assert!(!missing.exists());
}

#[tokio::test]
async fn cancellation_removes_partial_files() {
    let dir = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();
    let c = client(dir.path()).await;
    let payload = data(2_000_000);
    let src = local.path().join("big.bin");
    std::fs::write(&src, &payload).unwrap();
    let opts = TransferOptions {
        chunk_size: 32 * 1024,
        ..Default::default()
    };

    // Upload: cancel after the first chunk.
    let cancel = CancelToken::new();
    let c2 = cancel.clone();
    let mut progress = move |p: TransferProgress| {
        if p.transferred > 0 {
            c2.cancel();
        }
    };
    let r = c
        .upload(&src, "/big.bin", &opts, &mut progress, &cancel)
        .await;
    assert!(matches!(r, Err(SftpError::Cancelled)), "{r:?}");
    assert!(
        !dir.path().join("big.bin").exists(),
        "partial remote file removed"
    );

    // Download: cancel mid-way.
    std::fs::write(dir.path().join("remote.bin"), &payload).unwrap();
    let cancel = CancelToken::new();
    let c3 = cancel.clone();
    let mut progress = move |p: TransferProgress| {
        if p.transferred > 100_000 {
            c3.cancel();
        }
    };
    let dst = local.path().join("partial.bin");
    let r = c
        .download("/remote.bin", &dst, &opts, &mut progress, &cancel)
        .await;
    assert!(matches!(r, Err(SftpError::Cancelled)), "{r:?}");
    assert!(!dst.exists(), "partial local file removed");

    // Keep partial when asked.
    let keep = TransferOptions {
        remove_partial: false,
        ..opts.clone()
    };
    let cancel = CancelToken::new();
    cancel.cancel();
    let r = c
        .download("/remote.bin", &dst, &keep, &mut |_| {}, &cancel)
        .await;
    assert!(matches!(r, Err(SftpError::Cancelled)));
    assert!(dst.exists());
}

#[test]
fn mode_strings() {
    let e = RemoteEntry {
        name: "x".into(),
        path: "x".into(),
        kind: EntryKind::File,
        size: 0,
        permissions: Some(0o755),
        uid: None,
        gid: None,
        user: None,
        group: None,
        modified: None,
    };
    assert_eq!(e.mode_string(), "-rwxr-xr-x");
    assert_eq!(join("/a/", "b"), "/a/b");
    assert_eq!(join(".", "b"), "b");
    assert_eq!(base_name("/a/b/"), "b");
}

#[tokio::test]
async fn detailed_listing_links_create_head_and_copy() {
    let dir = tempfile::tempdir().unwrap();
    let c = crate::test_server::connect_local(dir.path().to_path_buf())
        .await
        .unwrap();
    c.mkdir("/d").await.unwrap();
    c.write("/d/a.txt", b"hello world").await.unwrap();
    c.mkdir("/d/sub").await.unwrap();

    // Detailed listing: sorted dirs first, owner names from the longname.
    let list = c.list_detailed("/d").await.unwrap();
    let names: Vec<_> = list.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["sub", "a.txt"]);
    assert_eq!(list[1].size, 11);
    assert!(list[1].user.is_some() && list[1].group.is_some());
    assert!(matches!(
        c.list_detailed("/missing").await,
        Err(SftpError::NotFound(_))
    ));

    // Symlinks (OpenSSH argument order) and readlink.
    #[cfg(unix)]
    {
        c.symlink("/d/link", "/d/a.txt").await.unwrap();
        assert_eq!(c.read_link("/d/link").await.unwrap(), "/d/a.txt");
        let l = c.lstat("/d/link").await.unwrap();
        assert_eq!(l.kind, EntryKind::Symlink);
        let listed = c.list_detailed("/d").await.unwrap();
        let link = listed.iter().find(|e| e.name == "link").unwrap();
        assert_eq!(link.kind, EntryKind::Symlink, "readdir entries are lstat");
    }

    // Exclusive create.
    c.create_file("/d/new.txt", 0o644).await.unwrap();
    assert_eq!(c.stat("/d/new.txt").await.unwrap().size, 0);
    assert!(matches!(
        c.create_file("/d/new.txt", 0o644).await,
        Err(SftpError::AlreadyExists(_))
    ));

    // Bounded head read.
    let (head, total) = c.read_head("/d/a.txt", 5).await.unwrap();
    assert_eq!(head, b"hello");
    assert_eq!(total, 11);
    let (all, total) = c.read_head("/d/a.txt", 1024).await.unwrap();
    assert_eq!(all, b"hello world");
    assert_eq!(total, 11);

    // Copy keeps the mode; the target must not exist.
    c.chmod("/d/a.txt", 0o640).await.unwrap();
    let mut last = 0;
    let s = c
        .copy_file(
            "/d/a.txt",
            "/d/b.txt",
            &mut |p| last = p.transferred,
            &CancelToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(s.bytes, 11);
    assert_eq!(last, 11);
    assert_eq!(c.read("/d/b.txt").await.unwrap(), b"hello world");
    #[cfg(unix)]
    assert_eq!(c.stat("/d/b.txt").await.unwrap().permissions, Some(0o640));
    assert!(matches!(
        c.copy_file("/d/a.txt", "/d/b.txt", &mut |_| {}, &CancelToken::new())
            .await,
        Err(SftpError::AlreadyExists(_))
    ));
    let cancel = CancelToken::new();
    cancel.cancel();
    assert!(matches!(
        c.copy_file("/d/a.txt", "/d/c.txt", &mut |_| {}, &cancel)
            .await,
        Err(SftpError::Cancelled)
    ));
    assert!(!c.exists("/d/c.txt").await.unwrap(), "partial copy removed");
}
