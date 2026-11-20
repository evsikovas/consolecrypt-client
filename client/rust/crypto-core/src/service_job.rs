//! Fixed service scheduling signer. Slot metadata is never job authority.
use crate::{DeviceSecretKeys, ServiceDelegationError, VerifiedServiceDelegation};
use base64::{Engine, engine::general_purpose::STANDARD};
use cc_models::automation::{
    ApprovedTarget, JOB_SIGNATURE_DOMAIN, JobManifest, JobMode, JobPolicy,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

type Result<T> = std::result::Result<T, ServiceDelegationError>;
fn require(ok: bool) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(ServiceDelegationError::Invalid)
    }
}
fn canonical(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut rows = map.into_iter().collect::<Vec<_>>();
            rows.sort_by(|a, b| a.0.cmp(&b.0));
            serde_json::Value::Object(rows.into_iter().map(|(k, v)| (k, canonical(v))).collect())
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(canonical).collect())
        }
        v => v,
    }
}
fn bytes<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(&canonical(
        serde_json::to_value(value).map_err(|_| ServiceDelegationError::Invalid)?,
    ))
    .map_err(|_| ServiceDelegationError::Invalid)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceScheduledSlot {
    pub schedule_id: Uuid,
    pub schedule_revision: u64,
    pub definition_digest: String,
    pub local_slot: String,
    pub due_utc: u64,
    pub grace_seconds: u32,
}
#[derive(Serialize)]
pub struct ServiceJobApproval {
    pub signer_id: Uuid,
    pub manifest: JobManifest,
    pub signature: String,
}
#[derive(Serialize)]
pub struct SignedServiceScheduledJob {
    pub approval: ServiceJobApproval,
    pub slot: serde_json::Value,
}
impl DeviceSecretKeys {
    /// Caller obtains the original slot from the SAME scheduler and repeats its
    /// durable floors at actual activation. This signs only the owner-approved
    /// playbook/inputs/targets/caps and cannot choose arbitrary manifest content.
    pub fn sign_service_scheduled_job(
        &self,
        delegation: &VerifiedServiceDelegation,
        slot: &ServiceScheduledSlot,
        targets: Vec<ApprovedTarget>,
        mode: JobMode,
        capsule_deadline: u64,
        now: u64,
    ) -> Result<SignedServiceScheduledJob> {
        let p = delegation.body();
        let origin = p
            .execution_origin
            .as_ref()
            .ok_or(ServiceDelegationError::Context)?;
        origin.validate()?;
        let public = self.public_keys();
        let key = |raw: &[u8]| raw.iter().map(|b| format!("{b:02x}")).collect::<String>();
        require(
            p.signer.public_key == key(&public.signing)
                && p.recipient.public_key == key(&public.encryption)
                && slot.schedule_id.to_string() == p.schedule_id
                && slot.schedule_revision == p.schedule_revision
                && slot.definition_digest == p.definition_digest
                && slot.grace_seconds <= 60
                && slot.due_utc <= now
                && now
                    <= slot
                        .due_utc
                        .checked_add(u64::from(slot.grace_seconds))
                        .ok_or(ServiceDelegationError::Expired)?
                && p.issued_at <= now
                && now < p.expires_at
                && now < capsule_deadline
                && !targets.is_empty()
                && targets.len() == p.hosts.len()
                && targets.len() <= 32,
        )?;
        // Scheduler owns timezone/date interpretation. This rejects alternate
        // serialization; it does not create a second calendar implementation.
        require(
            slot.local_slot.len() == 16
                && slot.local_slot.is_ascii()
                && slot.local_slot.bytes().enumerate().all(|(i, b)| match i {
                    4 | 7 => b == b'-',
                    10 => b == b'T',
                    13 => b == b':',
                    _ => b.is_ascii_digit(),
                }),
        )?;
        let selected = match mode {
            JobMode::Check => "check",
            JobMode::Apply => "apply",
        };
        require(p.modes.iter().any(|m| m == selected))?;
        let mut ids = std::collections::BTreeSet::new();
        for target in &targets {
            require(ids.insert(target.host_id))?;
            let hash = key(&Sha256::digest(bytes(target)?));
            require(
                p.hosts
                    .iter()
                    .any(|h| h.id == target.host_id.to_string() && h.target_digest == hash),
            )?;
        }
        let mut identity = b"ConsoleCrypt ServiceLogicalSlotJobId v1\0".to_vec();
        identity.extend(bytes(
            &serde_json::json!({"tenant":p.tenant,"serviceId":p.service_id,
            "projectId":p.project_id,"scheduleId":p.schedule_id,"localSlot":slot.local_slot}),
        )?);
        let mut id: [u8; 16] = Sha256::digest(identity)[..16]
            .try_into()
            .map_err(|_| ServiceDelegationError::Invalid)?;
        id[6] = (id[6] & 15) | 128;
        id[8] = (id[8] & 63) | 128;
        let timeout = u64::from(p.quotas.max_job_seconds)
            .min(p.expires_at - now)
            .min(capsule_deadline - now)
            .min(300);
        require(timeout > 0 && timeout <= 3600)?;
        let deadline = now
            .checked_add(timeout)
            .ok_or(ServiceDelegationError::Expired)?;
        let submission = deadline.min(
            now.checked_add(300)
                .ok_or(ServiceDelegationError::Expired)?,
        );
        let manifest = JobManifest {
            version: 1,
            job_id: Uuid::from_bytes(id),
            tenant_id: p
                .tenant
                .id
                .parse()
                .map_err(|_| ServiceDelegationError::Context)?,
            runner_id: p
                .service_id
                .parse()
                .map_err(|_| ServiceDelegationError::Context)?,
            project_id: p
                .project_id
                .parse()
                .map_err(|_| ServiceDelegationError::Context)?,
            revision_digest: p.template_digest.clone(),
            inventory_digest: p.inventory_digest.clone(),
            environment_digest: p.environment_digest.clone(),
            parameters_digest: p.parameters_digest.clone(),
            playbook: p.playbook.clone(),
            mode,
            targets,
            policy: JobPolicy {
                strict_isolation: true,
                cancel_on_lock: false,
                timeout_seconds: timeout as u32,
                forks: p.quotas.max_forks,
                production: false,
            },
            issued_at: now
                .try_into()
                .map_err(|_| ServiceDelegationError::Expired)?,
            expires_at: submission
                .try_into()
                .map_err(|_| ServiceDelegationError::Expired)?,
            access_expires_at: deadline
                .try_into()
                .map_err(|_| ServiceDelegationError::Expired)?,
            nonce: Uuid::new_v4(),
        };
        let signature = STANDARD.encode(
            self.sign_automation_manifest(&manifest)
                .map_err(|_| ServiceDelegationError::Invalid)?,
        );
        let mut digest = JOB_SIGNATURE_DOMAIN.to_vec();
        digest.extend(
            manifest
                .canonical_bytes()
                .map_err(|_| ServiceDelegationError::Invalid)?,
        );
        let manifest_digest = key(&Sha256::digest(digest));
        let grant = serde_json::json!({"version":1,"tenant":p.tenant,"service_id":p.service_id,
            "delegation_digest":delegation.digest()?,"policy_revision":p.policy_revision,"revocation_epoch":p.revocation_epoch,
            "project_id":p.project_id,"project_revision":p.project_revision,"template_digest":p.template_digest,
            "hosts":p.hosts,"credentials":p.credentials,"schedule_id":p.schedule_id,"schedule_revision":p.schedule_revision,
            "definition_digest":p.definition_digest,"local_slot":slot.local_slot,"due_utc":slot.due_utc,
            "job_id":manifest.job_id,"manifest_digest":manifest_digest,"nonce":Uuid::new_v4(),"issued_at":now,"expires_at":submission});
        let mut message = b"ConsoleCrypt ScheduledJobGrant v1\0".to_vec();
        message.extend(bytes(&grant)?);
        let slot_signature = STANDARD.encode(self.sign_message(&message));
        Ok(SignedServiceScheduledJob {
            approval: ServiceJobApproval {
                signer_id: manifest.runner_id,
                manifest,
                signature,
            },
            slot: serde_json::json!({"body":grant,"signature":slot_signature}),
        })
    }
}
