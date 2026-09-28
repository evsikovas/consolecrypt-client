//! In-process mock of the ConsoleCrypt sync server (feature `mock-server`).
//!
//! An axum server with in-memory state implementing the protocol v1 DTOs
//! and the normative server rules the client depends on:
//!
//! * auth: register / login (global device ids; keys must match; revoked
//!   device → `403 device_revoked`), refresh with rotation, single-use
//!   refresh tokens and reuse detection (session family revoked), each
//!   refresh invalidating the previous access token; logout, password
//!   reset/change, email verification;
//! * devices: list, trust request (empty vault list → all vaults), rename,
//!   Ed25519-verified approval, reject, attestation with the SHA-256(VAK)
//!   check, revocation (sessions, envelopes, sockets);
//! * vaults & envelopes: create (existing id → `409 already_exists`), list,
//!   get, delete, filtered envelope listing for untrusted devices,
//!   create/delete envelopes, recovery material and envelope replacement;
//! * sync: push with per-vault gap-free sequences, revision checks, `409`
//!   with per-mutation results, idempotent replays by `mutation_id`;
//!   paged `changes` (tombstones) and `snapshot` (live objects);
//! * WebSocket events with close codes 4001 (revoked) / 4002 (lagged);
//! * protocol 1.5 request proofs (`x-cc-device-proof`) on authenticated
//!   requests, refresh and the WebSocket upgrade: verified against the
//!   session's device key (skew, single-use nonce per device, signature over
//!   the exact target and body); required or accept-and-meter;
//! * ADR-0004 authorization (A / T(V) / K(V), IDOR → 404).
//!
//! Test hooks: network partition and lost responses through a loopback TCP
//! proxy, fault injection per path, state inspection, admin revocation,
//! restore drills (vault checkpoints, restore from a checkpoint with or
//! without epoch rotation, epoch rotation alone).
//!
//! Intended for tests (this crate, other crates, `cc` CLI E2E). Methods
//! that bind sockets panic on failure, like other test servers.

mod proxy;
mod routes;
mod state;

pub use state::MockObject;

use cc_protocol::version::ProtocolVersion;
use cc_protocol::{DeviceId, ErrorCode, ObjectId, UserId, VaultId, PROTOCOL_VERSION};
use state::{Outgoing, State};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use url::Url;

/// Mock server settings.
#[derive(Debug, Clone)]
pub struct MockServerConfig {
    pub access_token_ttl: chrono::Duration,
    pub refresh_token_ttl: chrono::Duration,
    pub registration_open: bool,
    pub email_verification_required: bool,
    /// Oldest client protocol accepted (older → `426`).
    pub minimum_protocol: ProtocolVersion,
    /// Protocol 1.5 rollout: `false` = accept-and-meter (a present proof is
    /// verified, a missing one is only counted), `true` = required
    /// (`422 invalid_proof` / `missing`).
    pub require_request_proof: bool,
}

impl Default for MockServerConfig {
    fn default() -> Self {
        Self {
            access_token_ttl: chrono::Duration::minutes(15),
            refresh_token_ttl: chrono::Duration::days(30),
            registration_open: true,
            email_verification_required: false,
            minimum_protocol: ProtocolVersion::new(PROTOCOL_VERSION.major, 0),
            require_request_proof: false,
        }
    }
}

/// Request-proof counters (like the real server's
/// `cc_request_proofs_total{result}` metric).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RequestProofStats {
    /// Proofs that verified.
    pub valid: u64,
    /// Requests subject to a proof that carried none.
    pub missing: u64,
    /// Proofs rejected (malformed, stale, replayed, invalid signature).
    pub rejected: u64,
}

/// An injected fault for requests whose path starts with a prefix.
#[derive(Clone)]
pub enum FaultAction {
    /// Answer with this error without running the handler.
    Error(ErrorCode),
    /// `429` with `retry_after_seconds` (and `Retry-After`).
    RateLimit { retry_after_seconds: u32 },
    /// Run the handler (state is committed), then drop the connection
    /// before the response is delivered.
    LoseResponse,
    /// Run the handler, then rewrite the JSON response body.
    Rewrite(Arc<dyn Fn(&mut serde_json::Value) + Send + Sync>),
    /// Run the handler, then replace the body with truncated JSON.
    InvalidJson,
}

