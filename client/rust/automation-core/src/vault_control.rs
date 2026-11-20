//! Validation-only secret control seam. No target/job/general resolver.
use crate::{
    AutomationError, HelperPeer, Result, VaultValidationPasswordLease,
    VaultValidationPasswordRequest,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use std::{
    future::Future,
    io::Write,
    pin::Pin,
    sync::{Arc, Weak},
    time::Duration,
};
use uuid::Uuid;
use zeroize::Zeroizing;
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct VaultValidationControlRequest {
    pub version: u8,
    #[serde(rename = "type")]
    pub kind: String,
    pub request_id: Uuid,
    pub body: VaultValidationPasswordRequest,
}
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct VaultJobControlRequest {
    pub version: u8,
    #[serde(rename = "type")]
    pub kind: String,
    pub request_id: Uuid,
    pub body: crate::VaultPasswordRequest,
}
pub type VaultValidationControlFuture = Pin<Box<dyn Future<Output = ()> + Send>>;
pub trait VaultValidationHandler: Send + Sync + 'static {
    /// Must await retained IO/cleanup, not detach material work. Final original
    /// context/unlock guards must cover responder.password().await.
    fn handle(
        &self,
        request: VaultValidationControlRequest,
        responder: VaultValidationResponder,
    ) -> VaultValidationControlFuture;
    fn revoke(&self);
    /// Existing validation-only handlers keep denying job material. Only the
    /// dedicated owner after actual signed-job admission overrides this path.
    fn handle_job(
        &self,
        _request: VaultJobControlRequest,
        mut responder: VaultJobResponder,
    ) -> VaultValidationControlFuture {
        Box::pin(async move {
            let _ = responder.denied().await;
        })
    }
}
pub struct VaultJobResponder {
    inner: VaultValidationResponder,
}
impl std::fmt::Debug for VaultJobResponder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VaultJobResponder([PRIVATE])")
    }
}
impl VaultJobResponder {
    pub(crate) fn new(
        peer: &Arc<HelperPeer>,
        identity: Uuid,
        deadline: std::time::Instant,
    ) -> Self {
        Self {
            inner: VaultValidationResponder::new(peer, identity, deadline),
        }
    }
    pub fn ensure_live(&self) -> Result<()> {
        self.inner.ensure_live()
    }
    pub fn bound_runtime(
        &self,
        runtime: crate::VaultValidationRuntime,
    ) -> Result<crate::VaultValidationRuntime> {
        self.inner.bound_runtime(runtime)
    }
    pub async fn password(
        &mut self,
        lease: &mut crate::AdmittedVaultPasswordLease,
        request: &crate::VaultPasswordRequest,
    ) -> Result<()> {
        self.inner.ensure_live()?;
        if std::mem::replace(&mut self.inner.used, true) {
            return Err(AutomationError::Closed);
        }
        let peer = self.inner.peer.upgrade().ok_or(AutomationError::Closed)?;
        let mut fence = lease.send_fence();
        fence.deadline = fence.deadline.min(self.inner.deadline);
        let mut raw = Zeroizing::new(Vec::with_capacity(4097));
        if let Err(error) = lease.write_once(request, &mut *raw) {
            peer.abort_dependency_exchange();
            return Err(error);
        }
        let encoded = Zeroizing::new(STANDARD.encode(&*raw));
        let mut frame = Zeroizing::new(Vec::with_capacity(8192));
        write!(&mut *frame,"{{\"version\":1,\"type\":\"vault_job_response\",\"request_id\":\"{}\",\"result\":{{\"password_base64\":\"{}\"}},\"error\":null}}\n",self.inner.identity,encoded.as_str())?;
        peer.write_vault_frame(frame, Some(fence)).await
    }
    pub async fn denied(&mut self) -> Result<()> {
        if std::mem::replace(&mut self.inner.used, true) {
            return Err(AutomationError::Closed);
        }
        let peer = self.inner.peer.upgrade().ok_or(AutomationError::Closed)?;
        let frame=Zeroizing::new(format!("{{\"version\":1,\"type\":\"vault_job_response\",\"request_id\":\"{}\",\"result\":null,\"error\":{{\"code\":\"vault_unavailable\",\"message\":\"Scoped Vault input unavailable\"}}}}\n",self.inner.identity).into_bytes());
        peer.write_vault_frame(frame, None).await
    }
}
/// No generic Value or secret getter; one fixed private response only.
pub struct VaultValidationResponder {
    peer: Weak<HelperPeer>,
    identity: Uuid,
    used: bool,
    deadline: std::time::Instant,
}
impl std::fmt::Debug for VaultValidationResponder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultValidationResponder")
            .finish_non_exhaustive()
    }
}
impl VaultValidationResponder {
    pub(crate) fn new(
        peer: &Arc<HelperPeer>,
        identity: Uuid,
        deadline: std::time::Instant,
    ) -> Self {
        Self {
            peer: Arc::downgrade(peer),
            identity,
            used: false,
            deadline,
        }
    }
    pub fn ensure_live(&self) -> Result<()> {
        if self.used
            || std::time::Instant::now() >= self.deadline
            || self.peer.upgrade().is_none_or(|peer| peer.is_closed())
        {
            return Err(AutomationError::Closed);
        }
        Ok(())
    }
    pub fn bound_runtime(
        &self,
        mut runtime: crate::VaultValidationRuntime,
    ) -> Result<crate::VaultValidationRuntime> {
        self.ensure_live()?;
        runtime.limit_to(self.deadline);
        runtime.require_fresh()?;
        Ok(runtime)
    }
    pub async fn password(
        &mut self,
        lease: &mut VaultValidationPasswordLease,
        request: &VaultValidationPasswordRequest,
    ) -> Result<()> {
        self.ensure_live()?;
        if std::mem::replace(&mut self.used, true) {
            return Err(AutomationError::Closed);
        }
        let peer = self.peer.upgrade().ok_or(AutomationError::Closed)?;
        if peer.is_closed() {
            return Err(AutomationError::Closed);
        }
        let mut fence = lease.send_fence();
        fence.deadline = fence.deadline.min(self.deadline);
        let mut raw = Zeroizing::new(Vec::with_capacity(4097));
        if let Err(error) = lease.write_once(request, &mut *raw) {
            peer.abort_dependency_exchange();
            return Err(error);
        }
        let encoded = Zeroizing::new(STANDARD.encode(&*raw));
        let mut frame = Zeroizing::new(Vec::with_capacity(8192));
        write!(&mut*frame,"{{\"version\":1,\"type\":\"vault_validation_response\",\"request_id\":\"{}\",\"result\":{{\"password_base64\":\"{}\"}},\"error\":null}}\n",self.identity,encoded.as_str())?;
        peer.write_vault_frame(frame, Some(fence)).await
    }
    pub async fn denied(&mut self) -> Result<()> {
        if std::mem::replace(&mut self.used, true) {
            return Err(AutomationError::Closed);
        }
        let peer = self.peer.upgrade().ok_or(AutomationError::Closed)?;
        let frame=Zeroizing::new(format!("{{\"version\":1,\"type\":\"vault_validation_response\",\"request_id\":\"{}\",\"result\":null,\"error\":{{\"code\":\"vault_unavailable\",\"message\":\"Scoped Vault input unavailable\"}}}}\n",self.identity).into_bytes());
        peer.write_vault_frame(frame, None).await
    }
}
impl Drop for VaultValidationResponder {
    fn drop(&mut self) {
        if !self.used {
            if let Some(peer) = self.peer.upgrade() {
                peer.abort_dependency_exchange();
            }
        }
    }
}
pub(crate) struct PrivateWriteGuard {
    pub peer: Arc<HelperPeer>,
    pub completed: bool,
}
impl Drop for PrivateWriteGuard {
    fn drop(&mut self) {
        if !self.completed {
            self.peer.abort_dependency_exchange();
        }
    }
}
impl HelperPeer {
    pub(crate) async fn write_vault_frame(
        self: &Arc<Self>,
        frame: Zeroizing<Vec<u8>>,
        fence: Option<crate::vault_validation::VaultWriteFence>,
    ) -> Result<()> {
        let mut guard = PrivateWriteGuard {
            peer: self.clone(),
            completed: false,
        };
        if frame.len() > 8192 || self.is_closed() || fence.as_ref().is_some_and(|f| f.denied()) {
            return Err(AutomationError::Closed);
        }
        let revoked = async {
            match &fence {
                Some(f) => {
                    tokio::select! {_=f.cancelled()=>(),_=tokio::time::sleep_until(tokio::time::Instant::from_std(f.deadline))=>()}
                }
                None => std::future::pending::<()>().await,
            }
        };
        tokio::pin!(revoked);
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            let mut writer = tokio::select! {biased;
                _=self.closed.cancelled()=>return Err(AutomationError::Closed),
                _=&mut revoked=>return Err(AutomationError::Closed),
                writer=self.writer.lock()=>writer,
            };
            let writer = writer.as_mut().ok_or(AutomationError::Closed)?;
            use tokio::io::AsyncWriteExt;
            let mut offset = 0;
            while offset < frame.len() {
                // Admission checks before first byte and each bounded chunk.
                // A revocation after delivery cannot retract past bytes; any
                // observed partial/revoked frame fences this dedicated stream.
                if self.is_closed() || fence.as_ref().is_some_and(|f| f.denied()) {
                    return Err(AutomationError::Closed);
                }
                let end = (offset + 1024).min(frame.len());
                let written = tokio::select! {biased;
                    _=self.closed.cancelled()=>return Err(AutomationError::Closed),
                    _=&mut revoked=>return Err(AutomationError::Closed),
                    result=writer.write(&frame[offset..end])=>result?,
                };
                if written == 0 {
                    return Err(AutomationError::Closed);
                }
                offset += written;
            }
            tokio::select! {biased;
                _=self.closed.cancelled()=>return Err(AutomationError::Closed),
                _=&mut revoked=>return Err(AutomationError::Closed),
                result=writer.flush()=>result?,
            }
            if fence.as_ref().is_some_and(|f| f.denied()) {
                return Err(AutomationError::Closed);
            }
            Ok(())
        })
        .await
        .map_err(|_| AutomationError::Timeout)?;
        result?;
        guard.completed = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
