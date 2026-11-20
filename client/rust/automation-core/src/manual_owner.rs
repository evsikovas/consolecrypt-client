//! Manual-owner configuration is a distinct fixed operation, not a transport
//! label or a Boolean copied from a grant/manifest. Service endpoints deny it.
use crate::{AutomationError, EnrolledJobGrant, HelperPeer, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::json;
use std::{
    sync::{Arc, Weak},
    time::Duration,
};

pub struct ManualOwnerGrant {
    grant: EnrolledJobGrant,
    original_peer: Weak<HelperPeer>,
}

#[cfg(test)]
mod tests;
impl std::fmt::Debug for ManualOwnerGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ManualOwnerGrant(<original interactive scope>)")
    }
}
impl ManualOwnerGrant {
    /// Rust-only Native caller must have independently captured the ORIGINAL
    /// manual Session/DeviceIdentity/HelperPeer and checked the signer pin. This
    /// never enrolls/replaces/renews a grant. It promotes only the exact grant
    /// already configured on this original manual interactive endpoint.
    pub async fn configure(
        original_peer: &Arc<HelperPeer>,
        grant: EnrolledJobGrant,
    ) -> Result<Self> {
        if original_peer.is_closed() || !grant.jobs_execute || grant.signer_id.is_nil() {
            return Err(AutomationError::Invalid(
                "original manual owner scope unavailable",
            ));
        }
        let weak = Arc::downgrade(original_peer);
        let response = original_peer.request("configure_manual_owner", json!({
            "tenant_id":grant.tenant_id,"runner_id":grant.runner_id,"signer_id":grant.signer_id,
            "public_key":STANDARD.encode(grant.public_key),"hosts":grant.hosts,"projects":grant.projects,
            "expires_at":grant.expires_at,"max_job_seconds":grant.max_job_seconds,
            "transport":"native_scoped_rpc_v1"
        }),Duration::from_secs(10)).await?;
        if original_peer.is_closed()
            || response
                != json!({"configured":true,"persistent":false,
            "owner_policy":"ownerConfirmationVaultPassphraseV1"})
        {
            return Err(AutomationError::Invalid(
                "manual owner configuration unavailable",
            ));
        }
        Ok(Self {
            grant,
            original_peer: weak,
        })
    }
    pub fn matches_enrolled(&self, current: &EnrolledJobGrant) -> bool {
        if self
            .original_peer
            .upgrade()
            .is_none_or(|peer| peer.is_closed())
        {
            return false;
        }
        let g = &self.grant;
        g.signer_id == current.signer_id
            && g.public_key == current.public_key
            && g.tenant_id == current.tenant_id
            && g.runner_id == current.runner_id
            && g.projects == current.projects
            && g.hosts == current.hosts
            && g.jobs_execute == current.jobs_execute
            && g.expires_at == current.expires_at
            && g.max_job_seconds == current.max_job_seconds
    }
    pub(crate) fn original(&self) -> Result<&EnrolledJobGrant> {
        if self
            .original_peer
            .upgrade()
            .is_none_or(|peer| peer.is_closed())
        {
            return Err(AutomationError::Invalid(
                "original manual owner peer retired",
            ));
        }
        Ok(&self.grant)
    }
    pub(crate) fn clone_original(&self) -> Self {
        Self {
            grant: self.grant.clone(),
            original_peer: self.original_peer.clone(),
        }
    }
}
