//! Bounded diagnostics and controller views from the native session.
use crate::{AppCore, AppError, AppResult};
use cc_k8s_core::{
    diagnostics::{diagnose, Issue},
    gitops::{flow_graph, overview, ArgoAction, FluxAction, GitOpsFlow, GitOpsOverview},
    helm::{HelmRelease, HelmSummary},
    resources::{
        AccessSummary, ListQuery, MetricUsage, ResourceKind, ResourceRef, ResourceRow,
        ResourceTable,
    },
};
use futures::{stream, StreamExt};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Serialize)]
pub struct KubeInsightResource {
    pub api: ResourceKind,
    pub resource: ResourceRow,
}

#[derive(Debug, Serialize)]
pub struct KubeInsights {
    pub issues: Vec<Issue>,
    pub gitops: GitOpsOverview,
    pub flow: GitOpsFlow,
    pub resources: Vec<KubeInsightResource>,
    pub partial: bool,
}

impl AppCore {
    pub async fn kube_helm_compare_live(
        &self,
        session_id: String,
        namespace: String,
        secret_name: String,
    ) -> AppResult<cc_k8s_core::resources::HelmLiveComparison> {
        self.with_kube_session(&session_id, |live| async move {
            live.resources
                .helm_compare_live(&namespace, &secret_name)
                .await
                .map_err(super::kubernetes_runtime::resource_error)
        })
        .await
    }

    pub async fn kube_insights(
        &self,
        session_id: String,
        namespace: Option<String>,
    ) -> AppResult<KubeInsights> {
        self.with_kube_session(&session_id, |live| async move {
            let discovery = live
                .resources
                .discover()
                .await
                .map_err(super::kubernetes_runtime::resource_error)?;
            let mut partial = !discovery.unavailable_groups.is_empty();
            let mut selected = BTreeSet::new();
            let mut kinds = discovery
                .resources
                .into_iter()
                .filter(|kind| {
                    let relevant = matches!(
                        kind.group.as_str(),
                        "argoproj.io"
                            | "source.toolkit.fluxcd.io"
                            | "kustomize.toolkit.fluxcd.io"
                            | "helm.toolkit.fluxcd.io"
                            | "notification.toolkit.fluxcd.io"
                            | "image.toolkit.fluxcd.io"
                    ) || (kind.group.is_empty()
                        && matches!(
                            kind.resource.as_str(),
                            "pods" | "nodes" | "persistentvolumeclaims" | "events"
                        ))
                        || (kind.group == "apps"
                            && matches!(
                                kind.resource.as_str(),
                                "deployments" | "statefulsets" | "daemonsets"
                            ))
                        || (kind.group == "batch" && kind.resource == "jobs");
                    relevant && kind.verbs.iter().any(|v| v == "list")
                })
                .collect::<Vec<_>>();
            // Prefer a stable served version; list each group/resource once.
            kinds.sort_by_key(|kind| {
                (
                    kind.version.contains("alpha"),
                    kind.version.contains("beta"),
                    kind.key(),
                )
            });
            kinds.retain(|kind| selected.insert((kind.group.clone(), kind.resource.clone())));
            if kinds.len() > 64 {
                kinds.truncate(64);
                partial = true;
            }
            let api = live.resources.clone();
            let mut batches = stream::iter(kinds)
                .map(|kind| {
                    let api = api.clone();
                    let namespace = namespace.clone();
                    async move {
                        let mut query = ListQuery {
                            namespace: if kind.namespaced { namespace } else { None },
                            limit: Some(200),
                            ..Default::default()
                        };
                        let mut rows = Vec::new();
                        let mut tokens = BTreeSet::new();
                        let mut bytes = 0;
                        loop {
                            let page = match api.list(&kind, &query).await {
                                Ok(page) => page,
                                Err(_) => return (kind, rows, true),
                            };
                            for row in page.items {
                                bytes += row.object.to_string().len();
                                if rows.len() >= 10_000 || bytes > 16 * 1024 * 1024 {
                                    return (kind, rows, true);
                                }
                                rows.push(row);
                            }
                            query.continue_token = page.continue_token;
                            match &query.continue_token {
                                Some(token) if tokens.insert(token.clone()) => {}
                                Some(_) => return (kind, rows, true),
                                None => return (kind, rows, false),
                            }
                        }
                    }
                })
                .buffer_unordered(4);
            let mut resources = Vec::new();
            let mut bytes = 0;
            while let Some((kind, rows, incomplete)) = batches.next().await {
                partial |= incomplete;
                for row in rows {
                    bytes += row.object.to_string().len();
                    if resources.len() >= 20_000 || bytes > 32 * 1024 * 1024 {
                        partial = true;
                        break;
                    }
                    resources.push(KubeInsightResource {
                        api: kind.clone(),
                        resource: row,
                    });
                }
                if partial && (resources.len() >= 20_000 || bytes > 32 * 1024 * 1024) {
                    break;
                }
            }
            if live.cancel.is_cancelled() {
                return Err(AppError::VaultLocked);
            }
            let objects: Vec<Value> = resources
                .iter()
                .map(|item| item.resource.object.clone())
                .collect();
            let mut issues = diagnose(&objects);
            match tokio::time::timeout(
                std::time::Duration::from_secs(10),
                live.resources.certificate_warnings(namespace.clone()),
            )
            .await
            {
                Ok(Ok(warnings)) => {
                    issues.extend(warnings.into_iter().map(|warning| Issue {
                        severity: if warning.code == "certificate_expired" {
                            cc_k8s_core::diagnostics::Severity::Error
                        } else {
                            cc_k8s_core::diagnostics::Severity::Warning
                        },
                        code: warning.code,
                        kind: "Secret".into(),
                        namespace: warning.namespace,
                        name: warning.name,
                        subject: warning.days_remaining.map(|days| days.to_string()),
                        actions: vec!["details".into()],
                        exit_code: None,
                        count: 1,
                        owner: None,
                    }));
                }
                _ => partial = true,
            }
            let gitops = overview(&objects);
            let flow = flow_graph(&objects);
            Ok(KubeInsights {
                issues,
                gitops,
                flow,
                resources,
                partial,
            })
        })
        .await
    }

