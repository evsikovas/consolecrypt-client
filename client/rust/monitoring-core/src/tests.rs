use super::*;
use serde_json::json;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpListener,
};

fn source(endpoint: String) -> SourceConfig {
    SourceConfig {
        id: "fixture".into(),
        name: "Fixture".into(),
        endpoint,
        route: Route::Direct,
        tls: TlsConfig::default(),
        auth: AuthRef::None,
    }
}
fn vector() -> Value {
    json!({"status":"success","data":{"resultType":"vector","result":[{"metric":{"job":"node"},"value":[1,"1"]}]}})
}
async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(mut stream: S, response: String) -> String {
    let mut bytes = vec![];
    loop {
        let mut buf = [0; 4096];
        let n = stream.read(&mut buf).await.unwrap();
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&buf[..n]);
        if let Some(pos) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..pos]);
            let length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .map(str::to_owned)
                })
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0);
            if bytes.len() >= pos + 4 + length {
                break;
            }
        }
    }
    stream.write_all(response.as_bytes()).await.unwrap();
    String::from_utf8(bytes).unwrap()
}
fn http_response(status: u16, body: &str, extra: &str) -> String {
    format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}", body.len())
}
async fn fixture(status: u16, body: Value) -> (MonitoringClient, tokio::task::JoinHandle<String>) {
    raw_fixture(
        http_response(status, &body.to_string(), ""),
        Budgets::default(),
    )
    .await
}
async fn raw_fixture(
    response: String,
    budgets: Budgets,
) -> (MonitoringClient, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = MonitoringClient::new(
        source(format!("http://{}/prefix", listener.local_addr().unwrap())),
        budgets,
    )
    .unwrap();
    let server =
        tokio::spawn(async move { exchange(listener.accept().await.unwrap().0, response).await });
    (client, server)
}

#[test]
fn config_rejects_unsafe_endpoints_and_oversized_budgets() {
    for endpoint in [
        "file:///etc/passwd",
        "http://u:password@example.test",
        "http://example.test?token=secret",
        "http://example.test/#secret",
        "ftp://example.test",
    ] {
        let config = source(endpoint.into());
        assert!(config.validate().is_err());
        assert!(!format!("{config:?}").contains("password"));
    }
    assert!(serde_json::to_string(&source("https://user:password@example.test".into())).is_err());
    let mut config = source("https://example.test".into());
    config.tls.custom_ca_pem =
        Some("-----BEGIN PRIVATE KEY-----\nsecret\n-----END PRIVATE KEY-----".into());
    assert!(config.validate().is_err());
    assert!(serde_json::to_string(&config).is_err());
    assert!(Budgets {
        max_series: 101,
        ..Budgets::default()
    }
    .validate()
    .is_err());
    assert!(Budgets {
        max_points_per_series: 1,
        ..Budgets::default()
    }
    .validate()
    .is_err());
    let mut config = source("https://example.test".into());
    config.auth = AuthRef::BearerRef {
        credential_id: "vault-entry".into(),
    };
    assert_eq!(
        MonitoringClient::new(config.clone(), Budgets::default())
            .unwrap_err()
            .code,
        ErrorCode::AuthUnavailable
    );
    config.route = Route::SshTunnel {
        route_id: "route".into(),
    };
    assert_eq!(
        MonitoringClient::new(config, Budgets::default())
            .unwrap_err()
            .code,
        ErrorCode::UnsupportedRoute
    );
}

#[test]
fn explicit_http_basic_and_anonymous_are_supported_but_http_bearer_is_rejected() {
    for endpoint in [
        "http://10.10.10.11:3000/api/datasources/proxy/uid/fixture",
        "http://127.0.0.1:1234/prefix",
    ] {
        let mut config = source(endpoint.into());
        assert_eq!(config.validate().unwrap().scheme(), "http");
        config.auth = AuthRef::BasicRef {
            credential_id: "native-credential-reference".into(),
        };
        let base = config.validate().unwrap();
        assert_eq!(base.scheme(), "http");
        assert!(base.as_str().starts_with(endpoint));
        assert!(
            MonitoringClient::new(config.clone(), Budgets::default()).is_err(),
            "reference alone is not authorization"
        );
        assert!(SourceAuthorization::basic(
            &config,
            Zeroizing::new("user".into()),
            Zeroizing::new("password".into()),
            Duration::from_secs(5)
        )
        .is_ok());
        config.auth = AuthRef::BearerRef {
            credential_id: "native-credential-reference".into(),
        };
        assert!(config.validate().is_err());
        assert!(SourceAuthorization::bearer(
            &config,
            Zeroizing::new("token".into()),
            Duration::from_secs(5)
        )
        .is_err());
    }
    for endpoint in [
        "http://user:password@example.test/prefix",
        "http://example.test/prefix?token=secret",
        "http://example.test/prefix#secret",
    ] {
        let mut config = source(endpoint.into());
        config.auth = AuthRef::BasicRef {
            credential_id: "ref".into(),
        };
        assert!(config.validate().is_err());
        assert!(serde_json::to_string(&config).is_err());
    }
}

