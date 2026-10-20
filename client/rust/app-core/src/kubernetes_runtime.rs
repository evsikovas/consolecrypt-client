//! Native ownership of Kubernetes connections. Flutter receives opaque handles;
//! policies and credentials are resolved from one captured vault generation.
use crate::dto::parse_id;
use crate::session::{Session, Unlocked};
use crate::working_set::{Entry, Item};
use crate::{AppCore, AppError, AppResult};
use cc_k8s_core::policy::{Audit, AuditEntry, DeviceClass, Policy};
use cc_k8s_core::resources::{
    ApplyError, Discovery, ListQuery, LogQuery, LogSnapshot, ReadOptions, ResourceApi,
    ResourceChange, ResourceDetail, ResourceError, ResourceKind, ResourcePage, ResourceRef,
    WatchOptions,
};
use cc_k8s_core::session::{ConnectOptions, ConnectionRoute, KubernetesSession};
use cc_models::host::Proxy;
use cc_models::kubernetes::{KubeCluster, KubeRoute};
use cc_models::{ObjectId, ObjectKind, VaultObject};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;
use url::Url;

const AUDIT_KEY: &str = "kubernetes.audit.v1";
const MAX_SESSIONS: usize = 16;

#[derive(Debug, Clone, Serialize)]
pub struct KubeSessionInfo {
    pub id: String,
    pub context_id: String,
    pub context_name: String,
    pub namespace: String,
    pub server_version: String,
    pub production: bool,
    pub allow_exec_in_prod: bool,
    pub allow_port_forward_in_prod: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KubeAuditRecord {
    pub timestamp_ms: i64,
    pub context_id: String,
    pub method: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct KubeWriteAccess {
    pub production: bool,
    pub can_request_window: bool,
    pub expires_at_ms: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct KubeWatchPoll {
    pub events: Vec<ResourceChange>,
    pub closed: bool,
}

struct StreamHandle<T> {
    receiver: tokio::sync::Mutex<tokio::sync::mpsc::Receiver<T>>,
    failure: Arc<Mutex<Option<ResourceError>>>,
    cancel: CancellationToken,
    task: tokio::task::AbortHandle,
}

impl<T> Drop for StreamHandle<T> {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.task.abort();
    }
}

#[derive(Serialize)]
pub struct KubeLogPoll {
    pub lines: Vec<String>,
    pub closed: bool,
}
impl std::fmt::Debug for KubeLogPoll {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KubeLogPoll")
            .field("lines", &self.lines.len())
            .field("closed", &self.closed)
            .finish()
    }
}

async fn poll_stream<T>(stream: &StreamHandle<T>, max_items: usize) -> AppResult<(Vec<T>, bool)> {
    let mut receiver = stream
        .receiver
        .try_lock()
        .map_err(|_| AppError::invalid("kubernetes", "stream_busy"))?;
    let first = tokio::select! {
        biased;
        _ = stream.cancel.cancelled() => return Ok((Vec::new(), true)),
        result = tokio::time::timeout(Duration::from_secs(1), receiver.recv()) => result,
    };
    let mut events = Vec::new();
    match first {
        Ok(Some(event)) => events.push(event),
        Ok(None) => {
            if let Some(error) = *stream.failure.lock().unwrap_or_else(|e| e.into_inner()) {
                return Err(resource_error(error));
            }
            return Ok((events, true));
        }
        Err(_) => {}
    }
    while events.len() < max_items {
        match receiver.try_recv() {
            Ok(event) => events.push(event),
            Err(_) => break,
        }
    }
    Ok((events, false))
}

struct DurableAudit(cc_storage_core::Storage);
impl Audit for DurableAudit {
    fn record(&self, entry: AuditEntry) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let record = KubeAuditRecord {
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            context_id: entry.context_id.to_string(),
            method: entry.method.to_string(),
            path: entry.path,
        };
        self.0.write_blocking(move |tx| {
            let mut records: Vec<KubeAuditRecord> = tx.setting_get(AUDIT_KEY)?.unwrap_or_default();
            let oldest = record.timestamp_ms - 7 * 24 * 60 * 60 * 1000;
            records.retain(|item| item.timestamp_ms >= oldest);
            if records.len() >= 4096 {
                records.drain(..records.len() - 4095);
            }
            records.push(record);
            tx.setting_set(AUDIT_KEY, &records)
        })?;
        Ok(())
    }
}

