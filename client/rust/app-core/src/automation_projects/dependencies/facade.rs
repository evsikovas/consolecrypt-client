//! Typed public AppCore facade only. No Bridge/UI/capability registration,
//! automatic acceptance, Helper command or credential resolver. Publication
//! consumes only an already retained Native acceptance; no approval flag.
use super::*;
use futures::FutureExt;
use cc_automation_core::{DependencyArtifactSource, DependencyKind};
use serde::Deserialize;
use std::sync::OnceLock;
const INPUT: usize = 8 * 1024;
const OUTPUT: usize = 4 * 1024 * 1024;
static CALLS: OnceLock<Arc<Semaphore>> = OnceLock::new();
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    version: u8,
    action: Action,
}
#[derive(Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Action {
    Review {
        project_id: String,
        project_revision: u64,
        runner_id: String,
        runner_revision: u64,
        dependency: Proposal,
    },
    Start {
        review_id: String,
    },
    Status {
        operation_id: String,
    },
    Evidence {
        operation_id: String,
        review_digest: String,
    },
    Changes {
        operation_id: String,
        review_digest: String,
        offset: u32,
        limit: u32,
    },
    File {
        operation_id: String,
        review_digest: String,
        path: String,
    },
    Accept {
        operation_id: String,
        review_digest: String,
    },
    Reject {
        operation_id: String,
        review_digest: String,
    },
    Publish {
        operation_id: String,
        review_digest: String,
    },
    Cancel {
        operation_id: String,
    },
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Proposal {
    kind: DependencyKind,
    name: String,
    version: String,
    url: String,
    archive_sha256: String,
}
impl Proposal {
    fn selected(self) -> AppResult<DependencySourceSelection> {
        DependencySourceSelection::new(
            self.kind,
            self.name,
            self.version,
            DependencyArtifactSource {
                url: self.url,
                sha256: self.archive_sha256,
            },
            0,
        )
        .map_err(|_| invalid("dependency_invalid_source"))
    }
}
fn parse(raw: &str) -> AppResult<Frame> {
    if raw.len() > INPUT {
        return Err(invalid("dependency_invalid_request"));
    }
    let frame: Frame =
        serde_json::from_str(raw).map_err(|_| invalid("dependency_invalid_request"))?;
    if frame.version != 1 {
        return Err(invalid("dependency_invalid_request"));
    }
    Ok(frame)
}
fn output(kind: &'static str, data: impl Serialize) -> AppResult<String> {
    let encoded = serde_json::to_string(&json!({"version":1,"kind":kind,"data":data}))
        .map_err(|_| invalid("dependency_evidence_unavailable"))?;
    if encoded.len() > OUTPUT {
        return Err(invalid("dependency_output_budget"));
    }
    Ok(encoded)
}
fn fixed(error: AppError) -> AppError {
    const CODES: &[&str] = &[
        "dependency_busy",
        "dependency_context_budget",
        "dependency_context_changed",
        "dependency_invalid_request",
        "dependency_invalid_source",
        "dependency_invalid_page",
        "dependency_invalid_review",
        "dependency_cancelled",
        "dependency_cancel_budget",
        "dependency_operation_unavailable",
        "dependency_review_budget",
        "dependency_review_changed",
        "dependency_review_expired",
        "dependency_existing_receipt_unsupported",
        "dependency_output_budget",
        "dependency_evidence_unavailable",
        "dependency_publication_busy",
        "dependency_publication_unsupported",
        "dependency_publication_recovery_required",
    ];
    match error {
        AppError::VaultLocked => AppError::VaultLocked,
        AppError::NoActiveProfile => AppError::NoActiveProfile,
        AppError::InvalidInput { field, reason } if field == "automation_dependency" => invalid(
            CODES
                .iter()
                .copied()
                .find(|code| *code == reason)
                .unwrap_or("dependency_evidence_unavailable"),
        ),
        _ => invalid("dependency_evidence_unavailable"),
    }
}
impl AppCore {
    /// Source URL is a proposal, not an execution/network authority. Explicit
    /// start consumes ONLY a previously captured Native one-use review id.
    /// Accept retains a reviewed candidate; it NEVER installs/publishes/runs it.
    pub async fn automation_dependency_action_json(&self, payload: String) -> AppResult<String> {
        let _call = CALLS
            .get_or_init(|| Arc::new(Semaphore::new(4)))
            .clone()
            .try_acquire_owned()
            .map_err(|_| invalid("dependency_busy"))?;
        self.dependency_action_inner(&payload).await.map_err(fixed)
    }
    async fn dependency_action_inner(&self, payload: &str) -> AppResult<String> {
        match parse(payload)?.action {
            Action::Review {
                project_id,
                project_revision,
                runner_id,
                runner_revision,
                dependency,
            } => {
                if project_revision == 0 || runner_revision == 0 {
                    return Err(invalid("dependency_invalid_request"));
                }
                let selected = dependency.selected()?;
                output(
                    "sourceReview",
                    self.automation_dependency_review(
                        project_id,
                        project_revision,
                        runner_id,
                        runner_revision,
                        selected,
                    )
                    .await?,
                )
            }
            Action::Start { review_id } => output(
                "operation",
                self.automation_dependency_start(review_id).await?,
            ),
            Action::Status { operation_id } => output(
                "operation",
                self.automation_dependency_status(operation_id).await?,
            ),
            Action::Evidence {
                operation_id,
                review_digest,
            } => output(
                "evidence",
                self.dependency_review_evidence(operation_id, review_digest)
                    .await?,
            ),
            Action::Changes {
                operation_id,
                review_digest,
                offset,
                limit,
            } => output(
                "changes",
                self.automation_dependency_changes(operation_id, review_digest, offset, limit)
                    .await?,
            ),
            Action::File {
                operation_id,
                review_digest,
                path,
            } => output(
                "file",
                self.automation_dependency_file(operation_id, review_digest, path)
                    .await?,
            ),
            Action::Accept {
                operation_id,
                review_digest,
            } => {
                let accepted = self
                    .automation_dependency_accept(operation_id, review_digest.clone())
                    .await?;
                let view = accepted.retain_in_registry(self, &review_digest).await?;
                output(
                    "acceptedCandidate",
                    json!({"operation":view,"retainedCandidate":true,"published":false,"executionEnabled":false}),
                )
            }
            Action::Publish { operation_id, review_digest } => output(
                "published",
                self.automation_dependency_publish(operation_id, review_digest).boxed().await?,
            ),
            Action::Reject {
                operation_id,
                review_digest,
            } => {
                self.automation_dependency_reject(operation_id, review_digest)
                    .await?;
                output(
                    "rejected",
                    json!({"published":false,"executionEnabled":false}),
                )
            }
            Action::Cancel { operation_id } => {
                self.automation_dependency_cancel(operation_id).await?;
                output(
                    "cancelled",
                    json!({"published":false,"executionEnabled":false}),
                )
            }
        }
    }
}
#[cfg(test)]
mod tests;