#[tokio::test]
async fn http_basic_sends_only_fixed_prefixed_read_path_and_redacts_echo_without_redirect_forwarding(
) {
    let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
    for redirect in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut config = source(format!(
            "http://{}/api/datasources/proxy/uid/owned-fixture",
            listener.local_addr().unwrap()
        ));
        config.auth = AuthRef::BasicRef {
            credential_id: "native-ref".into(),
        };
        let grant = SourceAuthorization::basic(
            &config,
            Zeroizing::new("user".into()),
            Zeroizing::new("pass".into()),
            Duration::from_secs(5),
        )
        .unwrap();
        let mut body = vector();
        body["warnings"] = json!(["echo Basic dXNlcjpwYXNz user:pass pass"]);
        let wire = if redirect {
            http_response(
                302,
                "",
                &format!(
                    "Location: http://{}/steal\r\n",
                    destination.local_addr().unwrap()
                ),
            )
        } else {
            http_response(200, &body.to_string(), "")
        };
        let server =
            tokio::spawn(async move { exchange(listener.accept().await.unwrap().0, wire).await });
        let client =
            MonitoringClient::with_authorization(config, Budgets::default(), grant).unwrap();
        let result = client.query("up", None).await;
        if redirect {
            assert_eq!(result.unwrap_err().code, ErrorCode::Transport)
        } else {
            let result = serde_json::to_string(&result.unwrap()).unwrap();
            for secret in ["dXNlcjpwYXNz", "user:pass", "pass"] {
                assert!(!result.contains(secret));
            }
            assert!(result.contains("[REDACTED]"));
        }
        let request = server.await.unwrap();
        assert!(request.starts_with(
            "POST /api/datasources/proxy/uid/owned-fixture/api/v1/query HTTP/1.1\r\n"
        ));
        assert!(request.contains("authorization: Basic dXNlcjpwYXNz\r\n"));
        assert!(!request
            .to_ascii_lowercase()
            .contains("proxy-authorization:"));
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(75), destination.accept())
            .await
            .is_err()
    );
}

#[test]
fn authorization_binds_complete_config_kind_tls_and_lifetime() {
    let mut config = source("https://example.test".into());
    config.auth = AuthRef::BearerRef {
        credential_id: "secret-ref".into(),
    };
    for ttl in [Duration::ZERO, Duration::from_secs(31)] {
        assert!(
            SourceAuthorization::bearer(&config, Zeroizing::new("secret".into()), ttl).is_err()
        );
    }
    for token in ["", "token\r\nInjected:yes", "token space", "token\t"] {
        assert!(SourceAuthorization::bearer(
            &config,
            Zeroizing::new(token.into()),
            Duration::from_secs(30)
        )
        .is_err());
    }
    let grant = || {
        SourceAuthorization::bearer(
            &config,
            Zeroizing::new("fixture-secret".into()),
            Duration::from_secs(30),
        )
        .unwrap()
    };
    assert!(!format!("{:?}", grant()).contains("fixture-secret"));
    for changed in 0..6 {
        let mut different = config.clone();
        match changed {
            0 => different.id = "other".into(),
            1 => different.endpoint = "https://other.test".into(),
            2 => {
                different.auth = AuthRef::BearerRef {
                    credential_id: "other-ref".into(),
                }
            }
            3 => different.name = "Other revision".into(),
            4 => different.auth = AuthRef::None,
            _ => {
                different.auth = AuthRef::BasicRef {
                    credential_id: "secret-ref".into(),
                }
            }
        }
        assert_eq!(
            MonitoringClient::with_authorization(different, Budgets::default(), grant())
                .unwrap_err()
                .code,
            ErrorCode::AuthUnavailable
        );
    }
    let mut plain = config.clone();
    plain.endpoint = "http://example.test".into();
    assert!(SourceAuthorization::bearer(
        &plain,
        Zeroizing::new("secret".into()),
        Duration::from_secs(30)
    )
    .is_err());
    assert!(SourceAuthorization::basic(
        &config,
        Zeroizing::new("user".into()),
        Zeroizing::new("password".into()),
        Duration::from_secs(30)
    )
    .is_err());
    config.auth = AuthRef::BasicRef {
        credential_id: "secret-ref".into(),
    };
    for (user, password) in [
        ("u:ser", "password"),
        ("user\r\n", "password"),
        ("user", "password\r\n"),
    ] {
        assert!(SourceAuthorization::basic(
            &config,
            Zeroizing::new(user.into()),
            Zeroizing::new(password.into()),
            Duration::from_secs(30)
        )
        .is_err());
    }
    assert!(SourceAuthorization::bearer(
        &config,
        Zeroizing::new("secret".into()),
        Duration::from_secs(30)
    )
    .is_err());
}

