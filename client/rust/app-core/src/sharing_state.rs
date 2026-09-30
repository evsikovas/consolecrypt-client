//! Durable selective-sharing trust state in one profile's SQLCipher database.
//!
//! This library boundary does not fetch keys from a directory or establish
//! human identity. `pin_share` requires an explicitly confirmed owner anchor.
//! The caller supplies this device's keys only while its vault is unlocked.
//! No plaintext, DEK or personal vault key is persisted here. Sharing does not
//! import hosts, execute commands or expose decrypted content to an LLM.
//!
//! Checkpoints are derived from authenticated signed documents, stored with
//! the owner pin and opaque latest body in one local record. Applying a remote
//! state verifies every intervening chain link, then opens and validates the
//! current projection before an atomic compare-and-swap. A stale candidate
//! never overwrites a concurrently advanced checkpoint; callers must reload.
//! Independent OS secure-store marks detect rollback of the profile database.
//! Missing/mismatched marks fail closed; recovery is an explicit reconciliation
//! against the preserved exact mark, never a new pin. Initial acceptance can
//! still be stale, and a server can withhold an unseen revocation. Rollback of
//! the OS secure store itself is outside this local guarantee.

use crate::sharing_highwater::{HighwaterMarker, SharingHighwater, SharingHighwaterError};
use crate::sharing_projection::SharedProjection;
use crate::AppError;
use cc_crypto_core::sharing::{
    verify_shared_manifest, verify_shared_mutation, SharingCryptoError, SharingManifestCheckpoint,
    SharingOwnerAnchor, SharingRevisionCheckpoint, SharingRevisionOpener, VerifiedSharingManifest,
};
use cc_crypto_core::DevicePublicKeys;
use cc_platform_core::SecureStore;
use cc_protocol::sharing::{
    validate_state, SharedItemKind, SharedItemState, SharingContext, SharingOperation,
    SignedAccessManifest, SignedSharingMutation,
};
use cc_protocol::{Bytes, DeviceId, ObjectId, ShareId, UserId};
use cc_storage_core::{ProfileId, Storage, StorageError};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt, sync::Arc};
use uuid::Uuid;

const RECORD_FORMAT: u16 = 1;
const MAX_HISTORY_STATES: usize = 10_000;

#[derive(Debug, thiserror::Error)]
pub enum SharingStateError {
    #[error(transparent)]
    Crypto(#[from] SharingCryptoError),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Highwater(#[from] SharingHighwaterError),
    #[error("sharing cache and independent checkpoint require explicit reconciliation")]
    ReconciliationRequired,
    #[error("invalid shared projection")]
    Projection(#[source] AppError),
    #[error("sharing state belongs to another instance or item")]
    BindingMismatch,
    #[error("sharing snapshot belongs to another profile store")]
    WrongStore,
    #[error("sharing owner is already pinned; reload current state")]
    AlreadyPinned,
    #[error("sharing state changed locally; reload before applying")]
    Conflict,
    #[error("stored sharing state is invalid")]
    CorruptRecord,
    #[error("sharing history is malformed or exceeds the limit")]
    InvalidHistory,
    #[error("a shared tombstone cannot be resurrected")]
    Deleted,
}

type Result<T> = std::result::Result<T, SharingStateError>;

/// Public identity of one item. This, the owner pin and profile database must
/// stay bound together; a list/directory response never replaces them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharingBinding {
    pub server_instance_id: Uuid,
    pub share_id: ShareId,
    pub item_id: ObjectId,
    pub kind: SharedItemKind,
}

impl SharingBinding {
    fn context(&self, revision: i64, access_epoch: u64) -> SharingContext {
        SharingContext {
            server_instance_id: self.server_instance_id,
            share_id: self.share_id,
            item_id: self.item_id,
            revision,
            access_epoch,
            kind: self.kind,
        }
    }

