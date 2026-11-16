//! Service-purpose use of existing ORIGINAL authentication provenance only.
//! No Reader context/key/grant, account token, signer or request owner fields.
use crate::agent_reader::{OriginalSessionAuth, OriginalSessionProvenance};
use crate::session::{Session, Unlocked};
use crate::{AppCore, AppError, AppResult};
use cc_crypto_core::{ServicePolicyTenant, ServiceTenantKind};
use cc_storage_core::{Profile, ProfileKind, ServicePersonalOwnerPin};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Weak};
use std::time::Duration;

fn invalid() -> AppError {
    AppError::invalid("service_owner", "service_original_owner_unavailable")
}

pub(crate) fn origin_fingerprint(profile: &Profile) -> AppResult<String> {
    let raw = profile.server_url.as_deref().ok_or_else(invalid)?;
    if raw.len() > 4096 {
        return Err(invalid());
    }
    let canonical = url::Url::parse(raw.trim())
        .map_err(|_| invalid())?
        .to_string();
    let mut hash = Sha256::new();
    hash.update(b"ConsoleCrypt ServiceOwnerOrigin v1\0");
    hash.update(canonical.as_bytes());
    Ok(hex::encode(hash.finalize()))
}

/// Original SQL-derived pin for stale DATA/denial only. It cannot create fresh
/// ownership evidence or a credential/delegation review. Matching an existing
/// ledger pin is mandatory for a local block; no row is created from this.
pub(crate) fn metadata_personal_pin(
    profile: &Profile,
    u: &Arc<Unlocked>,
) -> AppResult<Option<ServicePersonalOwnerPin>> {
    match profile.kind {
        ProfileKind::Local if profile.user_id.is_none() && profile.server_url.is_none() => Ok(None),
        ProfileKind::Synced => {
            let user = profile
                .user_id
                .filter(|id| *id != cc_protocol::UserId::NIL)
                .ok_or_else(invalid)?;
            Ok(Some(ServicePersonalOwnerPin {
                user_id: user,
                server_origin_fingerprint: origin_fingerprint(profile)?,
                vault_commitment: hex::encode(u.vault()?.key_commitment()),
            }))
        }
        _ => Err(invalid()),
    }
}

pub(crate) struct OwnerScope {
    auth: OriginalSessionAuth,
    provenance: OriginalSessionProvenance,
    profile: Profile,
    tenant: ServicePolicyTenant,
    personal: Option<ServicePersonalOwnerPin>,
    unlock: Weak<Unlocked>,
}

