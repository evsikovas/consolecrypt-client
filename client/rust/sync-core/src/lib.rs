//! # cc-sync-core — ConsoleCrypt sync client
//!
//! * [`ApiClient`] — typed HTTP client for every `cc_protocol` endpoint
//!   (self-hosted server URL, protocol headers, token refresh via a
//!   caller-provided [`TokenStore`], structured [`ApiError`]s).
//! * [`ObjectStore`] — per-vault local mutations (encrypt → objects + outbox
//!   in one transaction) and decrypting reads; works in local-only profiles.
//! * [`SyncEngine`] — ADR-0003 client algorithm: outbox push in batches with
//!   stable mutation ids, paged pull/snapshot applied transactionally,
//!   conflict resolution per the ADR policy table, backoff with jitter,
//!   offline detection, status and change-event streams, and recovery from
//!   a server restored from an older backup (protocol 1.3 epoch; ADR-0103
//!   addendum).
//! * [`EventStream`] — WebSocket `/v1/events/ws` client; feed it into
//!   [`SyncEngine::attach_events`].
//! * [`enable_sync`] / [`disconnect`] — ADR-0106 profile transitions.
//! * `mock` (feature `mock-server`) — in-process mock of the sync server.
//!
//! Encryption is injected through [`ObjectCodec`]; this crate never sees
//! keys and never stores plaintext.
//!
//! ## Wiring (app-core)
//!
//! ```no_run
//! # async fn demo(codec: std::sync::Arc<dyn cc_sync_core::ObjectCodec>,
//! #               tokens: std::sync::Arc<dyn cc_sync_core::TokenStore>,
//! #               signer: std::sync::Arc<dyn cc_sync_core::RequestSigner>,
//! #               storage: cc_storage_core::Storage,
//! #               vault_id: cc_protocol::VaultId,
//! #               device_id: cc_protocol::DeviceId) -> Result<(), cc_sync_core::SyncError> {
//! use cc_sync_core::*;
//! let api = ApiClient::new(
//!     ApiConfig::new("https://sync.example.org", "0.1.0", cc_protocol::version::Platform::Macos)?,
//!     tokens,
//! )?
//! // Protocol 1.5: sign every authenticated request with the device key.
//! .with_request_signer(signer);
//! let store = ObjectStore::new(vault_id, storage, codec);
//! let engine = SyncEngine::new(store, api.clone(), SyncEngineConfig::new(device_id)).await?;
//! let events = EventStream::spawn(api, EventStreamConfig::default());
//! engine.attach_events(events.subscribe());
//! engine.start();
//! # Ok(()) }
//! ```

pub mod api;
mod attach;
mod backoff;
mod codec;
mod engine;
mod error;
mod events;
mod store;
mod ws;

#[cfg(feature = "mock-server")]
pub mod mock;

/// Re-export of the storage crate whose types appear in this API.
pub use cc_storage_core as storage;

pub use api::{
    ApiClient, ApiConfig, ApiError, MemoryTokenStore, ProofRejection, RequestSigner, SignerError,
    TokenStore, TokenStoreError,
};
pub use attach::{
    disconnect, enable_sync, DisconnectOptions, DisconnectOutcome, EnableSync, EnableSyncOutcome,
    EnableSyncPath,
};
pub use backoff::BackoffConfig;
pub use cc_storage_core::RollbackReason;
pub use codec::{CodecError, ObjectCodec};
pub use engine::{
    make_conflict_copy, payload_updated_at, strategy_for, ConflictStrategy, SyncEngine,
    SyncEngineConfig, SyncPhase, SyncReport, SyncStatus, CONFLICT_COPY_SUFFIX,
};
pub use error::{StopReason, SyncError};
pub use events::{ChangeOrigin, ConflictResolution, SyncEvent};
pub use store::{DecryptedObject, ObjectStore};
pub use ws::{
    EventStream, EventStreamConfig, WsEvent, CLOSE_CODE_LAGGED, CLOSE_CODE_REVOKED,
    CLOSE_CODE_TOKEN_EXPIRED,
};

/// Tracing filter directives every binary linking sync-core MUST append to its
/// `EnvFilter` (after any user-supplied directives, so they win), even in
/// verbose/trace mode: third-party HTTP/WebSocket/TLS stacks log request
/// headers — including bearer tokens — at debug/trace level.
pub const SAFE_LOG_DIRECTIVES: &[&str] = &[
    "tungstenite=off",
    "tokio_tungstenite=off",
    "hyper=warn",
    "hyper_util=warn",
    "reqwest=warn",
    "h2=warn",
    "rustls=warn",
];