    fn matches(&self, c: &SharingContext) -> bool {
        self.server_instance_id == c.server_instance_id
            && self.share_id == c.share_id
            && self.item_id == c.item_id
            && self.kind == c.kind
    }
}

/// Signed lightweight history after the last accepted checkpoint. The batch
/// must be strictly ordered. It may include the latest state header/manifest;
/// otherwise they are verified as the final exact successors by `apply`.
#[derive(Debug, Clone, Default)]
pub struct SharingHistory {
    pub manifests: Vec<SignedAccessManifest>,
    pub revisions: Vec<SignedSharingMutation>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredOwner {
    user_id: UserId,
    device_id: DeviceId,
    encryption_public_key: Bytes,
    signing_public_key: Bytes,
}

impl StoredOwner {
    fn from_anchor(a: &SharingOwnerAnchor) -> Self {
        Self {
            user_id: a.user_id,
            device_id: a.device_id,
            encryption_public_key: a.public_keys.encryption_bytes(),
            signing_public_key: a.public_keys.signing_bytes(),
        }
    }

    fn anchor(&self) -> Result<SharingOwnerAnchor> {
        let public_keys = DevicePublicKeys::from_slices(
            self.encryption_public_key.as_slice(),
            self.signing_public_key.as_slice(),
        )
        .map_err(SharingCryptoError::from)?;
        Ok(SharingOwnerAnchor {
            user_id: self.user_id,
            device_id: self.device_id,
            public_keys,
        })
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredRecord {
    format: u16,
    binding: SharingBinding,
    owner: StoredOwner,
    state: SharedItemState,
}

/// An authenticated cache snapshot, tied to the store which loaded it.
/// Fields are private: callers cannot manufacture a trusted checkpoint.
#[derive(Clone)]
pub struct SharingSnapshot {
    origin: Arc<()>,
    serialized: String,
    record: StoredRecord,
    manifest: VerifiedSharingManifest,
    revision_checkpoint: SharingRevisionCheckpoint,
}

impl fmt::Debug for SharingSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SharingSnapshot")
            .field("binding", &self.record.binding)
            .field("revision", &self.revision_checkpoint.revision)
            .field("access_epoch", &self.revision_checkpoint.access_epoch)
            .finish_non_exhaustive()
    }
}

impl SharingSnapshot {
    pub fn binding(&self) -> &SharingBinding {
        &self.record.binding
    }

    pub fn state(&self) -> &SharedItemState {
        &self.record.state
    }

    pub fn owner_anchor(&self) -> SharingOwnerAnchor {
        self.record
            .owner
            .anchor()
            .expect("snapshot owner keys have been authenticated")
    }

    pub fn manifest_checkpoint(&self) -> SharingManifestCheckpoint {
        self.manifest.checkpoint()
    }

    pub fn revision_checkpoint(&self) -> SharingRevisionCheckpoint {
        self.revision_checkpoint
    }
}

/// Returned only after authentication, decryption, projection validation and
/// successful persistence. Projection fields zeroize on drop and Debug is
/// redacted. Tombstones have no projection.
#[derive(Debug)]
pub struct AcceptedSharingState {
    pub snapshot: SharingSnapshot,
    pub projection: Option<SharedProjection>,
}

#[derive(Clone)]
pub struct SharingStateStore {
    storage: Storage,
    profile_id: ProfileId,
    highwater: SharingHighwater,
    server_instance_id: Uuid,
    origin: Arc<()>,
}

impl fmt::Debug for SharingStateStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SharingStateStore")
            .field("server_instance_id", &self.server_instance_id)
            .finish_non_exhaustive()
    }
}

impl SharingStateStore {
    /// `storage` is the already opened, keyed database of one profile.
    pub fn new(
        storage: Storage,
        profile_id: ProfileId,
        server_instance_id: Uuid,
        secure: Arc<dyn SecureStore>,
    ) -> Result<Self> {
        if server_instance_id.is_nil() || profile_id.0.is_nil() {
            return Err(SharingStateError::BindingMismatch);
        }
        Ok(Self {
            storage,
            profile_id,
            highwater: SharingHighwater::new(profile_id, secure),
            server_instance_id,
            origin: Arc::new(()),
        })
    }

    fn check_binding(&self, binding: &SharingBinding) -> Result<()> {
        if binding.server_instance_id != self.server_instance_id
            || binding.share_id == ShareId::NIL
            || binding.item_id == ObjectId::NIL
        {
            return Err(SharingStateError::BindingMismatch);
        }
        Ok(())
    }

    fn key(&self, binding: &SharingBinding) -> String {
        format!(
            "cc.sharing.v1:{}:{}",
            binding.server_instance_id, binding.share_id
        )
    }

