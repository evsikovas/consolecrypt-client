//! Compose files of a project through app-core: read, passphrase grant,
//! validate-then-write with a backup, and `up -d` as a polled job.
//!
//! * this computer's engine — `CC_DOCKER_LIVE=1` (needs Docker with the
//!   compose plugin and `alpine:3.20`);
//! * a saved SSH host (SFTP + exec) — `CC_SSH_IT=1` (the SSH Docker testbed;
//!   installs the Docker CLI inside it, so it needs network access).
//!
//! ```sh
//! CC_DOCKER_LIVE=1 cargo test -p cc-app-core --test docker_compose_live -- --nocapture
//! CC_SSH_IT=1 cargo test -p cc-app-core --test docker_compose_live -- --nocapture
//! ```

mod common;

use cc_app_core::*;
use cc_models::host::HostKeyPolicy;
use cc_ssh_core::testing::{self, SshTestbed};
use common::*;
use secrecy::ExposeSecret;
use std::process::Command;

const SERVICE_V1: &str = "services:\n  app:\n    image: alpine:3.20\n    command: [\"sleep\", \"600\"]\n    environment:\n      DB_PASSWORD: hunter2\n    labels:\n      cc.version: \"1\"\n";
const SERVICE_V1_MASKED: &str = "services:\n  app:\n    image: alpine:3.20\n    command: [\"sleep\", \"600\"]\n    environment:\n      DB_PASSWORD: ••••••\n    labels:\n      cc.version: \"1\"\n";
const SERVICE_V2: &str = "services:\n  app:\n    image: alpine:3.20\n    command: [\"sleep\", \"600\"]\n    environment:\n      DB_PASSWORD: hunter2\n    labels:\n      cc.version: \"2\"\n";
const SERVICE_V3: &str = "services:\n  app:\n    image: alpine:3.20\n    command: [\"sleep\", \"600\"]\n    labels:\n      cc.version: \"3\"\n";
const BROKEN: &str = "services:\n  app:\n    image: [alpine\n";

fn edits(path: &str, content: &str, digest: &str) -> Vec<ComposeEdit> {
    serde_json::from_value(serde_json::json!([
        {"path": path, "content": content, "digest": digest}
    ]))
    .unwrap()
}

