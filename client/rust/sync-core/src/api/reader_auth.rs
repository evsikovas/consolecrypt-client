//! Local lifetime fence for a subject observed through the original API client.
//! This is not Reader approval, a grant, JWT validation or a server revocation feed.
use super::{ApiClient, ApiError, Inner};
use cc_protocol::{DeviceId, SessionId, UserId};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::watch;
use url::Url;

struct State {
    generation: Option<u64>,
    operation: Option<u64>,
    available: bool,
    pending: bool,
    barriers: usize,
}
pub(super) struct ReaderAuthState {
    state: Mutex<State>,
    changed: watch::Sender<Option<u64>>,
    published_generation: AtomicU64,
    published_available: AtomicBool,
}
#[derive(Clone)]
pub(super) struct AuthTicket {
    state: Arc<ReaderAuthState>,
    generation: u64,
    operation: u64,
    publish_tokens: bool,
}
impl ReaderAuthState {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                generation: Some(0),
                operation: Some(0),
                available: false,
                pending: false,
                barriers: 0,
            }),
            changed: watch::channel(Some(0)).0,
            published_generation: AtomicU64::new(0),
            published_available: AtomicBool::new(false),
        })
    }
    pub(super) fn begin(self: &Arc<Self>) -> Result<AuthTicket, ApiError> {
        let mut s = self.state.lock().map_err(|_| ApiError::NotAuthenticated)?;
        s.available = false;
        s.pending = true;
        s.generation = s.generation.and_then(|v| v.checked_add(1));
        s.operation = s.operation.and_then(|v| v.checked_add(1));
        if s.generation.is_none() || s.operation.is_none() {
            s.generation = None;
            s.operation = None;
        }
        self.changed.send_replace(s.generation);
        self.publish(&s);
        Ok(AuthTicket {
            state: self.clone(),
            generation: s.generation.ok_or(ApiError::NotAuthenticated)?,
            operation: s.operation.ok_or(ApiError::NotAuthenticated)?,
            publish_tokens: true,
        })
    }
    // Read-only cached-auth I/O never begins/finishes an auth mutation and
    // never publishes observer availability. An explicit diagnostic may read
    // cached tokens without an existing /me observation; all in-flight reads
    // are still fenced by generation/operation/pending/barriers throughout.
    pub(super) fn read_ticket(self: &Arc<Self>) -> Result<AuthTicket, ApiError> {
        let s = self.state.lock().map_err(|_| ApiError::NotAuthenticated)?;
        if s.pending || s.barriers != 0 {
            return Err(ApiError::NotAuthenticated);
        }
        Ok(AuthTicket {
            state: self.clone(),
            generation: s.generation.ok_or(ApiError::NotAuthenticated)?,
            operation: s.operation.ok_or(ApiError::NotAuthenticated)?,
            publish_tokens: false,
        })
    }
    pub(super) fn check_read_ticket(&self, ticket: &AuthTicket) -> Result<(), ApiError> {
        let s = self.state.lock().map_err(|_| ApiError::NotAuthenticated)?;
        if !std::ptr::eq(self, Arc::as_ptr(&ticket.state))
            || s.generation != Some(ticket.generation)
            || s.operation != Some(ticket.operation)
            || s.pending
            || s.barriers != 0
        {
            return Err(ApiError::NotAuthenticated);
        }
        Ok(())
    }
    pub(super) fn ticket(self: &Arc<Self>) -> Result<AuthTicket, ApiError> {
        let s = self.state.lock().map_err(|_| ApiError::NotAuthenticated)?;
        Ok(AuthTicket {
            state: self.clone(),
            generation: s.generation.ok_or(ApiError::NotAuthenticated)?,
            operation: s.operation.ok_or(ApiError::NotAuthenticated)?,
            publish_tokens: true,
        })
    }
    pub(super) fn check(&self, ticket: &AuthTicket) -> Result<(), ApiError> {
        self.finish(ticket, None, || ())
    }
    pub(super) fn finish<T>(
        &self,
        ticket: &AuthTicket,
        available: Option<bool>,
        f: impl FnOnce() -> T,
    ) -> Result<T, ApiError> {
        if !std::ptr::eq(self, Arc::as_ptr(&ticket.state)) {
            return Err(ApiError::NotAuthenticated);
        }
        let mut s = self.state.lock().map_err(|_| ApiError::NotAuthenticated)?;
        if s.generation.is_none() || s.operation != Some(ticket.operation) {
            return Err(ApiError::NotAuthenticated);
        }
        let result = f();
        if let Some(value) = available {
            s.pending = false;
            if s.generation == Some(ticket.generation) {
                s.available = value && s.barriers == 0;
            }
        }
        self.publish(&s);
        Ok(result)
    }
    pub(super) fn finish_load<T>(
        &self,
        ticket: &AuthTicket,
        has_tokens: bool,
        f: impl FnOnce() -> T,
    ) -> Result<T, ApiError> {
        let mut s = self.state.lock().map_err(|_| ApiError::NotAuthenticated)?;
        if !std::ptr::eq(self, Arc::as_ptr(&ticket.state))
            || s.generation.is_none()
            || s.operation != Some(ticket.operation)
        {
            return Err(ApiError::NotAuthenticated);
        }
        let result = f();
        if !s.pending && s.barriers == 0 && s.generation == Some(ticket.generation) {
            s.available = has_tokens;
        }
        self.publish(&s);
        Ok(result)
    }
    fn current<T>(&self, generation: u64, f: impl FnOnce() -> T) -> Result<T, ApiError> {
        let s = self.state.lock().map_err(|_| ApiError::NotAuthenticated)?;
        if s.generation != Some(generation) || !s.available || s.barriers != 0 {
            return Err(ApiError::NotAuthenticated);
        }
        let _poison = PanicObserver(self);
        Ok(f())
    }
    fn try_current<T>(&self, generation: u64, f: impl FnOnce() -> T) -> Result<T, ApiError> {
        let s = self
            .state
            .try_lock()
            .map_err(|_| ApiError::NotAuthenticated)?;
        if s.generation != Some(generation) || !s.available || s.barriers != 0 {
            return Err(ApiError::NotAuthenticated);
        }
        let _poison = PanicObserver(self);
        Ok(f())
    }
    fn capture_ticket(self: &Arc<Self>) -> Result<AuthTicket, ApiError> {
        let ticket = self.ticket()?;
        let s = self.state.lock().map_err(|_| ApiError::NotAuthenticated)?;
        if s.pending
            || s.barriers != 0
            || s.generation != Some(ticket.generation)
            || s.operation != Some(ticket.operation)
        {
            return Err(ApiError::NotAuthenticated);
        }
        Ok(ticket)
    }
    fn finish_capture<T>(
        &self,
        ticket: &AuthTicket,
        f: impl FnOnce() -> Result<T, ApiError>,
    ) -> Result<T, ApiError> {
        let mut s = self.state.lock().map_err(|_| ApiError::NotAuthenticated)?;
        if !std::ptr::eq(self, Arc::as_ptr(&ticket.state))
            || s.pending
            || s.barriers != 0
            || s.generation != Some(ticket.generation)
            || s.operation != Some(ticket.operation)
        {
            return Err(ApiError::NotAuthenticated);
        }
        let _poison = PanicObserver(self);
        let result = f()?;
        s.available = true;
        self.publish(&s);
        Ok(result)
    }
    fn publish(&self, state: &State) {
        self.published_available.store(false, Ordering::Release);
        self.published_generation
            .store(state.generation.unwrap_or(0), Ordering::Release);
        self.published_available.store(
            state.available && state.generation.is_some() && state.barriers == 0,
            Ordering::Release,
        );
    }
    fn observed_current(&self, generation: u64) -> Result<(), ApiError> {
        if self.published_generation.load(Ordering::Acquire) != generation
            || !self.published_available.load(Ordering::Acquire)
            || self.published_generation.load(Ordering::Acquire) != generation
        {
            return Err(ApiError::NotAuthenticated);
        }
        Ok(())
    }
    /// Queueing a refresh cancels Reader observations immediately, without
    /// superseding an earlier legitimate single-flight token refresh write.
    pub(super) fn invalidate_observers(&self) {
        if let Ok(mut s) = self.state.lock() {
            s.available = false;
            s.generation = s.generation.and_then(|v| v.checked_add(1));
            if s.generation.is_none() {
                s.operation = None;
            }
            self.changed.send_replace(s.generation);
            self.publish(&s);
        }
    }
    pub(super) fn barrier(self: &Arc<Self>) -> Result<AuthBarrier, ApiError> {
        let mut s = self.state.lock().map_err(|_| ApiError::NotAuthenticated)?;
        s.available = false;
        s.generation = s.generation.and_then(|v| v.checked_add(1));
        let Some(count) = s.barriers.checked_add(1) else {
            s.generation = None;
            s.operation = None;
            s.pending = true;
            self.publish(&s);
            self.changed.send_replace(None);
            return Err(ApiError::NotAuthenticated);
        };
        s.barriers = count;
        if s.generation.is_none() {
            s.operation = None;
        }
        self.publish(&s);
        self.changed.send_replace(s.generation);
        s.generation.ok_or(ApiError::NotAuthenticated)?;
        Ok(AuthBarrier(self.clone()))
    }
}
pub(super) struct AuthBarrier(Arc<ReaderAuthState>);
impl Drop for AuthBarrier {
    fn drop(&mut self) {
        if let Ok(mut s) = self.0.state.lock() {
            s.barriers -= 1;
            // Releasing a barrier never certifies a subject. A new explicit
            // capture must run actual /me after all pending mutation settles.
            self.0.publish(&s);
        }
    }
}
struct PanicObserver<'a>(&'a ReaderAuthState);
impl Drop for PanicObserver<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.published_available.store(false, Ordering::Release);
            self.0.changed.send_replace(None);
        }
    }
}
impl AuthTicket {
    pub(super) fn without_reader(mut self) -> Self {
        self.publish_tokens = false;
        self
    }
    pub(super) fn publishes_reader(&self) -> bool {
        self.publish_tokens
    }
}

