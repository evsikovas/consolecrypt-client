use super::*;
use crate::{AuthRef, Budgets, Route, Sample, Series, SourceConfig, TlsConfig};
fn selector() -> ExporterSelector {
    ExporterSelector::new(BTreeMap::from([
        ("job".into(), "reviewed".into()),
        ("instance".into(), "explicit:9100".into()),
    ]))
    .unwrap()
}
fn plan() -> ExporterVerificationPlan {
    ExporterVerificationPlan::new(ExporterKind::NodeExporter, selector(), 120).unwrap()
}
fn outcomes(plan: &ExporterVerificationPlan) -> Vec<(Probe, QueryOutcome)> {
    plan.queries
        .iter()
        .map(|(probe, _)| {
            (
                *probe,
                QueryOutcome {
                    status: "success".into(),
                    http_status: 200,
                    warnings: vec![],
                    infos: vec![],
                    partial: false,
                    error: None,
                    effective_step_seconds: None,
                    data: Some(QueryData::Vector {
                        series: vec![Series {
                            labels: if *probe == Probe::Up {
                                plan.selector.0.clone()
                            } else {
                                BTreeMap::new()
                            },
                            samples: vec![Sample {
                                timestamp: 1_700_000_000.0,
                                value: SampleValue::Finite {
                                    value: if *probe == Probe::Up { 1.0 } else { 10.0 },
                                },
                            }],
                        }],
                    }),
                },
            )
        })
        .collect()
}
fn sample(data: &mut [(Probe, QueryOutcome)], index: usize) -> &mut Sample {
    let Some(QueryData::Vector { series }) = &mut data[index].1.data else {
        panic!()
    };
    &mut series[0].samples[0]
}
#[test]
fn exact_quoted_fixed_plan_and_container_filter_are_bounded() {
    let p = plan();
    assert_eq!(p.queries.len(), 5);
    assert_eq!(
        p.queries[0].1,
        "up{instance=\"explicit:9100\",job=\"reviewed\"}"
    );
    assert!(
        p.queries[2]
            .1
            .contains("time() - timestamp(node_cpu_seconds_total{")
    );
    let p = ExporterVerificationPlan::new(ExporterKind::Cadvisor, selector(), 120).unwrap();
    assert_eq!(p.queries.len(), 4);
    assert!(p.queries[2].1.contains("image!=\"\""));
    let labels = ExporterSelector::new(BTreeMap::from([
        ("job".into(), "a\"} or vector(1) #".into()),
        ("instance".into(), r"c:\metrics".into()),
    ]))
    .unwrap();
    let p = ExporterVerificationPlan::new(ExporterKind::NodeExporter, labels, 120).unwrap();
    assert!(p.queries[0].1.contains(r#"job="a\"} or vector(1) #""#));
    assert!(p.queries[0].1.contains(r#"instance="c:\\metrics""#));
    assert!(ExporterVerificationPlan::new(ExporterKind::NodeExporter, selector(), 0).is_err());
    assert!(ExporterVerificationPlan::new(ExporterKind::NodeExporter, selector(), 301).is_err());
}
#[test]
fn selector_duplicate_reserved_control_and_expansion_budgets_fail() {
    assert!(
        serde_json::from_str::<ExporterSelector>(r#"{"job":"x","job":"x","instance":"y"}"#)
            .is_err()
    );
    for labels in [
        BTreeMap::new(),
        BTreeMap::from([("job".into(), "x".into())]),
        BTreeMap::from([
            ("job".into(), "x".into()),
            ("instance".into(), "y\n".into()),
        ]),
        BTreeMap::from([
            ("job".into(), "x".into()),
            ("instance".into(), "y".into()),
            ("__name__".into(), "up".into()),
        ]),
    ] {
        assert!(ExporterSelector::new(labels).is_err());
    }
    let mut image = selector().0;
    image.insert("image".into(), "x".into());
    assert!(
        ExporterVerificationPlan::new(
            ExporterKind::Cadvisor,
            ExporterSelector::new(image).unwrap(),
            120
        )
        .is_err()
    );
    let huge = ExporterSelector::new(BTreeMap::from([
        ("job".into(), "\\".repeat(4096)),
        ("instance".into(), "\\".repeat(4096)),
    ]))
    .unwrap();
    assert!(ExporterVerificationPlan::new(ExporterKind::NodeExporter, huge, 120).is_err());
    assert_eq!(format!("{:?}", selector()), "ExporterSelector(<redacted>)");
}
#[test]
fn complete_fresh_samples_are_only_observations_and_missing_duplicate_status_never_zero() {
    let p = plan();
    let good = outcomes(&p);
    let ExporterObservation::Observed {
        max_sample_age_seconds,
        ..
    } = p.observe(&good).unwrap()
    else {
        panic!("expected observation")
    };
    assert_eq!(max_sample_age_seconds, 10.0);
    assert!(matches!(
        p.observe(&good[..4]),
        Err(ExporterUnavailable::IncompleteResponse)
    ));
    let mut bad = outcomes(&p);
    bad[0].1.data = Some(QueryData::Vector { series: vec![] });
    assert_eq!(
        p.observe(&bad).unwrap_err(),
        ExporterUnavailable::MissingSeries
    );
    let mut bad = outcomes(&p);
    let Some(QueryData::Vector { series }) = &mut bad[0].1.data else {
        panic!()
    };
    series.push(series[0].clone());
    assert_eq!(
        p.observe(&bad).unwrap_err(),
        ExporterUnavailable::AmbiguousTarget
    );
    for case in 0..5 {
        let mut bad = outcomes(&p);
        match case {
            0 => bad[1].1.partial = true,
            1 => bad[1].1.warnings.push("untrusted".into()),
            2 => bad[1].1.infos.push("untrusted".into()),
            3 => bad[1].1.status = "error".into(),
            _ => bad[1].1.http_status = 503,
        };
        assert_eq!(
            p.observe(&bad).unwrap_err(),
            ExporterUnavailable::IncompleteResponse
        );
    }
}
#[test]
fn nonfinite_future_and_stale_sample_age_is_not_query_evaluation_time() {
    let p = plan();
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut bad = outcomes(&p);
        sample(&mut bad, 2).value = SampleValue::Finite { value };
        assert_eq!(
            p.observe(&bad).unwrap_err(),
            ExporterUnavailable::InvalidResponse
        );
    }
    let mut bad = outcomes(&p);
    sample(&mut bad, 2).value = SampleValue::Finite { value: -1.0 };
    assert_eq!(
        p.observe(&bad).unwrap_err(),
        ExporterUnavailable::ClockUnknown
    );
    let mut bad = outcomes(&p);
    sample(&mut bad, 2).value = SampleValue::Finite { value: 121.0 };
    assert_eq!(
        p.observe(&bad).unwrap_err(),
        ExporterUnavailable::StaleSeries
    );
    let mut bad = outcomes(&p);
    sample(&mut bad, 2).value = SampleValue::Finite { value: 119.0 };
    sample(&mut bad, 2).timestamp -= 5.0;
    assert_eq!(
        p.observe(&bad).unwrap_err(),
        ExporterUnavailable::StaleSeries
    );
    let mut bad = outcomes(&p);
    sample(&mut bad, 2).timestamp += 6.0;
    assert_eq!(
        p.observe(&bad).unwrap_err(),
        ExporterUnavailable::ClockUnknown
    );
    let mut bad = outcomes(&p);
    sample(&mut bad, 2).timestamp -= 5.0;
    sample(&mut bad, 3).timestamp += 5.0;
    assert_eq!(
        p.observe(&bad).unwrap_err(),
        ExporterUnavailable::ClockUnknown,
        "span10 cannot pass because each endpoint is5 from first"
    );
}
#[test]
fn wrong_labels_down_target_and_nonvector_or_extra_samples_are_denied() {
    let p = plan();
    let mut bad = outcomes(&p);
    sample(&mut bad, 0).value = SampleValue::Finite { value: 0.0 };
    assert_eq!(
        p.observe(&bad).unwrap_err(),
        ExporterUnavailable::TargetDown
    );
    let mut bad = outcomes(&p);
    let Some(QueryData::Vector { series }) = &mut bad[0].1.data else {
        panic!()
    };
    series[0].labels.insert("instance".into(), "wrong".into());
    assert_eq!(
        p.observe(&bad).unwrap_err(),
        ExporterUnavailable::InvalidResponse
    );
    let mut bad = outcomes(&p);
    let value = sample(&mut bad, 2).clone();
    bad[2].1.data = Some(QueryData::Scalar { sample: value });
    assert_eq!(
        p.observe(&bad).unwrap_err(),
        ExporterUnavailable::InvalidResponse
    );
    let mut bad = outcomes(&p);
    let Some(QueryData::Vector { series }) = &mut bad[2].1.data else {
        panic!()
    };
    series[0]
        .labels
        .insert("unexpected".into(), "untrusted".into());
    assert_eq!(
        p.observe(&bad).unwrap_err(),
        ExporterUnavailable::InvalidResponse
    );
}

#[tokio::test]
async fn actual_http_uses_only_fixed_queries_same_configured_origin_and_no_caller_time() {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let p = plan();
    let queries = p.queries.iter().map(|(_, q)| q.clone()).collect::<Vec<_>>();
    let worker = tokio::spawn(async move {
        for (index, query) in queries.iter().enumerate() {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut raw = Vec::new();
            let header_end = loop {
                let mut data = [0u8; 1024];
                let n = socket.read(&mut data).await.unwrap();
                assert!(n > 0);
                raw.extend_from_slice(&data[..n]);
                assert!(raw.len() < 32768);
                if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    break end + 4;
                }
            };
            let headers = String::from_utf8(raw[..header_end].to_vec()).unwrap();
            assert_eq!(headers.lines().next(), Some("POST /api/v1/query HTTP/1.1"));
            let mut fields = BTreeMap::new();
            for line in headers.lines().skip(1).filter(|line| !line.is_empty()) {
                let (name, value) = line.split_once(':').unwrap();
                assert!(
                    fields
                        .insert(name.to_ascii_lowercase(), value.trim().to_owned())
                        .is_none()
                );
            }
            assert_eq!(
                fields.get("host"),
                Some(&socket.local_addr().unwrap().to_string())
            );
            assert_eq!(
                fields.get("content-type").map(String::as_str),
                Some("application/x-www-form-urlencoded")
            );
            assert!(!fields.contains_key("transfer-encoding"));
            assert!(!fields.contains_key("authorization"));
            let length: usize = fields["content-length"].parse().unwrap();
            assert!(length > 0 && length < 32768 - header_end);
            while raw.len() < header_end + length {
                let mut data = [0u8; 1024];
                let n = socket.read(&mut data).await.unwrap();
                assert!(n > 0);
                raw.extend_from_slice(&data[..n]);
                assert!(raw.len() <= header_end + length);
            }
            assert_eq!(raw.len(), header_end + length);
            let pairs = url::form_urlencoded::parse(&raw[header_end..])
                .into_owned()
                .collect::<Vec<_>>();
            assert_eq!(
                pairs.len(),
                3,
                "only fixed query and Native timeout/limit budgets"
            );
            let pairs = pairs.into_iter().collect::<BTreeMap<_, _>>();
            assert_eq!(pairs.len(), 3, "duplicate form parameters are not accepted");
            assert!(!pairs.contains_key("time"));
            let budgets = Budgets::default();
            assert_eq!(
                pairs.get("timeout"),
                Some(&format!("{}s", budgets.timeout_seconds))
            );
            assert_eq!(
                pairs.get("limit"),
                Some(&(budgets.max_series + 1).to_string())
            );
            assert_eq!(pairs.get("query").map(String::as_str), Some(query.as_str()));
            let labels = if index == 0 {
                serde_json::json!({"job":"reviewed","instance":"explicit:9100","untrustedUrl":"https://never-follow.invalid"})
            } else {
                serde_json::json!({})
            };
            let body=serde_json::json!({"status":"success","data":{"resultType":"vector","result":[{"metric":labels,"value":[1700000000,if index==0{"1"}else{"10"}]}]}}).to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    });
    let client = MonitoringClient::new(
        SourceConfig {
            id: "owned".into(),
            name: "owned".into(),
            endpoint: format!("http://{address}"),
            route: Route::Direct,
            tls: TlsConfig::default(),
            auth: AuthRef::None,
        },
        Budgets::default(),
    )
    .unwrap();
    assert!(matches!(
        p.execute(&client).await,
        ExporterObservation::Observed { .. }
    ));
    worker.await.unwrap();
}
