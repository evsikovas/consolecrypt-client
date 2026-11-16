//! One privately enrolled service owner. Keys never leave this process.
use crate::{
    AutomationError, HelperLaunch, HelperProcess, JobScopeRegistry, Result, ServiceCapsuleInput,
    ServiceExecutionObservation, ServiceExecutionSupervisor,
};
use cc_crypto_core::{
    DevicePublicKeys, DeviceSecretKeys, ServiceExecutionOrigin, ServicePolicyTenant,
    SignedServiceDelegation,
};
use cc_models::{automation::JobMode, known_host::KnownHost};
use cc_ssh_core::ConnectionPlan;
use serde_json::Value;
use std::{sync::Arc, time::Duration};

fn denied() -> AutomationError {
    AutomationError::Invalid("original service enrollment unavailable")
}
pub struct ServiceNode {
    keys: Option<DeviceSecretKeys>,
    identity: Option<crate::ServiceIdentity>,
    public: DevicePublicKeys,
    registry: Arc<JobScopeRegistry>,
    process: Option<HelperProcess>,
    supervisor: Option<ServiceExecutionSupervisor>,
    original_owner: Option<(String, DevicePublicKeys, ServicePolicyTenant)>,
    origin: Option<ServiceExecutionOrigin>,
    bootstrap: Option<crate::service_restart::Bootstrap>,
    runtime_pin: Option<crate::service_restart::RuntimePin>,
}
impl ServiceNode {
    pub fn new() -> Result<Self> {
        if !cfg!(target_os = "linux") {
            return Err(denied());
        }
        #[cfg(target_os = "linux")]
        if rustix::process::geteuid().is_root() {
            return Err(denied());
        }
        let keys = DeviceSecretKeys::generate().map_err(|_| denied())?;
        let public = keys.public_keys();
        Ok(Self {
            keys: Some(keys),
            identity: None,
            public,
            registry: Arc::new(JobScopeRegistry::default()),
            process: None,
            supervisor: None,
            original_owner: None,
            origin: None,
            bootstrap: None,
            runtime_pin: None,
        })
    }
    /// A locally provisioned separate service key holder; never raw imported
    /// Desktop/Agent material. This restores identity only, not enrollment.
    pub fn with_identity(mut identity: crate::ServiceIdentity) -> Result<Self> {
        let keys = identity.take_keys()?;
        let public = keys.public_keys();
        Ok(Self {
            keys: Some(keys),
            identity: Some(identity),
            public,
            registry: Arc::new(JobScopeRegistry::default()),
            process: None,
            supervisor: None,
            original_owner: None,
            origin: None,
            bootstrap: None,
            runtime_pin: None,
        })
    }
    fn check_identity(&self) -> Result<()> {
        if let Some(identity) = &self.identity {
            identity.recheck()?;
        }
        if let Some(root) = &self.runtime_pin {
            root.check()?;
        }
        Ok(())
    }
    pub fn local_namespace(
        &self,
    ) -> Result<Option<crate::service_identity::ServiceLocalNamespace>> {
        self.check_identity()?;
        self.identity
            .as_ref()
            .map(|identity| identity.local_namespace())
            .transpose()
    }
    pub fn public_keys(&self) -> &DevicePublicKeys {
        &self.public
    }
    /// Bootstrap is original private control data over an owned inherited pipe.
    /// It is not accepted from a policy's self-asserted owner or a public socket.
    pub async fn start(
        &mut self,
        launch: &HelperLaunch,
        origin: ServiceExecutionOrigin,
        owner_id: String,
        owner: DevicePublicKeys,
        tenant: ServicePolicyTenant,
        owner_epoch: u64,
        owner_not_after: u64,
        schedule: Value,
    ) -> Result<()> {
        self.check_identity()?;
        if self.process.is_some()
            || self.keys.is_none()
            || self
                .identity
                .as_ref()
                .map(|i| i.enrollment())
                .transpose()?
                .flatten()
                .is_some()
        {
            return Err(denied());
        }
        origin.validate().map_err(|_| denied())?;
        use sha2::{Digest, Sha256};
        if origin.signer_key_sha256 != hex::encode(Sha256::digest(self.public.signing))
            || origin.recipient_key_sha256 != hex::encode(Sha256::digest(self.public.encryption))
        {
            return Err(denied());
        }
        let bootstrap = crate::service_restart::Bootstrap {
            origin: origin.clone(),
            owner: crate::service_restart::OwnerPin {
                owner_id: owner_id.clone(),
                owner_epoch,
                tenant: tenant.clone(),
                signing: hex::encode(owner.signing),
                encryption: hex::encode(owner.encryption),
                not_after: owner_not_after,
            },
            schedule,
        };
        let process = launch
            .spawn_service(self.registry.clone(), bootstrap.helper()?)
            .await?;
        if let Err(error) = self.check_identity() {
            process.close().await;
            return Err(error);
        }
        let runtime_pin = if self.identity.is_some() {
            match crate::service_restart::RuntimePin::open(launch) {
                Ok(pin) => Some(pin),
                Err(error) => {
                    process.close().await;
                    return Err(error);
                }
            }
        } else {
            None
        };
        self.runtime_pin = runtime_pin;
        self.bootstrap = Some(bootstrap);
        self.original_owner = Some((owner_id, owner, tenant));
        self.origin = Some(origin);
        self.process = Some(process);
        Ok(())
    }
    pub async fn request(&self, operation: &str, body: Value, timeout: Duration) -> Result<Value> {
        // Only the existing fixed project preparation protocol is bridged.
        // No caller generic operation may configure trust or accept a job.
        if ![
            "probe",
            "upload_begin",
            "upload_entry",
            "upload_chunk",
            "upload_commit",
            "upload_abort",
            "prepare_run",
            "inventory_capture",
            "inventory_chunk",
            "inventory_upload_begin",
            "inventory_upload_chunk",
            "inventory_upload_commit",
            "inventory_upload_abort",
            "read_chunk",
            "read_file",
            "files",
            "revision_list",
        ]
        .contains(&operation)
        {
            return Err(denied());
        }
        let mut value = self
            .process
            .as_ref()
            .ok_or_else(denied)?
            .peer
            .request(operation, body, timeout)
            .await?;
        if operation == "probe" {
            value
                .get_mut("capabilities")
                .and_then(Value::as_object_mut)
                .ok_or_else(denied)?
                .insert("service_local_executor_v1".into(), Value::Bool(true));
            value.as_object_mut().ok_or_else(denied)?.insert(
                "service_origin".into(),
                serde_json::to_value(self.origin.as_ref().ok_or_else(denied)?)
                    .map_err(|_| denied())?,
            );
        }
        Ok(value)
    }
    pub async fn enroll(
        &mut self,
        template: Value,
        plans: Vec<ConnectionPlan>,
        known_hosts: Vec<KnownHost>,
        capsules: Vec<ServiceCapsuleInput>,
    ) -> Result<Value> {
        self.check_identity()?;
        if self.supervisor.is_some() {
            return Err(denied());
        }
        let expected = serde_json::from_value::<Vec<cc_models::automation::ApprovedTarget>>(
            template.get("targets").cloned().ok_or_else(denied)?,
        )
        .map_err(|_| denied())?;
        let delegation = template.get("delegation").cloned().ok_or_else(denied)?;
        let verified = self
            .checked_enrollment(&delegation, &plans, &known_hosts, &capsules, &expected)
            .await?;
        let process = self.process.as_ref().ok_or_else(denied)?;
        // Node performs normal immutable snapshot+report checks and pins the
        // uploaded revision before this ACK. No desktop lease is released first.
        let mut ack = process
            .peer
            .request("service_enroll", template, Duration::from_secs(30))
            .await?;
        if ack.get("enrolled") != Some(&serde_json::json!(true)) {
            return Err(denied());
        }
        self.check_identity()?;
        if let Some(identity) = &mut self.identity {
            let root = self.runtime_pin.as_ref().ok_or_else(denied)?;
            root.check()?;
            let facts = identity.facts();
            let bootstrap = serde_json::from_value(
                serde_json::to_value(self.bootstrap.as_ref().ok_or_else(denied)?)
                    .map_err(|_| denied())?,
            )
            .map_err(|_| denied())?;
            let record = crate::service_restart::EnrollmentRecord {
                version: 1,
                identity_id: facts.identity_id,
                key_epoch: facts.key_epoch,
                signing: hex::encode(facts.public_keys.signing),
                recipient: hex::encode(facts.public_keys.encryption),
                runtime: crate::service_restart::RuntimeIdentity {
                    device: root.identity.device,
                    inode: root.identity.inode,
                },
                bootstrap,
                delegation,
                helper_ack: serde_json::from_value(ack.clone()).map_err(|_| denied())?,
                targets: expected,
                plans: plans.clone(),
                known_hosts: known_hosts.clone(),
                capsules: serde_json::from_value(
                    serde_json::to_value(&capsules).map_err(|_| denied())?,
                )
                .map_err(|_| denied())?,
            };
            identity.store_enrollment(&record)?;
            ack.as_object_mut().ok_or_else(denied)?.insert(
                "localRecordDurability".into(),
                serde_json::json!("fileAndDirectorySynced"),
            );
        }
        let keys = self.keys.take().ok_or_else(denied)?;
        let supervisor = ServiceExecutionSupervisor::new(
            process.peer.clone(),
            self.registry.clone(),
            verified,
            keys,
            self.origin.as_ref().ok_or_else(denied)?.clone(),
            plans,
            known_hosts,
            capsules,
        )?;
        self.supervisor = Some(supervisor);
        Ok(ack)
    }
    async fn checked_enrollment(
        &self,
        delegation: &Value,
        plans: &[ConnectionPlan],
        known_hosts: &[KnownHost],
        capsules: &[ServiceCapsuleInput],
        expected: &[cc_models::automation::ApprovedTarget],
    ) -> Result<cc_crypto_core::VerifiedServiceDelegation> {
        self.check_identity()?;
        let raw_policy = serde_json::to_vec(delegation).map_err(|_| denied())?;
        let (owner_id, owner, tenant) = self.original_owner.as_ref().ok_or_else(denied)?;
        let current = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| denied())?
            .as_secs();
        let verified =
            SignedServiceDelegation::verify_json(&raw_policy, owner_id, owner, tenant, current)
                .map_err(|_| denied())?;
        if verified.body().access_policy.mode != cc_crypto_core::ServiceAccessMode::BoundedOffline
            || plans.is_empty()
            || plans.len() > 32
            || known_hosts.is_empty()
            || known_hosts.len() > 512
            || verified.body().execution_origin.as_ref() != self.origin.as_ref()
        {
            return Err(denied());
        }
        let original_origin = self.origin.as_ref().ok_or_else(denied)?;
        let raw = self
            .process
            .as_ref()
            .ok_or_else(denied)?
            .peer
            .request("probe", serde_json::json!({}), Duration::from_secs(30))
            .await?;
        let actual = crate::EnvironmentProbe::parse(raw.clone())?;
        if actual.environment_digest != original_origin.environment_digest
            || raw.get("helper_digest").and_then(Value::as_str)
                != Some(original_origin.helper_digest.as_str())
            || actual.system != "Linux"
            || !actual.non_root
            || !actual.dependencies_ready
            || !actual.sandbox.filesystem
            || !actual.sandbox.network
            || !actual.sandbox.pid
            || actual.require_trusted_worker_bootstrap().is_err()
        {
            return Err(denied());
        }
        // The probe awaited actual I/O. Refresh the ORIGINAL absolute window
        // and held identities before decrypting even an enrollment capsule.
        self.check_identity()?;
        let current = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| denied())?
            .as_secs();
        let verified =
            SignedServiceDelegation::verify_json(&raw_policy, owner_id, owner, tenant, current)
                .map_err(|_| denied())?;
        let pin = &self.bootstrap.as_ref().ok_or_else(denied)?.owner;
        if current >= pin.not_after || verified.body().expires_at > pin.not_after {
            return Err(denied());
        }
        crate::service_execution::validate_service_capsules(
            &verified,
            self.origin.as_ref().ok_or_else(denied)?,
            self.keys.as_ref().ok_or_else(denied)?,
            capsules,
            current.checked_add(1).ok_or_else(denied)?,
            current,
        )?;
        let capsule_ids = capsules
            .iter()
            .map(|input| input.binding.credential_id.clone())
            .collect();
        crate::service_execution::validate_service_plans(
            &verified,
            self.origin.as_ref().ok_or_else(denied)?,
            plans,
            known_hosts,
            expected,
            &capsule_ids,
        )?;
        self.check_identity()?;
        Ok(verified)
    }
    pub(crate) async fn restore(&mut self, launch: &HelperLaunch) -> Result<Option<JobMode>> {
        self.check_identity()?;
        if self.process.is_some() || self.supervisor.is_some() {
            return Err(denied());
        }
        let Some(identity) = &self.identity else {
            return Ok(None);
        };
        let Some(record) = identity.enrollment()? else {
            return Ok(None);
        };
        let root = crate::service_restart::RuntimePin::open(launch)?;
        if root.identity != record.runtime {
            return Err(denied());
        }
        // Existing enrollment was verified at ORIGINAL issue by enrollment().
        // This fixed helper may remove ended plaintext inputs only after its
        // original quiescence/expiry/signed-revoke proof. It cannot resume,
        // decrypt a capsule, configure targets or mint a current job grant.
        let mut retirement = record.bootstrap.helper()?;
        let fields = retirement.as_object_mut().ok_or_else(denied)?;
        fields.remove("schedule");
        fields.insert("delegation".into(), record.delegation.clone());
        fields.insert(
            "revision_id".into(),
            serde_json::json!(record.helper_ack.revision_id),
        );
        let facts = identity.facts();
        fields.insert("identity".into(),serde_json::json!({"identity_id":facts.identity_id,"key_epoch":facts.key_epoch,
            "signing":hex::encode(facts.public_keys.signing),"recipient":hex::encode(facts.public_keys.encryption)}));
        fields.insert(
            "runtime_identity".into(),
            serde_json::to_value(&record.runtime).map_err(|_| denied())?,
        );
        root.check()?;
        self.check_identity()?;
        let retired = launch.retire_service_existing(retirement).await?;
        root.check()?;
        self.check_identity()?;
        if retired {
            return Err(denied());
        } // Closed tombstone is never restored.
        // Keep the SAME current expiry/authentication denial. No history
        // signature at original issue is substituted for current authority.
        let current = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| denied())?
            .as_secs();
        let public_policy = record.policy(current)?;
        let mode = record.mode(&public_policy)?;
        // Only public signed metadata has been processed so far. The helper's
        // existing-only preflight denies known revoke/future/missing state
        // BEFORE any journal recovery or capsule decryption.
        let mut bootstrap = record.bootstrap.helper()?;
        bootstrap
            .as_object_mut()
            .ok_or_else(denied)?
            .insert("delegation".into(), record.delegation.clone());
        bootstrap.as_object_mut().ok_or_else(denied)?.insert(
            "revision_id".into(),
            serde_json::json!(record.helper_ack.revision_id),
        );
        let process = launch
            .spawn_service_existing(self.registry.clone(), bootstrap)
            .await?;
        if let Err(error) = root.check().and_then(|_| self.check_identity()) {
            process.close().await;
            return Err(error);
        }
        self.original_owner = Some((
            record.bootstrap.owner.owner_id.clone(),
            record.bootstrap.owner.public()?,
            record.bootstrap.owner.tenant.clone(),
        ));
        self.origin = Some(record.bootstrap.origin.clone());
        self.bootstrap = Some(
            serde_json::from_value(serde_json::to_value(&record.bootstrap).map_err(|_| denied())?)
                .map_err(|_| denied())?,
        );
        self.runtime_pin = Some(root);
        self.process = Some(process);
        let verified = self
            .checked_enrollment(
                &record.delegation,
                &record.plans,
                &record.known_hosts,
                &record.capsules,
                &record.targets,
            )
            .await?;
        let ack = self.process.as_ref().ok_or_else(denied)?
            .peer
            .request(
                "service_resume",
                serde_json::json!({"delegation":record.delegation,"revision_id":record.helper_ack.revision_id}), Duration::from_secs(30),
            )
            .await?;
        if ack != serde_json::to_value(&record.helper_ack).map_err(|_| denied())? {
            return Err(denied());
        }
        self.check_identity()?;
        let owner = ServiceExecutionSupervisor::new(
            self.process.as_ref().ok_or_else(denied)?.peer.clone(),
            self.registry.clone(),
            verified,
            self.keys.take().ok_or_else(denied)?,
            record.bootstrap.origin.clone(),
            record.plans,
            record.known_hosts,
            record.capsules,
        )?;
        self.bootstrap = Some(record.bootstrap);
        self.supervisor = Some(owner);
        Ok(Some(mode))
    }
    pub(crate) fn original_origin(&self) -> Result<ServiceExecutionOrigin> {
        self.origin.clone().ok_or_else(denied)
    }
    pub async fn activate_due(
        &mut self,
        mode: JobMode,
    ) -> Result<Option<ServiceExecutionObservation>> {
        self.check_identity()?;
        self.supervisor
            .as_mut()
            .ok_or_else(denied)?
            .activate_due(mode)
            .await
    }
    pub fn control_peer(&self) -> Result<Arc<crate::HelperPeer>> {
        Ok(self.process.as_ref().ok_or_else(denied)?.peer.clone())
    }
    pub(crate) fn owner_status_verifier(
        &self,
    ) -> Result<cc_crypto_core::ServiceOwnerStatusVerifier> {
        self.supervisor
            .as_ref()
            .ok_or_else(denied)?
            .owner_status_verifier()
    }
    /// Original verified public policy only. No template/vars/capsules/private keys.
    pub(crate) fn owner_policy(&self) -> Result<Value> {
        let value = self.supervisor.as_ref().ok_or_else(denied)?.signed()?;
        if serde_json::to_vec(&value).map_err(|_| denied())?.len() > 64 * 1024 {
            return Err(denied());
        }
        Ok(value)
    }
    pub fn cancellation(&self) -> Result<tokio_util::sync::CancellationToken> {
        Ok(self.supervisor.as_ref().ok_or_else(denied)?.cancellation())
    }
    pub async fn shutdown(&mut self) -> bool {
        let closed = match &mut self.supervisor {
            Some(owner) => owner.shutdown().await,
            None => true,
        };
        if let Some(process) = self.process.take() {
            process.close().await;
        }
        self.keys.take();
        self.supervisor.take(); // Zeroize node private keys after the owned close.
        self.identity.take(); // Release the original file lock only after keys and actual close.
        self.original_owner.take();
        self.origin.take();
        self.bootstrap.take();
        self.runtime_pin.take();
        closed
    }
}
impl Drop for ServiceNode {
    fn drop(&mut self) {
        self.registry.cancel_all();
        self.keys.take();
        // Exceptional Drop is NOT a joined-close acknowledgement. Existing
        // JobStore owner/unknown-slot barriers remain mandatory on restart.
        // Destroy all local service signing material before releasing the file
        // lock; never depend on declaration-order field destruction.
        self.supervisor.take();
        self.process.take();
        self.identity.take();
    }
}
