//! Trusted, metric-probed templates. A binding is a native/user decision, not
//! proof from exporter labels; AppCore must resolve ambiguity before calling.
use crate::*;
use cc_host_mapping::{HostBinding, Labels};
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PanelKind {
    HostOverview,
    Cpu,
    Memory,
    Filesystem,
    DiskIo,
    Network,
    Docker,
    PostgreSql,
    Redis,
    Nginx,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Unit {
    Percent,
    Bytes,
    BytesPerSecond,
    CpuCores,
    Connections,
    TransactionsPerSecond,
    OperationsPerSecond,
    RequestsPerSecond,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedQuery {
    pub id: String,
    pub expression: String,
    pub unit: Unit,
    /// Stable localization key, not exporter-provided text.
    pub semantics: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelPlan {
    pub panel: PanelKind,
    pub source_id: String,
    pub host_id: String,
    pub binding_revision: u64,
    pub start: f64,
    pub end: f64,
    pub step_seconds: f64,
    pub max_series: usize,
    pub max_points_per_series: usize,
    pub max_total_points: usize,
    pub queries: Vec<PlannedQuery>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UnavailableReason {
    MissingMetrics,
    ProbeFailed,
    SelectorConflict,
    QueryBudget,
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum PanelPreparation {
    Ready {
        plan: PanelPlan,
        probe: QueryOutcome,
    },
    Unavailable {
        panel: PanelKind,
        reason: UnavailableReason,
        #[serde(rename = "missingMetrics")]
        missing_metrics: Vec<String>,
        probe: Option<QueryOutcome>,
        error: Option<Error>,
    },
}

const CPU: &str = "node_cpu_seconds_total";
const MEM_TOTAL: &str = "node_memory_MemTotal_bytes";
const MEM_AVAIL: &str = "node_memory_MemAvailable_bytes";
const FS_SIZE: &str = "node_filesystem_size_bytes";
const FS_AVAIL: &str = "node_filesystem_avail_bytes";
const DISK_READ: &str = "node_disk_read_bytes_total";
const DISK_WRITE: &str = "node_disk_written_bytes_total";
const NET_RX: &str = "node_network_receive_bytes_total";
const NET_TX: &str = "node_network_transmit_bytes_total";
const CONTAINER_CPU: &str = "container_cpu_usage_seconds_total";
const CONTAINER_MEMORY: &str = "container_memory_working_set_bytes";
const PG_CONNECTIONS: &str = "pg_stat_database_numbackends";
const PG_COMMITS: &str = "pg_stat_database_xact_commit";
const REDIS_MEMORY: &str = "redis_memory_used_bytes";
const REDIS_CLIENTS: &str = "redis_connected_clients";
const REDIS_COMMANDS: &str = "redis_commands_processed_total";
const NGINX_CONNECTIONS: &str = "nginx_connections_active";
const NGINX_REQUESTS: &str = "nginx_http_requests_total";

impl PanelKind {
    /// Preview only; existence of a template does not mean its metrics exist.
    pub fn query_templates(self, binding: &HostBinding) -> Result<Vec<PlannedQuery>, Error> {
        queries(self, binding)
    }
    pub fn required_metrics(self) -> &'static [&'static str] {
        match self {
            Self::HostOverview => &[CPU, MEM_TOTAL, MEM_AVAIL],
            Self::Cpu => &[CPU],
            Self::Memory => &[MEM_TOTAL, MEM_AVAIL],
            Self::Filesystem => &[FS_SIZE, FS_AVAIL],
            Self::DiskIo => &[DISK_READ, DISK_WRITE],
            Self::Network => &[NET_RX, NET_TX],
            Self::Docker => &[CONTAINER_CPU, CONTAINER_MEMORY],
            Self::PostgreSql => &[PG_CONNECTIONS, PG_COMMITS],
            Self::Redis => &[REDIS_MEMORY, REDIS_CLIENTS, REDIS_COMMANDS],
            Self::Nginx => &[NGINX_CONNECTIONS, NGINX_REQUESTS],
        }
    }
}
fn metric(binding: &HostBinding, name: &str) -> Result<String, Error> {
    let rendered = if name == CPU {
        binding.render_metric_with_exact(name, &Labels::from([("mode".into(), "idle".into())]))
    } else {
        binding.render_metric(name)
    }
    .map_err(|_| {
        Error::new(
            ErrorCode::InvalidQuery,
            "Panel template conflicts with the explicit selector or exceeds its budget",
        )
    })?;
    if matches!(name, CONTAINER_CPU | CONTAINER_MEMORY) {
        // Fixed template filter excludes cAdvisor root/parent cgroups without
        // an image label. Received labels never become operators or suffixes.
        Ok(format!(
            "{},image!=\"\"}}",
            rendered.strip_suffix('}').expect("validated renderer")
        ))
    } else {
        Ok(rendered)
    }
}
fn probe_expression(panel: PanelKind, binding: &HostBinding) -> Result<String, Error> {
    let metrics = panel
        .required_metrics()
        .iter()
        .map(|m| metric(binding, m))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format!(
        "count by (__name__) ({})",
        metrics.join(" or on (__name__) ")
    ))
}
fn queries(panel: PanelKind, binding: &HostBinding) -> Result<Vec<PlannedQuery>, Error> {
    let mut queries = vec![];
    let mut add = |id: &str, expression: String, unit: Unit, semantics: &str| {
        queries.push(PlannedQuery {
            id: id.into(),
            expression,
            unit,
            semantics: semantics.into(),
        });
    };
    if matches!(panel, PanelKind::HostOverview | PanelKind::Cpu) {
        // rate is applied per counter before aggregation, preserving resets.
        // Keep all identity labels; average only CPUs, never hosts.
        add(
            "cpuBusy",
            format!(
                "100 * (1 - avg without (cpu, mode) (rate({}[5m])))",
                metric(binding, CPU)?
            ),
            Unit::Percent,
            "cpu.busyPercent.averageLogicalCpus.idleRate5m",
        );
    }
    if matches!(panel, PanelKind::HostOverview | PanelKind::Memory) {
        let total = metric(binding, MEM_TOTAL)?;
        let available = metric(binding, MEM_AVAIL)?;
        add(
            "memoryUsedPercent",
            format!("100 * (1 - {available} / {total})"),
            Unit::Percent,
            "memory.usedPercent.totalMinusAvailable",
        );
        add(
            "memoryAvailable",
            available,
            Unit::Bytes,
            "memory.availableBytes",
        );
    }
    match panel {
        PanelKind::Filesystem => add(
            "filesystemUnavailablePercent",
            format!(
                "100 * (1 - {} / {})",
                metric(binding, FS_AVAIL)?,
                metric(binding, FS_SIZE)?
            ),
            Unit::Percent,
            "filesystem.unavailableToUnprivilegedPercent.includesReserved.perMount",
        ),
        PanelKind::DiskIo => {
            add(
                "diskRead",
                format!("rate({}[5m])", metric(binding, DISK_READ)?),
                Unit::BytesPerSecond,
                "disk.readBytesPerSecond.resetAware.perDevice",
            );
            add(
                "diskWrite",
                format!("rate({}[5m])", metric(binding, DISK_WRITE)?),
                Unit::BytesPerSecond,
                "disk.writeBytesPerSecond.resetAware.perDevice",
            );
        }
        PanelKind::Network => {
            add(
                "networkReceive",
                format!("rate({}[5m])", metric(binding, NET_RX)?),
                Unit::BytesPerSecond,
                "network.receiveBytesPerSecond.resetAware.perInterface.includesLoopback",
            );
            add(
                "networkTransmit",
                format!("rate({}[5m])", metric(binding, NET_TX)?),
                Unit::BytesPerSecond,
                "network.transmitBytesPerSecond.resetAware.perInterface.includesLoopback",
            );
        }
        PanelKind::Docker => {
            add(
                "containerCpu",
                format!("rate({}[5m])", metric(binding, CONTAINER_CPU)?),
                Unit::CpuCores,
                "container.cpuCores.notHostPercent.resetAware.nonemptyImage.preserveIdentity",
            );
            add(
                "containerMemory",
                metric(binding, CONTAINER_MEMORY)?,
                Unit::Bytes,
                "container.workingSetBytes.nonemptyImage.preserveIdentity",
            );
        }
        PanelKind::PostgreSql => {
            add(
                "postgresConnections",
                metric(binding, PG_CONNECTIONS)?,
                Unit::Connections,
                "postgres.currentBackends.perDatabase",
            );
            add(
                "postgresCommits",
                format!("rate({}[5m])", metric(binding, PG_COMMITS)?),
                Unit::TransactionsPerSecond,
                "postgres.commitsPerSecond.resetAware.perDatabase",
            );
        }
        PanelKind::Redis => {
            add(
                "redisMemory",
                metric(binding, REDIS_MEMORY)?,
                Unit::Bytes,
                "redis.usedMemoryBytes",
            );
            add(
                "redisClients",
                metric(binding, REDIS_CLIENTS)?,
                Unit::Connections,
                "redis.connectedClients",
            );
            add(
                "redisCommands",
                format!("rate({}[5m])", metric(binding, REDIS_COMMANDS)?),
                Unit::OperationsPerSecond,
                "redis.commandsPerSecond.resetAware",
            );
        }
        PanelKind::Nginx => {
            add(
                "nginxConnections",
                metric(binding, NGINX_CONNECTIONS)?,
                Unit::Connections,
                "nginx.activeConnections",
            );
            add(
                "nginxRequests",
                format!("rate({}[5m])", metric(binding, NGINX_REQUESTS)?),
                Unit::RequestsPerSecond,
                "nginx.requestsPerSecond.resetAware",
            );
        }
        _ => {}
    }
    Ok(queries)
}

impl MonitoringClient {
    /// The caller selects a Native-approved, unambiguous binding. No host/IP/name
    /// is inferred here. Missing exporter data produces a typed unavailable panel.
    pub async fn prepare_panel(
        &self,
        panel: PanelKind,
        binding: &HostBinding,
        start: f64,
        end: f64,
        step_seconds: f64,
    ) -> Result<PanelPreparation, Error> {
        if binding.source_id() != self.source.id {
            return Err(Error::new(
                ErrorCode::InvalidConfig,
                "Panel binding belongs to a different monitoring source",
            ));
        }
        if !valid_timestamp(start)
            || !valid_timestamp(end)
            || end < start
            || !step_seconds.is_finite()
            || step_seconds <= 0.0
        {
            return Err(invalid_query());
        }
        let expressions = match queries(panel, binding) {
            Ok(v) => v,
            Err(error) => {
                return Ok(unavailable(
                    panel,
                    UnavailableReason::SelectorConflict,
                    vec![],
                    None,
                    Some(error),
                ))
            }
        };
        let probe_expression = match probe_expression(panel, binding) {
            Ok(v) => v,
            Err(error) => {
                return Ok(unavailable(
                    panel,
                    UnavailableReason::SelectorConflict,
                    vec![],
                    None,
                    Some(error),
                ))
            }
        };
        if expressions
            .iter()
            .any(|q| q.expression.len() > self.budgets.max_query_bytes)
            || probe_expression.len() > self.budgets.max_query_bytes
        {
            return Ok(unavailable(
                panel,
                UnavailableReason::QueryBudget,
                vec![],
                None,
                None,
            ));
        }
        // Probe the selected historical end, rather than now: an exporter that
        // disappeared today must not disable its known historical panels.
        let probe = match self.query(&probe_expression, Some(end)).await {
            Ok(v) => v,
            Err(error) => {
                return Ok(unavailable(
                    panel,
                    UnavailableReason::ProbeFailed,
                    vec![],
                    None,
                    Some(error),
                ))
            }
        };
        Ok(prepare_from_probe(
            panel,
            binding,
            &self.budgets,
            expressions,
            probe,
            start,
            end,
            step_seconds,
        ))
    }
}
fn unavailable(
    panel: PanelKind,
    reason: UnavailableReason,
    missing_metrics: Vec<String>,
    probe: Option<QueryOutcome>,
    error: Option<Error>,
) -> PanelPreparation {
    PanelPreparation::Unavailable {
        panel,
        reason,
        missing_metrics,
        probe,
        error,
    }
}
#[allow(clippy::too_many_arguments)]
fn prepare_from_probe(
    panel: PanelKind,
    binding: &HostBinding,
    budgets: &Budgets,
    queries: Vec<PlannedQuery>,
    probe: QueryOutcome,
    start: f64,
    end: f64,
    step_seconds: f64,
) -> PanelPreparation {
    let available = present_metrics(panel, &probe);
    let Some(available) = available else {
        return unavailable(
            panel,
            UnavailableReason::ProbeFailed,
            vec![],
            Some(probe),
            None,
        );
    };
    let missing = panel
        .required_metrics()
        .iter()
        .filter(|name| !available.contains(**name))
        .map(|name| name.to_string())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return unavailable(
            panel,
            UnavailableReason::MissingMetrics,
            missing,
            Some(probe),
            None,
        );
    }
    // Total points across this whole panel, not merely each of its queries.
    let points_per_query = (budgets.max_total_points / queries.len().max(1) / budgets.max_series)
        .min(budgets.max_points_per_series);
    if points_per_query < 2 {
        return unavailable(
            panel,
            UnavailableReason::QueryBudget,
            vec![],
            Some(probe),
            None,
        );
    }
    let step = step_seconds.max((end - start) / (points_per_query - 1) as f64);
    PanelPreparation::Ready {
        plan: PanelPlan {
            panel,
            source_id: binding.source_id().into(),
            host_id: binding.host_id().to_string(),
            binding_revision: binding.revision(),
            start,
            end,
            step_seconds: step,
            max_series: budgets.max_series,
            max_points_per_series: points_per_query,
            max_total_points: budgets.max_total_points,
            queries,
        },
        probe,
    }
}
fn present_metrics(panel: PanelKind, probe: &QueryOutcome) -> Option<BTreeSet<String>> {
    if probe.status != "success" || probe.partial || probe.error.is_some() {
        return None;
    }
    let Some(QueryData::Vector { series }) = &probe.data else {
        return None;
    };
    let expected = panel.required_metrics();
    let mut present = BTreeSet::new();
    for series in series {
        if series.labels.len() != 1 || series.samples.len() != 1 {
            return None;
        }
        let name = series.labels.get("__name__")?;
        if !expected.contains(&name.as_str()) || !present.insert(name.clone()) {
            return None;
        }
        if !matches!(series.samples[0].value,SampleValue::Finite {value} if value.is_finite()&&value>=1.0&&value.fract()==0.0)
        {
            return None;
        }
    }
    Some(present)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    fn binding(value: &str) -> HostBinding {
        HostBinding::new(
            "5e7c5182-19df-48e1-99ed-87d28c9356e9",
            "fixture",
            1,
            BTreeMap::from([("instance".into(), value.into())]),
        )
        .unwrap()
    }
    fn probe(panel: PanelKind) -> QueryOutcome {
        QueryOutcome {
            status: "success".into(),
            http_status: 200,
            data: Some(QueryData::Vector {
                series: panel
                    .required_metrics()
                    .iter()
                    .map(|name| Series {
                        labels: BTreeMap::from([("__name__".into(), name.to_string())]),
                        samples: vec![Sample {
                            timestamp: 1.0,
                            value: SampleValue::Finite { value: 1.0 },
                        }],
                    })
                    .collect(),
            }),
            warnings: vec![],
            infos: vec![],
            partial: false,
            error: None,
            effective_step_seconds: None,
        }
    }
    #[test]
    fn templates_escape_literal_selectors_and_preserve_cpu_identity() {
        let binding = binding("node\"} or up{job=\"other\\line\n");
        let cpu = queries(PanelKind::Cpu, &binding).unwrap();
        assert!(cpu[0]
            .expression
            .contains("instance=\"node\\\"} or up{job=\\\"other\\\\line\\n\""));
        assert!(cpu[0].expression.contains("avg without (cpu, mode) (rate("));
        assert!(cpu[0].expression.contains("mode=\"idle\""));
        assert_eq!(cpu[0].unit, Unit::Percent);
        assert!(!cpu[0].expression.contains("sum("));
        let docker = queries(PanelKind::Docker, &binding).unwrap();
        assert!(docker[0].expression.contains("image!=\"\""));
        assert_eq!(docker[0].unit, Unit::CpuCores);
    }
    #[test]
    fn required_metric_probe_gates_every_panel_and_total_plan_budget() {
        for panel in [
            PanelKind::HostOverview,
            PanelKind::Cpu,
            PanelKind::Memory,
            PanelKind::Filesystem,
            PanelKind::DiskIo,
            PanelKind::Network,
            PanelKind::Docker,
            PanelKind::PostgreSql,
            PanelKind::Redis,
            PanelKind::Nginx,
        ] {
            let binding = binding("node");
            let queries = queries(panel, &binding).unwrap();
            let ready = prepare_from_probe(
                panel,
                &binding,
                &Budgets::default(),
                queries.clone(),
                probe(panel),
                0.0,
                86400.0,
                1.0,
            );
            let PanelPreparation::Ready { plan, .. } = ready else {
                panic!("panel should be ready");
            };
            assert!(plan.step_seconds >= 86400.0 / 999.0);
            assert!(
                plan.max_points_per_series * plan.max_series * plan.queries.len()
                    <= plan.max_total_points
            );
            let mut missing = probe(panel);
            if let Some(QueryData::Vector { series }) = &mut missing.data {
                series.pop();
            }
            assert!(matches!(
                prepare_from_probe(
                    panel,
                    &binding,
                    &Budgets::default(),
                    queries,
                    missing,
                    0.0,
                    86400.0,
                    1.0
                ),
                PanelPreparation::Unavailable {
                    reason: UnavailableReason::MissingMetrics,
                    ..
                }
            ));
        }
    }
    #[test]
    fn warnings_unknown_metrics_and_selector_conflicts_fail_closed() {
        let mut partial = probe(PanelKind::Cpu);
        partial.partial = true;
        partial.warnings.push("incomplete".into());
        assert!(present_metrics(PanelKind::Cpu, &partial).is_none());
        let mut wrong = probe(PanelKind::Cpu);
        if let Some(QueryData::Vector { series }) = &mut wrong.data {
            series[0]
                .labels
                .insert("__name__".into(), "other_metric".into());
        }
        assert!(present_metrics(PanelKind::Cpu, &wrong).is_none());
        let mut labels = Labels::new();
        labels.insert("mode".into(), "system".into());
        let binding =
            HostBinding::new("5e7c5182-19df-48e1-99ed-87d28c9356e9", "fixture", 1, labels).unwrap();
        assert!(queries(PanelKind::Cpu, &binding).is_err());
    }
}
