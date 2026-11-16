//! JSON contains bounded DATA only. Issued operations, passwords, destinations
//! and cancellation remain typed values, not IDs resolvable through a registry.
use super::*;
pub(super) fn scope(raw: &str) -> AppResult<PortableScope> {
    if raw.len() > 1024 {
        return Err(invalid());
    }
    serde_json::from_str(raw).map_err(|_| invalid())
}
impl AppCore {
    pub async fn automation_portable_scope_json(&self) -> AppResult<String> {
        serde_json::to_string(&self.automation_portable_scope().await?).map_err(|_| invalid())
    }
    pub async fn automation_portable_register_context_json(
        &self,
        scope_json: String,
        project_id: String,
        project_revision: u64,
    ) -> AppResult<PortableOperation> {
        self.portable_register_context(scope(&scope_json)?, project_id, project_revision)
            .await
    }
}
