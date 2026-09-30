//! Public-transcript enrollment orchestration. This is a child of sharing_api
//! so it reuses the exact unlocked generation, account/instance pin and gate.
//! No transcript contains plaintext, a DEK, personal vault keys or passwords.

use super::*;
use crate::enrollment_dto::*;
use crate::enrollment_highwater::{EnrollmentHighwater, EnrollmentMarkError, GrantMark};
use cc_crypto_core::sharing::VerifiedSharingMutation;
use cc_crypto_core::sharing_enrollment::*;
use cc_protocol::sharing::{SignedAccessManifest, SignedSharingMutation};
use cc_protocol::sharing_enrollment::*;
use cc_sync_core::api::OwnDeviceEnrollmentApi;
use std::collections::HashMap;
use std::sync::Mutex;

const MAX_BUNDLE_BYTES: usize = 512 * 1024;
const MAX_ENROLLMENT_RECORD_BYTES: usize = 8 * 1024 * 1024;
const RECORD_PREFIX: &str = "cc.enrollment.facade.v1/";

/// Contains only ephemeral zeroizing owner possession secrets. Lock drops all
/// entries before any slow SSH/file shutdown; restart always needs a challenge.
#[derive(Default)]
pub(crate) struct EnrollmentRuntime {
    pending: Mutex<HashMap<(ShareId, Uuid), OwnerChallengeSecret>>,
}
impl EnrollmentRuntime {
    pub(crate) fn clear(&self) {
        self.pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PublicBundle {
    format: u16,
    owner: SharingIdentityDto,
    manifests: Vec<SignedAccessManifest>,
    header: SignedSharingMutation,
    grant_history: Vec<SignedSharingOwnDevicesGrantState>,
    request: Option<SignedSharingOwnDeviceRequest>,
    endorsement: Option<SignedSharingAnchorEndorsement>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredGrant {
    history: Vec<SignedSharingOwnDevicesGrantState>,
    mark: GrantMark,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredChallengeMark {
    request_hash: [u8; 32],
    challenge_id: Uuid,
    generation: u64,
    hash: [u8; 32],
}
impl From<EnrollmentChallengeCheckpoint> for StoredChallengeMark {
    fn from(c: EnrollmentChallengeCheckpoint) -> Self {
        Self {
            request_hash: c.request_hash,
            challenge_id: c.challenge_id,
            generation: c.generation,
            hash: c.hash,
        }
    }
}
impl From<&StoredChallengeMark> for EnrollmentChallengeCheckpoint {
    fn from(c: &StoredChallengeMark) -> Self {
        Self {
            request_hash: c.request_hash,
            challenge_id: c.challenge_id,
            generation: c.generation,
            hash: c.hash,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredRequest {
    bundle: PublicBundle,
    state: OwnDeviceRequestState,
    checkpoint: Option<StoredChallengeMark>,
    /// The sole durable retry path: exact signed headers and ciphertext only.
    pending_acceptance: Option<AcceptOwnDeviceRequest>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnrollmentRecord {
    format: u16,
    scope: EnrollmentScope,
    owner: SharingIdentityDto,
    manifests: Vec<SignedAccessManifest>,
    header: SignedSharingMutation,
    grants: Vec<StoredGrant>,
    requests: Vec<StoredRequest>,
}
struct RecordState {
    value: EnrollmentRecord,
    serialized: Option<String>,
}
struct VerifiedBundle {
    owner: SharingOwnerAnchor,
    access: VerifiedSharingManifest,
    revision: VerifiedSharingMutation,
    grant: VerifiedEnrollmentGrant,
    request: Option<VerifiedEnrollmentRequest>,
    endorsement: Option<VerifiedAnchorEndorsement>,
}

fn enrollment_error(_: impl std::fmt::Display) -> AppError {
    AppError::Crypto(
        "enrollment identity, signature, permission or possession could not be verified".into(),
    )
}
fn mark_error(error: EnrollmentMarkError) -> AppError {
    match error {
        EnrollmentMarkError::ReconciliationRequired => AppError::SharingReconciliationRequired,
        EnrollmentMarkError::Frozen => invalid("this enrollment grant is locally frozen"),
        _ => enrollment_error(error),
    }
}
fn clock() -> i64 {
    chrono::Utc::now().timestamp()
}
fn record_key(id: ShareId) -> String {
    format!("{RECORD_PREFIX}{id}")
}
fn scope_from(snapshot: &SharingSnapshot) -> EnrollmentScope {
    EnrollmentScope {
        server_instance_id: snapshot.binding().server_instance_id,
        share_id: snapshot.binding().share_id,
        item_id: snapshot.binding().item_id,
        kind: snapshot.binding().kind,
    }
}
fn owner_from(identity: &SharingIdentityDto, instance: Uuid) -> AppResult<SharingOwnerAnchor> {
    if identity.server_instance_id != instance.to_string() {
        return Err(invalid("enrollment owner belongs to another instance"));
    }
    let owner = SharingOwnerAnchor {
        user_id: UserId::from_str(&identity.user_id)
            .map_err(|_| invalid("invalid enrollment owner account"))?,
        device_id: DeviceId::from_str(&identity.device_id)
            .map_err(|_| invalid("invalid enrollment owner device"))?,
        public_keys: DevicePublicKeys::from_slices(
            &unhex(&identity.encryption_public_key)?,
            &unhex(&identity.signing_public_key)?,
        )?,
    };
    let expected =
        sharing_identity_code(instance, owner.user_id, owner.device_id, &owner.public_keys)
            .map_err(enrollment_error)?;
    if !code_matches(&expected, &identity.verification_code) {
        return Err(invalid("enrollment owner code does not bind its keys"));
    }
    Ok(owner)
}
fn context_for(manifest: &SignedAccessManifest) -> SharingContext {
    SharingContext {
        server_instance_id: manifest.manifest.server_instance_id,
        share_id: manifest.manifest.share_id,
        item_id: manifest.manifest.item_id,
        kind: manifest.manifest.kind,
        revision: 1,
        access_epoch: manifest.manifest.access_epoch,
    }
}
fn verified_manifests(
    manifests: &[SignedAccessManifest],
    owner: &SharingOwnerAnchor,
) -> AppResult<Vec<VerifiedSharingManifest>> {
    if manifests.is_empty() || manifests.len() > 10_000 {
        return Err(invalid("enrollment access history exceeds the limit"));
    }
    let mut verified: Vec<VerifiedSharingManifest> = Vec::with_capacity(manifests.len());
    for access in manifests {
        let previous = verified.last().map(VerifiedSharingManifest::checkpoint);
        verified.push(
            verify_shared_manifest(access, &context_for(access), owner, previous.as_ref())
                .map_err(enrollment_error)?,
        );
    }
    Ok(verified)
}
fn verify_grant_chain(
    history: &[SignedSharingOwnDevicesGrantState],
    owner: &SharingOwnerAnchor,
    accesses: &[VerifiedSharingManifest],
    now: i64,
) -> AppResult<VerifiedEnrollmentGrant> {
    if history.is_empty() || history.len() > MAX_RETAINED_REQUESTS_PER_SHARE {
        return Err(invalid("enrollment grant history exceeds the limit"));
    }
    let find = |hash: &Bytes| {
        accesses
            .iter()
            .find(|a| a.hash().as_slice() == hash.as_slice())
            .ok_or(AppError::SharingReconciliationRequired)
    };
    let mut prior: Option<VerifiedEnrollmentGrant> = None;
    for signed in history {
        let before = match &prior {
            Some(p) => find(&p.signed().grant.access_manifest_hash)?,
            None => find(&signed.grant.access_manifest_hash)?,
        };
        // Signed historical records are checked at their original validity
        // interval. Every activation separately checks the actual current time.
        let at = bounded_time(now, signed.grant.not_before, signed.grant.expires_at);
        let transition = match &prior {
            None => GrantTransition::Genesis { access: before },
            Some(p)
                if signed.grant.access_manifest_hash == p.signed().grant.access_manifest_hash
                    && signed.grant.admitted_count == p.signed().grant.admitted_count =>
            {
                GrantTransition::Revoke { access: before }
            }
            Some(p) => GrantTransition::Acceptance {
                before,
                after: find(&signed.grant.access_manifest_hash)?,
                consumed: signed.grant.admitted_count == p.signed().grant.admitted_count + 1,
            },
        };
        prior = Some(
            verify_grant_state(signed, owner, prior.as_ref(), transition, at)
                .map_err(enrollment_error)?,
        );
    }
    prior.ok_or_else(|| invalid("missing enrollment grant"))
}
fn verify_bundle(bundle: &PublicBundle, now: i64) -> AppResult<VerifiedBundle> {
    if bundle.format != 1 {
        return Err(invalid("unsupported enrollment bundle format"));
    }
    let scope = bundle
        .grant_history
        .last()
        .ok_or_else(|| invalid("missing enrollment grant"))?
        .grant
        .scope
        .clone();
    let owner = owner_from(&bundle.owner, scope.server_instance_id)?;
    let accesses = verified_manifests(&bundle.manifests, &owner)?;
    let access = accesses
        .last()
        .cloned()
        .ok_or_else(|| invalid("missing enrollment access"))?;
    let revision =
        verify_shared_mutation(&access, &bundle.header, None).map_err(enrollment_error)?;
    let grant = verify_grant_chain(&bundle.grant_history, &owner, &accesses, now)?;
    let request = bundle
        .request
        .as_ref()
        .map(|r| verify_device_request(r, &grant, &access, now).map_err(enrollment_error))
        .transpose()?;
    let endorsement = match (&bundle.endorsement, &request) {
        (Some(e), Some(r)) => {
            Some(verify_anchor_endorsement(e, r, &access, now).map_err(enrollment_error)?)
        }
        (None, _) => None,
        _ => return Err(invalid("endorsement requires a signed request")),
    };
    Ok(VerifiedBundle {
        owner,
        access,
        revision,
        grant,
        request,
        endorsement,
    })
}
fn parse_bundle(raw: &str) -> AppResult<PublicBundle> {
    if raw.len() > MAX_BUNDLE_BYTES {
        return Err(invalid("enrollment bundle exceeds the limit"));
    }
    serde_json::from_str(raw).map_err(|_| invalid("invalid public enrollment bundle"))
}
fn pairing_dto(bundle: &PublicBundle) -> AppResult<EnrollmentPairingDto> {
    let verified = verify_bundle(bundle, historical_time(bundle))?;
    let public_bundle_json = serde_json::to_string(bundle)
        .map_err(|_| invalid("cannot encode public enrollment bundle"))?;
    if public_bundle_json.len() > MAX_BUNDLE_BYTES {
        return Err(invalid("enrollment bundle exceeds the limit"));
    }
    Ok(EnrollmentPairingDto {
        share_id: verified.grant.signed().grant.scope.share_id.to_string(),
        grant_id: verified.grant.signed().grant.grant_id.to_string(),
        request_id: verified
            .request
            .as_ref()
            .map(|r| r.signed().request.request_id.to_string()),
        public_bundle_json,
        comparison_code: verified
            .request
            .as_ref()
            .map(enrollment_pairing_code)
            .transpose()
            .map_err(enrollment_error)?,
        expires_at: verified
            .request
            .as_ref()
            .map_or(verified.grant.signed().grant.expires_at, |r| {
                r.signed().request.expires_at
            }),
        target: verified
            .request
            .as_ref()
            .map(|r| {
                let target = &r.signed().request.target;
                identity_dto(
                    r.signed().request.scope.server_instance_id,
                    &SharingMember {
                        user_id: target.user_id,
                        device_id: target.device_id,
                        encryption_public_key: target.encryption_public_key.clone(),
                        signing_public_key: target.signing_public_key.clone(),
                        role: r.signed().request.requested_role,
                    },
                )
            })
            .transpose()?,
        requested_role: verified
            .request
            .as_ref()
            .map(|r| role_dto(r.signed().request.requested_role)),
    })
}
fn grant_dto(
    signed: &SignedSharingOwnDevicesGrantState,
    mark: &GrantMark,
) -> AppResult<EnrollmentGrantDto> {
    let g = &signed.grant;
    let member = SharingMember {
        user_id: g.anchor.user_id,
        device_id: g.anchor.device_id,
        encryption_public_key: g.anchor.encryption_public_key.clone(),
        signing_public_key: g.anchor.signing_public_key.clone(),
        role: g.role_ceiling,
    };
    Ok(EnrollmentGrantDto {
        share_id: g.scope.share_id.to_string(),
        grant_id: g.grant_id.to_string(),
        revision: g.grant_revision,
        anchor: identity_dto(g.scope.server_instance_id, &member)?,
        role_ceiling: role_dto(g.role_ceiling),
        mode: match g.mode {
            EnrollmentMode::Manual => EnrollmentModeDto::Manual,
            EnrollmentMode::Automatic => EnrollmentModeDto::Automatic,
        },
        status: match g.status {
            EnrollmentGrantStatus::Active => EnrollmentGrantStatusDto::Active,
            EnrollmentGrantStatus::Revoked => EnrollmentGrantStatusDto::Revoked,
        },
        not_before: g.not_before,
        expires_at: g.expires_at,
        max_admissions: g.max_admissions,
        admitted_count: g.admitted_count,
        frozen: mark.frozen,
        blocked_reason: if mark.frozen {
            Some("enrollment_frozen".into())
        } else if clock() >= g.expires_at {
            Some("enrollment_expired".into())
        } else {
            None
        },
    })
}
fn role_dto(role: SharingRole) -> SharingRoleDto {
    match role {
        SharingRole::Reader => SharingRoleDto::Reader,
        SharingRole::Editor => SharingRoleDto::Editor,
    }
}
fn role_wire(role: SharingRoleDto) -> SharingRole {
    match role {
        SharingRoleDto::Reader => SharingRole::Reader,
        SharingRoleDto::Editor => SharingRole::Editor,
    }
}
fn request_dto(stored: &StoredRequest) -> AppResult<EnrollmentRequestDto> {
    let v = verify_bundle(&stored.bundle, historical_time(&stored.bundle))?;
    let request = v
        .request
        .as_ref()
        .ok_or_else(|| invalid("missing enrollment request"))?;
    let r = &request.signed().request;
    let member = SharingMember {
        user_id: r.target.user_id,
        device_id: r.target.device_id,
        encryption_public_key: r.target.encryption_public_key.clone(),
        signing_public_key: r.target.signing_public_key.clone(),
        role: r.requested_role,
    };
    Ok(EnrollmentRequestDto {
        share_id: r.scope.share_id.to_string(),
        grant_id: v.grant.signed().grant.grant_id.to_string(),
        request_id: r.request_id.to_string(),
        target: identity_dto(r.scope.server_instance_id, &member)?,
        requested_role: role_dto(r.requested_role),
        comparison_code: enrollment_pairing_code(request).map_err(enrollment_error)?,
        status: match stored.state.status {
            OwnDeviceRequestStatus::Pending => EnrollmentRequestStatusDto::Pending,
            OwnDeviceRequestStatus::Challenged => EnrollmentRequestStatusDto::Challenged,
            OwnDeviceRequestStatus::Responded => EnrollmentRequestStatusDto::Responded,
            OwnDeviceRequestStatus::Accepted => EnrollmentRequestStatusDto::Accepted,
            OwnDeviceRequestStatus::Denied => EnrollmentRequestStatusDto::Denied,
            OwnDeviceRequestStatus::Expired => EnrollmentRequestStatusDto::Expired,
        },
        expires_at: r.expires_at,
        challenge_generation: stored.checkpoint.as_ref().map(|c| c.generation),
        blocked_reason: None,
    })
}

fn bounded_time(now: i64, not_before: i64, expires_at: i64) -> i64 {
    if expires_at <= not_before {
        not_before
    } else {
        now.max(not_before).min(expires_at.saturating_sub(1))
    }
}
fn historical_time(bundle: &PublicBundle) -> i64 {
    bundle.request.as_ref().map_or(clock(), |r| {
        bounded_time(clock(), r.request.not_before, r.request.expires_at)
    })
}
fn highwater(env: &Environment) -> EnrollmentHighwater {
    EnrollmentHighwater::new(
        env.session.profile_id,
        env.session.ctx.secure.clone(),
        env.pin.instance,
    )
}
fn validate_record(record: &EnrollmentRecord) -> AppResult<()> {
    if record.format != 1
        || record.grants.len() > MAX_RETAINED_GRANTS_PER_SHARE
        || record.requests.len() > MAX_RETAINED_REQUESTS_PER_SHARE
    {
        return Err(invalid("enrollment transcript exceeds the limit"));
    }
    validate_scope(&record.scope).map_err(enrollment_error)?;
    let owner = owner_from(&record.owner, record.scope.server_instance_id)?;
    let accesses = verified_manifests(&record.manifests, &owner)?;
    let access = accesses
        .last()
        .ok_or_else(|| invalid("missing enrollment access"))?;
    verify_shared_mutation(access, &record.header, None).map_err(enrollment_error)?;
    let mut grants = std::collections::BTreeSet::new();
    for stored in &record.grants {
        let verified = verify_grant_chain(&stored.history, &owner, &accesses, clock())?;
        if verified.signed().grant.scope != record.scope
            || !grants.insert(verified.signed().grant.grant_id)
        {
            return Err(invalid("enrollment grants differ from their scope"));
        }
        let mut mark = GrantMark::authenticated(&verified).map_err(mark_error)?;
        mark.frozen = stored.mark.frozen;
        if mark != stored.mark {
            return Err(AppError::SharingReconciliationRequired);
        }
    }
    let mut requests = std::collections::BTreeSet::new();
    for request in &record.requests {
        let verified = verify_bundle(&request.bundle, historical_time(&request.bundle))?;
        let signed = verified
            .request
            .as_ref()
            .ok_or_else(|| invalid("missing enrollment request"))?;
        if signed.signed().request.scope != record.scope
            || verified.owner != owner
            || request.state.request != *signed.signed()
            || request.state.endorsement
                != *verified
                    .endorsement
                    .as_ref()
                    .ok_or_else(|| invalid("missing anchor endorsement"))?
                    .signed()
            || request.state.grant_id != verified.grant.signed().grant.grant_id
            || !requests.insert(signed.signed().request.request_id)
        {
            return Err(invalid(
                "enrollment request differs from its pinned transcript",
            ));
        }
        match (&request.state.challenge, &request.checkpoint) {
            (Some(challenge), Some(checkpoint)) => {
                let at = bounded_time(
                    historical_time(&request.bundle),
                    challenge.challenge.not_before,
                    challenge.challenge.expires_at,
                );
                let verified_challenge = verify_device_challenge(
                    challenge,
                    &owner,
                    signed,
                    verified.endorsement.as_ref().unwrap(),
                    &verified.access,
                    at,
                    None,
                )
                .map_err(enrollment_error)?;
                if verified_challenge.checkpoint()
                    != EnrollmentChallengeCheckpoint::from(checkpoint)
                {
                    return Err(AppError::SharingReconciliationRequired);
                }
                if let Some(response) = &request.state.response {
                    verify_device_response(
                        response,
                        signed,
                        &verified_challenge,
                        &verified.access,
                        at,
                    )
                    .map_err(enrollment_error)?;
                }
            }
            (None, None) => {}
            _ => return Err(AppError::SharingReconciliationRequired),
        }
        if let Some(pending) = &request.pending_acceptance {
            validate_accept_request(pending).map_err(enrollment_error)?;
            if pending.rotation.access.manifest.share_id != record.scope.share_id {
                return Err(invalid(
                    "pending enrollment acceptance differs from its scope",
                ));
            }
        }
        if let Some(receipt) = &request.state.acceptance {
            verify_stored_receipt(&verified, request, receipt)?;
        }
    }
    Ok(())
}
fn transcript_hash(result: Result<Vec<u8>, EnrollmentValidationError>) -> AppResult<[u8; 32]> {
    Ok(cc_crypto_core::request_body_sha256(
        &result.map_err(enrollment_error)?,
    ))
}
fn verify_stored_receipt(
    verified: &VerifiedBundle,
    stored: &StoredRequest,
    receipt: &SignedSharingOwnDeviceAcceptance,
) -> AppResult<()> {
    verify_acceptance_signature(receipt, &verified.owner).map_err(enrollment_error)?;
    let request = verified
        .request
        .as_ref()
        .ok_or_else(|| invalid("receipt requires a paired request"))?;
    let endorsement = verified
        .endorsement
        .as_ref()
        .ok_or_else(|| invalid("receipt requires an anchor endorsement"))?;
    let challenge = stored
        .state
        .challenge
        .as_ref()
        .ok_or_else(|| invalid("receipt requires a signed challenge"))?;
    let response = stored
        .state
        .response
        .as_ref()
        .ok_or_else(|| invalid("receipt requires a signed response"))?;
    let a = &receipt.acceptance;
    if a.request_hash.as_slice() != request.hash()
        || a.anchor_endorsement_hash.as_slice() != endorsement.hash()
        || a.consumed_grant_state_hash.as_slice() != verified.grant.hash()
        || a.challenge_hash.as_slice()
            != transcript_hash(enrollment_challenge_hash_input(challenge))?
        || a.response_hash.as_slice() != transcript_hash(enrollment_response_hash_input(response))?
    {
        return Err(AppError::SharingReconciliationRequired);
    }
    Ok(())
}
fn check_marks(marks: &EnrollmentHighwater, record: &EnrollmentRecord) -> AppResult<()> {
    let owner = owner_from(&record.owner, record.scope.server_instance_id)?;
    marks
        .pin_owner(&record.scope, &owner, true)
        .map_err(mark_error)?;
    let known_grants: std::collections::BTreeSet<_> = marks
        .known_grants(&record.scope)
        .map_err(mark_error)?
        .into_iter()
        .collect();
    let db_grants: std::collections::BTreeSet<_> = record
        .grants
        .iter()
        .map(|g| g.history.last().unwrap().grant.grant_id)
        .collect();
    let known_requests: std::collections::BTreeSet<_> = marks
        .known_requests(&record.scope)
        .map_err(mark_error)?
        .into_iter()
        .collect();
    let db_requests: std::collections::BTreeSet<_> = record
        .requests
        .iter()
        .map(|r| r.state.request.request.request_id)
        .collect();
    if known_grants != db_grants || known_requests != db_requests {
        return Err(AppError::SharingReconciliationRequired);
    }
    for grant in &record.grants {
        marks
            .check_grant(
                &record.scope,
                grant.history.last().unwrap().grant.grant_id,
                Some(&grant.mark),
            )
            .map_err(mark_error)?;
    }
    for request in &record.requests {
        let id = request.state.request.request.request_id;
        let hash = transcript_hash(enrollment_request_hash_input(&request.state.request))?;
        if marks
            .load_request_hash(&record.scope, id)
            .map_err(mark_error)?
            != Some(hash)
        {
            return Err(AppError::SharingReconciliationRequired);
        }
        if let Some(expected) = marks
            .load_pending_acceptance_hash(&record.scope, id)
            .map_err(mark_error)?
        {
            let actual = request
                .pending_acceptance
                .as_ref()
                .map(|p| &p.acceptance)
                .or(request.state.acceptance.as_ref())
                .map(|a| transcript_hash(enrollment_acceptance_hash_input(a)))
                .transpose()?;
            if actual != Some(expected) {
                return Err(AppError::SharingReconciliationRequired);
            }
        } else if request.pending_acceptance.is_some() {
            return Err(AppError::SharingReconciliationRequired);
        }
        marks
            .check_challenge(
                &record.scope,
                request.state.request.request.request_id,
                request
                    .checkpoint
                    .as_ref()
                    .map(EnrollmentChallengeCheckpoint::from)
                    .as_ref(),
            )
            .map_err(mark_error)?;
    }
    Ok(())
}
impl AppCore {
    async fn enrollment_transport(
        &self,
        env: &Environment,
        scope: EnrollmentScope,
        active: bool,
    ) -> AppResult<OwnDeviceEnrollmentApi> {
        if scope.server_instance_id != env.pin.instance {
            return Err(invalid("enrollment belongs to another server instance"));
        }
        if active {
            env.require_kind(scope.kind)?;
            if !env.pin.supports_owner_online_enrollment_v1 {
                return Err(AppError::Unsupported(
                    "owner-online enrollment is disabled".into(),
                ));
            }
        }
        let transport = env
            .wait(env.session.api()?.sharing_enrollment(scope))
            .await?;
        self.sharing_check(env).await?;
        Ok(transport)
    }
    async fn enrollment_load(
        &self,
        env: &Environment,
        id: ShareId,
    ) -> AppResult<Option<RecordState>> {
        let marks = highwater(env);
        let key = record_key(id);
        let instance = env.pin.instance;
        let result = env
            .wait(env.session.storage.call(move |db| -> AppResult<_> {
                let _lock = marks.lock().map_err(mark_error)?;
                db.read(|tx| -> AppResult<_> {
                    let serialized: Option<String> = tx.setting_get(&key)?;
                    let Some(raw) = &serialized else {
                        return Ok(None);
                    };
                    if raw.len() > MAX_ENROLLMENT_RECORD_BYTES {
                        return Err(invalid("enrollment transcript exceeds the limit"));
                    }
                    let value: EnrollmentRecord = serde_json::from_str(raw)
                        .map_err(|_| invalid("invalid encrypted enrollment transcript"))?;
                    if value.scope.server_instance_id != instance || value.scope.share_id != id {
                        return Err(invalid("enrollment transcript belongs to another scope"));
                    }
                    validate_record(&value)?;
                    check_marks(&marks, &value)?;
                    Ok(Some(RecordState { value, serialized }))
                })
            }))
            .await?;
        self.sharing_check(env).await?;
        Ok(result)
    }
    async fn enrollment_save(&self, env: &Environment, state: &mut RecordState) -> AppResult<()> {
        self.sharing_check(env).await?;
        validate_record(&state.value)?;
        let replacement = serde_json::to_string(&state.value)
            .map_err(|_| invalid("cannot encode enrollment transcript"))?;
        if replacement.len() > MAX_ENROLLMENT_RECORD_BYTES {
            return Err(invalid("enrollment transcript exceeds the limit"));
        }
        let expected = state.serialized.clone();
        let value = state.value.clone();
        let next = replacement.clone();
        let marks = highwater(env);
        let key = record_key(value.scope.share_id);
        let shutdown = env.unlocked.sharing_shutdown.subscribe();
        env.wait(env.session.storage.call(move |db| -> AppResult<()> {
            let _lock = marks.lock().map_err(mark_error)?;
            db.write(|tx| -> AppResult<()> {
                if *shutdown.borrow() {
                    return Err(AppError::VaultLocked);
                }
                let actual: Option<String> = tx.setting_get(&key)?;
                if actual != expected {
                    return Err(invalid("enrollment transcript changed locally; reload"));
                }
                let old: Option<EnrollmentRecord> = actual
                    .as_ref()
                    .map(|raw| {
                        serde_json::from_str(raw)
                            .map_err(|_| invalid("invalid enrollment transcript"))
                    })
                    .transpose()?;
                if let Some(old) = &old {
                    validate_record(old)?;
                    check_marks(&marks, old)?;
                }
                if old.is_none() && marks.owner_exists(&value.scope).map_err(mark_error)? {
                    return Err(AppError::SharingReconciliationRequired);
                }
                let owner = owner_from(&value.owner, value.scope.server_instance_id)?;
                marks
                    .pin_owner(&value.scope, &owner, old.is_some())
                    .map_err(mark_error)?;
                if let Some(old) = &old {
                    if old.scope != value.scope
                        || old.owner != value.owner
                        || !value.manifests.starts_with(&old.manifests)
                        || old.grants.iter().any(|g| {
                            !value
                                .grants
                                .iter()
                                .any(|n| n.history.starts_with(&g.history))
                        })
                        || old.requests.iter().any(|r| {
                            !value
                                .requests
                                .iter()
                                .any(|n| n.state.request == r.state.request)
                        })
                    {
                        return Err(AppError::SharingReconciliationRequired);
                    }
                }
                for grant in &value.grants {
                    let id = grant.history.last().unwrap().grant.grant_id;
                    let prior = old
                        .as_ref()
                        .and_then(|r| {
                            r.grants
                                .iter()
                                .find(|g| g.history.last().unwrap().grant.grant_id == id)
                        })
                        .map(|g| &g.mark);
                    if prior == Some(&grant.mark) {
                        continue;
                    }
                    let committed = if let Some(prior) = prior {
                        let accesses = verified_manifests(&value.manifests, &owner)?;
                        let mut checkpoint = prior.clone();
                        for length in (prior.revision as usize + 1)..=grant.history.len() {
                            let verified = verify_grant_chain(
                                &grant.history[..length],
                                &owner,
                                &accesses,
                                clock(),
                            )?;
                            let mut next =
                                GrantMark::authenticated(&verified).map_err(mark_error)?;
                            next.frozen = prior.frozen;
                            checkpoint = marks
                                .cas_grant(&value.scope, id, Some(&checkpoint), next)
                                .map_err(mark_error)?;
                        }
                        checkpoint
                    } else {
                        marks
                            .cas_grant(&value.scope, id, None, grant.mark.clone())
                            .map_err(mark_error)?
                    };
                    if committed != grant.mark {
                        return Err(AppError::SharingReconciliationRequired);
                    }
                }
                for request in &value.requests {
                    let id = request.state.request.request.request_id;
                    let old_request = old.as_ref().and_then(|r| {
                        r.requests
                            .iter()
                            .find(|r| r.state.request.request.request_id == id)
                    });
                    marks
                        .pin_request(
                            &value.scope,
                            id,
                            transcript_hash(enrollment_request_hash_input(&request.state.request))?,
                            old_request.is_some(),
                        )
                        .map_err(mark_error)?;
                    if let Some(pending) = &request.pending_acceptance {
                        marks
                            .pin_pending_acceptance(
                                &value.scope,
                                id,
                                transcript_hash(enrollment_acceptance_hash_input(
                                    &pending.acceptance,
                                ))?,
                            )
                            .map_err(mark_error)?;
                    }
                    if let Some(expected) = marks
                        .load_pending_acceptance_hash(&value.scope, id)
                        .map_err(mark_error)?
                    {
                        let actual = request
                            .pending_acceptance
                            .as_ref()
                            .map(|p| &p.acceptance)
                            .or(request.state.acceptance.as_ref())
                            .map(|a| transcript_hash(enrollment_acceptance_hash_input(a)))
                            .transpose()?;
                        if actual != Some(expected) {
                            return Err(AppError::SharingReconciliationRequired);
                        }
                    }
                    let previous = old
                        .as_ref()
                        .and_then(|r| {
                            r.requests
                                .iter()
                                .find(|r| r.state.request.request.request_id == id)
                        })
                        .and_then(|r| r.checkpoint.as_ref())
                        .map(EnrollmentChallengeCheckpoint::from);
                    if let Some(mark) = &request.checkpoint {
                        marks
                            .cas_challenge(&value.scope, id, previous.as_ref(), mark.into())
                            .map_err(mark_error)?;
                    } else if previous.is_some() {
                        return Err(AppError::SharingReconciliationRequired);
                    }
                }
                tx.setting_set(&key, &next)?;
                if *shutdown.borrow() {
                    return Err(AppError::VaultLocked);
                }
                Ok(())
            })
        }))
        .await?;
        self.sharing_check(env).await?;
        state.serialized = Some(replacement);
        Ok(())
    }
    async fn enrollment_current(
        &self,
        env: &Environment,
        id: ShareId,
    ) -> AppResult<SharingSnapshot> {
        let binding = env.binding(id)?;
        env.ensure_not_pending(id)?;
        let current = env.wait(env.transport()?.get_share(id)).await?;
        self.sharing_check(env).await?;
        self.sharing_refresh_state(env, &env.store()?, &binding, current)
            .await
    }
    async fn enrollment_record_from_snapshot(
        &self,
        env: &Environment,
        snapshot: &SharingSnapshot,
    ) -> AppResult<RecordState> {
        let scope = scope_from(snapshot);
        let mut record = match self.enrollment_load(env, scope.share_id).await? {
            Some(record) => record,
            None => RecordState {
                value: EnrollmentRecord {
                    format: 1,
                    scope,
                    owner: owner_dto(snapshot.state())?,
                    manifests: vec![],
                    header: snapshot.state().revision.signed.clone(),
                    grants: vec![],
                    requests: vec![],
                },
                serialized: None,
            },
        };
        if record.value.manifests.last() != Some(&snapshot.state().access) {
            let mut cursor = record
                .value
                .manifests
                .last()
                .map_or(0, |access| access.manifest.revision);
            for _ in 0..MAX_HISTORY_PAGES {
                if cursor >= snapshot.state().access.manifest.revision {
                    break;
                }
                let page = env
                    .wait(env.transport()?.share_history(
                        snapshot.binding().share_id,
                        cursor,
                        snapshot.state().revision.signed.mutation.context.revision,
                        100,
                    ))
                    .await?;
                self.sharing_check(env).await?;
                let before = cursor;
                for access in page.manifests {
                    if access.manifest.revision <= snapshot.state().access.manifest.revision {
                        cursor = access.manifest.revision;
                        record.value.manifests.push(access);
                    }
                }
                if before == cursor {
                    return Err(AppError::SharingReconciliationRequired);
                }
            }
        }
        record.value.header = snapshot.state().revision.signed.clone();
        if record.value.manifests.last() != Some(&snapshot.state().access) {
            return Err(AppError::SharingReconciliationRequired);
        }
        validate_record(&record.value)?;
        Ok(record)
    }
}

fn owner_dto(state: &SharedItemState) -> AppResult<SharingIdentityDto> {
    let access = &state.access.manifest;
    let owner = access
        .members
        .iter()
        .find(|m| m.device_id == access.owner_device_id)
        .ok_or_else(|| invalid("missing shared owner"))?;
    identity_dto(access.server_instance_id, owner)
}
fn stored_grant(record: &EnrollmentRecord, id: Uuid) -> AppResult<&StoredGrant> {
    record
        .grants
        .iter()
        .find(|g| g.history.last().unwrap().grant.grant_id == id)
        .ok_or_else(|| AppError::not_found("enrollment grant", id))
}
fn source_bundle(record: &EnrollmentRecord, id: Uuid) -> AppResult<PublicBundle> {
    Ok(PublicBundle {
        format: 1,
        owner: record.owner.clone(),
        manifests: record.manifests.clone(),
        header: record.header.clone(),
        grant_history: stored_grant(record, id)?.history.clone(),
        request: None,
        endorsement: None,
    })
}
fn grant_active(record: &EnrollmentRecord, id: Uuid) -> AppResult<()> {
    let grant = stored_grant(record, id)?;
    if grant.mark.frozen || grant.mark.status != EnrollmentGrantStatus::Active {
        return Err(invalid("enrollment grant is frozen or terminal"));
    }
    Ok(())
}
fn check_owned(env: &Environment, record: &EnrollmentRecord) -> AppResult<()> {
    let owner = owner_from(&record.owner, env.pin.instance)?;
    if owner != env.own_anchor() {
        return Err(invalid(
            "only the original owning device can authorize enrollment",
        ));
    }
    Ok(())
}
fn parse_share(id: &str) -> AppResult<ShareId> {
    ShareId::from_str(id).map_err(|_| invalid("invalid shared item id"))
}
fn parse_uuid(id: &str) -> AppResult<Uuid> {
    let id = Uuid::parse_str(id).map_err(|_| invalid("invalid enrollment id"))?;
    if id.is_nil() {
        Err(invalid("invalid enrollment id"))
    } else {
        Ok(id)
    }
}
impl AppCore {
    async fn enrollment_refresh_grants(
        &self,
        env: &Environment,
        record: &mut RecordState,
        transport: &OwnDeviceEnrollmentApi,
    ) -> AppResult<()> {
        let mut seen = std::collections::BTreeSet::new();
        let mut cursor = None;
        for _ in 0..MAX_HISTORY_PAGES {
            let page = env.wait(transport.list_grants(cursor, 100)).await?;
            self.sharing_check(env).await?;
            for signed in page.items {
                seen.insert(signed.grant.grant_id);
                self.enrollment_refresh_grant(env, record, transport, signed)
                    .await?;
            }
            if !page.has_more {
                // The target feed may intentionally omit grants. Independently
                // retained IDs still require their own authenticated history;
                // omission never silently downgrades a receipt/checkpoint.
                let retained = record
                    .value
                    .grants
                    .iter()
                    .map(|g| g.history.last().unwrap().grant.grant_id)
                    .filter(|id| !seen.contains(id))
                    .collect::<Vec<_>>();
                for id in retained {
                    let signed = env.wait(transport.get_grant(id)).await?;
                    self.sharing_check(env).await?;
                    self.enrollment_refresh_grant(env, record, transport, signed)
                        .await?;
                }
                return Ok(());
            }
            cursor = page.next_after;
        }
        Err(invalid(
            "enrollment grant listing exceeds the bounded page limit",
        ))
    }
    async fn enrollment_refresh_grant(
        &self,
        env: &Environment,
        record: &mut RecordState,
        transport: &OwnDeviceEnrollmentApi,
        signed: SignedSharingOwnDevicesGrantState,
    ) -> AppResult<()> {
        let owner = owner_from(&record.value.owner, env.pin.instance)?;
        let accesses = verified_manifests(&record.value.manifests, &owner)?;
        let id = signed.grant.grant_id;
        let index = record
            .value
            .grants
            .iter()
            .position(|g| g.history.last().unwrap().grant.grant_id == id);
        let after = index.map_or(0, |i| record.value.grants[i].mark.revision);
        if after == signed.grant.grant_revision {
            if record.value.grants[index.unwrap()].history.last() != Some(&signed) {
                return Err(AppError::SharingReconciliationRequired);
            }
            return Ok(());
        }
        let mut history = index.map_or_else(Vec::new, |i| record.value.grants[i].history.clone());
        let mut history_cursor = after;
        for _ in 0..MAX_HISTORY_PAGES {
            if history_cursor >= signed.grant.grant_revision {
                break;
            }
            let page = env
                .wait(transport.grant_history(id, history_cursor, 100))
                .await?;
            self.sharing_check(env).await?;
            let before = history_cursor;
            for state in page.states {
                if state.grant.grant_revision <= signed.grant.grant_revision {
                    history_cursor = state.grant.grant_revision;
                    history.push(state);
                }
            }
            if before == history_cursor {
                return Err(AppError::SharingReconciliationRequired);
            }
        }
        if history.last() != Some(&signed) {
            return Err(AppError::SharingReconciliationRequired);
        }
        let verified = verify_grant_chain(&history, &owner, &accesses, clock())?;
        let mut mark = GrantMark::authenticated(&verified).map_err(mark_error)?;
        if let Some(index) = index {
            mark.frozen = record.value.grants[index].mark.frozen;
            record.value.grants[index] = StoredGrant { history, mark };
        } else {
            record.value.grants.push(StoredGrant { history, mark });
        }
        Ok(())
    }
    pub async fn enrollment_list_grants(
        &self,
        share_id: String,
    ) -> AppResult<Vec<EnrollmentGrantDto>> {
        let session = self.session().await?;
        Box::pin(self.enrollment_list_grants_in(session, share_id)).await
    }
    async fn enrollment_list_grants_in(
        &self,
        session: Arc<Session>,
        share_id: String,
    ) -> AppResult<Vec<EnrollmentGrantDto>> {
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), true).await?;
        let id = parse_share(&share_id)?;
        let mut record = match self.enrollment_load(&env, id).await? {
            Some(r) => r,
            None => {
                let snapshot = self.enrollment_current(&env, id).await?;
                self.enrollment_record_from_snapshot(&env, &snapshot)
                    .await?
            }
        };
        if env.supports(record.value.scope.kind) && env.pin.supports_owner_online_enrollment_v1 {
            let snapshot = self.enrollment_current(&env, id).await?;
            record = self
                .enrollment_record_from_snapshot(&env, &snapshot)
                .await?;
        }
        let transport = self
            .enrollment_transport(&env, record.value.scope.clone(), false)
            .await?;
        self.enrollment_refresh_grants(&env, &mut record, &transport)
            .await?;
        self.enrollment_save(&env, &mut record).await?;
        record
            .value
            .grants
            .iter()
            .map(|g| grant_dto(g.history.last().unwrap(), &g.mark))
            .collect()
    }
    pub async fn enrollment_create_grant(
        &self,
        share_id: String,
        create: EnrollmentGrantCreateDto,
    ) -> AppResult<EnrollmentGrantDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), true).await?;
        let id = parse_share(&share_id)?;
        let snapshot = self.enrollment_current(&env, id).await?;
        let mut record = self
            .enrollment_record_from_snapshot(&env, &snapshot)
            .await?;
        check_owned(&env, &record.value)?;
        let transport = self
            .enrollment_transport(&env, record.value.scope.clone(), true)
            .await?;
        let anchor = owner_from(&create.anchor, env.pin.instance)?;
        if !code_matches(
            &create.anchor.verification_code,
            &create.confirmed_identity_code,
        ) {
            return Err(invalid(
                "confirm the complete independently compared anchor identity code",
            ));
        }
        let access = trusted_manifest(&snapshot)?;
        let member = access
            .member(anchor.device_id)
            .ok_or_else(|| invalid("anchor must already have explicit shared access"))?;
        if member.user_id != anchor.user_id
            || member.encryption_public_key != anchor.public_keys.encryption_bytes()
            || member.signing_public_key != anchor.public_keys.signing_bytes()
            || (member.role == SharingRole::Reader && create.role_ceiling == SharingRoleDto::Editor)
        {
            return Err(invalid(
                "anchor identity or role differs from current explicit access",
            ));
        }
        let now = clock();
        let state = SharingOwnDevicesGrantState {
            format: 1,
            scope: record.value.scope.clone(),
            owner_user_id: env.pin.user,
            owner_device_id: env.pin.device,
            grant_id: Uuid::new_v4(),
            grant_revision: 1,
            previous_grant_state_hash: Bytes::from([0; 32]),
            status: EnrollmentGrantStatus::Active,
            anchor: EnrollmentDeviceBinding {
                user_id: anchor.user_id,
                device_id: anchor.device_id,
                encryption_public_key: anchor.public_keys.encryption_bytes(),
                signing_public_key: anchor.public_keys.signing_bytes(),
            },
            access_manifest_hash: Bytes::from(access.hash()),
            access_epoch: access.manifest().access_epoch,
            role_ceiling: role_wire(create.role_ceiling),
            mode: match create.mode {
                EnrollmentModeDto::Manual => EnrollmentMode::Manual,
                EnrollmentModeDto::Automatic => EnrollmentMode::Automatic,
            },
            not_before: now,
            expires_at: create.expires_at,
            max_admissions: create.max_admissions,
            admitted_count: 0,
        };
        let signed = env
            .identity
            .sharing_sign_enrollment_grant(
                state,
                &env.own_anchor(),
                None,
                GrantTransition::Genesis { access: &access },
                now,
            )
            .map_err(enrollment_error)?;
        let verified = verify_grant_state(
            &signed,
            &env.own_anchor(),
            None,
            GrantTransition::Genesis { access: &access },
            now,
        )
        .map_err(enrollment_error)?;
        record.value.grants.push(StoredGrant {
            history: vec![signed.clone()],
            mark: GrantMark::authenticated(&verified).map_err(mark_error)?,
        });
        // Preserve the exact signed grant before dispatch. A lost response can
        // be retried by exporting/reading this head; never generate a new ID.
        self.enrollment_save(&env, &mut record).await?;
        env.wait(transport.publish_grant(&PublishOwnDevicesGrantRequest {
            grant: signed.clone(),
        }))
        .await?;
        self.sharing_check(&env).await?;
        grant_dto(
            &signed,
            &stored_grant(&record.value, signed.grant.grant_id)?.mark,
        )
    }
    pub async fn enrollment_revoke_grant(
        &self,
        share_id: String,
        grant_id: String,
    ) -> AppResult<EnrollmentGrantDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), false).await?;
        let id = parse_share(&share_id)?;
        let grant_id = parse_uuid(&grant_id)?;
        let mut record = self
            .enrollment_load(&env, id)
            .await?
            .ok_or_else(|| invalid("enrollment owner must already be pinned"))?;
        check_owned(&env, &record.value)?;
        // Freeze first, even if the network is unavailable or the server gate
        // was disabled. The signed terminal request can be retried afterwards.
        let key = record_key(id);
        let expected = record.serialized.clone();
        let mut replacement = record.value.clone();
        let marks = highwater(&env);
        let index = replacement
            .grants
            .iter()
            .position(|g| g.history.last().unwrap().grant.grant_id == grant_id)
            .ok_or_else(|| AppError::not_found("enrollment grant", grant_id))?;
        let current = replacement.grants[index].mark.clone();
        let shutdown = env.unlocked.sharing_shutdown.subscribe();
        let (frozen, next) = env
            .wait(env.session.storage.call(move |db| -> AppResult<_> {
                let _lock = marks.lock().map_err(mark_error)?;
                db.write(|tx| -> AppResult<_> {
                    if *shutdown.borrow() {
                        return Err(AppError::VaultLocked);
                    }
                    let actual: Option<String> = tx.setting_get(&key)?;
                    if actual != expected {
                        return Err(AppError::SharingReconciliationRequired);
                    }
                    check_marks(&marks, &replacement)?;
                    replacement.grants[index].mark = marks
                        .freeze_grant(&replacement.scope, grant_id, &current)
                        .map_err(mark_error)?;
                    let next = serde_json::to_string(&replacement)
                        .map_err(|_| invalid("cannot encode enrollment freeze"))?;
                    tx.setting_set(&key, &next)?;
                    Ok((replacement, next))
                })
            }))
            .await?;
        self.sharing_check(&env).await?;
        record.value = frozen;
        record.serialized = Some(next);
        let transport = self
            .enrollment_transport(&env, record.value.scope.clone(), false)
            .await?;
        let stored = stored_grant(&record.value, grant_id)?.clone();
        if stored.mark.status == EnrollmentGrantStatus::Revoked {
            let signed = stored.history.last().unwrap();
            env.wait(transport.publish_grant(&PublishOwnDevicesGrantRequest {
                grant: signed.clone(),
            }))
            .await?;
            self.sharing_check(&env).await?;
            return grant_dto(signed, &stored.mark);
        }
        let observed = env.wait(transport.get_grant(grant_id)).await?;
        self.sharing_check(&env).await?;
        if stored.history.last() != Some(&observed) {
            // Preserve the local freeze without signing a competing successor
            // from a stale head after an in-flight admission/lost response.
            return Err(AppError::SharingReconciliationRequired);
        }
        let owner = owner_from(&record.value.owner, env.pin.instance)?;
        let accesses = verified_manifests(&record.value.manifests, &owner)?;
        let prior = verify_grant_chain(&stored.history, &owner, &accesses, clock())?;
        let mut state = prior.signed().grant.clone();
        state.grant_revision += 1;
        state.previous_grant_state_hash = Bytes::from(prior.hash());
        state.status = EnrollmentGrantStatus::Revoked;
        let access = accesses.last().unwrap();
        let signed = env
            .identity
            .sharing_sign_enrollment_grant(
                state,
                &owner,
                Some(&prior),
                GrantTransition::Revoke { access },
                clock(),
            )
            .map_err(enrollment_error)?;
        let verified = verify_grant_state(
            &signed,
            &owner,
            Some(&prior),
            GrantTransition::Revoke { access },
            clock(),
        )
        .map_err(enrollment_error)?;
        let target = record
            .value
            .grants
            .iter_mut()
            .find(|g| g.history.last().unwrap().grant.grant_id == grant_id)
            .unwrap();
        target.history.push(signed.clone());
        target.mark = GrantMark::authenticated(&verified).map_err(mark_error)?;
        target.mark.frozen = true;
        self.enrollment_save(&env, &mut record).await?;
        env.wait(transport.publish_grant(&PublishOwnDevicesGrantRequest {
            grant: signed.clone(),
        }))
        .await?;
        self.sharing_check(&env).await?;
        grant_dto(&signed, &stored_grant(&record.value, grant_id)?.mark)
    }
    pub async fn enrollment_export_anchor_bundle(
        &self,
        share_id: String,
        grant_id: String,
    ) -> AppResult<EnrollmentPairingDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), true).await?;
        let id = parse_share(&share_id)?;
        let grant_id = parse_uuid(&grant_id)?;
        let snapshot = self.enrollment_current(&env, id).await?;
        let mut record = self
            .enrollment_record_from_snapshot(&env, &snapshot)
            .await?;
        let transport = self
            .enrollment_transport(&env, record.value.scope.clone(), true)
            .await?;
        self.enrollment_refresh_grants(&env, &mut record, &transport)
            .await?;
        grant_active(&record.value, grant_id)?;
        self.enrollment_save(&env, &mut record).await?;
        pairing_dto(&source_bundle(&record.value, grant_id)?)
    }
    pub async fn enrollment_prepare_target(
        &self,
        public_bundle_json: String,
        requested_role: SharingRoleDto,
    ) -> AppResult<EnrollmentPairingDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), true).await?;
        let mut bundle = parse_bundle(&public_bundle_json)?;
        if bundle.request.is_some() || bundle.endorsement.is_some() {
            return Err(invalid("prepare target requires an anchor source bundle"));
        }
        let verified = verify_bundle(&bundle, clock())?;
        self.enrollment_transport(&env, verified.grant.signed().grant.scope.clone(), true)
            .await?;
        let now = clock();
        let mut nonce = [0; 32];
        cc_crypto_core::fill_random(&mut nonce)?;
        let request = SharingOwnDeviceRequest {
            format: 1,
            scope: verified.grant.signed().grant.scope.clone(),
            request_id: Uuid::new_v4(),
            grant_state_hash: Bytes::from(verified.grant.hash()),
            access_manifest_hash: Bytes::from(verified.access.hash()),
            access_epoch: verified.access.manifest().access_epoch,
            target: EnrollmentDeviceBinding {
                user_id: env.pin.user,
                device_id: env.pin.device,
                encryption_public_key: env.pin.encryption.clone(),
                signing_public_key: env.pin.signing.clone(),
            },
            requested_role: role_wire(requested_role),
            nonce: Bytes::from(nonce),
            not_before: now,
            expires_at: (now + MAX_REQUEST_LIFETIME_SECONDS)
                .min(verified.grant.signed().grant.expires_at),
        };
        bundle.request = Some(
            env.identity
                .sharing_sign_enrollment_request(request, &verified.grant, &verified.access, now)
                .map_err(enrollment_error)?,
        );
        self.sharing_check(&env).await?;
        // No pin or local trust is written until the independently trusted
        // anchor endorses and the target confirms the whole-request code.
        pairing_dto(&bundle)
    }
    pub async fn enrollment_endorse_target(
        &self,
        public_bundle_json: String,
        confirmed_whole_code: String,
    ) -> AppResult<EnrollmentPairingDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), true).await?;
        let mut bundle = parse_bundle(&public_bundle_json)?;
        let verified = verify_bundle(&bundle, clock())?;
        let scope = verified.grant.signed().grant.scope.clone();
        let snapshot = self.enrollment_current(&env, scope.share_id).await?;
        if snapshot.owner_anchor() != verified.owner
            || snapshot.state().access != *bundle.manifests.last().unwrap()
            || snapshot.revision_checkpoint() != verified.revision.checkpoint()
        {
            return Err(AppError::SharingReconciliationRequired);
        }
        let mut record = self
            .enrollment_record_from_snapshot(&env, &snapshot)
            .await?;
        let transport = self.enrollment_transport(&env, scope, true).await?;
        self.enrollment_refresh_grants(&env, &mut record, &transport)
            .await?;
        let grant_id = verified.grant.signed().grant.grant_id;
        grant_active(&record.value, grant_id)?;
        if stored_grant(&record.value, grant_id)?.history.last() != Some(verified.grant.signed()) {
            return Err(AppError::SharingReconciliationRequired);
        }
        let request = verified
            .request
            .as_ref()
            .ok_or_else(|| invalid("missing signed target request"))?;
        bundle.endorsement = Some(
            env.identity
                .sharing_endorse_enrollment_request(
                    request,
                    &verified.access,
                    &confirmed_whole_code,
                    clock(),
                )
                .map_err(enrollment_error)?,
        );
        self.enrollment_save(&env, &mut record).await?;
        self.sharing_check(&env).await?;
        pairing_dto(&bundle)
    }
    pub async fn enrollment_submit_target(
        &self,
        public_bundle_json: String,
        confirmed_whole_code: String,
    ) -> AppResult<EnrollmentRequestDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), true).await?;
        let bundle = parse_bundle(&public_bundle_json)?;
        let verified = verify_bundle(&bundle, clock())?;
        let request = verified
            .request
            .as_ref()
            .ok_or_else(|| invalid("missing signed target request"))?;
        let endorsement = verified
            .endorsement
            .as_ref()
            .ok_or_else(|| invalid("trusted anchor endorsement is required"))?;
        if request.signed().request.target.device_id != env.pin.device
            || request.signed().request.target.user_id != env.pin.user
            || request.signed().request.target.encryption_public_key != env.pin.encryption
            || request.signed().request.target.signing_public_key != env.pin.signing
            || !code_matches(
                &enrollment_pairing_code(request).map_err(enrollment_error)?,
                &confirmed_whole_code,
            )
        {
            return Err(invalid(
                "confirm this device's whole-request code with the trusted anchor",
            ));
        }
        let scope = request.signed().request.scope.clone();
        let transport = self.enrollment_transport(&env, scope.clone(), true).await?;
        let mut record = match self.enrollment_load(&env, scope.share_id).await? {
            Some(record) => record,
            None => RecordState {
                value: EnrollmentRecord {
                    format: 1,
                    scope,
                    owner: bundle.owner.clone(),
                    manifests: bundle.manifests.clone(),
                    header: bundle.header.clone(),
                    grants: vec![StoredGrant {
                        history: bundle.grant_history.clone(),
                        mark: GrantMark::authenticated(&verified.grant).map_err(mark_error)?,
                    }],
                    requests: vec![],
                },
                serialized: None,
            },
        };
        grant_active(&record.value, verified.grant.signed().grant.grant_id)?;
        if stored_grant(&record.value, verified.grant.signed().grant.grant_id)?
            .history
            .last()
            != Some(verified.grant.signed())
        {
            return Err(AppError::SharingReconciliationRequired);
        }
        let submitted = SubmitOwnDeviceRequest {
            grant_id: verified.grant.signed().grant.grant_id,
            request: request.signed().clone(),
            endorsement: endorsement.signed().clone(),
        };
        let state = OwnDeviceRequestState {
            grant_id: submitted.grant_id,
            request: submitted.request.clone(),
            endorsement: submitted.endorsement.clone(),
            status: OwnDeviceRequestStatus::Pending,
            challenge: None,
            response: None,
            acceptance: None,
        };
        if let Some(existing) = record
            .value
            .requests
            .iter()
            .find(|r| r.state.request.request.request_id == request.signed().request.request_id)
        {
            if existing.bundle != bundle {
                return Err(AppError::SharingReconciliationRequired);
            }
        } else {
            record.value.requests.push(StoredRequest {
                bundle,
                state,
                checkpoint: None,
                pending_acceptance: None,
            });
        }
        self.enrollment_save(&env, &mut record).await?;
        let observed = env.wait(transport.submit_request(&submitted)).await?;
        self.sharing_check(&env).await?;
        let target = record
            .value
            .requests
            .iter_mut()
            .find(|r| r.state.request.request.request_id == request.signed().request.request_id)
            .unwrap();
        target.state = observed;
        self.enrollment_save(&env, &mut record).await?;
        request_dto(
            record
                .value
                .requests
                .iter()
                .find(|r| r.state.request.request.request_id == request.signed().request.request_id)
                .unwrap(),
        )
    }
}

