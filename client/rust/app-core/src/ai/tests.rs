//! Unit tests of the AI wiring that need crate internals: a fake execution
//! backend / terminal (no SSH server) for the generate → approve → exec and
//! Fix Last Error flows, and the context provider boundary. Facade-level
//! flows live in `tests/ai.rs`.

#[path = "../../tests/common/ai_mock.rs"]
mod ai_mock;

use super::context::AppAiContext;
use super::session::ExecBackend;
use super::*;
use crate::config::AppConfig;
use ai_mock::MockLlm;
use cc_ai_core::context::AiContextProvider;
use std::sync::Mutex;

const PASSPHRASE: &str = "correct horse vault passphrase 42";

#[tokio::test]
async fn cancelling_a_full_answer_queue_releases_the_pending_sender() {
    let (tx, mut rx) = mpsc::channel(1);
    tx.send(AiAskChunk::Delta {
        text: "public first chunk".into(),
    })
    .await
    .unwrap();
    let token = CancellationToken::new();
    let captured = token.clone();
    let pending = tokio::spawn(async move {
        send_ask_chunk(
            &tx,
            &captured,
            AiAskChunk::Delta {
                text: "must not queue".into(),
            },
        )
        .await
    });
    tokio::task::yield_now().await;
    assert!(!pending.is_finished());
    token.cancel();
    assert!(
        !tokio::time::timeout(std::time::Duration::from_secs(1), pending)
            .await
            .unwrap()
            .unwrap()
    );
    assert!(matches!(rx.recv().await, Some(AiAskChunk::Delta { .. })));
    assert!(rx.recv().await.is_none());
}

/// Records what would have been executed / typed.
#[derive(Default)]
struct FakeBackend {
    terminals: Mutex<HashMap<TerminalId, ObjectId>>,
    last_command: Mutex<Option<String>>,
    last_error: Mutex<Option<String>>,
    execs: Mutex<Vec<(ObjectId, String)>>,
    writes: Mutex<Vec<(TerminalId, Vec<u8>)>>,
}

#[async_trait]
impl ExecBackend for FakeBackend {
    async fn exec(&self, host_id: ObjectId, command: &str) -> AppResult<ExecResultDto> {
        self.execs
            .lock()
            .unwrap()
            .push((host_id, command.to_owned()));
        Ok(ExecResultDto {
            stdout: b"ok\n".to_vec(),
            stderr: Vec::new(),
            exit_status: Some(0),
            exit_signal: None,
        })
    }
    fn terminal_host(&self, id: TerminalId) -> AppResult<ObjectId> {
        self.terminals
            .lock()
            .unwrap()
            .get(&id)
            .copied()
            .ok_or_else(|| AppError::not_found("terminal", id))
    }
    async fn terminal_write(&self, id: TerminalId, data: &[u8]) -> AppResult<()> {
        self.writes.lock().unwrap().push((id, data.to_vec()));
        Ok(())
    }
    fn last_command(&self, id: TerminalId) -> Option<String> {
        self.terminals.lock().unwrap().get(&id)?;
        self.last_command.lock().unwrap().clone()
    }
    fn last_error(&self, id: TerminalId) -> Option<String> {
        self.terminals.lock().unwrap().get(&id)?;
        self.last_error.lock().unwrap().clone()
    }
}

struct Fixture {
    _tmp: tempfile::TempDir,
    app: AppCore,
    mock: MockLlm,
    fake: Arc<FakeBackend>,
    host: HostDto,
}

