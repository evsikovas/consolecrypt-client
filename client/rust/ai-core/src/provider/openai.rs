//! OpenAI-compatible provider: LM Studio, DeepSeek and generic servers.
//! `/v1/chat/completions` (SSE streaming), `/v1/embeddings`, `/v1/models`,
//! `/v1/responses`.

use super::http::{decode_json, decode_stream, join, Framing, HttpClient, StreamDecoder};
use super::{
    single_shot_stream, Capabilities, ChatEvent, ChatRequest, ChatResponse, ChatStream,
    EmbeddingRequest, EmbeddingResponse, LlmProvider, Message, ModelInfo, ProviderSettings,
    ResponseFormat, Role, StructuredOutput, ToolCall, Usage,
};
use crate::error::{AiError, Result};
use crate::sanitizer::sanitize_for_error;
use async_trait::async_trait;
use cc_models::ai::AiProviderKind;
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use url::Url;

/// Ensure the base URL ends with an API version segment (`/v1` unless the
/// user already gave one such as `/v1` or `/v4`).
pub(crate) fn normalize_base(mut url: Url) -> Url {
    let path = url.path().trim_end_matches('/').to_owned();
    let last = path.rsplit('/').next().unwrap_or("");
    let versioned =
        last.len() >= 2 && last.starts_with('v') && last[1..].chars().all(|c| c.is_ascii_digit());
    if versioned {
        url.set_path(&path);
    } else {
        url.set_path(&format!("{path}/v1"));
    }
    url
}

/// LM Studio / DeepSeek / generic OpenAI-compatible endpoint.
#[derive(Debug)]
pub struct OpenAiCompatibleProvider {
    settings: ProviderSettings,
    http: HttpClient,
    caps: Capabilities,
}

impl OpenAiCompatibleProvider {
    /// `api_key` is consumed and stored only in the HTTP client's sensitive
    /// `Authorization` header.
    pub fn new(settings: ProviderSettings, api_key: Option<SecretString>) -> Result<Self> {
        let http = HttpClient::new(&settings, api_key)?;
        let has_embed_model = settings.embedding_model.is_some();
        let caps = match settings.kind {
            AiProviderKind::LmStudio => Capabilities {
                chat: true,
                responses: true,
                embeddings: has_embed_model,
                streaming: settings.streaming,
                tool_calling: settings.tool_support,
                structured_output: StructuredOutput::JsonSchema,
            },
            AiProviderKind::Deepseek => Capabilities {
                chat: true,
                responses: false,
                embeddings: false,
                streaming: settings.streaming,
                tool_calling: settings.tool_support,
                structured_output: StructuredOutput::JsonObject,
            },
            _ => Capabilities {
                chat: true,
                responses: true,
                embeddings: has_embed_model,
                streaming: settings.streaming,
                tool_calling: settings.tool_support,
                structured_output: StructuredOutput::JsonObject,
            },
        };
        Ok(Self {
            settings,
            http,
            caps,
        })
    }

    fn url(&self, path: &str) -> Url {
        join(&self.settings.base_url, path)
    }

    fn model<'a>(&'a self, req: &'a ChatRequest) -> &'a str {
        req.model.as_deref().unwrap_or(&self.settings.chat_model)
    }

    fn chat_body(&self, req: &ChatRequest, stream: bool) -> Value {
        let mut body = json!({
            "model": self.model(req),
            "messages": messages_json(&req.messages),
            "stream": stream,
        });
        if let Some(t) = req.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(m) = req.max_tokens {
            body["max_tokens"] = json!(m);
        }
        if let Some(rf) = response_format_json(&req.response_format, self.caps.structured_output) {
            body["response_format"] = rf;
        }
        if !req.tools.is_empty() && self.caps.tool_calling {
            body["tools"] = tools_json(&req.tools);
        }
        body
    }
}

fn response_format_json(rf: &ResponseFormat, support: StructuredOutput) -> Option<Value> {
    match (rf, support) {
        (ResponseFormat::Text, _) | (_, StructuredOutput::None) => None,
        (_, StructuredOutput::JsonObject) => Some(json!({"type": "json_object"})),
        (ResponseFormat::Json, StructuredOutput::JsonSchema) => Some(json!({
            "type": "json_schema",
            "json_schema": {"name": "response", "schema": {"type": "object"}}
        })),
        (ResponseFormat::JsonSchema { name, schema }, StructuredOutput::JsonSchema) => {
            Some(json!({
                "type": "json_schema",
                "json_schema": {"name": name, "schema": schema}
            }))
        }
    }
}

