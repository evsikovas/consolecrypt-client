//! Shared helpers for sync-core integration tests (against the mock server).
#![allow(dead_code)]

use cc_models::{KekClass, ObjectPayload};
use cc_protocol::auth::{LoginRequest, RegisterRequest, SecretString};
use cc_protocol::devices::{AttestDeviceRequest, DeviceRegistration};
use cc_protocol::envelopes::{
    EnvelopeAlgorithm, EnvelopeKind, EnvelopeMetadata, KdfAlgorithm, KdfParams, NewEnvelope,
    RecipientType,
};
use cc_protocol::sync::{EncryptedBody, OBJECT_FORMAT_V1};
use cc_protocol::vaults::CreateVaultRequest;
use cc_protocol::version::Platform;
use cc_protocol::{Bytes, DeviceId, ObjectId, VaultId};
use cc_storage_core::{DatabaseKey, Profile, ProfileId, Storage};
use cc_sync_core::mock::MockServer;
use cc_sync_core::*;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const PASSWORD: &str = "correct horse battery staple";

/// Random bytes from the OS RNG via UUIDv4 (test-only).
pub fn random_bytes(n: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(n + 16);
    while v.len() < n {
        v.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    }
    v.truncate(n);
    v
}

// ---- test codec ----------------------------------------------------------------------------

/// Trivial reversible "encryption": JSON XOR a per-class byte, with the
/// object id + revision in the nonce and the vault id + class tag in the
/// wrapped DEK, so mis-bound or foreign ciphertexts fail like AEAD would.
#[derive(Debug)]
pub struct TestCodec {
    pub vault_id: VaultId,
    pub locked: AtomicBool,
}

impl TestCodec {
    pub fn new(vault_id: VaultId) -> Arc<Self> {
        Arc::new(Self {
            vault_id,
            locked: AtomicBool::new(false),
        })
    }

    pub fn set_locked(&self, locked: bool) {
        self.locked.store(locked, Ordering::SeqCst);
    }
}

fn class_tag(c: KekClass) -> u8 {
    match c {
        KekClass::Inventory => 1,
        KekClass::Secrets => 2,
        KekClass::Snippets => 3,
        KekClass::History => 4,
        KekClass::Settings => 5,
    }
}

fn tag_class(t: u8) -> Option<KekClass> {
    KekClass::ALL.into_iter().find(|c| class_tag(*c) == t)
}

fn binding(object_id: ObjectId, revision: i64) -> Vec<u8> {
    let mut n = object_id.as_bytes().to_vec();
    n.extend_from_slice(&revision.to_be_bytes());
    n
}

impl ObjectCodec for TestCodec {
    fn encrypt(
        &self,
        object_id: ObjectId,
        revision: i64,
        payload: &ObjectPayload,
    ) -> Result<EncryptedBody, CodecError> {
        if self.locked.load(Ordering::SeqCst) {
            return Err(CodecError::Locked);
        }
        let tag = class_tag(payload.object.kind().kek_class());
        let json = serde_json::to_vec(payload).map_err(|e| CodecError::Encoding(e.to_string()))?;
        let ciphertext: Vec<u8> = json.iter().map(|b| b ^ (0x5a ^ tag)).collect();
        let mut wrapped = self.vault_id.as_bytes().to_vec();
        wrapped.extend(std::iter::repeat_n(tag, 32));
        Ok(EncryptedBody {
            format: OBJECT_FORMAT_V1,
            ciphertext: Bytes::new(ciphertext),
            nonce: Bytes::new(binding(object_id, revision)),
            wrapped_dek: Bytes::new(wrapped),
            wrapped_dek_nonce: Bytes::new(vec![0; 24]),
        })
    }

    fn decrypt(
        &self,
        object_id: ObjectId,
        revision: i64,
        body: &EncryptedBody,
    ) -> Result<ObjectPayload, CodecError> {
        if self.locked.load(Ordering::SeqCst) {
            return Err(CodecError::Locked);
        }
        if body.nonce.as_slice() != binding(object_id, revision).as_slice()
            || body.wrapped_dek.len() != 48
            || &body.wrapped_dek.as_slice()[..16] != self.vault_id.as_bytes()
        {
            return Err(CodecError::Integrity);
        }
        let tag = body.wrapped_dek.as_slice()[16];
        let class = tag_class(tag).ok_or(CodecError::Integrity)?;
        let json: Vec<u8> = body
            .ciphertext
            .as_slice()
            .iter()
            .map(|b| b ^ (0x5a ^ tag))
            .collect();
        let payload: ObjectPayload =
            serde_json::from_slice(&json).map_err(|_| CodecError::Integrity)?;
        if payload.object.kind().kek_class() != class {
            return Err(CodecError::ClassMismatch);
        }
        Ok(payload)
    }
}

