//! One actual owned local stream -> maintained mTLS -> Relay carrier.
use super::*;
use cc_sync_core::api::RelayBinaryStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{timeout, timeout_at};
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug)]
pub(crate) enum TunnelChannelError {
    Rejected,
    OutcomeUnknown,
}
impl NativeTunnelRoute {
    /// Relay slot creation/capability provision is explicit and separate. The
    /// caller supplies an actual once-attached carrier, never a bool grant.
    pub(crate) async fn serve_channel_once(
        &self,
        app: &AppCore,
        accepted: AcceptedTunnelStream,
        carrier: RelayBinaryStream,
    ) -> std::result::Result<(u64, u64), TunnelChannelError> {
        if !Arc::ptr_eq(&self.route_identity, &accepted.route_identity) {
            return Err(TunnelChannelError::Rejected);
        }
        let actual_listener = accepted.address;
        let mut local = accepted.stream;
        let channel = self
            .budget
            .open()
            .map_err(|_| TunnelChannelError::Rejected)?;
        let action = async {
            self.scope
                .check(app)
                .await
                .map_err(|_| TunnelChannelError::Rejected)?;
            self.live().map_err(|_| TunnelChannelError::Rejected)?;
            // The local endpoint is accepted only from the signed listener.
            let actual = local
                .local_addr()
                .map_err(|_| TunnelChannelError::Rejected)?;
            let expected: std::net::IpAddr = self
                .policy
                .client_bind
                .ip
                .parse()
                .map_err(|_| TunnelChannelError::Rejected)?;
            if actual != actual_listener
                || actual.ip() != expected
                || !actual.ip().is_loopback()
                || (self.policy.client_bind.port != 0
                    && actual.port() != self.policy.client_bind.port)
            {
                return Err(TunnelChannelError::Rejected);
            }
            self.scope
                .bind_relay(&carrier)
                .map_err(|_| TunnelChannelError::Rejected)?;
            let limit = self
                .io_deadline()
                .map_err(|_| TunnelChannelError::OutcomeUnknown)?;
            let mut tls = timeout_at(limit, self.tls.connect(carrier, &self.policy))
                .await
                .map_err(|_| TunnelChannelError::OutcomeUnknown)?
                .map_err(|_| TunnelChannelError::OutcomeUnknown)?;
            self.live()
                .map_err(|_| TunnelChannelError::OutcomeUnknown)?;
            let open = open_frame(&self.binding, &self.policy)
                .map_err(|_| TunnelChannelError::Rejected)?;
            timeout_at(limit, tls.write_all(&open))
                .await
                .map_err(|_| TunnelChannelError::OutcomeUnknown)?
                .map_err(|_| TunnelChannelError::OutcomeUnknown)?;
            timeout_at(limit, tls.flush())
                .await
                .map_err(|_| TunnelChannelError::OutcomeUnknown)?
                .map_err(|_| TunnelChannelError::OutcomeUnknown)?;
            let ack = async {
                let mut bytes = Vec::new();
                loop {
                    let b = tls
                        .read_u8()
                        .await
                        .map_err(|_| TunnelChannelError::OutcomeUnknown)?;
                    bytes.push(b);
                    if bytes.len() > 4096 {
                        return Err(TunnelChannelError::OutcomeUnknown);
                    }
                    if b == b'\n' {
                        break;
                    }
                }
                Ok(bytes)
            };
            let bytes = timeout_at(limit, ack)
                .await
                .map_err(|_| TunnelChannelError::OutcomeUnknown)??;
            verify_ack(
                &bytes,
                &self.pin,
                &self.binding,
                &self.policy,
                self.scope
                    .now()
                    .map_err(|_| TunnelChannelError::OutcomeUnknown)?,
            )
            .map_err(|_| TunnelChannelError::OutcomeUnknown)?;
            self.scope
                .check(app)
                .await
                .map_err(|_| TunnelChannelError::OutcomeUnknown)?;
            self.live()
                .map_err(|_| TunnelChannelError::OutcomeUnknown)?;
            let mut sent = 0u64;
            let mut received = 0u64;
            let mut tick = tokio::time::interval(Duration::from_millis(100));
            tick.tick().await;
            loop {
                self.live()
                    .map_err(|_| TunnelChannelError::OutcomeUnknown)?;
                // A single reserved plaintext allocation. Maintained TLS/WS
                // retain separately bounded records; no duplicate frame queue.
                let frame = channel
                    .reserve()
                    .map_err(|_| TunnelChannelError::OutcomeUnknown)?;
                let mut data = Zeroizing::new(vec![0u8; frame.capacity()]);
                enum Ready {
                    Local,
                    Tls(usize),
                    Tick,
                }
                let ready = tokio::select! {biased;
                    _=self.scope.cancelled()=>return Err(TunnelChannelError::OutcomeUnknown),
                    _=tick.tick()=>Ready::Tick,
                    result=local.readable()=>{result.map_err(|_|TunnelChannelError::OutcomeUnknown)?;Ready::Local},
                    result=tls.read(data.as_mut_slice())=>Ready::Tls(result.map_err(|_|TunnelChannelError::OutcomeUnknown)?),
                };
                match ready {
                    Ready::Tick => {}
                    Ready::Local => {
                        let n = match local.try_read(data.as_mut_slice()) {
                            Ok(n) => n,
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                            Err(_) => return Err(TunnelChannelError::OutcomeUnknown),
                        };
                        if n == 0 {
                            break;
                        }
                        frame
                            .charge(n)
                            .map_err(|_| TunnelChannelError::OutcomeUnknown)?;
                        self.live()
                            .map_err(|_| TunnelChannelError::OutcomeUnknown)?;
                        timeout_at(
                            self.io_deadline()
                                .map_err(|_| TunnelChannelError::OutcomeUnknown)?,
                            tls.write_all(&data[..n]),
                        )
                        .await
                        .map_err(|_| TunnelChannelError::OutcomeUnknown)?
                        .map_err(|_| TunnelChannelError::OutcomeUnknown)?;
                        timeout_at(
                            self.io_deadline()
                                .map_err(|_| TunnelChannelError::OutcomeUnknown)?,
                            tls.flush(),
                        )
                        .await
                        .map_err(|_| TunnelChannelError::OutcomeUnknown)?
                        .map_err(|_| TunnelChannelError::OutcomeUnknown)?;
                        sent = sent
                            .checked_add(n as u64)
                            .ok_or(TunnelChannelError::OutcomeUnknown)?;
                    }
                    Ready::Tls(n) => {
                        if n == 0 {
                            break;
                        }
                        frame
                            .charge(n)
                            .map_err(|_| TunnelChannelError::OutcomeUnknown)?;
                        self.live()
                            .map_err(|_| TunnelChannelError::OutcomeUnknown)?;
                        timeout_at(
                            self.io_deadline()
                                .map_err(|_| TunnelChannelError::OutcomeUnknown)?,
                            local.write_all(&data[..n]),
                        )
                        .await
                        .map_err(|_| TunnelChannelError::OutcomeUnknown)?
                        .map_err(|_| TunnelChannelError::OutcomeUnknown)?;
                        received = received
                            .checked_add(n as u64)
                            .ok_or(TunnelChannelError::OutcomeUnknown)?;
                    }
                }
                // data is zeroized/dropped BEFORE its reservation is released.
            }
            let close = timeout(Duration::from_secs(1), tls.shutdown()).await;
            let local_close = timeout(Duration::from_secs(1), local.shutdown()).await;
            drop(tls);
            drop(local);
            if !matches!(close, Ok(Ok(()))) || !matches!(local_close, Ok(Ok(()))) {
                return Err(TunnelChannelError::OutcomeUnknown);
            }
            self.scope
                .check(app)
                .await
                .map_err(|_| TunnelChannelError::OutcomeUnknown)?;
            self.live()
                .map_err(|_| TunnelChannelError::OutcomeUnknown)?;
            Ok((sent, received))
        };
        let result = tokio::select! {biased;_=self.scope.cancelled()=>Err(TunnelChannelError::OutcomeUnknown),_=self.budget.cancelled()=>Err(TunnelChannelError::OutcomeUnknown),result=action=>result};
        // Any failed channel is terminal for this original route/lease. No new
        // slot, identity, grant, proof or attempt is minted here.
        if result.is_err() {
            self.budget.stop();
        }
        drop(channel);
        result
    }
}
