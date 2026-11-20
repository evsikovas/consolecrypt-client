use super::*;
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::State,
    http::{Request, Response, StatusCode},
};
use serde_json::json;
use std::sync::{
    Mutex,
    atomic::{AtomicU8, Ordering},
};
use tokio::{net::TcpListener, sync::Notify, task::JoinHandle};

#[derive(Default)]
struct Mock {
    mode: AtomicU8,
    generation: AtomicU8,
    requests: Mutex<Vec<(String, String, Option<Value>, bool)>>,
    entered: Notify,
    release: Notify,
}
struct Fixture {
    state: Arc<Mock>,
    base: String,
    task: JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn new() -> Self {
        let state = Arc::new(Mock::default());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new().fallback(handler).with_state(state.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self { state, base, task }
    }
    fn client(&self) -> AwxClient {
        AwxClient::construct(
            &self.base,
            None,
            token(),
            Duration::from_secs(300),
            CancellationToken::new(),
            true,
        )
        .unwrap()
    }
    fn posts(&self) -> usize {
        self.state
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.0 == "POST")
            .count()
    }
}
fn token() -> AwxAuthorization {
    AwxAuthorization::Bearer(Zeroizing::new("owned-awx-test-token".into()))
}
fn parameters() -> AwxLaunchParameters {
    AwxLaunchParameters {
        job_type: Some("check".into()),
        limit: Some("owned-group".into()),
        ..Default::default()
    }
}
fn template_body(generation: u8) -> Value {
    json!({"id":7,"name":"Owned existing template","modified":format!("2026-11-12T12:00:{generation:02}Z"),
        "credentials":[{"id":9,"secret":"OWNED_TEMPLATE_DATA_NOT_PROJECTED"}],"extra_vars":"OWNED_DEFAULT_NOT_PROJECTED"})
}
fn requirements_body(mode: u8) -> Value {
    json!({"passwords_needed_to_start":if mode==6 {json!(["ssh_password"])} else {json!([])},
        "variables_needed_to_start":[],"credential_needed_to_start":false,"inventory_needed_to_start":false,
        "ask_job_type_on_launch":true,"ask_limit_on_launch":true,"ask_tags_on_launch":false,
        "ask_skip_tags_on_launch":false,"ask_variables_on_launch":false,
        "defaults":{"credentials":[{"password":"OWNED_DEFAULT_NOT_PROJECTED"}]}})
}
fn job_body() -> Value {
    json!({"job":31,"id":31,"name":"Owned job","status":"pending","started":null,"finished":null,"ignored_fields":{}})
}
fn response(status: u16, value: Value) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(&value).unwrap()))
        .unwrap()
}
async fn handler(State(state): State<Arc<Mock>>, request: Request<Body>) -> Response<Body> {
    let method = request.method().to_string();
    let uri = request.uri().to_string();
    let path = request.uri().path().to_owned();
    let authorized = request
        .headers()
        .get("authorization")
        .is_some_and(|v| v == "Bearer owned-awx-test-token");
    let bytes = to_bytes(request.into_body(), REQUEST_LIMIT + 1)
        .await
        .unwrap();
    let body = if bytes.is_empty() {
        None
    } else {
        Some(serde_json::from_slice(&bytes).unwrap())
    };
    state
        .requests
        .lock()
        .unwrap()
        .push((method.clone(), uri, body, authorized));
    let mode = state.mode.load(Ordering::SeqCst);
    if mode == 11 {
        state.entered.notify_one();
        state.release.notified().await;
    }
    if mode == 1 {
        return Response::builder()
            .status(StatusCode::FOUND)
            .header("Location", "http://127.0.0.1:1/must-not-follow")
            .body(Body::empty())
            .unwrap();
    }
    if path.ends_with("/job_templates/") {
        if mode == 9 {
            let chunks: Vec<Result<_, std::io::Error>> =
                vec![Ok(vec![b' '; RESPONSE_LIMIT]), Ok(vec![b' '; 1])];
            return Response::builder()
                .header("Content-Type", "application/json")
                .body(Body::from_stream(futures::stream::iter(chunks)))
                .unwrap();
        }
        if mode == 10 {
            return Response::new(Body::from(r#"{"next":null,"results":[],"results":[]}"#));
        }
        return response(
            200,
            json!({"count":999,"next":"https://unrelated.invalid/credentials/","results":[template_body(state.generation.load(Ordering::SeqCst))]}),
        );
    }
    if path.ends_with("/job_templates/7/") {
        return response(200, template_body(state.generation.load(Ordering::SeqCst)));
    }
    if path.ends_with("/job_templates/7/launch/") {
        if method == "GET" {
            return response(200, requirements_body(mode));
        }
        if method != "POST" {
            return response(405, json!({}));
        }
        if mode == 2 {
            return response(403, json!({"detail":"OWNED_SERVER_SECRET_MUST_NOT_ESCAPE"}));
        }
        if mode == 3 {
            return response(500, json!({"error":"unknown outcome"}));
        }
        if mode == 4 {
            return Response::new(Body::from("not valid JSON after possible start"));
        }
        return response(201, job_body());
    }
    if path.ends_with("/jobs/31/job_events/") {
        return response(
            200,
            json!({"next":null,"results":[
            {"counter":2,"event":"runner_on_ok","created":"2026-11-12T12:10:00Z","job":if mode==7{99}else{31},"stdout":"owned\u{001b}[31m\u{202e}event\n"},
            {"counter":if mode==8{1}else{3},"event":"playbook_on_stats","created":null,"job":31,"stdout":"done"}]}),
        );
    }
    if path.ends_with("/jobs/31/") {
        return response(200, job_body());
    }
    response(404, json!({}))
}

#[test]
fn configuration_requires_https_and_excludes_ambient_credentials_and_authority() {
    for endpoint in [
        "http://localhost:8080",
        "https://user:pass@awx.invalid",
        "https://awx.invalid?a=b",
        "https://awx.invalid#frag",
        "file:///etc/passwd",
    ] {
        assert!(matches!(
            AwxClient::new(
                endpoint,
                None,
                token(),
                Duration::from_secs(60),
                CancellationToken::new()
            ),
            Err(AwxError::Configuration)
        ));
    }
    assert!(
        AwxClient::new(
            "https://awx.invalid/prefix",
            None,
            token(),
            Duration::from_secs(60),
            CancellationToken::new()
        )
        .is_ok()
    );
    assert!(
        AwxClient::new(
            "https://awx.invalid",
            Some("-----BEGIN PRIVATE KEY-----\nowned\n-----END PRIVATE KEY-----"),
            token(),
            Duration::from_secs(60),
            CancellationToken::new()
        )
        .is_err()
    );
    assert!(!format!("{:?}", token()).contains("owned-awx-test-token"));
    let forbidden = json!({"limit":"all","credentials":[1]});
    assert!(serde_json::from_value::<AwxLaunchParameters>(forbidden).is_err());
}

#[test]
fn duplicate_json_nested_keys_trailing_objects_depth_and_budget_reject() {
    for raw in [
        r#"{"id":1,"id":2}"#,
        r#"{"nested":{"id":1,"id":2}}"#,
        r#"{}{}"#,
        r#"[]"#,
    ] {
        assert_eq!(
            strict_json::parse(raw.as_bytes()).unwrap_err(),
            AwxError::Response
        );
    }
    assert!(
        strict_json::parse(format!("{}null{}", "[".repeat(140), "]".repeat(140)).as_bytes())
            .is_err()
    );
    assert_eq!(
        strict_json::parse(&vec![b' '; RESPONSE_LIMIT + 1]).unwrap_err(),
        AwxError::Budget
    );
}

#[tokio::test]
async fn template_listing_uses_fixed_route_encoded_search_no_next_follow_and_minimal_projection() {
    let fixture = Fixture::new().await;
    let client = fixture.client();
    let page = client
        .templates(2, "name &page=99? /credentials")
        .await
        .unwrap();
    assert_eq!(page.templates.len(), 1);
    assert!(page.has_next);
    assert_eq!(page.page, 2);
    let projected = serde_json::to_string(&page).unwrap();
    assert!(!projected.contains("OWNED_TEMPLATE_DATA"));
    assert!(!projected.contains("unrelated.invalid"));
    let requests = fixture.state.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].0, "GET");
    assert!(requests[0].3);
    let url = Url::parse(&format!("{}{}", fixture.base, requests[0].1)).unwrap();
    assert_eq!(url.path(), "/api/v2/job_templates/");
    let pairs: Vec<_> = url.query_pairs().collect();
    assert_eq!(pairs.len(), 3);
    assert!(
        pairs
            .iter()
            .any(|(k, v)| k == "search" && v == "name &page=99? /credentials")
    );
}