impl OwnerScope {
    /// SAME captured authentication/provenance retirement, not a new owner.
    pub(crate) async fn cancelled(&self) {
        tokio::select! {biased;
            _=self.auth.cancelled()=>{},
            _=self.provenance.cancelled()=>{},
        }
    }
    /// No strong Unlocked or profile/unlock guard is retained across account
    /// I/O. The actual existing /auth/me +personal vault verifier is invoked.
    pub(crate) async fn capture(
        app: &AppCore,
        owner: &Arc<Session>,
        weak: Weak<Unlocked>,
    ) -> AppResult<Arc<Self>> {
        let auth = OriginalSessionAuth::capture(app)?;
        let profile = owner.profile().await?;
        let u = weak.upgrade().ok_or(AppError::VaultLocked)?;
        if owner.profile_id != profile.profile_id
            || owner.device_id() != profile.device_id
            || owner.require_vault_id()? != u.vault_id
            || *u.sharing_shutdown.borrow()
        {
            return Err(invalid());
        }
        let personal = metadata_personal_pin(&profile, &u)?;
        let vault = u.vault_id;
        let mut shutdown = u.sharing_shutdown.subscribe();
        drop(u);
        let provenance = tokio::select! { biased;
            _=auth.cancelled()=>return Err(invalid()),
            _=shutdown.changed()=>return Err(AppError::VaultLocked),
            _=owner.automation.stopped.cancelled()=>return Err(invalid()),
            _=tokio::time::sleep(Duration::from_secs(30))=>return Err(invalid()),
            result=OriginalSessionProvenance::capture(owner,&profile,vault)=>result?,
        };
        let tenant = match (&personal, provenance.personal_user_id()) {
            (None, None) => ServicePolicyTenant {
                kind: ServiceTenantKind::LocalProfile,
                id: owner.profile_id.to_string(),
            },
            (Some(pin), Some(user)) if pin.user_id == user => ServicePolicyTenant {
                kind: ServiceTenantKind::PersonalAccount,
                id: user.to_string(),
            },
            _ => return Err(invalid()),
        };
        let result = Arc::new(Self {
            auth,
            provenance,
            profile,
            tenant,
            personal,
            unlock: weak.clone(),
        });
        // SQL comes FIRST; final original checks follow its await.
        let current = owner.profile().await?;
        result.check_profile(&current)?;
        let u = weak.upgrade().ok_or(AppError::VaultLocked)?;
        result.check(owner, &u)?;
        Ok(result)
    }
    pub(crate) fn tenant(&self) -> &ServicePolicyTenant {
        &self.tenant
    }
    pub(crate) fn personal_pin(&self) -> Option<ServicePersonalOwnerPin> {
        self.personal.clone()
    }
    pub(crate) fn check_profile(&self, current: &Profile) -> AppResult<()> {
        if current.profile_id != self.profile.profile_id
            || current.device_id != self.profile.device_id
            || current.kind != self.profile.kind
            || current.user_id != self.profile.user_id
        {
            return Err(invalid());
        }
        match &self.personal {
            None if current.server_url.is_none() => Ok(()),
            Some(pin) if origin_fingerprint(current)? == pin.server_origin_fingerprint => Ok(()),
            _ => Err(invalid()),
        }
    }
    pub(crate) fn check(&self, owner: &Arc<Session>, u: &Unlocked) -> AppResult<()> {
        let original = self.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        if !std::ptr::eq(u, original.as_ref()) {
            return Err(invalid());
        }
        if !self.auth.is_current()
            || !self.provenance.is_current(owner)
            || owner.profile_id != self.profile.profile_id
            || owner.device_id() != self.profile.device_id
            || owner.require_vault_id()? != u.vault_id
            || *u.sharing_shutdown.borrow()
            || owner.automation.stopped.is_cancelled()
        {
            return Err(invalid());
        }
        self.provenance.check_vault(&original)
    }
    pub(crate) fn with_current<T>(
        &self,
        owner: &Arc<Session>,
        f: impl FnOnce() -> AppResult<T>,
    ) -> AppResult<T> {
        // SAME order as existing Agent Tunnel provenance; no recursive auth lock.
        self.auth
            .with_current(|| self.provenance.with_current(owner, f))
    }
    pub(crate) async fn verify(&self, owner: &Arc<Session>, weak: Weak<Unlocked>) -> AppResult<()> {
        let u = weak.upgrade().ok_or(AppError::VaultLocked)?;
        self.check(owner, &u)?;
        let mut shutdown = u.sharing_shutdown.subscribe();
        drop(u);
        let work = async {
            let profile = owner.profile().await?;
            self.check_profile(&profile)?;
            self.provenance.verify(owner, &profile).await?;
            let profile = owner.profile().await?;
            self.check_profile(&profile)?;
            let u = weak.upgrade().ok_or(AppError::VaultLocked)?;
            self.check(owner, &u)
        };
        tokio::select! { biased;
            _=self.auth.cancelled()=>Err(invalid()),
            _=shutdown.changed()=>Err(AppError::VaultLocked),
            _=owner.automation.stopped.cancelled()=>Err(invalid()),
            _=tokio::time::sleep(Duration::from_secs(30))=>Err(invalid()),
            result=work=>result,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
