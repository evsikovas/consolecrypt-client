//! Private explicit V2 owner path. No saved consent, Bridge/UI or resume grant.
use super::*;
mod client;
mod resume;
pub(crate) use resume::OriginalGenerationOutcome;
mod stop;
#[cfg(test)]
mod tests;
use budget::Channel;
use stop::{OriginalIo, OriginalStop};

pub(crate) struct TunnelRouteReviewV2 {
    scope: Arc<OriginalSessionDeviceIdentity>,
    pin: OwnerPin,
    policy: TunnelRoutePolicyV2,
    prepared: PreparedTunnelOwner,
    tls: TunnelTls,
}
impl std::fmt::Debug for TunnelRouteReviewV2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TunnelRouteReviewV2([redacted])")
    }
}
impl TunnelRouteReviewV2 {
    pub(crate) async fn capture(
        app: &AppCore,
        pin: OwnerPin,
        certificate: Signed<Certificate>,
        policy: TunnelRoutePolicyV2,
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
                Duration::from_secs(policy.route.limits.max_ttl_seconds),
            )
            .await?,
        );
        scope.bind_owner_policy(&pin, &policy.route)?;
        let prepared = PreparedTunnelOwner::grant_original(
            &pin,
            certificate,
            &policy,
            lease,
            freshness,
            &policy.route.client_device_id,
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
        policy: TunnelRoutePolicyV2,
        lease: Lease,
        freshness: Freshness,
        tls: TunnelTls,
    ) -> AppResult<Self> {
        policy.validate().map_err(|_| denied())?;
        scope.check(app).await?;
        scope.bind_owner_policy(&pin, &policy.route)?;
        let prepared = PreparedTunnelOwner::grant_original(
            &pin, certificate, &policy, lease, freshness,
            &policy.route.client_device_id, scope.now()?,
        ).map_err(|_| denied())?;
        scope.check(app).await?;
        Ok(Self { scope, pin, policy, prepared, tls })
    }

    pub(crate) async fn approve_once(self, app: &AppCore) -> AppResult<NativeTunnelRouteV2> {
        let Self {
            scope,
            pin,
            policy,
            prepared,
            tls,
        } = self;
        scope.bind_owner_policy(&pin, &policy.route)?;
        let signed = scope
            .with_loaded(app, |id| {
                id.sign_agent_tunnel_owner_v1(prepared, scope.now()?)
                    .map_err(|_| denied())
            })
            .await?;
        let TunnelOwnerSigned::Grant(binding) = signed else {
            return Err(denied());
        };
        verify_binding_v2(&pin, &binding, &policy, scope.now()?).map_err(|_| denied())?;
        scope.check(app).await?;
        scope.bind_owner_policy(&pin, &policy.route)?;
        let budget = Budget::new(policy.route.limits.clone(), CancellationToken::new());
        // ONE original logical channel. Transport completion cannot replace it
        // or reset its cumulative debit, original TTL or idle origin.
        let channel = budget.open().map_err(|_| denied())?;
        let stop = OriginalStop::new(scope.clone());
        Ok(NativeTunnelRouteV2 {
            scope,
            pin,
            policy,
            binding,
            tls,
            budget,
            channel,
            stop,
            listener_started: AtomicBool::new(false),
            io_started: AtomicBool::new(false),
            route_identity: Arc::new(()),
        })
    }
}
pub(crate) struct NativeTunnelRouteV2 {
    scope: Arc<OriginalSessionDeviceIdentity>,
    pin: OwnerPin,
    policy: TunnelRoutePolicyV2,
    binding: NodeRequestBinding,
    tls: TunnelTls,
    budget: Budget,
    channel: Channel,
    stop: OriginalStop,
    listener_started: AtomicBool,
    io_started: AtomicBool,
    route_identity: Arc<()>,
}
impl std::fmt::Debug for NativeTunnelRouteV2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeTunnelRouteV2([redacted])")
    }
}
impl NativeTunnelRouteV2 {
    pub(crate) fn monitoring_target(&self) -> AppResult<(ResourceId, std::net::SocketAddr)> {
        self.live()?;
        if self.stop.is_requested() || !self.policy.resume_original {return Err(denied())}
        let target = &self.policy.route.destination;
        Ok((self.policy.route.route_id.clone(), std::net::SocketAddr::new(
            target.ip.parse().map_err(|_| denied())?, target.port)))
    }
    /// Existing signed binding for the fixed trusted producer, never Bridge data.
    pub(crate) fn original_binding(&self) -> AppResult<&NodeRequestBinding> {
        self.live()?;
        Ok(&self.binding)
    }

    fn live(&self) -> AppResult<()> {
        self.budget.check().map_err(|_| denied())?;
        self.scope
            .bind_owner_policy(&self.pin, &self.policy.route)?;
        verify_binding_v2(&self.pin, &self.binding, &self.policy, self.scope.now()?)
            .map_err(|_| denied())
    }
    fn io(&self) -> OriginalIo<'_> {
        OriginalIo::new(
            &self.scope,
            &self.budget,
            self.binding
                .lease
                .body
                .expires_at
                .min(self.binding.freshness.body.expires_at)
                .min(self.binding.certificate.body.expires_at)
                .min(self.pin.expires_at),
        )
    }
    /// Cancellation aborts the carrier. It is never explicit owner Stop.
    pub(crate) fn cancel(&self) {
        self.budget.stop();
    }
    /// Same captured owner/auth/unlock, never a global/current route lookup.
    pub(crate) async fn request_stop(&self, app: &AppCore) -> AppResult<()> {
        self.scope
            .with_loaded(app, |_| {
                self.live()?;
                self.stop.request_held()
            })
            .await
    }
    pub(crate) async fn bind_loopback(&self, app: &AppCore) -> AppResult<BoundTunnelListener> {
        self.scope.check(app).await?;
        let listener = self.bind_loopback_original().await?;
        self.scope.check(app).await?;
        Ok(listener)
    }
    /// Original guarded numeric bind; no Core or upgraded Session across Pending.
    pub(crate) async fn bind_loopback_original(&self) -> AppResult<BoundTunnelListener> {
        if self.listener_started.swap(true, Ordering::SeqCst) {
            return Err(denied());
        }
        self.scope.check_current()?;
        self.live()?;
        let address = std::net::SocketAddr::new(
            self.policy
                .route
                .client_bind
                .ip
                .parse()
                .map_err(|_| denied())?,
            self.policy.route.client_bind.port,
        );
        let listener = self.io().run(async {
            tokio::net::TcpListener::bind(address).await.map_err(|_| denied())
        }).await?;
        let actual = listener.local_addr().map_err(|_| denied())?;
        if actual.ip() != address.ip()
            || !actual.ip().is_loopback()
            || (address.port() != 0 && actual.port() != address.port())
        {
            return Err(denied());
        }
        self.scope.check_current()?;
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
impl Drop for NativeTunnelRouteV2 {
    fn drop(&mut self) {
        self.budget.stop();
    }
}

/// No Stopped/drained outcome: the Agent's actual join/CAS remains private.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OriginalChannelOutcome {
    StopRequested { sent: u64, received: u64 },
    Unknown,
}
