mod common;
use cc_app_core::automation_runners::AutomationRunnerConfig;
use cc_automation_core::RemoteHelperConfig;
use common::*;

fn remote() -> AutomationRunnerConfig {
    AutomationRunnerConfig::Remote {
        host_id: uuid::Uuid::new_v4().to_string(),
        helper: RemoteHelperConfig {
            python: "/opt/runner/bin/python".into(),
            module_dir: "/opt/runner/src".into(),
            root: "/home/runner/private state".into(),
        },
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runner_profiles_survive_restart_but_never_cross_profile_or_locked_boundaries() {
    let temporary = tempfile::tempdir().unwrap();
    let store = memory_store();
    let app = app_with(temporary.path(), store.clone(), false);
    let profile = app
        .create_local_profile("A".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let runner = app
        .automation_runner_create("Prepared remote".into(), remote())
        .await
        .unwrap();
    assert!(!runner.read_only);
    assert_eq!(
        app.automation_runners().await.unwrap()["runners"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // Creating a profile never dials its arbitrary UUID host or installs tools.
    assert!(app
        .automation_runner_connect(runner.id.clone())
        .await
        .is_err());
    app.lock().await.unwrap();
    assert!(app.automation_runners().await.is_err());
    assert!(app
        .automation_runner_forget(runner.id.clone(), 1)
        .await
        .is_err());
    app.shutdown().await.unwrap();
    let reopened = app_with(temporary.path(), store, false);
    reopened.open_profile(profile.profile.id).await.unwrap();
    reopened
        .unlock_with_passphrase(PASSPHRASE.into())
        .await
        .unwrap();
    assert_eq!(
        reopened.automation_runners().await.unwrap()["runners"][0]["id"],
        runner.id
    );
    assert!(reopened
        .automation_runner_forget(runner.id.clone(), 2)
        .await
        .is_err());
    reopened
        .automation_runner_forget(runner.id.clone(), 1)
        .await
        .unwrap();
    assert!(reopened.automation_runners().await.unwrap()["runners"]
        .as_array()
        .unwrap()
        .is_empty());
    let other = reopened
        .automation_runner_create("A only".into(), remote())
        .await
        .unwrap();
    reopened
        .create_local_profile("B".into(), PASSPHRASE.into())
        .await
        .unwrap();
    assert!(reopened.automation_runners().await.unwrap()["runners"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(reopened.automation_runner_connect(other.id).await.is_err());
    reopened.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_runner_paths_are_rejected_before_any_launch() {
    let temporary = tempfile::tempdir().unwrap();
    let app = app(temporary.path());
    app.create_local_profile("A".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let AutomationRunnerConfig::Remote {
        host_id,
        mut helper,
    } = remote()
    else {
        unreachable!()
    };
    helper.root = "/safe\nexec anything".into();
    assert!(app
        .automation_runner_create(
            "Invalid".into(),
            AutomationRunnerConfig::Remote { host_id, helper }
        )
        .await
        .is_err());
    assert!(app.automation_runners().await.unwrap()["runners"]
        .as_array()
        .unwrap()
        .is_empty());
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runner_edit_keeps_identity_checks_revision_and_never_launches_or_crosses_profiles() {
    let temporary = tempfile::tempdir().unwrap();
    let app = app(temporary.path());
    app.create_local_profile("A".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let original = app
        .automation_runner_create("Before".into(), remote())
        .await
        .unwrap();
    let edited = app
        .automation_runner_update(original.id.clone(), 1, "After".into(), remote())
        .await
        .unwrap();
    assert_eq!(edited.id, original.id);
    assert_eq!(edited.revision, Some(2));
    assert_eq!(edited.name.as_deref(), Some("After"));
    assert!(app
        .automation_runner_update(original.id.clone(), 1, "Stale".into(), remote())
        .await
        .is_err());
    assert_eq!(
        app.automation_runners().await.unwrap()["runners"][0]["name"],
        "After"
    );
    app.lock().await.unwrap();
    assert!(app
        .automation_runner_update(original.id.clone(), 2, "Locked".into(), remote())
        .await
        .is_err());
    app.create_local_profile("B".into(), PASSPHRASE.into())
        .await
        .unwrap();
    assert!(app
        .automation_runner_update(original.id, 2, "Wrong profile".into(), remote())
        .await
        .is_err());
    app.shutdown().await.unwrap();
}
