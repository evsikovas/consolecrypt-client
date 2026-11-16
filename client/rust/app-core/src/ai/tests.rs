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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn automation_ai_sends_only_the_reviewed_context_once_without_execution() {
    let f = fixture(PrivacyProfile::Strict).await;
    let provider = f.app.list_ai_providers().await.unwrap()[0].id.clone();
    assert!(f
        .app
        .automation_ai_prepare("".into(), "task".into(), "playbook.yml".into(), None)
        .await
        .is_err());
    let preview = f
        .app
        .automation_ai_prepare(
            provider.clone(),
            "Create file; password=do-not-transmit-this".into(),
            "playbook.yml".into(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(f.mock.count("/v1/chat/completions"), 0);
    let sent = preview["preview"].as_str().unwrap();
    assert!(!sent.contains("do-not-transmit-this"));
    assert!(!sent.contains("pg-main"));
    f.mock.push_chat(r#"{"version":1,"description":"Create a file","files":[{"path":"playbook.yml","content":"---\n- hosts: all\n  tasks: []\n"}],"variables":[],"dependencies":[{"source":"ansible.builtin","version":"2.12"}],"expected_changes":["Create file"],"checks":["Assert file"],"warnings":[]}"#);
    let id = preview["review_id"].as_str().unwrap().to_owned();
    let draft = f.app.automation_ai_generate(id.clone()).await.unwrap();
    assert_eq!(draft["files"][0]["path"], "playbook.yml");
    assert_eq!(draft["dependencies"], serde_json::json!([]));
    assert!(draft["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|warning| warning
            .as_str()
            .unwrap()
            .contains("Runner's pinned ansible-core")));
    let captured: serde_json::Value =
        serde_json::from_str(&f.mock.requests().last().unwrap().body).unwrap();
    let actual = captured["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|message| message["content"].as_str().unwrap())
        .collect::<Vec<_>>()
        .join("\n\n");
    assert_eq!(actual, sent);
    assert_eq!(captured["response_format"]["type"], "json_object");
    assert!(f.app.automation_ai_generate(id).await.is_err());
    assert_eq!(f.mock.count("/v1/chat/completions"), 1);
    let preview = f
        .app
        .automation_ai_prepare(provider, "New task".into(), "playbook.yml".into(), None)
        .await
        .unwrap();
    let id = preview["review_id"].as_str().unwrap().to_owned();
    f.app.automation_ai_discard(id.clone()).await.unwrap();
    assert!(f.app.automation_ai_generate(id).await.is_err());
    assert_eq!(f.mock.count("/v1/chat/completions"), 1);
    assert!(f.fake.execs.lock().unwrap().is_empty());
    assert!(f.fake.writes.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn automation_ai_uses_schema_where_supported_and_keeps_malformed_output_out_of_execution() {
    let f = fixture(PrivacyProfile::Strict).await;
    let mut configuration = f.app.list_ai_providers().await.unwrap().remove(0);
    configuration.provider = AiProviderKind::LmStudio;
    let provider = f.app.save_ai_provider(configuration).await.unwrap();
    let preview = f
        .app
        .automation_ai_prepare(
            provider.id,
            "Write a debug task".into(),
            "install-docker.yml".into(),
            Some("---\n- hosts: all\n  tasks: []\n".into()),
        )
        .await
        .unwrap();
    f.mock.push_chat("```yaml\n- hosts: all\n  tasks: []\n```");
    let failure = f
        .app
        .automation_ai_generate(preview["review_id"].as_str().unwrap().into())
        .await
        .unwrap_err();
    assert_eq!(
        failure.args().get("field").map(String::as_str),
        Some("automation_ai_response")
    );
    assert_eq!(
        failure.args().get("rule").map(String::as_str),
        Some("invalid_response")
    );
    let captured: serde_json::Value =
        serde_json::from_str(&f.mock.requests().last().unwrap().body).unwrap();
    assert_eq!(captured["response_format"]["type"], "json_schema");
    assert_eq!(
        captured["response_format"]["json_schema"]["schema"]["properties"]["files"]["items"]
            ["properties"]["path"]["const"],
        "install-docker.yml"
    );
    assert!(f.fake.execs.lock().unwrap().is_empty());
    assert!(f.fake.writes.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn automation_ai_cancel_and_lock_abort_a_real_pending_provider_request() {
    for action in ["cancel", "lock", "provider_changed"] {
        let f = fixture(PrivacyProfile::Strict).await;
        let provider = f.app.list_ai_providers().await.unwrap()[0].id.clone();
        let preview = f
            .app
            .automation_ai_prepare(provider, "Create file".into(), "playbook.yml".into(), None)
            .await
            .unwrap();
        let id = preview["review_id"].as_str().unwrap().to_owned();
        let gate = f.mock.block_chat();
        let generation = f.app.automation_ai_generate(id.clone());
        tokio::pin!(generation);
        tokio::select! {
            result=&mut generation=>panic!("provider returned before cancellation: {result:?}"),
            _=tokio::time::timeout(std::time::Duration::from_secs(5),async {
                while f.mock.count("/v1/chat/completions")==0 {tokio::task::yield_now().await;}
            })=>{},
        }
        assert_eq!(f.mock.count("/v1/chat/completions"), 1);
        if action == "lock" {
            f.app.lock().await.unwrap();
        } else if action == "provider_changed" {
            let mut config = f.app.list_ai_providers().await.unwrap().remove(0);
            config.name = "Changed while preview was pending".into();
            f.app.save_ai_provider(config).await.unwrap();
        } else {
            f.app.automation_ai_discard(id.clone()).await.unwrap();
        }
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(3), &mut generation)
                .await
                .unwrap()
                .is_err()
        );
        gate.notify_one();
        assert_eq!(f.mock.count("/v1/chat/completions"), 1);
        assert!(f.fake.execs.lock().unwrap().is_empty());
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn device_local_provider_is_ephemeral_scoped_and_keeps_command_policy() {
    let f = fixture(PrivacyProfile::Standard).await;
    let (session, unlocked) = f.app.unlocked().await.unwrap();
    let profile = session.profile_id.to_string();
    let vault = unlocked.vault_id.to_string();
    let providers_before = f.app.list_ai_providers().await.unwrap();
    let registration = uuid::Uuid::new_v4().to_string();
    // Runtime keys use unpadded base64url, including both URL-safe symbols.
    let key = format!("{}-_", "a".repeat(41));
    let port = f
        .mock
        .base
        .rsplit(':')
        .next()
        .unwrap()
        .parse::<u16>()
        .unwrap();
    f.mock.require_key(&key);
    assert!(f
        .app
        .register_device_local_ai_provider(
            uuid::Uuid::new_v4().to_string(),
            vault.clone(),
            registration.clone(),
            port,
            key.clone(),
            "mock-chat".into()
        )
        .await
        .is_err());
    assert!(f
        .app
        .register_device_local_ai_provider(
            profile.clone(),
            uuid::Uuid::new_v4().to_string(),
            registration.clone(),
            port,
            key.clone(),
            "mock-chat".into()
        )
        .await
        .is_err());
    f.app
        .register_device_local_ai_provider(
            profile.clone(),
            vault.clone(),
            registration.clone(),
            port,
            key.clone(),
            "mock-chat".into(),
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(f.app.list_ai_providers().await.unwrap()).unwrap(),
        serde_json::to_value(&providers_before).unwrap()
    );
    assert_eq!(
        unlocked.ai.resolve_config(None).unwrap().id.to_string(),
        providers_before[0].id
    );
    f.mock.push_chat(r#"{"command":"rm -rf /important","explanation":"dangerous","risk_suggestion":"read_only"}"#);
    let proposal = f
        .app
        .ai_generate_command(
            Some(DEVICE_LOCAL_PROVIDER_ID.into()),
            "remove files; PGPASSWORD=LocalAiSecret7781".into(),
            None,
            AiContextOptionsDto {
                host_id: Some(f.host.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(!f.mock.everything_sent().contains("LocalAiSecret7781"));
    assert!(f
        .mock
        .requests()
        .iter()
        .any(|r| r.authorization.as_deref() == Some(&format!("Bearer {key}"))));
    assert!(f.fake.execs.lock().unwrap().is_empty());
    assert_eq!(
        f.app
            .approve_run(proposal.run, false)
            .await
            .unwrap_err()
            .code(),
        "confirmation_required"
    );
    // An old completion cannot clear the newly registered process.
    f.app
        .clear_device_local_ai_provider(uuid::Uuid::new_v4().to_string())
        .await
        .unwrap();
    assert!(unlocked
        .ai
        .resolve_config(Some(DEVICE_LOCAL_PROVIDER_ID))
        .is_ok());
    f.app
        .clear_device_local_ai_provider(registration.clone())
        .await
        .unwrap();
    assert!(unlocked
        .ai
        .resolve_config(Some(DEVICE_LOCAL_PROVIDER_ID))
        .is_err());
    f.app
        .register_device_local_ai_provider(
            profile,
            vault,
            registration,
            port,
            key,
            "mock-chat".into(),
        )
        .await
        .unwrap();
    f.app.lock().await.unwrap();
    f.app
        .unlock_with_passphrase(PASSPHRASE.into())
        .await
        .unwrap();
    let (_, next) = f.app.unlocked().await.unwrap();
    assert!(next
        .ai
        .resolve_config(Some(DEVICE_LOCAL_PROVIDER_ID))
        .is_err());
    assert_eq!(
        serde_json::to_value(f.app.list_ai_providers().await.unwrap()).unwrap(),
        serde_json::to_value(providers_before).unwrap()
    );
    f.app.shutdown().await.unwrap();
}

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
    terminals: Mutex<HashMap<TerminalId, Option<ObjectId>>>,
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
    fn terminal_host(&self, id: TerminalId) -> AppResult<Option<ObjectId>> {
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
    f.fake.terminals.lock().unwrap().insert(tid, Some(hid));
    f.fake
        .terminals
        .lock()
        .unwrap()
        .insert(other_tid, Some(ObjectId::new()));
    let ok = f.app.approve_run(p.run.clone(), false).await.unwrap();
    assert_eq!(
        f.app
            .terminal_run_approved(other_tid.to_string(), ok.token)
            .await
            .unwrap_err()
            .code(),
        "approval_required"
    );
    // A local terminal has no SSH host. An approval for a remote host must
    // never be reused to execute that command on this computer.
    let local_tid = uuid::Uuid::new_v4();
    f.fake.terminals.lock().unwrap().insert(local_tid, None);
    let ok = f.app.approve_run(p.run.clone(), false).await.unwrap();
    assert_eq!(
        f.app
            .terminal_run_approved(local_tid.to_string(), ok.token)
            .await
            .unwrap_err()
            .code(),
        "approval_required"
    );
    assert!(f.fake.writes.lock().unwrap().is_empty());
    let (unlocked, ai) = f.app.ai_session().await.unwrap();
    let (_, host) = AppCore::ai_context(
        &unlocked,
        &ai,
        &AiContextOptionsDto {
            terminal_id: Some(local_tid.to_string()),
            ..Default::default()
        },
        None,
    )
    .unwrap();
    assert_eq!(host, None, "local context cannot inherit an SSH host");

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
    f.fake.terminals.lock().unwrap().insert(tid, Some(hid));
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
