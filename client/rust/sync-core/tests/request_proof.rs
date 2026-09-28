//! Protocol 1.5 sender-constrained tokens: `x-cc-device-proof` on every
//! authenticated request, refresh and the WebSocket upgrade.

mod common;

use cc_models::host::Host;
use cc_models::{ObjectPayload, VaultObject};
use cc_protocol::auth::{RegisterRequest, SecretString};
use cc_protocol::sync::{ChangesQuery, PushRequest, SnapshotQuery};
use cc_protocol::version::{Platform, HEADER_DEVICE_PROOF, HEADER_PROTOCOL_VERSION};
use cc_protocol::{paths, PROTOCOL_VERSION};
use cc_sync_core::mock::{MockServer, MockServerConfig};
use cc_sync_core::*;
use common::*;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

async fn required_server() -> MockServer {
    MockServer::start_with(MockServerConfig {
        require_request_proof: true,
        ..Default::default()
    })
    .await
}

fn email() -> String {
    format!("proof-{}@example.test", uuid::Uuid::new_v4().simple())
}

/// A client with its own token store (so tests can "steal" the tokens).
fn client_with_store(server: &MockServer) -> (ApiClient, Arc<MemoryTokenStore>) {
    let store = Arc::new(MemoryTokenStore::new());
    let cfg = ApiConfig::new(server.url().as_str(), "0.1.0-test", Platform::Cli).unwrap();
    (ApiClient::new(cfg, store.clone()).unwrap(), store)
}

async fn register_with(api: &ApiClient, keys: &DeviceKeys) {
    api.register(&RegisterRequest {
        email: email(),
        password: SecretString::new(PASSWORD),
        device: keys.registration("device A"),
        device_proof: Some(keys.proof()),
    })
    .await
    .unwrap();
}

fn rejected(r: Result<impl std::fmt::Debug, ApiError>) -> ProofRejection {
    match r {
        Err(ApiError::RequestProofRejected { reason }) => reason,
        other => panic!("expected a proof rejection, got {other:?}"),
    }
}

async fn first_ws_event(api: &ApiClient) -> (EventStream, WsEvent) {
    let es = EventStream::spawn(api.clone(), EventStreamConfig::default());
    let mut rx = es.subscribe();
    let ev = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("websocket event")
        .unwrap();
    (es, ev)
}

#[tokio::test]
async fn signed_client_works_against_a_server_that_requires_proofs() {
    let server = required_server().await;
    let keys = DeviceKeys::generate();
    let signer = keys.signer();
    let (api, _store) = client_with_store(&server);
    let api = api.with_request_signer(signer.clone());
    assert!(api.has_request_signer());
    register_with(&api, &keys).await;

    api.me().await.unwrap();
    let vault = create_vault(&api, keys.device_id).await; // POST with body
    api.list_vaults().await.unwrap();
    api.get_vault(vault.vault_id).await.unwrap();
    api.list_envelopes(vault.vault_id).await.unwrap();
    api.recovery_vault_envelope(vault.vault_id).await.unwrap(); // GET ?vault_id=
    api.changes(&ChangesQuery {
        vault_id: vault.vault_id,
        after: 0,
        limit: Some(10),
    })
    .await
    .unwrap();
    api.snapshot(&SnapshotQuery {
        vault_id: vault.vault_id,
        cursor: None,
        limit: Some(5),
    })
    .await
    .unwrap();
    api.refresh().await.unwrap();
    // Expired access token: signed refresh, then the signed retry.
    server.expire_access_tokens();
    api.me().await.unwrap();

    // WebSocket upgrade is signed too.
    let (es, ev) = first_ws_event(&api).await;
    assert_eq!(ev, WsEvent::Connected);

    // A full sync cycle: snapshot, pre-push pull, push (body), pull.
    let c = client(&server, api.clone(), keys, vault.vault_id).await;
    let h = Host::new("web", "10.0.0.1");
    c.engine
        .put(ObjectPayload::new(VaultObject::Host(h.clone())))
        .await
        .unwrap();
    let r = sync_ok(&c.engine).await;
    assert_eq!(r.pushed, 1);
    assert!(server.object(vault.vault_id, h.id).is_some());
    es.shutdown().await;
    api.logout(&Default::default()).await.unwrap();

    let stats = server.request_proof_stats();
    assert_eq!((stats.missing, stats.rejected), (0, 0), "{stats:?}");
    assert!(stats.valid >= 14, "{stats:?}");
    assert!(signer.calls.load(Ordering::SeqCst) as u64 >= stats.valid);
}

