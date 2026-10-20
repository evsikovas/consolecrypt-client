//! Read-only, per-question AI tools. There is no mutation or arbitrary HTTP
//! entry point. Results are projected and sanitized before leaving the native
//! layer, including when the destination is a local model.

use crate::resources::protected_json::{wipe_json, ProtectedJson};
use crate::{
    diagnostics, gitops,
    resources::{
        ListQuery, LogQuery, ReadOptions, ResourceApi, ResourceError, ResourceKind, ResourceRef,
    },
};
use cc_ai_core::sanitizer::{
    Category, PrivacyProfile, Rehydrated, SanitizedText, SanitizerSession,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
    time::Duration,
};
use zeroize::Zeroizing;

const MAX_CALLS: u32 = 12;
const PER_CALL_BYTES: usize = 64 * 1024;
const QUESTION_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ToolError {
    #[error("This Kubernetes AI tool is unknown or has invalid arguments")]
    InvalidTool,
    #[error("The Kubernetes AI question budget is exhausted")]
    BudgetExceeded,
    #[error("The Kubernetes AI session is closed")]
    Closed,
    #[error("The Kubernetes AI tool timed out")]
    Timeout,
    #[error("The Kubernetes AI tool could not read the requested data")]
    ReadFailed,
}

/// Constructed by the application after checking the selected provider and
/// device-local consent. It is deliberately not deserializable from a model
/// response. Consent is scoped to this one question by the owning app runtime.
#[derive(Debug, Clone, Copy)]
pub enum ToolDestination {
    Local,
    ApprovedExternal,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceKey {
    pub group: String,
    pub version: String,
    pub resource: String,
}

#[derive(Clone, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolRequest {
    ListResources {
        resource: ResourceKey,
        namespace: Option<String>,
        selector: Option<String>,
        #[serde(default = "default_rows")]
        max_rows: u32,
    },
    GetObject {
        resource: ResourceKey,
        namespace: Option<String>,
        name: String,
    },
    Describe {
        resource: ResourceKey,
        namespace: Option<String>,
        name: String,
    },
    Events {
        namespace: Option<String>,
        object_uid: Option<String>,
        since_seconds: Option<u32>,
    },
    LogsTail {
        namespace: String,
        pod: String,
        container: Option<String>,
        #[serde(default = "default_lines")]
        lines: u32,
        #[serde(default)]
        previous: bool,
        since_seconds: Option<u32>,
    },
    Top {
        #[serde(default)]
        pods: bool,
        namespace: Option<String>,
    },
    Problems {
        namespace: Option<String>,
    },
    RolloutHistory {
        resource: ResourceKey,
        namespace: String,
        name: String,
    },
    DiffRevisions {
        resource: ResourceKey,
        namespace: String,
        name: String,
        before: u64,
        after: u64,
    },
    HelmReleases {
        namespace: String,
    },
    HelmValues {
        namespace: String,
        secret_name: String,
    },
    FluxStatus {
        namespace: Option<String>,
    },
    FluxGraph {
        namespace: Option<String>,
    },
}
fn default_rows() -> u32 {
    100
}
fn default_lines() -> u32 {
    100
}
impl std::fmt::Debug for ToolRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ToolRequest([redacted])")
    }
}

pub struct ToolResult {
    pub content: SanitizedText,
    pub truncated: bool,
    pub calls_remaining: u32,
    pub bytes_remaining: usize,
}
impl std::fmt::Debug for ToolResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolResult")
            .field("bytes", &self.content.len())
            .field("truncated", &self.truncated)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct State {
    calls: u32,
    bytes: usize,
    closed: bool,
    sanitizer: SanitizerSession,
    event_history: ProtectedJson,
}

#[derive(Debug)]
pub struct AiToolSession {
    api: ResourceApi,
    state: Mutex<State>,
    config_map_values: bool,
}

impl AiToolSession {
    pub fn new(
        api: ResourceApi,
        destination: ToolDestination,
        profile: PrivacyProfile,
        cluster_name: &str,
        config_map_values: bool,
    ) -> Self {
        // External destinations never get the more permissive Local profile.
        let profile = match (destination, profile) {
            (ToolDestination::ApprovedExternal, PrivacyProfile::Local) => PrivacyProfile::Standard,
            (_, profile) => profile,
        };
        let mut sanitizer = SanitizerSession::new(profile);
        sanitizer.add_literal(Category::Host, cluster_name);
        Self {
            api,
            state: Mutex::new(State {
                calls: 0,
                bytes: 0,
                closed: false,
                sanitizer,
                event_history: ProtectedJson(json!([])),
            }),
            config_map_values,
        }
    }

