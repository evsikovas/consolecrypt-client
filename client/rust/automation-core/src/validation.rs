//! Native-owned inputs are checked independently before an opaque review can
//! become a signed job. A helper's `valid: true` alone is never sufficient.
mod declared_privileges;
use declared_privileges::DeclaredPrivileges;

use crate::{AutomationError, InputDigestWitness, Result};
use cc_models::automation::{ApprovedTarget, is_digest};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunParameters {
    pub extra_vars: BTreeMap<String, Value>,
    pub tags: Vec<String>,
    pub skip_tags: Vec<String>,
    pub diff: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vault: Option<crate::VaultRunBinding>,
}

impl std::fmt::Debug for RunParameters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunParameters")
            .field("variable_count", &self.extra_vars.len())
            .field("tag_count", &self.tags.len())
            .field("diff", &self.diff)
            .finish_non_exhaustive()
    }
}

impl RunParameters {
    pub fn validate(&self) -> Result<()> {
        if let Some(binding) = &self.vault {
            binding.validate()?;
            if self.diff {
                return Err(AutomationError::Invalid("Vault jobs require diff disabled"));
            }
        }
        let variables = serde_json::to_vec(&self.extra_vars)
            .map_err(|_| AutomationError::Invalid("invalid variables"))?;
        if variables.len() > 64 * 1024 || self.extra_vars.len() > 256 {
            return Err(AutomationError::Invalid(
                "execution variable limit exceeded",
            ));
        }
        crate::json::parse(&variables)?;
        for name in self.extra_vars.keys() {
            if name.is_empty()
                || name.len() > 128
                || name.starts_with("ansible_")
                || name.starts_with("consolecrypt_")
                || !name
                    .bytes()
                    .next()
                    .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                return Err(AutomationError::Invalid(
                    "reserved or invalid execution variable",
                ));
            }
        }
        for tags in [&self.tags, &self.skip_tags] {
            if tags.len() > 64
                || tags.iter().collect::<HashSet<_>>().len() != tags.len()
                || tags.iter().any(|tag| {
                    tag.is_empty()
                        || tag.len() > 128
                        || tag.starts_with('-')
                        || !tag
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
                })
            {
                return Err(AutomationError::Invalid("invalid execution tag list"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationCheck {
    pub name: String,
    pub exit_code: i32,
    pub output: String,
    pub truncated: bool,
    pub cancelled: bool,
    pub timed_out: bool,
    pub elapsed_ms: u64,
}

impl std::fmt::Debug for ValidationCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ValidationCheck")
            .field("name", &self.name)
            .field("exit_code", &self.exit_code)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Witnesses {
    parameters: InputDigestWitness,
    inventory: InputDigestWitness,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationProof {
    revision_digest: String,
    environment_digest: String,
    playbook: String,
    parameters_digest: String,
    inventory_digest: String,
    targets_digest: String,
    valid: bool,
    checks: Vec<ValidationCheck>,
    input_witnesses: Witnesses,
    #[serde(default)]
    declared_privileges: Option<Value>,
}

impl std::fmt::Debug for ValidationProof {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ValidationProof")
            .field("revision", &self.revision_digest)
            .field("valid", &self.valid)
            .finish_non_exhaustive()
    }
}

/// Contains no canonical witnesses or complete variable values. Only this
/// verified result may be kept with the native review's frozen target sessions.
#[derive(Clone, Serialize)]
pub struct VerifiedValidation {
    revision_digest: String,
    environment_digest: String,
    playbook: String,
    parameters_digest: String,
    inventory_digest: String,
    targets_digest: String,
    checks: Vec<ValidationCheck>,
    valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics_policy: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    declared_privileges: Option<DeclaredPrivileges>,
}

impl std::fmt::Debug for VerifiedValidation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifiedValidation")
            .field("revision", &self.revision_digest)
            .field("valid", &self.valid)
            .finish_non_exhaustive()
    }
}

impl VerifiedValidation {
    /// Trusted Native marker, never a guest flag. The attached binding was
    /// independently captured for an explicitly selected Vault reference. Mark
    /// before exposing/storing any report that used OR could use that phase.
    /// Raw decrypted YAML values can differ from the password: omit ALL raw
    /// check output, not a guessed password-only detector. Local raw diagnostics
    /// require a separate explicit future scope and are not retained here.
    pub fn with_vault_context(mut self, marker: &crate::VaultValidationMarker) -> Result<Self> {
        if self.revision_digest != marker.revision
            || self.environment_digest != marker.environment
            || self.playbook != marker.playbook
        {
            return Err(AutomationError::Invalid("Vault validation context changed"));
        }
        for check in &mut self.checks {
            let raw = std::mem::replace(
                &mut check.output,
                "Vault-sensitive diagnostics withheld".into(),
            );
            drop(cc_models::secret::SecretValue::new(raw));
        }
        self.declared_privileges = None;
        self.diagnostics_policy = Some("vault_sensitive_withheld");
        Ok(self)
    }
    /// Automatic AI/repair projection: fixed phase/status/limits only. Never
    /// includes raw diagnostics, decrypted vars, project paths or content.
    pub fn ai_summary(&self) -> Value {
        serde_json::json!({"kind":"automation_validation_summary","valid":self.valid,
            "diagnostics_policy":self.diagnostics_policy.unwrap_or("summary_only"),
            "checks":self.checks.iter().map(|check|serde_json::json!({"phase":check.name,
                "passed":check.exit_code==0&&!check.cancelled&&!check.timed_out&&!check.truncated,
                "cancelled":check.cancelled,"timed_out":check.timed_out,"truncated":check.truncated})).collect::<Vec<_>>()})
    }
    /// Guest advisory only. Never feeds validity, approval or effective rights.
    pub fn declared_privileges(&self) -> Option<Value> {
        self.declared_privileges
            .as_ref()
            .and_then(|summary| serde_json::to_value(summary).ok())
    }
    pub fn revision_digest(&self) -> &str {
        &self.revision_digest
    }
    pub fn environment_digest(&self) -> &str {
        &self.environment_digest
    }
    pub fn playbook(&self) -> &str {
        &self.playbook
    }
    pub fn parameters_digest(&self) -> &str {
        &self.parameters_digest
    }
    pub fn inventory_digest(&self) -> &str {
        &self.inventory_digest
    }
    pub fn targets_digest(&self) -> &str {
        &self.targets_digest
    }
    pub fn checks(&self) -> &[ValidationCheck] {
        &self.checks
    }
    pub fn is_valid(&self) -> bool {
        self.valid
    }
}

impl ValidationProof {
    pub fn verify(
        self,
        revision: &str,
        environment: &str,
        playbook: &str,
        parameters: &RunParameters,
        inventory: &Value,
        targets: &[ApprovedTarget],
    ) -> Result<VerifiedValidation> {
        parameters.validate()?;
        let parameters = serde_json::to_value(parameters)
            .map_err(|_| AutomationError::Invalid("invalid parameters"))?;
        let target_value = serde_json::to_value(targets)
            .map_err(|_| AutomationError::Invalid("invalid targets"))?;
        let target_bytes = serde_json::to_vec(&crate::project::canonical(target_value))
            .map_err(|_| AutomationError::Invalid("invalid targets"))?;
        if targets.is_empty()
            || targets.len() > 10000
            || !is_digest(revision)
            || !is_digest(environment)
            || self.revision_digest != revision
            || self.environment_digest != environment
            || self.playbook != playbook
            || self.targets_digest != hex::encode(Sha256::digest(target_bytes))
            || self.parameters_digest != self.input_witnesses.parameters.digest
            || self.inventory_digest != self.input_witnesses.inventory.digest
            || self.checks.is_empty()
            || self.checks.len() > 2
        {
            return Err(AutomationError::Protocol(
                "validation does not belong to reviewed inputs",
            ));
        }
        self.input_witnesses.parameters.verify(&parameters)?;
        self.input_witnesses.inventory.verify_inventory(inventory)?;
        for (index, check) in self.checks.iter().enumerate() {
            if check.name != ["syntax", "lint"][index] || check.output.len() > 128 * 1024 {
                return Err(AutomationError::Protocol("invalid validation diagnostics"));
            }
        }
        let passed = self.checks.len() == 2
            && self.checks.iter().all(|check| {
                check.exit_code == 0 && !check.truncated && !check.cancelled && !check.timed_out
            });
        if self.valid != passed {
            return Err(AutomationError::Protocol("inconsistent validation outcome"));
        }
        let declared_privileges = if self.checks.first().is_some_and(|check| {
            check.exit_code == 0 && !check.cancelled && !check.timed_out && !check.truncated
        }) {
            self.declared_privileges.and_then(DeclaredPrivileges::parse)
        } else {
            None
        };
        Ok(VerifiedValidation {
            revision_digest: self.revision_digest,
            environment_digest: self.environment_digest,
            playbook: self.playbook,
            parameters_digest: self.parameters_digest,
            inventory_digest: self.inventory_digest,
            targets_digest: self.targets_digest,
            checks: self.checks,
            valid: passed,
            diagnostics_policy: None,
            declared_privileges,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn execution_options_cannot_smuggle_connection_overrides_or_argv() {
        let mut params = RunParameters {
            extra_vars: BTreeMap::new(),
            tags: vec!["configure".into()],
            skip_tags: vec![],
            diff: false,
            vault: None,
        };
        params.validate().unwrap();
        params
            .extra_vars
            .insert("ansible_connection".into(), json!("local"));
        assert!(params.validate().is_err());
        params.extra_vars.clear();
        params.tags = vec!["--extra-vars=anything".into()];
        assert!(params.validate().is_err());
        params.tags = vec!["same".into(), "same".into()];
        assert!(params.validate().is_err());
    }

    #[test]
    fn a_helpers_valid_flag_cannot_substitute_another_revision_target_or_parameters() {
        let parameters = RunParameters {
            extra_vars: BTreeMap::new(),
            tags: vec![],
            skip_tags: vec![],
            diff: false,
            vault: None,
        };
        let inventory = json!({"fixture":"native-owned inventory"});
        let targets: Vec<ApprovedTarget> = serde_json::from_value(json!([{"host_id":uuid::Uuid::new_v4(),
            "alias":"fixture","address":"192.0.2.1","port":22,"username":"deployer","route_digest":"a".repeat(64),"credential_refs":[]}])).unwrap();
        let witness = |value: &Value| {
            let raw = serde_json::to_string(&crate::project::canonical(value.clone())).unwrap();
            json!({"digest":hex::encode(Sha256::digest(raw.as_bytes())),"canonical_json":raw})
        };
        let parameters_witness = witness(&serde_json::to_value(&parameters).unwrap());
        let inventory_witness = witness(&inventory);
        let checks = ["syntax","lint"].map(|name| json!({"name":name,"exit_code":0,"output":"fixture","truncated":false,"cancelled":false,"timed_out":false,"elapsed_ms":1}));
        let target_digest = hex::encode(Sha256::digest(
            serde_json::to_vec(&crate::project::canonical(
                serde_json::to_value(&targets).unwrap(),
            ))
            .unwrap(),
        ));
        let base = json!({"revision_digest":"b".repeat(64),"environment_digest":"c".repeat(64),"playbook":"playbook.yml",
            "parameters_digest":parameters_witness["digest"],"inventory_digest":inventory_witness["digest"],"targets_digest":target_digest,
            "valid":true,"checks":checks,"input_witnesses":{"parameters":parameters_witness,"inventory":inventory_witness}});
        let verify = |value| {
            serde_json::from_value::<ValidationProof>(value)
                .unwrap()
                .verify(
                    &"b".repeat(64),
                    &"c".repeat(64),
                    "playbook.yml",
                    &parameters,
                    &inventory,
                    &targets,
                )
        };
        assert!(verify(base.clone()).unwrap().is_valid());
        for optional in [
            json!({"secret":"PRIVATE"}),
            json!({"version":2}),
            json!(true),
        ] {
            let mut candidate = base.clone();
            candidate["declared_privileges"] = optional;
            let checked = verify(candidate).unwrap();
            assert!(checked.is_valid());
            assert!(checked.declared_privileges().is_none());
        }
        for (pointer, value) in [
            ("/revision_digest", json!("d".repeat(64))),
            ("/environment_digest", json!("d".repeat(64))),
            ("/targets_digest", json!("d".repeat(64))),
            ("/playbook", json!("different.yml")),
            ("/checks/1/exit_code", json!(2)),
            ("/checks/1/truncated", json!(true)),
            (
                "/input_witnesses/parameters/canonical_json",
                json!("{\"extra_vars\":{\"changed\":true}}"),
            ),
        ] {
            let mut changed = base.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            assert!(verify(changed).is_err(), "{pointer}");
        }
    }
}
