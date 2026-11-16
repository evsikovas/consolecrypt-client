//! Real AppCore -> own loopback HTTP/TLS. No evaluator, edits or notifications.
mod common;
use cc_monitoring_core::{AuthRef, Route, SourceConfig, TlsConfig};
use common::{app, PASSPHRASE};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::{Notify, Semaphore},
};
use uuid::Uuid;

fn alert(state: &str) -> Value {
    json!({"labels":{"alertname":"Own alert"},"annotations":{"summary":"http://never-fetch.invalid"},
        "state":state,"activeAt":"2026-11-11T12:00:00Z","value":"NaN"})
}
fn alerts() -> Value {
    json!({"status":"success","data":{"alerts":[alert("pending")]},"warnings":["one store missing"]})
}
fn rules() -> Value {
    json!({"status":"success","data":{"groups":[{"name":"Own group","file":"/never/open/rules.yml","rules":[{
        "name":"Own rule","type":"alerting","query":"http://never-execute.invalid","state":"inactive","health":"ok","alerts":[]
    }]}],"groupNextToken":"http://never-follow.invalid"}})
}
fn request(source: &str, revision: u64, operation: Uuid, kind: &str) -> String {
    json!({"operationId":operation,"sourceId":source,"sourceRevision":revision,"action":{"kind":kind}}).to_string()
}
fn source(fixture: &Fixture) -> SourceConfig {
    SourceConfig {
        id: String::new(),
        name: "Own configured alerts source".into(),
        endpoint: fixture.endpoint.clone(),
        route: Route::Direct,
        tls: TlsConfig {
            custom_ca_pem: fixture.ca.clone(),
        },
        auth: AuthRef::None,
    }
}
struct Request {
    target: String,
    authorization: String,
}
struct State {
    requests: Mutex<Vec<Request>>,
    entered: Notify,
    release: Semaphore,
    rejected_tls: AtomicUsize,
    responses: Mutex<(u16, Value, Value)>,
}
struct Fixture {
    endpoint: String,
    ca: Option<String>,
    state: Arc<State>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn wait(&self, count: usize) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let notified = self.state.entered.notified();
                if self.state.requests.lock().unwrap().len() >= count {
                    break;
                }
                notified.await;
            }
        })
        .await
        .expect("fixture did not receive expected fixed HTTP request");
    }
    fn count(&self) -> usize {
        self.state.requests.lock().unwrap().len()
    }
}
trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}
async fn fixture(tls: bool, blocked: bool) -> Fixture {
    let (acceptor, ca) = if tls {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let ca = cert.cert.pem();
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()).into(),
        )
        .unwrap();
        (
            Some(tokio_rustls::TlsAcceptor::from(Arc::new(config))),
            Some(ca),
        )
    } else {
        (None, None)
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = if tls {
        format!(
            "https://localhost:{}/saved",
            listener.local_addr().unwrap().port()
        )
    } else {
        format!("http://{}/saved", listener.local_addr().unwrap())
    };
    let state = Arc::new(State {
        requests: Mutex::new(vec![]),
        entered: Notify::new(),
        release: Semaphore::new(if blocked { 0 } else { 16 }),
        rejected_tls: AtomicUsize::new(0),
        responses: Mutex::new((200, alerts(), rules())),
    });
    let observed = state.clone();
    let task = tokio::spawn(async move {
        let mut children = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                accepted=listener.accept()=>{
                    let Ok((socket,_))=accepted else {break};
                    let acceptor=acceptor.clone();let state=observed.clone();
                    children.spawn(async move {
                        let mut stream:Box<dyn Io>=if let Some(acceptor)=acceptor {
                            match acceptor.accept(socket).await {Ok(stream)=>Box::new(stream),Err(_)=>{state.rejected_tls.fetch_add(1,Ordering::SeqCst);return;}}
                        }else{Box::new(socket)};
                        let mut raw=vec![];
                        loop {let mut part=[0;1024];let Ok(n)=stream.read(&mut part).await else {return};if n==0{return}
                            raw.extend_from_slice(&part[..n]);assert!(raw.len()<=16*1024);
                            if raw.windows(4).any(|bytes|bytes==b"\r\n\r\n"){break}
                        }
                        let headers=String::from_utf8(raw).unwrap();
                        let target=headers.lines().next().unwrap().to_owned();
                        let authorization=headers.lines().find_map(|line|{let(name,value)=line.split_once(':')?;
                            name.eq_ignore_ascii_case("authorization").then(||value.trim().to_owned())}).unwrap_or_default();
                        state.requests.lock().unwrap().push(Request{target:target.clone(),authorization});state.entered.notify_one();
                        let permit=state.release.acquire().await.unwrap();permit.forget();
                        let (status,body)={let responses=state.responses.lock().unwrap();
                            (responses.0,if target.contains("/rules?"){responses.2.clone()}else{responses.1.clone()})};
                        let body=body.to_string();let response=format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                        let _=stream.write_all(response.as_bytes()).await;let _=stream.shutdown().await;
                    });
                },
                Some(completed)=children.join_next(),if !children.is_empty()=>{completed.unwrap();}
            }
        }
    });
    Fixture {
        endpoint,
        ca,
        state,
        task,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn saved_alerts_and_rules_use_fixed_gets_and_keep_partial_states_without_url_following() {
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    app.create_local_profile("Own alerts".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let fixture = fixture(false, false).await;
    let catalog = app
        .monitoring_source_create(0, source(&fixture))
        .await
        .unwrap();
    let id = &catalog.sources[0].id;
    let result = app
        .monitoring_read(request(id, 1, Uuid::new_v4(), "alerts"))
        .await
        .unwrap();
    assert_eq!(result["sourceId"], *id);
    assert_eq!(result["sourceRevision"], 1);
    assert_eq!(result["result"]["partial"], true);
    assert_eq!(result["result"]["warnings"], json!(["one store missing"]));
    assert_eq!(
        result["result"]["data"]["alerts"][0]["state"]["kind"],
        "pending"
    );
    assert_eq!(
        result["result"]["data"]["alerts"][0]["value"]["kind"],
        "nan"
    );
    let result = app
        .monitoring_read(request(id, 1, Uuid::new_v4(), "alertRules"))
        .await
        .unwrap();
    assert_eq!(result["result"]["partial"], true);
    assert_eq!(
        result["result"]["data"]["groups"][0]["rules"][0]["state"]["kind"],
        "inactive"
    );
    assert_eq!(
        result["result"]["data"]["groupNextToken"],
        "http://never-follow.invalid"
    );
    let requests = fixture.state.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].target, "GET /saved/api/v1/alerts HTTP/1.1");
    assert_eq!(
        requests[1].target,
        "GET /saved/api/v1/rules?type=alert&group_limit=101 HTTP/1.1"
    );
    drop(requests);
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn api_error_partial_data_and_unknown_states_are_not_empty_success_or_zero() {
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    app.create_local_profile("Own partial alerts".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let fixture = fixture(false, false).await;
    fixture.state.responses.lock().unwrap().0 = 503;
    fixture.state.responses.lock().unwrap().1 = json!({"status":"error","errorType":"timeout","error":"shard unavailable",
        "warnings":["partial backend"],"data":{"alerts":[alert("future-state")]}});
    let catalog = app
        .monitoring_source_create(0, source(&fixture))
        .await
        .unwrap();
    let result = app
        .monitoring_read(request(&catalog.sources[0].id, 1, Uuid::new_v4(), "alerts"))
        .await
        .unwrap();
    assert_eq!(result["result"]["httpStatus"], 503);
    assert_eq!(result["result"]["status"], "error");
    assert_eq!(result["result"]["partial"], true);
    assert_eq!(result["result"]["error"]["type"], "timeout");
    assert_eq!(
        result["result"]["data"]["alerts"][0]["state"],
        json!({"kind":"unknown","value":"future-state"})
    );
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn alert_actions_reject_unknown_urls_filters_cursors_duplicates_and_stale_revision_before_http(
) {
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    app.create_local_profile("Own strict alerts".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let fixture = fixture(false, false).await;
    let catalog = app
        .monitoring_source_create(0, source(&fixture))
        .await
        .unwrap();
    let id = &catalog.sources[0].id;
    for kind in ["alerts", "alertRules"] {
        let raw = request(id, 1, Uuid::new_v4(), kind);
        for key in [
            "url",
            "endpoint",
            "expression",
            "filter",
            "cursor",
            "groupNextToken",
            "bindingId",
            "authorization",
        ] {
            let mut input: Value = serde_json::from_str(&raw).unwrap();
            input["action"][key] = json!("http://never-fetch.invalid");
            assert!(app.monitoring_read(input.to_string()).await.is_err());
        }
        let duplicate = raw.replacen(
            "\"sourceRevision\":1",
            "\"sourceRevision\":1,\"sourceRevision\":1",
            1,
        );
        assert!(app.monitoring_read(duplicate).await.is_err());
        assert!(app
            .monitoring_read(request(id, 0, Uuid::new_v4(), kind))
            .await
            .is_err());
    }
    assert_eq!(fixture.count(), 0);
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn four_source_requests_are_bounded_and_explicit_cancel_finishes_without_backend_release() {
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    app.create_local_profile("Own bounded alerts".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let fixture = fixture(false, true).await;
    let catalog = app
        .monitoring_source_create(0, source(&fixture))
        .await
        .unwrap();
    let id = catalog.sources[0].id.clone();
    let mut tasks = vec![];
    for _ in 0..4 {
        let operation = Uuid::new_v4();
        let input = request(&id, 1, operation, "alerts");
        let reader = app.clone();
        tasks.push((
            operation,
            tokio::spawn(async move { reader.monitoring_read(input).await }),
        ));
    }
    fixture.wait(4).await;
    assert!(app
        .monitoring_read(request(&id, 1, Uuid::new_v4(), "alertRules"))
        .await
        .is_err());
    for (operation, task) in tasks {
        app.monitoring_cancel(operation.to_string()).await.unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .is_err());
    }
    assert_eq!(fixture.count(), 4);
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn in_flight_alerts_never_return_after_source_lock_or_profile_change() {
    for transition in ["source", "lock", "profile"] {
        let directory = tempfile::tempdir().unwrap();
        let app = app(directory.path());
        app.create_local_profile("Own fenced alerts".into(), PASSPHRASE.into())
            .await
            .unwrap();
        let fixture = fixture(false, true).await;
        let catalog = app
            .monitoring_source_create(0, source(&fixture))
            .await
            .unwrap();
        let id = catalog.sources[0].id.clone();
        let input = request(&id, 1, Uuid::new_v4(), "alertRules");
        let reader = app.clone();
        let task = tokio::spawn(async move { reader.monitoring_read(input).await });
        fixture.wait(1).await;
        match transition {
            "source" => {
                let mut next = catalog.sources[0].configuration.clone().unwrap();
                next.name = "Changed generation".into();
                app.monitoring_source_update(id, 1, 1, next).await.unwrap();
            }
            "lock" => app.lock().await.unwrap(),
            _ => {
                app.create_local_profile("Own switched profile".into(), PASSPHRASE.into())
                    .await
                    .unwrap();
            }
        }
        assert!(tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        assert_eq!(fixture.count(), 1);
        app.shutdown().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn alerts_reuse_native_bearer_grant_strict_tls_and_remove_credential_echoes() {
    const SECRET: &str = "synthetic-alert-bearer-123-456";
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    app.create_local_profile("Own authenticated alerts".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let credential = app
        .add_password_credential("Own metadata".into(), None, SECRET.into())
        .await
        .unwrap();
    let fixture = fixture(true, false).await;
    fixture.state.responses.lock().unwrap().1 = json!({"status":"success","warnings":[format!("echo Bearer {SECRET}")],
        "data":{"alerts":[{"labels":{"alertname":SECRET},"annotations":{"summary":SECRET},"state":"firing","value":"1"}]}});
    let mut config = source(&fixture);
    config.auth = AuthRef::BearerRef {
        credential_id: credential.id,
    };
    let catalog = app.monitoring_source_create(0, config).await.unwrap();
    let result = app
        .monitoring_read(request(&catalog.sources[0].id, 1, Uuid::new_v4(), "alerts"))
        .await
        .unwrap();
    assert_eq!(result["result"]["status"], "success");
    assert!(!result.to_string().contains(SECRET));
    assert!(result.to_string().contains("[REDACTED]"));
    assert!(fixture.state.requests.lock().unwrap()[0].authorization == format!("Bearer {SECRET}"));
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn untrusted_tls_sends_no_alert_credentials_and_credential_rotation_cancels_waiting_response()
{
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    app.create_local_profile("Own TLS alert denial".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let credential = app
        .add_password_credential(
            "Own bearer".into(),
            None,
            "synthetic-old-alert-password".into(),
        )
        .await
        .unwrap();
    let fixture = fixture(true, true).await;
    let mut config = source(&fixture);
    config.auth = AuthRef::BearerRef {
        credential_id: credential.id.clone(),
    };
    config.tls.custom_ca_pem = None;
    let catalog = app.monitoring_source_create(0, config).await.unwrap();
    let id = catalog.sources[0].id.clone();
    let result = app
        .monitoring_read(request(&id, 1, Uuid::new_v4(), "alerts"))
        .await
        .unwrap();
    assert_eq!(result["result"]["error"]["code"], "transport");
    assert_eq!(fixture.count(), 0);
    let mut config = source(&fixture);
    config.id = id.clone();
    config.auth = AuthRef::BearerRef {
        credential_id: credential.id.clone(),
    };
    app.monitoring_source_update(id.clone(), 1, 1, config)
        .await
        .unwrap();
    let input = request(&id, 2, Uuid::new_v4(), "alertRules");
    let reader = app.clone();
    let task = tokio::spawn(async move { reader.monitoring_read(input).await });
    fixture.wait(1).await;
    app.set_credential_password(credential.id, "synthetic-new-alert-password".into())
        .await
        .unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn backend_byte_record_and_warning_budgets_return_typed_errors_without_truncation() {
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    app.create_local_profile("Own alert budgets".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let fixture = fixture(false, false).await;
    let catalog = app
        .monitoring_source_create(0, source(&fixture))
        .await
        .unwrap();
    for (body, code) in [
        (
            json!({"status":"success","data":{"alerts":vec![alert("pending");1001]}}),
            "budget_exceeded",
        ),
        (
            json!({"status":"success","warnings":vec!["warning";101],"data":{"alerts":[]}}),
            "budget_exceeded",
        ),
        (
            json!({"status":"success","padding":"x".repeat(2*1024*1024),"data":{"alerts":[]}}),
            "response_too_large",
        ),
    ] {
        fixture.state.responses.lock().unwrap().1 = body;
        let result = app
            .monitoring_read(request(&catalog.sources[0].id, 1, Uuid::new_v4(), "alerts"))
            .await
            .unwrap();
        assert_eq!(result["result"]["error"]["code"], code);
        assert!(result["result"].get("data").is_none());
    }
    app.shutdown().await.unwrap();
}