impl AppCore {
    pub async fn enrollment_inspect_pairing(
        &self,
        public_bundle_json: String,
    ) -> AppResult<EnrollmentPairingDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), true).await?;
        let bundle = parse_bundle(&public_bundle_json)?;
        let verified = verify_bundle(&bundle, clock())?;
        self.enrollment_transport(&env, verified.grant.signed().grant.scope.clone(), true)
            .await?;
        let anchor = &verified.grant.signed().grant.anchor;
        if anchor.user_id == env.pin.user && anchor.device_id == env.pin.device {
            let snapshot = self
                .enrollment_current(&env, verified.grant.signed().grant.scope.share_id)
                .await?;
            if snapshot.owner_anchor() != verified.owner
                || snapshot.state().access != *bundle.manifests.last().unwrap()
                || snapshot.revision_checkpoint() != verified.revision.checkpoint()
            {
                return Err(AppError::SharingReconciliationRequired);
            }
        }
        if let Some(request) = &verified.request {
            if request.signed().request.target.device_id == env.pin.device
                && (request.signed().request.target.user_id != env.pin.user
                    || request.signed().request.target.encryption_public_key != env.pin.encryption
                    || request.signed().request.target.signing_public_key != env.pin.signing)
            {
                return Err(invalid("pairing request differs from this device identity"));
            }
        }
        self.sharing_check(&env).await?;
        pairing_dto(&bundle)
    }
    fn enrollment_observe_request(
        &self,
        record: &mut RecordState,
        state: OwnDeviceRequestState,
    ) -> AppResult<usize> {
        let id = state.request.request.request_id;
        let index = record
            .value
            .requests
            .iter()
            .position(|r| r.state.request.request.request_id == id);
        let mut stored = match index {
            Some(i) => record.value.requests[i].clone(),
            None => {
                grant_active(&record.value, state.grant_id)?;
                let mut bundle = source_bundle(&record.value, state.grant_id)?;
                bundle.request = Some(state.request.clone());
                bundle.endorsement = Some(state.endorsement.clone());
                verify_bundle(&bundle, clock())?;
                StoredRequest {
                    bundle,
                    state: state.clone(),
                    checkpoint: None,
                    pending_acceptance: None,
                }
            }
        };
        if stored.state.request != state.request
            || stored.state.endorsement != state.endorsement
            || stored.state.grant_id != state.grant_id
        {
            return Err(AppError::SharingReconciliationRequired);
        }
        let verified = verify_bundle(&stored.bundle, historical_time(&stored.bundle))?;
        let request = verified.request.as_ref().unwrap();
        let endorsement = verified.endorsement.as_ref().unwrap();
        if let Some(challenge) = &state.challenge {
            let at = bounded_time(
                historical_time(&stored.bundle),
                challenge.challenge.not_before,
                challenge.challenge.expires_at,
            );
            let checkpoint = stored
                .checkpoint
                .as_ref()
                .map(EnrollmentChallengeCheckpoint::from);
            let verified_challenge = verify_device_challenge(
                challenge,
                &verified.owner,
                request,
                endorsement,
                &verified.access,
                at,
                checkpoint.as_ref(),
            )
            .map_err(enrollment_error)?;
            if let Some(response) = &state.response {
                verify_device_response(
                    response,
                    request,
                    &verified_challenge,
                    &verified.access,
                    at,
                )
                .map_err(enrollment_error)?;
            }
            stored.checkpoint = Some(verified_challenge.checkpoint().into());
        } else if stored.checkpoint.is_some() {
            return Err(AppError::SharingReconciliationRequired);
        }
        // Accepted is not trusted merely because the server says so. The
        // target adoption and owner receipt paths below verify signed results.
        if let Some(existing) = &stored.state.acceptance {
            if state.acceptance.as_ref() != Some(existing)
                || state.status != OwnDeviceRequestStatus::Accepted
            {
                return Err(AppError::SharingReconciliationRequired);
            }
        }
        stored.state = state;
        if let Some(receipt) = &stored.state.acceptance {
            verify_stored_receipt(&verified, &stored, receipt)?;
        }
        let index = match index {
            Some(i) => {
                record.value.requests[i] = stored;
                i
            }
            None => {
                record.value.requests.push(stored);
                record.value.requests.len() - 1
            }
        };
        Ok(index)
    }
    pub async fn enrollment_list_requests(
        &self,
        share_id: String,
    ) -> AppResult<Vec<EnrollmentRequestDto>> {
        let session = self.session().await?;
        Box::pin(self.enrollment_list_requests_in(session, share_id)).await
    }
    async fn enrollment_list_requests_in(
        &self,
        session: Arc<Session>,
        share_id: String,
    ) -> AppResult<Vec<EnrollmentRequestDto>> {
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), true).await?;
        let id = parse_share(&share_id)?;
        let mut record = self
            .enrollment_load(&env, id)
            .await?
            .ok_or_else(|| invalid("enrollment must first be independently paired"))?;
        let transport = self
            .enrollment_transport(&env, record.value.scope.clone(), true)
            .await?;
        if owner_from(&record.value.owner, env.pin.instance)? == env.own_anchor() {
            let snapshot = self.enrollment_current(&env, id).await?;
            record = self
                .enrollment_record_from_snapshot(&env, &snapshot)
                .await?;
            self.enrollment_refresh_grants(&env, &mut record, &transport)
                .await?;
        }
        let mut cursor = None;
        for _ in 0..MAX_HISTORY_PAGES {
            let page = env.wait(transport.list_requests(cursor, 100)).await?;
            self.sharing_check(&env).await?;
            for state in page.items {
                // A stale/frozen pending request is display metadata only; it
                // cannot silently reauthorize a known terminal grant.
                if stored_grant(&record.value, state.grant_id)?.mark.frozen
                    || stored_grant(&record.value, state.grant_id)?.mark.status
                        == EnrollmentGrantStatus::Revoked
                {
                    if record
                        .value
                        .requests
                        .iter()
                        .any(|r| r.state.request == state.request)
                    {
                        continue;
                    }
                    continue;
                }
                self.enrollment_observe_request(&mut record, state)?;
            }
            if !page.has_more {
                self.enrollment_save(&env, &mut record).await?;
                return record
                    .value
                    .requests
                    .iter()
                    .map(|r| {
                        let mut dto = request_dto(r)?;
                        if stored_grant(&record.value, r.state.grant_id)?.mark.frozen
                            && matches!(
                                r.state.status,
                                OwnDeviceRequestStatus::Pending
                                    | OwnDeviceRequestStatus::Challenged
                                    | OwnDeviceRequestStatus::Responded
                            )
                        {
                            dto.status = EnrollmentRequestStatusDto::Blocked;
                            dto.blocked_reason = Some("enrollment_frozen".into());
                        }
                        Ok(dto)
                    })
                    .collect();
            }
            cursor = page.next_after;
        }
        Err(invalid(
            "enrollment request listing exceeds the bounded page limit",
        ))
    }
    pub async fn enrollment_challenge(
        &self,
        share_id: String,
        request_id: String,
    ) -> AppResult<EnrollmentRequestDto> {
        let session = self.session().await?;
        Box::pin(self.enrollment_challenge_in(session, share_id, request_id)).await
    }
    async fn enrollment_challenge_in(
        &self,
        session: Arc<Session>,
        share_id: String,
        request_id: String,
    ) -> AppResult<EnrollmentRequestDto> {
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), true).await?;
        let id = parse_share(&share_id)?;
        let request_id = parse_uuid(&request_id)?;
        let snapshot = self.enrollment_current(&env, id).await?;
        let mut record = self
            .enrollment_record_from_snapshot(&env, &snapshot)
            .await?;
        check_owned(&env, &record.value)?;
        let transport = self
            .enrollment_transport(&env, record.value.scope.clone(), true)
            .await?;
        self.enrollment_refresh_grants(&env, &mut record, &transport)
            .await?;
        let mut remote = env.wait(transport.get_request(request_id)).await?;
        self.sharing_check(&env).await?;
        if let Some(cached) = record
            .value
            .requests
            .iter()
            .find(|r| r.state.request.request.request_id == request_id)
            .cloned()
        {
            if cached.pending_acceptance.is_some() || cached.state.acceptance.is_some() {
                return Err(invalid(
                    "a queued or completed acceptance cannot create a replacement challenge",
                ));
            }
            if let Some(challenge) = cached.state.challenge.as_ref() {
                let local_generation = challenge.challenge.generation;
                let remote_generation = remote
                    .challenge
                    .as_ref()
                    .map_or(0, |c| c.challenge.generation);
                if remote_generation < local_generation {
                    if remote_generation.checked_add(1) != Some(local_generation)
                        || remote.request != cached.state.request
                        || remote.endorsement != cached.state.endorsement
                        || remote.grant_id != cached.state.grant_id
                    {
                        return Err(AppError::SharingReconciliationRequired);
                    }
                    grant_active(&record.value, cached.state.grant_id)?;
                    let verified = verify_bundle(&cached.bundle, clock())?;
                    if stored_grant(&record.value, cached.state.grant_id)?
                        .history
                        .last()
                        != Some(verified.grant.signed())
                        || verified.access.hash() != trusted_manifest(&snapshot)?.hash()
                    {
                        return Err(AppError::SharingReconciliationRequired);
                    }
                    verify_device_challenge(
                        challenge,
                        &verified.owner,
                        verified.request.as_ref().unwrap(),
                        verified.endorsement.as_ref().unwrap(),
                        &verified.access,
                        clock(),
                        cached
                            .checkpoint
                            .as_ref()
                            .map(EnrollmentChallengeCheckpoint::from)
                            .as_ref(),
                    )
                    .map_err(enrollment_error)?;
                    // A dispatch that never reached the server retries its
                    // exact signed ciphertext before creating any successor.
                    self.enrollment_save(&env, &mut record).await?;
                    let marks = highwater(&env);
                    let key = record_key(id);
                    let expected = record.serialized.clone();
                    let stored = record.value.clone();
                    let grant_id = cached.state.grant_id;
                    let shutdown = env.unlocked.sharing_shutdown.subscribe();
                    let dispatch_lock = env
                        .wait(env.session.storage.call(move |db| -> AppResult<_> {
                            let guard = marks.lock().map_err(mark_error)?;
                            db.read(|tx| -> AppResult<()> {
                                let actual: Option<String> = tx.setting_get(&key)?;
                                if actual != expected {
                                    return Err(AppError::SharingReconciliationRequired);
                                }
                                check_marks(&marks, &stored)?;
                                grant_active(&stored, grant_id)?;
                                if *shutdown.borrow() {
                                    return Err(AppError::VaultLocked);
                                }
                                Ok(())
                            })?;
                            Ok(guard)
                        }))
                        .await?;
                    self.sharing_check(&env).await?;
                    let observed = env
                        .wait(transport.publish_challenge(
                            request_id,
                            &PublishOwnDeviceChallengeRequest {
                                challenge: challenge.clone(),
                            },
                        ))
                        .await;
                    drop(dispatch_lock);
                    remote = observed?;
                    self.sharing_check(&env).await?;
                }
            }
        }
        let index = self.enrollment_observe_request(&mut record, remote)?;
        grant_active(&record.value, record.value.requests[index].state.grant_id)?;
        let stored = record.value.requests[index].clone();
        if stored.state.status == OwnDeviceRequestStatus::Accepted {
            return Err(invalid("enrollment request was already accepted"));
        }
        let verified = verify_bundle(&stored.bundle, clock())?;
        if stored_grant(&record.value, stored.state.grant_id)?
            .history
            .last()
            != Some(verified.grant.signed())
            || verified.access.hash() != trusted_manifest(&snapshot)?.hash()
        {
            return Err(AppError::SharingReconciliationRequired);
        }
        let previous = stored
            .checkpoint
            .as_ref()
            .map(EnrollmentChallengeCheckpoint::from);
        let (challenge, pending) = env
            .identity
            .sharing_create_enrollment_challenge(
                &verified.owner,
                verified.request.as_ref().unwrap(),
                verified.endorsement.as_ref().unwrap(),
                &verified.access,
                clock(),
                previous.as_ref(),
            )
            .map_err(enrollment_error)?;
        let checked = verify_device_challenge(
            &challenge,
            &verified.owner,
            verified.request.as_ref().unwrap(),
            verified.endorsement.as_ref().unwrap(),
            &verified.access,
            clock(),
            previous.as_ref(),
        )
        .map_err(enrollment_error)?;
        record.value.requests[index].state.challenge = Some(challenge.clone());
        record.value.requests[index].state.response = None;
        record.value.requests[index].state.acceptance = None;
        record.value.requests[index].state.status = OwnDeviceRequestStatus::Challenged;
        record.value.requests[index].checkpoint = Some(checked.checkpoint().into());
        record.value.requests[index].pending_acceptance = None;
        // The next generation is durable before dispatch, invalidating every
        // prior possession. The random plaintext stays in the unlocked runtime.
        self.enrollment_save(&env, &mut record).await?;
        env.unlocked
            .enrollment
            .pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert((id, request_id), pending);
        let observed = env
            .wait(
                transport
                    .publish_challenge(request_id, &PublishOwnDeviceChallengeRequest { challenge }),
            )
            .await?;
        self.sharing_check(&env).await?;
        self.enrollment_observe_request(&mut record, observed)?;
        self.enrollment_save(&env, &mut record).await?;
        request_dto(&record.value.requests[index])
    }
    async fn enrollment_adopt_target(
        &self,
        env: &mut Environment,
        record: &mut RecordState,
        index: usize,
        transport: &OwnDeviceEnrollmentApi,
    ) -> AppResult<()> {
        let stored = record.value.requests[index].clone();
        let source = verify_bundle(&stored.bundle, historical_time(&stored.bundle))?;
        let request = source.request.as_ref().unwrap();
        if request.signed().request.target.device_id != env.pin.device
            || request.signed().request.target.user_id != env.pin.user
        {
            return Err(invalid("accepted target differs from this device"));
        }
        let id = record.value.scope.share_id;
        let current = env.wait(env.transport()?.get_share(id)).await?;
        self.sharing_check(env).await?;
        let mut raw_accesses = stored.bundle.manifests.clone();
        let mut accesses = verified_manifests(&raw_accesses, &source.owner)?;
        let mut revision = source.revision.clone();
        let receipt =
            stored.state.acceptance.as_ref().ok_or_else(|| {
                invalid("accepted enrollment requires the owner's signed receipt")
            })?;
        verify_stored_receipt(&source, &stored, receipt)?;
        let mut accepted_header = None;
        let mut manifest_cursor = source.access.manifest().revision;
        let mut revision_cursor = revision.mutation().context.revision;
        for _ in 0..MAX_HISTORY_PAGES {
            if manifest_cursor >= current.access.manifest.revision
                && revision_cursor >= current.revision.signed.mutation.context.revision
            {
                break;
            }
            let page = env
                .wait(
                    env.transport()?
                        .share_history(id, manifest_cursor, revision_cursor, 100),
                )
                .await?;
            self.sharing_check(env).await?;
            let before = (manifest_cursor, revision_cursor);
            for signed in page.manifests {
                let previous = accesses.last().unwrap();
                let next = verify_shared_manifest(
                    &signed,
                    &context_for(&signed),
                    &source.owner,
                    Some(&previous.checkpoint()),
                )
                .map_err(enrollment_error)?;
                manifest_cursor = next.manifest().revision;
                accesses.push(next);
                raw_accesses.push(signed);
            }
            for signed in page.revisions {
                let access = accesses
                    .iter()
                    .find(|a| a.hash().as_slice() == signed.mutation.manifest_hash.as_slice())
                    .ok_or(AppError::SharingReconciliationRequired)?;
                revision = verify_shared_mutation(access, &signed, Some(&revision.checkpoint()))
                    .map_err(enrollment_error)?;
                if revision.checkpoint().hash.as_slice()
                    == receipt.acceptance.result_revision_hash.as_slice()
                {
                    accepted_header = Some(revision.clone());
                }
                revision_cursor = revision.mutation().context.revision;
            }
            if before == (manifest_cursor, revision_cursor) {
                return Err(AppError::SharingReconciliationRequired);
            }
        }
        let access = accesses.last().unwrap();
        let accepted_header = accepted_header.ok_or(AppError::SharingReconciliationRequired)?;
        let accepted_access = accesses
            .iter()
            .find(|a| {
                a.hash().as_slice() == receipt.acceptance.result_access_manifest_hash.as_slice()
            })
            .ok_or(AppError::SharingReconciliationRequired)?;
        let target = &request.signed().request.target;
        if accepted_header.mutation().operation != SharingOperation::Put
            || accepted_header.mutation().writer_device_id != source.owner.device_id
            || accepted_header.mutation().manifest_hash.as_slice() != accepted_access.hash()
            || accepted_access.manifest().revision != source.access.manifest().revision + 1
            || accepted_access.manifest().access_epoch != source.access.manifest().access_epoch + 1
            || accepted_access.manifest().previous_manifest_hash.as_slice() != source.access.hash()
            || accepted_access.manifest().members.len()
                != source.access.manifest().members.len() + 1
            || source
                .access
                .manifest()
                .members
                .iter()
                .any(|m| accepted_access.member(m.device_id) != Some(m))
            || !accepted_access.member(target.device_id).is_some_and(|m| {
                m.user_id == target.user_id
                    && m.encryption_public_key == target.encryption_public_key
                    && m.signing_public_key == target.signing_public_key
                    && m.role == request.signed().request.requested_role
            })
        {
            return Err(AppError::SharingReconciliationRequired);
        }
        if access.hash()
            != verify_shared_manifest(
                &current.access,
                &current.revision.signed.mutation.context,
                &source.owner,
                Some(&access.checkpoint()),
            )
            .map_err(enrollment_error)?
            .hash()
            || revision.checkpoint()
                != verify_shared_mutation(
                    access,
                    &current.revision.signed,
                    Some(&revision.checkpoint()),
                )
                .map_err(enrollment_error)?
                .checkpoint()
        {
            return Err(AppError::SharingReconciliationRequired);
        }
        let member = access
            .member(env.pin.device)
            .ok_or_else(|| invalid("accepted device was removed from shared access"))?;
        if member.user_id != env.pin.user
            || member.encryption_public_key != env.pin.encryption
            || member.signing_public_key != env.pin.signing
        {
            return Err(invalid("accepted access differs from target keys"));
        }
        let binding = binding_of(&current);
        env.require_kind(binding.kind)?;
        let store = env.store()?;
        let snapshot = match env
            .wait(async { store.load(&binding).await.map_err(integrity) })
            .await?
        {
            Some(snapshot) => {
                if snapshot.owner_anchor() != source.owner {
                    return Err(AppError::SharingReconciliationRequired);
                }
                self.sharing_refresh_state(env, &store, &binding, current.clone())
                    .await?
            }
            None => {
                let accepted = env
                    .wait(async {
                        store
                            .pin_share(
                                binding.clone(),
                                source.owner,
                                current.clone(),
                                env.pin.device,
                                env.identity.as_ref(),
                            )
                            .await
                            .map_err(integrity)
                    })
                    .await?;
                drop(accepted.projection);
                accepted.snapshot
            }
        };
        self.sharing_check(env).await?;
        // Metadata-only transcript advances after complete signed history;
        // the S05 store independently authenticates and validates plaintext.
        if !raw_accesses.starts_with(&record.value.manifests) {
            return Err(AppError::SharingReconciliationRequired);
        }
        record.value.manifests = raw_accesses;
        record.value.header = snapshot.state().revision.signed.clone();
        self.enrollment_refresh_grants(env, record, transport)
            .await?;
        let consumed = stored_grant(&record.value, stored.state.grant_id)?;
        if !consumed.history.iter().any(|g| {
            transcript_hash(enrollment_grant_hash_input(g))
                .ok()
                .as_ref()
                .is_some_and(|hash| {
                    hash.as_slice() == receipt.acceptance.consumed_grant_successor_hash.as_slice()
                })
        }) {
            return Err(AppError::SharingReconciliationRequired);
        }
        self.enrollment_save(env, record).await?;
        if !env.workspace.items.iter().any(|i| i.binding == binding) {
            env.workspace.items.push(IndexedItem {
                binding,
                blocked: false,
            });
            self.sharing_save(env).await?;
        }
        self.sharing_check(env).await
    }
    pub async fn enrollment_respond(
        &self,
        share_id: String,
        request_id: String,
    ) -> AppResult<EnrollmentRequestDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let mut env = self.sharing_environment(session.clone(), true).await?;
        let id = parse_share(&share_id)?;
        let request_id = parse_uuid(&request_id)?;
        let mut record = self
            .enrollment_load(&env, id)
            .await?
            .ok_or_else(|| invalid("target pairing must first be independently confirmed"))?;
        let transport = self
            .enrollment_transport(&env, record.value.scope.clone(), true)
            .await?;
        let remote = env.wait(transport.get_request(request_id)).await?;
        self.sharing_check(&env).await?;
        let index = self.enrollment_observe_request(&mut record, remote)?;
        self.enrollment_save(&env, &mut record).await?;
        if record.value.requests[index].state.status == OwnDeviceRequestStatus::Accepted {
            self.enrollment_adopt_target(&mut env, &mut record, index, &transport)
                .await?;
            return request_dto(&record.value.requests[index]);
        }
        let stored = &record.value.requests[index];
        grant_active(&record.value, stored.state.grant_id)?;
        let verified = verify_bundle(&stored.bundle, clock())?;
        if stored_grant(&record.value, stored.state.grant_id)?
            .history
            .last()
            != Some(verified.grant.signed())
        {
            return Err(AppError::SharingReconciliationRequired);
        }
        let response = env
            .identity
            .sharing_answer_enrollment_challenge(
                &verified.owner,
                verified.request.as_ref().unwrap(),
                verified.endorsement.as_ref().unwrap(),
                &verified.access,
                stored
                    .state
                    .challenge
                    .as_ref()
                    .ok_or_else(|| invalid("owner must first create a challenge"))?,
                clock(),
                stored
                    .checkpoint
                    .as_ref()
                    .map(EnrollmentChallengeCheckpoint::from)
                    .as_ref(),
            )
            .map_err(enrollment_error)?;
        let observed = env
            .wait(transport.submit_response(
                request_id,
                &SubmitOwnDeviceChallengeResponseRequest { response },
            ))
            .await?;
        self.sharing_check(&env).await?;
        self.enrollment_observe_request(&mut record, observed)?;
        self.enrollment_save(&env, &mut record).await?;
        request_dto(&record.value.requests[index])
    }
}

