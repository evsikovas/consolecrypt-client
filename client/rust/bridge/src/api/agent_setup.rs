//! Agent installation wizard on its original Native operation, without an
//! Automation project or caller-selected shell command.
use crate::{api::error::BridgeError, state};
use std::sync::Arc;

#[flutter_rust_bridge::frb(opaque)]
pub struct AgentSetupHandle {
    original: Arc<cc_app_core::AgentSetupOperation>,
}

impl Drop for AgentSetupHandle {
    fn drop(&mut self) {
        let _runtime = state::runtime().enter();
        self.original.cancel();
    }
}

impl AgentSetupHandle {
    pub async fn review(&self) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move { original.review_json().await.map_err(BridgeError::from) }).await
    }

    pub async fn install_software(&self) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move {
            original
                .install_software_json()
                .await
                .map_err(BridgeError::from)
        })
        .await
    }

    #[flutter_rust_bridge::frb(sync)]
    pub fn cancel(&self) {
        let _runtime = state::runtime().enter();
        self.original.cancel();
    }

    pub async fn joined(&self) -> Result<String, BridgeError> {
        let original = self.original.clone();
        state::run(async move { original.joined_json().await.map_err(BridgeError::from) }).await
    }
}

pub async fn agent_setup_context(request_json: String) -> Result<String, BridgeError> {
    let original = state::core()?;
    state::run(async move {
        original
            .agent_setup_context_json(&request_json)
            .await
            .map_err(BridgeError::from)
    })
    .await
}

pub async fn agent_setup_prepare(request_json: String) -> Result<AgentSetupHandle, BridgeError> {
    let original = state::core()?;
    state::run(async move {
        let operation = original.agent_setup_prepare_json(&request_json).await?;
        Ok(AgentSetupHandle {
            original: Arc::new(operation),
        })
    })
    .await
}
