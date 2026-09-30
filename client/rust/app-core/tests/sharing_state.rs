use cc_app_core::sharing_projection::{SharedHostProjection, SharedProjection};
use cc_app_core::sharing_state::{
    SharingBinding, SharingHistory, SharingStateError, SharingStateStore,
};
use cc_crypto_core::sharing::{
    seal_shared_revision, shared_body_hash, sign_shared_manifest, sign_shared_mutation,
    verify_shared_manifest, verify_shared_mutation, SharingCryptoError, SharingOwnerAnchor,
};
use cc_crypto_core::{CryptoError, DeviceSecretKeys};
use cc_platform_core::{ExposeSecret, SecretSlice, SecureStoreError, MAX_SECRET_LEN};
use cc_platform_core::{InMemorySecureStore, SecureStore};
use cc_protocol::sharing::{
    AccessManifest, SharedItemKind, SharedItemState, SharedRevision, SharingContext, SharingMember,
    SharingMutation, SharingOperation, SharingRole, SignedAccessManifest,
};
use cc_protocol::{Bytes, DeviceId, MutationId, ObjectId, ShareId, UserId};
use cc_storage_core::{DatabaseKey, ProfileId, Storage};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use uuid::Uuid;

struct Fixture {
    owner: DeviceSecretKeys,
    owner_id: DeviceId,
    owner_user: UserId,
    reader: DeviceSecretKeys,
    reader_id: DeviceId,
    reader_user: UserId,
}

fn database_key() -> DatabaseKey {
    let generated = DeviceSecretKeys::generate().unwrap().to_secret_bytes();
    DatabaseKey::from_slice(&generated[5..37]).unwrap()
}

fn new_store(storage: Storage, instance: Uuid) -> SharingStateStore {
    SharingStateStore::new(
        storage,
        ProfileId::new(),
        instance,
        Arc::new(InMemorySecureStore::new()),
    )
    .unwrap()
}

fn db_setting(binding: &SharingBinding) -> String {
    format!(
        "cc.sharing.v1:{}:{}",
        binding.server_instance_id, binding.share_id
    )
}

fn os_marker(profile: ProfileId, binding: &SharingBinding) -> String {
    format!(
        "cc.shw.v1:{profile}:{}:{}",
        binding.server_instance_id, binding.share_id
    )
}

#[derive(Debug, Default)]
struct FaultSecureStore {
    inner: InMemorySecureStore,
    mode: AtomicU8,
    write_occurred: AtomicBool,
}

impl SecureStore for FaultSecureStore {
    fn get(&self, name: &str) -> Result<Option<SecretSlice<u8>>, SecureStoreError> {
        if self.mode.load(Ordering::SeqCst) == 4 && self.write_occurred.load(Ordering::SeqCst) {
            return Err(SecureStoreError::AccessDenied(
                "injected test denial".into(),
            ));
        }
        self.inner.get(name)
    }
    fn set(&self, name: &str, bytes: &[u8]) -> Result<(), SecureStoreError> {
        match self.mode.load(Ordering::SeqCst) {
            1 => Err(SecureStoreError::AccessDenied(
                "injected test denial".into(),
            )),
            2 => Ok(()), // A backend incorrectly acknowledges a dropped write.
            mode => {
                self.inner.set(name, bytes)?;
                self.write_occurred.store(true, Ordering::SeqCst);
                if mode == 3 {
                    Err(SecureStoreError::Backend("injected ambiguous write".into()))
                } else {
                    Ok(())
                }
            }
        }
    }
    fn delete(&self, name: &str) -> Result<bool, SecureStoreError> {
        self.inner.delete(name)
    }
}

fn binding() -> SharingBinding {
    SharingBinding {
        server_instance_id: Uuid::new_v4(),
        share_id: ShareId::new(),
        item_id: ObjectId::new(),
        kind: SharedItemKind::Host,
    }
}

fn projection(name: &str) -> SharedProjection {
    SharedProjection::Host(SharedHostProjection {
        name: name.into(),
        address: "example.test".into(),
        port: 22,
        username: None,
        keepalive_secs: None,
        tags: vec![],
        notes: None,
    })
}

impl Fixture {
    fn new() -> Self {
        Self {
            owner: DeviceSecretKeys::generate().unwrap(),
            owner_id: DeviceId::new(),
            owner_user: UserId::new(),
            reader: DeviceSecretKeys::generate().unwrap(),
            reader_id: DeviceId::new(),
            reader_user: UserId::new(),
        }
    }

    fn owner_anchor(&self) -> SharingOwnerAnchor {
        SharingOwnerAnchor {
            user_id: self.owner_user,
            device_id: self.owner_id,
            public_keys: self.owner.public_keys(),
        }
    }

    fn genesis(&self, binding: &SharingBinding, plaintext: &[u8]) -> SharedItemState {
        let member = |keys: &DeviceSecretKeys, user_id, device_id, role| SharingMember {
            user_id,
            device_id,
            encryption_public_key: keys.public_keys().encryption_bytes(),
            signing_public_key: keys.public_keys().signing_bytes(),
            role,
        };
        let manifest = AccessManifest {
            format: 1,
            server_instance_id: binding.server_instance_id,
            share_id: binding.share_id,
            item_id: binding.item_id,
            owner_user_id: self.owner_user,
            owner_device_id: self.owner_id,
            revision: 1,
            access_epoch: 1,
            previous_manifest_hash: Bytes::from([0; 32]),
            kind: binding.kind,
            members: vec![
                member(
                    &self.owner,
                    self.owner_user,
                    self.owner_id,
                    SharingRole::Editor,
                ),
                member(
                    &self.reader,
                    self.reader_user,
                    self.reader_id,
                    SharingRole::Reader,
                ),
            ],
        };
        let context = SharingContext {
            server_instance_id: binding.server_instance_id,
            share_id: binding.share_id,
            item_id: binding.item_id,
            revision: 1,
            access_epoch: 1,
            kind: binding.kind,
        };
        let access =
            sign_shared_manifest(&self.owner, manifest, &context, &self.owner_anchor(), None)
                .unwrap();
        self.revision(access, None, Some(plaintext))
    }

