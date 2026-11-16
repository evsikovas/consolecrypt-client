//! Native-only identity discovery and typed panels. No caller PromQL, URL or
//! claimed resolution can authorize a panel. Every request resolves again.
use super::{ReadAction, invalid};
use crate::monitoring_sources::{
    BindingDiscoveryPlan, binding_for_request, discovery_plan_for_binding, resolve_discovery,
};
use crate::session::{Session, Unlocked};
use crate::{AppCore, AppResult};
use cc_host_mapping::{HostBinding, Resolution};
use cc_monitoring_core::panels::{PanelKind, PanelPreparation};
use cc_monitoring_core::{Budgets, Error, ErrorCode, MonitoringClient, QueryData, QueryOutcome};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Clone, Copy)]
enum Kind {
    Discover,
    Panel {
        panel: PanelKind,
        start: f64,
        end: f64,
        step: f64,
    },
}
pub(super) struct BindingRead {
    id: String,
    revision: u64,
    binding: HostBinding,
    plan: BindingDiscoveryPlan,
    kind: Kind,
}
impl BindingRead {
    pub(super) fn prepare(
        u: &Unlocked,
        action: &ReadAction,
        source: &str,
        source_revision: u64,
    ) -> AppResult<Option<Self>> {
        let (id, revision, end, kind) = match action {
            ReadAction::BindingDiscover {
                binding_id,
                binding_revision,
                time,
            } => (binding_id, *binding_revision, *time, Kind::Discover),
            ReadAction::BindingPanel {
                binding_id,
                binding_revision,
                panel,
                start,
                end,
                step_seconds,
            } => {
                if !start.is_finite()
                    || start.abs() > 253_402_300_799.0
                    || end < start
                    || !step_seconds.is_finite()
                    || *step_seconds <= 0.0
                {
                    return Err(invalid("invalid binding panel time range"));
                }
                (
                    binding_id,
                    *binding_revision,
                    *end,
                    Kind::Panel {
                        panel: *panel,
                        start: *start,
                        end: *end,
                        step: *step_seconds,
                    },
                )
            }
            _ => return Ok(None),
        };
        let binding = binding_for_request(u, id, revision, source_revision)?;
        if binding.source_id() != source {
            return Err(invalid("binding source differs from requested source"));
        }
        let plan = discovery_plan_for_binding(u, id, revision, source_revision, end)?;
        Ok(Some(Self {
            id: id.clone(),
            revision,
            binding,
            plan,
            kind,
        }))
    }
    pub(super) fn host_id(&self) -> String {
        self.binding.host_id().to_string()
    }