    /// Caller supplies only this context's explicitly enabled local event
    /// history, in core/v1 Event shape. It remains question-local and is wiped
    /// with aliases when consent or the vault generation is revoked.
    pub fn with_event_history(mut self, history: Vec<Value>) -> Result<Self, ToolError> {
        let history = ProtectedJson(Value::Array(history));
        let rows = history.as_array().ok_or(ToolError::InvalidTool)?;
        if rows.len() > 200 || approximate_size(&history) > QUESTION_BYTES {
            return Err(ToolError::BudgetExceeded);
        }
        if rows.iter().any(|row| {
            row["kind"] != "Event"
                || row["apiVersion"] != "v1"
                || !row["metadata"]["uid"]
                    .as_str()
                    .is_some_and(|uid| !uid.is_empty() && uid.len() <= 128)
        }) {
            return Err(ToolError::InvalidTool);
        }
        let state = self.state.get_mut().map_err(|_| ToolError::Closed)?;
        if state.closed || self.api.is_closed() {
            return Err(ToolError::Closed);
        }
        state.event_history = history;
        Ok(self)
    }

    pub fn close(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.closed = true;
            // Drop reversible aliases immediately when permission is revoked.
            state.sanitizer = SanitizerSession::new(PrivacyProfile::Strict);
            wipe_json(&mut state.event_history);
        }
    }

    pub fn parse_request(json: &str) -> Result<ToolRequest, ToolError> {
        if json.len() > 16 * 1024 {
            return Err(ToolError::InvalidTool);
        }
        serde_json::from_str(json).map_err(|_| ToolError::InvalidTool)
    }

    pub async fn invoke(&self, request: ToolRequest) -> Result<ToolResult, ToolError> {
        self.charge_call()?;
        let mut value = ProtectedJson(
            tokio::time::timeout(Duration::from_secs(30), self.read(request))
                .await
                .map_err(|_| ToolError::Timeout)?
                .map_err(|error| match error {
                    ResourceError::Closed => ToolError::Closed,
                    ResourceError::InvalidRequest | ResourceError::UnknownResource => {
                        ToolError::InvalidTool
                    }
                    _ => ToolError::ReadFailed,
                })?,
        );
        project_for_ai(&mut value, self.config_map_values);
        let mut projection_budget = 512 * 1024;
        let projection_truncated = bound_projection(&mut value, &mut projection_budget);
        let mut state = self.state.lock().map_err(|_| ToolError::Closed)?;
        if state.closed || self.api.is_closed() {
            return Err(ToolError::Closed);
        }
        register_names(&mut state.sanitizer, &value);
        let serialized =
            Zeroizing::new(serde_json::to_string(&*value).map_err(|_| ToolError::ReadFailed)?);
        let sanitized = state.sanitizer.sanitize(&serialized);
        let available = PER_CALL_BYTES.min(QUESTION_BYTES.saturating_sub(state.bytes));
        if available < 128 {
            return Err(ToolError::BudgetExceeded);
        }
        let byte_truncated = sanitized.len() > available;
        let truncated = projection_truncated || byte_truncated;
        let content = if byte_truncated {
            let marker = "\n[truncated: Kubernetes AI result size limit]";
            let limit = available.saturating_sub(marker.len());
            let boundary = (0..=limit)
                .rev()
                .find(|index| sanitized.as_str().is_char_boundary(*index))
                .unwrap_or(0);
            let text = format!("{}{marker}", &sanitized.as_str()[..boundary]);
            // All text was already sanitized. Running it again preserves the
            // SanitizedText type contract without an unchecked constructor.
            state.sanitizer.sanitize(&text)
        } else {
            sanitized
        };
        if content.len() > available {
            return Err(ToolError::BudgetExceeded);
        }
        state.bytes = state.bytes.saturating_add(content.len());
        Ok(ToolResult {
            content,
            truncated,
            calls_remaining: MAX_CALLS.saturating_sub(state.calls),
            bytes_remaining: QUESTION_BYTES.saturating_sub(state.bytes),
        })
    }

    /// Provider integrations should use this entry point: malformed and
    /// forbidden tool requests also consume the per-question call budget.
    pub async fn invoke_json(&self, json: &str) -> Result<ToolResult, ToolError> {
        match self.restore_request(json) {
            Ok(request) => self.invoke(request).await,
            Err(error) => {
                self.charge_call()?;
                Err(error)
            }
        }
    }

    /// The question, instructions and model output share the aliases used by
    /// tool results. This does not consume a tool call or its result budget.
    pub fn sanitize_text(&self, text: &str) -> Result<SanitizedText, ToolError> {
        if text.len() > QUESTION_BYTES {
            return Err(ToolError::BudgetExceeded);
        }
        let mut state = self.state.lock().map_err(|_| ToolError::Closed)?;
        if state.closed || self.api.is_closed() {
            return Err(ToolError::Closed);
        }
        let text = state.sanitizer.sanitize(text);
        if text.len() > QUESTION_BYTES {
            return Err(ToolError::BudgetExceeded);
        }
        Ok(text)
    }

    fn restore_request(&self, json: &str) -> Result<ToolRequest, ToolError> {
        if json.len() > 16 * 1024 {
            return Err(ToolError::InvalidTool);
        }
        let mut value: Value = serde_json::from_str(json).map_err(|_| ToolError::InvalidTool)?;
        let state = self.state.lock().map_err(|_| ToolError::Closed)?;
        if state.closed || self.api.is_closed() {
            return Err(ToolError::Closed);
        }
        let mut budget = 16 * 1024;
        restore_argument_values(&mut value, &state.sanitizer, &mut budget)?;
        // Restore parsed string values rather than replacing raw JSON. A
        // quoted cluster/object name must never inject another tool argument.
        let restored =
            Zeroizing::new(serde_json::to_string(&value).map_err(|_| ToolError::InvalidTool)?);
        Self::parse_request(&restored)
    }

    fn charge_call(&self) -> Result<(), ToolError> {
        let mut state = self.state.lock().map_err(|_| ToolError::Closed)?;
        if state.closed || self.api.is_closed() {
            return Err(ToolError::Closed);
        }
        if state.calls >= MAX_CALLS || state.bytes >= QUESTION_BYTES {
            return Err(ToolError::BudgetExceeded);
        }
        state.calls += 1;
        Ok(())
    }

    /// For RAG providers, build the same projected read context without tool
    /// calling. It consumes the same question budget and returns the same
    /// sanitizer-enforced type.
    pub async fn selected_object_context(
        &self,
        resource: ResourceKey,
        namespace: Option<String>,
        name: String,
    ) -> Result<ToolResult, ToolError> {
        self.invoke(ToolRequest::Describe {
            resource,
            namespace,
            name,
        })
        .await
    }

    pub fn rehydrate(&self, text: &str) -> Result<Rehydrated, ToolError> {
        if text.len() > QUESTION_BYTES {
            return Err(ToolError::BudgetExceeded);
        }
        let state = self.state.lock().map_err(|_| ToolError::Closed)?;
        if state.closed || self.api.is_closed() {
            return Err(ToolError::Closed);
        }
        Ok(state.sanitizer.rehydrate(text))
    }

    async fn read(&self, request: ToolRequest) -> Result<Value, ResourceError> {
        match request {
            ToolRequest::ListResources {
                resource,
                namespace,
                selector,
                max_rows,
            } => {
                if max_rows == 0 || max_rows > 200 {
                    return Err(ResourceError::InvalidRequest);
                }
                let kind = self.kind(resource)?;
                let page = self
                    .api
                    .list(
                        &kind,
                        &ListQuery {
                            namespace,
                            label_selector: selector,
                            limit: Some(max_rows),
                            ..ListQuery::default()
                        },
                    )
                    .await?;
                Ok(json!({"items":page.items.into_iter().map(|row|json!({
                    "kind":row.kind,"name":row.name,"namespace":row.namespace,"status":row.status,
                    "created_at":row.created_at,"labels":row.object["metadata"]["labels"],
                    "restarts":row.object["status"]["containerStatuses"].as_array().map(|entries|entries.iter().filter_map(|c|c["restartCount"].as_u64()).sum::<u64>())
                })).collect::<Vec<_>>(),"has_more":page.continue_token.is_some()}))
            }
            ToolRequest::GetObject {
                resource,
                namespace,
                name,
            } => {
                let reference = ResourceRef {
                    api: self.kind(resource)?,
                    namespace,
                    name,
                };
                let detail = self
                    .api
                    .get(
                        &reference,
                        ReadOptions {
                            reveal_secret_values: false,
                            include_config_map_values: self.config_map_values,
                        },
                    )
                    .await?;
                Ok(detail.object)
            }
            ToolRequest::Describe {
                resource,
                namespace,
                name,
            } => {
                let reference = ResourceRef {
                    api: self.kind(resource)?,
                    namespace,
                    name,
                };
                self.describe(&reference).await
            }
            ToolRequest::Events {
                namespace,
                object_uid,
                since_seconds,
            } => {
                self.read_events(namespace, object_uid, since_seconds, 100)
                    .await
            }
            ToolRequest::LogsTail {
                namespace,
                pod,
                container,
                lines,
                previous,
                since_seconds,
            } => {
                if lines == 0 || lines > 500 {
                    return Err(ResourceError::InvalidRequest);
                }
                let logs = self
                    .api
                    .logs(
                        &namespace,
                        &pod,
                        &LogQuery {
                            container,
                            previous,
                            timestamps: true,
                            tail_lines: lines,
                            since_seconds,
                        },
                    )
                    .await?;
                let all_lines = logs.text.lines().collect::<Vec<_>>();
                let start = all_lines.len().saturating_sub(lines as usize);
                let text = all_lines[start..].join("\n");
                Ok(
                    json!({"namespace":namespace,"pod":pod,"logs":text,"truncated":logs.truncated||start>0}),
                )
            }
            ToolRequest::Top { pods, namespace } => {
                let rows = self.api.metric_usage(pods, namespace.as_deref()).await?;
                let truncated = rows.len() > 200;
                Ok(
                    json!({"items":rows.into_iter().take(200).collect::<Vec<_>>(),"truncated":truncated}),
                )
            }
            ToolRequest::Problems { namespace } => {
                let (objects, partial) = self.collect(namespace, false).await?;
                Ok(json!({"items":diagnostics::diagnose(&objects),"partial":partial}))
            }
            ToolRequest::RolloutHistory {
                resource,
                namespace,
                name,
            } => {
                let reference = ResourceRef {
                    api: self.kind(resource)?,
                    namespace: Some(namespace),
                    name,
                };
                let rows = self.api.rollout_history(&reference).await?;
                let truncated = rows.len() > 200;
                Ok(
                    json!({"items":rows.into_iter().rev().take(200).collect::<Vec<_>>(),"truncated":truncated}),
                )
            }
            ToolRequest::DiffRevisions {
                resource,
                namespace,
                name,
                before,
                after,
            } => {
                if before == 0 || after == 0 {
                    return Err(ResourceError::InvalidRequest);
                }
                let reference = ResourceRef {
                    api: self.kind(resource)?,
                    namespace: Some(namespace),
                    name,
                };
                let rows = self.api.rollout_history(&reference).await?;
                let mut before = rows
                    .iter()
                    .find(|row| row.revision == before)
                    .ok_or(ResourceError::NotFound)?
                    .template
                    .clone();
                let mut after = rows
                    .iter()
                    .find(|row| row.revision == after)
                    .ok_or(ResourceError::NotFound)?
                    .template
                    .clone();
                // Project before losing the field context in a diff leaf.
                project_for_ai(&mut before, false);
                project_for_ai(&mut after, false);
                serde_json::to_value(crate::resources::manifest_diff(before, after))
                    .map_err(|_| ResourceError::InvalidResponse)
            }
            ToolRequest::FluxStatus { namespace } => {
                let (objects, partial) = self.collect(namespace, true).await?;
                Ok(json!({"items":objects,"partial":partial}))
            }
            ToolRequest::FluxGraph { namespace } => {
                let (objects, partial) = self.collect(namespace, true).await?;
                Ok(
                    json!({"overview":gitops::overview(&objects),"flow":gitops::flow_graph(&objects),"partial":partial}),
                )
            }
            ToolRequest::HelmReleases { namespace } => {
                let releases = self.api.helm_history(&namespace, None).await?;
                let truncated = releases.len() > 200;
                Ok(
                    json!({"items":releases.into_iter().take(200).collect::<Vec<_>>(),"truncated":truncated}),
                )
            }
            ToolRequest::HelmValues {
                namespace,
                secret_name,
            } => {
                let release = self.api.helm_release(&namespace, &secret_name).await?;
                Ok(
                    json!({"name":release.name,"namespace":release.namespace,"revision":release.revision,"values":release.values,"chart_values":release.chart_values,"computed_values":release.computed_values,"computed_values_partial":release.computed_values_partial}),
                )
            }
        }
    }

    async fn read_events(
        &self,
        namespace: Option<String>,
        object_uid: Option<String>,
        since_seconds: Option<u32>,
        limit: u32,
    ) -> Result<Value, ResourceError> {
        if object_uid.as_ref().is_some_and(|uid| {
            uid.is_empty()
                || uid.len() > 128
                || !uid.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        }) || since_seconds.is_some_and(|n| n > 604_800)
        {
            return Err(ResourceError::InvalidRequest);
        }
        let history = {
            let state = self.state.lock().map_err(|_| ResourceError::Closed)?;
            if state.closed {
                return Err(ResourceError::Closed);
            }
            ProtectedJson((*state.event_history).clone())
        };
        let after = since_seconds
            .map(|seconds| chrono::Utc::now().timestamp_millis() - i64::from(seconds) * 1000);
        let matching = |event: &Value| {
            namespace
                .as_ref()
                .is_none_or(|ns| event["metadata"]["namespace"] == *ns)
                && object_uid
                    .as_ref()
                    .is_none_or(|uid| event["involvedObject"]["uid"] == *uid)
                && after
                    .is_none_or(|after| event_timestamp(event).is_some_and(|time| time >= after))
        };
        let mut items = BTreeMap::new();
        for event in history
            .as_array()
            .into_iter()
            .flatten()
            .filter(|event| matching(event))
        {
            if let Some(uid) = event["metadata"]["uid"].as_str() {
                items.insert(uid.to_owned(), ProtectedJson(event.clone()));
            }
        }
        let history_included = !items.is_empty();
        let live = async {
            let kind = self.api.resolve("", "v1", "events")?;
            self.api
                .list(
                    &kind,
                    &ListQuery {
                        namespace: namespace.clone(),
                        field_selector: object_uid
                            .as_ref()
                            .map(|uid| format!("involvedObject.uid={uid}")),
                        limit: Some(limit),
                        ..ListQuery::default()
                    },
                )
                .await
        }
        .await;
        let mut partial = false;
        match live {
            Ok(page) => {
                partial |= page.continue_token.is_some();
                for row in page.items {
                    if !matching(&row.object) {
                        continue;
                    }
                    if !row.uid.is_empty() {
                        items.insert(row.uid, ProtectedJson(row.object));
                    }
                }
            }
            Err(ResourceError::Closed) => return Err(ResourceError::Closed),
            Err(_) => partial = true,
        }
        let mut items = items.into_values().collect::<Vec<_>>();
        items.sort_by_key(|event| std::cmp::Reverse(event_timestamp(event).unwrap_or(0)));
        let has_more = items.len() > 200;
        items.truncate(200);
        Ok(
            json!({"items":items.into_iter().map(ProtectedJson::into_inner).collect::<Vec<_>>(),"has_more":has_more,"partial":partial,"history_included":history_included}),
        )
    }

    async fn describe(&self, reference: &ResourceRef) -> Result<Value, ResourceError> {
        let detail = self
            .api
            .get(
                reference,
                ReadOptions {
                    reveal_secret_values: false,
                    include_config_map_values: self.config_map_values,
                },
            )
            .await?;
        let events = self.read_events(
            reference.namespace.clone(),
            Some(detail.summary.uid.clone()),
            None,
            50,
        );
        let relations = self.api.related_objects_for_ai(reference, &detail);
        let logs = self.describe_logs(reference, &detail.object);
        let (events, related, logs) = tokio::join!(
            tokio::time::timeout(Duration::from_secs(6), events),
            tokio::time::timeout(Duration::from_secs(6), relations),
            tokio::time::timeout(Duration::from_secs(6), logs),
        );
        if self.api.is_closed() {
            return Err(ResourceError::Closed);
        }
        let mut partial = false;
        let events = match events {
            Ok(Ok(value)) => {
                partial |= value["has_more"] == true || value["partial"] == true;
                value
            }
            _ => {
                partial = true;
                json!({"unavailable":true})
            }
        };
        let related = match related {
            Ok(Ok(value)) => {
                partial |= value.partial;
                serde_json::to_value(value).map_err(|_| ResourceError::InvalidResponse)?
            }
            _ => {
                partial = true;
                json!({"unavailable":true})
            }
        };
        let logs = match logs {
            Ok(Ok(value)) => value,
            _ => {
                partial = true;
                json!({"unavailable":true})
            }
        };
        Ok(
            json!({"object":detail.object,"events":events,"related":related,"logs":logs,"partial":partial}),
        )
    }

    async fn describe_logs(
        &self,
        reference: &ResourceRef,
        object: &Value,
    ) -> Result<Value, ResourceError> {
        // Do not select an arbitrary Pod from a workload or read a controller's
        // logs on a Secret/ConfigMap question. The model may request a specific
        // related Pod explicitly with its own charged tool call.
        if !reference.api.group.is_empty() || reference.api.resource != "pods" {
            return Ok(json!({"not_applicable":true}));
        }
        let namespace = reference
            .namespace
            .as_deref()
            .ok_or(ResourceError::InvalidRequest)?;
        let statuses = object["status"]["containerStatuses"].as_array();
        let failing = statuses.and_then(|items| {
            items.iter().find(|entry| {
                entry["state"]["waiting"].is_object()
                    || entry["lastState"]["terminated"].is_object()
            })
        });
        let container = failing
            .and_then(|entry| entry["name"].as_str())
            .or_else(|| {
                object["spec"]["containers"]
                    .as_array()
                    .and_then(|items| items.first())
                    .and_then(|entry| entry["name"].as_str())
            })
            .map(str::to_owned);
        let previous = failing.is_some_and(|entry| entry["restartCount"].as_u64().unwrap_or(0) > 0);
        let snapshot = self
            .api
            .logs(
                namespace,
                &reference.name,
                &LogQuery {
                    container: container.clone(),
                    previous,
                    timestamps: true,
                    tail_lines: 100,
                    since_seconds: Some(3600),
                },
            )
            .await?;
        let text = Zeroizing::new(snapshot.text);
        let lines: Vec<_> = text.lines().collect();
        let start = lines.len().saturating_sub(100);
        Ok(
            json!({"pod":reference.name,"namespace":namespace,"container":container,
            "previous":previous,"text":lines[start..].join("\n"),"truncated":snapshot.truncated||start>0}),
        )
    }

    fn kind(&self, key: ResourceKey) -> Result<ResourceKind, ResourceError> {
        self.api.resolve(&key.group, &key.version, &key.resource)
    }

    async fn collect(
        &self,
        namespace: Option<String>,
        gitops_only: bool,
    ) -> Result<(Vec<Value>, bool), ResourceError> {
        let mut objects = Vec::new();
        let mut seen = BTreeSet::new();
        let mut partial = false;
        let mut bytes = 0_usize;
        for kind in self.api.kinds()? {
            let gitops = kind.group.ends_with(".toolkit.fluxcd.io") || kind.group == "argoproj.io";
            if (gitops_only && !gitops)
                || (!gitops_only
                    && !gitops
                    && !matches!(
                        kind.resource.as_str(),
                        "pods"
                            | "nodes"
                            | "deployments"
                            | "statefulsets"
                            | "persistentvolumeclaims"
                            | "jobs"
                    ))
            {
                continue;
            }
            if !kind.verbs.iter().any(|verb| verb == "list")
                || !seen.insert((kind.group.clone(), kind.resource.clone()))
            {
                continue;
            }
            if objects.len() >= 500 {
                partial = true;
                break;
            }
            let query = ListQuery {
                namespace: if kind.namespaced {
                    namespace.clone()
                } else {
                    None
                },
                limit: Some(50),
                ..ListQuery::default()
            };
            match self.api.list(&kind, &query).await {
                Ok(page) => {
                    partial |= page.continue_token.is_some();
                    for row in page.items {
                        bytes = bytes.saturating_add(approximate_size(&row.object));
                        if bytes > 4 * 1024 * 1024 {
                            partial = true;
                            break;
                        }
                        objects.push(row.object);
                    }
                    if bytes > 4 * 1024 * 1024 {
                        break;
                    }
                }
                Err(ResourceError::Closed) => return Err(ResourceError::Closed),
                Err(_) => partial = true,
            }
        }
        Ok((objects, partial))
    }
}

