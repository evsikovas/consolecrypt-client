//! Engine tests: fake remote + fake opener + real temp dir and real watcher.

use super::fake::{FakeOpener, FakeRemote};
use super::{cfg, eventually, EXTRA_WAIT};
use crate::edit::*;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

const HOST: &str = "host-1";
const FILE: &str = "/srv/app/config.php";

struct Fx {
    _tmp: tempfile::TempDir,
    root: std::path::PathBuf,
    mgr: EditManager,
    remote: Arc<FakeRemote>,
    opener: Arc<FakeOpener>,
}

fn fx_with(f: impl FnOnce(&mut EditConfig)) -> Fx {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("profiles/p1/cache/edit");
    let mut c = cfg(&root);
    f(&mut c);
    let opener = FakeOpener::new();
    let mgr = EditManager::new(c, opener.clone()).unwrap();
    let remote = FakeRemote::new();
    remote.put(FILE, b"<?php $db = 'original';\n", 0o640);
    Fx {
        _tmp: tmp,
        root,
        mgr,
        remote,
        opener,
    }
}

fn fx() -> Fx {
    fx_with(|_| {})
}

impl Fx {
    async fn open(&self) -> EditSession {
        self.mgr
            .open(self.remote.clone(), HOST, FILE, OpenWith::Default)
            .await
            .unwrap()
    }
}

fn uploads(s: &EditSession) -> u64 {
    s.info().uploads
}

fn session_dirs(root: &Path) -> Vec<String> {
    std::fs::read_dir(root)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(unix)]
