use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use cc_models::{
    kubernetes::{KubeAuth, KubeCluster, KubeContext},
    secret::{Secret, SecretKind, SecretValue},
};
use serde_json::{json, Value};
use std::{
    os::unix::fs::PermissionsExt,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
struct Fixture {
    address: std::net::SocketAddr,
    certificate: String,
    revision: Arc<AtomicU64>,
    writes: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn secret(revision: u64, resource_version: u64) -> Value {
    let release = json!({"name":"fixture","namespace":"default","version":revision,"info":{"status":if revision==2{"deployed"}else{"superseded"}},"config":{},"chart":{"metadata":{"name":"fixture","version":"1.0.0"},"values":{}},"manifest":""});
    json!({"apiVersion":"v1","kind":"Secret","type":"helm.sh/release.v1","metadata":{"name":format!("sh.helm.release.v1.fixture.v{revision}"),"namespace":"default","uid":format!("uid-{revision}"),"resourceVersion":resource_version.to_string(),"labels":{"owner":"helm","name":"fixture"}},"data":{"release":STANDARD.encode(STANDARD.encode(serde_json::to_vec(&release).unwrap()))}})
}
impl Fixture {
    async fn start() -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let cert = rcgen::generate_simple_self_signed(vec!["helm.fixture.test".into()]).unwrap();
        let certificate = cert.cert.pem();
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.cert.der().clone()],
                rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der())
                    .into(),
            )
            .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let revision = Arc::new(AtomicU64::new(1));
        let rev = revision.clone();
        let writes = Arc::new(AtomicUsize::new(0));
        let count = writes.clone();
        let task = tokio::spawn(async move {
            let mut children = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    Some(_)=children.join_next(),if !children.is_empty()=>{},
                    result=listener.accept()=>{let Ok((socket,_))=result else{break;};let acceptor=acceptor.clone();let revision=rev.clone();let writes=count.clone();children.spawn(async move{
                        let mut socket=acceptor.accept(socket).await.unwrap();let mut header=Vec::new();let mut byte=[0];while header.len()<32768&&!header.ends_with(b"\r\n\r\n"){if socket.read_exact(&mut byte).await.is_err(){return;}header.push(byte[0]);}
                        let header=String::from_utf8(header).unwrap();assert!(header.to_lowercase().contains("authorization: bearer synthetic-helm-token"));let first=header.lines().next().unwrap();let path=first.split(' ').nth(1).unwrap().split('?').next().unwrap();
                        let len=header.lines().filter_map(|line|line.split_once(':')).find(|(key,_)|key.eq_ignore_ascii_case("content-length")).and_then(|(_,v)|v.trim().parse::<usize>().ok()).unwrap_or(0);let mut body=vec![0;len.min(1024*1024)];if socket.read_exact(&mut body).await.is_err(){return;}
                        let body=if first.starts_with("PUT "){writes.fetch_add(1,Ordering::SeqCst);json!({"apiVersion":"v1","kind":"ConfigMap","metadata":{"name":"fixture","namespace":"default","uid":"cm-uid","resourceVersion":"1"}})}
                        else{match path{"/version"=>json!({"gitVersion":"v1.32.0"}),"/apis"=>json!({"groups":[]}),"/api/v1"=>json!({"resources":[{"name":"secrets","kind":"Secret","namespaced":true,"verbs":["get","list"]},{"name":"configmaps","kind":"ConfigMap","namespaced":true,"verbs":["get","list","update"]}]}),"/api/v1/namespaces/default/secrets"=>json!({"metadata":{"resourceVersion":"1"},"items":[secret(1,revision.load(Ordering::SeqCst)),secret(2,1)]}),p if p.ends_with(".v1")=>secret(1,revision.load(Ordering::SeqCst)),p if p.ends_with(".v2")=>secret(2,1),_=>json!({"items":[]})}}.to_string();
                        let reply=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());let _=socket.write_all(reply.as_bytes()).await;let _=socket.shutdown().await;
                    });}
                }
            }
        });
        Self {
            address,
            certificate,
            revision,
            writes,
            task,
        }
    }
    async fn app(&self) -> (tempfile::TempDir, AppCore, KubeContext) {
        let dir = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            dir.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile("helm".into(), "synthetic-helm-passphrase".into())
            .await
            .unwrap();
        let mut cluster = KubeCluster::new("fixture", format!("https://{}", self.address));
        cluster.tls_server_name = Some("helm.fixture.test".into());
        cluster.certificate_authority_pem = Some(self.certificate.clone());
        let secret = Secret::new(SecretKind::Token, SecretValue::new("synthetic-helm-token"));
        let mut context = KubeContext::new("production helm", cluster.id);
        context.production = true;
        context.auth = KubeAuth::Token {
            secret_id: secret.id,
        };
        app.import_kube_connection(cluster, context.clone(), vec![secret])
            .await
            .unwrap();
        (dir, app, context)
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rollback_requires_confirmation_unchanged_history_binary_and_guarded_proxy() {
    let fixture = Fixture::start().await;
    let (dir, app, context) = fixture.app().await;
    let session = app.kube_connect(context.id.to_string()).await.unwrap();
    app.kube_discover(session.id.clone()).await.unwrap();
    let binary = dir.path().join("helm");
    let marker = dir.path().join("executed");
    let program = format!(
        r#"#!/usr/bin/env python3
import sys,json,base64,ssl,urllib.request,pathlib
if sys.argv[1]=='version':
 print('v3.17.0');sys.exit(0)
pathlib.Path({marker:?}).write_text('executed')
p=sys.argv[sys.argv.index('--kubeconfig')+1]
c=json.loads(pathlib.Path(p).read_text());assert 'synthetic-helm-token' not in str(c)
s=c['clusters'][0]['cluster'];u=c['users'][0]['user'];assert s['server'].startswith('https://127.0.0.1:')
ctx=ssl.create_default_context(cadata=base64.b64decode(s['certificate-authority-data']).decode())
r=urllib.request.Request(s['server']+'/api/v1/namespaces/default/configmaps/fixture',data=b'{{}}',headers={{'Authorization':'Bearer '+u['token'],'Content-Type':'application/json'}},method='PUT')
with urllib.request.urlopen(r,context=ctx,timeout=10) as response:assert response.status==200
print('rollback complete')
"#,
        marker = marker.to_string_lossy()
    );
    std::fs::write(&binary, &program).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let review = app
        .kube_helm_rollback_review(
            session.id.clone(),
            "default".into(),
            "fixture".into(),
            1,
            Some(binary.to_string_lossy().into_owned()),
        )
        .await
        .unwrap();
    assert!(!marker.exists());
    assert!(review.arguments.contains(&"--no-hooks".into()));
    assert!(app
        .kube_helm_rollback_approve(review.ticket.clone())
        .await
        .unwrap_err()
        .to_string()
        .contains("read_only"));
    assert!(!marker.exists());
    assert_eq!(fixture.writes.load(Ordering::SeqCst), 0);
    app.kube_open_write_window(session.id.clone(), context.name)
        .await
        .unwrap();
    let review = app
        .kube_helm_rollback_review(
            session.id.clone(),
            "default".into(),
            "fixture".into(),
            1,
            Some(binary.to_string_lossy().into_owned()),
        )
        .await
        .unwrap();
    fixture.revision.store(2, Ordering::SeqCst);
    assert!(app
        .kube_helm_rollback_approve(review.ticket)
        .await
        .unwrap_err()
        .to_string()
        .contains("helm_history_changed"));
    assert!(!marker.exists());
    let review = app
        .kube_helm_rollback_review(
            session.id.clone(),
            "default".into(),
            "fixture".into(),
            1,
            Some(binary.to_string_lossy().into_owned()),
        )
        .await
        .unwrap();
    std::fs::write(&binary, format!("{program}\n# changed")).unwrap();
    assert!(app
        .kube_helm_rollback_approve(review.ticket)
        .await
        .unwrap_err()
        .to_string()
        .contains("cli_changed"));
    assert!(!marker.exists());
    std::fs::write(&binary, &program).unwrap();
    let review = app
        .kube_helm_rollback_review(
            session.id,
            "default".into(),
            "fixture".into(),
            1,
            Some(binary.to_string_lossy().into_owned()),
        )
        .await
        .unwrap();
    let output = app
        .kube_helm_rollback_approve(review.ticket.clone())
        .await
        .unwrap();
    let output: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(output["exit_code"], 0, "{output}");
    assert_eq!(fixture.writes.load(Ordering::SeqCst), 1);
    assert_eq!(
        app.kube_audit().await.unwrap().last().unwrap().method,
        "PUT"
    );
    assert!(app.kube_helm_rollback_approve(review.ticket).await.is_err());
}