#[tokio::test]
async fn unsigned_client_is_rejected_as_missing_when_required_and_metered_otherwise() {
    let server = required_server().await;
    let keys = DeviceKeys::generate();
    let (api, _store) = client_with_store(&server);
    register_with(&api, &keys).await; // login/register carry the 1.4 body proof
    assert_eq!(rejected(api.me().await), ProofRejection::Missing);
    let err = api.refresh().await.unwrap_err();
    assert!(matches!(
        err,
        ApiError::RequestProofRejected {
            reason: ProofRejection::Missing
        }
    ));
    assert!(api.is_authenticated().await.unwrap(), "tokens kept");
    assert_eq!(err.code(), Some(cc_protocol::ErrorCode::InvalidProof));
    let (es, ev) = first_ws_event(&api).await;
    match ev {
        WsEvent::Disconnected { reason } => assert!(reason.contains("missing"), "{reason}"),
        other => panic!("{other:?}"),
    }
    es.shutdown().await;

    // Accept-and-meter rollout: unsigned passes and is counted.
    let server = MockServer::start().await;
    let (api, _store) = client_with_store(&server);
    register_with(&api, &DeviceKeys::generate()).await;
    api.me().await.unwrap();
    assert_eq!(server.request_proof_stats().missing, 1);
    let keys = DeviceKeys::generate();
    let (signed, _s) = client_with_store(&server);
    let signed = signed.with_request_signer(keys.signer());
    register_with(&signed, &keys).await;
    signed.me().await.unwrap();
    let stats = server.request_proof_stats();
    assert_eq!((stats.valid, stats.missing), (1, 1), "{stats:?}");
}

#[tokio::test]
async fn stolen_access_and_refresh_tokens_are_useless_without_the_device_key() {
    let server = required_server().await;
    let keys = DeviceKeys::generate();
    let (victim, victim_store) = client_with_store(&server);
    let victim = victim.with_request_signer(keys.signer());
    register_with(&victim, &keys).await;
    let vault = create_vault(&victim, keys.device_id).await;
    let stolen = victim_store.load().await.unwrap().unwrap();

    // Thief without any key.
    let (thief, _) = client_with_store(&server);
    thief.set_tokens(&stolen).await.unwrap();
    assert_eq!(rejected(thief.me().await), ProofRejection::Missing);
    assert_eq!(rejected(thief.refresh().await), ProofRejection::Missing);

    // Thief signing with its own key while claiming the victim's device.
    let forged = Arc::new(TestSigner::new(
        keys.device_id,
        DeviceKeys::generate().signing,
    ));
    thief.set_request_signer(Some(forged));
    assert_eq!(rejected(thief.me().await), ProofRejection::InvalidSignature);
    assert_eq!(
        rejected(thief.list_vaults().await),
        ProofRejection::InvalidSignature
    );
    assert_eq!(
        rejected(
            thief
                .push(&PushRequest {
                    vault_id: vault.vault_id,
                    device_id: keys.device_id,
                    mutations: Vec::new(),
                })
                .await
        ),
        ProofRejection::InvalidSignature
    );
    assert_eq!(
        rejected(thief.refresh().await),
        ProofRejection::InvalidSignature
    );
    let (es, ev) = first_ws_event(&thief).await;
    assert!(
        matches!(&ev, WsEvent::Disconnected { reason } if reason.contains("invalid signature")),
        "{ev:?}"
    );
    es.shutdown().await;

    // The victim is unaffected: its refresh token was never consumed (no
    // rotation, no reuse detection) and its requests keep working.
    victim.me().await.unwrap();
    victim.refresh().await.unwrap();
    victim.me().await.unwrap();
    assert!(server.request_proof_stats().rejected >= 5);
}

