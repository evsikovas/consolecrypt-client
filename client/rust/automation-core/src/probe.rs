//! Validate the untrusted helper response before showing it or using its
//! environment identity. Reported capabilities never grant native authority.
use crate::{AutomationError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

const PACKAGES: [(&str, &str); 5] = [
    ("ansible-core", "2.20.11"),
    ("ansible-runner", "2.4.3"),
    ("ansible-lint", "26.11.0"),
    ("cryptography", "50.0.2"),
    ("PyYAML", "6.0.3"),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbePackage {
    pub expected: String,
    pub actual: Option<String>,
    pub compatible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeSandbox {
    pub filesystem: bool,
    pub network: bool,
    pub pid: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentProbe {
    pub helper: String,
    pub protocol: u32,
    pub python: String,
    pub system: String,
    pub machine: String,
    pub environment_digest: String,
    pub non_root: bool,
    pub wsl: bool,
    pub sandbox: ProbeSandbox,
    pub packages: BTreeMap<String, ProbePackage>,
    pub tools: BTreeMap<String, bool>,
    pub capabilities: BTreeMap<String, bool>,
    pub dependencies_ready: bool,
}

impl EnvironmentProbe {
    /// Compatibility with fixed trusted worker startup. A reported bit is NOT
    /// runtime attestation, target authority or permission to execute code.
    pub fn require_trusted_worker_bootstrap(&self) -> Result<()> {
        if self.capabilities.get("trusted_worker_bootstrap_v1") != Some(&true) {
            return Err(AutomationError::Invalid(
                "update the selected Runner to use trusted worker bootstrap",
            ));
        }
        Ok(())
    }

    pub fn require_target_match(&self) -> Result<()> {
        if self.capabilities.get("target_match_v1") != Some(&true) {
            return Err(AutomationError::Invalid(
                "update the selected Runner to validate playbook target matching",
            ));
        }
        Ok(())
    }
    /// Optional project semantics must never silently degrade on an older
    /// protocol-v1 helper. This is compatibility checking, not authorization.
    pub fn require_project_capabilities(&self, revision: &crate::ProjectRevision) -> Result<()> {
        for (path, capability) in [
            (".consolecrypt-inputs.json", "declared_inputs_v1"),
            (".consolecrypt-dependencies.json", "locked_dependencies_v1"),
            (".consolecrypt-requirements.json", "project_requirements_v1"),
        ] {
            if revision
                .entries
                .iter()
                .any(|entry| entry.path().eq_ignore_ascii_case(path))
                && self.capabilities.get(capability) != Some(&true)
            {
                return Err(AutomationError::Invalid(
                    "update the selected Runner to validate this project's contracts",
                ));
            }
        }
        Ok(())
    }
    pub fn parse(value: Value) -> Result<Self> {
        let probe: Self = serde_json::from_value(value)
            .map_err(|_| AutomationError::Protocol("invalid Runner environment report"))?;
        let printable = |value: &str| {
            !value.is_empty()
                && value.len() <= 96
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_graphic() || byte == b' ')
        };
        if probe.protocol != 1
            || [&probe.helper, &probe.python, &probe.system, &probe.machine]
                .iter()
                .any(|value| !printable(value))
            || probe.environment_digest.len() != 64
            || !probe
                .environment_digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || probe.packages.len() != PACKAGES.len()
            || probe.tools.len() > 32
            || probe.capabilities.len() > 32
            || probe
                .tools
                .keys()
                .chain(probe.capabilities.keys())
                .any(|key| !printable(key))
        {
            return Err(AutomationError::Protocol(
                "unsupported Runner environment report",
            ));
        }
        for (name, expected) in PACKAGES {
            let package = probe
                .packages
                .get(name)
                .ok_or(AutomationError::Protocol("missing Runner package report"))?;
            if package.expected != expected
                || package
                    .actual
                    .as_ref()
                    .is_some_and(|value| !printable(value))
                || package.compatible != (package.actual.as_deref() == Some(expected))
                || (probe.dependencies_ready && !package.compatible)
            {
                return Err(AutomationError::Protocol(
                    "inconsistent Runner dependency report",
                ));
            }
        }
        Ok(probe)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn old_helpers_cannot_silently_ignore_project_contracts_or_filename_variants() {
        let mut probe = EnvironmentProbe::parse(fixture()).unwrap();
        let mut revision = crate::ProjectRevision {
            version: 1,
            digest: "a".repeat(64),
            entries: vec![],
            bytes: 0,
        };
        assert!(probe.require_project_capabilities(&revision).is_ok());
        revision.entries.push(crate::ProjectEntry::File {
            path: ".consolecrypt-inputs.json".into(),
            mode: 0o600,
            bytes: 2,
            sha256: "b".repeat(64),
        });
        assert!(probe.require_project_capabilities(&revision).is_err());
        probe
            .capabilities
            .insert("declared_inputs_v1".into(), false);
        assert!(probe.require_project_capabilities(&revision).is_err());
        probe.capabilities.insert("declared_inputs_v1".into(), true);
        assert!(probe.require_project_capabilities(&revision).is_ok());
        revision.entries.push(crate::ProjectEntry::File {
            path: ".consolecrypt-dependencies.json".into(),
            mode: 0o600,
            bytes: 2,
            sha256: "c".repeat(64),
        });
        assert!(probe.require_project_capabilities(&revision).is_err());
        probe
            .capabilities
            .insert("locked_dependencies_v1".into(), false);
        assert!(probe.require_project_capabilities(&revision).is_err());
        probe
            .capabilities
            .insert("locked_dependencies_v1".into(), true);
        assert!(probe.require_project_capabilities(&revision).is_ok());
        revision.entries.push(crate::ProjectEntry::File {
            path: ".CONSOLECRYPT-REQUIREMENTS.JSON".into(),
            mode: 0o600,
            bytes: 2,
            sha256: "d".repeat(64),
        });
        assert!(probe.require_project_capabilities(&revision).is_err());
        probe
            .capabilities
            .insert("project_requirements_v1".into(), true);
        assert!(probe.require_project_capabilities(&revision).is_ok());
        if let crate::ProjectEntry::File { path, .. } = &mut revision.entries[1] {
            *path = ".CONSOLECRYPT-DEPENDENCIES.JSON".into();
        }
        probe.capabilities.remove("locked_dependencies_v1");
        assert!(probe.require_project_capabilities(&revision).is_err());
        probe
            .capabilities
            .insert("locked_dependencies_v1".into(), true);
        assert!(probe.require_project_capabilities(&revision).is_ok());
    }
    fn fixture() -> Value {
        let packages: BTreeMap<_, _> = PACKAGES
            .into_iter()
            .map(|(name, version)| {
                (
                    name,
                    json!({"expected":version,"actual":version,"compatible":true}),
                )
            })
            .collect();
        json!({"helper":"0.1.0","protocol":1,"python":"3.12.3","system":"Linux","machine":"x86_64",
            "environment_digest":"a".repeat(64),"non_root":true,"wsl":false,
            "sandbox":{"filesystem":true,"network":true,"pid":true},"packages":packages,
            "tools":{"ssh":true},"capabilities":{"execute":false,"validate":true},"dependencies_ready":true,
            "future_field":{"preserved_by_helper":true}})
    }
    #[test]
    fn trusted_worker_bootstrap_requires_explicit_true_even_with_current_version() {
        let mut probe=EnvironmentProbe::parse(fixture()).unwrap();
        probe.capabilities.insert("target_match_v1".into(),true);
        assert!(probe.require_target_match().is_ok());
        assert!(probe.require_trusted_worker_bootstrap().is_err());
        probe.capabilities.insert("trusted_worker_bootstrap_v1".into(),false);
        assert!(probe.require_trusted_worker_bootstrap().is_err());
        probe.capabilities.insert("trusted_worker_bootstrap_v1".into(),true);
        assert!(probe.require_trusted_worker_bootstrap().is_ok());
        assert!(!probe.capabilities.get("execute").copied().unwrap());
    }
    #[test]
    fn old_or_disabled_target_matching_needs_explicit_runner_update() {
        let mut probe=EnvironmentProbe::parse(fixture()).unwrap();
        assert!(probe.require_target_match().is_err());
        probe.capabilities.insert("target_match_v1".into(),false);
        assert!(probe.require_target_match().is_err());
        probe.capabilities.insert("target_match_v1".into(),true);
        assert!(probe.require_target_match().is_ok());
    }
    #[test]
    fn malformed_or_inconsistent_reports_never_reach_ui_as_capabilities() {
        assert!(EnvironmentProbe::parse(fixture()).is_ok());
        for (pointer, value) in [
            ("/protocol", json!(2)),
            ("/packages/ansible-core/actual", json!("0.1")),
            ("/system", json!("Linux\nroot allowed")),
            ("/environment_digest", json!("bad")),
            ("/sandbox/network", json!("true")),
            ("/capabilities/execute", json!("yes")),
        ] {
            let mut mutated = fixture();
            *mutated.pointer_mut(pointer).unwrap() = value;
            assert!(EnvironmentProbe::parse(mutated).is_err(), "{pointer}");
        }
    }
    #[test]
    fn missing_dependencies_and_unavailable_isolation_remain_diagnostics() {
        let mut value = fixture();
        value["packages"]["ansible-core"]["actual"] = Value::Null;
        value["packages"]["ansible-core"]["compatible"] = json!(false);
        value["dependencies_ready"] = json!(false);
        value["sandbox"]["network"] = json!(false);
        let probe = EnvironmentProbe::parse(value).unwrap();
        assert!(!probe.dependencies_ready && !probe.sandbox.network);
        assert_eq!(probe.capabilities["execute"], false);
    }
}
