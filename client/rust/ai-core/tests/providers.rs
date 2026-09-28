//! Provider tests against in-process mock HTTP servers (axum).

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use cc_ai_core::provider::{
    build_provider, ChatEvent, ChatRequest, EmbeddingRequest, Message, ResponseFormat,
    ToolDefinition,
};
use cc_ai_core::sanitizer::{sanitize, PrivacyProfile, SanitizedText};
use cc_ai_core::{with_cancellation, AiError, CancellationToken};
use cc_models::ai::{AiProviderConfig, AiProviderKind};
use cc_models::ObjectId;
use futures::StreamExt;
use secrecy::SecretString;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// (path, authorization header, JSON body)
type Request = (String, Option<String>, Value);

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<Request>>>);

impl Captured {
    fn push(&self, path: &str, headers: &HeaderMap, body: Value) {
        let auth = headers
            .get("authorization")
            .map(|v| v.to_str().unwrap_or("").to_owned());
        self.0.lock().unwrap().push((path.to_owned(), auth, body));
    }
    fn all(&self) -> Vec<Request> {
        self.0.lock().unwrap().clone()
    }
}

async fn spawn(router: Router) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    addr
}

fn config(kind: AiProviderKind, base: String) -> AiProviderConfig {
    let now = chrono::Utc::now();
    AiProviderConfig {
        id: ObjectId::new(),
        name: "test".into(),
        provider: kind,
        base_url: base,
        api_key_secret_id: None,
        chat_model: "test-model".into(),
        embedding_model: Some("embed-model".into()),
        timeout_secs: 5,
        streaming: true,
        tool_support: true,
        privacy_profile: PrivacyProfile::Standard,
        is_default: false,
        created_at: now,
        updated_at: now,
    }
}

fn text(s: &'static str) -> SanitizedText {
    SanitizedText::from_static(s)
}

fn chunked(chunks: Vec<String>, delay_ms: u64) -> Body {
    let stream = futures::stream::unfold(chunks.into_iter(), move |mut it| async move {
        let next = it.next()?;
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        Some((Ok::<Bytes, Infallible>(Bytes::from(next)), it))
    });
    Body::from_stream(stream)
}

// ---------------------------------------------------------------------------
// OpenAI-compatible
// ---------------------------------------------------------------------------

async fn oa_chat(
    State(cap): State<Captured>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    cap.push("/v1/chat/completions", &headers, body.clone());
    let model = body["model"].as_str().unwrap_or_default().to_owned();
    if model == "missing" {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "The model `missing` does not exist"}})),
        )
            .into_response();
    }
    if model == "limited" {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [("retry-after", "7")],
            Json(json!({"error": {"message": "slow down"}})),
        )
            .into_response();
    }
    if model == "boom" {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "upstream exploded; key sk-abcdefghijklmnopqrstuvwxyz0123456789",
        )
            .into_response();
    }
    if model == "slow" {
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    if body["stream"] == json!(true) {
        let mut chunks = vec![
            ": keep-alive\n\n".to_owned(),
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n".to_owned(),
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n".to_owned(),
            // split an event across TCP chunks
            "data: {\"choices\":[{\"delta\":{\"con".to_owned(),
            "tent\":\"lo \u{00e9}\"}}]}\r\n\r\n".to_owned(),
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"run\",\"arguments\":\"{\\\"cmd\\\":\"}}]}}]}\n\n".to_owned(),
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"ls\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n".to_owned(),
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":4}}\n\n".to_owned(),
            "data: [DONE]\n\n".to_owned(),
        ];
        if model == "stream-error" {
            chunks.truncate(3);
            chunks
                .push("data: {\"error\":{\"message\":\"context length exceeded\"}}\n\n".to_owned());
        }
        return Response::builder()
            .header("content-type", "text/event-stream")
            .body(chunked(chunks, 5))
            .unwrap();
    }
    Json(json!({
        "id": "x", "model": model,
        "choices": [{"index": 0, "message": {"role": "assistant", "content": "pong",
            "tool_calls": [{"id": "t1", "type": "function", "function": {"name": "lookup", "arguments": "{\"q\":1}"}}]},
            "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 5, "completion_tokens": 1}
    }))
    .into_response()
}

async fn oa_embed(
    State(cap): State<Captured>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    cap.push("/v1/embeddings", &headers, body.clone());
    let n = body["input"].as_array().map_or(0, Vec::len);
    // Return out of order to test index sorting.
    let data: Vec<Value> = (0..n)
        .rev()
        .map(|i| json!({"object": "embedding", "index": i, "embedding": [i as f32, 1.0, 0.5]}))
        .collect();
    Json(json!({"data": data, "model": "embed-model"}))
}

async fn oa_models(State(cap): State<Captured>, headers: HeaderMap) -> Json<Value> {
    cap.push("/v1/models", &headers, Value::Null);
    Json(json!({"data": [{"id": "m1", "owned_by": "me"}, {"id": "m2"}]}))
}

async fn oa_responses(
    State(cap): State<Captured>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    cap.push("/v1/responses", &headers, body);
    Json(json!({
        "model": "test-model", "status": "completed",
        "output": [
            {"type": "reasoning", "summary": []},
            {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "from responses"}]},
            {"type": "function_call", "call_id": "fc1", "name": "f", "arguments": "{\"a\":true}"}
        ],
        "usage": {"input_tokens": 2, "output_tokens": 3}
    }))
}