    fn revision(
        &self,
        access: SignedAccessManifest,
        previous: Option<&SharedItemState>,
        plaintext: Option<&[u8]>,
    ) -> SharedItemState {
        let a = &access.manifest;
        let context = SharingContext {
            server_instance_id: a.server_instance_id,
            share_id: a.share_id,
            item_id: a.item_id,
            revision: previous.map_or(1, |p| p.revision.signed.mutation.context.revision + 1),
            access_epoch: a.access_epoch,
            kind: a.kind,
        };
        let manifest =
            verify_shared_manifest(&access, &context, &self.owner_anchor(), None).unwrap();
        let checkpoint = previous.map(|p| {
            let previous_manifest = verify_shared_manifest(
                &p.access,
                &p.revision.signed.mutation.context,
                &self.owner_anchor(),
                None,
            )
            .unwrap();
            verify_shared_mutation(&previous_manifest, &p.revision.signed, None)
                .unwrap()
                .checkpoint()
        });
        let body = plaintext.map(|p| seal_shared_revision(&manifest, &context, p).unwrap());
        let mutation = SharingMutation {
            base_revision: context.revision - 1,
            context,
            mutation_id: MutationId::new(),
            manifest_revision: a.revision,
            manifest_hash: Bytes::from(manifest.hash()),
            writer_device_id: self.owner_id,
            previous_revision_hash: Bytes::from(checkpoint.map_or([0; 32], |p| p.hash)),
            operation: if body.is_some() {
                SharingOperation::Put
            } else {
                SharingOperation::Delete
            },
            body_hash: Bytes::from(
                body.as_ref()
                    .map_or([0; 32], |b| shared_body_hash(b).unwrap()),
            ),
        };
        let signed = sign_shared_mutation(
            &manifest,
            &self.owner,
            mutation,
            body.as_ref(),
            checkpoint.as_ref(),
        )
        .unwrap();
        SharedItemState {
            access,
            revision: SharedRevision { signed, body },
        }
    }

    fn put(&self, previous: &SharedItemState, plaintext: &[u8]) -> SharedItemState {
        self.revision(previous.access.clone(), Some(previous), Some(plaintext))
    }

    fn rotate(&self, previous: &SharedItemState, plaintext: &[u8]) -> SharedItemState {
        let verified = verify_shared_manifest(
            &previous.access,
            &previous.revision.signed.mutation.context,
            &self.owner_anchor(),
            None,
        )
        .unwrap();
        let mut manifest = previous.access.manifest.clone();
        manifest.revision += 1;
        manifest.access_epoch += 1;
        manifest.previous_manifest_hash = Bytes::from(verified.hash());
        let mut context = previous.revision.signed.mutation.context.clone();
        context.revision += 1;
        context.access_epoch += 1;
        let access = sign_shared_manifest(
            &self.owner,
            manifest,
            &context,
            &self.owner_anchor(),
            Some(&verified.checkpoint()),
        )
        .unwrap();
        self.revision(access, Some(previous), Some(plaintext))
    }
}

#[tokio::test]
async fn sqlcipher_restart_preserves_owner_pin_and_both_authenticated_checkpoints() {
    let f = Fixture::new();
    let binding = binding();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("profile.db");
    let key = database_key();
    let marker = format!("selected-host-{}", ObjectId::new());
    let projection = projection(&marker);
    let encoded = projection.encode().unwrap();
    let first = f.genesis(&binding, encoded.as_slice());
    let profile = ProfileId::new();
    let secure: Arc<dyn SecureStore> = Arc::new(InMemorySecureStore::new());
    let storage = Storage::open(&path, key.clone()).await.unwrap();
    let store = SharingStateStore::new(
        storage.clone(),
        profile,
        binding.server_instance_id,
        secure.clone(),
    )
    .unwrap();
    let accepted = store
        .pin_share(
            binding.clone(),
            f.owner_anchor(),
            first.clone(),
            f.reader_id,
            &f.reader,
        )
        .await
        .unwrap();
    assert_eq!(accepted.projection.as_ref(), Some(&projection));
    let second = f.rotate(&first, encoded.as_slice());
    let accepted = store
        .apply(
            &accepted.snapshot,
            SharingHistory::default(),
            second,
            f.reader_id,
            &f.reader,
        )
        .await
        .unwrap();
    let manifest_checkpoint = accepted.snapshot.manifest_checkpoint();
    let revision_checkpoint = accepted.snapshot.revision_checkpoint();
    assert!(!format!("{accepted:?}{store:?}").contains(&marker));
    storage.close().await;
    let raw = std::fs::read(&path).unwrap();
    assert!(!raw.starts_with(b"SQLite format 3"));
    assert!(!raw.windows(marker.len()).any(|w| w == marker.as_bytes()));

    let storage = Storage::open(&path, key).await.unwrap();
    let store = SharingStateStore::new(
        storage.clone(),
        profile,
        binding.server_instance_id,
        secure.clone(),
    )
    .unwrap();
    let cached = store.load(&binding).await.unwrap().unwrap();
    assert_eq!(cached.owner_anchor(), f.owner_anchor());
    assert_eq!(cached.manifest_checkpoint(), manifest_checkpoint);
    assert_eq!(cached.revision_checkpoint(), revision_checkpoint);
    assert_eq!(
        store
            .open_cached(&cached, f.reader_id, &f.reader)
            .await
            .unwrap()
            .as_ref(),
        Some(&projection)
    );
    assert!(matches!(
        store
            .pin_share(binding, f.owner_anchor(), first, f.reader_id, &f.reader,)
            .await,
        Err(SharingStateError::AlreadyPinned)
    ));
    storage.close().await;
}

