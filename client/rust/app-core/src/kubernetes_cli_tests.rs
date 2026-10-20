use super::*;
fn archive(name: &str, kind: tar::EntryType, bytes: &[u8]) -> Vec<u8> {
    let gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut builder = tar::Builder::new(gzip);
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o755);
    header.set_entry_type(kind);
    header.set_cksum();
    builder.append_data(&mut header, name, bytes).unwrap();
    builder.into_inner().unwrap().finish().unwrap()
}
#[test]
fn archive_extraction_rejects_links_paths_duplicates_and_wrong_checksum() {
    let bytes = archive("flux", tar::EntryType::Regular, b"fixture binary");
    assert_eq!(
        extract_binary(&bytes, "flux", false).unwrap(),
        b"fixture binary"
    );
    assert!(extract_binary(
        &archive("subdir/flux", tar::EntryType::Regular, b"x"),
        "flux",
        false
    )
    .is_err());
    assert!(extract_binary(
        &archive("flux", tar::EntryType::Symlink, b""),
        "flux",
        false
    )
    .is_err());
    assert!(extract_binary(
        &archive("other", tar::EntryType::Regular, b"x"),
        "flux",
        false
    )
    .is_err());
    let hash = sha256(&bytes);
    assert_eq!(
        checksum(
            format!("{hash}  fixture.tar.gz\n").as_bytes(),
            "fixture.tar.gz"
        )
        .unwrap(),
        hash
    );
    assert!(checksum(
        format!("{hash} fixture.tar.gz\n{hash} fixture.tar.gz\n").as_bytes(),
        "fixture.tar.gz"
    )
    .is_err());
    assert!(checksum(b"bad fixture.tar.gz", "fixture.tar.gz").is_err());
    assert!(checksum(
        format!("{hash} other.tar.gz\n").as_bytes(),
        "fixture.tar.gz"
    )
    .is_err());
}
#[test]
fn zip_extracts_exact_executable_and_rejects_unsafe_members() {
    use zip::write::SimpleFileOptions;
    fn zipped(name: &str) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        writer
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"fixture").unwrap();
        writer.finish().unwrap().into_inner()
    }
    assert_eq!(
        extract_binary(&zipped("flux.exe"), "flux.exe", true).unwrap(),
        b"fixture"
    );
    for name in ["../flux.exe", "nested/flux.exe", "nested\\flux.exe"] {
        assert!(extract_binary(&zipped(name), "flux.exe", true).is_err());
    }
}
#[test]
fn signature_policy_binds_exact_workflow_version_and_issuer() {
    let args = cosign_arguments(
        "2.9.6",
        Path::new("/tmp/checksums"),
        Path::new("/tmp/sig"),
        Path::new("/tmp/cert"),
    );
    assert!(args.contains(
        &"https://github.com/fluxcd/flux2/.github/workflows/release.yaml@refs/tags/v2.9.6".into()
    ));
    assert!(args.contains(&"https://token.actions.githubusercontent.com".into()));
    assert!(!args
        .iter()
        .any(|arg| arg.contains("insecure") || arg.contains("ignore") || arg.contains("regexp")));
    for version in [
        "latest",
        "2.9.6/../../evil",
        "2.9.6-rc.1",
        "v2.9.6+metadata",
        "3.0.0",
    ] {
        assert!(version_number(version).is_err());
    }
    assert!(certificate_pem(b"untrusted invalid data".to_vec()).is_err());
}
#[test]
fn typed_actions_do_not_accept_injected_flags_or_arbitrary_commands() {
    let args = FluxAction::Trace {
        kind: "Deployment".into(),
        name: "nginx".into(),
        namespace: "default".into(),
    }
    .arguments()
    .unwrap();
    assert_eq!(&args[..3], &["trace", "Deployment", "nginx"]);
    assert!(FluxAction::Tree {
        name: "--kubeconfig=evil".into(),
        namespace: "default".into()
    }
    .arguments()
    .is_err());
    assert!(FluxAction::Events {
        namespace: "default\n--token=secret".into()
    }
    .arguments()
    .is_err());
    assert!(serde_json::from_str::<FluxAction>(r#"{"action":"bootstrap"}"#).is_err());
}
#[cfg(unix)]
fn fixture(dir: &Path, name: &str, body: &str) -> PathBuf {
    let file = dir.join(name);
    std::fs::write(&file, format!("#!/bin/sh\n{body}\n")).unwrap();
    make_executable(&file).unwrap();
    file
}
#[cfg(unix)]
#[tokio::test]
async fn discovery_does_not_execute_and_probe_has_clean_bounded_environment() {
    let temp = tempfile::tempdir().unwrap();
    let marker = temp.path().join("ran");
    let binary=fixture(temp.path(),"flux",&format!("touch '{}'\nif [ \"$KUBECONFIG\" != /dev/null ]; then exit 2; fi\nprintf '{{\"flux\":\"v2.9.6\"}}'",marker.display()));
    let runtime = FluxCliRuntime::default();
    let found = runtime.discover(Some(&binary)).unwrap();
    assert!(found
        .candidates
        .iter()
        .any(|candidate| candidate.path == binary.canonicalize().unwrap()));
    assert!(!marker.exists());
    let result = runtime
        .probe(&binary, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.version, "2.9.6");
    assert!(marker.exists());
    assert_eq!(result.sha256.len(), 64);
    runtime.close();
    assert!(matches!(
        runtime.probe(&binary, CancellationToken::new()).await,
        Err(CliError::Cancelled)
    ));
}
#[cfg(unix)]
#[tokio::test]
async fn reviewed_binary_changes_and_cancelled_process_trees_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let path = fixture(temp.path(), "fixture", "echo ok");
    let binary = inspect(&path).unwrap();
    std::fs::write(&path, "#!/bin/sh\necho changed\n").unwrap();
    assert!(matches!(
        run(
            &binary,
            &[],
            None,
            Duration::from_secs(1),
            CancellationToken::new()
        )
        .await,
        Err(CliError::Changed)
    ));
    let marker = temp.path().join("child-survived");
    let path = fixture(
        temp.path(),
        "waiter",
        &format!("(sleep 1; touch '{}') &\nwait", marker.display()),
    );
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        trigger.cancel();
    });
    assert!(matches!(
        run(
            &inspect(&path).unwrap(),
            &[],
            None,
            Duration::from_secs(5),
            cancel
        )
        .await,
        Err(CliError::Cancelled)
    ));
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(!marker.exists());
}
#[cfg(unix)]
#[tokio::test]
async fn output_is_bounded_and_nonzero_output_is_protected() {
    let temp = tempfile::tempdir().unwrap();
    let path = fixture(temp.path(), "loud", "head -c 4194305 /dev/zero");
    assert!(matches!(
        run(
            &inspect(&path).unwrap(),
            &[],
            None,
            Duration::from_secs(5),
            CancellationToken::new()
        )
        .await,
        Err(CliError::TooLarge)
    ));
    let path = fixture(temp.path(), "error", "printf synthetic-secret >&2\nexit 7");
    let output = run(
        &inspect(&path).unwrap(),
        &[],
        None,
        Duration::from_secs(1),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(output.exit_code, 7);
    assert!(!format!("{output:?}").contains("synthetic-secret"));
}
#[cfg(unix)]
#[test]
fn install_tickets_are_one_use_and_destination_change_does_not_clobber() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), "source", "echo new");
    let destination = fixture(temp.path(), "destination", "echo old");
    let binary = inspect(&source).unwrap();
    let runtime = FluxCliRuntime::default();
    let ticket = "fixture-ticket";
    let review = InstallReview {
        tool: "flux".into(),
        ticket: ticket.into(),
        method: InstallMethod::Homebrew,
        operation: InstallOperation::Install,
        version: "2.9.6".into(),
        executable: Some(source.clone()),
        arguments: vec!["install".into()],
        source: "fixture".into(),
        destination: destination.clone(),
        verifier: None,
        executable_sha256: Some(binary.hash.clone()),
    };
    runtime.plans.lock().unwrap().insert(
        ticket.into(),
        Plan {
            created: Instant::now(),
            review,
            binary,
            previous: None,
        },
    );
    assert!(runtime.approve_package_install(ticket).is_ok());
    assert!(matches!(
        runtime.approve_package_install(ticket),
        Err(CliError::Cancelled)
    ));
    assert!(matches!(
        publish(
            &source,
            &destination,
            Some("wrong-hash"),
            &CancellationToken::new()
        ),
        Err(CliError::Changed)
    ));
    assert!(std::fs::read_to_string(&destination)
        .unwrap()
        .contains("old"));
    let previous = inspect(&destination).unwrap().hash;
    publish(
        &source,
        &destination,
        Some(&previous),
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(std::fs::read_to_string(&destination)
        .unwrap()
        .contains("new"));
}
