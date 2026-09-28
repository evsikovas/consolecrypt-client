//! Explicit cancellation helpers.
//!
//! Every future and stream in this crate is also cancelled by simply dropping
//! it (the HTTP request is aborted). A [`CancellationToken`] is convenient
//! when the owner of the request is elsewhere (e.g. a "Stop" button in the UI
//! routed through app-core).

use crate::error::AiError;
use futures::stream::{BoxStream, StreamExt};
use std::future::Future;
pub use tokio_util::sync::CancellationToken;

/// Race `fut` against `token`; returns [`AiError::Cancelled`] if the token
/// fires first (the future is dropped, aborting any in-flight request).
pub async fn with_cancellation<F, T>(token: &CancellationToken, fut: F) -> Result<T, AiError>
where
    F: Future<Output = Result<T, AiError>>,
{
    tokio::select! {
        biased;
        () = token.cancelled() => Err(AiError::Cancelled),
        r = fut => r,
    }
}

/// Wrap a stream so that it yields a final `Err(Cancelled)` and ends as soon
/// as `token` fires.
pub fn cancellable<T: Send + 'static>(
    stream: BoxStream<'static, Result<T, AiError>>,
    token: CancellationToken,
) -> BoxStream<'static, Result<T, AiError>> {
    futures::stream::unfold(
        (stream, token, false),
        |(mut stream, token, done)| async move {
            if done {
                return None;
            }
            tokio::select! {
                biased;
                () = token.cancelled() => Some((Err(AiError::Cancelled), (stream, token, true))),
                item = stream.next() => item.map(|i| (i, (stream, token, false))),
            }
        },
    )
    .boxed()
}
