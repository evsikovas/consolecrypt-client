//! Secret key types and the ADR-0002 key hierarchy.
//!
//! Every secret key lives in a heap-allocated [`SecretBox`] (so moving the
//! wrapper never copies key bytes around the stack), is zeroized on drop and
//! has a redacted `Debug`. Raw bytes are only reachable inside this crate,
//! except for the vault access key, which is a server-facing credential by
//! design.

use crate::error::Result;
use crate::kdf::hkdf_sha256_32;
use crate::rng::fill_random;
use cc_models::KekClass;
use cc_protocol::canonical::labels;
use cc_protocol::limits::KEY_LEN;
use cc_protocol::{Bytes, VaultId};
use secrecy::{ExposeSecret, ExposeSecretMut, SecretBox};
use sha2::{Digest, Sha256};
use std::fmt;

/// A boxed, zeroize-on-drop 32-byte secret.
pub(crate) type Secret32 = SecretBox<[u8; KEY_LEN]>;

/// Allocate a zeroed secret and fill it from the OS CSPRNG in place.
pub(crate) fn random_secret32() -> Result<Secret32> {
    let mut s = Secret32::init_with_mut(|_| {});
    fill_random(s.expose_secret_mut())?;
    Ok(s)
}

/// Copy `bytes` into a fresh secret box. The caller remains responsible for
/// zeroizing its own copy.
pub(crate) fn secret32_from(bytes: &[u8; KEY_LEN]) -> Secret32 {
    Secret32::init_with_mut(|s| s.copy_from_slice(bytes))
}

macro_rules! secret_key32 {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        pub struct $name(Secret32);

        impl $name {
            /// Wrap an existing secret (crate-internal).
            #[allow(dead_code)]
            pub(crate) fn from_secret(secret: Secret32) -> Self {
                Self(secret)
            }

            /// Copy raw bytes into a new key (crate-internal; tests/vectors
            /// and decryption results).
            #[allow(dead_code)]
            pub(crate) fn from_bytes(bytes: &[u8; KEY_LEN]) -> Self {
                Self(secret32_from(bytes))
            }

            /// Raw key bytes (crate-internal).
            #[allow(dead_code)]
            pub(crate) fn expose(&self) -> &[u8; KEY_LEN] {
                self.0.expose_secret()
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($name), "(<redacted>)"))
            }
        }
    };
}

secret_key32!(
    /// Vault Root Key: 32 random bytes, never derived from a password.
    /// Everything in a vault is ultimately protected by it.
    Vrk
);
secret_key32!(
    /// Key-encryption key of one [`KekClass`], `HKDF(VRK, vault_id, label)`.
    /// Wraps per-revision DEKs.
    Kek
);
secret_key32!(
    /// Data-encryption key: fresh random key per object revision.
    Dek
);
secret_key32!(
    /// Vault access key: `HKDF(VRK, vault_id, "…/vault-access-key")`. A
    /// server-verifiable proof of VRK knowledge that reveals nothing about
    /// the VRK. The server stores only `SHA-256(VAK)`.
    Vak
);

impl Vrk {
    /// Generate a fresh VRK from the OS CSPRNG.
    pub fn generate() -> Result<Self> {
        random_secret32().map(Self)
    }
}

impl Dek {
    /// Generate a fresh DEK from the OS CSPRNG.
    pub fn generate() -> Result<Self> {
        random_secret32().map(Self)
    }
}

impl Vak {
    /// The key as protocol bytes, for `CreateVaultRequest`,
    /// `AttestDeviceRequest`, `ReplaceEnvelopeRequest`, … .
    ///
    /// The returned DTO buffer is not zeroized; drop it as soon as the
    /// request is sent.
    pub fn to_protocol_bytes(&self) -> Bytes {
        Bytes::new(self.expose().to_vec())
    }

