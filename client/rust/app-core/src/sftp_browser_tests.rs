//! SFTP browser, transfer jobs and edit sessions through the facade against
//! sftp-core's in-process SFTP server (no SSH, no Docker). The file opener
//! records launches instead of starting applications.

use crate::platform::{FileOpener, LaunchOutput, LaunchSpec, Launcher, OpenError, OsFamily};
use crate::*;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const PASSPHRASE: &str = "correct horse vault passphrase 42";

#[derive(Debug, Default)]
struct Recorder {
    specs: Mutex<Vec<LaunchSpec>>,
}

impl Launcher for Recorder {
    fn launch(&self, spec: &LaunchSpec) -> Result<LaunchOutput, OpenError> {
        self.specs.lock().unwrap().push(spec.clone());
        Ok(LaunchOutput::default())
    }
}

struct Fixture {
    _tmp: tempfile::TempDir,
    app: AppCore,
    remote_root: PathBuf,
    local: PathBuf,
    host_id: String,
    sftp: String,
    launches: Arc<Recorder>,
}

async fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let remote_root = tmp.path().join("remote");
    let local = tmp.path().join("local");
    std::fs::create_dir_all(&remote_root).unwrap();
    std::fs::create_dir_all(&local).unwrap();
    let app = AppCore::new(AppConfig::for_tests(
        tmp.path().join("data").to_string_lossy().to_string(),
    ))
    .unwrap();
    let launches = Arc::new(Recorder::default());
    app.set_file_opener(FileOpener::new(OsFamily::current(), launches.clone()));
    app.create_local_profile("Sftp".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let host = app
        .save_host(HostDto::new("files", "files.example"))
        .await
        .unwrap();
    let client = cc_sftp_core::test_server::connect_local(remote_root.clone())
        .await
        .unwrap();
    let sftp = app
        .sftp_attach_client_for_tests(host.id.clone(), client)
        .await
        .unwrap();
    Fixture {
        _tmp: tmp,
        app,
        remote_root,
        local,
        host_id: host.id,
        sftp,
        launches,
    }
}