impl AppCore {
    fn enrollment_verify_pending(
        &self,
        record: &EnrollmentRecord,
        stored: &StoredRequest,
        pending: &AcceptOwnDeviceRequest,
    ) -> AppResult<()> {
        let verified = verify_bundle(&stored.bundle, historical_time(&stored.bundle))?;
        let request = verified.request.as_ref().unwrap();
        let endorsement = verified.endorsement.as_ref().unwrap();
        let challenge = stored
            .state
            .challenge
            .as_ref()
            .ok_or_else(|| invalid("missing owner challenge"))?;
        let at = bounded_time(
            historical_time(&stored.bundle),
            challenge.challenge.not_before,
            challenge.challenge.expires_at,
        );
        let checked = verify_device_challenge(
            challenge,
            &verified.owner,
            request,
            endorsement,
            &verified.access,
            at,
            None,
        )
        .map_err(enrollment_error)?;
        let response = verify_device_response(
            stored
                .state
                .response
                .as_ref()
                .ok_or_else(|| invalid("missing target response"))?,
            request,
            &checked,
            &verified.access,
            at,
        )
        .map_err(enrollment_error)?;
        let accesses = verified_manifests(&record.manifests, &verified.owner)?;
        let mut others = Vec::new();
        for successor in &pending.other_grant_successors {
            let stored = stored_grant(record, successor.grant.grant_id)?;
            let index = stored
                .history
                .iter()
                .position(|g| {
                    cc_crypto_core::request_body_sha256(
                        &enrollment_grant_hash_input(g).unwrap_or_default(),
                    )
                    .as_slice()
                        == successor.grant.previous_grant_state_hash.as_slice()
                })
                .ok_or(AppError::SharingReconciliationRequired)?;
            others.push(verify_grant_chain(
                &stored.history[..=index],
                &verified.owner,
                &accesses,
                at,
            )?);
        }
        verify_own_device_acceptance(
            pending,
            &verified.owner,
            &verified.access,
            &verified.revision,
            request,
            endorsement,
            &checked,
            &response,
            &others,
            at,
        )
        .map_err(enrollment_error)?;
        Ok(())
    }
    async fn enrollment_finish_owner_accept(
        &self,
        env: &Environment,
        record: &mut RecordState,
        index: usize,
        pending: &AcceptOwnDeviceRequest,
        result: OwnDeviceAcceptanceResult,
    ) -> AppResult<SharingItemDto> {
        let expected = SharedItemState {
            access: pending.rotation.access.clone(),
            revision: pending.rotation.revision.clone(),
        };
        if result.state != expected
            || result.acceptance != pending.acceptance
            || result.consumed_grant_successor != pending.consumed_grant_successor
            || result.other_grant_successors != pending.other_grant_successors
        {
            return Err(AppError::SharingReconciliationRequired);
        }
        self.enrollment_verify_pending(&record.value, &record.value.requests[index], pending)?;
        let binding = env.binding(record.value.scope.share_id)?;
        let snapshot = self
            .sharing_refresh_state(env, &env.store()?, &binding, result.state.clone())
            .await?;
        self.sharing_check(env).await?;
        if record.value.manifests.last() != Some(&result.state.access) {
            record.value.manifests.push(result.state.access.clone());
        }
        record.value.header = result.state.revision.signed.clone();
        let owner = owner_from(&record.value.owner, env.pin.instance)?;
        let accesses = verified_manifests(&record.value.manifests, &owner)?;
        for successor in
            std::iter::once(&result.consumed_grant_successor).chain(&result.other_grant_successors)
        {
            let grant = record
                .value
                .grants
                .iter_mut()
                .find(|g| g.history.last().unwrap().grant.grant_id == successor.grant.grant_id)
                .ok_or(AppError::SharingReconciliationRequired)?;
            if grant.history.last() != Some(successor) {
                grant.history.push(successor.clone());
            }
            let verified = verify_grant_chain(&grant.history, &owner, &accesses, clock())?;
            let frozen = grant.mark.frozen;
            grant.mark = GrantMark::authenticated(&verified).map_err(mark_error)?;
            grant.mark.frozen = frozen;
        }
        record.value.requests[index].state.acceptance = Some(result.acceptance);
        record.value.requests[index].state.status = OwnDeviceRequestStatus::Accepted;
        record.value.requests[index].pending_acceptance = None;
        self.enrollment_save(env, record).await?;
        env.unlocked
            .enrollment
            .pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&(
                binding.share_id,
                record.value.requests[index]
                    .state
                    .request
                    .request
                    .request_id,
            ));
        item_dto(env, snapshot.state(), SharingTrustDto::Verified, None)
    }
    pub async fn enrollment_accept(
        &self,
        share_id: String,
        request_id: String,
        confirmed_manual: bool,
    ) -> AppResult<SharingItemDto> {
        let session = self.session().await?;
        Box::pin(self.enrollment_accept_in(session, share_id, request_id, confirmed_manual)).await
    }
    async fn enrollment_accept_in(
        &self,
        session: Arc<Session>,
        share_id: String,
        request_id: String,
        confirmed_manual: bool,
    ) -> AppResult<SharingItemDto> {
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), true).await?;
        let id = parse_share(&share_id)?;
        let request_id = parse_uuid(&request_id)?;
        let mut record = self
            .enrollment_load(&env, id)
            .await?
            .ok_or_else(|| invalid("enrollment must first be independently paired"))?;
        check_owned(&env, &record.value)?;
        let transport = self
            .enrollment_transport(&env, record.value.scope.clone(), true)
            .await?;
        let remote = env.wait(transport.get_request(request_id)).await?;
        self.sharing_check(&env).await?;
        let already_completed = record.value.requests.iter().any(|request| {
            request.state.request.request.request_id == request_id
                && request.state.status == OwnDeviceRequestStatus::Accepted
                && request.state.acceptance.is_some()
                && request.pending_acceptance.is_none()
        });
        let index = self.enrollment_observe_request(&mut record, remote.clone())?;
        if already_completed && remote.status == OwnDeviceRequestStatus::Accepted {
            let snapshot = self.enrollment_current(&env, id).await?;
            return item_dto(&env, snapshot.state(), SharingTrustDto::Verified, None);
        }
        if let Some(pending) = record.value.requests[index].pending_acceptance.clone() {
            self.enrollment_verify_pending(&record.value, &record.value.requests[index], &pending)?;
            if remote.status == OwnDeviceRequestStatus::Accepted {
                if remote.acceptance.as_ref() != Some(&pending.acceptance) {
                    return Err(AppError::SharingReconciliationRequired);
                }
                let current = env.wait(env.transport()?.get_share(id)).await?;
                self.sharing_check(&env).await?;
                let result = OwnDeviceAcceptanceResult {
                    state: current,
                    acceptance: pending.acceptance.clone(),
                    consumed_grant_successor: pending.consumed_grant_successor.clone(),
                    other_grant_successors: pending.other_grant_successors.clone(),
                };
                return self
                    .enrollment_finish_owner_accept(&env, &mut record, index, &pending, result)
                    .await;
            }
        }
        let snapshot = self.enrollment_current(&env, id).await?;
        let previous_serialized = record.serialized.clone();
        let refreshed = self
            .enrollment_record_from_snapshot(&env, &snapshot)
            .await?;
        record.value.manifests = refreshed.value.manifests;
        record.value.header = refreshed.value.header;
        record.serialized = previous_serialized;
        self.enrollment_refresh_grants(&env, &mut record, &transport)
            .await?;
        let grant_id = record.value.requests[index].state.grant_id;
        grant_active(&record.value, grant_id)?;
        let grant = stored_grant(&record.value, grant_id)?;
        if grant.history.last().unwrap().grant.mode == EnrollmentMode::Manual && !confirmed_manual {
            return Err(invalid(
                "manual owner confirmation is required for this enrollment",
            ));
        }
        let mut bundle = source_bundle(&record.value, grant_id)?;
        bundle.request = Some(remote.request.clone());
        bundle.endorsement = Some(remote.endorsement.clone());
        let verified = verify_bundle(&bundle, clock())?;
        record.value.requests[index].bundle = bundle;
        self.enrollment_save(&env, &mut record).await?;
        let pending = match record.value.requests[index].pending_acceptance.clone() {
            Some(pending) => pending,
            None => {
                let request = verified.request.as_ref().unwrap();
                let endorsement = verified.endorsement.as_ref().unwrap();
                let response = remote
                    .response
                    .as_ref()
                    .ok_or_else(|| invalid("target must first prove the challenge"))?;
                let checkpoint = record.value.requests[index]
                    .checkpoint
                    .as_ref()
                    .map(EnrollmentChallengeCheckpoint::from)
                    .ok_or_else(|| invalid("missing durable challenge checkpoint"))?;
                let secret = {
                    let mut secrets = env
                        .unlocked
                        .enrollment
                        .pending
                        .lock()
                        .unwrap_or_else(|p| p.into_inner());
                    secrets.remove(&(id, request_id)).ok_or_else(|| {
                        invalid("owner challenge secret was discarded; create a fresh challenge")
                    })?
                };
                let proof = verify_device_possession(
                    response,
                    &secret,
                    request,
                    &verified.access,
                    clock(),
                    &checkpoint,
                )
                .map_err(enrollment_error)?;
                drop(secret);
                let accesses = verified_manifests(&record.value.manifests, &verified.owner)?;
                let others = record
                    .value
                    .grants
                    .iter()
                    .filter(|g| {
                        g.history.last().unwrap().grant.grant_id != grant_id
                            && !g.mark.frozen
                            && g.mark.status == EnrollmentGrantStatus::Active
                    })
                    .map(|g| verify_grant_chain(&g.history, &verified.owner, &accesses, clock()))
                    .collect::<AppResult<Vec<_>>>()?
                    .into_iter()
                    .filter(|g| {
                        g.signed().grant.expires_at > clock()
                            && g.signed().grant.access_manifest_hash.as_slice()
                                == verified.access.hash()
                    })
                    .collect::<Vec<_>>();
                // Final signing happens under the same independent profile
                // lock as the exact SQLCipher/grant/challenge check. A cached
                // possession cannot override a concurrently frozen/revoked
                // grant or a newer replacement challenge from another process.
                let marks = highwater(&env);
                let key = record_key(id);
                let expected = record.serialized.clone();
                let stored = record.value.clone();
                let identity = env.identity.clone();
                let before = verified.access.clone();
                let owner = verified.owner;
                let latest_grant = verified.grant.clone();
                let previous = snapshot.state().revision.signed.clone();
                let revision =
                    verify_shared_mutation(&before, &previous, None).map_err(enrollment_error)?;
                let body = snapshot
                    .state()
                    .revision
                    .body
                    .clone()
                    .ok_or_else(|| invalid("deleted shared items cannot enroll devices"))?;
                let request = request.clone();
                let endorsement = endorsement.clone();
                let shutdown = env.unlocked.sharing_shutdown.subscribe();
                env.wait(env.session.storage.call(move |db| -> AppResult<_> {
                    let _lock = marks.lock().map_err(mark_error)?;
                    db.read(|tx| -> AppResult<_> {
                        let actual: Option<String> = tx.setting_get(&key)?;
                        if actual != expected {
                            return Err(AppError::SharingReconciliationRequired);
                        }
                        check_marks(&marks, &stored)?;
                        grant_active(&stored, grant_id)?;
                        if *shutdown.borrow() {
                            return Err(AppError::VaultLocked);
                        }
                        let pending = identity
                            .sharing_accept_own_device(
                                &owner,
                                &before,
                                &revision,
                                &body,
                                &request,
                                &endorsement,
                                &proof,
                                &checkpoint,
                                &latest_grant,
                                &others,
                                clock(),
                            )
                            .map_err(enrollment_error)?;
                        if *shutdown.borrow() {
                            return Err(AppError::VaultLocked);
                        }
                        Ok(pending)
                    })
                }))
                .await?
            }
        };
        self.sharing_check(&env).await?;
        self.enrollment_verify_pending(&record.value, &record.value.requests[index], &pending)?;
        record.value.requests[index].pending_acceptance = Some(pending.clone());
        self.enrollment_save(&env, &mut record).await?;
        env.wait(transport.recheck_capabilities()).await?;
        self.sharing_check(&env).await?;
        // Dispatch is ciphertext-only and retries this exact request. Any
        // changed role/access/grant/challenge leaves it blocked for review.
        let current_grant = env.wait(transport.get_grant(grant_id)).await?;
        let current_request = env.wait(transport.get_request(request_id)).await?;
        self.sharing_check(&env).await?;
        if current_grant != *verified.grant.signed()
            || current_request.challenge != record.value.requests[index].state.challenge
            || current_request.response != record.value.requests[index].state.response
        {
            return Err(AppError::SharingReconciliationRequired);
        }
        let marks = highwater(&env);
        let expected = record.serialized.clone();
        let stored = record.value.clone();
        let key = record_key(id);
        let shutdown = env.unlocked.sharing_shutdown.subscribe();
        let dispatch_lock = env
            .wait(env.session.storage.call(move |db| -> AppResult<_> {
                let guard = marks.lock().map_err(mark_error)?;
                db.read(|tx| -> AppResult<()> {
                    let actual: Option<String> = tx.setting_get(&key)?;
                    if actual != expected {
                        return Err(AppError::SharingReconciliationRequired);
                    }
                    check_marks(&marks, &stored)?;
                    grant_active(&stored, grant_id)?;
                    if *shutdown.borrow() {
                        return Err(AppError::VaultLocked);
                    }
                    Ok(())
                })?;
                Ok(guard)
            }))
            .await?;
        self.sharing_check(&env).await?;
        // Only the already-checked ciphertext POST spans this lock. Local
        // freeze on another process waits until this in-flight request ends.
        let result = env
            .wait(transport.accept_request(request_id, &pending))
            .await;
        drop(dispatch_lock);
        let result = result?;
        self.sharing_check(&env).await?;
        self.enrollment_finish_owner_accept(&env, &mut record, index, &pending, result)
            .await
    }
    pub async fn enrollment_pending_requests(&self) -> AppResult<Vec<EnrollmentRequestDto>> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), true).await?;
        let keys = env
            .wait(env.session.storage.read(|tx| tx.setting_keys()))
            .await?;
        self.sharing_check(&env).await?;
        let mut result = Vec::new();
        for key in keys
            .iter()
            .filter(|k| k.starts_with(RECORD_PREFIX))
            .take(MAX_ITEMS)
        {
            let id = parse_share(key.strip_prefix(RECORD_PREFIX).unwrap())?;
            let mut record = self
                .enrollment_load(&env, id)
                .await?
                .ok_or(AppError::SharingReconciliationRequired)?;
            let transport = self
                .enrollment_transport(&env, record.value.scope.clone(), true)
                .await?;
            let known = record
                .value
                .requests
                .iter()
                .filter(|r| {
                    r.state.request.request.target.device_id == env.pin.device
                        && r.state.request.request.target.user_id == env.pin.user
                })
                .map(|r| r.state.request.request.request_id)
                .collect::<Vec<_>>();
            for id in known {
                let state = env.wait(transport.get_request(id)).await?;
                self.sharing_check(&env).await?;
                let index = self.enrollment_observe_request(&mut record, state)?;
                result.push(request_dto(&record.value.requests[index])?);
            }
            self.enrollment_save(&env, &mut record).await?;
        }
        Ok(result)
    }
}

