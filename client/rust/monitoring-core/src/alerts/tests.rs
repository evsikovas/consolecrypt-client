use super::*;
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpListener,
};
use zeroize::Zeroizing;

#[test]
fn alert_message_counts_lengths_and_total_bytes_fail_closed() {
    assert_eq!(messages(None).unwrap(), Vec::<String>::new());
    for value in [
        json!(vec![""; MAX_ALERT_MESSAGES + 1]),
        json!(["x".repeat(MAX_TEXT_BYTES + 1)]),
        json!(vec!["x".repeat(MAX_TEXT_BYTES); 100]),
    ] {
        assert_eq!(
            messages(Some(&value)).unwrap_err().code,
            ErrorCode::BudgetExceeded
        );
    }
}

#[tokio::test]
async fn alerts_and_rules_enforce_smaller_http_byte_ceiling_before_parsing_body() {
    for rules in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = MonitoringClient::new(
            source(format!("http://{}", listener.local_addr().unwrap())),
            Budgets::default(),
        )
        .unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            exchange(
                stream,
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    MAX_ALERT_RESPONSE_BYTES + 1
                ),
            )
            .await
        });
        let error = if rules {
            client.alert_rules().await.unwrap_err()
        } else {
            client.alerts().await.unwrap_err()
        };
        assert_eq!(error.code, ErrorCode::ResponseTooLarge);
        server.await.unwrap();
    }
}

fn alert(state: &str) -> Value {
    json!({"labels":{"alertname":"Own fixture"},"annotations":{"summary":"Opaque http://do-not-fetch.invalid/path"},"state":state,"activeAt":"2026-11-11T12:34:56.123456789Z","value":"1e+00"})
}
fn rule() -> Value {
    json!({"name":"Own rule","type":"alerting","query":"http://do-not-execute.invalid/admin","state":"inactive","health":"ok","duration":300,"keepFiringFor":0,"alerts":[],"labels":{},"annotations":{},"lastEvaluation":"2026-11-11T12:34:56Z","evaluationTime":0.001})
}
fn rules() -> Value {
    json!({"status":"success","data":{"groups":[{"name":"Own group","file":"C:/never-open-this/rules.yml","interval":15,"limit":100,"rules":[rule()]}]}})
}
fn source(endpoint: String) -> SourceConfig {
    SourceConfig {
        id: "alerts-fixture".into(),
        name: "Own alerts fixture".into(),
        endpoint,
        route: Route::Direct,
        tls: TlsConfig::default(),
        auth: AuthRef::None,
    }
}
fn response(status: u16, body: Value, extra: &str) -> String {
    let body = body.to_string();
    format!("HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n{extra}\r\n{body}",body.len())
}
async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(mut stream: S, response: String) -> String {
    let mut bytes = vec![];
    loop {
        let mut part = [0; 1024];
        let count = stream.read(&mut part).await.unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&part[..count]);
        assert!(bytes.len() < 32 * 1024);
        if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    stream.write_all(response.as_bytes()).await.unwrap();
    String::from_utf8(bytes).unwrap()
}
async fn fixture(status: u16, body: Value) -> (MonitoringClient, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = MonitoringClient::new(
        source(format!("http://{}/prefix", listener.local_addr().unwrap())),
        Budgets::default(),
    )
    .unwrap();
    let response = response(status, body, "");
    let server = tokio::spawn(async move {
        let (stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
            .await
            .unwrap()
            .unwrap();
        exchange(stream, response).await
    });
    (client, server)
}
async fn tls_fixture(
    status: u16,
    body: Value,
    extra: &str,
) -> (SourceConfig, tokio::task::JoinHandle<Option<String>>) {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let pem = cert.cert.pem();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![cert.cert.der().clone()],
        rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()).into(),
    )
    .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut config = source(format!(
        "https://localhost:{}/prefix",
        listener.local_addr().unwrap().port()
    ));
    config.tls.custom_ca_pem = Some(pem);
    let response = response(status, body, extra);
    let server = tokio::spawn(async move {
        let (stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
            .await
            .unwrap()
            .unwrap();
        match acceptor.accept(stream).await {
            Ok(stream) => Some(exchange(stream, response).await),
            Err(_) => None,
        }
    });
    (config, server)
}

