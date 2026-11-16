//! One original local channel, one maintained TLS stream, no reconnect/retry.
use super::*;
use cc_sync_core::api::RelayBinaryStream;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_rustls::client::TlsStream;
use zeroize::Zeroizing;

pub(super) async fn read_frame<S: AsyncRead + Unpin>(stream: &mut S) -> AppResult<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        bytes.push(stream.read_u8().await.map_err(|_| denied())?);
        if bytes.len() > 16384 {
            return Err(denied());
        }
        if bytes.last() == Some(&b'\n') {
            return Ok(bytes);
        }
    }
}
#[derive(Default)]
pub(super) struct Totals {
    pub(super) sent: u64,
    pub(super) received: u64,
}

/// Called only after actual cryptographic V2 ACK on this SAME stream. EOF is
/// always unknown and performs no TLS shutdown, even if payload looks like JSON.
pub(super) async fn copy_data<S: AsyncRead + AsyncWrite + Unpin>(
    tls: &mut TlsStream<S>,
    local: &mut tokio::net::TcpStream,
    channel: &Channel,
    io: &OriginalIo<'_>,
    totals: &mut Totals,
) -> AppResult<()> {
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.tick().await;
    loop {
        io.check()?;
        let frame = channel.reserve().map_err(|_| denied())?;
        let mut data = Zeroizing::new(vec![0u8; frame.capacity()]);
        enum Ready {
            Local,
            Tls(usize),
            Tick,
        }
        let ready = tokio::select! {biased;_=io.cancelled()=>return Err(denied()),
        _=tick.tick()=>Ready::Tick,
        r=local.readable()=>{r.map_err(|_|denied())?;Ready::Local},
        r=io.read_live(tls,data.as_mut_slice())=>Ready::Tls(r?)};
        match ready {
            Ready::Tick => {}
            Ready::Local => {
                let n = match local.try_read(data.as_mut_slice()) {
                    Ok(n) => n,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                    Err(_) => return Err(denied()),
                };
                if n == 0 {
                    return Err(denied());
                }
                frame.charge(n).map_err(|_| denied())?;
                io.check()?;
                io.run(async { tls.write_all(&data[..n]).await.map_err(|_| denied()) })
                    .await?;
                io.run(async { tls.flush().await.map_err(|_| denied()) })
                    .await?;
                totals.sent = totals.sent.checked_add(n as u64).ok_or_else(denied)?;
            }
            Ready::Tls(n) => {
                if n == 0 {
                    return Err(denied());
                }
                frame.charge(n).map_err(|_| denied())?;
                io.check()?;
                io.run(async { local.write_all(&data[..n]).await.map_err(|_| denied()) })
                    .await?;
                totals.received = totals.received.checked_add(n as u64).ok_or_else(denied)?;
            }
        }
        // Zeroize payload before releasing its original queue reservation.
    }
}

fn original_app(core: &std::sync::Weak<crate::app::Inner>) -> AppResult<AppCore> {
    Ok(AppCore { inner: core.upgrade().ok_or_else(denied)? })
}