fn restore_argument_values(
    value: &mut Value,
    sanitizer: &SanitizerSession,
    budget: &mut usize,
) -> Result<(), ToolError> {
    match value {
        Value::String(text) => {
            let restored = sanitizer.rehydrate(text);
            if !restored.unknown_placeholders.is_empty()
                || !restored.unresolved_secrets.is_empty()
                || restored.text.len() > *budget
            {
                return Err(ToolError::InvalidTool);
            }
            *budget -= restored.text.len();
            *text = restored.text;
        }
        Value::Array(values) => {
            for value in values {
                restore_argument_values(value, sanitizer, budget)?;
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                restore_argument_values(value, sanitizer, budget)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn event_timestamp(event: &Value) -> Option<i64> {
    [
        &event["series"]["lastObservedTime"],
        &event["lastTimestamp"],
        &event["eventTime"],
        &event["metadata"]["creationTimestamp"],
    ]
    .into_iter()
    .filter_map(Value::as_str)
    .filter_map(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
    .map(|value| value.timestamp_millis())
    .max()
}

fn approximate_size(value: &Value) -> usize {
    match value {
        Value::Object(object) => object.iter().fold(2_usize, |size, (key, value)| {
            size.saturating_add(key.len())
                .saturating_add(4)
                .saturating_add(approximate_size(value))
        }),
        Value::Array(values) => values.iter().fold(2_usize, |size, value| {
            size.saturating_add(1)
                .saturating_add(approximate_size(value))
        }),
        Value::String(text) => text.len().saturating_add(2),
        _ => 24,
    }
}

fn bound_projection(value: &mut Value, budget: &mut usize) -> bool {
    if *budget < 64 {
        *value = Value::String("[truncated]".into());
        return true;
    }
    match value {
        Value::Object(object) => {
            let mut truncated = false;
            object.retain(|key, value| {
                if *budget < key.len().saturating_add(64) {
                    truncated = true;
                    return false;
                }
                *budget = budget.saturating_sub(key.len() + 4);
                truncated |= bound_projection(value, budget);
                true
            });
            truncated
        }
        Value::Array(values) => {
            let mut truncated = values.len() > 500;
            values.truncate(500);
            let mut keep = 0;
            for value in values.iter_mut() {
                if *budget < 64 {
                    truncated = true;
                    break;
                }
                truncated |= bound_projection(value, budget);
                keep += 1;
            }
            values.truncate(keep);
            truncated
        }
        Value::String(text) => {
            let allowed = (*budget).min(4096);
            let truncated = text.len() > allowed;
            if truncated {
                let boundary = (0..=allowed.saturating_sub(16))
                    .rev()
                    .find(|index| text.is_char_boundary(*index))
                    .unwrap_or(0);
                text.truncate(boundary);
                text.push_str("[truncated]");
            }
            *budget = budget.saturating_sub(text.len() + 2);
            truncated
        }
        _ => {
            *budget = budget.saturating_sub(24);
            false
        }
    }
}

pub fn project_for_ai(value: &mut Value, config_map_values: bool) {
    match value {
        Value::Object(object) => {
            let kind = object
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let core = object.get("apiVersion").and_then(Value::as_str) == Some("v1");
            if core && (kind == "Secret" || (kind == "ConfigMap" && !config_map_values)) {
                for key in ["data", "stringData", "binaryData"] {
                    if let Some(Value::Object(values)) = object.get_mut(key) {
                        for value in values.values_mut() {
                            *value = Value::String("[redacted]".into());
                        }
                    }
                }
            }
            if let Some(Value::Object(metadata)) = object.get_mut("metadata") {
                metadata.remove("managedFields");
                if let Some(Value::Object(annotations)) = metadata.get_mut("annotations") {
                    annotations.remove("kubectl.kubernetes.io/last-applied-configuration");
                    if core && kind == "Secret" {
                        annotations.clear();
                    }
                }
            }
            for (key, value) in object {
                let normalized = key.to_ascii_lowercase().replace(['-', '_'], "");
                if matches!(
                    normalized.as_str(),
                    "password"
                        | "passwd"
                        | "token"
                        | "accesstoken"
                        | "refreshtoken"
                        | "idtoken"
                        | "clientsecret"
                        | "privatekey"
                        | "clientkey"
                        | "authorization"
                        | "cookie"
                        | "accesskey"
                        | "secretkey"
                ) {
                    *value = Value::String("[redacted]".into());
                } else if key == "env" {
                    if let Some(entries) = value.as_array_mut() {
                        for entry in entries {
                            if let Some(value) = entry.get_mut("value") {
                                *value = Value::String("[redacted]".into());
                            }
                            project_for_ai(entry, config_map_values);
                        }
                    }
                } else {
                    project_for_ai(value, config_map_values);
                }
            }
        }
        Value::Array(entries) => {
            for value in entries {
                project_for_ai(value, config_map_values);
            }
        }
        _ => {}
    }
}

fn register_names(sanitizer: &mut SanitizerSession, value: &Value) {
    match value {
        Value::Object(object) => {
            if let Some(metadata) = object.get("metadata") {
                for key in ["name", "namespace"] {
                    if let Some(value) = metadata[key].as_str() {
                        sanitizer.add_literal(Category::Host, value);
                    }
                }
            }
            for key in ["name", "namespace"] {
                if let Some(identity) = object.get(key).and_then(Value::as_str) {
                    sanitizer.add_literal(Category::Host, identity);
                }
            }
            for value in object.values() {
                register_names(sanitizer, value);
            }
        }
        Value::Array(entries) => {
            for entry in entries {
                register_names(sanitizer, entry);
            }
        }
        _ => {}
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

pub fn tool_definitions() -> Vec<ToolDefinition> {
    let resource = json!({"type":"object","additionalProperties":false,"required":["group","version","resource"],"properties":{
        "group":{"type":"string"},"version":{"type":"string"},"resource":{"type":"string"}
    }});
    let nullable_string = json!({"type":["string","null"]});
    let definitions = [
        (
            "list_resources",
            "Read a bounded table of Kubernetes resources.",
            json!({"resource":resource,"namespace":nullable_string,"selector":nullable_string,"max_rows":{"type":"integer","minimum":1,"maximum":200}}),
            vec!["resource"],
        ),
        (
            "get_object",
            "Read a Kubernetes object with all Secret values masked.",
            json!({"resource":resource,"namespace":nullable_string,"name":{"type":"string"}}),
            vec!["resource", "name"],
        ),
        (
            "describe",
            "Read object conditions, ownership, configuration and status with secrets masked.",
            json!({"resource":resource,"namespace":nullable_string,"name":{"type":"string"}}),
            vec!["resource", "name"],
        ),
        (
            "events",
            "Read live events and explicitly enabled local event history, deduplicated by UID.",
            json!({"namespace":nullable_string,"object_uid":nullable_string,"since_seconds":{"type":["integer","null"],"minimum":0,"maximum":604800}}),
            vec![],
        ),
        (
            "logs_tail",
            "Read and sanitize at most 500 lines of pod logs.",
            json!({"namespace":{"type":"string"},"pod":{"type":"string"},"container":nullable_string,"lines":{"type":"integer","minimum":1,"maximum":500},"previous":{"type":"boolean"},"since_seconds":{"type":["integer","null"],"minimum":0}}),
            vec!["namespace", "pod"],
        ),
        (
            "top",
            "Read pod or node CPU and memory usage.",
            json!({"pods":{"type":"boolean"},"namespace":nullable_string}),
            vec![],
        ),
        (
            "problems",
            "Read bounded local diagnostics; partial results are explicitly marked.",
            json!({"namespace":nullable_string}),
            vec![],
        ),
        (
            "rollout_history",
            "Read controller-owned Deployment, StatefulSet or DaemonSet revisions.",
            json!({"resource":resource,"namespace":{"type":"string"},"name":{"type":"string"}}),
            vec!["resource", "namespace", "name"],
        ),
        (
            "diff_revisions",
            "Read differences between two controller-owned workload revisions.",
            json!({"resource":resource,"namespace":{"type":"string"},"name":{"type":"string"},"before":{"type":"integer","minimum":1},"after":{"type":"integer","minimum":1}}),
            vec!["resource", "namespace", "name", "before", "after"],
        ),
        (
            "helm_releases",
            "Read Helm release revision summaries.",
            json!({"namespace":{"type":"string"}}),
            vec!["namespace"],
        ),
        (
            "helm_values",
            "Read Helm values with string values masked.",
            json!({"namespace":{"type":"string"},"secret_name":{"type":"string"}}),
            vec!["namespace", "secret_name"],
        ),
        (
            "flux_status",
            "Read Flux and Argo objects without credentials.",
            json!({"namespace":nullable_string}),
            vec![],
        ),
        (
            "flux_graph",
            "Read GitOps dependency readiness and cycles.",
            json!({"namespace":nullable_string}),
            vec![],
        ),
    ];
    definitions.into_iter().map(|(name,description,properties,required)|ToolDefinition {name:name.into(),description:description.into(),
        parameters:json!({"type":"object","additionalProperties":false,"properties":properties,"required":required})}).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_mutating_tools_and_extra_arguments_are_rejected() {
        for request in [
            r#"{"tool":"delete_resource","resource":{"group":"","version":"v1","resource":"pods"}}"#,
            r#"{"tool":"get_object","resource":{"group":"","version":"v1","resource":"secrets"},"name":"x","reveal_secret_values":true}"#,
            r#"{"tool":"logs_tail","namespace":"apps","pod":"x","url":"https://attacker.example"}"#,
        ] {
            assert!(AiToolSession::parse_request(request).is_err());
        }
        assert!(AiToolSession::parse_request(
            r#"{"tool":"logs_tail","namespace":"apps","pod":"web"}"#
        )
        .is_ok());
        assert!(tool_definitions()
            .iter()
            .all(|definition| !definition.name.contains("delete")
                && !definition.name.contains("apply")));
    }
    #[test]
    fn structural_projection_masks_arbitrary_secret_keys_config_maps_and_env_literals() {
        let mut value = json!([
            {"apiVersion":"v1","kind":"Secret","metadata":{"name":"app","annotations":{"custom":"fixture-annotation"}},"data":{"unusual":"fixture-secret"}},
            {"apiVersion":"v1","kind":"ConfigMap","data":{"unusual":"fixture-config"}},
            {"apiVersion":"apps/v1","kind":"Deployment","spec":{"template":{"spec":{"containers":[{"name":"app","env":[{"name":"UNUSUAL","value":"fixture-env"}]}]}}}},
            {"spec":{"clientSecret":"fixture-client"}}
        ]);
        project_for_ai(&mut value, false);
        let text = value.to_string();
        for secret in [
            "fixture-annotation",
            "fixture-secret",
            "fixture-config",
            "fixture-env",
            "fixture-client",
        ] {
            assert!(!text.contains(secret));
        }
    }
}
