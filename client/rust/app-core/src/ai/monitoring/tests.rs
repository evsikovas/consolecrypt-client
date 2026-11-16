use super::*;
use cc_ai_core::PrivacyProfile;
use cc_models::ai::AiProviderKind;
use cc_monitoring_core::{AuthRef, Route, TlsConfig};
#[path = "../../../tests/common/ai_mock.rs"]
mod ai_mock;

const PASSWORD: &str = "owned selected monitoring fixture password 42";
struct Fixture {
    _dir: tempfile::TempDir,
    app: AppCore,
    mock: ai_mock::MockLlm,
    input: Value,
}
fn selected() -> Value {
    json!({"schemaVersion":1,"dataClassification":"untrustedSelectedMonitoringData",
        "projectionCreatedUtc":"2026-11-12T12:00:00.000Z","alerts":[],"metrics":[{
        "selectionRef":"metric-1","metricName":"owned_counter_total","unit":{"kind":"unknown"},
        "labels":{"mode":"user"},"selectedLabelsOmitted":0,
        "samples":[{"timestampUnixSecondsUtc":10.0,"value":{"kind":"finite","value":1.0}}],
        "availableSampleCount":1,"selectedSampleCount":1,"sampleSelection":"explicitIndicesCallerOrder",
        "gapKnowledge":"unselectedOrAbsentSamplesAreUnknownNeverZero",
        "actualAcceptedWindow":{"kind":"instant","requestedAtUtc":null},
        "acceptedCompletedUtc":"2026-11-12T12:00:00.000Z","stale":false,"refreshPending":false,
        "partial":false,"warningCount":0,"infoCount":0,"clockSkew":{"kind":"unknown"}}]})
}
async fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        dir.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("Owned monitoring AI".into(), PASSWORD.into())
        .await
        .unwrap();
    let mock = ai_mock::MockLlm::start().await;
    let provider = app
        .save_ai_provider(crate::AiProviderDto {
            id: String::new(),
            name: "Owned model".into(),
            provider: AiProviderKind::OpenaiCompatible,
            base_url: mock.url(),
            has_api_key: false,
            chat_model: "owned-model".into(),
            embedding_model: None,
            timeout_secs: 10,
            streaming: false,
            tool_support: false,
            privacy_profile: PrivacyProfile::Strict,
            is_default: true,
            created_at_ms: 0,
            updated_at_ms: 0,
        })
        .await
        .unwrap();
    let catalog = app
        .monitoring_source_create(
            0,
            SourceConfig {
                id: String::new(),
                name: "private-native-source-name".into(),
                endpoint: "https://private-native-source.example.invalid/prometheus".into(),
                route: Route::Direct,
                tls: TlsConfig::default(),
                auth: AuthRef::None,
            },
        )
        .await
        .unwrap();
    let (owner, _) = app.unlocked().await.unwrap();
    let input = json!({"version":1,"profileId":owner.profile_id.to_string(),
        "sourceId":catalog.sources[0].id,"sourceRevision":1,"providerId":provider.id,
        "action":"explain","question":"Explain this; password=never-send-canary-7491","selectedData":selected()});
    Fixture {
        _dir: dir,
        app,
        mock,
        input,
    }
}
async fn prepare(f: &Fixture) -> Value {
    serde_json::from_str(
        &f.app
            .monitoring_ai_prepare_json(&f.input.to_string())
            .await
            .unwrap(),
    )
    .unwrap()
}
fn send(f: &Fixture, p: &Value, approve: bool) -> String {
    json!({"version":1,"profileId":f.input["profileId"],"ticket":p["ticket"],"approveExternal":approve}).to_string()
}
fn response() -> &'static str {
    r#"{"version":1,"explanation":"Observed selected value only","proposedPlaybook":null,"limitations":["No host identity or execution attestation"]}"#
}
async fn reached(mock: &ai_mock::MockLlm) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while mock.count("/v1/chat/completions") == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("owned HTTP request entered");
}