async fn fixture(profile: PrivacyProfile) -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let app = AppCore::new(AppConfig::for_tests(
        tmp.path().to_string_lossy().to_string(),
    ))
    .unwrap();
    app.create_local_profile("AI".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let mock = MockLlm::start().await;
    app.save_ai_provider(AiProviderDto {
        id: String::new(),
        name: "mock".into(),
        provider: AiProviderKind::OpenaiCompatible,
        base_url: mock.url(),
        has_api_key: false,
        chat_model: "mock-chat".into(),
        embedding_model: None,
        timeout_secs: 10,
        streaming: true,
        tool_support: false,
        privacy_profile: profile,
        is_default: true,
        created_at_ms: 0,
        updated_at_ms: 0,
    })
    .await
    .unwrap();
    let host = app
        .save_host(HostDto::new("pg-main", "10.9.8.7"))
        .await
        .unwrap();
    let fake = Arc::new(FakeBackend::default());
    let (_, u) = app.unlocked().await.unwrap();
    u.ai.set_backend(fake.clone());
    Fixture {
        _tmp: tmp,
        app,
        mock,
        fake,
        host,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generate_approve_and_run_via_exec_and_terminal() {
    let f = fixture(PrivacyProfile::Standard).await;
    let hid = parse_id("id", &f.host.id).unwrap();
    let ctx = AiContextOptionsDto {
        host_id: Some(f.host.id.clone()),
        ..Default::default()
    };
    f.mock.push_chat(r#"{"command": "df -h /var/lib/postgresql", "explanation": "disk usage", "risk_suggestion": "read_only"}"#);
    let p = f
        .app
        .ai_generate_command(None, "how full is the pg disk?".into(), None, ctx.clone())
        .await
        .unwrap();
    assert_eq!(p.command, "df -h /var/lib/postgresql");
    assert!(
        f.fake.execs.lock().unwrap().is_empty(),
        "nothing runs by itself"
    );

    // Exec path.
    let ok = f.app.approve_run(p.run.clone(), false).await.unwrap();
    let out = f.app.exec_approved(ok.token.clone()).await.unwrap();
    assert_eq!(out.stdout, b"ok\n");
    assert_eq!(
        f.fake.execs.lock().unwrap().clone(),
        vec![(hid, "df -h /var/lib/postgresql".to_owned())]
    );
    assert_eq!(
        f.app.exec_approved(ok.token).await.unwrap_err().code(),
        "approval_required",
        "single use"
    );

    // Terminal path: only a terminal of the approved host.
    let tid = uuid::Uuid::new_v4();
    let other_tid = uuid::Uuid::new_v4();
    f.fake.terminals.lock().unwrap().insert(tid, hid);
    f.fake
        .terminals
        .lock()
        .unwrap()
        .insert(other_tid, ObjectId::new());
    let ok = f.app.approve_run(p.run.clone(), false).await.unwrap();
    assert_eq!(
        f.app
            .terminal_run_approved(other_tid.to_string(), ok.token)
            .await
            .unwrap_err()
            .code(),
        "approval_required"
    );
    let ok = f.app.approve_run(p.run.clone(), false).await.unwrap();
    f.app
        .terminal_run_approved(tid.to_string(), ok.token)
        .await
        .unwrap();
    assert_eq!(
        f.fake.writes.lock().unwrap().clone(),
        vec![(tid, b"df -h /var/lib/postgresql\r".to_vec())]
    );

    // A destructive proposal needs the confirmation before anything runs.
    f.mock
        .push_chat(r#"{"command": "dropdb billing", "risk_suggestion": "destructive"}"#);
    let p = f
        .app
        .ai_generate_command(None, "remove the billing db".into(), None, ctx)
        .await
        .unwrap();
    assert_eq!(
        f.app
            .approve_run(p.run.clone(), false)
            .await
            .unwrap_err()
            .code(),
        "confirmation_required"
    );
    assert_eq!(f.fake.execs.lock().unwrap().len(), 1);
    let ok = f.app.approve_run(p.run, true).await.unwrap();
    f.app.exec_approved(ok.token).await.unwrap();
    assert_eq!(f.fake.execs.lock().unwrap()[1].1, "dropdb billing");
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fix_last_error_reads_terminal_hooks_through_the_sanitizer() {
    let f = fixture(PrivacyProfile::Standard).await;
    let hid = parse_id("id", &f.host.id).unwrap();
    let tid = uuid::Uuid::new_v4();
    f.fake.terminals.lock().unwrap().insert(tid, hid);
    *f.fake.last_command.lock().unwrap() =
        Some("PGPASSWORD=HookPw-canary-7781 psql -h db -U app bilSing".into());
    *f.fake.last_error.lock().unwrap() =
        Some("psql: error: FATAL: database \"bilSing\" does not exist".into());
    f.mock.push_chat(r#"{"command": "psql -h db -U app billing", "diagnosis": "typo in the database name", "risk_suggestion": "read_only"}"#);
    let p = f
        .app
        .ai_fix_last_error(None, tid.to_string(), None)
        .await
        .unwrap();
    assert_eq!(p.command, "psql -h db -U app billing");
    assert_eq!(p.diagnosis.as_deref(), Some("typo in the database name"));
    assert_eq!(
        p.run.host_id.as_deref(),
        Some(f.host.id.as_str()),
        "terminal's host"
    );
    let sent = f.mock.requests().last().unwrap().text.clone();
    assert!(
        sent.contains("does not exist") && sent.contains("bilSing"),
        "{sent}"
    );
    assert!(
        !sent.contains("HookPw-canary-7781"),
        "password in the last command leaked"
    );
    assert!(p.redactions.passwords >= 1);

    // Unknown terminal → not found; no failed command → invalid input.
    assert_eq!(
        f.app
            .ai_fix_last_error(None, uuid::Uuid::new_v4().to_string(), None)
            .await
            .unwrap_err()
            .code(),
        "not_found"
    );
    *f.fake.last_command.lock().unwrap() = None;
    *f.fake.last_error.lock().unwrap() = None;
    assert_eq!(
        f.app
            .ai_fix_last_error(None, tid.to_string(), None)
            .await
            .unwrap_err()
            .code(),
        "invalid_input"
    );

    // Explain with no command uses the selection passed by the UI.
    f.mock.push_chat(r#"{"summary": "lists files"}"#);
    let e = f
        .app
        .ai_explain_command(
            None,
            String::new(),
            None,
            AiContextOptionsDto {
                terminal_id: Some(tid.to_string()),
                selected_text: Some("ls -la /etc".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(e.summary, "lists files");
    assert_eq!(e.local_risk.level, RiskLevel::ReadOnly);
    assert!(f
        .mock
        .requests()
        .last()
        .unwrap()
        .text
        .contains("ls -la /etc"));
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn context_provider_exposes_no_credentials() {
    let f = fixture(PrivacyProfile::Standard).await;
    let cred = f
        .app
        .add_password_credential(
            "root".into(),
            Some("root".into()),
            "CtxPw-canary-1234".into(),
        )
        .await
        .unwrap();
    let mut h = f.app.get_host(f.host.id.clone()).await.unwrap();
    h.credential_id = Some(cred.id.clone());
    h.metadata.insert("os".into(), "Debian 12".into());
    h.metadata.insert("shell".into(), "zsh".into());
    let h = f.app.save_host(h).await.unwrap();
    let hid = parse_id("id", &h.id).unwrap();
    f.app
        .save_snippet(SnippetDto {
            package_name: None,
            catalog_id: None,
            id: String::new(),
            name: "Vacuum".into(),
            description: "maintenance".into(),
            snippet_type: SnippetType::Postgresql,
            shell: None,
            template: "VACUUM ANALYZE;".into(),
            variables: Vec::new(),
            tags: Vec::new(),
            risk_level: RiskLevel::Modifying,
            source: SnippetSource::User,
            created_by_device_id: None,
            last_used_at_ms: None,
            usage_count: 0,
            created_at_ms: 0,
            updated_at_ms: 0,
        })
        .await
        .unwrap();
    let (u, ai) = f.app.ai_session().await.unwrap();
    let index = ai.fresh_index().await.unwrap();
    for idx in [None, Some(index)] {
        let (ctx, host) = AppCore::ai_context(
            &u,
            &ai,
            &AiContextOptionsDto {
                host_id: Some(h.id.clone()),
                ..Default::default()
            },
            idx,
        )
        .unwrap();
        assert_eq!(host, Some(hid));
        let hc = ctx.get_host_context(hid).await.unwrap();
        assert_eq!(hc.os.as_deref(), Some("Debian 12"));
        assert_eq!(hc.shell.as_deref(), Some("zsh"));
        let json = serde_json::to_string(&hc).unwrap();
        assert!(
            !json.contains(&cred.id) && !json.contains("CtxPw-canary-1234"),
            "{json}"
        );
        let snippets = ctx.search_snippets("vacuum", 5).await;
        assert_eq!(snippets.len(), 1);
        assert!(ctx.get_last_command().await.is_none(), "no terminal chosen");
        assert!(ctx.get_selected_terminal_text().await.is_none());
    }
    let kb = AppAiContext {
        working: u.working().clone(),
        index: ai.index_if_open(),
        backend: ai.backend(),
        terminal: None,
        selected_text: None,
    }
    .search_local_kb("pg-main", 5)
    .await;
    assert_eq!(kb[0].id, hid);
    assert!(!kb[0].body.contains("CtxPw-canary-1234"));
    f.app.shutdown().await.unwrap();
}

#[test]
fn secret_placeholders_are_found() {
    assert_eq!(
        secret_placeholders(
            "mysql -p<PASSWORD_1> <IP_1> <SECRET_12> <PASSWORD_1> <x_1> <PASSWORD_>"
        ),
        vec!["<PASSWORD_1>", "<SECRET_12>"]
    );
    assert_eq!(
        secret_placeholders("<PRIVATE_KEY_2>"),
        vec!["<PRIVATE_KEY_2>"]
    );
    assert!(secret_placeholders("a < b > c").is_empty());
}

#[test]
fn ai_errors_map_to_stable_codes() {
    let cases: Vec<(AiError, &str)> = vec![
        (AiError::Auth { status: 401 }, "ai_auth_failed"),
        (AiError::Timeout, "ai_unavailable"),
        (AiError::Connect("refused".into()), "ai_unavailable"),
        (AiError::ModelNotFound("m".into()), "ai_provider"),
        (
            AiError::Http {
                status: 500,
                message: "x".into(),
            },
            "ai_provider",
        ),
        (AiError::Cancelled, "cancelled"),
        (AiError::Unsupported("embeddings"), "unsupported"),
        (AiError::Config("bad url".into()), "invalid_input"),
        (
            AiError::RateLimited {
                retry_after_secs: Some(3),
            },
            "rate_limited",
        ),
        (AiError::InvalidInput("empty".into()), "invalid_input"),
    ];
    for (e, code) in cases {
        assert_eq!(AppError::from(e).code(), code);
    }
    assert_eq!(
        policy_error(PolicyError::ConfirmationRequired(RiskLevel::Destructive)).code(),
        "confirmation_required"
    );
    assert!(
        policy_error(PolicyError::ConfirmationRequired(RiskLevel::Destructive))
            .message()
            .contains("destructive")
    );
}
