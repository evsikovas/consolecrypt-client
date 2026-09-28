//! SFTP integration test over the 2-hop jump chain (Docker).
//! Run with `CC_SSH_IT=1 cargo test -p cc-sftp-core --test docker_it -- --nocapture`.

use cc_sftp_core::{
    CancelToken, EntryKind, SftpClient, SftpError, TransferOptions, TransferProgress,
};
use cc_ssh_core::testing::{self, connector, SshTestbed};
use cc_ssh_core::{ConnectionPlanner, HostKeyDecision, MemoryKnownHosts};
use std::sync::Arc;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn docker_sftp_roundtrip_over_jump_chain() {
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
    let sftp = SftpClient::open(&session).await.unwrap();

    let home = sftp.home_dir().await.unwrap();
    assert_eq!(home, "/home/tester");
    let base = format!("{home}/it");
    sftp.mkdir_all(&format!("{base}/nested/deeper"))
        .await
        .unwrap();
    println!("IT sftp_mkdir_all ... ok");

    // 5 MiB upload + download with progress.
    let local = tempfile::tempdir().unwrap();
    let payload: Vec<u8> = (0..5 * 1024 * 1024u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 24) as u8)
        .collect();
    let src = local.path().join("payload.bin");
    std::fs::write(&src, &payload).unwrap();
    let remote = format!("{base}/payload.bin");
    let mut last = TransferProgress {
        transferred: 0,
        total: None,
    };
    let mut calls = 0usize;
    let up = sftp
        .upload(
            &src,
            &remote,
            &TransferOptions::default(),
            &mut |p| {
                last = p;
                calls += 1;
            },
            &CancelToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(up.bytes, payload.len() as u64);
    assert_eq!(last.transferred, payload.len() as u64);
    assert!(calls > 5);
    let st = sftp.stat(&remote).await.unwrap();
    assert_eq!(st.size, payload.len() as u64);
    assert_eq!(st.kind, EntryKind::File);
    // server-side checksum equals local content
    let sum = session
        .exec(&format!("sha256sum {remote} | cut -d' ' -f1"))
        .await
        .unwrap();
    assert_eq!(sum.stdout_lossy().trim(), sha256_hex(&payload));
    println!(
        "IT sftp_upload_5MiB ... ok ({} ms, {:.1} MiB/s)",
        up.elapsed_ms,
        payload.len() as f64 / 1048576.0 / (up.elapsed_ms.max(1) as f64 / 1000.0)
    );

    let dst = local.path().join("back.bin");
    let down = sftp
        .download(
            &remote,
            &dst,
            &TransferOptions::default(),
            &mut |_| {},
            &CancelToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(down.bytes, payload.len() as u64);
    assert_eq!(std::fs::read(&dst).unwrap(), payload);
    println!("IT sftp_download_roundtrip ... ok ({} ms)", down.elapsed_ms);

    // Cancelled upload leaves no partial file.
    let cancel = CancelToken::new();
    let c2 = cancel.clone();
    let partial = format!("{base}/partial.bin");
    let r = sftp
        .upload(
            &src,
            &partial,
            &TransferOptions {
                chunk_size: 64 * 1024,
                ..Default::default()
            },
            &mut move |p| {
                if p.transferred >= 256 * 1024 {
                    c2.cancel();
                }
            },
            &cancel,
        )
        .await;
    assert!(matches!(r, Err(SftpError::Cancelled)), "{r:?}");
    assert!(!sftp.exists(&partial).await.unwrap());
    println!("IT sftp_cancel_upload ... ok");

    // chmod / rename / list / remove
    sftp.chmod(&remote, 0o600).await.unwrap();
    assert_eq!(sftp.stat(&remote).await.unwrap().permissions, Some(0o600));
    let renamed = format!("{base}/nested/renamed.bin");
    sftp.rename(&remote, &renamed).await.unwrap();
    let list = sftp.list(&format!("{base}/nested")).await.unwrap();
    let names: Vec<_> = list.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["deeper", "renamed.bin"]);
    assert_eq!(list[1].mode_string(), "-rw-------");
    assert!(matches!(
        sftp.stat(&remote).await,
        Err(SftpError::NotFound(_))
    ));
    assert!(matches!(
        sftp.list("/root").await,
        Err(SftpError::PermissionDenied(_)) | Err(SftpError::Remote { .. })
    ));
    sftp.remove_dir(&base, true).await.unwrap();
    assert!(!sftp.exists(&base).await.unwrap());
    println!("IT sftp_chmod_rename_list_remove ... ok");

    sftp.close().await.unwrap();
    session.disconnect().await.unwrap();
    drop(tb);
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
