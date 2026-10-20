use cc_k8s_core::{
    ai_tools::{AiToolSession, ResourceKey, ToolDestination, ToolError, ToolRequest},
    gitops::{ArgoAction, FluxAction},
    policy::{Audit, AuditEntry, DeviceClass, Policy},
    resources::{
        DebugContainer, ListQuery, LogQuery, PodTerminalInput, PodTerminalOutput,
        PodTerminalRequest, ReadOptions, ResourceApi, ResourceChange, ResourceError, ResourceRef,
        ResourceRelation, WatchOptions,
    },
    session::{Authentication, ConnectOptions, ConnectionRoute, KubernetesSession},
};
use futures::{SinkExt, StreamExt};
use secrecy::SecretString;
use serde_json::{json, Value};
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;
use tower::BoxError;
use uuid::Uuid;
use zeroize::Zeroizing;

struct AuditLog(Mutex<Vec<AuditEntry>>);
impl Audit for AuditLog {
    fn record(&self, entry: AuditEntry) -> Result<(), BoxError> {
        self.0.lock().unwrap().push(entry);
        Ok(())
    }
}

struct Fixture {
    address: SocketAddr,
    cert: Vec<u8>,
    requests: Arc<Mutex<Vec<String>>>,
    watches: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn resource(name: &str, kind: &str, namespaced: bool) -> Value {
    json!({"name":name,"kind":kind,"namespaced":namespaced,"verbs":["get","list","watch","create","patch","delete"]})
}
fn secret() -> Value {
    json!({"apiVersion":"v1","kind":"Secret","metadata":{"name":"sample","namespace":"default","uid":"id-1","resourceVersion":"20",
        "managedFields":[{"secret":"fixture-managed"}],"annotations":{"kubectl.kubernetes.io/last-applied-configuration":"fixture-last","custom":"fixture-annotation"}},
        "data":{"password":"fixture-password"},"stringData":{"token":"fixture-token"}})
}
fn pod(rv: &str) -> Value {
    json!({"apiVersion":"v1","kind":"Pod","metadata":{"name":"web","namespace":"default","uid":"pod-id","resourceVersion":rv},
        "spec":{"containers":[{"name":"web","envFrom":[{"secretRef":{"name":"sample"}}]}]},"status":{"phase":"Running"}})
}

#[tokio::test]
async fn helm_live_comparison_is_read_only_in_production_masked_and_partial_on_rbac() {
    let fixture = Fixture::start().await;
    let session = fixture.session(true);
    let api = ResourceApi::new(&session);
    api.discover().await.unwrap();
    let comparison = api
        .helm_compare_live("helm-compare", "sh.helm.release.v1.fixture.v4")
        .await
        .unwrap();
    assert_eq!(comparison.revision, 4);
    assert!(comparison.partial && comparison.masked_fields);
    assert_eq!(comparison.items.len(), 4);
    let changed = comparison
        .items
        .iter()
        .find(|item| item.reference.name == "changed")
        .unwrap();
    assert_eq!(changed.state, "changed");
    assert!(changed
        .diff
        .changes
        .iter()
        .any(|change| change.path == "/spec/containers/0/image"));
    assert!(comparison.items.iter().any(|item| item.state == "missing"));
    assert!(comparison
        .items
        .iter()
        .any(|item| item.state == "unavailable"));
    let output = serde_json::to_string(&comparison).unwrap();
    assert!(!output.contains("private") && !output.contains("imagePullPolicy"));
    assert!(fixture
        .requests
        .lock()
        .unwrap()
        .iter()
        .all(|request| request.starts_with("GET ")));
    session.guard().close();
    assert_eq!(
        api.helm_compare_live("helm-compare", "sh.helm.release.v1.fixture.v4")
            .await
            .unwrap_err(),
        ResourceError::Closed
    );
}

impl Fixture {
    async fn start() -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let cert = rcgen::generate_simple_self_signed(vec!["api.example.test".into()]).unwrap();
        let cert_der = cert.cert.der().to_vec();
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.cert.der().clone()],
                rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der())
                    .into(),
            )
            .unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        let watches = Arc::new(AtomicUsize::new(0));
        let watch_count = watches.clone();
        let task = tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let acceptor = acceptor.clone();
                let seen = seen.clone();
                let watch_count = watch_count.clone();
                tokio::spawn(async move {
                    let Ok(mut stream) = acceptor.accept(socket).await else {
                        return;
                    };
                    let mut header = Vec::new();
                    let mut byte = [0];
                    while header.len() < 32 * 1024 && !header.ends_with(b"\r\n\r\n") {
                        if stream.read_exact(&mut byte).await.is_err() {
                            return;
                        }
                        header.push(byte[0]);
                    }
                    let header = String::from_utf8(header).unwrap();
                    let line = header.lines().next().unwrap().to_owned();
                    seen.lock().unwrap().push(line.clone());
                    let mut parts = line.split(' ');
                    let method = parts.next().unwrap();
                    let path = parts.next().unwrap();
                    if header
                        .lines()
                        .any(|line| line.eq_ignore_ascii_case("upgrade: websocket"))
                    {
                        let key = header
                            .lines()
                            .filter_map(|line| line.split_once(':'))
                            .find(|(key, _)| key.eq_ignore_ascii_case("sec-websocket-key"))
                            .unwrap()
                            .1
                            .trim();
                        let accept = tokio_tungstenite::tungstenite::handshake::derive_accept_key(
                            key.as_bytes(),
                        );
                        let reply=format!("HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: {accept}\r\nSec-WebSocket-Protocol: v4.channel.k8s.io\r\n\r\n");
                        if stream.write_all(reply.as_bytes()).await.is_err() {
                            return;
                        }
                        use tokio_tungstenite::tungstenite::{protocol::Role, Message};
                        let mut socket = tokio_tungstenite::WebSocketStream::from_raw_socket(
                            stream,
                            Role::Server,
                            None,
                        )
                        .await;
                        if path.contains("/portforward?") {
                            socket
                                .send(Message::Binary(vec![0, 80, 0].into()))
                                .await
                                .unwrap();
                            socket
                                .send(Message::Binary(vec![1, 80, 0].into()))
                                .await
                                .unwrap();
                            while let Some(Ok(message)) = socket.next().await {
                                if let Message::Binary(bytes) = message {
                                    let _ = socket.send(Message::Binary(bytes)).await;
                                } else if message.is_close() {
                                    break;
                                }
                            }
                        } else {
                            // Exercise stdin and output multiplexing through
                            // the real TLS/WebSocket upgrade, then remote exit.
                            while let Some(Ok(message)) = socket.next().await {
                                if let Message::Binary(bytes) = message {
                                    if bytes.first() == Some(&0) {
                                        let mut out = vec![1];
                                        out.extend_from_slice(&bytes[1..]);
                                        socket.send(Message::Binary(out.into())).await.unwrap();
                                        let mut status = vec![3];
                                        status.extend_from_slice(br#"{"apiVersion":"v1","kind":"Status","status":"Success"}"#);
                                        socket.send(Message::Binary(status.into())).await.unwrap();
                                        let _ = socket.close(None).await;
                                        break;
                                    }
                                }
                            }
                        }
                        return;
                    }
                    let mut status = "200 OK";
                    let body = if path == "/version" {
                        json!({"gitVersion":"v1.32.0"}).to_string()
                    } else if path.starts_with("/api/v1/namespaces/helm-compare/secrets/sh.helm.release.v1.") {
                        use base64::Engine;
                        let object = |kind: &str, name: &str| {
                            let mut object = json!({"apiVersion":"v1","kind":kind,
                                "metadata":{"name":name,"namespace":"helm-compare"}});
                            if kind == "Secret" {
                                object["data"] = json!({"token":"private"});
                            } else {
                                object["spec"] = json!({"containers":[{"name":"web","image":"web:1",
                                    "env":[{"name":"PASSWORD","value":"private-env"}]}]});
                            }
                            object
                        };
                        let mut objects = vec![object("Secret","masked"), object("Pod","changed"),
                            object("Pod","missing"), object("Pod","blocked")];
                        objects.push(json!({"apiVersion":"unknown/v1","kind":"Unknown","metadata":{"name":"skip"}}));
                        let manifest = objects.into_iter().map(|object| serde_saphyr::to_string(&object).unwrap())
                            .collect::<Vec<_>>().join("---\n");
                        let release = json!({"name":"fixture","namespace":"helm-compare","version":4,
                            "info":{"status":"deployed"},"chart":{"metadata":{"name":"fixture","version":"1"}},"manifest":manifest});
                        let engine = base64::engine::general_purpose::STANDARD;
                        json!({"apiVersion":"v1","kind":"Secret","type":"helm.sh/release.v1",
                            "data":{"release":engine.encode(engine.encode(release.to_string()))}}).to_string()
                    } else if path == "/api/v1/namespaces/helm-compare/secrets/masked" {
                        json!({"apiVersion":"v1","kind":"Secret","metadata":{"name":"masked","namespace":"helm-compare"},
                            "data":{"token":"different-private"}}).to_string()
                    } else if path == "/api/v1/namespaces/helm-compare/pods/changed" {
                        json!({"apiVersion":"v1","kind":"Pod","metadata":{"name":"changed","namespace":"helm-compare"},
                            "spec":{"containers":[{"name":"web","image":"web:2","env":[{"name":"PASSWORD","value":"different-private-env"}],
                                "imagePullPolicy":"Always"}]}}).to_string()
                    } else if path == "/api/v1/namespaces/helm-compare/pods/missing" {
                        status = "404 Not Found";
                        "{}".into()
                    } else if path == "/api/v1/namespaces/helm-compare/pods/blocked" {
                        status = "403 Forbidden";
                        "{}".into()
                    } else if path.contains("/namespaces/privacy/") {
                        let helm = path.contains("/helmreleases?");
                        let object = if helm {
                            json!({"apiVersion":"helm.toolkit.fluxcd.io/v2","kind":"HelmRelease",
                                "metadata":{"name":"private","namespace":"privacy","uid":"private-id","resourceVersion":"1"},
                                "spec":{"values":{"password":"fixture-table-secret"}}})
                        } else {
                            json!({"apiVersion":"v1","kind":"Secret",
                                "metadata":{"name":"private","namespace":"privacy","uid":"private-id","resourceVersion":"1"},
                                "data":{"credential":"fixture-table-secret"}})
                        };
                        if path.contains("includeObject=") {
                            json!({"apiVersion":"meta.k8s.io/v1","kind":"Table","metadata":{"resourceVersion":"1"},
                                "columnDefinitions":[{"name":"Credential","type":"string"}],
                                "rows":[{"cells":["fixture-table-secret"],"object":object}]}).to_string()
                        } else {
                            json!({"metadata":{"resourceVersion":"1"},"items":[object]}).to_string()
                        }
                    } else if path.contains("/namespaces/oversized/gitrepositories?") {
                        json!({"metadata":{"resourceVersion":"1"},"items":[{
                            "apiVersion":"source.toolkit.fluxcd.io/v1","kind":"GitRepository",
                            "metadata":{"name":"large","namespace":"oversized","uid":"large-id","resourceVersion":"1",
                                "annotations":{"fixture": "x".repeat(512 * 1024)}}}]}).to_string()
                    } else if path == "/openapi/v3" {
                        json!({"paths":{"api/v1":{"serverRelativeURL":"/openapi/v3/api/v1?hash=fixture"}}}).to_string()
                    } else if path == "/openapi/v3/api/v1?hash=fixture" {
                        json!({"components":{"schemas":{"io.k8s.api.core.v1.ConfigMap":{"type":"object","x-kubernetes-group-version-kind":[{"group":"","version":"v1","kind":"ConfigMap"}]}}}}).to_string()
                    } else if method == "POST"
                        && path.starts_with("/api/v1/namespaces/default/configmaps?")
                    {
                        json!({"apiVersion":"v1","kind":"ConfigMap","metadata":{"name":"created","namespace":"default","uid":"created-id","resourceVersion":"40"},"data":{"example":"safe"}}).to_string()
                    } else if path == "/api/v1" {
                        json!({"resources":[resource("pods","Pod",true),resource("pods/log","Pod",true),resource("secrets","Secret",true),resource("configmaps","ConfigMap",true),resource("namespaces","Namespace",false),resource("nodes","Node",false),resource("events","Event",true)]}).to_string()
                    } else if path == "/apis" {
                        json!({"groups":[
                            {"name":"apps","versions":[{"version":"v1"}]},
                            {"name":"example.test","versions":[{"version":"v1"}]},
                            {"name":"kustomize.toolkit.fluxcd.io","versions":[{"version":"v1"}]},
                            {"name":"source.toolkit.fluxcd.io","versions":[{"version":"v1"}]},
                            {"name":"helm.toolkit.fluxcd.io","versions":[{"version":"v2"}]},
                            {"name":"argoproj.io","versions":[{"version":"v1alpha1"}]},
                            {"name":"offline.test","versions":[{"version":"v1"}]}
                        ]})
                        .to_string()
                    } else if path == "/apis/apps/v1" {
                        json!({"resources":[resource("deployments","Deployment",true)]}).to_string()
                    } else if path == "/apis/example.test/v1" {
                        json!({"resources":[resource("widgets","Widget",true)]}).to_string()
                    } else if path == "/apis/kustomize.toolkit.fluxcd.io/v1" {
                        json!({"resources":[resource("kustomizations","Kustomization",true)]})
                            .to_string()
                    } else if path == "/apis/source.toolkit.fluxcd.io/v1" {
                        json!({"resources":[resource("gitrepositories","GitRepository",true),resource("helmcharts","HelmChart",true)]}).to_string()
                    } else if path == "/apis/helm.toolkit.fluxcd.io/v2" {
                        json!({"resources":[resource("helmreleases","HelmRelease",true)]}).to_string()
                    } else if path == "/apis/argoproj.io/v1alpha1" {
                        json!({"resources":[resource("applications","Application",true)]})
                            .to_string()
                    } else if path == "/apis/offline.test/v1" {
                        status = "503 Service Unavailable";
                        "fixture-error-credential".into()
                    } else if path == "/apis/authorization.k8s.io/v1/selfsubjectrulesreviews" {
                        json!({"apiVersion":"authorization.k8s.io/v1","kind":"SelfSubjectRulesReview","status":{"incomplete":false,"resourceRules":[{"verbs":["get","list","watch"],"apiGroups":[""],"resources":["pods"]}]}}).to_string()
                    } else if path.starts_with("/apis/example.test/v1/namespaces/default/widgets?")
                    {
                        json!({"apiVersion":"meta.k8s.io/v1","kind":"Table","metadata":{"resourceVersion":"30"},
                            "columnDefinitions":[{"name":"Name","type":"string","format":"name"},{"name":"Phase","type":"string"}],
                            "rows":[{"cells":["widget","Active"],"object":{"apiVersion":"example.test/v1","kind":"Widget","metadata":{"name":"widget","namespace":"default","uid":"widget-id","resourceVersion":"30"},"status":{"phase":"Active"}}}]
                        }).to_string()
                    } else if method == "PATCH" || method == "DELETE" || path.ends_with("/eviction")
                    {
                        pod("31").to_string()
                    } else if path == "/apis/helm.toolkit.fluxcd.io/v2/namespaces/default/helmreleases/demo" {
                        json!({"apiVersion":"helm.toolkit.fluxcd.io/v2","kind":"HelmRelease","metadata":{"name":"demo","namespace":"default","uid":"helm-id","resourceVersion":"30"},
                            "spec":{"chartRef":{"kind":"HelmChart","name":"chart"}}}).to_string()
                    } else if path == "/apis/helm.toolkit.fluxcd.io/v2/namespaces/default/helmreleases/cycle" {
                        json!({"apiVersion":"helm.toolkit.fluxcd.io/v2","kind":"HelmRelease","metadata":{"name":"cycle","namespace":"default","uid":"cycle-id","resourceVersion":"30"},
                            "spec":{"chartRef":{"kind":"HelmChart","name":"cycle"}}}).to_string()
                    } else if path == "/apis/source.toolkit.fluxcd.io/v1/namespaces/default/helmcharts/chart" {
                        json!({"apiVersion":"source.toolkit.fluxcd.io/v1","kind":"HelmChart","metadata":{"name":"chart","namespace":"default","uid":"chart-id","resourceVersion":"30"},
                            "spec":{"sourceRef":{"kind":"GitRepository","name":"git"}}}).to_string()
                    } else if path == "/apis/source.toolkit.fluxcd.io/v1/namespaces/default/helmcharts/cycle" {
                        json!({"apiVersion":"source.toolkit.fluxcd.io/v1","kind":"HelmChart","metadata":{"name":"cycle","namespace":"default","uid":"chart-cycle-id","resourceVersion":"30"},
                            "spec":{"sourceRef":{"kind":"HelmChart","name":"cycle"}}}).to_string()
                    } else if path == "/apis/source.toolkit.fluxcd.io/v1/namespaces/default/gitrepositories/git" {
                        json!({"apiVersion":"source.toolkit.fluxcd.io/v1","kind":"GitRepository","metadata":{"name":"git","namespace":"default","uid":"git-id","resourceVersion":"30"},
                            "spec":{"url":"https://example.test/repo"}}).to_string()
                    } else if path.starts_with("/api/v1/namespaces/default/secrets/sample") {
                        secret().to_string()
                    } else if path.starts_with("/api/v1/namespaces/default/secrets?") {
                        json!({"metadata":{"resourceVersion":"20"},"items":[secret()]}).to_string()
                    } else if path.starts_with("/api/v1/namespaces/default/configmaps/malformed") {
                        "{\"data\":\"fixture-malformed-credential\"".into()
                    } else if path.starts_with("/api/v1/namespaces/default/configmaps/huge") {
                        "X".repeat(8 * 1024 * 1024 + 1)
                    } else if path.starts_with("/api/v1/namespaces/default/events?") {
                        json!({"metadata":{"resourceVersion":"30"},"items":[{
                            "kind":"Event","apiVersion":"v1","metadata":{"name":"web.backoff","namespace":"default","uid":"event-id","resourceVersion":"30"},
                            "reason":"BackOff","type":"Warning","message":"password=fixture-event-password","lastTimestamp":chrono::Utc::now().to_rfc3339(),
                            "involvedObject":{"kind":"Pod","name":"web","namespace":"default","uid":"pod-id"}
                        }]}).to_string()
                    } else if path.starts_with("/api/v1/namespaces/default/pods/web/log?") {
                        "2026-10-20T01:00:00Z first\n2026-10-20T01:00:01Z second password=fixture-log-password\n".into()
                    } else if path == "/api/v1/namespaces/default/pods/web" {
                        pod("30").to_string()
                    } else if path == "/api/v1/namespaces/default/pods/large" {
                        let mut object = pod("30");
                        object["metadata"]["name"] = json!("large");
                        object["metadata"]["annotations"] = json!((0..400)
                            .map(|index| (
                                format!("example.test/note-{index}"),
                                "readable words ".repeat(200)
                            ))
                            .collect::<std::collections::BTreeMap<_, _>>());
                        object.to_string()
                    } else if path.starts_with("/api/v1/namespaces/default/pods?")
                        && path.contains("watch=true")
                    {
                        let n = watch_count.fetch_add(1, Ordering::SeqCst);
                        if n == 0 {
                            format!(
                                "{}\n",
                                json!({"type":"ERROR","object":{"kind":"Status","code":410,"message":"fixture-expired-private"}})
                            )
                        } else {
                            format!(
                                "{}\n{}\n",
                                json!({"type":"MODIFIED","object":pod("31")}),
                                json!({"type":"BOOKMARK","object":{"metadata":{"resourceVersion":"32"}}})
                            )
                        }
                    } else if path.starts_with("/api/v1/namespaces/default/pods?") {
                        let mut item = pod("30");
                        item.as_object_mut().unwrap().remove("kind");
                        item.as_object_mut().unwrap().remove("apiVersion");
                        json!({"metadata":{"resourceVersion":"30"},"items":[item]}).to_string()
                    } else if path.starts_with("/api/v1/namespaces?") {
                        json!({"metadata":{"resourceVersion":"30"},"items":[{"kind":"Namespace","metadata":{"name":"default","uid":"ns-id","resourceVersion":"30"}}]}).to_string()
                    } else {
                        status = "404 Not Found";
                        "fixture-error-credential".into()
                    };
                    let reply=format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len());
                    if stream.write_all(reply.as_bytes()).await.is_ok() {
                        let _ = stream.write_all(body.as_bytes()).await;
                    }
                    let _ = stream.shutdown().await;
                });
            }
        });
        Self {
            address,
            cert: cert_der,
            requests,
            watches,
            task,
        }
    }
    fn session(&self, production: bool) -> KubernetesSession {
        KubernetesSession::new(
            ConnectOptions {
                server: "https://api.example.test".parse().unwrap(),
                namespace: "default".into(),
                root_certificates: vec![self.cert.clone()],
                tls_server_name: None,
                insecure_skip_tls_verify: false,
                route: ConnectionRoute::SshLoopback(self.address),
            },
            Authentication::Bearer(SecretString::from("synthetic-test-token")),
            Policy {
                context_id: Uuid::new_v4(),
                context_name: "fixture".into(),
                production,
                device: DeviceClass::Desktop,
                allow_exec_in_production: false,
                allow_port_forward_in_production: false,
            },
            Arc::new(AuditLog(Mutex::new(Vec::new()))),
        )
        .unwrap()
    }
}

