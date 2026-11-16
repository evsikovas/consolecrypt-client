//! Native monitoring auth boundary: synthetic credentials and own loopback TLS.
//! No Cargo invocation is performed by the author while root's build is active.
mod common;

use base64::{engine::general_purpose::STANDARD, Engine};
use cc_monitoring_core::{AuthRef, Route, SourceConfig, TlsConfig};
use common::{app, PASSPHRASE};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Notify,
};
use uuid::Uuid;

#[derive(Default)]
struct Gate {
    entered: Notify,
    release_first: Notify,
    requests: AtomicUsize,
    rejected_tls: AtomicUsize,
    authorization: Mutex<Vec<String>>,
}

struct Fixture {
    endpoint: String,
    ca_pem: String,
    gate: Arc<Gate>,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn fixture(block_first: bool) -> Fixture {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let ca_pem = cert.cert.pem();
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
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "https://localhost:{}",
        listener.local_addr().unwrap().port()
    );
    let gate = Arc::new(Gate::default());
    let observed = gate.clone();
    let server = tokio::spawn(async move {
        let mut connections = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                accepted=listener.accept() => {
                    let Ok((socket,_))=accepted else {break;};
                    let acceptor=acceptor.clone();
                    let gate=observed.clone();
                    connections.spawn(async move {
                        let mut stream=match acceptor.accept(socket).await {
                            Ok(stream)=>stream,
                            Err(_)=>{gate.rejected_tls.fetch_add(1,Ordering::SeqCst);return;}
                        };
                        let mut bytes=Vec::new();
                        let header_end=loop {
                            let mut chunk=[0;2048];
                            let Ok(n)=stream.read(&mut chunk).await else {return;};
                            if n==0 {return;}
                            bytes.extend_from_slice(&chunk[..n]);
                            assert!(bytes.len()<=32*1024,"fixture request exceeded its own bound");
                            if let Some(end)=bytes.windows(4).position(|w|w==b"\r\n\r\n") {break end;}
                        };
                        let headers=String::from_utf8(bytes[..header_end].to_vec()).unwrap();
                        let authorization=headers.lines().find_map(|line| {
                            let (name,value)=line.split_once(':')?;
                            name.eq_ignore_ascii_case("authorization").then(||value.trim().to_owned())
                        }).unwrap_or_default();
                        let content_length=headers.lines().find_map(|line| {
                            let (name,value)=line.split_once(':')?;
                            if name.eq_ignore_ascii_case("content-length") {value.trim().parse::<usize>().ok()}else{None}
                        }).unwrap_or(0);
                        assert!(content_length<=16*1024);
                        while bytes.len()<header_end+4+content_length {
                            let mut chunk=[0;2048];
                            let Ok(n)=stream.read(&mut chunk).await else {return;};
                            if n==0 {return;}
                            bytes.extend_from_slice(&chunk[..n]);
                            assert!(bytes.len()<=32*1024);
                        }
                        gate.authorization.lock().unwrap().push(authorization.clone());
                        let index=gate.requests.fetch_add(1,Ordering::SeqCst);
                        gate.entered.notify_one();
                        if block_first&&index==0 {gate.release_first.notified().await;}
                        let encoded=authorization.strip_prefix("Basic ").unwrap_or("");
                        let decoded=String::from_utf8(STANDARD.decode(encoded).unwrap()).unwrap();
                        let (_,password)=decoded.split_once(':').unwrap();
                        // An untrusted endpoint tries to echo all material forms.
                        let body=json!({"status":"success","data":{"resultType":"vector","result":[{"metric":{"job":"own-tls-fixture"},"value":[1,"1"]}]},"warnings":[format!("echo {password}"),format!("echo {encoded}"),format!("echo {authorization}")] }).to_string();
                        let response=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                        // A cancelled native request may already have closed TLS.
                        let _=stream.write_all(response.as_bytes()).await;
                        let _=stream.shutdown().await;
                    });
                }
                Some(completed)=connections.join_next(),if !connections.is_empty()=>{completed.unwrap();}
            }
        }
    });
    Fixture {
        endpoint,
        ca_pem,
        gate,
        server,
    }
}

