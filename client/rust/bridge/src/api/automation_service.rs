//! Fixed service delegation workflow. Core parses a closed request and retains
//! original owner/nonce/review provenance; no public terms or signer endpoint.
use crate::api::error::BridgeError;
use crate::state::with_core;
pub async fn automation_service_schedules_json(
    request_json: String,
) -> Result<String, BridgeError> {
    with_core(
        move |core| async move { Ok(core.automation_service_schedules_json(request_json).await?) },
    )
    .await
}

pub async fn automation_service_prepare_json(request_json: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { Ok(c.automation_service_prepare_json(request_json).await?) })
        .await
}

pub async fn automation_service_confirm_json(request_json: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { Ok(c.automation_service_confirm_json(request_json).await?) })
        .await
}

pub async fn automation_service_cancel_json(request_json: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { Ok(c.automation_service_cancel_json(request_json).await?) })
        .await
}

pub async fn automation_service_status_json(request_json: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { Ok(c.automation_service_status_json(request_json).await?) })
        .await
}

pub async fn automation_service_revoke_json(request_json: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { Ok(c.automation_service_revoke_json(request_json).await?) })
        .await
}

pub async fn automation_service_results_json(request_json: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { Ok(c.automation_service_results_json(request_json).await?) })
        .await
}
