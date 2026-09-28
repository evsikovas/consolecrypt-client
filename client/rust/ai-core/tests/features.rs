//! Feature flows through `AiAssistant` with a scripted provider.

mod common;

use cc_ai_core::context::{HostContext, StaticContext};
use cc_ai_core::policy::ExecutionGate;
use cc_ai_core::sanitizer::PrivacyProfile;
use cc_ai_core::{
    AiAssistant, AskOptions, CommandTarget, ConvertRequest, CreateSnippetRequest, ExplainRequest,
    FixRequest, GenerateRequest, ParseQuality, RiskLevel,
};
use cc_models::snippet::SnippetSource;
use cc_models::ObjectId;
use common::MockProvider;
use futures::StreamExt;
use std::sync::Arc;

fn host(id: ObjectId) -> HostContext {
    HostContext {
        host_id: Some(id),
        name: "billing-db".into(),
        address: "10.20.30.40".into(),
        port: Some(22),
        username: Some("svc_billing".into()),
        tags: vec!["prod".into()],
        os: Some("Ubuntu 24.04".into()),
        shell: Some("bash".into()),
    }
}

fn setup(mock: MockProvider, ctx: StaticContext) -> (Arc<MockProvider>, AiAssistant) {
    let mock = Arc::new(mock);
    let a = AiAssistant::new(mock.clone(), Arc::new(ctx));
    (mock, a)
}

