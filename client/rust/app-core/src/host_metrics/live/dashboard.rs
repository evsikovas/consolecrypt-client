use super::{READ, Snapshot};
use crate::host_metrics::{OwnedHostMetricsRegistration, invalid};
use crate::{AppCore, AppError, AppResult};
use cc_models::host::SshBackend;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

#[derive(Deserialize)]
enum Mode {
    #[serde(rename = "dashboard")]
    Dashboard,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DashboardRequest {
    #[serde(rename = "mode")]
    _mode: Mode,
    operation_id: Uuid,
    host_id: String,
    host_updated_at_ms: i64,
    profile_id: String,
    notification_scope_id: String,
}

// Two readings in one SSH channel give a meaningful CPU value on the very first refresh.
const CPU_AGAIN: &str = r#"sleep 1 || exit 1
printf '%s\n' '---'
awk 'NR == 1 && $1 == "cpu" { print $2,$3,$4,$5,$6,$7,$8,$9 }' /proc/stat"#;

fn overview(stdout: &[u8]) -> AppResult<Value> {
    let bad = || invalid("host_metrics_invalid_sample");
    let (data, ticks) = std::str::from_utf8(stdout)
        .map_err(|_| bad())?
        .split_once("\n---\n")
        .ok_or_else(bad)?;
    let sample = Snapshot::parse(data.as_bytes())?;
    let ticks = ticks
        .split_whitespace()
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| bad())?;
    let cpu = Snapshot::cpu_delta(
        ticks.try_into().map_err(|_| bad())?,
        sample.cpu.ok_or_else(bad)?,
    );
    Ok(json!({
        "timestampMs": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
        "cpuPercent": cpu, "memoryTotal": sample.memory.map(|v|v.0), "memoryUsed": sample.memory.map(|v|v.1),
        "diskTotal": sample.disk.map(|v|v.0), "diskUsed": sample.disk.map(|v|v.1),
        "uptimeSeconds": sample.uptime, "logicalCpus": sample.logical_cpus, "loadAverage": sample.load_average,
    }))
}

impl AppCore {
    pub(super) async fn host_metrics_dashboard(
        &self,
        request: DashboardRequest,
    ) -> AppResult<Value> {
        let id = crate::dto::parse_id("host_id", &request.host_id)?;
        let (owner, unlock) = self.unlocked().await?;
        let mut shutdown = unlock.sharing_shutdown.subscribe();
        let (cancel, _registration) = {
            let _profile = self.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&unlock).await?;
            if owner.profile_id.to_string() != request.profile_id
                || unlock.notices.scope_id().as_deref() != Some(&request.notification_scope_id)
                || *shutdown.borrow()
            {
                return Err(AppError::VaultLocked);
            }
            let working = unlock.working();
            let stamp = working.version();
            let host = crate::inventory::host_of(&unlock, id)?;
            if crate::dto::ms(&host.updated_at) != request.host_updated_at_ms {
                return Err(invalid("host_metrics_host_changed"));
            }
            if host.backend != SshBackend::Native {
                return Err(invalid("host_metrics_ssh_backend"));
            }
            let revisions = crate::kubernetes_runtime::dependencies(&working.snapshot(), id)?;
            let cancel = unlock.host_metrics.register(request.operation_id)?;
            let registration = OwnedHostMetricsRegistration {
                unlock: Arc::downgrade(&unlock),
                id: request.operation_id,
            };
            if working.version() != stamp || !working.watch_revisions(revisions, cancel.clone()) {
                return Err(invalid("host_metrics_host_changed"));
            }
            (cancel, registration)
        };
        let read = async {
            let _permit = unlock
                .host_metrics
                .permits
                .acquire()
                .await
                .map_err(|_| AppError::Cancelled)?;
            let prepared = self.sharing_prepare_connection(&request.host_id).await?;
            if !Arc::ptr_eq(&unlock, &prepared.unlocked) {
                return Err(AppError::VaultLocked);
            }
            prepared
                .run(async {
                    let ssh = unlock.ssh.dashboard_connector().connect(&prepared.plan).await?;
                    let output = ssh
                        .exec_bounded(
                            &format!("{READ}\n{CPU_AGAIN}"),
                            64 * 1024,
                            cancel.cancelled(),
                        )
                        .await;
                    let _ = tokio::time::timeout(Duration::from_secs(1), ssh.disconnect()).await;
                    let output = output?.ok_or(AppError::Cancelled)?;
                    if output.exit_status == Some(42) {
                        return Err(invalid("host_metrics_linux_only"));
                    }
                    if output.exit_status != Some(0) || output.exit_signal.is_some() {
                        return Err(invalid("host_metrics_read_failed"));
                    }
                    overview(&output.stdout)
                })
                .await
        };
        tokio::select! { biased;
            _ = cancel.cancelled() => Err(AppError::Cancelled),
            _ = shutdown.changed() => Err(AppError::VaultLocked),
            result = tokio::time::timeout(Duration::from_secs(20),read) =>
                result.map_err(|_| invalid("host_metrics_timeout"))?,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_snapshot_uses_measured_cpu_and_reports_partial_data() {
        let sample = overview(b"cpu 10 0 10 80 0 0 0 0\nuptime 123\nmem 1024 256\nload 1 2 3\ncores 4\n---\n20 0 20 160 0 0 0 0\n").unwrap();
        assert_eq!(sample["cpuPercent"], 20.0);
        assert_eq!(sample["logicalCpus"], 4);
        assert_eq!(sample["memoryUsed"], 768 * 1024);
        assert!(sample["diskUsed"].is_null());
        assert!(overview(b"cpu 1 2 3 4 5 6 7 8\nuptime 1\n---\ngarbage").is_err());
    }
}
