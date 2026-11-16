//! Public AppCore binding reads against owned loopback HTTP, with observed
//! wire selectors/times. The fixture evaluates no PromQL and executes no jobs.
mod common;

use axum::{Form, Json, Router, extract::State, routing::post};
use cc_app_core::{AppCore, HostDto};
use cc_host_mapping::Labels;
use cc_monitoring_core::{AuthRef, Route, SourceConfig, TlsConfig};
use common::{PASSPHRASE, app};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::Notify;
use uuid::Uuid;

const END: f64 = 1_700_000_000.0;
const START: f64 = END - 300.0;
const INSTANCE: &str = "synthetic\"scope:9100";
const SCOPE: &str = r#"{instance="synthetic\"scope:9100",job="node"}"#;
const CPU_METRIC: &str =
    r#"node_cpu_seconds_total{instance="synthetic\"scope:9100",job="node",mode="idle"}"#;
const CPU: &str = r#"100 * (1 - avg without (cpu, mode) (rate(node_cpu_seconds_total{instance="synthetic\"scope:9100",job="node",mode="idle"}[5m])))"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn panel_combined_budgets_empty_history_nan_and_probe_warnings_remain_explicit() {
    for (mode, expected_state, expected_requests) in [
        (Discovery::ManyRangeSeries, "partial", 4),
        (Discovery::EmptyRange, "noData", 5),
        (Discovery::WarningRange, "partial", 5),
        (Discovery::NanRange, "ready", 5),
        (Discovery::WarningProbe, "unavailable", 2),
    ] {
        let fixture = fixture(mode, false).await;
        let scenario = Scenario::new(&fixture).await;
        let result = scenario
            .app
            .monitoring_read(scenario.request(scenario.panel()))
            .await
            .unwrap();
        let panel = &result["result"];
        assert_eq!(panel["state"], expected_state);
        assert_eq!(fixture.seen().len(), expected_requests);
        match mode {
            Discovery::ManyRangeSeries => {
                assert_eq!(panel["limited"], true);
                assert_eq!(panel["error"]["code"], "budget_exceeded");
                assert_eq!(panel["queries"].as_array().unwrap().len(), 1);
            }
            Discovery::EmptyRange => {
                for query in panel["queries"].as_array().unwrap() {
                    assert!(
                        query["outcome"]["data"]["series"]
                            .as_array()
                            .unwrap()
                            .is_empty()
                    );
                }
            }
            Discovery::NanRange => {
                for query in panel["queries"].as_array().unwrap() {
                    assert_eq!(
                        query["outcome"]["data"]["series"][0]["samples"][0]["value"]["kind"],
                        "nan"
                    );
                }
            }
            Discovery::WarningRange => assert!(
                panel["queries"][0]["outcome"]["warnings"]
                    .as_array()
                    .unwrap()
                    .len()
                    == 1
            ),
            Discovery::WarningProbe => {
                assert!(panel.get("queries").is_none());
                assert_eq!(panel["preparation"]["state"], "unavailable");
                assert_eq!(panel["preparation"]["reason"], "probeFailed");
            }
            _ => unreachable!(),
        }
        scenario.app.shutdown().await.unwrap();
    }
}

#[derive(Clone, Copy)]
enum Discovery {
    Single,
    Missing,
    Ambiguous,
    Warning,
    Partial,
    ManyRangeSeries,
    EmptyRange,
    WarningRange,
    NanRange,
    WarningProbe,
}

#[derive(Clone, Debug)]
struct Seen {
    path: &'static str,
    form: BTreeMap<String, String>,
}

struct FixtureState {
    discovery: Discovery,
    block_first: bool,
    seen: Mutex<Vec<Seen>>,
    entered: Notify,
    release: Notify,
}

struct Fixture {
    endpoint: String,
    state: Arc<FixtureState>,
    server: tokio::task::JoinHandle<()>,
}
impl Fixture {
    fn seen(&self) -> Vec<Seen> {
        self.state.seen.lock().unwrap().clone()
    }
    async fn first_entered(&self) {
        tokio::time::timeout(Duration::from_secs(5), self.state.entered.notified())
            .await
            .expect("own HTTP fixture was not reached");
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.state.release.notify_waiters();
        self.server.abort();
    }
}

fn labels() -> Labels {
    Labels::from([
        ("instance".into(), INSTANCE.into()),
        ("job".into(), "node".into()),
    ])
}

fn up_row() -> Value {
    json!({"metric":{"__name__":"up","instance":INSTANCE,"job":"node"},"value":[END,"1"]})
}

