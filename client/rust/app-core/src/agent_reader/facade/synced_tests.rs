//! DRAFT: real Synced AppCore + MockServer authentication and Root scope.
//! No fake passing authorizer, Node pin, SSH session or grant. These cases
//! do NOT complete the fourteen-case account/transport acceptance matrix.
use super::*;
use cc_monitoring_core::{AuthRef, Route, SourceConfig, TlsConfig};
use cc_sync_core::mock::MockServer;

const ACCOUNT_PASSWORD: &str = "reader synced account password 42";
const VAULT_PASSWORD: &str = "reader synced personal vault passphrase 42";

#[test]
fn profile_shape_routing_is_closed_and_not_account_authority() {
    assert!(supported_profile_shape(&ProfileKind::Local, false, false, false).is_ok());
    assert!(supported_profile_shape(&ProfileKind::Synced, true, true, true).is_ok());
    for fields in [
        (false, true, true),
        (true, false, true),
        (true, true, false),
    ] {
        assert!(
            supported_profile_shape(&ProfileKind::Synced, fields.0, fields.1, fields.2).is_err()
        );
    }
    assert!(supported_profile_shape(&ProfileKind::Local, true, true, true).is_err());
    // Positive routing is NOT a passing Context; all following account tests
    // invoke actual Root capture, which must perform real server/owner checks.
}

