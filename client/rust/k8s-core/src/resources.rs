//! Bounded, discovery-backed Kubernetes requests. Callers choose resource
//! identities, never paths or URLs; every request retains the session guard.

macro_rules! redacted_debug {
    ($($name:ty),+ $(,)?) => {$(
        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(concat!(stringify!($name), "([redacted])"))
            }
        }
    )+};
}

mod certificates;
mod diff;
mod forward_targets;
mod helm_compare;
mod history;
mod metrics;
mod operations;
pub(crate) mod protected_json;
mod relations;
mod schema;
mod terminal;
mod watch;

use crate::{policy::ProdGuard, session::KubernetesSession};
use futures::{stream, StreamExt};
use http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use kube::{client::Body, Client};
use protected_json::{wipe_json, ProtectedJson};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};
use zeroize::Zeroizing;

pub use certificates::{certificate_warning, CertificateWarning};
pub use diff::{manifest_diff, ManifestChange, ManifestDiff};
pub use forward_targets::{ForwardPort, ForwardTarget, ForwardTargets};
pub use helm_compare::{HelmLiveComparison, HelmObjectComparison};
pub use history::WorkloadRevision;
pub use metrics::{parse_metrics, ContainerUsage, MetricUsage, ResourceAmounts};
pub use operations::{DebugContainer, DrainCandidate};
pub use relations::{RelatedObject, ResourceRelation, ResourceRelations};
pub use schema::ResourceSchema;
pub use terminal::{
    ForwardListener, ForwardUpdate, PodTerminalInput, PodTerminalOutput, PodTerminalRequest,
};
pub use watch::LogBuffer;
pub use watch::{ResourceChange, WatchOptions};

const RESPONSE_LIMIT: usize = 8 * 1024 * 1024;
const MAX_MANIFEST: usize = 2 * 1024 * 1024;
pub const MAX_LOG_LINES: usize = 20_000;
pub const MAX_LOG_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceError {
    #[error("The Kubernetes resource or request is invalid")]
    InvalidRequest,
    #[error("The Kubernetes resource is not available in discovery")]
    UnknownResource,
    #[error("The Kubernetes API denied access")]
    Forbidden,
    #[error("The Kubernetes authentication expired or was rejected")]
    Unauthorized,
    #[error("The Kubernetes object was not found")]
    NotFound,
    #[error("The Kubernetes resource changed; reload before applying")]
    Conflict,
    #[error("The Kubernetes watch must be restarted from a fresh list")]
    Gone,
    #[error("The Kubernetes response exceeds the size limit")]
    TooLarge,
    #[error("The Kubernetes API returned an invalid response")]
    InvalidResponse,
    #[error("The Kubernetes session is closed")]
    Closed,
    #[error("The Kubernetes request timed out")]
    Timeout,
    #[error("The Kubernetes request was blocked or failed")]
    Transport,
    #[error("The Kubernetes API is temporarily unavailable")]
    Unavailable,
    #[error("The Kubernetes API rejected the object as invalid")]
    Rejected,
}