#[tokio::test]
async fn alerts_fixed_get_retains_states_special_values_and_opaque_labels() {
    let mut unknown = alert("future-state");
    unknown["value"] = json!("NaN");
    unknown["generatorURL"] = json!("http://do-not-fetch.invalid");
    let mut firing = alert("firing");
    firing["value"] = json!("+Inf");
    let (client,server)=fixture(200,json!({"status":"success","data":{"alerts":[alert("pending"),firing,unknown]},"warnings":["incomplete store"],"infos":["annotation"]})).await;
    let result = client.alerts().await.unwrap();
    assert!(result.partial);
    assert_eq!(result.warnings, ["incomplete store"]);
    let alerts = result.data.unwrap().alerts;
    assert_eq!(alerts[0].state, AlertState::Pending);
    assert_eq!(alerts[1].state, AlertState::Firing);
    assert_eq!(alerts[1].value, SampleValue::PositiveInfinity);
    assert_eq!(
        alerts[2].state,
        AlertState::Unknown {
            value: Some("future-state".into())
        }
    );
    assert_eq!(alerts[2].value, SampleValue::NaN);
    assert_eq!(alerts[2].unsupported_fields, ["generatorURL"]);
    assert!(alerts[0].annotations["summary"].contains("http://"));
    assert!(server
        .await
        .unwrap()
        .starts_with("GET /prefix/api/v1/alerts HTTP/1.1"));
}

#[tokio::test]
async fn rules_fixed_filter_preserves_inactive_unknown_health_and_pagination() {
    let mut body = rules();
    body["data"]["groupNextToken"] = json!("http://never-follow-token.invalid");
    body["data"]["groups"][0]["rules"][0]["health"] = json!("err");
    body["data"]["groups"][0]["rules"][0]["lastError"] = json!("evaluation failed on backend");
    let (client, server) = fixture(200, body).await;
    let result = client.alert_rules().await.unwrap();
    assert!(result.partial);
    let data = result.data.unwrap();
    let rule = &data.groups[0].rules[0];
    assert_eq!(rule.state, AlertState::Inactive);
    assert_eq!(rule.health, RuleHealth::Error);
    assert!(rule.query.starts_with("http://"));
    assert_eq!(rule.duration_seconds, Some(300.0));
    assert_eq!(
        data.group_next_token,
        Some("http://never-follow-token.invalid".into())
    );
    assert!(server
        .await
        .unwrap()
        .starts_with("GET /prefix/api/v1/rules?type=alert&group_limit=101 HTTP/1.1"));
    let mut body = rules();
    body["data"]["groups"][0]["rules"][0]
        .as_object_mut()
        .unwrap()
        .remove("state");
    let groups = parse_groups(&body["data"]).unwrap();
    assert_eq!(
        groups.groups[0].rules[0].state,
        AlertState::Unknown { value: None }
    );
}