impl AppCore {
    /// One explicit owner-online pass. Only already owner-signed Automatic
    /// grants in the captured unlocked profile participate. This method never
    /// creates grants, pairs targets, imports data or executes shared commands.
    pub async fn enrollment_process_automatic(&self) -> AppResult<Vec<EnrollmentRequestDto>> {
        let session = self.session().await?;
        let candidates = {
            let _gate = self.sharing_gate(&session).await?;
            let env = self.sharing_environment(session.clone(), true).await?;
            if !env.pin.supports_owner_online_enrollment_v1 {
                return Ok(vec![]);
            }
            let keys = env
                .wait(env.session.storage.read(|tx| tx.setting_keys()))
                .await?;
            self.sharing_check(&env).await?;
            let mut shares = Vec::new();
            for key in keys
                .iter()
                .filter(|k| k.starts_with(RECORD_PREFIX))
                .take(MAX_ITEMS)
            {
                let id = parse_share(key.strip_prefix(RECORD_PREFIX).unwrap())?;
                match self.enrollment_load(&env, id).await {
                    Ok(Some(record))
                        if owner_from(&record.value.owner, env.pin.instance)?
                            == env.own_anchor()
                            && env.supports(record.value.scope.kind)
                            && record.value.grants.iter().any(|g| {
                                !g.mark.frozen
                                    && g.mark.status == EnrollmentGrantStatus::Active
                                    && g.history.last().unwrap().grant.mode
                                        == EnrollmentMode::Automatic
                                    && g.history.last().unwrap().grant.expires_at > clock()
                            }) =>
                    {
                        shares.push(id)
                    }
                    Err(AppError::VaultLocked) => return Err(AppError::VaultLocked),
                    _ => {}
                }
            }
            shares
        };
        let unlocked = session.unlocked().await?;
        let ensure_same = || async {
            let active = self.session().await?;
            if !Arc::ptr_eq(&session, &active)
                || !Arc::ptr_eq(&unlocked, &session.unlocked().await?)
                || *unlocked.sharing_shutdown.borrow()
            {
                Err(AppError::VaultLocked)
            } else {
                Ok(())
            }
        };
        let mut result = Vec::new();
        for share in candidates {
            ensure_same().await?;
            let grants =
                match Box::pin(self.enrollment_list_grants_in(session.clone(), share.to_string()))
                    .await
                {
                    Ok(grants) => grants,
                    Err(AppError::VaultLocked) => return Err(AppError::VaultLocked),
                    Err(_) => continue,
                };
            ensure_same().await?;
            let requests = match Box::pin(
                self.enrollment_list_requests_in(session.clone(), share.to_string()),
            )
            .await
            {
                Ok(requests) => requests,
                Err(AppError::VaultLocked) => return Err(AppError::VaultLocked),
                Err(_) => continue,
            };
            for mut request in requests {
                let automatic = grants.iter().any(|g| {
                    g.grant_id == request.grant_id
                        && g.mode == EnrollmentModeDto::Automatic
                        && !g.frozen
                        && g.status == EnrollmentGrantStatusDto::Active
                        && g.expires_at > clock()
                });
                if !automatic || request.expires_at <= clock() {
                    continue;
                }
                ensure_same().await?;
                let action = match request.status {
                    EnrollmentRequestStatusDto::Pending => Box::pin(self.enrollment_challenge_in(
                        session.clone(),
                        share.to_string(),
                        request.request_id.clone(),
                    ))
                    .await
                    .map(Some),
                    EnrollmentRequestStatusDto::Responded => Box::pin(self.enrollment_accept_in(
                        session.clone(),
                        share.to_string(),
                        request.request_id.clone(),
                        false,
                    ))
                    .await
                    .map(|_| {
                        request.status = EnrollmentRequestStatusDto::Accepted;
                        Some(request.clone())
                    }),
                    _ => Ok(None),
                };
                ensure_same().await?;
                match action {
                    Ok(Some(updated)) => result.push(updated),
                    Err(AppError::VaultLocked) => return Err(AppError::VaultLocked),
                    Err(_) => {
                        request.status = EnrollmentRequestStatusDto::Blocked;
                        request.blocked_reason = Some("enrollment_review_required".into());
                        result.push(request);
                    }
                    Ok(None) => {}
                }
            }
        }
        ensure_same().await?;
        Ok(result)
    }
}