#[tokio::test]
async fn profiles_and_shares_do_not_reuse_each_others_pins_or_snapshots() {
    let f = Fixture::new();
    let first_binding = binding();
    let mut second_binding = first_binding.clone();
    second_binding.share_id = ShareId::new();
    second_binding.item_id = ObjectId::new();
    let bytes = projection("selected").encode().unwrap();
    let a = Storage::open_in_memory(database_key()).await.unwrap();
    let b = Storage::open_in_memory(database_key()).await.unwrap();
    let first_store = new_store(a.clone(), first_binding.server_instance_id);
    let second_store = new_store(b.clone(), first_binding.server_instance_id);
    let first = f.genesis(&first_binding, bytes.as_slice());
    let accepted = first_store
        .pin_share(
            first_binding.clone(),
            f.owner_anchor(),
            first.clone(),
            f.reader_id,
            &f.reader,
        )
        .await
        .unwrap();
    assert!(first_store.load(&second_binding).await.unwrap().is_none());
    assert!(second_store.load(&first_binding).await.unwrap().is_none());
    assert!(matches!(
        second_store
            .apply(
                &accepted.snapshot,
                SharingHistory::default(),
                f.put(&first, bytes.as_slice()),
                f.reader_id,
                &f.reader,
            )
            .await,
        Err(SharingStateError::WrongStore)
    ));
    first_store
        .pin_share(
            second_binding.clone(),
            f.owner_anchor(),
            f.genesis(&second_binding, bytes.as_slice()),
            f.reader_id,
            &f.reader,
        )
        .await
        .unwrap();
    assert_ne!(
        first_store
            .load(&first_binding)
            .await
            .unwrap()
            .unwrap()
            .binding(),
        first_store
            .load(&second_binding)
            .await
            .unwrap()
            .unwrap()
            .binding()
    );
    a.close().await;
    b.close().await;
}

#[tokio::test]
async fn owner_instance_item_and_kind_binding_fail_before_establishing_a_pin() {
    let f = Fixture::new();
    let binding = binding();
    let bytes = projection("selected").encode().unwrap();
    let first = f.genesis(&binding, bytes.as_slice());
    let storage = Storage::open_in_memory(database_key()).await.unwrap();
    let store = new_store(storage.clone(), binding.server_instance_id);
    let mut wrong_owner = f.owner_anchor();
    wrong_owner.public_keys = DeviceSecretKeys::generate().unwrap().public_keys();
    assert!(matches!(
        store
            .pin_share(
                binding.clone(),
                wrong_owner,
                first.clone(),
                f.reader_id,
                &f.reader,
            )
            .await,
        Err(SharingStateError::Crypto(SharingCryptoError::OwnerMismatch))
    ));
    for mode in 0..4 {
        let mut wrong = binding.clone();
        match mode {
            0 => wrong.server_instance_id = Uuid::new_v4(),
            1 => wrong.item_id = ObjectId::new(),
            2 => wrong.share_id = ShareId::new(),
            _ => wrong.kind = SharedItemKind::Snippet,
        }
        assert!(matches!(
            store
                .pin_share(
                    wrong,
                    f.owner_anchor(),
                    first.clone(),
                    f.reader_id,
                    &f.reader,
                )
                .await,
            Err(SharingStateError::BindingMismatch)
        ));
    }
    assert!(store.load(&binding).await.unwrap().is_none());
    storage.close().await;
}

#[tokio::test]
async fn complete_gap_free_history_bridges_offline_rotations_and_detects_rollback() {
    let f = Fixture::new();
    let binding = binding();
    let bytes = projection("selected").encode().unwrap();
    let first = f.genesis(&binding, bytes.as_slice());
    let second = f.put(&first, bytes.as_slice());
    let third = f.rotate(&second, bytes.as_slice());
    let fourth = f.put(&third, bytes.as_slice());
    let fifth = f.rotate(&fourth, bytes.as_slice());
    let storage = Storage::open_in_memory(database_key()).await.unwrap();
    let store = new_store(storage.clone(), binding.server_instance_id);
    let accepted = store
        .pin_share(
            binding.clone(),
            f.owner_anchor(),
            first.clone(),
            f.reader_id,
            &f.reader,
        )
        .await
        .unwrap();
    let history = SharingHistory {
        manifests: vec![third.access.clone(), fifth.access.clone()],
        revisions: vec![
            second.revision.signed.clone(),
            third.revision.signed.clone(),
            fourth.revision.signed.clone(),
            fifth.revision.signed.clone(),
        ],
    };
    for omit_manifest in [false, true] {
        let mut incomplete = history.clone();
        if omit_manifest {
            incomplete.manifests.remove(0);
        } else {
            incomplete.revisions.remove(0);
        }
        assert!(store
            .apply(
                &accepted.snapshot,
                incomplete,
                fifth.clone(),
                f.reader_id,
                &f.reader,
            )
            .await
            .is_err());
        assert_eq!(
            store
                .load(&binding)
                .await
                .unwrap()
                .unwrap()
                .revision_checkpoint()
                .revision,
            1
        );
    }
    let accepted = store
        .apply(&accepted.snapshot, history, fifth, f.reader_id, &f.reader)
        .await
        .unwrap();
    assert_eq!(accepted.snapshot.manifest_checkpoint().revision, 3);
    assert_eq!(accepted.snapshot.revision_checkpoint().revision, 5);
    assert!(matches!(
        store
            .apply(
                &accepted.snapshot,
                SharingHistory::default(),
                first,
                f.reader_id,
                &f.reader,
            )
            .await,
        Err(SharingStateError::Crypto(SharingCryptoError::Rollback))
    ));
    let duplicate = store
        .apply(
            &accepted.snapshot,
            SharingHistory::default(),
            accepted.snapshot.state().clone(),
            f.reader_id,
            &f.reader,
        )
        .await
        .unwrap();
    assert_eq!(
        duplicate.snapshot.revision_checkpoint(),
        accepted.snapshot.revision_checkpoint()
    );
    storage.close().await;
}