pub(crate) struct LiveSession {
    pub session: KubernetesSession,
    pub resources: ResourceApi,
    pub cancel: CancellationToken,
    pub info: KubeSessionInfo,
    pub terminal_slots: Arc<Semaphore>,
    pub cli_slots: Arc<Semaphore>,
    pub forwards: Arc<crate::kubernetes_forward::ForwardRuntime>,
    pub ai: crate::kubernetes_ai::KubeAiRuntime,
    pub history: Option<crate::kubernetes_history::EventRecorder>,
    pub insights_watch: crate::kubernetes_insights_watch::InsightsWatchRuntime,
    watches: Mutex<HashMap<String, Arc<StreamHandle<ResourceChange>>>>,
    logs: Mutex<HashMap<String, Arc<StreamHandle<String>>>>,
    _permit: OwnedSemaphorePermit,
}

impl Drop for LiveSession {
    fn drop(&mut self) {
        self.insights_watch.close();
        self.ai.close();
        self.cancel.cancel();
        self.session.close();
    }
}

pub(crate) struct KubeRuntime {
    pub(crate) auth: Arc<crate::kubernetes_auth::RuntimeAuth>,
    pub(crate) cli: Arc<crate::kubernetes_cli::FluxCliRuntime>,
    shutdown: CancellationToken,
    slots: Arc<Semaphore>,
    sessions: Mutex<HashMap<String, Arc<LiveSession>>>,
}

impl Default for KubeRuntime {
    fn default() -> Self {
        Self {
            auth: Arc::new(crate::kubernetes_auth::RuntimeAuth::default()),
            cli: Arc::new(crate::kubernetes_cli::FluxCliRuntime::default()),
            shutdown: CancellationToken::new(),
            slots: Arc::new(Semaphore::new(MAX_SESSIONS)),
            sessions: Mutex::new(HashMap::new()),
        }
    }
}

impl KubeRuntime {
    pub(crate) fn connections(&self) -> Vec<Arc<LiveSession>> {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .filter(|live| !live.cancel.is_cancelled())
            .cloned()
            .collect()
    }
    pub(crate) fn close(&self) {
        self.shutdown.cancel();
        self.auth.close();
        self.cli.close();
        self.slots.close();
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    pub(crate) fn get(&self, id: &str) -> AppResult<Arc<LiveSession>> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        sessions.retain(|_, live| !live.cancel.is_cancelled());
        let session = sessions
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::invalid("kube_session", "session_closed"))?;
        if session.cancel.is_cancelled() {
            return Err(AppError::invalid("kube_session", "session_closed"));
        }
        Ok(session)
    }
}

impl Drop for KubeRuntime {
    fn drop(&mut self) {
        self.close();
    }
}

struct PendingScope(Option<CancellationToken>);
impl Drop for PendingScope {
    fn drop(&mut self) {
        if let Some(cancel) = &self.0 {
            cancel.cancel();
        }
    }
}

pub(crate) fn resource_error(error: ResourceError) -> AppError {
    // Only the static code crosses the bridge. Kubernetes error bodies may
    // contain submitted data or credentials and are never forwarded.
    let code = serde_json::to_value(error)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unavailable".into());
    AppError::invalid("kubernetes", code)
}

fn object(snapshot: &BTreeMap<ObjectId, Entry>, id: ObjectId) -> AppResult<&VaultObject> {
    match snapshot.get(&id).map(|e| &e.item) {
        Some(Item::Object(value)) => Ok(value),
        _ => Err(AppError::invalid(
            "kubernetes",
            "connection_dependency_unavailable",
        )),
    }
}