/// A real `/auth/me` observation, bound to one original client and generation.
/// No tokens, serialization, generic authority bit or mutable public fields.
pub struct ReaderAuthenticatedSubject {
    client: Weak<Inner>,
    state: Arc<ReaderAuthState>,
    generation: u64,
    user: UserId,
    device: DeviceId,
    session: SessionId,
    server: Url,
    expires_at: cc_protocol::Timestamp,
}
impl std::fmt::Debug for ReaderAuthenticatedSubject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReaderAuthenticatedSubject([redacted])")
    }
}
impl ReaderAuthenticatedSubject {
    pub fn user_id(&self) -> UserId {
        self.user
    }
    pub fn device_id(&self) -> DeviceId {
        self.device
    }
    pub fn session_id(&self) -> SessionId {
        self.session
    }
    /// Canonical original ApiConfig URL, including any deployment base path.
    pub fn server_url(&self) -> &str {
        self.server.as_str()
    }
    pub fn check_current(&self, api: &ApiClient) -> Result<(), ApiError> {
        if !Weak::ptr_eq(&self.client, &Arc::downgrade(&api.inner))
            || self.server != api.config().server_url
            || chrono::Utc::now() >= self.expires_at
        {
            return Err(ApiError::NotAuthenticated);
        }
        self.state.observed_current(self.generation)
    }
    /// Short synchronous local fence. Never run network, callbacks that acquire
    /// API auth locks, token operations or async/provider I/O in this closure.
    pub fn with_current<T>(&self, api: &ApiClient, f: impl FnOnce() -> T) -> Result<T, ApiError> {
        if !Weak::ptr_eq(&self.client, &Arc::downgrade(&api.inner))
            || self.server != api.config().server_url
        {
            return Err(ApiError::NotAuthenticated);
        }
        self.state.current(self.generation, || {
            if chrono::Utc::now() >= self.expires_at {
                return Err(ApiError::NotAuthenticated);
            }
            let result = f();
            if chrono::Utc::now() >= self.expires_at {
                return Err(ApiError::NotAuthenticated);
            }
            Ok(result)
        })?
    }
    /// Nonblocking version of the same original-subject fence. A queued SQL
    /// worker must fail closed rather than wait for a lock holder awaiting SQL.
    pub fn try_with_current<T>(
        &self,
        api: &ApiClient,
        f: impl FnOnce() -> T,
    ) -> Result<T, ApiError> {
        if !Weak::ptr_eq(&self.client, &Arc::downgrade(&api.inner))
            || self.server != api.config().server_url
        {
            return Err(ApiError::NotAuthenticated);
        }
        self.state.try_current(self.generation, || {
            if chrono::Utc::now() >= self.expires_at {
                return Err(ApiError::NotAuthenticated);
            }
            let result = f();
            if chrono::Utc::now() >= self.expires_at {
                return Err(ApiError::NotAuthenticated);
            }
            Ok(result)
        })?
    }
    pub async fn cancelled(&self) {
        let mut rx = self.state.changed.subscribe();
        loop {
            if self.client.strong_count() == 0
                || self.state.observed_current(self.generation).is_err()
            {
                return;
            }
            let Ok(left) = (self.expires_at - chrono::Utc::now()).to_std() else {
                return;
            };
            if left.is_zero() {
                return;
            }
            // Bounded sleep avoids Instant overflow for malformed far-future
            // persisted timestamps; original Reader scope owns OS rollback.
            tokio::select! {
                result = rx.changed() => if result.is_err() { return; },
                _ = tokio::time::sleep(left.min(std::time::Duration::from_secs(60))) => {},
            }
        }
    }
}
impl ApiClient {
    /// Invalidates pending Reader work synchronously, before an AppCore auth
    /// transition awaits. It neither logs out nor authenticates a new subject.
    pub fn invalidate_reader_auth(&self) {
        self.inner.reader_auth.invalidate_observers();
    }
    pub async fn capture_reader_auth(&self) -> Result<ReaderAuthenticatedSubject, ApiError> {
        self.cached_tokens()
            .await?
            .ok_or(ApiError::NotAuthenticated)?;
        let ticket = self.inner.reader_auth.capture_ticket()?;
        let me = self.me().await?;
        if me.user_id == UserId::NIL
            || me.current_device_id == DeviceId::NIL
            || me.current_session_id == SessionId::NIL
        {
            return Err(ApiError::NotAuthenticated);
        }
        self.inner.reader_auth.finish_capture(&ticket, || {
            let cache = self.lock_cache();
            let tokens = cache.tokens.as_ref().ok_or(ApiError::NotAuthenticated)?;
            if tokens.session_id != me.current_session_id
                || chrono::Utc::now() >= tokens.access_expires_at.min(tokens.refresh_expires_at)
            {
                return Err(ApiError::NotAuthenticated);
            }
            Ok(ReaderAuthenticatedSubject {
                client: Arc::downgrade(&self.inner),
                state: self.inner.reader_auth.clone(),
                generation: ticket.generation,
                user: me.user_id,
                device: me.current_device_id,
                session: me.current_session_id,
                server: self.config().server_url.clone(),
                expires_at: tokens.access_expires_at.min(tokens.refresh_expires_at),
            })
        })
    }
}

#[cfg(test)]
#[path = "reader_auth_tests.rs"]
mod tests;