async fn instant(
    State(state): State<Arc<FixtureState>>,
    Form(form): Form<BTreeMap<String, String>>,
) -> Json<Value> {
    respond(state, "/api/v1/query", form).await
}
async fn range(
    State(state): State<Arc<FixtureState>>,
    Form(form): Form<BTreeMap<String, String>>,
) -> Json<Value> {
    respond(state, "/api/v1/query_range", form).await
}
async fn respond(
    state: Arc<FixtureState>,
    path: &'static str,
    form: BTreeMap<String, String>,
) -> Json<Value> {
    let index = {
        let mut seen = state.seen.lock().unwrap();
        let index = seen.len();
        assert!(index < 16, "fixture request budget exceeded");
        assert!(form["query"].len() < 16 * 1024);
        seen.push(Seen {
            path,
            form: form.clone(),
        });
        index
    };
    state.entered.notify_one();
    if state.block_first && index == 0 {
        state.release.notified().await;
    }
    if path == "/api/v1/query_range" {
        let mut body = json!({"status":"success","data":{"resultType":"matrix","result":[
            {"metric":{"instance":INSTANCE,"job":"node"},"values":[[START,"5"],[END,"7"]]}
        ]}});
        match state.discovery {
            Discovery::ManyRangeSeries => {
                body["data"]["result"] =
                    json!((0..60).map(|index| json!({
                "metric":{"instance":INSTANCE,"job":"node","shard":index.to_string()},
                "values":[[START,"5"],[END,"7"]]
            })).collect::<Vec<_>>())
            }
            Discovery::EmptyRange => body["data"]["result"] = json!([]),
            Discovery::WarningRange => body["warnings"] = json!(["owned fixture range warning"]),
            Discovery::NanRange => {
                body["data"]["result"][0]["values"] = json!([[START, "NaN"], [END, "NaN"]])
            }
            _ => {}
        }
        return Json(body);
    }
    if form["query"].starts_with("count by (__name__)") {
        let mut body = json!({"status":"success","data":{"resultType":"vector","result":[
            {"metric":{"__name__":"node_cpu_seconds_total"},"value":[END,"1"]},
            {"metric":{"__name__":"node_memory_MemTotal_bytes"},"value":[END,"1"]},
            {"metric":{"__name__":"node_memory_MemAvailable_bytes"},"value":[END,"1"]}
        ]}});
        if matches!(state.discovery, Discovery::WarningProbe) {
            body["warnings"] = json!(["owned fixture metric probe warning"]);
        }
        return Json(body);
    }
    let rows = match state.discovery {
        Discovery::Missing => vec![],
        Discovery::Ambiguous => vec![up_row(), up_row()],
        _ => vec![up_row()],
    };
    let mut body = json!({"status":"success","data":{"resultType":"vector","result":rows}});
    if matches!(state.discovery, Discovery::Warning) {
        body["warnings"] = json!(["owned fixture incomplete discovery"]);
    }
    if matches!(state.discovery, Discovery::Partial) {
        body["status"] = json!("error");
        body["errorType"] = json!("timeout");
        body["error"] = json!("owned fixture partial discovery");
    }
    Json(body)
}

async fn fixture(discovery: Discovery, block_first: bool) -> Fixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let state = Arc::new(FixtureState {
        discovery,
        block_first,
        seen: Mutex::new(vec![]),
        entered: Notify::new(),
        release: Notify::new(),
    });
    let router = Router::new()
        .route("/api/v1/query", post(instant))
        .route("/api/v1/query_range", post(range))
        .with_state(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    Fixture {
        endpoint,
        state,
        server,
    }
}

struct Scenario {
    _directory: tempfile::TempDir,
    app: AppCore,
    host: HostDto,
    source: SourceConfig,
    binding_id: String,
}
impl Scenario {
    async fn new(fixture: &Fixture) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let app = app(directory.path());
        app.create_local_profile("Own binding runtime fixture".into(), PASSPHRASE.into())
            .await
            .unwrap();
        let host = app
            .save_host(HostDto::new(
                "Own native host",
                "host-fixture.example.invalid",
            ))
            .await
            .unwrap();
        let catalog = app
            .monitoring_source_create(
                0,
                SourceConfig {
                    id: String::new(),
                    name: "Own binding HTTP fixture".into(),
                    endpoint: fixture.endpoint.clone(),
                    route: Route::Direct,
                    tls: TlsConfig::default(),
                    auth: AuthRef::None,
                },
            )
            .await
            .unwrap();
        let source = catalog.sources[0].configuration.clone().unwrap();
        let catalog = app
            .monitoring_binding_create(
                1,
                "Node exporter".into(),
                host.id.clone(),
                source.id.clone(),
                1,
                labels(),
            )
            .await
            .unwrap();
        Self {
            _directory: directory,
            app,
            host,
            source,
            binding_id: catalog.bindings[0].id.clone(),
        }
    }
    fn panel(&self) -> Value {
        json!({"kind":"bindingPanel","bindingId":self.binding_id,"bindingRevision":1,
            "panel":"hostOverview","start":START,"end":END,"stepSeconds":60})
    }
    fn discover(&self) -> Value {
        json!({"kind":"bindingDiscover","bindingId":self.binding_id,"bindingRevision":1,"time":END})
    }
    fn request(&self, action: Value) -> String {
        json!({"operationId":Uuid::new_v4(),"sourceId":self.source.id,"sourceRevision":1,"action":action}).to_string()
    }
}

