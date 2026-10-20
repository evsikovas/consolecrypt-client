use super::*;
use std::os::unix::fs::PermissionsExt;

fn script(directory: &tempfile::TempDir, body: &str) -> PathBuf {
    let path = directory.path().join("credential-plugin");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

fn request(path: &Path, context_id: Uuid) -> ExecRequest {
    ExecRequest {
        context_id,
        binary: path.into(),
        args: vec![],
        environment: BTreeMap::new(),
        api_version: "client.authentication.k8s.io/v1".into(),
        interactive_mode: ExecInteractiveMode::Never,
        cluster_info: None,
    }
}

fn output() -> &'static str {
    r#"printf '%s' '{"apiVersion":"client.authentication.k8s.io/v1","kind":"ExecCredential","status":{"token":"synthetic-native-token"}}'"#
}

#[tokio::test]
async fn executable_never_runs_before_approval_and_environment_is_exact() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("executed");
    let path = script(
        &directory,
        &format!(
            "test -z \"$HOME\" || exit 10\ntest \"$EXPLICIT_VALUE\" = 'synthetic-env' || exit 11\nprintf marker > '{}'\n{}",
            marker.display(),
            output()
        ),
    );
    let engine = ExecEngine::default();
    let context = Uuid::new_v4();
    let mut value = request(&path, context);
    value
        .environment
        .insert("EXPLICIT_VALUE".into(), "synthetic-env".into());
    let prepared = engine.prepare(value).await.unwrap();
    assert!(!marker.exists());
    assert!(!engine.is_approved(&prepared));
    assert_eq!(
        engine
            .execute(prepared, CancellationToken::new())
            .await
            .unwrap_err(),
        AuthError::ApprovalRequired
    );
    assert!(!marker.exists());
    let mut value = request(&path, context);
    value
        .environment
        .insert("EXPLICIT_VALUE".into(), "synthetic-env".into());
    let prepared = engine.prepare(value).await.unwrap();
    engine.approve(&prepared).unwrap();
    let credential = engine
        .execute(prepared, CancellationToken::new())
        .await
        .unwrap();
    assert!(marker.exists());
    let Authentication::Bearer(token) = credential.authentication else {
        panic!("expected a native bearer token")
    };
    assert_eq!(token.expose_secret(), "synthetic-native-token");
    assert!(credential.expires_at > SystemTime::now());
    assert!(!format!("{engine:?}").contains("synthetic"));
}

