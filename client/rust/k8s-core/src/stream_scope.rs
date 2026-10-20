//! Cancellation remains attached to HTTP bodies after headers arrive. A
//! request-future guard alone cannot revoke an established watch/log stream.
use crate::policy::PolicyError;
use http::{Request, Response};
use http_body::{Body, Frame, SizeHint};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tokio_util::sync::{CancellationToken, WaitForCancellationFutureOwned};
use tower::{BoxError, Layer, Service};

#[derive(Clone)]
pub(crate) struct BodyScopeLayer(pub CancellationToken);

impl<S> Layer<S> for BodyScopeLayer {
    type Service = BodyScopeService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        BodyScopeService {
            inner,
            cancel: self.0.clone(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct BodyScopeService<S> {
    inner: S,
    cancel: CancellationToken,
}

impl<S, Req, Res> Service<Request<Req>> for BodyScopeService<S>
where
    S: Service<Request<Req>, Response = Response<Res>> + Send,
    S::Future: Send + 'static,
    S::Error: Into<BoxError>,
    Res: Body + Send + 'static,
    Res::Error: Into<BoxError>,
{
    type Response = Response<ScopedBody<Res>>;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, BoxError>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, request: Request<Req>) -> Self::Future {
        let cancel = self.cancel.clone();
        let future = self.inner.call(request);
        Box::pin(async move {
            let response = future.await.map_err(Into::into)?;
            Ok(response.map(|body| ScopedBody::new(body, cancel)))
        })
    }
}

pub(crate) struct ScopedBody<B> {
    inner: Option<Pin<Box<B>>>,
    cancelled: Pin<Box<WaitForCancellationFutureOwned>>,
}

impl<B> ScopedBody<B> {
    fn new(body: B, cancel: CancellationToken) -> Self {
        Self {
            inner: Some(Box::pin(body)),
            cancelled: Box::pin(cancel.cancelled_owned()),
        }
    }
}

impl<B: Body> Body for ScopedBody<B>
where
    B::Error: Into<BoxError>,
{
    type Data = B::Data;
    type Error = BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        if self.inner.is_none() {
            return Poll::Ready(None);
        }
        // Register the token's waker even when the underlying body is idle.
        // Prioritize revocation over already buffered remote plaintext.
        if self.cancelled.as_mut().poll(cx).is_ready() {
            self.inner = None;
            return Poll::Ready(Some(Err(Box::new(PolicyError::Closed))));
        }
        let result = self.inner.as_mut().unwrap().as_mut().poll_frame(cx);
        match result {
            Poll::Ready(None) => {
                self.inner = None;
                Poll::Ready(None)
            }
            Poll::Ready(Some(Err(error))) => {
                self.inner = None;
                Poll::Ready(Some(Err(error.into())))
            }
            Poll::Ready(Some(Ok(frame))) => Poll::Ready(Some(Ok(frame))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.as_ref().is_none_or(|body| body.is_end_stream())
    }

    fn size_hint(&self) -> SizeHint {
        self.inner
            .as_ref()
            .map_or_else(|| SizeHint::with_exact(0), |body| body.size_hint())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http_body_util::{BodyExt, Full};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    struct IdleBody(Arc<AtomicBool>);
    impl Drop for IdleBody {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    impl Body for IdleBody {
        type Data = Bytes;
        type Error = BoxError;
        fn poll_frame(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
            Poll::Pending
        }
    }

    #[tokio::test]
    async fn lock_wakes_an_idle_stream_and_drops_its_transport() {
        let cancel = CancellationToken::new();
        let dropped = Arc::new(AtomicBool::new(false));
        let mut body = ScopedBody::new(IdleBody(dropped.clone()), cancel.clone());
        let (started, ready) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let mut started = Some(started);
            std::future::poll_fn(|cx| {
                let result = Pin::new(&mut body).poll_frame(cx);
                if let Some(started) = started.take() {
                    let _ = started.send(());
                }
                result
            })
            .await
            .unwrap()
            .unwrap_err()
        });
        ready.await.unwrap();
        assert!(!dropped.load(Ordering::SeqCst));
        cancel.cancel();
        let error = tokio::time::timeout(std::time::Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
        assert!(error.downcast_ref::<PolicyError>() == Some(&PolicyError::Closed));
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn closed_scope_never_returns_buffered_data() {
        let cancel = CancellationToken::new();
        let mut body = ScopedBody::new(
            Full::new(Bytes::from_static(b"private-log")),
            cancel.clone(),
        );
        cancel.cancel();
        let error = body.frame().await.unwrap().unwrap_err();
        assert!(error.downcast_ref::<PolicyError>() == Some(&PolicyError::Closed));
        assert!(body.frame().await.is_none());
        assert!(body.is_end_stream());
    }
}