    /// Initial trust comes from an explicit out-of-band owner verification,
    /// not from account login or the server directory. An initial latest state
    /// may have revision > 1 and does not prove the server's current freshness.
    /// Existing pins are immutable through this API, including after restart.
    pub async fn pin_share(
        &self,
        binding: SharingBinding,
        owner_confirmed: SharingOwnerAnchor,
        state: SharedItemState,
        device_id: DeviceId,
        keys: &impl SharingRevisionOpener,
    ) -> Result<AcceptedSharingState> {
        self.check_binding(&binding)?;
        let record = StoredRecord {
            format: RECORD_FORMAT,
            binding,
            owner: StoredOwner::from_anchor(&owner_confirmed),
            state,
        };
        let snapshot = self.authenticate_record(record, None)?;
        let projection = self.projection(&snapshot, device_id, keys)?;
        if !self
            .compare_and_swap(&snapshot, None, CommitMode::Normal)
            .await?
        {
            return Err(SharingStateError::AlreadyPinned);
        }
        Ok(AcceptedSharingState {
            snapshot,
            projection,
        })
    }

    /// Reauthenticate the pinned owner and signed cached documents. The body
    /// is not decrypted by loading metadata; call `open_cached` while unlocked.
    /// A malformed existing record fails closed and is never reset as a new pin.
    pub async fn load(&self, binding: &SharingBinding) -> Result<Option<SharingSnapshot>> {
        self.check_binding(binding)?;
        let (raw, marker) = self.read_pair(binding).await?;
        let Some(serialized) = raw else {
            return if marker.is_none() {
                Ok(None)
            } else {
                Err(SharingStateError::ReconciliationRequired)
            };
        };
        let record: StoredRecord =
            serde_json::from_str(&serialized).map_err(|_| SharingStateError::CorruptRecord)?;
        if record.binding != *binding {
            return Err(SharingStateError::BindingMismatch);
        }
        let snapshot = self.authenticate_record(record, Some(serialized))?;
        if marker.as_ref() != Some(&self.marker(&snapshot)) {
            return Err(SharingStateError::ReconciliationRequired);
        }
        Ok(Some(snapshot))
    }

    pub async fn open_cached(
        &self,
        snapshot: &SharingSnapshot,
        device_id: DeviceId,
        keys: &impl SharingRevisionOpener,
    ) -> Result<Option<SharedProjection>> {
        self.check_snapshot(snapshot)?;
        let (raw, marker) = self.read_pair(snapshot.binding()).await?;
        if raw.as_deref() != Some(&snapshot.serialized) {
            return Err(SharingStateError::Conflict);
        }
        if marker.as_ref() != Some(&self.marker(snapshot)) {
            return Err(SharingStateError::ReconciliationRequired);
        }
        self.projection(snapshot, device_id, keys)
    }

    /// Apply a complete candidate against the exact snapshot previously read.
    /// History is authenticated before opening current ciphertext. A failed
    /// projection/AEAD check or CAS leaves the entire existing record unchanged.
    pub async fn apply(
        &self,
        previous: &SharingSnapshot,
        history: SharingHistory,
        current: SharedItemState,
        device_id: DeviceId,
        keys: &impl SharingRevisionOpener,
    ) -> Result<AcceptedSharingState> {
        let candidate = self.verify_candidate(previous, history, current, None)?;
        let projection = self.projection(&candidate, device_id, keys)?;
        if !self
            .compare_and_swap(
                &candidate,
                Some(previous.serialized.clone()),
                CommitMode::Normal,
            )
            .await?
        {
            return Err(SharingStateError::Conflict);
        }
        Ok(AcceptedSharingState {
            snapshot: candidate,
            projection,
        })
    }

