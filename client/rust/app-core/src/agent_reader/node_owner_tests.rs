//! DRAFT: actual Local AppCore/SQLCipher source selection and loaded identity.
//! The existing scope-test fixture is reused verbatim; no fake review context.
use super::*;
use cc_agent_node_wire::{AgentId, Context, DeploymentId, Hash, Header,
    InvitationId, LocalProfileId, PersonalAccountId, PublicKey, Scope, Scopes, Tenant,
    verify_statement};
use cc_monitoring_core::{AuthRef, Route, SourceConfig, TlsConfig};
use std::{collections::BTreeMap, time::Duration};

const PASSWORD: &str = "reader original context ephemeral password 42";
async fn fixture() -> (tempfile::TempDir, AppCore, ReaderSourceSelection) {
    let root = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        root.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("Reader context fixture".into(), PASSWORD.into())
        .await
        .unwrap();
    let host = app
        .save_host(crate::HostDto::new("Reader fixture", "127.0.0.1"))
        .await
        .unwrap();
    let view = app
        .monitoring_source_create(
            0,
            SourceConfig {
                id: String::new(),
                name: "Reader metadata only".into(),
                endpoint: "https://reader-fixture.example.invalid/prometheus".into(),
                route: Route::Direct,
                tls: TlsConfig::default(),
                auth: AuthRef::None,
            },
        )
        .await
        .unwrap();
    let source = view.sources[0].id.clone();
    let view = app
        .monitoring_binding_create(
            view.revision.unwrap(),
            "Reader explicit metadata binding".into(),
            host.id.clone(),
            source.clone(),
            1,
            BTreeMap::from([("job".into(), "reader-fixture".into())]),
        )
        .await
        .unwrap();
    // The real object-event worker processes store events FIFO. Subscribe
    // before a fresh sentinel write and wait for its post-refresh event, so
    // all earlier fixture writes/readbacks finish BEFORE the Reader baseline.
    // save_dto/put returns only after its own synchronous refresh, too.
    let mut events = app.subscribe_events();
    let sentinel = app
        .save_host(crate::HostDto::new("Reader fixture writer barrier", "127.0.0.2"))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let crate::dto::AppEvent::ObjectsChanged {
                kind: cc_models::ObjectKind::Host,
                origin: crate::dto::ChangeOriginDto::Local,
                ids,
            } = events.recv().await.unwrap()
            {
                if ids.len() == 1 && ids[0] == sentinel.id {
                    break;
                }
            }
        }
    })
    .await
    .expect("fixture writer did not cross its real object-event barrier");
    let selection = ReaderSourceSelection {
        profile_id: app.session().await.unwrap().profile_id.to_string(),
        host_id: host.id,
        source_id: source,
        source_revision: 1,
        binding_id: view.bindings[0].id.clone(),
        binding_revision: view.bindings[0].revision,
    };
    (root, app, selection)
}

fn request(app_identity_key: [u8; 32], stamp: u64) -> (OwnerPin, Offer) {
    let node = cc_crypto_core::DeviceSecretKeys::generate().unwrap();
    let pin = OwnerPin {
        context: Context {
            tenant: Tenant::LocalProfile { id: LocalProfileId::new("11111111-1111-4111-8111-111111111111").unwrap() },
            deployment_id: DeploymentId::new("22222222-2222-4222-8222-222222222222").unwrap(),
        },
        key: PublicKey::from_bytes(app_identity_key), epoch: 1, checkpoint: 0,
        expires_at: Timestamp::new(stamp + 10000).unwrap(),
    };
    let offer = Offer {
        header: Header { version: 1, context: pin.context.clone(), owner_epoch: 1, checkpoint: 1 },
        invitation_id: InvitationId::new("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap(),
        agent_id: AgentId::new("33333333-3333-4333-8333-333333333333").unwrap(),
        agent_key: PublicKey::from_bytes(node.public_keys().signing), token_hash: Hash::from_bytes([5; 32]),
        scopes: Scopes(vec![Scope::TelemetryRead]), issued_at: Timestamp::new(stamp).unwrap(),
        expires_at: Timestamp::new(stamp + 300).unwrap(),
    };
    (pin, offer)
}
fn stamp() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reader_node_review_uses_loaded_identity_and_distinct_local_namespace() {
    let (_root, app, selection) = fixture().await;
    assert_ne!(selection.profile_id, "11111111-1111-4111-8111-111111111111");
    let identity = app.session().await.unwrap().identity();
    let (pin, offer) = request(identity.public_keys().signing, stamp());
    let review = ReaderNodeOwnerReview::enrollment_offer(&app, selection, &pin, offer.clone(), &CancellationToken::new()).await.unwrap();
    assert_eq!(review.details().unwrap().owner_pin, pin);
    assert_eq!(review.source_snapshot().native_host_id.len(), 36);
    let fence = review.scope.scope_identity();
    let ReaderNodeOwnerSigned::Offer(signed) = review.approve_once(&app, &pin).await.unwrap() else { panic!() };
    assert_eq!(signed.body, offer);
    verify_statement(&signed, PublicKey::from_bytes(identity.public_keys().signing)).unwrap();
    assert!(!fence.is_live());
    app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reader_node_review_cancel_changed_pin_and_drop_retire_original() {
    let (_root, app, selection) = fixture().await;
    let identity = app.session().await.unwrap().identity();
    let cancel = CancellationToken::new();
    let (pin, offer) = request(identity.public_keys().signing, stamp());
    let review = ReaderNodeOwnerReview::enrollment_offer(&app, selection.clone(), &pin, offer, &cancel).await.unwrap();
    let fence = review.scope.scope_identity();
    cancel.cancel();
    assert!(review.approve_once(&app, &pin).await.is_err());
    assert!(!fence.is_live());
    let (pin, offer) = request(identity.public_keys().signing, stamp());
    let review = ReaderNodeOwnerReview::enrollment_offer(&app, selection.clone(), &pin, offer, &CancellationToken::new()).await.unwrap();
    let fence = review.scope.scope_identity();
    let mut changed = pin.clone();
    changed.checkpoint += 1;
    assert!(review.approve_once(&app, &changed).await.is_err());
    assert!(!fence.is_live());
    let (pin, offer) = request(identity.public_keys().signing, stamp());
    let review = ReaderNodeOwnerReview::enrollment_offer(&app, selection, &pin, offer, &CancellationToken::new()).await.unwrap();
    let fence = review.scope.scope_identity();
    drop(review);
    assert!(!fence.is_live());
    app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reader_node_review_rejects_foreign_pin_key_and_personal_profile_cast() {
    let (_root, app, selection) = fixture().await;
    let foreign = cc_crypto_core::DeviceSecretKeys::generate().unwrap();
    let (pin, offer) = request(foreign.public_keys().signing, stamp());
    assert!(ReaderNodeOwnerReview::enrollment_offer(&app, selection.clone(), &pin, offer, &CancellationToken::new()).await.is_err());
    let identity = app.session().await.unwrap().identity();
    let (mut pin, mut offer) = request(identity.public_keys().signing, stamp());
    pin.context.tenant = Tenant::PersonalAccount { id: PersonalAccountId::new(&selection.profile_id).unwrap() };
    offer.header.context = pin.context.clone();
    assert!(ReaderNodeOwnerReview::enrollment_offer(&app, selection, &pin, offer, &CancellationToken::new()).await.is_err());
    app.shutdown().await.unwrap();
}
