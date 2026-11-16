use super::*;
use cc_models::{
    VaultObject,
    host::{Host, SshBackend},
};
mod monitoring;
#[cfg(windows)]
fn protect_empty_owned_root(path: &std::path::Path) {
    use std::process::{Command, Stdio};
    let temp = std::env::temp_dir().canonicalize().unwrap();
    let actual = path.canonicalize().unwrap();
    assert_eq!(actual.parent(), Some(temp.as_path()));
    assert!(actual.file_name().unwrap().to_string_lossy().starts_with("owned-publication-agent-installer-"));
    assert!(std::fs::read_dir(&actual).unwrap().next().is_none());
    let powershell = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let mut child = Command::new(powershell).args(["-NoProfile", "-NonInteractive", "-Command", include_str!("fixture_acl.ps1")])
        .env("CC_AGENT_INSTALLER_TEST_ROOT", path)
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().unwrap().is_some() { break; }
        if Instant::now() >= until {
            child.kill().unwrap(); child.wait().unwrap();
            panic!("owned empty ACL fixture preparation timeout");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "owned empty ACL preparation: {}", String::from_utf8_lossy(&output.stderr));
}
async fn fixture() -> (tempfile::TempDir, AppCore, Value) {
    let dir = tempfile::Builder::new().prefix("owned-publication-agent-installer-").tempdir().unwrap();
    #[cfg(windows)]
    protect_empty_owned_root(dir.path());
    let app = AppCore::new(crate::AppConfig::for_tests(
        dir.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile(
        "Installer source fixture".into(),
        "synthetic installer fixture password42".into(),
    )
    .await
    .unwrap();
    let (owner, u) = app.unlocked().await.unwrap();
    let mut applied = app.subscribe_events();
    let mut host = Host::new("Saved Ask/OpenSSH source target", "192.0.2.48");
    host.backend = SshBackend::OpenSsh;
    let host_id = host.id.to_string();
    u.writer.put(VaultObject::Host(host)).await.unwrap();
    // SAME ordered object task has completed its queued readback, so a later
    // original source frame cannot be retired by fixture setup still in flight.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let crate::AppEvent::ObjectsChanged { ids, .. } = applied.recv().await.unwrap() {
                if ids.contains(&host_id) { break; }
            }
        }
    }).await.unwrap();
    let frame = json!({"version":1,"profileId":owner.profile_id,"notificationScopeId":u.notices.scope_id().unwrap()});
    drop(u);
    drop(owner);
    (dir, app, frame)
}
#[tokio::test]
async fn source_target_choices_keep_real_revision_without_widening_association_trust() {
    let (_dir, app, frame) = fixture().await;
    let raw = app
        .automation_agent_installer_context_json(&frame.to_string())
        .await
        .unwrap();
    let v: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["hosts"].as_array().unwrap().len(), 1);
    let rev = v["hosts"][0]["hostRevision"]
        .as_str()
        .unwrap()
        .parse::<i64>()
        .unwrap();
    assert!(rev > 0);
    let (_, u) = app.unlocked().await.unwrap();
    assert!(
        u.working()
            .native_association_host_choices()
            .unwrap()
            .is_empty()
    );
    assert_eq!(v["notificationScopeId"], frame["notificationScopeId"]);
}
#[tokio::test]
async fn actual_factory_creates_only_source_project_with_exact_selected_target_and_no_run() {
    let (_dir, app, frame) = fixture().await;
    let context: Value = serde_json::from_str(
        &app.automation_agent_installer_context_json(&frame.to_string())
            .await
            .unwrap(),
    )
    .unwrap();
    let host = &context["hosts"][0];
    let artifact = _dir.path().join("public-fixture.zip");
    let manifest = _dir.path().join("release-manifest.json");
    std::fs::write(&artifact,include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"),"/../automation-core/src/project/publication/agent_installer_seed/fixtures/parse-fixture.zip"))).unwrap();
    std::fs::write(&manifest,include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"),"/../automation-core/src/project/publication/agent_installer_seed/fixtures/signed/release-manifest.json"))).unwrap();
    let mut request = frame.clone();
    request.as_object_mut().unwrap().extend(json!({"name":"Owned Agent source","operation":"install","start":false,
        "hostId":host["hostId"],"expectedHostRevision":host["hostRevision"],"artifactPath":artifact,"manifestPath":manifest}).as_object().unwrap().clone());
    let result: Value = serde_json::from_str(
        &app.automation_agent_installer_create_json(&request.to_string())
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(result["actualRuntimeVersion"], Value::Null);
    assert_eq!(result["claimedRelease"]["signatureVerified"], false);
    let id = result["project"]["id"].as_str().unwrap().to_owned();
    let projects = app.automation_projects().await.unwrap();
    assert_eq!(projects.projects.len(), 1);
    assert_eq!(projects.projects[0].id, id);
    let source = app.automation_project_context(&id).await.unwrap();
    assert_eq!(
        source.inventory.selected_hosts.unwrap(),
        vec![Uuid::parse_str(host["hostId"].as_str().unwrap()).unwrap()]
    );
    let play = app
        .automation_project_file(id, "playbook.yml".into())
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&play.text).unwrap()[0]["vars"]["cc_agent_install_operation"],
        "install"
    );
    assert!(
        app.automation_jobs().await.unwrap()["jobs"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // Untouched serializer capture after the actual source factory and durable
    // catalog path. NOT proof of an installed/running or enrolled Agent.
    println!("CC_AGENT_INSTALLER_CONTEXT={}", context);
    println!("CC_AGENT_INSTALLER_CREATED={}", result);
}
#[tokio::test]
async fn wrong_original_nonce_is_refused_before_any_project_or_binary_read() {
    let (_dir, app, mut frame) = fixture().await;
    frame["notificationScopeId"] = json!("old opaque nonce");
    let mut request = frame.clone();
    request.as_object_mut().unwrap().extend(json!({"name":"Must not exist","operation":"install","start":false,
        "hostId":Uuid::new_v4(),"expectedHostRevision":"1","artifactPath":"never-opened.zip","manifestPath":"never-opened.json"}).as_object().unwrap().clone());
    assert!(
        app.automation_agent_installer_create_json(&request.to_string())
            .await
            .is_err()
    );
    assert!(app.automation_projects().await.unwrap().projects.is_empty());
    let (owner, _) = app.unlocked().await.unwrap();
    assert!(
        !owner
            .dir
            .as_ref()
            .unwrap()
            .join("automation/projects")
            .exists()
    );
}

#[tokio::test]
async fn malformed_release_schema_is_refused_before_source_directory_creation() {
    let (dir, app, frame) = fixture().await;
    let context: Value = serde_json::from_str(
        &app.automation_agent_installer_context_json(&frame.to_string())
            .await
            .unwrap(),
    )
    .unwrap();
    let artifact = dir.path().join("public.zip");
    let manifest = dir.path().join("invalid.json");
    std::fs::write(&artifact,include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"),"/../automation-core/src/project/publication/agent_installer_seed/fixtures/parse-fixture.zip"))).unwrap();
    std::fs::write(&manifest, b"{}").unwrap();
    let mut request = frame;
    request.as_object_mut().unwrap().extend(json!({"name":"Not published","operation":"install","start":false,
        "hostId":context["hosts"][0]["hostId"],"expectedHostRevision":context["hosts"][0]["hostRevision"],"artifactPath":artifact,"manifestPath":manifest}).as_object().unwrap().clone());
    assert!(
        app.automation_agent_installer_create_json(&request.to_string())
            .await
            .is_err()
    );
    assert!(app.automation_projects().await.unwrap().projects.is_empty());
    let (owner, _) = app.unlocked().await.unwrap();
    assert!(
        !owner
            .dir
            .as_ref()
            .unwrap()
            .join("automation/projects")
            .exists()
    );
}

#[tokio::test]
async fn edited_original_host_revision_cannot_be_adopted_by_source_creation() {
    let (_dir, app, frame) = fixture().await;
    let context: Value = serde_json::from_str(
        &app.automation_agent_installer_context_json(&frame.to_string())
            .await
            .unwrap(),
    )
    .unwrap();
    let host = Uuid::parse_str(context["hosts"][0]["hostId"].as_str().unwrap()).unwrap();
    let (_, u) = app.unlocked().await.unwrap();
    let mut edited = Host::new("Changed source target", "192.0.2.49");
    edited.id = serde_json::from_value(context["hosts"][0]["hostId"].clone()).unwrap();
    u.writer.put(VaultObject::Host(edited)).await.unwrap();
    drop(u);
    let mut request = frame;
    request.as_object_mut().unwrap().extend(json!({"name":"Not published","operation":"status","start":false,
        "hostId":host,"expectedHostRevision":context["hosts"][0]["hostRevision"],"artifactPath":null,"manifestPath":null}).as_object().unwrap().clone());
    assert!(
        app.automation_agent_installer_create_json(&request.to_string())
            .await
            .is_err()
    );
    assert!(app.automation_projects().await.unwrap().projects.is_empty());
}

#[tokio::test]
async fn original_cancel_ends_held_sql_wait_and_queued_publication_remains_absent() {
    use std::future::Future;
    let (_dir, app, frame) = fixture().await;
    let context: Value = serde_json::from_str(
        &app.automation_agent_installer_context_json(&frame.to_string())
            .await
            .unwrap(),
    )
    .unwrap();
    let original = Original::capture(
        &app,
        parse(&frame.to_string()).unwrap(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let (owner, _) = app.unlocked().await.unwrap();
    let profile = owner.dir.clone().unwrap();
    let storage = owner.storage().unwrap();
    drop(owner);
    let project = Uuid::new_v4();
    let worker = original.clone();
    let seed = tokio::task::spawn_blocking(move || {
        let seed = AgentInstallerSeed::create(
            &profile,
            project,
            AgentInstallOperation::Status,
            false,
            None,
            |_, _, _| Ok(()),
            || {
                worker
                    .check()
                    .map_err(|_| cc_automation_core::AutomationError::Closed)
            },
        )?;
        // Same full recheck placement as the production OS worker, beforeSQL.
        seed.recheck()?;
        Ok::<_, cc_automation_core::AutomationError>(seed)
    })
    .await
    .unwrap()
    .unwrap();
    let (entered, started) = tokio::sync::oneshot::channel();
    let (release, held) = std::sync::mpsc::channel();
    let control = storage.clone();
    let blocker = tokio::spawn(async move {
        control
            .call(move |_| {
                entered.send(()).map_err(|_| unavailable())?;
                held.recv_timeout(Duration::from_secs(5))
                    .map_err(|_| unavailable())?;
                Ok::<_, AppError>(())
            })
            .await
    });
    started.await.unwrap();
    let host = Uuid::parse_str(context["hosts"][0]["hostId"].as_str().unwrap()).unwrap();
    let revision = context["hosts"][0]["hostRevision"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let mut publish = Box::pin(publish_seed(
        original.clone(),
        storage,
        seed,
        project,
        "Held source publication".into(),
        host,
        revision,
    ));
    // First poll drives the REAL production body to storage.call's enqueue,
    // then observes Pending behind the already-held actual SQL worker.
    std::future::poll_fn(|cx| {
        assert!(publish.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    original.cancel.cancel();
    assert!(
        tokio::time::timeout(Duration::from_millis(250), publish)
            .await
            .expect("original cancel must end wait before worker release")
            .is_err()
    );
    release.send(()).unwrap();
    blocker.await.unwrap().unwrap();
    // The queued body still uses the cancelled ORIGINAL scope; it cannot
    // resurrect a record when its worker finally executes after callerretire.
    assert!(app.automation_projects().await.unwrap().projects.is_empty());
    assert!(
        app.automation_jobs().await.unwrap()["jobs"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
