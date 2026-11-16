//! Owned actual-Prometheus acceptance fixture. No existing profiles or targets.
//! Build/run only after the root agent freezes active Native source.
use cc_app_core::{AppConfig, AppCore, HostDto};
use cc_monitoring_core::{
    AuthRef, QueryData, QueryOutcome, Route, SampleValue, SourceConfig, TlsConfig,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{self, Read},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

type ProofResult<T> = Result<T, Box<dyn std::error::Error>>;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Fixture {
    version: u32,
    token: String,
    fixture_root: PathBuf,
    port: u16,
    selected_end: f64,
}
fn require(ok: bool, message: &'static str) -> ProofResult<()> {
    if ok {
        Ok(())
    } else {
        Err(message.into())
    }
}
fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_secs_f64()
}
fn read_fixture() -> ProofResult<Fixture> {
    let mut raw = Vec::new();
    io::stdin().lock().take(4097).read_to_end(&mut raw)?;
    require(raw.len() <= 4096, "fixture input exceeds limit")?;
    let fixture: Fixture = serde_json::from_slice(&raw).map_err(|_| "invalid fixture input")?;
    require(
        fixture.version == 1
            && fixture.port >= 1024
            && fixture.selected_end.is_finite()
            && fixture.selected_end < now() - 0.1
            && fixture.selected_end > now() - 120.0,
        "invalid fixture endpoint/time",
    )?;
    let token = Uuid::parse_str(&fixture.token).map_err(|_| "invalid fixture token")?;
    require(
        !token.is_nil() && token.to_string() == fixture.token,
        "invalid fixture token",
    )?;
    let tasks = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .ok_or("fixture workspace unavailable")?
        .join("Tasks")
        .canonicalize()?;
    let expected_directory = format!("monitoring-bindings-prometheus-fixture-{}", fixture.token);
    require(
        fixture.fixture_root.is_absolute()
            && fixture.fixture_root.file_name().and_then(|s| s.to_str())
                == Some(expected_directory.as_str()),
        "invalid owned fixture directory",
    )?;
    require(
        fixture
            .fixture_root
            .parent()
            .ok_or("invalid fixture parent")?
            .canonicalize()?
            == tasks
            && !std::fs::symlink_metadata(&fixture.fixture_root)?
                .file_type()
                .is_symlink(),
        "fixture directory is not owned workspace",
    )?;
    let marker = fixture.fixture_root.join(".fixture-token");
    require(
        !std::fs::symlink_metadata(&marker)?.file_type().is_symlink()
            && std::fs::metadata(&marker)?.len() <= 128
            && std::fs::read_to_string(marker)? == fixture.token,
        "fixture token mismatch",
    )?;
    Ok(fixture)
}
async fn read(app: &AppCore, source: &str, action: Value) -> ProofResult<Value> {
    let value=app.monitoring_read(json!({"operationId":Uuid::new_v4().to_string(),"sourceId":source,"sourceRevision":1,"action":action}).to_string()).await.map_err(|_|"native monitoring read failed")?;
    require(
        value["sourceId"] == source && value["sourceRevision"] == 1,
        "native source identity mismatch",
    )?;
    Ok(value["result"].clone())
}
fn expected(id: &str) -> ProofResult<(&'static str, f64, f64)> {
    Ok(match id {
        "cpuBusy" => ("percent", 10.0, 0.5),
        "memoryUsedPercent" => ("percent", 60.0, 0.0001),
        "memoryAvailable" => ("bytes", 400.0, 0.0001),
        "filesystemUnavailablePercent" => ("percent", 75.0, 0.0001),
        "diskRead" => ("bytesPerSecond", 100.0, 0.5),
        "diskWrite" => ("bytesPerSecond", 200.0, 1.0),
        "networkReceive" => ("bytesPerSecond", 300.0, 1.5),
        "networkTransmit" => ("bytesPerSecond", 400.0, 2.0),
        "containerCpu" => ("cpuCores", 0.25, 0.002),
        "containerMemory" => ("bytes", 256.0, 0.0001),
        "postgresConnections" => ("connections", 3.0, 0.0001),
        "postgresCommits" => ("transactionsPerSecond", 5.0, 0.03),
        "redisMemory" => ("bytes", 512.0, 0.0001),
        "redisClients" => ("connections", 4.0, 0.0001),
        "redisCommands" => ("operationsPerSecond", 7.0, 0.04),
        "nginxConnections" => ("connections", 2.0, 0.0001),
        "nginxRequests" => ("requestsPerSecond", 11.0, 0.06),
        _ => return Err("unexpected typed panel query".into()),
    })
}
async fn prove(app: &AppCore, fixture: &Fixture) -> ProofResult<Value> {
    app.create_local_profile(
        "Owned Prometheus acceptance".into(),
        "Synthetic monitoring fixture passphrase 42".into(),
    )
    .await
    .map_err(|_| "fixture profile creation failed")?;
    let host = app
        .save_host(HostDto::new(
            "Synthetic monitoring host",
            "fixture.example.invalid",
        ))
        .await
        .map_err(|_| "synthetic host creation failed")?;
    let catalog = app
        .monitoring_source_create(
            0,
            SourceConfig {
                id: String::new(),
                name: "Owned Prometheus fixture".into(),
                endpoint: format!("http://127.0.0.1:{}", fixture.port),
                route: Route::Direct,
                tls: TlsConfig::default(),
                auth: AuthRef::None,
            },
        )
        .await
        .map_err(|_| "source creation failed")?;
    require(
        catalog.revision == Some(1) && catalog.sources.len() == 1,
        "unexpected source catalog",
    )?;
    let source = catalog.sources[0].id.clone();
    let catalog = app
        .monitoring_binding_create(
            1,
            "Explicit fixture exporter".into(),
            host.id,
            source.clone(),
            1,
            BTreeMap::from([
                ("job".into(), "fixture".into()),
                ("instance".into(), "fixture:9100".into()),
            ]),
        )
        .await
        .map_err(|_| "explicit binding creation failed")?;
    require(
        catalog.schema_version == Some(2) && catalog.bindings.len() == 1,
        "explicit binding not persisted",
    )?;
    let binding = &catalog.bindings[0].id;
    let probe = read(app, &source, json!({"kind":"probe"})).await?;
    require(
        probe["query"]["supported"] == true && probe["queryRange"]["supported"] == true,
        "real Prometheus probe failed",
    )?;
    let discovery=read(app,&source,json!({"kind":"bindingDiscover","bindingId":binding,"bindingRevision":1,"time":fixture.selected_end})).await?;
    require(
        discovery["state"] == "resolved" && discovery["resolution"]["state"] == "resolved",
        "explicit discovery did not resolve",
    )?;
    let panels = [
        "hostOverview",
        "cpu",
        "memory",
        "filesystem",
        "diskIo",
        "network",
        "docker",
        "postgreSql",
        "redis",
        "nginx",
    ];
    let start = fixture.selected_end - 1.0;
    let mut summary = BTreeMap::new();
    let mut count = 0;
    for panel in panels {
        let result=read(app,&source,json!({"kind":"bindingPanel","bindingId":binding,"bindingRevision":1,"panel":panel,"start":start,"end":fixture.selected_end,"stepSeconds":0.5})).await?;
        require(
            result["state"] == "ready" && result["panel"] == panel,
            "real typed panel unavailable or partial",
        )?;
        require(
            result["preparation"]["state"] == "ready"
                && result["preparation"]["plan"]["start"] == start
                && result["preparation"]["plan"]["end"] == fixture.selected_end,
            "historical panel range changed",
        )?;
        let queries = result["queries"]
            .as_array()
            .ok_or("missing typed panel queries")?;
        require(!queries.is_empty(), "empty typed panel queries")?;
        for row in queries {
            let id = row["id"].as_str().ok_or("missing typed query identifier")?;
            let (unit, value, tolerance) = expected(id)?;
            require(row["unit"] == unit, "typed panel unit mismatch")?;
            let outcome: QueryOutcome = serde_json::from_value(row["outcome"].clone())
                .map_err(|_| "invalid native query outcome")?;
            require(
                outcome.http_status == 200
                    && outcome.status == "success"
                    && !outcome.partial
                    && outcome.warnings.is_empty()
                    && outcome.error.is_none(),
                "real query was partial or unsuccessful",
            )?;
            let Some(QueryData::Matrix { series }) = outcome.data else {
                return Err("real range result is not a matrix".into());
            };
            require(series.len() == 1, "unexpected fixture series count")?;
            let series = &series[0];
            require(
                series.labels.get("job").map(String::as_str) == Some("fixture")
                    && series.labels.get("instance").map(String::as_str) == Some("fixture:9100")
                    && series.labels.get("fixture_run") == Some(&fixture.token),
                "query escaped owned fixture identity",
            )?;
            require(series.samples.len() >= 2, "range history was not returned")?;
            let mut last = None;
            for sample in &series.samples {
                require(
                    sample.timestamp.is_finite()
                        && sample.timestamp >= start - 0.001
                        && sample.timestamp <= fixture.selected_end + 0.001
                        && sample.timestamp < now() - 0.1,
                    "historical sample time invalid",
                )?;
                let SampleValue::Finite { value: actual } = sample.value else {
                    return Err("nonfinite fixture sample".into());
                };
                require(
                    actual.is_finite() && (actual - value).abs() <= tolerance,
                    "real PromQL numeric result mismatch",
                )?;
                last = Some(actual);
            }
            summary.insert(
                id.to_owned(),
                json!({"unit":unit,"last":last.ok_or("missing finite sample")?}),
            );
            count += 1;
        }
    }
    require(
        count == 20 && summary.len() == 17,
        "typed panel/query coverage mismatch",
    )?;
    let saved = app
        .monitoring_sources()
        .await
        .map_err(|_| "catalog reload failed")?;
    require(
        saved.bindings.len() == 1 && saved.sources.len() == 1 && saved.schema_version == Some(2),
        "saved binding/source catalog changed",
    )?;
    Ok(
        json!({"fixture":"monitoringBindingPrometheus","probe":true,"discovery":"resolved","panels":10,"queries":count,"historical":true,"values":summary}),
    )
}
#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ProofResult<()> {
    let fixture = read_fixture()?;
    let profile = fixture.fixture_root.join("native-profile");
    std::fs::create_dir(&profile).map_err(|_| "fresh fixture profile directory required")?;
    let app = AppCore::new(AppConfig::for_tests(profile.to_string_lossy().into_owned()))
        .map_err(|_| "fixture AppCore creation failed")?;
    let result = prove(&app, &fixture).await;
    app.shutdown()
        .await
        .map_err(|_| "fixture shutdown failed")?;
    println!("{}", result?);
    Ok(())
}