#[tokio::test]
async fn authorization_deadline_cancels_inflight_and_blocks_expired_cache() {
    let (mut config, server) =
        tls_fixture_delayed(vector().to_string(), "", Duration::from_secs(2)).await;
    config.auth = AuthRef::BearerRef {
        credential_id: "ref".into(),
    };
    let credential = SourceAuthorization::bearer(
        &config,
        Zeroizing::new("fixture-token".into()),
        Duration::from_secs(30),
    )
    .unwrap();
    let mut client =
        MonitoringClient::with_authorization(config, Budgets::default(), credential).unwrap();
    client.credential.as_mut().unwrap().expires = Instant::now();
    assert_eq!(
        client.label_values("__name__").await.unwrap_err().code,
        ErrorCode::AuthUnavailable
    );
    server.abort();
    let (mut config, server) =
        tls_fixture_delayed(vector().to_string(), "", Duration::from_secs(2)).await;
    config.auth = AuthRef::BearerRef {
        credential_id: "ref".into(),
    };
    let credential = SourceAuthorization::bearer(
        &config,
        Zeroizing::new("fixture-token".into()),
        Duration::from_secs(30),
    )
    .unwrap();
    let mut client =
        MonitoringClient::with_authorization(config, Budgets::default(), credential).unwrap();
    // The trusted TLS server intentionally withholds its response beyond the
    // short grant deadline, rather than waiting for the normal 15s budget.
    client.credential.as_mut().unwrap().expires = Instant::now() + Duration::from_millis(50);
    let result = tokio::time::timeout(Duration::from_secs(1), client.query("up", None))
        .await
        .unwrap();
    assert_eq!(result.unwrap_err().code, ErrorCode::AuthUnavailable);
    assert_eq!(client.permits.available_permits(), 4);
    server.abort();
}

#[tokio::test]
async fn query_uses_post_form_explicit_endpoint_and_preserves_warnings() {
    let mut body = vector();
    body["warnings"] = json!(["partial store response"]);
    body["infos"] = json!(["annotation"]);
    let (client, server) = fixture(200, body).await;
    let result = client
        .query("up{url=\"http://evil.invalid/admin\"}", Some(1.5))
        .await
        .unwrap();
    assert!(result.partial);
    assert_eq!(result.warnings, ["partial store response"]);
    assert_eq!(result.infos, ["annotation"]);
    let request = server.await.unwrap();
    assert!(request.starts_with("POST /prefix/api/v1/query HTTP/1.1"));
    assert!(request.contains("application/x-www-form-urlencoded"));
    assert!(
        request.contains("time=1.5")
            && request.contains("limit=101")
            && request.contains("timeout=15s")
    );
    assert!(!request.lines().next().unwrap().contains("evil"));
}

#[tokio::test]
async fn api_error_statuses_and_partial_data_remain_visible() {
    for status in [400, 401, 403, 422, 429, 503] {
        let mut body = vector();
        body["status"] = json!("error");
        body["errorType"] = json!("execution");
        body["error"] = json!("budget exceeded");
        let (client, server) = fixture(status, body).await;
        let result = client.query("up", None).await.unwrap();
        assert_eq!(result.http_status, status);
        assert_eq!(result.error.unwrap().message, "budget exceeded");
        assert!(result.partial && result.data.is_some());
        server.await.unwrap();
    }
}

