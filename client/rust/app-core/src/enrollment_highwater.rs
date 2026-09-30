//! Independent public enrollment checkpoints. Callers hold the profile lock
//! while checking the SQLCipher transcript, updating these marks, and committing
//! its matching record. A crash after OS update fails closed on the next read.
use crate::sharing_highwater::{ProfileLock, SharingHighwater};
use cc_crypto_core::sharing::SharingOwnerAnchor;
use cc_crypto_core::sharing_enrollment::{EnrollmentChallengeCheckpoint, VerifiedEnrollmentGrant};
use cc_crypto_core::DevicePublicKeys;
use cc_platform_core::{ExposeSecret, SecureStore, MAX_SECRET_LEN};
use cc_protocol::sharing_enrollment::{EnrollmentGrantStatus, EnrollmentScope};
use cc_protocol::{DeviceId, UserId};
use cc_storage_core::ProfileId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

type Result<T> = std::result::Result<T, EnrollmentMarkError>;
#[derive(Debug, thiserror::Error)]
pub(crate) enum EnrollmentMarkError {
    #[error("enrollment secure checkpoint could not be read or written")]
    SecureStore,
    #[error("enrollment checkpoint or binding is invalid")]
    Invalid,
    #[error("enrollment transcript requires signed history reconciliation")]
    ReconciliationRequired,
    #[error("enrollment grant is locally frozen")]
    Frozen,
    #[error("enrollment secure checkpoint coordination lock is unavailable")]
    LockUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GrantMark {
    pub revision: u64,
    pub signed_state_hash: [u8; 32],
    pub status: EnrollmentGrantStatus,
    pub admitted_count: u32,
    pub access_epoch: u64,
    pub access_manifest_hash: [u8; 32],
    pub frozen: bool,
}
impl GrantMark {
    pub(crate) fn authenticated(grant: &VerifiedEnrollmentGrant) -> Result<Self> {
        let state = &grant.signed().grant;
        Ok(Self {
            revision: state.grant_revision,
            signed_state_hash: grant.hash(),
            status: state.status,
            admitted_count: state.admitted_count,
            access_epoch: state.access_epoch,
            access_manifest_hash: state
                .access_manifest_hash
                .as_slice()
                .try_into()
                .map_err(|_| EnrollmentMarkError::Invalid)?,
            frozen: false,
        })
    }
    fn validate(&self) -> Result<()> {
        if self.revision == 0
            || self.revision > i64::MAX as u64
            || self.access_epoch == 0
            || self.access_epoch > i64::MAX as u64
            || self.admitted_count > 16
            || self.signed_state_hash == [0; 32]
            || self.access_manifest_hash == [0; 32]
        {
            return Err(EnrollmentMarkError::Invalid);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnerPin {
    user_id: UserId,
    device_id: DeviceId,
    encryption_key: [u8; 32],
    signing_key: [u8; 32],
}
impl OwnerPin {
    fn from_anchor(owner: &SharingOwnerAnchor) -> Result<Self> {
        if owner.user_id == UserId::NIL || owner.device_id == DeviceId::NIL {
            return Err(EnrollmentMarkError::Invalid);
        }
        Ok(Self {
            user_id: owner.user_id,
            device_id: owner.device_id,
            encryption_key: owner
                .public_keys
                .encryption_bytes()
                .as_slice()
                .try_into()
                .map_err(|_| EnrollmentMarkError::Invalid)?,
            signing_key: owner
                .public_keys
                .signing_bytes()
                .as_slice()
                .try_into()
                .map_err(|_| EnrollmentMarkError::Invalid)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record<T> {
    format: u16,
    profile: ProfileId,
    scope: EnrollmentScope,
    id: Uuid,
    value: T,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChallengeMark {
    request_hash: [u8; 32],
    challenge_id: Uuid,
    generation: u64,
    hash: [u8; 32],
}
impl From<EnrollmentChallengeCheckpoint> for ChallengeMark {
    fn from(v: EnrollmentChallengeCheckpoint) -> Self {
        Self {
            request_hash: v.request_hash,
            challenge_id: v.challenge_id,
            generation: v.generation,
            hash: v.hash,
        }
    }
}
impl From<ChallengeMark> for EnrollmentChallengeCheckpoint {
    fn from(v: ChallengeMark) -> Self {
        Self {
            request_hash: v.request_hash,
            challenge_id: v.challenge_id,
            generation: v.generation,
            hash: v.hash,
        }
    }
}

// Immutable append-only pages are committed before the head and before a new
// per-ID mark. Interrupted writes leave either harmless orphan pages or a known
// ID with a missing mark, which explicit recovery must never silently reset.
const MAX_INDEX_IDS: usize = 4096;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexHead {
    count: u32,
    tip: Option<Uuid>,
    hash: [u8; 32],
}
impl IndexHead {
    fn empty() -> Self {
        Self {
            count: 0,
            tip: None,
            hash: [0; 32],
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexPage {
    count: u32,
    ids: Vec<Uuid>,
    previous: Option<Uuid>,
    previous_hash: [u8; 32],
}

#[derive(Clone)]
pub(crate) struct EnrollmentHighwater {
    profile: ProfileId,
    instance: Uuid,
    secure: Arc<dyn SecureStore>,
}
impl EnrollmentHighwater {
    pub(crate) fn new(profile: ProfileId, secure: Arc<dyn SecureStore>, instance: Uuid) -> Self {
        Self {
            profile,
            instance,
            secure,
        }
    }
    pub(crate) fn lock(&self) -> Result<ProfileLock> {
        SharingHighwater::new(self.profile, self.secure.clone())
            .lock()
            .map_err(|_| EnrollmentMarkError::LockUnavailable)
    }
    fn validate_scope(&self, scope: &EnrollmentScope, id: Uuid) -> Result<()> {
        if self.profile.0.is_nil()
            || self.instance.is_nil()
            || id.is_nil()
            || scope.server_instance_id != self.instance
            || scope.share_id == cc_protocol::ShareId::NIL
            || scope.item_id == cc_protocol::ObjectId::NIL
        {
            return Err(EnrollmentMarkError::Invalid);
        }
        Ok(())
    }
    fn name(&self, domain: &str, scope: &EnrollmentScope, id: Uuid) -> Result<String> {
        self.validate_scope(scope, id)?;
        let input = serde_json::to_vec(&(1u16, self.profile, domain, scope, id))
            .map_err(|_| EnrollmentMarkError::Invalid)?;
        let hash = cc_crypto_core::request_body_sha256(&input);
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        Ok(format!("cc.enroll.v1:{hex}"))
    }
    fn load<T: for<'de> Deserialize<'de>>(
        &self,
        domain: &str,
        scope: &EnrollmentScope,
        id: Uuid,
    ) -> Result<Option<T>> {
        let name = self.name(domain, scope, id)?;
        let Some(raw) = self
            .secure
            .get(&name)
            .map_err(|_| EnrollmentMarkError::SecureStore)?
        else {
            return Ok(None);
        };
        if raw.expose_secret().len() > MAX_SECRET_LEN {
            return Err(EnrollmentMarkError::Invalid);
        }
        let record: Record<T> = serde_json::from_slice(raw.expose_secret())
            .map_err(|_| EnrollmentMarkError::Invalid)?;
        if record.format != 1
            || record.profile != self.profile
            || record.scope != *scope
            || record.id != id
        {
            return Err(EnrollmentMarkError::Invalid);
        }
        Ok(Some(record.value))
    }
    fn save<T>(&self, domain: &str, scope: &EnrollmentScope, id: Uuid, value: T) -> Result<()>
    where
        T: Serialize + for<'de> Deserialize<'de> + PartialEq,
    {
        let name = self.name(domain, scope, id)?;
        let record = Record {
            format: 1,
            profile: self.profile,
            scope: scope.clone(),
            id,
            value,
        };
        let bytes = serde_json::to_vec(&record).map_err(|_| EnrollmentMarkError::Invalid)?;
        if bytes.len() > MAX_SECRET_LEN {
            return Err(EnrollmentMarkError::Invalid);
        }
        self.secure
            .set(&name, &bytes)
            .map_err(|_| EnrollmentMarkError::SecureStore)?;
        if self.load::<T>(domain, scope, id)?.as_ref() != Some(&record.value) {
            return Err(EnrollmentMarkError::SecureStore);
        }
        Ok(())
    }
    pub(crate) fn owner(&self, scope: &EnrollmentScope) -> Result<Option<SharingOwnerAnchor>> {
        self.load::<OwnerPin>("owner", scope, scope.share_id.0)?
            .map(|pin| {
                let keys = DevicePublicKeys::from_slices(&pin.encryption_key, &pin.signing_key)
                    .map_err(|_| EnrollmentMarkError::Invalid)?;
                let owner = SharingOwnerAnchor {
                    user_id: pin.user_id,
                    device_id: pin.device_id,
                    public_keys: keys,
                };
                OwnerPin::from_anchor(&owner)?;
                Ok(owner)
            })
            .transpose()
    }
    fn scope_name(&self, share_id: cc_protocol::ShareId) -> Result<String> {
        if self.profile.0.is_nil()
            || self.instance.is_nil()
            || share_id == cc_protocol::ShareId::NIL
        {
            return Err(EnrollmentMarkError::Invalid);
        }
        let bytes = serde_json::to_vec(&(1u16, self.profile, self.instance, share_id, "scope"))
            .map_err(|_| EnrollmentMarkError::Invalid)?;
        Ok(format!(
            "cc.enroll.v1:{}",
            cc_crypto_core::request_body_sha256(&bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        ))
    }
    pub(crate) fn scope_for_share(
        &self,
        share_id: cc_protocol::ShareId,
    ) -> Result<Option<EnrollmentScope>> {
        let Some(raw) = self
            .secure
            .get(&self.scope_name(share_id)?)
            .map_err(|_| EnrollmentMarkError::SecureStore)?
        else {
            return Ok(None);
        };
        if raw.expose_secret().len() > MAX_SECRET_LEN {
            return Err(EnrollmentMarkError::Invalid);
        }
        let record: Record<EnrollmentScope> = serde_json::from_slice(raw.expose_secret())
            .map_err(|_| EnrollmentMarkError::Invalid)?;
        self.validate_scope(&record.scope, share_id.0)?;
        if record.format != 1
            || record.profile != self.profile
            || record.id != share_id.0
            || record.scope.share_id != share_id
            || record.scope != record.value
        {
            return Err(EnrollmentMarkError::Invalid);
        }
        Ok(Some(record.value))
    }
    fn pin_scope(&self, scope: &EnrollmentScope) -> Result<()> {
        match self.scope_for_share(scope.share_id)? {
            Some(old) if old == *scope => return Ok(()),
            Some(_) => return Err(EnrollmentMarkError::ReconciliationRequired),
            None => {}
        }
        self.validate_scope(scope, scope.share_id.0)?;
        let record = Record {
            format: 1,
            profile: self.profile,
            scope: scope.clone(),
            id: scope.share_id.0,
            value: scope.clone(),
        };
        let bytes = serde_json::to_vec(&record).map_err(|_| EnrollmentMarkError::Invalid)?;
        if bytes.len() > MAX_SECRET_LEN {
            return Err(EnrollmentMarkError::Invalid);
        }
        self.secure
            .set(&self.scope_name(scope.share_id)?, &bytes)
            .map_err(|_| EnrollmentMarkError::SecureStore)?;
        if self.scope_for_share(scope.share_id)?.as_ref() != Some(scope) {
            return Err(EnrollmentMarkError::SecureStore);
        }
        Ok(())
    }
    fn index_head(&self, domain: &str, scope: &EnrollmentScope) -> Result<IndexHead> {
        match self.load::<IndexHead>(domain, scope, scope.share_id.0)? {
            Some(head) => Ok(head),
            None if self.owner_exists(scope)? => Err(EnrollmentMarkError::ReconciliationRequired),
            None => Ok(IndexHead::empty()),
        }
    }
    fn index_ids(&self, domain: &str, scope: &EnrollmentScope) -> Result<Vec<Uuid>> {
        let mut head = self.index_head(domain, scope)?;
        if head.count as usize > MAX_INDEX_IDS {
            return Err(EnrollmentMarkError::Invalid);
        }
        let mut ids = Vec::with_capacity(head.count as usize);
        let mut seen = std::collections::HashSet::new();
        let page_domain = format!("{domain}:page");
        while let Some(tip) = head.tip {
            let page = self
                .load::<IndexPage>(&page_domain, scope, tip)?
                .ok_or(EnrollmentMarkError::ReconciliationRequired)?;
            let hash = cc_crypto_core::request_body_sha256(
                &serde_json::to_vec(&page).map_err(|_| EnrollmentMarkError::Invalid)?,
            );
            if hash != head.hash
                || page.count != head.count
                || page.ids.is_empty()
                || page.ids.len() > 16
                || page.ids.len() > head.count as usize
            {
                return Err(EnrollmentMarkError::Invalid);
            }
            for id in &page.ids {
                if id.is_nil() || !seen.insert(*id) {
                    return Err(EnrollmentMarkError::Invalid);
                }
                ids.push(*id);
            }
            head = IndexHead {
                count: head.count - page.ids.len() as u32,
                tip: page.previous,
                hash: page.previous_hash,
            };
        }
        if head.count != 0 || head.hash != [0; 32] {
            return Err(EnrollmentMarkError::ReconciliationRequired);
        }
        Ok(ids)
    }
    fn index_add(&self, domain: &str, scope: &EnrollmentScope, id: Uuid) -> Result<()> {
        self.validate_scope(scope, id)?;
        let ids = self.index_ids(domain, scope)?;
        if ids.contains(&id) {
            return Ok(());
        }
        if ids.len() >= MAX_INDEX_IDS {
            return Err(EnrollmentMarkError::Invalid);
        }
        let head = self.index_head(domain, scope)?;
        let tip = Uuid::new_v4();
        let page = IndexPage {
            count: head.count + 1,
            ids: vec![id],
            previous: head.tip,
            previous_hash: head.hash,
        };
        let hash = cc_crypto_core::request_body_sha256(
            &serde_json::to_vec(&page).map_err(|_| EnrollmentMarkError::Invalid)?,
        );
        self.save(&format!("{domain}:page"), scope, tip, page)?;
        self.save(
            domain,
            scope,
            scope.share_id.0,
            IndexHead {
                count: head.count + 1,
                tip: Some(tip),
                hash,
            },
        )
    }
    pub(crate) fn known_grants(&self, scope: &EnrollmentScope) -> Result<Vec<Uuid>> {
        self.index_ids("grant-index", scope)
    }
    pub(crate) fn known_requests(&self, scope: &EnrollmentScope) -> Result<Vec<Uuid>> {
        self.index_ids("request-index", scope)
    }
    pub(crate) fn load_request_hash(
        &self,
        scope: &EnrollmentScope,
        id: Uuid,
    ) -> Result<Option<[u8; 32]>> {
        let hash = self.load::<[u8; 32]>("request-hash", scope, id)?;
        if hash == Some([0; 32]) {
            return Err(EnrollmentMarkError::Invalid);
        }
        Ok(hash)
    }
    pub(crate) fn pin_request(
        &self,
        scope: &EnrollmentScope,
        id: Uuid,
        hash: [u8; 32],
        existing_db_record: bool,
    ) -> Result<()> {
        if hash == [0; 32] {
            return Err(EnrollmentMarkError::Invalid);
        }
        match self.load_request_hash(scope, id)? {
            Some(old) if old == hash && existing_db_record => Ok(()),
            Some(_) => Err(EnrollmentMarkError::ReconciliationRequired),
            None if existing_db_record || self.known_requests(scope)?.contains(&id) => {
                Err(EnrollmentMarkError::ReconciliationRequired)
            }
            None => {
                self.index_add("request-index", scope, id)?;
                self.save("request-hash", scope, id, hash)
            }
        }
    }
    /// A pending owner acceptance is an immutable uncertainty fence. Losing
    /// its SQL ciphertext cannot authorize a different mutation for this ID.
    pub(crate) fn load_pending_acceptance_hash(
        &self,
        scope: &EnrollmentScope,
        id: Uuid,
    ) -> Result<Option<[u8; 32]>> {
        let hash = self.load::<[u8; 32]>("pending-hash", scope, id)?;
        if hash == Some([0; 32]) {
            return Err(EnrollmentMarkError::Invalid);
        }
        if hash.is_none() && self.index_ids("pending-index", scope)?.contains(&id) {
            return Err(EnrollmentMarkError::ReconciliationRequired);
        }
        Ok(hash)
    }
    pub(crate) fn pin_pending_acceptance(
        &self,
        scope: &EnrollmentScope,
        id: Uuid,
        hash: [u8; 32],
    ) -> Result<()> {
        if hash == [0; 32] || self.load_request_hash(scope, id)?.is_none() {
            return Err(EnrollmentMarkError::ReconciliationRequired);
        }
        match self.load_pending_acceptance_hash(scope, id)? {
            Some(old) if old == hash => Ok(()),
            Some(_) => Err(EnrollmentMarkError::ReconciliationRequired),
            None => {
                self.index_add("pending-index", scope, id)?;
                self.save("pending-hash", scope, id, hash)
            }
        }
    }
    /// Call only after an independent pairing or an existing exact owner pin.
    /// Merely receiving a server directory/bundle never authorizes this write.
    pub(crate) fn pin_owner(
        &self,
        scope: &EnrollmentScope,
        owner: &SharingOwnerAnchor,
        existing_db_record: bool,
    ) -> Result<()> {
        let id = scope.share_id.0;
        let expected = OwnerPin::from_anchor(owner)?;
        match self.load::<OwnerPin>("owner", scope, id)? {
            Some(actual) if actual == expected && existing_db_record => Ok(()),
            Some(_) => Err(EnrollmentMarkError::ReconciliationRequired),
            None if existing_db_record || self.scope_for_share(scope.share_id)?.is_some() => {
                Err(EnrollmentMarkError::ReconciliationRequired)
            }
            None => {
                for domain in [
                    "grant-index",
                    "request-index",
                    "challenge-index",
                    "pending-index",
                ] {
                    if self.load::<IndexHead>(domain, scope, id)?.is_some() {
                        return Err(EnrollmentMarkError::ReconciliationRequired);
                    }
                }
                self.pin_scope(scope)?;
                for domain in [
                    "grant-index",
                    "request-index",
                    "challenge-index",
                    "pending-index",
                ] {
                    if self.load::<IndexHead>(domain, scope, id)?.is_none() {
                        self.save(domain, scope, id, IndexHead::empty())?;
                    }
                }
                self.save("owner", scope, id, expected)
            }
        }
    }
    pub(crate) fn owner_exists(&self, scope: &EnrollmentScope) -> Result<bool> {
        Ok(self
            .load::<OwnerPin>("owner", scope, scope.share_id.0)?
            .is_some())
    }
    pub(crate) fn load_grant(
        &self,
        scope: &EnrollmentScope,
        id: Uuid,
    ) -> Result<Option<GrantMark>> {
        let mark = self.load::<GrantMark>("grant", scope, id)?;
        if let Some(m) = &mark {
            m.validate()?;
        }
        Ok(mark)
    }
    pub(crate) fn check_grant(
        &self,
        scope: &EnrollmentScope,
        id: Uuid,
        db: Option<&GrantMark>,
    ) -> Result<Option<GrantMark>> {
        let actual = self.load_grant(scope, id)?;
        if actual.as_ref() != db || (actual.is_none() && self.known_grants(scope)?.contains(&id)) {
            return Err(EnrollmentMarkError::ReconciliationRequired);
        }
        Ok(actual)
    }
    /// Cryptographic caller verifies the complete signed successor and hashes.
    /// This CAS additionally prevents a known terminal/frozen head being reused.
    pub(crate) fn cas_grant(
        &self,
        scope: &EnrollmentScope,
        id: Uuid,
        expected: Option<&GrantMark>,
        next: GrantMark,
    ) -> Result<GrantMark> {
        self.cas_grant_inner(scope, id, expected, next, false)
    }
    /// Explicit recovery only: the caller has verified every owner-signed
    /// successor from this exact independent head. Preserve the local freeze
    /// while catching up, so a later revoke uses the actual remote head and no
    /// frozen active grant becomes eligible for enrollment again.
    pub(crate) fn cas_reconciled_grant(
        &self,
        scope: &EnrollmentScope,
        id: Uuid,
        expected: &GrantMark,
        next: GrantMark,
    ) -> Result<GrantMark> {
        if !expected.frozen || !next.frozen {
            return Err(EnrollmentMarkError::Frozen);
        }
        self.cas_grant_inner(scope, id, Some(expected), next, true)
    }
    fn cas_grant_inner(
        &self,
        scope: &EnrollmentScope,
        id: Uuid,
        expected: Option<&GrantMark>,
        mut next: GrantMark,
        reconcile_frozen: bool,
    ) -> Result<GrantMark> {
        next.validate()?;
        let old = self.check_grant(scope, id, expected)?;
        if let Some(old) = old {
            next.frozen |= old.frozen;
            if old == next {
                return Ok(next);
            }
            if old.frozen {
                if next.status == EnrollmentGrantStatus::Active && !reconcile_frozen {
                    return Err(EnrollmentMarkError::Frozen);
                }
                next.frozen = true;
            }
            if old.status == EnrollmentGrantStatus::Revoked
                || next.revision
                    != old
                        .revision
                        .checked_add(1)
                        .ok_or(EnrollmentMarkError::Invalid)?
                || next.admitted_count < old.admitted_count
                || next.admitted_count > old.admitted_count + 1
                || next.access_epoch < old.access_epoch
            {
                return Err(EnrollmentMarkError::ReconciliationRequired);
            }
        }
        self.index_add("grant-index", scope, id)?;
        self.save("grant", scope, id, next.clone())?;
        Ok(next)
    }
    /// Freeze before sending revoke. Network failure cannot reactivate this ID.
    pub(crate) fn freeze_grant(
        &self,
        scope: &EnrollmentScope,
        id: Uuid,
        expected: &GrantMark,
    ) -> Result<GrantMark> {
        let mut actual = self
            .check_grant(scope, id, Some(expected))?
            .ok_or(EnrollmentMarkError::ReconciliationRequired)?;
        actual.frozen = true;
        self.save("grant", scope, id, actual.clone())?;
        Ok(actual)
    }
    pub(crate) fn load_challenge(
        &self,
        scope: &EnrollmentScope,
        request_id: Uuid,
    ) -> Result<Option<EnrollmentChallengeCheckpoint>> {
        let checkpoint = self.load::<ChallengeMark>("challenge", scope, request_id)?;
        if let Some(c) = &checkpoint {
            if c.generation == 0
                || c.generation > i64::MAX as u64
                || c.challenge_id.is_nil()
                || c.hash == [0; 32]
                || c.request_hash == [0; 32]
            {
                return Err(EnrollmentMarkError::Invalid);
            }
        }
        Ok(checkpoint.map(Into::into))
    }
    pub(crate) fn check_challenge(
        &self,
        scope: &EnrollmentScope,
        id: Uuid,
        db: Option<&EnrollmentChallengeCheckpoint>,
    ) -> Result<Option<EnrollmentChallengeCheckpoint>> {
        let actual = self.load_challenge(scope, id)?;
        if actual.as_ref() != db
            || (actual.is_none() && self.index_ids("challenge-index", scope)?.contains(&id))
        {
            return Err(EnrollmentMarkError::ReconciliationRequired);
        }
        Ok(actual)
    }
    pub(crate) fn cas_challenge(
        &self,
        scope: &EnrollmentScope,
        id: Uuid,
        expected: Option<&EnrollmentChallengeCheckpoint>,
        next: EnrollmentChallengeCheckpoint,
    ) -> Result<()> {
        if next.generation == 0
            || next.generation > i64::MAX as u64
            || next.challenge_id.is_nil()
            || next.hash == [0; 32]
            || next.request_hash == [0; 32]
        {
            return Err(EnrollmentMarkError::Invalid);
        }
        if let Some(old) = self.check_challenge(scope, id, expected)? {
            if old == next {
                return Ok(());
            }
            if next.generation
                != old
                    .generation
                    .checked_add(1)
                    .ok_or(EnrollmentMarkError::Invalid)?
                || next.request_hash != old.request_hash
                || next.challenge_id == old.challenge_id
            {
                return Err(EnrollmentMarkError::ReconciliationRequired);
            }
        }
        if self.load_request_hash(scope, id)? != Some(next.request_hash) {
            if self.owner_exists(scope)? {
                return Err(EnrollmentMarkError::ReconciliationRequired);
            }
            self.pin_request(scope, id, next.request_hash, false)?;
        }
        self.index_add("challenge-index", scope, id)?;
        self.save("challenge", scope, id, ChallengeMark::from(next))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_crypto_core::DeviceSecretKeys;
    use cc_platform_core::{InMemorySecureStore, SecureStoreError, MAX_SECRET_NAME_LEN};
    use cc_protocol::sharing::SharedItemKind;
    use secrecy::SecretSlice;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn hash() -> [u8; 32] {
        let mut value = [0; 32];
        value[..16].copy_from_slice(Uuid::new_v4().as_bytes());
        value[16..].copy_from_slice(Uuid::new_v4().as_bytes());
        value
    }
    fn scope() -> EnrollmentScope {
        EnrollmentScope {
            server_instance_id: Uuid::new_v4(),
            share_id: cc_protocol::ShareId::new(),
            item_id: cc_protocol::ObjectId::new(),
            kind: SharedItemKind::Host,
        }
    }
    fn mark() -> GrantMark {
        GrantMark {
            revision: 1,
            signed_state_hash: hash(),
            status: EnrollmentGrantStatus::Active,
            admitted_count: 0,
            access_epoch: 1,
            access_manifest_hash: hash(),
            frozen: false,
        }
    }
    fn owner() -> SharingOwnerAnchor {
        let keys = DeviceSecretKeys::generate().unwrap();
        SharingOwnerAnchor {
            user_id: UserId::new(),
            device_id: DeviceId::new(),
            public_keys: keys.public_keys(),
        }
    }
    #[test]
    fn scope_owner_and_record_binding_cannot_silently_repin() {
        let secure = Arc::new(InMemorySecureStore::new());
        let scope = scope();
        let store =
            EnrollmentHighwater::new(ProfileId::new(), secure.clone(), scope.server_instance_id);
        let _lock = store.lock().unwrap();
        let owner = owner();
        assert!(matches!(
            store.pin_owner(&scope, &owner, true),
            Err(EnrollmentMarkError::ReconciliationRequired)
        ));
        store.pin_owner(&scope, &owner, false).unwrap();
        store.pin_owner(&scope, &owner, true).unwrap();
        assert!(matches!(
            store.pin_owner(&scope, &owner, false),
            Err(EnrollmentMarkError::ReconciliationRequired)
        ));
        let other = SharingOwnerAnchor {
            device_id: DeviceId::new(),
            ..owner
        };
        assert!(matches!(
            store.pin_owner(&scope, &other, false),
            Err(EnrollmentMarkError::ReconciliationRequired)
        ));
        let id = Uuid::new_v4();
        let name = store.name("grant", &scope, id).unwrap();
        assert!(name.len() <= MAX_SECRET_NAME_LEN);
        let m = mark();
        store.cas_grant(&scope, id, None, m.clone()).unwrap();
        let stored = secure.get(&name).unwrap().unwrap();
        assert!(stored.expose_secret().len() <= MAX_SECRET_LEN);
        let foreign =
            EnrollmentHighwater::new(ProfileId::new(), secure.clone(), scope.server_instance_id);
        let foreign_name = foreign.name("grant", &scope, id).unwrap();
        secure.set(&foreign_name, stored.expose_secret()).unwrap();
        assert!(matches!(
            foreign.load_grant(&scope, id),
            Err(EnrollmentMarkError::Invalid)
        ));
        let mut altered_scope = scope.clone();
        altered_scope.server_instance_id = Uuid::new_v4();
        assert!(matches!(
            store.load_grant(&altered_scope, id),
            Err(EnrollmentMarkError::Invalid)
        ));
    }
    #[test]
    fn known_freeze_and_terminal_heads_survive_old_database_or_restart() {
        let secure = Arc::new(InMemorySecureStore::new());
        let scope = scope();
        let profile = ProfileId::new();
        let store = EnrollmentHighwater::new(profile, secure.clone(), scope.server_instance_id);
        let _lock = store.lock().unwrap();
        let id = Uuid::new_v4();
        let first = mark();
        store.cas_grant(&scope, id, None, first.clone()).unwrap();
        let frozen = store.freeze_grant(&scope, id, &first).unwrap();
        let restarted = EnrollmentHighwater::new(profile, secure, scope.server_instance_id);
        assert!(matches!(
            restarted.check_grant(&scope, id, Some(&first)),
            Err(EnrollmentMarkError::ReconciliationRequired)
        ));
        assert!(matches!(
            restarted.check_grant(&scope, id, None),
            Err(EnrollmentMarkError::ReconciliationRequired)
        ));
        let mut competing = first.clone();
        competing.revision += 1;
        competing.signed_state_hash = hash();
        competing.admitted_count += 1;
        assert!(matches!(
            restarted.cas_grant(&scope, id, Some(&frozen), competing),
            Err(EnrollmentMarkError::Frozen)
        ));
        let mut revoked = frozen.clone();
        revoked.revision += 1;
        revoked.status = EnrollmentGrantStatus::Revoked;
        revoked.signed_state_hash = hash();
        let revoked = restarted
            .cas_grant(&scope, id, Some(&frozen), revoked)
            .unwrap();
        let mut new_active = revoked.clone();
        new_active.revision += 1;
        new_active.status = EnrollmentGrantStatus::Active;
        new_active.signed_state_hash = hash();
        assert!(restarted
            .cas_grant(&scope, id, Some(&revoked), new_active)
            .is_err());
        restarted
            .cas_grant(&scope, id, Some(&revoked), revoked.clone())
            .unwrap();
    }
    #[test]
    fn explicit_recovery_catches_up_frozen_head_without_reactivating_it() {
        let scope = scope();
        let store = EnrollmentHighwater::new(
            ProfileId::new(),
            Arc::new(InMemorySecureStore::new()),
            scope.server_instance_id,
        );
        let _lock = store.lock().unwrap();
        let id = Uuid::new_v4();
        let first = store.cas_grant(&scope, id, None, mark()).unwrap();
        let frozen = store.freeze_grant(&scope, id, &first).unwrap();
        let mut successor = frozen.clone();
        successor.revision += 1;
        successor.admitted_count += 1;
        successor.access_epoch += 1;
        successor.signed_state_hash = hash();
        successor.access_manifest_hash = hash();
        let mut unfrozen = successor.clone();
        unfrozen.frozen = false;
        assert!(store
            .cas_reconciled_grant(&scope, id, &frozen, unfrozen)
            .is_err());
        let mut skipped = successor.clone();
        skipped.revision += 1;
        assert!(store
            .cas_reconciled_grant(&scope, id, &frozen, skipped)
            .is_err());
        assert!(store
            .cas_grant(&scope, id, Some(&frozen), successor.clone())
            .is_err());
        let recovered = store
            .cas_reconciled_grant(&scope, id, &frozen, successor)
            .unwrap();
        assert!(recovered.frozen);
        assert!(store
            .cas_reconciled_grant(&scope, id, &frozen, recovered.clone())
            .is_err());
        let mut terminal = recovered.clone();
        terminal.revision += 1;
        terminal.status = EnrollmentGrantStatus::Revoked;
        terminal.signed_state_hash = hash();
        let terminal = store
            .cas_grant(&scope, id, Some(&recovered), terminal)
            .unwrap();
        let mut revived = terminal.clone();
        revived.revision += 1;
        revived.status = EnrollmentGrantStatus::Active;
        revived.signed_state_hash = hash();
        assert!(store
            .cas_reconciled_grant(&scope, id, &terminal, revived)
            .is_err());
    }
    #[test]
    fn challenge_generation_and_exact_hash_are_monotonic() {
        let scope = scope();
        let store = EnrollmentHighwater::new(
            ProfileId::new(),
            Arc::new(InMemorySecureStore::new()),
            scope.server_instance_id,
        );
        let _lock = store.lock().unwrap();
        let id = Uuid::new_v4();
        let first = EnrollmentChallengeCheckpoint {
            request_hash: hash(),
            challenge_id: Uuid::new_v4(),
            generation: 1,
            hash: hash(),
        };
        store.cas_challenge(&scope, id, None, first).unwrap();
        let next = EnrollmentChallengeCheckpoint {
            challenge_id: Uuid::new_v4(),
            generation: 2,
            hash: hash(),
            ..first
        };
        store.cas_challenge(&scope, id, Some(&first), next).unwrap();
        assert!(matches!(
            store.check_challenge(&scope, id, Some(&first)),
            Err(EnrollmentMarkError::ReconciliationRequired)
        ));
        let competing = EnrollmentChallengeCheckpoint {
            hash: hash(),
            ..next
        };
        assert!(store
            .cas_challenge(&scope, id, Some(&next), competing)
            .is_err());
        let wrong_request = EnrollmentChallengeCheckpoint {
            request_hash: hash(),
            generation: 3,
            challenge_id: Uuid::new_v4(),
            hash: hash(),
        };
        assert!(store
            .cas_challenge(&scope, id, Some(&next), wrong_request)
            .is_err());
        assert!(store.check_challenge(&scope, id, None).is_err());
    }
    #[test]
    fn independent_index_preserves_ids_missing_from_restored_database() {
        let scope = scope();
        let profile = ProfileId::new();
        let secure = Arc::new(InMemorySecureStore::new());
        let store = EnrollmentHighwater::new(profile, secure.clone(), scope.server_instance_id);
        let _lock = store.lock().unwrap();
        let anchor = owner();
        store.pin_owner(&scope, &anchor, false).unwrap();
        let mut grants = Vec::new();
        for _ in 0..20 {
            let id = Uuid::new_v4();
            store.cas_grant(&scope, id, None, mark()).unwrap();
            grants.push(id);
        }
        let id = grants[7];
        let mark = store.load_grant(&scope, id).unwrap().unwrap();
        store.freeze_grant(&scope, id, &mark).unwrap();
        let request = Uuid::new_v4();
        let request_hash = hash();
        store
            .pin_request(&scope, request, request_hash, false)
            .unwrap();
        let restarted = EnrollmentHighwater::new(profile, secure.clone(), scope.server_instance_id);
        assert_eq!(
            restarted.scope_for_share(scope.share_id).unwrap(),
            Some(scope.clone())
        );
        assert_eq!(restarted.owner(&scope).unwrap(), Some(anchor));
        let found = restarted.known_grants(&scope).unwrap();
        assert_eq!(found.len(), 20);
        assert!(grants.iter().all(|id| found.contains(id)));
        assert!(restarted.load_grant(&scope, id).unwrap().unwrap().frozen);
        assert_eq!(restarted.known_requests(&scope).unwrap(), vec![request]);
        assert_eq!(
            restarted.load_request_hash(&scope, request).unwrap(),
            Some(request_hash)
        );
        secure
            .delete(&store.name("grant", &scope, id).unwrap())
            .unwrap();
        assert!(matches!(
            restarted.cas_grant(&scope, id, None, mark),
            Err(EnrollmentMarkError::ReconciliationRequired)
        ));
        secure
            .delete(&store.name("request-hash", &scope, request).unwrap())
            .unwrap();
        assert!(restarted
            .pin_request(&scope, request, request_hash, false)
            .is_err());
        let head = store.index_head("grant-index", &scope).unwrap();
        secure
            .delete(
                &store
                    .name("grant-index:page", &scope, head.tip.unwrap())
                    .unwrap(),
            )
            .unwrap();
        assert!(restarted.known_grants(&scope).is_err());
    }
    #[test]
    fn missing_index_or_foreign_scope_reference_cannot_reset_existing_marks() {
        let scope = scope();
        let secure = Arc::new(InMemorySecureStore::new());
        let store =
            EnrollmentHighwater::new(ProfileId::new(), secure.clone(), scope.server_instance_id);
        let _lock = store.lock().unwrap();
        store.pin_owner(&scope, &owner(), false).unwrap();
        let id = Uuid::new_v4();
        store.cas_grant(&scope, id, None, mark()).unwrap();
        let other =
            EnrollmentHighwater::new(ProfileId::new(), secure.clone(), scope.server_instance_id);
        let raw = secure
            .get(&store.scope_name(scope.share_id).unwrap())
            .unwrap()
            .unwrap();
        secure
            .set(
                &other.scope_name(scope.share_id).unwrap(),
                raw.expose_secret(),
            )
            .unwrap();
        assert!(other.scope_for_share(scope.share_id).is_err());
        secure
            .delete(&store.name("grant-index", &scope, scope.share_id.0).unwrap())
            .unwrap();
        assert!(store.known_grants(&scope).is_err());
        assert!(store
            .cas_grant(&scope, Uuid::new_v4(), None, mark())
            .is_err());
        assert!(store.pin_owner(&scope, &owner(), false).is_err());
    }
    #[test]
    fn pending_acceptance_hash_fence_survives_lost_ciphertext_and_rejects_replacement() {
        let scope = scope();
        let secure = Arc::new(InMemorySecureStore::new());
        let store =
            EnrollmentHighwater::new(ProfileId::new(), secure.clone(), scope.server_instance_id);
        let _lock = store.lock().unwrap();
        store.pin_owner(&scope, &owner(), false).unwrap();
        let request = Uuid::new_v4();
        let receipt_hash = hash();
        assert!(store
            .pin_pending_acceptance(&scope, request, receipt_hash)
            .is_err());
        store.pin_request(&scope, request, hash(), false).unwrap();
        store
            .pin_pending_acceptance(&scope, request, receipt_hash)
            .unwrap();
        store
            .pin_pending_acceptance(&scope, request, receipt_hash)
            .unwrap();
        assert_eq!(
            store.load_pending_acceptance_hash(&scope, request).unwrap(),
            Some(receipt_hash)
        );
        assert!(store
            .pin_pending_acceptance(&scope, request, hash())
            .is_err());
        secure
            .delete(&store.name("pending-hash", &scope, request).unwrap())
            .unwrap();
        assert!(store.load_pending_acceptance_hash(&scope, request).is_err());
        assert!(store
            .pin_pending_acceptance(&scope, request, receipt_hash)
            .is_err());
    }
    #[test]
    fn lost_owner_marker_does_not_turn_a_known_scope_into_a_fresh_pairing() {
        let scope = scope();
        let secure = Arc::new(InMemorySecureStore::new());
        let store =
            EnrollmentHighwater::new(ProfileId::new(), secure.clone(), scope.server_instance_id);
        let _lock = store.lock().unwrap();
        let anchor = owner();
        store.pin_owner(&scope, &anchor, false).unwrap();
        secure
            .delete(&store.name("owner", &scope, scope.share_id.0).unwrap())
            .unwrap();
        assert!(matches!(
            store.pin_owner(&scope, &anchor, false),
            Err(EnrollmentMarkError::ReconciliationRequired)
        ));
        secure
            .delete(&store.scope_name(scope.share_id).unwrap())
            .unwrap();
        assert!(matches!(
            store.pin_owner(&scope, &anchor, false),
            Err(EnrollmentMarkError::ReconciliationRequired)
        ));
    }
    #[derive(Debug, Default)]
    struct LostStore {
        inner: InMemorySecureStore,
        lose: AtomicBool,
    }
    impl SecureStore for LostStore {
        fn get(
            &self,
            name: &str,
        ) -> std::result::Result<Option<SecretSlice<u8>>, SecureStoreError> {
            self.inner.get(name)
        }
        fn set(&self, name: &str, bytes: &[u8]) -> std::result::Result<(), SecureStoreError> {
            if self.lose.load(Ordering::SeqCst) {
                Ok(())
            } else {
                self.inner.set(name, bytes)
            }
        }
        fn delete(&self, name: &str) -> std::result::Result<bool, SecureStoreError> {
            self.inner.delete(name)
        }
    }
    #[test]
    fn os_write_readback_and_missing_marker_fail_closed() {
        let scope = scope();
        let secure = Arc::new(LostStore::default());
        let store =
            EnrollmentHighwater::new(ProfileId::new(), secure.clone(), scope.server_instance_id);
        let _lock = store.lock().unwrap();
        let id = Uuid::new_v4();
        let first = mark();
        store.cas_grant(&scope, id, None, first.clone()).unwrap();
        secure.lose.store(true, Ordering::SeqCst);
        assert!(matches!(
            store.freeze_grant(&scope, id, &first),
            Err(EnrollmentMarkError::SecureStore)
        ));
        secure.lose.store(false, Ordering::SeqCst);
        secure
            .delete(&store.name("grant", &scope, id).unwrap())
            .unwrap();
        assert!(matches!(
            store.check_grant(&scope, id, Some(&first)),
            Err(EnrollmentMarkError::ReconciliationRequired)
        ));
    }
}