#[tokio::test]
async fn malformed_projection_or_aead_never_advances_trust_state() {
    let f = Fixture::new();
    let binding = binding();
    let bytes = projection("selected").encode().unwrap();
    let first = f.genesis(&binding, bytes.as_slice());
    let storage = Storage::open_in_memory(database_key()).await.unwrap();
    let store = new_store(storage.clone(), binding.server_instance_id);
    let accepted = store
        .pin_share(
            binding.clone(),
            f.owner_anchor(),
            first.clone(),
            f.reader_id,
            &f.reader,
        )
        .await
        .unwrap();
    let invalid = serde_json::json!({"kind":"host","data":{
        "name":"selected","address":"example.test","port":22,"username":null,
        "keepalive_secs":null,"tags":[],"notes":null,"credential_id":ObjectId::new()
    }});
    let invalid = zeroize::Zeroizing::new(serde_json::to_vec(&invalid).unwrap());
    let malformed = f.put(&first, &invalid);
    assert!(matches!(
        store
            .apply(
                &accepted.snapshot,
                SharingHistory::default(),
                malformed,
                f.reader_id,
                &f.reader,
            )
            .await,
        Err(SharingStateError::Projection(_))
    ));
    let mut tampered = f.put(&first, bytes.as_slice());
    tampered.revision.body.as_mut().unwrap().ciphertext.0[0] ^= 1;
    // Re-sign the body hash: exercise AEAD failure after valid authorship,
    // rather than stopping at an unauthenticated ciphertext hash change.
    let body = tampered.revision.body.as_ref().unwrap();
    tampered.revision.signed.mutation.body_hash = Bytes::from(shared_body_hash(body).unwrap());
    let manifest = verify_shared_manifest(
        &tampered.access,
        &tampered.revision.signed.mutation.context,
        &f.owner_anchor(),
        None,
    )
    .unwrap();
    tampered.revision.signed = sign_shared_mutation(
        &manifest,
        &f.owner,
        tampered.revision.signed.mutation.clone(),
        Some(body),
        Some(&accepted.snapshot.revision_checkpoint()),
    )
    .unwrap();
    assert!(matches!(
        store
            .apply(
                &accepted.snapshot,
                SharingHistory::default(),
                tampered,
                f.reader_id,
                &f.reader,
            )
            .await,
        Err(SharingStateError::Crypto(SharingCryptoError::Crypto(
            CryptoError::Decrypt
        )))
    ));
    let cached = store.load(&binding).await.unwrap().unwrap();
    assert_eq!(cached.state(), &first);
    assert_eq!(
        cached.revision_checkpoint(),
        accepted.snapshot.revision_checkpoint()
    );
    storage.close().await;
}

#[tokio::test]
async fn two_sqlcipher_connections_have_exactly_one_compare_and_swap_winner() {
    let f = Fixture::new();
    let binding = binding();
    let bytes = projection("selected").encode().unwrap();
    let first = f.genesis(&binding, bytes.as_slice());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("profile.db");
    let key = database_key();
    let profile = ProfileId::new();
    let secure: Arc<dyn SecureStore> = Arc::new(InMemorySecureStore::new());
    let a = Storage::open(&path, key.clone()).await.unwrap();
    let b = Storage::open(&path, key).await.unwrap();
    let first_store = SharingStateStore::new(
        a.clone(),
        profile,
        binding.server_instance_id,
        secure.clone(),
    )
    .unwrap();
    let second_store = SharingStateStore::new(
        b.clone(),
        profile,
        binding.server_instance_id,
        secure.clone(),
    )
    .unwrap();
    let accepted = first_store
        .pin_share(
            binding.clone(),
            f.owner_anchor(),
            first.clone(),
            f.reader_id,
            &f.reader,
        )
        .await
        .unwrap();
    let other_snapshot = second_store.load(&binding).await.unwrap().unwrap();
    let second = f.put(&first, bytes.as_slice());
    let alternative = f.put(&first, bytes.as_slice());
    let results = tokio::join!(
        first_store.apply(
            &accepted.snapshot,
            SharingHistory::default(),
            second,
            f.reader_id,
            &f.reader
        ),
        second_store.apply(
            &other_snapshot,
            SharingHistory::default(),
            alternative,
            f.reader_id,
            &f.reader
        ),
    );
    assert_ne!(results.0.is_ok(), results.1.is_ok());
    let (winner, loser) = match results {
        (Ok(winner), Err(loser)) | (Err(loser), Ok(winner)) => (winner, loser),
        _ => unreachable!("CAS must have exactly one winner"),
    };
    assert!(matches!(loser, SharingStateError::Conflict));
    let stored = first_store.load(&binding).await.unwrap().unwrap();
    assert_eq!(stored.state(), winner.snapshot.state());
    assert_eq!(stored.revision_checkpoint().revision, 2);
    a.close().await;
    b.close().await;
}