fn tools_json(tools: &[super::ToolDefinition]) -> Value {
    Value::Array(
        tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {"name": t.name, "description": t.description, "parameters": t.parameters}
                })
            })
            .collect(),
    )
}

fn messages_json(messages: &[Message]) -> Value {
    Value::Array(
        messages
            .iter()
            .map(|m| {
                let mut o = json!({"role": m.role.as_str(), "content": m.content().as_str()});
                if !m.tool_calls.is_empty() {
                    o["tool_calls"] = Value::Array(
                        m.tool_calls
                            .iter()
                            .map(|c| {
                                let args = match &c.arguments {
                                    Value::String(s) => s.clone(),
                                    v => v.to_string(),
                                };
                                json!({"id": c.id, "type": "function", "function": {"name": c.name, "arguments": args}})
                            })
                            .collect(),
                    );
                }
                if let Some(id) = &m.tool_call_id {
                    o["tool_call_id"] = json!(id);
                }
                o
            })
            .collect(),
    )
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OaFunction {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OaToolCall {
    index: Option<usize>,
    id: Option<String>,
    function: Option<OaFunction>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OaMessage {
    content: Option<String>,
    tool_calls: Option<Vec<OaToolCall>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OaChoice {
    message: Option<OaMessage>,
    delta: Option<OaMessage>,
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OaUsage {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OaCompletion {
    model: Option<String>,
    choices: Vec<OaChoice>,
    usage: Option<OaUsage>,
    error: Option<Value>,
}

fn parse_args(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.to_owned()))
}

fn provider_error(v: &Value) -> AiError {
    let msg = v
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| v.to_string());
    AiError::Provider(sanitize_for_error(&msg))
}

impl From<OaUsage> for Usage {
    fn from(u: OaUsage) -> Self {
        Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
        }
    }
}

/// SSE decoder for `/chat/completions` streams.
#[derive(Default)]
struct OaStreamDecoder {
    tools: BTreeMap<usize, (Option<String>, String, String)>,
    finish_reason: Option<String>,
    usage: Option<Usage>,
    done: bool,
}

impl OaStreamDecoder {
    fn flush(&mut self) -> Vec<Result<ChatEvent>> {
        let mut out: Vec<Result<ChatEvent>> = std::mem::take(&mut self.tools)
            .into_iter()
            .map(|(i, (id, name, args))| {
                Ok(ChatEvent::ToolCall(ToolCall {
                    id: id.unwrap_or_else(|| format!("call_{i}")),
                    name,
                    arguments: parse_args(&args),
                }))
            })
            .collect();
        if !self.done {
            self.done = true;
            out.push(Ok(ChatEvent::Done {
                finish_reason: self.finish_reason.take(),
                usage: self.usage.take(),
            }));
        }
        out
    }
}

impl StreamDecoder for OaStreamDecoder {
    fn on_payload(&mut self, payload: &str) -> Vec<Result<ChatEvent>> {
        let p = payload.trim();
        if p == "[DONE]" {
            return self.flush();
        }
        if p.is_empty() {
            return Vec::new();
        }
        let chunk: OaCompletion = match serde_json::from_str(p) {
            Ok(c) => c,
            Err(_) => {
                return vec![Err(AiError::InvalidResponse(
                    "malformed stream chunk".into(),
                ))]
            }
        };
        if let Some(e) = &chunk.error {
            return vec![Err(provider_error(e))];
        }
        if let Some(u) = chunk.usage {
            self.usage = Some(u.into());
        }
        let mut out = Vec::new();
        for c in chunk.choices {
            if let Some(d) = c.delta {
                if let Some(t) = d.content.filter(|t| !t.is_empty()) {
                    out.push(Ok(ChatEvent::Delta(t)));
                }
                for (n, tc) in d.tool_calls.unwrap_or_default().into_iter().enumerate() {
                    let idx = tc.index.unwrap_or(n);
                    let e = self.tools.entry(idx).or_default();
                    if tc.id.is_some() {
                        e.0 = tc.id;
                    }
                    if let Some(f) = tc.function {
                        if let Some(name) = f.name {
                            e.1.push_str(&name);
                        }
                        if let Some(a) = f.arguments {
                            e.2.push_str(&a);
                        }
                    }
                }
            }
            if c.finish_reason.is_some() {
                self.finish_reason = c.finish_reason;
            }
        }
        out
    }

    fn on_eof(&mut self) -> Vec<Result<ChatEvent>> {
        self.flush()
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    fn settings(&self) -> &ProviderSettings {
        &self.settings
    }

    fn capabilities(&self) -> Capabilities {
        self.caps
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        #[derive(Deserialize)]
        struct Model {
            id: String,
            #[serde(default)]
            owned_by: Option<String>,
        }
        #[derive(Deserialize)]
        struct Models {
            data: Vec<Model>,
        }
        let resp = self.http.get(self.url("models")).await?;
        let m: Models = decode_json(resp, "unexpected /models response").await?;
        Ok(m.data
            .into_iter()
            .map(|m| ModelInfo {
                id: m.id,
                owned_by: m.owned_by,
                size_bytes: None,
            })
            .collect())
    }

    async fn chat(&self, req: &ChatRequest) -> Result<ChatResponse> {
        let body = self.chat_body(req, false);
        let resp = self
            .http
            .post_json(self.url("chat/completions"), &body, false)
            .await?;
        let c: OaCompletion = decode_json(resp, "unexpected chat completion response").await?;
        if let Some(e) = &c.error {
            return Err(provider_error(e));
        }
        let choice = c
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| AiError::InvalidResponse("no choices in response".into()))?;
        let msg = choice.message.unwrap_or_default();
        let tool_calls = msg
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(i, t)| {
                let f = t.function.unwrap_or_default();
                ToolCall {
                    id: t.id.unwrap_or_else(|| format!("call_{i}")),
                    name: f.name.unwrap_or_default(),
                    arguments: parse_args(f.arguments.as_deref().unwrap_or("{}")),
                }
            })
            .collect();
        Ok(ChatResponse {
            content: msg.content.unwrap_or_default(),
            tool_calls,
            finish_reason: choice.finish_reason,
            model: c.model,
            usage: c.usage.map(Into::into),
        })
    }

    async fn chat_stream(&self, req: &ChatRequest) -> Result<ChatStream> {
        if !self.caps.streaming {
            return Ok(single_shot_stream(self.chat(req).await?));
        }
        let body = self.chat_body(req, true);
        let resp = self
            .http
            .post_json(self.url("chat/completions"), &body, true)
            .await?;
        Ok(decode_stream(
            resp,
            Framing::Sse,
            OaStreamDecoder::default(),
        ))
    }

    async fn respond(&self, req: &ChatRequest) -> Result<ChatResponse> {
        if !self.caps.responses {
            return Err(AiError::Unsupported("responses"));
        }
        let instructions: Vec<&str> = req
            .messages
            .iter()
            .filter(|m| m.role == Role::System)
            .map(|m| m.content().as_str())
            .collect();
        let input: Vec<Value> = req
            .messages
            .iter()
            .filter(|m| m.role != Role::System)
            .map(|m| json!({"role": m.role.as_str(), "content": m.content().as_str()}))
            .collect();
        let mut body = json!({"model": self.model(req), "input": input});
        if !instructions.is_empty() {
            body["instructions"] = json!(instructions.join("\n\n"));
        }
        if let Some(t) = req.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(m) = req.max_tokens {
            body["max_output_tokens"] = json!(m);
        }
        match &req.response_format {
            ResponseFormat::Text => {}
            ResponseFormat::Json => body["text"] = json!({"format": {"type": "json_object"}}),
            ResponseFormat::JsonSchema { name, schema } => {
                body["text"] =
                    json!({"format": {"type": "json_schema", "name": name, "schema": schema}});
            }
        }
        let resp = self
            .http
            .post_json(self.url("responses"), &body, false)
            .await?;
        let v: Value = decode_json(resp, "unexpected /responses response").await?;
        if let Some(e) = v.get("error").filter(|e| !e.is_null()) {
            return Err(provider_error(e));
        }
        let mut content = String::new();
        let mut tool_calls = Vec::new();
        for item in v
            .get("output")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            match item.get("type").and_then(Value::as_str) {
                Some("message") => {
                    for c in item
                        .get("content")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        if c.get("type").and_then(Value::as_str) == Some("output_text") {
                            content.push_str(c.get("text").and_then(Value::as_str).unwrap_or(""));
                        }
                    }
                }
                Some("function_call") => tool_calls.push(ToolCall {
                    id: item
                        .get("call_id")
                        .and_then(Value::as_str)
                        .unwrap_or("call_0")
                        .to_owned(),
                    name: item
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                    arguments: parse_args(
                        item.get("arguments")
                            .and_then(Value::as_str)
                            .unwrap_or("{}"),
                    ),
                }),
                _ => {}
            }
        }
        if content.is_empty() {
            if let Some(t) = v.get("output_text").and_then(Value::as_str) {
                content = t.to_owned();
            }
        }
        let usage = v.get("usage").map(|u| Usage {
            prompt_tokens: u
                .get("input_tokens")
                .and_then(Value::as_u64)
                .map(|n| n as u32),
            completion_tokens: u
                .get("output_tokens")
                .and_then(Value::as_u64)
                .map(|n| n as u32),
        });
        Ok(ChatResponse {
            content,
            tool_calls,
            finish_reason: v.get("status").and_then(Value::as_str).map(str::to_owned),
            model: v.get("model").and_then(Value::as_str).map(str::to_owned),
            usage,
        })
    }

    async fn embed(&self, req: &EmbeddingRequest) -> Result<EmbeddingResponse> {
        if self.settings.kind == AiProviderKind::Deepseek {
            return Err(AiError::Unsupported("embeddings"));
        }
        let model = req
            .model
            .clone()
            .or_else(|| self.settings.embedding_model.clone())
            .ok_or_else(|| AiError::Config("no embedding model configured".into()))?;
        if req.inputs.is_empty() {
            return Ok(EmbeddingResponse {
                vectors: Vec::new(),
                model,
            });
        }
        let inputs: Vec<&str> = req.inputs.iter().map(|t| t.as_str()).collect();
        let body = json!({"model": model, "input": inputs});
        let resp = self
            .http
            .post_json(self.url("embeddings"), &body, false)
            .await?;
        #[derive(Deserialize)]
        struct Item {
            embedding: Vec<f32>,
            #[serde(default)]
            index: Option<usize>,
        }
        #[derive(Deserialize)]
        struct Out {
            data: Vec<Item>,
            #[serde(default)]
            model: Option<String>,
        }
        let out: Out = decode_json(resp, "unexpected /embeddings response").await?;
        let mut items: Vec<(usize, Vec<f32>)> = out
            .data
            .into_iter()
            .enumerate()
            .map(|(i, it)| (it.index.unwrap_or(i), it.embedding))
            .collect();
        items.sort_by_key(|x| x.0);
        if items.len() != req.inputs.len() {
            return Err(AiError::InvalidResponse(format!(
                "expected {} embeddings, got {}",
                req.inputs.len(),
                items.len()
            )));
        }
        Ok(EmbeddingResponse {
            vectors: items.into_iter().map(|x| x.1).collect(),
            model: out.model.unwrap_or(model),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_normalization() {
        let n = |s: &str| normalize_base(Url::parse(s).unwrap()).to_string();
        assert_eq!(n("https://api.deepseek.com"), "https://api.deepseek.com/v1");
        assert_eq!(n("http://localhost:1234/v1/"), "http://localhost:1234/v1");
        assert_eq!(
            n("https://openrouter.ai/api/v1"),
            "https://openrouter.ai/api/v1"
        );
        assert_eq!(
            n("https://open.bigmodel.cn/api/paas/v4"),
            "https://open.bigmodel.cn/api/paas/v4"
        );
        assert_eq!(
            n("http://gpu.lan:8000/openai"),
            "http://gpu.lan:8000/openai/v1"
        );
    }

    #[test]
    fn stream_decoder_accumulates_tool_calls() {
        let mut d = OaStreamDecoder::default();
        let mut ev = Vec::new();
        ev.extend(d.on_payload(r#"{"choices":[{"delta":{"content":"Hel"}}]}"#));
        ev.extend(d.on_payload(r#"{"choices":[{"delta":{"content":"lo","tool_calls":[{"index":0,"id":"c1","function":{"name":"run","arguments":"{\"cmd\":"}}]}}]}"#));
        ev.extend(d.on_payload(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"ls\"}"}}]},"finish_reason":"tool_calls"}]}"#));
        ev.extend(d.on_payload("[DONE]"));
        let ev: Vec<ChatEvent> = ev.into_iter().map(|e| e.unwrap()).collect();
        assert_eq!(ev[0], ChatEvent::Delta("Hel".into()));
        assert_eq!(ev[1], ChatEvent::Delta("lo".into()));
        match &ev[2] {
            ChatEvent::ToolCall(t) => {
                assert_eq!(t.name, "run");
                assert_eq!(t.arguments["cmd"], "ls");
            }
            other => panic!("{other:?}"),
        }
        assert!(
            matches!(&ev[3], ChatEvent::Done { finish_reason: Some(r), .. } if r == "tool_calls")
        );
        assert!(d.on_eof().is_empty());
    }
}
