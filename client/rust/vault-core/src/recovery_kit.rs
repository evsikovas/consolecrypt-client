//! Recovery Kit (ADR-0002 §Recovery envelope): what the user prints or
//! saves — vault ID, 24-word phrase, QR payload, server URL, creation date —
//! and the mandatory onboarding check (re-enter 3 random words).

use crate::error::Result;
use cc_crypto_core::{random_below, RecoveryKey, RecoveryPhrase, RECOVERY_WORD_COUNT};
use cc_protocol::{Timestamp, VaultId};
use std::fmt;
use zeroize::Zeroizing;

/// Number of words the user must re-enter during onboarding.
pub const RECOVERY_CHECK_WORDS: usize = 3;

/// Printable Recovery Kit. Shown once; the Recovery Key itself is never
/// stored by the client. `Debug` redacts the phrase and QR payload.
pub struct RecoveryKit {
    vault_id: VaultId,
    phrase: RecoveryPhrase,
    qr_payload: Zeroizing<String>,
    server_url: Option<String>,
    created_at: Timestamp,
}

impl RecoveryKit {
    pub(crate) fn new(
        vault_id: VaultId,
        rk: &RecoveryKey,
        server_url: Option<&str>,
        created_at: Timestamp,
    ) -> Self {
        Self {
            vault_id,
            phrase: rk.to_mnemonic(),
            qr_payload: rk.to_qr_payload(vault_id),
            server_url: server_url.map(str::to_owned),
            created_at,
        }
    }

    /// Vault this kit recovers.
    pub fn vault_id(&self) -> VaultId {
        self.vault_id
    }

    /// The 24 words (show / print; never log).
    pub fn phrase(&self) -> &RecoveryPhrase {
        &self.phrase
    }

    /// QR payload `consolecrypt-recovery:v1:<vault_id>:<key>` to render as
    /// a QR code (never log).
    pub fn expose_qr_payload(&self) -> &str {
        &self.qr_payload
    }

    /// Server URL of a synced profile; `None` for local-only profiles.
    pub fn server_url(&self) -> Option<&str> {
        self.server_url.as_deref()
    }

    /// When the kit was generated.
    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }

    /// Start the onboarding check with 3 random distinct word positions.
    pub fn start_check(&self) -> Result<RecoveryKitCheck> {
        let mut positions = [0usize; RECOVERY_CHECK_WORDS];
        let mut filled = 0;
        while filled < RECOVERY_CHECK_WORDS {
            // RECOVERY_WORD_COUNT (24) fits in u32; result is in 1..=24.
            let p = random_below(RECOVERY_WORD_COUNT as u32)? as usize + 1;
            if !positions[..filled].contains(&p) {
                positions[filled] = p;
                filled += 1;
            }
        }
        Ok(RecoveryKitCheck::with_positions(self, positions))
    }
}

impl fmt::Debug for RecoveryKit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecoveryKit")
            .field("vault_id", &self.vault_id)
            .field("phrase", &"<redacted>")
            .field("qr_payload", &"<redacted>")
            .field("server_url", &self.server_url)
            .field("created_at", &self.created_at)
            .finish()
    }
}

/// Onboarding verification: the user must type the words at
/// [`RecoveryKitCheck::positions`] before continuing.
pub struct RecoveryKitCheck {
    positions: [usize; RECOVERY_CHECK_WORDS],
    expected: [Zeroizing<String>; RECOVERY_CHECK_WORDS],
}

impl RecoveryKitCheck {
    pub(crate) fn with_positions(
        kit: &RecoveryKit,
        mut positions: [usize; RECOVERY_CHECK_WORDS],
    ) -> Self {
        positions.sort_unstable();
        let expected =
            positions.map(|p| Zeroizing::new(kit.phrase.word(p).unwrap_or_default().to_owned()));
        Self {
            positions,
            expected,
        }
    }

    /// 1-based word positions to ask for, ascending ("word #3, #11, #20").
    pub fn positions(&self) -> [usize; RECOVERY_CHECK_WORDS] {
        self.positions
    }

    /// Positions whose answer is wrong (answers in the order of
    /// [`RecoveryKitCheck::positions`]; trimmed, case-insensitive). Empty ⇒ passed.
    pub fn wrong_positions(&self, answers: [&str; RECOVERY_CHECK_WORDS]) -> Vec<usize> {
        self.positions
            .iter()
            .zip(self.expected.iter())
            .zip(answers)
            .filter(|((_, expected), answer)| {
                let answer = Zeroizing::new(answer.trim().to_lowercase());
                answer.as_str() != expected.as_str()
            })
            .map(|((p, _), _)| *p)
            .collect()
    }

    /// True if all three answers are correct.
    pub fn verify(&self, answers: [&str; RECOVERY_CHECK_WORDS]) -> bool {
        self.wrong_positions(answers).is_empty()
    }
}

impl fmt::Debug for RecoveryKitCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecoveryKitCheck")
            .field("positions", &self.positions)
            .field("expected", &"<redacted>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kit() -> RecoveryKit {
        RecoveryKit::new(
            VaultId::new(),
            &RecoveryKey::generate().unwrap(),
            Some("https://sync.example.org"),
            chrono::Utc::now(),
        )
    }

    #[test]
    fn check_positions_distinct_and_in_range() {
        let k = kit();
        for _ in 0..50 {
            let c = k.start_check().unwrap();
            let p = c.positions();
            assert!(p.iter().all(|x| (1..=24).contains(x)));
            assert!(p[0] < p[1] && p[1] < p[2]);
        }
    }

    #[test]
    fn check_verifies_answers() {
        let k = kit();
        let c = RecoveryKitCheck::with_positions(&k, [20, 3, 11]);
        assert_eq!(c.positions(), [3, 11, 20]);
        let w = |p| k.phrase().word(p).unwrap().to_owned();
        let (a, b, d) = (w(3), w(11), w(20));
        assert!(c.verify([&a, &b, &d]));
        assert!(c.verify([&format!(" {} ", a.to_uppercase()), &b, &d]));
        assert_eq!(c.wrong_positions([&a, "wrong", &d]), vec![11]);
        assert!(!c.verify([&b, &a, &d]) || a == b);
    }

    #[test]
    fn debug_redacts() {
        let k = kit();
        let c = k.start_check().unwrap();
        let s = format!("{k:?} {c:?}");
        for word in k.phrase().words() {
            assert!(!s.contains(&format!(" {word} ")), "{word}");
        }
        assert!(!s.contains(k.expose_qr_payload()));
        assert!(s.contains("sync.example.org"));
    }
}