#[tokio::test]
async fn initial_latest_acceptance_is_explicit_and_signed_tombstones_are_final() {
    let f = Fixture::new();
    let binding = binding();
    let bytes = projection("selected").encode().unwrap();
    let first = f.genesis(&binding, bytes.as_slice());
    let second = f.put(&first, bytes.as_slice());
    let third = f.put(&second, bytes.as_slice());
    let storage = Storage::open_in_memory(database_key()).await.unwrap();
    let store = new_store(storage.clone(), binding.server_instance_id);
    let accepted = store
        .pin_share(
            binding.clone(),
            f.owner_anchor(),
            third.clone(),
            f.reader_id,
            &f.reader,
        )
        .await
        .unwrap();
    assert_eq!(accepted.snapshot.revision_checkpoint().revision, 3);
    let tombstone = f.revision(third.access.clone(), Some(&third), None);
    let deleted = store
        .apply(
            &accepted.snapshot,
            SharingHistory::default(),
            tombstone.clone(),
            f.reader_id,
            &f.reader,
        )
        .await
        .unwrap();
    assert!(deleted.projection.is_none());
    assert!(matches!(
        store
            .apply(
                &deleted.snapshot,
                SharingHistory::default(),
                f.put(&tombstone, bytes.as_slice()),
                f.reader_id,
                &f.reader,
            )
            .await,
        Err(SharingStateError::Deleted)
    ));
    storage.close().await;
}

#[tokio::test]
async fn restored_whole_sqlcipher_database_is_blocked_until_exact_gap_free_reconciliation() {
    let f = Fixture::new();
    let binding = binding();
    let profile = ProfileId::new();
    let secure: Arc<dyn SecureStore> = Arc::new(InMemorySecureStore::new());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("profile.db");
    let backup = dir.path().join("old-profile.db");
    let key = database_key();
    let bytes = projection("runtime-generated-test-content")
        .encode()
        .unwrap();
    let first = f.genesis(&binding, bytes.as_slice());
    let storage = Storage::open(&path, key.clone()).await.unwrap();
    let store = SharingStateStore::new(
        storage.clone(),
        profile,
        binding.server_instance_id,
        secure.clone(),
    )
    .unwrap();
    store
        .pin_share(
            binding.clone(),
            f.owner_anchor(),
            first.clone(),
            f.reader_id,
            &f.reader,
        )
        .await
        .unwrap();
    storage.close().await;
    std::fs::copy(&path, &backup).unwrap();

    let storage = Storage::open(&path, key.clone()).await.unwrap();
    let store = SharingStateStore::new(
        storage.clone(),
        profile,
        binding.server_instance_id,
        secure.clone(),
    )
    .unwrap();
    let base = store.load(&binding).await.unwrap().unwrap();
    let second = f.put(&first, bytes.as_slice());
    let third = f.rotate(&second, bytes.as_slice());
    let latest = store
        .apply(
            &base,
            SharingHistory {
                manifests: vec![],
                revisions: vec![second.revision.signed.clone()],
            },
            third.clone(),
            f.reader_id,
            &f.reader,
        )
        .await
        .unwrap();
    let mark = secure.get(&os_marker(profile, &binding)).unwrap().unwrap();
    assert!(mark.expose_secret().len() <= MAX_SECRET_LEN);
    assert!(
        !String::from_utf8_lossy(mark.expose_secret()).contains("runtime-generated-test-content")
    );
    storage.close().await;
    std::fs::copy(&backup, &path).unwrap();

    let storage = Storage::open(&path, key).await.unwrap();
    let store = SharingStateStore::new(
        storage.clone(),
        profile,
        binding.server_instance_id,
        secure.clone(),
    )
    .unwrap();
    assert!(matches!(
        store.load(&binding).await,
        Err(SharingStateError::ReconciliationRequired)
    ));
    let base = store.reconciliation_base(&binding).await.unwrap();
    assert_eq!(base.revision_checkpoint().revision, 1);
    assert!(matches!(
        store.open_cached(&base, f.reader_id, &f.reader).await,
        Err(SharingStateError::ReconciliationRequired)
    ));
    assert!(store
        .reconcile_history(
            &base,
            SharingHistory::default(),
            third.clone(),
            f.reader_id,
            &f.reader
        )
        .await
        .is_err());
    assert!(matches!(
        store
            .reconcile_history(
                &base,
                SharingHistory::default(),
                second.clone(),
                f.reader_id,
                &f.reader
            )
            .await,
        Err(SharingStateError::ReconciliationRequired)
    ));
    assert!(matches!(
        store.load(&binding).await,
        Err(SharingStateError::ReconciliationRequired)
    ));
    let recovered = store
        .reconcile_history(
            &base,
            SharingHistory {
                manifests: vec![],
                revisions: vec![second.revision.signed.clone()],
            },
            third,
            f.reader_id,
            &f.reader,
        )
        .await
        .unwrap();
    assert_eq!(
        recovered.snapshot.revision_checkpoint(),
        latest.snapshot.revision_checkpoint()
    );
    assert_eq!(
        secure
            .get(&os_marker(profile, &binding))
            .unwrap()
            .unwrap()
            .expose_secret(),
        mark.expose_secret()
    );
    assert_eq!(
        store
            .load(&binding)
            .await
            .unwrap()
            .unwrap()
            .revision_checkpoint()
            .revision,
        3
    );
    storage.close().await;
}

