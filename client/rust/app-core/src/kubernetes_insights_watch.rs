//! A visible GitOps overview receives only coalesced invalidation flags.
//! Object bodies are consumed and dropped natively; no snapshot cache or
//! unbounded event queue is retained by this subscription.
use crate::{AppCore, AppError, AppResult};
use cc_k8s_core::resources::{ResourceApi, ResourceChange, ResourceKind};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

const MAX_KINDS: usize = 16;
const POLL_LEASE_MS: u64 = 30_000;

#[derive(Debug, Serialize)]
pub struct InsightsWatchStarted {
    pub id: String,
    pub partial: bool,
}
#[derive(Debug, Serialize)]
pub struct InsightsWatchPoll {
    pub dirty: bool,
    pub closed: bool,
    pub partial: bool,
}

#[derive(Default)]
pub(crate) struct InsightsWatchRuntime {
    handle: Mutex<Option<Arc<Handle>>>,
}
struct State {
    dirty: AtomicBool,
    partial: AtomicBool,
    closed: AtomicBool,
    notify: Notify,
    poll: tokio::sync::Mutex<()>,
    started: Instant,
    last_poll_ms: AtomicU64,
}
impl State {
    fn invalidate(&self) {
        self.dirty.store(true, Ordering::Release);
        self.notify.notify_one();
    }
    fn mark_partial(&self) {
        if !self.partial.swap(true, Ordering::AcqRel) {
            self.notify.notify_one();
        }
    }
    fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis().min(u64::MAX as u128) as u64
    }
}
struct Handle {
    id: String,
    state: Arc<State>,
    cancel: CancellationToken,
    task: tokio::task::AbortHandle,
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.close();
    }
}
impl Handle {
    fn close(&self) {
        self.cancel.cancel();
        self.state.closed.store(true, Ordering::Release);
        self.state.notify.notify_one();
        self.task.abort();
    }
}
impl InsightsWatchRuntime {
    pub(crate) fn close(&self) {
        if let Some(handle) = self.handle.lock().unwrap_or_else(|e| e.into_inner()).take() {
            handle.close();
        }
    }
    fn start(
        &self,
        api: ResourceApi,
        kinds: Vec<ResourceKind>,
        namespace: Option<String>,
        scope: CancellationToken,
        partial: bool,
    ) -> AppResult<InsightsWatchStarted> {
        let mut slot = self.handle.lock().unwrap_or_else(|e| e.into_inner());
        if slot.as_ref().is_some_and(|handle| {
            !handle.state.closed.load(Ordering::Acquire) && !handle.cancel.is_cancelled()
        }) {
            return Err(AppError::invalid("kubernetes", "insights_watch_exists"));
        }
        let state = Arc::new(State {
            dirty: AtomicBool::new(false),
            partial: AtomicBool::new(partial),
            closed: AtomicBool::new(false),
            notify: Notify::new(),
            poll: tokio::sync::Mutex::new(()),
            started: Instant::now(),
            last_poll_ms: AtomicU64::new(0),
        });
        let cancel = scope.child_token();
        let task = tokio::spawn(run(api, kinds, namespace, state.clone(), cancel.clone()));
        let id = uuid::Uuid::new_v4().to_string();
        *slot = Some(Arc::new(Handle {
            id: id.clone(),
            state,
            cancel,
            task: task.abort_handle(),
        }));
        Ok(InsightsWatchStarted { id, partial })
    }
    fn get(&self, id: &str) -> AppResult<Arc<Handle>> {
        self.handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .filter(|handle| handle.id == id)
            .cloned()
            .ok_or_else(|| AppError::invalid("kubernetes", "unknown_insights_watch"))
    }
    fn stop(&self, id: &str) {
        let mut slot = self.handle.lock().unwrap_or_else(|e| e.into_inner());
        if slot.as_ref().is_some_and(|handle| handle.id == id) {
            if let Some(handle) = slot.take() {
                handle.close();
            }
        }
    }
}

fn select_kinds(api: &ResourceApi, mut kinds: Vec<ResourceKind>) -> (Vec<ResourceKind>, bool) {
    kinds.retain(|kind| {
        matches!(
            kind.group.as_str(),
            "source.toolkit.fluxcd.io"
                | "kustomize.toolkit.fluxcd.io"
                | "helm.toolkit.fluxcd.io"
                | "notification.toolkit.fluxcd.io"
                | "image.toolkit.fluxcd.io"
        ) || kind.group == "argoproj.io" && kind.kind == "Application"
    });
    let mut partial = kinds.iter().any(|kind| {
        !kind.verbs.iter().any(|v| v == "watch") || !kind.verbs.iter().any(|v| v == "list")
    });
    kinds.retain(|kind| {
        kind.verbs.iter().any(|v| v == "watch") && kind.verbs.iter().any(|v| v == "list")
    });
    kinds.sort_by_key(|kind| {
        (
            api.preferred_version(&kind.group).as_deref() != Some(kind.version.as_str()),
            kind.version.contains("alpha"),
            kind.version.contains("beta"),
            kind.group.clone(),
            kind.resource.clone(),
            kind.version.clone(),
        )
    });
    let mut seen = BTreeSet::new();
    kinds.retain(|kind| seen.insert((kind.group.clone(), kind.resource.clone())));
    partial |= kinds.len() > MAX_KINDS;
    kinds.truncate(MAX_KINDS);
    (kinds, partial)
}

