//! AppCore selective-sharing lifecycle and ciphertext-only outbox. Personal
//! vault objects/keys never enter the sharing transport. A directory response
//! is metadata until the human confirms its independently compared key code.

use crate::secrets::blocking;
use crate::session::{Session, Unlocked};
use crate::sharing_dto::*;
use crate::sharing_projection::SharedProjection;
use crate::sharing_state::{SharingBinding, SharingHistory, SharingSnapshot, SharingStateStore};
use crate::{AppCore, AppError, AppResult};
use cc_crypto_core::sharing::{
    shared_body_hash, sharing_identity_code, verify_shared_manifest, verify_shared_mutation,
    SharingOwnerAnchor, VerifiedSharingManifest,
};
use cc_crypto_core::DevicePublicKeys;
use cc_platform_core::ExposeSecret as _;
use cc_protocol::sharing::{
    validate_state, AccessManifest, CreateShareRequest, PutSharedRevisionRequest,
    RotateShareAccessRequest, SharedItemKind, SharedItemState, SharedRevision, SharingContext,
    SharingMember, SharingMutation, SharingOperation, SharingRole,
};
use cc_protocol::{Bytes, DeviceId, MutationId, ObjectId, ShareId, UserId};
use cc_storage_core::ProfileKind;
use cc_sync_core::api::SharingApi;
use cc_sync_core::ApiError;
use cc_vault_core::DeviceIdentity;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::str::FromStr as _;
use std::sync::Arc;
use uuid::Uuid;

const WORKSPACE_KEY: &str = "cc.sharing.facade.v1";
const MAX_ITEMS: usize = 10_000;
const MAX_HISTORY_PAGES: usize = 100;

#[path = "sharing_enrollment_api.rs"]
pub(crate) mod enrollment;

async fn wait_generation<T, E: Into<AppError>>(
    unlocked: &Unlocked,
    future: impl std::future::Future<Output = Result<T, E>>,
) -> AppResult<T> {
    let mut shutdown = unlocked.sharing_shutdown.subscribe();
    if *shutdown.borrow() {
        return Err(AppError::VaultLocked);
    }
    tokio::select! {
        biased;
        _ = shutdown.changed() => Err(AppError::VaultLocked),
        result = future => result.map_err(Into::into),
    }
}

fn invalid(reason: &'static str) -> AppError {
    AppError::invalid("sharing", reason)
}

fn integrity(error: impl std::fmt::Display + 'static) -> AppError {
    if matches!(
        (&error as &dyn std::any::Any).downcast_ref::<crate::sharing_state::SharingStateError>(),
        Some(crate::sharing_state::SharingStateError::ReconciliationRequired)
    ) {
        return AppError::SharingReconciliationRequired;
    }
    AppError::Crypto("shared signatures, trust state or ciphertext could not be verified".into())
}