    fn verify_candidate(
        &self,
        previous: &SharingSnapshot,
        history: SharingHistory,
        current: SharedItemState,
        required_marker: Option<&HighwaterMarker>,
    ) -> Result<SharingSnapshot> {
        self.check_snapshot(previous)?;
        if history.manifests.len() > MAX_HISTORY_STATES
            || history.revisions.len() > MAX_HISTORY_STATES
        {
            return Err(SharingStateError::InvalidHistory);
        }
        if previous.record.state.revision.signed.mutation.operation == SharingOperation::Delete
            && current.revision.signed.mutation.context.revision
                > previous.revision_checkpoint.revision
        {
            return Err(SharingStateError::Deleted);
        }
        let binding = &previous.record.binding;
        if !binding.matches(&current.revision.signed.mutation.context) {
            return Err(SharingStateError::BindingMismatch);
        }
        validate_state(&current).map_err(|_| SharingCryptoError::Structure("shared state"))?;
        let owner = previous.owner_anchor();
        let mut manifest = previous.manifest.clone();
        let mut manifests = BTreeMap::from([(manifest.manifest().revision, manifest.clone())]);
        for signed in history.manifests {
            if signed.manifest.revision <= manifest.manifest().revision
                || signed.manifest.revision > current.access.manifest.revision
            {
                return Err(SharingStateError::InvalidHistory);
            }
            manifest = self.next_manifest(binding, &owner, &manifest, &signed)?;
            manifests.insert(manifest.manifest().revision, manifest.clone());
        }
        manifest = self.next_manifest(binding, &owner, &manifest, &current.access)?;
        manifests.insert(manifest.manifest().revision, manifest.clone());

        let mut checkpoint = previous.revision_checkpoint;
        let mut witnessed_marker = required_marker.is_none_or(|m| *m == self.marker(previous));
        for signed in history.revisions {
            if signed.mutation.context.revision <= checkpoint.revision
                || signed.mutation.context.revision
                    > current.revision.signed.mutation.context.revision
            {
                return Err(SharingStateError::InvalidHistory);
            }
            let access = manifests
                .get(&signed.mutation.manifest_revision)
                .ok_or(SharingStateError::InvalidHistory)?;
            checkpoint = verify_shared_mutation(access, &signed, Some(&checkpoint))?.checkpoint();
            witnessed_marker |=
                required_marker.is_some_and(|m| *m == self.marker_at(previous, checkpoint));
        }
        let verified =
            verify_shared_mutation(&manifest, &current.revision.signed, Some(&checkpoint))?;
        let record = StoredRecord {
            format: RECORD_FORMAT,
            binding: binding.clone(),
            owner: previous.record.owner.clone(),
            state: current,
        };
        let candidate = self.authenticate_record(record, None)?;
        if candidate.revision_checkpoint != verified.checkpoint() {
            return Err(SharingStateError::CorruptRecord);
        }
        witnessed_marker |= required_marker.is_some_and(|m| *m == self.marker(&candidate));
        if !witnessed_marker {
            return Err(SharingStateError::ReconciliationRequired);
        }
        Ok(candidate)
    }

    /// Explicit recovery input for a rolled-back database. This authenticates
    /// only public metadata; it never opens the body or replaces a checkpoint.
    /// The immutable owner pin must match the preserved OS mark. Callers must
    /// obtain the complete signed history and reach that exact mark with
    /// `reconcile_history` before content is usable again.
    pub async fn reconciliation_base(&self, binding: &SharingBinding) -> Result<SharingSnapshot> {
        self.check_binding(binding)?;
        let (raw, marker) = self.read_pair(binding).await?;
        let raw = raw.ok_or(SharingStateError::ReconciliationRequired)?;
        let marker = marker.ok_or(SharingStateError::ReconciliationRequired)?;
        let record = serde_json::from_str(&raw).map_err(|_| SharingStateError::CorruptRecord)?;
        let base = self.authenticate_record(record, Some(raw))?;
        if !marker.permits_reconciliation_base(&self.marker(&base)) {
            return Err(SharingStateError::ReconciliationRequired);
        }
        Ok(base)
    }