    /// `SHA-256(VAK)` — what the server stores and compares against.
    pub fn verifier(&self) -> [u8; 32] {
        vault_access_key_verifier(self.expose())
    }
}

/// `SHA-256(vault_access_key)`: the verifier a server stores at vault
/// creation (exposed for mock servers and tests).
pub fn vault_access_key_verifier(vault_access_key: &[u8]) -> [u8; 32] {
    Sha256::digest(vault_access_key).into()
}

/// Generate a fresh Vault Root Key (convenience for [`Vrk::generate`]).
pub fn generate_vrk() -> Result<Vrk> {
    Vrk::generate()
}

/// All keys derived from a VRK for one vault (ADR-0002 §Keys).
///
/// Holds the five KEKs and the VAK — not the VRK itself, so code that only
/// encrypts/decrypts objects never needs the root key.
pub struct KeyHierarchy {
    vault_id: VaultId,
    inventory: Kek,
    secrets: Kek,
    snippets: Kek,
    history: Kek,
    settings: Kek,
    vak: Vak,
}

impl KeyHierarchy {
    /// Derive every KEK and the VAK:
    /// `HKDF-SHA256(ikm = VRK, salt = vault_id (16 raw bytes), info = label)`.
    pub fn derive(vrk: &Vrk, vault_id: VaultId) -> Self {
        let salt = vault_id.as_bytes();
        let kek = |class: KekClass| Kek(hkdf_sha256_32(vrk.expose(), salt, &[class.hkdf_label()]));
        Self {
            vault_id,
            inventory: kek(KekClass::Inventory),
            secrets: kek(KekClass::Secrets),
            snippets: kek(KekClass::Snippets),
            history: kek(KekClass::History),
            settings: kek(KekClass::Settings),
            vak: Vak(hkdf_sha256_32(
                vrk.expose(),
                salt,
                &[labels::VAULT_ACCESS_KEY],
            )),
        }
    }

    /// The vault these keys belong to.
    pub fn vault_id(&self) -> VaultId {
        self.vault_id
    }

    /// KEK for `class`.
    pub fn kek(&self, class: KekClass) -> &Kek {
        match class {
            KekClass::Inventory => &self.inventory,
            KekClass::Secrets => &self.secrets,
            KekClass::Snippets => &self.snippets,
            KekClass::History => &self.history,
            KekClass::Settings => &self.settings,
        }
    }

    /// Vault access key.
    pub fn vault_access_key(&self) -> &Vak {
        &self.vak
    }
}

impl fmt::Debug for KeyHierarchy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyHierarchy")
            .field("vault_id", &self.vault_id)
            .field("keys", &"<redacted>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_is_redacted() {
        let vrk = Vrk::from_bytes(&[0xAB; 32]);
        let h = KeyHierarchy::derive(&vrk, VaultId::NIL);
        for s in [
            format!("{vrk:?}"),
            format!("{h:?}"),
            format!("{:?}", h.kek(KekClass::Secrets)),
            format!("{:?}", h.vault_access_key()),
            format!("{:?}", Dek::generate().unwrap()),
        ] {
            assert!(s.contains("redacted"), "{s}");
            assert!(!s.to_lowercase().contains("ab, ab"), "{s}");
            assert!(!s.contains("171"), "{s}");
        }
    }

    #[test]
    fn keks_are_distinct_and_bound_to_vault() {
        let vrk = Vrk::generate().unwrap();
        let a = KeyHierarchy::derive(&vrk, VaultId::new());
        let b = KeyHierarchy::derive(&vrk, VaultId::new());
        let mut seen = std::collections::HashSet::new();
        for c in KekClass::ALL {
            assert!(seen.insert(*a.kek(c).expose()));
            assert_ne!(a.kek(c).expose(), b.kek(c).expose());
        }
        assert!(seen.insert(*a.vault_access_key().expose()));
        assert_ne!(
            a.vault_access_key().verifier(),
            b.vault_access_key().verifier()
        );
    }
}
