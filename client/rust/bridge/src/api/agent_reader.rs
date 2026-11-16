//! Original Native Reader handle. Only initial prepare/status capture global Core.
use crate::{api::error::BridgeError, state};
use std::sync::Arc;

#[flutter_rust_bridge::frb(opaque)]
pub struct ReaderActionHandle {
    inner: Arc<cc_app_core::ReaderActionHandle>,
}
impl Drop for ReaderActionHandle {
    fn drop(&mut self) {
        let _runtime = state::runtime().enter();
        self.inner.cancel();
    }
}
impl ReaderActionHandle {
    #[flutter_rust_bridge::frb(sync)]
    pub fn details(&self) -> Result<String, BridgeError> {
        self.inner.details_json().map_err(BridgeError::from)
    }
    pub async fn confirm(&self) -> Result<String, BridgeError> {
        let original = self.inner.clone();
        state::run(async move { original.confirm_json().await.map_err(BridgeError::from) }).await
    }
    #[flutter_rust_bridge::frb(sync)]
    pub fn cancel(&self) {
        let _runtime = state::runtime().enter();
        self.inner.cancel();
    }
    pub async fn transfer_export(&self,public_destination:String,invitation_destination:Option<String>)->Result<String,BridgeError>{
        let original=self.inner.clone();
        state::run(async move{original.transfer_export_json(public_destination,invitation_destination).await.map_err(BridgeError::from)}).await
    }
    pub async fn consume_read(&self) -> Result<String, BridgeError> {
        let original = self.inner.clone();
        state::run(async move {
            original
                .consume_read_json()
                .await
                .map_err(BridgeError::from)
        })
        .await
    }
}
pub async fn agent_reader_prepare_action(
    request_json: String,
) -> Result<ReaderActionHandle, BridgeError> {
    let original = state::core()?;
    state::run(async move {
        let handle = original.reader_prepare_action_json(&request_json).await?;
        Ok(ReaderActionHandle {
            inner: Arc::new(handle),
        })
    })
    .await
}
pub async fn agent_reader_status(request_json: String) -> Result<String, BridgeError> {
    let original = state::core()?;
    state::run(async move {
        original
            .reader_status_json(&request_json)
            .await
            .map_err(BridgeError::from)
    })
    .await
}
