//! Purpose-specific owner policy signatures. No keys, generic signer, job lease
//! or storage resolver is exposed. Native must consume its actual source review.
use crate::{DevicePublicKeys, DeviceSecretKeys, ServiceTenantKind};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const SERVICE_DELEGATION_DOMAIN_V4: &[u8] = b"ConsoleCrypt ServiceDelegation v4\0";
const MAX_WIRE: usize = 64 * 1024;
const MAX_REV: u64 = 9_007_199_254_740_991;
const MAX_TIME: u64 = 253_402_300_799;
type Result<T> = std::result::Result<T, ServiceDelegationError>;
#[derive(Debug, thiserror::Error)]
pub enum ServiceDelegationError {
    #[error("invalid bounded service policy")]
    Invalid,
    #[error("service policy owner or tenant differs")]
    Context,
    #[error("service policy signature rejected")]
    Signature,
    #[error("service policy validity ended")]
    Expired,
}
fn require(ok: bool) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(ServiceDelegationError::Invalid)
    }
}
fn identifier(value: &str) -> bool {
    value.len() == 36
        && Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.to_string() == value)
}
fn revision(value: u64) -> bool {
    (1..=MAX_REV).contains(&value)
}
fn digest(value: &str) -> bool {
    cc_models::automation::is_digest(value)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn canonical(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut fields: Vec<_> = map.into_iter().collect();
            fields.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(
                fields
                    .into_iter()
                    .map(|(key, value)| (key, canonical(value)))
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(values.into_iter().map(canonical).collect()),
        other => other,
    }
}

fn hex_decode_key(value: &str) -> Result<Vec<u8>> {
    require(value.len() == 64)?;
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|p| {
            let n = |c: u8| match c {
                b'0'..=b'9' => Ok(c - b'0'),
                b'a'..=b'f' => Ok(c - b'a' + 10),
                _ => Err(ServiceDelegationError::Invalid),
            };
            Ok(n(p[0])? * 16 + n(p[1])?)
        })
        .collect()
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServicePolicyTenant {
    pub kind: ServiceTenantKind,
    pub id: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServicePolicyKey {
    pub kind: String,
    pub public_key: String,
    pub key_epoch: u64,
    pub certificate_epoch: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServicePolicyHostRef {
    pub id: String,
    pub revision: u64,
    pub target_digest: String,
}
impl ServicePolicyHostRef {
    /// Metadata projection only. Native obtains target from its actual retained
    /// authenticated session, never from a caller manifest or alias expansion.
    pub fn from_target(
        target: &cc_models::automation::ApprovedTarget,
        current_revision: u64,
    ) -> Result<Self> {
        require(revision(current_revision))?;
        let value = serde_json::to_value(target).map_err(|_| ServiceDelegationError::Invalid)?;
        let bytes =
            serde_json::to_vec(&canonical(value)).map_err(|_| ServiceDelegationError::Invalid)?;
        require(bytes.len() <= 16 * 1024)?;
        Ok(Self {
            id: target.host_id.to_string(),
            revision: current_revision,
            target_digest: hex(&Sha256::digest(bytes)),
        })
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServicePolicyCredentialRef {
    pub id: String,
    pub revision: u64,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceRouteInputRef {
    pub id: String,
    pub revision: u64,
    pub kind: cc_models::ObjectKind,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceRouteInputs {
    pub dependencies: Vec<ServiceRouteInputRef>,
    pub key_trust: Vec<ServiceRouteInputRef>,
}
impl ServiceRouteInputs {
    fn validate(&self) -> Result<()> {
        require(
            !self.dependencies.is_empty()
                && !self.key_trust.is_empty()
                && self.dependencies.len() + self.key_trust.len() <= 512,
        )?;
        for (rows, trust) in [(&self.dependencies, false), (&self.key_trust, true)] {
            let mut previous = "";
            for row in rows {
                require(
                    identifier(&row.id) && row.id.as_str() > previous && revision(row.revision),
                )?;
                if trust {
                    require(row.kind == cc_models::ObjectKind::KnownHost)?;
                } else {
                    require(matches!(
                        row.kind,
                        cc_models::ObjectKind::Host
                            | cc_models::ObjectKind::Group
                            | cc_models::ObjectKind::JumpProfile
                            | cc_models::ObjectKind::Proxy
                            | cc_models::ObjectKind::Credential
                            | cc_models::ObjectKind::Secret
                            | cc_models::ObjectKind::Tunnel
                    ))?;
                }
                previous = &row.id;
            }
        }
        require(
            self.key_trust
                .iter()
                .all(|row| !self.dependencies.iter().any(|r| r.id == row.id)),
        )
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServicePolicyQuotas {
    pub max_jobs: u16,
    pub max_per_utc_day: u16,
    pub max_concurrent: u8,
    pub max_job_seconds: u32,
    pub max_forks: u8,
}
/// DATA selected from an original Native PreparedReview. A signature needs a
/// separate explicit owner action; constructing/decoding this is no approval.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceAccessMode {
    FreshOwnerHead,
    BoundedOffline,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceAccessPolicy {
    pub mode: ServiceAccessMode,
    // Required even for fresh mode, whose deadline is explicitly null.
    #[serde(deserialize_with = "required_offline_deadline")]
    pub offline_not_after: Option<u64>,
}
fn required_offline_deadline<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<u64>, D::Error> {
    Option::<u64>::deserialize(deserializer)
}
impl ServiceAccessPolicy {
    fn validate(&self, expires_at: u64) -> Result<()> {
        require(match self.mode {
            ServiceAccessMode::FreshOwnerHead => self.offline_not_after.is_none(),
            ServiceAccessMode::BoundedOffline => self.offline_not_after == Some(expires_at),
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceDelegationPolicy {
    pub version: u8,
    pub id: String,
    pub owner_id: String,
    pub owner_epoch: u64,
    pub tenant: ServicePolicyTenant,
    pub service_id: String,
    pub signer: ServicePolicyKey,
    pub recipient: ServicePolicyKey,
    pub project_id: String,
    pub project_revision: u64,
    pub template_digest: String,
    pub playbook: String,
    pub schedule_id: String,
    pub schedule_revision: u64,
    pub definition_digest: String,
    pub inventory_digest: String,
    pub parameters_digest: String,
    pub environment_digest: String,
    pub hosts: Vec<ServicePolicyHostRef>,
    pub credentials: Vec<ServicePolicyCredentialRef>,
    pub route_inputs: ServiceRouteInputs,
    pub modes: Vec<String>,
    pub quotas: ServicePolicyQuotas,
    pub policy_revision: u64,
    pub revocation_epoch: u64,
    pub issued_at: u64,
    pub expires_at: u64,
    pub desktop_lock_policy: String,
    pub access_policy: ServiceAccessPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_origin: Option<crate::ServiceExecutionOrigin>,
}
impl ServiceDelegationPolicy {
    pub fn validate(
        &self,
        owner_id: &str,
        owner: &DevicePublicKeys,
        tenant: &ServicePolicyTenant,
        now: u64,
    ) -> Result<()> {
        require(self.version == 4 && self.desktop_lock_policy == "independentDelegatedService")?;
        if let Some(origin) = &self.execution_origin {
            origin.validate()?;
            require(
                origin.service_id == self.service_id
                    && origin.environment_digest == self.environment_digest
                    && origin.signer_key_sha256
                        == hex(&Sha256::digest(hex_decode_key(&self.signer.public_key)?))
                    && origin.recipient_key_sha256
                        == hex(&Sha256::digest(hex_decode_key(&self.recipient.public_key)?)),
            )?;
        }
        for id in [
            &self.id,
            &self.owner_id,
            &self.tenant.id,
            &self.service_id,
            &self.project_id,
            &self.schedule_id,
        ] {
            require(identifier(id))?;
        }
        require(self.tenant.kind != ServiceTenantKind::Team)?;
        if self.owner_id != owner_id || &self.tenant != tenant {
            return Err(ServiceDelegationError::Context);
        }
        for value in [
            self.owner_epoch,
            self.project_revision,
            self.schedule_revision,
            self.policy_revision,
            self.revocation_epoch,
        ] {
            require(revision(value))?;
        }
        for value in [
            &self.template_digest,
            &self.definition_digest,
            &self.inventory_digest,
            &self.parameters_digest,
            &self.environment_digest,
        ] {
            require(digest(value))?;
        }
        cc_models::automation::validate_selected_playbook(&self.playbook)
            .map_err(|_| ServiceDelegationError::Invalid)?;
        for (key, kind) in [(&self.signer, "ed25519"), (&self.recipient, "x25519")] {
            require(
                key.kind == kind
                    && digest(&key.public_key)
                    && revision(key.key_epoch)
                    && revision(key.certificate_epoch),
            )?;
        }
        require(
            self.signer.public_key != self.recipient.public_key
                && self.signer.public_key != hex(&owner.signing)
                && self.recipient.public_key != hex(&owner.signing)
                && self.recipient.public_key != hex(&owner.encryption),
        )?;
        require(!self.hosts.is_empty() && self.hosts.len() <= 64 && self.credentials.len() <= 64)?;
        let mut previous = "";
        for host in &self.hosts {
            require(
                identifier(&host.id)
                    && host.id.as_str() > previous
                    && revision(host.revision)
                    && digest(&host.target_digest),
            )?;
            previous = &host.id;
        }
        previous = "";
        for credential in &self.credentials {
            require(
                identifier(&credential.id)
                    && credential.id.as_str() > previous
                    && revision(credential.revision),
            )?;
            previous = &credential.id;
        }
        self.route_inputs.validate()?;
        require(self.hosts.iter().all(|host| {
            self.route_inputs.dependencies.iter().any(|r| {
                r.id == host.id
                    && r.revision == host.revision
                    && r.kind == cc_models::ObjectKind::Host
            })
        }))?;
        require(self.credentials.iter().all(|credential| {
            self.route_inputs.dependencies.iter().any(|r| {
                r.id == credential.id
                    && r.revision == credential.revision
                    && r.kind == cc_models::ObjectKind::Credential
            })
        }))?;
        require(
            self.modes == ["check"] || self.modes == ["apply"] || self.modes == ["apply", "check"],
        )?;
        require(
            (1..=128).contains(&self.quotas.max_jobs)
                && (1..=self.quotas.max_jobs).contains(&self.quotas.max_per_utc_day)
                && self.quotas.max_concurrent == 1
                && (1..=3600).contains(&self.quotas.max_job_seconds)
                && (1..=5).contains(&self.quotas.max_forks),
        )?;
        if now > MAX_TIME
            || self.issued_at > now
            || now >= self.expires_at
            || self.expires_at
                > self
                    .issued_at
                    .checked_add(86400)
                    .ok_or(ServiceDelegationError::Expired)?
            || self.expires_at > MAX_TIME
        {
            return Err(ServiceDelegationError::Expired);
        }
        self.access_policy.validate(self.expires_at)?;
        self.canonical_bytes()?; // Preview and signature use the SAME exact budget.
        Ok(())
    }
    fn canonical_bytes(&self) -> Result<Vec<u8>> {
        let value = serde_json::to_value(self).map_err(|_| ServiceDelegationError::Invalid)?;
        let body =
            serde_json::to_vec(&canonical(value)).map_err(|_| ServiceDelegationError::Invalid)?;
        require(body.len() < MAX_WIRE - 256)?;
        Ok(body)
    }
    fn message(&self) -> Result<Vec<u8>> {
        let mut bytes = SERVICE_DELEGATION_DOMAIN_V4.to_vec();
        bytes.extend(self.canonical_bytes()?);
        Ok(bytes)
    }
}
impl std::fmt::Debug for ServiceDelegationPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ServiceDelegationPolicy([METADATA REDACTED])")
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedServiceDelegation {
    body: ServiceDelegationPolicy,
    signature: String,
}
/// Real signature evidence only. Non-Clone/non-Deserialize: a caller cannot
/// turn decoded policy metadata into this type without actual owner checks.
/// It still grants no job/secret/route by itself; fresh head/slot required.
pub struct VerifiedServiceDelegation {
    signed: SignedServiceDelegation,
    owner_signing: [u8; 32],
}
impl VerifiedServiceDelegation {
    pub fn signed_json(&self) -> Result<Vec<u8>> {
        self.signed.to_json()
    }
    pub fn owner_signing_key(&self) -> &[u8; 32] {
        &self.owner_signing
    }
    pub fn body(&self) -> &ServiceDelegationPolicy {
        &self.signed.body
    }
    pub fn digest(&self) -> Result<String> {
        self.signed.digest()
    }
}
impl std::fmt::Debug for VerifiedServiceDelegation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VerifiedServiceDelegation([METADATA REDACTED])")
    }
}
impl SignedServiceDelegation {
    pub fn to_json(&self) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec(self).map_err(|_| ServiceDelegationError::Invalid)?;
        require(bytes.len() <= MAX_WIRE)?;
        Ok(bytes)
    }
    pub fn body(&self) -> &ServiceDelegationPolicy {
        &self.body
    }
    pub fn digest(&self) -> Result<String> {
        Ok(hex(&Sha256::digest(self.body.message()?)))
    }
    pub fn verify_json(
        raw: &[u8],
        owner_id: &str,
        owner: &DevicePublicKeys,
        tenant: &ServicePolicyTenant,
        now: u64,
    ) -> Result<VerifiedServiceDelegation> {
        require(raw.len() <= MAX_WIRE)?;
        let signed: Self =
            serde_json::from_slice(raw).map_err(|_| ServiceDelegationError::Invalid)?;
        signed.body.validate(owner_id, owner, tenant, now)?;
        let bytes = STANDARD
            .decode(&signed.signature)
            .map_err(|_| ServiceDelegationError::Signature)?;
        let bytes: [u8; 64] = bytes
            .try_into()
            .map_err(|_| ServiceDelegationError::Signature)?;
        VerifyingKey::from_bytes(&owner.signing)
            .map_err(|_| ServiceDelegationError::Signature)?
            .verify_strict(&signed.body.message()?, &Signature::from_bytes(&bytes))
            .map_err(|_| ServiceDelegationError::Signature)?;
        Ok(VerifiedServiceDelegation {
            signed,
            owner_signing: owner.signing,
        })
    }
}
impl std::fmt::Debug for SignedServiceDelegation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SignedServiceDelegation([METADATA REDACTED])")
    }
}
impl DeviceSecretKeys {
    /// Fixed v4 purpose only; Native must consume the actual reviewed policy.
    pub fn sign_service_delegation(
        &self,
        policy: ServiceDelegationPolicy,
        owner_id: &str,
        tenant: &ServicePolicyTenant,
        now: u64,
    ) -> Result<SignedServiceDelegation> {
        policy.validate(owner_id, &self.public_keys(), tenant, now)?;
        let signature = STANDARD.encode(self.sign_message(&policy.message()?));
        Ok(SignedServiceDelegation {
            body: policy,
            signature,
        })
    }
}

#[cfg(test)]
mod tests;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InitialServiceStatusBody {
    version: u8,
    owner_id: String,
    owner_epoch: u64,
    tenant: ServicePolicyTenant,
    service_id: String,
    delegation_digest: String,
    policy_revision: u64,
    revocation_epoch: u64,
    signer_epoch: u64,
    recipient_epoch: u64,
    certificate_epoch: u64,
    recipient_certificate_epoch: u64,
    revoked: bool,
    issued_at: u64,
    expires_at: u64,
}
/// Initial enrollment observation only, fixed to the original policy issuance.
/// No Deserialize/constructor/renewal or generic owner statement signer.
#[derive(Serialize)]
pub struct SignedInitialServiceStatus {
    body: InitialServiceStatusBody,
    signature: String,
}
impl SignedInitialServiceStatus {
    pub fn to_json(&self) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec(self).map_err(|_| ServiceDelegationError::Invalid)?;
        require(bytes.len() <= 4096)?;
        Ok(bytes)
    }
}
impl DeviceSecretKeys {
    pub fn sign_initial_service_status(
        &self,
        signed: &SignedServiceDelegation,
        owner_id: &str,
        tenant: &ServicePolicyTenant,
        now: u64,
    ) -> Result<SignedInitialServiceStatus> {
        let verified = SignedServiceDelegation::verify_json(
            &signed.to_json()?,
            owner_id,
            &self.public_keys(),
            tenant,
            now,
        )?;
        let policy = verified.body();
        let expires_at = policy.expires_at.min(
            policy
                .issued_at
                .checked_add(300)
                .ok_or(ServiceDelegationError::Expired)?,
        );
        require(now < expires_at)?;
        let body = InitialServiceStatusBody {
            version: 1,
            owner_id: policy.owner_id.clone(),
            owner_epoch: policy.owner_epoch,
            tenant: policy.tenant.clone(),
            service_id: policy.service_id.clone(),
            delegation_digest: verified.digest()?,
            policy_revision: policy.policy_revision,
            revocation_epoch: policy.revocation_epoch,
            signer_epoch: policy.signer.key_epoch,
            recipient_epoch: policy.recipient.key_epoch,
            certificate_epoch: policy.signer.certificate_epoch,
            recipient_certificate_epoch: policy.recipient.certificate_epoch,
            revoked: false,
            issued_at: policy.issued_at,
            expires_at,
        };
        let message = owner_status_message(&body)?;
        Ok(SignedInitialServiceStatus {
            body,
            signature: STANDARD.encode(self.sign_message(&message)),
        })
    }
}

/// Fixed revoked=true original-owner statement; never an authorization renewal.
#[derive(Serialize)]
pub struct SignedServiceRevocation { body: InitialServiceStatusBody, signature: String }
impl SignedServiceRevocation {
    pub fn to_json(&self)->Result<Vec<u8>> {
        let bytes=serde_json::to_vec(self).map_err(|_|ServiceDelegationError::Invalid)?;
        require(bytes.len()<=4096)?; Ok(bytes)
    }
}
impl DeviceSecretKeys {
    /// AppCore supplies actual persisted floors after explicit owner action.
    /// No arbitrary message, revoked flag, owner, key, tenant or JSON input.
    pub fn sign_service_revocation(&self, signed:&SignedServiceDelegation, owner_id:&str,
        policy_revision:u64, revocation_epoch:u64, clock_floor:u64, now:u64)->Result<SignedServiceRevocation> {
        let p=signed.body();
        let verified=SignedServiceDelegation::verify_json(&signed.to_json()?,owner_id,&self.public_keys(),&p.tenant,p.issued_at)?;
        let p=verified.body();
        require(revision(policy_revision) && policy_revision>=p.policy_revision
            && revision(revocation_epoch) && revocation_epoch>p.revocation_epoch
            && now>=clock_floor && now<=MAX_TIME)?;
        let expires=now.checked_add(300).ok_or(ServiceDelegationError::Expired)?;
        require(expires<=MAX_TIME)?;
        let body=InitialServiceStatusBody { version:1, owner_id:p.owner_id.clone(),owner_epoch:p.owner_epoch,
            tenant:p.tenant.clone(),service_id:p.service_id.clone(),delegation_digest:verified.digest()?,
            policy_revision,revocation_epoch,signer_epoch:p.signer.key_epoch,recipient_epoch:p.recipient.key_epoch,
            certificate_epoch:p.signer.certificate_epoch,recipient_certificate_epoch:p.recipient.certificate_epoch,
            revoked:true,issued_at:now,expires_at:expires };
        let signature=STANDARD.encode(self.sign_message(&owner_status_message(&body)?));
        Ok(SignedServiceRevocation{body,signature})
    }
}

// Cancellation observation only. It can neither sign nor renew any policy,
// job, lease or material grant, and is constructed only from genuine verification.
pub struct ServiceOwnerStatusVerifier {
    owner_signing: [u8;32],
    original: InitialServiceStatusBody,
}
pub struct VerifiedServiceOwnerStatus { invalidates_original: bool }
impl VerifiedServiceOwnerStatus {
    pub fn invalidates_original(&self)->bool { self.invalidates_original }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnerStatusWire { body:InitialServiceStatusBody, signature:String }
impl VerifiedServiceDelegation {
    pub fn owner_status_verifier(&self)->Result<ServiceOwnerStatusVerifier> {
        let p=self.body();
        Ok(ServiceOwnerStatusVerifier { owner_signing:self.owner_signing,
            original:InitialServiceStatusBody { version:1, owner_id:p.owner_id.clone(), owner_epoch:p.owner_epoch,
                tenant:p.tenant.clone(), service_id:p.service_id.clone(), delegation_digest:self.digest()?,
                policy_revision:p.policy_revision, revocation_epoch:p.revocation_epoch,
                signer_epoch:p.signer.key_epoch, recipient_epoch:p.recipient.key_epoch,
                certificate_epoch:p.signer.certificate_epoch, recipient_certificate_epoch:p.recipient.certificate_epoch,
                revoked:false, issued_at:p.issued_at, expires_at:p.expires_at } })
    }
}
fn owner_status_message(body:&InitialServiceStatusBody)->Result<Vec<u8>> {
    let mut message=b"ConsoleCrypt ServiceTrustHead v1\0".to_vec();
    message.extend(serde_json::to_vec(&canonical(serde_json::to_value(body).map_err(|_|ServiceDelegationError::Invalid)?))
        .map_err(|_|ServiceDelegationError::Invalid)?);
    Ok(message)
}
impl ServiceOwnerStatusVerifier {
    pub fn verify_json(&self,raw:&[u8],now:u64)->Result<VerifiedServiceOwnerStatus> {
        require(raw.len()<=4096)?;
        let wire:OwnerStatusWire=serde_json::from_slice(raw).map_err(|_|ServiceDelegationError::Invalid)?;
        let b=&wire.body;let p=&self.original;
        require(b.version==1 && b.owner_id==p.owner_id && b.owner_epoch==p.owner_epoch && b.tenant==p.tenant
            && b.service_id==p.service_id && b.delegation_digest==p.delegation_digest
            && revision(b.policy_revision) && b.policy_revision>=p.policy_revision
            && revision(b.revocation_epoch) && b.revocation_epoch>=p.revocation_epoch
            && b.signer_epoch==p.signer_epoch && b.recipient_epoch==p.recipient_epoch
            && b.certificate_epoch==p.certificate_epoch && b.recipient_certificate_epoch==p.recipient_certificate_epoch)?;
        if now>MAX_TIME || b.issued_at>now || now>=b.expires_at || b.expires_at>MAX_TIME
            || b.expires_at>b.issued_at.checked_add(300).ok_or(ServiceDelegationError::Expired)? {
            return Err(ServiceDelegationError::Expired);
        }
        let signature:[u8;64]=STANDARD.decode(&wire.signature).map_err(|_|ServiceDelegationError::Signature)?
            .try_into().map_err(|_|ServiceDelegationError::Signature)?;
        VerifyingKey::from_bytes(&self.owner_signing).map_err(|_|ServiceDelegationError::Signature)?
            .verify_strict(&owner_status_message(b)?,&Signature::from_bytes(&signature))
            .map_err(|_|ServiceDelegationError::Signature)?;
        Ok(VerifiedServiceOwnerStatus { invalidates_original:b.revoked || b.policy_revision>p.policy_revision || b.revocation_epoch>p.revocation_epoch })
    }
}
