//! Optional original exporter-review metadata, never a grant or installation proof.
use super::*;
use crate::{automation_jobs::metric_context, session::Session};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MonitoringExporterBindingOrigin {
    version: u32,
    profile_id: String,
    notification_scope_id: String,
    host_id: String,
    host_revision: u64,
    source_id: String,
    source_revision: u64,
    job_id: String,
    job_revision: u64,
    project_id: String,
    revision_digest: String,
}

impl MonitoringExporterBindingOrigin {
    pub(super) async fn check(
        &self,
        owner: &Session,
        u: &Unlocked,
        host_id: &str,
        source_id: &str,
        source_revision: u64,
    ) -> AppResult<()> {
        for id in [
            &self.profile_id,
            &self.notification_scope_id,
            &self.host_id,
            &self.source_id,
            &self.job_id,
            &self.project_id,
        ] {
            canonical_id(id)?;
        }
        if self.version != 1
            || self.host_id != host_id
            || self.source_id != source_id
            || self.source_revision != source_revision
            || self.host_revision == 0
            || self.host_revision > 9_007_199_254_740_991
            || self.revision_digest.len() != 64
            || !self
                .revision_digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid("exporter binding review changed; verify again"));
        }
        // The actual successful saved job/approved target is re-read BEFORE
        // final synchronous scope/host checks, under the original writer guards.
        let current = metric_context::capture(
            owner,
            Uuid::parse_str(&self.profile_id).unwrap(),
            Uuid::parse_str(&self.job_id).unwrap(),
            self.job_revision,
            Uuid::parse_str(host_id).unwrap(),
        )
        .await?;
        let host = crate::dto::parse_id("monitoring_host", host_id)?;
        if owner.profile_id.to_string() != self.profile_id
            || u.notices.scope_id().as_deref() != Some(self.notification_scope_id.as_str())
            || u.working().correlation_host_metadata(host)
                != Some((self.host_revision as i64, "ssh"))
            || current.project.to_string() != self.project_id
            || current.revision_digest != self.revision_digest
        {
            return Err(invalid("exporter binding review changed; verify again"));
        }
        Ok(())
    }
}