fn assert_discovery_wire(seen: &Seen) {
    assert_eq!(seen.path, "/api/v1/query");
    assert_eq!(seen.form["query"], format!("up{SCOPE}"));
    assert_eq!(seen.form["time"].parse::<f64>().unwrap(), END);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_binding_discover_returns_one_candidate_without_metric_or_panel_queries() {
    let fixture = fixture(Discovery::Single, false).await;
    let scenario = Scenario::new(&fixture).await;
    let response = scenario
        .app
        .monitoring_read(scenario.request(scenario.discover()))
        .await
        .unwrap();
    assert_eq!(response["sourceId"], scenario.source.id);
    assert_eq!(response["sourceRevision"], 1);
    let result = &response["result"];
    assert_eq!(result["kind"], "bindingDiscovery");
    assert_eq!(result["bindingId"], scenario.binding_id);
    assert_eq!(result["bindingRevision"], 1);
    assert_eq!(result["state"], "resolved");
    assert_eq!(
        result["resolution"],
        json!({"state":"resolved","candidateIndex":0})
    );
    assert!(result.get("panel").is_none());
    assert!(result.get("preparation").is_none());
    assert!(result.get("queries").is_none());
    let seen = fixture.seen();
    assert_eq!(seen.len(), 1);
    assert_discovery_wire(&seen[0]);
    scenario.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_and_ambiguous_discovery_stop_before_any_metric_probe_or_range_query() {
    for (discovery, expected) in [
        (Discovery::Missing, "missing"),
        (Discovery::Ambiguous, "ambiguous"),
    ] {
        let fixture = fixture(discovery, false).await;
        let scenario = Scenario::new(&fixture).await;
        let response = scenario
            .app
            .monitoring_read(scenario.request(scenario.panel()))
            .await
            .unwrap();
        let result = &response["result"];
        assert_eq!(result["state"], expected);
        assert_eq!(result["resolution"]["state"], expected);
        if expected == "ambiguous" {
            assert_eq!(result["resolution"]["candidateIndices"], json!([0, 1]));
        }
        assert!(result.get("preparation").is_none());
        assert!(result.get("queries").is_none());
        let seen = fixture.seen();
        assert_eq!(seen.len(), 1);
        assert_discovery_wire(&seen[0]);
        scenario.app.shutdown().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn complete_host_panel_uses_exact_quoted_selectors_historical_end_and_typed_units() {
    let fixture = fixture(Discovery::Single, false).await;
    let scenario = Scenario::new(&fixture).await;
    let response = scenario
        .app
        .monitoring_read(scenario.request(scenario.panel()))
        .await
        .unwrap();
    let result = &response["result"];
    assert_eq!(response["sourceId"], scenario.source.id);
    assert_eq!(response["sourceRevision"], 1);
    assert_eq!(result["state"], "ready");
    assert_eq!(
        result["resolution"],
        json!({"state":"resolved","candidateIndex":0})
    );
    assert_eq!(result["bindingId"], scenario.binding_id);
    let queries = result["queries"].as_array().unwrap();
    assert_eq!(
        queries
            .iter()
            .map(|q| q["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["cpuBusy", "memoryUsedPercent", "memoryAvailable"]
    );
    assert_eq!(
        queries
            .iter()
            .map(|q| q["unit"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["percent", "percent", "bytes"]
    );
    assert!(
        queries
            .iter()
            .all(|q| q["outcome"]["data"]["kind"] == "matrix")
    );
    let seen = fixture.seen();
    assert_eq!(seen.len(), 5);
    assert_discovery_wire(&seen[0]);
    assert_eq!(seen[1].path, "/api/v1/query");
    assert_eq!(seen[1].form["time"].parse::<f64>().unwrap(), END);
    assert_eq!(
        seen[1].form["query"],
        format!(
            "count by (__name__) ({CPU_METRIC} or on (__name__) node_memory_MemTotal_bytes{SCOPE} or on (__name__) node_memory_MemAvailable_bytes{SCOPE})"
        )
    );
    let expected = [
        CPU.to_owned(),
        format!(
            "100 * (1 - node_memory_MemAvailable_bytes{SCOPE} / node_memory_MemTotal_bytes{SCOPE})"
        ),
        format!("node_memory_MemAvailable_bytes{SCOPE}"),
    ];
    for (seen, expected) in seen[2..].iter().zip(expected) {
        assert_eq!(seen.path, "/api/v1/query_range");
        assert_eq!(seen.form["query"], expected);
        assert_eq!(seen.form["start"].parse::<f64>().unwrap(), START);
        assert_eq!(seen.form["end"].parse::<f64>().unwrap(), END);
        assert_eq!(seen.form["step"].parse::<f64>().unwrap(), 60.0);
    }
    scenario.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn warning_or_partial_discovery_is_unavailable_and_never_manufactures_metric_zeroes() {
    for discovery in [Discovery::Warning, Discovery::Partial] {
        let fixture = fixture(discovery, false).await;
        let scenario = Scenario::new(&fixture).await;
        let response = scenario
            .app
            .monitoring_read(scenario.request(scenario.panel()))
            .await
            .unwrap();
        let result = &response["result"];
        assert_eq!(result["state"], "unavailable");
        assert_eq!(result["reason"], "discoveryInvalid");
        assert!(result.get("resolution").is_none());
        assert!(result.get("preparation").is_none());
        assert!(result.get("queries").is_none());
        match discovery {
            Discovery::Warning => assert_eq!(
                result["discovery"]["warnings"],
                json!(["owned fixture incomplete discovery"])
            ),
            Discovery::Partial => {
                assert_eq!(result["discovery"]["partial"], true);
                assert_eq!(result["discovery"]["error"]["type"], "timeout");
            }
            _ => unreachable!(),
        }
        assert_eq!(fixture.seen().len(), 1);
        scenario.app.shutdown().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_binding_and_host_mutations_cancel_before_blocked_http_can_return_data() {
    for mutation in ["source", "binding", "host"] {
        let fixture = fixture(Discovery::Single, true).await;
        let mut scenario = Scenario::new(&fixture).await;
        let reader = scenario.app.clone();
        let input = scenario.request(scenario.panel());
        let read = tokio::spawn(async move { reader.monitoring_read(input).await });
        fixture.first_entered().await;
        match mutation {
            "source" => {
                let mut source = scenario.source.clone();
                source.name = "Explicitly changed source generation".into();
                scenario
                    .app
                    .monitoring_source_update(source.id.clone(), 2, 1, source)
                    .await
                    .unwrap();
            }
            "binding" => {
                scenario
                    .app
                    .monitoring_binding_update(
                        scenario.binding_id.clone(),
                        2,
                        1,
                        "Explicitly rereviewed exporter".into(),
                        scenario.host.id.clone(),
                        scenario.source.id.clone(),
                        1,
                        labels(),
                    )
                    .await
                    .unwrap();
            }
            "host" => {
                scenario.host.name = "Explicitly changed native host metadata".into();
                scenario.app.save_host(scenario.host.clone()).await.unwrap();
            }
            _ => unreachable!(),
        }
        let result = tokio::time::timeout(Duration::from_secs(3), read)
            .await
            .expect("metadata mutation must cancel before fixture releases HTTP")
            .unwrap();
        assert!(
            result.is_err(),
            "{mutation} mutation returned old metric data"
        );
        assert_eq!(fixture.seen().len(), 1);
        fixture.state.release.notify_one();
        scenario.app.shutdown().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn caller_resolution_urls_expressions_and_wrong_sources_cannot_authorize_binding_reads() {
    let fixture = fixture(Discovery::Single, false).await;
    let scenario = Scenario::new(&fixture).await;
    for (field, value) in [
        ("resolution", json!({"state":"resolved","candidateIndex":0})),
        ("url", json!("http://127.0.0.1:1/unapproved")),
        ("expression", json!("up")),
    ] {
        for mut action in [scenario.panel(), scenario.discover()] {
            action[field] = value.clone();
            assert!(
                scenario
                    .app
                    .monitoring_read(scenario.request(action))
                    .await
                    .is_err()
            );
        }
    }
    let mut action = scenario.discover();
    action["bindingRevision"] = json!(2);
    assert!(
        scenario
            .app
            .monitoring_read(scenario.request(action))
            .await
            .is_err()
    );
    let mut input: Value = serde_json::from_str(&scenario.request(scenario.discover())).unwrap();
    input["sourceId"] = json!(Uuid::new_v4());
    assert!(
        scenario
            .app
            .monitoring_read(input.to_string())
            .await
            .is_err()
    );
    assert!(fixture.seen().is_empty());
    scenario.app.shutdown().await.unwrap();
}
