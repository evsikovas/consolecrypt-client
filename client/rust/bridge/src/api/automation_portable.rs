//! Original-issued portable handles. Only initial registration captures Core.
//! Password transfer is synchronous and independently cancelable before ACK.
use crate::{api::error::BridgeError, state};
use std::sync::Arc;

fn unavailable() -> BridgeError {
    BridgeError::invalid("portable_project", "portable_operation_unavailable")
}
#[flutter_rust_bridge::frb(opaque)]
pub struct PortablePasswordTransfer {
    pub(super) inner: Arc<cc_app_core::PortablePasswordTransfer>,
}
impl PortablePasswordTransfer {
    #[flutter_rust_bridge::frb(sync)]
    pub fn new(password_bytes: Vec<u8>) -> Result<Self, BridgeError> {
        // Existing single native runtime; no per-secret thread/runtime. Both
        // validation and SecretString creation run synchronously before return.
        let _runtime = state::runtime().enter();
        Ok(Self {
            inner: cc_app_core::PortablePasswordTransfer::from_bytes(password_bytes)
                .map_err(|_| unavailable())?,
        })
    }
    #[flutter_rust_bridge::frb(sync)]
    pub fn cancel(&self) {
        self.inner.cancel();
    }
}
#[flutter_rust_bridge::frb(opaque)]
pub struct PortableRequestHandle {
    inner: Arc<cc_app_core::PortableOperation>,
}
impl Drop for PortableRequestHandle {
    fn drop(&mut self) {
        self.inner.cancel();
    }
}
impl PortableRequestHandle {
    pub async fn start(&self) -> Result<String, BridgeError> {
        let operation = self.inner.clone();
        state::run(async move { operation.start().await.map_err(|_| unavailable()) }).await
    }
    #[flutter_rust_bridge::frb(sync)]
    pub fn cancel(&self) {
        self.inner.cancel();
    }
    pub async fn joined(&self) -> Result<(), BridgeError> {
        let operation = self.inner.clone();
        state::run(async move {
            operation.joined().await;
            Ok(())
        })
        .await
    }
}
pub async fn automation_portable_scope() -> Result<String, BridgeError> {
    state::with_core(|core| async move {
        core.automation_portable_scope_json()
            .await
            .map_err(|_| unavailable())
    })
    .await
}
pub async fn automation_portable_register_context(
    scope_json: String,
    project_id: String,
    project_revision: u64,
) -> Result<PortableRequestHandle, BridgeError> {
    // Captured once here. start/cancel/join above never recapture global Core.
    let core = state::core()?;
    state::run(async move {
        let operation = core
            .automation_portable_register_context_json(scope_json, project_id, project_revision)
            .await
            .map_err(|_| unavailable())?;
        Ok(PortableRequestHandle {
            inner: Arc::new(operation),
        })
    })
    .await
}
pub async fn automation_portable_register_export(
    request_json: String,
    password: &PortablePasswordTransfer,
    destination: String,
) -> Result<PortableRequestHandle, BridgeError> {
    let core = state::core()?;
    let transfer = password.inner.clone();
    state::run(async move {
        let operation = core
            .automation_portable_register_export_json(request_json, transfer, destination)
            .await
            .map_err(|_| unavailable())?;
        Ok(PortableRequestHandle {
            inner: Arc::new(operation),
        })
    })
    .await
}
pub async fn automation_portable_register_inspect(
    scope_json: String,
    source: String,
    password: &PortablePasswordTransfer,
) -> Result<PortableRequestHandle, BridgeError> {
    let core = state::core()?;
    let transfer = password.inner.clone();
    state::run(async move {
        let operation = core
            .automation_portable_register_inspect_json(scope_json, source, transfer)
            .await
            .map_err(|_| unavailable())?;
        Ok(PortableRequestHandle {
            inner: Arc::new(operation),
        })
    })
    .await
}
#[cfg(test)]
mod tests;
