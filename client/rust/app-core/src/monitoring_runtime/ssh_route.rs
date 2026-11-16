//! One saved-source read owns one dedicated Native planned SSH/jump session.
//! No loopback listener, cached route, generic proxy or endpoint rewrite.
use super::*;
use crate::session::{Session, Unlocked};
use crate::working_set::Entry;
use cc_models::{ObjectId, ObjectKind};
use cc_monitoring_core::{MonitoringConnect, MonitoringIo, MonitoringSourceConnector, Route};
use std::sync::{
    Weak,
    atomic::{AtomicBool, Ordering},
};

pub(super) struct SshSource {
    source: SourceConfig,
    ssh: Arc<cc_ssh_core::SshSession>,
    owner: Weak<Session>,
    app: Weak<crate::app::Inner>,
    unlock: Weak<Unlocked>,
    generation: u64,
    scope: String,
    mutation: u64,
    cancel: CancellationToken,
    closed: AtomicBool,
}
impl std::fmt::Debug for SshSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MonitoringSshSource([scoped])")
    }
}
fn transport_error() -> cc_monitoring_core::Error {
    cc_monitoring_core::Error {
        code: cc_monitoring_core::ErrorCode::Transport,
        message: "Original monitoring SSH route unavailable".into(),
        http_status: None,
    }
}
impl SshSource {
    pub(super) async fn open(
        app: &AppCore,
        owner: &Arc<Session>,
        unlock: &Arc<Unlocked>,
        source: &SourceConfig,
        snapshot: &BTreeMap<ObjectId, Entry>,
        mutation: u64,
        cancel: &CancellationToken,
    ) -> AppResult<Option<Arc<Self>>> {
        let host_id = match &source.route {
            Route::Direct => return Ok(None),
            Route::SshHost { host_id } => crate::dto::parse_id("monitoring_ssh_host", host_id)?,
            // Legacy tunnel identifiers are not inventory Host identifiers.
            Route::SshTunnel { .. } | Route::AgentTunnel { .. } | Route::AgentDestination { .. } => {
                return Err(invalid("selected route is not available for monitoring"));
            }
        };
        source
            .validate()
            .map_err(|_| invalid("invalid monitoring source"))?;
        if snapshot.len() > 16_384
            || snapshot
                .get(&host_id)
                .is_none_or(|entry| entry.kind() != ObjectKind::Host)
        {
            return Err(invalid("saved monitoring SSH host unavailable"));
        }
        let mut revisions = crate::kubernetes_runtime::dependencies(snapshot, host_id)?;
        // The existing Native host-key store may match exact, hashed or CA
        // records. Include its bounded original metadata; never infer trust.
        for (id, entry) in snapshot {
            if entry.kind() == ObjectKind::KnownHost {
                revisions.insert(*id, entry.revision);
            }
            if revisions.len() > 512 {
                return Err(invalid("monitoring SSH route budget"));
            }
        }
        let route_cancel = cancel.child_token();
        if unlock.working().mutation_stamp() != mutation
            || !unlock
                .working()
                .watch_revisions(revisions, route_cancel.clone())
        {
            return Err(invalid("monitoring SSH route changed"));
        }
        let generation = owner.automation.unlocked_generation()?;
        let scope = unlock.notices.scope_id().ok_or(AppError::VaultLocked)?;
        let weak = Arc::downgrade(unlock);
        // Existing Native planner/credential resolver/host-key verification,
        // with its original Weak unlock and all direct/jump/proxy references.
        // It clears forwarding and agent forwarding on this private session.
        let deadline = Instant::now() + Duration::from_secs(30);
        let ssh = app
            .open_private_runner_session_fenced(
                &host_id.to_string(),
                &weak,
                &route_cancel,
                Some(deadline),
                || {
                    let original = weak.upgrade().ok_or(AppError::VaultLocked)?;
                    if route_cancel.is_cancelled()
                        || *original.sharing_shutdown.borrow()
                        || original.working().mutation_stamp() != mutation
                        || original.notices.scope_id().as_deref() != Some(scope.as_str())
                        || owner.automation.unlocked_generation()? != generation
                        || Instant::now() >= deadline
                    {
                        return Err(invalid("monitoring SSH route changed before dial"));
                    }
                    Ok(())
                },
            )
            .await?;
        let result = Arc::new(Self {
            source: source.clone(),
            ssh: Arc::new(ssh),
            owner: Arc::downgrade(owner),
            app: Arc::downgrade(&app.inner),
            unlock: weak,
            generation,
            scope,
            mutation,
            cancel: route_cancel,
            closed: AtomicBool::new(false),
        });
        if result.ssh.host_id() != host_id || result.check().is_err() {
            result.close().await;
            return Err(invalid("monitoring SSH route changed"));
        }
        Ok(Some(result))
    }
    fn unlock_current(&self) -> bool {
        !self.cancel.is_cancelled()
            && !self.closed.load(Ordering::Acquire)
            && !self.ssh.is_closed()
            && self.unlock.upgrade().is_some_and(|u| {
                !*u.sharing_shutdown.borrow() && u.working().mutation_stamp() == self.mutation
            })
    }
    pub(super) fn check(&self) -> AppResult<()> {
        let owner = self.owner.upgrade().ok_or(AppError::VaultLocked)?;
        let u = self.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        if !self.unlock_current()
            || owner.automation.unlocked_generation()? != self.generation
            || u.notices.scope_id().as_deref() != Some(self.scope.as_str())
        {
            return Err(invalid("monitoring SSH route changed"));
        }
        Ok(())
    }
    pub(super) async fn close(&self) {
        self.cancel.cancel();
        if !self.closed.swap(true, Ordering::AcqRel) {
            let _ = tokio::time::timeout(Duration::from_secs(2), self.ssh.disconnect()).await;
        }
    }
}
impl MonitoringSourceConnector for SshSource {
    fn source(&self) -> &SourceConfig {
        &self.source
    }
    fn is_current(&self) -> bool {
        if !self.unlock_current() {
            return false;
        }
        let Some(app) = self.app.upgrade() else {
            return false;
        };
        let Some(owner) = self.owner.upgrade() else {
            return false;
        };
        // Try only: never wait/reenter a global guard from IO. A concurrently
        // held transition/commit conservatively retires this bounded GET.
        let Ok(active) = app.active.try_lock() else {
            return false;
        };
        active
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &owner))
    }
    fn connect(&self) -> MonitoringConnect<'_> {
        Box::pin(async move {
            self.check().map_err(|_| transport_error())?;
            let url = self.source.validate()?;
            let host = match url.host().ok_or_else(transport_error)? {
                url::Host::Ipv6(ip) => ip.to_string(),
                host => host.to_string(),
            };
            let port = url
                .port_or_known_default()
                .filter(|port| *port != 0)
                .ok_or_else(transport_error)?;
            let stream = self
                .ssh
                .open_direct_tcpip(&host, port, None)
                .await
                .map_err(|_| transport_error())?;
            // No current-profile selection or credential lookup follows this
            // await. Actual IO also carries the cheap original-scope fence.
            self.check().map_err(|_| transport_error())?;
            Ok(Box::new(stream) as MonitoringIo)
        })
    }
}
impl Drop for SshSource {
    fn drop(&mut self) {
        self.cancel.cancel();
        if !self.closed.swap(true, Ordering::AcqRel) {
            let ssh = self.ssh.clone();
            // Caller-abort fallback only. Scope is revoked synchronously;
            // ordinary success/cancel/timeout explicitly await close above.
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move {
                    let _ = tokio::time::timeout(Duration::from_secs(2), ssh.disconnect()).await;
                });
            }
        }
    }
}

#[cfg(test)]
mod live;

#[cfg(test)]
mod fence_tests;
