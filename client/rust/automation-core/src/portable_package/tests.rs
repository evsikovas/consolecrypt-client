use super::*;
use std::sync::MutexGuard;
fn serial() -> MutexGuard<'static, ()> {
    super::test_serial()
}
fn local_slot() -> OwnedSemaphorePermit {
    Arc::new(Semaphore::new(1)).try_acquire_owned().unwrap()
}
fn entry(path: &str, raw: &[u8]) -> ProjectEntry {
    ProjectEntry::File {
        path: path.into(),
        mode: 0o600,
        bytes: raw.len() as u64,
        sha256: sha(raw),
    }
}
fn plain(entries: Vec<ProjectEntry>, bytes: &[u8]) -> Zeroizing<Vec<u8>> {
    let revision = crate::dependency_archive::revision(entries).unwrap();
    let manifest = Manifest {
        version: 1,
        policy: CapturePolicy::ExplicitSnapshotSelectionV1,
        source_revision_digest: "a".repeat(64),
        revision,
        playbook: None,
        exclusions: vec![],
    };
    let raw = serde_json::to_vec(&manifest).unwrap();
    let mut out = Zeroizing::new(Vec::new());
    out.extend_from_slice(&(raw.len() as u32).to_be_bytes());
    out.extend_from_slice(&raw);
    out.extend_from_slice(bytes);
    out
}
fn assert_invalid_before_write(raw: Zeroizing<Vec<u8>>) {
    let directory = tempfile::tempdir().unwrap();
    let destination = ProjectRoot::open(directory.path()).unwrap();
    let result = verify_plain(raw, local_slot())
        .and_then(|package| package.write_uncommitted(&destination, &CancellationToken::new()));
    assert!(result.is_err());
    assert!(std::fs::read_dir(directory.path())
        .unwrap()
        .next()
        .is_none());
}

#[test]
fn actual_encrypted_roundtrip_preserves_selected_bytes_and_excludes_native_and_unselected_material()
{
    let _serial = serial();
    let source = tempfile::tempdir().unwrap();
    let destination = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(source.path().join("templates/empty")).unwrap();
    std::fs::create_dir_all(source.path().join(".git")).unwrap();
    std::fs::write(
        source.path().join("playbook.yml"),
        b"---\r\n# reviewed source\r\n",
    )
    .unwrap();
    std::fs::write(
        source.path().join("templates/data.bin"),
        [0, 255, 13, 10, 0],
    )
    .unwrap();
    let native_id = uuid::Uuid::new_v4().to_string();
    let runner_id = uuid::Uuid::new_v4().to_string();
    std::fs::write(
        source.path().join(crate::VAULT_INPUT_FILE),
        format!("{{\"version\":1,\"ansible_vault\":{{\"credential_id\":\"{native_id}\"}}}}"),
    )
    .unwrap();
    std::fs::write(source.path().join(".consolecrypt-runner.json"), &runner_id).unwrap();
    std::fs::write(
        source.path().join(".env"),
        b"SYNTHETIC_KNOWN_SENSITIVE_FILE",
    )
    .unwrap();
    std::fs::write(
        source.path().join("hosts.ini"),
        b"SYNTHETIC_UNSELECTED_PASSWORD",
    )
    .unwrap();
    std::fs::write(
        source.path().join(".git/config"),
        b"SYNTHETIC_GIT_METADATA_NOT_IN_SNAPSHOT",
    )
    .unwrap();
    let lock = b"{\"version\":1,\"dependencies\":[]}";
    std::fs::write(source.path().join(LOCK), lock).unwrap();
    let root = ProjectRoot::open(source.path()).unwrap();
    let before = root.revision().unwrap();
    let selected = [
        "playbook.yml",
        "templates/data.bin",
        "templates/empty",
        LOCK,
    ]
    .map(str::to_owned);
    let pass = SecretString::from("synthetic separate package password");
    let sealed = seal_project(
        &root,
        &before,
        &selected,
        Some("playbook.yml".into()),
        &pass,
        &CancellationToken::new(),
    )
    .unwrap();
    let package = open_project(&sealed, &pass, &CancellationToken::new()).unwrap();
    let summary = package.summary();
    assert_eq!(summary.source_digest, before.digest);
    assert_ne!(summary.portable_digest, before.digest);
    assert_eq!(summary.files, 3);
    assert!(summary.exclusions >= 4);
    for marker in [
        native_id.as_bytes(),
        runner_id.as_bytes(),
        b"SYNTHETIC_KNOWN_SENSITIVE_FILE",
        b"SYNTHETIC_UNSELECTED_PASSWORD",
        b"synthetic separate package password",
    ] {
        assert!(!package.plaintext.windows(marker.len()).any(|w| w == marker));
    }
    let result = package
        .write_uncommitted(
            &ProjectRoot::open(destination.path()).unwrap(),
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(result.digest, summary.portable_digest);
    assert_eq!(
        std::fs::read(destination.path().join("playbook.yml")).unwrap(),
        b"---\r\n# reviewed source\r\n"
    );
    assert_eq!(
        std::fs::read(destination.path().join("templates/data.bin")).unwrap(),
        vec![0, 255, 13, 10, 0]
    );
    assert_eq!(std::fs::read(destination.path().join(LOCK)).unwrap(), lock);
    assert!(destination.path().join("templates/empty").is_dir());
    for path in [
        crate::VAULT_INPUT_FILE,
        ".consolecrypt-runner.json",
        ".env",
        "hosts.ini",
        ".git",
    ] {
        assert!(!destination.path().join(path).exists());
    }
    assert_eq!(root.revision().unwrap(), before);
    assert_eq!(
        std::fs::read(source.path().join(".git/config")).unwrap(),
        b"SYNTHETIC_GIT_METADATA_NOT_IN_SNAPSHOT"
    );
}

#[test]
fn actual_wrong_passphrase_tamper_and_version_leave_destination_empty() {
    let _serial = serial();
    let source = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join("playbook.yml"), b"---\n").unwrap();
    let root = ProjectRoot::open(source.path()).unwrap();
    let before = root.revision().unwrap();
    let pass = SecretString::from("owned package authentication proof");
    let encrypted = seal_project(
        &root,
        &before,
        &["playbook.yml".into()],
        None,
        &pass,
        &CancellationToken::new(),
    )
    .unwrap();
    let mut tampered = encrypted.clone();
    *tampered.last_mut().unwrap() ^= 1;
    let mut version = encrypted.clone();
    version[0] ^= 1;
    let wrong = SecretString::from("different owned package passphrase");
    for (raw, password) in [(&encrypted, &wrong), (&tampered, &pass), (&version, &pass)] {
        let destination = tempfile::tempdir().unwrap();
        let empty = ProjectRoot::open(destination.path()).unwrap();
        assert!(open_project(raw, password, &CancellationToken::new())
            .and_then(|p| p.write_uncommitted(&empty, &CancellationToken::new()))
            .is_err());
        assert!(std::fs::read_dir(destination.path())
            .unwrap()
            .next()
            .is_none());
    }
    assert_eq!(root.revision().unwrap(), before);
}

