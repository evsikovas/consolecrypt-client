//! The AI provider API key (and secrets in AI context) never reach the logs:
//! capture everything at TRACE through app-core's logging helper (with the
//! `log` → tracing bridge / LogTracer and user directives that try to
//! re-enable HTTP internals) while running every AI flow against a mock
//! endpoint that requires the key — including a rejected key — then grep.

#[path = "common/ai_mock.rs"]
mod ai_mock;
mod common;

use ai_mock::MockLlm;
use cc_app_core::*;
use common::*;
use std::io::Write;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

impl Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = CaptureWriter;
    fn make_writer(&'a self) -> Self::Writer {
        CaptureWriter(self.0.clone())
    }
}

const API_KEY: &str = "sk-log-canary-7f3a9c1e5b2d4086a1c3e5f7091b3d5f";
const WRONG_KEY: &str = "sk-wrong-canary-0a1b2c3d4e5f60718293a4b5c6d7e8f9";
const PG_PASSWORD: &str = "PgLogPw-canary-5512";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ai_flows_at_trace_leak_no_api_key() {
    let capture = Capture::default();
    logging::init_with_writer(
        Some("trace,reqwest=trace,hyper=trace,hyper_util=trace,h2=trace,rustls=trace"),
        capture.clone(),
    )
    .unwrap();
    tracing::trace!(target: "ai_log_hygiene", "trace level marker");
    log::debug!(target: "ai_log_hygiene", "bridged debug marker");
    log::info!(target: "ai_log_hygiene", "bridged info marker");

    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    app.create_local_profile("AI".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let mock = MockLlm::start().await;
    mock.require_key(API_KEY);
    let p = app
        .save_ai_provider(AiProviderDto {
            id: String::new(),
            name: "mock".into(),
            provider: AiProviderKind::OpenaiCompatible,
            base_url: mock.url(),
            has_api_key: false,
            chat_model: "mock-chat".into(),
            embedding_model: Some("mock-embed".into()),
            timeout_secs: 10,
            streaming: true,
            tool_support: false,
            privacy_profile: PrivacyProfile::Standard,
            is_default: true,
            created_at_ms: 0,
            updated_at_ms: 0,
        })
        .await
        .unwrap();

    // Rejected key first (error paths log too).
    app.set_ai_provider_api_key(p.id.clone(), Some(WRONG_KEY.into()))
        .await
        .unwrap();
    assert_eq!(
        app.test_ai_provider(p.id.clone()).await.unwrap_err().code(),
        "ai_auth_failed"
    );
    app.set_ai_provider_api_key(p.id.clone(), Some(API_KEY.into()))
        .await
        .unwrap();
    app.test_ai_provider(p.id.clone()).await.unwrap();

    let mut snippet = SnippetDto {
        package_name: None,
        catalog_id: None,
        id: String::new(),
        name: "psql billing".into(),
        description: String::new(),
        snippet_type: SnippetType::Bash,
        shell: None,
        template: format!("PGPASSWORD={PG_PASSWORD} psql -h db billing"),
        variables: Vec::new(),
        tags: Vec::new(),
        risk_level: RiskLevel::ReadOnly,
        source: SnippetSource::User,
        created_by_device_id: None,
        last_used_at_ms: None,
        usage_count: 0,
        created_at_ms: 0,
        updated_at_ms: 0,
    };
    app.save_snippet(snippet.clone()).await.unwrap();
    snippet.name = "tail logs".into();
    snippet.template = "tail -f /var/log/syslog".into();
    app.save_snippet(snippet).await.unwrap();

    mock.push_chat(
        r#"{"command": "psql -h db billing -c 'select 1'", "risk_suggestion": "read_only"}"#,
    );
    let mut h = HostDto::new("billing-db", "127.0.0.1");
    h.port = Some(9); // refused: the approved exec fails fast
    let host = app.save_host(h).await.unwrap();
    let ctx = AiContextOptionsDto {
        host_id: Some(host.id.clone()),
        selected_text: Some(format!("PGPASSWORD={PG_PASSWORD}")),
        include_terminal: true,
        ..Default::default()
    };
    let proposal = app
        .ai_generate_command(None, "query billing".into(), None, ctx.clone())
        .await
        .unwrap();
    let approved = app.approve_run(proposal.run, false).await.unwrap();
    let _ = app.exec_approved(approved.token).await;

    mock.push_chat("Streaming answer text.");
    let mut s = app
        .ai_ask(
            None,
            AiAskRequestDto {
                conversation_id: None,
                question: format!("why PGPASSWORD={PG_PASSWORD}?"),
            },
            ctx.clone(),
        )
        .await
        .unwrap();
    while let Some(c) = s.chunks.recv().await {
        if !matches!(c, AiAskChunk::Delta { .. }) {
            break;
        }
    }
    mock.push_chat(r#"{"summary": "x"}"#);
    app.ai_explain_command(None, "ls".into(), None, ctx.clone())
        .await
        .unwrap();
    mock.push_chat(r#"{"command": "tail -f /var/log/syslog", "sources": [1]}"#);
    app.ai_search(
        "logs".into(),
        AiSearchOptionsDto {
            llm: LlmMode::Always,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    eventually(15, "embeddings synced", || async {
        let st = app.ai_index_status().await.unwrap();
        st.documents > 0 && st.embedded == st.documents
    })
    .await;
    app.lock().await.unwrap();
    app.shutdown().await.unwrap();

    let logs = String::from_utf8_lossy(&capture.0.lock().unwrap()).into_owned();
    assert!(logs.len() > 1000, "capture works ({} bytes)", logs.len());
    assert!(logs.contains("trace level marker"), "TRACE captured");
    assert!(logs.contains("bridged info marker"), "log records bridged");
    // Release builds compile out log::debug through release_max_level_info.
    assert_eq!(
        logs.contains("bridged debug marker"),
        log::STATIC_MAX_LEVEL >= log::LevelFilter::Debug,
        "debug records follow the compile-time log level"
    );
    assert!(
        logs.contains("command approved to run"),
        "app-core AI events captured"
    );
    assert!(
        logs.contains("search index rebuilt"),
        "index events captured"
    );
    for secret in [API_KEY, WRONG_KEY, PG_PASSWORD] {
        assert!(!logs.contains(secret), "a secret leaked into the logs");
    }
    let lower = logs.to_lowercase();
    for needle in ["authorization:", "bearer sk-"] {
        assert!(!lower.contains(needle), "logs contain {needle:?}");
    }
    // The mock did receive the key — so it was really in use.
    assert!(mock
        .requests()
        .iter()
        .any(|r| r.authorization.as_deref() == Some(format!("Bearer {API_KEY}").as_str())));
}