/// Include inherited SSH/jump/proxy credentials as well as direct Kubernetes
/// credentials. A remote edit cannot silently redirect an existing connection.
pub(crate) fn dependencies(
    snapshot: &BTreeMap<ObjectId, Entry>,
    root: ObjectId,
) -> AppResult<BTreeMap<ObjectId, i64>> {
    let mut pending = vec![root];
    let mut result = BTreeMap::new();
    while let Some(id) = pending.pop() {
        if result.contains_key(&id) {
            continue;
        }
        if result.len() >= 512 {
            return Err(AppError::invalid("kubernetes", "route_too_complex"));
        }
        let entry = snapshot
            .get(&id)
            .ok_or_else(|| AppError::invalid("kubernetes", "connection_dependency_unavailable"))?;
        result.insert(id, entry.revision);
        if let Item::Object(value) = &entry.item {
            match value.as_ref() {
                VaultObject::DbConnection(v) => {
                    pending.extend(v.route.reference());
                    pending.extend(v.auth.secret_ids());
                    pending.extend(v.group_id);
                }
                VaultObject::KubeContext(v) => {
                    pending.push(v.cluster_id);
                    pending.extend(v.auth.secret_ids());
                }
                VaultObject::KubeCluster(v) => pending.extend(v.route.reference()),
                VaultObject::Host(v) => {
                    pending.extend(v.group_id);
                    pending.extend(v.credential_id);
                    pending.extend(v.jump_profile_id);
                    pending.extend(v.proxy_id);
                    pending.extend(&v.jump_chain);
                }
                VaultObject::JumpProfile(v) => pending.extend(&v.chain),
                VaultObject::Group(v) => {
                    pending.extend(v.parent_id);
                    pending.extend(v.inherited_credential_id);
                    pending.extend(v.inherited_jump_profile_id);
                }
                VaultObject::Credential(v) => {
                    pending.extend(v.secret_id);
                    pending.extend(v.passphrase_secret_id);
                }
                VaultObject::Proxy(v) => pending.extend(v.password_secret_id),
                _ => {}
            }
        }
    }
    Ok(result)
}