#[tokio::test]
async fn stale_proofs_are_resigned_once_and_replays_are_rejected() {
    let server = required_server().await;
    let keys = DeviceKeys::generate();
    let signer = keys.signer();
    let (api, _) = client_with_store(&server);
    let api = api.with_request_signer(signer.clone());
    register_with(&api, &keys).await;

    // One stale proof (clock hiccup): re-signed and retried once.
    signer.stale_next.store(1, Ordering::SeqCst);
    let hits = server.hits(paths::AUTH_ME);
    api.me().await.unwrap();
    assert_eq!(server.hits(paths::AUTH_ME), hits + 2);
    // Persistently stale (device clock off): surfaced, not retried forever.
    signer.stale_next.store(2, Ordering::SeqCst);
    let err = api.me().await.unwrap_err();
    assert!(err.to_string().contains("clock"), "{err}");
    assert!(matches!(
        err,
        ApiError::RequestProofRejected {
            reason: ProofRejection::Stale
        }
    ));
    signer.stale_next.store(1, Ordering::SeqCst);
    let (es, ev) = first_ws_event(&api).await;
    assert_eq!(
        ev,
        WsEvent::Connected,
        "websocket re-signs a stale proof once"
    );
    es.shutdown().await;

    // Replaying a used proof (same nonce) is refused.
    api.me().await.unwrap();
    signer.set_tamper(Some(Tamper::Replay));
    assert_eq!(rejected(api.me().await), ProofRejection::Replayed);
    signer.set_tamper(None);
    api.me().await.unwrap();
}

#[tokio::test]
async fn altered_target_or_body_fails_verification() {
    let server = required_server().await;
    let keys = DeviceKeys::generate();
    let signer = keys.signer();
    let (api, store) = client_with_store(&server);
    let api = api.with_request_signer(signer.clone());
    register_with(&api, &keys).await;
    let vault = create_vault(&api, keys.device_id).await;

    signer.set_tamper(Some(Tamper::SignPath(paths::AUTH_ME.into())));
    assert_eq!(
        rejected(api.list_vaults().await),
        ProofRejection::InvalidSignature
    );
    // The query string is covered.
    signer.set_tamper(Some(Tamper::DropQuery));
    assert_eq!(
        rejected(
            api.changes(&ChangesQuery {
                vault_id: vault.vault_id,
                after: 0,
                limit: None,
            })
            .await
        ),
        ProofRejection::InvalidSignature
    );
    // The exact body bytes are covered.
    signer.set_tamper(Some(Tamper::SignBody(b"{}".to_vec())));
    assert_eq!(
        rejected(
            api.push(&PushRequest {
                vault_id: vault.vault_id,
                device_id: keys.device_id,
                mutations: Vec::new(),
            })
            .await
        ),
        ProofRejection::InvalidSignature
    );
    signer.set_tamper(None);

    // A garbage header is "malformed" (raw request, valid bearer token).
    let token = store.load().await.unwrap().unwrap().access_token;
    let resp = reqwest::Client::new()
        .get(
            server
                .url()
                .join(paths::AUTH_ME.trim_start_matches('/'))
                .unwrap(),
        )
        .header(HEADER_PROTOCOL_VERSION, PROTOCOL_VERSION.to_string())
        .header(HEADER_DEVICE_PROOF, "not-a-proof")
        .bearer_auth(token.expose_secret())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 422);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "invalid_proof");
    assert_eq!(body["details"]["reason"], "malformed");
    api.me().await.unwrap();
}

#[tokio::test]
async fn signer_failure_never_falls_back_to_an_unsigned_request() {
    struct Broken;
    impl RequestSigner for Broken {
        fn request_proof(
            &self,
            _: &str,
            _: &str,
            _: &[u8],
        ) -> Result<cc_protocol::devices::RequestProof, SignerError> {
            Err(SignerError::Unavailable)
        }
    }
    let server = MockServer::start().await; // meter mode would accept unsigned
    let keys = DeviceKeys::generate();
    let (api, _) = client_with_store(&server);
    register_with(&api, &keys).await;
    api.set_request_signer(Some(Arc::new(Broken)));
    let hits = server.hits(paths::AUTH_ME);
    let err = api.me().await.unwrap_err();
    assert!(
        matches!(err, ApiError::RequestSigning(SignerError::Unavailable)),
        "{err:?}"
    );
    assert_eq!(server.hits(paths::AUTH_ME), hits, "nothing was sent");
    api.set_request_signer(None);
    api.me().await.unwrap();
}