async fn openai_server() -> (SocketAddr, Captured) {
    let cap = Captured::default();
    let app = Router::new()
        .route("/v1/chat/completions", post(oa_chat))
        .route("/v1/embeddings", post(oa_embed))
        .route("/v1/models", get(oa_models))
        .route("/v1/responses", post(oa_responses))
        .with_state(cap.clone());
    (spawn(app).await, cap)
}

fn key() -> Option<SecretString> {
    Some(SecretString::from(format!(
        "{}{}",
        "test-", "key-value-123"
    )))
}

#[tokio::test]
async fn openai_chat_models_embeddings_and_auth_header() {
    let (addr, cap) = openai_server().await;
    let p = build_provider(
        &config(AiProviderKind::OpenaiCompatible, format!("http://{addr}")),
        key(),
    )
    .unwrap();
    assert!(!format!("{p:?}").contains("key-value-123"), "{p:?}");

    let req = ChatRequest {
        messages: vec![Message::system(text("sys")), Message::user(text("ping"))],
        temperature: Some(0.1),
        max_tokens: Some(20),
        response_format: ResponseFormat::Json,
        tools: vec![ToolDefinition {
            name: "lookup".into(),
            description: "d".into(),
            parameters: json!({"type": "object"}),
        }],
        model: None,
    };
    let r = p.chat(&req).await.unwrap();
    assert_eq!(r.content, "pong");
    assert_eq!(r.tool_calls[0].name, "lookup");
    assert_eq!(r.tool_calls[0].arguments["q"], 1);
    assert_eq!(r.usage.unwrap().prompt_tokens, Some(5));

    let models = p.list_models().await.unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].owned_by.as_deref(), Some("me"));

    let e = p
        .embed(&EmbeddingRequest {
            inputs: vec![text("a"), text("b"), text("c")],
            model: None,
        })
        .await
        .unwrap();
    assert_eq!(e.vectors.len(), 3);
    assert_eq!(e.vectors[2][0], 2.0, "sorted by index");
    assert_eq!(e.dim(), 3);

    let calls = cap.all();
    let (path, auth, body) = &calls[0];
    assert_eq!(path, "/v1/chat/completions");
    assert_eq!(auth.as_deref(), Some("Bearer test-key-value-123"));
    assert_eq!(body["model"], "test-model");
    assert_eq!(body["messages"][1]["content"], "ping");
    assert_eq!(body["max_tokens"], 20);
    assert_eq!(body["response_format"]["type"], "json_object");
    assert_eq!(body["tools"][0]["function"]["name"], "lookup");
    assert!(calls
        .iter()
        .all(|c| c.1.as_deref() == Some("Bearer test-key-value-123")));
}

#[tokio::test]
async fn openai_sse_streaming_with_tool_calls_and_usage() {
    let (addr, _) = openai_server().await;
    let p = build_provider(
        &config(AiProviderKind::LmStudio, format!("http://{addr}/v1")),
        None,
    )
    .unwrap();
    let mut s = p
        .chat_stream(&ChatRequest::new(vec![Message::user(text("hi"))]))
        .await
        .unwrap();
    let mut content = String::new();
    let mut tool = None;
    let mut done = None;
    while let Some(ev) = s.next().await {
        match ev.unwrap() {
            ChatEvent::Delta(d) => content.push_str(&d),
            ChatEvent::ToolCall(t) => tool = Some(t),
            ChatEvent::Done {
                finish_reason,
                usage,
            } => done = Some((finish_reason, usage)),
        }
    }
    assert_eq!(content, "Hello é");
    let tool = tool.unwrap();
    assert_eq!(tool.id, "c1");
    assert_eq!(tool.arguments["cmd"], "ls");
    let (reason, usage) = done.unwrap();
    assert_eq!(reason.as_deref(), Some("tool_calls"));
    assert_eq!(usage.unwrap().completion_tokens, Some(4));
}

