//! AI features that app-core offers today (ADR-0107 "AI"): provider test,
//! Ask AI (streamed), command generation, snippet drafts and the local risk
//! rules. Everything sent to a provider passes app-core's sanitizer; the AI
//! never runs commands (CLIENT_SPEC §14–16).
//!
//! JSON results: `AiProviderTestDto`, `CommandProposalDto`,
//! `SnippetDraftDto`, `RiskAssessmentDto`; the Ask stream carries
//! `{"type":"started",…}` followed by `AiAskChunk` JSON.

use crate::api::error::BridgeError;
use crate::frb_generated::StreamSink;
use crate::state::{self, from_json, to_json, with_core};
use cc_app_core::{
    AiAskChunk, AiAskRequestDto, AiContextOptionsDto, CommandDialect, CommandTarget, SnippetType,
};

fn parse_snippet_type(wire: &str) -> Result<SnippetType, BridgeError> {
    serde_json::from_value(serde_json::Value::String(wire.to_owned()))
        .map_err(|_| BridgeError::invalid("snippet_type", "unknown snippet type"))
}

fn target(snippet_type_wire: Option<String>) -> Result<Option<CommandTarget>, BridgeError> {
    snippet_type_wire
        .filter(|s| !s.trim().is_empty())
        .map(|s| parse_snippet_type(&s).map(CommandTarget::from_snippet_type))
        .transpose()
}

fn context(context_json: Option<String>) -> Result<AiContextOptionsDto, BridgeError> {
    match context_json.filter(|s| !s.trim().is_empty()) {
        Some(j) => from_json("ai_context", &j),
        None => Ok(AiContextOptionsDto::default()),
    }
}

/// Check a provider (models, capabilities, latency) → `AiProviderTestDto`.
pub async fn ai_test_provider(provider_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.test_ai_provider(provider_id).await?) }).await
}

/// Ask AI. The stream starts with
/// `{"type":"started","request_id":…,"conversation_id":…}`, then `AiAskChunk`
/// items, ending with exactly one `done` or `error`.
pub async fn ai_ask(
    provider_id: Option<String>,
    conversation_id: Option<String>,
    question: String,
    context_json: Option<String>,
    sink: StreamSink<String>,
) -> Result<(), BridgeError> {
    let context = context(context_json)?;
    let core = state::core()?;
    let request = AiAskRequestDto {
        conversation_id: conversation_id.filter(|s| !s.trim().is_empty()),
        question,
    };
    let stream = state::run(async move {
        core.ai_ask(provider_id, request, context)
            .await
            .map_err(BridgeError::from)
    })
    .await?;
    let started = serde_json::json!({
        "type": "started",
        "request_id": stream.request_id,
        "conversation_id": stream.conversation_id,
    });
    if sink.add(started.to_string()).is_err() {
        return Ok(());
    }
    let mut chunks = stream.chunks;
    state::runtime().spawn(async move {
        while let Some(chunk) = chunks.recv().await {
            let end = matches!(chunk, AiAskChunk::Done(_) | AiAskChunk::Error { .. });
            let Ok(json) = serde_json::to_string(&chunk) else {
                continue;
            };
            if sink.add(json).is_err() || end {
                break;
            }
        }
    });
    Ok(())
}

/// Abort a running Ask request. `false` if it already finished.
pub async fn ai_cancel(request_id: String) -> Result<bool, BridgeError> {
    with_core(move |c| async move { Ok(c.ai_cancel(request_id).await?) }).await
}

/// "Generate command" → `CommandProposalDto` (never executed here).
pub async fn ai_generate_command(
    provider_id: Option<String>,
    request: String,
    snippet_type: Option<String>,
    context_json: Option<String>,
) -> Result<String, BridgeError> {
    let target = target(snippet_type)?;
    let context = context(context_json)?;
    with_core(move |c| async move {
        to_json(
            &c.ai_generate_command(provider_id, request, target, context)
                .await?,
        )
    })
    .await
}

/// "Convert command into a parameterized snippet" → `SnippetDraftDto`
/// (local parsing; the LLM refines it when `use_llm`).
pub async fn ai_convert_to_snippet(
    provider_id: Option<String>,
    command: String,
    snippet_type: Option<String>,
    use_llm: bool,
) -> Result<String, BridgeError> {
    let target = target(snippet_type)?;
    with_core(move |c| async move {
        to_json(
            &c.ai_convert_to_snippet(provider_id, command, target, use_llm)
                .await?,
        )
    })
    .await
}

/// Local risk rules (no provider involved) → `RiskAssessmentDto`.
pub fn ai_assess_risk(
    command: String,
    snippet_type: Option<String>,
    shell: Option<String>,
) -> Result<String, BridgeError> {
    let dialect = match snippet_type.filter(|s| !s.trim().is_empty()) {
        Some(t) => CommandDialect::for_snippet(parse_snippet_type(&t)?, shell.as_deref()),
        None => CommandDialect::default(),
    };
    to_json(&state::core()?.assess_risk(command, dialect))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_types_map_to_targets() {
        assert_eq!(
            target(Some("postgresql".into())).unwrap(),
            Some(CommandTarget::PostgreSql)
        );
        assert_eq!(
            target(Some("http_curl".into())).unwrap(),
            Some(CommandTarget::Curl)
        );
        assert_eq!(target(None).unwrap(), None);
        assert_eq!(
            target(Some("nope".into())).unwrap_err().code,
            "invalid_input"
        );
    }

    #[test]
    fn empty_context_is_default() {
        assert_eq!(context(None).unwrap(), AiContextOptionsDto::default());
        let c = context(Some(
            r#"{"host_id":"h","terminal_id":null,"selected_text":null,"include_terminal":true}"#
                .into(),
        ))
        .unwrap();
        assert!(c.include_terminal);
    }
}