async fn wait_job(app: &AppCore, id: &str) -> TransferJobDto {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let j = app.sftp_transfer(id.to_owned()).await.unwrap();
        if !j.state.is_active() {
            return j;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "job {id} stuck: {j:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn write(p: &Path, data: &[u8]) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, data).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn browse_stat_resolve_create_duplicate_preview() {
    let f = fixture().await;
    let (app, sftp) = (&f.app, f.sftp.clone());
    write(&f.remote_root.join("www/index.html"), b"<h1>hi</h1>");
    write(&f.remote_root.join("www/css/site.css"), b"body{}");
    #[cfg(unix)]
    {
        // Relative targets: the test server resolves links on the real disk.
        std::os::unix::fs::symlink("index.html", f.remote_root.join("www/home")).unwrap();
        std::os::unix::fs::symlink("nowhere", f.remote_root.join("www/dangling")).unwrap();
    }

    let list = app
        .sftp_list_detailed(sftp.clone(), "/www".into())
        .await
        .unwrap();
    let css = list.iter().find(|e| e.name == "css").unwrap();
    assert_eq!(css.kind, "dir");
    let index = list.iter().find(|e| e.name == "index.html").unwrap();
    assert_eq!((index.kind.as_str(), index.size), ("file", 11));
    assert!(index.owner.is_some());
    #[cfg(unix)]
    assert!(index.uid.is_some());
    // The local Windows test server has no Unix UID in filesystem metadata.
    #[cfg(windows)]
    assert!(index.uid.is_none());
    #[cfg(unix)]
    {
        let link = list.iter().find(|e| e.name == "home").unwrap();
        assert_eq!(link.kind, "symlink");
        assert_eq!(link.link_target.as_deref(), Some("index.html"));
        assert_eq!(link.link_target_kind.as_deref(), Some("file"));
        let dangling = list.iter().find(|e| e.name == "dangling").unwrap();
        assert_eq!(dangling.link_target_kind, None);
        let st = app
            .sftp_stat(sftp.clone(), "/www/home".into())
            .await
            .unwrap();
        assert_eq!(st.kind, "symlink");
    }
    let e = app
        .sftp_list_detailed(sftp.clone(), "/missing".into())
        .await
        .unwrap_err();
    assert_eq!(
        (e.code(), e.reason()),
        ("not_found", Some("directory_not_found"))
    );
    assert_eq!(e.args()["path"], "/missing");

    // Typed paths.
    assert_eq!(
        app.sftp_resolve_directory(sftp.clone(), "css/..".into(), "/www".into())
            .await
            .unwrap(),
        "/www"
    );
    assert_eq!(
        app.sftp_resolve_directory(sftp.clone(), "~".into(), "/www".into())
            .await
            .unwrap(),
        "/"
    );
    let e = app
        .sftp_resolve_directory(sftp.clone(), "index.html".into(), "/www".into())
        .await
        .unwrap_err();
    assert_eq!(e.reason(), Some("directory_not_found"));

    // chmod / new file.
    app.sftp_chmod(sftp.clone(), "/www/index.html".into(), 0o600)
        .await
        .unwrap();
    app.sftp_create_file(sftp.clone(), "/www/new.txt".into())
        .await
        .unwrap();
    let e = app
        .sftp_create_file(sftp.clone(), "/www/new.txt".into())
        .await
        .unwrap_err();
    assert_eq!(
        (e.code(), e.reason(), e.args()["name"].as_str()),
        ("already_exists", Some("already_exists"), "new.txt")
    );

    // Recursive duplicate keeps modes (and links).
    app.sftp_duplicate(sftp.clone(), "/www".into(), "/www-copy".into())
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(f.remote_root.join("www-copy/css/site.css")).unwrap(),
        b"body{}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let m = std::fs::metadata(f.remote_root.join("www-copy/index.html")).unwrap();
        assert_eq!(m.permissions().mode() & 0o777, 0o600);
        let l = std::fs::read_link(f.remote_root.join("www-copy/home")).unwrap();
        assert_eq!(l, Path::new("index.html"));
    }
    let e = app
        .sftp_duplicate(sftp.clone(), "/www".into(), "/www-copy".into())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "already_exists");
    let e = app
        .sftp_duplicate(sftp.clone(), "/www".into(), "/www/inner".into())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "invalid_input");

    // Bounded preview.
    let p = app
        .sftp_read_preview(sftp.clone(), "/www/css/site.css".into(), 4)
        .await
        .unwrap();
    assert_eq!((p.data.as_slice(), p.total_size), (&b"body"[..], 6));
    let e = app
        .sftp_read_preview(sftp.clone(), "/www/css".into(), 4)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "invalid_input");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn recursive_transfers_cancel_and_retry() {
    let f = fixture().await;
    let (app, sftp) = (&f.app, f.sftp.clone());
    let mut events = app.subscribe_events();

    // Folder upload.
    write(&f.local.join("site/index.html"), b"12345");
    write(&f.local.join("site/assets/app.js"), &vec![7u8; 300_000]);
    let job = app
        .sftp_start_transfer(TransferRequestDto {
            transfer_id: "up-1".into(),
            sftp_id: sftp.clone(),
            direction: TransferDirectionDto::Upload,
            local_path: f.local.join("site").to_string_lossy().into_owned(),
            remote_path: "/site".into(),
        })
        .await
        .unwrap();
    assert_eq!(job.state, TransferStateDto::Queued);
    let done = wait_job(app, "up-1").await;
    assert_eq!(done.state, TransferStateDto::Completed, "{done:?}");
    assert!(done.is_directory);
    assert_eq!((done.files_done, done.files_total), (2, 2));
    assert_eq!(done.transferred, 300_005);
    assert_eq!(
        std::fs::read(f.remote_root.join("site/assets/app.js"))
            .unwrap()
            .len(),
        300_000
    );
    let mut saw_update = false;
    while let Ok(ev) = events.try_recv() {
        if let AppEvent::TransferUpdate(j) = ev {
            saw_update |= j.transfer_id == "up-1";
        }
    }
    assert!(saw_update, "transfer_update events");

    // Folder download.
    let dest = f.local.join("down");
    app.sftp_start_transfer(TransferRequestDto {
        transfer_id: "down-1".into(),
        sftp_id: sftp.clone(),
        direction: TransferDirectionDto::Download,
        local_path: dest.to_string_lossy().into_owned(),
        remote_path: "/site".into(),
    })
    .await
    .unwrap();
    assert_eq!(
        wait_job(app, "down-1").await.state,
        TransferStateDto::Completed
    );
    assert_eq!(std::fs::read(dest.join("index.html")).unwrap(), b"12345");

    // Failure → retry under a new id.
    app.sftp_start_transfer(TransferRequestDto {
        transfer_id: "bad".into(),
        sftp_id: sftp.clone(),
        direction: TransferDirectionDto::Download,
        local_path: f.local.join("x.bin").to_string_lossy().into_owned(),
        remote_path: "/later.bin".into(),
    })
    .await
    .unwrap();
    let failed = wait_job(app, "bad").await;
    assert_eq!(failed.state, TransferStateDto::Failed);
    assert_eq!(failed.error.as_ref().unwrap().code, "not_found");
    write(&f.remote_root.join("later.bin"), b"now here");
    app.sftp_retry_transfer("bad".into(), "bad-2".into())
        .await
        .unwrap();
    assert_eq!(
        wait_job(app, "bad-2").await.state,
        TransferStateDto::Completed
    );
    assert!(
        app.sftp_transfer("bad".into()).await.is_err(),
        "old job removed"
    );

    // Duplicate active id refused; cancel; clear.
    write(&f.local.join("big.bin"), &vec![1u8; 4 << 20]);
    app.sftp_start_transfer(TransferRequestDto {
        transfer_id: "big".into(),
        sftp_id: sftp.clone(),
        direction: TransferDirectionDto::Upload,
        local_path: f.local.join("big.bin").to_string_lossy().into_owned(),
        remote_path: "/big.bin".into(),
    })
    .await
    .unwrap();
    assert!(app.sftp_cancel_transfer("big".into()).await.unwrap());
    let j = wait_job(app, "big").await;
    assert_eq!(j.state, TransferStateDto::Cancelled);
    assert!(!app.sftp_cancel_transfer("big".into()).await.unwrap());
    assert!(app.sftp_transfers().await.unwrap().len() >= 4);
    app.sftp_clear_finished_transfers().await.unwrap();
    assert!(app.sftp_transfers().await.unwrap().is_empty());
}

