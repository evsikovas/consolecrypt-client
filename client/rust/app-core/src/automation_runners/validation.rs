//! Typed dedicated validation factory. No shared RunnerConnection/job peer.
use super::*;
use crate::session::Unlocked;
use cc_automation_core::{ValidationHelperConnection, VaultValidationHandler};
use tokio_util::sync::CancellationToken;
impl AppCore {
    pub(crate) async fn open_validation_owner(
        &self,
        runner: &dependency::DependencyRunner,
        original: &std::sync::Weak<Unlocked>,
        operation: Uuid,
        cancel: &CancellationToken,
        handler: Arc<dyn VaultValidationHandler>,
    ) -> AppResult<ValidationHelperConnection> {
        match &runner.configuration {
            AutomationRunnerConfig::Local { launch } => launch
                .spawn_validation(operation, cancel, handler)
                .await
                .map_err(runner_error),
            AutomationRunnerConfig::ServiceRemote { .. } => Err(AppError::invalid(
                "automation_runner",
                "service_runner_purpose_required",
            )),
            AutomationRunnerConfig::Remote { host_id, helper } => {
                let session = self
                    .open_private_runner_session(host_id, original, cancel)
                    .await?;
                // Factory internally closes/awaits failed mode verification;
                // never outer-select/drop it and abandon the owned transport.
                helper
                    .connect_validation(session, operation, cancel, handler)
                    .await
                    .map_err(runner_error)
            }
        }
    }
}
