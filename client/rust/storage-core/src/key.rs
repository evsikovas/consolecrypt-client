//! Database encryption key.

use std::fmt;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

/// Raw 32-byte SQLCipher key.
///
/// Key management (generation, OS secure storage) is the caller's job; this
/// type only carries the bytes into `PRAGMA key` using SQLCipher's raw-key
/// form, so no additional KDF runs. The bytes are zeroized on drop and never
/// printed.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct DatabaseKey([u8; 32]);

impl DatabaseKey {
    /// Wrap raw key bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Build from a slice; `None` unless exactly 32 bytes.
    pub fn from_slice(bytes: &[u8]) -> Option<Self> {
        <[u8; 32]>::try_from(bytes).ok().map(Self)
    }

    /// Explicit, grep-able access to the key bytes.
    pub fn expose_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// `PRAGMA key = "x'<hex>'";` statement (zeroized after use).
    pub(crate) fn pragma_statement(&self) -> Zeroizing<String> {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        let mut s = Zeroizing::new(String::with_capacity(90));
        s.push_str("PRAGMA key = \"x'");
        for b in self.0.iter() {
            s.push(HEX[(b >> 4) as usize] as char);
            s.push(HEX[(b & 0x0f) as usize] as char);
        }
        s.push_str("'\";");
        s
    }
}

impl fmt::Debug for DatabaseKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DatabaseKey(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_is_redacted_and_pragma_is_raw_hex() {
        let k = DatabaseKey::from_bytes([0xAB; 32]);
        assert_eq!(format!("{k:?}"), "DatabaseKey(<redacted>)");
        let p = k.pragma_statement();
        assert!(p.starts_with("PRAGMA key = \"x'ABAB"));
        assert!(p.ends_with("'\";"));
        assert_eq!(p.len(), "PRAGMA key = \"x'".len() + 64 + 3);
        assert!(DatabaseKey::from_slice(&[0; 31]).is_none());
    }
}
