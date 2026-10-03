//! Direct RDP transport. Passwords are released only after an explicitly pinned TLS handshake.
mod limits;
mod manager;
mod tls;
mod transport;
mod types;

pub use manager::RdpManager;
pub use types::*;

mod clipboard;
mod clipboard_offers;
mod directory;
mod permissions;
pub use permissions::{DirectoryGrant, FolderStatus, SessionCapabilities, SessionPermissions};