    pub async fn kube_helm_history(
        &self,
        session_id: String,
        namespace: Option<String>,
        release_name: Option<String>,
    ) -> AppResult<Vec<HelmSummary>> {
        self.with_kube_session(&session_id, |live| async move {
            let namespaces = match namespace {
                Some(namespace) => vec![namespace],
                None => live
                    .resources
                    .namespaces()
                    .await
                    .map_err(super::kubernetes_runtime::resource_error)?,
            };
            if namespaces.len() > 256 {
                return Err(AppError::invalid("kubernetes", "select_namespace"));
            }
            let mut result = Vec::new();
            let mut batches = stream::iter(namespaces)
                .map(|namespace| {
                    let api = live.resources.clone();
                    let release_name = release_name.clone();
                    async move { api.helm_history(&namespace, release_name.as_deref()).await }
                })
                .buffer_unordered(4);
            while let Some(batch) = batches.next().await {
                result.extend(batch.map_err(super::kubernetes_runtime::resource_error)?);
                if result.len() > 4096 {
                    return Err(AppError::invalid("kubernetes", "select_namespace"));
                }
            }
            Ok(result)
        })
        .await
    }
    pub async fn kube_helm_release(
        &self,
        session_id: String,
        namespace: String,
        secret_name: String,
    ) -> AppResult<HelmRelease> {
        self.with_kube_session(&session_id, |live| async move {
            live.resources
                .helm_release(&namespace, &secret_name)
                .await
                .map_err(super::kubernetes_runtime::resource_error)
        })
        .await
    }
    pub async fn kube_flux_action(
        &self,
        session_id: String,
        reference: ResourceRef,
        action: String,
        reason: Option<String>,
    ) -> AppResult<()> {
        self.with_kube_session(&session_id, |live| async move {
            let action = match action.as_str() {
                "reconcile" => FluxAction::Reconcile,
                "reconcile_with_source" => FluxAction::ReconcileWithSource,
                "suspend" => FluxAction::Suspend,
                "resume" => FluxAction::Resume,
                "reset_failures" => FluxAction::ResetFailures,
                "force_upgrade" => FluxAction::ForceUpgrade,
                _ => return Err(AppError::invalid("kubernetes", "invalid_action")),
            };
            live.resources
                .flux_action(&reference, action, reason.as_deref())
                .await
                .map_err(super::kubernetes_runtime::resource_error)
        })
        .await
    }
    pub async fn kube_argo_action(
        &self,
        session_id: String,
        reference: ResourceRef,
        action: String,
    ) -> AppResult<()> {
        self.with_kube_session(&session_id, |live| async move {
            let action = match action.as_str() {
                "refresh" => ArgoAction::Refresh,
                "hard_refresh" => ArgoAction::HardRefresh,
                "sync" => ArgoAction::Sync,
                _ => return Err(AppError::invalid("kubernetes", "invalid_action")),
            };
            live.resources
                .argo_action(&reference, action)
                .await
                .map_err(super::kubernetes_runtime::resource_error)
        })
        .await
    }
    pub async fn kube_resources_table(
        &self,
        session_id: String,
        kind: ResourceKind,
        query: ListQuery,
    ) -> AppResult<ResourceTable> {
        self.with_kube_session(&session_id, |live| async move {
            live.resources
                .table(&kind, &query)
                .await
                .map_err(super::kubernetes_runtime::resource_error)
        })
        .await
    }
    pub async fn kube_permissions(
        &self,
        session_id: String,
        namespace: String,
    ) -> AppResult<AccessSummary> {
        self.with_kube_session(&session_id, |live| async move {
            live.resources
                .check_permissions(&namespace)
                .await
                .map_err(super::kubernetes_runtime::resource_error)
        })
        .await
    }
    pub async fn kube_metrics(
        &self,
        session_id: String,
        pods: bool,
        namespace: Option<String>,
    ) -> AppResult<Vec<MetricUsage>> {
        self.with_kube_session(&session_id, |live| async move {
            live.resources
                .metric_usage(pods, namespace.as_deref())
                .await
                .map_err(super::kubernetes_runtime::resource_error)
        })
        .await
    }
}