    pub(super) async fn execute(
        self,
        app: &AppCore,
        owner: &Arc<Session>,
        u: &Arc<Unlocked>,
        client: &MonitoringClient,
    ) -> AppResult<Value> {
        let mut response = json!({
            "kind": if matches!(self.kind, Kind::Discover) { "bindingDiscovery" } else { "bindingPanel" },
            "bindingId": self.id, "bindingRevision": self.revision, "state": "unavailable"
        });
        if let Kind::Panel { panel, .. } = self.kind {
            response["panel"] = json!(panel);
        }
        let discovery = match client
            .query(self.plan.expression(), Some(self.plan.selected_end()))
            .await
        {
            Ok(outcome) => outcome,
            Err(error) => {
                response["reason"] = json!("discoveryFailed");
                response["error"] = json!(error);
                return Ok(response);
            }
        };
        response["discovery"] = json!(discovery);
        let resolution = {
            let _profile = app.hold_session(owner).await?;
            let _unlock = owner.hold_unlocked(u).await?;
            resolve_discovery(
                u,
                &self.plan,
                self.plan.source_id(),
                self.plan.source_revision(),
                self.plan.selected_end(),
                &discovery,
            )
        };
        let Ok(resolution) = resolution else {
            response["reason"] = json!("discoveryInvalid");
            return bounded(response);
        };
        response["resolution"] = json!(resolution);
        response["state"] = json!(match resolution {
            Resolution::Resolved { .. } => "resolved",
            Resolution::Missing => "missing",
            Resolution::Ambiguous { .. } => "ambiguous",
        });
        if !matches!(resolution, Resolution::Resolved { .. }) {
            return bounded(response);
        }
        let Kind::Panel {
            panel,
            start,
            end,
            step,
        } = self.kind
        else {
            return bounded(response);
        };
        let preparation = match client
            .prepare_panel(panel, &self.binding, start, end, step)
            .await
        {
            Ok(PanelPreparation::Ready { plan, probe })
                if probe.http_status == 200 && probe.warnings.is_empty() =>
            {
                PanelPreparation::Ready { plan, probe }
            }
            Ok(PanelPreparation::Ready { probe, .. }) => PanelPreparation::Unavailable {
                panel,
                reason: cc_monitoring_core::panels::UnavailableReason::ProbeFailed,
                missing_metrics: vec![],
                probe: Some(probe),
                error: None,
            },
            Ok(value) => value,
            Err(error) => {
                response["state"] = json!("unavailable");
                response["reason"] = json!("metricProbeFailed");
                response["error"] = json!(error);
                return bounded(response);
            }
        };
        response["preparation"] = json!(preparation);
        let PanelPreparation::Ready { plan, .. } = preparation else {
            response["state"] = json!("unavailable");
            return bounded(response);
        };
        let budgets = Budgets::default();
        let mut total_series = 0usize;
        let mut total_points = 0usize;
        let mut payload_bytes = serde_json::to_vec(&response)
            .map_err(|_| invalid("panel response encoding failed"))?
            .len();
        let mut queries = Vec::new();
        let mut partial = false;
        let mut any_samples = false;
        for query in &plan.queries {
            let result = client
                .query_range(&query.expression, plan.start, plan.end, plan.step_seconds)
                .await;
            let mut row = json!({"id":query.id,"unit":query.unit,"semantics":query.semantics});
            match result {
                Ok(outcome) => {
                    let (series, points) = counts(&outcome);
                    total_series = total_series.saturating_add(series);
                    total_points = total_points.saturating_add(points);
                    any_samples |= points > 0;
                    partial |= outcome.http_status != 200
                        || outcome.status != "success"
                        || outcome.partial
                        || !outcome.warnings.is_empty()
                        || outcome.error.is_some()
                        || !matches!(outcome.data, Some(QueryData::Matrix { .. }));
                    row["outcome"] = json!(outcome);
                }
                Err(error) => {
                    row["error"] = json!(error);
                    partial = true;
                }
            }
            let row_bytes = serde_json::to_vec(&row)
                .map_err(|_| invalid("panel response encoding failed"))?
                .len();
            payload_bytes = payload_bytes.saturating_add(row_bytes);
            if total_series > budgets.max_series
                || total_points > budgets.max_total_points
                || payload_bytes > 16 * 1024 * 1024 - 4096
            {
                response["limited"] = json!(true);
                response["error"] = json!(Error {
                    code: ErrorCode::BudgetExceeded,
                    message: "Combined panel result budget exceeded".into(),
                    http_status: None
                });
                partial = true;
                break;
            }
            queries.push(row);
        }
        response["queries"] = json!(queries);
        response["state"] = json!(if partial {
            "partial"
        } else if any_samples {
            "ready"
        } else {
            "noData"
        });
        bounded(response)
    }
}
fn counts(value: &QueryOutcome) -> (usize, usize) {
    match &value.data {
        Some(QueryData::Matrix { series } | QueryData::Vector { series }) => {
            (series.len(), series.iter().map(|s| s.samples.len()).sum())
        }
        Some(QueryData::Scalar { .. } | QueryData::String { .. }) => (1, 1),
        None => (0, 0),
    }
}
fn bounded(value: Value) -> AppResult<Value> {
    if serde_json::to_vec(&value)
        .map_err(|_| invalid("panel response encoding failed"))?
        .len()
        > 16 * 1024 * 1024
    {
        return Err(invalid("binding read result exceeds budget"));
    }
    Ok(value)
}
