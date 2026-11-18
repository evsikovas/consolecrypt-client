//! Live observations on one dedicated SSH session. The caller selects a saved
//! Host and unlock epoch; the command and file paths are fixed here.
use super::{OwnedHostMetricsRegistration, invalid};
use crate::{AppCore, AppError, AppResult};
use cc_models::host::SshBackend;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const READ: &str = r#"export LC_ALL=C
[ "$(uname -s)" = Linux ] && [ -r /proc/stat ] || exit 42
awk 'NR == 1 && $1 == "cpu" { print "cpu", $2, $3, $4, $5, $6, $7, $8, $9; exit }' /proc/stat
awk '/^MemTotal:/ { t=$2 } /^MemAvailable:/ { a=$2 } END { if (t!="" && a!="") print "mem", t, a }' /proc/meminfo
df -Pk / | awk 'NR == 2 && NF >= 6 { print "disk", $(NF-4), $(NF-3) }'
awk 'NR == 1 { print "uptime", $1; exit }' /proc/uptime
awk -F: 'NR > 2 { n=$1; gsub(/^[[:space:]]+|[[:space:]]+$/, "", n); if (n != "lo") { s=$2; sub(/^[[:space:]]+/, "", s); c=split(s, v, /[[:space:]]+/); if (c >= 16) print "net", n, v[1], v[9] } }' /proc/net/dev
"#;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    operation_id: Uuid,
    host_id: String,
    host_updated_at_ms: i64,
    profile_id: String,
    notification_scope_id: String,
}

#[derive(Default)]
struct Snapshot {
    cpu: Option<[u64; 8]>,
    memory: Option<(u64, u64)>,
    disk: Option<(u64, u64)>,
    uptime: Option<f64>,
    network: BTreeMap<String, (u64, u64)>,
}

impl Snapshot {
    fn parse(bytes: &[u8]) -> AppResult<Self> {
        let bad = || invalid("host_metrics_invalid_sample");
        let output = std::str::from_utf8(bytes).map_err(|_| bad())?;
        let mut sample = Self::default();
        for line in output.lines() {
            let fields: Vec<_> = line.split_whitespace().collect();
            let number = |s: &str| s.parse::<u64>().map_err(|_| bad());
            let bytes = |s: &str| number(s)?.checked_mul(1024).ok_or_else(bad);
            match fields.as_slice() {
                ["cpu", a, b, c, d, e, f, g, h] if sample.cpu.is_none() => {
                    sample.cpu = Some([
                        number(a)?,
                        number(b)?,
                        number(c)?,
                        number(d)?,
                        number(e)?,
                        number(f)?,
                        number(g)?,
                        number(h)?,
                    ]);
                }
                ["mem", total, available] if sample.memory.is_none() => {
                    let (total, available) = (bytes(total)?, bytes(available)?);
                    if total == 0 || available > total {
                        return Err(bad());
                    }
                    sample.memory = Some((total, total - available));
                }
                ["disk", total, used] if sample.disk.is_none() => {
                    let (total, used) = (bytes(total)?, bytes(used)?);
                    if total == 0 || used > total {
                        return Err(bad());
                    }
                    sample.disk = Some((total, used));
                }
                ["uptime", seconds] if sample.uptime.is_none() => {
                    let seconds: f64 = seconds.parse().map_err(|_| bad())?;
                    if !seconds.is_finite() || !(0.0..=1e12).contains(&seconds) {
                        return Err(bad());
                    }
                    sample.uptime = Some(seconds);
                }
                ["net", name, received, sent] if name.len() <= 64 && sample.network.len() < 256 => {
                    if sample
                        .network
                        .insert(name.to_string(), (number(received)?, number(sent)?))
                        .is_some()
                    {
                        return Err(bad());
                    }
                }
                _ => return Err(bad()),
            }
        }
        if sample.cpu.is_none() || sample.uptime.is_none() {
            return Err(bad());
        }
        Ok(sample)
    }

    fn cpu_percent(&self, previous: &Self) -> Option<f64> {
        let (current, previous) = (self.cpu?, previous.cpu?);
        let mut ticks = [0u64; 8];
        for (i, tick) in ticks.iter_mut().enumerate() {
            *tick = current[i].checked_sub(previous[i])?;
        }
        let total = ticks.into_iter().try_fold(0_u64, u64::checked_add)?;
        let idle = ticks[3].checked_add(ticks[4])?;
        (total > 0).then(|| 100.0 * (total - idle) as f64 / total as f64)
    }

    fn network_rate(&self, previous: &Self, elapsed: Duration) -> Option<(f64, f64)> {
        if self.network.is_empty()
            || !self.network.keys().eq(previous.network.keys())
            || elapsed.is_zero()
        {
            return None;
        }
        let mut rx = 0u64;
        let mut tx = 0u64;
        for (name, &(received, sent)) in &self.network {
            let (old_rx, old_tx) = previous.network[name];
            rx = rx.checked_add(received.checked_sub(old_rx)?)?;
            tx = tx.checked_add(sent.checked_sub(old_tx)?)?;
        }
        Some((
            rx as f64 / elapsed.as_secs_f64(),
            tx as f64 / elapsed.as_secs_f64(),
        ))
    }
}