/// Independent public OS marks captured before requesting recovery data. This
/// snapshot is compared again under the profile lock at the eventual commit.
#[derive(Clone, PartialEq, Eq)]
struct RecoveryRequestMark {
    hash: [u8; 32],
    challenge: Option<EnrollmentChallengeCheckpoint>,
    pending_receipt: Option<[u8; 32]>,
}
#[derive(Clone, PartialEq, Eq)]
struct RecoveryMarks {
    scope: EnrollmentScope,
    owner: SharingOwnerAnchor,
    grants: std::collections::BTreeMap<Uuid, GrantMark>,
    requests: std::collections::BTreeMap<Uuid, RecoveryRequestMark>,
}
fn recovery_marks(marks: &EnrollmentHighwater, id: ShareId) -> AppResult<RecoveryMarks> {
    let scope = marks
        .scope_for_share(id)
        .map_err(mark_error)?
        .ok_or(AppError::SharingReconciliationRequired)?;
    let owner = marks
        .owner(&scope)
        .map_err(mark_error)?
        .ok_or(AppError::SharingReconciliationRequired)?;
    let mut grants = std::collections::BTreeMap::new();
    for id in marks.known_grants(&scope).map_err(mark_error)? {
        let mark = marks
            .load_grant(&scope, id)
            .map_err(mark_error)?
            .ok_or(AppError::SharingReconciliationRequired)?;
        marks
            .check_grant(&scope, id, Some(&mark))
            .map_err(mark_error)?;
        grants.insert(id, mark);
    }
    let mut requests = std::collections::BTreeMap::new();
    for id in marks.known_requests(&scope).map_err(mark_error)? {
        let hash = marks
            .load_request_hash(&scope, id)
            .map_err(mark_error)?
            .ok_or(AppError::SharingReconciliationRequired)?;
        let challenge = marks.load_challenge(&scope, id).map_err(mark_error)?;
        marks
            .check_challenge(&scope, id, challenge.as_ref())
            .map_err(mark_error)?;
        if challenge.as_ref().is_some_and(|c| c.request_hash != hash) {
            return Err(AppError::SharingReconciliationRequired);
        }
        let pending_receipt = marks
            .load_pending_acceptance_hash(&scope, id)
            .map_err(mark_error)?;
        requests.insert(
            id,
            RecoveryRequestMark {
                hash,
                challenge,
                pending_receipt,
            },
        );
    }
    Ok(RecoveryMarks {
        scope,
        owner,
        grants,
        requests,
    })
}
fn identity_for_anchor(
    instance: Uuid,
    owner: &SharingOwnerAnchor,
) -> AppResult<SharingIdentityDto> {
    identity_dto(
        instance,
        &SharingMember {
            user_id: owner.user_id,
            device_id: owner.device_id,
            encryption_public_key: owner.public_keys.encryption_bytes(),
            signing_public_key: owner.public_keys.signing_bytes(),
            role: SharingRole::Editor,
        },
    )
}
/// Authenticate a retained receipt against the complete owner-signed access
/// and editor-signed revision histories, without decrypting a historical body.
fn recovery_receipt(
    stored: &StoredRequest,
    record: &EnrollmentRecord,
    accesses: &[VerifiedSharingManifest],
    headers: &[VerifiedSharingMutation],
) -> AppResult<()> {
    let Some(receipt) = &stored.state.acceptance else {
        return Ok(());
    };
    let source = verify_bundle(&stored.bundle, historical_time(&stored.bundle))?;
    verify_stored_receipt(&source, stored, receipt)?;
    let result_access = accesses
        .iter()
        .find(|a| a.hash().as_slice() == receipt.acceptance.result_access_manifest_hash.as_slice())
        .ok_or(AppError::SharingReconciliationRequired)?;
    let result_header = headers
        .iter()
        .find(|h| {
            h.checkpoint().hash.as_slice() == receipt.acceptance.result_revision_hash.as_slice()
        })
        .ok_or(AppError::SharingReconciliationRequired)?;
    let request = source.request.as_ref().unwrap();
    let target = &request.signed().request.target;
    if result_header.mutation().operation != SharingOperation::Put
        || result_header.mutation().writer_device_id != source.owner.device_id
        || result_header.mutation().manifest_hash.as_slice() != result_access.hash()
        || result_access.manifest().revision != source.access.manifest().revision + 1
        || result_access.manifest().access_epoch != source.access.manifest().access_epoch + 1
        || result_access.manifest().previous_manifest_hash.as_slice() != source.access.hash()
        || result_access.manifest().members.len() != source.access.manifest().members.len() + 1
        || source
            .access
            .manifest()
            .members
            .iter()
            .any(|m| result_access.member(m.device_id) != Some(m))
        || !result_access.member(target.device_id).is_some_and(|m| {
            m.user_id == target.user_id
                && m.encryption_public_key == target.encryption_public_key
                && m.signing_public_key == target.signing_public_key
                && m.role == request.signed().request.requested_role
        })
    {
        return Err(AppError::SharingReconciliationRequired);
    }
    let consumed = stored_grant(record, stored.state.grant_id)?;
    let successor = consumed
        .history
        .iter()
        .find(|g| {
            transcript_hash(enrollment_grant_hash_input(g))
                .ok()
                .as_ref()
                .is_some_and(|hash| {
                    hash.as_slice() == receipt.acceptance.consumed_grant_successor_hash.as_slice()
                })
        })
        .ok_or(AppError::SharingReconciliationRequired)?;
    if successor.grant.previous_grant_state_hash.as_slice() != source.grant.hash()
        || successor.grant.access_manifest_hash.as_slice() != result_access.hash()
        || successor.grant.admitted_count != source.grant.signed().grant.admitted_count + 1
    {
        return Err(AppError::SharingReconciliationRequired);
    }
    for other in &receipt.acceptance.other_grant_successor_hashes {
        // A non-owner may only read its own grant. The owner-signed receipt
        // still binds inaccessible successors; every locally known grant must
        // have its exact countersigned successor independently witnessed.
        if let Ok(grant) = stored_grant(record, other.grant_id) {
            if !grant.history.iter().any(|g| {
                transcript_hash(enrollment_grant_hash_input(g))
                    .ok()
                    .as_ref()
                    .is_some_and(|hash| hash.as_slice() == other.state_hash.as_slice())
                    && g.grant.access_manifest_hash.as_slice() == result_access.hash()
            }) {
                return Err(AppError::SharingReconciliationRequired);
            }
        }
    }
    Ok(())
}
impl AppCore {
    async fn enrollment_commit_recovery(
        &self,
        env: &Environment,
        retained: &RecoveryMarks,
        old_raw: Option<String>,
        rebuilt: &EnrollmentRecord,
    ) -> AppResult<()> {
        validate_record(rebuilt)?;
        let id = retained.scope.share_id;
        let expected_marks = retained.clone();
        let replacement = serde_json::to_string(&rebuilt)
            .map_err(|_| invalid("cannot encode enrollment transcript"))?;
        if replacement.len() > MAX_ENROLLMENT_RECORD_BYTES {
            return Err(invalid("enrollment transcript exceeds the limit"));
        }
        let marks = highwater(env);
        let key = record_key(id);
        let value = rebuilt.clone();
        let shutdown = env.unlocked.sharing_shutdown.subscribe();
        env.wait(env.session.storage.call(move |db| -> AppResult<()> {
            let _lock = marks.lock().map_err(mark_error)?;
            db.write(|tx| -> AppResult<()> {
                if *shutdown.borrow() {
                    return Err(AppError::VaultLocked);
                }
                let actual: Option<String> = tx.setting_get(&key)?;
                if actual != old_raw || recovery_marks(&marks, id)? != expected_marks {
                    return Err(AppError::SharingReconciliationRequired);
                }
                let accesses = verified_manifests(&value.manifests, &expected_marks.owner)?;
                for grant in &value.grants {
                    let id = grant.history.last().unwrap().grant.grant_id;
                    let original = expected_marks
                        .grants
                        .get(&id)
                        .ok_or(AppError::SharingReconciliationRequired)?;
                    let mut checkpoint = original.clone();
                    for len in (original.revision as usize + 1)..=grant.history.len() {
                        let verified = verify_grant_chain(
                            &grant.history[..len],
                            &expected_marks.owner,
                            &accesses,
                            clock(),
                        )?;
                        let mut next = GrantMark::authenticated(&verified).map_err(mark_error)?;
                        next.frozen = original.frozen;
                        checkpoint = if original.frozen {
                            marks.cas_reconciled_grant(&value.scope, id, &checkpoint, next)
                        } else {
                            marks.cas_grant(&value.scope, id, Some(&checkpoint), next)
                        }
                        .map_err(mark_error)?;
                    }
                    if checkpoint != grant.mark {
                        return Err(AppError::SharingReconciliationRequired);
                    }
                }
                for request in &value.requests {
                    let id = request.state.request.request.request_id;
                    let original = expected_marks
                        .requests
                        .get(&id)
                        .ok_or(AppError::SharingReconciliationRequired)?;
                    if let Some(next) = &request.checkpoint {
                        marks
                            .cas_challenge(
                                &value.scope,
                                id,
                                original.challenge.as_ref(),
                                next.into(),
                            )
                            .map_err(mark_error)?;
                    }
                }
                check_marks(&marks, &value)?;
                tx.setting_set(&key, &replacement)?;
                if *shutdown.borrow() {
                    return Err(AppError::VaultLocked);
                }
                Ok(())
            })
        }))
        .await?;
        self.sharing_check(env).await?;
        env.unlocked
            .enrollment
            .pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .retain(|(share, _), _| *share != id);
        Ok(())
    }
    /// Explicit forward-only reconstruction. The complete independent owner
    /// code is required again; server directory keys cannot create or replace
    /// a pin. Missing OS marks and unresolved lost ciphertext remain blocked.
    pub async fn enrollment_reconcile(
        &self,
        share_id: String,
        confirmed_owner_code: String,
    ) -> AppResult<Vec<EnrollmentGrantDto>> {
        let session = self.session().await?;
        Box::pin(self.enrollment_reconcile_in(session, share_id, confirmed_owner_code)).await
    }
    async fn enrollment_reconcile_in(
        &self,
        session: Arc<Session>,
        share_id: String,
        confirmed_owner_code: String,
    ) -> AppResult<Vec<EnrollmentGrantDto>> {
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), true).await?;
        let id = parse_share(&share_id)?;
        let marks = highwater(&env);
        let key = record_key(id);
        let (retained, old_raw, old) = env
            .wait(env.session.storage.call(move |db| -> AppResult<_> {
                let _lock = marks.lock().map_err(mark_error)?;
                db.read(|tx| -> AppResult<_> {
                    let retained = recovery_marks(&marks, id)?;
                    let raw: Option<String> = tx.setting_get(&key)?;
                    let old = raw
                        .as_ref()
                        .map(|raw| -> AppResult<EnrollmentRecord> {
                            if raw.len() > MAX_ENROLLMENT_RECORD_BYTES {
                                return Err(invalid("enrollment transcript exceeds the limit"));
                            }
                            let old: EnrollmentRecord = serde_json::from_str(raw)
                                .map_err(|_| invalid("invalid encrypted enrollment transcript"))?;
                            validate_record(&old)?;
                            if old.scope != retained.scope
                                || owner_from(&old.owner, retained.scope.server_instance_id)?
                                    != retained.owner
                                || old.grants.iter().any(|g| {
                                    !retained
                                        .grants
                                        .contains_key(&g.history.last().unwrap().grant.grant_id)
                                })
                                || old.requests.iter().any(|r| {
                                    !retained
                                        .requests
                                        .get(&r.state.request.request.request_id)
                                        .is_some_and(|mark| {
                                            transcript_hash(enrollment_request_hash_input(
                                                &r.state.request,
                                            ))
                                            .ok()
                                                == Some(mark.hash)
                                        })
                                })
                            {
                                return Err(AppError::SharingReconciliationRequired);
                            }
                            Ok(old)
                        })
                        .transpose()?;
                    Ok((retained, raw, old))
                })
            }))
            .await?;
        self.sharing_check(&env).await?;
        let code = sharing_identity_code(
            env.pin.instance,
            retained.owner.user_id,
            retained.owner.device_id,
            &retained.owner.public_keys,
        )
        .map_err(enrollment_error)?;
        if !code_matches(&code, &confirmed_owner_code) {
            return Err(invalid(
                "independently compared owner verification code is required",
            ));
        }
        env.require_kind(retained.scope.kind)?;
        let transport = self
            .enrollment_transport(&env, retained.scope.clone(), true)
            .await?;
        let current = env.wait(env.transport()?.get_share(id)).await?;
        self.sharing_check(&env).await?;
        if scope_from_state(&current) != retained.scope {
            return Err(AppError::SharingReconciliationRequired);
        }
        let mut raw_accesses = Vec::new();
        let mut raw_headers = Vec::new();
        let mut mc = 0;
        let mut rc = 0;
        for _ in 0..MAX_HISTORY_PAGES {
            if mc >= current.access.manifest.revision
                && rc >= current.revision.signed.mutation.context.revision
            {
                break;
            }
            let page = env
                .wait(env.transport()?.share_history(id, mc, rc, 100))
                .await?;
            self.sharing_check(&env).await?;
            let before = (mc, rc);
            for signed in page.manifests {
                if signed.manifest.revision <= current.access.manifest.revision {
                    mc = signed.manifest.revision;
                    raw_accesses.push(signed)
                }
            }
            for signed in page.revisions {
                if signed.mutation.context.revision
                    <= current.revision.signed.mutation.context.revision
                {
                    rc = signed.mutation.context.revision;
                    raw_headers.push(signed)
                }
            }
            if before == (mc, rc) {
                return Err(AppError::SharingReconciliationRequired);
            }
        }
        if raw_accesses
            .first()
            .is_none_or(|a| a.manifest.revision != 1)
            || raw_accesses.last() != Some(&current.access)
            || raw_headers.first().is_none_or(|h| {
                h.mutation.context.revision != 1
                    || h.mutation.base_revision != 0
                    || h.mutation.previous_revision_hash.as_slice() != [0; 32]
            })
            || raw_headers.last() != Some(&current.revision.signed)
        {
            return Err(AppError::SharingReconciliationRequired);
        }
        let accesses = verified_manifests(&raw_accesses, &retained.owner)?;
        let mut headers: Vec<VerifiedSharingMutation> = Vec::new();
        for signed in &raw_headers {
            let access = accesses
                .iter()
                .find(|a| a.hash().as_slice() == signed.mutation.manifest_hash.as_slice())
                .ok_or(AppError::SharingReconciliationRequired)?;
            headers.push(
                verify_shared_mutation(
                    access,
                    signed,
                    headers
                        .last()
                        .map(VerifiedSharingMutation::checkpoint)
                        .as_ref(),
                )
                .map_err(enrollment_error)?,
            );
        }
        if let Some(old) = &old {
            if !raw_accesses.starts_with(&old.manifests) || !raw_headers.contains(&old.header) {
                return Err(AppError::SharingReconciliationRequired);
            }
        }
        let owner_dto = identity_for_anchor(env.pin.instance, &retained.owner)?;
        let mut rebuilt = EnrollmentRecord {
            format: 1,
            scope: retained.scope.clone(),
            owner: owner_dto,
            manifests: raw_accesses,
            header: current.revision.signed.clone(),
            grants: vec![],
            requests: vec![],
        };
        for (id, mark) in &retained.grants {
            let latest = env.wait(transport.get_grant(*id)).await?;
            self.sharing_check(&env).await?;
            let mut history = Vec::new();
            let mut cursor = 0;
            for _ in 0..MAX_HISTORY_PAGES {
                if cursor >= latest.grant.grant_revision {
                    break;
                }
                let page = env.wait(transport.grant_history(*id, cursor, 100)).await?;
                self.sharing_check(&env).await?;
                let before = cursor;
                for state in page.states {
                    if state.grant.grant_revision <= latest.grant.grant_revision {
                        cursor = state.grant.grant_revision;
                        history.push(state)
                    }
                }
                if before == cursor {
                    return Err(AppError::SharingReconciliationRequired);
                }
            }
            if history.last() != Some(&latest) || history.len() < mark.revision as usize {
                return Err(AppError::SharingReconciliationRequired);
            }
            let witnessed = verify_grant_chain(
                &history[..mark.revision as usize],
                &retained.owner,
                &accesses,
                clock(),
            )?;
            let mut witness_mark = GrantMark::authenticated(&witnessed).map_err(mark_error)?;
            witness_mark.frozen = mark.frozen;
            if &witness_mark != mark {
                return Err(AppError::SharingReconciliationRequired);
            }
            let verified = verify_grant_chain(&history, &retained.owner, &accesses, clock())?;
            let mut next = GrantMark::authenticated(&verified).map_err(mark_error)?;
            next.frozen = mark.frozen;
            rebuilt.grants.push(StoredGrant {
                history,
                mark: next,
            });
        }
        for (id, mark) in &retained.requests {
            let remote = env.wait(transport.get_request(*id)).await?;
            self.sharing_check(&env).await?;
            if transcript_hash(enrollment_request_hash_input(&remote.request))? != mark.hash {
                return Err(AppError::SharingReconciliationRequired);
            }
            let grant = stored_grant(&rebuilt, remote.grant_id)?;
            let gi = grant
                .history
                .iter()
                .position(|g| {
                    transcript_hash(enrollment_grant_hash_input(g))
                        .ok()
                        .as_ref()
                        .is_some_and(|hash| {
                            hash.as_slice() == remote.request.request.grant_state_hash.as_slice()
                        })
                })
                .ok_or(AppError::SharingReconciliationRequired)?;
            let ai = accesses
                .iter()
                .position(|a| {
                    a.hash().as_slice() == remote.request.request.access_manifest_hash.as_slice()
                })
                .ok_or(AppError::SharingReconciliationRequired)?;
            let header = raw_headers
                .iter()
                .rev()
                .find(|h| h.mutation.manifest_hash.as_slice() == accesses[ai].hash())
                .ok_or(AppError::SharingReconciliationRequired)?
                .clone();
            let bundle = PublicBundle {
                format: 1,
                owner: rebuilt.owner.clone(),
                manifests: rebuilt.manifests[..=ai].to_vec(),
                header,
                grant_history: grant.history[..=gi].to_vec(),
                request: Some(remote.request.clone()),
                endorsement: Some(remote.endorsement.clone()),
            };
            let verified = verify_bundle(&bundle, historical_time(&bundle))?;
            let challenge = remote
                .challenge
                .as_ref()
                .map(|signed| {
                    verify_device_challenge(
                        signed,
                        &retained.owner,
                        verified.request.as_ref().unwrap(),
                        verified.endorsement.as_ref().unwrap(),
                        &verified.access,
                        bounded_time(
                            historical_time(&bundle),
                            signed.challenge.not_before,
                            signed.challenge.expires_at,
                        ),
                        mark.challenge.as_ref(),
                    )
                    .map_err(enrollment_error)
                })
                .transpose()?;
            if mark.challenge.is_some() && challenge.is_none() {
                return Err(AppError::SharingReconciliationRequired);
            }
            let old_request = old.as_ref().and_then(|r| {
                r.requests
                    .iter()
                    .find(|r| r.state.request.request.request_id == *id)
            });
            if old_request.is_some_and(|r| {
                r.state.acceptance.is_some() && r.state.acceptance != remote.acceptance
            }) {
                return Err(AppError::SharingReconciliationRequired);
            }
            let pending = old_request.and_then(|r| r.pending_acceptance.clone());
            if let Some(expected) = mark.pending_receipt {
                let accepted = remote
                    .acceptance
                    .as_ref()
                    .map(|a| transcript_hash(enrollment_acceptance_hash_input(a)))
                    .transpose()?;
                let pending_hash = pending
                    .as_ref()
                    .map(|p| transcript_hash(enrollment_acceptance_hash_input(&p.acceptance)))
                    .transpose()?;
                if accepted != Some(expected) && pending_hash != Some(expected) {
                    return Err(AppError::SharingReconciliationRequired);
                }
            }
            let pending = if remote.acceptance.is_some() {
                None
            } else {
                pending
            };
            let stored = StoredRequest {
                bundle,
                state: remote,
                checkpoint: challenge.map(|c| c.checkpoint().into()),
                pending_acceptance: pending,
            };
            // Accepted data is checked below after every retained grant has
            // been reconstructed. No key possession is inferred from status.
            rebuilt.requests.push(stored);
        }
        validate_record(&rebuilt)?;
        for request in &rebuilt.requests {
            recovery_receipt(request, &rebuilt, &accesses, &headers)?;
            if let Some(pending) = &request.pending_acceptance {
                self.enrollment_verify_pending(&rebuilt, request, pending)?;
            }
        }
        self.enrollment_commit_recovery(&env, &retained, old_raw, &rebuilt)
            .await?;
        rebuilt
            .grants
            .iter()
            .map(|g| grant_dto(g.history.last().unwrap(), &g.mark))
            .collect()
    }
}
fn scope_from_state(state: &SharedItemState) -> EnrollmentScope {
    EnrollmentScope {
        server_instance_id: state.access.manifest.server_instance_id,
        share_id: state.access.manifest.share_id,
        item_id: state.access.manifest.item_id,
        kind: state.access.manifest.kind,
    }
}

