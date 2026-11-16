//! Fixed source planning, not hardware facts, installation or a permission.
use crate::{AutomationError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const PROMETHEUS_VERSION: &str = "3.16.0";
pub const PROMETHEUS_SHA256: &str =
    "68727009472a2c02e1ff7f062fd5b3e637285bffc8fc2d340c76fb449d6e0f34";
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MonitoringNodeOperation {
    Install,
    Restore,
}
/// Same public, non-authority fields and bounds as canonical MonitoringPlan.
/// No URL/path, identity, capabilities, command or credentials are configurable.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonitoringNodePlan {
    pub cpu_cores: u32,
    pub memory_mib: u32,
    pub disk_gib: u32,
    pub retention_days: u32,
    pub retention_gib: u32,
    pub receiver_enabled: bool,
    pub alertmanager_enabled: bool,
    pub expected_samples_per_second: u32,
}
fn invalid() -> AutomationError {
    AutomationError::Invalid("monitoring_node_plan_invalid")
}
impl Default for MonitoringNodePlan {
    fn default() -> Self {
        Self {
            cpu_cores: 2,
            memory_mib: 2048,
            disk_gib: 50,
            retention_days: 15,
            retention_gib: 40,
            receiver_enabled: false,
            alertmanager_enabled: false,
            expected_samples_per_second: 2000,
        }
    }
}
impl MonitoringNodePlan {
    pub fn validate(&self) -> Result<()> {
        if !(2..=64).contains(&self.cpu_cores)
            || !(2048..=262144).contains(&self.memory_mib)
            || !(10..=10000).contains(&self.disk_gib)
            || !(1..=365).contains(&self.retention_days)
            || !(1..=9000).contains(&self.retention_gib)
            || !(1..=1_000_000).contains(&self.expected_samples_per_second)
            || self.retention_gib as u64 * 10 > self.disk_gib as u64 * 8
            || self.estimated_sample_bytes() > self.retention_gib as u64 * 1024 * 1024 * 1024
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub fn estimated_sample_bytes(&self) -> u64 {
        2 * self.expected_samples_per_second as u64 * self.retention_days as u64 * 86400
    }
    pub fn variables(&self) -> Result<Value> {
        self.validate()?;
        let mut plan = serde_json::to_value(self).map_err(|_| invalid())?;
        plan["estimated_sample_bytes"] = json!(self.estimated_sample_bytes());
        plan["estimate_excludes"] =
            json!(["series index", "head", "WAL", "compaction", "filesystem"]);
        plan["reference_benchmark"] = json!("not_measured");
        let mut config = json!({"global":{"scrape_interval":"15s","evaluation_interval":"15s"},
            "rule_files":["/etc/consolecrypt-monitoring/rules.yml"],
            "scrape_configs":[{"job_name":"monitoring_node","scrape_interval":"15s","scrape_timeout":"10s",
                "body_size_limit":"8MB","sample_limit":20000,"label_limit":24,"label_name_length_limit":128,
                "label_value_length_limit":256,"static_configs":[{"targets":["127.0.0.1:9090"]}]}]});
        if self.alertmanager_enabled {
            config["alerting"] =
                json!({"alertmanagers":[{"static_configs":[{"targets":["127.0.0.1:9093"]}]}]});
        }
        Ok(
            json!({"cc_monitoring_plan":plan,"cc_prometheus_version":PROMETHEUS_VERSION,
            "cc_prometheus_sha256":PROMETHEUS_SHA256,"cc_prometheus_config":config}),
        )
    }
    pub(crate) fn playbook(&self, operation: MonitoringNodeOperation) -> Result<String> {
        let mut source = include_str!("../../../../agent/delivery/monitoring-node.yml").to_owned();
        let eol = if source.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let marker = format!("  vars:{eol}");
        let install = "    cc_monitoring_install_approved: false";
        let restore = "    cc_monitoring_restore_approved: false";
        let op = "    cc_monitoring_operation: install";
        // Only this reviewed known source is rendered. Future changed markers
        // refuse creation; no general YAML classifier or caller text template.
        if source.matches(&marker).count() != 1
            || source.matches(install).count() != 1
            || source.matches(restore).count() != 1
            || source.matches(op).count() != 1
        {
            return Err(invalid());
        }
        let vars = self.variables()?;
        let mut fixed = marker.clone();
        for name in [
            "cc_monitoring_plan",
            "cc_prometheus_version",
            "cc_prometheus_sha256",
            "cc_prometheus_config",
        ] {
            fixed.push_str(&format!(
                "    {name}: {}{eol}",
                serde_json::to_string(&vars[name]).map_err(|_| invalid())?
            ));
        }
        source = source.replacen(&marker, &fixed, 1);
        if operation == MonitoringNodeOperation::Install {
            source = source.replacen(install, "    cc_monitoring_install_approved: true", 1);
        } else {
            source = source.replacen(restore, "    cc_monitoring_restore_approved: true", 1);
            source = source.replacen(op, "    cc_monitoring_operation: restore", 1);
        }
        Ok(source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn impossible_capacity_and_incomplete_sample_estimate_remain_refused() {
        let mut plan = MonitoringNodePlan::default();
        plan.retention_gib = 41;
        assert!(plan.validate().is_err());
        plan = MonitoringNodePlan::default();
        plan.expected_samples_per_second = 1_000_000;
        assert!(plan.validate().is_err());
        plan = MonitoringNodePlan::default();
        plan.memory_mib = 1536;
        assert!(plan.validate().is_err());
        let mut raw = serde_json::to_value(MonitoringNodePlan::default()).unwrap();
        raw["receiver_enabled"] = json!(1);
        assert!(serde_json::from_value::<MonitoringNodePlan>(raw).is_err());
    }
    #[test]
    fn fixed_source_keeps_actual_hardware_and_separate_storage_trust_preflight() {
        let plan = MonitoringNodePlan::default();
        let install = plan.playbook(MonitoringNodeOperation::Install).unwrap();
        assert!(install.contains("cc_monitoring_install_approved: true"));
        assert!(install.contains("cc_monitoring_restore_approved: false"));
        assert!(install.contains("ansible_memtotal_mb | int >= cc_monitoring_plan.memory_mib"));
        assert!(install.contains("already provisioned separate storage mount"));
        assert!(
            install
                .contains("do not automatically inherit ConsoleCrypt vault end-to-end encryption")
        );
        let restore = plan.playbook(MonitoringNodeOperation::Restore).unwrap();
        assert!(restore.contains("cc_monitoring_operation: restore"));
        assert!(restore.contains("cc_monitoring_install_approved: false"));
        assert!(restore.contains("cc_monitoring_restore_approved: true"));
        let vars = plan.variables().unwrap();
        assert_eq!(
            vars["cc_monitoring_plan"]["reference_benchmark"],
            "not_measured"
        );
        assert_eq!(
            vars["cc_prometheus_config"]["scrape_configs"][0]["static_configs"][0]["targets"][0],
            "127.0.0.1:9090"
        );
        assert!(vars.get("capabilities").is_none());
        assert!(vars.get("actualRuntimeVersion").is_none());
    }
}