impl AppCore {
    pub async fn kube_connect(&self, context_id: String) -> AppResult<KubeSessionInfo> {
        let (owner, unlocked) = self.unlocked().await?;
        unlocked
            .kube
            .sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|_, live| !live.cancel.is_cancelled());
        let permit = unlocked
            .kube
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| AppError::invalid("kubernetes", "too_many_connections"))?;
        let cancel = unlocked.kube.shutdown.child_token();
        let mut pending = PendingScope(Some(cancel.clone()));
        let id = parse_id("context_id", &context_id)?;
        let snapshot = unlocked.working().snapshot();
        let VaultObject::KubeContext(context) = object(&snapshot, id)? else {
            return Err(AppError::invalid("context_id", "not_a_kubernetes_context"));
        };
        let context = context.clone();
        let VaultObject::KubeCluster(cluster) = object(&snapshot, context.cluster_id)? else {
            return Err(AppError::invalid("cluster_id", "not_a_kubernetes_cluster"));
        };
        let cluster = cluster.clone();
        context.validate_cluster(&cluster)?;
        let revisions = dependencies(&snapshot, context.id)?;
        if !unlocked
            .working()
            .watch_kube_revisions(revisions, cancel.clone())
        {
            return Err(AppError::invalid("kubernetes", "connection_changed"));
        }
        let connection = async {
            let auth = unlocked
                .kube
                .auth
                .resolve(&unlocked, &context, &cluster, cancel.clone())
                .await?;
            if let Some(expires_at) = auth.expires_at {
                let remaining = expires_at
                    .duration_since(std::time::SystemTime::now())
                    .map_err(|_| AppError::invalid("kubernetes", "authentication_expired"))?;
                let expired = cancel.clone();
                tokio::spawn(async move {
                    tokio::select! {
                        biased;
                        _ = expired.cancelled() => {},
                        _ = tokio::time::sleep(remaining) => expired.cancel(),
                    }
                });
            }
            let route = self
                .kube_route(&owner, &unlocked, &cluster, &cancel)
                .await?;
            let root_certificates = match &cluster.certificate_authority_pem {
                Some(bundle) => pem::parse_many(bundle)
                    .map_err(|_| AppError::invalid("kubernetes", "invalid_certificate"))?
                    .into_iter()
                    .map(|p| p.into_contents())
                    .collect(),
                None => Vec::new(),
            };
            let session = KubernetesSession::new_scoped(
                ConnectOptions {
                    server: Url::parse(&cluster.server)
                        .map_err(|_| AppError::invalid("kubernetes", "invalid_server"))?,
                    namespace: context.namespace.clone(),
                    root_certificates,
                    tls_server_name: cluster.tls_server_name.clone(),
                    insecure_skip_tls_verify: cluster.insecure_skip_tls_verify,
                    route,
                },
                auth.authentication,
                Policy {
                    context_id: context
                        .id
                        .to_string()
                        .parse()
                        .map_err(|_| AppError::invalid("context_id", "invalid_id"))?,
                    context_name: context.name.clone(),
                    production: context.production,
                    // The device class is fixed by the native build, not a Dart
                    // request or a synchronized flag.
                    device: if cfg!(any(target_os = "android", target_os = "ios")) {
                        DeviceClass::Mobile
                    } else {
                        DeviceClass::Desktop
                    },
                    allow_exec_in_production: context.allow_exec_in_prod,
                    allow_port_forward_in_production: context.allow_port_forward_in_prod,
                },
                Arc::new(DurableAudit(owner.storage.clone())),
                cancel.clone(),
            )
            .map_err(|_| AppError::invalid("kubernetes", "invalid_connection"))?;
            let resources = ResourceApi::new(&session);
            let version = tokio::time::timeout(Duration::from_secs(10), resources.server_version())
                .await
                .map_err(|_| AppError::invalid("kubernetes", "timeout"))?
                .map_err(resource_error)?;
            Ok::<_, AppError>((session, resources, version))
        };
        let (session, resources, server_version) = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(AppError::VaultLocked),
            result = connection => result?,
        };
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        if cancel.is_cancelled() {
            return Err(AppError::VaultLocked);
        }
        let history = context.event_history_enabled.then(|| {
            crate::kubernetes_history::EventRecorder::start(
                owner.storage.clone(),
                context_id.clone(),
                resources.clone(),
                cancel.clone(),
            )
        });
        let info = KubeSessionInfo {
            id: uuid::Uuid::new_v4().to_string(),
            context_id,
            context_name: context.name,
            namespace: context.namespace,
            production: context.production,
            allow_exec_in_prod: context.allow_exec_in_prod,
            allow_port_forward_in_prod: context.allow_port_forward_in_prod,
            server_version,
        };
        unlocked
            .kube
            .sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                info.id.clone(),
                Arc::new(LiveSession {
                    ai: crate::kubernetes_ai::KubeAiRuntime::default(),
                    history,
                    insights_watch: crate::kubernetes_insights_watch::InsightsWatchRuntime::default(
                    ),
                    session,
                    resources,
                    cancel,
                    info: info.clone(),
                    terminal_slots: Arc::new(Semaphore::new(8)),
                    cli_slots: Arc::new(Semaphore::new(8)),
                    forwards: Arc::new(crate::kubernetes_forward::ForwardRuntime::default()),
                    watches: Mutex::new(HashMap::new()),
                    logs: Mutex::new(HashMap::new()),
                    _permit: permit,
                }),
            );
        pending.0.take();
        Ok(info)
    }

    pub async fn kube_disconnect(&self, session_id: String) -> AppResult<()> {
        let (_, u) = self.unlocked().await?;
        if let Some(session) = u
            .kube
            .sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&session_id)
        {
            session.cancel.cancel();
            session.session.close();
        }
        Ok(())
    }

    pub(crate) async fn kube_live(&self, session_id: &str) -> AppResult<Arc<LiveSession>> {
        let (_, u) = self.unlocked().await?;
        u.kube.get(session_id)
    }

    /// Capture one vault generation for the whole operation. Transport
    /// cancellation alone is insufficient: a response may already be buffered
    /// when the vault locks or the active profile changes.
    pub(crate) async fn with_kube_session<T, F, Fut>(
        &self,
        session_id: &str,
        operation: F,
    ) -> AppResult<T>
    where
        F: FnOnce(Arc<LiveSession>) -> Fut,
        Fut: std::future::Future<Output = AppResult<T>>,
    {
        let (owner, unlocked) = self.unlocked().await?;
        let live = unlocked.kube.get(session_id)?;
        let result = tokio::select! {
            biased;
            _ = live.cancel.cancelled() => return Err(AppError::Cancelled),
            result = operation(live.clone()) => result,
        };
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        if live.cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        result
    }

    pub async fn kube_discover(&self, session_id: String) -> AppResult<Discovery> {
        self.with_kube_session(&session_id, |live| async move {
            live.resources.discover().await.map_err(resource_error)
        })
        .await
    }
    pub async fn kube_namespaces(&self, session_id: String) -> AppResult<Vec<String>> {
        self.with_kube_session(&session_id, |live| async move {
            live.resources.namespaces().await.map_err(resource_error)
        })
        .await
    }
    pub async fn kube_resources_list(
        &self,
        session_id: String,
        kind: ResourceKind,
        query: ListQuery,
    ) -> AppResult<ResourcePage> {
        self.with_kube_session(&session_id, |live| async move {
            live.resources
                .list(&kind, &query)
                .await
                .map_err(resource_error)
        })
        .await
    }
    pub async fn kube_resource_get(
        &self,
        session_id: String,
        reference: ResourceRef,
    ) -> AppResult<ResourceDetail> {
        self.with_kube_session(&session_id, |live| async move {
            live.resources
                .get(&reference, ReadOptions::default())
                .await
                .map_err(resource_error)
        })
        .await
    }
    pub async fn kube_logs(
        &self,
        session_id: String,
        namespace: String,
        pod: String,
        query: LogQuery,
    ) -> AppResult<LogSnapshot> {
        self.with_kube_session(&session_id, |live| async move {
            live.resources
                .logs(&namespace, &pod, &query)
                .await
                .map_err(resource_error)
        })
        .await
    }

    pub async fn kube_watch_start(
        &self,
        session_id: String,
        kind: ResourceKind,
        query: ListQuery,
        resource_version: Option<String>,
    ) -> AppResult<String> {
        self.with_kube_session(&session_id, |live| async move {
            let mut watches = live.watches.lock().unwrap_or_else(|e| e.into_inner());
            if watches.len() >= 4 {
                return Err(AppError::invalid("kubernetes", "too_many_watches"));
            }
            let (sender, receiver) = tokio::sync::mpsc::channel(2);
            let cancel = live.cancel.child_token();
            let failure = Arc::new(Mutex::new(None));
            let api = live.resources.clone();
            let worker_cancel = cancel.clone();
            let worker_failure = failure.clone();
            let task = tokio::spawn(async move {
                let completion = sender.clone();
                if let Err(error) = api
                    .watch(
                        &kind,
                        WatchOptions {
                            query,
                            resource_version,
                        },
                        sender,
                        worker_cancel,
                    )
                    .await
                {
                    *worker_failure.lock().unwrap_or_else(|e| e.into_inner()) = Some(error);
                }
                drop(completion);
            });
            let id = uuid::Uuid::new_v4().to_string();
            watches.insert(
                id.clone(),
                Arc::new(StreamHandle {
                    receiver: tokio::sync::Mutex::new(receiver),
                    cancel,
                    failure,
                    task: task.abort_handle(),
                }),
            );
            Ok(id)
        })
        .await
    }

    pub async fn kube_watch_poll(
        &self,
        session_id: String,
        watch_id: String,
    ) -> AppResult<KubeWatchPoll> {
        self.with_kube_session(&session_id, |live| async move {
            let watch = live
                .watches
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&watch_id)
                .cloned()
                .ok_or_else(|| AppError::invalid("kubernetes", "watch_closed"))?;
            let (events, closed) = poll_stream(&watch, 4).await?;
            Ok(KubeWatchPoll { events, closed })
        })
        .await
    }

    pub async fn kube_watch_stop(&self, session_id: String, watch_id: String) -> AppResult<()> {
        self.with_kube_session(&session_id, |live| async move {
            if let Some(watch) = live
                .watches
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&watch_id)
            {
                watch.cancel.cancel();
                watch.task.abort();
            }
            Ok(())
        })
        .await
    }

    pub async fn kube_logs_start(
        &self,
        session_id: String,
        namespace: String,
        pod: String,
        query: LogQuery,
    ) -> AppResult<String> {
        self.with_kube_session(&session_id, |live| async move {
            let mut logs = live.logs.lock().unwrap_or_else(|e| e.into_inner());
            if logs.len() >= 8 {
                return Err(AppError::invalid("kubernetes", "too_many_log_streams"));
            }
            let (sender, receiver) = tokio::sync::mpsc::channel(8);
            let cancel = live.cancel.child_token();
            let failure = Arc::new(Mutex::new(None));
            let api = live.resources.clone();
            let worker_cancel = cancel.clone();
            let worker_failure = failure.clone();
            let task = tokio::spawn(async move {
                let completion = sender.clone();
                if let Err(error) = api
                    .follow_logs(&namespace, &pod, &query, sender, worker_cancel)
                    .await
                {
                    *worker_failure.lock().unwrap_or_else(|e| e.into_inner()) = Some(error);
                }
                drop(completion);
            });
            let id = uuid::Uuid::new_v4().to_string();
            logs.insert(
                id.clone(),
                Arc::new(StreamHandle {
                    receiver: tokio::sync::Mutex::new(receiver),
                    cancel,
                    failure,
                    task: task.abort_handle(),
                }),
            );
            Ok(id)
        })
        .await
    }

    pub async fn kube_logs_poll(
        &self,
        session_id: String,
        stream_id: String,
    ) -> AppResult<KubeLogPoll> {
        self.with_kube_session(&session_id, |live| async move {
            let stream = live
                .logs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&stream_id)
                .cloned()
                .ok_or_else(|| AppError::invalid("kubernetes", "stream_closed"))?;
            let (lines, closed) = poll_stream(&stream, 8).await?;
            Ok(KubeLogPoll { lines, closed })
        })
        .await
    }

    pub async fn kube_logs_stop(&self, session_id: String, stream_id: String) -> AppResult<()> {
        self.with_kube_session(&session_id, |live| async move {
            if let Some(stream) = live
                .logs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&stream_id)
            {
                stream.cancel.cancel();
                stream.task.abort();
            }
            Ok(())
        })
        .await
    }

    pub async fn kube_write_access(&self, session_id: String) -> AppResult<KubeWriteAccess> {
        self.with_kube_session(&session_id, |live| async move {
            Ok(KubeWriteAccess {
                production: live.info.production,
                can_request_window: live.info.production
                    && !cfg!(any(target_os = "android", target_os = "ios")),
                expires_at_ms: live
                    .session
                    .guard()
                    .write_window_remaining()
                    .map(|remaining| {
                        chrono::Utc::now().timestamp_millis() + remaining.as_millis() as i64
                    }),
            })
        })
        .await
    }

    pub async fn kube_apply_yaml(
        &self,
        session_id: String,
        reference: ResourceRef,
        manifest: zeroize::Zeroizing<Vec<u8>>,
        dry_run: bool,
        force: bool,
    ) -> AppResult<ResourceDetail> {
        self.with_kube_session(&session_id, |live| async move {
            if manifest.len() > 2 * 1024 * 1024 {
                return Err(AppError::invalid("kubernetes", "invalid_manifest"));
            }
            let text = std::str::from_utf8(&manifest)
                .map_err(|_| AppError::invalid("kubernetes", "invalid_manifest"))?;
            live.resources
                .apply(
                    &reference,
                    zeroize::Zeroizing::new(text.to_owned()),
                    dry_run,
                    force,
                )
                .await
                .map_err(|error| match error {
                    ApplyError::Resource(error) => resource_error(error),
                    // Only manager names and field paths cross the bridge.
                    ApplyError::Rejected(fields) => AppError::InvalidInput {
                        field: "kubernetes_fields".into(),
                        reason: fields.join("\n"),
                    },
                    ApplyError::FieldConflict(conflict) => AppError::InUse {
                        what: "kubernetes_fields".into(),
                        id: conflict.fields.join("\n"),
                        used_by: conflict.managers.join(", "),
                    },
                })
        })
        .await
    }

    pub async fn kube_resource_delete(
        &self,
        session_id: String,
        reference: ResourceRef,
        uid: String,
        resource_version: String,
    ) -> AppResult<()> {
        self.with_kube_session(&session_id, |live| async move {
            live.resources
                .delete(&reference, &uid, &resource_version)
                .await
                .map_err(resource_error)
        })
        .await
    }

    pub async fn kube_open_write_window(
        &self,
        session_id: String,
        typed_context_name: String,
    ) -> AppResult<u64> {
        self.with_kube_session(&session_id, |live| async move {
            live.session
                .guard()
                .open_write_window(&typed_context_name)
                .map_err(|_| AppError::invalid("kubernetes", "write_window_denied"))?;
            Ok(600)
        })
        .await
    }
    pub async fn kube_close_write_window(&self, session_id: String) -> AppResult<()> {
        self.with_kube_session(&session_id, |live| async move {
            live.session.guard().close_write_window();
            Ok(())
        })
        .await
    }
    pub async fn kube_audit(&self) -> AppResult<Vec<KubeAuditRecord>> {
        let (owner, unlocked) = self.unlocked().await?;
        let audit = owner
            .storage
            .setting_get(AUDIT_KEY)
            .await?
            .unwrap_or_default();
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        Ok(audit)
    }

    async fn kube_route(
        &self,
        owner: &Arc<Session>,
        u: &Arc<Unlocked>,
        cluster: &KubeCluster,
        cancel: &CancellationToken,
    ) -> AppResult<ConnectionRoute> {
        let server = Url::parse(&cluster.server)
            .map_err(|_| AppError::invalid("kubernetes", "invalid_server"))?;
        let target = server
            .host_str()
            .ok_or_else(|| AppError::invalid("kubernetes", "invalid_server"))?
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_owned();
        let port = server
            .port_or_known_default()
            .ok_or_else(|| AppError::invalid("kubernetes", "invalid_server"))?;
        let transport = match &cluster.route {
            KubeRoute::Direct => return Ok(ConnectionRoute::Direct),
            KubeRoute::Ssh { host_id } => {
                let prepared = self
                    .sharing_prepare_connection(&host_id.to_string())
                    .await?;
                if !Arc::ptr_eq(u, &prepared.unlocked) {
                    return Err(AppError::VaultLocked);
                }
                let session = prepared
                    .run(async { Ok(u.ssh.connector.connect(&prepared.plan).await?) })
                    .await?;
                RouteTransport::Ssh(Arc::new(session))
            }
            KubeRoute::Jump { jump_profile_id } => {
                let jump = u
                    .working()
                    .jump_profile(*jump_profile_id)
                    .ok_or_else(|| AppError::invalid("kubernetes", "route_unavailable"))?;
                if jump.chain.is_empty() || jump.chain.len() > cc_ssh_core::planner::MAX_HOPS {
                    return Err(AppError::invalid("kubernetes", "invalid_jump_chain"));
                }
                let mut visited = BTreeSet::new();
                let mut plans = Vec::new();
                for id in jump.chain {
                    if !visited.insert(id) {
                        return Err(AppError::invalid("kubernetes", "invalid_jump_chain"));
                    }
                    let prepared = self.sharing_prepare_connection(&id.to_string()).await?;
                    if !Arc::ptr_eq(u, &prepared.unlocked) {
                        return Err(AppError::VaultLocked);
                    }
                    plans.push(prepared.plan);
                }
                let first_proxy = plans[0].proxy.clone();
                let mut last = plans
                    .pop()
                    .ok_or_else(|| AppError::invalid("kubernetes", "invalid_jump_chain"))?;
                last.route = plans.into_iter().map(|p| p.target_hop()).collect();
                last.proxy = first_proxy;
                RouteTransport::Ssh(Arc::new(u.ssh.connector.connect(&last).await?))
            }
            KubeRoute::Proxy { proxy_id } => {
                let proxy = u
                    .working()
                    .proxy(*proxy_id)
                    .ok_or_else(|| AppError::invalid("kubernetes", "route_unavailable"))?;
                let password = match proxy.password_secret_id {
                    Some(id) => Some(SecretString::from(
                        u.writer.read_secret(id).await?.value.expose_secret(),
                    )),
                    None => None,
                };
                RouteTransport::Proxy(proxy, password)
            }
        };
        // Include host keys learned by the connection itself in the fence.
        let trust = u
            .working()
            .snapshot()
            .into_iter()
            .filter(|(_, e)| e.kind() == ObjectKind::KnownHost)
            .map(|(id, entry)| (id, entry.revision))
            .collect();
        if !u.working().watch_revisions(trust, cancel.clone()) {
            return Err(AppError::VaultLocked);
        }
        let _profile = self.hold_session(owner).await?;
        let _generation = owner.hold_unlocked(u).await?;
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|_| AppError::invalid("kubernetes", "route_unavailable"))?;
        let address = listener
            .local_addr()
            .map_err(|_| AppError::invalid("kubernetes", "route_unavailable"))?;
        tokio::spawn(route_listener(
            listener,
            transport,
            target,
            port,
            cancel.clone(),
        ));
        Ok(ConnectionRoute::SshLoopback(address))
    }
}

