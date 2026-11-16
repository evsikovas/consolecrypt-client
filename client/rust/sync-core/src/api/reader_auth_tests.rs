//! Draft component regressions. No production subject is fabricated by these
//! private mutation tests; actual /me capture is exercised in integration tests.
use super::*;
use crate::{ApiConfig, TokenStore, TokenStoreError};
use async_trait::async_trait;
use cc_protocol::auth::{SecretString, TokenPair};
use cc_protocol::version::Platform;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::Notify;

struct PausedStore {
    value: Mutex<Option<TokenPair>>,
    writes: Mutex<Vec<&'static str>>,
    saves: AtomicUsize,
    pause_first: bool,
    fail_save: AtomicBool,
    entered: Notify,
    release: Notify,
}
impl PausedStore {
    fn new(pause_first: bool) -> Arc<Self> {
        Arc::new(Self {
            value: Mutex::new(None),
            writes: Mutex::new(vec![]),
            saves: AtomicUsize::new(0),
            pause_first,
            fail_save: AtomicBool::new(false),
            entered: Notify::new(),
            release: Notify::new(),
        })
    }
}
#[async_trait]
impl TokenStore for PausedStore {
    async fn load(&self) -> Result<Option<TokenPair>, TokenStoreError> {
        Ok(self.value.lock().unwrap().clone())
    }
    async fn save(&self, tokens: &TokenPair) -> Result<(), TokenStoreError> {
        let n = self.saves.fetch_add(1, Ordering::SeqCst);
        if self.pause_first && n == 0 {
            self.entered.notify_one();
            self.release.notified().await;
        }
        if self.fail_save.load(Ordering::SeqCst) {
            return Err(TokenStoreError("injected".into()));
        }
        *self.value.lock().unwrap() = Some(tokens.clone());
        self.writes.lock().unwrap().push("save");
        Ok(())
    }
    async fn clear(&self) -> Result<(), TokenStoreError> {
        *self.value.lock().unwrap() = None;
        self.writes.lock().unwrap().push("clear");
        Ok(())
    }
}
fn api(store: Arc<PausedStore>) -> ApiClient {
    ApiClient::new(
        ApiConfig::new("https://reader.invalid/base/", "test", Platform::Cli).unwrap(),
        store,
    )
    .unwrap()
}
fn tokens() -> TokenPair {
    TokenPair {
        session_id: SessionId::new(),
        access_token: SecretString::new("test access"),
        refresh_token: SecretString::new("test refresh"),
        access_expires_at: chrono::Utc::now() + chrono::Duration::minutes(5),
        refresh_expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
    }
}

#[test]
fn nonmutating_read_ticket_preserves_generation_but_rejects_invalidation_barrier_and_auth_change() {
    let s = ReaderAuthState::new();
    let ticket = s.read_ticket().unwrap();
    assert!(!ticket.publishes_reader());
    s.check_read_ticket(&ticket).unwrap();
    let another = s.read_ticket().unwrap();
    assert_eq!(ticket.generation, another.generation);
    assert_eq!(ticket.operation, another.operation);
    s.invalidate_observers();
    assert!(s.check_read_ticket(&ticket).is_err());
    let next = s.read_ticket().unwrap();
    let barrier = s.barrier().unwrap();
    assert!(s.read_ticket().is_err());
    assert!(s.check_read_ticket(&next).is_err());
    drop(barrier);
    assert!(
        s.check_read_ticket(&next).is_err(),
        "no old-ticket ABA after barrier drop"
    );
    let next = s.read_ticket().unwrap();
    let _pending = s.begin().unwrap();
    assert!(s.read_ticket().is_err());
    assert!(s.check_read_ticket(&next).is_err());
    let foreign = ReaderAuthState::new();
    assert!(foreign.check_read_ticket(&ticket).is_err());
}

