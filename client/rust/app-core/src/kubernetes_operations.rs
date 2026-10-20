//! Typed native mutations; the guard checks the current policy again at send.
use super::kubernetes_runtime::resource_error;
use crate::{AppCore, AppError, AppResult};
use cc_k8s_core::resources::{
    DebugContainer, DrainCandidate, ManifestDiff, ResourceDetail, ResourceKind, ResourcePage,
    ResourceRef, ResourceSchema,
};
use zeroize::{Zeroize, Zeroizing};

impl AppCore {
    pub async fn kube_related_objects(
        &self,
        session_id: String,
        reference: ResourceRef,
    ) -> AppResult<cc_k8s_core::resources::ResourceRelations> {
        self.with_kube_session(&session_id, |live| async move {
            live.resources
                .related_objects(&reference)
                .await
                .map_err(resource_error)
        })
        .await
    }
    pub async fn kube_rollout_history(
        &self,
        session_id: String,
        reference: ResourceRef,
    ) -> AppResult<Vec<cc_k8s_core::resources::WorkloadRevision>> {
        self.with_kube_session(&session_id, |live| async move {
            live.resources
                .rollout_history(&reference)
                .await
                .map_err(resource_error)
        })
        .await
    }
    pub async fn kube_rollout_diff(
        &self,
        session_id: String,
        reference: ResourceRef,
        before: u64,
        after: u64,
    ) -> AppResult<ManifestDiff> {
        self.with_kube_session(&session_id, |live| async move {
            live.resources
                .diff_revisions(&reference, before, after)
                .await
                .map_err(resource_error)
        })
        .await
    }
    pub async fn kube_secret_reveal(
        &self,
        session_id: String,
        reference: ResourceRef,
    ) -> AppResult<Zeroizing<Vec<u8>>> {
        self.with_kube_session(&session_id, |live| async move {
            if !reference.api.group.is_empty() || reference.api.resource != "secrets" {
                return Err(AppError::invalid("kubernetes", "not_secret"));
            }
            let mut detail = live
                .resources
                .get(
                    &reference,
                    cc_k8s_core::resources::ReadOptions {
                        reveal_secret_values: true,
                        include_config_map_values: false,
                    },
                )
                .await
                .map_err(resource_error)?;
            let bytes = serde_json::to_vec(&detail.object["data"])
                .map(Zeroizing::new)
                .map_err(|_| AppError::invalid("kubernetes", "invalid_secret"));
            crate::kubernetes_inventory::wipe_json(&mut detail.object);
            crate::kubernetes_inventory::wipe_json(&mut detail.summary.object);
            detail.yaml.zeroize();
            if live.cancel.is_cancelled() {
                return Err(AppError::Cancelled);
            }
            bytes
        })
        .await
    }
    pub async fn kube_scale(
        &self,
        session_id: String,
        reference: ResourceRef,
        resource_version: String,
        replicas: u32,
    ) -> AppResult<()> {
        self.with_kube_session(&session_id, |live| async move {
            live.resources
                .scale(&reference, &resource_version, replicas)
                .await
                .map_err(resource_error)
        })
        .await
    }
    pub async fn kube_restart(
        &self,
        session_id: String,
        reference: ResourceRef,
        resource_version: String,
    ) -> AppResult<()> {
        self.with_kube_session(&session_id, |live| async move {
            live.resources
                .restart(&reference, &resource_version)
                .await
                .map_err(resource_error)
        })
        .await
    }
    pub async fn kube_cordon(
        &self,
        session_id: String,
        node: String,
        resource_version: String,
        unschedulable: bool,
    ) -> AppResult<()> {
        self.with_kube_session(&session_id, |live| async move {
            live.resources
                .cordon(&node, &resource_version, unschedulable)
                .await
                .map_err(resource_error)
        })
        .await
    }
    pub async fn kube_drain_plan(
        &self,
        session_id: String,
        node: String,
    ) -> AppResult<Vec<DrainCandidate>> {
        self.with_kube_session(&session_id, |live| async move {
            live.resources
                .drain_plan(&node)
                .await
                .map_err(resource_error)
        })
        .await
    }
    pub async fn kube_evict_pod(
        &self,
        session_id: String,
        reference: ResourceRef,
        uid: String,
        resource_version: String,
    ) -> AppResult<()> {
        self.with_kube_session(&session_id, |live| async move {
            live.resources
                .evict_pod(&reference, &uid, &resource_version)
                .await
                .map_err(resource_error)
        })
        .await
    }
    pub async fn kube_debug_container(
        &self,
        session_id: String,
        reference: ResourceRef,
        resource_version: String,
        container: DebugContainer,
    ) -> AppResult<()> {
        self.with_kube_session(&session_id, |live| async move {
            live.resources
                .debug_container(&reference, &resource_version, container)
                .await
                .map_err(resource_error)
        })
        .await
    }
    pub async fn kube_workload_pods(
        &self,
        session_id: String,
        reference: ResourceRef,
    ) -> AppResult<ResourcePage> {
        self.with_kube_session(&session_id, |live| async move {
            live.resources
                .workload_pods(&reference)
                .await
                .map_err(resource_error)
        })
        .await
    }
    pub async fn kube_object_events(
        &self,
        session_id: String,
        reference: ResourceRef,
        uid: String,
    ) -> AppResult<ResourcePage> {
        self.with_kube_session(&session_id, |live| async move {
            live.resources
                .object_events(&reference, &uid)
                .await
                .map_err(resource_error)
        })
        .await
    }
    pub async fn kube_schema(
        &self,
        session_id: String,
        kind: ResourceKind,
    ) -> AppResult<ResourceSchema> {
        self.with_kube_session(&session_id, |live| async move {
            live.resources.schema(&kind).await.map_err(resource_error)
        })
        .await
    }
    pub async fn kube_manifest_diff(
        &self,
        session_id: String,
        reference: ResourceRef,
        manifest: Zeroizing<Vec<u8>>,
    ) -> AppResult<ManifestDiff> {
        self.with_kube_session(&session_id, |live| async move {
            if manifest.len() > 2 * 1024 * 1024 {
                return Err(AppError::invalid("kubernetes", "invalid_manifest"));
            }
            let text = std::str::from_utf8(&manifest)
                .map_err(|_| AppError::invalid("kubernetes", "invalid_manifest"))?;
            live.resources
                .compare_manifest(&reference, Zeroizing::new(text.to_owned()))
                .await
                .map_err(resource_error)
        })
        .await
    }
    pub async fn kube_create_resource(
        &self,
        session_id: String,
        reference: ResourceRef,
        manifest: Zeroizing<Vec<u8>>,
        dry_run: bool,
    ) -> AppResult<ResourceDetail> {
        self.with_kube_session(&session_id, |live| async move {
            if manifest.len() > 2 * 1024 * 1024 {
                return Err(AppError::invalid("kubernetes", "invalid_manifest"));
            }
            let text = std::str::from_utf8(&manifest)
                .map_err(|_| AppError::invalid("kubernetes", "invalid_manifest"))?;
            live.resources
                .create(&reference, Zeroizing::new(text.to_owned()), dry_run)
                .await
                .map_err(resource_error)
        })
        .await
    }
}
