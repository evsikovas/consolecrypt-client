//! One-use, hash-bound desktop Helm rollback approval. The CLI sees only a
//! revocable local proxy and every write is still evaluated by ProdGuard.
use crate::kubernetes_cli::{self as cli, Binary};
use crate::{AppCore, AppError, AppResult};
use cc_k8s_core::resources::{ReadOptions, ResourceRef};
use serde::Serialize;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize)]
pub struct HelmRollbackReview {
    pub ticket: String,
    pub path: PathBuf,
    pub sha256: String,
    pub namespace: String,
    pub release: String,
    pub revision: u64,
    pub current_revision: u64,
    pub arguments: Vec<String>,
    pub expires_at_ms: i64,
}
pub(crate) struct HelmPlan {
    created: Instant,
    session_id: String,
    binary: Binary,
    review: HelmRollbackReview,
    fingerprint: String,
}
impl AppCore {
    pub async fn kube_helm_rollback_review(
        &self,
        session_id: String,
        namespace: String,
        release: String,
        revision: u64,
        manual_path: Option<String>,
    ) -> AppResult<HelmRollbackReview> {
        if cfg!(any(target_os = "android", target_os = "ios")) {
            return Err(AppError::invalid("kubernetes", "desktop_only"));
        }
        cli::identifier(&namespace)
            .and_then(|_| cli::identifier(&release))
            .map_err(super::kubernetes_flux::error)?;
        if revision == 0 || revision > i32::MAX as u64 {
            return Err(AppError::invalid("revision", "invalid_revision"));
        }
        let (owner, unlocked) = self.unlocked().await?;
        let live = unlocked.kube.get(&session_id)?;
        let (current_revision, fingerprint) =
            helm_state(&live, &namespace, &release, revision).await?;
        let path = manual_path
            .map(PathBuf::from)
            .or_else(|| cli::find("helm"))
            .ok_or_else(|| AppError::invalid("kubernetes", "helm_cli_not_found"))?;
        let binary = cli::inspect(&path).map_err(super::kubernetes_flux::error)?;
        let arguments = vec![
            "rollback".into(),
            release.clone(),
            revision.to_string(),
            "--namespace".into(),
            namespace.clone(),
            "--wait".into(),
            "--timeout=90s".into(),
            "--no-hooks".into(),
        ];
        let review = HelmRollbackReview {
            ticket: Uuid::new_v4().to_string(),
            path: binary.path.clone(),
            sha256: binary.hash.clone(),
            namespace,
            release,
            revision,
            current_revision,
            arguments,
            expires_at_ms: chrono::Utc::now().timestamp_millis() + 600_000,
        };
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        if live.cancel.is_cancelled() {
            return Err(AppError::VaultLocked);
        }
        let mut plans = unlocked
            .kube
            .cli
            .helm_plans
            .lock()
            .map_err(|_| AppError::VaultLocked)?;
        plans.retain(|_, plan| plan.created.elapsed() < Duration::from_secs(600));
        if plans.len() >= 4 {
            return Err(AppError::invalid("kubernetes", "too_many_reviews"));
        }
        plans.insert(
            review.ticket.clone(),
            HelmPlan {
                created: Instant::now(),
                session_id,
                binary,
                review: review.clone(),
                fingerprint,
            },
        );
        Ok(review)
    }
    pub async fn kube_helm_rollback_approve(&self, ticket: String) -> AppResult<Vec<u8>> {
        let (owner, unlocked) = self.unlocked().await?;
        let plan = unlocked
            .kube
            .cli
            .helm_plans
            .lock()
            .map_err(|_| AppError::VaultLocked)?
            .remove(&ticket)
            .ok_or_else(|| AppError::invalid("ticket", "expired"))?;
        if plan.created.elapsed() >= Duration::from_secs(600) {
            return Err(AppError::invalid("ticket", "expired"));
        }
        let live = unlocked.kube.get(&plan.session_id)?;
        let operation = unlocked
            .kube
            .cli
            .track_operation(&ticket, &live.cancel)
            .map_err(super::kubernetes_flux::error)?;
        if live.info.production && live.session.guard().write_window_remaining().is_none() {
            return Err(AppError::invalid("kubernetes", "read_only"));
        }
        let (_, fingerprint) = helm_state(
            &live,
            &plan.review.namespace,
            &plan.review.release,
            plan.review.revision,
        )
        .await?;
        if fingerprint != plan.fingerprint {
            return Err(AppError::invalid("kubernetes", "helm_history_changed"));
        }
        cli::unchanged(&plan.binary).map_err(super::kubernetes_flux::error)?;
        let lease = self.kube_cli_proxy(plan.session_id).await?;
        let check = cli::run(
            &plan.binary,
            &["version".into(), "--short".into()],
            Some(&lease.path()),
            Duration::from_secs(10),
            operation.cancellation.clone(),
        )
        .await
        .map_err(super::kubernetes_flux::error)?;
        let version = std::str::from_utf8(&check.stdout)
            .map_err(|_| AppError::invalid("kubernetes", "helm_cli_invalid"))?
            .trim();
        if check.exit_code != 0 || !(version.starts_with("v3.") || version.starts_with("v4.")) {
            return Err(AppError::invalid("kubernetes", "helm_cli_invalid"));
        }
        let mut arguments = plan.review.arguments;
        arguments.extend([
            "--kubeconfig".into(),
            lease.path().to_string_lossy().into_owned(),
            "--kube-context".into(),
            "consolecrypt".into(),
        ]);
        let result = cli::run(
            &plan.binary,
            &arguments,
            Some(&lease.path()),
            Duration::from_secs(120),
            operation.cancellation.clone(),
        )
        .await
        .map_err(super::kubernetes_flux::error)?;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        if lease.cancellation().is_cancelled() || operation.cancellation.is_cancelled() {
            return Err(AppError::VaultLocked);
        }
        super::kubernetes_flux::output_bytes(result)
    }
}
async fn helm_state(
    live: &crate::kubernetes_runtime::LiveSession,
    namespace: &str,
    name: &str,
    revision: u64,
) -> AppResult<(u64, String)> {
    let history = live
        .resources
        .helm_history(namespace, Some(name))
        .await
        .map_err(super::kubernetes_runtime::resource_error)?;
    let selected = history
        .iter()
        .find(|release| release.revision == revision)
        .ok_or_else(|| AppError::invalid("kubernetes", "helm_revision_not_found"))?;
    let current = history
        .iter()
        .max_by_key(|release| release.revision)
        .ok_or_else(|| AppError::invalid("kubernetes", "helm_revision_not_found"))?;
    if selected.status.starts_with("pending-")
        || current.status.starts_with("pending-")
        || current.revision == revision
    {
        return Err(AppError::invalid("kubernetes", "helm_rollback_unavailable"));
    }
    let kind = live
        .resources
        .resolve("", "v1", "secrets")
        .map_err(super::kubernetes_runtime::resource_error)?;
    let mut fingerprint = String::new();
    for release in [current, selected] {
        let reference = ResourceRef {
            api: kind.clone(),
            namespace: Some(namespace.into()),
            name: release.secret_name.clone(),
        };
        let object = live
            .resources
            .get(&reference, ReadOptions::default())
            .await
            .map_err(super::kubernetes_runtime::resource_error)?;
        let uid = object.object["metadata"]["uid"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| AppError::invalid("kubernetes", "helm_history_changed"))?;
        let resource_version = object.object["metadata"]["resourceVersion"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| AppError::invalid("kubernetes", "helm_history_changed"))?;
        fingerprint.push_str(&format!(
            "{}:{}:{}:{};",
            uid.len(),
            uid,
            resource_version.len(),
            resource_version
        ));
    }
    Ok((current.revision, fingerprint))
}

#[cfg(all(test, unix))]
#[path = "kubernetes_helm_cli_tests.rs"]
mod tests;
