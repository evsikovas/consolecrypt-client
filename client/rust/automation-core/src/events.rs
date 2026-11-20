//! Untrusted journal pages are checked before a cursor or a task reaches UI.
use crate::{AutomationError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobEvent {
    pub version: u8,
    pub job_id: Uuid,
    pub sequence: u64,
    pub timestamp: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub payload: Value,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobEventPage {
    pub job_id: Uuid,
    pub manifest_digest: String,
    pub events: Vec<JobEvent>,
    pub cursor: u64,
    pub output_truncated: bool,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_retirement: Option<JobArtifactRetirement>,
}
/// Explicit missing raw output, not a cursor reset, synthetic event or grant.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobArtifactRetirement {
    pub version: u8,
    pub retired_at: u64,
    pub last_sequence: u64,
    pub output_bytes: u64,
}
impl std::fmt::Debug for JobEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobEvent")
            .field("job", &self.job_id)
            .field("sequence", &self.sequence)
            .finish_non_exhaustive()
    }
}
impl std::fmt::Debug for JobEventPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobEventPage")
            .field("cursor", &self.cursor)
            .field("count", &self.events.len())
            .finish_non_exhaustive()
    }
}
impl JobEventPage {
    pub fn parse(
        value: Value,
        job: Uuid,
        manifest_digest: &str,
        after: u64,
        limit: u16,
    ) -> Result<Self> {
        if !(1..=500).contains(&limit) || after > i64::MAX as u64 {
            return Err(AutomationError::Invalid("invalid event page"));
        }
        let page: Self = serde_json::from_value(value)
            .map_err(|_| AutomationError::Protocol("invalid job event page"))?;
        if page.job_id != job
            || !cc_models::automation::is_digest(manifest_digest)
            || page.manifest_digest != manifest_digest
        {
            return Err(AutomationError::Protocol(
                "journal belongs to another approved job",
            ));
        }
        if page.events.len() > usize::from(limit)
            || !matches!(
                page.state.as_str(),
                "queued"
                    | "running"
                    | "cancelling"
                    | "succeeded"
                    | "failed"
                    | "cancelled"
                    | "interrupted"
                    | "unknown"
            )
        {
            return Err(AutomationError::Protocol("invalid job event state or size"));
        }
        if let Some(retired) = &page.artifact_retirement {
            if retired.version != 1
                || retired.retired_at == 0
                || retired.retired_at > 253402300799
                || retired.last_sequence > 1_000_000_000
                || retired.output_bytes > 50 * 1024 * 1024
                || !page.events.is_empty()
                || after > retired.last_sequence
                || !matches!(
                    page.state.as_str(),
                    "succeeded" | "failed" | "cancelled" | "interrupted" | "unknown"
                )
            {
                return Err(AutomationError::Protocol("invalid job artifact retirement"));
            }
        }
        let mut cursor = after;
        for event in &page.events {
            let printable = |value: &str, max: usize| {
                !value.is_empty()
                    && value.len() <= max
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_graphic() || byte == b' ')
            };
            if event.version != 1
                || event.job_id != job
                || Some(event.sequence) != cursor.checked_add(1)
                || !printable(&event.kind, 64)
                || !printable(&event.timestamp, 64)
            {
                return Err(AutomationError::Protocol(
                    "job events contain a gap, replay or another job",
                ));
            }
            cursor = event.sequence;
        }
        if page.cursor != cursor {
            return Err(AutomationError::Protocol(
                "event cursor differs from verified events",
            ));
        }
        Ok(page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn page(job: Uuid) -> Value {
        json!({"job_id":job,"manifest_digest":"a".repeat(64),"events":[{"version":1,"job_id":job,"sequence":5,"timestamp":"2026-11-10T12:00:00Z",
        "type":"runner_on_ok","payload":{"task":"synthetic event"}}],"cursor":5,"output_truncated":false,"state":"running"})
    }
    #[test]
    fn exact_job_and_contiguous_cursor_are_required_before_publishing_a_page() {
        let job = Uuid::new_v4();
        let value = page(job);
        assert_eq!(
            JobEventPage::parse(value.clone(), job, &"a".repeat(64), 4, 10)
                .unwrap()
                .cursor,
            5
        );
        assert!(
            JobEventPage::parse(value.clone(), Uuid::new_v4(), &"a".repeat(64), 4, 10).is_err()
        );
        assert!(JobEventPage::parse(value.clone(), job, &"f".repeat(64), 4, 10).is_err());
        assert!(JobEventPage::parse(value.clone(), job, &"a".repeat(64), 5, 10).is_err());
        let mut gap = value.clone();
        gap["events"][0]["sequence"] = json!(6);
        gap["cursor"] = json!(6);
        assert!(JobEventPage::parse(gap, job, &"a".repeat(64), 4, 10).is_err());
        let mut cursor = value;
        cursor["cursor"] = json!(99);
        assert!(JobEventPage::parse(cursor, job, &"a".repeat(64), 4, 10).is_err());
    }
    #[test]
    fn retired_raw_history_is_explicit_and_never_fabricates_cursor_or_events() {
        let job = Uuid::new_v4();
        let raw = json!({"job_id":job,"manifest_digest":"a".repeat(64),"events":[],"cursor":4,"output_truncated":false,"state":"unknown",
            "artifact_retirement":{"version":1,"retired_at":1800000000u64,"last_sequence":7,"output_bytes":1024}});
        let parsed = JobEventPage::parse(raw.clone(), job, &"a".repeat(64), 4, 10).unwrap();
        assert_eq!(parsed.cursor, 4);
        assert!(parsed.events.is_empty());
        // This owned synthetic page has passed the production validator. Capture
        // its actual serializer output for the independent frontend contract.
        let wire = serde_json::to_string(&parsed).unwrap();
        assert!(wire.len() <= 4096);
        println!("CC_RETENTION_SYNTHETIC_PAGE={wire}");
        assert_eq!(parsed.artifact_retirement.unwrap().last_sequence, 7);
        for field in ["version", "unexpected"] {
            let mut future = raw.clone();
            future["artifact_retirement"][field] = json!(2);
            assert!(JobEventPage::parse(future, job, &"a".repeat(64), 4, 10).is_err());
        }
        let mut running = raw.clone();
        running["state"] = json!("running");
        assert!(JobEventPage::parse(running, job, &"a".repeat(64), 4, 10).is_err());
        let mut cursor = raw;
        cursor["cursor"] = json!(7);
        assert!(JobEventPage::parse(cursor, job, &"a".repeat(64), 4, 10).is_err());
    }
    #[test]
    fn empty_pages_keep_the_cursor_without_inventing_events() {
        let job = Uuid::new_v4();
        let page = json!({"job_id":job,"manifest_digest":"a".repeat(64),"events":[],"cursor":42,"output_truncated":true,"state":"succeeded"});
        let parsed = JobEventPage::parse(page, job, &"a".repeat(64), 42, 100).unwrap();
        assert!(parsed.events.is_empty());
        assert!(parsed.output_truncated);
    }
}
