//! Dedicated validation mode. Typed owner cannot be built from a job peer.
use crate::{
    AutomationError, DependencyHelperConnection, HelperLaunch, HelperPeer, ProjectRevision,
    ProjectRoot, RemoteHelperConfig, Result, RunParameters, UploadedRevision, ValidationProof,
    VaultValidationHandler, VaultValidationMarker, VaultValidationPasswordRequest,
    VaultValidationRuntime, VerifiedValidation,
};
use cc_models::automation::ApprovedTarget;
use cc_ssh_core::SshSession;
use serde_json::{json, Value};
use std::{
    sync::{Arc, Weak},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
pub struct ValidationHelperConnection {
    inner: DependencyHelperConnection,
}
impl std::fmt::Debug for ValidationHelperConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ValidationHelperConnection")
            .finish_non_exhaustive()
    }
}
/// Minted only by verified dedicated-mode owner. No Deserialize/normal job peer.
#[derive(Clone)]
pub struct ValidationPeerObserver {
    peer: Weak<HelperPeer>,
    operation: Uuid,
}
impl ValidationPeerObserver {
    /// Same bounded upload on a factory-issued dedicated peer. Keeping this
    /// weak observer never turns a shared/default job peer into a Vault peer.
    pub async fn upload(
        &self,
        root: &ProjectRoot,
        revision: &ProjectRevision,
    ) -> Result<UploadedRevision> {
        let peer = self.peer.upgrade().ok_or(AutomationError::Closed)?;
        root.upload_dependency(revision, &peer).await
    }
    pub async fn observe(&self) -> Result<VaultValidationRuntime> {
        let peer = self.peer.upgrade().ok_or(AutomationError::Closed)?;
        let report = peer
            .request_dependency_bounded(
                "environment_probe",
                json!({}),
                Duration::from_secs(10),
                64 * 1024,
            )
            .await?;
        VaultValidationRuntime::parse_pinned_probe(report)
    }
    pub fn operation_id(&self) -> Uuid {
        self.operation
    }
    pub fn cancel(&self) {
        if let Some(peer) = self.peer.upgrade() {
            peer.abort_dependency_exchange();
        }
    }
}
/// Raw decrypted diagnostics have no accessor/Serialize/Clone. Only the original
/// Native sensitivity marker releases a safe result, not a helper used_vault flag.
pub struct VaultValidationResult {
    validation: VerifiedValidation,
}
impl std::fmt::Debug for VaultValidationResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultValidationResult")
            .finish_non_exhaustive()
    }
}
impl VaultValidationResult {
    pub fn seal(self, marker: &VaultValidationMarker) -> Result<VerifiedValidation> {
        self.validation.with_vault_context(marker)
    }
}
impl ValidationHelperConnection {
    pub async fn open_default_validation_sidecar(
        &self,
        uploaded: &UploadedRevision,
        binding: &crate::VaultRunBinding,
        children: Vec<VaultValidationPasswordRequest>,
    ) -> Result<()> {
        binding.validate()?;
        if binding.operation_id() != self.inner.operation_id() || children.len() != 4 {
            return Err(AutomationError::Invalid("private sidecar input changed"));
        }
        let value = self
            .inner
            .private_control_peer()
            .request_dependency_bounded(
                "vault_sidecar_begin",
                json!({"revision_id":uploaded.revision_id,"binding":binding,"children":children}),
                Duration::from_secs(10),
                4096,
            )
            .await?;
        if value
            != json!({"version":1,"operation_id":self.inner.operation_id(),"phases":["syntax","lint"]})
        {
            return Err(AutomationError::Protocol(
                "invalid private sidecar observation",
            ));
        }
        Ok(())
    }
    pub async fn finish_default_validation_sidecar(&self) -> Result<()> {
        let value = self
            .inner
            .private_control_peer()
            .request_dependency_bounded(
                "vault_sidecar_finish_validation",
                json!({}),
                Duration::from_secs(10),
                4096,
            )
            .await?;
        if value
            != json!({"version":1,"operation_id":self.inner.operation_id(),"validation_closed":true})
        {
            return Err(AutomationError::Protocol(
                "invalid private sidecar observation",
            ));
        }
        self.inner
            .private_control_peer()
            .drain_vault_handlers()
            .await
    }
    pub async fn open_admitted_job_sidecar(
        &self,
        capsule: &crate::VaultPasswordRequest,
    ) -> Result<()> {
        let value = self
            .inner
            .private_control_peer()
            .request_dependency_bounded(
                "vault_sidecar_material",
                json!({"capsule":capsule}),
                Duration::from_secs(10),
                4096,
            )
            .await?;
        if value
            != json!({"version":1,"operation_id":self.inner.operation_id(),"phase":"execution"})
        {
            return Err(AutomationError::Protocol(
                "invalid private material observation",
            ));
        }
        Ok(())
    }
    pub fn observer(&self) -> ValidationPeerObserver {
        ValidationPeerObserver {
            peer: Arc::downgrade(self.inner.private_control_peer()),
            operation: self.inner.operation_id(),
        }
    }
    pub async fn upload(
        &self,
        root: &ProjectRoot,
        revision: &ProjectRevision,
    ) -> Result<UploadedRevision> {
        root.upload_dependency(revision, self.inner.private_control_peer())
            .await
    }
    pub async fn prepare(
        &self,
        uploaded: &UploadedRevision,
        playbook: &str,
        parameters: &RunParameters,
        inventory: &Value,
        targets: &[ApprovedTarget],
        children: Vec<VaultValidationPasswordRequest>,
    ) -> Result<VaultValidationResult> {
        parameters.validate()?;
        if targets.is_empty() || targets.len() > 100 || children.len() != 4 {
            return Err(AutomationError::Invalid(
                "private Vault validation input budget",
            ));
        }
        let environment = children[0].environment_digest.clone();
        let body = json!({"revision_id":uploaded.revision_id,"playbook":playbook,"parameters":parameters,"inventory":inventory,"targets":targets,"vault_children":children});
        if serde_json::to_vec(&body).map_or(true, |bytes| bytes.len() > 512 * 1024) {
            return Err(AutomationError::Invalid(
                "private Vault validation input budget",
            ));
        }
        let value = self
            .inner
            .private_control_peer()
            .request_dependency_bounded(
                "validation_prepare",
                body,
                Duration::from_secs(270),
                1024 * 1024 - 4096,
            )
            .await?;
        let proof: ValidationProof = serde_json::from_value(value)
            .map_err(|_| AutomationError::Protocol("invalid private validation proof"))?;
        Ok(VaultValidationResult {
            validation: proof.verify(
                &uploaded.digest,
                &environment,
                playbook,
                parameters,
                inventory,
                targets,
            )?,
        })
    }
    pub async fn close(self) -> bool {
        self.inner.close().await
    }
}
impl HelperLaunch {
    pub async fn spawn_validation(
        &self,
        operation: Uuid,
        cancel: &CancellationToken,
        handler: Arc<dyn VaultValidationHandler>,
    ) -> Result<ValidationHelperConnection> {
        if cancel.is_cancelled() {
            return Err(AutomationError::Closed);
        }
        let root = match self {
            Self::Native { root, .. } => root
                .to_str()
                .ok_or(AutomationError::Invalid("validation root must be UTF8"))?,
            Self::Wsl { root, .. } => root,
        };
        let expected =
            crate::dependency_launch::DependencyExpectation::validation(root, operation)?;
        let mut command = self.base_command()?;
        command.args([
            "--validation-stdio",
            "--root",
            root,
            "--operation-id",
            &operation.to_string(),
        ]);
        let local = Self::spawn_command_channels(command, None, Some(handler)).await?;
        Ok(ValidationHelperConnection {
            inner: DependencyHelperConnection::local(local, expected, cancel).await?,
        })
    }
}
impl RemoteHelperConfig {
    pub async fn connect_validation(
        &self,
        session: SshSession,
        operation: Uuid,
        cancel: &CancellationToken,
        handler: Arc<dyn VaultValidationHandler>,
    ) -> Result<ValidationHelperConnection> {
        let expected =
            crate::dependency_launch::DependencyExpectation::validation(&self.root, operation)?;
        let command = self.private_validation_command(operation)?;
        let mut remote = crate::dependency_launch::OwnedDependencySsh::new(session);
        let opened=async {tokio::select! {biased;
            _=cancel.cancelled()=>Err(AutomationError::Closed),
            result=tokio::time::timeout(Duration::from_secs(10),remote.session().open_exec(&command))=>result.map_err(|_|AutomationError::Timeout)?.map_err(|_|AutomationError::Remote("validation_ssh_exec".into())),
        }}.await;
        let channel = match opened {
            Ok(channel) => channel,
            Err(error) => {
                remote.close().await;
                return Err(error);
            }
        };
        let (reader, writer) = tokio::io::split(channel.into_stream());
        let peer = HelperPeer::connect_with_vault(reader, writer, handler);
        Ok(ValidationHelperConnection {
            inner: DependencyHelperConnection::remote(peer, remote, expected, cancel).await?,
        })
    }
}
