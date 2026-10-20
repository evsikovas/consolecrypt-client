use super::*;
use cc_models::{
    kubernetes::{KubeAuth, KubeCluster, KubeContext},
    secret::{Secret, SecretKind, SecretValue},
};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
struct TaskGuard(tokio::task::AbortHandle);
impl Drop for TaskGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn https_workload_selection_is_bounded_and_export_is_private_masked_and_revoked_on_lock() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let certificate = rcgen::generate_simple_self_signed(vec!["logs.fixture.test".into()]).unwrap();
    let ca = certificate.cert.pem();
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate.cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(certificate.signing_key.serialize_der())
                .into(),
        )
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = requests.clone();
    let task = tokio::spawn(async move {
        let mut connections = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                Some(_)=connections.join_next(),if !connections.is_empty()=>{},
                connection=listener.accept()=>{
                    let (socket,_)=connection.unwrap();
                    let acceptor=acceptor.clone();
                    let seen=seen.clone();
                    connections.spawn(async move {
                        let mut socket=acceptor.accept(socket).await.unwrap();
                        let mut headers=Vec::new();
                        while headers.len()<32768 && !headers.ends_with(b"\r\n\r\n") {
                            let mut byte=[0]; if socket.read_exact(&mut byte).await.is_err(){return;} headers.push(byte[0]);
                        }
                        let headers=String::from_utf8(headers).unwrap();
                        assert!(headers.to_ascii_lowercase().contains("authorization: bearer synthetic-logs-token"));
                        let path=headers.lines().next().unwrap().split(' ').nth(1).unwrap();
                        seen.lock().unwrap().push(path.into());
                        let endpoint=path.split('?').next().unwrap();
                        let resource=|name:&str,kind:&str| json!({"name":name,"kind":kind,"namespaced":true,"verbs":["get","list"]});
                        let body=match endpoint {
                            "/version"=>json!({"gitVersion":"v1.32.0"}),
                            "/apis"=>json!({"groups":[{"name":"apps","versions":[{"version":"v1"}]}]}),
                            "/api/v1"=>json!({"resources":[resource("pods","Pod"),resource("pods/log","Pod")]}),
                            "/apis/apps/v1"=>json!({"resources":[resource("deployments","Deployment")]}),
                            "/apis/apps/v1/namespaces/default/deployments/api"=>json!({
                                "apiVersion":"apps/v1","kind":"Deployment","metadata":{"name":"api","namespace":"default","uid":"deployment-id","resourceVersion":"1"},
                                "spec":{"selector":{"matchLabels":{"app":"api"},"matchExpressions":[{"key":"tier","operator":"In","values":["backend"]}]}}
                            }),
                            "/api/v1/namespaces/default/pods"=>json!({"metadata":{"resourceVersion":"1","continue":"next-page"},"items":(0..9).map(|i|json!({
                                "apiVersion":"v1","kind":"Pod","metadata":{"name":format!("api-{i}"),"namespace":"default","uid":format!("pod-{i}"),"resourceVersion":"1"},
                                "spec":{"containers":[{"name":"main"}],"initContainers":[{"name":"init"}],"ephemeralContainers":[{"name":"debug"}]}
                            })).collect::<Vec<_>>()}),
                            _=>json!({"items":[]}),
                        }.to_string();
                        let response=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                        socket.write_all(response.as_bytes()).await.unwrap();
                    });
                }
            }
        }
    });
    let cancel_task = task.abort_handle();
    let _guard = TaskGuard(cancel_task);
    let temp = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        temp.path().join("app").to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("logs".into(), "logs-test-passphrase".into())
        .await
        .unwrap();
    let mut cluster = KubeCluster::new("logs", format!("https://{address}"));
    cluster.tls_server_name = Some("logs.fixture.test".into());
    cluster.certificate_authority_pem = Some(ca);
    let secret = Secret::new(SecretKind::Token, SecretValue::new("synthetic-logs-token"));
    let mut context = KubeContext::new("logs", cluster.id);
    context.auth = KubeAuth::Token {
        secret_id: secret.id,
    };
    app.import_kube_connection(cluster, context.clone(), vec![secret])
        .await
        .unwrap();
    let session = app.kube_connect(context.id.to_string()).await.unwrap();
    let kinds = app.kube_discover(session.id.clone()).await.unwrap();
    let deployment = kinds
        .resources
        .into_iter()
        .find(|kind| kind.resource == "deployments")
        .unwrap();
    let targets = app
        .kube_log_targets(
            session.id.clone(),
            ResourceRef {
                api: deployment,
                namespace: Some("default".into()),
                name: "api".into(),
            },
        )
        .await
        .unwrap();
    assert!(targets.truncated);
    assert_eq!(targets.targets.len(), 8);
    assert_eq!(targets.targets[0].containers, ["main", "init", "debug"]);
    let paths = requests.lock().unwrap().clone();
    let requested = paths
        .iter()
        .find(|path| path.starts_with("/api/v1/namespaces/default/pods?"))
        .unwrap();
    let url = url::Url::parse(&format!("https://fixture{requested}")).unwrap();
    let query = url
        .query_pairs()
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(
        query.get("labelSelector").unwrap(),
        "app=api,tier in (backend)"
    );
    assert_eq!(query.get("limit").unwrap(), "9");
    let output = temp.path().join("masked.log");
    app.kube_export_logs(
        session.id.clone(),
        output.to_string_lossy().into_owned(),
        Zeroizing::new(
            b"INFO ready\nAuthorization: Bearer synthetic-super-secret-value\n".to_vec(),
        ),
        true,
    )
    .await
    .unwrap();
    let text = std::fs::read_to_string(&output).unwrap();
    assert!(text.contains("INFO ready"));
    assert!(!text.contains("synthetic-super-secret-value"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&output).unwrap().permissions().mode() & 0o077,
            0
        );
    }
    assert!(app
        .kube_export_logs(
            session.id.clone(),
            output.to_string_lossy().into_owned(),
            Zeroizing::new(b"overwrite".to_vec()),
            false
        )
        .await
        .is_err());
    assert_eq!(std::fs::read_to_string(&output).unwrap(), text);
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let proxy_path = {
        let profiles = app.kube_context_terminal_profiles().await.unwrap();
        assert!(!profiles.wsl_supported);
        assert!(!profiles
            .profiles
            .iter()
            .any(|profile| profile.id.starts_with("wsl:")));
        let profile = &profiles.profiles[0];
        let terminal = app
            .kube_context_terminal(
                session.id.clone(),
                "synthetic-namespace".into(),
                profile.id.clone(),
            )
            .await
            .unwrap();
        assert!(app
            .terminal_info(terminal.clone())
            .await
            .unwrap()
            .title
            .contains("synthetic-namespace"));
        let marker = temp.path().join("context-terminal.json");
        let code = format!(
            "import os,json,pathlib; p=os.environ[\"KUBECONFIG\"]; c=json.loads(pathlib.Path(p).read_text()); assert c[\"contexts\"][0][\"context\"][\"namespace\"]==\"synthetic-namespace\"; assert \"synthetic-logs-token\" not in str(c); assert c[\"clusters\"][0][\"cluster\"][\"server\"].startswith(\"https://127.0.0.1:\"); pathlib.Path({:?}).write_text(p)",
            marker.to_string_lossy()
        );
        let command = format!("/usr/bin/python3 -c '{}'\n", code.replace('\'', "'\\''"));
        app.terminal_write(terminal, command.into_bytes())
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !marker.exists() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        PathBuf::from(std::fs::read_to_string(marker).unwrap())
    };
    app.lock().await.unwrap();
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    assert!(
        !proxy_path.exists(),
        "context terminal lease is removed on lock"
    );
    let blocked = temp.path().join("after-lock.log");
    assert!(app
        .kube_export_logs(
            session.id,
            blocked.to_string_lossy().into_owned(),
            Zeroizing::new(b"secret".to_vec()),
            false
        )
        .await
        .is_err());
    assert!(!blocked.exists());
}