#[tokio::test]
async fn approval_is_bound_to_binary_arguments_environment_cluster_and_context() {
    let directory = tempfile::tempdir().unwrap();
    let path = script(&directory, output());
    let engine = ExecEngine::default();
    let context = Uuid::new_v4();
    let baseline = engine.prepare(request(&path, context)).await.unwrap();
    engine.approve(&baseline).unwrap();
    assert!(engine.is_approved(&baseline));
    for alteration in 0..4 {
        let mut value = request(&path, context);
        match alteration {
            0 => value.args.push("different".into()),
            1 => {
                value.environment.insert("VALUE".into(), "different".into());
            }
            2 => value.cluster_info = Some(r#"{"server":"https://another.example.test"}"#.into()),
            _ => value.context_id = Uuid::new_v4(),
        }
        let changed = engine.prepare(value).await.unwrap();
        assert!(!engine.is_approved(&changed));
        assert_eq!(
            engine
                .execute(changed, CancellationToken::new())
                .await
                .unwrap_err(),
            AuthError::ApprovalRequired
        );
    }
    script(&directory, &format!("# changed executable\n{}", output()));
    assert_eq!(
        engine
            .execute(baseline, CancellationToken::new())
            .await
            .unwrap_err(),
        AuthError::CommandChanged
    );
    let changed = engine.prepare(request(&path, context)).await.unwrap();
    assert!(!engine.is_approved(&changed));
}

#[tokio::test]
async fn cancelled_or_locked_command_cannot_finish_or_leave_children_running() {
    let directory = tempfile::tempdir().unwrap();
    let started = directory.path().join("started");
    let released = directory.path().join("released");
    let completed = directory.path().join("completed");
    let path = script(
        &directory,
        &format!(
            "(printf ready > '{}'; while ! test -f '{}'; do /bin/sleep 0.01; done; printf wrong > '{}') &\n/bin/sleep 20\n{}",
            started.display(),
            released.display(),
            completed.display(),
            output()
        ),
    );
    let engine = std::sync::Arc::new(ExecEngine::default());
    let prepared = engine
        .prepare(request(&path, Uuid::new_v4()))
        .await
        .unwrap();
    engine.approve(&prepared).unwrap();
    let worker = {
        let engine = engine.clone();
        tokio::spawn(async move { engine.execute(prepared, CancellationToken::new()).await })
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        while !started.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    engine.close();
    assert_eq!(worker.await.unwrap().unwrap_err(), AuthError::Closed);
    // Release the descendant only after close completes. A time-based child
    // can legitimately wake before the parent test is scheduled on busy CI.
    std::fs::write(released, b"release").unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!completed.exists());
    assert_eq!(
        engine
            .prepare(request(&path, Uuid::new_v4()))
            .await
            .unwrap_err(),
        AuthError::Closed
    );
}

#[tokio::test]
async fn responses_are_bounded_and_stderr_never_appears_in_errors() {
    let directory = tempfile::tempdir().unwrap();
    let path = script(&directory, "printf 'synthetic-stderr-secret' >&2; exit 1");
    let engine = ExecEngine::default();
    let prepared = engine
        .prepare(request(&path, Uuid::new_v4()))
        .await
        .unwrap();
    engine.approve(&prepared).unwrap();
    let error = engine
        .execute(prepared, CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error, AuthError::Rejected);
    assert!(!format!("{error:?} {error}").contains("synthetic"));
    let path = script(&directory, "/usr/bin/yes synthetic-oversized-output");
    let prepared = engine
        .prepare(request(&path, Uuid::new_v4()))
        .await
        .unwrap();
    engine.approve(&prepared).unwrap();
    assert_eq!(
        engine
            .execute(prepared, CancellationToken::new())
            .await
            .unwrap_err(),
        AuthError::ResponseTooLarge
    );
}

#[tokio::test]
async fn interactive_and_reserved_environment_are_rejected_without_launch() {
    let directory = tempfile::tempdir().unwrap();
    let path = script(&directory, output());
    let engine = ExecEngine::default();
    let mut value = request(&path, Uuid::new_v4());
    value.interactive_mode = ExecInteractiveMode::Always;
    assert_eq!(
        engine.prepare(value).await.unwrap_err(),
        AuthError::InteractiveRequired
    );
    let mut value = request(&path, Uuid::new_v4());
    value
        .environment
        .insert("KUBERNETES_EXEC_INFO".into(), "override".into());
    assert_eq!(
        engine.prepare(value).await.unwrap_err(),
        AuthError::InvalidConfiguration
    );
    assert!(resolve_binary("./plugin", "/tmp").is_err());
    assert!(resolve_binary("missing", ".:").is_err());
}

#[test]
fn invalid_credential_shapes_and_expired_credentials_are_rejected() {
    for status in [
        r#"{}"#,
        r#"{"token":"value","clientCertificateData":"cert","clientKeyData":"key"}"#,
        r#"{"token":"value\nheader"}"#,
        r#"{"token":"value","expirationTimestamp":"2000-01-01T00:00:00Z"}"#,
        r#"{"clientCertificateData":"cert"}"#,
    ] {
        let body = format!(
            r#"{{"apiVersion":"client.authentication.k8s.io/v1","kind":"ExecCredential","status":{status}}}"#
        );
        assert!(parse_credential(body.as_bytes(), "client.authentication.k8s.io/v1").is_err());
    }
}
