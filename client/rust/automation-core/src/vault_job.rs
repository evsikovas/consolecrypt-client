//! Exact signed execution metadata. These serializable facts grant nothing;
//! Native explicit selection and the original dedicated owner remain authority.
use crate::{AutomationError, Result, VaultInputBinding, VaultValidationRuntime};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Omitted entirely for ordinary jobs. No credential identifier, endpoint,
/// filesystem path, secret value or arbitrary purpose/command is represented.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultRunBinding {
    version: u8,
    scope_id: Uuid,
    operation_id: Uuid,
    reference_sha256: String,
    environment_digest: String,
    helper_digest: String,
}
impl VaultRunBinding {
    pub fn from_native(
        reference: &VaultInputBinding,
        runtime: &VaultValidationRuntime,
        scope: Uuid,
        operation: Uuid,
    ) -> Result<Self> {
        runtime.require_fresh()?;
        let value = Self {
            version: 1,
            scope_id: scope,
            operation_id: operation,
            reference_sha256: reference.reference_sha256().into(),
            environment_digest: runtime.environment_digest().into(),
            helper_digest: runtime.helper_digest().into(),
        };
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<()> {
        if self.version != 1
            || self.scope_id.is_nil()
            || self.operation_id.is_nil()
            || [
                &self.reference_sha256,
                &self.environment_digest,
                &self.helper_digest,
            ]
            .iter()
            .any(|value| !cc_models::automation::is_digest(value))
        {
            return Err(AutomationError::Invalid(
                "unsupported Vault execution binding",
            ));
        }
        Ok(())
    }
    pub fn scope_id(&self) -> Uuid {
        self.scope_id
    }
    pub fn operation_id(&self) -> Uuid {
        self.operation_id
    }
    pub fn reference_sha256(&self) -> &str {
        &self.reference_sha256
    }
    pub fn environment_digest(&self) -> &str {
        &self.environment_digest
    }
    pub fn helper_digest(&self) -> &str {
        &self.helper_digest
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RunParameters;
    use serde_json::json;
    #[test]
    fn ordinary_parameters_keep_exact_legacy_wire_without_vault_null_or_metadata() {
        let value = json!({"extra_vars":{},"tags":[],"skip_tags":[],"diff":false});
        let parameters: RunParameters = serde_json::from_value(value.clone()).unwrap();
        assert!(parameters.vault.is_none());
        assert_eq!(serde_json::to_value(parameters).unwrap(), value);
    }
    #[test]
    fn signed_vault_metadata_is_closed_bounded_and_contains_no_path_or_credential_authority() {
        let value = json!({"version":1,"scope_id":Uuid::new_v4(),"operation_id":Uuid::new_v4(),
            "reference_sha256":"a".repeat(64),"environment_digest":"b".repeat(64),"helper_digest":"c".repeat(64)});
        let binding: VaultRunBinding = serde_json::from_value(value.clone()).unwrap();
        binding.validate().unwrap();
        for field in ["credential_id", "password", "path", "approved", "command"] {
            let mut bad = value.clone();
            bad[field] = json!("synthetic");
            assert!(serde_json::from_value::<VaultRunBinding>(bad).is_err());
        }
        for bad in [json!(true), json!(2)] {
            let mut future = value.clone();
            future["version"] = bad;
            match serde_json::from_value::<VaultRunBinding>(future) {
                Err(_) => (),
                Ok(value) => assert!(value.validate().is_err()),
            }
        }
    }
}