fn pin_name(profile: cc_storage_core::ProfileId) -> String {
    format!("profiles/{profile}/sharing-instance-v1")
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstancePin {
    format: u16,
    instance: Uuid,
    user: UserId,
    device: DeviceId,
    encryption: Bytes,
    signing: Bytes,
    server_url: String,
    #[serde(default)]
    supports_groups: bool,
    #[serde(default)]
    supports_secrets: bool,
    #[serde(default)]
    supports_owner_online_enrollment_v1: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexedItem {
    binding: SharingBinding,
    blocked: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum QueuedKind {
    Create,
    Put,
    Rotate,
}

/// This type has no plaintext, DEK, VRK, password or secret-key field.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EncryptedOutboxEntry {
    kind: QueuedKind,
    binding: SharingBinding,
    state: SharedItemState,
    expected_revision: i64,
    expected_revision_hash: Bytes,
    expected_manifest_revision: u64,
    expected_manifest_hash: Bytes,
    blocked_reason: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Workspace {
    format: u16,
    instance: Uuid,
    user: UserId,
    device: DeviceId,
    items: Vec<IndexedItem>,
    outbox: Vec<EncryptedOutboxEntry>,
}

struct Environment {
    session: Arc<Session>,
    unlocked: Arc<Unlocked>,
    identity: Arc<DeviceIdentity>,
    pin: InstancePin,
    workspace: Workspace,
    serialized: Option<String>,
    transport: Option<SharingApi>,
}

impl Environment {
    fn supports(&self, kind: SharedItemKind) -> bool {
        match kind {
            SharedItemKind::Host | SharedItemKind::Snippet => true,
            SharedItemKind::Group => self.pin.supports_groups,
            SharedItemKind::Secret => self.pin.supports_secrets,
        }
    }

    fn require_kind(&self, kind: SharedItemKind) -> AppResult<()> {
        if !self.supports(kind) {
            return Err(AppError::Unsupported(
                "this shared content kind is disabled on the server".into(),
            ));
        }
        Ok(())
    }
    async fn wait<T, E: Into<AppError>>(
        &self,
        future: impl std::future::Future<Output = Result<T, E>>,
    ) -> AppResult<T> {
        wait_generation(&self.unlocked, future).await
    }

    async fn raw<T>(
        &self,
        future: impl std::future::Future<Output = Result<T, ApiError>>,
    ) -> AppResult<Result<T, ApiError>> {
        self.wait(async { Ok::<_, AppError>(future.await) }).await
    }
    fn store(&self) -> AppResult<SharingStateStore> {
        SharingStateStore::new(
            self.session.storage.clone(),
            self.session.profile_id,
            self.pin.instance,
            self.session.ctx.secure.clone(),
        )
        .map_err(integrity)
    }

    fn own_anchor(&self) -> SharingOwnerAnchor {
        SharingOwnerAnchor {
            user_id: self.pin.user,
            device_id: self.pin.device,
            public_keys: self.identity.public_keys(),
        }
    }

    fn own_member(&self) -> SharingMember {
        SharingMember {
            user_id: self.pin.user,
            device_id: self.pin.device,
            encryption_public_key: self.pin.encryption.clone(),
            signing_public_key: self.pin.signing.clone(),
            role: SharingRole::Editor,
        }
    }

    fn transport(&self) -> AppResult<&SharingApi> {
        self.transport
            .as_ref()
            .ok_or_else(|| invalid("online capability verification is required"))
    }

    fn ensure_not_pending(&self, id: ShareId) -> AppResult<()> {
        if self
            .workspace
            .outbox
            .iter()
            .any(|entry| entry.binding.share_id == id)
        {
            return Err(invalid(
                "this item already has a pending change; review or discard it",
            ));
        }
        Ok(())
    }

    fn binding(&self, id: ShareId) -> AppResult<SharingBinding> {
        let item = self
            .workspace
            .items
            .iter()
            .find(|item| item.binding.share_id == id)
            .ok_or_else(|| AppError::not_found("shared item", id))?;
        if item.blocked {
            return Err(invalid(
                "shared access is blocked; review the current rights",
            ));
        }
        Ok(item.binding.clone())
    }

    fn queue(
        &mut self,
        kind: QueuedKind,
        binding: SharingBinding,
        state: SharedItemState,
        previous: Option<&SharingSnapshot>,
    ) -> AppResult<()> {
        self.ensure_not_pending(binding.share_id)?;
        if self.workspace.outbox.len() >= 100 {
            return Err(invalid("sharing outbox is full"));
        }
        validate_state(&state).map_err(integrity)?;
        let checkpoint = previous.map(SharingSnapshot::revision_checkpoint);
        let manifest = previous.map(SharingSnapshot::manifest_checkpoint);
        self.workspace.outbox.push(EncryptedOutboxEntry {
            kind,
            binding: binding.clone(),
            state,
            expected_revision: checkpoint.map_or(0, |checkpoint| checkpoint.revision),
            expected_revision_hash: Bytes::from(
                checkpoint.map_or([0; 32], |checkpoint| checkpoint.hash),
            ),
            expected_manifest_revision: manifest.map_or(0, |checkpoint| checkpoint.revision),
            expected_manifest_hash: Bytes::from(
                manifest.map_or([0; 32], |checkpoint| checkpoint.hash),
            ),
            blocked_reason: None,
        });
        if !self
            .workspace
            .items
            .iter()
            .any(|item| item.binding == binding)
        {
            self.workspace.items.push(IndexedItem {
                binding,
                blocked: false,
            });
        }
        Ok(())
    }

    fn outbox_dto(&self) -> SharingOutboxDto {
        SharingOutboxDto {
            entries: self
                .workspace
                .outbox
                .iter()
                .map(|entry| SharingOutboxEntryDto {
                    mutation_id: entry.state.revision.signed.mutation.mutation_id.to_string(),
                    share_id: entry.binding.share_id.to_string(),
                    state: if entry.blocked_reason.is_some() {
                        SharingOutboxStateDto::Blocked
                    } else {
                        SharingOutboxStateDto::Pending
                    },
                    reason: entry.blocked_reason.clone(),
                })
                .collect(),
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("String formatting cannot fail");
    }
    output
}

fn unhex(input: &str) -> AppResult<[u8; 32]> {
    if input.len() != 64 || !input.is_ascii() {
        return Err(invalid("public keys must be 32-byte hexadecimal values"));
    }
    let mut output = [0; 32];
    for (index, byte) in output.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&input[index * 2..index * 2 + 2], 16)
            .map_err(|_| invalid("invalid hexadecimal public key"))?;
    }
    Ok(output)
}

fn identity_dto(instance: Uuid, member: &SharingMember) -> AppResult<SharingIdentityDto> {
    let keys = DevicePublicKeys::from_slices(
        member.encryption_public_key.as_slice(),
        member.signing_public_key.as_slice(),
    )?;
    Ok(SharingIdentityDto {
        server_instance_id: instance.to_string(),
        user_id: member.user_id.to_string(),
        device_id: member.device_id.to_string(),
        encryption_public_key: hex(member.encryption_public_key.as_slice()),
        signing_public_key: hex(member.signing_public_key.as_slice()),
        verification_code: sharing_identity_code(instance, member.user_id, member.device_id, &keys)
            .map_err(integrity)?,
    })
}

fn code_matches(actual: &str, confirmed: &str) -> bool {
    let normalize = |code: &str| {
        code.bytes()
            .filter(|byte| *byte != b'-' && !byte.is_ascii_whitespace())
            .map(|byte| byte.to_ascii_uppercase())
            .collect::<Vec<_>>()
    };
    let actual = normalize(actual);
    let confirmed = normalize(confirmed);
    confirmed.len() == 64 && confirmed.iter().all(u8::is_ascii_hexdigit) && confirmed == actual
}

fn grants(
    instance: Uuid,
    own: SharingMember,
    input: Vec<SharingGrantDto>,
) -> AppResult<Vec<SharingMember>> {
    let mut members = BTreeMap::from([(own.device_id, own)]);
    for grant in input {
        let identity = grant.identity;
        if identity.server_instance_id != instance.to_string() {
            return Err(invalid("recipient belongs to another server instance"));
        }
        let member = SharingMember {
            user_id: UserId::from_str(&identity.user_id)
                .map_err(|_| invalid("invalid recipient account"))?,
            device_id: DeviceId::from_str(&identity.device_id)
                .map_err(|_| invalid("invalid recipient device"))?,
            encryption_public_key: Bytes::from(unhex(&identity.encryption_public_key)?),
            signing_public_key: Bytes::from(unhex(&identity.signing_public_key)?),
            role: match grant.role {
                SharingRoleDto::Reader => SharingRole::Reader,
                SharingRoleDto::Editor => SharingRole::Editor,
            },
        };
        let computed = identity_dto(instance, &member)?;
        if !code_matches(&computed.verification_code, &identity.verification_code)
            || !code_matches(&computed.verification_code, &grant.confirmed_code)
        {
            return Err(invalid(
                "independently compared recipient verification code is required",
            ));
        }
        if members.insert(member.device_id, member).is_some() {
            return Err(invalid("a recipient device was supplied more than once"));
        }
    }
    if members.len() > cc_protocol::sharing::MAX_MEMBERS {
        return Err(invalid("too many recipient devices"));
    }
    Ok(members.into_values().collect())
}

fn binding_of(state: &SharedItemState) -> SharingBinding {
    let context = &state.revision.signed.mutation.context;
    SharingBinding {
        server_instance_id: context.server_instance_id,
        share_id: context.share_id,
        item_id: context.item_id,
        kind: context.kind,
    }
}

fn owner_from(state: &SharedItemState) -> AppResult<SharingOwnerAnchor> {
    let access = &state.access.manifest;
    let owner = access
        .members
        .iter()
        .find(|member| {
            member.device_id == access.owner_device_id && member.user_id == access.owner_user_id
        })
        .ok_or_else(|| invalid("missing shared owner identity"))?;
    Ok(SharingOwnerAnchor {
        user_id: owner.user_id,
        device_id: owner.device_id,
        public_keys: DevicePublicKeys::from_slices(
            owner.encryption_public_key.as_slice(),
            owner.signing_public_key.as_slice(),
        )?,
    })
}

fn item_dto(
    env: &Environment,
    state: &SharedItemState,
    trust: SharingTrustDto,
    projection: Option<&SharedProjection>,
) -> AppResult<SharingItemDto> {
    let access = &state.access.manifest;
    Ok(SharingItemDto {
        share_id: access.share_id.to_string(),
        item_id: access.item_id.to_string(),
        kind: match access.kind {
            SharedItemKind::Host => SharingKindDto::Host,
            SharedItemKind::Snippet => SharingKindDto::Snippet,
            SharedItemKind::Group => SharingKindDto::Group,
            SharedItemKind::Secret => SharingKindDto::Secret,
        },
        owner_user_id: access.owner_user_id.to_string(),
        owner_device_id: access.owner_device_id.to_string(),
        revision: state.revision.signed.mutation.context.revision,
        access_epoch: access.access_epoch,
        owned: access.owner_user_id == env.pin.user,
        role: if trust == SharingTrustDto::Unverified {
            None
        } else {
            access
                .members
                .iter()
                .find(|member| member.device_id == env.pin.device)
                .map(|member| match member.role {
                    SharingRole::Reader => SharingRoleDto::Reader,
                    SharingRole::Editor => SharingRoleDto::Editor,
                })
        },
        trust,
        blocked_reason: (trust == SharingTrustDto::Blocked).then(|| "access_removed".into()),
        preview_json: projection.map(projection_preview_json).transpose()?,
        members: access
            .members
            .iter()
            .map(|member| identity_dto(env.pin.instance, member))
            .collect::<AppResult<_>>()?,
        member_roles: access
            .members
            .iter()
            .map(|member| {
                Ok(SharingMemberDto {
                    identity: identity_dto(env.pin.instance, member)?,
                    role: match member.role {
                        SharingRole::Reader => SharingRoleDto::Reader,
                        SharingRole::Editor => SharingRoleDto::Editor,
                    },
                })
            })
            .collect::<AppResult<_>>()?,
    })
}

/// Ordinary UI metadata never serializes a SecretValue, even temporarily.
/// The reveal action below is the sole sharing facade path returning it.
fn projection_preview_json(projection: &SharedProjection) -> AppResult<String> {
    if let SharedProjection::Secret(secret) = projection {
        #[derive(Serialize)]
        struct SecretMetadata<'a> {
            name: &'a str,
            secret_kind: cc_models::secret::SecretKind,
        }
        #[derive(Serialize)]
        struct MetadataEnvelope<'a> {
            kind: &'static str,
            data: SecretMetadata<'a>,
        }
        return serde_json::to_string(&MetadataEnvelope {
            kind: "secret",
            data: SecretMetadata {
                name: &secret.name,
                secret_kind: secret.secret_kind,
            },
        })
        .map_err(|_| invalid("invalid shared secret metadata"));
    }
    let bytes = projection.encode()?;
    String::from_utf8(bytes.as_slice().to_vec())
        .map_err(|_| invalid("invalid shared projection encoding"))
}