struct Fixture {
    _root: tempfile::TempDir,
    server: MockServer,
    app: AppCore,
    selection: ReaderSourceSelection,
}
impl Fixture {
    async fn new() -> Self {
        let server = MockServer::start().await;
        let root = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            root.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_synced_profile(
            "Reader personal fixture".into(),
            server.url().to_string(),
            "reader-synced@example.org".into(),
            ACCOUNT_PASSWORD.into(),
            crate::AccountMode::Register,
        )
        .await
        .unwrap();
        app.create_vault(VAULT_PASSWORD.into()).await.unwrap();
        let host = app
            .save_host(crate::HostDto::new("Reader personal Host", "127.0.0.1"))
            .await
            .unwrap();
        let view = app
            .monitoring_source_create(
                0,
                SourceConfig {
                    id: String::new(),
                    name: "Reader personal source".into(),
                    endpoint: "https://reader-source.example.invalid/prometheus".into(),
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
                "Reader personal binding".into(),
                host.id.clone(),
                source.clone(),
                1,
                BTreeMap::from([("job".into(), "reader-test".into())]),
            )
            .await
            .unwrap();
        let selection = ReaderSourceSelection {
            profile_id: app.session().await.unwrap().profile_id.to_string(),
            host_id: host.id,
            source_id: source,
            source_revision: 1,
            binding_id: view.bindings[0].id.clone(),
            binding_revision: view.bindings[0].revision,
        };
        // Drain and stop ONLY background sync BEFORE every test capture.
        // Equal-revision queued edits below are deliberate, never ACK races.
        app.sync_now().await.unwrap();
        app.session()
            .await
            .unwrap()
            .unlocked_opt()
            .await
            .unwrap()
            .stop_engine()
            .await;
        Self {
            _root: root,
            server,
            app,
            selection,
        }
    }
    async fn capture(&self) -> ReaderOriginalContext {
        ReaderOriginalContext::capture(&self.app, self.selection.clone(), &CancellationToken::new())
            .await
            .unwrap()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn personal_synced_capture_uses_actual_me_not_profile_uuid() {
    let f = Fixture::new().await;
    let owner = f.app.session().await.unwrap();
    let me = owner.api().unwrap().me().await.unwrap();
    let profile = owner.profile().await.unwrap();
    assert_eq!(profile.user_id, Some(me.user_id));
    assert_eq!(profile.device_id, me.current_device_id);
    assert_ne!(profile.profile_id.to_string(), me.user_id.to_string());
    let first = f.capture().await;
    let second = f.capture().await;
    assert!(first.scope_identity().check(&second).is_err());
    assert!(first.check(&f.app).await.is_ok());
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn logout_cancels_reader_while_personal_vault_remains_unlocked() {
    let f = Fixture::new().await;
    let ctx = f.capture().await;
    let token = ctx.scope_identity();
    let owner = f.app.session().await.unwrap();
    let unlock = owner.unlocked_opt().await.unwrap();
    f.app.logout().await.unwrap();
    assert!(
        Arc::ptr_eq(&unlock, &owner.unlocked_opt().await.unwrap()),
        "logout is not a vault lock"
    );
    assert!(!token.is_live());
    assert!(ctx.check(&f.app).await.is_err());
    assert!(
        ReaderOriginalContext::capture(&f.app, f.selection.clone(), &CancellationToken::new())
            .await
            .is_err()
    );
    // No SSH exists in this test. A later actual existing-SSH test must prove
    // Reader cancellation independently of the intentionally surviving tunnel.
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_same_subject_refresh_cancels_old_attempt_without_renewal() {
    let f = Fixture::new().await;
    let ctx = f.capture().await;
    let deadline = ctx.deadline();
    let api = f.app.session().await.unwrap().api().unwrap();
    let before = api.me().await.unwrap();
    api.refresh().await.unwrap();
    let after = api.me().await.unwrap();
    assert_eq!(before.user_id, after.user_id);
    assert!(!ctx.scope_identity().is_live());
    assert_eq!(ctx.deadline(), deadline);
    let fresh = f.capture().await;
    assert!(ctx.scope_identity().check(&fresh).is_err());
    assert_eq!(
        ctx.stable_owner_binding_hash(),
        fresh.stable_owner_binding_hash(),
        "same owner remains stable across refresh; auth epoch is attempt-only"
    );
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cached_subject_offline_me_or_cleared_tokens_never_capture() {
    let f = Fixture::new().await;
    let owner = f.app.session().await.unwrap();
    assert!(owner.profile().await.unwrap().user_id.is_some());
    f.server.set_offline(true);
    assert!(
        ReaderOriginalContext::capture(&f.app, f.selection.clone(), &CancellationToken::new())
            .await
            .is_err()
    );
    f.server.set_offline(false);
    owner.api().unwrap().clear_tokens().await.unwrap();
    assert!(
        ReaderOriginalContext::capture(&f.app, f.selection.clone(), &CancellationToken::new())
            .await
            .is_err()
    );
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn persisted_subject_or_server_swap_denies_real_authenticated_capture() {
    let f = Fixture::new().await;
    let owner = f.app.session().await.unwrap();
    let original = owner.profile().await.unwrap();
    let mut changed = original.clone();
    changed.user_id = Some(cc_protocol::UserId::new());
    owner.put_profile(changed).await.unwrap();
    assert!(
        ReaderOriginalContext::capture(&f.app, f.selection.clone(), &CancellationToken::new())
            .await
            .is_err()
    );
    let another = MockServer::start().await;
    let mut changed = original.clone();
    changed.server_url = Some(another.url().to_string());
    owner.put_profile(changed).await.unwrap();
    assert!(
        ReaderOriginalContext::capture(&f.app, f.selection.clone(), &CancellationToken::new())
            .await
            .is_err()
    );
    owner.put_profile(original).await.unwrap();
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replacing_actual_loaded_device_or_profile_closing_invalidates_original() {
    let f = Fixture::new().await;
    let ctx = f.capture().await;
    let token = ctx.scope_identity();
    let other = Fixture::new().await;
    let identity = other.app.session().await.unwrap().identity();
    f.app.session().await.unwrap().set_identity(identity);
    assert!(!token.is_live());
    assert!(ctx.check(&f.app).await.is_err());
    f.app.close_profile().await.unwrap();
    assert!(!token.is_live());
    other.app.shutdown().await.unwrap();
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn coalesced_host_body_same_revision_aba_cannot_revive_original_scope() {
    let f = Fixture::new().await;
    let owner = f.app.session().await.unwrap();
    let unlocked = owner.unlocked_opt().await.unwrap();
    let id = crate::dto::parse_id("reader_host", &f.selection.host_id).unwrap();
    // First edit creates an unsent queued slot at base+1. No engine runs.
    let mut host = f.app.get_host(f.selection.host_id.clone()).await.unwrap();
    host.notes = "queued body A".into();
    f.app.save_host(host).await.unwrap();
    let model_a = unlocked.working().host(id).unwrap();
    let body_a = serde_json::to_value(&model_a).unwrap();
    let revision = unlocked.working().revision_of(id).unwrap();
    let ctx = f.capture().await;
    let token = ctx.scope_identity();
    // This second original scope is NOT observed between B and restored A.
    let unobserved = f.capture().await;
    let unobserved_token = unobserved.scope_identity();
    assert!(token.is_live() && unobserved_token.is_live());
    let version = unlocked.working().version();
    let mut host = f.app.get_host(f.selection.host_id.clone()).await.unwrap();
    host.notes = "coalesced body B".into();
    f.app.save_host(host).await.unwrap();
    assert_eq!(
        unlocked.working().revision_of(id),
        Some(revision),
        "queued edit must coalesce at base+1"
    );
    assert_ne!(
        serde_json::to_value(unlocked.working().host(id).unwrap()).unwrap(),
        body_a
    );
    assert!(unlocked.working().version() > version);
    assert!(
        !token.is_live(),
        "same-revision body mutation must revoke before any held action"
    );
    let called = std::sync::atomic::AtomicBool::new(false);
    assert!(ctx
        .with_held(&f.app, |_| {
            called.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        })
        .await
        .is_err());
    assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
    // Restore the EXACT actual model, not a DTO save that rewrites timestamps.
    unlocked
        .writer
        .put(cc_models::VaultObject::Host(model_a))
        .await
        .unwrap();
    assert_eq!(unlocked.working().revision_of(id), Some(revision));
    assert_eq!(
        serde_json::to_value(unlocked.working().host(id).unwrap()).unwrap(),
        body_a
    );
    assert!(
        !token.is_live(),
        "observed mismatch must stay sticky after exact restoration"
    );
    assert!(
        !unobserved_token.is_live(),
        "unobserved body ABA must also fail the captured WS counter"
    );
    assert!(ctx.check(&f.app).await.is_err());
    assert!(unobserved.check(&f.app).await.is_err());
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn owning_a_team_does_not_make_its_host_personal_reader_authority() {
    let f = Fixture::new().await;
    let team = f
        .app
        .create_team("Reader team denial".into(), None, None, None)
        .await
        .unwrap();
    let group = f
        .app
        .create_team_group(team.vault_id, crate::GroupDto::new("Team reader host"))
        .await
        .unwrap();
    let mut host = crate::HostDto::new("Team-owned Host", "127.0.0.1");
    host.group_id = Some(group.id);
    let host = f.app.save_host(host).await.unwrap();
    let view = f.app.monitoring_sources().await.unwrap();
    let view = f
        .app
        .monitoring_binding_create(
            view.revision.unwrap(),
            "Team denial binding".into(),
            host.id.clone(),
            f.selection.source_id.clone(),
            1,
            BTreeMap::from([("job".into(), "reader-team".into())]),
        )
        .await
        .unwrap();
    let b = view
        .bindings
        .iter()
        .find(|b| b.binding.host_id().to_string() == host.id)
        .unwrap();
    let mut selection = f.selection.clone();
    selection.host_id = host.id;
    selection.binding_id = b.id.clone();
    selection.binding_revision = b.revision;
    assert!(
        ReaderOriginalContext::capture(&f.app, selection, &CancellationToken::new())
            .await
            .is_err()
    );
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn api_old_other_old_aba_never_revives_original_reader_scope() {
    let f = Fixture::new().await;
    let old = f.capture().await;
    let token = old.scope_identity();
    let owner = f.app.session().await.unwrap();
    let original = owner.api().unwrap();
    let other = Fixture::new().await;
    let replacement = other.app.session().await.unwrap().api().unwrap();
    owner.set_api(Some(replacement));
    assert!(!token.is_live());
    owner.set_api(Some(original));
    assert!(
        !token.is_live(),
        "restoring the same API Arc cannot restore its old auth epoch"
    );
    assert!(old.check(&f.app).await.is_err());
    let fresh = f.capture().await;
    assert!(old.snapshot() == fresh.snapshot());
    assert!(token.check(&fresh).is_err());
    assert!(fresh.check(&f.app).await.is_ok());
    other.app.shutdown().await.unwrap();
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn identity_old_other_old_aba_never_revives_original_reader_scope() {
    let f = Fixture::new().await;
    let old = f.capture().await;
    let token = old.scope_identity();
    let owner = f.app.session().await.unwrap();
    let original = owner.identity();
    let other = Fixture::new().await;
    owner.set_identity(other.app.session().await.unwrap().identity());
    assert!(!token.is_live());
    owner.set_identity(original);
    assert!(
        !token.is_live(),
        "restoring the same DeviceIdentity Arc cannot restore auth epoch"
    );
    assert!(old.check(&f.app).await.is_err());
    let fresh = f.capture().await;
    assert!(old.snapshot() == fresh.snapshot());
    assert!(token.check(&fresh).is_err());
    assert!(fresh.check(&f.app).await.is_ok());
    other.app.shutdown().await.unwrap();
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocked_logout_cancels_before_session_acquisition_and_drop_releases_pending() {
    let f = Fixture::new().await;
    let old = f.capture().await;
    let token = old.scope_identity();
    let owner = f.app.session().await.unwrap();
    let unlocked = owner.unlocked_opt().await.unwrap();
    let api = owner.api().unwrap();
    let hold = f.app.hold_session(&owner).await.unwrap();
    // Box owns the actual future. Dropping a Pin<&mut Future> alone would NOT
    // drop the underlying logout and would not test transition-guard cleanup.
    let mut logout = Box::pin(f.app.logout());
    tokio::select! { biased;
        _ = &mut logout => panic!("logout unexpectedly acquired held active Session"),
        _ = tokio::time::sleep(Duration::from_millis(20)) => {},
    }
    assert!(
        !token.is_live(),
        "first-line auth transition must precede session().await"
    );
    assert!(
        api.is_authenticated().await.unwrap(),
        "blocked logout has not cleared cached tokens yet"
    );
    assert!(Arc::ptr_eq(&unlocked, &owner.unlocked_opt().await.unwrap()));
    drop(logout);
    drop(hold);
    assert!(
        !token.is_live(),
        "dropping pending logout never resurrects the old attempt"
    );
    let fresh = tokio::time::timeout(Duration::from_secs(5), f.capture())
        .await
        .unwrap();
    assert!(
        fresh.check(&f.app).await.is_ok(),
        "cancelled entry must not leave permanent pending"
    );
    assert!(token.check(&fresh).is_err());
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retained_context_and_scope_do_not_keep_closed_session_device_identity_alive() {
    let f = Fixture::new().await;
    let ctx = f.capture().await;
    let scope = ctx.scope_identity();
    let (session, device) = {
        let owner = f.app.session().await.unwrap();
        let identity = owner.identity();
        (Arc::downgrade(&owner), Arc::downgrade(&identity))
    }; // No test-held Session/API/Unlocked/DeviceIdentity strong references.
    assert!(device.strong_count() > 0);
    f.app.close_profile().await.unwrap();
    // Real owned Session.close/engine cleanup may complete its final task
    // drops asynchronously. Poll only weak counts; never prolong ownership
    // with an upgraded Arc while testing its disappearance.
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if session.strong_count() == 0 && device.strong_count() == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("retained Reader scope must not keep API signer IdentityCell alive");
    assert!(session.upgrade().is_none());
    assert!(device.upgrade().is_none());
    assert!(!scope.is_live());
    assert!(ctx.check(&f.app).await.is_err());
    // Both values remain alive across the weak-count assertions. This tests
    // loaded-object custody, not deletion of OS-stored keys or all key bytes.
    drop(scope);
    drop(ctx);
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn known_vault_key_conflict_after_capture_blocks_held_action_and_new_capture() {
    let f = Fixture::new().await;
    let owner = f.app.session().await.unwrap();
    let unlocked = owner.unlocked_opt().await.unwrap();
    unlocked.stop_engine().await;
    let ctx = f.capture().await;
    unlocked.set_key_conflicts(vec![crate::dto::VaultKeyConflictDto {
        device_id: cc_protocol::DeviceId::new().to_string(),
        device_name: "Known conflicting public commitment".into(),
        fingerprint: "0102030405060708".into(),
    }]);
    let ran = std::sync::atomic::AtomicBool::new(false);
    assert!(ctx
        .with_held(&f.app, |_| {
            ran.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        })
        .await
        .is_err());
    assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
    assert!(
        ReaderOriginalContext::capture(&f.app, f.selection.clone(), &CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(
        unlocked.key_conflicts().len(),
        1,
        "reader capture must not repair conflict state"
    );
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unconfirmed_existing_public_vault_pin_blocks_current_and_new_reader_scope() {
    let f = Fixture::new().await;
    let owner = f.app.session().await.unwrap();
    owner.unlocked_opt().await.unwrap().stop_engine().await;
    let ctx = f.capture().await;
    let mut pin = owner.pinned_vault_key().await.unwrap();
    assert!(!pin.unconfirmed);
    pin.unconfirmed = true;
    // Mutate the actual existing keyfile/database metadata, never supply a
    // fake passing key/owner/authentication object to the Reader.
    owner
        .storage()
        .unwrap()
        .setting_set(crate::key_trust::VAULT_KEY_SETTING, pin.clone())
        .await
        .unwrap();
    owner
        .update_keyfile(|keyfile| keyfile.vault_key = Some(pin))
        .await
        .unwrap();
    assert!(
        owner.api().unwrap().me().await.is_ok(),
        "account auth remains valid"
    );
    assert!(ctx.check(&f.app).await.is_err());
    assert!(
        ReaderOriginalContext::capture(&f.app, f.selection.clone(), &CancellationToken::new())
            .await
            .is_err()
    );
    assert!(
        owner.pinned_vault_key().await.unwrap().unconfirmed,
        "no automatic key confirmation"
    );
    f.app.shutdown().await.unwrap();
}
