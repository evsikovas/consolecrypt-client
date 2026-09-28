//! Strongly typed identifiers.
//!
//! All IDs are UUIDs serialized as canonical hyphenated lowercase strings.
//! Client-generated IDs (vault, object, device, mutation) SHOULD be UUIDv7 so
//! they sort roughly by creation time; the server accepts any UUID version.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            /// Generate a new time-ordered (v7) identifier.
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            pub const fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }

            /// Raw 16 bytes, used when binding IDs into AEAD associated data
            /// or canonical signed messages.
            pub const fn as_bytes(&self) -> &[u8; 16] {
                self.0.as_bytes()
            }

            pub const NIL: Self = Self(Uuid::nil());
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), self.0)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s).map(Self)
            }
        }

        impl From<Uuid> for $name {
            fn from(value: Uuid) -> Self {
                Self(value)
            }
        }

        impl From<$name> for Uuid {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

define_id!(
    /// Account identity. Server-generated.
    UserId
);
define_id!(
    /// A client installation. Client-generated at first launch, stable for
    /// the lifetime of the installation.
    DeviceId
);
define_id!(
    /// A Vault. Client-generated so it can be bound into AEAD associated data
    /// before the vault exists on the server.
    VaultId
);
define_id!(
    /// An encrypted Vault object. Client-generated.
    ObjectId
);
define_id!(
    /// Idempotency key of a single sync mutation. Client-generated; retries of
    /// the same logical change MUST reuse the same id.
    MutationId
);
define_id!(
    /// A stored key envelope. Server-generated.
    EnvelopeId
);
define_id!(
    /// A login session (refresh-token family). Server-generated.
    SessionId
);
define_id!(
    /// A pending "trust this new device" request. Server-generated.
    DeviceRequestId
);
define_id!(
    /// Per-request correlation id (echoed in `x-request-id` and in errors).
    RequestId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_roundtrips_as_plain_uuid_string() {
        let id = VaultId::new();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{}\"", id.0));
        let back: VaultId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
        assert_eq!(VaultId::from_str(&id.to_string()).unwrap(), id);
    }

    #[test]
    fn new_ids_are_v7() {
        assert_eq!(ObjectId::new().0.get_version_num(), 7);
    }
}