impl AppCore {
    pub async fn host_metrics_watch(
        &self,
        raw: &str,
        mut emit: impl FnMut(Value) -> bool + Send,
    ) -> AppResult<()> {
        if raw.len() > 1024 {
            return Err(invalid("invalid host metrics request"));
        }
        let request: Request =
            serde_json::from_str(raw).map_err(|_| invalid("invalid host metrics request"))?;
        let host_id = crate::dto::parse_id("host_id", &request.host_id)?;
        let (owner, unlock) = self.unlocked().await?;
        let original = Arc::downgrade(&unlock);
        let mut shutdown = unlock.sharing_shutdown.subscribe();
        let (_registration, cancel) = {
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
            let host = crate::inventory::host_of(&unlock, host_id)?;
            if crate::dto::ms(&host.updated_at) != request.host_updated_at_ms {
                return Err(invalid("host_metrics_host_changed"));
            }
            if host.backend != SshBackend::Native {
                return Err(invalid("host_metrics_ssh_backend"));
            }
            let revisions = crate::kubernetes_runtime::dependencies(&working.snapshot(), host_id)?;
            let permit = unlock
                .host_metrics
                .permits
                .clone()
                .try_acquire_owned()
                .map_err(|_| invalid("host metrics concurrency budget reached"))?;
            let cancel = unlock.host_metrics.register(request.operation_id)?;
            let registration = OwnedHostMetricsRegistration {
                unlock: original.clone(),
                id: request.operation_id,
                _permit: permit,
            };
            if working.version() != stamp || !working.watch_revisions(revisions, cancel.clone()) {
                return Err(invalid("host_metrics_host_changed"));
            }
            (registration, cancel)
        };
        drop(unlock);
        drop(owner);
        let deadline = Instant::now() + Duration::from_secs(45);
        let session = self
            .open_private_runner_session_fenced(
                &request.host_id,
                &original,
                &cancel,
                Some(deadline),
                || {
                    if cancel.is_cancelled() {
                        Err(AppError::Cancelled)
                    } else {
                        Ok(())
                    }
                },
            )
            .await?;
        let result = async {
            let mut last: Option<(Instant, Snapshot)> = None;
            loop {
                let output = tokio::select! { biased;
                    _ = cancel.cancelled() => return Err(AppError::Cancelled),
                    _ = shutdown.changed() => return Err(AppError::VaultLocked),
                    value = tokio::time::timeout(Duration::from_secs(8), session.exec_with_private_input(READ, &[], 16 * 1024)) =>
                        value.map_err(|_| invalid("host_metrics_timeout"))??,
                };
                if output.exit_status == Some(42) {
                    return Err(invalid("host_metrics_linux_only"));
                }
                if output.exit_status != Some(0) || output.exit_signal.is_some() {
                    return Err(invalid("host_metrics_read_failed"));
                }
                let sample = Snapshot::parse(&output.stdout)?;
                let now = Instant::now();
                let cpu = last.as_ref().and_then(|(_, old)| sample.cpu_percent(old));
                let rates = last.as_ref().and_then(|(time, old)| sample.network_rate(old, now.duration_since(*time)));
                if cancel.is_cancelled() || *shutdown.borrow() {
                    return Err(AppError::Cancelled);
                }
                if !emit(json!({
                    "timestampMs": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
                    "cpuPercent": cpu,
                    "memoryTotal": sample.memory.map(|v| v.0), "memoryUsed": sample.memory.map(|v| v.1),
                    "diskTotal": sample.disk.map(|v| v.0), "diskUsed": sample.disk.map(|v| v.1),
                    "receivedPerSecond": rates.map(|v| v.0), "sentPerSecond": rates.map(|v| v.1),
                    "uptimeSeconds": sample.uptime,
                })) {
                    return Ok(());
                }
                let seconds = if last.is_none() { 2 } else { 5 };
                last = Some((now, sample));
                tokio::select! { biased;
                    _ = cancel.cancelled() => return Err(AppError::Cancelled),
                    _ = shutdown.changed() => return Err(AppError::VaultLocked),
                    _ = tokio::time::sleep(Duration::from_secs(seconds)) => {},
                }
            }
        }.await;
        let _ = tokio::time::timeout(Duration::from_secs(2), session.disconnect()).await;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(cpu: &str, rx: u64, tx: u64) -> Snapshot {
        Snapshot::parse(
            format!("cpu {cpu}\nmem 1000 400\ndisk 2000 500\nuptime 123.5\nnet eth0 {rx} {tx}\n")
                .as_bytes(),
        )
        .unwrap()
    }
    #[test]
    fn current_proc_counters_produce_bytes_and_rates_without_storage() {
        let old = sample("10 0 10 80 0 0 0 0", 100, 200);
        let now = sample("20 0 20 160 0 0 0 0", 300, 300);
        assert_eq!(now.memory, Some((1024000, 614400)));
        assert_eq!(now.disk, Some((2048000, 512000)));
        assert_eq!(now.cpu_percent(&old), Some(20.0));
        assert_eq!(
            now.network_rate(&old, Duration::from_secs(2)),
            Some((100.0, 50.0))
        );
    }
    #[test]
    fn reset_or_changed_counters_do_not_invent_usage() {
        let old = sample("20 0 20 160 0 0 0 0", 300, 300);
        let now = sample("10 0 10 80 0 0 0 0", 100, 200);
        assert!(now.cpu_percent(&old).is_none());
        assert!(now.network_rate(&old, Duration::from_secs(2)).is_none());
    }
    #[test]
    fn no_caller_commands_and_invalid_samples_are_accepted() {
        assert!(Snapshot::parse(b"cpu 1 2 3 4 5 6 7 8\nuptime nan\n").is_err());
        assert!(Snapshot::parse(b"cpu 1 2 3 4 5 6 7 8\nuptime 1\nmem 1 2\n").is_err());
        let mut request = json!({"operationId":Uuid::new_v4(),"hostId":Uuid::new_v4().to_string(),
            "hostUpdatedAtMs":1,"profileId":Uuid::new_v4().to_string(),"notificationScopeId":"nonce"});
        request["command"] = json!("caller command");
        assert!(serde_json::from_value::<Request>(request).is_err());
    }
}
