//! Run with the labelled disposable fixture script; never discovers a user server.
use cc_host_mapping::{HostBinding, Labels};
use cc_monitoring_core::panels::*;
use cc_monitoring_core::*;

#[tokio::test]
#[ignore = "requires an explicitly configured disposable Prometheus fixture"]
async fn official_prometheus_read_api() {
    let endpoint =
        std::env::var("CC_PROMETHEUS_FIXTURE_URL").expect("fixture endpoint must be explicit");
    let client = MonitoringClient::new(
        SourceConfig {
            id: "disposable-prometheus".into(),
            name: "Disposable test fixture".into(),
            endpoint,
            route: Route::Direct,
            tls: TlsConfig::default(),
            auth: AuthRef::None,
        },
        Budgets::default(),
    )
    .unwrap();
    let capabilities = client.probe().await;
    assert!(capabilities.supports_mvp(), "{capabilities:?}");
    let query = client.query("vector(1)", Some(1.0)).await.unwrap();
    assert!(matches!(query.data, Some(QueryData::Vector { .. })));
    let range = client
        .query_range("vector(1)", 0.0, 86400.0, 1.0)
        .await
        .unwrap();
    assert!(range.effective_step_seconds.unwrap() > 86.0);
    let Some(QueryData::Matrix { series }) = range.data else {
        panic!("matrix expected")
    };
    assert_eq!(series.len(), 1);
    assert!(series[0].samples.len() <= 1000);
    for (expression, expected) in [
        ("0/0", SampleValue::NaN),
        ("1/0", SampleValue::PositiveInfinity),
        ("-1/0", SampleValue::NegativeInfinity),
    ] {
        let Some(QueryData::Scalar { sample }) = client.query(expression, None).await.unwrap().data
        else {
            panic!("scalar expected")
        };
        assert_eq!(sample.value, expected);
    }
    let error = client.query("bad{", None).await.unwrap();
    assert_eq!(error.status, "error");
    assert_eq!(error.http_status, 400);
    assert!(error.error.is_some());
    let empty = client
        .query("absent_metric_fixture_94738", None)
        .await
        .unwrap();
    assert_eq!(empty.data, Some(QueryData::Vector { series: vec![] }));
    let metadata = client.label_values("__name__").await.unwrap();
    assert!(metadata.values.len() <= 1000);
    assert!(client.label_values("__name__").await.unwrap().cached);
    let binding = HostBinding::new(
        "5e7c5182-19df-48e1-99ed-87d28c9356e9",
        "disposable-prometheus",
        1,
        Labels::from([(
            "instance".into(),
            "literal\"} or up{job=\"escape\\\n".into(),
        )]),
    )
    .unwrap();
    for panel in [
        PanelKind::HostOverview,
        PanelKind::Cpu,
        PanelKind::Memory,
        PanelKind::Filesystem,
        PanelKind::DiskIo,
        PanelKind::Network,
        PanelKind::Docker,
        PanelKind::PostgreSql,
        PanelKind::Redis,
        PanelKind::Nginx,
    ] {
        for query in panel.query_templates(&binding).unwrap() {
            let result = client
                .query_range(&query.expression, 0.0, 15.0, 15.0)
                .await
                .unwrap();
            assert_eq!(result.status, "success", "{panel:?}: {}", query.expression);
        }
        let prepared = client
            .prepare_panel(panel, &binding, 0.0, 15.0, 15.0)
            .await
            .unwrap();
        assert!(
            matches!(
                prepared,
                PanelPreparation::Unavailable {
                    reason: UnavailableReason::MissingMetrics,
                    ..
                }
            ),
            "{prepared:?}"
        );
    }
    // Set union must retain metrics with identical non-name labels.
    let union=client.query("count by (__name__) (label_replace(vector(1), \"__name__\", \"metric_a\", \"\", \"\") or on (__name__) label_replace(vector(1), \"__name__\", \"metric_b\", \"\", \"\"))",Some(1.0)).await.unwrap();
    let Some(QueryData::Vector { series }) = union.data else {
        panic!("vector expected")
    };
    assert_eq!(series.len(), 2);
}
