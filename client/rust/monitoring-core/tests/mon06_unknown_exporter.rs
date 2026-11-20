//! Owned one-request HTTP fixture for the EXISTING Native presence gate.
//! Missing standard metric must be unavailable, never an invented zero or a
//! fallback expression based on an unknown vendor metric.
use cc_host_mapping::{HostBinding, Labels};
use cc_monitoring_core::panels::{PanelKind, PanelPreparation, UnavailableReason};
use cc_monitoring_core::{AuthRef, Budgets, MonitoringClient, Route, SourceConfig, TlsConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn unknown_exporter_is_missing_metric_at_selected_historical_end() {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let worker = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        loop {
            let mut chunk = [0; 2048];
            let count = socket.read(&mut chunk).await.unwrap();
            assert!(count > 0 && request.len() + count <= 16384);
            request.extend_from_slice(&chunk[..count]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let header_end = request.windows(4).position(|window| window == b"\r\n\r\n").unwrap() + 4;
        let headers = std::str::from_utf8(&request[..header_end]).unwrap();
        let content_length: usize = headers.lines().filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .unwrap().1.trim().parse().unwrap();
        assert!(content_length <= 8192);
        while request.len() < header_end + content_length {
            let mut chunk = [0; 2048];
            let count = socket.read(&mut chunk).await.unwrap();
            assert!(count > 0 && request.len() + count <= 16384);
            request.extend_from_slice(&chunk[..count]);
        }
        let headers = std::str::from_utf8(&request[..header_end]).unwrap();
        let first = headers.lines().next().unwrap();
        assert!(first.starts_with("POST /api/v1/query HTTP/"));
        let params = url::form_urlencoded::parse(&request[header_end..header_end + content_length])
            .into_owned().collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(params.get("time").map(|value| value.as_str()), Some("300"));
        let expression = params.get("query").unwrap();
        assert!(expression.starts_with("count by (__name__) (node_cpu_seconds_total{"));
        assert!(expression.contains("mode=\"idle\""));
        assert!(!expression.contains("vendor_cpu_idle_seconds_total"));
        assert!(!expression.contains("rate("));
        let body = r#"{"status":"success","data":{"resultType":"vector","result":[]}}"#;
        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        // Closing the listener means any accidental second metric query fails;
        // a legitimate missing-metrics path returns after this sole probe.
    });
    let binding = HostBinding::new(
        "5e7c5182-19df-48e1-99ed-87d28c9356e9", "owned-mon06", 1,
        Labels::from([("instance".into(),"unknown:9100".into()),("job".into(),"owned".into())]),
    ).unwrap();
    let client = MonitoringClient::new(SourceConfig {
        id:"owned-mon06".into(),name:"Owned disposable fixture".into(),endpoint:format!("http://{address}"),
        route:Route::Direct,tls:TlsConfig::default(),auth:AuthRef::None,
    },Budgets::default()).unwrap();
    let prepared = tokio::time::timeout(std::time::Duration::from_secs(5),
        client.prepare_panel(PanelKind::Cpu,&binding,0.0,300.0,60.0)).await.unwrap().unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), worker).await.unwrap().unwrap();
    let PanelPreparation::Unavailable { reason,missing_metrics,probe,error,.. }=prepared else {
        panic!("unknown exporter must not become a ready zero-valued CPU panel")
    };
    assert_eq!(reason,UnavailableReason::MissingMetrics);
    assert_eq!(missing_metrics,["node_cpu_seconds_total"]);
    assert!(probe.is_some());
    assert!(error.is_none());
}
