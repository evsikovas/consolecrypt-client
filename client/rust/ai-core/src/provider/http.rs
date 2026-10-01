//! HTTP plumbing shared by providers: client construction (API key only in a
//! sensitive default header), error mapping, SSE / NDJSON framing.

use super::{ChatEvent, ChatStream, Locality, ProviderSettings};
use crate::error::{AiError, Result};
use crate::sanitizer::sanitize_for_error;
use bytes::Bytes;
use futures::stream::{BoxStream, StreamExt};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, RETRY_AFTER};
use secrecy::{ExposeSecret, SecretString};
use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;
use url::Url;
use zeroize::Zeroizing;

// Bounds apply to hostile or broken providers, including streaming bodies
// without a newline. They comfortably exceed ordinary chat/model responses
// and embedding batches, but prevent unbounded process memory growth.
pub(crate) const MAX_JSON_BYTES: usize = 32 * 1024 * 1024;
pub(crate) const MAX_STREAM_BYTES: usize = 32 * 1024 * 1024;
pub(crate) const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_ERROR_BYTES: usize = 64 * 1024;
const MAX_LINE_BYTES: usize = 1024 * 1024;
const MAX_SSE_EVENT_BYTES: usize = 2 * 1024 * 1024;

fn oversized_response() -> AiError {
    AiError::InvalidResponse("AI provider response exceeds the size limit".into())
}

