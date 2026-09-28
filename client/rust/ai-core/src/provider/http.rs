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
    let body = resp.text().await.unwrap_or_default();
    let message = extract_error_message(&body);
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
}

impl SseParser {
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(pos) = self.buf.iter().position(|b| *b == b'\n') {
            let mut line: Vec<u8> = self.buf.drain(..=pos).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let line = String::from_utf8_lossy(&line).into_owned();
            self.line(&line, &mut out);
        }
        out
    }

    fn line(&mut self, line: &str, out: &mut Vec<SseEvent>) {
        if line.is_empty() {
            if !self.data.is_empty() {
                out.push(SseEvent {
                    event: self.event.take(),
                    data: self.data.join("\n"),
                });
                self.data.clear();
            } else {
                self.event = None;
            }
            return;
        }
        if line.starts_with(':') {
            return;
        }
        let (field, value) = match line.find(':') {
            Some(i) => {
                let v = &line[i + 1..];
                (&line[..i], v.strip_prefix(' ').unwrap_or(v))
            }
            None => (line, ""),
        };
        match field {
            "data" => self.data.push(value.to_owned()),
            "event" => self.event = Some(value.to_owned()),
            _ => {}
        }
    }

    pub(crate) fn finish(&mut self) -> Vec<SseEvent> {
        let mut out = Vec::new();
        if !self.buf.is_empty() {
            let rest = std::mem::take(&mut self.buf);
            let line = String::from_utf8_lossy(&rest)
                .trim_end_matches('\r')
                .to_owned();
            self.line(&line, &mut out);
        }
        self.line("", &mut out);
        out
    }
}

/// Incremental newline-delimited parser (NDJSON).
#[derive(Debug, Default)]
pub(crate) struct LineParser {
    buf: Vec<u8>,
}

impl LineParser {
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(pos) = self.buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=pos).collect();
            let s = String::from_utf8_lossy(&line).trim().to_owned();
            if !s.is_empty() {
                out.push(s);
            }
        }
        out
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
                    let payloads = match st.framing {
                        Framing::Sse => st.sse.push(&bytes).into_iter().map(|e| e.data).collect(),
                        Framing::Ndjson => st.lines.push(&bytes),
                    };
                    st.feed(payloads);
                }
                Some(Err(e)) => st.queue.push_back(Err(match map_err(e) {
                    AiError::Transport(m) => AiError::Transport(format!("stream interrupted: {m}")),
                    other => other,
                })),
                None => {
                    st.eof = true;
                    let payloads = match st.framing {
                        Framing::Sse => st.sse.finish().into_iter().map(|e| e.data).collect(),
                        Framing::Ndjson => st.lines.finish().into_iter().collect(),
                    };
                    st.feed(payloads);
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

    #[test]
    fn sse_parser_handles_chunking_comments_and_crlf() {
        let mut p = SseParser::default();
        let mut evs = p.push(b": keep-alive\r\ndata: {\"a\":");
        assert!(evs.is_empty());
        evs.extend(p.push(b"1}\r\n\r\nevent: x\ndata: line1\ndata: line2\n\n"));
        evs.extend(p.push("data: é".as_bytes().split_at(7).0));
        evs.extend(p.push(&"data: é".as_bytes()[7..]));
        evs.extend(p.push(b"\n\ndata: [DONE]"));
        evs.extend(p.finish());
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
        let mut l = p.push(b"{\"a\":1}\n{\"b\"");
        l.extend(p.push(b":2}\n\n"));
        assert_eq!(l, vec!["{\"a\":1}", "{\"b\":2}"]);
        p.push(b"{\"c\":3}");
        assert_eq!(p.finish().as_deref(), Some("{\"c\":3}"));
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
