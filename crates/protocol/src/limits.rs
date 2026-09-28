//! Hard limits shared by client and server. The server enforces them and
//! answers `413`/`400`; the client checks them before sending.

/// XChaCha20-Poly1305 nonce length.
pub const NONCE_LEN: usize = 24;
/// Poly1305 tag length.
pub const TAG_LEN: usize = 16;
/// Symmetric key length (VRK, KEK, DEK).
pub const KEY_LEN: usize = 32;
/// Wrapped DEK = encrypt(KEK, DEK) = 32 bytes key + 16 bytes tag.
pub const WRAPPED_DEK_LEN: usize = KEY_LEN + TAG_LEN;
/// Encrypted VRK inside an envelope = 32 + 16.
pub const ENVELOPE_CIPHERTEXT_LEN: usize = KEY_LEN + TAG_LEN;
/// X25519 public key length.
pub const X25519_PUBLIC_KEY_LEN: usize = 32;
/// Ed25519 public key length.
pub const ED25519_PUBLIC_KEY_LEN: usize = 32;
/// Ed25519 signature length.
pub const ED25519_SIGNATURE_LEN: usize = 64;
/// Vault access key length (see ADR-0004).
pub const VAULT_ACCESS_KEY_LEN: usize = 32;
/// Argon2id salt length used by password envelopes.
pub const ARGON2_SALT_LEN: usize = 16;

/// Maximum ciphertext size of a single vault object (1 MiB). Larger payloads
/// (session logs, attachments) go to the future encrypted-blob store.
pub const MAX_OBJECT_CIPHERTEXT_BYTES: usize = 1024 * 1024;
/// Maximum number of mutations in one `POST /v1/sync/push`.
pub const MAX_PUSH_BATCH: usize = 500;
/// Maximum total request body for push (batch × object + overhead).
pub const MAX_PUSH_BODY_BYTES: usize = 16 * 1024 * 1024;
/// Default / maximum page size for `changes` and `snapshot`.
pub const DEFAULT_PAGE_LIMIT: u32 = 500;
pub const MAX_PAGE_LIMIT: u32 = 1000;

/// Account password length bounds (UTF-8 bytes).
pub const MIN_ACCOUNT_PASSWORD_LEN: usize = 12;
pub const MAX_ACCOUNT_PASSWORD_LEN: usize = 1024;
/// Device display name max length (characters).
pub const MAX_DEVICE_NAME_LEN: usize = 128;
/// Allowed clock skew for signed approvals.
pub const MAX_SIGNATURE_SKEW_SECONDS: i64 = 600;
/// Allowed clock skew of per-request device proofs (protocol 1.5).
pub const MAX_REQUEST_PROOF_SKEW_SECONDS: i64 = 120;