async fn bounded_body(resp: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    if resp.content_length().is_some_and(|n| n > limit as u64) {
        return Err(oversized_response());
    }
    let mut body = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(map_err)?;
        if chunk.len() > limit.saturating_sub(body.len()) {
            return Err(oversized_response());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

pub(crate) async fn decode_json<T: serde::de::DeserializeOwned>(
    resp: reqwest::Response,
    reason: &'static str,
) -> Result<T> {
    let body = bounded_body(resp, MAX_JSON_BYTES).await?;
    serde_json::from_slice(&body).map_err(|_| AiError::InvalidResponse(reason.into()))
}

/// reqwest client configured for one provider.
#[derive(Clone)]
pub(crate) struct HttpClient {
    client: reqwest::Client,
    timeout: Duration,
}

impl fmt::Debug for HttpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpClient")
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl HttpClient {
    /// Build the client. The API key is consumed: it only survives inside the
    /// client's default `Authorization` header, marked sensitive.
    pub(crate) fn new(settings: &ProviderSettings, api_key: Option<SecretString>) -> Result<Self> {
        let locality = settings.locality();
        let key = api_key.filter(|k| !k.expose_secret().trim().is_empty());
        if key.is_some() && settings.base_url.scheme() == "http" && locality == Locality::Public {
            return Err(AiError::Config(
                "refusing to send an API key over plain HTTP to a public host; use https".into(),
            ));
        }
        let mut headers = HeaderMap::new();
        if let Some(k) = key {
            let value = Zeroizing::new(format!("Bearer {}", k.expose_secret().trim()));
            let mut hv = HeaderValue::from_str(&value)
                .map_err(|_| AiError::Config("API key contains invalid characters".into()))?;
            hv.set_sensitive(true);
            headers.insert(AUTHORIZATION, hv);
        }
        let mut b = reqwest::Client::builder()
            .default_headers(headers)
            .connect_timeout(settings.connect_timeout)
            .read_timeout(settings.timeout)
            .user_agent(concat!("ConsoleCrypt/", env!("CARGO_PKG_VERSION")));
        if locality.is_local() {
            // Never route local model traffic through a system proxy.
            b = b.no_proxy();
        }
        let client = b.build().map_err(|e| {
            AiError::Config(format!("cannot create HTTP client: {}", e.without_url()))
        })?;
        Ok(Self {
            client,
            timeout: settings.timeout,
        })
    }

    pub(crate) async fn post_json(
        &self,
        url: Url,
        body: &serde_json::Value,
        streaming: bool,
    ) -> Result<reqwest::Response> {
        let mut rb = self.client.post(url).json(body);
        if !streaming {
            rb = rb.timeout(self.timeout);
        }
        let resp = rb.send().await.map_err(map_err)?;
        check_status(resp).await
    }

    pub(crate) async fn get(&self, url: Url) -> Result<reqwest::Response> {
        let resp = self
            .client
            .get(url)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(map_err)?;
        check_status(resp).await
    }
}

/// Map a reqwest error without leaking URLs or bodies.
pub(crate) fn map_err(e: reqwest::Error) -> AiError {
    if e.is_timeout() {
        return AiError::Timeout;
    }
    let (connect, decode) = (e.is_connect(), e.is_decode());
    let msg = sanitize_for_error(&e.without_url().to_string());
    if connect {
        AiError::Connect(msg)
    } else if decode {
        AiError::InvalidResponse(msg)
    } else {
        AiError::Transport(msg)
    }
}

/// Extract a human message from an error body (OpenAI / Ollama / plain).
pub(crate) fn extract_error_message(body: &str) -> String {
    let parsed: Option<serde_json::Value> = serde_json::from_str(body).ok();
    let msg = parsed.as_ref().and_then(|v| {
        v.get("error")
            .and_then(|e| {
                e.get("message")
                    .and_then(|m| m.as_str())
                    .or_else(|| e.as_str())
            })
            .or_else(|| v.get("message").and_then(|m| m.as_str()))
            .or_else(|| v.get("detail").and_then(|m| m.as_str()))
            .map(str::to_owned)
    });
    sanitize_for_error(msg.as_deref().unwrap_or(body).trim())
}

async fn check_status(resp: reqwest::Response) -> Result<reqwest::Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let code = status.as_u16();
    let retry_after = resp
        .headers()
        .get(RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok());
    let body = bounded_body(resp, MAX_ERROR_BYTES).await?;
    let message = extract_error_message(&String::from_utf8_lossy(&body));
    tracing::debug!(status = code, "AI provider returned an error status");
    Err(match code {
        401 | 403 => AiError::Auth { status: code },
        429 => AiError::RateLimited {
            retry_after_secs: retry_after,
        },
        404 if message.to_ascii_lowercase().contains("model") => AiError::ModelNotFound(message),
        _ => AiError::Http {
            status: code,
            message,
        },
    })
}

/// One server-sent event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

/// Incremental SSE parser (byte-level line splitting, so multi-byte UTF-8
/// characters split across chunks are handled).
#[derive(Debug, Default)]
pub(crate) struct SseParser {
    buf: Vec<u8>,
    data: Vec<String>,
    event: Option<String>,
    data_bytes: usize,
}

impl SseParser {
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>> {
        let mut out = Vec::new();
        for part in chunk.split_inclusive(|b| *b == b'\n') {
            if part.len() > MAX_LINE_BYTES.saturating_sub(self.buf.len()) {
                return Err(oversized_response());
            }
            self.buf.extend_from_slice(part);
            if !part.ends_with(b"\n") {
                continue;
            }
            let mut line = std::mem::take(&mut self.buf);
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let line = String::from_utf8_lossy(&line).into_owned();
            self.line(&line, &mut out)?;
        }
        Ok(out)
    }

    fn line(&mut self, line: &str, out: &mut Vec<SseEvent>) -> Result<()> {
        if line.is_empty() {
            if !self.data.is_empty() {
                out.push(SseEvent {
                    event: self.event.take(),
                    data: self.data.join("\n"),
                });
                self.data.clear();
                self.data_bytes = 0;
            } else {
                self.event = None;
            }
            return Ok(());
        }
        if line.starts_with(':') {
            return Ok(());
        }
        let (field, value) = match line.find(':') {
            Some(i) => {
                let v = &line[i + 1..];
                (&line[..i], v.strip_prefix(' ').unwrap_or(v))
            }
            None => (line, ""),
        };
        match field {
            "data" => {
                let bytes = value.len().saturating_add(1);
                if bytes > MAX_SSE_EVENT_BYTES.saturating_sub(self.data_bytes) {
                    return Err(oversized_response());
                }
                self.data_bytes += bytes;
                self.data.push(value.to_owned());
            }
            "event" => self.event = Some(value.to_owned()),
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn finish(&mut self) -> Result<Vec<SseEvent>> {
        let mut out = Vec::new();
        if !self.buf.is_empty() {
            let rest = std::mem::take(&mut self.buf);
            let line = String::from_utf8_lossy(&rest)
                .trim_end_matches('\r')
                .to_owned();
            self.line(&line, &mut out)?;
        }
        self.line("", &mut out)?;
        Ok(out)
    }
}

/// Incremental newline-delimited parser (NDJSON).
#[derive(Debug, Default)]
pub(crate) struct LineParser {
    buf: Vec<u8>,
}

impl LineParser {
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>> {
        let mut out = Vec::new();
        for part in chunk.split_inclusive(|b| *b == b'\n') {
            if part.len() > MAX_LINE_BYTES.saturating_sub(self.buf.len()) {
                return Err(oversized_response());
            }
            self.buf.extend_from_slice(part);
            if !part.ends_with(b"\n") {
                continue;
            }
            let line = std::mem::take(&mut self.buf);
            let s = String::from_utf8_lossy(&line).trim().to_owned();
            if !s.is_empty() {
                out.push(s);
            }
        }
        Ok(out)
    }

    pub(crate) fn finish(&mut self) -> Option<String> {
        let rest = std::mem::take(&mut self.buf);
        let s = String::from_utf8_lossy(&rest).trim().to_owned();
        (!s.is_empty()).then_some(s)
    }
}

/// Converts framed payloads into chat events.
pub(crate) trait StreamDecoder: Send + 'static {
    fn on_payload(&mut self, payload: &str) -> Vec<Result<ChatEvent>>;
    /// End of body; must emit `Done` if not already emitted.
    fn on_eof(&mut self) -> Vec<Result<ChatEvent>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Framing {
    Sse,
    Ndjson,
}

struct DecodeState<D> {
    body: BoxStream<'static, reqwest::Result<Bytes>>,
    framing: Framing,
    sse: SseParser,
    lines: LineParser,
    decoder: D,
    queue: VecDeque<Result<ChatEvent>>,
    finished: bool,
    eof: bool,
    received: usize,
}

impl<D: StreamDecoder> DecodeState<D> {
    fn feed(&mut self, payloads: Vec<String>) {
        for p in payloads {
            let evs = self.decoder.on_payload(&p);
            self.queue.extend(evs);
        }
    }
}

/// Turn a streaming HTTP response into a [`ChatStream`]. The stream ends
/// after the first `Done` or error.
pub(crate) fn decode_stream<D: StreamDecoder>(
    resp: reqwest::Response,
    framing: Framing,
    decoder: D,
) -> ChatStream {
    let st = DecodeState {
        body: resp.bytes_stream().boxed(),
        framing,
        sse: SseParser::default(),
        lines: LineParser::default(),
        decoder,
        queue: VecDeque::new(),
        finished: false,
        eof: false,
        received: 0,
    };
    futures::stream::unfold(st, |mut st| async move {
        loop {
            if let Some(ev) = st.queue.pop_front() {
                if matches!(ev, Ok(ChatEvent::Done { .. }) | Err(_)) {
                    st.finished = true;
                    st.queue.clear();
                }
                return Some((ev, st));
            }
            if st.finished || st.eof {
                return None;
            }
            match st.body.next().await {
                Some(Ok(bytes)) => {
                    if bytes.len() > MAX_STREAM_BYTES.saturating_sub(st.received) {
                        st.queue.push_back(Err(oversized_response()));
                        continue;
                    }
                    st.received += bytes.len();
                    let payloads = match st.framing {
                        Framing::Sse => st
                            .sse
                            .push(&bytes)
                            .map(|events| events.into_iter().map(|e| e.data).collect()),
                        Framing::Ndjson => st.lines.push(&bytes),
                    };
                    match payloads {
                        Ok(payloads) => st.feed(payloads),
                        Err(e) => st.queue.push_back(Err(e)),
                    }
                }
                Some(Err(e)) => st.queue.push_back(Err(match map_err(e) {
                    AiError::Transport(m) => AiError::Transport(format!("stream interrupted: {m}")),
                    other => other,
                })),
                None => {
                    st.eof = true;
                    let payloads = match st.framing {
                        Framing::Sse => st
                            .sse
                            .finish()
                            .map(|events| events.into_iter().map(|e| e.data).collect()),
                        Framing::Ndjson => Ok(st.lines.finish().into_iter().collect()),
                    };
                    match payloads {
                        Ok(payloads) => st.feed(payloads),
                        Err(e) => {
                            st.queue.push_back(Err(e));
                            continue;
                        }
                    }
                    let tail = st.decoder.on_eof();
                    st.queue.extend(tail);
                }
            }
        }
    })
    .boxed()
}

/// Join a relative API path onto a base URL (keeps the base path).
pub(crate) fn join(base: &Url, path: &str) -> Url {
    let mut u = base.clone();
    {
        let trimmed = u.path().trim_end_matches('/').to_owned();
        u.set_path(&format!("{trimmed}/{}", path.trim_start_matches('/')));
    }
    u
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_response(body: axum::body::Body) -> reqwest::Response {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let body = std::sync::Arc::new(std::sync::Mutex::new(Some(body)));
        let app = axum::Router::new().route(
            "/",
            axum::routing::get(move || {
                let body = body.lock().unwrap().take().unwrap();
                async move { body }
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let response = reqwest::get(format!("http://{addr}/")).await.unwrap();
        // This fixture lives only inside the individual test's Tokio runtime.
        drop(server);
        response
    }

    #[tokio::test]
    async fn bounded_body_checks_both_content_length_and_chunked_transfer() {
        let response = test_response(axum::body::Body::from(vec![b'x'; 1025])).await;
        assert!(matches!(
            bounded_body(response, 1024).await,
            Err(AiError::InvalidResponse(_))
        ));
        let chunks = futures::stream::iter(
            (0..3).map(|_| Ok::<_, std::convert::Infallible>(Bytes::from(vec![b'x'; 512]))),
        );
        let response = test_response(axum::body::Body::from_stream(chunks)).await;
        assert!(response.content_length().is_none());
        assert!(matches!(
            bounded_body(response, 1024).await,
            Err(AiError::InvalidResponse(_))
        ));
        let response = test_response(axum::body::Body::from(vec![b'x'; 1024])).await;
        assert_eq!(bounded_body(response, 1024).await.unwrap().len(), 1024);
    }

    struct CommentDecoder;
    impl StreamDecoder for CommentDecoder {
        fn on_payload(&mut self, _payload: &str) -> Vec<Result<ChatEvent>> {
            vec![Ok(ChatEvent::Delta("unexpected".into()))]
        }
        fn on_eof(&mut self) -> Vec<Result<ChatEvent>> {
            Vec::new()
        }
    }

    #[tokio::test]
    async fn streaming_total_limit_ends_once_even_for_valid_small_lines() {
        // Comments produce no chat output, so this exercises the total body
        // cap independently of the per-line/event/answer caps.
        let chunk = Bytes::from(b": public comment\n".repeat(4096));
        let count = MAX_STREAM_BYTES / chunk.len() + 1;
        let chunks = futures::stream::iter(
            (0..count).map(move |_| Ok::<_, std::convert::Infallible>(chunk.clone())),
        );
        let response = test_response(axum::body::Body::from_stream(chunks)).await;
        let mut stream = decode_stream(response, Framing::Sse, CommentDecoder);
        assert!(matches!(
            stream.next().await,
            Some(Err(AiError::InvalidResponse(_)))
        ));
        assert!(stream.next().await.is_none());
    }

    #[test]
    fn sse_parser_handles_chunking_comments_and_crlf() {
        let mut p = SseParser::default();
        let mut evs = p.push(b": keep-alive\r\ndata: {\"a\":").unwrap();
        assert!(evs.is_empty());
        evs.extend(
            p.push(b"1}\r\n\r\nevent: x\ndata: line1\ndata: line2\n\n")
                .unwrap(),
        );
        evs.extend(p.push("data: é".as_bytes().split_at(7).0).unwrap());
        evs.extend(p.push(&"data: é".as_bytes()[7..]).unwrap());
        evs.extend(p.push(b"\n\ndata: [DONE]").unwrap());
        evs.extend(p.finish().unwrap());
        assert_eq!(evs.len(), 4);
        assert_eq!(evs[0].data, "{\"a\":1}");
        assert_eq!(evs[1].event.as_deref(), Some("x"));
        assert_eq!(evs[1].data, "line1\nline2");
        assert_eq!(evs[2].data, "é");
        assert_eq!(evs[3].data, "[DONE]");
    }

    #[test]
    fn line_parser() {
        let mut p = LineParser::default();
        let mut l = p.push(b"{\"a\":1}\n{\"b\"").unwrap();
        l.extend(p.push(b":2}\n\n").unwrap());
        assert_eq!(l, vec!["{\"a\":1}", "{\"b\":2}"]);
        p.push(b"{\"c\":3}").unwrap();
        assert_eq!(p.finish().as_deref(), Some("{\"c\":3}"));
    }

    #[test]
    fn framing_rejects_oversized_unterminated_lines_and_sse_events() {
        let chunk = vec![b'x'; 64 * 1024];
        let mut sse = SseParser::default();
        let mut lines = LineParser::default();
        for _ in 0..MAX_LINE_BYTES / chunk.len() {
            assert!(sse.push(&chunk).unwrap().is_empty());
            assert!(lines.push(&chunk).unwrap().is_empty());
        }
        assert!(sse.push(b"x").is_err());
        assert!(lines.push(b"x").is_err());
        let mut sse = SseParser::default();
        let data = format!("data: {}\n", "x".repeat(64 * 1024));
        for _ in 0..30 {
            assert!(sse.push(data.as_bytes()).is_ok());
        }
        assert!(sse.push(data.as_bytes()).is_ok());
        assert!(sse.push(data.as_bytes()).is_err());
    }

    #[test]
    fn a_large_chunk_of_small_frames_is_valid() {
        let chunk = b"data: ok\n\n".repeat(MAX_LINE_BYTES / 10 + 1);
        let events = SseParser::default().push(&chunk).unwrap();
        assert_eq!(events.len(), MAX_LINE_BYTES / 10 + 1);
    }

    #[test]
    fn error_message_extraction_is_sanitized() {
        let m = extract_error_message(
            r#"{"error":{"message":"bad key sk-abcdefghijklmnopqrstuvwxyz123456"}}"#,
        );
        assert!(m.starts_with("bad key"));
        assert!(!m.contains("sk-abcdefghijklmnopqrstuvwxyz123456"));
        assert_eq!(
            extract_error_message(r#"{"error":"model 'x' not found"}"#),
            "model 'x' not found"
        );
        assert_eq!(extract_error_message("plain text"), "plain text");
    }

    #[test]
    fn url_join() {
        let b = Url::parse("http://h:1/v1").unwrap();
        assert_eq!(
            join(&b, "chat/completions").as_str(),
            "http://h:1/v1/chat/completions"
        );
        let b = Url::parse("http://h:1/").unwrap();
        assert_eq!(join(&b, "/api/chat").as_str(), "http://h:1/api/chat");
    }
}