#[tokio::test]
async fn lost_os_marker_blocks_existing_database_and_wrong_profile_never_repins() {
    let f = Fixture::new();
    let binding = binding();
    let profile = ProfileId::new();
    let secure: Arc<dyn SecureStore> = Arc::new(InMemorySecureStore::new());
    let storage = Storage::open_in_memory(database_key()).await.unwrap();
    let store = SharingStateStore::new(
        storage.clone(),
        profile,
        binding.server_instance_id,
        secure.clone(),
    )
    .unwrap();
    let bytes = projection("selected").encode().unwrap();
    let first = f.genesis(&binding, bytes.as_slice());
    store
        .pin_share(
            binding.clone(),
            f.owner_anchor(),
            first.clone(),
            f.reader_id,
            &f.reader,
        )
        .await
        .unwrap();
    let wrong_profile = SharingStateStore::new(
        storage.clone(),
        ProfileId::new(),
        binding.server_instance_id,
        secure.clone(),
    )
    .unwrap();
    assert!(matches!(
        wrong_profile.load(&binding).await,
        Err(SharingStateError::ReconciliationRequired)
    ));
    assert!(secure.delete(&os_marker(profile, &binding)).unwrap());
    assert!(matches!(
        store.load(&binding).await,
        Err(SharingStateError::ReconciliationRequired)
    ));
    assert!(matches!(
        store.reconciliation_base(&binding).await,
        Err(SharingStateError::ReconciliationRequired)
    ));
    assert!(matches!(
        store
            .pin_share(
                binding.clone(),
                f.owner_anchor(),
                first.clone(),
                f.reader_id,
                &f.reader
            )
            .await,
        Err(SharingStateError::AlreadyPinned)
    ));
    assert!(matches!(
        store
            .reconcile_missing_record(binding, f.owner_anchor(), first, f.reader_id, &f.reader)
            .await,
        Err(SharingStateError::AlreadyPinned)
    ));
    assert!(SharingStateStore::new(
        storage.clone(),
        ProfileId(Uuid::nil()),
        Uuid::new_v4(),
        secure
    )
    .is_err());
    storage.close().await;
}

#[tokio::test]
async fn missing_database_record_requires_explicit_exact_checkpoint_and_owner_recovery() {
    let f = Fixture::new();
    let binding = binding();
    let profile = ProfileId::new();
    let secure: Arc<dyn SecureStore> = Arc::new(InMemorySecureStore::new());
    let storage = Storage::open_in_memory(database_key()).await.unwrap();
    let store = SharingStateStore::new(
        storage.clone(),
        profile,
        binding.server_instance_id,
        secure.clone(),
    )
    .unwrap();
    let bytes = projection("selected").encode().unwrap();
    let first = f.genesis(&binding, bytes.as_slice());
    let accepted = store
        .pin_share(
            binding.clone(),
            f.owner_anchor(),
            first.clone(),
            f.reader_id,
            &f.reader,
        )
        .await
        .unwrap();
    let second = f.put(&first, bytes.as_slice());
    store
        .apply(
            &accepted.snapshot,
            SharingHistory::default(),
            second.clone(),
            f.reader_id,
            &f.reader,
        )
        .await
        .unwrap();
    let key = db_setting(&binding);
    storage
        .write(move |tx| tx.setting_delete(&key))
        .await
        .unwrap();
    assert!(matches!(
        store.load(&binding).await,
        Err(SharingStateError::ReconciliationRequired)
    ));
    assert!(matches!(
        store
            .pin_share(
                binding.clone(),
                f.owner_anchor(),
                second.clone(),
                f.reader_id,
                &f.reader
            )
            .await,
        Err(SharingStateError::ReconciliationRequired)
    ));
    assert!(matches!(
        store
            .reconcile_missing_record(
                binding.clone(),
                f.owner_anchor(),
                first,
                f.reader_id,
                &f.reader
            )
            .await,
        Err(SharingStateError::ReconciliationRequired)
    ));
    assert!(store
        .reconcile_missing_record(
            binding.clone(),
            Fixture::new().owner_anchor(),
            second.clone(),
            f.reader_id,
            &f.reader
        )
        .await
        .is_err());
    let recovered = store
        .reconcile_missing_record(
            binding.clone(),
            f.owner_anchor(),
            second,
            f.reader_id,
            &f.reader,
        )
        .await
        .unwrap();
    assert_eq!(recovered.snapshot.revision_checkpoint().revision, 2);
    assert!(store.load(&binding).await.unwrap().is_some());
    storage.close().await;
}

#[tokio::test]
async fn secure_store_denial_and_dropped_acknowledged_write_return_no_projection_or_db_advance() {
    for mode in [1, 2] {
        let f = Fixture::new();
        let binding = binding();
        let profile = ProfileId::new();
        let secure = Arc::new(FaultSecureStore::default());
        let storage = Storage::open_in_memory(database_key()).await.unwrap();
        let store = SharingStateStore::new(
            storage.clone(),
            profile,
            binding.server_instance_id,
            secure.clone(),
        )
        .unwrap();
        let bytes = projection("selected").encode().unwrap();
        let first = f.genesis(&binding, bytes.as_slice());
        let accepted = store
            .pin_share(
                binding.clone(),
                f.owner_anchor(),
                first.clone(),
                f.reader_id,
                &f.reader,
            )
            .await
            .unwrap();
        let mark = secure.get(&os_marker(profile, &binding)).unwrap().unwrap();
        secure.mode.store(mode, Ordering::SeqCst);
        assert!(matches!(
            store
                .apply(
                    &accepted.snapshot,
                    SharingHistory::default(),
                    f.put(&first, bytes.as_slice()),
                    f.reader_id,
                    &f.reader
                )
                .await,
            Err(SharingStateError::Highwater(_))
        ));
        secure.mode.store(0, Ordering::SeqCst);
        assert_eq!(
            store
                .load(&binding)
                .await
                .unwrap()
                .unwrap()
                .revision_checkpoint()
                .revision,
            1
        );
        assert_eq!(
            secure
                .get(&os_marker(profile, &binding))
                .unwrap()
                .unwrap()
                .expose_secret(),
            mark.expose_secret()
        );
        storage.close().await;
    }
}

