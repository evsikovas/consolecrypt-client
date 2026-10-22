//! Native database sessions. Credentials and driver connections remain native;
//! the bridge exchanges only bounded metadata and explicitly requested rows.

pub mod types;
pub use types::*;
pub mod cassandra;
mod cassandra_value;
mod clickhouse;
pub mod details;
pub mod edit;
pub mod export;
pub mod guard;
pub mod import;
pub mod introspect;
mod mongo;
mod mysql;
pub mod parser;
mod postgres;
mod postgres_value;
mod redis_driver;
pub mod session;
pub mod streaming;
pub mod transport;
pub use session::DbSession;

/// Columns whose values are treated as secrets (masked by default, copied with
/// clipboard auto-clear): password, token, secret, API/private keys and the like.
pub fn sensitive_column(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        "password",
        "passwd",
        "pwd",
        "token",
        "secret",
        "api_key",
        "apikey",
        "private_key",
        "credential",
        "salt",
        "otp",
    ]
    .iter()
    .any(|marker| name.contains(marker))
        || name.ends_with("_hash")
        || name == "hash"
}
