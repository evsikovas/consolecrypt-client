//! Native-internal explicit owner review and TLS tunnel. No Bridge/UI activation.
mod budget;
mod client;
mod scope;
#[path = "../agent_setup.rs"]
pub(crate) mod setup;
#[cfg(test)]
mod tests;
mod tls;
mod v2;
use crate::{AppCore, AppError, AppResult};
use budget::Budget;
use cc_agent_node_wire::*;
use scope::OriginalSessionDeviceIdentity;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tls::TunnelTls;
use tokio_util::sync::CancellationToken;
fn denied() -> AppError {
    AppError::invalid("agent_tunnel", "tunnel_unavailable")
}
/// Explicit Native user review call remains a MainRoot integration obligation.
/// This private one-use type is not, by itself, evidence a human approved it.
pub(crate) struct TunnelOwnerReview {
    scope: Arc<OriginalSessionDeviceIdentity>,
    prepared: PreparedTunnelOwner,
}
impl std::fmt::Debug for TunnelOwnerReview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TunnelOwnerReview([redacted])")
    }
}
impl TunnelOwnerReview {
    pub(crate) async fn enrollment_offer(
        app: &AppCore,
        pin: &OwnerPin,
        offer: Offer,
        cancel: &CancellationToken,
    ) -> AppResult<Self> {
        let scope = Arc::new(
            OriginalSessionDeviceIdentity::capture(app, cancel, Duration::from_secs(60)).await?,
        );
        scope.bind_owner_pin(pin)?;
        let prepared =
            PreparedTunnelOwner::offer(pin, offer, scope.now()?).map_err(|_| denied())?;
        // Actual key match happens inside the original held Native identity.
        scope
            .with_loaded(app, |id| {
                if id.public_keys().signing == *pin.key.as_bytes() {
                    Ok(())
                } else {
                    Err(denied())
                }
            })
            .await?;
        Ok(Self { scope, prepared })
    }
    pub(crate) async fn enrollment_certificate(
        app: &AppCore,
        pin: &OwnerPin,
        offer: Signed<Offer>,
        claim: Signed<Claim>,
        cert: Certificate,
        cancel: &CancellationToken,
    ) -> AppResult<Self> {
        let scope = Arc::new(
            OriginalSessionDeviceIdentity::capture(app, cancel, Duration::from_secs(60)).await?,
        );
        scope.bind_owner_pin(pin)?;
        let prepared = PreparedTunnelOwner::certificate(pin, offer, claim, cert, scope.now()?)
            .map_err(|_| denied())?;
        scope
            .with_loaded(app, |id| {
                if id.public_keys().signing == *pin.key.as_bytes() {
                    Ok(())
                } else {
                    Err(denied())
                }
            })
            .await?;
        Ok(Self { scope, prepared })
    }
    pub(crate) async fn approve_once(self, app: &AppCore) -> AppResult<TunnelOwnerSigned> {
        let Self { scope, prepared } = self;
        let result = scope
            .with_loaded(app, |id| {
                id.sign_agent_tunnel_owner_v1(prepared, scope.now()?)
                    .map_err(|_| denied())
            })
            .await?;
        scope.check(app).await?;
        scope.now()?;
        Ok(result)
    }
}
pub(crate) struct TunnelRouteReview {
    scope: Arc<OriginalSessionDeviceIdentity>,
    pin: OwnerPin,
    policy: TunnelRoutePolicy,
    prepared: PreparedTunnelOwner,
    tls: TunnelTls,
}
impl std::fmt::Debug for TunnelRouteReview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TunnelRouteReview([redacted])")
    }
}
impl TunnelRouteReview {
    pub(crate) async fn capture(
        app: &AppCore,
        pin: OwnerPin,
        certificate: Signed<Certificate>,
        policy: TunnelRoutePolicy,
        lease: Lease,
        freshness: Freshness,
        tls: TunnelTls,
        cancel: &CancellationToken,
    ) -> AppResult<Self> {
        policy.validate().map_err(|_| denied())?;
        let scope = Arc::new(
            OriginalSessionDeviceIdentity::capture(
                app,
                cancel,
                Duration::from_secs(policy.limits.max_ttl_seconds),
            )
            .await?,
        );
        scope.bind_owner_policy(&pin, &policy)?;
        let prepared = PreparedTunnelOwner::grant(
            &pin,
            certificate,
            &policy,
            lease,
            freshness,
            &policy.client_device_id,
            scope.now()?,
        )
        .map_err(|_| denied())?;
        Ok(Self {
            scope,
            pin,
            policy,
            prepared,
            tls,
        })
    }
    /// Public route preparation threads one original Native scope through TLS,
    /// owner signing and listener custody; approval never captures a replacement.
    pub(super) async fn capture_original(
        app: &AppCore,
        scope: Arc<OriginalSessionDeviceIdentity>,
        pin: OwnerPin,
        certificate: Signed<Certificate>,
        policy: TunnelRoutePolicy,
        lease: Lease,
        freshness: Freshness,
        tls: TunnelTls,
    ) -> AppResult<Self> {
        policy.validate().map_err(|_| denied())?;
        scope.check(app).await?;
        scope.bind_owner_policy(&pin, &policy)?;
        let prepared = PreparedTunnelOwner::grant(
            &pin, certificate, &policy, lease, freshness,
            &policy.client_device_id, scope.now()?,
        ).map_err(|_| denied())?;
        scope.check(app).await?;
        Ok(Self { scope, pin, policy, prepared, tls })
    }

