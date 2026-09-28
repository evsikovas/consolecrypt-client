//! "No secret leaks through any feature prompt" (CLIENT_SPEC §14/§15/§19).
//!
//! The context provider is poisoned with secrets in every field an AI
//! feature can read (selection, last command, last error, snippets, KB,
//! request text). Every feature is run under every privacy profile and the
//! test asserts that none of the secrets appears in anything sent to the
//! provider — at the `LlmProvider` level and on the wire (raw HTTP bodies),
//! and that neither secrets nor the API key show up in logs.

mod common;

use axum::body::Bytes;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Router;
use cc_ai_core::context::{HostContext, KbHit, StaticContext};
use cc_ai_core::pipeline::{LlmMode, SearchOptions};
use cc_ai_core::provider::build_provider;
use cc_ai_core::sanitizer::PrivacyProfile;
use cc_ai_core::{
    AiAssistant, AskOptions, CommandTarget, ConvertRequest, CreateSnippetRequest, ExplainRequest,
    FixRequest, GenerateRequest, LlmProvider,
};
use cc_models::ai::AiProviderKind;
use cc_models::snippet::{RiskLevel, Snippet, SnippetSource, SnippetType};
use cc_models::ObjectId;
use cc_search_core::DocKind;
use common::MockProvider;
use futures::StreamExt;
use secrecy::SecretString;
use std::sync::{Arc, Mutex};

fn gen(alphabet: &str, n: usize, seed: u64) -> String {
    let a: Vec<char> = alphabet.chars().collect();
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            a[(x % a.len() as u64) as usize]
        })
        .collect()
}
const B62: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
const B64: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

struct Secrets {
    all: Vec<String>,
    key_line: String,
    pg: String,
    mysql: String,
    sshpass: String,
    curl: String,
    url_pw: String,
    aws: String,
    gh: String,
    jwt: String,
    cookie: String,
    desc_pw: String,
    kb_key: String,
    openai: String,
}

fn secrets() -> Secrets {
    let s = Secrets {
        key_line: gen(B64, 64, 1),
        pg: "PgLeakPass-1".into(),
        mysql: "MyLeakPass2".into(),
        sshpass: "SshLeak!Pass3".into(),
        curl: "CurlLeak4pw".into(),
        url_pw: "UrlLeak5pw".into(),
        aws: gen(B64, 40, 2),
        gh: format!("{}{}", "gh".to_owned() + "p_", gen(B62, 36, 3)),
        jwt: format!(
            "{}.{}.{}",
            "ey".to_owned() + "JhbGciOiJIUzI1NiJ9",
            "ey".to_owned() + "JzdWIiOiJsZWFrIn0",
            gen(B62, 43, 4)
        ),
        cookie: "CookieLeakValue6".into(),
        desc_pw: "DescLeak7Secret".into(),
        kb_key: gen(B62, 40, 5),
        openai: format!("{}{}", "s".to_owned() + "k-", gen(B62, 48, 6)),
        all: Vec::new(),
    };
    let all = vec![
        s.key_line.clone(),
        s.pg.clone(),
        s.mysql.clone(),
        s.sshpass.clone(),
        s.curl.clone(),
        s.url_pw.clone(),
        s.aws.clone(),
        s.gh.clone(),
        s.jwt.clone(),
        s.cookie.clone(),
        s.desc_pw.clone(),
        s.kb_key.clone(),
        s.openai.clone(),
    ];
    Secrets { all, ..s }
}