#[test]
fn strict_outer_and_nested_duplicates_are_rejected_before_value_collapse() {
    let base=json!({"version":1,"profileId":Uuid::new_v4(),"sourceId":Uuid::new_v4(),
        "sourceRevision":1,"providerId":Uuid::new_v4(),"action":"explain","question":"q","selectedData":selected()}).to_string();
    assert!(parse::<Prepare>(&base).is_ok());
    for bad in [
        base.replacen("\"version\":1", "\"version\":1,\"version\":1", 1),
        base.replacen(
            "\"mode\":\"user\"",
            "\"mode\":\"user\",\"mo\\u0064e\":\"other\"",
            1,
        ),
        base.replacen("\"question\":", "\"endpoint\":\"hidden\",\"question\":", 1),
        " ".repeat(MAX_REQUEST + 1),
    ] {
        assert!(parse::<Prepare>(&bad).is_err());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exact_preview_is_sanitized_selected_only_one_use_and_readonly() {
    let f = fixture().await;
    let p = prepare(&f).await;
    assert!(p["expiresUtc"].as_str().unwrap().ends_with('Z'));
    assert_eq!(f.mock.count("/v1/chat/completions"), 0);
    let preview = p["previewText"].as_str().unwrap().to_owned();
    for private in [
        "never-send-canary-7491",
        "private-native-source",
        f.input["sourceId"].as_str().unwrap(),
        f.input["profileId"].as_str().unwrap(),
    ] {
        assert!(!preview.contains(private));
    }
    let (_, u) = f.app.unlocked().await.unwrap();
    let before = u.working().version();
    drop(u);
    f.mock.push_chat(response());
    let result = f
        .app
        .monitoring_ai_send_json(&send(&f, &p, false))
        .await
        .unwrap();
    let answer: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(answer["proposedPlaybook"], Value::Null);
    let captured: Value = serde_json::from_str(&f.mock.requests().last().unwrap().body).unwrap();
    assert_eq!(
        captured["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["content"].as_str().unwrap())
            .collect::<Vec<_>>()
            .join("\n\n"),
        preview
    );
    assert!(f
        .app
        .monitoring_ai_send_json(&send(&f, &p, false))
        .await
        .is_err());
    assert_eq!(f.mock.count("/v1/chat/completions"), 1);
    assert_eq!(
        f.app.unlocked().await.unwrap().1.working().version(),
        before
    );
    // The second supported action crosses the actual Native -> HTTP -> typed
    // response boundary too. Receiving YAML must never create a project/job.
    let mut task = f.input.clone();
    task["action"] = json!("proposePlaybook");
    let proposal_preview: Value = serde_json::from_str(
        &f.app
            .monitoring_ai_prepare_json(&task.to_string())
            .await
            .unwrap(),
    )
    .unwrap();
    let yaml = "---\n- hosts: separately_reviewed_targets\n  tasks: []\n";
    f.mock.push_chat(json!({"version":1,"explanation":"Unvalidated draft only","proposedPlaybook":yaml,"limitations":["Requires separate validation and target approval"]}).to_string());
    let proposal: Value = serde_json::from_str(
        &f.app
            .monitoring_ai_send_json(&send(&f, &proposal_preview, false))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(proposal["proposedPlaybook"], yaml);
    assert_eq!(f.mock.count("/v1/chat/completions"), 2);
    assert_eq!(
        f.app.unlocked().await.unwrap().1.working().version(),
        before
    );
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn external_consent_is_required_without_consuming_or_sending() {
    let f = fixture().await;
    let p = prepare(&f).await;
    // Keep transport on the owned loopback fixture; exercise the exact issuer's
    // non-loopback consent branch without contacting an external service.
    let (_, u) = f.app.unlocked().await.unwrap();
    lock(&u.ai.metrics.state)
        .pending
        .get_mut(&id(p["ticket"].as_str().unwrap()).unwrap())
        .unwrap()
        .external = true;
    drop(u);
    assert!(f
        .app
        .monitoring_ai_send_json(&send(&f, &p, false))
        .await
        .is_err());
    assert_eq!(f.mock.count("/v1/chat/completions"), 0);
    f.mock.push_chat(response());
    assert!(f
        .app
        .monitoring_ai_send_json(&send(&f, &p, true))
        .await
        .is_ok());
    assert_eq!(f.mock.count("/v1/chat/completions"), 1);
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_and_provider_edits_revoke_preview_before_http() {
    for change_source in [true, false] {
        let f = fixture().await;
        let p = prepare(&f).await;
        if change_source {
            let catalog = f.app.monitoring_sources().await.unwrap();
            let row = &catalog.sources[0];
            let mut cfg = row.configuration.clone().unwrap();
            cfg.name.push_str(" changed");
            f.app
                .monitoring_source_update(
                    row.id.clone(),
                    catalog.revision.unwrap(),
                    row.revision.unwrap(),
                    cfg,
                )
                .await
                .unwrap();
        } else {
            let mut cfg = f.app.list_ai_providers().await.unwrap().remove(0);
            cfg.chat_model = "changed-model".into();
            f.app.save_ai_provider(cfg).await.unwrap();
        }
        assert!(f
            .app
            .monitoring_ai_send_json(&send(&f, &p, false))
            .await
            .is_err());
        assert_eq!(f.mock.count("/v1/chat/completions"), 0);
        f.app.shutdown().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn in_flight_source_change_cancels_and_drops_the_obsolete_reply() {
    let f = fixture().await;
    let p = prepare(&f).await;
    let gate = f.mock.block_chat();
    f.mock.push_chat(response());
    let input = send(&f, &p, false);
    let future = f.app.monitoring_ai_send_json(&input);
    tokio::pin!(future);
    tokio::select! {r=&mut future=>panic!("completed before own held response: {r:?}"),_=reached(&f.mock)=>{}}
    let catalog = f.app.monitoring_sources().await.unwrap();
    let row = &catalog.sources[0];
    let mut cfg = row.configuration.clone().unwrap();
    cfg.name.push_str(" changed");
    f.app
        .monitoring_source_update(
            row.id.clone(),
            catalog.revision.unwrap(),
            row.revision.unwrap(),
            cfg,
        )
        .await
        .unwrap();
    gate.notify_one();
    assert!(tokio::time::timeout(Duration::from_secs(5), &mut future)
        .await
        .unwrap()
        .is_err());
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discard_cancels_inflight_and_no_ticket_survives_same_profile_reopen() {
    let f = fixture().await;
    let p = prepare(&f).await;
    let gate = f.mock.block_chat();
    f.mock.push_chat(response());
    let input = send(&f, &p, false);
    let future = f.app.monitoring_ai_send_json(&input);
    tokio::pin!(future);
    tokio::select! {r=&mut future=>panic!("completed before held response: {r:?}"),_=reached(&f.mock)=>{}}
    f.app
        .monitoring_ai_discard_json(
            &json!({"version":1,"profileId":f.input["profileId"],"ticket":p["ticket"]}).to_string(),
        )
        .await
        .unwrap();
    gate.notify_one();
    assert!(tokio::time::timeout(Duration::from_secs(5), &mut future)
        .await
        .unwrap()
        .is_err());
    drop(future);
    let p2 = prepare(&f).await;
    f.app.lock().await.unwrap();
    f.app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    assert!(f
        .app
        .monitoring_ai_send_json(&send(&f, &p2, false))
        .await
        .is_err());
    assert_eq!(f.mock.count("/v1/chat/completions"), 1);
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preview_limit_expiration_and_foreign_profile_never_dispatch() {
    let f = fixture().await;
    let mut previews = Vec::new();
    for _ in 0..4 {
        previews.push(prepare(&f).await);
    }
    assert!(f
        .app
        .monitoring_ai_prepare_json(&f.input.to_string())
        .await
        .is_err());
    let (_, u) = f.app.unlocked().await.unwrap();
    for review in lock(&u.ai.metrics.state).pending.values_mut() {
        review.deadline = Instant::now() - Duration::from_secs(1);
    }
    drop(u);
    assert!(f
        .app
        .monitoring_ai_send_json(&send(&f, &previews[0], false))
        .await
        .is_err());
    let p = prepare(&f).await;
    let mut wrong: Value = serde_json::from_str(&send(&f, &p, false)).unwrap();
    wrong["profileId"] = json!(Uuid::new_v4());
    assert!(f
        .app
        .monitoring_ai_send_json(&wrong.to_string())
        .await
        .is_err());
    assert_eq!(f.mock.count("/v1/chat/completions"), 0);
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn equal_revision_readback_is_checked_even_without_a_revision_cancellation() {
    for provider_change in [true, false] {
        let f = fixture().await;
        let p = prepare(&f).await;
        let gate = f.mock.block_chat();
        f.mock.push_chat(response());
        let input = send(&f, &p, false);
        let future = f.app.monitoring_ai_send_json(&input);
        tokio::pin!(future);
        tokio::select! {r=&mut future=>panic!("completed before held response: {r:?}"),_=reached(&f.mock)=>{}}
        let (_, u) = f.app.unlocked().await.unwrap();
        let (object, object_id) = if provider_change {
            let mut cfg = u
                .working()
                .ai_provider(
                    crate::dto::parse_id("provider", f.input["providerId"].as_str().unwrap())
                        .unwrap(),
                )
                .unwrap();
            cfg.chat_model = "equal-revision-changed".into();
            let id = cfg.id;
            (cc_models::VaultObject::AiProvider(cfg), id)
        } else {
            let mut cfg = u
                .working()
                .all_vault_settings()
                .into_iter()
                .max_by_key(|s| s.updated_at)
                .unwrap();
            cfg.monitoring.as_mut().unwrap()["sources"][f.input["sourceId"].as_str().unwrap()]
                ["configuration"]["name"] = json!("equal-revision-source-change");
            let id = cfg.id;
            (cc_models::VaultObject::VaultSettings(cfg), id)
        };
        let rev = u.working().revision_of(object_id).unwrap();
        let cancel = lock(&u.ai.metrics.state)
            .active
            .values()
            .next()
            .unwrap()
            .cancel
            .clone();
        u.working().apply(
            object_id,
            crate::working_set::Readback::Live(Box::new(cc_sync_core::DecryptedObject {
                object_id,
                revision: rev,
                local_state: cc_storage_core::LocalState::LocalOnly,
                conflict_origin: None,
                payload: cc_models::ObjectPayload::new(object),
            })),
        );
        assert!(
            !cancel.is_cancelled(),
            "exercise synchronous checks, not a revision bump"
        );
        drop(u);
        gate.notify_one();
        assert!(tokio::time::timeout(Duration::from_secs(5), &mut future)
            .await
            .unwrap()
            .is_err());
        f.app.shutdown().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_model_replacement_revokes_pending_and_active_review() {
    let mut f = fixture().await;
    let port: u16 = f.mock.base.rsplit(':').next().unwrap().parse().unwrap();
    let (_, u) = f.app.unlocked().await.unwrap();
    u.ai.register_device_local(
        "owned-local-one".into(),
        port,
        secrecy::SecretString::from("owned-only-local-key"),
        "owned-model".into(),
    )
    .unwrap();
    f.input["providerId"] = json!(super::super::DEVICE_LOCAL_PROVIDER_ID);
    let old = prepare(&f).await;
    u.ai.clear_device_local("owned-local-one");
    assert!(f
        .app
        .monitoring_ai_send_json(&send(&f, &old, false))
        .await
        .is_err());
    assert_eq!(f.mock.count("/v1/chat/completions"), 0);
    u.ai.register_device_local(
        "owned-local-two".into(),
        port,
        secrecy::SecretString::from("owned-only-local-key"),
        "owned-model".into(),
    )
    .unwrap();
    let p = prepare(&f).await;
    let gate = f.mock.block_chat();
    f.mock.push_chat(response());
    let input = send(&f, &p, false);
    let future = f.app.monitoring_ai_send_json(&input);
    tokio::pin!(future);
    tokio::select! {r=&mut future=>panic!("completed before held response: {r:?}"),_=reached(&f.mock)=>{}}
    u.ai.register_device_local(
        "owned-local-three".into(),
        port,
        secrecy::SecretString::from("owned-only-local-key"),
        "replacement-model".into(),
    )
    .unwrap();
    gate.notify_one();
    assert!(tokio::time::timeout(Duration::from_secs(5), &mut future)
        .await
        .unwrap()
        .is_err());
    drop(u);
    f.app.shutdown().await.unwrap();
}