// ---- devices, accounts, vaults ------------------------------------------------------------------

pub struct DeviceKeys {
    pub device_id: DeviceId,
    pub signing: ed25519_dalek::SigningKey,
    pub encryption_public: [u8; 32],
}

impl DeviceKeys {
    pub fn generate() -> Self {
        let seed: [u8; 32] = random_bytes(32).try_into().unwrap();
        Self {
            device_id: DeviceId::new(),
            signing: ed25519_dalek::SigningKey::from_bytes(&seed),
            encryption_public: random_bytes(32).try_into().unwrap(),
        }
    }

    /// Protocol 1.4 login proof (fresh nonce each call).
    pub fn proof(&self) -> cc_protocol::devices::DeviceProof {
        self.proof_at(chrono::Utc::now().timestamp())
    }

    pub fn proof_at(&self, issued_at: i64) -> cc_protocol::devices::DeviceProof {
        use ed25519_dalek::Signer;
        let nonce: [u8; 32] = random_bytes(32).try_into().unwrap();
        let msg = cc_protocol::canonical::device_login_message(self.device_id, issued_at, &nonce);
        cc_protocol::devices::DeviceProof {
            issued_at,
            nonce: Bytes::new(nonce.to_vec()),
            signature: Bytes::new(self.signing.sign(&msg).to_bytes().to_vec()),
        }
    }

    /// Protocol 1.5 request signer with this device's key.
    pub fn signer(&self) -> Arc<TestSigner> {
        Arc::new(TestSigner::new(self.device_id, self.signing.clone()))
    }

    pub fn registration(&self, name: &str) -> DeviceRegistration {
        DeviceRegistration {
            device_id: self.device_id,
            name: name.to_owned(),
            platform: Platform::Cli,
            encryption_public_key: Bytes::new(self.encryption_public.to_vec()),
            signing_public_key: Bytes::new(self.signing.verifying_key().to_bytes().to_vec()),
            client_version: Some("test".into()),
        }
    }
}

// ---- request signing (protocol 1.5) --------------------------------------------------------------

/// How a [`TestSigner`] misbehaves (tamper / replay tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tamper {
    /// Sign this target instead of the real one.
    SignPath(String),
    /// Sign the path without its query string.
    DropQuery,
    /// Sign these body bytes instead of the real ones.
    SignBody(Vec<u8>),
    /// Return the previous proof again (same nonce).
    Replay,
}

/// Ed25519 request signer for tests, built like
/// `cc_vault_core::DeviceIdentity::request_proof`.
pub struct TestSigner {
    pub device_id: DeviceId,
    signing: ed25519_dalek::SigningKey,
    /// The next N proofs are issued 10 minutes in the past.
    pub stale_next: AtomicU32,
    pub tamper: Mutex<Option<Tamper>>,
    pub calls: AtomicUsize,
    last: Mutex<Option<cc_protocol::devices::RequestProof>>,
}

impl TestSigner {
    pub fn new(device_id: DeviceId, signing: ed25519_dalek::SigningKey) -> Self {
        Self {
            device_id,
            signing,
            stale_next: AtomicU32::new(0),
            tamper: Mutex::new(None),
            calls: AtomicUsize::new(0),
            last: Mutex::new(None),
        }
    }

    pub fn set_tamper(&self, t: Option<Tamper>) {
        *self.tamper.lock().unwrap() = t;
    }
}