#[tokio::test]
async fn invalid_parameters_reject_before_http_or_credential_use() {
    let fixture = Fixture::new().await;
    let client = fixture.client();
    assert_eq!(
        client.templates(0, "").await.unwrap_err(),
        AwxError::InvalidInput
    );
    assert_eq!(
        client.job(MAX_ID + 1).await.unwrap_err(),
        AwxError::InvalidInput
    );
    let invalid = AwxLaunchParameters {
        extra_vars: Some(json!("not an object")),
        ..Default::default()
    };
    assert!(matches!(
        client.prepare_launch(7, invalid).await,
        Err(AwxError::InvalidInput)
    ));
    assert!(fixture.state.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn redirects_denied_without_a_second_request_and_http_body_not_projected() {
    let fixture = Fixture::new().await;
    fixture.state.mode.store(1, Ordering::SeqCst);
    assert_eq!(
        fixture.client().templates(1, "").await.unwrap_err(),
        AwxError::Http(302)
    );
    assert_eq!(fixture.state.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn streaming_response_budget_and_duplicate_response_fail_closed() {
    let fixture = Fixture::new().await;
    fixture.state.mode.store(9, Ordering::SeqCst);
    assert_eq!(
        fixture.client().templates(1, "").await.unwrap_err(),
        AwxError::Budget
    );
    fixture.state.mode.store(10, Ordering::SeqCst);
    assert_eq!(
        fixture.client().templates(1, "").await.unwrap_err(),
        AwxError::Response
    );
}

#[tokio::test]
async fn job_events_bind_job_and_monotonic_cursor_sanitize_plain_text() {
    let fixture = Fixture::new().await;
    let client = fixture.client();
    let page = client.events(31, 1).await.unwrap();
    assert_eq!(page.after, 3);
    assert_eq!(page.events.len(), 2);
    assert!(!page.events[0].stdout.contains('\u{001b}'));
    assert!(!page.events[0].stdout.contains('\u{202e}'));
    assert!(page.events[0].stdout.contains('\n'));
    fixture.state.mode.store(7, Ordering::SeqCst);
    assert_eq!(client.events(31, 1).await.unwrap_err(), AwxError::Response);
    fixture.state.mode.store(8, Ordering::SeqCst);
    assert_eq!(client.events(31, 1).await.unwrap_err(), AwxError::Response);
    assert!(
        client
            .original_job_url(31)
            .unwrap()
            .ends_with("/api/v2/jobs/31/")
    );
}

#[tokio::test]
async fn review_performs_only_reads_and_explicit_launch_sends_exact_parameters_once() {
    let fixture = Fixture::new().await;
    let client = fixture.client();
    let review = client.prepare_launch(7, parameters()).await.unwrap();
    assert_eq!(fixture.posts(), 0);
    assert_eq!(review.template().id, 7);
    assert_eq!(review.parameters(), &parameters());
    let outcome = client.launch(review).await.unwrap();
    match outcome {
        AwxLaunchOutcome::Started {
            job,
            parameters_ignored,
        } => {
            assert_eq!(job.id, 31);
            assert!(!parameters_ignored);
        }
        _ => panic!("Expected acknowledged owned job"),
    }
    assert_eq!(fixture.posts(), 1);
    let requests = fixture.state.requests.lock().unwrap();
    let post = requests.iter().find(|r| r.0 == "POST").unwrap();
    assert_eq!(post.1, "/api/v2/job_templates/7/launch/");
    assert_eq!(
        post.2.as_ref().unwrap(),
        &json!({"job_type":"check","limit":"owned-group"})
    );
    assert!(requests.iter().all(|r| r.3));
}

#[tokio::test]
async fn cross_client_review_and_changed_template_never_post() {
    let fixture = Fixture::new().await;
    let client = fixture.client();
    let review = client.prepare_launch(7, parameters()).await.unwrap();
    assert!(matches!(
        fixture.client().launch(review).await,
        Err(AwxError::Expired)
    ));
    let review = client.prepare_launch(7, parameters()).await.unwrap();
    fixture.state.generation.store(1, Ordering::SeqCst);
    assert!(matches!(
        client.launch(review).await,
        Err(AwxError::ChangedTemplate)
    ));
    assert_eq!(fixture.posts(), 0);
}

#[tokio::test]
async fn explicit_expiry_and_cancellation_before_dispatch_never_post_or_rebind() {
    let fixture = Fixture::new().await;
    let client = fixture.client();
    let mut review = client.prepare_launch(7, parameters()).await.unwrap();
    review.expires = Instant::now();
    assert!(matches!(
        client.launch(review).await,
        Err(AwxError::Expired)
    ));
    client.cancel.cancel();
    assert_eq!(
        client.templates(1, "").await.unwrap_err(),
        AwxError::Cancelled
    );
    assert_eq!(fixture.posts(), 0);
}

#[tokio::test]
async fn pending_read_cancellation_discards_response_without_followup() {
    let fixture = Fixture::new().await;
    let client = fixture.client();
    fixture.state.mode.store(11, Ordering::SeqCst);
    let entered = fixture.state.entered.notified();
    let pending = client.templates(1, "");
    tokio::pin!(pending);
    tokio::select! {_ = entered =>{},result=&mut pending=>panic!("Read unexpectedly finished: {result:?}")};
    client.cancel.cancel();
    assert_eq!(pending.await.unwrap_err(), AwxError::Cancelled);
    fixture.state.release.notify_waiters();
    assert_eq!(fixture.state.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn required_awx_credentials_or_unaccepted_parameter_prompts_refuse_before_post() {
    let fixture = Fixture::new().await;
    let client = fixture.client();
    fixture.state.mode.store(6, Ordering::SeqCst);
    assert!(matches!(
        client.prepare_launch(7, parameters()).await,
        Err(AwxError::NeedsAwxInput)
    ));
    fixture.state.mode.store(0, Ordering::SeqCst);
    let p = AwxLaunchParameters {
        extra_vars: Some(json!({"owned":"value"})),
        ..Default::default()
    };
    assert!(matches!(
        client.prepare_launch(7, p).await,
        Err(AwxError::UnsupportedPrompts)
    ));
    assert_eq!(fixture.posts(), 0);
}

#[tokio::test]
async fn awx_rbac_rejection_is_not_retried_and_does_not_return_server_text() {
    let fixture = Fixture::new().await;
    let client = fixture.client();
    let review = client.prepare_launch(7, parameters()).await.unwrap();
    fixture.state.mode.store(2, Ordering::SeqCst);
    let error = client.launch(review).await.unwrap_err();
    assert_eq!(error, AwxError::Http(403));
    assert!(!format!("{error:?}").contains("OWNED_SERVER_SECRET"));
    assert_eq!(fixture.posts(), 1);
}

#[tokio::test]
async fn ambiguous_post_or_malformed_ack_is_unknown_and_no_automatic_retry_occurs() {
    let fixture = Fixture::new().await;
    let client = fixture.client();
    for mode in [3, 4] {
        fixture.state.mode.store(0, Ordering::SeqCst);
        let review = client.prepare_launch(7, parameters()).await.unwrap();
        fixture.state.mode.store(mode, Ordering::SeqCst);
        assert!(matches!(
            client.launch(review).await.unwrap(),
            AwxLaunchOutcome::Unknown
        ));
        assert_eq!(fixture.posts(), if mode == 3 { 1 } else { 2 });
    }
}

#[tokio::test]
async fn fresh_secret_resumption_uses_only_sealed_endpoint_review_and_original_expiry() {
    let fixture = Fixture::new().await;
    let first = fixture.client();
    let review = first.prepare_launch(7, parameters()).await.unwrap();
    let original_expiry = review.expires;
    assert_eq!(fixture.posts(), 0);
    drop(first); // No token-bearing client is needed during human review.
    let resumed = AwxClient::resume_review(&review, token(), CancellationToken::new()).unwrap();
    assert_eq!(resumed.base.as_str(), format!("{}/", fixture.base));
    assert_eq!(resumed.expires, original_expiry);
    assert!(review.remaining() <= REVIEW_TTL);
    assert!(matches!(
        resumed.launch(review).await.unwrap(),
        AwxLaunchOutcome::Started { .. }
    ));
    assert_eq!(fixture.posts(), 1);
    assert!(fixture.state.requests.lock().unwrap().iter().all(|r| r.3));
}

#[tokio::test]
async fn sealed_review_resumption_cannot_extend_expiry_or_ignore_original_cancel() {
    let fixture = Fixture::new().await;
    let first = fixture.client();
    let mut review = first.prepare_launch(7, parameters()).await.unwrap();
    review.expires = Instant::now();
    assert!(matches!(
        AwxClient::resume_review(&review, token(), CancellationToken::new()),
        Err(AwxError::Expired)
    ));
    let review = first.prepare_launch(7, parameters()).await.unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(matches!(
        AwxClient::resume_review(&review, token(), cancel),
        Err(AwxError::Cancelled)
    ));
    assert_eq!(fixture.posts(), 0);
}