#[tokio::test]
async fn non_json_http_status_and_metadata_api_errors_are_preserved() {
    let (client, server) =
        raw_fixture(http_response(401, "Unauthorized", ""), Budgets::default()).await;
    let error = client.query("up", None).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidResponse);
    assert_eq!(error.http_status, Some(401));
    server.await.unwrap();
    let (client,server) = fixture(403,json!({"status":"error","errorType":"forbidden","error":"No tenant scope","warnings":["scope denied"]})).await;
    let result = client.label_values("__name__").await.unwrap();
    assert_eq!(result.http_status, 403);
    assert_eq!(result.error.unwrap().message, "No tenant scope");
    assert_eq!(result.warnings, ["scope denied"]);
    assert!(client.cache.lock().unwrap().is_empty());
    server.await.unwrap();
}

#[test]
fn nan_infinities_gaps_histograms_and_malformed_values() {
    let body = json!({"status":"success","data":{"resultType":"matrix","result":[{"metric":{},"values":[[0,"0"],[15,"NaN"],[45,"+Inf"],[60,"-Inf"]]}]}});
    let parsed = parse::envelope(body, 200, &Budgets::default(), Some(15.0)).unwrap();
    let QueryData::Matrix { series } = parsed.data.unwrap() else {
        panic!()
    };
    assert_eq!(series[0].samples.len(), 4); // gap at 30 remains absent
    assert_eq!(series[0].samples[1].value, SampleValue::NaN);
    assert_eq!(series[0].samples[2].value, SampleValue::PositiveInfinity);
    assert_eq!(series[0].samples[3].value, SampleValue::NegativeInfinity);
    assert_eq!(
        serde_json::to_value(SampleValue::NaN).unwrap(),
        json!({"kind":"nan"})
    );
    for invalid in ["nan", "Infinity", "1e999", "bogus"] {
        let mut body = vector();
        body["data"]["result"][0]["value"][1] = json!(invalid);
        assert_eq!(
            parse::envelope(body, 200, &Budgets::default(), None)
                .unwrap_err()
                .code,
            ErrorCode::InvalidResponse
        );
    }
    let body = json!({"status":"success","data":{"resultType":"vector","result":[{"metric":{},"histogram":[1,{}]}]}});
    assert_eq!(
        parse::envelope(body, 200, &Budgets::default(), None)
            .unwrap_err()
            .code,
        ErrorCode::UnsupportedResult
    );
}

#[tokio::test]
async fn range_step_is_coarsened_and_returned() {
    let body = json!({"status":"success","data":{"resultType":"matrix","result":[]}});
    let (client, server) = fixture(200, body).await;
    let result = client.query_range("up", 0.0, 99900.0, 1.0).await.unwrap();
    assert_eq!(result.effective_step_seconds, Some(100.0));
    assert!(server.await.unwrap().contains("step=100"));
    for (start, end, step) in [
        (f64::NAN, 1., 1.),
        (0., f64::INFINITY, 1.),
        (1., 0., 1.),
        (0., 1., 0.),
        (0., 1., f64::NAN),
    ] {
        assert_eq!(
            client
                .query_range("up", start, end, step)
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidQuery
        );
    }
}

#[tokio::test]
async fn panel_presence_uses_selected_historical_end_and_rejects_invalid_time() {
    use crate::panels::{PanelKind, PanelPreparation};
    use cc_host_mapping::{HostBinding, Labels};
    let body = json!({"status":"success","data":{"resultType":"vector","result":[
        {"metric":{"__name__":"node_memory_MemTotal_bytes"},"value":[1234,"1"]},
        {"metric":{"__name__":"node_memory_MemAvailable_bytes"},"value":[1234,"1"]}
    ]}});
    let (client, server) = fixture(200, body).await;
    let binding = HostBinding::new(
        "5e7c5182-19df-48e1-99ed-87d28c9356e9",
        "fixture",
        1,
        Labels::from([("instance".into(), "historical-node".into())]),
    )
    .unwrap();
    assert!(matches!(
        client
            .prepare_panel(PanelKind::Memory, &binding, 1000.0, 1234.0, 15.0)
            .await
            .unwrap(),
        PanelPreparation::Ready { .. }
    ));
    let request = server.await.unwrap();
    assert!(request.contains("time=1234"));
    assert_eq!(
        client
            .prepare_panel(PanelKind::Memory, &binding, 1000.0, f64::NAN, 15.0)
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidQuery
    );
    assert_eq!(
        client
            .prepare_panel(PanelKind::Memory, &binding, 1000.0, f64::INFINITY, 15.0)
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidQuery
    );
}