impl AppCore {
    /// Recover a previously human-confirmed pre-admission target transcript
    /// from its public paired packet. No API read grants target access to the
    /// item body before admission; this path neither pins new trust nor signs
    /// a fresh request/response. Its immutable OS witnesses must already exist.
    pub async fn enrollment_restore_pairing(
        &self,
        public_bundle_json: String,
        confirmed_whole_code: String,
    ) -> AppResult<Vec<EnrollmentRequestDto>> {
        let session = self.session().await?;
        Box::pin(self.enrollment_restore_pairing_in(
            session,
            public_bundle_json,
            confirmed_whole_code,
        ))
        .await
    }
    async fn enrollment_restore_pairing_in(
        &self,
        session: Arc<Session>,
        public_bundle_json: String,
        confirmed_whole_code: String,
    ) -> AppResult<Vec<EnrollmentRequestDto>> {
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), true).await?;
        let packet = parse_bundle(&public_bundle_json)?;
        let verified = verify_bundle(&packet, historical_time(&packet))?;
        let request = verified
            .request
            .as_ref()
            .ok_or_else(|| invalid("restoration requires the signed target request"))?;
        verified.endorsement.as_ref().ok_or_else(|| {
            invalid("restoration requires the independently paired anchor endorsement")
        })?;
        let code = enrollment_pairing_code(request).map_err(enrollment_error)?;
        if !code_matches(&code, &confirmed_whole_code) {
            return Err(invalid(
                "confirm the complete independently compared pairing code",
            ));
        }
        let target = &request.signed().request.target;
        if target.user_id != env.pin.user
            || target.device_id != env.pin.device
            || target.encryption_public_key != env.pin.encryption
            || target.signing_public_key != env.pin.signing
        {
            return Err(invalid("restoration packet belongs to another device"));
        }
        let id = request.signed().request.scope.share_id;
        let marks = highwater(&env);
        let key = record_key(id);
        let (retained, old_raw) = env
            .wait(env.session.storage.call(move |db| -> AppResult<_> {
                let _lock = marks.lock().map_err(mark_error)?;
                db.read(|tx| -> AppResult<_> {
                    let retained = recovery_marks(&marks, id)?;
                    let raw: Option<String> = tx.setting_get(&key)?;
                    if let Some(raw) = &raw {
                        if raw.len() > MAX_ENROLLMENT_RECORD_BYTES {
                            return Err(invalid("enrollment transcript exceeds the limit"));
                        }
                        let old: EnrollmentRecord = serde_json::from_str(raw)
                            .map_err(|_| invalid("invalid encrypted enrollment transcript"))?;
                        validate_record(&old)?;
                        if old.scope != retained.scope
                            || owner_from(&old.owner, retained.scope.server_instance_id)?
                                != retained.owner
                            || old.grants.iter().any(|g| {
                                !retained
                                    .grants
                                    .contains_key(&g.history.last().unwrap().grant.grant_id)
                            })
                            || old.requests.iter().any(|r| {
                                !retained
                                    .requests
                                    .get(&r.state.request.request.request_id)
                                    .is_some_and(|m| {
                                        transcript_hash(enrollment_request_hash_input(
                                            &r.state.request,
                                        ))
                                        .ok()
                                            == Some(m.hash)
                                    })
                            })
                        {
                            return Err(AppError::SharingReconciliationRequired);
                        }
                    }
                    Ok((retained, raw))
                })
            }))
            .await?;
        self.sharing_check(&env).await?;
        let grant_id = verified.grant.signed().grant.grant_id;
        if retained.scope != request.signed().request.scope
            || retained.owner != verified.owner
            || retained
                .requests
                .get(&request.signed().request.request_id)
                .is_none_or(|m| m.hash != request.hash())
            || retained.grants.len() != 1
            || !retained.grants.contains_key(&grant_id)
        {
            return Err(AppError::SharingReconciliationRequired);
        }
        let original = retained.grants.get(&grant_id).unwrap();
        let mut mark = GrantMark::authenticated(&verified.grant).map_err(mark_error)?;
        mark.frozen = original.frozen;
        // A single source packet cannot invent omitted signed grant/history
        // heads. A packet older than an independently retained head is blocked.
        if &mark != original {
            return Err(AppError::SharingReconciliationRequired);
        }
        let transport = self
            .enrollment_transport(&env, retained.scope.clone(), true)
            .await?;
        let mut rebuilt = EnrollmentRecord {
            format: 1,
            scope: retained.scope.clone(),
            owner: packet.owner.clone(),
            manifests: packet.manifests.clone(),
            header: packet.header.clone(),
            grants: vec![StoredGrant {
                history: packet.grant_history.clone(),
                mark,
            }],
            requests: vec![],
        };
        for (id, mark) in &retained.requests {
            let remote = env.wait(transport.get_request(*id)).await?;
            self.sharing_check(&env).await?;
            if transcript_hash(enrollment_request_hash_input(&remote.request))? != mark.hash
                || remote.grant_id != grant_id
                || remote.request.request.grant_state_hash.as_slice() != verified.grant.hash()
                || remote.request.request.access_manifest_hash.as_slice() != verified.access.hash()
                || remote.request.request.target != *target
            {
                return Err(AppError::SharingReconciliationRequired);
            }
            if remote.status == OwnDeviceRequestStatus::Accepted {
                // The admitted target can now request full signed item history.
                // Re-enter the same captured session after releasing this gate;
                // the immutable OS owner code is already independently bound.
                drop(_gate);
                self.enrollment_reconcile_in(
                    session.clone(),
                    retained.scope.share_id.to_string(),
                    packet.owner.verification_code.clone(),
                )
                .await?;
                let _gate = self.sharing_gate(&session).await?;
                let env = self.sharing_environment(session.clone(), true).await?;
                let record = self
                    .enrollment_load(&env, retained.scope.share_id)
                    .await?
                    .ok_or(AppError::SharingReconciliationRequired)?;
                return record.value.requests.iter().map(request_dto).collect();
            }
            if mark.pending_receipt.is_some() {
                return Err(AppError::SharingReconciliationRequired);
            }
            let mut bundle = packet.clone();
            bundle.request = Some(remote.request.clone());
            bundle.endorsement = Some(remote.endorsement.clone());
            let current = verify_bundle(&bundle, historical_time(&bundle))?;
            let challenge = remote
                .challenge
                .as_ref()
                .map(|signed| {
                    verify_device_challenge(
                        signed,
                        &retained.owner,
                        current.request.as_ref().unwrap(),
                        current.endorsement.as_ref().unwrap(),
                        &current.access,
                        bounded_time(
                            historical_time(&bundle),
                            signed.challenge.not_before,
                            signed.challenge.expires_at,
                        ),
                        mark.challenge.as_ref(),
                    )
                    .map_err(enrollment_error)
                })
                .transpose()?;
            if mark.challenge.is_some() && challenge.is_none() {
                return Err(AppError::SharingReconciliationRequired);
            }
            if let (Some(response), Some(challenge)) = (&remote.response, &challenge) {
                verify_device_response(
                    response,
                    current.request.as_ref().unwrap(),
                    challenge,
                    &current.access,
                    bounded_time(
                        historical_time(&bundle),
                        challenge.signed().challenge.not_before,
                        challenge.signed().challenge.expires_at,
                    ),
                )
                .map_err(enrollment_error)?;
            }
            rebuilt.requests.push(StoredRequest {
                bundle,
                state: remote,
                checkpoint: challenge.map(|c| c.checkpoint().into()),
                pending_acceptance: None,
            });
        }
        self.enrollment_commit_recovery(&env, &retained, old_raw, &rebuilt)
            .await?;
        rebuilt.requests.iter().map(request_dto).collect()
    }
}