impl NativeTunnelRouteV2 {
    pub(crate) async fn serve_channel_once(
        &self,
        app: &AppCore,
        accepted: AcceptedTunnelStream,
        carrier: RelayBinaryStream,
    ) -> OriginalChannelOutcome {
        self.serve_channel_once_inner(Arc::downgrade(&app.inner), accepted, carrier, None).await
    }
    pub(crate) async fn serve_channel_once_audited(
        &self, app: &AppCore, accepted: AcceptedTunnelStream, carrier: RelayBinaryStream,
        audit: &super::super::association::access_audit::RouteAudit,
    ) -> OriginalChannelOutcome {
        if audit.require_binding(&self.scope, &self.binding, &self.policy).is_err() {
            return OriginalChannelOutcome::Unknown;
        }
        self.serve_channel_once_inner(Arc::downgrade(&app.inner), accepted, carrier, Some(audit)).await
    }
    pub(crate) async fn serve_channel_once_audited_weak(
        &self, core: std::sync::Weak<crate::app::Inner>, accepted: AcceptedTunnelStream,
        carrier: RelayBinaryStream, audit: &super::super::association::access_audit::RouteAudit,
    ) -> OriginalChannelOutcome {
        if audit.require_binding(&self.scope, &self.binding, &self.policy).is_err() {
            return OriginalChannelOutcome::Unknown;
        }
        self.serve_channel_once_inner(core, accepted, carrier, Some(audit)).await
    }
    async fn serve_channel_once_inner(
        &self, core: std::sync::Weak<crate::app::Inner>, accepted: AcceptedTunnelStream, carrier: RelayBinaryStream,
        audit: Option<&super::super::association::access_audit::RouteAudit>,
    ) -> OriginalChannelOutcome {
        if !Arc::ptr_eq(&self.route_identity, &accepted.route_identity)
            || self.io_started.swap(true, Ordering::SeqCst)
        {
            return OriginalChannelOutcome::Unknown;
        }
        let mut local = accepted.stream;
        let action = async {
            self.scope.check(&original_app(&core)?).await?;
            self.live()?;
            let actual = local.local_addr().map_err(|_| denied())?;
            let expected: std::net::IpAddr = self
                .policy
                .route
                .client_bind
                .ip
                .parse()
                .map_err(|_| denied())?;
            if actual != accepted.address
                || actual.ip() != expected
                || !actual.ip().is_loopback()
                || (self.policy.route.client_bind.port != 0
                    && actual.port() != self.policy.route.client_bind.port)
            {
                return Err(denied());
            }
            self.scope.bind_relay(&carrier)?;
            let io = self.io();
            let handshake = async {
                let mut tls = io
                    .run(async {
                        self.tls
                            .connect(carrier, &self.policy.route)
                            .await
                            .map_err(|_| denied())
                    })
                    .await?;
                self.live()?;
                // Fresh explicit V2 admission only. No automatic resume or
                // second attempt exists until a trusted drain bridge is wired.
                let open =
                    open_original_frame(&self.binding, &self.policy, OriginalTunnelMode::Admit)
                        .map_err(|_| denied())?;
                io.run(async { tls.write_all(&open).await.map_err(|_| denied()) })
                    .await?;
                io.run(async { tls.flush().await.map_err(|_| denied()) })
                    .await?;
                let challenge = io.run(read_frame(&mut tls)).await?;
                let mut pending = verify_original_challenge(
                    &challenge,
                    &self.pin,
                    &self.binding,
                    &self.policy,
                    self.scope.now()?,
                )
                .map_err(|_| denied())?;
                self.scope.check(&original_app(&core)?).await?;
                self.live()?;
                let echo = pending.echo_once().map_err(|_| denied())?;
                io.run(async { tls.write_all(&echo).await.map_err(|_| denied()) })
                    .await?;
                io.run(async { tls.flush().await.map_err(|_| denied()) })
                    .await?;
                let ack = io.run(read_frame(&mut tls)).await?;
                verify_original_ack(
                    &ack,
                    pending,
                    &self.pin,
                    &self.binding,
                    &self.policy,
                    self.scope.now()?,
                )
                .map_err(|_| denied())?;
                self.scope.check(&original_app(&core)?).await?;
                self.live()?;
                if let Some(audit) = audit {
                    // Genuine SAME-stream Node ACK has verified. Record actual
                    // access before the first application byte can be copied.
                    audit.observe_opened().await?;
                    self.scope.check(&original_app(&core)?).await?;
                    self.live()?;
                }
                Ok::<_, AppError>(tls)
            };
            // Stop during incomplete admission is an abort, never close_notify.
            let mut tls = tokio::select! {biased;_=io.cancelled()=>return Err(denied()),
            _=self.stop.requested()=>return Err(denied()),r=handshake=>r?};
            let mut totals = Totals::default();
            let explicit_stop = {
                let copy = copy_data(&mut tls, &mut local, &self.channel, &io, &mut totals);
                tokio::select! {biased;_=io.cancelled()=>false,
                // Observed EOF/error wins over a concurrent Stop request.
                _=copy=>false,_=self.stop.requested()=>true}
            };
            if !explicit_stop {
                return Err(denied());
            }
            self.live()?;
            if !self.stop.send_request_original(&mut tls, &io).await {
                return Err(denied());
            }
            // This only observes our explicit authenticated stop REQUEST.
            // No public Agent join/CAS acknowledgement exists at this layer.
            Ok(OriginalChannelOutcome::StopRequested {
                sent: totals.sent,
                received: totals.received,
            })
        };
        let result = tokio::select! {biased;_=self.scope.cancelled()=>Err(denied()),
        _=self.budget.cancelled()=>Err(denied()),r=action=>r};
        match result {
            Ok(outcome) => outcome,
            Err(_) => {
                self.budget.stop();
                // Dropping TLS/raw Relay/local stream performs NO shutdown.
                OriginalChannelOutcome::Unknown
            }
        }
    }
}