    pub(crate) async fn approve_once(self, app: &AppCore) -> AppResult<NativeTunnelRoute> {
        let Self {
            scope,
            pin,
            policy,
            prepared,
            tls,
        } = self;
        scope.bind_owner_policy(&pin, &policy)?;
        let signed = scope
            .with_loaded(app, |id| {
                id.sign_agent_tunnel_owner_v1(prepared, scope.now()?)
                    .map_err(|_| denied())
            })
            .await?;
        let TunnelOwnerSigned::Grant(binding) = signed else {
            return Err(denied());
        };
        verify_binding(&pin, &binding, &policy, scope.now()?).map_err(|_| denied())?;
        scope.check(app).await?;
        scope.bind_owner_policy(&pin, &policy)?;
        let budget = Budget::new(policy.limits.clone(), CancellationToken::new());
        Ok(NativeTunnelRoute {
            scope,
            pin,
            policy,
            binding,
            tls,
            budget,
            binding_started: AtomicBool::new(false),
            route_identity: Arc::new(()),
        })
    }
}
pub(crate) struct NativeTunnelRoute {
    scope: Arc<OriginalSessionDeviceIdentity>,
    pin: OwnerPin,
    policy: TunnelRoutePolicy,
    binding: NodeRequestBinding,
    tls: TunnelTls,
    budget: Budget,
    binding_started: AtomicBool,
    route_identity: Arc<()>,
}
impl std::fmt::Debug for NativeTunnelRoute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeTunnelRoute([redacted])")
    }
}
impl NativeTunnelRoute {
    pub(crate) fn cancel(&self) {
        self.budget.stop();
    }
    fn io_deadline(&self) -> AppResult<tokio::time::Instant> {
        self.live()?;
        let expiry = self
            .binding
            .lease
            .body
            .expires_at
            .min(self.binding.freshness.body.expires_at)
            .min(self.binding.certificate.body.expires_at)
            .min(self.pin.expires_at);
        let remaining = self
            .scope
            .remaining_until(expiry)?
            .min(Duration::from_secs(5));
        Ok(tokio::time::Instant::from_std(self.scope.deadline())
            .min(tokio::time::Instant::from_std(
                self.budget.deadline().map_err(|_| denied())?,
            ))
            .min(tokio::time::Instant::now() + remaining))
    }
    fn live(&self) -> AppResult<()> {
        self.budget.check().map_err(|_| denied())?;
        self.scope.bind_owner_policy(&self.pin, &self.policy)?;
        verify_binding(&self.pin, &self.binding, &self.policy, self.scope.now()?)
            .map_err(|_| denied())
    }
    /// Bind exactly signed numeric loopback. Port zero is explicit ephemeral.
    pub(crate) async fn bind_loopback(&self, app: &AppCore) -> AppResult<BoundTunnelListener> {
        if self.binding_started.swap(true, Ordering::SeqCst) {
            return Err(denied());
        }
        self.scope.check(app).await?;
        self.live()?;
        let address = std::net::SocketAddr::new(
            self.policy.client_bind.ip.parse().map_err(|_| denied())?,
            self.policy.client_bind.port,
        );
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .map_err(|_| denied())?;
        let actual = listener.local_addr().map_err(|_| denied())?;
        if actual.ip() != address.ip()
            || !actual.ip().is_loopback()
            || (address.port() != 0 && actual.port() != address.port())
        {
            return Err(denied());
        }
        self.scope.check(app).await?;
        self.live()?;
        Ok(BoundTunnelListener {
            listener,
            address: actual,
            route_identity: self.route_identity.clone(),
            scope: self.scope.clone(),
            budget: self.budget.clone(),
        })
    }
}
impl Drop for NativeTunnelRoute {
    fn drop(&mut self) {
        self.budget.stop();
    }
}

/// Actual original listener ownership, not a caller-provided socket or grant.
pub(crate) struct BoundTunnelListener {
    listener: tokio::net::TcpListener,
    address: std::net::SocketAddr,
    route_identity: Arc<()>,
    scope: Arc<OriginalSessionDeviceIdentity>,
    budget: Budget,
}
pub(crate) struct AcceptedTunnelStream {
    stream: tokio::net::TcpStream,
    address: std::net::SocketAddr,
    route_identity: Arc<()>,
}
impl AcceptedTunnelStream {
    pub(crate) fn monitoring_peer(&self) -> AppResult<std::net::SocketAddr> {
        self.stream.peer_addr().map_err(|_| denied())
    }
}
impl BoundTunnelListener {
    pub(crate) fn address(&self) -> std::net::SocketAddr {
        self.address
    }
    pub(crate) async fn accept_once(&self) -> AppResult<AcceptedTunnelStream> {
        self.scope.check_current()?;
        let (stream, peer) = tokio::select! {biased;_=self.scope.cancelled()=>return Err(denied()),_=self.budget.cancelled()=>return Err(denied()),result=self.listener.accept()=>result.map_err(|_|denied())?};
        self.scope.check_current()?;
        if !peer.ip().is_loopback() || stream.local_addr().map_err(|_| denied())? != self.address {
            return Err(denied());
        }
        Ok(AcceptedTunnelStream {
            stream,
            address: self.address,
            route_identity: self.route_identity.clone(),
        })
    }
}

mod managed_tls;

mod association;
pub(crate) use association::MonitoringTunnelRoute;
pub use association::{NodeAssociationOperation, NodeClientIdentityReview, NodeRevocationReview, NodeRotationReview, NodeTunnelControlReview, AgentTunnelRouteOperation, AgentCollectorOperation, AgentExporterOperation, AgentCollectorServiceReview, AgentCollectorServiceRevocationReview};
