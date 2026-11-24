//! Live observations on the active terminal connection using a separate exec channel.
//! The command and file paths are fixed here.
use super::{OwnedHostMetricsRegistration, invalid};
use crate::{AppCore, AppError, AppResult};
use cc_models::host::SshBackend;
use cc_terminal_core::TerminalStatus;
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
awk 'NR == 1 && $1 == "cpu" { print "cpu", $2, $3, $4, $5, $6, $7, $8, $9 }
     /^cpu[0-9]+ / { n++; if (n<=128) print "core", $1, $2, $3, $4, $5, $6, $7, $8, $9 }
     END { if (n) print "cores", n }' /proc/stat
printf 'kernel %s\narch %s\n' "$(uname -r)" "$(uname -m)"
if [ -r /etc/os-release ]; then
  awk '/^PRETTY_NAME=/ { v=substr($0, 13); sub(/^"/, "", v); sub(/"$/, "", v); if (v!="") print "os", v; exit }' /etc/os-release
else
  printf 'os Linux\n'
fi
if [ -r /proc/cpuinfo ]; then
  awk -F: '/^(model name|Hardware)[[:space:]]*:/ { sub(/^[[:space:]]+/, "", $2); if ($2!="") print "model", $2; exit }' /proc/cpuinfo
fi
awk 'NR == 1 && NF >= 3 { print "load", $1, $2, $3; exit }' /proc/loadavg
awk '/^MemTotal:/ { t=$2 } /^MemAvailable:/ { a=$2 } END { if (t!="" && a!="") print "mem", t, a }' /proc/meminfo
awk '/^SwapTotal:/ { t=$2 } /^SwapFree:/ { f=$2 } END { if (t!="" && f!="") print "swap",t,f }' /proc/meminfo
if command -v who >/dev/null 2>&1; then who 2>/dev/null | awk 'END { print "logins",NR }'; fi
df -Pk / | awk 'NR == 2 && NF >= 6 { print "disk", $(NF-4), $(NF-3) }'
awk 'NR == 1 { print "uptime", $1; exit }' /proc/uptime
awk -F: 'NR > 2 { n=$1; gsub(/^[[:space:]]+|[[:space:]]+$/, "", n); if (n != "lo") { s=$2; sub(/^[[:space:]]+/, "", s); c=split(s, v, /[[:space:]]+/); if (c >= 16) print "net", n, v[1], v[9] } }' /proc/net/dev
"#;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    operation_id: Uuid,
    host_id: String,
    terminal_id: Uuid,
    host_updated_at_ms: i64,
    profile_id: String,
    notification_scope_id: String,
}

#[derive(Default)]
struct Snapshot {
    cpu: Option<[u64; 8]>,
    memory: Option<(u64, u64)>,
    cores: BTreeMap<String, [u64; 8]>,
    swap: Option<(u64, u64)>,
    logins: Option<u32>,
    docker: bool,
    disk: Option<(u64, u64)>,
    uptime: Option<f64>,
    os: Option<String>,
    kernel: Option<String>,
    architecture: Option<String>,
    cpu_model: Option<String>,
    logical_cpus: Option<u32>,
    load_average: Option<String>,
    network: BTreeMap<String, (u64, u64)>,
}