#[derive(Clone)]
enum RouteTransport {
    Ssh(Arc<cc_ssh_core::SshSession>),
    Proxy(Proxy, Option<SecretString>),
}

async fn route_listener(
    listener: TcpListener,
    transport: RouteTransport,
    target: String,
    port: u16,
    cancel: CancellationToken,
) {
    let slots = Arc::new(Semaphore::new(32));
    let mut tasks = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => break,
            Some(_) = tasks.join_next(), if !tasks.is_empty() => {},
            incoming = listener.accept() => {
                let Ok((mut socket, _)) = incoming else { break; };
                let Ok(slot) = slots.clone().try_acquire_owned() else { continue; };
                let route = transport.clone();
                let target = target.clone();
                let cancelled = cancel.clone();
                tasks.spawn(async move {
                    let _slot = slot;
                    let copy = async {
                        match route {
                            RouteTransport::Ssh(ssh) => {
                                if let Ok(mut remote) = ssh.open_direct_tcpip(&target, port, None).await {
                                    let _ = tokio::io::copy_bidirectional(&mut socket, &mut remote).await;
                                }
                            }
                            RouteTransport::Proxy(proxy, password) => {
                                if let Ok(Ok(mut remote)) = tokio::time::timeout(Duration::from_secs(10), cc_ssh_core::proxy::connect_via_proxy(&proxy, password.as_ref(), &target, port)).await {
                                    let _ = tokio::io::copy_bidirectional(&mut socket, &mut remote).await;
                                }
                            }
                        }
                    };
                    tokio::select! { biased; _ = cancelled.cancelled() => {}, _ = copy => {} }
                });
            }
        }
    }
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    if let RouteTransport::Ssh(ssh) = transport {
        let _ = ssh.disconnect().await;
    }
}

#[cfg(test)]
mod tests;
