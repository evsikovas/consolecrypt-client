//! No-secrets-in-logs invariant (CLIENT_ARCHITECTURE §5). Own test binary:
//! it installs a capturing tracing subscriber for a whole flow.

mod common;

use cc_models::secret::{Secret, SecretKind, SecretValue};
use cc_models::{ObjectPayload, VaultObject};
use cc_sync_core::mock::MockServer;
use common::*;
use std::sync::Arc;

fn p(obj: VaultObject) -> ObjectPayload {
    ObjectPayload::new(obj)
}

/// CLIENT_ARCHITECTURE §5: no secret in logs. Capture all tracing output of
/// a full flow and grep for every secret the flow handled.
#[tokio::test]
async fn logs_never_contain_secrets_or_tokens() {
    use std::io::Write;
    #[derive(Clone, Default)]
    struct Buf(Arc<std::sync::Mutex<Vec<u8>>>);
    impl Write for Buf {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let buf = Buf::default();
    let writer = buf.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let server = MockServer::start().await;
    let (a, b, _vault) = two_devices(&server).await;
    let secret_value = "TOP-SECRET-3f1c9a77";
    let s = Secret::new(SecretKind::Password, SecretValue::new(secret_value));
    a.engine
        .put(p(VaultObject::Secret(s.clone())))
        .await
        .unwrap();
    sync_ok(&a.engine).await;
    sync_ok(&b.engine).await;
    let mut sb = s.clone();
    sb.value = SecretValue::new("OTHER-SECRET-77aa01");
    let mut sa = s.clone();
    sa.value = SecretValue::new("THIRD-SECRET-9d2e11");
    a.engine.put(p(VaultObject::Secret(sa))).await.unwrap();
    b.engine.put(p(VaultObject::Secret(sb))).await.unwrap();
    sync_ok(&a.engine).await;
    server.expire_access_tokens();
    sync_ok(&b.engine).await; // refresh + conflict resolution
    let tokens = [
        a.api.refresh().await.unwrap(),
        b.api.refresh().await.unwrap(),
    ];

    let logs = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("api request"), "logging was captured");
    for needle in [
        secret_value,
        "OTHER-SECRET-77aa01",
        "THIRD-SECRET-9d2e11",
        PASSWORD,
    ] {
        assert!(!logs.contains(needle), "secret leaked into logs");
    }
    for t in &tokens {
        assert!(!logs.contains(t.access_token.expose_secret()));
        assert!(!logs.contains(t.refresh_token.expose_secret()));
    }
    assert!(
        !logs.to_lowercase().contains("bearer "),
        "no auth headers in logs"
    );
}

/// Regression (found by the Server Dev log-hygiene suite): tungstenite logs
/// the whole WebSocket handshake request, including `Authorization: Bearer`,
/// with `log::trace!`. Bridge `log` into tracing at TRACE, open the event
/// stream, and make sure the token never shows up.
#[tokio::test]
async fn websocket_handshake_never_logs_bearer_token_even_at_trace() {
    use cc_protocol::events::ServerEvent;
    use cc_protocol::version::Platform;
    use cc_sync_core::{ApiClient, ApiConfig, EventStream, EventStreamConfig, TokenStore, WsEvent};
    use std::io::Write;

    #[derive(Clone, Default)]
    struct Buf(Arc<std::sync::Mutex<Vec<u8>>>);
    impl Write for Buf {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    // Route `log` records (tungstenite uses `log`, not `tracing`) into tracing.
    let _ = tracing_log::LogTracer::init();
    let buf = Buf::default();
    let writer = buf.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let server = MockServer::start().await;
    let store = Arc::new(cc_sync_core::MemoryTokenStore::new());
    let api = ApiClient::new(
        ApiConfig::new(server.url().as_str(), "0.1.0-test", Platform::Cli).unwrap(),
        store.clone(),
    )
    .unwrap();
    let keys = DeviceKeys::generate();
    api.register(&cc_protocol::auth::RegisterRequest {
        email: "ws-hygiene@example.test".into(),
        password: cc_protocol::auth::SecretString::new(PASSWORD),
        device: keys.registration("ws device"),
        device_proof: Some(keys.proof()),
    })
    .await
    .unwrap();

    let stream = EventStream::spawn(api.clone(), EventStreamConfig::default());
    let mut rx = stream.subscribe();
    let hello = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if let Ok(WsEvent::Event(ServerEvent::Hello { .. })) = rx.recv().await {
                return;
            }
        }
    })
    .await;
    assert!(hello.is_ok(), "websocket connected and received hello");
    stream.shutdown().await;

    // The bridge really captures `log` records (the test is not vacuous) …
    log::info!(target: "tungstenite::probe", "log-bridge-probe");
    let tokens = store.load().await.unwrap().expect("signed in");
    let logs = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("log-bridge-probe"), "log records are bridged");
    // … and the handshake request never reaches the logs.
    assert!(
        !logs.contains(tokens.access_token.expose_secret()),
        "access token leaked"
    );
    assert!(
        !logs.to_lowercase().contains("authorization"),
        "auth header leaked"
    );
    assert!(!logs.to_lowercase().contains("bearer "), "bearer leaked");
}

#[test]
fn trace_records_of_the_log_crate_are_compiled_out() {
    assert!(log::STATIC_MAX_LEVEL <= log::LevelFilter::Debug);
}