fn mode(p: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn open_downloads_privately_and_opens_editor() {
    let f = fx();
    let s = f.open().await;
    let local = s.local_path();
    assert_eq!(std::fs::read(&local).unwrap(), b"<?php $db = 'original';\n");
    assert_eq!(local.file_name().unwrap(), "config.php");
    assert_eq!(s.status(), EditStatus::Synced);
    let info = s.info();
    assert_eq!(info.remote_path, FILE);
    assert_eq!(info.target_path, FILE);
    assert_eq!(info.host_id, HOST);
    assert_eq!(f.opener.calls(), vec![(local.clone(), OpenWith::Default)]);
    // <root>/<uuid>/<name>
    let dir = local.parent().unwrap();
    assert_eq!(
        dir.file_name().unwrap().to_string_lossy(),
        s.id().to_string()
    );
    assert_eq!(
        std::fs::canonicalize(dir.parent().unwrap()).unwrap(),
        std::fs::canonicalize(&f.root).unwrap()
    );
    #[cfg(unix)]
    {
        assert_eq!(mode(&local), 0o600);
        assert_eq!(mode(dir), 0o700);
        assert_eq!(mode(&f.root), 0o700);
    }
    #[cfg(target_os = "macos")]
    assert!(f.root.join(".metadata_never_index").exists());
    // The manifest holds metadata only, never the content.
    let manifest = std::fs::read_to_string(dir.join(".cc-edit-session.json")).unwrap();
    assert!(manifest.contains(FILE) && !manifest.contains("original"));
    assert_eq!(f.mgr.sessions().len(), 1);
    s.stop(StopMode::Discard).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn save_uploads_once_after_debounce() {
    let f = fx();
    let mut events = f.mgr.subscribe();
    let s = f.open().await;
    let edited = b"<?php $db = 'edited';\n";
    std::fs::write(s.local_path(), edited).unwrap();
    eventually("upload", || uploads(&s) == 1).await;
    assert_eq!(f.remote.data(FILE).unwrap(), b"<?php $db = 'edited';\n");
    assert_eq!(s.status(), EditStatus::Synced);
    tokio::time::sleep(EXTRA_WAIT).await;
    assert_eq!(uploads(&s), 1, "no repeated upload");
    assert_eq!(f.remote.count("create "), 1);
    // atomic replace via temp file + posix-rename, original mode restored
    let ops = f.remote.ops();
    let tmp = ops
        .iter()
        .find_map(|o| o.strip_prefix("create "))
        .unwrap()
        .to_string();
    assert!(tmp.starts_with("/srv/app/.config.php.cc-upload-"), "{tmp}");
    assert!(ops.contains(&format!("chmod {tmp} 640")));
    assert!(ops.contains(&format!("posix_rename {tmp} {FILE}")));
    assert_eq!(f.remote.file(FILE).unwrap().perms, 0o640);
    assert_eq!(
        f.remote.paths(),
        vec![FILE.to_string()],
        "no temp leftovers"
    );
    // event stream: Opening → Synced → Uploading… → Synced
    let mut seen = Vec::new();
    while let Ok(e) = events.try_recv() {
        assert_eq!(e.session_id, s.id());
        seen.push(e.status);
    }
    assert_eq!(seen.first(), Some(&EditStatus::Opening));
    assert!(seen
        .iter()
        .any(|st| matches!(st, EditStatus::Uploading { progress } if progress.total == Some(edited.len() as u64))));
    assert_eq!(seen.last(), Some(&EditStatus::Synced));
    s.stop(StopMode::Upload).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rapid_saves_are_coalesced() {
    let f = fx_with(|c| c.debounce = Duration::from_millis(400));
    let s = f.open().await;
    for i in 0..6 {
        std::fs::write(s.local_path(), format!("version {i}\n")).unwrap();
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    eventually("upload", || uploads(&s) >= 1).await;
    tokio::time::sleep(EXTRA_WAIT).await;
    assert_eq!(uploads(&s), 1);
    assert_eq!(f.remote.count("create "), 1);
    assert_eq!(f.remote.data(FILE).unwrap(), b"version 5\n");
    s.stop(StopMode::Discard).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn editor_save_patterns_temp_rename_and_recreate() {
    let f = fx();
    let s = f.open().await;
    let local = s.local_path();
    let dir = local.parent().unwrap().to_path_buf();

    // 1. write a temp file, rename it over the working copy (vim, VS Code, …)
    let tmp = dir.join(".config.php.tmp.1234");
    std::fs::write(&tmp, b"saved via rename\n").unwrap();
    std::fs::rename(&tmp, &local).unwrap();
    eventually("rename save", || uploads(&s) == 1).await;
    assert_eq!(f.remote.data(FILE).unwrap(), b"saved via rename\n");

    // 2. delete + recreate
    std::fs::remove_file(&local).unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    std::fs::write(&local, b"saved via recreate\n").unwrap();
    eventually("recreate save", || uploads(&s) == 2).await;
    assert_eq!(f.remote.data(FILE).unwrap(), b"saved via recreate\n");
    assert_eq!(s.status(), EditStatus::Synced);
    s.stop(StopMode::Upload).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn editor_artifacts_are_ignored() {
    let f = fx();
    let s = f.open().await;
    let dir = s.local_path().parent().unwrap().to_path_buf();
    for name in [
        ".config.php.swp",
        "config.php~",
        ".#config.php",
        "4913",
        ".DS_Store",
    ] {
        std::fs::write(dir.join(name), b"artefact").unwrap();
    }
    tokio::time::sleep(EXTRA_WAIT).await;
    assert_eq!(f.remote.count("create "), 0);
    assert_eq!(uploads(&s), 0);
    assert_eq!(s.status(), EditStatus::Synced);
    // Artefacts are removed with the session directory.
    s.stop(StopMode::Upload).await.unwrap();
    assert!(!dir.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remote_change_gives_conflict_and_no_overwrite() {
    let f = fx();
    let s = f.open().await;
    f.remote.edit_by_other(FILE, b"changed by a colleague\n");
    std::fs::write(s.local_path(), b"my edit\n").unwrap();
    eventually("conflict", || {
        matches!(s.status(), EditStatus::Conflict { .. })
    })
    .await;
    let EditStatus::Conflict { remote_meta } = s.status() else {
        unreachable!()
    };
    assert_eq!(remote_meta.unwrap().size, 23);
    assert_eq!(f.remote.data(FILE).unwrap(), b"changed by a colleague\n");
    assert_eq!(f.remote.count("create "), 0, "nothing uploaded");
    // further saves while in conflict do not upload either
    std::fs::write(s.local_path(), b"my edit 2\n").unwrap();
    tokio::time::sleep(EXTRA_WAIT).await;
    assert_eq!(f.remote.count("create "), 0);
    assert!(matches!(
        s.sync_now().await,
        Err(EditError::InvalidState(_))
    ));
    // Stop editing asks the user instead of uploading / deleting.
    let out = s.stop(StopMode::Upload).await.unwrap();
    assert!(matches!(out, StopOutcome::Conflict { .. }), "{out:?}");
    assert!(!s.is_closed());
    assert_eq!(f.mgr.sessions().len(), 1);
    // OverwriteRemote
    assert_eq!(
        s.resolve(ConflictResolution::OverwriteRemote)
            .await
            .unwrap(),
        None
    );
    assert_eq!(f.remote.data(FILE).unwrap(), b"my edit 2\n");
    assert_eq!(s.status(), EditStatus::Synced);
    assert!(matches!(
        s.resolve(ConflictResolution::OverwriteRemote).await,
        Err(EditError::InvalidState(_))
    ));
    s.stop(StopMode::Upload).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resolve_keep_remote_copy_locally() {
    let f = fx();
    let s = f.open().await;
    f.remote.edit_by_other(FILE, b"theirs\n");
    std::fs::write(s.local_path(), b"mine\n").unwrap();
    eventually("conflict", || {
        matches!(s.status(), EditStatus::Conflict { .. })
    })
    .await;
    let copy = s
        .resolve(ConflictResolution::KeepRemoteCopyLocally)
        .await
        .unwrap()
        .unwrap();
    let name = copy.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with("config.remote-") && name.ends_with(".php"),
        "{name}"
    );
    assert_eq!(copy.parent(), s.local_path().parent());
    assert_eq!(std::fs::read(&copy).unwrap(), b"theirs\n");
    #[cfg(unix)]
    assert_eq!(mode(&copy), 0o600);
    assert_eq!(
        f.opener.calls().last().unwrap().0,
        copy,
        "copy opened for comparison"
    );
    assert_eq!(s.info().remote_copies, vec![copy.clone()]);
    assert_eq!(s.status(), EditStatus::Modified);
    assert_eq!(
        f.remote.data(FILE).unwrap(),
        b"theirs\n",
        "remote untouched"
    );
    // After merging, the next save uploads against the new base (no conflict).
    std::fs::write(s.local_path(), b"merged\n").unwrap();
    eventually("upload after merge", || uploads(&s) == 1).await;
    assert_eq!(f.remote.data(FILE).unwrap(), b"merged\n");
    assert_eq!(s.status(), EditStatus::Synced);
    s.stop(StopMode::Upload).await.unwrap();
    assert!(!copy.exists(), "remote copy removed with the session");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resolve_discard_local() {
    let f = fx();
    let s = f.open().await;
    f.remote.edit_by_other(FILE, b"theirs\n");
    std::fs::write(s.local_path(), b"mine\n").unwrap();
    eventually("conflict", || {
        matches!(s.status(), EditStatus::Conflict { .. })
    })
    .await;
    assert_eq!(
        s.resolve(ConflictResolution::DiscardLocal).await.unwrap(),
        None
    );
    assert_eq!(std::fs::read(s.local_path()).unwrap(), b"theirs\n");
    #[cfg(unix)]
    assert_eq!(mode(&s.local_path()), 0o600);
    assert_eq!(s.status(), EditStatus::Synced);
    tokio::time::sleep(EXTRA_WAIT).await;
    assert_eq!(
        f.remote.count("create "),
        0,
        "rewriting the copy uploads nothing"
    );
    assert_eq!(s.status(), EditStatus::Synced);
    // and editing continues normally
    std::fs::write(s.local_path(), b"theirs + mine\n").unwrap();
    eventually("upload", || uploads(&s) == 1).await;
    assert_eq!(f.remote.data(FILE).unwrap(), b"theirs + mine\n");
    s.stop(StopMode::Upload).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remote_deleted_meanwhile_is_a_conflict() {
    let f = fx();
    let s = f.open().await;
    f.remote.delete(FILE);
    std::fs::write(s.local_path(), b"mine\n").unwrap();
    eventually("conflict", || {
        s.status() == EditStatus::Conflict { remote_meta: None }
    })
    .await;
    assert!(s.resolve(ConflictResolution::DiscardLocal).await.is_err());
    s.resolve(ConflictResolution::OverwriteRemote)
        .await
        .unwrap();
    assert_eq!(f.remote.data(FILE).unwrap(), b"mine\n");
    assert_eq!(f.remote.file(FILE).unwrap().perms, 0o640, "original mode");
    s.stop(StopMode::Upload).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_failure_then_manual_retry() {
    let f = fx();
    let s = f.open().await;
    f.remote.with(|st| st.fail_writes = 1);
    std::fs::write(s.local_path(), b"edit\n").unwrap();
    eventually("error", || matches!(s.status(), EditStatus::Error { .. })).await;
    let EditStatus::Error { message, retryable } = s.status() else {
        unreachable!()
    };
    assert!(retryable);
    assert!(message.contains("injected failure"), "{message}");
    assert_eq!(f.remote.data(FILE).unwrap(), b"<?php $db = 'original';\n");
    assert!(s.local_path().exists(), "local copy kept");
    assert_eq!(s.sync_now().await.unwrap(), EditStatus::Synced);
    assert_eq!(f.remote.data(FILE).unwrap(), b"edit\n");
    assert_eq!(uploads(&s), 1);
    s.stop(StopMode::Upload).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transient_failures_are_retried_automatically() {
    let f = fx_with(|c| {
        c.auto_retries = 3;
        c.retry_backoff = Duration::from_millis(40);
    });
    let s = f.open().await;
    f.remote.with(|st| st.fail_writes = 2);
    std::fs::write(s.local_path(), b"edit\n").unwrap();
    eventually("auto retry", || uploads(&s) == 1).await;
    assert_eq!(f.remote.data(FILE).unwrap(), b"edit\n");
    assert_eq!(f.remote.count("create "), 3);
    assert_eq!(s.status(), EditStatus::Synced);
    s.stop(StopMode::Upload).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fallback_without_posix_rename_removes_then_renames() {
    let f = fx();
    f.remote.with(|st| st.posix_rename = false);
    let s = f.open().await;
    std::fs::write(s.local_path(), b"edit\n").unwrap();
    eventually("upload", || uploads(&s) == 1).await;
    let ops = f.remote.ops();
    let tmp = ops
        .iter()
        .find_map(|o| o.strip_prefix("create "))
        .unwrap()
        .to_string();
    let tail: Vec<_> = ops
        .iter()
        .skip_while(|o| !o.starts_with("rename"))
        .cloned()
        .collect();
    assert_eq!(
        tail,
        vec![
            format!("rename {tmp} {FILE}"),
            format!("remove {FILE}"),
            format!("rename {tmp} {FILE}"),
        ]
    );
    assert_eq!(f.remote.data(FILE).unwrap(), b"edit\n");
    assert_eq!(f.remote.file(FILE).unwrap().perms, 0o640);
    s.stop(StopMode::Upload).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn in_place_fallbacks_keep_owner() {
    // Directory not writable → in place.
    let f = fx();
    f.remote.with(|st| st.deny_create = true);
    let s = f.open().await;
    std::fs::write(s.local_path(), b"edit 1\n").unwrap();
    eventually("upload", || uploads(&s) == 1).await;
    assert_eq!(f.remote.count(&format!("overwrite {FILE}")), 1);
    assert_eq!(f.remote.data(FILE).unwrap(), b"edit 1\n");
    s.stop(StopMode::Upload).await.unwrap();

    // Replacement would change the owner → in place.
    let f = fx();
    f.remote.with(|st| st.new_file_uid = 0);
    let s = f.open().await;
    std::fs::write(s.local_path(), b"edit 2\n").unwrap();
    eventually("upload", || uploads(&s) == 1).await;
    assert_eq!(f.remote.count(&format!("overwrite {FILE}")), 1);
    assert_eq!(f.remote.count("posix_rename"), 0);
    assert_eq!(f.remote.file(FILE).unwrap().uid, 1000);
    assert_eq!(
        f.remote.paths(),
        vec![FILE.to_string()],
        "temp file removed"
    );
    s.stop(StopMode::Upload).await.unwrap();

    // Disabled fallback: the error is reported instead.
    let f = fx_with(|c| c.in_place_fallback = false);
    f.remote.with(|st| st.deny_create = true);
    let s = f.open().await;
    std::fs::write(s.local_path(), b"edit 3\n").unwrap();
    eventually("error", || matches!(s.status(), EditStatus::Error { .. })).await;
    assert_eq!(f.remote.count("overwrite"), 0);
    s.stop(StopMode::Discard).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_uploads_pending_change_and_cleans_up() {
    let f = fx_with(|c| c.debounce = Duration::from_secs(30));
    let s = f.open().await;
    let dir = s.local_path().parent().unwrap().to_path_buf();
    std::fs::write(s.local_path(), b"last minute edit\n").unwrap();
    // Stop before the (long) debounce fires: the final upload happens anyway.
    let out = s.stop(StopMode::Upload).await.unwrap();
    assert_eq!(out, StopOutcome::Closed { uploaded: true });
    assert_eq!(f.remote.data(FILE).unwrap(), b"last minute edit\n");
    assert!(!dir.exists(), "working directory removed");
    assert_eq!(s.status(), EditStatus::Closed);
    assert!(s.is_closed());
    assert!(f.mgr.sessions().is_empty());
    assert!(f.mgr.leftovers().unwrap().is_empty());
    assert!(matches!(s.sync_now().await, Err(EditError::Closed)));

    // Nothing to upload → Closed { uploaded: false }.
    let s = f.open().await;
    assert_eq!(
        s.stop(StopMode::Upload).await.unwrap(),
        StopOutcome::Closed { uploaded: false }
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unattended_stop_keeps_files_and_recovery_resumes() {
    let f = fx();
    let s = f.open().await;
    let id = s.id();
    f.remote.with(|st| st.fail_writes = 100);
    std::fs::write(s.local_path(), b"unsaved work\n").unwrap();
    let out = s.stop(StopMode::UploadOrKeep).await.unwrap();
    let StopOutcome::KeptFiles { dir } = out else {
        panic!("{out:?}")
    };
    assert!(dir.join("config.php").exists());
    assert!(f.mgr.sessions().is_empty());
    // A junk directory in the edit root is not a leftover.
    std::fs::create_dir(f.root.join("not-a-session")).unwrap();

    let left = f.mgr.leftovers().unwrap();
    assert_eq!(left.len(), 1);
    let l = &left[0];
    assert_eq!(l.session_id, id);
    assert_eq!(l.host_id.as_deref(), Some(HOST));
    assert_eq!(l.remote_path.as_deref(), Some(FILE));
    assert_eq!(l.locally_modified, Some(true));
    assert_eq!(list_leftovers(&f.root).unwrap(), left);

    // "Recover unsaved edits?" → resume uploads the working copy.
    f.remote.with(|st| st.fail_writes = 0);
    let s = f.mgr.resume(f.remote.clone(), id, None).await.unwrap();
    assert_eq!(s.id(), id);
    eventually("resumed upload", || uploads(&s) == 1).await;
    assert_eq!(f.remote.data(FILE).unwrap(), b"unsaved work\n");
    assert!(
        f.mgr.leftovers().unwrap().is_empty(),
        "active sessions are not leftovers"
    );
    assert!(matches!(
        f.mgr.discard_leftover(id),
        Err(EditError::InvalidState(_))
    ));
    s.stop(StopMode::Upload).await.unwrap();
    assert!(!dir.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn leftovers_can_be_discarded() {
    let f = fx();
    let a = f.open().await;
    let other = "/srv/app/other.txt";
    f.remote.put(other, b"x", 0o644);
    let b = f
        .mgr
        .open(f.remote.clone(), HOST, other, OpenWith::Default)
        .await
        .unwrap();
    let a_dir = a.stop(StopMode::KeepFiles).await.unwrap();
    assert!(matches!(a_dir, StopOutcome::KeptFiles { .. }));
    b.stop(StopMode::KeepFiles).await.unwrap();
    // Remote untouched by KeepFiles.
    assert_eq!(f.remote.count("create "), 0);
    let left = f.mgr.leftovers().unwrap();
    assert_eq!(left.len(), 2);
    assert!(left.iter().all(|l| l.locally_modified == Some(false)));
    f.mgr.discard_leftover(a.id()).unwrap();
    assert!(matches!(
        f.mgr.discard_leftover(a.id()),
        Err(EditError::UnknownSession)
    ));
    assert_eq!(f.mgr.discard_all_leftovers().unwrap(), 1);
    assert!(session_dirs(&f.root).is_empty());
    // After a crash (session dir without a readable manifest) it is still listed.
    let crashed = uuid::Uuid::new_v4();
    std::fs::create_dir(f.root.join(crashed.to_string())).unwrap();
    let left = list_leftovers(&f.root).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].host_id, None);
    remove_leftover(&f.root, crashed, 1024).unwrap();
    assert!(list_leftovers(&f.root).unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn size_limits() {
    let f = fx_with(|c| c.max_file_size = 64);
    f.remote.put("/big.bin", &[7u8; 65], 0o644);
    let r = f
        .mgr
        .open(f.remote.clone(), HOST, "/big.bin", OpenWith::Default)
        .await;
    assert!(
        matches!(
            r,
            Err(EditError::TooLarge {
                size: 65,
                limit: 64
            })
        ),
        "{r:?}"
    );
    assert!(session_dirs(&f.root).is_empty());
    assert!(f.opener.calls().is_empty());

    // A local copy that grows beyond the limit is not uploaded.
    let s = f.open().await;
    std::fs::write(s.local_path(), [b'x'; 100]).unwrap();
    eventually("error", || matches!(s.status(), EditStatus::Error { .. })).await;
    assert!(matches!(
        s.status(),
        EditStatus::Error {
            retryable: false,
            ..
        }
    ));
    assert_eq!(f.remote.count("create "), 0);
    // Shrinking it again uploads normally.
    std::fs::write(s.local_path(), b"small again\n").unwrap();
    eventually("upload", || uploads(&s) == 1).await;
    s.stop(StopMode::Upload).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_session_per_path_and_symlinks_resolve() {
    let f = fx();
    let a = f.open().await;
    let b = f
        .mgr
        .open(
            f.remote.clone(),
            HOST,
            "/srv/app/config.php/",
            OpenWith::Default,
        )
        .await
        .unwrap();
    assert_eq!(a.id(), b.id());
    assert_eq!(f.mgr.sessions().len(), 1);
    assert_eq!(f.opener.calls().len(), 2, "editor brought up again");
    assert_eq!(f.remote.count("read "), 1, "downloaded once");
    // same path on another host is a different session
    let c = f
        .mgr
        .open(f.remote.clone(), "host-2", FILE, OpenWith::Default)
        .await
        .unwrap();
    assert_ne!(a.id(), c.id());
    c.stop(StopMode::Discard).await.unwrap();

    // A symlink edits (and replaces) its target, never the link itself.
    f.remote.symlink("/etc/site.conf", FILE);
    let l = f
        .mgr
        .open(f.remote.clone(), HOST, "/etc/site.conf", OpenWith::Default)
        .await
        .unwrap();
    assert_eq!(l.id(), a.id(), "link and target share the session");
    a.stop(StopMode::Discard).await.unwrap();

    let l = f
        .mgr
        .open(f.remote.clone(), HOST, "/etc/site.conf", OpenWith::Default)
        .await
        .unwrap();
    let info = l.info();
    assert_eq!(info.remote_path, "/etc/site.conf");
    assert_eq!(info.target_path, FILE);
    assert_eq!(info.local_path.file_name().unwrap(), "site.conf");
    assert_eq!(f.mgr.find(HOST, FILE).unwrap().id(), l.id());
    std::fs::write(l.local_path(), b"via link\n").unwrap();
    eventually("upload", || uploads(&l) == 1).await;
    assert_eq!(f.remote.data(FILE).unwrap(), b"via link\n");
    assert!(f.remote.file("/etc/site.conf").unwrap().link.is_some());
    l.stop(StopMode::Upload).await.unwrap();

    // missing files are reported
    let r = f
        .mgr
        .open(f.remote.clone(), HOST, "/nope", OpenWith::Default)
        .await;
    assert!(matches!(
        r,
        Err(EditError::Sftp(crate::SftpError::NotFound(_)))
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn opener_cancel_or_failure_cleans_up() {
    let f = fx();
    f.opener.set_next(Ok(ChooseOutcome::Cancelled));
    let r = f
        .mgr
        .open(f.remote.clone(), HOST, FILE, OpenWith::Choose)
        .await;
    assert!(matches!(r, Err(EditError::Cancelled)), "{r:?}");
    assert!(session_dirs(&f.root).is_empty());
    assert!(f.mgr.sessions().is_empty());

    f.opener.set_next(Err("no application".into()));
    let r = f
        .mgr
        .open(f.remote.clone(), HOST, FILE, OpenWith::Default)
        .await;
    assert!(matches!(r, Err(EditError::Opener(ref m)) if m.contains("no application")));
    assert!(session_dirs(&f.root).is_empty());

    // A chooser's pick is remembered for re-opens.
    let s = f
        .mgr
        .open(f.remote.clone(), HOST, FILE, OpenWith::Choose)
        .await
        .unwrap();
    s.reopen(None).await.unwrap();
    let calls = f.opener.calls();
    assert_eq!(
        calls.last().unwrap().1,
        OpenWith::App(AppRef::Name("Chosen".into()))
    );
    let text_edit = OpenWith::App(AppRef::BundleId("com.apple.TextEdit".into()));
    s.reopen(Some(text_edit.clone())).await.unwrap();
    assert_eq!(f.opener.calls().last().unwrap().1, text_edit);
    s.stop(StopMode::Discard).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_all_and_stop_host() {
    let f = fx();
    f.remote.put("/a.txt", b"a", 0o644);
    f.remote.put("/b.txt", b"b", 0o644);
    let a = f
        .mgr
        .open(f.remote.clone(), "h1", "/a.txt", OpenWith::Default)
        .await
        .unwrap();
    let b = f
        .mgr
        .open(f.remote.clone(), "h2", "/b.txt", OpenWith::Default)
        .await
        .unwrap();
    std::fs::write(a.local_path(), b"a2").unwrap();
    let out = f.mgr.stop_host("h1", StopMode::Upload).await;
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0, a.id());
    assert_eq!(f.remote.data("/a.txt").unwrap(), b"a2");
    assert_eq!(f.mgr.sessions().len(), 1);
    let out = f.mgr.stop_all(StopMode::UploadOrKeep).await;
    assert_eq!(out.len(), 1);
    assert!(b.is_closed());
    assert!(f.mgr.sessions().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn polling_safety_net_detects_changes() {
    // Events are the primary source; the poll catches anything they miss.
    let f = fx_with(|c| {
        c.debounce = Duration::from_secs(3600);
        c.poll_interval = Some(Duration::from_millis(100));
    });
    let s = f.open().await;
    std::fs::write(s.local_path(), b"seen by polling\n").unwrap();
    eventually("upload", || uploads(&s) == 1).await;
    assert_eq!(f.remote.data(FILE).unwrap(), b"seen by polling\n");
    s.stop(StopMode::Upload).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_the_manager_keeps_files_for_recovery() {
    let f = fx();
    let s = f.open().await;
    let id = s.id();
    let dir = s.local_path().parent().unwrap().to_path_buf();
    let status = s.watch_status();
    drop(s);
    let Fx {
        mgr, root, _tmp, ..
    } = f;
    drop(mgr);
    eventually("closed", || *status.borrow() == EditStatus::Closed).await;
    assert!(dir.join("config.php").exists(), "files kept");
    let left = list_leftovers(&root).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].session_id, id);
    assert_eq!(left[0].locally_modified, Some(false));
}
