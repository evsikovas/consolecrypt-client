//! Admission provenance DATA. Never an execution, credential or navigation grant.
use crate::{AppError, AppResult};
use cc_models::automation::{is_digest, JobManifest};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct JobProvenance {
    version: u8,
    job_id: Uuid,
    profile_id: Uuid,
    device_id: Uuid,
    project_id: Uuid,
    project_revision: u64,
    runner_id: Uuid,
    revision_object_id: Uuid,
    revision_digest: String,
}

fn invalid() -> AppError {
    AppError::invalid("automation_job", "job_provenance_invalid")
}

impl JobProvenance {
    /// Caller has verified the real SignedJobApproval, exact enrolled device
    /// key, original project revision and original uploaded snapshot. These
    /// private typed arguments are never populated by a Bridge JSON request.
    pub(super) fn admitted(
        manifest: &JobManifest,
        original_profile: Uuid,
        original_project: Uuid,
        signer_device: Uuid,
        original_project_revision: u64,
        uploaded_revision: Uuid,
    ) -> AppResult<Self> {
        if manifest.version != 1
            || manifest.tenant_id != original_profile
            || manifest.project_id != original_project
            || [
                original_profile,
                original_project,
                signer_device,
                uploaded_revision,
                manifest.runner_id,
                manifest.job_id,
            ]
            .iter()
            .any(Uuid::is_nil)
            || !(1..=9_007_199_254_740_991).contains(&original_project_revision)
            || !is_digest(&manifest.revision_digest)
        {
            return Err(invalid());
        }
        Ok(Self {
            version: 1,
            job_id: manifest.job_id,
            profile_id: original_profile,
            device_id: signer_device,
            project_id: original_project,
            project_revision: original_project_revision,
            runner_id: manifest.runner_id,
            revision_object_id: uploaded_revision,
            revision_digest: manifest.revision_digest.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_models::automation::{ApprovedTarget, JobMode, JobPolicy};

    fn manifest() -> JobManifest {
        JobManifest {
            version: 1,
            job_id: Uuid::new_v4(),
            tenant_id: Uuid::new_v4(),
            runner_id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            revision_digest: "a".repeat(64),
            inventory_digest: "b".repeat(64),
            environment_digest: "c".repeat(64),
            parameters_digest: "d".repeat(64),
            playbook: "playbook.yml".into(),
            policy: JobPolicy {
                strict_isolation: true,
                cancel_on_lock: true,
                timeout_seconds: 60,
                forks: 1,
                production: false,
            },
            mode: JobMode::Check,
            targets: vec![ApprovedTarget {
                host_id: Uuid::new_v4(),
                alias: "owned".into(),
                address: "127.0.0.1".into(),
                port: 22,
                username: "fixture".into(),
                route_digest: "e".repeat(64),
                credential_refs: vec![],
            }],
            issued_at: 100,
            expires_at: 200,
            access_expires_at: 160,
            nonce: Uuid::new_v4(),
        }
    }

    #[test]
    fn native_signer_and_uploaded_object_are_distinct_from_content_digest_and_review_id() {
        let m = manifest();
        let signer = Uuid::new_v4();
        let uploaded = Uuid::new_v4();
        let view =
            JobProvenance::admitted(&m, m.tenant_id, m.project_id, signer, 7, uploaded).unwrap();
        let raw = serde_json::to_value(&view).unwrap();
        assert_eq!(raw["deviceId"], signer.to_string());
        assert_eq!(raw["revisionObjectId"], uploaded.to_string());
        assert_eq!(raw["revisionDigest"], m.revision_digest);
        assert_eq!(raw["projectRevision"], 7);
        assert_eq!(raw["jobId"], m.job_id.to_string());
        assert_eq!(raw.as_object().unwrap().len(), 9);
        assert!(raw.get("approved").is_none());
        assert!(raw.get("executionStartedUtc").is_none());
    }

    #[test]
    fn original_profile_project_and_bounded_actual_revision_cannot_be_replaced() {
        let m = manifest();
        let signer = Uuid::new_v4();
        let uploaded = Uuid::new_v4();
        for (profile, project, revision) in [
            (Uuid::new_v4(), m.project_id, 1),
            (m.tenant_id, Uuid::new_v4(), 1),
            (m.tenant_id, m.project_id, 0),
            (m.tenant_id, m.project_id, 9_007_199_254_740_992),
        ] {
            assert!(
                JobProvenance::admitted(&m, profile, project, signer, revision, uploaded).is_err()
            );
        }
        assert!(
            JobProvenance::admitted(&m, m.tenant_id, m.project_id, Uuid::nil(), 1, uploaded)
                .is_err()
        );
        assert!(
            JobProvenance::admitted(&m, m.tenant_id, m.project_id, signer, 1, Uuid::nil()).is_err()
        );
    }

    #[test]
    fn future_or_invalid_manifest_does_not_invent_provenance() {
        let mut m = manifest();
        let signer = Uuid::new_v4();
        let uploaded = Uuid::new_v4();
        m.version = 2;
        assert!(
            JobProvenance::admitted(&m, m.tenant_id, m.project_id, signer, 1, uploaded).is_err()
        );
        m.version = 1;
        m.revision_digest = "future-or-invalid".into();
        assert!(
            JobProvenance::admitted(&m, m.tenant_id, m.project_id, signer, 1, uploaded).is_err()
        );
    }
}