#[test]
fn response_timestamp_bounds_are_validated() {
    let mut large_value = vector();
    large_value["data"]["result"][0]["value"][1] = json!("1e300");
    assert!(parse::envelope(large_value, 200, &Budgets::default(), None).is_ok());
    for timestamp in [-253_402_300_800.0, 253_402_300_800.0] {
        let mut body = vector();
        body["data"]["result"][0]["value"][0] = json!(timestamp);
        assert_eq!(
            parse::envelope(body, 200, &Budgets::default(), None)
                .unwrap_err()
                .code,
            ErrorCode::InvalidResponse
        );
    }
}

#[tokio::test]
async fn range_rejects_outside_timestamps_and_more_points_than_requested() {
    for samples in [
        json!([[0, "1"], [16, "1"]]),
        json!([[-1, "1"], [15, "1"]]),
        json!([[0, "1"], [5, "1"], [15, "1"]]),
    ] {
        let body = json!({"status":"success","data":{"resultType":"matrix","result":[{"metric":{},"values":samples}]}});
        let (client, server) = fixture(200, body).await;
        let error = client
            .query_range("vector(1)", 0.0, 15.0, 15.0)
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidResponse);
        assert_eq!(error.http_status, Some(200));
        server.await.unwrap();
    }
}

#[test]
fn series_and_point_limits_never_silently_truncate() {
    let mut body = vector();
    body["data"]["result"] = Value::Array(vec![body["data"]["result"][0].clone(); 101]);
    assert_eq!(
        parse::envelope(body, 200, &Budgets::default(), None)
            .unwrap_err()
            .code,
        ErrorCode::BudgetExceeded
    );
    let body = json!({"status":"success","data":{"resultType":"matrix","result":[{"metric":{},"values":[[0,"0"],[1,"1"],[2,"2"]]}]}});
    assert_eq!(
        parse::envelope(
            body.clone(),
            200,
            &Budgets {
                max_points_per_series: 2,
                ..Budgets::default()
            },
            None
        )
        .unwrap_err()
        .code,
        ErrorCode::BudgetExceeded
    );
    assert_eq!(
        parse::envelope(
            body,
            200,
            &Budgets {
                max_total_points: 2,
                ..Budgets::default()
            },
            None
        )
        .unwrap_err()
        .code,
        ErrorCode::BudgetExceeded
    );
}

#[tokio::test]
async fn byte_budget_applies_to_content_length_and_chunked_bodies() {
    let budgets = Budgets {
        max_response_bytes: 64,
        ..Budgets::default()
    };
    let (client, server) =
        raw_fixture(http_response(200, &"x".repeat(65), ""), budgets.clone()).await;
    assert_eq!(
        client.query("up", None).await.unwrap_err().code,
        ErrorCode::ResponseTooLarge
    );
    server.await.unwrap();
    let response = format!(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n41\r\n{}\r\n0\r\n\r\n",
        "x".repeat(65)
    );
    let (client, server) = raw_fixture(response, budgets).await;
    assert_eq!(
        client.query("up", None).await.unwrap_err().code,
        ErrorCode::ResponseTooLarge
    );
    server.await.unwrap();
}