fn trusted_manifest(snapshot: &SharingSnapshot) -> AppResult<VerifiedSharingManifest> {
    verify_shared_manifest(
        &snapshot.state().access,
        &snapshot.state().revision.signed.mutation.context,
        &snapshot.owner_anchor(),
        None,
    )
    .map_err(integrity)
}

fn make_revision(
    env: &Environment,
    manifest: &VerifiedSharingManifest,
    context: SharingContext,
    projection: Option<&SharedProjection>,
    previous: Option<&SharingSnapshot>,
) -> AppResult<SharedRevision> {
    let body = projection
        .map(|projection| {
            projection.encode().and_then(|encoded| {
                env.identity
                    .sharing_seal_revision(manifest, &context, encoded.as_slice())
                    .map_err(integrity)
            })
        })
        .transpose()?;
    let previous_checkpoint = previous.map(SharingSnapshot::revision_checkpoint);
    let mutation = SharingMutation {
        base_revision: context.revision - 1,
        context,
        mutation_id: MutationId::new(),
        manifest_revision: manifest.manifest().revision,
        manifest_hash: Bytes::from(manifest.hash()),
        writer_device_id: env.pin.device,
        previous_revision_hash: Bytes::from(
            previous_checkpoint.map_or([0; 32], |checkpoint| checkpoint.hash),
        ),
        operation: if body.is_some() {
            SharingOperation::Put
        } else {
            SharingOperation::Delete
        },
        body_hash: Bytes::from(
            body.as_ref()
                .map(shared_body_hash)
                .transpose()
                .map_err(integrity)?
                .unwrap_or([0; 32]),
        ),
    };
    let signed = env
        .identity
        .sharing_sign_mutation(
            manifest,
            mutation,
            body.as_ref(),
            previous_checkpoint.as_ref(),
        )
        .map_err(integrity)?;
    Ok(SharedRevision { signed, body })
}

impl AppCore {
    async fn sharing_recover_own_create(
        &self,
        env: &Environment,
        store: &SharingStateStore,
        binding: &SharingBinding,
    ) -> AppResult<Option<SharingSnapshot>> {
        let Some(entry) = env
            .workspace
            .outbox
            .iter()
            .find(|entry| entry.kind == QueuedKind::Create && entry.binding == *binding)
        else {
            return Ok(None);
        };
        self.sharing_validate_dispatch(env, entry, None)?;
        self.sharing_check(env).await?;
        let accepted = env
            .wait(async {
                store
                    .pin_share(
                        binding.clone(),
                        env.own_anchor(),
                        entry.state.clone(),
                        env.pin.device,
                        env.identity.as_ref(),
                    )
                    .await
                    .map_err(integrity)
            })
            .await?;
        self.sharing_check(env).await?;
        drop(accepted.projection);
        Ok(Some(accepted.snapshot))
    }