impl RequestSigner for TestSigner {
    fn request_proof(
        &self,
        method: &str,
        path_and_query: &str,
        body: &[u8],
    ) -> Result<cc_protocol::devices::RequestProof, SignerError> {
        use ed25519_dalek::Signer;
        use sha2::Digest;
        self.calls.fetch_add(1, Ordering::SeqCst);
        let tamper = self.tamper.lock().unwrap().clone();
        if tamper == Some(Tamper::Replay) {
            if let Some(p) = *self.last.lock().unwrap() {
                return Ok(p);
            }
        }
        let (target, body): (String, Vec<u8>) = match tamper {
            Some(Tamper::SignPath(p)) => (p, body.to_vec()),
            Some(Tamper::DropQuery) => (
                path_and_query
                    .split('?')
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
                body.to_vec(),
            ),
            Some(Tamper::SignBody(b)) => (path_and_query.to_owned(), b),
            _ => (path_and_query.to_owned(), body.to_vec()),
        };
        let mut issued_at = chrono::Utc::now().timestamp();
        if self
            .stale_next
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            issued_at -= 600;
        }
        let nonce: [u8; 32] = random_bytes(32).try_into().unwrap();
        let body_hash: [u8; 32] = sha2::Sha256::digest(&body).into();
        let msg = cc_protocol::canonical::request_proof_message(
            self.device_id,
            method,
            &target,
            &body_hash,
            issued_at,
            &nonce,
        );
        let proof = cc_protocol::devices::RequestProof {
            issued_at,
            nonce,
            signature: self.signing.sign(&msg).to_bytes(),
        };
        *self.last.lock().unwrap() = Some(proof);
        Ok(proof)
    }
}

pub fn api_for(server: &MockServer) -> ApiClient {
    let cfg = ApiConfig::new(server.url().as_str(), "0.1.0-test", Platform::Cli).unwrap();
    ApiClient::new(cfg, Arc::new(MemoryTokenStore::new())).unwrap()
}

pub struct Account {
    pub email: String,
    pub user_id: cc_protocol::UserId,
}

pub async fn register(server: &MockServer, email: &str, keys: &DeviceKeys) -> (ApiClient, Account) {
    let api = api_for(server);
    let resp = api
        .register(&RegisterRequest {
            email: email.into(),
            password: SecretString::new(PASSWORD),
            device: keys.registration("device A"),
            device_proof: Some(keys.proof()),
        })
        .await
        .unwrap();
    (
        api,
        Account {
            email: email.into(),
            user_id: resp.user_id,
        },
    )
}

pub async fn login(server: &MockServer, email: &str, keys: &DeviceKeys) -> ApiClient {
    let api = api_for(server);
    api.login(&LoginRequest {
        email: email.into(),
        password: SecretString::new(PASSWORD),
        device: keys.registration("device B"),
        device_proof: Some(keys.proof()),
    })
    .await
    .unwrap();
    api
}

pub fn password_envelope() -> NewEnvelope {
    NewEnvelope {
        recipient_type: RecipientType::Password,
        recipient_id: None,
        kind: EnvelopeKind::VrkV1,
        metadata: EnvelopeMetadata {
            algorithm: EnvelopeAlgorithm::Argon2idXchacha20poly1305V1,
            kdf: Some(KdfParams {
                algorithm: KdfAlgorithm::Argon2id,
                salt: Bytes::new(random_bytes(16)),
                memory_kib: 64 * 1024,
                iterations: 3,
                parallelism: 1,
            }),
            ephemeral_public_key: None,
        },
        ciphertext: Bytes::new(random_bytes(48)),
        nonce: Bytes::new(random_bytes(24)),
    }
}

pub fn recovery_envelope() -> NewEnvelope {
    NewEnvelope {
        recipient_type: RecipientType::Recovery,
        recipient_id: None,
        kind: EnvelopeKind::VrkV1,
        metadata: EnvelopeMetadata {
            algorithm: EnvelopeAlgorithm::HkdfSha256Xchacha20poly1305V1,
            kdf: None,
            ephemeral_public_key: None,
        },
        ciphertext: Bytes::new(random_bytes(48)),
        nonce: Bytes::new(random_bytes(24)),
    }
}

pub fn device_envelope(device_id: DeviceId) -> NewEnvelope {
    NewEnvelope {
        recipient_type: RecipientType::Device,
        recipient_id: Some(device_id.0),
        kind: EnvelopeKind::VrkV1,
        metadata: EnvelopeMetadata {
            algorithm: EnvelopeAlgorithm::X25519HkdfSha256Xchacha20poly1305V1,
            kdf: None,
            ephemeral_public_key: Some(Bytes::new(random_bytes(32))),
        },
        ciphertext: Bytes::new(random_bytes(48)),
        nonce: Bytes::new(random_bytes(24)),
    }
}

pub struct VaultMaterial {
    pub vault_id: VaultId,
    pub vak: Bytes,
}

pub fn create_vault_request(
    vault_id: VaultId,
    vak: &Bytes,
    device_id: DeviceId,
) -> CreateVaultRequest {
    CreateVaultRequest {
        vault_id,
        vault_access_key: vak.clone(),
        password_envelope: password_envelope(),
        recovery_envelope: recovery_envelope(),
        device_envelope: device_envelope(device_id),
    }
}