fn host_docker(args: &[&str]) -> String {
    let output = Command::new("docker").args(args).output().unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn remove_project(project: &str) {
    let ids = host_docker(&[
        "ps",
        "-aq",
        "--filter",
        &format!("label=com.docker.compose.project={project}"),
    ]);
    for id in ids.lines().filter(|id| !id.is_empty()) {
        let _ = Command::new("docker").args(["rm", "-f", id]).output();
    }
    let _ = Command::new("docker")
        .args(["network", "rm", &format!("{project}_default")])
        .output();
}

async fn wait_job(app: &AppCore, job: String) -> (Vec<String>, Option<i32>) {
    let mut lines = Vec::new();
    for _ in 0..300 {
        let update = app
            .docker_compose_job_poll(job.clone(), 1000)
            .await
            .unwrap();
        lines.extend(update.lines);
        if update.done {
            return (lines, update.exit_code);
        }
    }
    panic!("up -d did not finish: {lines:?}");
}

/// The common flow once the project runs: view, lock, invalid edit,
/// conflict, save and apply.
async fn exercise(
    app: &AppCore,
    session: &str,
    project: &str,
    path_suffix: &str,
    read_on_host: &dyn Fn(&str) -> String,
) -> String {
    let files = app
        .docker_compose_project(session.into(), project.into())
        .await
        .unwrap();
    let file = files
        .files
        .iter()
        .find(|f| f.path.ends_with(path_suffix))
        .unwrap_or_else(|| panic!("{files:?}"))
        .clone();
    // Shown without the passphrase: the password is masked, lines kept.
    assert_eq!(file.content.as_deref(), Some(SERVICE_V1_MASKED));
    assert!(file.masked);
    let digest = file.digest.clone().unwrap();

    let wrong = app
        .docker_compose_unlock(session.into(), project.into(), "wrong passphrase".into())
        .await;
    assert!(matches!(wrong, Err(AppError::WrongPassphrase)), "{wrong:?}");
    // Without a grant nothing is written.
    let locked = app
        .docker_compose_save(
            session.into(),
            project.into(),
            "forged".into(),
            edits(&file.path, SERVICE_V2, &digest),
            true,
            false,
        )
        .await
        .unwrap();
    assert_eq!(locked, ComposeSaveOutcome::Locked);
    assert!(app
        .docker_compose_reveal(session.into(), project.into(), "forged".into())
        .await
        .unwrap()
        .is_none());
    let grant = app
        .docker_compose_unlock(session.into(), project.into(), PASSPHRASE.into())
        .await
        .unwrap();
    let full = app
        .docker_compose_reveal(session.into(), project.into(), grant.clone())
        .await
        .unwrap()
        .expect("grant is valid");
    let revealed = full.files.iter().find(|f| f.path == file.path).unwrap();
    assert_eq!(revealed.content.as_deref(), Some(SERVICE_V1));
    assert!(!revealed.masked);
    assert_eq!(revealed.digest.as_deref(), Some(digest.as_str()));

    let invalid = app
        .docker_compose_save(
            session.into(),
            project.into(),
            grant.clone(),
            edits(&file.path, BROKEN, &digest),
            true,
            false,
        )
        .await
        .unwrap();
    match invalid {
        ComposeSaveOutcome::Invalid { output } => {
            assert!(!output.is_empty());
            assert!(!output.contains("consolecrypt-"), "{output}");
        }
        other => panic!("expected invalid, got {other:?}"),
    }
    assert_eq!(read_on_host(&file.path), SERVICE_V1);

    let conflict = app
        .docker_compose_save(
            session.into(),
            project.into(),
            grant.clone(),
            edits(&file.path, SERVICE_V2, "0000"),
            true,
            false,
        )
        .await
        .unwrap();
    assert!(
        matches!(conflict, ComposeSaveOutcome::Conflict { .. }),
        "{conflict:?}"
    );

    let saved = app
        .docker_compose_save(
            session.into(),
            project.into(),
            grant,
            edits(&file.path, SERVICE_V2, &digest),
            true,
            false,
        )
        .await
        .unwrap();
    let ComposeSaveOutcome::Saved {
        backups,
        job_id: Some(job),
    } = saved
    else {
        panic!("expected saved, got {saved:?}");
    };
    assert_eq!(backups.len(), 1);
    assert!(backups[0].contains(".bak-"), "{backups:?}");
    let (lines, exit) = wait_job(app, job).await;
    assert_eq!(exit, Some(0), "{lines:?}");
    assert_eq!(read_on_host(&file.path), SERVICE_V2);
    assert_eq!(read_on_host(&backups[0]), SERVICE_V1);
    let version = host_docker(&[
        "inspect",
        "-f",
        "{{index .Config.Labels \"cc.version\"}}",
        &format!("{project}-app-1"),
    ]);
    assert_eq!(version, "2", "recreated by up -d: {lines:?}");
    backups[0].clone()
}

/// A project directory every user can write to: saving asks first, then
/// writes with consent.
async fn shared_directory(
    app: &AppCore,
    session: &str,
    project: &str,
    path_suffix: &str,
    dir: &str,
    read_on_host: &dyn Fn(&str) -> String,
) {
    let files = app
        .docker_compose_project(session.into(), project.into())
        .await
        .unwrap();
    let file = files
        .files
        .iter()
        .find(|f| f.path.ends_with(path_suffix))
        .unwrap()
        .clone();
    let digest = file.digest.clone().unwrap();
    let grant = app
        .docker_compose_unlock(session.into(), project.into(), PASSPHRASE.into())
        .await
        .unwrap();
    let asked = app
        .docker_compose_save(
            session.into(),
            project.into(),
            grant.clone(),
            edits(&file.path, SERVICE_V3, &digest),
            false,
            false,
        )
        .await
        .unwrap();
    assert_eq!(
        asked,
        ComposeSaveOutcome::UnsafeDirectory {
            dirs: vec![dir.to_owned()]
        }
    );
    assert_eq!(read_on_host(&file.path), SERVICE_V2);
    let saved = app
        .docker_compose_save(
            session.into(),
            project.into(),
            grant,
            edits(&file.path, SERVICE_V3, &digest),
            false,
            true,
        )
        .await
        .unwrap();
    assert!(
        matches!(saved, ComposeSaveOutcome::Saved { job_id: None, .. }),
        "{saved:?}"
    );
    assert_eq!(read_on_host(&file.path), SERVICE_V3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compose_files_on_this_computer() {
    if std::env::var("CC_DOCKER_LIVE").is_err() {
        eprintln!("CC_DOCKER_LIVE not set; skipped");
        return;
    }
    let project = "cccomposelive";
    remove_project(project);
    let dir = tempfile::tempdir().unwrap();
    let compose = dir.path().join("compose.yaml");
    std::fs::write(&compose, SERVICE_V1).unwrap();
    let up = Command::new("docker")
        .args(["compose", "-p", project, "-f", "compose.yaml", "up", "-d"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        up.status.success(),
        "{}",
        String::from_utf8_lossy(&up.stderr)
    );

    let tmp = tempfile::tempdir().unwrap();
    let app = common::app(&tmp.path().join("app"));
    app.create_local_profile("Compose local".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let session = match app.docker_connect("local".into()).await.unwrap() {
        DockerConnectOutcome::Connected(session) => session,
        other => panic!("local engine: {other:?}"),
    };
    let read = |path: &str| std::fs::read_to_string(path).unwrap_or_default();
    exercise(&app, &session.id, project, "compose.yaml", &read).await;
    // Group/world-writable directory warning: Unix permission bits only.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        let real = dir.path().canonicalize().unwrap();
        shared_directory(
            &app,
            &session.id,
            project,
            "compose.yaml",
            real.to_str().unwrap(),
            &read,
        )
        .await;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let unknown = app
        .docker_compose_project(session.id.clone(), "cc-no-such-project".into())
        .await
        .unwrap_err();
    assert_eq!(code(&unknown), "invalid_input");
    app.docker_disconnect(session.id).await.unwrap();
    remove_project(project);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn compose_files_over_ssh() {
    if !testing::enabled() {
        eprintln!("skipped: set CC_SSH_IT=1 to run Docker integration tests");
        return;
    }
    let project = "ccsshcomposelive";
    remove_project(project);
    let tb = tokio::task::spawn_blocking(SshTestbed::start_docker_host)
        .await
        .unwrap()
        .expect("docker host testbed");
    let host = tb.bastion1.clone();
    // The project directory belongs to the SSH user; one override is
    // root-only, as on servers where compose files are owned by root.
    tb.exec_in(
        &host,
        &format!(
            "apk add --no-cache docker-cli docker-cli-compose >/dev/null && addgroup tester docker && \
             mkdir -p /srv/cc && printf '%s' '{SERVICE_V1}' > /srv/cc/compose.yaml && \
             printf 'services: {{}}\\n' > /srv/cc/compose.override.yaml && \
             chown tester /srv/cc /srv/cc/compose.yaml && chmod 755 /srv/cc && \
             chmod 600 /srv/cc/compose.override.yaml && \
             cd /srv/cc && docker compose -p {project} -f compose.yaml -f compose.override.yaml up -d"
        ),
    )
    .expect("compose project on the SSH host");

    let tmp = tempfile::tempdir().unwrap();
    let app = common::app(&tmp.path().join("app"));
    app.create_local_profile("Compose SSH".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let s = &tb.secrets;
    let cred = app
        .import_ssh_key(
            "it key".into(),
            Some(s.user.clone()),
            s.key.private_openssh.expose_secret().to_owned(),
            None,
            false,
            None,
        )
        .await
        .unwrap();
    let mut h = common::host("compose-host", "127.0.0.1", Some(&cred.id));
    h.port = Some(tb.bastion1_port);
    h.host_key_policy = HostKeyPolicy::AcceptNew;
    let saved = app.save_host(h).await.unwrap();
    let session = match app.docker_connect(saved.id.clone()).await.unwrap() {
        DockerConnectOutcome::Connected(session) => session,
        other => panic!("ssh engine: {other:?}"),
    };

    let files = app
        .docker_compose_project(session.id.clone(), project.into())
        .await
        .unwrap();
    assert_eq!(files.host, "ssh");
    assert_eq!(files.working_dir, "/srv/cc");
    let paths: Vec<_> = files.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        ["/srv/cc/compose.yaml", "/srv/cc/compose.override.yaml"]
    );
    assert_eq!(files.files[1].error, Some("permission_denied"));

    // Validation runs as the SSH user and cannot read the root-only file.
    let grant = app
        .docker_compose_unlock(session.id.clone(), project.into(), PASSPHRASE.into())
        .await
        .unwrap();
    let digest = files.files[0].digest.clone().unwrap();
    let refused = app
        .docker_compose_save(
            session.id.clone(),
            project.into(),
            grant,
            edits("/srv/cc/compose.yaml", SERVICE_V2, &digest),
            true,
            false,
        )
        .await
        .unwrap();
    assert!(
        matches!(&refused, ComposeSaveOutcome::Invalid { output } if output.contains("permission denied")),
        "{refused:?}"
    );
    tb.exec_in(&host, "chmod 644 /srv/cc/compose.override.yaml")
        .unwrap();

    let read = |path: &str| {
        tb.exec_in(&host, &format!("cat '{path}'"))
            .unwrap_or_default()
    };
    // `exec_in` trims output; compare against trimmed texts.
    let read_full = |path: &str| format!("{}\n", read(path));
    let backup = exercise(&app, &session.id, project, "/compose.yaml", &read_full).await;
    let owner = tb
        .exec_in(&host, &format!("stat -c %U '{backup}'"))
        .unwrap();
    assert_eq!(owner, "tester");
    tb.exec_in(&host, "chmod 0777 /srv/cc").unwrap();
    shared_directory(
        &app,
        &session.id,
        project,
        "/compose.yaml",
        "/srv/cc",
        &read_full,
    )
    .await;
    app.docker_disconnect(session.id).await.unwrap();
    remove_project(project);
}
