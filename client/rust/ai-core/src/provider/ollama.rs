//! Ollama native API: `/api/chat` (NDJSON streaming), `/api/embed`,
//! `/api/tags`.

use super::http::{decode_json, decode_stream, join, Framing, HttpClient, StreamDecoder};
use super::{
    single_shot_stream, Capabilities, ChatEvent, ChatRequest, ChatResponse, ChatStream,
    EmbeddingRequest, EmbeddingResponse, LlmProvider, Message, ModelInfo, ProviderSettings,
    ResponseFormat, StructuredOutput, ToolCall, Usage,
};
use crate::error::{AiError, Result};
use crate::sanitizer::sanitize_for_error;
use async_trait::async_trait;
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::{json, Value};
use url::Url;

/// Ollama (local by default; a key is only needed behind an auth proxy).
#[derive(Debug)]
pub struct OllamaProvider {
    settings: ProviderSettings,
    http: HttpClient,
}

impl OllamaProvider {
    pub fn new(settings: ProviderSettings, api_key: Option<SecretString>) -> Result<Self> {
        let http = HttpClient::new(&settings, api_key)?;
        Ok(Self { settings, http })
    }

    fn url(&self, path: &str) -> Url {
        join(&self.settings.base_url, path)
    }

    fn chat_body(&self, req: &ChatRequest, stream: bool) -> Value {
        let mut body = json!({
            "model": req.model.as_deref().unwrap_or(&self.settings.chat_model),
            "messages": messages_json(&req.messages),
            "stream": stream,
        });
        let mut options = serde_json::Map::new();
        if let Some(t) = req.temperature {
            options.insert("temperature".into(), json!(t));
        }
        if let Some(m) = req.max_tokens {
            options.insert("num_predict".into(), json!(m));
        }
        if !options.is_empty() {
            body["options"] = Value::Object(options);
        }
        match &req.response_format {
            ResponseFormat::Text => {}
            ResponseFormat::Json => body["format"] = json!("json"),
            ResponseFormat::JsonSchema { schema, .. } => body["format"] = schema.clone(),
        }
        if !req.tools.is_empty() && self.settings.tool_support {
            body["tools"] = Value::Array(
                req.tools
                    .iter()
                    .map(|t| {
                        json!({"type": "function", "function": {
                            "name": t.name, "description": t.description, "parameters": t.parameters
                        }})
                    })
                    .collect(),
            );
        }
        body
    }
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
                            .map(
                                |c| json!({"function": {"name": c.name, "arguments": c.arguments}}),
                            )
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
struct OlFunction {
    name: String,
    arguments: Value,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OlToolCall {
    id: Option<String>,
    function: OlFunction,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OlMessage {
    content: String,
    tool_calls: Vec<OlToolCall>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OlChat {
    model: Option<String>,
    message: Option<OlMessage>,
    done: bool,
    done_reason: Option<String>,
    prompt_eval_count: Option<u32>,
    eval_count: Option<u32>,
    error: Option<String>,
}

fn tool_calls(calls: Vec<OlToolCall>, offset: usize) -> Vec<ToolCall> {
    calls
        .into_iter()
        .enumerate()
        .map(|(i, c)| ToolCall {
            id: c.id.unwrap_or_else(|| format!("call_{}", offset + i)),
            name: c.function.name,
            arguments: match c.function.arguments {
                Value::String(s) => serde_json::from_str(&s).unwrap_or(Value::String(s)),
                v => v,
            },
        })
        .collect()
}

#[derive(Default)]
struct OlStreamDecoder {
    done: bool,
    tool_count: usize,
}

impl StreamDecoder for OlStreamDecoder {
    fn on_payload(&mut self, payload: &str) -> Vec<Result<ChatEvent>> {
        let chunk: OlChat = match serde_json::from_str(payload) {
            Ok(c) => c,
            Err(_) => {
                return vec![Err(AiError::InvalidResponse(
                    "malformed stream line".into(),
                ))]
            }
        };
        if let Some(e) = chunk.error {
            return vec![Err(AiError::Provider(sanitize_for_error(&e)))];
        }
        let mut out = Vec::new();
        if let Some(m) = chunk.message {
            if !m.content.is_empty() {
                out.push(Ok(ChatEvent::Delta(m.content)));
            }
            let calls = tool_calls(m.tool_calls, self.tool_count);
            self.tool_count += calls.len();
            out.extend(calls.into_iter().map(|c| Ok(ChatEvent::ToolCall(c))));
        }
        if chunk.done && !self.done {
            self.done = true;
            out.push(Ok(ChatEvent::Done {
                finish_reason: chunk.done_reason,
                usage: Some(Usage {
                    prompt_tokens: chunk.prompt_eval_count,
                    completion_tokens: chunk.eval_count,
                }),
            }));
        }
        out
    }

    fn on_eof(&mut self) -> Vec<Result<ChatEvent>> {
        if self.done {
            Vec::new()
        } else {
            self.done = true;
            vec![Err(AiError::Transport(
                "stream ended before completion".into(),
            ))]
        }
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn settings(&self) -> &ProviderSettings {
        &self.settings
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            chat: true,
            responses: false,
            embeddings: self.settings.embedding_model.is_some(),
            streaming: self.settings.streaming,
            tool_calling: self.settings.tool_support,
            structured_output: StructuredOutput::JsonSchema,
        }
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        #[derive(Deserialize)]
        struct Tag {
            name: String,
            #[serde(default)]
            size: Option<u64>,
        }
        #[derive(Deserialize)]
        struct Tags {
            #[serde(default)]
            models: Vec<Tag>,
        }
        let resp = self.http.get(self.url("api/tags")).await?;
        let t: Tags = decode_json(resp, "unexpected /api/tags response").await?;
        Ok(t.models
            .into_iter()
            .map(|m| ModelInfo {
                id: m.name,
                owned_by: None,
                size_bytes: m.size,
            })
            .collect())
    }

    async fn chat(&self, req: &ChatRequest) -> Result<ChatResponse> {
        let body = self.chat_body(req, false);
        let resp = self
            .http
            .post_json(self.url("api/chat"), &body, false)
            .await?;
        let c: OlChat = decode_json(resp, "unexpected /api/chat response").await?;
        if let Some(e) = c.error {
            return Err(AiError::Provider(sanitize_for_error(&e)));
        }
        let m = c.message.unwrap_or_default();
        Ok(ChatResponse {
            content: m.content,
            tool_calls: tool_calls(m.tool_calls, 0),
            finish_reason: c.done_reason,
            model: c.model,
            usage: Some(Usage {
                prompt_tokens: c.prompt_eval_count,
                completion_tokens: c.eval_count,
            }),
        })
    }

    async fn chat_stream(&self, req: &ChatRequest) -> Result<ChatStream> {
        if !self.settings.streaming {
            return Ok(single_shot_stream(self.chat(req).await?));
        }
        let body = self.chat_body(req, true);
        let resp = self
            .http
            .post_json(self.url("api/chat"), &body, true)
            .await?;
        Ok(decode_stream(
            resp,
            Framing::Ndjson,
            OlStreamDecoder::default(),
        ))
    }

    async fn embed(&self, req: &EmbeddingRequest) -> Result<EmbeddingResponse> {
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
            .post_json(self.url("api/embed"), &body, false)
            .await?;
        #[derive(Deserialize)]
        struct Out {
            embeddings: Vec<Vec<f32>>,
            #[serde(default)]
            model: Option<String>,
        }
        let out: Out = decode_json(resp, "unexpected /api/embed response").await?;
        if out.embeddings.len() != req.inputs.len() {
            return Err(AiError::InvalidResponse(format!(
                "expected {} embeddings, got {}",
                req.inputs.len(),
                out.embeddings.len()
            )));
        }
        Ok(EmbeddingResponse {
            vectors: out.embeddings,
            model: out.model.unwrap_or(model),
        })
    }
}
