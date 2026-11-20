//! Fixed live metrics for a saved Host. No Monitoring product API.
use crate::{api::error::BridgeError, frb_generated::StreamSink, state::with_core};
pub async fn host_metrics_watch(
    request_json: String,
    sink: StreamSink<String>,
) -> Result<(), BridgeError> {
    with_core(move |core| async move {
        core.host_metrics_watch(&request_json, move |sample| {
            sink.add(sample.to_string()).is_ok()
        })
        .await?;
        Ok(())
    })
    .await
}
pub async fn host_metrics_cancel(operation_id: String) -> Result<(), BridgeError> {
    with_core(move |core| async move { Ok(core.host_metrics_cancel(operation_id).await?) }).await
}
