//! Public AppCore + own loopback HTTP only. No PromQL evaluation or job run.
mod common;
use axum::{extract::State, http::Uri, routing::post, Form, Json, Router};
use cc_app_core::{
    monitoring_sources::{MonitoringPanelDefinition, MonitoringUnitHint},
    AppCore,
};
use cc_monitoring_core::{AuthRef, Route, SourceConfig, TlsConfig};
use common::{app, PASSPHRASE};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::Notify;
use uuid::Uuid;

const END: f64 = 1_700_000_000.0;
const QUERY: &str = r#"sum(rate(saved_metric{job="explicit\"scope"}[5m]))"#;
#[derive(Default)]
struct Gate {
    seen: Mutex<Vec<(String, BTreeMap<String, String>)>>,
    entered: Notify,
    release: Notify,
    slow: bool,
}
async fn reply(
    State(gate): State<Arc<Gate>>,
    uri: Uri,
    Form(form): Form<BTreeMap<String, String>>,
) -> Json<Value> {
    gate.seen.lock().unwrap().push((uri.path().into(), form));
    gate.entered.notify_one();
    if gate.slow {
        gate.release.notified().await;
    }
    Json(if uri.path().ends_with("query_range") {
        json!({"status":"success","warnings":["own saved-panel warning"],"data":{"resultType":"matrix","result":[{"metric":{"job":"owned-fixture"},"values":[[END,"NaN"]]}]}})
    } else {
        json!({"status":"success","data":{"resultType":"vector","result":[{"metric":{"job":"owned-fixture"},"value":[END,"1"]}]}})
    })
}
struct Fixture {
    gate: Arc<Gate>,
    endpoint: String,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.gate.release.notify_waiters();
        self.server.abort();
    }
}
async fn fixture(slow: bool) -> Fixture {
    let gate = Arc::new(Gate {
        slow,
        ..Default::default()
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .route("/api/v1/query", post(reply))
        .route("/api/v1/query_range", post(reply))
        .with_state(gate.clone());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    Fixture {
        gate,
        endpoint,
        server,
    }
}
fn source(endpoint: &str) -> SourceConfig {
    SourceConfig {
        id: String::new(),
        name: "Owned saved panel source".into(),
        endpoint: endpoint.into(),
        route: Route::Direct,
        tls: TlsConfig::default(),
        auth: AuthRef::None,
    }
}
fn definition(source: &str) -> MonitoringPanelDefinition {
    MonitoringPanelDefinition {
        title: "Owned saved query".into(),
        source_id: source.into(),
        source_revision: 1,
        query: QUERY.into(),
        unit_hint: MonitoringUnitHint::Unknown,
    }
}
async fn setup(fixture: &Fixture) -> (tempfile::TempDir, AppCore, String, String) {
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    app.create_local_profile("Saved-panel HTTP fixture".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let catalog = app
        .monitoring_source_create(0, source(&fixture.endpoint))
        .await
        .unwrap();
    let source = catalog.sources[0].id.clone();
    let catalog = app
        .monitoring_panel_create(1, definition(&source))
        .await
        .unwrap();
    let panel = catalog.panels.unwrap()[0].id.clone();
    (directory, app, source, panel)
}
fn request(source: &str, panel: &str, range: bool) -> Value {
    let mut action = json!({"kind":"savedPanel","panelId":panel,"panelRevision":1});
    if range {
        action["range"] = json!({"start":END-300.0,"end":END,"stepSeconds":15.0});
    }
    json!({"operationId":Uuid::new_v4(),"sourceId":source,"sourceRevision":1,"action":action})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_saved_expression_and_historical_range_are_the_actual_http_request_with_unknown_unit_and_partial_nan(
) {
    let f = fixture(false).await;
    let (_dir, app, source, panel) = setup(&f).await;
    let response = app
        .monitoring_read(request(&source, &panel, true).to_string())
        .await
        .unwrap();
    let seen = f.gate.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, "/api/v1/query_range");
    assert_eq!(seen[0].1["query"], QUERY);
    assert_eq!(seen[0].1["start"], (END - 300.0).to_string());
    assert_eq!(seen[0].1["end"], END.to_string());
    assert_eq!(seen[0].1["step"], "15");
    let result = &response["result"];
    assert_eq!(result["kind"], "savedPanel");
    assert_eq!(result["panelId"], panel);
    assert_eq!(result["panelRevision"], 1);
    assert_eq!(result["unitHint"], "unknown");
    assert_eq!(result["outcome"]["partial"], true);
    assert_eq!(result["outcome"]["warnings"][0], "own saved-panel warning");
    assert_eq!(
        result["outcome"]["data"]["series"][0]["samples"][0]["value"]["kind"],
        "nan"
    );
    app.monitoring_read(request(&source, &panel, false).to_string())
        .await
        .unwrap();
    let seen = f.gate.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[1].0, "/api/v1/query");
    assert_eq!(seen[1].1["query"], QUERY);
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_expression_url_range_fields_wrong_source_and_stale_panel_or_source_fail_before_http(
) {
    let f = fixture(false).await;
    let (_dir, app, source_id, panel) = setup(&f).await;
    for key in ["expression", "endpoint", "url", "authorization", "unitHint"] {
        let mut invalid = request(&source_id, &panel, false);
        invalid["action"][key] = json!("not-authority");
        assert!(app.monitoring_read(invalid.to_string()).await.is_err());
    }
    for change in [
        json!({"panelRevision":2}),
        json!({"panelId":Uuid::new_v4()}),
        json!({"range":{"start":END,"end":END-1.0,"stepSeconds":1}}),
        json!({"range":{"start":0,"end":END,"stepSeconds":0}}),
        json!({"range":{"start":0,"end":END,"stepSeconds":1,"url":"https://never-fetch.invalid"}}),
    ] {
        let mut invalid = request(&source_id, &panel, false);
        for (key, value) in change.as_object().unwrap() {
            invalid["action"][key] = value.clone();
        }
        assert!(app.monitoring_read(invalid.to_string()).await.is_err());
    }
    let other = app
        .monitoring_source_create(2, source(&f.endpoint))
        .await
        .unwrap()
        .sources
        .into_iter()
        .find(|s| s.id != source_id)
        .unwrap()
        .id;
    assert!(app
        .monitoring_read(request(&other, &panel, false).to_string())
        .await
        .is_err());
    let mut changed = source(&f.endpoint);
    changed.id = source_id.clone();
    app.monitoring_source_update(source_id.clone(), 3, 1, changed)
        .await
        .unwrap();
    let mut stale = request(&source_id, &panel, false);
    stale["sourceRevision"] = json!(2);
    assert!(app.monitoring_read(stale.to_string()).await.is_err());
    assert!(f.gate.seen.lock().unwrap().is_empty());
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn in_flight_panel_update_delete_source_edit_lock_and_profile_switch_never_return_saved_metrics(
) {
    for mutation in [
        "panelUpdate",
        "panelDelete",
        "sourceEdit",
        "lock",
        "profile",
    ] {
        let f = fixture(true).await;
        let (_dir, app, source_id, panel) = setup(&f).await;
        let read = tokio::spawn({
            let app = app.clone();
            let text = request(&source_id, &panel, true).to_string();
            async move { app.monitoring_read(text).await }
        });
        tokio::time::timeout(Duration::from_secs(20), f.gate.entered.notified())
            .await
            .unwrap();
        match mutation {
            "panelUpdate" => {
                let mut next = definition(&source_id);
                next.query = "different_saved_query".into();
                app.monitoring_panel_update(panel.clone(), 2, 1, next)
                    .await
                    .unwrap();
            }
            "panelDelete" => {
                app.monitoring_panel_delete(panel.clone(), 2, 1)
                    .await
                    .unwrap();
            }
            "sourceEdit" => {
                let mut config = source(&f.endpoint);
                config.id = source_id.clone();
                app.monitoring_source_update(source_id.clone(), 2, 1, config)
                    .await
                    .unwrap();
            }
            "lock" => app.lock().await.unwrap(),
            _ => {
                app.create_local_profile("Different saved-panel fixture".into(), PASSPHRASE.into())
                    .await
                    .unwrap();
            }
        }
        assert!(tokio::time::timeout(Duration::from_secs(5), read)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        f.gate.release.notify_waiters();
        assert_eq!(f.gate.seen.lock().unwrap().len(), 1);
        app.shutdown().await.unwrap();
    }
}