#[test]
fn known_sensitive_control_files_cannot_be_selected_even_explicitly() {
    let _serial = serial();
    let source = tempfile::tempdir().unwrap();
    for name in [
        crate::VAULT_INPUT_FILE,
        ".consolecrypt-dependency-receipts.json",
        ".env",
        "id_ed25519",
        "backup.kdbx",
    ] {
        std::fs::write(source.path().join(name), b"synthetic private marker").unwrap();
    }
    let root = ProjectRoot::open(source.path()).unwrap();
    let before = root.revision().unwrap();
    for entry in &before.entries {
        assert!(seal_project(
            &root,
            &before,
            &[entry.path().into()],
            None,
            &SecretString::from(""),
            &CancellationToken::new()
        )
        .is_err());
    }
    assert_eq!(root.revision().unwrap(), before);
}

#[test]
fn traversal_duplicate_case_parent_file_and_control_paths_reject_before_any_import() {
    let _serial = serial();
    for path in [
        "../escape",
        "/absolute",
        "C:/outside",
        "folder\\outside",
        "con.txt",
        crate::VAULT_INPUT_FILE,
    ] {
        assert_invalid_before_write(plain(vec![entry(path, b"x")], b"x"));
    }
    assert_invalid_before_write(plain(vec![entry("same", b"x"), entry("same", b"x")], b"xx"));
    assert_invalid_before_write(plain(vec![entry("A", b"x"), entry("a", b"x")], b"xx"));
    assert_invalid_before_write(plain(
        vec![entry("file", b"x"), entry("file/child", b"x")],
        b"xx",
    ));
    assert_invalid_before_write(plain(vec![entry("missing/child", b"x")], b"x"));
}

