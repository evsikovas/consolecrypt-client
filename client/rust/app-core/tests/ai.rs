//! AI wiring through the facade (CLIENT_SPEC §11–§16, ADR-0105) against an
//! in-process mock LLM endpoint: search pipeline over the real per-profile
//! index, index lifecycle across lock/unlock, background embedding sync,
//! the sanitizer on the wire, provider checks / API keys, Ask AI streaming
//! and conversation persistence, snippets and the execution gate.

#[path = "common/ai_mock.rs"]
mod ai_mock;
mod common;

use ai_mock::MockLlm;
use cc_app_core::*;
use common::*;
use std::collections::HashMap;
use std::sync::Arc;

// ---- helpers ---------------------------------------------------------------------------

async fn local_app(dir: &std::path::Path) -> (AppCore, String) {
    let app = app(dir);
    let created = app
        .create_local_profile("AI".into(), PASSPHRASE.into())
        .await
        .unwrap();
    (app, created.profile.id)
}

async fn provider(
    app: &AppCore,
    mock: &MockLlm,
    profile: PrivacyProfile,
    embedding_model: Option<&str>,
    api_key: Option<&str>,
) -> AiProviderDto {
    let saved = app
        .save_ai_provider(AiProviderDto {
            id: String::new(),
            name: "mock".into(),
            provider: AiProviderKind::OpenaiCompatible,
            base_url: mock.url(),
            has_api_key: false,
            chat_model: "mock-chat".into(),
            embedding_model: embedding_model.map(str::to_owned),
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
    match api_key {
        Some(k) => app
            .set_ai_provider_api_key(saved.id.clone(), Some(k.into()))
            .await
            .unwrap(),
        None => saved,
    }
}

fn snippet(name: &str, template: &str, tags: &[&str]) -> SnippetDto {
    SnippetDto {
        package_name: None,
        catalog_id: None,
        id: String::new(),
        name: name.into(),
        description: String::new(),
        snippet_type: SnippetType::Bash,
        shell: None,
        template: template.into(),
        variables: Vec::new(),
        tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        risk_level: RiskLevel::ReadOnly,
        source: SnippetSource::User,
        created_by_device_id: None,
        last_used_at_ms: None,
        usage_count: 0,
        created_at_ms: 0,
        updated_at_ms: 0,
    }
}

fn note(title: &str, body: &str) -> NoteDto {
    NoteDto {
        id: String::new(),
        title: title.into(),
        body: body.into(),
        tags: Vec::new(),
        created_at_ms: 0,
        updated_at_ms: 0,
    }
}

fn local_only(limit: u32) -> AiSearchOptionsDto {
    AiSearchOptionsDto {
        limit,
        llm: LlmMode::Never,
        ..Default::default()
    }
}

async fn titles(app: &AppCore, query: &str) -> Vec<String> {
    app.ai_search(query.into(), local_only(20))
        .await
        .map(|r| r.hits.into_iter().map(|h| h.title).collect())
        .unwrap_or_default()
}

async fn collect(mut s: AiAskStream) -> (String, AiAskChunk) {
    let mut text = String::new();
    while let Some(c) = s.chunks.recv().await {
        match c {
            AiAskChunk::Delta { text: t } => text.push_str(&t),
            other => return (text, other),
        }
    }
    panic!("stream ended without Done/Error");
}

// ---- search pipeline -------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn search_pipeline_over_the_real_index() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, pid) = local_app(tmp.path()).await;
    let logs = app
        .save_snippet(snippet(
            "Tail pod logs",
            "kubectl logs -n {{namespace}} {{pod}} --tail=100",
            &["k8s"],
        ))
        .await
        .unwrap();
    app.save_snippet(snippet("Disk usage", "df -h /var/lib", &[]))
        .await
        .unwrap();
    app.save_note(note(
        "Postgres vacuum",
        "run vacuum analyze weekly on the billing db",
    ))
    .await
    .unwrap();
    let mut h = HostDto::new("billing-db", "10.20.30.40");
    h.notes = "primary postgres".into();
    app.save_host(h).await.unwrap();

    // Local only (no provider): FTS with prefix matching, kind filter.
    let r = app
        .ai_search("kub log".into(), local_only(10))
        .await
        .unwrap();
    assert_eq!(r.hits[0].id, logs.id);
    assert_eq!(r.hits[0].kind, DocKind::Snippet);
    assert!(r.answer.is_none());
    assert!(r
        .stages
        .iter()
        .any(|s| s.stage == StageKind::Semantic && !s.ran));
    let r = app
        .ai_search(
            "postgres".into(),
            AiSearchOptionsDto {
                kinds: vec![DocKind::Host],
                ..local_only(10)
            },
        )
        .await
        .unwrap();
    assert_eq!(r.hits.len(), 1);
    assert_eq!(r.hits[0].title, "billing-db");
    let exact = app
        .ai_search("Disk usage".into(), local_only(5))
        .await
        .unwrap();
    assert!(exact.hits[0].exact);
    assert!(matches!(
        app.ai_search("   ".into(), local_only(5)).await,
        Err(AppError::InvalidInput { .. })
    ));

    // Incremental updates from object events: create, rename, delete.
    let s = app
        .save_snippet(snippet(
            "Restart nginx",
            "sudo systemctl restart nginx",
            &[],
        ))
        .await
        .unwrap();
    eventually(10, "new snippet indexed", || async {
        titles(&app, "nginx")
            .await
            .contains(&"Restart nginx".to_owned())
    })
    .await;
    let mut renamed = s.clone();
    renamed.name = "Reload web server".into();
    app.save_snippet(renamed).await.unwrap();
    eventually(10, "renamed snippet re-indexed", || async {
        titles(&app, "reload web").await == vec!["Reload web server".to_owned()]
    })
    .await;
    app.delete_snippet(s.id.clone()).await.unwrap();
    eventually(10, "deleted snippet removed", || async {
        titles(&app, "nginx").await.is_empty()
    })
    .await;

    let status = app.ai_index_status().await.unwrap();
    assert_eq!(status.state, AiIndexStateDto::Ready);
    assert_eq!(status.documents, 4, "2 snippets + 1 note + 1 host");

    // The index file lives in the profile directory and is encrypted.
    let db = tmp.path().join("profiles").join(&pid).join("search.db");
    let header = std::fs::read(&db).unwrap();
    assert!(
        !header.starts_with(b"SQLite format 3"),
        "index must be SQLCipher"
    );

    // With a provider and no exact hit: RAG over the local hits.
    let mock = MockLlm::start().await;
    provider(&app, &mock, PrivacyProfile::Standard, None, None).await;
    mock.push_chat(
        r#"{"command": "kubectl logs -n prod web-1 --tail=100", "explanation": "tails the pod", "risk_suggestion": "read_only", "sources": [1]}"#,
    );
    let r = app
        .ai_search(
            "show logs of the web pod".into(),
            AiSearchOptionsDto {
                llm: LlmMode::Auto,
                ..local_only(5)
            },
        )
        .await
        .unwrap();
    let answer = r.answer.expect("rag answer");
    assert_eq!(answer.origin, AnswerOrigin::Rag);
    assert_eq!(
        answer.proposal.command,
        "kubectl logs -n prod web-1 --tail=100"
    );
    assert_eq!(answer.proposal.run.risk, RiskLevel::ReadOnly);
    assert!(!answer.proposal.run.proposal_id.is_empty());
    assert_eq!(answer.source_ids, vec![r.hits[0].id.clone()]);
    assert!(r.stages.iter().any(|s| s.stage == StageKind::Rag && s.ran));
    let sent = mock.everything_sent();
    assert!(
        sent.contains("kubectl logs"),
        "local hits are the RAG context"
    );
    app.shutdown().await.unwrap();
}

// ---- index lifecycle ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn index_is_dropped_on_lock_and_rebuilt_after_unlock() {
    let tmp = tempfile::tempdir().unwrap();
    let store = memory_store();
    let app = app_with(tmp.path(), store.clone(), false);
    let pid = app
        .create_local_profile("AI".into(), PASSPHRASE.into())
        .await
        .unwrap()
        .profile
        .id;
    app.save_snippet(snippet("Tail pod logs", "kubectl logs {{pod}}", &[]))
        .await
        .unwrap();
    assert_eq!(titles(&app, "kubectl").await, vec!["Tail pod logs"]);
    let key_name = format!("profiles/{pid}/search-index-key");
    assert!(
        store.get(&key_name).unwrap().is_some(),
        "random index key in the SecureStore"
    );

    // Locked: every AI call refuses; the file can be removed (handle closed).
    app.lock().await.unwrap();
    for r in [
        app.ai_search("kubectl".into(), local_only(5)).await.err(),
        app.ai_index_status().await.err(),
    ] {
        assert_eq!(r.map(|e| e.code()), Some("vault_locked"));
    }
    let dir = tmp.path().join("profiles").join(&pid);
    for f in ["search.db", "search.db-wal", "search.db-shm"] {
        let _ = std::fs::remove_file(dir.join(f));
    }

    // Unlock: a fresh index is built from the vault.
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert_eq!(titles(&app, "kubectl").await, vec!["Tail pod logs"]);
    assert!(dir.join("search.db").exists());

    // Lost key (e.g. keychain reset): the unreadable file is recreated and
    // rebuilt, never an error for the user.
    app.lock().await.unwrap();
    store.delete(&key_name).unwrap();
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert_eq!(titles(&app, "kubectl").await, vec!["Tail pod logs"]);
    let status = app.ai_index_status().await.unwrap();
    assert_eq!(
        (status.state, status.documents),
        (AiIndexStateDto::Ready, 1)
    );

    // Removing the profile removes its index key too.
    app.remove_profile(pid).await.unwrap();
    assert!(store.get(&key_name).unwrap().is_none());
}