#[tokio::test]
async fn openai_stream_error_event_and_http_errors() {
    let (addr, _) = openai_server().await;
    let base = format!("http://{addr}");
    let mk = |model: &str| {
        let mut c = config(AiProviderKind::OpenaiCompatible, base.clone());
        c.chat_model = model.into();
        build_provider(&c, None).unwrap()
    };
    let mut s = mk("stream-error")
        .chat_stream(&ChatRequest::new(vec![Message::user(text("x"))]))
        .await
        .unwrap();
    let mut saw_error = false;
    while let Some(ev) = s.next().await {
        if let Err(e) = ev {
            assert!(
                matches!(e, AiError::Provider(ref m) if m.contains("context length")),
                "{e:?}"
            );
            saw_error = true;
        }
    }
    assert!(saw_error);

    let req = ChatRequest::new(vec![Message::user(text("x"))]);
    assert!(matches!(
        mk("missing").chat(&req).await,
        Err(AiError::ModelNotFound(_))
    ));
    assert!(matches!(
        mk("limited").chat(&req).await,
        Err(AiError::RateLimited {
            retry_after_secs: Some(7)
        })
    ));
    match mk("boom").chat(&req).await {
        Err(AiError::Http {
            status: 500,
            message,
        }) => {
            assert!(message.contains("upstream exploded"));
            assert!(
                !message.contains("sk-abcdefghijklmnopqrstuvwxyz0123456789"),
                "error bodies are sanitized"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn openai_responses_api() {
    let (addr, cap) = openai_server().await;
    let p = build_provider(
        &config(AiProviderKind::OpenaiCompatible, format!("http://{addr}")),
        None,
    )
    .unwrap();
    assert!(p.capabilities().responses);
    let r = p
        .respond(&ChatRequest {
            messages: vec![Message::system(text("be brief")), Message::user(text("q"))],
            response_format: ResponseFormat::JsonSchema {
                name: "x".into(),
                schema: json!({"type": "object"}),
            },
            max_tokens: Some(9),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(r.content, "from responses");
    assert_eq!(r.tool_calls[0].arguments["a"], true);
    let body = &cap.all()[0].2;
    assert_eq!(body["instructions"], "be brief");
    assert_eq!(body["input"][0]["content"], "q");
    assert_eq!(body["max_output_tokens"], 9);
    assert_eq!(body["text"]["format"]["type"], "json_schema");

    let ds = build_provider(
        &config(AiProviderKind::Deepseek, format!("http://{addr}")),
        None,
    )
    .unwrap();
    assert!(matches!(
        ds.respond(&ChatRequest::default()).await,
        Err(AiError::Unsupported("responses"))
    ));
    assert!(!ds.capabilities().embeddings);
}

#[tokio::test]
async fn structured_output_mapping_per_kind() {
    let (addr, cap) = openai_server().await;
    let schema = ResponseFormat::JsonSchema {
        name: "cmd".into(),
        schema: json!({"type": "object", "properties": {"command": {"type": "string"}}}),
    };
    let req = ChatRequest {
        messages: vec![Message::user(text("x"))],
        response_format: schema,
        ..Default::default()
    };
    let lm = build_provider(
        &config(AiProviderKind::LmStudio, format!("http://{addr}/v1")),
        None,
    )
    .unwrap();
    lm.chat(&req).await.unwrap();
    let ds = build_provider(
        &config(AiProviderKind::Deepseek, format!("http://{addr}")),
        None,
    )
    .unwrap();
    ds.chat(&req).await.unwrap();
    let calls = cap.all();
    assert_eq!(calls[0].2["response_format"]["type"], "json_schema");
    assert_eq!(calls[0].2["response_format"]["json_schema"]["name"], "cmd");
    assert_eq!(calls[1].2["response_format"]["type"], "json_object");
}

#[tokio::test]
async fn timeouts_and_cancellation() {
    let (addr, _) = openai_server().await;
    let mut c = config(AiProviderKind::OpenaiCompatible, format!("http://{addr}"));
    c.chat_model = "slow".into();
    c.timeout_secs = 1;
    let p = build_provider(&c, None).unwrap();
    let req = ChatRequest::new(vec![Message::user(text("x"))]);
    let started = std::time::Instant::now();
    assert!(matches!(p.chat(&req).await, Err(AiError::Timeout)));
    assert!(started.elapsed() < Duration::from_secs(3));

    let token = CancellationToken::new();
    let t2 = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        t2.cancel();
    });
    let started = std::time::Instant::now();
    let r = with_cancellation(&token, p.chat(&req)).await;
    assert!(matches!(r, Err(AiError::Cancelled)));
    assert!(started.elapsed() < Duration::from_millis(900));

    // Connection refused.
    let dead = build_provider(
        &config(
            AiProviderKind::OpenaiCompatible,
            "http://127.0.0.1:1".into(),
        ),
        None,
    )
    .unwrap();
    assert!(matches!(dead.chat(&req).await, Err(AiError::Connect(_))));
}

struct DropFlag(Arc<AtomicBool>);
impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn dropping_a_stream_aborts_the_request() {
    let dropped = Arc::new(AtomicBool::new(false));
    let flag = dropped.clone();
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move || {
            let flag = flag.clone();
            async move {
                let guard = DropFlag(flag);
                let stream = futures::stream::unfold((0u64, guard), |(i, g)| async move {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    let chunk = format!(
                        "data: {{\"choices\":[{{\"delta\":{{\"content\":\"t{i} \"}}}}]}}\n\n"
                    );
                    Some((Ok::<Bytes, Infallible>(Bytes::from(chunk)), (i + 1, g)))
                });
                Response::builder()
                    .header("content-type", "text/event-stream")
                    .body(Body::from_stream(stream))
                    .unwrap()
            }
        }),
    );
    let addr = spawn(app).await;
    let p = build_provider(
        &config(AiProviderKind::OpenaiCompatible, format!("http://{addr}")),
        None,
    )
    .unwrap();
    let mut s = p
        .chat_stream(&ChatRequest::new(vec![Message::user(text("x"))]))
        .await
        .unwrap();
    let first = s.next().await.unwrap().unwrap();
    assert!(matches!(first, ChatEvent::Delta(_)));
    drop(s);
    for _ in 0..50 {
        if dropped.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("server did not observe the client disconnect");
}

#[tokio::test]
async fn cancellable_stream_ends_with_cancelled() {
    let (addr, _) = openai_server().await;
    let p = build_provider(
        &config(AiProviderKind::OpenaiCompatible, format!("http://{addr}")),
        None,
    )
    .unwrap();
    let token = CancellationToken::new();
    token.cancel();
    let s = p
        .chat_stream(&ChatRequest::new(vec![Message::user(text("x"))]))
        .await
        .unwrap();
    let mut s = cc_ai_core::cancellable(s, token);
    assert!(matches!(s.next().await, Some(Err(AiError::Cancelled))));
    assert!(s.next().await.is_none());
}

// ---------------------------------------------------------------------------
// Ollama
// ---------------------------------------------------------------------------

async fn ol_chat(
    State(cap): State<Captured>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    cap.push("/api/chat", &headers, body.clone());
    if body["model"] == "missing" {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "model 'missing' not found, try pulling it first"})),
        )
            .into_response();
    }
    if body["stream"] == json!(false) {
        return Json(json!({
            "model": "test-model", "created_at": "2026-01-01T00:00:00Z",
            "message": {"role": "assistant", "content": "native pong",
                "tool_calls": [{"function": {"name": "get_host", "arguments": {"id": 7}}}]},
            "done": true, "done_reason": "stop", "prompt_eval_count": 11, "eval_count": 2
        }))
        .into_response();
    }
    let mut lines = vec![
        "{\"model\":\"m\",\"message\":{\"role\":\"assistant\",\"content\":\"Stre\"},\"done\":false}\n".to_owned(),
        "{\"model\":\"m\",\"message\":{\"role\":\"assistant\",\"con".to_owned(),
        "tent\":\"amed\"},\"done\":false}\n{\"model\":\"m\",\"message\":{\"role\":\"assistant\",\"content\":\"!\"},\"done\":false}\n".to_owned(),
        "{\"model\":\"m\",\"message\":{\"role\":\"assistant\",\"content\":\"\",\"tool_calls\":[{\"function\":{\"name\":\"t\",\"arguments\":{\"x\":1}}}]},\"done\":false}\n".to_owned(),
        "{\"model\":\"m\",\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\",\"prompt_eval_count\":4,\"eval_count\":3}\n".to_owned(),
    ];
    if body["model"] == "midstream" {
        lines.truncate(1);
        lines.push("{\"error\":\"out of memory\"}\n".to_owned());
    }
    if body["model"] == "truncated" {
        lines.truncate(1);
    }
    Response::builder()
        .header("content-type", "application/x-ndjson")
        .body(chunked(lines, 5))
        .unwrap()
}

