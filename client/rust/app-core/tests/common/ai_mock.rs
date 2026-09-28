//! In-process mock of an OpenAI-compatible LLM endpoint (axum), in the style
//! of ai-core's provider tests: `/v1/chat/completions` (JSON and SSE
//! streaming), `/v1/embeddings` (deterministic toy vectors) and
//! `/v1/models`. Every request is captured (path, `Authorization` header,
//! raw body and the decoded prompt/input text) so tests can assert what
//! left the device. Shared by `tests/ai*.rs` and app-core's unit tests.
#![allow(dead_code)]

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// One captured request.
#[derive(Debug, Clone)]
pub struct Captured {
    pub path: String,
    pub authorization: Option<String>,
    pub body: String,
    /// Message contents (chat) or inputs (embeddings), decoded from JSON.
    pub text: String,
    pub model: Option<String>,
}

#[derive(Default)]
struct Inner {
    requests: Mutex<Vec<Captured>>,
    chat: Mutex<VecDeque<String>>,
    required_key: Mutex<Option<String>>,
    embeddings: AtomicUsize,
}

/// Handle to a running mock endpoint.
#[derive(Clone, Default)]
pub struct MockLlm {
    inner: Arc<Inner>,
    /// `http://127.0.0.1:<port>` (append `/v1` for the provider base URL).
    pub base: String,
}

impl MockLlm {
    pub async fn start() -> Self {
        let mut mock = MockLlm::default();
        let app = Router::new()
            .route("/v1/chat/completions", post(chat))
            .route("/v1/embeddings", post(embeddings))
            .route("/v1/models", get(models))
            .with_state(mock.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        mock.base = format!("http://{addr}");
        mock
    }

    /// Provider base URL (OpenAI-compatible).
    pub fn url(&self) -> String {
        format!("{}/v1", self.base)
    }

    /// Queue the content of the next chat reply (default `{}`).
    pub fn push_chat(&self, content: impl Into<String>) {
        self.inner.chat.lock().unwrap().push_back(content.into());
    }

    /// Answer 401 unless `Authorization: Bearer <key>` is sent.
    pub fn require_key(&self, key: &str) {
        *self.inner.required_key.lock().unwrap() = Some(key.to_owned());
    }

    pub fn requests(&self) -> Vec<Captured> {
        self.inner.requests.lock().unwrap().clone()
    }

    pub fn count(&self, path: &str) -> usize {
        self.requests().iter().filter(|r| r.path == path).count()
    }

    /// Number of texts embedded so far.
    pub fn embedded_texts(&self) -> usize {
        self.inner.embeddings.load(Ordering::SeqCst)
    }

    /// Everything that was sent in request bodies (raw + decoded).
    pub fn everything_sent(&self) -> String {
        let mut s = String::new();
        for r in self.requests() {
            s.push_str(&r.body);
            s.push('\n');
            s.push_str(&r.text);
            s.push('\n');
        }
        s
    }

    fn capture(
        &self,
        path: &str,
        headers: &HeaderMap,
        body: &str,
        text: String,
        model: Option<String>,
    ) {
        let authorization = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        self.inner.requests.lock().unwrap().push(Captured {
            path: path.to_owned(),
            authorization,
            body: body.to_owned(),
            text,
            model,
        });
    }

    fn authorized(&self, headers: &HeaderMap) -> bool {
        match self.inner.required_key.lock().unwrap().as_deref() {
            None => true,
            Some(k) => {
                headers.get("authorization").and_then(|v| v.to_str().ok())
                    == Some(format!("Bearer {k}").as_str())
            }
        }
    }
}

/// Deterministic toy embedding over a few concept groups (as in ai-core's
/// tests) — similar topics get similar vectors.
pub fn toy_embed(text: &str) -> Vec<f32> {
    let t = text.to_lowercase();
    let groups: [&[&str]; 5] = [
        &[
            "kubectl",
            "pod",
            "k8s",
            "kubernetes",
            "deployment",
            "container",
        ],
        &["log", "journal", "tail", "error"],
        &["disk", "space", "storage", "du ", "df "],
        &["postgres", "database", "sql", "vacuum", "psql"],
        &["restart", "service", "systemctl", "nginx"],
    ];
    groups
        .iter()
        .map(|g| g.iter().filter(|w| t.contains(*w)).count() as f32 + 0.01)
        .collect()
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        axum::Json(json!({"error": {"message": "invalid api key"}})),
    )
        .into_response()
}

async fn chat(State(m): State<MockLlm>, headers: HeaderMap, body: String) -> Response {
    let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let text = v["messages"]
        .as_array()
        .map(|msgs| {
            msgs.iter()
                .filter_map(|m| m["content"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let model = v["model"].as_str().map(str::to_owned);
    m.capture("/v1/chat/completions", &headers, &body, text, model.clone());
    if !m.authorized(&headers) {
        return unauthorized();
    }
    let content = m
        .inner
        .chat
        .lock()
        .unwrap()
        .pop_front()
        .unwrap_or_else(|| "{}".into());
    if v["stream"] == json!(true) {
        let chars: Vec<char> = content.chars().collect();
        let mut chunks: Vec<String> = chars
            .chunks(3)
            .map(|c| {
                let piece: String = c.iter().collect();
                format!(
                    "data: {}\n\n",
                    json!({"choices": [{"index": 0, "delta": {"content": piece}}]})
                )
            })
            .collect();
        chunks.push(format!(
            "data: {}\n\n",
            json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]})
        ));
        chunks.push("data: [DONE]\n\n".into());
        let stream = futures::stream::iter(
            chunks
                .into_iter()
                .map(|c| Ok::<Bytes, Infallible>(Bytes::from(c))),
        );
        return Response::builder()
            .header("content-type", "text/event-stream")
            .body(Body::from_stream(stream))
            .unwrap();
    }
    axum::Json(json!({
        "id": "mock", "model": model,
        "choices": [{"index": 0, "message": {"role": "assistant", "content": content}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1}
    }))
    .into_response()
}

async fn embeddings(State(m): State<MockLlm>, headers: HeaderMap, body: String) -> Response {
    let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let inputs: Vec<String> = match &v["input"] {
        Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str().map(str::to_owned))
            .collect(),
        Value::String(s) => vec![s.clone()],
        _ => Vec::new(),
    };
    let model = v["model"].as_str().map(str::to_owned);
    m.capture(
        "/v1/embeddings",
        &headers,
        &body,
        inputs.join("\n"),
        model.clone(),
    );
    if !m.authorized(&headers) {
        return unauthorized();
    }
    m.inner.embeddings.fetch_add(inputs.len(), Ordering::SeqCst);
    let data: Vec<Value> = inputs
        .iter()
        .enumerate()
        .map(|(i, t)| json!({"object": "embedding", "index": i, "embedding": toy_embed(t)}))
        .collect();
    axum::Json(json!({"data": data, "model": model})).into_response()
}

async fn models(State(m): State<MockLlm>, headers: HeaderMap) -> Response {
    m.capture("/v1/models", &headers, "", String::new(), None);
    if !m.authorized(&headers) {
        return unauthorized();
    }
    axum::Json(json!({"data": [{"id": "mock-chat", "owned_by": "mock"}, {"id": "mock-embed"}]}))
        .into_response()
}