#[tokio::test]
async fn redirect_is_never_followed() {
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let response = http_response(
        302,
        "",
        &format!(
            "Location: http://{}/steal\r\n",
            target.local_addr().unwrap()
        ),
    );
    let (client, server) = raw_fixture(response, Budgets::default()).await;
    assert_eq!(
        client.query("up", None).await.unwrap_err().code,
        ErrorCode::Transport
    );
    server.await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(75), target.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn metadata_is_bounded_cached_and_cleared() {
    let (client, server) = fixture(200, json!({"status":"success","data":["up","cpu"]})).await;
    assert!(!client.label_values("__name__").await.unwrap().cached);
    assert!(server
        .await
        .unwrap()
        .starts_with("GET /prefix/api/v1/label/__name__/values?limit=1001"));
    assert!(client.label_values("__name__").await.unwrap().cached);
    client.clear_cache();
    assert!(client.cache.lock().unwrap().is_empty());
    assert_eq!(
        client.label_values("../../admin").await.unwrap_err().code,
        ErrorCode::InvalidQuery
    );
}

#[tokio::test]
async fn timeout_concurrency_and_cancellation_release_permits() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Arc::new(
        MonitoringClient::new(
            source(format!("http://{}", listener.local_addr().unwrap())),
            Budgets {
                timeout_seconds: 1,
                max_concurrent_requests: 1,
                ..Budgets::default()
            },
        )
        .unwrap(),
    );
    let c = client.clone();
    let task = tokio::spawn(async move { c.query("up", None).await });
    let (_stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        client.query("up", None).await.unwrap_err().code,
        ErrorCode::Busy
    );
    // Saturated clients never enqueue a waiter: return within a tiny fraction
    // of the network deadline, and cancellation cannot leave a queued request.
    assert_eq!(
        tokio::time::timeout(Duration::from_millis(50), client.query("up", None))
            .await
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    task.abort();
    let _ = task.await;
    assert_eq!(client.permits.available_permits(), 1);
    assert_eq!(
        client.query("up", None).await.unwrap_err().code,
        ErrorCode::Timeout
    );
    assert_eq!(client.permits.available_permits(), 1);
}

async fn tls_fixture(
    body: String,
    extra: &str,
) -> (SourceConfig, tokio::task::JoinHandle<Option<String>>) {
    tls_fixture_delayed(body, extra, Duration::ZERO).await
}
async fn tls_fixture_delayed(
    body: String,
    extra: &str,
    delay: Duration,
) -> (SourceConfig, tokio::task::JoinHandle<Option<String>>) {
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let pem = cert.pem();
    let key = rustls::pki_types::PrivatePkcs8KeyDer::from(signing_key.serialize_der());
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert.der().clone()], key.into())
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut config = source(format!(
        "https://localhost:{}",
        listener.local_addr().unwrap().port()
    ));
    config.tls.custom_ca_pem = Some(pem);
    let response = http_response(if extra.is_empty() { 200 } else { 302 }, &body, extra);
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        match acceptor.accept(stream).await {
            Ok(stream) => {
                tokio::time::sleep(delay).await;
                Some(exchange(stream, response).await)
            }
            Err(_) => None,
        }
    });
    (config, server)
}

#[tokio::test]
async fn tls_custom_ca_works_and_untrusted_or_wrong_hostname_fails() {
    let (config, server) = tls_fixture(vector().to_string(), "").await;
    MonitoringClient::new(config, Budgets::default())
        .unwrap()
        .query("up", None)
        .await
        .unwrap();
    assert!(server.await.unwrap().is_some());
    let (mut config, server) = tls_fixture(vector().to_string(), "").await;
    config.tls.custom_ca_pem = None;
    assert_eq!(
        MonitoringClient::new(config, Budgets::default())
            .unwrap()
            .query("up", None)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Transport
    );
    assert!(server.await.unwrap().is_none());
    let (mut config, server) = tls_fixture(vector().to_string(), "").await;
    config.endpoint = config.endpoint.replace("localhost", "127.0.0.1");
    assert_eq!(
        MonitoringClient::new(config, Budgets::default())
            .unwrap()
            .query("up", None)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Transport
    );
    assert!(server.await.unwrap().is_none());
}

#[tokio::test]
async fn scoped_auth_is_source_bound_expiring_redacted_and_cannot_redirect() {
    let secret = "fixture-SECRET-47";
    let mut body = vector();
    body["warnings"] = json!([format!("echo {secret}")]);
    let (mut config, server) = tls_fixture(body.to_string(), "").await;
    config.auth = AuthRef::BearerRef {
        credential_id: "credential-ref".into(),
    };
    let credential = SourceAuthorization::bearer(
        &config,
        Zeroizing::new(secret.into()),
        Duration::from_secs(5),
    )
    .unwrap();
    let client =
        MonitoringClient::with_authorization(config, Budgets::default(), credential).unwrap();
    assert!(!format!("{client:?}").contains(secret));
    let result = client.query("up", None).await.unwrap();
    assert!(!serde_json::to_string(&result).unwrap().contains(secret));
    assert!(server
        .await
        .unwrap()
        .unwrap()
        .contains(&format!("authorization: Bearer {secret}")));
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (mut config, server) = tls_fixture(
        String::new(),
        &format!(
            "Location: http://{}/steal\r\n",
            target.local_addr().unwrap()
        ),
    )
    .await;
    config.auth = AuthRef::BasicRef {
        credential_id: "ref".into(),
    };
    let credential = SourceAuthorization::basic(
        &config,
        Zeroizing::new("user".into()),
        Zeroizing::new("pass".into()),
        Duration::from_secs(5),
    )
    .unwrap();
    let mut client =
        MonitoringClient::with_authorization(config, Budgets::default(), credential).unwrap();
    assert_eq!(
        client.query("up", None).await.unwrap_err().code,
        ErrorCode::Transport
    );
    assert!(server
        .await
        .unwrap()
        .unwrap()
        .contains("authorization: Basic dXNlcjpwYXNz"));
    assert!(
        tokio::time::timeout(Duration::from_millis(75), target.accept())
            .await
            .is_err()
    );
    client.credential.as_mut().unwrap().expires = Instant::now();
    assert_eq!(
        client.query("up", None).await.unwrap_err().code,
        ErrorCode::AuthUnavailable
    );
}