async fn ol_embed(
    State(cap): State<Captured>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    cap.push("/api/embed", &headers, body.clone());
    let n = body["input"].as_array().map_or(0, Vec::len);
    Json(
        json!({"model": "embed-model", "embeddings": (0..n).map(|i| vec![i as f32, 2.0]).collect::<Vec<_>>()}),
    )
}

async fn ol_tags() -> Json<Value> {
    Json(
        json!({"models": [{"name": "llama3.2:3b", "size": 2019393189u64}, {"name": "nomic-embed-text:latest", "size": 274302450u64}]}),
    )
}

async fn ollama_server() -> (SocketAddr, Captured) {
    let cap = Captured::default();
    let app = Router::new()
        .route("/api/chat", post(ol_chat))
        .route("/api/embed", post(ol_embed))
        .route("/api/tags", get(ol_tags))
        .with_state(cap.clone());
    (spawn(app).await, cap)
}

#[tokio::test]
async fn ollama_chat_tools_embed_tags() {
    let (addr, cap) = ollama_server().await;
    let p = build_provider(
        &config(AiProviderKind::Ollama, format!("http://{addr}")),
        None,
    )
    .unwrap();
    assert!(p.locality().is_local());
    let req = ChatRequest {
        messages: vec![Message::user(text("hi"))],
        temperature: Some(0.3),
        max_tokens: Some(64),
        response_format: ResponseFormat::JsonSchema {
            name: "x".into(),
            schema: json!({"type": "object", "properties": {"a": {"type": "string"}}}),
        },
        tools: vec![ToolDefinition {
            name: "get_host".into(),
            description: "d".into(),
            parameters: json!({"type": "object"}),
        }],
        model: None,
    };
    let r = p.chat(&req).await.unwrap();
    assert_eq!(r.content, "native pong");
    assert_eq!(r.tool_calls[0].name, "get_host");
    assert_eq!(r.tool_calls[0].arguments["id"], 7);
    assert_eq!(r.usage.unwrap().prompt_tokens, Some(11));

    let body = &cap.all()[0].2;
    assert_eq!(body["stream"], false);
    assert_eq!(
        body["options"]["temperature"]
            .as_f64()
            .map(|t| (t * 10.0).round()),
        Some(3.0)
    );
    assert_eq!(body["options"]["num_predict"], 64);
    assert_eq!(body["format"]["type"], "object");
    assert_eq!(body["tools"][0]["function"]["name"], "get_host");
    assert!(cap.all()[0].1.is_none(), "no auth header without a key");

    let e = p
        .embed(&EmbeddingRequest {
            inputs: vec![text("a"), text("b")],
            model: None,
        })
        .await
        .unwrap();
    assert_eq!(e.vectors, vec![vec![0.0, 2.0], vec![1.0, 2.0]]);
    assert_eq!(cap.all()[1].2["model"], "embed-model");

    let tags = p.list_models().await.unwrap();
    assert_eq!(tags[0].id, "llama3.2:3b");
    assert_eq!(tags[0].size_bytes, Some(2019393189));
}

