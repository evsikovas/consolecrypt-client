//! Fixed saved-panel read. Caller supplies identity/window, never an expression.
use super::{invalid, ReadAction};
use crate::monitoring_sources::{panel_for_request, MonitoringPanelDefinition};
use crate::session::Unlocked;
use crate::AppResult;
use cc_monitoring_core::MonitoringClient;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SavedPanelRange {
    start: f64,
    end: f64,
    step_seconds: f64,
}
pub(super) struct SavedPanelRead {
    id: String,
    revision: u64,
    definition: MonitoringPanelDefinition,
    range: Option<SavedPanelRange>,
}
impl SavedPanelRead {
    pub(super) fn prepare(
        u: &Unlocked,
        action: &ReadAction,
        source: &str,
        source_revision: u64,
    ) -> AppResult<Option<Self>> {
        let ReadAction::SavedPanel {
            panel_id,
            panel_revision,
            range,
        } = action
        else {
            return Ok(None);
        };
        if let Some(range) = range {
            if !range.start.is_finite()
                || !range.end.is_finite()
                || range.start.abs() > 253_402_300_799.0
                || range.end.abs() > 253_402_300_799.0
                || range.end < range.start
                || !range.step_seconds.is_finite()
                || range.step_seconds <= 0.0
            {
                return Err(invalid("invalid saved panel range"));
            }
        }
        let definition = panel_for_request(u, panel_id, *panel_revision)?;
        if definition.source_id != source || definition.source_revision != source_revision {
            return Err(invalid("saved panel source differs from requested source"));
        }
        Ok(Some(Self {
            id: panel_id.clone(),
            revision: *panel_revision,
            definition,
            range: *range,
        }))
    }
    pub(super) async fn execute(self, client: &MonitoringClient) -> Value {
        let result = match self.range {
            None => client.query(&self.definition.query, None).await,
            Some(range) => {
                client
                    .query_range(
                        &self.definition.query,
                        range.start,
                        range.end,
                        range.step_seconds,
                    )
                    .await
            }
        };
        // Same bounded Core query transport/parser; no result cache or vault
        // write. Unit is a user hint only, never a numeric conversion here.
        json!({"kind":"savedPanel","panelId":self.id,"panelRevision":self.revision,"unitHint":self.definition.unit_hint,
            "outcome":result.map_or_else(|error|json!({"error":error}),|outcome|json!(outcome))})
    }
}