impl Snapshot {
    fn parse(bytes: &[u8]) -> AppResult<Self> {
        let bad = || invalid("host_metrics_invalid_sample");
        let output = std::str::from_utf8(bytes).map_err(|_| bad())?;
        let mut sample = Self::default();
        for line in output.lines() {
            let (field, value) = line.split_once(' ').ok_or_else(bad)?;
            let value = value.trim();
            let description = match field {
                "os" => Some(&mut sample.os),
                "kernel" => Some(&mut sample.kernel),
                "arch" => Some(&mut sample.architecture),
                "model" => Some(&mut sample.cpu_model),
                _ => None,
            };
            if let Some(description) = description {
                if description.is_some() || value.len() > 256 || value.chars().any(char::is_control)
                {
                    return Err(bad());
                }
                if !value.is_empty() {
                    *description = Some(value.to_owned());
                }
                continue;
            }
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
                ["core", name, a, b, c, d, e, f, g, h]
                    if sample.cores.len() < 128
                        && name
                            .strip_prefix("cpu")
                            .and_then(|v| v.parse::<u32>().ok())
                            .is_some() =>
                {
                    sample.cores.insert(
                        name.to_string(),
                        [
                            number(a)?,
                            number(b)?,
                            number(c)?,
                            number(d)?,
                            number(e)?,
                            number(f)?,
                            number(g)?,
                            number(h)?,
                        ],
                    );
                }
                ["swap", total, free] => {
                    let total = bytes(total)?;
                    sample.swap = Some((total, total.checked_sub(bytes(free)?).ok_or_else(bad)?));
                }
                ["docker", "1"] => sample.docker = true,
                ["logins", n] => sample.logins = Some(n.parse().map_err(|_| bad())?),
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
                ["cores", count] if sample.logical_cpus.is_none() => {
                    sample.logical_cpus = Some(count.parse::<u32>().map_err(|_| bad())?);
                }
                ["load", one, five, fifteen] if sample.load_average.is_none() => {
                    if ![one, five, fifteen].into_iter().all(|value| {
                        value.len() <= 12
                            && value.bytes().all(|c| c.is_ascii_digit() || c == b'.')
                            && value.parse::<f64>().is_ok()
                    }) {
                        return Err(bad());
                    }
                    sample.load_average = Some(format!("{one} / {five} / {fifteen}"));
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
        Self::cpu_delta(self.cpu?, previous.cpu?)
    }

    fn cpu_delta(current: [u64; 8], previous: [u64; 8]) -> Option<f64> {
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
        let (_registration, cancel, terminals, session, permits) = {
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
            let terminals = unlock.ssh.terminals.clone();
            let session = terminals
                .connected_ssh(request.terminal_id, host_id)
                .ok_or_else(|| invalid("host_metrics_session_closed"))?;
            let revisions = crate::kubernetes_runtime::dependencies(&working.snapshot(), host_id)?;
            let cancel = unlock.host_metrics.register(request.operation_id)?;
            let registration = OwnedHostMetricsRegistration {
                unlock: original.clone(),
                id: request.operation_id,
            };
            if working.version() != stamp || !working.watch_revisions(revisions, cancel.clone()) {
                return Err(invalid("host_metrics_host_changed"));
            }
            (
                registration,
                cancel,
                terminals,
                session,
                unlock.host_metrics.permits.clone(),
            )
        };
        drop(unlock);
        drop(owner);
        // Use the same socket and permission rules as the Docker workspace, including rootless engines.
        let read = format!("{READ}\ncase \"$({})\" in 'ok '*) echo 'docker 1';; esac\n",
            crate::docker_runtime::SOCKET_PROBE);
        let mut last: Option<(Instant, Snapshot)> = None;
        let closed = terminals.wait_closed(request.terminal_id);
        tokio::pin!(closed);
        loop {
            // Share probe slots between sessions. An idle watcher never occupies one.
            let slot = tokio::select! { biased;
                _ = cancel.cancelled() => return Err(AppError::Cancelled),
                _ = shutdown.changed() => return Err(AppError::VaultLocked),
                _ = &mut closed => return Err(invalid("host_metrics_session_closed")),
                slot = permits.acquire() => slot.map_err(|_| AppError::Cancelled)?,
            };
            let output = session
                .exec_bounded(&read, 64 * 1024, async {
                    tokio::select! { biased;
                        _ = cancel.cancelled() => {},
                        _ = shutdown.changed() => {},
                        _ = &mut closed => {},
                        _ = tokio::time::sleep(Duration::from_secs(8)) => {},
                    }
                })
                .await?;
            drop(slot);
            let Some(output) = output else {
                if cancel.is_cancelled() {
                    return Err(AppError::Cancelled);
                }
                if *shutdown.borrow() {
                    return Err(AppError::VaultLocked);
                }
                return Err(
                    if matches!(
                        terminals.status(request.terminal_id),
                        Ok(TerminalStatus::Connected)
                    ) {
                        invalid("host_metrics_timeout")
                    } else {
                        invalid("host_metrics_session_closed")
                    },
                );
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
            let rates = last
                .as_ref()
                .and_then(|(time, old)| sample.network_rate(old, now.duration_since(*time)));
            if cancel.is_cancelled()
                || *shutdown.borrow()
                || !matches!(
                    terminals.status(request.terminal_id),
                    Ok(TerminalStatus::Connected)
                )
            {
                return Err(AppError::Cancelled);
            }
            let core_usage = sample
                .cores
                .iter()
                .map(|(name, current)| {
                    let value = last
                        .as_ref()
                        .and_then(|(_, old)| Snapshot::cpu_delta(*current, *old.cores.get(name)?));
                    (name, value)
                })
                .collect::<BTreeMap<_, _>>();
            let interfaces = sample
                .network
                .iter()
                .map(|(name, (rx, tx))| {
                    let rate = last.as_ref().and_then(|(time, old)| {
                        let (old_rx, old_tx) = old.network.get(name)?;
                        let seconds = now.duration_since(*time).as_secs_f64();
                        if seconds <= 0.0 {
                            return None;
                        }
                        Some((
                            rx.checked_sub(*old_rx)? as f64 / seconds,
                            tx.checked_sub(*old_tx)? as f64 / seconds,
                        ))
                    });
                    json!({"name":name,"rx":rate.map(|v|v.0),"tx":rate.map(|v|v.1)})
                })
                .collect::<Vec<_>>();
            if !emit(json!({
                "cores":core_usage,"interfaces":interfaces,"docker":sample.docker,"logins":sample.logins,
                "swapUsed":sample.swap.map(|v|v.1),"swapTotal":sample.swap.map(|v|v.0),
                "timestampMs": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
                "cpuPercent": cpu,
                "channelLatencyMs": output.channel_latency.map(|v| v.as_millis()),
                "memoryTotal": sample.memory.map(|v| v.0), "memoryUsed": sample.memory.map(|v| v.1),
                "diskTotal": sample.disk.map(|v| v.0), "diskUsed": sample.disk.map(|v| v.1),
                "receivedPerSecond": rates.map(|v| v.0), "sentPerSecond": rates.map(|v| v.1),
                "uptimeSeconds": sample.uptime,
                "os": sample.os, "kernel": sample.kernel, "architecture": sample.architecture,
                "cpuModel": sample.cpu_model, "logicalCpus": sample.logical_cpus, "loadAverage": sample.load_average,
            })) {
                return Ok(());
            }
            let seconds = if last.is_none() { 2 } else { 5 };
            last = Some((now, sample));
            tokio::select! { biased;
                _ = cancel.cancelled() => return Err(AppError::Cancelled),
                _ = shutdown.changed() => return Err(AppError::VaultLocked),
                _ = &mut closed => return Err(invalid("host_metrics_session_closed")),
                _ = tokio::time::sleep(Duration::from_secs(seconds)) => {},
            }
        }
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
    fn server_details_keep_spaces_and_reject_invalid_numbers() {
        let sample = Snapshot::parse(b"cpu 1 2 3 4 5 6 7 8\nuptime 1\nos Debian GNU/Linux\nkernel 6.12\nmodel Example CPU\narch aarch64\ncores 4\nload 0.01 0.08 0.13\n").unwrap();
        assert_eq!(sample.os.as_deref(), Some("Debian GNU/Linux"));
        assert_eq!(sample.kernel.as_deref(), Some("6.12"));
        assert_eq!(sample.cpu_model.as_deref(), Some("Example CPU"));
        assert_eq!(sample.architecture.as_deref(), Some("aarch64"));
        assert_eq!(sample.logical_cpus, Some(4));
        assert_eq!(sample.load_average.as_deref(), Some("0.01 / 0.08 / 0.13"));
        assert!(Snapshot::parse(b"cpu 1 2 3 4 5 6 7 8\nuptime 1\nload nan 0 0\n").is_err());
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
            "terminalId":Uuid::new_v4(),"hostUpdatedAtMs":1,"profileId":Uuid::new_v4().to_string(),"notificationScopeId":"nonce"});
        request["command"] = json!("caller command");
        assert!(serde_json::from_value::<Request>(request).is_err());
    }
}