/// Server-side apply was refused because other field managers own fields
/// the manifest changes. Only manager names and field paths are kept,
/// never values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldConflict {
    pub managers: Vec<String>,
    pub fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApplyError {
    #[error(transparent)]
    Resource(#[from] ResourceError),
    #[error("Fields are owned by other field managers")]
    FieldConflict(FieldConflict),
    /// The object is invalid (400/422): `path: CauseType` pairs only, e.g.
    /// `spec: FieldValueForbidden` for an immutable Pod field.
    #[error("The Kubernetes API rejected fields of the object")]
    Rejected(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResourceKind {
    pub group: String,
    pub version: String,
    pub resource: String,
    pub kind: String,
    pub namespaced: bool,
    pub verbs: Vec<String>,
}

impl ResourceKind {
    pub fn key(&self) -> String {
        format!("{}/{}/{}", self.group, self.version, self.resource)
    }

    fn base_path(&self) -> String {
        if self.group.is_empty() {
            format!("/api/{}", self.version)
        } else {
            format!("/apis/{}/{}", self.group, self.version)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceRef {
    pub api: ResourceKind,
    pub namespace: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    pub namespace: Option<String>,
    pub label_selector: Option<String>,
    pub field_selector: Option<String>,
    pub continue_token: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
pub struct ReadOptions {
    /// Only an explicit reveal action may enable this. AI and watch callers
    /// always use the default, and raw values must never enter a disk cache.
    pub reveal_secret_values: bool,
    pub include_config_map_values: bool,
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            reveal_secret_values: false,
            include_config_map_values: true,
        }
    }
}

impl ReadOptions {
    pub const AI: Self = Self {
        reveal_secret_values: false,
        include_config_map_values: false,
    };
}

#[derive(Debug, Clone, Serialize)]
pub struct Discovery {
    pub server_version: String,
    pub resources: Vec<ResourceKind>,
    /// Group/version names only; error response bodies are never exposed.
    pub unavailable_groups: Vec<String>,
}

#[derive(Clone, Serialize)]
pub struct ResourceRow {
    pub name: String,
    pub namespace: Option<String>,
    pub uid: String,
    pub resource_version: String,
    pub created_at: Option<String>,
    pub status: String,
    pub kind: String,
    pub object: Value,
}

#[derive(Clone, Serialize)]
pub struct ResourcePage {
    pub items: Vec<ResourceRow>,
    pub resource_version: String,
    pub continue_token: Option<String>,
    pub remaining: Option<u64>,
}

#[derive(Clone, Serialize)]
pub struct ResourceDetail {
    pub object: Value,
    pub yaml: String,
    pub containers: Vec<String>,
    pub summary: ResourceRow,
}

#[derive(Debug, Clone, Serialize)]
pub struct TableColumn {
    pub name: String,
    #[serde(rename = "type")]
    pub column_type: String,
    pub format: String,
    pub description: String,
    pub priority: u32,
}
#[derive(Clone, Serialize)]
pub struct TableRow {
    pub cells: Vec<Value>,
    pub resource: ResourceRow,
}
#[derive(Clone, Serialize)]
pub struct ResourceTable {
    pub columns: Vec<TableColumn>,
    pub rows: Vec<TableRow>,
    pub resource_version: String,
    pub continue_token: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct AccessSummary {
    pub incomplete: bool,
    pub can_mutate: bool,
    pub resource_rules: Vec<AccessRule>,
}
#[derive(Debug, Clone, Serialize)]
pub struct AccessRule {
    pub verbs: Vec<String>,
    pub api_groups: Vec<String>,
    pub resources: Vec<String>,
    pub resource_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogQuery {
    pub container: Option<String>,
    #[serde(default)]
    pub previous: bool,
    #[serde(default = "yes")]
    pub timestamps: bool,
    #[serde(default = "default_tail")]
    pub tail_lines: u32,
    pub since_seconds: Option<u32>,
}

fn yes() -> bool {
    true
}
fn default_tail() -> u32 {
    500
}
impl Default for LogQuery {
    fn default() -> Self {
        Self {
            container: None,
            previous: false,
            timestamps: true,
            tail_lines: default_tail(),
            since_seconds: None,
        }
    }
}

#[derive(Clone, Serialize)]
pub struct LogSnapshot {
    pub text: String,
    pub truncated: bool,
}

#[derive(Clone)]
pub struct ResourceApi {
    client: Client,
    guard: ProdGuard,
    discovered: Arc<RwLock<BTreeMap<String, ResourceKind>>>,
    preferred_versions: Arc<RwLock<BTreeMap<String, String>>>,
    discovery_incomplete: Arc<AtomicBool>,
}

redacted_debug!(
    ResourceRow,
    ResourcePage,
    ResourceDetail,
    LogSnapshot,
    ResourceApi,
    TableRow,
    ResourceTable
);

impl ResourceApi {
    pub fn is_closed(&self) -> bool {
        self.guard.cancellation().is_cancelled()
    }
    pub fn new(session: &KubernetesSession) -> Self {
        Self {
            client: session.client(),
            guard: session.guard().clone(),
            discovered: Arc::new(RwLock::new(BTreeMap::new())),
            preferred_versions: Arc::new(RwLock::new(BTreeMap::new())),
            discovery_incomplete: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn server_version(&self) -> Result<String, ResourceError> {
        let value = self.get_json("/version").await?;
        let version = value["gitVersion"]
            .as_str()
            .filter(|value| safe_query_value(value, 128))
            .ok_or(ResourceError::InvalidResponse)?;
        Ok(version.to_owned())
    }

    pub async fn discover(&self) -> Result<Discovery, ResourceError> {
        let version = self.get_json("/version").await?;
        let core = self.get_json("/api/v1").await?;
        let groups = self.get_json("/apis").await?;
        let mut resources = parse_discovery("", "v1", &core)?;
        let groups = groups["groups"]
            .as_array()
            .ok_or(ResourceError::InvalidResponse)?;
        if groups.len() > 512 {
            return Err(ResourceError::TooLarge);
        }
        let mut requests = BTreeSet::new();
        let mut preferred = BTreeMap::new();
        for group in groups {
            let Some(name) = group["name"].as_str() else {
                continue;
            };
            if !dns_name(name, 253) {
                return Err(ResourceError::InvalidResponse);
            }
            // All served versions are retained: CRDs, including GitOps
            // controllers, can expose different capabilities per version.
            let versions = group["versions"]
                .as_array()
                .ok_or(ResourceError::InvalidResponse)?;
            if let Some(version) = group["preferredVersion"]["version"].as_str() {
                if versions.iter().any(|served| served["version"] == version) {
                    preferred.insert(name.to_owned(), version.to_owned());
                }
            }
            for version in versions {
                let version = version["version"]
                    .as_str()
                    .ok_or(ResourceError::InvalidResponse)?;
                if !dns_name(version, 63) {
                    return Err(ResourceError::InvalidResponse);
                }
                requests.insert((name.to_owned(), version.to_owned()));
            }
        }
        if requests.len() > 1024 {
            return Err(ResourceError::TooLarge);
        }
        let results = stream::iter(requests.into_iter().map(|(group, version)| async move {
            let result = self
                .get_json(&format!("/apis/{group}/{version}"))
                .await
                .and_then(|value| parse_discovery(&group, &version, &value));
            (group, version, result)
        }))
        .buffer_unordered(8)
        .collect::<Vec<_>>()
        .await;
        let mut unavailable_groups = Vec::new();
        for (group, version, result) in results {
            match result {
                Ok(found) => resources.extend(found),
                Err(ResourceError::Closed) => return Err(ResourceError::Closed),
                Err(_) => unavailable_groups.push(format!("{group}/{version}")),
            }
        }
        if resources.len() > 16_384 {
            return Err(ResourceError::TooLarge);
        }
        resources.sort_by_key(ResourceKind::key);
        resources.dedup_by_key(|kind| kind.key());
        let map = resources
            .iter()
            .map(|kind| (kind.key(), kind.clone()))
            .collect();
        *self.discovered.write().map_err(|_| ResourceError::Closed)? = map;
        *self
            .preferred_versions
            .write()
            .map_err(|_| ResourceError::Closed)? = preferred;
        unavailable_groups.sort();
        self.discovery_incomplete
            .store(!unavailable_groups.is_empty(), Ordering::Release);
        Ok(Discovery {
            server_version: version["gitVersion"]
                .as_str()
                .filter(|v| v.len() <= 128)
                .unwrap_or("unknown")
                .to_owned(),
            resources,
            unavailable_groups,
        })
    }

    pub fn kinds(&self) -> Result<Vec<ResourceKind>, ResourceError> {
        Ok(self
            .discovered
            .read()
            .map_err(|_| ResourceError::Closed)?
            .values()
            .cloned()
            .collect())
    }

    pub fn preferred_version(&self, group: &str) -> Option<String> {
        self.preferred_versions.read().ok()?.get(group).cloned()
    }

    pub fn discovery_is_partial(&self) -> bool {
        self.discovery_incomplete.load(Ordering::Acquire)
    }

    pub fn resolve(
        &self,
        group: &str,
        version: &str,
        resource: &str,
    ) -> Result<ResourceKind, ResourceError> {
        self.discovered
            .read()
            .map_err(|_| ResourceError::Closed)?
            .get(&format!("{group}/{version}/{resource}"))
            .cloned()
            .ok_or(ResourceError::UnknownResource)
    }

    pub async fn namespaces(&self) -> Result<Vec<String>, ResourceError> {
        let kind = self.resolve("", "v1", "namespaces")?;
        let mut query = ListQuery::default();
        let mut result = Vec::new();
        let mut continuations = std::collections::BTreeSet::new();
        loop {
            let page = self.list(&kind, &query).await?;
            result.extend(page.items.into_iter().map(|item| item.name));
            if result.len() > 10_000 {
                return Err(ResourceError::TooLarge);
            }
            query.continue_token = page.continue_token;
            if query.continue_token.is_none() {
                break;
            }
            if !continuations.insert(query.continue_token.clone().unwrap()) {
                return Err(ResourceError::InvalidResponse);
            }
        }
        result.sort();
        Ok(result)
    }

    pub async fn list(
        &self,
        kind: &ResourceKind,
        query: &ListQuery,
    ) -> Result<ResourcePage, ResourceError> {
        self.list_bounded(kind, query, RESPONSE_LIMIT).await
    }

    async fn list_bounded(
        &self,
        kind: &ResourceKind,
        query: &ListQuery,
        response_limit: usize,
    ) -> Result<ResourcePage, ResourceError> {
        let path = self.list_path(kind, query, "list")?;
        let bytes = self
            .bytes_request(Method::GET, &path, Vec::new(), None, response_limit)
            .await?;
        let value = serde_json::from_slice(&bytes).map_err(|_| ResourceError::InvalidResponse)?;
        page_from_value(value, kind, query.limit.unwrap_or(200) as usize)
    }

    /// Kubernetes evaluates CRD additionalPrinterColumns itself. The native
    /// client neither executes JSONPath code nor trusts an arbitrary URL.
    pub async fn table(
        &self,
        kind: &ResourceKind,
        query: &ListQuery,
    ) -> Result<ResourceTable, ResourceError> {
        // Table cells are server-generated and could contain masked values
        // independently of the object projection. Derive only safe columns.
        if (kind.group.is_empty() && kind.kind == "Secret")
            || matches!(
                (kind.group.as_str(), kind.kind.as_str()),
                ("helm.toolkit.fluxcd.io", "HelmRelease")
                    | ("kustomize.toolkit.fluxcd.io", "Kustomization")
                    | ("notification.toolkit.fluxcd.io", "Provider" | "Receiver")
            )
        {
            let page = self.list(kind, query).await?;
            return Ok(basic_table(page));
        }
        let path = format!(
            "{}&includeObject=Object",
            self.list_path(kind, query, "list")?
        );
        let request = Request::builder()
            .method(Method::GET)
            .uri(&path)
            .header(
                "Accept",
                "application/json;as=Table;v=v1;g=meta.k8s.io,application/json",
            )
            .body(Body::empty())
            .map_err(|_| ResourceError::InvalidRequest)?;
        let cancel = self.guard.cancellation();
        let bytes = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(ResourceError::Closed),
            result = tokio::time::timeout(Duration::from_secs(30),async {
                let response = self.client.send(request).await.map_err(|_|ResourceError::Transport)?;
                if !response.status().is_success() { return Err(status_error(response.status())); }
                let mut body = response.into_body();
                let mut bytes = Zeroizing::new(Vec::new());
                while let Some(frame) = body.frame().await {
                    if let Ok(data) = frame.map_err(|_|ResourceError::Transport)?.into_data() {
                        if bytes.len().saturating_add(data.len()) > RESPONSE_LIMIT { return Err(ResourceError::TooLarge); }
                        bytes.extend_from_slice(&data);
                    }
                }
                Ok(bytes)
            }) => result.map_err(|_|ResourceError::Timeout)??,
        };
        let mut value = ProtectedJson(
            serde_json::from_slice(&bytes).map_err(|_| ResourceError::InvalidResponse)?,
        );
        if value["kind"] != "Table" || value["apiVersion"] != "meta.k8s.io/v1" {
            let page = page_from_value(
                value.into_inner(),
                kind,
                query.limit.unwrap_or(200) as usize,
            )?;
            return Ok(basic_table(page));
        }
        let columns = value["columnDefinitions"]
            .as_array()
            .ok_or(ResourceError::InvalidResponse)?;
        if columns.len() > 64 {
            return Err(ResourceError::TooLarge);
        }
        let columns = columns
            .iter()
            .map(|column| {
                Ok(TableColumn {
                    name: response_string(&column["name"], 128)?
                        .ok_or(ResourceError::InvalidResponse)?,
                    column_type: response_string(&column["type"], 64)?
                        .unwrap_or_else(|| "string".into()),
                    format: response_string(&column["format"], 128)?.unwrap_or_default(),
                    description: column["description"]
                        .as_str()
                        .filter(|value| value.len() <= 4096 && !value.contains('\0'))
                        .unwrap_or_default()
                        .to_owned(),
                    priority: column["priority"]
                        .as_u64()
                        .unwrap_or_default()
                        .min(u32::MAX as u64) as u32,
                })
            })
            .collect::<Result<Vec<_>, ResourceError>>()?;
        let mut rows = Vec::new();
        let entries = value["rows"]
            .as_array_mut()
            .ok_or(ResourceError::InvalidResponse)?;
        if entries.len() > query.limit.unwrap_or(200) as usize {
            return Err(ResourceError::TooLarge);
        }
        for row in entries {
            let cells = row["cells"]
                .as_array()
                .ok_or(ResourceError::InvalidResponse)?;
            if cells.len() != columns.len() {
                return Err(ResourceError::InvalidResponse);
            }
            let mut object = ProtectedJson(row["object"].take());
            redact_object(&mut object, kind, ReadOptions::default());
            rows.push(TableRow {
                cells: row["cells"].as_array().cloned().unwrap_or_default(),
                resource: row_from_value(object.into_inner(), kind)?,
            });
        }
        Ok(ResourceTable {
            columns,
            rows,
            resource_version: response_string(&value["metadata"]["resourceVersion"], 128)?
                .unwrap_or_default(),
            continue_token: response_string(&value["metadata"]["continue"], 16_384)?
                .filter(|v| !v.is_empty()),
        })
    }

    pub async fn check_permissions(&self, namespace: &str) -> Result<AccessSummary, ResourceError> {
        if !dns_name(namespace, 63) {
            return Err(ResourceError::InvalidRequest);
        }
        let body = json!({"apiVersion":"authorization.k8s.io/v1","kind":"SelfSubjectRulesReview","spec":{"namespace":namespace}});
        let value = self
            .json_request(
                Method::POST,
                "/apis/authorization.k8s.io/v1/selfsubjectrulesreviews",
                serde_json::to_vec(&body).map_err(|_| ResourceError::InvalidRequest)?,
                Some("application/json"),
            )
            .await?;
        let entries = value["status"]["resourceRules"]
            .as_array()
            .ok_or(ResourceError::InvalidResponse)?;
        if entries.len() > 4096 {
            return Err(ResourceError::TooLarge);
        }
        let strings = |value: &Value| -> Result<Vec<String>, ResourceError> {
            let Some(values) = value.as_array() else {
                return Ok(Vec::new());
            };
            if values.len() > 4096 {
                return Err(ResourceError::TooLarge);
            }
            values
                .iter()
                .map(|value| response_string(value, 253)?.ok_or(ResourceError::InvalidResponse))
                .collect()
        };
        let mut rules = Vec::new();
        let mut can_mutate = false;
        for rule in entries {
            let verbs = strings(&rule["verbs"])?;
            can_mutate |= verbs.iter().any(|verb| {
                matches!(
                    verb.as_str(),
                    "*" | "create"
                        | "update"
                        | "patch"
                        | "delete"
                        | "deletecollection"
                        | "bind"
                        | "escalate"
                        | "impersonate"
                )
            });
            rules.push(AccessRule {
                verbs,
                api_groups: strings(&rule["apiGroups"])?,
                resources: strings(&rule["resources"])?,
                resource_names: strings(&rule["resourceNames"])?,
            });
        }
        Ok(AccessSummary {
            incomplete: value["status"]["incomplete"].as_bool().unwrap_or(true),
            can_mutate,
            resource_rules: rules,
        })
    }

    pub async fn get(
        &self,
        reference: &ResourceRef,
        options: ReadOptions,
    ) -> Result<ResourceDetail, ResourceError> {
        let path = self.object_path(reference, "get")?;
        let mut object = ProtectedJson(self.get_json(&path).await?);
        redact_object(&mut object, &reference.api, options);
        let summary = row_from_value((*object).clone(), &reference.api)?;
        let containers = ["containers", "initContainers", "ephemeralContainers"]
            .into_iter()
            .filter_map(|key| object["spec"][key].as_array())
            .flatten()
            .filter_map(|entry| entry["name"].as_str().map(str::to_owned))
            .collect();
        let yaml = serde_saphyr::to_string(&*object).map_err(|_| ResourceError::InvalidResponse)?;
        Ok(ResourceDetail {
            object: object.into_inner(),
            yaml,
            containers,
            summary,
        })
    }

    /// Server-side apply uses a fixed manager and an identity-bound manifest.
    /// Dry run is still a PATCH and therefore subject to the native ProdGuard.
    pub async fn apply(
        &self,
        reference: &ResourceRef,
        manifest: Zeroizing<String>,
        dry_run: bool,
        force: bool,
    ) -> Result<ResourceDetail, ApplyError> {
        if manifest.len() > MAX_MANIFEST {
            return Err(ResourceError::TooLarge.into());
        }
        let mut object = ProtectedJson(
            serde_saphyr::from_str(&manifest).map_err(|_| ResourceError::InvalidRequest)?,
        );
        strip_server_metadata(&mut object);
        validate_manifest(reference, &object)?;
        let path = self.object_path(reference, "patch")?;
        let parameters = {
            let mut query = url::form_urlencoded::Serializer::new(String::new());
            query.append_pair("fieldManager", "consoleCrypt");
            if dry_run {
                query.append_pair("dryRun", "All");
            }
            if force {
                query.append_pair("force", "true");
            }
            query.finish()
        };
        let bytes = serde_json::to_vec(&*object).map_err(|_| ResourceError::InvalidRequest)?;
        let mut object = ProtectedJson(
            self.apply_request(&format!("{path}?{parameters}"), bytes)
                .await?,
        );
        redact_object(&mut object, &reference.api, ReadOptions::default());
        let summary = row_from_value((*object).clone(), &reference.api)?;
        let yaml = serde_saphyr::to_string(&*object).map_err(|_| ResourceError::InvalidResponse)?;
        Ok(ResourceDetail {
            object: object.into_inner(),
            yaml,
            containers: Vec::new(),
            summary,
        })
    }

    /// A UID/resourceVersion precondition prevents a stale dialog from
    /// deleting a recreated or changed object with the same name.
    pub async fn delete(
        &self,
        reference: &ResourceRef,
        uid: &str,
        resource_version: &str,
    ) -> Result<(), ResourceError> {
        if uid.is_empty()
            || !safe_query_value(uid, 128)
            || !safe_query_value(resource_version, 128)
            || resource_version.is_empty()
        {
            return Err(ResourceError::InvalidRequest);
        }
        let path = self.object_path(reference, "delete")?;
        let body = json!({
            "apiVersion":"v1","kind":"DeleteOptions",
            "preconditions":{"uid":uid,"resourceVersion":resource_version},
            "propagationPolicy":"Background"
        });
        self.json_request(
            Method::DELETE,
            &path,
            serde_json::to_vec(&body).map_err(|_| ResourceError::InvalidRequest)?,
            Some("application/json"),
        )
        .await?;
        Ok(())
    }

    pub async fn logs(
        &self,
        namespace: &str,
        pod: &str,
        query: &LogQuery,
    ) -> Result<LogSnapshot, ResourceError> {
        let path = self.log_path(namespace, pod, query, false)?;
        let bytes = self
            .bytes_request(Method::GET, &path, Vec::new(), None, MAX_LOG_BYTES)
            .await?;
        let text = String::from_utf8(bytes.to_vec()).map_err(|_| ResourceError::InvalidResponse)?;
        let truncated = text.lines().count() > MAX_LOG_LINES;
        let text = if truncated {
            text.lines()
                .rev()
                .take(MAX_LOG_LINES)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            text
        };
        Ok(LogSnapshot { text, truncated })
    }

    pub async fn metrics(
        &self,
        pods: bool,
        namespace: Option<&str>,
    ) -> Result<Value, ResourceError> {
        let resource = if pods { "pods" } else { "nodes" };
        let kind = self.resolve("metrics.k8s.io", "v1beta1", resource)?;
        let query = ListQuery {
            namespace: namespace.map(str::to_owned),
            ..ListQuery::default()
        };
        let path = self.list_path(&kind, &query, "list")?;
        self.get_json(&path).await
    }

    pub async fn helm_release(
        &self,
        namespace: &str,
        secret_name: &str,
    ) -> Result<crate::helm::HelmRelease, ResourceError> {
        if !secret_name.starts_with("sh.helm.release.v1.") {
            return Err(ResourceError::InvalidRequest);
        }
        let reference = ResourceRef {
            api: self.resolve("", "v1", "secrets")?,
            namespace: Some(namespace.into()),
            name: secret_name.into(),
        };
        let path = self.object_path(&reference, "get")?;
        let secret = ProtectedJson(self.get_json(&path).await?);
        crate::helm::decode_release_secret(&secret)
    }

    pub async fn helm_history(
        &self,
        namespace: &str,
        release_name: Option<&str>,
    ) -> Result<Vec<crate::helm::HelmSummary>, ResourceError> {
        if release_name.is_some_and(|name| !dns_name(name, 53)) {
            return Err(ResourceError::InvalidRequest);
        }
        let kind = self.resolve("", "v1", "secrets")?;
        let mut query = ListQuery {
            namespace: Some(namespace.into()),
            label_selector: Some(
                release_name
                    .map(|name| format!("owner=helm,name={name}"))
                    .unwrap_or_else(|| "owner=helm".into()),
            ),
            limit: Some(5),
            ..ListQuery::default()
        };
        let mut history = Vec::new();
        let mut seen = BTreeSet::new();
        loop {
            let path = self.list_path(&kind, &query, "list")?;
            let mut page = ProtectedJson(self.get_json(&path).await?);
            let entries = page["items"]
                .as_array_mut()
                .ok_or(ResourceError::InvalidResponse)?;
            if entries.len() > 5 {
                return Err(ResourceError::TooLarge);
            }
            for secret in entries {
                complete_type_metadata(secret, &kind)?;
                let release = crate::helm::decode_release_secret(secret)?;
                history.push(crate::helm::HelmSummary::from(&release));
                if history.len() > 2_000 {
                    return Err(ResourceError::TooLarge);
                }
            }
            query.continue_token =
                response_string(&page["metadata"]["continue"], 16_384)?.filter(|v| !v.is_empty());
            if let Some(token) = &query.continue_token {
                if !seen.insert(token.clone()) {
                    return Err(ResourceError::InvalidResponse);
                }
            } else {
                break;
            }
        }
        history.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then_with(|| b.revision.cmp(&a.revision))
        });
        Ok(history)
    }

    pub(crate) async fn merge_patch(
        &self,
        reference: &ResourceRef,
        patch: Value,
    ) -> Result<Value, ResourceError> {
        let path = self.object_path(reference, "patch")?;
        let bytes = serde_json::to_vec(&patch).map_err(|_| ResourceError::InvalidRequest)?;
        if bytes.len() > MAX_MANIFEST {
            return Err(ResourceError::TooLarge);
        }
        self.json_request(
            Method::PATCH,
            &path,
            bytes,
            Some("application/merge-patch+json"),
        )
        .await
    }

    fn checked_kind(
        &self,
        supplied: &ResourceKind,
        verb: &str,
    ) -> Result<ResourceKind, ResourceError> {
        let kind = self.resolve(&supplied.group, &supplied.version, &supplied.resource)?;
        if &kind != supplied || !kind.verbs.iter().any(|v| v == verb) {
            return Err(ResourceError::InvalidRequest);
        }
        Ok(kind)
    }

    fn collection_path(
        &self,
        kind: &ResourceKind,
        namespace: Option<&str>,
        verb: &str,
    ) -> Result<String, ResourceError> {
        let kind = self.checked_kind(kind, verb)?;
        let mut path = kind.base_path();
        match namespace {
            Some(namespace) if kind.namespaced && dns_name(namespace, 63) => {
                path.push_str(&format!("/namespaces/{namespace}"));
            }
            Some(_) => return Err(ResourceError::InvalidRequest),
            None => {}
        }
        path.push('/');
        path.push_str(&kind.resource);
        Ok(path)
    }

    fn object_path(&self, reference: &ResourceRef, verb: &str) -> Result<String, ResourceError> {
        if !object_name(&reference.name)
            || reference.api.namespaced != reference.namespace.is_some()
        {
            return Err(ResourceError::InvalidRequest);
        }
        let path = self.collection_path(&reference.api, reference.namespace.as_deref(), verb)?;
        Ok(format!("{path}/{}", reference.name))
    }

    fn list_path(
        &self,
        kind: &ResourceKind,
        query: &ListQuery,
        verb: &str,
    ) -> Result<String, ResourceError> {
        let path = self.collection_path(kind, query.namespace.as_deref(), verb)?;
        let limit = query.limit.unwrap_or(200);
        if limit == 0 || limit > 500 {
            return Err(ResourceError::InvalidRequest);
        }
        let mut pairs = url::form_urlencoded::Serializer::new(String::new());
        pairs.append_pair("limit", &limit.to_string());
        for (key, value, max) in [
            ("labelSelector", query.label_selector.as_deref(), 4096),
            ("fieldSelector", query.field_selector.as_deref(), 4096),
            ("continue", query.continue_token.as_deref(), 16_384),
        ] {
            if let Some(value) = value {
                if !safe_query_value(value, max) {
                    return Err(ResourceError::InvalidRequest);
                }
                pairs.append_pair(key, value);
            }
        }
        Ok(format!("{path}?{}", pairs.finish()))
    }

    fn log_path(
        &self,
        namespace: &str,
        pod: &str,
        query: &LogQuery,
        follow: bool,
    ) -> Result<String, ResourceError> {
        if !dns_name(namespace, 63)
            || !dns_name(pod, 253)
            || query.tail_lines == 0
            || query.tail_lines > MAX_LOG_LINES as u32
            || query.container.as_deref().is_some_and(|c| !dns_name(c, 63))
        {
            return Err(ResourceError::InvalidRequest);
        }
        self.resolve("", "v1", "pods")?;
        let mut pairs = url::form_urlencoded::Serializer::new(String::new());
        pairs
            .append_pair("tailLines", &query.tail_lines.to_string())
            .append_pair(
                "timestamps",
                if query.timestamps { "true" } else { "false" },
            )
            .append_pair("previous", if query.previous { "true" } else { "false" })
            .append_pair("follow", if follow { "true" } else { "false" });
        if !follow {
            pairs.append_pair("limitBytes", &MAX_LOG_BYTES.to_string());
        }
        if let Some(container) = &query.container {
            pairs.append_pair("container", container);
        }
        if let Some(since) = query.since_seconds {
            pairs.append_pair("sinceSeconds", &since.to_string());
        }
        Ok(format!(
            "/api/v1/namespaces/{namespace}/pods/{pod}/log?{}",
            pairs.finish()
        ))
    }

    async fn get_json(&self, path: &str) -> Result<Value, ResourceError> {
        self.json_request(Method::GET, path, Vec::new(), None).await
    }

    async fn json_request(
        &self,
        method: Method,
        path: &str,
        body: Vec<u8>,
        content_type: Option<&str>,
    ) -> Result<Value, ResourceError> {
        let bytes = self
            .bytes_request(method, path, body, content_type, RESPONSE_LIMIT)
            .await?;
        // Do not use kube::Client::request<T>: it logs the response on JSON
        // decoding errors, which could disclose credentials from a Secret.
        serde_json::from_slice(&bytes).map_err(|_| ResourceError::InvalidResponse)
    }

    async fn bytes_request(
        &self,
        method: Method,
        path: &str,
        body: Vec<u8>,
        content_type: Option<&str>,
        limit: usize,
    ) -> Result<Zeroizing<Vec<u8>>, ResourceError> {
        let cancel = self.guard.cancellation();
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(ResourceError::Closed),
            result = tokio::time::timeout(Duration::from_secs(30), async {
                let mut body = self.open_body(method, path, body, content_type).await?;
                let mut bytes = Zeroizing::new(Vec::new());
                while let Some(frame) = body.frame().await {
                    let frame = frame.map_err(|_| ResourceError::Transport)?;
                    if let Ok(data) = frame.into_data() {
                        if bytes.len().saturating_add(data.len()) > limit {
                            return Err(ResourceError::TooLarge);
                        }
                        bytes.extend_from_slice(&data);
                    }
                }
                Ok(bytes)
            }) => result.map_err(|_| ResourceError::Timeout)?,
        }
    }

    /// Server-side apply. Like every request, error bodies are never shown;
    /// a 409 is the one exception that is read (bounded), and only the field
    /// manager names and field paths of its conflict causes are kept.
    async fn apply_request(&self, path: &str, body: Vec<u8>) -> Result<Value, ApplyError> {
        let cancel = self.guard.cancellation();
        let bytes = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(ResourceError::Closed.into()),
            result = tokio::time::timeout(Duration::from_secs(30), async {
                let response = self
                    .send(Method::PATCH, path, body, Some("application/apply-patch+yaml"))
                    .await?;
                let status = response.status();
                let explained = matches!(
                    status,
                    StatusCode::CONFLICT | StatusCode::UNPROCESSABLE_ENTITY | StatusCode::BAD_REQUEST
                );
                if !status.is_success() && !explained {
                    return Err(ApplyError::from(status_error(status)));
                }
                let limit = if status.is_success() { RESPONSE_LIMIT } else { 64 * 1024 };
                let mut body = response.into_body();
                let mut bytes = Zeroizing::new(Vec::new());
                while let Some(frame) = body.frame().await {
                    let frame = frame.map_err(|_| ResourceError::Transport)?;
                    if let Ok(data) = frame.into_data() {
                        if bytes.len().saturating_add(data.len()) > limit {
                            return Err(if status.is_success() {
                                ResourceError::TooLarge.into()
                            } else {
                                status_error(status).into()
                            });
                        }
                        bytes.extend_from_slice(&data);
                    }
                }
                if status == StatusCode::CONFLICT {
                    return Err(match field_conflict(&bytes) {
                        Some(conflict) => ApplyError::FieldConflict(conflict),
                        None => ResourceError::Conflict.into(),
                    });
                }
                if !status.is_success() {
                    return Err(match rejected_fields(&bytes) {
                        Some(fields) => ApplyError::Rejected(fields),
                        None => ResourceError::Rejected.into(),
                    });
                }
                Ok(bytes)
            }) => result.map_err(|_| ResourceError::Timeout)??,
        };
        serde_json::from_slice(&bytes).map_err(|_| ResourceError::InvalidResponse.into())
    }

    async fn open_body(
        &self,
        method: Method,
        path: &str,
        bytes: Vec<u8>,
        content_type: Option<&str>,
    ) -> Result<Body, ResourceError> {
        let response = self.send(method, path, bytes, content_type).await?;
        if !response.status().is_success() {
            // Never collect, parse or display a server-controlled error body.
            return Err(status_error(response.status()));
        }
        Ok(response.into_body())
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        bytes: Vec<u8>,
        content_type: Option<&str>,
    ) -> Result<http::Response<Body>, ResourceError> {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("Accept", "application/json");
        if let Some(content_type) = content_type {
            request = request.header("Content-Type", content_type);
        }
        let request = request
            .body(Body::from(bytes))
            .map_err(|_| ResourceError::InvalidRequest)?;
        self.client.send(request).await.map_err(|_| {
            if self.guard.cancellation().is_cancelled() {
                ResourceError::Closed
            } else {
                ResourceError::Transport
            }
        })
    }
}

fn basic_table(page: ResourcePage) -> ResourceTable {
    ResourceTable {
        columns: vec![
            TableColumn {
                name: "Name".into(),
                column_type: "string".into(),
                format: "name".into(),
                description: String::new(),
                priority: 0,
            },
            TableColumn {
                name: "Status".into(),
                column_type: "string".into(),
                format: String::new(),
                description: String::new(),
                priority: 0,
            },
        ],
        rows: page
            .items
            .into_iter()
            .map(|resource| TableRow {
                cells: vec![
                    Value::String(resource.name.clone()),
                    Value::String(resource.status.clone()),
                ],
                resource,
            })
            .collect(),
        resource_version: page.resource_version,
        continue_token: page.continue_token,
    }
}

/// Field managers and paths from a server-side apply conflict (`Status`
/// with `FieldManagerConflict` causes). Messages look like
/// `conflict with "kube-controller-manager" using v1`; anything that does
/// not fit the expected shape is dropped.
fn field_conflict(body: &[u8]) -> Option<FieldConflict> {
    let status: Value = serde_json::from_slice(body).ok()?;
    let mut managers = BTreeSet::new();
    let mut fields = BTreeSet::new();
    for cause in status["details"]["causes"].as_array()?.iter().take(50) {
        if cause["reason"] != "FieldManagerConflict" {
            continue;
        }
        let manager = cause["message"]
            .as_str()
            .and_then(|message| message.split('"').nth(1))
            .filter(|name| {
                !name.is_empty()
                    && name.len() <= 128
                    && name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "._:/@+-".contains(c))
            });
        if let Some(manager) = manager {
            managers.insert(manager.to_owned());
        }
        if let Some(field) = cause["field"]
            .as_str()
            .filter(|field| safe_query_value(field, 256))
        {
            fields.insert(field.to_owned());
        }
    }
    if managers.is_empty() {
        return None;
    }
    Some(FieldConflict {
        managers: managers.into_iter().take(10).collect(),
        fields: fields.into_iter().take(20).collect(),
    })
}

/// `path: CauseType` from an invalid-object `Status` (cause messages may
/// quote submitted values, so they are dropped).
fn rejected_fields(body: &[u8]) -> Option<Vec<String>> {
    let status: Value = serde_json::from_slice(body).ok()?;
    let fields: BTreeSet<String> = status["details"]["causes"]
        .as_array()?
        .iter()
        .take(50)
        .filter_map(|cause| {
            let reason = cause["reason"].as_str().filter(|r| {
                !r.is_empty() && r.len() <= 64 && r.chars().all(|c| c.is_ascii_alphanumeric())
            })?;
            let field = cause["field"]
                .as_str()
                .filter(|f| safe_query_value(f, 256))?;
            Some(format!("{field}: {reason}"))
        })
        .take(20)
        .collect();
    (!fields.is_empty()).then(|| fields.into_iter().collect())
}

fn status_error(status: StatusCode) -> ResourceError {
    match status {
        StatusCode::UNAUTHORIZED => ResourceError::Unauthorized,
        StatusCode::FORBIDDEN => ResourceError::Forbidden,
        StatusCode::NOT_FOUND => ResourceError::NotFound,
        StatusCode::CONFLICT => ResourceError::Conflict,
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => ResourceError::Rejected,
        StatusCode::GONE => ResourceError::Gone,
        StatusCode::TOO_MANY_REQUESTS
        | StatusCode::SERVICE_UNAVAILABLE
        | StatusCode::BAD_GATEWAY
        | StatusCode::GATEWAY_TIMEOUT => ResourceError::Unavailable,
        _ => ResourceError::InvalidResponse,
    }
}

fn parse_discovery(
    group: &str,
    version: &str,
    value: &Value,
) -> Result<Vec<ResourceKind>, ResourceError> {
    let entries = value["resources"]
        .as_array()
        .ok_or(ResourceError::InvalidResponse)?;
    if entries.len() > 4096 {
        return Err(ResourceError::TooLarge);
    }
    let mut output = Vec::new();
    for entry in entries {
        let resource = entry["name"]
            .as_str()
            .ok_or(ResourceError::InvalidResponse)?;
        if resource.contains('/') {
            continue;
        }
        let kind = entry["kind"]
            .as_str()
            .ok_or(ResourceError::InvalidResponse)?;
        if !dns_name(resource, 253)
            || kind.len() > 128
            || !kind.bytes().all(|c| c.is_ascii_alphanumeric())
        {
            return Err(ResourceError::InvalidResponse);
        }
        let mut verbs: Vec<_> = entry["verbs"]
            .as_array()
            .ok_or(ResourceError::InvalidResponse)?
            .iter()
            .filter_map(Value::as_str)
            .filter(|v| {
                matches!(
                    *v,
                    "get"
                        | "list"
                        | "watch"
                        | "create"
                        | "update"
                        | "patch"
                        | "delete"
                        | "deletecollection"
                )
            })
            .map(str::to_owned)
            .collect();
        verbs.sort();
        verbs.dedup();
        if !verbs.iter().any(|v| v == "get" || v == "list") {
            continue;
        }
        output.push(ResourceKind {
            group: group.into(),
            version: version.into(),
            resource: resource.into(),
            kind: kind.into(),
            namespaced: entry["namespaced"]
                .as_bool()
                .ok_or(ResourceError::InvalidResponse)?,
            verbs,
        });
    }
    Ok(output)
}

fn page_from_value(
    value: Value,
    kind: &ResourceKind,
    limit: usize,
) -> Result<ResourcePage, ResourceError> {
    let mut value = ProtectedJson(value);
    let entries = value["items"]
        .as_array_mut()
        .ok_or(ResourceError::InvalidResponse)?;
    if entries.len() > limit {
        return Err(ResourceError::TooLarge);
    }
    let mut items = Vec::with_capacity(entries.len());
    for item in entries {
        redact_object(item, kind, ReadOptions::default());
        items.push(row_from_value(item.take(), kind)?);
    }
    let resource_version =
        response_string(&value["metadata"]["resourceVersion"], 128)?.unwrap_or_default();
    let continue_token =
        response_string(&value["metadata"]["continue"], 16_384)?.filter(|s| !s.is_empty());
    Ok(ResourcePage {
        items,
        resource_version,
        continue_token,
        remaining: value["metadata"]["remainingItemCount"].as_u64(),
    })
}

fn complete_type_metadata(object: &mut Value, kind: &ResourceKind) -> Result<(), ResourceError> {
    let object = object
        .as_object_mut()
        .ok_or(ResourceError::InvalidResponse)?;
    // Kubernetes list items normally omit TypeMeta. Recover it exclusively
    // from the discovery-validated endpoint, never from caller-supplied YAML.
    object
        .entry("kind")
        .or_insert_with(|| Value::String(kind.kind.clone()));
    object.entry("apiVersion").or_insert_with(|| {
        Value::String(if kind.group.is_empty() {
            kind.version.clone()
        } else {
            format!("{}/{}", kind.group, kind.version)
        })
    });
    Ok(())
}

fn row_from_value(object: Value, kind: &ResourceKind) -> Result<ResourceRow, ResourceError> {
    let mut object = ProtectedJson(object);
    complete_type_metadata(&mut object, kind)?;
    let name =
        response_string(&object["metadata"]["name"], 253)?.ok_or(ResourceError::InvalidResponse)?;
    if !object_name(&name) {
        return Err(ResourceError::InvalidResponse);
    }
    let namespace = response_string(&object["metadata"]["namespace"], 63)?;
    let uid = response_string(&object["metadata"]["uid"], 128)?.unwrap_or_default();
    let resource_version =
        response_string(&object["metadata"]["resourceVersion"], 128)?.unwrap_or_default();
    let created_at = response_string(&object["metadata"]["creationTimestamp"], 64)?;
    let status = object_status(&object);
    Ok(ResourceRow {
        name,
        namespace,
        uid,
        resource_version,
        created_at,
        status,
        kind: kind.kind.clone(),
        object: object.into_inner(),
    })
}

pub fn object_status(object: &Value) -> String {
    if object["metadata"]["deletionTimestamp"].is_string() {
        return "Terminating".into();
    }
    if object["spec"]["suspend"] == true {
        return "Suspended".into();
    }
    for status in ["initContainerStatuses", "containerStatuses"] {
        if let Some(entries) = object["status"][status].as_array() {
            for entry in entries {
                if let Some(reason) = entry["state"]["waiting"]["reason"].as_str() {
                    return reason.chars().take(128).collect();
                }
                if entry["state"]["terminated"]["exitCode"]
                    .as_i64()
                    .is_some_and(|v| v != 0)
                {
                    return entry["state"]["terminated"]["reason"]
                        .as_str()
                        .unwrap_or("Error")
                        .chars()
                        .take(128)
                        .collect();
                }
            }
        }
    }
    if let Some(conditions) = object["status"]["conditions"].as_array() {
        for desired in [
            "Stalled",
            "Reconciling",
            "Ready",
            "Available",
            "Complete",
            "Failed",
        ] {
            if conditions
                .iter()
                .any(|c| c["type"] == desired && c["status"] == "True")
            {
                return desired.into();
            }
        }
        if conditions
            .iter()
            .any(|c| c["type"] == "Ready" && c["status"] == "False")
        {
            return "NotReady".into();
        }
    }
    object["status"]["phase"]
        .as_str()
        .or_else(|| object["status"]["health"]["status"].as_str())
        .unwrap_or("Unknown")
        .chars()
        .take(128)
        .collect()
}

pub fn redact_object(object: &mut Value, kind: &ResourceKind, options: ReadOptions) {
    if let Some(metadata) = object.get_mut("metadata").and_then(Value::as_object_mut) {
        if let Some(mut value) = metadata.remove("managedFields") {
            wipe_json(&mut value);
        }
        if let Some(annotations) = metadata
            .get_mut("annotations")
            .and_then(Value::as_object_mut)
        {
            if let Some(mut value) =
                annotations.remove("kubectl.kubernetes.io/last-applied-configuration")
            {
                wipe_json(&mut value);
            }
        }
    }
    let core_secret = kind.group.is_empty() && kind.resource == "secrets";
    let core_config = kind.group.is_empty() && kind.resource == "configmaps";
    if (core_secret && !options.reveal_secret_values)
        || (core_config && !options.include_config_map_values)
    {
        for key in ["data", "stringData", "binaryData"] {
            if let Some(values) = object.get_mut(key).and_then(Value::as_object_mut) {
                for value in values.values_mut() {
                    wipe_json(value);
                    *value = Value::String("[redacted]".into());
                }
            }
        }
        // Arbitrary annotations may duplicate the entire original object.
        if core_secret {
            if let Some(metadata) = object.get_mut("metadata").and_then(Value::as_object_mut) {
                if let Some(mut value) = metadata.remove("annotations") {
                    wipe_json(&mut value);
                }
            }
        }
    }
    // GitOps CRDs can contain credentials directly, outside core Secret data.
    // Preserve references to Secret objects, but never expose inline values or
    // notification endpoints that commonly contain webhook tokens.
    match (kind.group.as_str(), kind.kind.as_str()) {
        ("helm.toolkit.fluxcd.io", "HelmRelease") => {
            if let Some(values) = object.pointer_mut("/spec/values") {
                crate::helm::mask_values(values);
            }
        }
        ("kustomize.toolkit.fluxcd.io", "Kustomization") => {
            if let Some(values) = object.pointer_mut("/spec/postBuild/substitute") {
                crate::helm::mask_values(values);
            }
        }
        ("notification.toolkit.fluxcd.io", "Provider") => {
            for path in ["/spec/address", "/spec/proxy", "/spec/headers"] {
                if let Some(value) = object.pointer_mut(path) {
                    wipe_json(value);
                    *value = Value::String("[redacted]".into());
                }
            }
        }
        ("notification.toolkit.fluxcd.io", "Receiver") => {
            if let Some(value) = object.pointer_mut("/spec/token") {
                wipe_json(value);
                *value = Value::String("[redacted]".into());
            }
        }
        _ => {}
    }
}

fn contains_redacted_value(object: &Value) -> bool {
    match object {
        Value::String(value) => value.contains("[redacted]"),
        Value::Array(values) => values.iter().any(contains_redacted_value),
        Value::Object(values) => values.values().any(contains_redacted_value),
        _ => false,
    }
}

/// Objects made by controllers (ReplicaSet Pods, Job Pods) carry
/// `generateName` and other fields the server owns. An edit of an existing,
/// named object never needs them, so they are dropped before applying
/// instead of rejecting the whole manifest.
fn strip_server_metadata(object: &mut Value) {
    if let Some(metadata) = object.get_mut("metadata").and_then(Value::as_object_mut) {
        if metadata.get("name").is_some_and(Value::is_string) {
            metadata.remove("generateName");
        }
        metadata.remove("managedFields");
    }
}

fn validate_manifest(reference: &ResourceRef, object: &Value) -> Result<(), ResourceError> {
    validate_manifest_identity(reference, object)?;
    if !object["metadata"]["resourceVersion"]
        .as_str()
        .is_some_and(|value| !value.is_empty() && safe_query_value(value, 128))
    {
        return Err(ResourceError::InvalidRequest);
    }
    Ok(())
}

fn validate_manifest_identity(
    reference: &ResourceRef,
    object: &Value,
) -> Result<(), ResourceError> {
    let version = if reference.api.group.is_empty() {
        reference.api.version.clone()
    } else {
        format!("{}/{}", reference.api.group, reference.api.version)
    };
    if !object_name(&reference.name)
        || reference.api.namespaced != reference.namespace.is_some()
        || object["apiVersion"] != version
        || object["kind"] != reference.api.kind
        || object["metadata"]["name"] != reference.name
        || object["metadata"]["namespace"].as_str() != reference.namespace.as_deref()
        || object["metadata"]["generateName"].is_string()
        || contains_redacted_value(object)
    {
        return Err(ResourceError::InvalidRequest);
    }
    Ok(())
}

fn response_string(value: &Value, max: usize) -> Result<Option<String>, ResourceError> {
    if value.is_null() {
        return Ok(None);
    }
    let value = value.as_str().ok_or(ResourceError::InvalidResponse)?;
    if !safe_query_value(value, max) {
        return Err(ResourceError::InvalidResponse);
    }
    Ok(Some(value.to_owned()))
}
fn safe_query_value(value: &str, max: usize) -> bool {
    value.len() <= max && !value.chars().any(char::is_control)
}
fn dns_name(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.split('.').all(|part| {
            !part.is_empty()
                && !part.starts_with('-')
                && !part.ends_with('-')
                && part
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        })
}

fn object_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && !matches!(value, "." | "..")
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_' | b':'))
}

#[cfg(test)]
mod redaction_tests {
    use super::*;
    #[test]
    fn gitops_inline_values_are_masked_and_masked_manifests_cannot_be_applied() {
        for (group, resource, kind, body) in [
            (
                "helm.toolkit.fluxcd.io",
                "helmreleases",
                "HelmRelease",
                json!({"values":{"password":"fixture-password","replicas":2},
                    "valuesFrom":[{"kind":"Secret","name":"reference-kept"}]}),
            ),
            (
                "kustomize.toolkit.fluxcd.io",
                "kustomizations",
                "Kustomization",
                json!({"postBuild":{"substitute":{"TOKEN":"fixture-password"},
                    "substituteFrom":[{"kind":"Secret","name":"reference-kept"}]}}),
            ),
            (
                "notification.toolkit.fluxcd.io",
                "providers",
                "Provider",
                json!({"address":"https://example.test/fixture-password",
                    "secretRef":{"name":"reference-kept"}}),
            ),
        ] {
            let descriptor = ResourceKind {
                group: group.into(),
                version: "v1".into(),
                resource: resource.into(),
                kind: kind.into(),
                namespaced: true,
                verbs: vec!["get".into(), "patch".into()],
            };
            let reference = ResourceRef {
                api: descriptor.clone(),
                namespace: Some("apps".into()),
                name: "sample".into(),
            };
            let mut object = json!({"apiVersion":format!("{group}/v1"),"kind":kind,
                "metadata":{"name":"sample","namespace":"apps","resourceVersion":"1"},"spec":body});
            assert!(validate_manifest(&reference, &object).is_ok());
            redact_object(&mut object, &descriptor, ReadOptions::default());
            let encoded = serde_json::to_string(&object).unwrap();
            assert!(!encoded.contains("fixture-password"));
            assert!(encoded.contains("reference-kept"));
            assert_eq!(
                validate_manifest(&reference, &object),
                Err(ResourceError::InvalidRequest)
            );
        }
    }
    #[test]
    fn apply_conflicts_keep_only_managers_and_paths() {
        let body = serde_json::to_vec(&json!({"kind":"Status","reason":"Conflict",
            "message":"Apply failed with 2 conflicts: secret-value",
            "details":{"causes":[
                {"reason":"FieldManagerConflict","message":"conflict with \"kube-controller-manager\" using v1","field":".metadata.labels.app"},
                {"reason":"FieldManagerConflict","message":"conflict with \"bad name; rm\" using v1","field":".spec.x"},
                {"reason":"FieldValueInvalid","message":"conflict with \"other\"","field":".spec.y"}]}}))
        .unwrap();
        let conflict = field_conflict(&body).unwrap();
        assert_eq!(conflict.managers, ["kube-controller-manager"]);
        assert_eq!(conflict.fields, [".metadata.labels.app", ".spec.x"]);
        let invalid = serde_json::to_vec(&json!({"kind":"Status","reason":"Invalid",
            "details":{"causes":[{"reason":"FieldValueForbidden","message":"value \"secret\"","field":"spec"}]}}))
        .unwrap();
        assert_eq!(
            rejected_fields(&invalid).unwrap(),
            ["spec: FieldValueForbidden"]
        );
        let stale = serde_json::to_vec(&json!({"kind":"Status","reason":"Conflict",
            "message":"the object has been modified"}))
        .unwrap();
        assert!(field_conflict(&stale).is_none());
    }
    #[test]
    fn controller_made_pods_can_be_edited() {
        let descriptor = ResourceKind {
            group: "".into(),
            version: "v1".into(),
            resource: "pods".into(),
            kind: "Pod".into(),
            namespaced: true,
            verbs: vec!["patch".into()],
        };
        let reference = ResourceRef {
            api: descriptor,
            namespace: Some("apps".into()),
            name: "web-7fd48655c6-xs4v2".into(),
        };
        let mut object = json!({"apiVersion":"v1","kind":"Pod",
            "metadata":{"name":"web-7fd48655c6-xs4v2","generateName":"web-7fd48655c6-",
                "namespace":"apps","resourceVersion":"7","managedFields":[{"manager":"kubelet"}]}});
        assert_eq!(
            validate_manifest(&reference, &object),
            Err(ResourceError::InvalidRequest)
        );
        strip_server_metadata(&mut object);
        assert!(validate_manifest(&reference, &object).is_ok());
        assert!(object["metadata"].get("managedFields").is_none());
    }
    #[test]
    fn masked_config_maps_and_embedded_yaml_are_never_submitted_as_real_values() {
        let descriptor = ResourceKind {
            group: "".into(),
            version: "v1".into(),
            resource: "configmaps".into(),
            kind: "ConfigMap".into(),
            namespaced: true,
            verbs: vec!["create".into()],
        };
        let reference = ResourceRef {
            api: descriptor,
            namespace: Some("apps".into()),
            name: "sample".into(),
        };
        let object = json!({"apiVersion":"v1","kind":"ConfigMap",
            "metadata":{"name":"sample","namespace":"apps"},"data":{"settings.yaml":"password: [redacted]\n"}});
        assert_eq!(
            validate_manifest_identity(&reference, &object),
            Err(ResourceError::InvalidRequest)
        );
    }
}