    /// Repair an old cache using a gap-free authenticated chain ending at the
    /// exact independently observed checkpoint (possibly as an intermediate
    /// header before a newer current body). This does not accept an
    /// arbitrary "latest" server response or establish remote freshness.
    /// The preserved checkpoint must be witnessed before decryption or any
    /// local write, then OS-mark/DB CAS allows an authenticated newer state.
    pub async fn reconcile_history(
        &self,
        base: &SharingSnapshot,
        history: SharingHistory,
        observed_state: SharedItemState,
        device_id: DeviceId,
        keys: &impl SharingRevisionOpener,
    ) -> Result<AcceptedSharingState> {
        self.check_snapshot(base)?;
        let (raw, marker) = self.read_pair(base.binding()).await?;
        if raw.as_deref() != Some(&base.serialized) {
            return Err(SharingStateError::Conflict);
        }
        let marker = marker.ok_or(SharingStateError::ReconciliationRequired)?;
        if !marker.permits_reconciliation_base(&self.marker(base)) {
            return Err(SharingStateError::ReconciliationRequired);
        }
        let candidate = self.verify_candidate(base, history, observed_state, Some(&marker))?;
        let projection = self.projection(&candidate, device_id, keys)?;
        if !self
            .compare_and_swap(
                &candidate,
                Some(base.serialized.clone()),
                CommitMode::Reconcile {
                    expected_mark: Box::new(marker),
                },
            )
            .await?
        {
            return Err(SharingStateError::Conflict);
        }
        Ok(AcceptedSharingState {
            snapshot: candidate,
            projection,
        })
    }

    /// Explicit recovery when the database record was lost entirely. Both
    /// owner public keys and every signed checkpoint field must equal the
    /// existing OS mark. The caller separately reconfirms the owner; no new
    /// pin is created and a missing OS mark cannot be recovered by this API.
    pub async fn reconcile_missing_record(
        &self,
        binding: SharingBinding,
        owner_confirmed: SharingOwnerAnchor,
        observed_state: SharedItemState,
        device_id: DeviceId,
        keys: &impl SharingRevisionOpener,
    ) -> Result<AcceptedSharingState> {
        self.check_binding(&binding)?;
        let snapshot = self.authenticate_record(
            StoredRecord {
                format: RECORD_FORMAT,
                binding,
                owner: StoredOwner::from_anchor(&owner_confirmed),
                state: observed_state,
            },
            None,
        )?;
        let (raw, marker) = self.read_pair(snapshot.binding()).await?;
        if raw.is_some() {
            return Err(SharingStateError::AlreadyPinned);
        }
        if marker.as_ref() != Some(&self.marker(&snapshot)) {
            return Err(SharingStateError::ReconciliationRequired);
        }
        let marker = marker.ok_or(SharingStateError::ReconciliationRequired)?;
        let projection = self.projection(&snapshot, device_id, keys)?;
        if !self
            .compare_and_swap(
                &snapshot,
                None,
                CommitMode::Reconcile {
                    expected_mark: Box::new(marker),
                },
            )
            .await?
        {
            return Err(SharingStateError::Conflict);
        }
        Ok(AcceptedSharingState {
            snapshot,
            projection,
        })
    }

    fn next_manifest(
        &self,
        binding: &SharingBinding,
        owner: &SharingOwnerAnchor,
        previous: &VerifiedSharingManifest,
        signed: &SignedAccessManifest,
    ) -> Result<VerifiedSharingManifest> {
        let context = binding.context(1, signed.manifest.access_epoch);
        let verified =
            verify_shared_manifest(signed, &context, owner, Some(&previous.checkpoint()))?;
        for next in &verified.manifest().members {
            if let Some(old) = previous.member(next.device_id) {
                if next.user_id != old.user_id
                    || next.encryption_public_key != old.encryption_public_key
                    || next.signing_public_key != old.signing_public_key
                {
                    return Err(SharingCryptoError::DeviceKeyMismatch.into());
                }
            }
        }
        Ok(verified)
    }

    fn authenticate_record(
        &self,
        record: StoredRecord,
        serialized: Option<String>,
    ) -> Result<SharingSnapshot> {
        self.check_binding(&record.binding)?;
        if record.format != RECORD_FORMAT {
            return Err(SharingStateError::CorruptRecord);
        }
        let context = &record.state.revision.signed.mutation.context;
        if !record.binding.matches(context) {
            return Err(SharingStateError::BindingMismatch);
        }
        validate_state(&record.state).map_err(|_| SharingCryptoError::Structure("shared state"))?;
        let manifest =
            verify_shared_manifest(&record.state.access, context, &record.owner.anchor()?, None)?;
        let verified = verify_shared_mutation(&manifest, &record.state.revision.signed, None)?;
        // The header is authenticated above; opening the body also verifies its
        // signed hash before decryption. Recompute no unauthenticated counters.
        let serialized = match serialized {
            Some(raw) => raw,
            None => serde_json::to_string(&record).map_err(|_| SharingStateError::CorruptRecord)?,
        };
        Ok(SharingSnapshot {
            origin: self.origin.clone(),
            serialized,
            record,
            manifest,
            revision_checkpoint: verified.checkpoint(),
        })
    }

