use super::*;
use crate::dto::AutomationJobStateDto;
use tokio::sync::broadcast;

async fn fixture(
    state: &str,
    revision: Option<Value>,
) -> (tempfile::TempDir, AppCore, Arc<Session>, Uuid) {
    let directory = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        directory.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile(
        "Owned job notice fixture".into(),
        "correct horse notice fixture 42".into(),
    )
    .await
    .unwrap();
    let (owner, _) = app.unlocked().await.unwrap();
    let job = Uuid::new_v4();
    let mut row = json!({"version":1,"job_id":job,"runner_id":Uuid::new_v4(),"project_id":Uuid::new_v4(),
        "host_ids":[Uuid::new_v4()],"state":state,"manifest":{"secret_fixture_marker":"never_in_notice"},
        "manifest_digest":"a".repeat(64),"future_field":{"preserve":true}});
    if let Some(revision) = revision {
        row["notification_revision"] = revision;
    }
    let mut catalog = Catalog::default();
    catalog.records.insert(job.to_string(), row);
    owner
        .storage()
        .unwrap()
        .write(move |tx| tx.setting_set(JOBS, &catalog))
        .await
        .unwrap();
    (directory, app, owner, job)
}

async fn next_job(events: &mut broadcast::Receiver<AppEvent>) -> AppEvent {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let event = events.recv().await.unwrap();
            if matches!(event, AppEvent::AutomationJobChanged { .. }) {
                return event;
            }
        }
    })
    .await
    .unwrap()
}

fn assert_no_job(events: &mut broadcast::Receiver<AppEvent>) {
    while let Ok(event) = events.try_recv() {
        assert!(
            !matches!(event, AppEvent::AutomationJobChanged { .. }),
            "unexpected job notice"
        );
    }
}

#[test]
fn strict_job_event_serializes_only_allowlisted_metadata() {
    for state in [
        "queued",
        "dispatching",
        "running",
        "cancelling",
        "succeeded",
        "failed",
        "cancelled",
        "interrupted",
        "unknown",
        "acknowledged",
    ] {
        let state = AutomationJobStateDto::from_state(state).unwrap();
        let event = AppEvent::AutomationJobChanged {
            profile_id: Uuid::new_v4().to_string(),
            job_id: Uuid::new_v4().to_string(),
            revision: 1,
            state,
        };
        let encoded = serde_json::to_value(&event).unwrap();
        assert_eq!(encoded.as_object().unwrap().len(), 5);
        assert_eq!(encoded["type"], "automation_job_changed");
        assert_eq!(encoded["state"], state.as_state());
        assert_eq!(serde_json::from_value::<AppEvent>(encoded).unwrap(), event);
    }
    assert!(AutomationJobStateDto::from_state("successful").is_none());
    assert!(AutomationJobStateDto::from_state("server_text").is_none());
    assert!(serde_json::from_value::<AutomationJobStateDto>(json!("future_state")).is_err());
}

#[tokio::test]
async fn legacy_record_changes_start_at_one_and_event_follows_committed_state() {
    let (_directory, app, owner, job) = fixture("queued", None).await;
    let mut events = app.subscribe_events();
    change_state(&owner, job, "dispatching", None)
        .await
        .unwrap();
    let event = next_job(&mut events).await;
    assert_eq!(
        event,
        AppEvent::AutomationJobChanged {
            profile_id: owner.profile_id.to_string(),
            job_id: job.to_string(),
            revision: 1,
            state: AutomationJobStateDto::Dispatching
        }
    );
    let saved: Catalog = owner
        .storage()
        .unwrap()
        .setting_get(JOBS)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.records[&job.to_string()]["state"], "dispatching");
    assert_eq!(saved.records[&job.to_string()]["notification_revision"], 1);
    assert_eq!(
        saved.records[&job.to_string()]["manifest"]["secret_fixture_marker"],
        "never_in_notice"
    );
    let encoded = serde_json::to_string(&event).unwrap();
    for forbidden in [
        "manifest",
        "never_in_notice",
        "host_ids",
        "runner_result",
        "command",
        "path",
    ] {
        assert!(!encoded.contains(forbidden));
    }
}

#[tokio::test]
async fn repeated_same_state_does_not_increment_or_emit_but_keeps_evidence_write() {
    let (_directory, app, owner, job) = fixture("running", Some(json!(7))).await;
    let mut events = app.subscribe_events();
    let evidence = json!({"reason":"control_channel_or_storage_lost","control_failure":"rpc_closed",
        "remote_changes_may_be_partial":true,"bounded_fixture":"must_be_redacted"});
    use sha2::{Digest, Sha256};
    let expected_sha = Sha256::digest(serde_json::to_vec(&evidence).unwrap())
        .iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    change_state(&owner, job, "running", Some(evidence))
    .await
    .unwrap();
    assert_no_job(&mut events);
    let saved: Catalog = owner
        .storage()
        .unwrap()
        .setting_get(JOBS)
        .await
        .unwrap()
        .unwrap();
    let row = &saved.records[&job.to_string()];
    assert_eq!(row["notification_revision"], 7);
    assert_eq!(row["runner_result"], json!({"reason":"control_channel_or_storage_lost",
        "control_failure":"rpc_closed","remote_changes_may_be_partial":true}));
    assert!(row["runner_result"].get("bounded_fixture").is_none());
    assert_eq!(row["runner_result_original_sha256"], expected_sha);
    assert_eq!(row["runner_result_redacted"], true);
    assert_eq!(row["future_field"]["preserve"], true);
}