#[test]
fn strict_json_hash_trailing_bytes_and_quotas_reject_before_any_import() {
    let _serial = serial();
    let mut raw = plain(vec![entry("file", b"x")], b"x");
    *raw.last_mut().unwrap() = b'y';
    assert_invalid_before_write(raw);
    let mut raw = plain(vec![entry("file", b"x")], b"x");
    raw.push(0);
    assert_invalid_before_write(raw);
    let mut raw = plain(vec![entry("file", b"x")], b"x");
    let size = u32::from_be_bytes(raw[..4].try_into().unwrap()) as usize;
    let manifest = std::str::from_utf8(&raw[4..4 + size]).unwrap();
    let duplicate = format!("{{\"version\":1,{}", &manifest[1..]);
    let mut changed = Zeroizing::new(Vec::new());
    changed.extend_from_slice(&(duplicate.len() as u32).to_be_bytes());
    changed.extend_from_slice(duplicate.as_bytes());
    changed.push(b'x');
    assert_invalid_before_write(changed);
    raw[..4].copy_from_slice(&((MAX_MANIFEST + 1) as u32).to_be_bytes());
    assert_invalid_before_write(raw);
    let mut large = entry("large", b"");
    if let ProjectEntry::File { bytes, .. } = &mut large {
        *bytes = MAX_FILE + 1;
    }
    assert_invalid_before_write(plain(vec![large], b""));
    let files = (0..=MAX_FILES)
        .map(|i| entry(&format!("f{i:04}"), b""))
        .collect();
    assert_invalid_before_write(plain(files, b""));
    let dirs = (0..=MAX_ENTRIES)
        .map(|i| ProjectEntry::Directory {
            path: format!("d{i:04}"),
        })
        .collect();
    assert_invalid_before_write(plain(dirs, b""));
    let mut entries = Vec::new();
    for i in 0..17 {
        let mut file = entry(&format!("f{i:04}"), b"");
        if let ProjectEntry::File { bytes, .. } = &mut file {
            *bytes = MAX_FILE;
        }
        entries.push(file);
    }
    assert_invalid_before_write(plain(entries, b""));
}

#[test]
fn existing_lock_parser_preserves_exact_supported_bytes_and_rejects_changed_subtree_or_future_schema(
) {
    let _serial = serial();
    let task = b"---\n# pinned role data, never executed\n";
    let subtree = crate::dependency_archive::revision(vec![
        ProjectEntry::Directory {
            path: "tasks".into(),
        },
        entry("tasks/main.yml", task),
    ])
    .unwrap();
    let lock=serde_json::to_vec(&serde_json::json!({"version":1,"dependencies":[{"kind":"role","name":"demo","version":"1.0.0","source":{"url":"https://fixture.invalid/demo.tar.gz","sha256":"a".repeat(64)},"tree_digest":subtree.digest}]})).unwrap();
    let entries = vec![
        entry(LOCK, &lock),
        ProjectEntry::Directory {
            path: "roles".into(),
        },
        ProjectEntry::Directory {
            path: "roles/demo".into(),
        },
        ProjectEntry::Directory {
            path: "roles/demo/tasks".into(),
        },
        entry("roles/demo/tasks/main.yml", task),
    ];
    let mut body = lock.clone();
    body.extend_from_slice(task);
    let package = verify_plain(plain(entries.clone(), &body), local_slot()).unwrap();
    let destination = tempfile::tempdir().unwrap();
    package
        .write_uncommitted(
            &ProjectRoot::open(destination.path()).unwrap(),
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(std::fs::read(destination.path().join(LOCK)).unwrap(), lock);
    let mut broken = entries;
    *broken.last_mut().unwrap() = entry("roles/demo/tasks/main.yml", b"changed task");
    let mut body = lock;
    body.extend_from_slice(b"changed task");
    assert_invalid_before_write(plain(broken, &body));
    let future = b"{\"version\":2,\"dependencies\":[]}";
    assert_invalid_before_write(plain(vec![entry(LOCK, future)], future));
}

#[test]
fn existing_destination_including_snapshot_excluded_git_is_not_overwritten() {
    let _serial = serial();
    for name in ["keep", ".git"] {
        let destination = tempfile::tempdir().unwrap();
        std::fs::write(destination.path().join(name), b"owned existing bytes").unwrap();
        let package = verify_plain(plain(vec![entry("new", b"x")], b"x"), local_slot()).unwrap();
        assert!(package
            .write_uncommitted(
                &ProjectRoot::open(destination.path()).unwrap(),
                &CancellationToken::new()
            )
            .is_err());
        assert_eq!(
            std::fs::read(destination.path().join(name)).unwrap(),
            b"owned existing bytes"
        );
        assert!(!destination.path().join("new").exists());
    }
}

#[test]
fn process_slots_bound_retained_plaintext_and_detached_worker_admission() {
    let _serial = serial();
    let first = permit().unwrap();
    let second = permit().unwrap();
    assert!(matches!(permit(), Err(AutomationError::QueueFull)));
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _first = first;
        release_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
    });
    assert!(matches!(permit(), Err(AutomationError::QueueFull)));
    release_tx.send(()).unwrap();
    worker.join().unwrap();
    drop(second);
    assert!(permit().is_ok());
}

