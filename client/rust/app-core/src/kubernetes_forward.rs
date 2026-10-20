//! Device-local loopback forwards owned by the captured Kubernetes session.
use crate::{AppCore, AppError, AppResult};
use cc_k8s_core::resources::ForwardUpdate;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::{mpsc, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KubeForwardRequest {
    pub namespace: String,
    pub pod: String,
    pub remote_port: u16,
    pub local_port: u16,
    pub local_address: String,
    pub approve_network: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct KubeForwardInfo {
    pub id: String,
    pub session_id: String,
    pub context_name: String,
    pub namespace: String,
    pub pod: String,
    pub remote_port: u16,
    pub address: String,
    pub state: String,
    pub error_code: Option<String>,
    pub active_connections: usize,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub failed_connections: u64,
}

struct ForwardHandle {
    info: Arc<Mutex<KubeForwardInfo>>,
    stop: CancellationToken,
    task: tokio::task::AbortHandle,
    _slot: OwnedSemaphorePermit,
}
impl Drop for ForwardHandle {
    fn drop(&mut self) {
        self.stop.cancel();
        self.task.abort();
    }
}
pub(crate) struct ForwardRuntime {
    handles: Mutex<HashMap<String, ForwardHandle>>,
    slots: Arc<Semaphore>,
}
impl Default for ForwardRuntime {
    fn default() -> Self {
        Self {
            handles: Mutex::new(HashMap::new()),
            slots: Arc::new(Semaphore::new(8)),
        }
    }
}

#[cfg(target_os = "android")]
fn mobile_forwards() -> &'static Mutex<Vec<std::sync::Weak<ForwardRuntime>>> {
    static REGISTRY: std::sync::OnceLock<Mutex<Vec<std::sync::Weak<ForwardRuntime>>>> =
        std::sync::OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}

/// Android's foreground service owns the notification and must be able to stop
/// sockets even if Dart is suspended or its Activity has already been removed.
#[cfg(target_os = "android")]
pub fn mobile_forward_stop_all() {
    let mut registry = mobile_forwards().lock().unwrap_or_else(|e| e.into_inner());
    for runtime in registry.iter().filter_map(std::sync::Weak::upgrade) {
        runtime
            .handles
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
    registry.clear();
}

#[cfg(target_os = "android")]
pub fn mobile_forward_count() -> usize {
    let mut registry = mobile_forwards().lock().unwrap_or_else(|e| e.into_inner());
    registry.retain(|runtime| runtime.strong_count() > 0);
    registry
        .iter()
        .filter_map(std::sync::Weak::upgrade)
        .map(|runtime| {
            runtime
                .handles
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .filter(|handle| {
                    !handle.stop.is_cancelled()
                        && handle.info.lock().unwrap_or_else(|e| e.into_inner()).state == "running"
                })
                .count()
        })
        .sum()
}

impl AppCore {
    pub async fn kube_forward_targets(
        &self,
        session_id: String,
        reference: cc_k8s_core::resources::ResourceRef,
    ) -> AppResult<cc_k8s_core::resources::ForwardTargets> {
        let (owner, unlocked) = self.unlocked().await?;
        let live = unlocked.kube.get(&session_id)?;
        let targets = live
            .resources
            .forward_targets(&reference)
            .await
            .map_err(super::kubernetes_runtime::resource_error)?;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        if live.cancel.is_cancelled() {
            return Err(AppError::VaultLocked);
        }
        Ok(targets)
    }

    /// Called only after the user confirms the displayed pod and local port.
    pub async fn kube_forward_start(
        &self,
        session_id: String,
        namespace: String,
        pod: String,
        remote_port: u16,
        local_port: u16,
    ) -> AppResult<KubeForwardInfo> {
        self.kube_forward_start_configured(
            session_id,
            KubeForwardRequest {
                namespace,
                pod,
                remote_port,
                local_port,
                local_address: "127.0.0.1".into(),
                approve_network: false,
            },
        )
        .await
    }

    pub async fn kube_forward_start_configured(
        &self,
        session_id: String,
        request: KubeForwardRequest,
    ) -> AppResult<KubeForwardInfo> {
        let KubeForwardRequest {
            namespace,
            pod,
            remote_port,
            local_port,
            local_address,
            approve_network,
        } = request;
        let ip: std::net::IpAddr = local_address
            .parse()
            .map_err(|_| AppError::invalid("kubernetes", "invalid_bind_address"))?;
        let (owner, unlocked) = self.unlocked().await?;
        let live = unlocked.kube.get(&session_id)?;
        let slot = live
            .forwards
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| AppError::invalid("kubernetes", "too_many_forwards"))?;
        let listener = live
            .resources
            .forward_listener_on(
                &namespace,
                &pod,
                remote_port,
                std::net::SocketAddr::new(ip, local_port),
                approve_network,
            )
            .await
            .map_err(super::kubernetes_runtime::resource_error)?;
        let info = KubeForwardInfo {
            id: uuid::Uuid::new_v4().to_string(),
            session_id,
            context_name: live.info.context_name.clone(),
            namespace,
            pod,
            remote_port,
            address: listener
                .address()
                .map_err(super::kubernetes_runtime::resource_error)?
                .to_string(),
            state: "running".into(),
            error_code: None,
            active_connections: 0,
            bytes_sent: 0,
            bytes_received: 0,
            failed_connections: 0,
        };
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        if live.cancel.is_cancelled() {
            return Err(AppError::VaultLocked);
        }
        let shared = Arc::new(Mutex::new(info.clone()));
        let state = shared.clone();
        let stop = listener.cancellation();
        let (sender, mut receiver) = mpsc::channel::<ForwardUpdate>(1);
        let task = tokio::spawn(async move {
            let serve = listener.serve(sender);
            tokio::pin!(serve);
            let mut receive = true;
            loop {
                tokio::select! {
                    result = &mut serve => {
                        let mut info = state.lock().unwrap_or_else(|e| e.into_inner());
                        info.active_connections = 0;
                        info.state = if result.is_ok() { "stopped" } else { "failed" }.into();
                        info.error_code = result.err().and_then(|error| serde_json::to_value(error).ok())
                            .and_then(|value| value.as_str().map(str::to_owned));
                        break;
                    },
                    update = receiver.recv(), if receive => {
                        let Some(update) = update else { receive = false; continue; };
                        let mut info = state.lock().unwrap_or_else(|e| e.into_inner());
                        info.active_connections = update.active_connections;
                        info.bytes_sent = update.bytes_sent;
                        info.bytes_received = update.bytes_received;
                        info.failed_connections = update.failed_connections;
                    },
                }
            }
        });
        #[cfg(target_os = "android")]
        let mut registry = mobile_forwards().lock().unwrap_or_else(|e| e.into_inner());
        live.forwards
            .handles
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                info.id.clone(),
                ForwardHandle {
                    info: shared,
                    stop,
                    task: task.abort_handle(),
                    _slot: slot,
                },
            );
        #[cfg(target_os = "android")]
        {
            let weak = Arc::downgrade(&live.forwards);
            if !registry.iter().any(|existing| existing.ptr_eq(&weak)) {
                registry.push(weak);
            }
        }
        Ok(info)
    }

    pub async fn kube_forward_list(&self) -> AppResult<Vec<KubeForwardInfo>> {
        let (_, unlocked) = self.unlocked().await?;
        let mut result = Vec::new();
        for live in unlocked.kube.connections() {
            result.extend(
                live.forwards
                    .handles
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .values()
                    .map(|handle| {
                        handle
                            .info
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .clone()
                    }),
            );
        }
        result.sort_by(|a, b| {
            (&a.context_name, &a.namespace, &a.pod, &a.id).cmp(&(
                &b.context_name,
                &b.namespace,
                &b.pod,
                &b.id,
            ))
        });
        Ok(result)
    }

    pub async fn kube_forward_stop(&self, session_id: String, forward_id: String) -> AppResult<()> {
        let live = self.kube_live(&session_id).await?;
        live.forwards
            .handles
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&forward_id);
        Ok(())
    }
}
