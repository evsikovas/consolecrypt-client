//! TLS close_notify is ONLY explicit original-owner stop intent, not drainage.
use super::*;
use std::task::Poll;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_rustls::client::TlsStream;

pub(super) struct OriginalIo<'a> {
    scope: &'a OriginalSessionDeviceIdentity,
    budget: &'a Budget,
    expiry: Timestamp,
}
impl<'a> OriginalIo<'a> {
    pub(super) fn new(
        scope: &'a OriginalSessionDeviceIdentity,
        budget: &'a Budget,
        expiry: Timestamp,
    ) -> Self {
        Self {
            scope,
            budget,
            expiry,
        }
    }
    pub(super) fn check(&self) -> AppResult<()> {
        self.scope.check_current()?;
        self.budget.check().map_err(|_| denied())?;
        self.scope.remaining_until(self.expiry)?;
        Ok(())
    }
    pub(super) fn deadline(&self) -> AppResult<tokio::time::Instant> {
        self.check()?;
        let remaining = self
            .scope
            .remaining_until(self.expiry)?
            .min(Duration::from_secs(5));
        Ok(tokio::time::Instant::from_std(self.scope.deadline())
            .min(tokio::time::Instant::from_std(
                self.budget.deadline().map_err(|_| denied())?,
            ))
            .min(tokio::time::Instant::now() + remaining))
    }
    fn with_poll<T>(&self, poll: impl FnOnce() -> T) -> AppResult<T> {
        self.scope.with_io_current(|wall| {
            self.budget.check().map_err(|_| denied())?;
            if wall >= Duration::from_secs(self.expiry.seconds()) {
                return Err(denied());
            }
            Ok(poll())
        })
    }
    /// Original auth/provenance/version guards cover each bounded poll, never
    /// Pending. Bytes already dispatched during a valid poll cannot be recalled.
    pub(super) async fn run<T>(
        &self,
        future: impl std::future::Future<Output = AppResult<T>>,
    ) -> AppResult<T> {
        let deadline = self.deadline()?;
        tokio::pin!(future);
        let guarded = std::future::poll_fn(|cx| {
            self.with_poll(|| std::future::Future::poll(future.as_mut(), cx))
                .unwrap_or_else(|error| Poll::Ready(Err(error)))
        });
        tokio::select! {biased;_=self.cancelled()=>Err(denied()),
        _=tokio::time::sleep_until(deadline)=>Err(denied()),result=guarded=>result}
    }
    /// Read/termination waits obey original TTL/idle only, not the five-second
    /// individual write budget. TLS reads may themselves emit control traffic.
    pub(super) async fn read_live<S: AsyncRead + Unpin>(
        &self,
        stream: &mut S,
        bytes: &mut [u8],
    ) -> AppResult<usize> {
        let read = async { stream.read(bytes).await.map_err(|_| denied()) };
        tokio::pin!(read);
        let guarded = std::future::poll_fn(|cx| {
            self.with_poll(|| std::future::Future::poll(read.as_mut(), cx))
                .unwrap_or_else(|error| Poll::Ready(Err(error)))
        });
        tokio::select! {biased;_=self.cancelled()=>Err(denied()),result=guarded=>result}
    }
    pub(super) async fn cancelled(&self) {
        let expiry = self
            .scope
            .remaining_until(self.expiry)
            .map(|remaining| tokio::time::Instant::now() + remaining)
            .unwrap_or_else(|_| tokio::time::Instant::now());
        tokio::select! {biased;_=self.scope.cancelled()=>{},_=self.budget.cancelled()=>{},
        _=tokio::time::sleep_until(expiry)=>{}}
    }
}
pub(super) struct OriginalStop {
    scope: Arc<OriginalSessionDeviceIdentity>,
    requested: CancellationToken,
}
impl OriginalStop {
    pub(super) fn new(scope: Arc<OriginalSessionDeviceIdentity>) -> Self {
        Self {
            scope,
            requested: CancellationToken::new(),
        }
    }
    // Called only inside SAME-original with_loaded guard by the route API.
    pub(super) fn request_held(&self) -> AppResult<()> {
        self.scope.check_current()?;
        self.requested.cancel();
        Ok(())
    }
    pub(super) async fn requested(&self) {
        self.requested.cancelled().await;
    }
    pub(super) fn is_requested(&self) -> bool {
        self.requested.is_cancelled()
    }

    pub(super) async fn send_request<S: AsyncRead + AsyncWrite + Unpin>(
        &self,
        app: &AppCore,
        tls: &mut TlsStream<S>,
        io: &OriginalIo<'_>,
    ) -> bool {
        self.send_request_inner(Some(app), tls, io).await
    }
    /// Public weak-owner route: no upgraded AppCore survives network Pending.
    pub(super) async fn send_request_original<S: AsyncRead + AsyncWrite + Unpin>(
        &self, tls: &mut TlsStream<S>, io: &OriginalIo<'_>,
    ) -> bool {
        self.send_request_inner(None, tls, io).await
    }
    async fn send_request_inner<S: AsyncRead + AsyncWrite + Unpin>(
        &self, app: Option<&AppCore>, tls: &mut TlsStream<S>, io: &OriginalIo<'_>,
    ) -> bool {
        if !self.is_requested() || !std::ptr::eq(io.scope, Arc::as_ptr(&self.scope)) {
            return false;
        }
        // Guard acquisition cannot retain the old stream beyond the original
        // cancellation/TTL. No shutdown on any failed/retired original frame.
        let admitted = tokio::select! {biased;_=io.cancelled()=>false,
        r=async { match app { Some(app) => self.scope.check(app).await, None => self.scope.check_current() } }=>r.is_ok()};
        if !admitted {
            return false;
        }
        if io
            .with_poll(|| tls.get_mut().1.send_close_notify())
            .is_err()
        {
            return false;
        }
        // poll_shutdown also shuts down the WSS carrier. Flush ONLY the
        // authenticated close_notify, retaining the actual carrier for Agent
        // child/parent termination; no generic plaintext Close is introduced.
        if io
            .run(async { tls.flush().await.map_err(|_| denied()) })
            .await
            .is_err()
        {
            return false;
        }
        let Ok(cleanup_deadline) = io.deadline() else {
            return false;
        };
        let termination = async {
            let mut ciphertext = zeroize::Zeroizing::new([0u8; 8192]);
            let mut observed = 0usize;
            loop {
                // No application payload may be forwarded after Stop. Bounded
                // encrypted close/control overhead is discarded, never interpreted
                // as a public join/CAS ACK. Original TTL/idle remain unchanged.
                let n = match io
                    .read_live(tls.get_mut().0, ciphertext.as_mut_slice())
                    .await
                {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(_) => return false,
                };
                observed = match observed.checked_add(n) {
                    Some(v) if v <= 65536 => v,
                    _ => return false,
                };
            }
            tokio::select! {biased;_=io.cancelled()=>false,r=async { match app { Some(app) => self.scope.check(app).await, None => self.scope.check_current() } }=>r.is_ok()&&io.check().is_ok()}
        };
        // ONE post-flush cleanup bound, never reset by data/control traffic.
        // Ordinary DATA read_live retains only original TTL/idle limits.
        tokio::time::timeout_at(cleanup_deadline, termination)
            .await
            .unwrap_or(false)
    }
}