    fn check_snapshot(&self, snapshot: &SharingSnapshot) -> Result<()> {
        if !Arc::ptr_eq(&self.origin, &snapshot.origin) {
            return Err(SharingStateError::WrongStore);
        }
        self.check_binding(&snapshot.record.binding)
    }

    fn projection(
        &self,
        snapshot: &SharingSnapshot,
        device_id: DeviceId,
        keys: &impl SharingRevisionOpener,
    ) -> Result<Option<SharedProjection>> {
        let verified = verify_shared_mutation(
            &snapshot.manifest,
            &snapshot.record.state.revision.signed,
            None,
        )?;
        let plain = keys.open_shared_revision(
            &snapshot.manifest,
            &verified,
            snapshot.record.state.revision.body.as_ref(),
            device_id,
        )?;
        plain
            .map(|plain| {
                SharedProjection::decode(snapshot.record.binding.kind, plain.as_slice())
                    .map_err(SharingStateError::Projection)
            })
            .transpose()
    }

    async fn compare_and_swap(
        &self,
        candidate: &SharingSnapshot,
        expected: Option<String>,
        mode: CommitMode,
    ) -> Result<bool> {
        let key = self.key(&candidate.record.binding);
        let next = candidate.serialized.clone();
        let binding = candidate.binding().clone();
        let mark = self.marker(candidate);
        let highwater = self.highwater.clone();
        let store = self.clone();
        // Lock before BEGIN IMMEDIATE: holding a database transaction while
        // waiting for this lock would invert lock order across connections.
        self.storage
            .call(move |db| -> Result<bool> {
                let _lock = highwater.lock()?;
                db.write(move |tx| -> Result<bool> {
                    let existing: Option<String> = tx.setting_get(&key)?;
                    if existing != expected {
                        return Ok(false);
                    }
                    let existing_mark = highwater.load(&binding)?;
                    match mode {
                        CommitMode::Normal => {
                            let expected_mark = expected
                                .as_ref()
                                .map(|raw| {
                                    let record = serde_json::from_str(raw)
                                        .map_err(|_| SharingStateError::CorruptRecord)?;
                                    store
                                        .authenticate_record(record, Some(raw.clone()))
                                        .map(|s| store.marker(&s))
                                })
                                .transpose()?;
                            if existing_mark != expected_mark {
                                return Err(SharingStateError::ReconciliationRequired);
                            }
                            // Save first: a crash or SQL commit error can only leave
                            // an advanced independent mark and a blocked cache.
                            if existing_mark.as_ref() != Some(&mark) {
                                highwater.save(&mark)?;
                            }
                        }
                        CommitMode::Reconcile { expected_mark } => {
                            if existing_mark.as_ref() != Some(expected_mark.as_ref()) {
                                return Err(SharingStateError::ReconciliationRequired);
                            }
                            if mark != *expected_mark {
                                highwater.save(&mark)?;
                            }
                        }
                    }
                    tx.setting_set(&key, &next)?;
                    Ok(true)
                })
            })
            .await
    }

    fn marker(&self, snapshot: &SharingSnapshot) -> HighwaterMarker {
        self.marker_at(snapshot, snapshot.revision_checkpoint)
    }

    fn marker_at(
        &self,
        snapshot: &SharingSnapshot,
        checkpoint: SharingRevisionCheckpoint,
    ) -> HighwaterMarker {
        HighwaterMarker::authenticated(
            self.profile_id,
            snapshot.binding().clone(),
            snapshot.owner_anchor(),
            checkpoint,
        )
    }

    async fn read_pair(
        &self,
        binding: &SharingBinding,
    ) -> Result<(Option<String>, Option<HighwaterMarker>)> {
        let key = self.key(binding);
        let binding = binding.clone();
        let highwater = self.highwater.clone();
        self.storage
            .call(move |db| -> Result<_> {
                let _lock = highwater.lock()?;
                db.read(|tx| -> Result<_> {
                    Ok((tx.setting_get(&key)?, highwater.load(&binding)?))
                })
            })
            .await
    }
}

enum CommitMode {
    Normal,
    Reconcile { expected_mark: Box<HighwaterMarker> },
}
