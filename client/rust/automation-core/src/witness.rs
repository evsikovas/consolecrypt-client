//! Bind helper digests to native-owned input data without assuming identical
//! JSON float formatting across Rust and Python. Witnesses stay private and
//! must not be included in diagnostics, logs or public review DTOs.
use crate::{AutomationError, Result, MAX_FRAME_BYTES};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputDigestWitness {
    pub digest: String,
    pub canonical_json: String,
}

impl std::fmt::Debug for InputDigestWitness {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InputDigestWitness")
            .field("digest", &self.digest)
            .finish_non_exhaustive()
    }
}

impl InputDigestWitness {
    pub fn verify(&self, expected: &Value) -> Result<()> {
        self.verify_bounded(expected, MAX_FRAME_BYTES)
    }
    pub fn verify_inventory(&self, expected: &Value) -> Result<()> {
        self.verify_bounded(expected, 5 * 1024 * 1024)
    }
    fn verify_bounded(&self, expected: &Value, max_bytes: usize) -> Result<()> {
        if self.canonical_json.len() > max_bytes || !cc_models::automation::is_digest(&self.digest)
        {
            return Err(AutomationError::Protocol("invalid input digest witness"));
        }
        let decoded = crate::json::parse(self.canonical_json.as_bytes())?;
        if &decoded != expected
            || hex::encode(Sha256::digest(self.canonical_json.as_bytes())) != self.digest
        {
            return Err(AutomationError::Protocol(
                "helper input digest differs from reviewed data",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn python_float_format_can_differ_without_changing_authorized_values() {
        let expected = json!({"threshold":1e-7,"large":1e20,"nested":[false,"кириллица",null]});
        let raw = r#"{"large":1e+20,"nested":[false,"кириллица",null],"threshold":1e-07}"#;
        assert_ne!(serde_json::to_string(&expected).unwrap(), raw);
        let witness = InputDigestWitness {
            digest: hex::encode(Sha256::digest(raw.as_bytes())),
            canonical_json: raw.into(),
        };
        witness.verify(&expected).unwrap();
        assert!(witness
            .verify(&json!({"threshold":2e-7,"large":1e20,"nested":[false,"кириллица",null]}))
            .is_err());
        assert!(!format!("{witness:?}").contains("threshold"));
    }
    #[test]
    fn duplicate_fields_nonfinite_values_or_unbound_hashes_fail_closed() {
        for raw in [
            r#"{"value":1,"value":2}"#,
            r#"{"value":NaN}"#,
            r#"{"value":2}"#,
        ] {
            let witness = InputDigestWitness {
                digest: "a".repeat(64),
                canonical_json: raw.into(),
            };
            assert!(witness.verify(&json!({"value":1})).is_err());
        }
    }
}