/// `cc_request_proofs_total{result="…"}` values from a Prometheus text page.
async fn proof_metrics(url: &str) -> std::collections::BTreeMap<String, u64> {
    let text = reqwest::get(url).await.unwrap().text().await.unwrap();
    text.lines()
        .filter_map(|l| l.strip_prefix("cc_request_proofs_total{result=\""))
        .filter_map(|l| {
            let (result, rest) = l.split_once("\"}")?;
            Some((result.to_owned(), rest.trim().parse().ok()?))
        })
        .collect()
}

/// Gated live check against a real server (protocol ≥ 1.5), skipped unless
/// `CC_E2E_SERVER` is set, e.g.
/// `CC_E2E_SERVER=http://localhost:8080 CC_E2E_METRICS=http://127.0.0.1:9090/metrics`.
/// Registers a throwaway account; every request of a full client session
/// (REST, refresh, WebSocket upgrade, sync cycle) must be metered `valid`.
#[tokio::test]
async fn live_server_accepts_signed_requests() {
    let Ok(base) = std::env::var("CC_E2E_SERVER") else {
        eprintln!("CC_E2E_SERVER not set; skipping live request-proof check");
        return;
    };
    let metrics = std::env::var("CC_E2E_METRICS").ok();
    let before = match &metrics {
        Some(m) => proof_metrics(m).await,
        None => Default::default(),
    };
    let keys = DeviceKeys::generate();
    let api = ApiClient::new(
        ApiConfig::new(&base, "0.1.0-e2e", Platform::Cli).unwrap(),
        Arc::new(MemoryTokenStore::new()),
    )
    .unwrap()
    .with_request_signer(keys.signer());
    register_with(&api, &keys).await;
    api.me().await.unwrap();
    let vault = create_vault(&api, keys.device_id).await;
    api.list_vaults().await.unwrap();
    api.changes(&ChangesQuery {
        vault_id: vault.vault_id,
        after: 0,
        limit: Some(10),
    })
    .await
    .unwrap();
    let (es, ev) = first_ws_event(&api).await;
    assert_eq!(ev, WsEvent::Connected);
    api.refresh().await.unwrap();
    let storage = cc_storage_core::Storage::open_in_memory(
        cc_storage_core::DatabaseKey::from_bytes(random_bytes(32).try_into().unwrap()),
    )
    .await
    .unwrap();
    storage
        .put_profile(cc_storage_core::Profile::new_synced(
            cc_storage_core::ProfileId::new(),
            keys.device_id,
            base.clone(),
            None,
            None,
        ))
        .await
        .unwrap();
    let objects = ObjectStore::new(vault.vault_id, storage, TestCodec::new(vault.vault_id));
    let engine = SyncEngine::new(objects, api.clone(), fast_config(keys.device_id))
        .await
        .unwrap();
    engine
        .put(ObjectPayload::new(VaultObject::Host(Host::new(
            "live", "10.9.9.9",
        ))))
        .await
        .unwrap();
    assert_eq!(sync_ok(&engine).await.pushed, 1);
    es.shutdown().await;
    api.logout(&Default::default()).await.unwrap();
    if let Some(m) = &metrics {
        let after = proof_metrics(m).await;
        let get = |map: &std::collections::BTreeMap<String, u64>, k: &str| {
            map.get(k).copied().unwrap_or(0)
        };
        eprintln!("request proof metrics before {before:?} after {after:?}");
        for bad in [
            "missing",
            "malformed",
            "stale",
            "replayed",
            "invalid_signature",
        ] {
            assert_eq!(
                get(&after, bad),
                get(&before, bad),
                "result={bad} increased"
            );
        }
        assert!(get(&after, "valid") >= get(&before, "valid") + 10);
    }
}