// ---- embeddings ------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn embedding_sync_follows_the_default_provider() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, _) = local_app(tmp.path()).await;
    app.save_snippet(snippet(
        "Tail pod logs",
        "kubectl logs {{pod}} --tail=50",
        &[],
    ))
    .await
    .unwrap();
    app.save_snippet(snippet("Disk usage", "df -h /", &[]))
        .await
        .unwrap();
    let mock = MockLlm::start().await;
    let p = provider(&app, &mock, PrivacyProfile::Local, Some("mock-embed"), None).await;

    let embedded_all = |model: &'static str| {
        let app = app.clone();
        move || {
            let app = app.clone();
            async move {
                let s = app.ai_index_status().await.unwrap();
                s.documents > 0
                    && s.embedded == s.documents
                    && s.embedding_model.as_deref() == Some(model)
            }
        }
    };
    eventually(
        15,
        "all documents embedded",
        embedded_all("openai-compatible:mock-embed"),
    )
    .await;
    assert!(mock.count("/v1/embeddings") >= 2, "probe + batch");
    assert!(mock
        .requests()
        .iter()
        .filter(|r| r.path == "/v1/embeddings")
        .all(|r| r.model.as_deref() == Some("mock-embed")));

    // Semantic stage runs; "container output" shares no word with the
    // snippet, but the toy vectors do.
    let r = app
        .ai_search(
            "pod container".into(),
            AiSearchOptionsDto {
                llm: LlmMode::Never,
                semantic: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(r
        .stages
        .iter()
        .any(|s| s.stage == StageKind::Semantic && s.ran));
    assert_eq!(r.hits[0].title, "Tail pod logs");

    // Incremental: a new document gets a vector.
    app.save_note(note("Kubernetes pods", "kubectl get pods -A"))
        .await
        .unwrap();
    eventually(15, "new note embedded", || async {
        let s = app.ai_index_status().await.unwrap();
        s.documents == 3 && s.embedded == 3
    })
    .await;

    // Config change restarts the sync with the new model (vectors dropped
    // and recomputed).
    let before = mock.embedded_texts();
    let mut changed = p.clone();
    changed.embedding_model = Some("mock-embed-2".into());
    app.save_ai_provider(changed).await.unwrap();
    eventually(
        15,
        "re-embedded with the new model",
        embedded_all("openai-compatible:mock-embed-2"),
    )
    .await;
    assert!(mock.embedded_texts() >= before + 3);
    assert!(mock
        .requests()
        .iter()
        .any(|r| r.model.as_deref() == Some("mock-embed-2")));

    // Removing the embedding model stops the sync (no further calls).
    let mut none = app.list_ai_providers().await.unwrap().remove(0);
    none.embedding_model = None;
    app.save_ai_provider(none).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let calls = mock.count("/v1/embeddings");
    app.save_note(note("Another", "more text")).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert_eq!(mock.count("/v1/embeddings"), calls);
    app.shutdown().await.unwrap();
}

// ---- sanitizer on the wire -----------------------------------------------------------------

const CRED_PASSWORD: &str = "CredPw-canary-4471";
const PG_PASSWORD: &str = "PgPw-canary-9921";
const DUMP_PASSWORD: &str = "DumpPw-canary-3310";
const TOKEN: &str = "ghp_CanaryTokenAbcdefghijklmnopqrstuv0123";
const API_KEY: &str = "sk-apikey-canary-0123456789abcdef0123456789";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn secrets_never_reach_the_provider() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, _) = local_app(tmp.path()).await;
    let mock = MockLlm::start().await;
    mock.require_key(API_KEY);
    provider(
        &app,
        &mock,
        PrivacyProfile::Standard,
        Some("mock-embed"),
        Some(API_KEY),
    )
    .await;
    // A host whose credential holds a password (a Secret object), and
    // "poisoned" knowledge-base content with secrets in it.
    let cred = app
        .add_password_credential(
            "db admin".into(),
            Some("postgres".into()),
            CRED_PASSWORD.into(),
        )
        .await
        .unwrap();
    let mut h = HostDto::new("billing-db", "10.20.30.40");
    h.credential_id = Some(cred.id.clone());
    h.notes = format!("export GITHUB_TOKEN={TOKEN}");
    let host = app.save_host(h).await.unwrap();
    app.save_snippet(snippet(
        "psql billing",
        &format!("PGPASSWORD={PG_PASSWORD} psql -h db -U app billing"),
        &[],
    ))
    .await
    .unwrap();
    app.save_note(note(
        "backup",
        &format!("mysqldump --password={DUMP_PASSWORD} shop > shop.sql"),
    ))
    .await
    .unwrap();
    let ctx = AiContextOptionsDto {
        host_id: Some(host.id.clone()),
        selected_text: Some(format!(
            "$ env\nPGPASSWORD={PG_PASSWORD}\nAuthorization: Bearer {TOKEN}"
        )),
        include_terminal: true,
        ..Default::default()
    };

    mock.push_chat(r#"{"command": "psql -h db -U app -c 'select 1'", "explanation": "check", "risk_suggestion": "read_only"}"#);
    let p = app
        .ai_generate_command(None, "check the billing database".into(), None, ctx.clone())
        .await
        .unwrap();
    assert!(
        p.redactions.passwords + p.redactions.secrets >= 2,
        "{:?}",
        p.redactions
    );
    assert_eq!(p.privacy_profile, PrivacyProfile::Standard);

    mock.push_chat("Use `psql` with the stored credential.");
    let (_, done) = collect(
        app.ai_ask(
            None,
            AiAskRequestDto {
                conversation_id: None,
                question: format!("why does PGPASSWORD={PG_PASSWORD} fail?"),
            },
            ctx.clone(),
        )
        .await
        .unwrap(),
    )
    .await;
    assert!(matches!(done, AiAskChunk::Done(_)), "{done:?}");

    mock.push_chat(r#"{"summary": "connects to postgres"}"#);
    app.ai_explain_command(
        None,
        format!("PGPASSWORD={PG_PASSWORD} psql -h db"),
        None,
        ctx.clone(),
    )
    .await
    .unwrap();

    mock.push_chat(r#"{"command": "mysqldump shop > shop.sql", "sources": [1]}"#);
    let r = app
        .ai_search(
            "backup shop database".into(),
            AiSearchOptionsDto {
                llm: LlmMode::Always,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(r.answer.is_some());
    eventually(15, "poisoned docs embedded", || async {
        let s = app.ai_index_status().await.unwrap();
        s.documents > 0 && s.embedded == s.documents
    })
    .await;

    let sent = mock.everything_sent();
    assert!(mock.count("/v1/chat/completions") >= 4 && mock.count("/v1/embeddings") >= 1);
    for secret in [CRED_PASSWORD, PG_PASSWORD, DUMP_PASSWORD, TOKEN, API_KEY] {
        assert!(!sent.contains(secret), "{secret} reached the provider");
    }
    // The API key is only ever sent as the Authorization header (requests
    // made before the key was set — the background embedding sync starts
    // as soon as the provider is saved — carry none).
    let bearer = format!("Bearer {API_KEY}");
    for r in mock.requests() {
        if r.path == "/v1/chat/completions" || r.authorization.is_some() {
            assert_eq!(
                r.authorization.as_deref(),
                Some(bearer.as_str()),
                "{}",
                r.path
            );
        }
    }
    // … and never comes back through the facade.
    let dto = format!("{:?}", app.list_ai_providers().await.unwrap());
    assert!(!dto.contains(API_KEY) && dto.contains("has_api_key: true"));
    app.shutdown().await.unwrap();
}

// ---- provider checks ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn provider_test_keys_and_selection() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, _) = local_app(tmp.path()).await;
    assert_eq!(
        app.ai_generate_command(None, "x".into(), None, Default::default())
            .await
            .unwrap_err()
            .code(),
        "ai_not_configured"
    );
    let mock = MockLlm::start().await;
    mock.require_key("right-key-0000000000000000");
    let p = provider(
        &app,
        &mock,
        PrivacyProfile::Local,
        None,
        Some("wrong-key-00000000000000000"),
    )
    .await;
    let err = app.test_ai_provider(p.id.clone()).await.unwrap_err();
    assert_eq!(err.code(), "ai_auth_failed");
    // A new key → new config revision → a new provider (no stale cache).
    app.set_ai_provider_api_key(p.id.clone(), Some("right-key-0000000000000000".into()))
        .await
        .unwrap();
    let t = app.test_ai_provider(p.id.clone()).await.unwrap();
    assert_eq!(t.models, vec!["mock-chat", "mock-embed"]);
    assert!(t.chat_model_available);
    assert_eq!(t.locality, Locality::Loopback);
    assert_eq!(t.effective_privacy_profile, PrivacyProfile::Local);
    assert!(t.capabilities.chat && t.capabilities.streaming && !t.capabilities.embeddings);

    // Unreachable endpoint.
    let mut down = p.clone();
    down.base_url = "http://127.0.0.1:9/v1".into();
    down.is_default = false;
    down.id = String::new();
    let down = app.save_ai_provider(down).await.unwrap();
    assert_eq!(
        app.test_ai_provider(down.id.clone())
            .await
            .unwrap_err()
            .code(),
        "ai_unavailable"
    );
    // Explicit provider id wins over the default; unknown ids are not found.
    assert_eq!(
        app.ai_generate_command(Some(down.id.clone()), "x".into(), None, Default::default())
            .await
            .unwrap_err()
            .code(),
        "ai_unavailable"
    );
    assert_eq!(
        app.test_ai_provider(uuid_like()).await.unwrap_err().code(),
        "not_found"
    );
    app.shutdown().await.unwrap();
}

fn uuid_like() -> String {
    "0190a2b4-0000-7000-8000-000000000001".into()
}

// ---- Ask AI & conversations --------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ask_streams_and_persists_conversations_when_allowed() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, _) = local_app(tmp.path()).await;
    let mock = MockLlm::start().await;
    provider(&app, &mock, PrivacyProfile::Strict, None, None).await;
    let h = app
        .save_host(HostDto::new("web-7", "203.0.113.7"))
        .await
        .unwrap();
    let ctx = AiContextOptionsDto {
        host_id: Some(h.id.clone()),
        ..Default::default()
    };

    // Not allowed by the vault settings: memory only.
    mock.push_chat("Check <HOST_1> with `uptime` — it runs on <IP_1>.");
    let s = app
        .ai_ask(
            None,
            AiAskRequestDto {
                conversation_id: None,
                question: "is web-7 overloaded?".into(),
            },
            ctx.clone(),
        )
        .await
        .unwrap();
    let conv = s.conversation_id.clone();
    let (text, done) = collect(s).await;
    assert_eq!(
        text, "Check web-7 with `uptime` — it runs on 203.0.113.7.",
        "re-hydrated locally"
    );
    let AiAskChunk::Done(summary) = done else {
        panic!("{done:?}")
    };
    assert!(!summary.persisted);
    assert!(summary.redactions.hosts + summary.redactions.ips >= 2);
    let sent = mock.everything_sent();
    assert!(
        !sent.contains("web-7") && !sent.contains("203.0.113.7"),
        "strict hides host metadata"
    );
    assert!(app
        .ai_list_conversations()
        .await
        .unwrap()
        .iter()
        .any(|c| c.id == conv && !c.persisted && c.messages.len() == 2));

    // Allowed: every completed turn is stored as an AiConversation object.
    let mut settings = app.get_vault_settings().await.unwrap();
    settings.sync_ai_conversations = true;
    app.save_vault_settings(settings).await.unwrap();
    mock.push_chat("Yes, load is high.");
    let (_, done) = collect(
        app.ai_ask(
            None,
            AiAskRequestDto {
                conversation_id: Some(conv.clone()),
                question: "and now? PGPASSWORD=AskPw-canary-6612 psql".into(),
            },
            ctx.clone(),
        )
        .await
        .unwrap(),
    )
    .await;
    assert!(matches!(
        done,
        AiAskChunk::Done(AiAskSummaryDto {
            persisted: true,
            ..
        })
    ));
    // The follow-up carried the history (placeholders stable across turns).
    let last = mock.requests().last().unwrap().text.clone();
    assert!(
        last.contains("is <HOST_1> overloaded?") && last.contains("and now?"),
        "{last}"
    );
    let meta = app.object_meta(conv.clone()).await.unwrap();
    assert_eq!(meta.kind, ObjectKind::AiConversation);
    let listed = app.ai_list_conversations().await.unwrap();
    let c = listed.iter().find(|c| c.id == conv).unwrap();
    assert!(c.persisted);
    assert_eq!(c.messages.len(), 4);
    assert_eq!(c.title, "is web-7 overloaded?");
    // A secret typed into a question is not persisted.
    let stored = format!("{:?}", c.messages);
    assert!(!stored.contains("AskPw-canary-6612"), "{stored}");
    assert!(stored.contains("PGPASSWORD=<PASSWORD_1>"), "{stored}");

    // Survives lock/unlock (persisted), and can be continued.
    app.lock().await.unwrap();
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert!(app
        .ai_list_conversations()
        .await
        .unwrap()
        .iter()
        .any(|c| c.id == conv));
    mock.push_chat("ok");
    let (_, done) = collect(
        app.ai_ask(
            None,
            AiAskRequestDto {
                conversation_id: Some(conv.clone()),
                question: "thanks".into(),
            },
            ctx,
        )
        .await
        .unwrap(),
    )
    .await;
    assert!(matches!(done, AiAskChunk::Done(_)));
    app.ai_delete_conversation(conv.clone()).await.unwrap();
    assert!(!app
        .ai_list_conversations()
        .await
        .unwrap()
        .iter()
        .any(|c| c.id == conv));

    // Provider errors arrive as an Error chunk.
    mock.require_key("some-key-that-is-not-sent-0000");
    let (_, err) = collect(
        app.ai_ask(
            None,
            AiAskRequestDto {
                conversation_id: None,
                question: "hi".into(),
            },
            Default::default(),
        )
        .await
        .unwrap(),
    )
    .await;
    assert!(
        matches!(err, AiAskChunk::Error { ref code, .. } if code == "ai_auth_failed"),
        "{err:?}"
    );
    app.shutdown().await.unwrap();
}

// ---- snippets --------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snippet_form_render_and_convert() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, _) = local_app(tmp.path()).await;
    let mut s = snippet(
        "Tail pod logs",
        "kubectl logs -n {{namespace}} {{pod}} --tail={{lines}}",
        &[],
    );
    s.variables = vec![
        SnippetVariableDto {
            name: "namespace".into(),
            description: "k8s namespace".into(),
            default: Some("default".into()),
            required: true,
        },
        SnippetVariableDto {
            name: "pod".into(),
            description: String::new(),
            default: None,
            required: true,
        },
        SnippetVariableDto {
            name: "lines".into(),
            description: String::new(),
            default: Some("100".into()),
            required: true,
        },
    ];
    let s = app.save_snippet(s).await.unwrap();
    let form = app.snippet_form(s.id.clone()).await.unwrap();
    let names: Vec<&str> = form.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["namespace", "pod", "lines"]);
    assert_eq!(form.fields[0].default.as_deref(), Some("default"));

    let missing = app
        .snippet_render(s.id.clone(), HashMap::new())
        .await
        .unwrap();
    assert!(missing.command.is_none());
    assert_eq!(missing.field_errors[0].name, "pod");

    let values: HashMap<String, String> = [("pod".to_owned(), "web-1; rm -rf /".to_owned())]
        .into_iter()
        .collect();
    let r = app.snippet_render(s.id.clone(), values).await.unwrap();
    let cmd = r.command.unwrap();
    assert_eq!(cmd, "kubectl logs -n default 'web-1; rm -rf /' --tail=100");
    let run = r.run.unwrap();
    assert_eq!(run.origin, ProposalOrigin::Snippet);
    assert_eq!(
        run.risk,
        RiskLevel::ReadOnly,
        "the injection stayed a quoted argument"
    );

    // Convert a command: secrets are removed locally, no provider needed.
    let d = app
        .ai_convert_to_snippet(
            None,
            "mysql -u root -pS3cretPw-canary -h db.internal shop".into(),
            None,
            false,
        )
        .await
        .unwrap();
    assert!(d.secrets_removed >= 1);
    assert!(
        !d.snippet.template.contains("S3cretPw-canary"),
        "{}",
        d.snippet.template
    );
    assert!(d.snippet.id.is_empty());
    let saved = app.save_snippet(d.snippet.clone()).await.unwrap();
    assert!(!saved.id.is_empty());
    app.shutdown().await.unwrap();
}