    fn sharing_blocked_metadata(
        &self,
        _env: &Environment,
        binding: &SharingBinding,
    ) -> AppResult<SharingItemDto> {
        Ok(SharingItemDto {
            share_id: binding.share_id.to_string(),
            item_id: binding.item_id.to_string(),
            kind: match binding.kind {
                SharedItemKind::Host => SharingKindDto::Host,
                SharedItemKind::Snippet => SharingKindDto::Snippet,
                SharedItemKind::Group => SharingKindDto::Group,
                SharedItemKind::Secret => SharingKindDto::Secret,
            },
            owner_user_id: String::new(),
            owner_device_id: String::new(),
            revision: 0,
            access_epoch: 0,
            owned: false,
            role: None,
            trust: SharingTrustDto::Blocked,
            blocked_reason: Some("sharing_reconciliation_required".into()),
            preview_json: None,
            members: vec![],
            member_roles: vec![],
        })
    }
    async fn sharing_gate<'a>(
        &self,
        session: &'a Session,
    ) -> AppResult<tokio::sync::MutexGuard<'a, ()>> {
        let unlocked = session.unlocked().await?;
        wait_generation(&unlocked, async {
            Ok::<_, AppError>(session.sharing_gate.lock().await)
        })
        .await
    }
    async fn sharing_history(
        &self,
        env: &Environment,
        previous: &SharingSnapshot,
        current: &SharedItemState,
    ) -> AppResult<SharingHistory> {
        let target_manifest = current.access.manifest.revision;
        let target_revision = current.revision.signed.mutation.context.revision;
        let mut manifest_cursor = previous.manifest_checkpoint().revision;
        let mut revision_cursor = previous.revision_checkpoint().revision;
        let mut history = SharingHistory::default();
        for _ in 0..MAX_HISTORY_PAGES {
            if manifest_cursor >= target_manifest && revision_cursor >= target_revision {
                return Ok(history);
            }
            self.sharing_check(env).await?;
            let page = env
                .wait(env.transport()?.share_history(
                    previous.binding().share_id,
                    manifest_cursor,
                    revision_cursor,
                    100,
                ))
                .await?;
            self.sharing_check(env).await?;
            let before = (manifest_cursor, revision_cursor);
            for signed in page.manifests {
                if signed.manifest.revision <= target_manifest {
                    manifest_cursor = signed.manifest.revision;
                    history.manifests.push(signed);
                }
            }
            for signed in page.revisions {
                if signed.mutation.context.revision <= target_revision {
                    revision_cursor = signed.mutation.context.revision;
                    history.revisions.push(signed);
                }
            }
            if before == (manifest_cursor, revision_cursor) || !page.has_more {
                // StateStore requires exact successors and rejects a withheld
                // intermediate state before ciphertext is decrypted.
                return Ok(history);
            }
        }
        Err(invalid(
            "shared history exceeds the bounded verification limit",
        ))
    }

    async fn sharing_refresh_state(
        &self,
        env: &Environment,
        store: &SharingStateStore,
        binding: &SharingBinding,
        current: SharedItemState,
    ) -> AppResult<SharingSnapshot> {
        self.sharing_check(env).await?;
        let previous = env
            .wait(async { store.load(binding).await.map_err(integrity) })
            .await?
            .ok_or_else(|| invalid("shared owner has not been explicitly accepted"))?;
        self.sharing_check(env).await?;
        if previous.state() == &current {
            return Ok(previous);
        }
        let history = self.sharing_history(env, &previous, &current).await?;
        self.sharing_check(env).await?;
        let accepted = env
            .wait(async {
                store
                    .apply(
                        &previous,
                        history,
                        current,
                        env.pin.device,
                        env.identity.as_ref(),
                    )
                    .await
                    .map_err(integrity)
            })
            .await?;
        self.sharing_check(env).await?;
        // Projection is validated by the store but never becomes a retained
        // session cache or an allocation spanning another network request.
        drop(accepted.projection);
        Ok(accepted.snapshot)
    }

    pub async fn sharing_list(&self, refresh: bool) -> AppResult<Vec<SharingItemDto>> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let mut env = self.sharing_environment(session.clone(), refresh).await?;
        let store = env.store()?;
        let mut remote = BTreeMap::new();
        if refresh {
            let mut after = None;
            let mut complete = false;
            for _ in 0..100 {
                let page = env
                    .wait(env.transport()?.list_shares_with_kinds(
                        after,
                        100,
                        env.pin.supports_groups,
                        env.pin.supports_secrets,
                    ))
                    .await?;
                self.sharing_check(&env).await?;
                for item in page.items {
                    remote.insert(item.access.manifest.share_id, item);
                }
                if remote.len() > MAX_ITEMS {
                    return Err(invalid("too many shared items"));
                }
                if !page.has_more {
                    complete = true;
                    break;
                }
                after = page.next_after;
                if after.is_none() {
                    return Err(invalid("invalid shared item cursor"));
                }
            }
            if !complete {
                return Err(invalid("shared item listing exceeds its page bound"));
            }
        }
        let mut snapshots = Vec::new();
        let mut blocked_items = Vec::new();
        let mut index_changed = false;
        for indexed in env.workspace.items.clone() {
            let binding = indexed.binding;
            if !env.supports(binding.kind) {
                remote.remove(&binding.share_id);
                let mut item = self.sharing_blocked_metadata(&env, &binding)?;
                item.blocked_reason = Some("unsupported_sharing_kind".into());
                blocked_items.push(item);
                continue;
            }
            let cached = match env
                .wait(async { store.load(&binding).await.map_err(integrity) })
                .await
            {
                Ok(cached) => cached,
                Err(AppError::SharingReconciliationRequired) => {
                    remote.remove(&binding.share_id);
                    blocked_items.push(self.sharing_blocked_metadata(&env, &binding)?);
                    continue;
                }
                Err(error) => return Err(error),
            };
            self.sharing_check(&env).await?;
            let cached = if cached.is_none() {
                self.sharing_recover_own_create(&env, &store, &binding)
                    .await?
            } else {
                cached
            };
            let Some(cached) = cached else {
                return Err(invalid("accepted shared item cache is missing"));
            };
            let mut blocked = indexed.blocked;
            let snapshot = if let Some(current) = remote.remove(&binding.share_id) {
                let verified = self
                    .sharing_refresh_state(&env, &store, &binding, current)
                    .await?;
                blocked = false;
                verified
            } else if refresh
                && !env.workspace.outbox.iter().any(|entry| {
                    entry.binding.share_id == binding.share_id && entry.kind == QueuedKind::Create
                })
            {
                match env
                    .raw(env.transport()?.get_share(binding.share_id))
                    .await?
                {
                    Ok(current) => {
                        blocked = false;
                        self.sharing_refresh_state(&env, &store, &binding, current)
                            .await?
                    }
                    Err(error) if matches!(error.status(), Some(403 | 404)) => {
                        blocked = true;
                        cached
                    }
                    Err(error) => return Err(error.into()),
                }
            } else {
                cached
            };
            self.sharing_check(&env).await?;
            if let Some(indexed) = env
                .workspace
                .items
                .iter_mut()
                .find(|item| item.binding == binding)
            {
                index_changed |= indexed.blocked != blocked;
                indexed.blocked = blocked;
            }
            snapshots.push((snapshot, blocked));
        }
        if index_changed {
            self.sharing_save(&mut env).await?;
        }
        let mut items = blocked_items;
        for current in remote.into_values() {
            if !env.supports(current.access.manifest.kind) {
                let mut item = item_dto(&env, &current, SharingTrustDto::Blocked, None)?;
                item.blocked_reason = Some("unsupported_sharing_kind".into());
                items.push(item);
                continue;
            }
            items.push(item_dto(&env, &current, SharingTrustDto::Unverified, None)?);
        }
        // Only after all network/history work finishes, open accepted cached
        // projections. Shutdown cancels the remaining work and zeroizes DTOs.
        for (snapshot, blocked) in snapshots {
            self.sharing_check(&env).await?;
            let projection = if blocked {
                None
            } else {
                env.wait(async {
                    store
                        .open_cached(&snapshot, env.pin.device, env.identity.as_ref())
                        .await
                        .map_err(integrity)
                })
                .await?
            };
            self.sharing_check(&env).await?;
            let trust = if blocked {
                SharingTrustDto::Blocked
            } else if snapshot.state().revision.signed.mutation.operation
                == SharingOperation::Delete
            {
                SharingTrustDto::Deleted
            } else {
                SharingTrustDto::Verified
            };
            items.push(item_dto(
                &env,
                snapshot.state(),
                trust,
                projection.as_ref(),
            )?);
        }
        self.sharing_check(&env).await?;
        Ok(items)
    }

    fn sharing_validate_dispatch(
        &self,
        env: &Environment,
        entry: &EncryptedOutboxEntry,
        previous: Option<&SharingSnapshot>,
    ) -> AppResult<()> {
        env.require_kind(entry.binding.kind)?;
        if entry.binding != binding_of(&entry.state)
            || entry.binding.server_instance_id != env.pin.instance
        {
            return Err(invalid(
                "queued sharing request belongs to another item or instance",
            ));
        }
        validate_state(&entry.state).map_err(integrity)?;
        let anchor = if entry.kind == QueuedKind::Create {
            env.own_anchor()
        } else {
            previous
                .ok_or_else(|| invalid("queued sharing request has no accepted owner"))?
                .owner_anchor()
        };
        let context = &entry.state.revision.signed.mutation.context;
        let manifest_checkpoint = if entry.kind == QueuedKind::Rotate {
            previous.map(SharingSnapshot::manifest_checkpoint)
        } else {
            None
        };
        let manifest = verify_shared_manifest(
            &entry.state.access,
            context,
            &anchor,
            manifest_checkpoint.as_ref(),
        )
        .map_err(integrity)?;
        let previous_checkpoint = if entry.kind == QueuedKind::Create {
            None
        } else {
            previous.map(SharingSnapshot::revision_checkpoint)
        };
        let revision = verify_shared_mutation(
            &manifest,
            &entry.state.revision.signed,
            previous_checkpoint.as_ref(),
        )
        .map_err(integrity)?;
        if revision.mutation().writer_device_id != env.pin.device {
            return Err(invalid("queued writer is another device"));
        }
        use cc_crypto_core::sharing::SharingRevisionOpener as _;
        let plaintext = env
            .identity
            .open_shared_revision(
                &manifest,
                &revision,
                entry.state.revision.body.as_ref(),
                env.pin.device,
            )
            .map_err(integrity)?;
        if let Some(plain) = plaintext {
            // Validate then drop before dispatch; ciphertext alone survives.
            SharedProjection::decode(entry.binding.kind, plain.as_slice())?;
        }
        Ok(())
    }

    async fn sharing_block(
        &self,
        env: &mut Environment,
        mutation: MutationId,
        reason: &'static str,
    ) -> AppResult<()> {
        if let Some(entry) = env
            .workspace
            .outbox
            .iter_mut()
            .find(|entry| entry.state.revision.signed.mutation.mutation_id == mutation)
        {
            entry.blocked_reason = Some(reason.into());
        }
        self.sharing_save(env).await
    }

    async fn sharing_accepted_outbox(
        &self,
        env: &mut Environment,
        store: &SharingStateStore,
        entry: &EncryptedOutboxEntry,
        current: SharedItemState,
    ) -> AppResult<()> {
        if current != entry.state {
            return Err(invalid(
                "accepted sharing request differs from the queued signed request",
            ));
        }
        self.sharing_refresh_state(env, store, &entry.binding, current)
            .await?;
        let id = entry.state.revision.signed.mutation.mutation_id;
        env.workspace
            .outbox
            .retain(|candidate| candidate.state.revision.signed.mutation.mutation_id != id);
        self.sharing_save(env).await
    }

    /// Explicit dispatch, including reconnect after an offline edit. Support,
    /// instance and the current signed access/head are checked before each
    /// request. CAS conflicts block; lost responses recover only exact state.
    pub async fn sharing_flush(&self) -> AppResult<SharingOutboxDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let mut env = self.sharing_environment(session.clone(), true).await?;
        let store = env.store()?;
        let transport = env.transport()?.clone();
        for entry in env.workspace.outbox.clone() {
            if entry.blocked_reason.is_some() {
                continue;
            }
            if !env.supports(entry.binding.kind) {
                self.sharing_block(
                    &mut env,
                    entry.state.revision.signed.mutation.mutation_id,
                    "unsupported_sharing_kind",
                )
                .await?;
                continue;
            }
            self.sharing_check(&env).await?;
            let cached = env
                .wait(async { store.load(&entry.binding).await.map_err(integrity) })
                .await?;
            self.sharing_check(&env).await?;
            if cached.is_none() && entry.kind == QueuedKind::Create {
                self.sharing_recover_own_create(&env, &store, &entry.binding)
                    .await?;
            }
            env.wait(transport.recheck_capabilities()).await?;
            self.sharing_check(&env).await?;
            let mutation = entry.state.revision.signed.mutation.mutation_id;
            let remote = env.raw(transport.get_share(entry.binding.share_id)).await?;
            self.sharing_check(&env).await?;
            let previous = match remote {
                Ok(current) if current == entry.state => {
                    // A prior request reached the server but its response was
                    // lost. No replay: verify and remove this exact request.
                    self.sharing_accepted_outbox(&mut env, &store, &entry, current)
                        .await?;
                    continue;
                }
                Ok(current) => {
                    let previous = self
                        .sharing_refresh_state(&env, &store, &entry.binding, current)
                        .await?;
                    let checkpoint = previous.revision_checkpoint();
                    let manifest = previous.manifest_checkpoint();
                    if entry.kind == QueuedKind::Create
                        || checkpoint.revision != entry.expected_revision
                        || checkpoint.hash.as_slice() != entry.expected_revision_hash.as_slice()
                        || manifest.revision != entry.expected_manifest_revision
                        || manifest.hash.as_slice() != entry.expected_manifest_hash.as_slice()
                    {
                        self.sharing_block(&mut env, mutation, "head_changed")
                            .await?;
                        continue;
                    }
                    Some(previous)
                }
                Err(error) if error.status() == Some(404) && entry.kind == QueuedKind::Create => {
                    None
                }
                Err(error) if matches!(error.status(), Some(403 | 404)) => {
                    self.sharing_block(&mut env, mutation, "access_removed")
                        .await?;
                    if let Some(index) = env
                        .workspace
                        .items
                        .iter_mut()
                        .find(|item| item.binding == entry.binding)
                    {
                        index.blocked = true;
                    }
                    self.sharing_save(&mut env).await?;
                    continue;
                }
                Err(ApiError::Network(_) | ApiError::Timeout) => break,
                Err(error) => return Err(error.into()),
            };
            if self
                .sharing_validate_dispatch(&env, &entry, previous.as_ref())
                .is_err()
            {
                self.sharing_block(&mut env, mutation, "rights_or_integrity_changed")
                    .await?;
                continue;
            }
            self.sharing_check(&env).await?;
            let request = async {
                match entry.kind {
                    QueuedKind::Create => {
                        transport
                            .create_share(&CreateShareRequest {
                                access: entry.state.access.clone(),
                                revision: entry.state.revision.clone(),
                            })
                            .await
                    }
                    QueuedKind::Put => {
                        transport
                            .put_shared_revision(
                                entry.binding.share_id,
                                &PutSharedRevisionRequest {
                                    revision: entry.state.revision.clone(),
                                },
                            )
                            .await
                    }
                    QueuedKind::Rotate => {
                        transport
                            .rotate_shared_access(
                                entry.binding.share_id,
                                &RotateShareAccessRequest {
                                    access: entry.state.access.clone(),
                                    revision: entry.state.revision.clone(),
                                },
                            )
                            .await
                    }
                }
            };
            let sent = env.raw(request).await?;
            self.sharing_check(&env).await?;
            match sent {
                Ok(current) => {
                    self.sharing_accepted_outbox(&mut env, &store, &entry, current)
                        .await?
                }
                Err(error) => {
                    // Recover a lost response or a raced CAS only if all signed
                    // document bytes and ciphertext equal the accepted state.
                    if let Ok(current) =
                        env.raw(transport.get_share(entry.binding.share_id)).await?
                    {
                        self.sharing_check(&env).await?;
                        if current == entry.state {
                            self.sharing_accepted_outbox(&mut env, &store, &entry, current)
                                .await?;
                            continue;
                        }
                    }
                    match error {
                        ApiError::Network(_) | ApiError::Timeout => break,
                        error if error.status() == Some(409) => {
                            self.sharing_block(&mut env, mutation, "conflict").await?
                        }
                        error if matches!(error.status(), Some(403 | 404)) => {
                            self.sharing_block(&mut env, mutation, "access_removed")
                                .await?
                        }
                        error => return Err(error.into()),
                    }
                }
            }
        }
        self.sharing_check(&env).await?;
        Ok(env.outbox_dto())
    }

    /// Every await crossing checks the same active profile/unlocked generation
    /// and identity. A lock/profile change cancels plaintext results even when
    /// an already-authorized ciphertext HTTP request finishes afterwards.
    async fn sharing_check(&self, env: &Environment) -> AppResult<()> {
        let active = self.session().await?;
        if !Arc::ptr_eq(&active, &env.session) {
            return Err(AppError::VaultLocked);
        }
        let unlocked = env.session.unlocked().await?;
        if !Arc::ptr_eq(&unlocked, &env.unlocked)
            || !unlocked.codec.is_unlocked()
            || env.session.identity().device_id() != env.identity.device_id()
            || env.session.identity().public_keys() != env.identity.public_keys()
        {
            return Err(AppError::VaultLocked);
        }
        Ok(())
    }

    async fn sharing_environment(
        &self,
        session: Arc<Session>,
        online: bool,
    ) -> AppResult<Environment> {
        let unlocked = session.unlocked().await?;
        let identity = session.identity();
        let profile = wait_generation(&unlocked, session.profile()).await?;
        if profile.kind != ProfileKind::Synced {
            return Err(AppError::LocalProfile);
        }
        let user = profile.user_id.ok_or(AppError::ReauthRequired)?;
        if profile.device_id != identity.device_id() {
            return Err(AppError::NewDeviceIdentityRequired);
        }
        let server_url = profile.server_url.ok_or(AppError::LocalProfile)?;
        let secure = session.ctx.secure.clone();
        let name = pin_name(session.profile_id);
        let existing = wait_generation(
            &unlocked,
            blocking(move || {
                secure
                    .get(&name)?
                    .map(|bytes| {
                        serde_json::from_slice::<InstancePin>(bytes.expose_secret())
                            .map_err(|_| invalid("stored sharing instance identity is invalid"))
                    })
                    .transpose()
            }),
        )
        .await?;
        let mut pin = existing.clone().unwrap_or(InstancePin {
            format: 1,
            instance: Uuid::nil(),
            user,
            device: identity.device_id(),
            encryption: identity.public_keys().encryption_bytes(),
            signing: identity.public_keys().signing_bytes(),
            server_url: server_url.clone(),
            supports_groups: false,
            supports_secrets: false,
            supports_owner_online_enrollment_v1: false,
        });
        if pin.format != 1
            || pin.user != user
            || pin.device != identity.device_id()
            || pin.server_url != server_url
            || pin.encryption != identity.public_keys().encryption_bytes()
            || pin.signing != identity.public_keys().signing_bytes()
        {
            return Err(invalid(
                "sharing instance/account/device pin differs; use a separate profile",
            ));
        }
        let mut env = Environment {
            session,
            unlocked,
            identity,
            workspace: Workspace {
                format: 1,
                instance: pin.instance,
                user,
                device: pin.device,
                items: vec![],
                outbox: vec![],
            },
            pin: pin.clone(),
            serialized: None,
            transport: None,
        };
        self.sharing_check(&env).await?;
        if online || existing.is_none() {
            let api = env.session.api()?;
            let caps = env.wait(api.sharing_capabilities()).await?;
            self.sharing_check(&env).await?;
            if !caps.enabled
                || caps.format != 1
                || caps.server_instance_id.is_nil()
                || caps.max_members < 1
                || caps.max_ciphertext_bytes < 20
            {
                return Err(AppError::Unsupported(
                    "selective sharing is unavailable on this server".into(),
                ));
            }
            if existing.is_some() && pin.instance != caps.server_instance_id {
                return Err(invalid(
                    "server instance differs from the securely pinned instance",
                ));
            }
            let me = env.wait(api.me()).await?;
            self.sharing_check(&env).await?;
            if me.user_id != pin.user || me.current_device_id != pin.device {
                return Err(invalid(
                    "authenticated sharing account/device differs from this profile",
                ));
            }
            pin.instance = caps.server_instance_id;
            pin.supports_groups = caps.supports_groups;
            pin.supports_secrets = caps.supports_secrets;
            pin.supports_owner_online_enrollment_v1 = caps.supports_owner_online_enrollment_v1;
            env.pin = pin.clone();
            env.workspace.instance = pin.instance;
            let transport = env.wait(api.sharing(pin.instance)).await?;
            self.sharing_check(&env).await?;
            env.transport = Some(transport);
            if existing.as_ref() != Some(&pin) {
                let secure = env.session.ctx.secure.clone();
                let name = pin_name(env.session.profile_id);
                let serialized = serde_json::to_vec(&pin)
                    .map_err(|_| invalid("could not encode sharing instance pin"))?;
                env.wait(blocking(move || {
                    secure.set(&name, &serialized)?;
                    Ok(())
                }))
                .await?;
                self.sharing_check(&env).await?;
            }
        }
        if env.pin.instance.is_nil() {
            return Err(invalid(
                "sharing server instance must be pinned online first",
            ));
        }
        let serialized: Option<String> = env
            .wait(env.session.storage.setting_get(WORKSPACE_KEY))
            .await?;
        self.sharing_check(&env).await?;
        if let Some(raw) = &serialized {
            env.workspace = serde_json::from_str(raw)
                .map_err(|_| invalid("stored sharing workspace is invalid"))?;
            if env.workspace.format != 1
                || env.workspace.instance != env.pin.instance
                || env.workspace.user != env.pin.user
                || env.workspace.device != env.pin.device
                || env.workspace.items.len() > MAX_ITEMS
                || env.workspace.outbox.len() > 100
            {
                return Err(invalid("stored sharing workspace identity differs"));
            }
        }
        env.serialized = serialized;
        Ok(env)
    }

    async fn sharing_save(&self, env: &mut Environment) -> AppResult<()> {
        self.sharing_check(env).await?;
        let next = serde_json::to_string(&env.workspace)
            .map_err(|_| invalid("cannot encode encrypted sharing workspace"))?;
        let expected = env.serialized.clone();
        let replacement = next.clone();
        let changed = env
            .wait(env.session.storage.write(
                move |tx| -> Result<bool, cc_storage_core::StorageError> {
                    let existing: Option<String> = tx.setting_get(WORKSPACE_KEY)?;
                    if existing != expected {
                        return Ok(false);
                    }
                    tx.setting_set(WORKSPACE_KEY, &replacement)?;
                    Ok(true)
                },
            ))
            .await?;
        self.sharing_check(env).await?;
        if !changed {
            return Err(invalid(
                "sharing state changed locally; reload before continuing",
            ));
        }
        env.serialized = Some(next);
        Ok(())
    }

    pub async fn sharing_status(&self) -> AppResult<SharingStatusDto> {
        let session = self.session().await?;
        if session.unlocked_opt().await.is_none() {
            return Ok(SharingStatusDto {
                enabled: false,
                locked: true,
                server_instance_id: None,
                identity: None,
                pending: 0,
                blocked: 0,
                supports_groups: false,
                supports_secrets: false,
                supports_owner_online_enrollment_v1: false,
            });
        }
        if session.kind().await? != ProfileKind::Synced {
            return Ok(SharingStatusDto {
                enabled: false,
                locked: false,
                server_instance_id: None,
                identity: None,
                pending: 0,
                blocked: 0,
                supports_groups: false,
                supports_secrets: false,
                supports_owner_online_enrollment_v1: false,
            });
        }
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), true).await?;
        self.sharing_check(&env).await?;
        Ok(SharingStatusDto {
            enabled: true,
            locked: false,
            server_instance_id: Some(env.pin.instance.to_string()),
            identity: Some(identity_dto(env.pin.instance, &env.own_member())?),
            pending: env
                .workspace
                .outbox
                .iter()
                .filter(|entry| entry.blocked_reason.is_none())
                .count() as u32,
            blocked: env
                .workspace
                .outbox
                .iter()
                .filter(|entry| entry.blocked_reason.is_some())
                .count() as u32,
            supports_groups: env.pin.supports_groups,
            supports_secrets: env.pin.supports_secrets,
            supports_owner_online_enrollment_v1: env.pin.supports_owner_online_enrollment_v1,
        })
    }

    pub async fn sharing_identity(&self) -> AppResult<SharingIdentityDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), false).await?;
        self.sharing_check(&env).await?;
        identity_dto(env.pin.instance, &env.own_member())
    }

    pub async fn sharing_discover(&self, email: String) -> AppResult<SharingRecipientDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), true).await?;
        let recipient = env.wait(env.transport()?.sharing_recipient(&email)).await?;
        self.sharing_check(&env).await?;
        Ok(SharingRecipientDto {
            user_id: recipient.user_id.to_string(),
            devices: recipient
                .devices
                .iter()
                .map(|member| identity_dto(env.pin.instance, member))
                .collect::<AppResult<_>>()?,
        })
    }

    /// Explicit recovery after restoring an older encrypted database. The
    /// independently preserved OS checkpoint and immutable owner pin must be
    /// reached exactly by signed history. Missing OS trust is never recreated.
    pub async fn sharing_reconcile(
        &self,
        share_id: String,
        confirmed_owner_code: String,
    ) -> AppResult<SharingItemDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let mut env = self.sharing_environment(session.clone(), true).await?;
        let id = ShareId::from_str(&share_id).map_err(|_| invalid("invalid shared item id"))?;
        let observed = env.wait(env.transport()?.get_share(id)).await?;
        self.sharing_check(&env).await?;
        let binding = binding_of(&observed);
        env.require_kind(binding.kind)?;
        let owner = owner_from(&observed)?;
        let code = sharing_identity_code(
            env.pin.instance,
            owner.user_id,
            owner.device_id,
            &owner.public_keys,
        )
        .map_err(integrity)?;
        if !code_matches(&code, &confirmed_owner_code) {
            return Err(invalid(
                "independently compared owner verification code is required",
            ));
        }
        let store = env.store()?;
        let base = env
            .wait(async { store.reconciliation_base(&binding).await.map_err(integrity) })
            .await;
        self.sharing_check(&env).await?;
        let accepted = match base {
            Ok(base) => {
                if base.owner_anchor() != owner {
                    return Err(invalid("an accepted owner pin cannot be replaced"));
                }
                let history = self.sharing_history(&env, &base, &observed).await?;
                self.sharing_check(&env).await?;
                env.wait(async {
                    store
                        .reconcile_history(
                            &base,
                            history,
                            observed,
                            env.pin.device,
                            env.identity.as_ref(),
                        )
                        .await
                        .map_err(integrity)
                })
                .await?
            }
            Err(AppError::SharingReconciliationRequired) => {
                env.wait(async {
                    store
                        .reconcile_missing_record(
                            binding.clone(),
                            owner,
                            observed,
                            env.pin.device,
                            env.identity.as_ref(),
                        )
                        .await
                        .map_err(|_| AppError::SharingReconciliationRequired)
                })
                .await?
            }
            Err(error) => return Err(error),
        };
        self.sharing_check(&env).await?;
        drop(accepted.projection);
        if let Some(indexed) = env
            .workspace
            .items
            .iter_mut()
            .find(|indexed| indexed.binding == binding)
        {
            indexed.blocked = false;
        } else {
            if env.workspace.items.len() >= MAX_ITEMS {
                return Err(invalid("too many shared items"));
            }
            env.workspace.items.push(IndexedItem {
                binding,
                blocked: false,
            });
        }
        self.sharing_save(&mut env).await?;
        let projection = env
            .wait(async {
                store
                    .open_cached(&accepted.snapshot, env.pin.device, env.identity.as_ref())
                    .await
                    .map_err(integrity)
            })
            .await?;
        self.sharing_check(&env).await?;
        item_dto(
            &env,
            accepted.snapshot.state(),
            SharingTrustDto::Verified,
            projection.as_ref(),
        )
    }

    pub async fn sharing_inspect(&self, share_id: String) -> AppResult<SharingInvitationDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), true).await?;
        let id = ShareId::from_str(&share_id).map_err(|_| invalid("invalid shared item id"))?;
        let state = env.wait(env.transport()?.get_share(id)).await?;
        self.sharing_check(&env).await?;
        let anchor = owner_from(&state)?;
        let owner = SharingMember {
            user_id: anchor.user_id,
            device_id: anchor.device_id,
            encryption_public_key: anchor.public_keys.encryption_bytes(),
            signing_public_key: anchor.public_keys.signing_bytes(),
            role: SharingRole::Editor,
        };
        Ok(SharingInvitationDto {
            item: item_dto(&env, &state, SharingTrustDto::Unverified, None)?,
            owner: identity_dto(env.pin.instance, &owner)?,
        })
    }

    pub async fn sharing_accept(
        &self,
        share_id: String,
        confirmed_owner_code: String,
    ) -> AppResult<SharingItemDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let mut env = self.sharing_environment(session.clone(), true).await?;
        let id = ShareId::from_str(&share_id).map_err(|_| invalid("invalid shared item id"))?;
        let state = env.wait(env.transport()?.get_share(id)).await?;
        self.sharing_check(&env).await?;
        let anchor = owner_from(&state)?;
        let code = sharing_identity_code(
            env.pin.instance,
            anchor.user_id,
            anchor.device_id,
            &anchor.public_keys,
        )
        .map_err(integrity)?;
        if !code_matches(&code, &confirmed_owner_code) {
            return Err(invalid(
                "independently compared owner verification code is required",
            ));
        }
        let binding = binding_of(&state);
        env.require_kind(binding.kind)?;
        let store = env.store()?;
        let existing = env
            .wait(async { store.load(&binding).await.map_err(integrity) })
            .await?;
        self.sharing_check(&env).await?;
        let snapshot = if let Some(existing) = existing {
            if existing.owner_anchor() != anchor {
                return Err(invalid("an accepted owner pin cannot be replaced"));
            }
            self.sharing_refresh_state(&env, &store, &binding, state)
                .await?
        } else {
            let accepted = env
                .wait(async {
                    store
                        .pin_share(
                            binding.clone(),
                            anchor,
                            state,
                            env.pin.device,
                            env.identity.as_ref(),
                        )
                        .await
                        .map_err(integrity)
                })
                .await?;
            drop(accepted.projection);
            accepted.snapshot
        };
        self.sharing_check(&env).await?;
        if !env
            .workspace
            .items
            .iter()
            .any(|item| item.binding == binding)
        {
            if env.workspace.items.len() >= MAX_ITEMS {
                return Err(invalid("too many shared items"));
            }
            env.workspace.items.push(IndexedItem {
                binding,
                blocked: false,
            });
        }
        self.sharing_save(&mut env).await?;
        let projection = env
            .wait(async {
                store
                    .open_cached(&snapshot, env.pin.device, env.identity.as_ref())
                    .await
                    .map_err(integrity)
            })
            .await?;
        self.sharing_check(&env).await?;
        item_dto(
            &env,
            snapshot.state(),
            SharingTrustDto::Verified,
            projection.as_ref(),
        )
    }

    pub async fn sharing_publish(
        &self,
        projection: SharedProjection,
        recipients: Vec<SharingGrantDto>,
    ) -> AppResult<SharingItemDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let mut env = self.sharing_environment(session.clone(), false).await?;
        self.sharing_publish_prepared(&mut env, projection, recipients)
            .await
    }

    /// Selected stored secrets remain inside Rust until encrypted for the
    /// independently verified recipients. The result contains metadata only.
    pub async fn sharing_publish_secret(
        &self,
        credential_id: String,
        passphrase: bool,
        recipients: Vec<SharingGrantDto>,
    ) -> AppResult<SharingItemDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let mut env = self.sharing_environment(session.clone(), false).await?;
        env.require_kind(SharedItemKind::Secret)?;
        self.sharing_check(&env).await?;
        let projection = env
            .wait(self.sharing_prepare_credential(credential_id, passphrase))
            .await?;
        self.sharing_check(&env).await?;
        self.sharing_publish_prepared(&mut env, projection, recipients)
            .await
    }

    /// Explicit Reveal/Copy only. Revalidate access and the complete signed
    /// history before opening the current ciphertext; do not retain a cache.
    pub async fn sharing_reveal_secret(
        &self,
        share_id: String,
    ) -> AppResult<crate::RevealedSecret> {
        self.sharing_with_verified_projection(
            share_id,
            SharedItemKind::Secret,
            |_, _, projection| {
                Box::pin(async move {
                    let SharedProjection::Secret(secret) = projection else {
                        return Err(invalid("the shared item has another kind"));
                    };
                    Ok(crate::RevealedSecret {
                        kind: "shared_secret",
                        value: secret.value.expose_secret().to_owned(),
                    })
                })
            },
        )
        .await
    }

    /// Execute an explicit local action against one captured unlocked profile.
    /// The callback cannot outlive verified plaintext or reenter a different
    /// profile through an unguarded AppCore writer. It receives no private key.
    pub(crate) async fn sharing_with_verified_projection<T, F>(
        &self,
        share_id: String,
        kind: SharedItemKind,
        action: F,
    ) -> AppResult<T>
    where
        T: Send,
        F: for<'a> FnOnce(
            &'a Unlocked,
            &'a SharingSnapshot,
            &'a SharedProjection,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = AppResult<T>> + Send + 'a>,
        >,
    {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), true).await?;
        let id = ShareId::from_str(&share_id).map_err(|_| invalid("invalid shared item id"))?;
        let binding = env.binding(id)?;
        if binding.kind != kind {
            return Err(invalid("this shared item has another kind"));
        }
        env.require_kind(binding.kind)?;
        let store = env.store()?;
        let current = env.wait(env.transport()?.get_share(id)).await?;
        self.sharing_check(&env).await?;
        let snapshot = self
            .sharing_refresh_state(&env, &store, &binding, current)
            .await?;
        let projection = env
            .wait(async {
                store
                    .open_cached(&snapshot, env.pin.device, env.identity.as_ref())
                    .await
                    .map_err(integrity)
            })
            .await?;
        self.sharing_check(&env).await?;
        let Some(projection) = projection else {
            return Err(invalid("the shared item is deleted"));
        };
        let result = env
            .wait(action(&env.unlocked, &snapshot, &projection))
            .await?;
        self.sharing_check(&env).await?;
        Ok(result)
    }

    async fn sharing_publish_prepared(
        &self,
        env: &mut Environment,
        projection: SharedProjection,
        recipients: Vec<SharingGrantDto>,
    ) -> AppResult<SharingItemDto> {
        env.require_kind(projection.kind())?;
        let encoded = projection.encode()?;
        let projection = SharedProjection::decode(projection.kind(), encoded.as_slice())?;
        drop(encoded);
        let context = SharingContext {
            server_instance_id: env.pin.instance,
            share_id: ShareId::new(),
            item_id: ObjectId::new(),
            revision: 1,
            access_epoch: 1,
            kind: projection.kind(),
        };
        let manifest = AccessManifest {
            format: 1,
            server_instance_id: env.pin.instance,
            share_id: context.share_id,
            item_id: context.item_id,
            owner_user_id: env.pin.user,
            owner_device_id: env.pin.device,
            revision: 1,
            access_epoch: 1,
            previous_manifest_hash: Bytes::from([0; 32]),
            kind: context.kind,
            members: grants(env.pin.instance, env.own_member(), recipients)?,
        };
        let access = env
            .identity
            .sharing_sign_manifest(manifest, &context, &env.own_anchor(), None)
            .map_err(integrity)?;
        let verified = verify_shared_manifest(&access, &context, &env.own_anchor(), None)
            .map_err(integrity)?;
        let state = SharedItemState {
            access,
            revision: make_revision(env, &verified, context, Some(&projection), None)?,
        };
        let binding = binding_of(&state);
        self.sharing_check(env).await?;
        // Persist the recoverable ciphertext request before the independent
        // cache pin. Cancellation here cannot lose the publication request.
        env.queue(QueuedKind::Create, binding.clone(), state.clone(), None)?;
        self.sharing_save(env).await?;
        let store = env.store()?;
        let accepted = env
            .wait(async {
                store
                    .pin_share(
                        binding,
                        env.own_anchor(),
                        state.clone(),
                        env.pin.device,
                        env.identity.as_ref(),
                    )
                    .await
                    .map_err(integrity)
            })
            .await?;
        self.sharing_check(env).await?;
        item_dto(
            env,
            &state,
            SharingTrustDto::Verified,
            accepted.projection.as_ref(),
        )
    }

    pub async fn sharing_edit(
        &self,
        share_id: String,
        projection: SharedProjection,
    ) -> AppResult<SharingOutboxDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let mut env = self.sharing_environment(session.clone(), false).await?;
        let id = ShareId::from_str(&share_id).map_err(|_| invalid("invalid shared item id"))?;
        env.ensure_not_pending(id)?;
        let binding = env.binding(id)?;
        env.require_kind(binding.kind)?;
        if binding.kind != projection.kind() {
            return Err(invalid("shared content kind differs"));
        }
        let encoded = projection.encode()?;
        let projection = SharedProjection::decode(binding.kind, encoded.as_slice())?;
        let store = env.store()?;
        let previous = env
            .wait(async { store.load(&binding).await.map_err(integrity) })
            .await?
            .ok_or_else(|| invalid("shared owner has not been accepted"))?;
        self.sharing_check(&env).await?;
        let manifest = trusted_manifest(&previous)?;
        let mut context = previous.state().revision.signed.mutation.context.clone();
        context.revision = context
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("shared revision limit reached"))?;
        let state = SharedItemState {
            access: previous.state().access.clone(),
            revision: make_revision(&env, &manifest, context, Some(&projection), Some(&previous))?,
        };
        env.queue(QueuedKind::Put, binding, state, Some(&previous))?;
        self.sharing_save(&mut env).await?;
        Ok(env.outbox_dto())
    }

    pub async fn sharing_rotate(
        &self,
        share_id: String,
        recipients: Vec<SharingGrantDto>,
    ) -> AppResult<SharingOutboxDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let mut env = self.sharing_environment(session.clone(), false).await?;
        let id = ShareId::from_str(&share_id).map_err(|_| invalid("invalid shared item id"))?;
        env.ensure_not_pending(id)?;
        let binding = env.binding(id)?;
        env.require_kind(binding.kind)?;
        let store = env.store()?;
        let previous = env
            .wait(async { store.load(&binding).await.map_err(integrity) })
            .await?
            .ok_or_else(|| invalid("shared owner has not been accepted"))?;
        self.sharing_check(&env).await?;
        if previous.owner_anchor() != env.own_anchor() {
            return Err(invalid("only the original owner device may change access"));
        }
        let projection = env
            .wait(async {
                store
                    .open_cached(&previous, env.pin.device, env.identity.as_ref())
                    .await
                    .map_err(integrity)
            })
            .await?
            .ok_or_else(|| invalid("deleted shared items cannot be changed"))?;
        self.sharing_check(&env).await?;
        let mut manifest = previous.state().access.manifest.clone();
        manifest.revision = manifest
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("manifest limit reached"))?;
        manifest.access_epoch = manifest
            .access_epoch
            .checked_add(1)
            .ok_or_else(|| invalid("access epoch limit reached"))?;
        manifest.previous_manifest_hash = Bytes::from(previous.manifest_checkpoint().hash);
        let next_members = grants(env.pin.instance, env.own_member(), recipients)?;
        for next in &next_members {
            if let Some(old) = manifest
                .members
                .iter()
                .find(|old| old.device_id == next.device_id)
            {
                if old.user_id != next.user_id
                    || old.encryption_public_key != next.encryption_public_key
                    || old.signing_public_key != next.signing_public_key
                {
                    return Err(invalid(
                        "changed device keys require a separately verified new device identity",
                    ));
                }
            }
        }
        manifest.members = next_members;
        let mut context = previous.state().revision.signed.mutation.context.clone();
        context.revision = context
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("shared revision limit reached"))?;
        context.access_epoch = manifest.access_epoch;
        let access = env
            .identity
            .sharing_sign_manifest(
                manifest,
                &context,
                &env.own_anchor(),
                Some(&previous.manifest_checkpoint()),
            )
            .map_err(integrity)?;
        let verified = verify_shared_manifest(
            &access,
            &context,
            &env.own_anchor(),
            Some(&previous.manifest_checkpoint()),
        )
        .map_err(integrity)?;
        let state = SharedItemState {
            access,
            revision: make_revision(&env, &verified, context, Some(&projection), Some(&previous))?,
        };
        env.queue(QueuedKind::Rotate, binding, state, Some(&previous))?;
        self.sharing_save(&mut env).await?;
        Ok(env.outbox_dto())
    }

    pub async fn sharing_delete(&self, share_id: String) -> AppResult<SharingOutboxDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let mut env = self.sharing_environment(session.clone(), false).await?;
        let id = ShareId::from_str(&share_id).map_err(|_| invalid("invalid shared item id"))?;
        env.ensure_not_pending(id)?;
        let binding = env.binding(id)?;
        env.require_kind(binding.kind)?;
        let store = env.store()?;
        let previous = env
            .wait(async { store.load(&binding).await.map_err(integrity) })
            .await?
            .ok_or_else(|| invalid("shared owner has not been accepted"))?;
        self.sharing_check(&env).await?;
        if previous.owner_anchor() != env.own_anchor() {
            return Err(invalid(
                "only the original owner device may delete this item",
            ));
        }
        let manifest = trusted_manifest(&previous)?;
        let mut context = previous.state().revision.signed.mutation.context.clone();
        context.revision = context
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("shared revision limit reached"))?;
        let state = SharedItemState {
            access: previous.state().access.clone(),
            revision: make_revision(&env, &manifest, context, None, Some(&previous))?,
        };
        env.queue(QueuedKind::Put, binding, state, Some(&previous))?;
        self.sharing_save(&mut env).await?;
        Ok(env.outbox_dto())
    }

    pub async fn sharing_outbox(&self) -> AppResult<SharingOutboxDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), false).await?;
        self.sharing_check(&env).await?;
        Ok(env.outbox_dto())
    }

    /// Explicit local review action. A blocked request is never rewritten or
    /// re-encrypted automatically; a later edit creates a fresh signed request.
    pub async fn sharing_discard_pending(
        &self,
        mutation_id: String,
    ) -> AppResult<SharingOutboxDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let mut env = self.sharing_environment(session.clone(), false).await?;
        let id = MutationId::from_str(&mutation_id)
            .map_err(|_| invalid("invalid sharing mutation id"))?;
        let before = env.workspace.outbox.len();
        env.workspace
            .outbox
            .retain(|entry| entry.state.revision.signed.mutation.mutation_id != id);
        if before == env.workspace.outbox.len() {
            return Err(AppError::not_found("sharing mutation", id));
        }
        self.sharing_save(&mut env).await?;
        Ok(env.outbox_dto())
    }
}

#[path = "sharing_secret_api.rs"]
mod sharing_secrets;