async fn run(
    api: ResourceApi,
    kinds: Vec<ResourceKind>,
    namespace: Option<String>,
    state: Arc<State>,
    cancel: CancellationToken,
) {
    let mut jobs = tokio::task::JoinSet::new();
    for kind in kinds {
        let api = api.clone();
        let state = state.clone();
        let stop = cancel.child_token();
        let namespace = if kind.namespaced {
            namespace.clone()
        } else {
            None
        };
        jobs.spawn(async move {
            let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
            let watch = api.watch_invalidation(&kind, namespace, sender, stop.clone());
            tokio::pin!(watch);
            loop {
                tokio::select! {
                    biased;
                    _ = stop.cancelled() => return,
                    event = receiver.recv() => match event {
                        Some(ResourceChange::Bookmark { .. }) => {},
                        Some(ResourceChange::Reconnecting) => state.mark_partial(),
                        Some(_) => state.invalidate(),
                        None => return,
                    },
                    result = &mut watch => {
                        if result.is_err() && !stop.is_cancelled() { state.mark_partial(); }
                        return;
                    }
                }
            }
        });
    }
    let mut lease = tokio::time::interval(Duration::from_secs(5));
    while !jobs.is_empty() {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => break,
            _ = lease.tick() => {
                if state.elapsed_ms().saturating_sub(state.last_poll_ms.load(Ordering::Acquire)) > POLL_LEASE_MS {
                    cancel.cancel();
                    break;
                }
            }
            result = jobs.join_next() => {
                if result.is_some_and(|result| result.is_err()) { state.mark_partial(); }
            }
        }
    }
    jobs.abort_all();
    while jobs.join_next().await.is_some() {}
    state.closed.store(true, Ordering::Release);
    state.notify.notify_one();
}

impl AppCore {
    pub async fn kube_insights_watch_start(
        &self,
        session_id: String,
        namespace: Option<String>,
    ) -> AppResult<InsightsWatchStarted> {
        self.with_kube_session(&session_id, |live| async move {
            if namespace.as_deref().is_some_and(|ns| {
                ns.is_empty()
                    || ns.len() > 63
                    || ns.starts_with('-')
                    || ns.ends_with('-')
                    || !ns
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
            }) {
                return Err(AppError::invalid("kubernetes", "invalid_namespace"));
            }
            let mut kinds = live
                .resources
                .kinds()
                .map_err(crate::kubernetes_runtime::resource_error)?;
            let mut partial = live.resources.discovery_is_partial();
            if kinds.is_empty() {
                let discovery = live
                    .resources
                    .discover()
                    .await
                    .map_err(crate::kubernetes_runtime::resource_error)?;
                partial |= !discovery.unavailable_groups.is_empty();
                kinds = discovery.resources;
            }
            let (kinds, incomplete) = select_kinds(&live.resources, kinds);
            live.insights_watch.start(
                live.resources.clone(),
                kinds,
                namespace,
                live.cancel.clone(),
                partial || incomplete,
            )
        })
        .await
    }
    pub async fn kube_insights_watch_poll(
        &self,
        session_id: String,
        watch_id: String,
    ) -> AppResult<InsightsWatchPoll> {
        self.with_kube_session(&session_id, |live| async move {
            let handle = live.insights_watch.get(&watch_id)?;
            let state = &handle.state;
            let _poll = state
                .poll
                .try_lock()
                .map_err(|_| AppError::invalid("kubernetes", "stream_busy"))?;
            state
                .last_poll_ms
                .store(state.elapsed_ms(), Ordering::Release);
            let notified = state.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !state.dirty.load(Ordering::Acquire) && !state.closed.load(Ordering::Acquire) {
                tokio::select! {
                    biased;
                    _ = handle.cancel.cancelled() => {},
                    _ = tokio::time::timeout(Duration::from_secs(1), &mut notified) => {},
                }
            }
            Ok(InsightsWatchPoll {
                dirty: state.dirty.swap(false, Ordering::AcqRel),
                closed: state.closed.load(Ordering::Acquire) || handle.cancel.is_cancelled(),
                partial: state.partial.load(Ordering::Acquire),
            })
        })
        .await
    }
    pub async fn kube_insights_watch_stop(
        &self,
        session_id: String,
        watch_id: String,
    ) -> AppResult<()> {
        self.with_kube_session(&session_id, |live| async move {
            live.insights_watch.stop(&watch_id);
            Ok(())
        })
        .await
    }
}
