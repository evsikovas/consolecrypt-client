//! Actual AppCore Git source start/poll/edit, using only an owned TLS fixture.
use cc_app_core::{automation_runners::AutomationRunnerConfig, AppConfig, AppCore};
use cc_automation_core::{HelperLaunch, GIT_SOURCE_FILE};
use serde::Deserialize;
use serde_json::json;
use std::{
    io::{self, BufRead},
    time::Duration,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    launch: HelperLaunch,
    url: String,
    ca_pem: String,
    commit: String,
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    if line.len() > 128 * 1024 {
        return Err("fixture exceeds limit".into());
    }
    let fixture: Fixture = serde_json::from_str(&line)?;
    let folder = tempfile::tempdir()?;
    let app = AppCore::new(AppConfig::for_tests(
        folder.path().to_string_lossy().into_owned(),
    ))?;
    app.create_local_profile(
        "Git source fixture".into(),
        "correct horse source fixture passphrase".into(),
    )
    .await?;
    let result = async {
        let runner = app
            .automation_runner_create(
                "Owned source Runner".into(),
                AutomationRunnerConfig::Local {
                    launch: fixture.launch,
                },
            )
            .await?;
        app.automation_runner_connect(runner.id.clone()).await?;
        let started = app
            .automation_project_git_start(
                "TLS source fixture".into(),
                runner.id,
                fixture.url,
                "main".into(),
                Some(fixture.ca_pem),
            )
            .await?;
        assert!(started.read_only);
        assert_eq!(started.import_state.as_deref(), Some("importing"));
        let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
        let ready = loop {
            let projects = app.automation_projects().await?;
            let row = projects
                .projects
                .into_iter()
                .find(|row| row.id == started.id)
                .ok_or("missing durable import")?;
            match row.import_state.as_deref() {
                Some("ready") => break row,
                Some("importing") => {}
                _ => return Err(format!("source import failed: {:?}", row.import_error).into()),
            }
            if tokio::time::Instant::now() > deadline {
                return Err("source import deadline exceeded".into());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        assert!(!ready.read_only);
        assert_eq!(ready.git_source.as_ref().unwrap()["commit"], fixture.commit);
        let baseline = app.automation_project_files(started.id.clone()).await?;
        assert_eq!(
            ready.git_source.as_ref().unwrap()["snapshot_digest"],
            baseline.digest
        );
        let before = app
            .automation_project_file(started.id.clone(), "playbook.yml".into())
            .await?;
        let changed = format!("{}\n# Manual local edit\n", before.text);
        app.automation_project_save(
            started.id.clone(),
            "playbook.yml".into(),
            Some(before.sha256),
            changed.clone(),
        )
        .await?;
        assert_eq!(
            app.automation_project_file(started.id.clone(), "playbook.yml".into())
                .await?
                .text,
            changed
        );
        let modified = app.automation_project_files(started.id.clone()).await?;
        assert_ne!(baseline.digest, modified.digest);
        let diff=app.automation_project_diff(started.id.clone(),None,0,50).await?;
        assert_eq!(diff["total"],1);
        assert_eq!(diff["changes"][0]["path"],"playbook.yml");
        assert_eq!(diff["baseline_digest"],baseline.digest);
        assert_eq!(diff["current_digest"],modified.digest);
        let file_diff=app.automation_project_file_diff(started.id.clone(),"playbook.yml".into(),modified.digest.clone()).await?;
        assert_eq!(file_diff["before"],before.text);
        assert_eq!(file_diff["after"],changed);
        assert!(app.automation_project_diff(started.id.clone(),Some(baseline.digest),0,50).await.is_err());
        assert!(app.automation_project_file_diff(started.id.clone(),"../outside".into(),modified.digest).await.is_err());
        let catalog = app.automation_projects().await?;
        let saved = catalog
            .projects
            .iter()
            .find(|row| row.id == started.id)
            .unwrap();
        assert_eq!(saved.git_source, ready.git_source);
        let metadata = app
            .automation_project_file(started.id.clone(), GIT_SOURCE_FILE.into())
            .await?;
        assert!(app
            .automation_project_save(
                started.id,
                GIT_SOURCE_FILE.into(),
                Some(metadata.sha256),
                "forged".into()
            )
            .await
            .is_err());
        Ok::<_, Box<dyn std::error::Error>>(())
    }
    .await;
    app.shutdown().await?;
    result?;
    println!(
        "{}",
        json!({"status":"PASS","native_appcore_git":true,"durable_import":true,"partial_tree_read_only":true,
        "manual_edits_preserved":true,"source_metadata_protected":true,"original_tree_diff_verified":true,"stale_diff_denied":true})
    );
    Ok(())
}