#[tokio::test]
async fn ollama_ndjson_streaming_and_errors() {
    let (addr, _) = ollama_server().await;
    let base = format!("http://{addr}");
    let mk = |model: &str| {
        let mut c = config(AiProviderKind::Ollama, base.clone());
        c.chat_model = model.into();
        build_provider(&c, None).unwrap()
    };
    let req = ChatRequest::new(vec![Message::user(text("hi"))]);
    let events: Vec<_> = mk("test-model")
        .chat_stream(&req)
        .await
        .unwrap()
        .collect()
        .await;
    let mut content = String::new();
    let mut tools = 0;
    let mut done = false;
    for e in events {
        match e.unwrap() {
            ChatEvent::Delta(d) => content.push_str(&d),
            ChatEvent::ToolCall(t) => {
                assert_eq!(t.arguments["x"], 1);
                tools += 1;
            }
            ChatEvent::Done {
                finish_reason,
                usage,
            } => {
                assert_eq!(finish_reason.as_deref(), Some("stop"));
                assert_eq!(usage.unwrap().completion_tokens, Some(3));
                done = true;
            }
        }
    }
    assert_eq!(content, "Streamed!");
    assert_eq!(tools, 1);
    assert!(done);

    let events: Vec<_> = mk("midstream")
        .chat_stream(&req)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(
        matches!(events.last(), Some(Err(AiError::Provider(m))) if m.contains("out of memory"))
    );
    let events: Vec<_> = mk("truncated")
        .chat_stream(&req)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(matches!(events.last(), Some(Err(AiError::Transport(_)))));
    assert!(
        matches!(mk("missing").chat(&req).await, Err(AiError::ModelNotFound(m)) if m.contains("pulling"))
    );
}