#[tokio::test]
async fn marker_saved_before_db_failure_requires_explicit_recovery_and_never_lowers_mark() {
    for mode in [3, 4] {
        let f = Fixture::new();
        let binding = binding();
        let profile = ProfileId::new();
        let secure = Arc::new(FaultSecureStore::default());
        let storage = Storage::open_in_memory(database_key()).await.unwrap();
        let store = SharingStateStore::new(
            storage.clone(),
            profile,
            binding.server_instance_id,
            secure.clone(),
        )
        .unwrap();
        let bytes = projection("selected").encode().unwrap();
        let first = f.genesis(&binding, bytes.as_slice());
        let accepted = store
            .pin_share(
                binding.clone(),
                f.owner_anchor(),
                first.clone(),
                f.reader_id,
                &f.reader,
            )
            .await
            .unwrap();
        let second = f.put(&first, bytes.as_slice());
        secure.write_occurred.store(false, Ordering::SeqCst);
        secure.mode.store(mode, Ordering::SeqCst);
        assert!(matches!(
            store
                .apply(
                    &accepted.snapshot,
                    SharingHistory::default(),
                    second.clone(),
                    f.reader_id,
                    &f.reader
                )
                .await,
            Err(SharingStateError::Highwater(_))
        ));
        secure.mode.store(0, Ordering::SeqCst);
        assert!(matches!(
            store.load(&binding).await,
            Err(SharingStateError::ReconciliationRequired)
        ));
        assert!(matches!(
            store
                .open_cached(&accepted.snapshot, f.reader_id, &f.reader)
                .await,
            Err(SharingStateError::ReconciliationRequired)
        ));
        let base = store.reconciliation_base(&binding).await.unwrap();
        let recovered = store
            .reconcile_history(
                &base,
                SharingHistory::default(),
                second,
                f.reader_id,
                &f.reader,
            )
            .await
            .unwrap();
        assert_eq!(recovered.snapshot.revision_checkpoint().revision, 2);
        assert_eq!(
            store
                .load(&binding)
                .await
                .unwrap()
                .unwrap()
                .revision_checkpoint()
                .revision,
            2
        );
        storage.close().await;
    }
}

#[tokio::test]
async fn independent_database_copies_share_one_monotonic_os_marker() {
    let f = Fixture::new();
    let binding = binding();
    let profile = ProfileId::new();
    let secure: Arc<dyn SecureStore> = Arc::new(InMemorySecureStore::new());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("original.db");
    let alias = dir.path().join("restored-copy.db");
    let key = database_key();
    let bytes = projection("selected").encode().unwrap();
    let first = f.genesis(&binding, bytes.as_slice());
    let storage = Storage::open(&path, key.clone()).await.unwrap();
    let store = SharingStateStore::new(
        storage.clone(),
        profile,
        binding.server_instance_id,
        secure.clone(),
    )
    .unwrap();
    store
        .pin_share(
            binding.clone(),
            f.owner_anchor(),
            first.clone(),
            f.reader_id,
            &f.reader,
        )
        .await
        .unwrap();
    storage.close().await;
    std::fs::copy(&path, &alias).unwrap();
    let a = Storage::open(&path, key.clone()).await.unwrap();
    let b = Storage::open(&alias, key).await.unwrap();
    let one = SharingStateStore::new(
        a.clone(),
        profile,
        binding.server_instance_id,
        secure.clone(),
    )
    .unwrap();
    let two = SharingStateStore::new(
        b.clone(),
        profile,
        binding.server_instance_id,
        secure.clone(),
    )
    .unwrap();
    let sa = one.load(&binding).await.unwrap().unwrap();
    let sb = two.load(&binding).await.unwrap().unwrap();
    let next_a = f.put(&first, bytes.as_slice());
    let next_b = f.put(&first, bytes.as_slice());
    let (ra, rb) = tokio::join!(
        one.apply(
            &sa,
            SharingHistory::default(),
            next_a,
            f.reader_id,
            &f.reader
        ),
        two.apply(
            &sb,
            SharingHistory::default(),
            next_b,
            f.reader_id,
            &f.reader
        )
    );
    assert_ne!(ra.is_ok(), rb.is_ok());
    let (winner_store, loser_store, winner, loser) = match (ra, rb) {
        (Ok(w), Err(e)) => (&one, &two, w, e),
        (Err(e), Ok(w)) => (&two, &one, w, e),
        _ => unreachable!("a single OS marker must have one winner"),
    };
    assert!(matches!(loser, SharingStateError::ReconciliationRequired));
    assert!(matches!(
        loser_store.load(&binding).await,
        Err(SharingStateError::ReconciliationRequired)
    ));
    assert_eq!(
        winner_store
            .load(&binding)
            .await
            .unwrap()
            .unwrap()
            .revision_checkpoint(),
        winner.snapshot.revision_checkpoint()
    );
    a.close().await;
    b.close().await;
}