async fn wait_status(
    app: &AppCore,
    id: &str,
    pred: impl Fn(&EditStatusDto) -> bool,
) -> EditSessionDto {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let s = app
            .edit_sessions()
            .await
            .unwrap()
            .into_iter()
            .find(|s| s.id == id)
            .expect("session");
        if pred(&s.status) {
            return s;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "status: {:?}",
            s.status
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn edit_sessions_upload_conflict_stop_and_leftovers() {
    let f = fixture().await;
    let (app, sftp) = (&f.app, f.sftp.clone());
    write(&f.remote_root.join("etc/app.conf"), b"port=80\n");

    let s = app
        .edit_open(sftp.clone(), "/etc/app.conf".into(), OpenWithDto::Default)
        .await
        .unwrap();
    assert_eq!(s.host_id, f.host_id);
    assert_eq!(s.sftp_id.as_deref(), Some(sftp.as_str()));
    assert_eq!(s.status, EditStatusDto::Synced);
    assert!(
        !f.launches.specs.lock().unwrap().is_empty(),
        "editor opened"
    );
    let work = PathBuf::from(&s.local_path);
    assert_eq!(std::fs::read(&work).unwrap(), b"port=80\n");
    // Same file again → same session.
    let again = app
        .edit_open(sftp.clone(), "/etc/app.conf".into(), OpenWithDto::Default)
        .await
        .unwrap();
    assert_eq!(again.id, s.id);

    // Save → upload.
    std::fs::write(&work, b"port=8080\n").unwrap();
    app.edit_sync_now(s.id.clone()).await.unwrap();
    wait_status(app, &s.id, |st| *st == EditStatusDto::Synced).await;
    assert_eq!(
        std::fs::read(f.remote_root.join("etc/app.conf")).unwrap(),
        b"port=8080\n"
    );

    // Someone else changes the remote → conflict → overwrite.
    std::fs::write(f.remote_root.join("etc/app.conf"), b"port=9999 # ops\n").unwrap();
    std::fs::write(&work, b"port=8081\n").unwrap();
    app.edit_sync_now(s.id.clone()).await.unwrap();
    wait_status(app, &s.id, |st| {
        matches!(st, EditStatusDto::Conflict { .. })
    })
    .await;
    app.edit_resolve(s.id.clone(), EditConflictResolutionDto::OverwriteRemote)
        .await
        .unwrap();
    wait_status(app, &s.id, |st| *st == EditStatusDto::Synced).await;
    assert_eq!(
        std::fs::read(f.remote_root.join("etc/app.conf")).unwrap(),
        b"port=8081\n"
    );

    // Reopen / reveal go through the (recording) opener.
    let before = f.launches.specs.lock().unwrap().len();
    app.edit_reopen(s.id.clone(), None).await.unwrap();
    app.edit_reveal(s.id.clone()).await.unwrap();
    assert_eq!(f.launches.specs.lock().unwrap().len(), before + 2);

    // Stop with upload → closed, working copy removed.
    std::fs::write(&work, b"port=1\n").unwrap();
    let out = app
        .edit_stop(s.id.clone(), EditStopModeDto::Upload)
        .await
        .unwrap();
    assert_eq!(out, EditStopOutcomeDto::Closed { uploaded: true });
    assert!(!work.exists());
    assert_eq!(
        std::fs::read(f.remote_root.join("etc/app.conf")).unwrap(),
        b"port=1\n"
    );
    assert!(app.edit_sessions().await.unwrap().is_empty());

    // Keep files → leftover → resume → discard.
    let s2 = app
        .edit_open(sftp.clone(), "/etc/app.conf".into(), OpenWithDto::Default)
        .await
        .unwrap();
    std::fs::write(&s2.local_path, b"port=2\n").unwrap();
    let out = app
        .edit_stop(s2.id.clone(), EditStopModeDto::KeepFiles)
        .await
        .unwrap();
    assert!(matches!(out, EditStopOutcomeDto::KeptFiles { .. }));
    let left = app.edit_leftovers().await.unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].locally_modified, Some(true));
    assert_eq!(left[0].host_id.as_deref(), Some(f.host_id.as_str()));
    let resumed = app
        .edit_resume(left[0].id.clone(), sftp.clone(), None)
        .await
        .unwrap();
    let conf = f.remote_root.join("etc/app.conf");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while std::fs::read(&conf).unwrap() != b"port=2\n" {
        assert!(
            tokio::time::Instant::now() < deadline,
            "resume uploads the unsaved edit"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    wait_status(app, &resumed.id, |st| *st == EditStatusDto::Synced).await;
    app.edit_stop(resumed.id.clone(), EditStopModeDto::KeepFiles)
        .await
        .unwrap();
    app.edit_discard_leftover(resumed.id.clone()).await.unwrap();
    assert!(app.edit_leftovers().await.unwrap().is_empty());

    // Not a regular file / too large.
    let e = app
        .edit_open(sftp.clone(), "/etc".into(), OpenWithDto::Default)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "invalid_input");

    // Closing the SFTP session stops its edit sessions (final upload).
    let s3 = app
        .edit_open(sftp.clone(), "/etc/app.conf".into(), OpenWithDto::Default)
        .await
        .unwrap();
    std::fs::write(&s3.local_path, b"port=3\n").unwrap();
    app.sftp_close(sftp.clone()).await.unwrap();
    assert!(app.edit_sessions().await.unwrap().is_empty());
    assert_eq!(
        std::fs::read(f.remote_root.join("etc/app.conf")).unwrap(),
        b"port=3\n"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lock_keeps_conflicting_edits_as_leftovers() {
    let f = fixture().await;
    let (app, sftp) = (&f.app, f.sftp.clone());
    write(&f.remote_root.join("notes.md"), b"# notes\n");
    let s = app
        .edit_open(sftp.clone(), "/notes.md".into(), OpenWithDto::Default)
        .await
        .unwrap();
    std::fs::write(f.remote_root.join("notes.md"), b"# changed remotely!\n").unwrap();
    std::fs::write(&s.local_path, b"# mine\n").unwrap();
    app.lock().await.unwrap();
    assert!(PathBuf::from(&s.local_path).exists(), "kept for recovery");
    let mut events = app.subscribe_events();
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    let left = app.edit_leftovers().await.unwrap();
    assert_eq!(left.len(), 1);
    let mut announced = false;
    while let Ok(ev) = events.try_recv() {
        announced |= matches!(ev, AppEvent::EditLeftovers { count: 1 });
    }
    assert!(announced, "leftovers announced after unlock");
}