#[tokio::test]
async fn overflow_or_malformed_revision_rolls_back_state_and_emits_nothing() {
    for revision in [
        json!(u64::MAX),
        json!(MAX_NOTIFICATION_REVISION),
        json!(MAX_NOTIFICATION_REVISION + 1),
        json!(-1),
        json!("bad"),
        Value::Null,
    ] {
        let (_directory, app, owner, job) = fixture("running", Some(revision.clone())).await;
        let mut events = app.subscribe_events();
        assert!(
            change_state(&owner, job, "failed", Some(json!({"must_rollback":true})))
                .await
                .is_err()
        );
        assert_no_job(&mut events);
        let saved: Catalog = owner
            .storage()
            .unwrap()
            .setting_get(JOBS)
            .await
            .unwrap()
            .unwrap();
        let row = &saved.records[&job.to_string()];
        assert_eq!(row["state"], "running");
        assert_eq!(row["notification_revision"], revision);
        assert!(row.get("runner_result").is_none());
    }
}

#[tokio::test]
async fn unknown_input_state_and_finished_transition_do_not_notify() {
    let (_directory, app, owner, job) = fixture("running", None).await;
    let mut events = app.subscribe_events();
    assert!(change_state(&owner, job, "future_state", None)
        .await
        .is_err());
    assert_no_job(&mut events);
    change_state(&owner, job, "succeeded", None).await.unwrap();
    next_job(&mut events).await;
    assert!(change_state(&owner, job, "failed", None).await.is_err());
    assert_no_job(&mut events);
}

#[tokio::test]
async fn recovery_emits_only_actual_durable_changes_and_retains_unknown_leases() {
    let (_directory, app, owner, queued) = fixture("queued", None).await;
    let active = Uuid::new_v4();
    let unknown = Uuid::new_v4();
    owner
        .storage()
        .unwrap()
        .write(move |tx| {
            let mut catalog: Catalog = tx.setting_get(JOBS)?.unwrap();
            let row = catalog.records[&queued.to_string()].clone();
            for (id, state, revision) in [(active, "running", 3), (unknown, "unknown", 4)] {
                let mut next = row.clone();
                next["job_id"] = json!(id);
                next["state"] = json!(state);
                next["notification_revision"] = json!(revision);
                catalog.records.insert(id.to_string(), next);
            }
            tx.setting_set(JOBS, &catalog)
        })
        .await
        .unwrap();
    let mut events = app.subscribe_events();
    app.automation_initialize_jobs(&owner).await.unwrap();
    let changes = [next_job(&mut events).await, next_job(&mut events).await];
    assert_no_job(&mut events);
    assert!(changes.contains(&AppEvent::AutomationJobChanged {
        profile_id: owner.profile_id.to_string(),
        job_id: queued.to_string(),
        revision: 1,
        state: AutomationJobStateDto::Interrupted
    }));
    assert!(changes.contains(&AppEvent::AutomationJobChanged {
        profile_id: owner.profile_id.to_string(),
        job_id: active.to_string(),
        revision: 4,
        state: AutomationJobStateDto::Unknown
    }));
    let leases = owner.automation.coordinator.unknown_jobs();
    assert!(leases.contains(&active) && leases.contains(&unknown));
    app.automation_initialize_jobs(&owner).await.unwrap();
    assert_no_job(&mut events);
}

#[tokio::test]
async fn concurrent_changes_have_distinct_committed_monotonic_revisions() {
    let (_directory, app, owner, job) = fixture("queued", Some(json!(10))).await;
    let mut events = app.subscribe_events();
    let (first, second) = tokio::join!(
        change_state(&owner, job, "dispatching", None),
        change_state(&owner, job, "running", None)
    );
    first.unwrap();
    second.unwrap();
    let changes = [next_job(&mut events).await, next_job(&mut events).await];
    let mut revisions = changes
        .into_iter()
        .map(|event| match event {
            AppEvent::AutomationJobChanged { revision, .. } => revision,
            _ => unreachable!(),
        })
        .collect::<Vec<_>>();
    revisions.sort();
    assert_eq!(revisions, [11, 12]);
    let saved: Catalog = owner
        .storage()
        .unwrap()
        .setting_get(JOBS)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.records[&job.to_string()]["notification_revision"], 12);
}

#[tokio::test]
async fn safe_integer_boundary_emits_exact_maximum_and_valid_cancelling_state() {
    let (_directory, app, owner, job) =
        fixture("running", Some(json!(MAX_NOTIFICATION_REVISION - 1))).await;
    let mut events = app.subscribe_events();
    change_state(&owner, job, "cancelling", None).await.unwrap();
    assert_eq!(
        next_job(&mut events).await,
        AppEvent::AutomationJobChanged {
            profile_id: owner.profile_id.to_string(),
            job_id: job.to_string(),
            revision: MAX_NOTIFICATION_REVISION,
            state: AutomationJobStateDto::Cancelling,
        }
    );
    assert!(change_state(&owner, job, "cancelled", None).await.is_err());
    assert_no_job(&mut events);
    let saved: Catalog = owner
        .storage()
        .unwrap()
        .setting_get(JOBS)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.records[&job.to_string()]["state"], "cancelling");
    assert_eq!(
        saved.records[&job.to_string()]["notification_revision"],
        MAX_NOTIFICATION_REVISION
    );
}
