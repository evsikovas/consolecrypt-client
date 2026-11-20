use super::*;

#[tokio::test]
#[ignore = "requires explicitly owned offline Docker fixture; see run_live_dependency_preview.py"]
async fn live_dependency_only_cli_tls_worker_and_native_prepare_preview() {
    use tokio::process::Command;
    let _serial = crate::dependency_transfer::DEPENDENCY_TEST_SERIAL
        .lock()
        .await;
    let token = std::env::var("CC_DEPENDENCY_LIVE_TOKEN").expect("owned live fixture required");
    let operation = uuid::Uuid::parse_str(&token).unwrap();
    assert_eq!(operation.to_string(), token);
    assert!(!operation.is_nil());
    let container = std::env::var("CC_DEPENDENCY_LIVE_CONTAINER").unwrap();
    assert_eq!(container, format!("cc-dependency-live-{token}"));
    let inspect = std::process::Command::new("docker.exe")
        .args(["inspect", &container])
        .env("DOCKER_HOST", "npipe:////./pipe/dockerDesktopLinuxEngine")
        .env("DOCKER_CONTEXT", "")
        .output()
        .unwrap();
    assert!(inspect.status.success());
    let inspect: Value = serde_json::from_slice(&inspect.stdout).unwrap();
    assert_eq!(
        inspect[0]["Config"]["Labels"]["consolecrypt.test"],
        "ans05-native-live-helper"
    );
    assert_eq!(
        inspect[0]["Config"]["Labels"]["consolecrypt.fixture"],
        token
    );
    assert_eq!(inspect[0]["HostConfig"]["NetworkMode"], "none");
    let mut child = Command::new("docker.exe")
        .args([
            "exec",
            "-i",
            &container,
            "/opt/cc-runtime/bin/python",
            "-I",
            "-B",
            "/fixture/live_dependency_helper.py",
            "--operation-id",
            &token,
        ])
        .env("DOCKER_HOST", "npipe:////./pipe/dockerDesktopLinuxEngine")
        .env("DOCKER_CONTEXT", "")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let line = tokio::time::timeout(Duration::from_secs(15), crate::read_frame(&mut reader))
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(line.len() <= 4096);
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Hello {
        version: u32,
        operation_id: String,
        configured_root: String,
        url: String,
        sha256: String,
        archive_bytes: usize,
    }
    let hello: Hello = serde_json::from_value(crate::json::parse(&line).unwrap()).unwrap();
    assert_eq!(hello.version, 1);
    assert_eq!(hello.operation_id, token);
    let configured = format!("/tmp/cc-live-{token}/jobs");
    assert_eq!(hello.configured_root, configured);
    let parsed = url::Url::parse(&hello.url).unwrap();
    let port = parsed.port().unwrap();
    assert!(port > 0);
    assert_eq!(
        hello.url,
        format!("https://127.0.0.1:{port}/artifact.tar.gz")
    );
    let fixture = fixture();
    assert_eq!(hello.sha256, fixture["selection"]["source"]["sha256"]);
    let fixture_bytes = fixture["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|chunk| {
            STANDARD
                .decode(chunk["response"]["data"].as_str().unwrap())
                .unwrap()
                .len()
        })
        .sum::<usize>();
    assert_eq!(hello.archive_bytes, fixture_bytes);
    let selected = SourceSelection::new(
        crate::DependencyKind::Role,
        "demo".into(),
        "1.2.3".into(),
        crate::DependencyArtifactSource {
            url: hello.url,
            sha256: hello.sha256,
        },
        0,
    )
    .unwrap();
    let peer = HelperPeer::connect(reader, child.stdin.take().unwrap());
    let owner = DependencyHelperConnection::for_protocol_fixture(peer, operation, &configured)
        .await
        .unwrap();
    let own = tempfile::tempdir().unwrap();
    let base_path = own.path().join("base");
    let candidate_path = own.path().join("candidate");
    fs::create_dir(&base_path).unwrap();
    fs::create_dir(&candidate_path).unwrap();
    fs::write(base_path.join("play.yml"), b"---\n").unwrap();
    let base = ProjectRoot::open(&base_path).unwrap();
    let candidate = ProjectRoot::open(&candidate_path).unwrap();
    let before = base.revision().unwrap();
    let context = crate::dependency_preflight(&base, &before, &selected).unwrap();
    assert_eq!(context.lock_sha256(), None);
    let preview = owner
        .prepare_preview(
            &base,
            &before,
            &selected,
            &candidate,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(
        preview.archive().archive().tree().digest,
        fixture["tree_digest"]
    );
    assert_eq!(preview.candidate(), &candidate.revision().unwrap());
    assert_eq!(base.revision().unwrap(), before);
    assert_eq!(fs::read(base_path.join("play.yml")).unwrap(), b"---\n");
    assert_eq!(
        fs::read(candidate_path.join("roles/demo/files/payload.bin"))
            .unwrap()
            .len(),
        288000
    );
    let lock: Value = crate::json::parse(&fs::read(candidate_path.join(LOCK)).unwrap()).unwrap();
    assert_eq!(
        lock["dependencies"][0]["source"],
        serde_json::to_value(selected.source()).unwrap()
    );
    assert!(preview
        .diff()
        .changes
        .iter()
        .any(|change| change.path == "roles/demo/lookup_plugins/evil.py"));
    assert!(cc_models::automation::is_digest(preview.review_digest()));
    owner.close().await;
    assert!(tokio::time::timeout(Duration::from_secs(15), child.wait())
        .await
        .unwrap()
        .unwrap()
        .success());
    println!("actual dependency-only CLI + trusted TLS worker + Native archive/candidate verification PASS; no commit/execution");
}