#[tokio::test]
async fn generate_command_rehydrates_locally_and_hides_host_under_strict() {
    let hid = ObjectId::new();
    let ctx = StaticContext {
        hosts: vec![host(hid)],
        ..Default::default()
    };
    let (mock, a) = setup(MockProvider::remote(PrivacyProfile::Strict), ctx);
    mock.push(r#"{"command": "ssh <USER_1>@<IP_1> 'df -h /var/lib/postgresql'", "explanation": "Checks disk usage on <HOST_1>", "risk_suggestion": "read_only", "variables": [], "alternatives": []}"#);
    let p = a
        .generate_command(&GenerateRequest {
            description: "how full is the postgres disk on billing-db?".into(),
            host_id: Some(hid),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(p.target, CommandTarget::Bash, "derived from the host shell");
    assert_eq!(
        p.suggestion.command,
        "ssh svc_billing@10.20.30.40 'df -h /var/lib/postgresql'"
    );
    assert_eq!(p.suggestion.explanation, "Checks disk usage on billing-db");
    assert_eq!(p.run.risk, RiskLevel::ReadOnly);
    assert!(!p.run.requires_confirmation);
    assert_eq!(
        p.run.host.as_ref().unwrap().display,
        "billing-db (10.20.30.40)"
    );
    assert!(p.redactions.total() >= 3);
    let sent = mock.all_sent();
    for leaked in ["billing-db", "10.20.30.40", "svc_billing"] {
        assert!(!sent.contains(leaked), "{leaked} leaked: {sent}");
    }
    assert!(sent.contains("<context>") && sent.contains("Target: a bash command line."));
    // Approval flow: read-only needs no confirmation.
    assert!(ExecutionGate.approve(&p.run, false).is_ok());
}

#[tokio::test]
async fn ai_risk_can_only_raise_and_secrets_block() {
    let (mock, a) = setup(
        MockProvider::remote(PrivacyProfile::Standard),
        StaticContext::default(),
    );
    mock.push(
        r#"{"command": "rm -rf /tmp/cache", "explanation": "x", "risk_suggestion": "read_only"}"#,
    );
    mock.push(r#"{"command": "ls -la", "explanation": "x", "risk_suggestion": "destructive"}"#);
    mock.push(r#"{"command": "mysql -u app -p<PASSWORD_1> -e 'select 1'", "explanation": "x", "risk_suggestion": "read_only"}"#);
    mock.push(r#"{"command": "kubectl logs -n {{namespace}} {{pod}}", "explanation": "x", "variables": [{"name":"namespace","default":"prod"},{"name":"pod"},{"name":"db_password","default":"hunter2hunter2"}]}"#);
    let gen = |d: &str| GenerateRequest {
        description: d.into(),
        ..Default::default()
    };
    let p = a.generate_command(&gen("clear cache")).await.unwrap();
    assert_eq!(p.run.risk, RiskLevel::Destructive);
    assert!(p.run.requires_confirmation);
    assert!(ExecutionGate.approve(&p.run, false).is_err());
    assert!(ExecutionGate.approve(&p.run, true).is_ok());

    let p = a.generate_command(&gen("list")).await.unwrap();
    assert_eq!(p.run.local_risk, RiskLevel::ReadOnly);
    assert_eq!(p.run.risk, RiskLevel::Destructive);

    let p = a.generate_command(&gen("query")).await.unwrap();
    assert_eq!(p.unresolved_secrets, vec!["<PASSWORD_1>"]);
    assert!(p.run.is_blocked());

    let p = a.generate_command(&gen("logs")).await.unwrap();
    let form = p.form.unwrap();
    assert_eq!(form.fields.len(), 2);
    assert_eq!(form.fields[0].default.as_deref(), Some("prod"));
    let pw = p
        .suggestion
        .variables
        .iter()
        .find(|v| v.name == "db_password")
        .unwrap();
    assert!(
        pw.default.is_none(),
        "secret-named variables never keep a default"
    );
}

#[tokio::test]
async fn structured_format_rejection_falls_back_to_text() {
    let mut m = MockProvider::remote(PrivacyProfile::Standard);
    m.fail_format = true;
    let (mock, a) = setup(m, StaticContext::default());
    mock.push("Here:\n```bash\nuptime\n```");
    let p = a
        .generate_command(&GenerateRequest {
            description: "uptime".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(p.suggestion.command, "uptime");
    assert_eq!(p.suggestion.parse_quality, ParseQuality::Fallback);
}

#[tokio::test]
async fn explain_uses_local_rules_and_rehydrates() {
    let (mock, a) = setup(
        MockProvider::remote(PrivacyProfile::Strict),
        StaticContext::default(),
    );
    mock.push(r#"{"summary": "Deletes the data dir on <IP_1>", "parts": [{"text": "rm -rf", "meaning": "recursive delete"}, {"text": "<IP_1>", "meaning": "target"}], "risk_suggestion": "modifying", "warnings": ["irreversible"]}"#);
    let r = a
        .explain_command(&ExplainRequest {
            command: "ssh root@10.9.8.7 'rm -rf /var/lib/app'".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(r.local_risk.level, RiskLevel::Destructive);
    assert_eq!(
        r.effective_risk,
        RiskLevel::Destructive,
        "AI 'modifying' cannot lower it"
    );
    assert_eq!(r.explanation.summary, "Deletes the data dir on 10.9.8.7");
    assert_eq!(r.explanation.parts[1].text, "10.9.8.7");
    assert!(!mock.all_sent().contains("10.9.8.7"));

    // Empty command → selection / last command from the terminal.
    let ctx = StaticContext {
        last_command: Some("df -h".into()),
        ..Default::default()
    };
    let (mock, a) = setup(MockProvider::remote(PrivacyProfile::Standard), ctx);
    mock.push("It shows disk usage.");
    let r = a.explain_command(&ExplainRequest::default()).await.unwrap();
    assert_eq!(r.explanation.summary, "It shows disk usage.");
    assert_eq!(r.explanation.parse_quality, ParseQuality::Fallback);
    assert!(mock.all_sent().contains("df -h"));
    let (_, empty) = setup(
        MockProvider::remote(PrivacyProfile::Standard),
        StaticContext::default(),
    );
    assert!(empty
        .explain_command(&ExplainRequest::default())
        .await
        .is_err());
}

#[tokio::test]
async fn fix_last_error_sends_sanitized_terminal_context() {
    let ctx = StaticContext {
        last_command: Some("PGPASSWORD=Pr0dPass! psql -h db.internal -U app billing".into()),
        last_error: Some("psql: error: connection to server at \"db.internal\" (10.0.0.12), port 5432 failed: FATAL: password authentication failed for user \"app\"".into()),
        ..Default::default()
    };
    let (mock, a) = setup(MockProvider::remote(PrivacyProfile::Standard), ctx);
    mock.push(r#"{"diagnosis": "wrong password", "command": "psql -h db.internal -U app -W billing", "explanation": "prompt for the password", "risk_suggestion": "read_only"}"#);
    let p = a.fix_last_error(&FixRequest::default()).await.unwrap();
    assert_eq!(p.suggestion.diagnosis.as_deref(), Some("wrong password"));
    assert_eq!(
        p.suggestion.command,
        "psql -h db.internal -U app -W billing"
    );
    let sent = mock.all_sent();
    assert!(!sent.contains("Pr0dPass!"));
    assert!(sent.contains("db.internal"), "Standard keeps host metadata");
    assert!(sent.contains("Last error output"));
    let (_, a) = setup(
        MockProvider::remote(PrivacyProfile::Standard),
        StaticContext::default(),
    );
    assert!(a.fix_last_error(&FixRequest::default()).await.is_err());
}

#[tokio::test]
async fn create_snippet_scrubs_secrets() {
    let (mock, a) = setup(
        MockProvider::remote(PrivacyProfile::Standard),
        StaticContext::default(),
    );
    mock.push(r#"{"name": "Tail app logs", "description": "Follow logs", "template": "kubectl logs -n {{namespace}} -l app={{app}} --tail={{lines}} -f --token=<SECRET_1>", "variables": [{"name":"namespace","description":"ns","default":"prod"},{"name":"lines","default":"200"}], "tags": ["K8s", "logs"], "risk_suggestion": "read_only"}"#);
    let d = a
        .create_snippet(&CreateSnippetRequest {
            description: "follow logs of an app".into(),
            target: Some(CommandTarget::Kubectl),
            host_id: None,
        })
        .await
        .unwrap();
    assert_eq!(d.name, "Tail app logs");
    assert!(d.template.contains("--token={{token}}"), "{}", d.template);
    let names: Vec<&str> = d.variables.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, vec!["namespace", "app", "lines", "token"]);
    assert!(d.variables[3].default.is_none());
    assert_eq!(d.variables[0].default.as_deref(), Some("prod"));
    assert_eq!(d.tags, vec!["k8s", "logs", "kubectl"]);
    assert_eq!(d.risk, RiskLevel::ReadOnly);
    assert_eq!(d.source, SnippetSource::Ai);
    let s = d.into_snippet(None);
    assert_eq!(s.snippet_type, cc_models::snippet::SnippetType::Kubectl);
}

#[tokio::test]
async fn convert_to_snippet_local_and_llm() {
    let cmd = "PGPASSWORD=S3cr3tPass psql -h db.internal -U app -d billing -c 'select count(*) from invoices'";
    let (mock, a) = setup(
        MockProvider::remote(PrivacyProfile::Standard),
        StaticContext::default(),
    );
    let d = a
        .convert_to_snippet(&ConvertRequest {
            command: cmd.into(),
            target: Some(CommandTarget::PostgreSql),
            use_llm: false,
        })
        .await
        .unwrap();
    assert!(!d.template.contains("S3cr3tPass"));
    assert!(
        d.template
            .starts_with("PGPASSWORD={{password}} psql -h {{host}} -U {{user}} -d {{database}}"),
        "{}",
        d.template
    );
    assert_eq!(d.secrets_removed, 1);
    assert!(d
        .variables
        .iter()
        .find(|v| v.name == "password")
        .unwrap()
        .default
        .is_none());
    assert_eq!(
        d.variables
            .iter()
            .find(|v| v.name == "host")
            .unwrap()
            .default
            .as_deref(),
        Some("db.internal")
    );
    assert!(mock.all_sent().is_empty(), "local conversion sends nothing");

    mock.push(r#"{"name": "Count invoices", "description": "Counts invoices", "template": "PGPASSWORD={{password}} psql -h {{db_host}} -U {{user}} -d {{database}} -c 'select count(*) from invoices'", "variables": [{"name": "db_host", "description": "Database host", "default": "db.internal"}, {"name":"password","default":"S3cr3tPass"}], "tags": ["postgres"]}"#);
    let d = a
        .convert_to_snippet(&ConvertRequest {
            command: cmd.into(),
            target: Some(CommandTarget::PostgreSql),
            use_llm: true,
        })
        .await
        .unwrap();
    assert_eq!(d.name, "Count invoices");
    assert!(!mock.all_sent().contains("S3cr3tPass"));
    assert!(d
        .variables
        .iter()
        .find(|v| v.name == "password")
        .unwrap()
        .default
        .is_none());
    assert_eq!(
        d.variables
            .iter()
            .find(|v| v.name == "user")
            .unwrap()
            .default
            .as_deref(),
        Some("app"),
        "local default kept"
    );
    assert!(!serde_json::to_string(&d).unwrap().contains("S3cr3tPass"));
}

#[tokio::test]
async fn ask_ai_conversation_and_streaming() {
    let hid = ObjectId::new();
    let ctx = StaticContext {
        hosts: vec![host(hid)],
        selected_text: Some("ERROR: disk full on 10.20.30.40".into()),
        ..Default::default()
    };
    let (mock, a) = setup(MockProvider::remote(PrivacyProfile::Strict), ctx);
    let mut conv = a.new_conversation();
    mock.push("Free space on <IP_1> with `du -sh /var/*`.");
    let ans = a
        .ask(
            &mut conv,
            "why is my disk full?",
            &AskOptions {
                host_id: Some(hid),
                include_terminal: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(ans.text, "Free space on 10.20.30.40 with `du -sh /var/*`.");
    assert_eq!(conv.len(), 2);
    assert!(!mock.all_sent().contains("10.20.30.40"));

    mock.push("Then restart <HOST_1> if needed. <PASSWORD_1> stays.");
    let mut s = a
        .ask_stream(&mut conv, "and then?", &AskOptions::default())
        .await
        .unwrap();
    let mut out = String::new();
    while let Some(chunk) = s.next().await {
        out.push_str(&chunk.unwrap());
    }
    s.finish(&a, &mut conv);
    assert_eq!(
        out,
        "Then restart billing-db if needed. <PASSWORD_1> stays."
    );
    assert_eq!(conv.len(), 4);
    // The second request carried the history (placeholders only).
    let chats = mock.chats.lock().unwrap().clone();
    assert!(chats[1].contains("Free space on <IP_1>"));
    assert!(!chats[1].contains("10.20.30.40"));
}

#[tokio::test]
async fn local_profile_for_local_provider_keeps_hosts_but_not_passwords() {
    let hid = ObjectId::new();
    let ctx = StaticContext {
        hosts: vec![host(hid)],
        last_command: Some("mysql -h 10.20.30.40 -u svc_billing -pTopSecret1 billing".into()),
        ..Default::default()
    };
    let (mock, a) = setup(MockProvider::local(PrivacyProfile::Local), ctx);
    assert_eq!(a.privacy_profile(), PrivacyProfile::Local);
    mock.push(r#"{"command":"uptime","explanation":"x"}"#);
    a.generate_command(&GenerateRequest {
        description: "check load".into(),
        host_id: Some(hid),
        include_terminal: true,
        ..Default::default()
    })
    .await
    .unwrap();
    let sent = mock.all_sent();
    assert!(sent.contains("10.20.30.40") && sent.contains("billing-db"));
    assert!(!sent.contains("TopSecret1"));

    // The same config on a public endpoint is clamped to Standard.
    let (mock, a) = setup(
        MockProvider::remote(PrivacyProfile::Local),
        StaticContext::default(),
    );
    assert_eq!(a.privacy_profile(), PrivacyProfile::Standard);
    drop(mock);
}