fn poisoned(host_id: ObjectId, s: &Secrets) -> StaticContext {
    let now = chrono::Utc::now();
    let pem = format!(
        "-----BEGIN OPENSSH PRIVATE KEY-----\n{}\n{}\n-----END OPENSSH PRIVATE KEY-----",
        s.key_line,
        gen(B64, 64, 9)
    );
    StaticContext {
        hosts: vec![HostContext {
            host_id: Some(host_id),
            name: "prod-api".into(),
            address: "10.1.1.10".into(),
            port: Some(22),
            username: Some("deploy".into()),
            tags: vec!["prod".into()],
            os: Some("Debian 12".into()),
            shell: Some("bash".into()),
        }],
        selected_text: Some(format!(
            "$ cat ~/.ssh/id_ed25519\n{pem}\n$ env\nAWS_SECRET_ACCESS_KEY={}\nAuthorization: Bearer {}",
            s.aws, s.jwt
        )),
        last_command: Some(format!(
            "PGPASSWORD={} psql -h db -U app && mysql -u root -p{} shop && sshpass -p '{}' ssh deploy@web1 && curl -u bob:{} https://api.example.com",
            s.pg, s.mysql, s.sshpass, s.curl
        )),
        last_error: Some(format!(
            "connection to postgres://app:{}@db.internal:5432/app failed\n< Set-Cookie: session={}; HttpOnly\nerror: OPENAI_API_KEY={} rejected",
            s.url_pw, s.cookie, s.openai
        )),
        snippets: vec![Snippet {
            package_name: None,
            catalog_id: None,
            id: ObjectId::new(),
            name: "gh login".into(),
            description: String::new(),
            snippet_type: SnippetType::Bash,
            shell: None,
            template: format!("export GITHUB_TOKEN={} && gh auth status", s.gh),
            variables: vec![],
            tags: vec![],
            risk_level: RiskLevel::ReadOnly,
            source: SnippetSource::User,
            created_by: None,
            created_at: now,
            updated_at: now,
            last_used_at: None,
            usage_count: 0,
        }],
        kb: vec![KbHit {
            id: ObjectId::new(),
            kind: DocKind::Note,
            title: "grafana api".into(),
            body: format!("grafana api_key: {} (rotate monthly)", s.kb_key),
            tags: vec![],
            score: 1.0,
        }],
    }
}

async fn run_every_feature(a: &AiAssistant, hid: ObjectId, s: &Secrets) {
    let mut conv = a.new_conversation();
    let ask = AskOptions {
        host_id: Some(hid),
        include_terminal: true,
    };
    let _ = a
        .ask(
            &mut conv,
            &format!("why does login fail? my password: {}", s.desc_pw),
            &ask,
        )
        .await;
    if let Ok(mut st) = a.ask_stream(&mut conv, "and the key above?", &ask).await {
        while st.next().await.is_some() {}
        st.finish(a, &mut conv);
    }
    let _ = a
        .generate_command(&GenerateRequest {
            description: format!(
                "connect with mysql -pwd {}; password={}",
                s.mysql, s.desc_pw
            ),
            target: None,
            host_id: Some(hid),
            include_terminal: true,
        })
        .await;
    let _ = a
        .explain_command(&ExplainRequest {
            command: format!(
                "sshpass -p '{}' ssh deploy@web1 && export GITHUB_TOKEN={}",
                s.sshpass, s.gh
            ),
            target: None,
            host_id: Some(hid),
        })
        .await;
    let _ = a.explain_command(&ExplainRequest::default()).await; // selection with the key
    let _ = a
        .fix_last_error(&FixRequest {
            target: None,
            host_id: Some(hid),
        })
        .await;
    let _ = a
        .create_snippet(&CreateSnippetRequest {
            description: format!("login to grafana, token: {}", s.kb_key),
            target: Some(CommandTarget::Curl),
            host_id: Some(hid),
        })
        .await;
    let _ = a
        .convert_to_snippet(&ConvertRequest {
            command: format!(
                "curl -H 'Authorization: Bearer {}' -u bob:{} https://api.example.com",
                s.jwt, s.curl
            ),
            target: None,
            use_llm: true,
        })
        .await;
    let _ = a.convert_to_snippet(&ConvertRequest::default()).await; // last command
    let _ = a
        .search(
            "grafana api",
            &SearchOptions {
                llm: LlmMode::Always,
                host_id: Some(hid),
                ..Default::default()
            },
        )
        .await;
}

fn assert_clean(what: &str, sent: &str, s: &Secrets) {
    assert!(
        !sent.is_empty(),
        "{what}: nothing was sent — test is broken"
    );
    for secret in &s.all {
        assert!(
            !sent.contains(secret.as_str()),
            "{what}: secret {secret:?} leaked"
        );
    }
    assert!(
        !sent.contains("BEGIN OPENSSH PRIVATE KEY"),
        "{what}: key header leaked"
    );
}