#[test]
fn operation_adapter_choices_and_no_replace_cipher_roundtrip() {
    let _serial = serial();
    let source = tempfile::tempdir().unwrap();
    std::fs::write(
        source.path().join("play.yml"),
        b"---\n- hosts: all\n  tasks: []\n",
    )
    .unwrap();
    std::fs::write(source.path().join(".env"), b"synthetic excluded secret").unwrap();
    std::fs::create_dir(source.path().join("empty")).unwrap();
    let root = ProjectRoot::open(source.path()).unwrap();
    let revision = root.revision().unwrap();
    let choices = selection_choices(&revision).unwrap();
    assert!(choices.iter().any(|c| c.path == ".env" && !c.selectable));
    assert!(
        choices
            .iter()
            .any(|c| c.path == "empty" && c.selectable && c.directory)
    );
    let password = SecretString::from("explicit separate portable password");
    let token = CancellationToken::new();
    let encrypted = encrypt_selection(
        &root,
        &revision,
        &["play.yml".into(), "empty".into()],
        Some("play.yml".into()),
        &password,
        &token,
    )
    .unwrap();
    let out = tempfile::tempdir().unwrap();
    let parent = ProjectRoot::open(out.path()).unwrap();
    write_encrypted_new(&parent, "owned.ccproject", &encrypted, &token).unwrap();
    let old = std::fs::read(out.path().join("owned.ccproject")).unwrap();
    assert_eq!(old, encrypted);
    assert!(write_encrypted_new(&parent, "owned.ccproject", b"replacement", &token).is_err());
    assert_eq!(
        std::fs::read(out.path().join("owned.ccproject")).unwrap(),
        old
    );
    let read = read_encrypted_file(&parent, "owned.ccproject").unwrap();
    let verified = decrypt_package(&read, &password, &token).unwrap();
    assert_eq!(verified.summary().files, 1);
    assert_eq!(verified.summary().exclusions, 1);
    let target = tempfile::tempdir().unwrap();
    let target_root = ProjectRoot::open(target.path()).unwrap();
    verified.write_uncommitted(&target_root, &token).unwrap();
    assert!(target.path().join("empty").is_dir());
    assert_eq!(
        std::fs::read(target.path().join("play.yml")).unwrap(),
        std::fs::read(source.path().join("play.yml")).unwrap()
    );
    assert!(!target.path().join(".env").exists());
    assert_eq!(root.revision().unwrap(), revision);
}
#[test]
fn operation_adapter_cancel_and_invalid_leaf_never_create_or_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let parent = ProjectRoot::open(dir.path()).unwrap();
    let token = CancellationToken::new();
    token.cancel();
    assert!(write_encrypted_new(&parent, "cancelled.ccproject", b"encrypted", &token).is_err());
    assert!(!dir.path().join("cancelled.ccproject").exists());
    for leaf in [
        "../escape.ccproject",
        "nested/file.ccproject",
        "CON.ccproject",
        "file.yml",
        ".ccproject",
    ] {
        assert!(
            write_encrypted_new(&parent, leaf, b"encrypted", &CancellationToken::new()).is_err(),
            "{leaf}"
        );
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn operation_write_original_native_fence_denies_before_create_and_before_first_byte() {
    let dir = tempfile::tempdir().unwrap();
    let parent = ProjectRoot::open(dir.path()).unwrap();
    let token = CancellationToken::new();
    assert!(
        write_encrypted_new_checked(&parent, "blocked.ccproject", b"ciphertext", &token, || Err(
            AutomationError::Closed
        ))
        .is_err()
    );
    assert!(!dir.path().join("blocked.ccproject").exists());
    let mut calls = 0;
    assert!(
        write_encrypted_new_checked(
            &parent,
            "new-empty.ccproject",
            b"ciphertext",
            &token,
            || {
                calls += 1;
                if calls == 2 {
                    Err(AutomationError::Closed)
                } else {
                    Ok(())
                }
            }
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read(dir.path().join("new-empty.ccproject")).unwrap(),
        b""
    );
    assert_eq!(calls, 2);
    // The partial NEW file is an explicit failed output, never auto-deleted or
    // reused. Existing user bytes remain untouched even on repeated cancellation.
    std::fs::write(dir.path().join("existing.ccproject"), b"owned original").unwrap();
    assert!(
        write_encrypted_new_checked(
            &parent,
            "existing.ccproject",
            b"replacement",
            &token,
            || Ok(())
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read(dir.path().join("existing.ccproject")).unwrap(),
        b"owned original"
    );
}