fn source(fixture: &Fixture, credential_id: String, trusted: bool) -> SourceConfig {
    SourceConfig {
        id: String::new(),
        name: "Synthetic native Basic TLS fixture".into(),
        endpoint: fixture.endpoint.clone(),
        route: Route::Direct,
        tls: TlsConfig {
            custom_ca_pem: trusted.then(|| fixture.ca_pem.clone()),
        },
        auth: AuthRef::BasicRef { credential_id },
    }
}
fn request(id: &str) -> String {
    json!({"operationId":Uuid::new_v4(),"sourceId":id,"sourceRevision":1,"action":{"kind":"query","expression":"up","time":null}}).to_string()
}
fn assert_no_material(result: &Value, password: &str, username: &str) {
    let serialized = result.to_string();
    let encoded = STANDARD.encode(format!("{username}:{password}"));
    assert!(
        !serialized.contains(password),
        "password escaped the native boundary"
    );
    assert!(
        !serialized.contains(&encoded),
        "Basic payload escaped the native boundary"
    );
    assert!(serialized.contains("[REDACTED]"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn basic_credential_uses_native_username_and_secret_over_trusted_tls() {
    const USER: &str = "synthetic-native-user";
    const PASSWORD: &str = "synthetic-basic-password-7852";
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    app.create_local_profile("Own Basic monitoring".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let credential = app
        .add_password_credential("Own Basic".into(), Some(USER.into()), PASSWORD.into())
        .await
        .unwrap();
    let fixture = fixture(false).await;
    let catalog = app
        .monitoring_source_create(0, source(&fixture, credential.id, true))
        .await
        .unwrap();
    let result = app
        .monitoring_read(request(&catalog.sources[0].id))
        .await
        .unwrap();
    assert_eq!(result["result"]["status"], "success");
    assert_no_material(&result, PASSWORD, USER);
    let expected = format!("Basic {}", STANDARD.encode(format!("{USER}:{PASSWORD}")));
    assert!(fixture.gate.authorization.lock().unwrap()[0] == expected);
    assert_eq!(fixture.gate.requests.load(Ordering::SeqCst), 1);
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn untrusted_tls_never_sends_native_basic_authorization() {
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    app.create_local_profile("Own untrusted TLS".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let credential = app
        .add_password_credential(
            "Own Basic".into(),
            Some("fixture-user".into()),
            "synthetic-untrusted-password-2468".into(),
        )
        .await
        .unwrap();
    let fixture = fixture(false).await;
    let catalog = app
        .monitoring_source_create(0, source(&fixture, credential.id, false))
        .await
        .unwrap();
    let result = app
        .monitoring_read(request(&catalog.sources[0].id))
        .await
        .unwrap();
    assert_eq!(result["result"]["error"]["code"], "transport");
    assert_eq!(fixture.gate.requests.load(Ordering::SeqCst), 0);
    assert!(fixture.gate.authorization.lock().unwrap().is_empty());
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn credential_rotation_cancels_inflight_read_and_next_read_uses_new_secret() {
    const USER: &str = "synthetic-rotation-user";
    const OLD: &str = "synthetic-old-basic-password-13579";
    const NEW: &str = "synthetic-new-basic-password-97531";
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    app.create_local_profile("Own credential rotation".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let credential = app
        .add_password_credential("Own Basic".into(), Some(USER.into()), OLD.into())
        .await
        .unwrap();
    let fixture = fixture(true).await;
    let catalog = app
        .monitoring_source_create(0, source(&fixture, credential.id.clone(), true))
        .await
        .unwrap();
    let id = catalog.sources[0].id.clone();
    let reader = app.clone();
    let input = request(&id);
    let read = tokio::spawn(async move { reader.monitoring_read(input).await });
    tokio::time::timeout(Duration::from_secs(5), fixture.gate.entered.notified())
        .await
        .unwrap();
    app.set_credential_password(credential.id, NEW.into())
        .await
        .unwrap();
    // The old request must stop before the endpoint is allowed to return data.
    let old_result = tokio::time::timeout(Duration::from_secs(2), read)
        .await
        .expect("credential rotation did not cancel the in-flight monitoring read")
        .unwrap();
    assert!(old_result.is_err());
    fixture.gate.release_first.notify_one();
    let result = app.monitoring_read(request(&id)).await.unwrap();
    assert_eq!(result["result"]["status"], "success");
    assert_no_material(&result, NEW, USER);
    {
        let observed = fixture.gate.authorization.lock().unwrap();
        assert_eq!(observed.len(), 2);
        assert!(observed[0] == format!("Basic {}", STANDARD.encode(format!("{USER}:{OLD}"))));
        assert!(observed[1] == format!("Basic {}", STANDARD.encode(format!("{USER}:{NEW}"))));
    }
    app.shutdown().await.unwrap();
}