// ---- execution gate ------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn risk_gate_requires_confirmation_and_single_use_tokens() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, _) = local_app(tmp.path()).await;
    let mock = MockLlm::start().await;
    provider(&app, &mock, PrivacyProfile::Standard, None, None).await;
    let mut h = HostDto::new("app-1", "127.0.0.1");
    h.port = Some(9);
    let host = app.save_host(h).await.unwrap();
    let ctx = AiContextOptionsDto {
        host_id: Some(host.id.clone()),
        ..Default::default()
    };

    let a = app.assess_risk("rm -rf /var/lib/app".into(), CommandDialect::Posix);
    assert_eq!(a.level, RiskLevel::Destructive);
    assert!(a.requires_confirmation && !a.reasons.is_empty());
    assert!(
        !app.assess_risk("ls -la".into(), CommandDialect::Posix)
            .requires_confirmation
    );

    mock.push_chat(r#"{"command": "rm -rf /var/lib/app/cache", "explanation": "clears the cache", "risk_suggestion": "modifying"}"#);
    let p = app
        .ai_generate_command(None, "clear the app cache".into(), None, ctx.clone())
        .await
        .unwrap();
    assert_eq!(
        p.run.risk,
        RiskLevel::Destructive,
        "local rules win over the AI's 'modifying'"
    );
    assert!(p.run.requires_confirmation);
    assert_eq!(p.run.host_id.as_deref(), Some(host.id.as_str()));
    assert_eq!(
        app.approve_run(p.run.clone(), false)
            .await
            .unwrap_err()
            .code(),
        "confirmation_required"
    );
    // Tampering with the DTO does not help: the gate re-classifies.
    let mut tampered = p.run.clone();
    tampered.risk = RiskLevel::ReadOnly;
    tampered.local_risk = RiskLevel::ReadOnly;
    tampered.requires_confirmation = false;
    tampered.origin = ProposalOrigin::User;
    assert_eq!(
        app.approve_run(tampered, false).await.unwrap_err().code(),
        "confirmation_required"
    );
    let mut no_host = p.run.clone();
    no_host.host_id = None;
    assert_eq!(
        app.approve_run(no_host, true).await.unwrap_err().code(),
        "invalid_input"
    );

    let ok = app.approve_run(p.run.clone(), true).await.unwrap();
    assert_eq!(ok.command, "rm -rf /var/lib/app/cache");
    assert_eq!(ok.risk, RiskLevel::Destructive);
    assert_eq!(ok.host_id, host.id);
    assert!(ok.expires_at_ms > 0);
    // The token is consumed by the attempt (the host is unreachable here).
    let first = app.exec_approved(ok.token.clone()).await.unwrap_err();
    assert_ne!(first.code(), "approval_required", "{first:?}");
    assert_eq!(
        app.exec_approved(ok.token.clone())
            .await
            .unwrap_err()
            .code(),
        "approval_required"
    );
    assert_eq!(
        app.exec_approved("not-a-token".into())
            .await
            .unwrap_err()
            .code(),
        "approval_required"
    );

    // The AI's risk hint of a stored proposal is re-applied even if the DTO
    // drops it: "ls" flagged destructive by the model still needs a yes.
    mock.push_chat(r#"{"command": "ls /srv", "risk_suggestion": "destructive"}"#);
    let p = app
        .ai_generate_command(None, "list".into(), None, ctx.clone())
        .await
        .unwrap();
    let mut dropped = p.run.clone();
    dropped.ai_risk_suggestion = None;
    assert_eq!(
        app.approve_run(dropped, false).await.unwrap_err().code(),
        "confirmation_required"
    );

    // Placeholders block regardless of confirmation.
    mock.push_chat(r#"{"command": "tar czf {{archive}} /srv", "risk_suggestion": "read_only"}"#);
    let p = app
        .ai_generate_command(None, "archive /srv".into(), None, ctx.clone())
        .await
        .unwrap();
    assert!(p.form.is_some());
    assert_eq!(
        app.approve_run(p.run.clone(), true).await.unwrap_err(),
        AppError::UnresolvedPlaceholders(vec!["{{archive}}".into()])
    );
    let mut filled = p.run.clone();
    filled.command = "tar czf /tmp/srv.tgz /srv".into();
    assert!(app.approve_run(filled, true).await.is_ok());

    // Read-only commands need no confirmation; tokens die with the lock.
    mock.push_chat(r#"{"command": "uptime", "risk_suggestion": "read_only"}"#);
    let p = app
        .ai_generate_command(None, "load".into(), None, ctx)
        .await
        .unwrap();
    assert!(!p.run.requires_confirmation);
    let ok = app.approve_run(p.run, false).await.unwrap();
    app.lock().await.unwrap();
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert_eq!(
        app.exec_approved(ok.token).await.unwrap_err().code(),
        "approval_required"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn facade_refuses_ai_calls_without_an_unlocked_vault() {
    let tmp = tempfile::tempdir().unwrap();
    let app = Arc::new(app(tmp.path()));
    assert_eq!(
        app.ai_search("x".into(), local_only(1))
            .await
            .unwrap_err()
            .code(),
        "no_active_profile"
    );
}
