//! Fixed new-project factory only. No installer, signer or execution endpoint.
use crate::api::error::BridgeError;
use crate::state::with_core;

pub async fn automation_agent_installer_context_json(
    request_json: String,
) -> Result<String, BridgeError> {
    with_core(move |core| async move {
        Ok(core
            .automation_agent_installer_context_json(&request_json)
            .await?)
    })
    .await
}
pub async fn automation_agent_installer_create_json(
    request_json: String,
) -> Result<String, BridgeError> {
    with_core(move |core| async move {
        Ok(core
            .automation_agent_installer_create_json(&request_json)
            .await?)
    })
    .await
}

pub async fn automation_monitoring_node_create_json(
    request_json: String,
) -> Result<String, BridgeError> {
    with_core(move |core| async move {
        Ok(core
            .automation_monitoring_node_create_json(&request_json)
            .await?)
    })
    .await
}