impl std::fmt::Debug for FaultAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FaultAction::Error(c) => write!(f, "Error({c:?})"),
            FaultAction::RateLimit {
                retry_after_seconds,
            } => {
                write!(f, "RateLimit({retry_after_seconds}s)")
            }
            FaultAction::LoseResponse => f.write_str("LoseResponse"),
            FaultAction::Rewrite(_) => f.write_str("Rewrite(..)"),
            FaultAction::InvalidJson => f.write_str("InvalidJson"),
        }
    }
}

struct Fault {
    path_prefix: String,
    remaining: u32,
    action: FaultAction,
}

pub(crate) struct Shared {
    state: Mutex<State>,
    pub(crate) events: broadcast::Sender<Outgoing>,
    faults: Mutex<Vec<Fault>>,
    pub(crate) proxy: Arc<proxy::Proxy>,
    hits: Mutex<HashMap<String, usize>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Shared {
    pub(crate) fn lock(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    fn record(&self, path: &str) {
        *lock(&self.hits).entry(path.to_owned()).or_default() += 1;
    }

    fn take_fault(&self, path: &str) -> Option<FaultAction> {
        let mut faults = lock(&self.faults);
        let f = faults
            .iter_mut()
            .find(|f| f.remaining > 0 && path.starts_with(&f.path_prefix))?;
        f.remaining -= 1;
        let action = f.action.clone();
        faults.retain(|f| f.remaining > 0);
        Some(action)
    }
}

/// A running mock server. Dropping it stops the server.
pub struct MockServer {
    shared: Arc<Shared>,
    url: Url,
    tasks: Vec<JoinHandle<()>>,
}

impl std::fmt::Debug for MockServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockServer")
            .field("url", &self.url.as_str())
            .finish_non_exhaustive()
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

impl MockServer {
    /// Start with default settings on an ephemeral loopback port.
    ///
    /// # Panics
    /// If a loopback socket cannot be bound.
    pub async fn start() -> Self {
        Self::start_with(MockServerConfig::default()).await
    }

    /// Start with custom settings.
    ///
    /// # Panics
    /// If a loopback socket cannot be bound.
    pub async fn start_with(config: MockServerConfig) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let upstream = listener.local_addr().expect("mock server address");
        let (proxy, proxy_task) = proxy::Proxy::start(upstream)
            .await
            .expect("bind mock proxy");
        let (events, _) = broadcast::channel(1024);
        let shared = Arc::new(Shared {
            state: Mutex::new(State::new(config, events.clone())),
            events,
            faults: Mutex::new(Vec::new()),
            proxy: proxy.clone(),
            hits: Mutex::new(HashMap::new()),
        });
        let app = routes::router(shared.clone());
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let url = Url::parse(&format!("http://{}/", proxy.addr)).expect("mock url");
        Self {
            shared,
            url,
            tasks: vec![server, proxy_task],
        }
    }

    /// Base URL for clients (through the partition proxy).
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Simulate a network partition: refuse new connections and drop
    /// existing ones (HTTP and WebSocket) while `true`.
    pub fn set_offline(&self, offline: bool) {
        self.shared.proxy.set_offline(offline);
    }

    /// Drop all current client connections once.
    pub fn kill_connections(&self) {
        self.shared.proxy.kill_all();
    }

    /// Inject `action` for the next `times` requests whose path starts with
    /// `path_prefix` (e.g. `cc_protocol::paths::SYNC_PUSH`).
    pub fn inject(&self, path_prefix: &str, times: u32, action: FaultAction) {
        lock(&self.shared.faults).push(Fault {
            path_prefix: path_prefix.to_owned(),
            remaining: times,
            action,
        });
    }

    /// Remove all pending faults.
    pub fn clear_faults(&self) {
        lock(&self.shared.faults).clear();
    }

    /// Number of requests received for exactly `path` (the filled path, e.g.
    /// `/v1/sync/push`).
    pub fn hits(&self, path: &str) -> usize {
        lock(&self.shared.hits).get(path).copied().unwrap_or(0)
    }

    /// Latest server state of an object.
    pub fn object(&self, vault_id: VaultId, object_id: ObjectId) -> Option<MockObject> {
        self.shared.lock().object(vault_id, object_id)
    }

    /// Latest allocated sequence of a vault.
    pub fn latest_sequence(&self, vault_id: VaultId) -> i64 {
        self.shared.lock().latest_sequence(vault_id)
    }

    /// Ids of live (non-deleted) objects, sorted.
    pub fn live_objects(&self, vault_id: VaultId) -> Vec<ObjectId> {
        self.shared.lock().live_objects(vault_id)
    }

    /// Unused email tokens (verification / password reset) of an account —
    /// what the real server would email.
    pub fn email_tokens(&self, email: &str) -> Vec<String> {
        self.shared.lock().email_tokens(email)
    }

    /// Account id of an email.
    pub fn user_id(&self, email: &str) -> Option<UserId> {
        self.shared.lock().user_id(email)
    }

    /// Whether the device holds a non-revoked device envelope for the vault.
    pub fn device_trusted(&self, device_id: DeviceId, vault_id: VaultId) -> bool {
        self.shared.lock().device_trusted(device_id, vault_id)
    }

    /// Revoke a device as if from another trusted device (admin hook).
    pub fn revoke_device(&self, device_id: DeviceId) -> bool {
        self.shared.lock().revoke_device_internal(device_id)
    }

    /// Take a "backup" of the vault's sync state (objects, idempotency
    /// records, sequence). Returns the sequence it was taken at.
    ///
    /// # Panics
    /// If the vault does not exist.
    pub fn checkpoint(&self, vault_id: VaultId) -> i64 {
        self.shared
            .lock()
            .checkpoint(vault_id)
            .expect("checkpoint: unknown vault")
    }

    /// Restore the vault from the checkpoint taken at `to_sequence` and
    /// rotate its epoch — what an operator does after restoring the server
    /// from a backup (`admin rotate-epoch`). Mutations after the checkpoint
    /// are forgotten; `vault_changed` is published with the restored
    /// sequence.
    ///
    /// # Panics
    /// If there is no checkpoint at `to_sequence`.
    pub fn simulate_restore(&self, vault_id: VaultId, to_sequence: i64) {
        self.shared
            .lock()
            .restore(vault_id, Some(to_sequence), true)
            .expect("simulate_restore: no checkpoint at that sequence");
    }

    /// Like [`Self::simulate_restore`] but the operator forgot to rotate the
    /// epoch (clients must detect the rollback from sequences alone).
    ///
    /// # Panics
    /// If there is no checkpoint at `to_sequence`.
    pub fn simulate_restore_keep_epoch(&self, vault_id: VaultId, to_sequence: i64) {
        self.shared
            .lock()
            .restore(vault_id, Some(to_sequence), false)
            .expect("simulate_restore_keep_epoch: no checkpoint at that sequence");
    }

    /// Restore the most recent checkpoint (rotating the epoch). Returns its
    /// sequence.
    ///
    /// # Panics
    /// If the vault has no checkpoint.
    pub fn restore_checkpoint(&self, vault_id: VaultId) -> i64 {
        self.shared
            .lock()
            .restore(vault_id, None, true)
            .expect("restore_checkpoint: no checkpoint")
    }

    /// Rotate only the vault epoch (no data change). Returns the new epoch.
    ///
    /// # Panics
    /// If the vault does not exist.
    pub fn rotate_epoch(&self, vault_id: VaultId) -> uuid::Uuid {
        self.shared
            .lock()
            .rotate_epoch(vault_id)
            .expect("rotate_epoch: unknown vault")
    }

    /// Current epoch of a vault.
    pub fn epoch(&self, vault_id: VaultId) -> Option<uuid::Uuid> {
        self.shared.lock().epoch(vault_id)
    }

    /// Request-proof counters so far.
    pub fn request_proof_stats(&self) -> RequestProofStats {
        self.shared.lock().proof_stats
    }

    /// Expire every access token (forces refresh on the next request).
    pub fn expire_access_tokens(&self) {
        self.shared.lock().expire_access_tokens();
    }

    /// Close every WebSocket with `code` (e.g. 4002 = lagged).
    pub fn close_websockets(&self, code: u16) {
        let _ = self.shared.events.send(Outgoing {
            audience: state::Audience::User(UserId::NIL),
            event: cc_protocol::events::ServerEvent::Unknown,
            close_device: None,
            close_sessions: Vec::new(),
            force_close: Some(code),
        });
    }
}