#[tokio::test]
async fn api_error_and_partial_alert_data_are_retained() {
    let (client,server)=fixture(503,json!({"status":"error","errorType":"timeout","error":"backend timeout","warnings":["one shard missing"],"data":{"alerts":[alert("pending")]}})).await;
    let result = client.alerts().await.unwrap();
    assert_eq!(result.http_status, 503);
    assert!(result.partial);
    assert_eq!(result.error.unwrap().r#type, "timeout");
    assert_eq!(result.data.unwrap().alerts.len(), 1);
    server.await.unwrap();
    let (client, server) = fixture(
        403,
        json!({"status":"error","errorType":"forbidden","error":"No tenant scope"}),
    )
    .await;
    let result = client.alert_rules().await.unwrap();
    assert_eq!(result.http_status, 403);
    assert!(result.data.is_none());
    assert_eq!(result.error.unwrap().message, "No tenant scope");
    server.await.unwrap();
}

#[test]
fn group_rule_alert_and_label_budgets_are_enforced_without_truncation() {
    let mut body = rules();
    let group = body["data"]["groups"][0].clone();
    body["data"]["groups"] = Value::Array(vec![group.clone(); MAX_RULE_GROUPS + 1]);
    assert_eq!(
        parse_groups(&body["data"]).unwrap_err().code,
        ErrorCode::BudgetExceeded
    );
    body["data"]["groups"] = json!([group.clone(), group]);
    body["data"]["groups"][0]["rules"] = Value::Array(vec![rule(); MAX_ALERT_RULES]);
    assert_eq!(
        parse_groups(&body["data"]).unwrap_err().code,
        ErrorCode::BudgetExceeded
    );
    assert_eq!(
        parse_alerts(
            &Value::Array(vec![alert("firing"); MAX_ACTIVE_ALERTS + 1]),
            &mut ParseBudget::default()
        )
        .unwrap_err()
        .code,
        ErrorCode::BudgetExceeded
    );
    let mut value = alert("firing");
    value["labels"] = json!((0..65)
        .map(|i| (format!("key{i}"), "value"))
        .collect::<BTreeMap<_, _>>());
    assert_eq!(
        parse_alerts(&json!([value]), &mut ParseBudget::default())
            .unwrap_err()
            .code,
        ErrorCode::BudgetExceeded
    );
    let mut budget = ParseBudget {
        label_bytes: MAX_ALERT_LABEL_BYTES,
        ..ParseBudget::default()
    };
    assert_eq!(
        labels(Some(&json!({"key":"value"})), &mut budget)
            .unwrap_err()
            .code,
        ErrorCode::BudgetExceeded
    );
}

#[test]
fn supported_optional_fields_are_strict_and_unknowns_are_explicit() {
    for (key, value) in [
        ("duration", json!("5m")),
        ("keepFiringFor", json!(-1)),
        ("evaluationTime", json!(null)),
        ("lastEvaluation", json!("yesterday")),
        ("health", json!(true)),
        ("annotations", json!([])),
    ] {
        let mut value_rule = rule();
        value_rule[key] = value;
        assert_eq!(
            parse_rule(&value_rule, &mut ParseBudget::default())
                .unwrap_err()
                .code,
            ErrorCode::InvalidResponse
        );
    }
    let mut value = rule();
    value["type"] = json!("recording");
    assert_eq!(
        parse_rule(&value, &mut ParseBudget::default())
            .unwrap_err()
            .code,
        ErrorCode::UnsupportedResult
    );
    let mut value = alert("new-state");
    value["value"] = json!("1e999");
    assert_eq!(
        parse_alerts(&json!([value]), &mut ParseBudget::default())
            .unwrap_err()
            .code,
        ErrorCode::InvalidResponse
    );
}

#[tokio::test]
async fn alerts_reuse_verified_tls_scoped_auth_and_secret_redaction() {
    let token = "synthetic-alerts-token-741";
    let mut body = alert("firing");
    body["labels"]["echo"] = json!(token);
    let (mut config, server) = tls_fixture(
        200,
        json!({"status":"success","data":{"alerts":[body]},"warnings":[format!("echo {token}")]}),
        "",
    )
    .await;
    config.auth = AuthRef::BearerRef {
        credential_id: "credential-ref".into(),
    };
    let grant = SourceAuthorization::bearer(
        &config,
        Zeroizing::new(token.into()),
        Duration::from_secs(5),
    )
    .unwrap();
    let client = MonitoringClient::with_authorization(config, Budgets::default(), grant).unwrap();
    let result = client.alerts().await.unwrap();
    let serialized = serde_json::to_string(&result).unwrap();
    assert!(!serialized.contains(token));
    assert!(serialized.contains("[REDACTED]"));
    let request = server.await.unwrap().unwrap();
    assert!(request.contains(&format!("authorization: Bearer {token}")));
    assert!(request.starts_with("GET /prefix/api/v1/alerts HTTP/1.1"));
    let (mut config, server) = tls_fixture(200, rules(), "").await;
    config.tls.custom_ca_pem = None;
    let client = MonitoringClient::new(config, Budgets::default()).unwrap();
    assert_eq!(
        client.alert_rules().await.unwrap_err().code,
        ErrorCode::Transport
    );
    assert!(server.await.unwrap().is_none());
}

#[tokio::test]
async fn rule_redirect_cannot_forward_auth_or_follow_response_urls() {
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (mut config, server) = tls_fixture(
        302,
        json!({}),
        &format!(
            "Location: http://{}/steal\r\n",
            target.local_addr().unwrap()
        ),
    )
    .await;
    config.auth = AuthRef::BearerRef {
        credential_id: "credential-ref".into(),
    };
    let grant = SourceAuthorization::bearer(
        &config,
        Zeroizing::new("fixture-rule-token".into()),
        Duration::from_secs(5),
    )
    .unwrap();
    let client = MonitoringClient::with_authorization(config, Budgets::default(), grant).unwrap();
    assert_eq!(
        client.alert_rules().await.unwrap_err().code,
        ErrorCode::Transport
    );
    assert!(server
        .await
        .unwrap()
        .unwrap()
        .starts_with("GET /prefix/api/v1/rules?type=alert&group_limit=101 HTTP/1.1"));
    assert!(
        tokio::time::timeout(Duration::from_millis(75), target.accept())
            .await
            .is_err()
    );
}
