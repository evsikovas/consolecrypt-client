//! # cc-ssh-core — native SSH backend
//!
//! * [`ConnectionPlanner`] → [`ConnectionPlan`]: group inheritance, jump
//!   chains, proxies, per-hop credentials and host-key policies. The UI never
//!   assembles SSH parameters itself (CLIENT_SPEC §6.2).
//! * [`SshConnector`] (russh): direct and unlimited multi-hop connections via
//!   `direct-tcpip`, optional SOCKS5 / HTTP CONNECT proxy to the first hop,
//!   password / keyboard-interactive / public key / certificate / agent
//!   authentication, host-key verification ([`HostKeyPolicy`]), keepalive,
//!   exec, PTY shells, `direct-tcpip`, remote forwards, subsystems (SFTP).
//! * [`keys`]: Ed25519 / RSA generation, OpenSSH (encrypted) key import,
//!   public keys, certificates, fingerprints. Keys are decrypted in memory
//!   only; stored passphrase protection is never removed.
//! * [`known_hosts`]: `~/.ssh/known_hosts` parsing/import incl. hashed names,
//!   `@cert-authority`, `@revoked`; OpenSSH pattern matching.
//! * [`openssh`]: system OpenSSH fallback (command line with `ProxyJump`,
//!   `IdentityAgent` pointing at the built-in agent — never a temp
//!   `IdentityFile`) and [`ssh_config`] import of `~/.ssh/config`.
//!
//! Secret material reaches this crate only through [`CredentialResolver`]
//! and is kept in [`secrecy::SecretString`] wrappers.

pub mod agent;
pub mod client;
pub mod error;
pub mod keys;
pub mod known_hosts;
pub mod memory;
pub mod openssh;
pub mod planner;
pub mod proxy;
pub mod ssh_config;
pub mod traits;

#[cfg(feature = "test-harness")]
pub mod testing;

pub use client::{
    ConnectOptions, ExecOutput, ForwardedTcpip, PtyRequest, RemoteForward, SessionEvent,
    ShellChannel, ShellEvent, ShellInput, ShellReader, ShellRemote, ShellWriter, SshConnector,
    SshSession, SshStream,
};
pub use error::SshError;
pub use memory::{FixedHostKeyPrompt, MemoryCredentialResolver, MemoryInventory, MemoryKnownHosts};
pub use planner::{ConnectionPlan, ConnectionPlanner, Endpoint, Hop, PlanError, PlannerDefaults};
pub use traits::{
    CredentialResolver, HostKeyDecision, HostKeyInfo, HostKeyPrompt, InventoryLookup,
    KnownHostsError, KnownHostsStore, ResolveError, ResolvedCredential,
};

pub use cc_models::host::{HostKeyPolicy, SshBackend};
/// Re-export so dependants use the exact russh/ssh-key versions of this crate.
pub use russh;
pub use secrecy;
