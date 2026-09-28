//! # cc-protocol — ConsoleCrypt wire contract
//!
//! Versioned DTOs shared by the ConsoleCrypt client and the self-hosted
//! Cloud/Sync server. Everything that crosses the network between them is
//! defined here.
//!
//! Hard rules for this crate (see `docs/adr/ADR-0001-*` and `PROTOCOL_CHANGELOG.md`):
//!
//! * No cryptography implementations and no key material. Ciphertexts,
//!   nonces and envelopes are carried as opaque [`Bytes`].
//! * No server secrets, no client secrets.
//! * Changes are additive within a major version. Every change is recorded in
//!   `PROTOCOL_CHANGELOG.md` and bumps [`version::PROTOCOL_VERSION`].
//! * The server never needs to understand the plaintext domain model
//!   (`cc-models`); this crate must not depend on it.

pub mod auth;
pub mod bytes;
pub mod canonical;
pub mod devices;
pub mod envelopes;
pub mod error;
pub mod events;
pub mod ids;
pub mod limits;
pub mod meta;
pub mod paths;
pub mod recovery;
pub mod sync;
pub mod vaults;
pub mod version;

pub use bytes::Bytes;
pub use error::{ApiError, ErrorCode};
pub use ids::*;
pub use version::{ProtocolVersion, PROTOCOL_VERSION};

/// Timestamp type used across the protocol (RFC 3339, UTC).
pub type Timestamp = chrono::DateTime<chrono::Utc>;