#[tokio::test]
async fn streaming_disabled_falls_back_to_single_shot() {
    let (addr, cap) = ollama_server().await;
    let mut c = config(AiProviderKind::Ollama, format!("http://{addr}"));
    c.streaming = false;
    let p = build_provider(&c, None).unwrap();
    let events: Vec<_> = p
        .chat_stream(&ChatRequest::new(vec![Message::user(text("hi"))]))
        .await
        .unwrap()
        .collect()
        .await;
    assert!(matches!(&events[0], Ok(ChatEvent::Delta(d)) if d == "native pong"));
    assert!(matches!(events.last(), Some(Ok(ChatEvent::Done { .. }))));
    assert_eq!(cap.all()[0].2["stream"], false);
}

// ---------------------------------------------------------------------------
// Configuration safety
// ---------------------------------------------------------------------------

#[test]
fn configuration_validation_and_key_safety() {
    // API key over plain HTTP to a public host is refused.
    let c = config(
        AiProviderKind::OpenaiCompatible,
        "http://api.example.com".into(),
    );
    assert!(matches!(build_provider(&c, key()), Err(AiError::Config(_))));
    // …but fine for local endpoints and https.
    build_provider(
        &config(
            AiProviderKind::OpenaiCompatible,
            "http://192.168.1.5:8000".into(),
        ),
        key(),
    )
    .unwrap();
    build_provider(&config(AiProviderKind::Deepseek, String::new()), key()).unwrap();
    // Credentials in the URL are refused.
    let c = config(
        AiProviderKind::OpenaiCompatible,
        "https://u:p@api.example.com".into(),
    );
    assert!(matches!(build_provider(&c, None), Err(AiError::Config(_))));
    // Generic needs a base URL; defaults exist for the others.
    assert!(build_provider(
        &config(AiProviderKind::OpenaiCompatible, String::new()),
        None
    )
    .is_err());
    let o = build_provider(&config(AiProviderKind::Ollama, String::new()), None).unwrap();
    assert_eq!(o.settings().base_url.as_str(), "http://localhost:11434/");
    let d = build_provider(&config(AiProviderKind::Deepseek, String::new()), None).unwrap();
    assert_eq!(
        d.settings().base_url.as_str(),
        "https://api.deepseek.com/v1"
    );
    let mut c = config(AiProviderKind::OpenaiCompatible, "ftp://x".into());
    assert!(build_provider(&c, None).is_err());
    c.base_url = "https://x.example".into();
    c.chat_model = " ".into();
    assert!(build_provider(&c, None).is_err());

    // Local profile is clamped for public endpoints.
    let mut c = config(AiProviderKind::Deepseek, String::new());
    c.privacy_profile = PrivacyProfile::Local;
    let p = build_provider(&c, None).unwrap();
    assert_eq!(p.privacy_profile(), PrivacyProfile::Standard);
    let mut c = config(AiProviderKind::Ollama, String::new());
    c.privacy_profile = PrivacyProfile::Local;
    assert_eq!(
        build_provider(&c, None).unwrap().privacy_profile(),
        PrivacyProfile::Local
    );

    // Debug output never shows the key.
    let p = build_provider(&config(AiProviderKind::Deepseek, String::new()), key()).unwrap();
    let dbg = format!("{p:?}");
    assert!(!dbg.contains("key-value-123"), "{dbg}");
    let _ = sanitize(PrivacyProfile::Strict, "x");
}