#[tokio::test]
async fn schemas_create_preconditions_and_ai_aliases_budgets_are_enforced() {
    use cc_ai_core::PrivacyProfile;
    let fixture = Fixture::start().await;
    let session = fixture.session(true);
    let api = ResourceApi::new(&session);
    api.discover().await.unwrap();
    let config = ResourceRef {
        api: api.resolve("", "v1", "configmaps").unwrap(),
        namespace: Some("default".into()),
        name: "created".into(),
    };
    assert_eq!(
        api.schema(&config.api).await.unwrap().schema["type"],
        "object"
    );
    let manifest=Zeroizing::new("apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: created\n  namespace: default\ndata:\n  example: safe\n".to_owned());
    assert!(api.create(&config, manifest.clone(), true).await.is_err());
    session.guard().open_write_window("fixture").unwrap();
    let result = api.create(&config, manifest.clone(), true).await.unwrap();
    assert_eq!(result.summary.name, "created");
    assert!(fixture
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|line| line.contains("fieldManager=consoleCrypt&dryRun=All")));
    assert!(api
        .create(
            &config,
            Zeroizing::new(manifest.replace("name: created", "name: mismatch")),
            false
        )
        .await
        .is_err());
    let tools = AiToolSession::new(
        api.clone(),
        ToolDestination::ApprovedExternal,
        PrivacyProfile::Strict,
        "private-cluster",
        false,
    );
    tools.invoke_json(r#"{"tool":"get_object","resource":{"group":"","version":"v1","resource":"secrets"},"namespace":"default","name":"sample"}"#).await.unwrap();
    let namespace = tools.sanitize_text("default").unwrap();
    let name = tools.sanitize_text("sample").unwrap();
    assert_ne!(namespace.as_str(), "default");
    assert_ne!(name.as_str(), "sample");
    let result=tools.invoke_json(&json!({"tool":"get_object","resource":{"group":"","version":"v1","resource":"secrets"},"namespace":namespace.as_str(),"name":name.as_str()}).to_string()).await.unwrap();
    assert!(!result.content.as_str().contains("fixture-password"));
    for _ in 2..12 {
        assert_eq!(
            tools.invoke_json(r#"{"tool":"delete"}"#).await.err(),
            Some(ToolError::InvalidTool)
        );
    }
    let count = fixture.requests.lock().unwrap().len();
    assert_eq!(
        tools.invoke_json(r#"{"tool":"delete"}"#).await.err(),
        Some(ToolError::BudgetExceeded)
    );
    assert_eq!(count, fixture.requests.lock().unwrap().len());
    let large = AiToolSession::new(
        api,
        ToolDestination::Local,
        PrivacyProfile::Standard,
        "fixture",
        false,
    );
    let mut total = 0;
    let request = r#"{"tool":"get_object","resource":{"group":"","version":"v1","resource":"pods"},"namespace":"default","name":"large"}"#;
    for _ in 0..4 {
        let result = large.invoke_json(request).await.unwrap();
        assert!(result.truncated);
        assert!(result.content.len() <= 64 * 1024);
        total += result.content.len();
        assert!(total <= 256 * 1024);
    }
    let count = fixture.requests.lock().unwrap().len();
    assert_eq!(
        large.invoke_json(request).await.err(),
        Some(ToolError::BudgetExceeded)
    );
    assert_eq!(count, fixture.requests.lock().unwrap().len());
    large.close();
    assert_eq!(
        large.sanitize_text("question").err(),
        Some(ToolError::Closed)
    );
}

#[tokio::test]
async fn discovery_namespaces_redaction_and_encoded_selectors_use_real_https() {
    let fixture = Fixture::start().await;
    let session = fixture.session(true);
    let api = ResourceApi::new(&session);
    assert_eq!(api.server_version().await.unwrap(), "v1.32.0");
    let discovery = api.discover().await.unwrap();
    assert!(discovery.resources.iter().any(|r| r.resource == "widgets"));
    assert!(!discovery.resources.iter().any(|r| r.resource.contains('/')));
    assert_eq!(discovery.unavailable_groups, vec!["offline.test/v1"]);
    assert_eq!(api.namespaces().await.unwrap(), vec!["default"]);
    let kind = api.resolve("", "v1", "secrets").unwrap();
    let page = api
        .list(
            &kind,
            &ListQuery {
                namespace: Some("default".into()),
                label_selector: Some("a=x&watch=true".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let serialized = serde_json::to_string(&page).unwrap();
    for key in [
        "fixture-password",
        "fixture-token",
        "fixture-last",
        "fixture-annotation",
        "fixture-managed",
    ] {
        assert!(!serialized.contains(key));
    }
    assert!(fixture
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|path| path.contains("labelSelector=a%3Dx%26watch%3Dtrue")));
    let detail = api
        .get(
            &ResourceRef {
                api: kind,
                namespace: Some("default".into()),
                name: "sample".into(),
            },
            ReadOptions::default(),
        )
        .await
        .unwrap();
    assert!(detail.yaml.contains("[redacted]"));
    assert!(!detail.yaml.contains("fixture-password"));
}

#[tokio::test]
async fn related_objects_follow_configuration_refs_without_exposing_secret_payloads() {
    let fixture = Fixture::start().await;
    let session = fixture.session(true);
    let api = ResourceApi::new(&session);
    api.discover().await.unwrap();
    let source = ResourceRef {
        api: api.resolve("", "v1", "secrets").unwrap(),
        namespace: Some("default".into()),
        name: "sample".into(),
    };
    let related = api.related_objects(&source).await.unwrap();
    assert!(
        related.partial,
        "unavailable list endpoints must remain explicit"
    );
    assert!(!related.unavailable_kinds.is_empty());
    let usage = related
        .items
        .iter()
        .find(|item| item.relation == ResourceRelation::UsesConfiguration)
        .unwrap();
    assert_eq!(usage.reference.name, "web");
    assert_eq!(usage.reference.api.resource, "pods");
    assert_eq!(usage.uid, "pod-id");
    let encoded = serde_json::to_string(&related).unwrap();
    for secret in [
        "fixture-password",
        "fixture-token",
        "fixture-last",
        "fixture-annotation",
    ] {
        assert!(!encoded.contains(secret));
    }
    let pod_ref = ResourceRef {
        api: api.resolve("", "v1", "pods").unwrap(),
        namespace: Some("default".into()),
        name: "web".into(),
    };
    let reverse = api.related_objects(&pod_ref).await.unwrap();
    assert!(reverse
        .items
        .iter()
        .any(|item| item.reference.name == "sample"
            && item.relation == ResourceRelation::Configuration));
    assert!(fixture
        .requests
        .lock()
        .unwrap()
        .iter()
        .all(|line| line.starts_with("GET ")));
}

#[tokio::test]
async fn forged_descriptor_traversal_and_production_mutations_never_reach_api() {
    let fixture = Fixture::start().await;
    let session = fixture.session(true);
    let api = ResourceApi::new(&session);
    api.discover().await.unwrap();
    let kind = api.resolve("", "v1", "pods").unwrap();
    let mut fake = kind.clone();
    fake.resource = "../secrets".into();
    let count = fixture.requests.lock().unwrap().len();
    assert!(api.list(&fake, &ListQuery::default()).await.is_err());
    let invalid = ResourceRef {
        api: kind.clone(),
        namespace: Some("default".into()),
        name: "../../secret".into(),
    };
    assert!(api.get(&invalid, ReadOptions::default()).await.is_err());
    let valid = ResourceRef {
        api: kind,
        namespace: Some("default".into()),
        name: "web".into(),
    };
    let manifest=Zeroizing::new("apiVersion: v1\nkind: Pod\nmetadata:\n  name: web\n  namespace: default\n  resourceVersion: '30'\n".to_owned());
    assert_eq!(
        api.apply(&valid, manifest, true, false).await.err(),
        Some(ResourceError::Transport)
    );
    assert!(api.delete(&valid, "pod-id", "30").await.is_err());
    let flux = ResourceRef {
        api: api
            .resolve("kustomize.toolkit.fluxcd.io", "v1", "kustomizations")
            .unwrap(),
        namespace: Some("default".into()),
        name: "web".into(),
    };
    assert!(api
        .flux_action(&flux, FluxAction::Reconcile, None)
        .await
        .is_err());
    let argo = ResourceRef {
        api: api
            .resolve("argoproj.io", "v1alpha1", "applications")
            .unwrap(),
        namespace: Some("default".into()),
        name: "web".into(),
    };
    assert!(api.argo_action(&argo, ArgoAction::Sync).await.is_err());
    let (_input_tx, input) = tokio::sync::mpsc::channel(2);
    let (output, _output_rx) = tokio::sync::mpsc::channel(2);
    assert!(api
        .terminal(
            PodTerminalRequest {
                namespace: "default".into(),
                pod: "web".into(),
                container: None,
                command: Some(vec!["/bin/sh".into()]),
                tty: true
            },
            input,
            output,
            CancellationToken::new()
        )
        .await
        .is_err());
    assert!(api.forward_listener("default", "web", 80, 0).await.is_err());
    assert_eq!(fixture.requests.lock().unwrap().len(), count);
    session.guard().open_write_window("fixture").unwrap();
    assert!(api
        .flux_action(&flux, FluxAction::Suspend, Some("maintenance"))
        .await
        .is_ok());
    assert_eq!(fixture.requests.lock().unwrap().len(), count + 1);
}

#[tokio::test]
async fn stale_watch_relists_then_resumes_and_stops_on_receiver_close() {
    let fixture = Fixture::start().await;
    let session = fixture.session(true);
    let api = ResourceApi::new(&session);
    api.discover().await.unwrap();
    let kind = api.resolve("", "v1", "pods").unwrap();
    let (sender, mut receiver) = tokio::sync::mpsc::channel(2);
    let stop = CancellationToken::new();
    let task = tokio::spawn(async move {
        api.watch(
            &kind,
            WatchOptions {
                query: ListQuery {
                    namespace: Some("default".into()),
                    ..Default::default()
                },
                resource_version: Some("1".into()),
            },
            sender,
            stop,
        )
        .await
    });
    assert!(matches!(
        receiver.recv().await,
        Some(ResourceChange::SnapshotBegin)
    ));
    assert!(matches!(
        receiver.recv().await,
        Some(ResourceChange::SnapshotPage { .. })
    ));
    assert!(matches!(
        receiver.recv().await,
        Some(ResourceChange::Modified { .. })
    ));
    assert!(matches!(
        receiver.recv().await,
        Some(ResourceChange::Bookmark { .. })
    ));
    drop(receiver);
    tokio::time::timeout(std::time::Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(fixture.watches.load(Ordering::SeqCst) >= 2);
}

#[tokio::test]
async fn malformed_large_and_error_bodies_are_sanitized_and_logs_are_bounded() {
    let fixture = Fixture::start().await;
    let session = fixture.session(true);
    let api = ResourceApi::new(&session);
    api.discover().await.unwrap();
    let kind = api.resolve("", "v1", "configmaps").unwrap();
    for (name, expected) in [
        ("malformed", ResourceError::InvalidResponse),
        ("huge", ResourceError::TooLarge),
        ("missing", ResourceError::NotFound),
    ] {
        let error = api
            .get(
                &ResourceRef {
                    api: kind.clone(),
                    namespace: Some("default".into()),
                    name: name.into(),
                },
                ReadOptions::default(),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error, expected);
        assert!(!error.to_string().contains("fixture"));
    }
    let logs = api
        .logs("default", "web", &LogQuery::default())
        .await
        .unwrap();
    assert!(logs.text.contains("first"));
    assert!(!logs.truncated);
    let count = fixture.requests.lock().unwrap().len();
    assert!(api
        .logs(
            "default",
            "web",
            &LogQuery {
                tail_lines: 20_001,
                ..Default::default()
            }
        )
        .await
        .is_err());
    assert_eq!(fixture.requests.lock().unwrap().len(), count);
    session.close();
    assert_eq!(
        api.server_version().await.unwrap_err(),
        ResourceError::Closed
    );
}

#[tokio::test]
async fn websocket_exec_round_trip_and_loopback_forward_stop_with_vault_scope() {
    let fixture = Fixture::start().await;
    let session = fixture.session(false);
    let api = ResourceApi::new(&session);
    api.discover().await.unwrap();
    let (input_tx, input) = tokio::sync::mpsc::channel(2);
    let (output, mut output_rx) = tokio::sync::mpsc::channel(2);
    let terminal_api = api.clone();
    let terminal = tokio::spawn(async move {
        terminal_api
            .terminal(
                PodTerminalRequest {
                    namespace: "default".into(),
                    pod: "web".into(),
                    container: None,
                    command: Some(vec!["/bin/sh".into()]),
                    tty: true,
                },
                input,
                output,
                CancellationToken::new(),
            )
            .await
    });
    input_tx
        .send(PodTerminalInput::Stdin(Zeroizing::new(
            b"fixture-input".to_vec(),
        )))
        .await
        .unwrap();
    let connected = tokio::time::timeout(std::time::Duration::from_secs(5), output_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(connected, PodTerminalOutput::Connected));
    let event = tokio::time::timeout(std::time::Duration::from_secs(5), output_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(event,PodTerminalOutput::Stdout(bytes) if bytes==b"fixture-input"));
    let event = tokio::time::timeout(std::time::Duration::from_secs(5), output_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(event, PodTerminalOutput::Exited { code: Some(0) }));
    terminal.await.unwrap().unwrap();

    let forward = api.forward_listener("default", "web", 80, 0).await.unwrap();
    let address = forward.address().unwrap();
    assert!(address.ip().is_loopback());
    let (updates, _updates_rx) = tokio::sync::mpsc::channel(2);
    let task = tokio::spawn(async move { forward.serve(updates).await });
    let mut local = tokio::net::TcpStream::connect(address).await.unwrap();
    local.write_all(b"fixture-forward").await.unwrap();
    let mut received = vec![0; 15];
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        local.read_exact(&mut received),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(received, b"fixture-forward");
    session.close();
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap(),
        Err(ResourceError::Closed)
    );
    let mut byte = [0];
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(2), local.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
}

#[tokio::test]
async fn overview_watch_has_a_lower_response_budget_than_full_resource_views() {
    let fixture = Fixture::start().await;
    let session = fixture.session(false);
    let api = ResourceApi::new(&session);
    api.discover().await.unwrap();
    let kind = api
        .resolve("source.toolkit.fluxcd.io", "v1", "gitrepositories")
        .unwrap();
    let (sender, _receiver) = tokio::sync::mpsc::channel(1);
    assert_eq!(
        api.watch_invalidation(
            &kind,
            Some("oversized".into()),
            sender,
            CancellationToken::new()
        )
        .await,
        Err(ResourceError::TooLarge)
    );
    let requests = fixture.requests.lock().unwrap();
    assert!(requests
        .iter()
        .any(|request| request.contains("/namespaces/oversized/") && request.contains("limit=25")));
    assert!(!requests.iter().any(
        |request| request.contains("/namespaces/oversized/") && request.contains("watch=true")
    ));
}

#[tokio::test]
async fn masked_resource_tables_do_not_request_or_expose_server_printer_cells() {
    let fixture = Fixture::start().await;
    let session = fixture.session(false);
    let api = ResourceApi::new(&session);
    api.discover().await.unwrap();
    for (group, version, resource) in [
        ("", "v1", "secrets"),
        ("helm.toolkit.fluxcd.io", "v2", "helmreleases"),
    ] {
        let kind = api.resolve(group, version, resource).unwrap();
        let table = api
            .table(
                &kind,
                &ListQuery {
                    namespace: Some("privacy".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(table.rows.len(), 1);
        assert!(!serde_json::to_string(&table)
            .unwrap()
            .contains("fixture-table-secret"));
    }
    assert!(fixture
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|line| line.contains("/namespaces/privacy/"))
        .all(|line| !line.contains("includeObject=")));
}

#[tokio::test]
async fn network_forward_requires_explicit_desktop_non_production_approval() {
    let fixture = Fixture::start().await;
    let session = fixture.session(false);
    let api = ResourceApi::new(&session);
    let network: SocketAddr = "0.0.0.0:0".parse().unwrap();
    assert!(matches!(
        api.forward_listener_on("default", "web", 80, network, false)
            .await,
        Err(ResourceError::Transport)
    ));
    let production = fixture.session(true);
    production.guard().open_write_window("fixture").unwrap();
    assert!(matches!(
        ResourceApi::new(&production)
            .forward_listener_on("default", "web", 80, network, true)
            .await,
        Err(ResourceError::Transport)
    ));
    for invalid in ["224.0.0.1:0", "255.255.255.255:0", "[ff02::1]:0"] {
        assert!(matches!(
            api.forward_listener_on("default", "web", 80, invalid.parse().unwrap(), true)
                .await,
            Err(ResourceError::InvalidRequest)
        ));
    }
    // Policy denial precedes discovery, Kubernetes requests, and binding.
    assert!(fixture.requests.lock().unwrap().is_empty());
    api.discover().await.unwrap();
    let forward = api
        .forward_listener_on("default", "web", 80, network, true)
        .await
        .unwrap();
    let address = forward.address().unwrap();
    assert!(address.ip().is_unspecified());
    let loopback = SocketAddr::from(([127, 0, 0, 1], address.port()));
    let (updates, _updates_rx) = tokio::sync::mpsc::channel(2);
    let task = tokio::spawn(async move { forward.serve(updates).await });
    let mut local = tokio::net::TcpStream::connect(loopback).await.unwrap();
    local.write_all(b"approved-forward").await.unwrap();
    let mut received = [0; 16];
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        local.read_exact(&mut received),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(&received, b"approved-forward");
    session.close();
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap(),
        Err(ResourceError::Closed)
    );
    assert!(tokio::net::TcpStream::connect(loopback).await.is_err());
}

#[tokio::test]
async fn crd_columns_access_review_and_advanced_mutations_keep_native_guard() {
    let fixture = Fixture::start().await;
    let session = fixture.session(true);
    let api = ResourceApi::new(&session);
    api.discover().await.unwrap();
    let table = api
        .table(
            &api.resolve("example.test", "v1", "widgets").unwrap(),
            &ListQuery {
                namespace: Some("default".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(table.columns[1].name, "Phase");
    assert_eq!(table.rows[0].cells[1], "Active");
    let access = api.check_permissions("default").await.unwrap();
    assert!(!access.incomplete);
    assert!(!access.can_mutate);
    let deployment = ResourceRef {
        api: api.resolve("apps", "v1", "deployments").unwrap(),
        namespace: Some("default".into()),
        name: "web".into(),
    };
    let pod = ResourceRef {
        api: api.resolve("", "v1", "pods").unwrap(),
        namespace: Some("default".into()),
        name: "web".into(),
    };
    assert!(api.scale(&deployment, "30", 2).await.is_err());
    assert!(api.restart(&deployment, "30").await.is_err());
    assert!(api.cordon("node", "30", true).await.is_err());
    assert!(api.evict_pod(&pod, "pod-id", "30").await.is_err());
    assert!(api
        .debug_container(
            &pod,
            "30",
            DebugContainer {
                name: "debug".into(),
                image: "busybox:1.36".into(),
                target_container: Some("web".into()),
                command: vec!["/bin/sh".into()]
            }
        )
        .await
        .is_err());
    assert!(!fixture
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|request| request.starts_with("PATCH ") || request.contains("/eviction")));
    session.guard().open_write_window("fixture").unwrap();
    api.scale(&deployment, "30", 2).await.unwrap();
    api.restart(&deployment, "30").await.unwrap();
    api.cordon("node", "30", true).await.unwrap();
    api.evict_pod(&pod, "pod-id", "30").await.unwrap();
    api.debug_container(
        &pod,
        "30",
        DebugContainer {
            name: "debug".into(),
            image: "busybox:1.36".into(),
            target_container: Some("web".into()),
            command: vec!["/bin/sh".into()],
        },
    )
    .await
    .unwrap();
    let requests = fixture.requests.lock().unwrap();
    assert!(requests.iter().any(|request| request.contains("/scale ")));
    assert!(requests
        .iter()
        .any(|request| request.contains("/ephemeralcontainers ")));
    assert!(requests
        .iter()
        .any(|request| request.contains("/eviction ")));
}

#[tokio::test]
async fn ai_tools_never_expose_secrets_and_question_budget_prevents_network_calls() {
    use cc_ai_core::PrivacyProfile;
    let fixture = Fixture::start().await;
    let session = fixture.session(true);
    let api = ResourceApi::new(&session);
    api.discover().await.unwrap();
    let tools = AiToolSession::new(
        api,
        ToolDestination::ApprovedExternal,
        PrivacyProfile::Strict,
        "private-cluster",
        false,
    );
    let result = tools
        .invoke(ToolRequest::GetObject {
            resource: ResourceKey {
                group: "".into(),
                version: "v1".into(),
                resource: "secrets".into(),
            },
            namespace: Some("default".into()),
            name: "sample".into(),
        })
        .await
        .unwrap();
    for private in [
        "fixture-password",
        "fixture-token",
        "fixture-annotation",
        "default",
        "sample",
    ] {
        assert!(!result.content.as_str().contains(private));
    }
    let result = tools
        .invoke(ToolRequest::LogsTail {
            namespace: "default".into(),
            pod: "web".into(),
            container: None,
            lines: 100,
            previous: false,
            since_seconds: None,
        })
        .await
        .unwrap();
    assert!(!result.content.as_str().contains("fixture-log-password"));
    assert!(result.content.as_str().contains("first"));
    for _ in 2..12 {
        tools
            .invoke(ToolRequest::ListResources {
                resource: ResourceKey {
                    group: "".into(),
                    version: "v1".into(),
                    resource: "pods".into(),
                },
                namespace: Some("default".into()),
                selector: None,
                max_rows: 1,
            })
            .await
            .unwrap();
    }
    let count = fixture.requests.lock().unwrap().len();
    let extra = tools
        .invoke(ToolRequest::ListResources {
            resource: ResourceKey {
                group: "".into(),
                version: "v1".into(),
                resource: "pods".into(),
            },
            namespace: Some("default".into()),
            selector: None,
            max_rows: 1,
        })
        .await;
    assert_eq!(extra.err(), Some(ToolError::BudgetExceeded));
    assert_eq!(fixture.requests.lock().unwrap().len(), count);
    tools.close();
    assert!(tools.rehydrate("<HOST_1>").is_err());
}

#[tokio::test]
async fn selected_problem_context_includes_bounded_sanitized_events_relations_and_log_tail() {
    use cc_ai_core::PrivacyProfile;
    let fixture = Fixture::start().await;
    let session = fixture.session(true);
    let api = ResourceApi::new(&session);
    api.discover().await.unwrap();
    fixture.requests.lock().unwrap().clear();
    let tools = AiToolSession::new(
        api,
        ToolDestination::ApprovedExternal,
        PrivacyProfile::Strict,
        "private-cluster",
        false,
    );
    let result = tools
        .selected_object_context(
            ResourceKey {
                group: "".into(),
                version: "v1".into(),
                resource: "pods".into(),
            },
            Some("default".into()),
            "web".into(),
        )
        .await
        .unwrap();
    assert_eq!(result.calls_remaining, 11);
    assert!(result.content.len() <= 64 * 1024);
    let content = result.content.as_str();
    for included in [
        "\"object\"",
        "\"events\"",
        "\"related\"",
        "\"logs\"",
        "BackOff",
        "first",
        "\"partial\":true",
    ] {
        assert!(
            content.contains(included),
            "missing projected context field"
        );
    }
    for excluded in [
        "fixture-password",
        "fixture-token",
        "fixture-event-password",
        "fixture-log-password",
        "\"web\"",
        "\"default\"",
        "\"sample\"",
    ] {
        assert!(!content.contains(excluded), "private data leaked");
    }
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.iter().filter(|request| request.as_str() == "GET /api/v1/namespaces/default/pods/web HTTP/1.1").count(), 1);
    assert!(requests
        .iter()
        .any(|request| request.contains("involvedObject.uid%3Dpod-id")));
    assert!(requests
        .iter()
        .any(|request| request.contains("tailLines=100")));
    assert!(
        requests.len() <= 23,
        "bounded context must not crawl the cluster"
    );
    assert!(requests.iter().all(|request| request.starts_with("GET ")));
}

#[tokio::test]
async fn ai_events_merge_opted_in_history_filter_scope_time_and_deduplicate_uid() {
    use cc_ai_core::PrivacyProfile;
    let fixture = Fixture::start().await;
    let session = fixture.session(false);
    let api = ResourceApi::new(&session);
    api.discover().await.unwrap();
    let now = chrono::Utc::now();
    let event = |uid: &str, namespace: &str, age: i64, reason: &str| {
        json!({
            "apiVersion":"v1","kind":"Event","metadata":{"uid":uid,"name":uid,"namespace":namespace},
            "involvedObject":{"uid":"pod-id","kind":"Pod","name":"web"},
            "reason":reason,"message":"password=fixture-historical-secret",
            "lastTimestamp":(now-chrono::Duration::seconds(age)).to_rfc3339(),
        })
    };
    let tools = AiToolSession::new(
        api,
        ToolDestination::ApprovedExternal,
        PrivacyProfile::Strict,
        "private-cluster",
        false,
    )
    .with_event_history(vec![
        event("historical-id", "default", 600, "HistoricalWarning"),
        event("event-id", "default", 600, "OutdatedDuplicate"),
        event("foreign-id", "another-team", 10, "ForeignNamespace"),
        event("old-id", "default", 7200, "OutsideTimeWindow"),
    ])
    .unwrap();
    let result = tools
        .invoke_json(
            r#"{"tool":"events","namespace":"default","object_uid":"pod-id","since_seconds":3600}"#,
        )
        .await
        .unwrap();
    let value: Value = serde_json::from_str(result.content.as_str()).unwrap();
    assert_eq!(value["items"].as_array().unwrap().len(), 2);
    assert_eq!(value["history_included"], true);
    assert!(result.content.as_str().contains("HistoricalWarning"));
    assert!(result.content.as_str().contains("BackOff"));
    for excluded in [
        "OutdatedDuplicate",
        "ForeignNamespace",
        "OutsideTimeWindow",
        "fixture-historical-secret",
        "fixture-event-password",
    ] {
        assert!(!result.content.as_str().contains(excluded));
    }
    let requests = fixture.requests.lock().unwrap().len();
    tools.close();
    assert_eq!(
        tools.invoke_json(r#"{"tool":"events"}"#).await.err(),
        Some(ToolError::Closed)
    );
    assert_eq!(fixture.requests.lock().unwrap().len(), requests);
}

#[tokio::test]
async fn reconcile_with_source_validates_full_chain_and_native_write_policy() {
    let fixture = Fixture::start().await;
    let session = fixture.session(true);
    let api = ResourceApi::new(&session);
    api.discover().await.unwrap();
    let reference = ResourceRef {
        api: api
            .resolve("helm.toolkit.fluxcd.io", "v2", "helmreleases")
            .unwrap(),
        namespace: Some("default".into()),
        name: "demo".into(),
    };
    assert!(api
        .flux_action(&reference, FluxAction::ReconcileWithSource, None)
        .await
        .is_err());
    assert!(!fixture
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|line| line.starts_with("PATCH ")));
    session.guard().open_write_window("fixture").unwrap();
    let cycle = ResourceRef {
        name: "cycle".into(),
        ..reference.clone()
    };
    assert_eq!(
        api.flux_action(&cycle, FluxAction::ReconcileWithSource, None)
            .await
            .err(),
        Some(ResourceError::InvalidRequest)
    );
    assert!(!fixture
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|line| line.starts_with("PATCH ")));
    api.flux_action(&reference, FluxAction::ReconcileWithSource, None)
        .await
        .unwrap();
    let writes = fixture
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|line| line.starts_with("PATCH "))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(writes.len(), 3);
    assert!(writes[0].contains("/gitrepositories/git "));
    assert!(writes[1].contains("/helmcharts/chart "));
    assert!(writes[2].contains("/helmreleases/demo "));
}