#[test]
fn probe_requires_real_expected_data() {
    let wrong = QueryOutcome {
        status: "success".into(),
        http_status: 200,
        data: Some(QueryData::Vector { series: vec![] }),
        warnings: vec![],
        infos: vec![],
        partial: false,
        error: None,
        effective_step_seconds: None,
    };
    assert!(!capability(Ok(wrong), false).supported);
    let right = parse::envelope(vector(), 200, &Budgets::default(), None).unwrap();
    assert!(capability(Ok(right), false).supported);
}

#[test]
fn saved_agent_destination_roundtrip_is_data_not_a_transport_grant() {
    let mut config = source("http://127.0.0.1:9090/tenant".into());
    config.route = Route::AgentDestination {
        association_id: "11111111-1111-4111-8111-111111111111".into(),
        host_id: "22222222-2222-4222-8222-222222222222".into(),
        destination_ip: "127.0.0.1".into(), destination_port: 9090,
    };
    let data = serde_json::to_value(&config).unwrap();
    assert_eq!(data["route"], json!({"kind":"agentDestination",
        "associationId":"11111111-1111-4111-8111-111111111111",
        "hostId":"22222222-2222-4222-8222-222222222222",
        "destinationIp":"127.0.0.1","destinationPort":9090}));
    let decoded: SourceConfig = serde_json::from_value(data.clone()).unwrap();
    assert_eq!(decoded, config);
    assert!(decoded.validate().is_ok());
    assert_eq!(MonitoringClient::new(decoded, Budgets::default()).err().unwrap().code,
        ErrorCode::UnsupportedRoute);
    let mut extra = data;
    extra["route"]["routeId"] = json!("33333333-3333-4333-8333-333333333333");
    assert!(serde_json::from_value::<SourceConfig>(extra).is_err());
}

#[test]
fn saved_agent_destination_rejects_noncanonical_data_but_preserves_logical_origin() {
    let valid = json!({"id":"saved", "name":"Agent source", "endpoint":"http://127.0.0.1:9090",
        "route":{"kind":"agentDestination","associationId":"11111111-1111-4111-8111-111111111111",
        "hostId":"22222222-2222-4222-8222-222222222222","destinationIp":"127.0.0.1","destinationPort":9090},
        "tls":{"customCaPem":null},"auth":{"kind":"none"}});
    for (key, value) in [("associationId",json!("00000000-0000-0000-0000-000000000000")),
        ("hostId",json!("22222222-2222-4222-8222-22222222222A")),
        ("destinationIp",json!("localhost")),("destinationIp",json!("127.0.0.2")),
        ("destinationPort",json!(0))] {
        let mut bad=valid.clone();bad["route"][key]=value;
        assert!(serde_json::from_value::<SourceConfig>(bad).unwrap().validate().is_err());
    }
    for endpoint in ["https://metrics.example.invalid:9443/prefix", "http://10.20.30.40:3000/api/datasources/proxy/1"] {
        let mut logical=valid.clone();logical["endpoint"]=json!(endpoint);
        let source=serde_json::from_value::<SourceConfig>(logical).unwrap();
        assert!(source.validate().is_ok());assert_eq!(source.endpoint,endpoint);
        assert!(matches!(source.route,Route::AgentDestination{destination_port:9090,..}));
    }
    let mut ipv6=valid;ipv6["endpoint"]=json!("http://[::1]:9090");ipv6["route"]["destinationIp"]=json!("::1");
    assert!(serde_json::from_value::<SourceConfig>(ipv6).unwrap().validate().is_ok());
}