pub async fn create_vault(api: &ApiClient, device_id: DeviceId) -> VaultMaterial {
    let vault_id = VaultId::new();
    let vak = Bytes::new(random_bytes(32));
    api.create_vault(&create_vault_request(vault_id, &vak, device_id))
        .await
        .unwrap();
    VaultMaterial { vault_id, vak }
}

pub fn attest_request(vault: &VaultMaterial, device_id: DeviceId) -> AttestDeviceRequest {
    AttestDeviceRequest {
        vault_id: vault.vault_id,
        vault_access_key: vault.vak.clone(),
        envelope: device_envelope(device_id),
    }
}

pub async fn attest(api: &ApiClient, vault: &VaultMaterial, device_id: DeviceId) {
    api.attest_device(device_id, &attest_request(vault, device_id))
        .await
        .unwrap();
}

// ---- engines --------------------------------------------------------------------------------------

pub fn fast_config(device_id: DeviceId) -> SyncEngineConfig {
    let mut cfg = SyncEngineConfig::new(device_id);
    cfg.backoff = BackoffConfig {
        initial: Duration::from_millis(20),
        max: Duration::from_millis(200),
        multiplier: 2.0,
    };
    cfg.periodic_interval = None;
    cfg
}

pub struct Client {
    pub api: ApiClient,
    pub keys: DeviceKeys,
    pub storage: Storage,
    pub codec: Arc<TestCodec>,
    pub store: ObjectStore,
    pub engine: SyncEngine,
}

pub async fn synced_storage(server: &MockServer, device_id: DeviceId) -> Storage {
    let storage = Storage::open_in_memory(DatabaseKey::from_bytes(
        random_bytes(32).try_into().unwrap(),
    ))
    .await
    .unwrap();
    storage
        .put_profile(Profile::new_synced(
            ProfileId::new(),
            device_id,
            server.url().as_str(),
            None,
            None,
        ))
        .await
        .unwrap();
    storage
}

pub async fn client(
    server: &MockServer,
    api: ApiClient,
    keys: DeviceKeys,
    vault_id: VaultId,
) -> Client {
    client_with(server, api, keys, vault_id, |c| c).await
}

pub async fn client_with(
    server: &MockServer,
    api: ApiClient,
    keys: DeviceKeys,
    vault_id: VaultId,
    tweak: impl FnOnce(SyncEngineConfig) -> SyncEngineConfig,
) -> Client {
    let storage = synced_storage(server, keys.device_id).await;
    let codec = TestCodec::new(vault_id);
    let store = ObjectStore::new(vault_id, storage.clone(), codec.clone());
    let engine = SyncEngine::new(
        store.clone(),
        api.clone(),
        tweak(fast_config(keys.device_id)),
    )
    .await
    .unwrap();
    Client {
        api,
        keys,
        storage,
        codec,
        store,
        engine,
    }
}

/// Two devices of one account, both trusted for one vault.
pub async fn two_devices(server: &MockServer) -> (Client, Client, VaultMaterial) {
    let ka = DeviceKeys::generate();
    let email = format!("user-{}@example.test", uuid::Uuid::new_v4().simple());
    let (api_a, _acct) = register(server, &email, &ka).await;
    let vault = create_vault(&api_a, ka.device_id).await;
    let kb = DeviceKeys::generate();
    let api_b = login(server, &email, &kb).await;
    attest(&api_b, &vault, kb.device_id).await;
    let a = client(server, api_a, ka, vault.vault_id).await;
    let b = client(server, api_b, kb, vault.vault_id).await;
    (a, b, vault)
}

/// Poll `cond` until true (or panic after `secs`).
pub async fn eventually<F, Fut>(secs: u64, mut cond: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    loop {
        if cond().await {
            return;
        }
        if tokio::time::Instant::now() > deadline {
            panic!("condition not reached within {secs}s");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Run `sync_now` until it succeeds (the first attempt after a partition may
/// hit a stale pooled connection).
pub async fn sync_ok(engine: &SyncEngine) -> SyncReport {
    for _ in 0..20 {
        match engine.sync_now().await {
            Ok(r) => return r,
            Err(e) if e.is_offline() => tokio::time::sleep(Duration::from_millis(30)).await,
            Err(e) => panic!("sync failed: {e}"),
        }
    }
    panic!("sync never succeeded");
}
