//! Edit engine tests. No test launches an application (fake opener) or
//! touches the user's data directories (temp dirs only).

mod engine;
mod fake;
mod sftp;

use super::store::{
    remote_base_name, remote_copy_name, remote_parent, sanitize_file_name, secure_remove_dir,
    split_ext,
};
use super::watch::is_relevant;
use super::{is_editor_artifact, EditConfig};
use notify_debouncer_full::notify::event::{
    AccessKind, AccessMode, CreateKind, DataChange, ModifyKind, RemoveKind, RenameMode,
};
use notify_debouncer_full::notify::{Event, EventKind};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long "nothing else happens" is observed.
pub(crate) const EXTRA_WAIT: Duration = Duration::from_millis(900);

/// Fast test configuration: short debounce, no polling (the watcher is
/// under test), no automatic retries, no Time Machine calls.
pub(crate) fn cfg(root: &Path) -> EditConfig {
    EditConfig {
        debounce: Duration::from_millis(150),
        poll_interval: None,
        auto_retries: 0,
        retry_backoff: Duration::from_millis(50),
        exclude_from_backup: false,
        ..EditConfig::new(root)
    }
}

pub(crate) async fn eventually(what: &str, f: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while std::time::Instant::now() < deadline {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn artifacts() {
    for n in [
        ".config.php.swp",
        "x.swo",
        ".x.swx",
        "config.php~",
        ".#config.php",
        "4913",
        ".DS_Store",
    ] {
        assert!(is_editor_artifact(n), "{n}");
    }
    for n in ["config.php", "4914", "swp", "a.swp.txt", "x.remote-1.php"] {
        assert!(!is_editor_artifact(n), "{n}");
    }
}

#[test]
fn event_relevance() {
    let dir = PathBuf::from("/e/uuid");
    let ev = |kind: EventKind, names: &[&str]| {
        names
            .iter()
            .fold(Event::new(kind), |e, n| e.add_path(dir.join(n)))
    };
    let modify = EventKind::Modify(ModifyKind::Data(DataChange::Content));
    let rename = EventKind::Modify(ModifyKind::Name(RenameMode::Both));
    let name = "config.php";
    assert!(is_relevant(&ev(modify, &[name]), &dir, name));
    assert!(is_relevant(
        &ev(EventKind::Create(CreateKind::File), &[name]),
        &dir,
        name
    ));
    assert!(is_relevant(
        &ev(EventKind::Remove(RemoveKind::File), &[name]),
        &dir,
        name
    ));
    // temp file renamed over the working copy
    assert!(is_relevant(
        &ev(rename, &[".config.php.tmp", name]),
        &dir,
        name
    ));
    // artefacts and unrelated files
    for other in [
        ".config.php.swp",
        "config.php~",
        ".#config.php",
        "4913",
        ".DS_Store",
    ] {
        assert!(!is_relevant(&ev(modify, &[other]), &dir, name), "{other}");
    }
    assert!(!is_relevant(
        &ev(modify, &["config.remote-1.php"]),
        &dir,
        name
    ));
    assert!(!is_relevant(
        &ev(modify, &[".cc-edit-session.json"]),
        &dir,
        name
    ));
    // reads never count; a write-close does (inotify)
    assert!(!is_relevant(
        &ev(
            EventKind::Access(AccessKind::Open(AccessMode::Any)),
            &[name]
        ),
        &dir,
        name
    ));
    assert!(is_relevant(
        &ev(
            EventKind::Access(AccessKind::Close(AccessMode::Write)),
            &[name]
        ),
        &dir,
        name
    ));
    // directory-level / unknown events trigger a (cheap) re-check
    assert!(is_relevant(
        &Event::new(modify).add_path(dir.clone()),
        &dir,
        name
    ));
    assert!(is_relevant(&Event::new(EventKind::Any), &dir, name));
    assert!(is_relevant(
        &Event::new(modify)
            .add_path(dir.join("4913"))
            .set_flag(notify_debouncer_full::notify::event::Flag::Rescan),
        &dir,
        name
    ));
    // a working copy whose name looks like an artefact is still watched
    assert!(is_relevant(
        &ev(modify, &["notes.txt~"]),
        &dir,
        "notes.txt~"
    ));
}

#[test]
fn file_names() {
    assert_eq!(sanitize_file_name("index.php"), "index.php");
    assert_eq!(sanitize_file_name("a:b*c?.txt"), "a_b_c_.txt");
    assert_eq!(sanitize_file_name("tab\there"), "tab_here");
    assert_eq!(sanitize_file_name("trailing. . "), "trailing");
    assert_eq!(sanitize_file_name(".."), "file");
    assert_eq!(sanitize_file_name(""), "file");
    assert_eq!(sanitize_file_name("CON"), "_CON");
    assert_eq!(sanitize_file_name("com1.txt"), "_com1.txt");
    assert_eq!(sanitize_file_name("console.log"), "console.log");
    assert_eq!(
        sanitize_file_name(".cc-edit-session.json"),
        "_.cc-edit-session.json"
    );
    let long = format!("{}.conf", "ж".repeat(300));
    let s = sanitize_file_name(&long);
    assert!(s.len() <= 200 && s.ends_with(".conf"), "{s}");
    assert_eq!(sanitize_file_name(&s), s, "idempotent");

    assert_eq!(split_ext("a.tar.gz"), ("a.tar", Some("gz")));
    assert_eq!(split_ext(".bashrc"), (".bashrc", None));
    assert_eq!(split_ext("Makefile"), ("Makefile", None));
    let ts = chrono::DateTime::from_timestamp(1_790_000_000, 0).unwrap();
    assert_eq!(
        remote_copy_name("index.php", ts, 0),
        "index.remote-20260921T141320Z.php"
    );
    assert_eq!(
        remote_copy_name(".bashrc", ts, 2),
        ".bashrc.remote-20260921T141320Z-2"
    );

    assert_eq!(remote_base_name("/var/www/index.php"), "index.php");
    assert_eq!(remote_parent("/var/www/index.php"), "/var/www");
    assert_eq!(remote_parent("/index.php"), "/");
    assert_eq!(remote_parent("index.php"), "");
    let t = super::replace::temp_name(&"n".repeat(250));
    assert!(t.len() < 210 && t.starts_with(".nnn") && t.contains(".cc-upload-"));
}

#[cfg(unix)]
#[test]
fn secure_remove_never_follows_symlinks() {
    let tmp = tempfile::tempdir().unwrap();
    let outside = tmp.path().join("outside.txt");
    std::fs::write(&outside, b"keep me").unwrap();
    let dir = tmp.path().join("session");
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("file.txt"), b"secret").unwrap();
    std::fs::write(dir.join("sub/nested.txt"), b"secret 2").unwrap();
    std::fs::write(dir.join("big.bin"), vec![1u8; 4096]).unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("link")).unwrap();
    secure_remove_dir(&dir, 1024).unwrap();
    assert!(!dir.exists());
    assert_eq!(std::fs::read(&outside).unwrap(), b"keep me");
    // missing directory is fine
    secure_remove_dir(&dir, 1024).unwrap();
}