#[tokio::test]
async fn no_secret_leaks_through_any_feature_prompt() {
    let s = secrets();
    let hid = ObjectId::new();
    for (name, mock) in [
        (
            "strict/remote",
            MockProvider::remote(PrivacyProfile::Strict),
        ),
        (
            "standard/remote",
            MockProvider::remote(PrivacyProfile::Standard),
        ),
        (
            "local-clamped/remote",
            MockProvider::remote(PrivacyProfile::Local),
        ),
        ("local/local", MockProvider::local(PrivacyProfile::Local)),
    ] {
        let mock = Arc::new(mock);
        for _ in 0..20 {
            mock.push(r#"{"command": "echo ok", "explanation": "e", "summary": "s", "name": "n", "template": "echo {{x}}"}"#);
        }
        let a = AiAssistant::new(mock.clone(), Arc::new(poisoned(hid, &s)));
        run_every_feature(&a, hid, &s).await;
        let chats = mock.chats.lock().unwrap().len();
        assert!(chats >= 9, "{name}: only {chats} requests were made");
        assert_clean(name, &mock.all_sent(), &s);
    }
}

#[derive(Clone, Default)]
struct Wire(Arc<Mutex<Vec<u8>>>);

#[derive(Clone, Default)]
struct LogBuf(Arc<Mutex<Vec<u8>>>);
impl std::io::Write for LogBuf {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

async fn wire_handler(
    State(w): State<Wire>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    {
        let mut g = w.0.lock().unwrap();
        for (k, v) in &headers {
            g.extend_from_slice(k.as_str().as_bytes());
            g.extend_from_slice(b": ");
            g.extend_from_slice(v.as_bytes());
            g.push(b'\n');
        }
        g.extend_from_slice(&body);
        g.push(b'\n');
    }
    let stream = serde_json::from_slice::<serde_json::Value>(&body)
        .map(|v| v["stream"] == serde_json::json!(true))
        .unwrap_or(false);
    let content = r#"{\"command\": \"echo ok\", \"explanation\": \"e\"}"#;
    if stream {
        axum::response::Response::builder()
            .header("content-type", "text/event-stream")
            .body(axum::body::Body::from(format!(
                "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{content}\"}}}}]}}\n\ndata: [DONE]\n\n"
            )))
            .unwrap()
    } else {
        axum::response::Response::builder()
            .header("content-type", "application/json")
            .body(axum::body::Body::from(format!(
                "{{\"choices\":[{{\"message\":{{\"content\":\"{content}\"}},\"finish_reason\":\"stop\"}}]}}"
            )))
            .unwrap()
    }
}

#[tokio::test]
async fn no_secret_on_the_wire_or_in_logs() {
    let logs = LogBuf::default();
    let lw = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || lw.clone())
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let wire = Wire::default();
    let app = Router::new()
        .route("/v1/chat/completions", post(wire_handler))
        .with_state(wire.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let s = secrets();
    let hid = ObjectId::new();
    let api_key = format!("{}{}", "wire-", "api-key-XYZ987");
    let cfg = common::config(
        AiProviderKind::LmStudio,
        &format!("http://{addr}/v1"),
        PrivacyProfile::Local,
    );
    let provider: Arc<dyn LlmProvider> =
        build_provider(&cfg, Some(SecretString::from(api_key.clone()))).unwrap();
    tracing::debug!(provider = ?provider, "constructed provider");
    let a = AiAssistant::new(provider, Arc::new(poisoned(hid, &s)));
    run_every_feature(&a, hid, &s).await;

    let sent = String::from_utf8_lossy(&wire.0.lock().unwrap()).into_owned();
    assert_clean("wire", &sent, &s);
    assert!(
        sent.contains(&format!("authorization: Bearer {api_key}")),
        "key is only in the header"
    );
    assert_eq!(
        sent.matches(api_key.as_str()).count(),
        sent.matches("authorization: Bearer").count()
    );

    let log = String::from_utf8_lossy(&logs.0.lock().unwrap()).into_owned();
    for secret in s.all.iter().chain(std::iter::once(&api_key)) {
        assert!(!log.contains(secret.as_str()), "secret in logs: {secret}");
    }
    assert!(log.contains("constructed provider"), "log capture works");
}