#[tokio::test]
async fn late_network_ticket_cannot_write_after_clear_entry() {
    let store = PausedStore::new(false);
    let api = api(store.clone());
    let old_login = api.inner.reader_auth.begin().unwrap();
    api.clear_tokens().await.unwrap();
    assert!(api.store_tokens(&tokens(), &old_login).await.is_err());
    assert_eq!(*store.writes.lock().unwrap(), ["clear"]);
    assert!(store.value.lock().unwrap().is_none());
}
#[tokio::test]
async fn clear_during_save_erases_stale_disk_write_and_waiter_abort_does_not_cancel_completion() {
    let store = PausedStore::new(true);
    let api = api(store.clone());
    let t = tokens();
    let writer_api = api.clone();
    let writer = tokio::spawn(async move { writer_api.set_tokens(&t).await });
    store.entered.notified().await;
    let clear_ticket = api.inner.reader_auth.begin().unwrap(); // Clear ENTRY, before writer lock.
    let clear_api = api.clone();
    let clear =
        tokio::spawn(async move { clear_api.clear_tokens_with_ticket(&clear_ticket).await });
    writer.abort(); // The already-spawned OS completion must continue.
    store.release.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(2), clear)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(*store.writes.lock().unwrap(), ["save", "clear", "clear"]);
    assert!(store.value.lock().unwrap().is_none());
    assert!(!api.is_authenticated().await.unwrap());
}
#[tokio::test]
async fn failed_save_and_dropped_auth_operation_never_certify_reader_generation() {
    let store = PausedStore::new(false);
    let api = api(store.clone());
    api.set_tokens(&tokens()).await.unwrap();
    let old = api.inner.reader_auth.ticket().unwrap();
    store.fail_save.store(true, Ordering::SeqCst);
    assert!(api.set_tokens(&tokens()).await.is_err());
    assert!(
        api.inner
            .reader_auth
            .current(old.generation, || ())
            .is_err()
    );
    let now = api.inner.reader_auth.ticket().unwrap();
    assert!(
        api.inner
            .reader_auth
            .current(now.generation, || ())
            .is_err()
    );
    api.inner.reader_auth.begin().unwrap(); // Auth caller dropped before response.
    let dropped = api.inner.reader_auth.ticket().unwrap();
    assert!(
        api.inner
            .reader_auth
            .current(dropped.generation, || ())
            .is_err()
    );
}
#[tokio::test]
async fn latest_explicit_login_wins_queued_old_clear_without_old_ticket_revival() {
    let store = PausedStore::new(false);
    let api = api(store.clone());
    let old_clear = api.inner.reader_auth.begin().unwrap();
    let latest = tokens();
    api.set_tokens(&latest).await.unwrap();
    assert!(api.clear_tokens_with_ticket(&old_clear).await.is_err());
    assert_eq!(
        store.value.lock().unwrap().as_ref().unwrap().session_id,
        latest.session_id
    );
    assert_eq!(*store.writes.lock().unwrap(), ["save"]);
}
#[test]
fn generation_overflow_is_permanently_unavailable_and_no_aba() {
    let state = ReaderAuthState::new();
    state.state.lock().unwrap().generation = Some(u64::MAX);
    assert!(state.begin().is_err());
    assert!(state.ticket().is_err());
    assert!(state.begin().is_err());
}
#[tokio::test]
async fn subject_expiry_and_original_client_drop_wake_without_event_or_refresh() {
    let store = PausedStore::new(false);
    let api = api(store);
    api.set_tokens(&tokens()).await.unwrap();
    let generation = api.inner.reader_auth.ticket().unwrap().generation;
    let mut subject = ReaderAuthenticatedSubject {
        client: Arc::downgrade(&api.inner),
        state: api.inner.reader_auth.clone(),
        generation,
        user: UserId::new(),
        device: DeviceId::new(),
        session: SessionId::new(),
        server: api.config().server_url.clone(),
        expires_at: chrono::Utc::now() + chrono::Duration::milliseconds(10),
    };
    tokio::time::timeout(std::time::Duration::from_secs(1), subject.cancelled())
        .await
        .unwrap();
    assert!(subject.check_current(&api).is_err());
    subject.expires_at = chrono::Utc::now() + chrono::Duration::hours(1);
    drop(api);
    tokio::time::timeout(std::time::Duration::from_secs(1), subject.cancelled())
        .await
        .unwrap();
}

#[tokio::test]
async fn stale_refresh_noop_does_not_leave_a_phantom_pending_mutation() {
    let store = PausedStore::new(false);
    let api = api(store.clone());
    api.set_tokens(&tokens()).await.unwrap();
    let old_generation = api.inner.reader_auth.ticket().unwrap().generation;
    api.refresh_after_rejection("already-rotated-old-access-token")
        .await
        .unwrap();
    assert_eq!(store.saves.load(Ordering::SeqCst), 1); // No refresh/store happened.
    assert!(
        api.inner
            .reader_auth
            .current(old_generation, || ())
            .is_err()
    );
    assert!(api.inner.reader_auth.capture_ticket().is_ok()); // New explicit me may run.
}

#[tokio::test]
async fn nested_logout_refresh_save_cannot_clear_the_outer_pending_lifetime() {
    let store = PausedStore::new(false);
    let api = api(store);
    api.set_tokens(&tokens()).await.unwrap();
    let outer = api.inner.reader_auth.barrier().unwrap();
    let logout = api.inner.reader_auth.begin().unwrap().without_reader();
    api.store_tokens(&tokens(), &logout).await.unwrap(); // Actual nested refresh publication path.
    assert!(api.inner.reader_auth.capture_ticket().is_err());
    api.clear_tokens_with_ticket(&logout).await.unwrap();
    assert!(api.inner.reader_auth.capture_ticket().is_err()); // Outer logout not done yet.
    drop(outer);
    assert!(api.inner.reader_auth.capture_ticket().is_ok());
    assert!(api.capture_reader_auth().await.is_err()); // Tokens really cleared.
}

#[tokio::test]
async fn setter_observer_invalidation_allows_fresh_capture_but_not_unfinished_auth() {
    let store = PausedStore::new(false);
    let api = api(store);
    api.set_tokens(&tokens()).await.unwrap();
    let prior = api.inner.reader_auth.ticket().unwrap().generation;
    api.invalidate_reader_auth(); // Same primitive used by actual Session setters.
    assert!(api.inner.reader_auth.current(prior, || ()).is_err());
    assert!(api.inner.reader_auth.capture_ticket().is_ok());
    let _unfinished_login = api.inner.reader_auth.begin().unwrap();
    api.invalidate_reader_auth();
    assert!(api.inner.reader_auth.capture_ticket().is_err()); // No setter shortcut over pending login.
}