#[tokio::test]
async fn ambiguous_first_pin_cannot_be_reset_and_recovers_only_exact_observed_state() {
    let f = Fixture::new();
    let binding = binding();
    let profile = ProfileId::new();
    let secure = Arc::new(FaultSecureStore::default());
    let storage = Storage::open_in_memory(database_key()).await.unwrap();
    let store = SharingStateStore::new(
        storage.clone(),
        profile,
        binding.server_instance_id,
        secure.clone(),
    )
    .unwrap();
    let bytes = projection("selected").encode().unwrap();
    let first = f.genesis(&binding, bytes.as_slice());
    secure.mode.store(3, Ordering::SeqCst);
    assert!(matches!(
        store
            .pin_share(
                binding.clone(),
                f.owner_anchor(),
                first.clone(),
                f.reader_id,
                &f.reader
            )
            .await,
        Err(SharingStateError::Highwater(_))
    ));
    secure.mode.store(0, Ordering::SeqCst);
    assert!(matches!(
        store.load(&binding).await,
        Err(SharingStateError::ReconciliationRequired)
    ));
    assert!(matches!(
        store
            .pin_share(
                binding.clone(),
                f.owner_anchor(),
                first.clone(),
                f.reader_id,
                &f.reader
            )
            .await,
        Err(SharingStateError::ReconciliationRequired)
    ));
    let fork = f.genesis(&binding, bytes.as_slice());
    assert!(matches!(
        store
            .reconcile_missing_record(
                binding.clone(),
                f.owner_anchor(),
                fork,
                f.reader_id,
                &f.reader
            )
            .await,
        Err(SharingStateError::ReconciliationRequired)
    ));
    let recovered = store
        .reconcile_missing_record(
            binding.clone(),
            f.owner_anchor(),
            first,
            f.reader_id,
            &f.reader,
        )
        .await
        .unwrap();
    assert_eq!(recovered.snapshot.revision_checkpoint().revision, 1);
    assert!(store.load(&binding).await.unwrap().is_some());
    storage.close().await;
}

#[tokio::test]
async fn recovery_can_cross_preserved_marker_to_newer_server_state_but_rejects_a_signed_fork() {
    use cc_crypto_core::sharing::{
        SharedPlaintext, SharingRevisionOpener, VerifiedSharingManifest, VerifiedSharingMutation,
    };
    use cc_protocol::sharing::SharedEncryptedBody;
    use std::sync::atomic::AtomicUsize;
    struct CountingOpener<'a> {
        keys: &'a DeviceSecretKeys,
        calls: AtomicUsize,
    }
    impl SharingRevisionOpener for CountingOpener<'_> {
        fn open_shared_revision(
            &self,
            manifest: &VerifiedSharingManifest,
            revision: &VerifiedSharingMutation,
            body: Option<&SharedEncryptedBody>,
            device: DeviceId,
        ) -> Result<Option<SharedPlaintext>, SharingCryptoError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.keys
                .open_shared_revision(manifest, revision, body, device)
        }
    }
    let f = Fixture::new();
    let binding = binding();
    let profile = ProfileId::new();
    let secure: Arc<dyn SecureStore> = Arc::new(InMemorySecureStore::new());
    let storage = Storage::open_in_memory(database_key()).await.unwrap();
    let store = SharingStateStore::new(
        storage.clone(),
        profile,
        binding.server_instance_id,
        secure.clone(),
    )
    .unwrap();
    let bytes = projection("selected").encode().unwrap();
    let first = f.genesis(&binding, bytes.as_slice());
    let accepted = store
        .pin_share(
            binding.clone(),
            f.owner_anchor(),
            first.clone(),
            f.reader_id,
            &f.reader,
        )
        .await
        .unwrap();
    let raw: String = storage
        .setting_get(db_setting(&binding))
        .await
        .unwrap()
        .unwrap();
    let second = f.put(&first, bytes.as_slice());
    let third = f.rotate(&second, bytes.as_slice());
    store
        .apply(
            &accepted.snapshot,
            SharingHistory {
                manifests: vec![],
                revisions: vec![second.revision.signed.clone()],
            },
            third.clone(),
            f.reader_id,
            &f.reader,
        )
        .await
        .unwrap();
    let before = secure.get(&os_marker(profile, &binding)).unwrap().unwrap();
    let key = db_setting(&binding);
    storage
        .write(move |tx| tx.setting_set(&key, &raw))
        .await
        .unwrap();
    let base = store.reconciliation_base(&binding).await.unwrap();
    let opener = CountingOpener {
        keys: &f.reader,
        calls: AtomicUsize::new(0),
    };

    // A correctly signed alternate owner chain has the same counters, but
    // never visits the exact previously observed object/manifest hashes.
    let fork_third = f.rotate(&second, bytes.as_slice());
    let fork_fourth = f.put(&fork_third, bytes.as_slice());
    assert!(matches!(
        store
            .reconcile_history(
                &base,
                SharingHistory {
                    manifests: vec![],
                    revisions: vec![second.revision.signed.clone(), fork_third.revision.signed]
                },
                fork_fourth,
                f.reader_id,
                &opener
            )
            .await,
        Err(SharingStateError::ReconciliationRequired)
    ));
    assert_eq!(opener.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        secure
            .get(&os_marker(profile, &binding))
            .unwrap()
            .unwrap()
            .expose_secret(),
        before.expose_secret()
    );

    // Server advances beyond this device's mark. Its header history bridges
    // through that exact mark; no historical ciphertext body is required.
    let fourth = f.put(&third, bytes.as_slice());
    let recovered = store
        .reconcile_history(
            &base,
            SharingHistory {
                manifests: vec![],
                revisions: vec![second.revision.signed, third.revision.signed],
            },
            fourth,
            f.reader_id,
            &opener,
        )
        .await
        .unwrap();
    assert_eq!(opener.calls.load(Ordering::SeqCst), 1);
    assert_eq!(recovered.snapshot.revision_checkpoint().revision, 4);
    assert_eq!(
        store
            .load(&binding)
            .await
            .unwrap()
            .unwrap()
            .revision_checkpoint()
            .revision,
        4
    );
    storage.close().await;
}
