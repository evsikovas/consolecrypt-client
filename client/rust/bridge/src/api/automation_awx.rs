//! Opaque original registration. Cancellation/join never looks up a replacement
//! global profile and cannot create a request from an unknown operation UUID.
use crate::{api::error::BridgeError, state};
use cc_app_core::{AppCore, AwxNativeOperation};
use std::sync::Arc;

#[flutter_rust_bridge::frb(opaque)]
pub struct AwxRequestHandle {
    core: AppCore,
    registration: Arc<AwxNativeOperation>,
}
impl AwxRequestHandle {
    pub async fn start(&self) -> Result<String, BridgeError> {
        let core = self.core.clone();
        let registration = self.registration.clone();
        state::run(async move {
            core.automation_awx_start_registered(&registration)
                .await
                .map_err(BridgeError::from)
        })
        .await
    }
    #[flutter_rust_bridge::frb(sync)]
    pub fn cancel(&self) {
        self.registration.cancel();
    }
    pub async fn joined(&self) -> Result<(), BridgeError> {
        let registration = self.registration.clone();
        state::run(async move {
            registration.joined().await;
            Ok(())
        })
        .await
    }
}

pub async fn automation_awx_scope() -> Result<String, BridgeError> {
    state::with_core(|core| async move {
        core.automation_awx_scope_json()
            .await
            .map_err(BridgeError::from)
    })
    .await
}
pub async fn automation_awx_catalog_request(request_json: String) -> Result<String, BridgeError> {
    state::with_core(move |core| async move {
        core.automation_awx_catalog_request_json(request_json)
            .await
            .map_err(BridgeError::from)
    })
    .await
}
pub async fn automation_awx_register_request(
    request_json: String,
) -> Result<AwxRequestHandle, BridgeError> {
    state::with_core(move |core| async move {
        let registration = core
            .automation_awx_register_request_json(request_json)
            .await?;
        Ok(AwxRequestHandle {
            core,
            registration: Arc::new(registration),
        })
    })
    .await
}
pub async fn automation_awx_discard_review(request_json: String) -> Result<(), BridgeError> {
    state::with_core(move |core| async move {
        core.automation_awx_discard_review_json(request_json)
            .await
            .map_err(BridgeError::from)
    })
    .await
}
