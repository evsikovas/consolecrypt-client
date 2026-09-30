//! Facade acceptance over real signed HTTP, real profile SQLCipher and runtime
//! identities. The test server routes personal sync to its existing mock and
//! verifies sharing using registered public keys; it never decrypts a share.

use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::http::Uri;
use axum::response::Response;
use axum::Router;
use base64::Engine as _;
use cc_app_core::enrollment_dto::{
    EnrollmentGrantCreateDto, EnrollmentGrantStatusDto, EnrollmentModeDto,
    EnrollmentRequestStatusDto,
};
use cc_app_core::platform::{
    InMemorySecureStore, SecureStore, SecureStoreError, UnsupportedOsAuthenticator,
};
use cc_app_core::sharing_projection::{
    SharedGroupProjection, SharedHostProjection, SharedProjection, SharedSnippetProjection,
};
use cc_app_core::*;
use cc_crypto_core::sharing::{
    shared_body_hash, verify_shared_manifest, verify_shared_mutation, SharingOwnerAnchor,
    SharingRevisionOpener,
};
use cc_crypto_core::{request_body_sha256, verify_request_proof, DevicePublicKeys};
use cc_protocol::auth::{AuthResponse, RegisterRequest, SecretString, VerifyEmailRequest};
use cc_protocol::devices::{DeviceRegistration, RequestProof};
use cc_protocol::sharing::*;
use cc_protocol::{DeviceId, ErrorCode, ShareId, UserId};
use cc_storage_core::{DatabaseKey, Storage};
use cc_sync_core::mock::{MockServer, MockServerConfig};
use cc_sync_core::{ApiClient, ApiConfig, MemoryTokenStore};
use cc_vault_core::DeviceIdentity;
use secrecy::{ExposeSecret as _, SecretSlice};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::str::FromStr as _;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use uuid::Uuid;
use zeroize::Zeroizing;

struct History {
    state: SharedItemState,
    manifests: Vec<SignedAccessManifest>,
    revisions: Vec<SignedSharingMutation>,
}

struct HttpState {
    upstream: String,
    client: reqwest::Client,
    instance: Mutex<Uuid>,
    enabled: AtomicBool,
    supports_groups: AtomicBool,
    supports_secrets: AtomicBool,
    users: Mutex<HashMap<DeviceId, (UserId, String, DeviceRegistration)>>,
    tokens: Mutex<HashMap<String, (UserId, DeviceId)>>,
    nonces: Mutex<HashSet<(DeviceId, [u8; 32])>>,
    shares: Mutex<BTreeMap<ShareId, History>>,
    posts: AtomicUsize,
    conflict_once: AtomicBool,
    lost_once: AtomicBool,
    pause: Mutex<Option<String>>,
    request_started: Notify,
    resume: Notify,
}

struct Harness {
    _personal: MockServer,
    state: Arc<HttpState>,
    task: tokio::task::JoinHandle<()>,
    url: String,
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Harness {
    async fn new() -> Self {
        let personal = MockServer::start_with(MockServerConfig {
            require_request_proof: true,
            ..Default::default()
        })
        .await;
        let state = Arc::new(HttpState {
            upstream: personal.url().as_str().trim_end_matches('/').into(),
            client: reqwest::Client::new(),
            instance: Mutex::new(Uuid::new_v4()),
            enabled: AtomicBool::new(true),
            supports_groups: AtomicBool::new(false),
            supports_secrets: AtomicBool::new(false),
            users: Mutex::new(HashMap::new()),
            tokens: Mutex::new(HashMap::new()),
            nonces: Mutex::new(HashSet::new()),
            shares: Mutex::new(BTreeMap::new()),
            posts: AtomicUsize::new(0),
            conflict_once: AtomicBool::new(false),
            lost_once: AtomicBool::new(false),
            pause: Mutex::new(None),
            request_started: Notify::new(),
            resume: Notify::new(),
        });
        let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", socket.local_addr().unwrap());
        let router = Router::new().fallback(route).with_state(state.clone());
        let task = tokio::spawn(async move {
            axum::serve(socket, router).await.unwrap();
        });
        Self {
            _personal: personal,
            state,
            task,
            url,
        }
    }
}

fn response(status: u16, value: impl serde::Serialize) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&value).unwrap()))
        .unwrap()
}

fn denied(code: ErrorCode) -> Response {
    response(
        code.http_status(),
        cc_protocol::ApiError::new(code, "test request refused"),
    )
}

async fn route(State(state): State<Arc<HttpState>>, request: Request) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let headers = request.headers().clone();
    let body = to_bytes(request.into_body(), 8 * 1024 * 1024)
        .await
        .unwrap();
    if !uri.path().starts_with("/v1/shares") {
        let registration = if uri.path() == "/v1/auth/register" {
            Some(serde_json::from_slice::<RegisterRequest>(&body).unwrap())
        } else {
            None
        };
        let mut outgoing = state
            .client
            .request(method, format!("{}{}", state.upstream, uri))
            .body(body.to_vec());
        for (key, value) in &headers {
            if !matches!(key.as_str(), "host" | "content-length" | "connection") {
                outgoing = outgoing.header(key, value);
            }
        }
        let upstream = outgoing.send().await.unwrap();
        let status = upstream.status().as_u16();
        let bytes = upstream.bytes().await.unwrap();
        if let Ok(auth) = serde_json::from_slice::<AuthResponse>(&bytes) {
            state.tokens.lock().unwrap().insert(
                auth.tokens.access_token.expose_secret().to_owned(),
                (auth.user_id, auth.device_id),
            );
            if let Some(registration) = registration {
                state.users.lock().unwrap().insert(
                    auth.device_id,
                    (auth.user_id, registration.email, registration.device),
                );
            }
        }
        return Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(Body::from(bytes))
            .unwrap();
    }
    let authorization = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let principal =
        authorization.and_then(|token| state.tokens.lock().unwrap().get(token).copied());
    let Some((user, device)) = principal else {
        return denied(ErrorCode::Unauthorized);
    };
    let registration = state.users.lock().unwrap().get(&device).unwrap().2.clone();
    let proof = headers
        .get(cc_protocol::version::HEADER_DEVICE_PROOF)
        .and_then(|value| value.to_str().ok())
        .and_then(RequestProof::decode);
    let Some(proof) = proof else {
        return denied(ErrorCode::InvalidProof);
    };
    if verify_request_proof(
        &registration.signing_public_key.to_array().unwrap(),
        device,
        method.as_str(),
        &uri.to_string(),
        &request_body_sha256(&body),
        proof.issued_at,
        &proof.nonce,
        &proof.signature,
    )
    .is_err()
        || !state.nonces.lock().unwrap().insert((device, proof.nonce))
    {
        return denied(ErrorCode::InvalidProof);
    }
    let paused = state.pause.lock().unwrap().as_deref() == Some(uri.path());
    if paused {
        state.request_started.notify_one();
        state.resume.notified().await;
    }
    let instance = *state.instance.lock().unwrap();
    if uri.path() == "/v1/shares/capabilities" {
        return response(
            200,
            SharingCapabilities {
                enabled: state.enabled.load(Ordering::SeqCst),
                format: 1,
                server_instance_id: instance,
                max_members: 64,
                max_ciphertext_bytes: cc_protocol::sharing::MAX_CIPHERTEXT_BYTES as u32,
                supports_groups: state.supports_groups.load(Ordering::SeqCst),
                supports_secrets: state.supports_secrets.load(Ordering::SeqCst),
                supports_owner_online_enrollment_v1: false,
            },
        );
    }
    if !state.enabled.load(Ordering::SeqCst) {
        return denied(ErrorCode::NotFound);
    }
    let query: HashMap<String, String> = url_query(&uri);
    if uri.path() == "/v1/shares/recipients" {
        let users = state.users.lock().unwrap();
        let targets: Vec<_> = users
            .iter()
            .filter(|(_, (_, email, _))| query.get("email") == Some(email))
            .collect();
        let Some((_, (target_user, _, _))) = targets.first() else {
            return denied(ErrorCode::NotFound);
        };
        return response(
            200,
            SharingRecipient {
                user_id: *target_user,
                devices: targets
                    .into_iter()
                    .map(|(id, (user, _, keys))| SharingMember {
                        user_id: *user,
                        device_id: *id,
                        encryption_public_key: keys.encryption_public_key.clone(),
                        signing_public_key: keys.signing_public_key.clone(),
                        role: SharingRole::Reader,
                    })
                    .collect(),
            },
        );
    }
    let mut shares = state.shares.lock().unwrap();
    if uri.path() == "/v1/shares" && method == "GET" {
        let after = query.get("after").map(|id| ShareId::from_str(id).unwrap());
        let items: Vec<_> = shares
            .iter()
            .filter(|(id, history)| {
                after.is_none_or(|after| **id > after)
                    && match history.state.access.manifest.kind {
                        SharedItemKind::Host | SharedItemKind::Snippet => true,
                        SharedItemKind::Group => query
                            .get("include_groups")
                            .is_some_and(|value| value == "true"),
                        SharedItemKind::Secret => query
                            .get("include_secrets")
                            .is_some_and(|value| value == "true"),
                    }
                    && history
                        .state
                        .access
                        .manifest
                        .members
                        .iter()
                        .any(|member| member.device_id == device)
            })
            .map(|(_, history)| history.state.clone())
            .collect();
        return response(
            200,
            ShareListPage {
                items,
                next_after: None,
                has_more: false,
            },
        );
    }
    if uri.path() == "/v1/shares" && method == "POST" {
        state.posts.fetch_add(1, Ordering::SeqCst);
        let input: CreateShareRequest = serde_json::from_slice(&body).unwrap();
        let created = SharedItemState {
            access: input.access,
            revision: input.revision,
        };
        let access = &created.access.manifest;
        if access.server_instance_id != instance
            || access.owner_user_id != user
            || access.owner_device_id != device
        {
            return denied(ErrorCode::Forbidden);
        }
        let anchor = SharingOwnerAnchor {
            user_id: user,
            device_id: device,
            public_keys: DevicePublicKeys::from_slices(
                registration.encryption_public_key.as_slice(),
                registration.signing_public_key.as_slice(),
            )
            .unwrap(),
        };
        let context = &created.revision.signed.mutation.context;
        let manifest = verify_shared_manifest(&created.access, context, &anchor, None).unwrap();
        verify_shared_mutation(&manifest, &created.revision.signed, None).unwrap();
        assert!(
            created.revision.signed.mutation.body_hash.as_slice()
                == shared_body_hash(created.revision.body.as_ref().unwrap()).unwrap()
        );
        if shares.contains_key(&access.share_id) {
            return denied(ErrorCode::Conflict);
        }
        shares.insert(
            access.share_id,
            History {
                manifests: vec![created.access.clone()],
                revisions: vec![created.revision.signed.clone()],
                state: created.clone(),
            },
        );
        if state.lost_once.swap(false, Ordering::SeqCst) {
            return denied(ErrorCode::Internal);
        }
        return response(201, created);
    }
    let segments: Vec<_> = uri
        .path()
        .trim_start_matches("/v1/shares/")
        .split('/')
        .collect();
    let Ok(id) = ShareId::from_str(segments[0]) else {
        return denied(ErrorCode::NotFound);
    };
    let Some(history) = shares.get_mut(&id) else {
        return denied(ErrorCode::NotFound);
    };
    let Some(member) = history
        .state
        .access
        .manifest
        .members
        .iter()
        .find(|member| member.device_id == device)
        .cloned()
    else {
        return denied(ErrorCode::NotFound);
    };
    if segments.len() == 1 && method == "GET" {
        return response(200, &history.state);
    }
    if segments.get(1) == Some(&"history") && method == "GET" {
        let after_manifest: u64 = query.get("after_manifest").unwrap().parse().unwrap();
        let after_revision: i64 = query.get("after_revision").unwrap().parse().unwrap();
        return response(
            200,
            ShareHistoryPage {
                manifests: history
                    .manifests
                    .iter()
                    .filter(|signed| signed.manifest.revision > after_manifest)
                    .cloned()
                    .collect(),
                revisions: history
                    .revisions
                    .iter()
                    .filter(|signed| signed.mutation.context.revision > after_revision)
                    .cloned()
                    .collect(),
                latest_manifest_revision: history.state.access.manifest.revision,
                latest_revision: history.state.revision.signed.mutation.context.revision,
                has_more: false,
            },
        );
    }
    if method == "POST" {
        state.posts.fetch_add(1, Ordering::SeqCst);
        if state.conflict_once.swap(false, Ordering::SeqCst) {
            return denied(ErrorCode::Conflict);
        }
        if member.role != SharingRole::Editor {
            return denied(ErrorCode::Forbidden);
        }
        let anchor = owner_anchor(&history.state);
        let previous_manifest = verify_shared_manifest(
            &history.state.access,
            &history.state.revision.signed.mutation.context,
            &anchor,
            None,
        )
        .unwrap();
        let previous_revision =
            verify_shared_mutation(&previous_manifest, &history.state.revision.signed, None)
                .unwrap();
        let changed = if segments.get(1) == Some(&"access") {
            if history.state.access.manifest.owner_device_id != device {
                return denied(ErrorCode::Forbidden);
            }
            let input: RotateShareAccessRequest = serde_json::from_slice(&body).unwrap();
            SharedItemState {
                access: input.access,
                revision: input.revision,
            }
        } else {
            let input: PutSharedRevisionRequest = serde_json::from_slice(&body).unwrap();
            SharedItemState {
                access: history.state.access.clone(),
                revision: input.revision,
            }
        };
        if changed.revision.signed.mutation.base_revision != previous_revision.checkpoint().revision
        {
            return denied(ErrorCode::Conflict);
        }
        let manifest = verify_shared_manifest(
            &changed.access,
            &changed.revision.signed.mutation.context,
            &anchor,
            Some(&previous_manifest.checkpoint()),
        )
        .unwrap();
        verify_shared_mutation(
            &manifest,
            &changed.revision.signed,
            Some(&previous_revision.checkpoint()),
        )
        .unwrap();
        if let Some(body) = &changed.revision.body {
            assert!(
                changed.revision.signed.mutation.body_hash.as_slice()
                    == shared_body_hash(body).unwrap()
            );
        }
        if changed.access != history.state.access {
            history.manifests.push(changed.access.clone());
        }
        history.revisions.push(changed.revision.signed.clone());
        history.state = changed.clone();
        if state.lost_once.swap(false, Ordering::SeqCst) {
            return denied(ErrorCode::Internal);
        }
        return response(200, changed);
    }
    denied(ErrorCode::NotFound)
}

fn url_query(uri: &Uri) -> HashMap<String, String> {
    reqwest::Url::parse(&format!("http://localhost{uri}"))
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect()
}

fn owner_anchor(state: &SharedItemState) -> SharingOwnerAnchor {
    let access = &state.access.manifest;
    let member = access
        .members
        .iter()
        .find(|member| member.device_id == access.owner_device_id)
        .unwrap();
    SharingOwnerAnchor {
        user_id: member.user_id,
        device_id: member.device_id,
        public_keys: DevicePublicKeys::from_slices(
            member.encryption_public_key.as_slice(),
            member.signing_public_key.as_slice(),
        )
        .unwrap(),
    }
}

struct Account {
    app: AppCore,
    store: Arc<dyn SecureStore>,
    directory: PathBuf,
    profile: String,
    email: String,
    account_password: Zeroizing<String>,
    passphrase: String,
}

#[derive(Debug)]
struct FaultStore {
    memory: InMemorySecureStore,
    fail_marker_once: AtomicBool,
}

impl SecureStore for FaultStore {
    fn get(&self, name: &str) -> Result<Option<SecretSlice<u8>>, SecureStoreError> {
        self.memory.get(name)
    }
    fn set(&self, name: &str, value: &[u8]) -> Result<(), SecureStoreError> {
        if name.starts_with("cc.shw.v1:") && self.fail_marker_once.swap(false, Ordering::SeqCst) {
            return Err(SecureStoreError::Backend(
                "injected marker write failure".into(),
            ));
        }
        self.memory.set(name, value)
    }
    fn delete(&self, name: &str) -> Result<bool, SecureStoreError> {
        self.memory.delete(name)
    }
}

/// Records only OS entry names so recovery tests can remove one public
/// checkpoint without touching generated account credentials or device keys.
#[derive(Default)]
struct EnrollmentCheckpointStore {
    memory: InMemorySecureStore,
    names: Mutex<HashSet<String>>,
}

impl std::fmt::Debug for EnrollmentCheckpointStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EnrollmentCheckpointStore")
            .finish_non_exhaustive()
    }
}

impl SecureStore for EnrollmentCheckpointStore {
    fn get(&self, name: &str) -> Result<Option<SecretSlice<u8>>, SecureStoreError> {
        self.memory.get(name)
    }
    fn set(&self, name: &str, value: &[u8]) -> Result<(), SecureStoreError> {
        self.memory.set(name, value)?;
        if name.starts_with("cc.enroll.v1:") {
            self.names.lock().unwrap().insert(name.to_owned());
        }
        Ok(())
    }
    fn delete(&self, name: &str) -> Result<bool, SecureStoreError> {
        self.memory.delete(name)
    }
}

impl EnrollmentCheckpointStore {
    fn forget_grant_checkpoint(&self, grant_id: &str) {
        for name in self.names.lock().unwrap().iter() {
            let Some(raw) = self.memory.get(name).unwrap() else {
                continue;
            };
            let public_record: serde_json::Value =
                serde_json::from_slice(raw.expose_secret()).unwrap();
            if public_record["id"].as_str() == Some(grant_id)
                && public_record["value"].get("signed_state_hash").is_some()
            {
                assert!(raw.expose_secret().len() < 2048);
                assert!(self.memory.delete(name).unwrap());
                return;
            }
        }
        panic!("expected an independently retained public grant checkpoint");
    }
}

struct LiveChallengeFaultState {
    upstream: String,
    client: reqwest::Client,
    drop_once: AtomicBool,
    drop_accept_reply_once: AtomicBool,
    challenge_hashes: Mutex<Vec<[u8; 32]>>,
}

struct LiveChallengeFaultProxy {
    state: Arc<LiveChallengeFaultState>,
    url: String,
    task: tokio::task::JoinHandle<()>,
}

impl LiveChallengeFaultProxy {
    async fn new(upstream: &str) -> Self {
        let state = Arc::new(LiveChallengeFaultState {
            upstream: upstream.trim_end_matches('/').into(),
            client: reqwest::Client::new(),
            drop_once: AtomicBool::new(false),
            drop_accept_reply_once: AtomicBool::new(false),
            challenge_hashes: Mutex::new(vec![]),
        });
        let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", socket.local_addr().unwrap());
        let router = Router::new()
            .fallback(live_challenge_fault_route)
            .with_state(state.clone());
        let task = tokio::spawn(async move {
            axum::serve(socket, router).await.unwrap();
        });
        Self { state, url, task }
    }
}

impl Drop for LiveChallengeFaultProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn live_challenge_fault_route(
    State(state): State<Arc<LiveChallengeFaultState>>,
    request: Request,
) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let headers = request.headers().clone();
    let drop_accepted_reply = method == reqwest::Method::POST
        && uri.path().contains("/own-device-requests/")
        && uri.path().ends_with("/accept")
        && state.drop_accept_reply_once.swap(false, Ordering::SeqCst);
    let body = to_bytes(request.into_body(), 8 * 1024 * 1024)
        .await
        .unwrap();
    if method == reqwest::Method::POST
        && uri.path().contains("/own-device-requests/")
        && uri.path().ends_with("/challenge")
    {
        state
            .challenge_hashes
            .lock()
            .unwrap()
            .push(request_body_sha256(&body));
        if state.drop_once.swap(false, Ordering::SeqCst) {
            // No server state changed. Only the public ciphertext body hash is
            // retained, so the next dispatch can prove exact retransmission.
            return denied(ErrorCode::Internal);
        }
    }
    let mut outgoing = state
        .client
        .request(method, format!("{}{}", state.upstream, uri))
        .body(body.to_vec());
    for (key, value) in &headers {
        if !matches!(key.as_str(), "host" | "content-length" | "connection") {
            outgoing = outgoing.header(key, value);
        }
    }
    let upstream = outgoing.send().await.unwrap();
    let status = upstream.status().as_u16();
    let bytes = upstream.bytes().await.unwrap();
    if drop_accepted_reply && status == 200 {
        // The ordinary atomic server acceptance happened; only its response
        // is lost. The owner must retain its exact encrypted pending outbox.
        return denied(ErrorCode::Internal);
    }
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(bytes))
        .unwrap()
}

async fn reopened(account: &Account) -> AppCore {
    let mut config = AppConfig::for_tests(account.directory.to_string_lossy().to_string());
    config.background_sync = false;
    let core = AppCore::with_platform(
        config,
        account.store.clone(),
        Arc::new(UnsupportedOsAuthenticator),
    )
    .unwrap();
    core.open_profile(account.profile.clone()).await.unwrap();
    core.unlock_with_passphrase(account.passphrase.clone())
        .await
        .unwrap();
    core
}

async fn account(url: &str, directory: &Path, store: Arc<dyn SecureStore>) -> Account {
    let mut config = AppConfig::for_tests(directory.to_string_lossy().to_string());
    config.background_sync = false;
    let app = AppCore::with_platform(config, store.clone(), Arc::new(UnsupportedOsAuthenticator))
        .unwrap();
    let email = format!("sharing-{}@example.invalid", Uuid::new_v4().simple());
    let password = Zeroizing::new(format!("Cc-{}-{}!", Uuid::new_v4(), Uuid::new_v4()));
    let profile = app
        .create_synced_profile(
            "Generated".into(),
            url.into(),
            email.clone(),
            password.to_string(),
            AccountMode::Register,
        )
        .await
        .unwrap()
        .profile
        .id;
    let passphrase = format!("generated vault phrase {}", Uuid::new_v4());
    app.create_vault(passphrase.clone()).await.unwrap();
    Account {
        app,
        store,
        directory: directory.to_owned(),
        profile,
        email,
        account_password: password,
        passphrase,
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

fn grant(identity: SharingIdentityDto, role: SharingRoleDto) -> SharingGrantDto {
    SharingGrantDto {
        confirmed_code: identity.verification_code.clone(),
        identity,
        role,
    }
}

async fn local_storage(account: &Account) -> Storage {
    let name = format!("profiles/{}/db-key", account.profile);
    let bytes = account.store.get(&name).unwrap().unwrap();
    Storage::open(
        account
            .directory
            .join("profiles")
            .join(&account.profile)
            .join("vault.db"),
        DatabaseKey::from_slice(bytes.expose_secret()).unwrap(),
    )
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn facade_trust_permissions_history_revoke_cas_and_exact_response_recovery() {
    let server = Harness::new().await;
    let directory = tempfile::tempdir().unwrap();
    let owner = account(
        &server.url,
        &directory.path().join("owner"),
        Arc::new(InMemorySecureStore::new()),
    )
    .await;
    let recipient = account(
        &server.url,
        &directory.path().join("recipient"),
        Arc::new(InMemorySecureStore::new()),
    )
    .await;
    let owner_identity = owner.app.sharing_identity().await.unwrap();
    let identity = recipient.app.sharing_identity().await.unwrap();
    let discovered = owner
        .app
        .sharing_discover(recipient.email.clone())
        .await
        .unwrap();
    assert!(discovered.devices == vec![identity.clone()]);
    let mut unconfirmed = grant(identity.clone(), SharingRoleDto::Reader);
    unconfirmed.confirmed_code = String::new();
    assert!(owner
        .app
        .sharing_publish(projection("Public host"), vec![unconfirmed])
        .await
        .is_err());
    assert!(owner.app.sharing_outbox().await.unwrap().entries.is_empty());
    let created = owner
        .app
        .sharing_publish(
            projection("Public host"),
            vec![grant(identity.clone(), SharingRoleDto::Reader)],
        )
        .await
        .unwrap();
    assert_eq!(owner.app.sharing_flush().await.unwrap().entries.len(), 0);
    let incoming = recipient.app.sharing_list(true).await.unwrap();
    assert_eq!(incoming[0].trust, SharingTrustDto::Unverified);
    assert!(incoming[0].preview_json.is_none());
    assert!(recipient
        .app
        .sharing_accept(created.share_id.clone(), "incorrect".into())
        .await
        .is_err());
    let accepted = recipient
        .app
        .sharing_accept(
            created.share_id.clone(),
            owner_identity.verification_code.clone(),
        )
        .await
        .unwrap();
    assert!(accepted
        .preview_json
        .as_ref()
        .unwrap()
        .contains("Public host"));
    assert!(
        recipient.app.list_hosts().await.unwrap().is_empty(),
        "acceptance cannot import or execute content"
    );
    assert!(recipient
        .app
        .sharing_edit(created.share_id.clone(), projection("Reader edit"))
        .await
        .is_err());
    assert!(recipient
        .app
        .sharing_outbox()
        .await
        .unwrap()
        .entries
        .is_empty());
    // Simulate cancellation after owner pin persisted, before index commit.
    let storage = local_storage(&recipient).await;
    let raw: String = storage
        .setting_get("cc.sharing.facade.v1")
        .await
        .unwrap()
        .unwrap();
    let mut workspace: serde_json::Value = serde_json::from_str(&raw).unwrap();
    workspace["items"] = serde_json::json!([]);
    storage
        .setting_set(
            "cc.sharing.facade.v1",
            serde_json::to_string(&workspace).unwrap(),
        )
        .await
        .unwrap();
    storage.close().await;
    recipient
        .app
        .sharing_accept(
            created.share_id.clone(),
            owner_identity.verification_code.clone(),
        )
        .await
        .unwrap();
    owner
        .app
        .sharing_rotate(
            created.share_id.clone(),
            vec![grant(identity.clone(), SharingRoleDto::Editor)],
        )
        .await
        .unwrap();
    owner.app.sharing_flush().await.unwrap();
    let updated = recipient.app.sharing_list(true).await.unwrap();
    assert_eq!(updated[0].role, Some(SharingRoleDto::Editor));
    // Both devices prepare edits from the same signed revision. Only one
    // commits; the stale request stays encrypted and blocked for human review.
    owner
        .app
        .sharing_edit(created.share_id.clone(), projection("Owner stale edit"))
        .await
        .unwrap();
    recipient
        .app
        .sharing_edit(created.share_id.clone(), projection("Editor accepted edit"))
        .await
        .unwrap();
    recipient.app.sharing_flush().await.unwrap();
    let blocked = owner.app.sharing_flush().await.unwrap();
    assert_eq!(blocked.entries[0].state, SharingOutboxStateDto::Blocked);
    let posts = server.state.posts.load(Ordering::SeqCst);
    owner.app.sharing_flush().await.unwrap();
    assert_eq!(server.state.posts.load(Ordering::SeqCst), posts);
    owner
        .app
        .sharing_discard_pending(blocked.entries[0].mutation_id.clone())
        .await
        .unwrap();
    // A genuine 409 after the dispatch check is never retried automatically.
    owner
        .app
        .sharing_edit(created.share_id.clone(), projection("CAS edit"))
        .await
        .unwrap();
    server.state.conflict_once.store(true, Ordering::SeqCst);
    let blocked = owner.app.sharing_flush().await.unwrap();
    assert_eq!(blocked.entries[0].reason.as_deref(), Some("conflict"));
    owner
        .app
        .sharing_discard_pending(blocked.entries[0].mutation_id.clone())
        .await
        .unwrap();
    recipient
        .app
        .sharing_edit(created.share_id.clone(), projection("Queued before revoke"))
        .await
        .unwrap();
    owner
        .app
        .sharing_rotate(created.share_id.clone(), vec![])
        .await
        .unwrap();
    owner.app.sharing_flush().await.unwrap();
    let removed = recipient.app.sharing_flush().await.unwrap();
    assert_eq!(removed.entries[0].reason.as_deref(), Some("access_removed"));
    let cached = recipient.app.sharing_list(false).await.unwrap();
    assert_eq!(cached[0].trust, SharingTrustDto::Blocked);
    assert!(cached[0].preview_json.is_none());
    // Committed request whose HTTP response is lost is recognized by exact
    // signed document/ciphertext equality, without a second POST.
    owner
        .app
        .sharing_edit(created.share_id.clone(), projection("Recovered exact edit"))
        .await
        .unwrap();
    server.state.lost_once.store(true, Ordering::SeqCst);
    let before = server.state.posts.load(Ordering::SeqCst);
    assert!(owner.app.sharing_flush().await.unwrap().entries.is_empty());
    assert_eq!(server.state.posts.load(Ordering::SeqCst), before + 1);
    owner
        .app
        .sharing_delete(created.share_id.clone())
        .await
        .unwrap();
    owner.app.sharing_flush().await.unwrap();
    assert_eq!(
        owner.app.sharing_list(false).await.unwrap()[0].trust,
        SharingTrustDto::Deleted
    );
    owner.app.shutdown().await.unwrap();
    recipient.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lock_cancels_inflight_metadata_and_secure_instance_pin_survives_restart() {
    let server = Harness::new().await;
    let directory = tempfile::tempdir().unwrap();
    let account = account(
        &server.url,
        directory.path(),
        Arc::new(InMemorySecureStore::new()),
    )
    .await;
    let identity = account.app.sharing_identity().await.unwrap();
    *server.state.pause.lock().unwrap() = Some("/v1/shares/recipients".into());
    let app = account.app.clone();
    let email = account.email.clone();
    let action = tokio::spawn(async move { app.sharing_discover(email).await });
    server.state.request_started.notified().await;
    account.app.lock().await.unwrap();
    let error = tokio::time::timeout(Duration::from_secs(1), action)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(error, AppError::VaultLocked);
    assert!(account.app.sharing_status().await.unwrap().locked);
    assert_eq!(
        account.app.sharing_list(false).await.unwrap_err(),
        AppError::VaultLocked
    );
    server.state.resume.notify_one();
    *server.state.pause.lock().unwrap() = None;
    account.app.shutdown().await.unwrap();
    let mut config = AppConfig::for_tests(directory.path().to_string_lossy().to_string());
    config.background_sync = false;
    let restarted = AppCore::with_platform(
        config,
        account.store.clone(),
        Arc::new(UnsupportedOsAuthenticator),
    )
    .unwrap();
    restarted
        .open_profile(account.profile.clone())
        .await
        .unwrap();
    restarted
        .unlock_with_passphrase(account.passphrase.clone())
        .await
        .unwrap();
    assert!(restarted.sharing_identity().await.unwrap() == identity);
    *server.state.instance.lock().unwrap() = Uuid::new_v4();
    assert!(restarted.sharing_list(true).await.is_err());
    assert!(
        restarted.sharing_identity().await.unwrap() == identity,
        "replaced server must not overwrite the secure pin"
    );
    restarted.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn encrypted_create_outbox_survives_pin_failure_restart_and_lost_create_response() {
    let server = Harness::new().await;
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(FaultStore {
        memory: InMemorySecureStore::new(),
        fail_marker_once: AtomicBool::new(false),
    });
    let account = account(&server.url, directory.path(), store.clone()).await;
    account.app.sharing_identity().await.unwrap();
    let private_name = format!("Selected projection {}", Uuid::new_v4());
    store.fail_marker_once.store(true, Ordering::SeqCst);
    assert!(account
        .app
        .sharing_publish(projection(&private_name), vec![])
        .await
        .is_err());
    let queue = account.app.sharing_outbox().await.unwrap();
    assert_eq!(
        queue.entries.len(),
        1,
        "request remains durable if cache pin fails after queue commit"
    );
    let share_id = queue.entries[0].share_id.clone();
    let storage = local_storage(&account).await;
    let serialized: String = storage
        .setting_get("cc.sharing.facade.v1")
        .await
        .unwrap()
        .unwrap();
    assert!(
        !serialized.contains(&private_name),
        "outbox must contain ciphertext, never projection plaintext"
    );
    storage.close().await;
    account.app.shutdown().await.unwrap();
    let restarted = reopened(&account).await;
    assert_eq!(restarted.sharing_outbox().await.unwrap().entries.len(), 1);
    let cache = restarted.sharing_list(false).await.unwrap();
    assert!(cache[0]
        .preview_json
        .as_ref()
        .unwrap()
        .contains(&private_name));
    server.state.lost_once.store(true, Ordering::SeqCst);
    let before = server.state.posts.load(Ordering::SeqCst);
    assert!(restarted.sharing_flush().await.unwrap().entries.is_empty());
    assert_eq!(server.state.posts.load(Ordering::SeqCst), before + 1);
    assert_eq!(
        server.state.shares.lock().unwrap().len(),
        1,
        "lost create response cannot duplicate a share"
    );
    assert!(restarted.sharing_list(true).await.unwrap()[0].share_id == share_id);
    restarted.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn older_database_restore_is_blocked_then_explicit_signed_history_reconciles() {
    let server = Harness::new().await;
    let directory = tempfile::tempdir().unwrap();
    let account = account(
        &server.url,
        directory.path(),
        Arc::new(InMemorySecureStore::new()),
    )
    .await;
    let identity = account.app.sharing_identity().await.unwrap();
    let created = account
        .app
        .sharing_publish(projection("Initial accepted state"), vec![])
        .await
        .unwrap();
    account.app.sharing_flush().await.unwrap();
    account.app.shutdown().await.unwrap();
    let database = account
        .directory
        .join("profiles")
        .join(&account.profile)
        .join("vault.db");
    let old_database = directory.path().join("old-encrypted-database.snapshot");
    std::fs::copy(&database, &old_database).unwrap();
    let advanced = reopened(&account).await;
    advanced
        .sharing_edit(
            created.share_id.clone(),
            projection("New independently checkpointed state"),
        )
        .await
        .unwrap();
    advanced.sharing_flush().await.unwrap();
    advanced.shutdown().await.unwrap();
    std::fs::copy(&old_database, &database).unwrap();
    let restored = reopened(&account).await;
    let cache = restored.sharing_list(false).await.unwrap();
    assert_eq!(cache[0].trust, SharingTrustDto::Blocked);
    assert_eq!(
        cache[0].blocked_reason.as_deref(),
        Some("sharing_reconciliation_required")
    );
    assert!(cache[0].preview_json.is_none());
    assert!(restored
        .sharing_reconcile(created.share_id.clone(), "wrong code".into())
        .await
        .is_err());
    let recovered = restored
        .sharing_reconcile(created.share_id.clone(), identity.verification_code.clone())
        .await
        .unwrap();
    assert!(recovered
        .preview_json
        .as_ref()
        .unwrap()
        .contains("New independently checkpointed state"));
    assert_eq!(recovered.revision, 2);
    assert_eq!(
        restored.sharing_list(false).await.unwrap()[0].trust,
        SharingTrustDto::Verified
    );
    restored.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn disabled_capability_never_creates_a_pin_or_encrypted_outbox() {
    let server = Harness::new().await;
    let directory = tempfile::tempdir().unwrap();
    let account = account(
        &server.url,
        directory.path(),
        Arc::new(InMemorySecureStore::new()),
    )
    .await;
    server.state.enabled.store(false, Ordering::SeqCst);
    assert_eq!(
        account
            .app
            .sharing_publish(projection("Never published"), vec![])
            .await
            .unwrap_err()
            .code(),
        "unsupported"
    );
    assert!(account
        .store
        .get(&format!("profiles/{}/sharing-instance-v1", account.profile))
        .unwrap()
        .is_none());
    let storage = local_storage(&account).await;
    assert!(storage
        .setting_get::<String>("cc.sharing.facade.v1")
        .await
        .unwrap()
        .is_none());
    storage.close().await;
    account.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn secret_publication_is_capability_gated_and_metadata_is_always_masked() {
    let server = Harness::new().await;
    let directory = tempfile::tempdir().unwrap();
    let owner = account(
        &server.url,
        &directory.path().join("owner"),
        Arc::new(InMemorySecureStore::new()),
    )
    .await;
    let recipient = account(
        &server.url,
        &directory.path().join("recipient"),
        Arc::new(InMemorySecureStore::new()),
    )
    .await;
    let value = zeroize::Zeroizing::new(format!("runtime shared secret {}", Uuid::new_v4()));
    let credential = owner
        .app
        .add_password_credential("Selected secret".into(), None, value.to_string())
        .await
        .unwrap();
    assert_eq!(
        owner
            .app
            .sharing_publish_secret(credential.id.clone(), false, vec![])
            .await
            .unwrap_err()
            .code(),
        "unsupported"
    );
    assert!(owner.app.sharing_outbox().await.unwrap().entries.is_empty());
    server.state.supports_secrets.store(true, Ordering::SeqCst);
    owner.app.sharing_status().await.unwrap();
    let owner_identity = owner.app.sharing_identity().await.unwrap();
    let recipient_identity = recipient.app.sharing_identity().await.unwrap();
    assert!(
        owner
            .app
            .sharing_publish_secret(credential.id.clone(), true, vec![])
            .await
            .is_err(),
        "a missing ancillary passphrase cannot fall back to the primary secret"
    );
    let created = owner
        .app
        .sharing_publish_secret(
            credential.id,
            false,
            vec![grant(recipient_identity, SharingRoleDto::Reader)],
        )
        .await
        .unwrap();
    let preview = created.preview_json.as_ref().unwrap();
    assert!(!preview.contains(value.as_str()));
    let metadata: serde_json::Value = serde_json::from_str(preview).unwrap();
    assert!(metadata["data"].get("value").is_none());
    assert_eq!(metadata["data"]["name"], "Selected secret");
    assert!(!format!("{created:?}").contains(value.as_str()));
    owner.app.sharing_flush().await.unwrap();
    let invitations = recipient.app.sharing_list(true).await.unwrap();
    assert!(invitations[0].preview_json.is_none());
    assert!(
        recipient
            .app
            .sharing_reveal_secret(created.share_id.clone())
            .await
            .is_err(),
        "unaccepted directory keys cannot open a secret"
    );
    let accepted = recipient
        .app
        .sharing_accept(created.share_id.clone(), owner_identity.verification_code)
        .await
        .unwrap();
    assert!(!accepted
        .preview_json
        .as_ref()
        .unwrap()
        .contains(value.as_str()));
    assert!(
        serde_json::from_str::<serde_json::Value>(accepted.preview_json.as_ref().unwrap()).unwrap()
            ["data"]
            .get("value")
            .is_none()
    );
    let revealed = recipient
        .app
        .sharing_reveal_secret(created.share_id.clone())
        .await
        .unwrap();
    assert!(revealed.value == *value);
    assert!(!format!("{revealed:?}").contains(value.as_str()));
    drop(revealed);
    let storage = local_storage(&owner).await;
    let workspace: String = storage
        .setting_get("cc.sharing.facade.v1")
        .await
        .unwrap()
        .unwrap();
    assert!(
        !workspace.contains(value.as_str()),
        "the outbox contains ciphertext alone"
    );
    storage.close().await;
    server.state.supports_secrets.store(false, Ordering::SeqCst);
    let blocked = recipient.app.sharing_list(true).await.unwrap();
    assert_eq!(blocked[0].trust, SharingTrustDto::Blocked);
    assert_eq!(
        blocked[0].blocked_reason.as_deref(),
        Some("unsupported_sharing_kind")
    );
    assert!(blocked[0].preview_json.is_none());
    assert!(recipient
        .app
        .sharing_reveal_secret(created.share_id.clone())
        .await
        .is_err());
    owner.app.shutdown().await.unwrap();
    recipient.app.shutdown().await.unwrap();
}

/// Decode only the disposable FileMailer's bounded, single-part messages.
/// Errors are static: neither MIME bytes nor account tokens enter test output.
fn verification_mail_token(
    wire: &[u8],
    recipient: &str,
) -> Result<Option<SecretString>, &'static str> {
    let wire = std::str::from_utf8(wire).map_err(|_| "invalid MIME wire encoding")?;
    let (headers, body) = wire
        .split_once("\r\n\r\n")
        .ok_or("missing MIME header separator")?;
    let mut fields = BTreeMap::<String, String>::new();
    let mut previous = None;
    for line in headers.split("\r\n") {
        if line.starts_with([' ', '\t']) {
            let value = fields
                .get_mut(previous.as_ref().ok_or("unexpected folded MIME header")?)
                .ok_or("missing folded MIME header")?;
            value.push(' ');
            value.push_str(line.trim());
        } else {
            let (name, value) = line.split_once(':').ok_or("invalid MIME header")?;
            let name = name.to_ascii_lowercase();
            if fields.insert(name.clone(), value.trim().into()).is_some() {
                return Err("duplicate MIME header");
            }
            previous = Some(name);
        }
    }
    // Exact To matching prevents a previous account's message being used.
    if fields.get("to").map(String::as_str) != Some(recipient) {
        return Ok(None);
    }
    if fields.get("content-type").map(String::as_str) != Some("text/plain; charset=utf-8") {
        return Err("unexpected verification MIME content type");
    }
    let decoded = match fields
        .get("content-transfer-encoding")
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("quoted-printable") => decode_quoted_printable(body.as_bytes())?,
        Some("base64") => {
            let compact = Zeroizing::new(body.split_whitespace().collect::<String>());
            Zeroizing::new(
                base64::engine::general_purpose::STANDARD
                    .decode(compact.as_bytes())
                    .map_err(|_| "invalid base64 verification MIME")?,
            )
        }
        _ => return Err("unexpected verification MIME transfer encoding"),
    };
    let text = std::str::from_utf8(&decoded).map_err(|_| "invalid verification body UTF-8")?;
    let mut token = None;
    for candidate in text.lines().map(str::trim) {
        if !candidate.starts_with("cct_") {
            continue;
        }
        if !(24..=132).contains(&candidate.len())
            || !candidate
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            || token.is_some()
        {
            return Err("invalid verification token line");
        }
        token = Some(SecretString::new(candidate.to_owned()));
    }
    token.ok_or("missing verification token line").map(Some)
}

fn decode_quoted_printable(input: &[u8]) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    let mut decoded = Zeroizing::new(Vec::with_capacity(input.len()));
    let mut index = 0;
    while index < input.len() {
        if input[index] != b'=' {
            decoded.push(input[index]);
            index += 1;
        } else if input.get(index + 1..index + 3) == Some(b"\r\n") {
            index += 3;
        } else {
            let high = input
                .get(index + 1)
                .and_then(|byte| char::from(*byte).to_digit(16))
                .ok_or("invalid quoted-printable escape")?;
            let low = input
                .get(index + 2)
                .and_then(|byte| char::from(*byte).to_digit(16))
                .ok_or("invalid quoted-printable escape")?;
            decoded.push(((high << 4) | low) as u8);
            index += 3;
        }
    }
    Ok(decoded)
}

async fn wait_verification_mail(directory: &Path, recipient: &str) -> SecretString {
    let metadata =
        std::fs::symlink_metadata(directory).expect("the isolated FileMailer directory must exist");
    assert!(
        metadata.is_dir(),
        "FileMailer directory must be a real directory"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            metadata.permissions().mode() & 0o077,
            0,
            "mail directory must be private"
        );
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let entries = std::fs::read_dir(directory).expect("cannot read isolated mail directory");
        for entry in entries {
            let entry = entry.expect("cannot read isolated mail entry");
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !name.ends_with(".eml") || !name.contains("-verify_email-") {
                continue;
            }
            let metadata = std::fs::symlink_metadata(entry.path())
                .expect("cannot inspect isolated verification mail");
            assert!(
                metadata.is_file() && metadata.len() <= 256 * 1024,
                "invalid isolated mail file"
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                assert_eq!(
                    metadata.permissions().mode() & 0o077,
                    0,
                    "mail file must be private"
                );
            }
            let mut wire = Zeroizing::new(Vec::new());
            std::fs::File::open(entry.path())
                .expect("cannot open isolated verification mail")
                .take(256 * 1024 + 1)
                .read_to_end(&mut wire)
                .expect("cannot read isolated verification mail");
            assert!(wire.len() <= 256 * 1024, "isolated mail exceeds size bound");
            // FileMailer creates the private file before its single write.
            if wire.is_empty() {
                continue;
            }
            if let Some(token) = verification_mail_token(&wire, recipient)
                .expect("cannot decode isolated verification mail")
            {
                return token;
            }
        }
        assert!(
            Instant::now() < deadline,
            "verification mail was not delivered locally"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn verify_live_account(server: &str, mail: &Path, account: &Account) {
    let anonymous = ApiClient::new(
        ApiConfig::new(
            server,
            "0.1.14-live-test",
            cc_protocol::version::Platform::Cli,
        )
        .unwrap(),
        Arc::new(MemoryTokenStore::new()),
    )
    .unwrap();
    let token = wait_verification_mail(mail, &account.email).await;
    assert!(anonymous
        .email_verify(&VerifyEmailRequest { token })
        .await
        .expect("normal verification endpoint must accept delivered mail")
        .is_none());
}

/// A normal second installation of the anchor's personal account. Enrollment
/// never substitutes for personal-vault device approval in this fixture.
async fn live_approved_own_device(server: &str, directory: &Path, anchor: &Account) -> Account {
    let store: Arc<dyn SecureStore> = Arc::new(InMemorySecureStore::new());
    let mut config = AppConfig::for_tests(directory.to_string_lossy().to_string());
    config.background_sync = false;
    let app = AppCore::with_platform(config, store.clone(), Arc::new(UnsupportedOsAuthenticator))
        .unwrap();
    let signed_in = app
        .create_synced_profile(
            "Normal second installation".into(),
            server.into(),
            anchor.email.clone(),
            anchor.account_password.to_string(),
            AccountMode::Login,
        )
        .await
        .unwrap();
    let vault_id = anchor
        .app
        .active_profile()
        .await
        .unwrap()
        .unwrap()
        .vault_id
        .unwrap();
    let own = app.request_device_approval(Some(vault_id)).await.unwrap();
    assert!(!app.finish_device_approval(None).await.unwrap());
    assert!(anchor
        .app
        .confirm_device_approval(own.request_id.clone())
        .await
        .is_err());
    let pending = anchor
        .app
        .start_device_approval(own.request_id.clone())
        .await
        .unwrap();
    assert_eq!(pending.verification_code, own.verification_code);
    anchor
        .app
        .confirm_device_approval(own.request_id)
        .await
        .unwrap();
    assert!(app.finish_device_approval(None).await.unwrap());
    Account {
        app,
        store,
        directory: directory.to_owned(),
        profile: signed_in.profile.id,
        email: anchor.email.clone(),
        account_password: Zeroizing::new(anchor.account_password.to_string()),
        passphrase: anchor.passphrase.clone(),
    }
}

async fn live_snapshot(
    account: &Account,
    item: &SharingItemDto,
    instance: Uuid,
) -> cc_app_core::sharing_state::SharingSnapshot {
    use cc_app_core::sharing_state::{SharingBinding, SharingStateStore};
    let storage = local_storage(account).await;
    let store = SharingStateStore::new(
        storage.clone(),
        account.profile.parse().unwrap(),
        instance,
        account.store.clone(),
    )
    .unwrap();
    let snapshot = store
        .load(&SharingBinding {
            server_instance_id: instance,
            share_id: item.share_id.parse().unwrap(),
            item_id: item.item_id.parse().unwrap(),
            kind: match item.kind {
                SharingKindDto::Host => SharedItemKind::Host,
                SharingKindDto::Snippet => SharedItemKind::Snippet,
                SharingKindDto::Group => SharedItemKind::Group,
                SharingKindDto::Secret => SharedItemKind::Secret,
            },
        })
        .await
        .unwrap()
        .unwrap();
    storage.close().await;
    snapshot
}

/// Actual AppCore -> authenticated transport -> isolated PostgreSQL server ->
/// recipient AppCore. Never target production or a non-loopback host.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires isolated sharing-enabled loopback PostgreSQL server and private FileMailer"]
async fn live_facade_verified_accounts_hosts_snippets_groups_secrets_and_revoke() {
    let server =
        std::env::var("CC_SHARING_TEST_SERVER_URL").expect("isolated server URL is required");
    let url = reqwest::Url::parse(&server).unwrap();
    assert_eq!(url.scheme(), "http");
    assert!(
        matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")),
        "test must remain on loopback"
    );
    let mail = PathBuf::from(
        std::env::var("CC_SHARING_TEST_MAIL_DIR")
            .expect("private FileMailer directory is required"),
    );
    let directory = tempfile::tempdir().unwrap();
    let owner = account(
        &server,
        &directory.path().join("owner"),
        Arc::new(InMemorySecureStore::new()),
    )
    .await;
    let recipient = account(
        &server,
        &directory.path().join("recipient"),
        Arc::new(InMemorySecureStore::new()),
    )
    .await;
    verify_live_account(&server, &mail, &owner).await;
    verify_live_account(&server, &mail, &recipient).await;
    let status = owner.app.sharing_status().await.unwrap();
    assert!(status.enabled && status.supports_groups && status.supports_secrets);
    let owner_identity = owner.app.sharing_identity().await.unwrap();
    let recipient_identity = recipient.app.sharing_identity().await.unwrap();
    let discovery = owner
        .app
        .sharing_discover(recipient.email.clone())
        .await
        .unwrap();
    assert!(
        discovery.user_id == recipient_identity.user_id
            && discovery.devices == vec![recipient_identity.clone()]
    );
    let grant_reader = || vec![grant(recipient_identity.clone(), SharingRoleDto::Reader)];
    let host = owner
        .app
        .sharing_publish(projection("Shared live host"), grant_reader())
        .await
        .unwrap();
    let snippet_projection = |name: &str| {
        SharedProjection::Snippet(SharedSnippetProjection {
            name: name.into(),
            description: "Explicit shared command".into(),
            snippet_type: cc_models::snippet::SnippetType::Shell,
            shell: Some("sh".into()),
            template: "printf shared".into(),
            variables: vec![],
            tags: vec![],
        })
    };
    let snippet = owner
        .app
        .sharing_publish(snippet_projection("Shared live snippet"), grant_reader())
        .await
        .unwrap();
    let group = owner
        .app
        .sharing_publish(
            SharedProjection::Group(SharedGroupProjection {
                name: "Explicit shared collection".into(),
                tags: vec![],
                children: [&host, &snippet]
                    .into_iter()
                    .map(
                        |item| cc_app_core::sharing_projection::SharedChildReference {
                            share_id: item.share_id.parse().unwrap(),
                            item_id: item.item_id.parse().unwrap(),
                            kind: if item.kind == SharingKindDto::Host {
                                SharedItemKind::Host
                            } else {
                                SharedItemKind::Snippet
                            },
                        },
                    )
                    .collect(),
            }),
            grant_reader(),
        )
        .await
        .unwrap();
    let value = Zeroizing::new(format!("runtime live selected secret {}", Uuid::new_v4()));
    let credential = owner
        .app
        .add_password_credential(
            "Explicit live secret".into(),
            Some("private personal username".into()),
            value.to_string(),
        )
        .await
        .unwrap();
    let secret = owner
        .app
        .sharing_publish_secret(credential.id.clone(), false, grant_reader())
        .await
        .unwrap();
    assert!(owner.app.sharing_outbox().await.unwrap().entries.len() == 4);
    assert!(owner.app.sharing_flush().await.unwrap().entries.is_empty());
    let invitations = recipient.app.sharing_list(true).await.unwrap();
    assert!(
        invitations.len() == 4
            && invitations.iter().all(
                |item| item.trust == SharingTrustDto::Unverified && item.preview_json.is_none()
            )
    );
    for item in [&host, &snippet, &group, &secret] {
        let accepted = recipient
            .app
            .sharing_accept(
                item.share_id.clone(),
                owner_identity.verification_code.clone(),
            )
            .await
            .unwrap();
        assert!(accepted.trust == SharingTrustDto::Verified);
        let preview = accepted.preview_json.as_ref().unwrap();
        assert!(
            !preview.contains(value.as_str())
                && !preview.contains(&credential.id)
                && !preview.contains("private personal username")
        );
        if item.kind == SharingKindDto::Secret {
            let metadata: serde_json::Value = serde_json::from_str(preview).unwrap();
            assert!(metadata["data"].get("value").is_none());
        }
    }
    assert!(
        recipient.app.list_hosts().await.unwrap().is_empty()
            && recipient.app.list_credentials().await.unwrap().is_empty()
    );
    let reveal = recipient
        .app
        .sharing_reveal_secret(secret.share_id.clone())
        .await
        .unwrap();
    assert!(reveal.value == *value);
    drop(reveal);
    assert!(recipient
        .app
        .sharing_edit(
            snippet.share_id.clone(),
            snippet_projection("Reader denied")
        )
        .await
        .is_err());
    owner
        .app
        .sharing_rotate(
            snippet.share_id.clone(),
            vec![grant(recipient_identity.clone(), SharingRoleDto::Editor)],
        )
        .await
        .unwrap();
    owner.app.sharing_flush().await.unwrap();
    recipient.app.sharing_list(true).await.unwrap();
    recipient
        .app
        .sharing_edit(
            snippet.share_id.clone(),
            snippet_projection("Editor accepted"),
        )
        .await
        .unwrap();
    assert!(recipient
        .app
        .sharing_flush()
        .await
        .unwrap()
        .entries
        .is_empty());
    let refreshed = owner.app.sharing_list(true).await.unwrap();
    assert!(refreshed
        .iter()
        .find(|item| item.share_id == snippet.share_id)
        .unwrap()
        .preview_json
        .as_ref()
        .unwrap()
        .contains("Editor accepted"));
    recipient
        .app
        .sharing_edit(
            snippet.share_id.clone(),
            snippet_projection("Queued before revocation"),
        )
        .await
        .unwrap();
    owner
        .app
        .sharing_rotate(snippet.share_id.clone(), vec![])
        .await
        .unwrap();
    owner.app.sharing_flush().await.unwrap();
    let denied = recipient.app.sharing_flush().await.unwrap();
    assert!(denied.entries.len() == 1 && denied.entries[0].state == SharingOutboxStateDto::Blocked);
    let before = live_snapshot(
        &owner,
        &secret,
        owner_identity.server_instance_id.parse().unwrap(),
    )
    .await;
    owner
        .app
        .sharing_rotate(secret.share_id.clone(), vec![])
        .await
        .unwrap();
    owner.app.sharing_flush().await.unwrap();
    let after = live_snapshot(
        &owner,
        &secret,
        owner_identity.server_instance_id.parse().unwrap(),
    )
    .await;
    assert!(
        before.state().revision.body.as_ref().unwrap().ciphertext
            != after.state().revision.body.as_ref().unwrap().ciphertext
    );
    let verified_access = verify_shared_manifest(
        &after.state().access,
        &after.state().revision.signed.mutation.context,
        &before.owner_anchor(),
        Some(&before.manifest_checkpoint()),
    )
    .unwrap();
    let verified_update = verify_shared_mutation(
        &verified_access,
        &after.state().revision.signed,
        Some(&before.revision_checkpoint()),
    )
    .unwrap();
    let old_device =
        DeviceIdentity::load_or_create(recipient.store.as_ref(), &recipient.profile).unwrap();
    assert!(
        old_device
            .open_shared_revision(
                &verified_access,
                &verified_update,
                after.state().revision.body.as_ref(),
                old_device.device_id()
            )
            .is_err(),
        "revoked device keys cannot decrypt the newly encrypted secret"
    );
    assert!(recipient
        .app
        .sharing_reveal_secret(secret.share_id.clone())
        .await
        .is_err());
    let now = recipient.app.sharing_list(true).await.unwrap();
    assert!(now
        .iter()
        .find(|item| item.share_id == secret.share_id)
        .unwrap()
        .preview_json
        .is_none());
    owner.app.shutdown().await.unwrap();
    recipient.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn secret_reader_editor_copy_and_revocation_use_only_explicit_private_credentials() {
    use cc_models::secret::SecretValue;
    let server = Harness::new().await;
    server.state.supports_secrets.store(true, Ordering::SeqCst);
    let directory = tempfile::tempdir().unwrap();
    let owner = account(
        &server.url,
        &directory.path().join("owner"),
        Arc::new(InMemorySecureStore::new()),
    )
    .await;
    let recipient = account(
        &server.url,
        &directory.path().join("recipient"),
        Arc::new(InMemorySecureStore::new()),
    )
    .await;
    let original = Zeroizing::new(format!("runtime password {}", Uuid::new_v4()));
    let replacement = Zeroizing::new(format!("runtime replacement {}", Uuid::new_v4()));
    let credential = owner
        .app
        .add_password_credential("Explicit password".into(), None, original.to_string())
        .await
        .unwrap();
    let owner_identity = owner.app.sharing_identity().await.unwrap();
    let recipient_identity = recipient.app.sharing_identity().await.unwrap();
    let created = owner
        .app
        .sharing_publish_secret(
            credential.id.clone(),
            false,
            vec![grant(recipient_identity.clone(), SharingRoleDto::Reader)],
        )
        .await
        .unwrap();
    owner.app.sharing_flush().await.unwrap();
    assert!(
        recipient
            .app
            .sharing_copy_secret_credential(created.share_id.clone())
            .await
            .is_err(),
        "an unaccepted directory entry never creates a credential"
    );
    assert!(recipient.app.list_credentials().await.unwrap().is_empty());
    recipient
        .app
        .sharing_accept(created.share_id.clone(), owner_identity.verification_code)
        .await
        .unwrap();
    assert!(
        recipient
            .app
            .sharing_edit_secret(
                created.share_id.clone(),
                SecretValue::new(replacement.to_string())
            )
            .await
            .is_err(),
        "read-only members cannot sign a secret change"
    );
    assert!(recipient
        .app
        .sharing_outbox()
        .await
        .unwrap()
        .entries
        .is_empty());
    let copied = recipient
        .app
        .sharing_copy_secret_credential(created.share_id.clone())
        .await
        .unwrap();
    assert_eq!(copied.kind, CredentialKind::Password);
    assert!(copied.has_secret && !copied.has_remembered_passphrase);
    assert!(!serde_json::to_string(&copied)
        .unwrap()
        .contains(original.as_str()));
    let private = recipient
        .app
        .reveal_credential_secret(copied.id.clone())
        .await
        .unwrap();
    assert!(private.value == *original);
    drop(private);
    owner
        .app
        .sharing_rotate(
            created.share_id.clone(),
            vec![grant(recipient_identity, SharingRoleDto::Editor)],
        )
        .await
        .unwrap();
    owner.app.sharing_flush().await.unwrap();
    recipient.app.sharing_list(true).await.unwrap();
    let queued = recipient
        .app
        .sharing_edit_secret(
            created.share_id.clone(),
            SecretValue::new(replacement.to_string()),
        )
        .await
        .unwrap();
    assert_eq!(queued.entries.len(), 1);
    let storage = local_storage(&recipient).await;
    let outbox: String = storage
        .setting_get("cc.sharing.facade.v1")
        .await
        .unwrap()
        .unwrap();
    assert!(
        !outbox.contains(original.as_str()) && !outbox.contains(replacement.as_str()),
        "secret edits persist signed ciphertext, never a plaintext outbox"
    );
    storage.close().await;
    recipient.app.sharing_flush().await.unwrap();
    let refreshed = owner.app.sharing_list(true).await.unwrap();
    let metadata = refreshed[0].preview_json.as_ref().unwrap();
    assert!(!metadata.contains(original.as_str()) && !metadata.contains(replacement.as_str()));
    assert!(
        serde_json::from_str::<serde_json::Value>(metadata).unwrap()["data"]
            .get("value")
            .is_none()
    );
    let current = owner
        .app
        .sharing_reveal_secret(created.share_id.clone())
        .await
        .unwrap();
    assert!(current.value == *replacement);
    drop(current);
    // Sharing changes never rewrite the publisher's selected personal secret,
    // nor a recipient's already disclosed and explicitly independent copy.
    let own = owner
        .app
        .reveal_credential_secret(credential.id)
        .await
        .unwrap();
    assert!(own.value == *original);
    let old_copy = recipient
        .app
        .reveal_credential_secret(copied.id.clone())
        .await
        .unwrap();
    assert!(old_copy.value == *original);
    drop((own, old_copy));
    owner
        .app
        .sharing_rotate(created.share_id.clone(), vec![])
        .await
        .unwrap();
    owner.app.sharing_flush().await.unwrap();
    let count = recipient.app.list_credentials().await.unwrap().len();
    assert!(
        recipient
            .app
            .sharing_copy_secret_credential(created.share_id.clone())
            .await
            .is_err(),
        "removed access is checked online before creating another private copy"
    );
    assert_eq!(recipient.app.list_credentials().await.unwrap().len(), count);
    assert!(recipient
        .app
        .sharing_reveal_secret(created.share_id.clone())
        .await
        .is_err());
    let old_copy = recipient
        .app
        .reveal_credential_secret(copied.id)
        .await
        .unwrap();
    assert!(old_copy.value == *original);
    drop(old_copy);
    owner.app.shutdown().await.unwrap();
    recipient.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn protected_private_key_copy_preserves_exact_original_and_never_auto_attaches_passphrase() {
    let server = Harness::new().await;
    server.state.supports_secrets.store(true, Ordering::SeqCst);
    let directory = tempfile::tempdir().unwrap();
    let owner = account(
        &server.url,
        &directory.path().join("owner"),
        Arc::new(InMemorySecureStore::new()),
    )
    .await;
    let recipient = account(
        &server.url,
        &directory.path().join("recipient"),
        Arc::new(InMemorySecureStore::new()),
    )
    .await;
    let passphrase = Zeroizing::new(format!("runtime key phrase {}", Uuid::new_v4()));
    let original = owner
        .app
        .generate_ssh_key(
            "Protected shared key".into(),
            Some("owner-only-user".into()),
            KeyGenAlgorithm::Ed25519,
            Some(passphrase.to_string()),
            true,
        )
        .await
        .unwrap();
    let original_bytes = owner
        .app
        .reveal_credential_secret(original.id.clone())
        .await
        .unwrap();
    assert!(cc_ssh_core::keys::is_private_key_encrypted(&original_bytes.value).unwrap());
    let owner_identity = owner.app.sharing_identity().await.unwrap();
    let recipient_identity = recipient.app.sharing_identity().await.unwrap();
    let grant_reader = || vec![grant(recipient_identity.clone(), SharingRoleDto::Reader)];
    let key_share = owner
        .app
        .sharing_publish_secret(original.id.clone(), false, grant_reader())
        .await
        .unwrap();
    owner.app.sharing_flush().await.unwrap();
    recipient
        .app
        .sharing_accept(
            key_share.share_id.clone(),
            owner_identity.verification_code.clone(),
        )
        .await
        .unwrap();
    let copied = recipient
        .app
        .sharing_copy_secret_credential(key_share.share_id.clone())
        .await
        .unwrap();
    assert_eq!(copied.kind, CredentialKind::SshPrivateKey);
    assert!(copied.has_secret && copied.key_encrypted && !copied.has_remembered_passphrase);
    assert!(
        copied.username.is_none(),
        "a shared value does not link the owner's account or credential settings"
    );
    assert_eq!(copied.fingerprint, original.fingerprint);
    let copied_bytes = recipient
        .app
        .reveal_credential_secret(copied.id.clone())
        .await
        .unwrap();
    assert!(copied_bytes.value == original_bytes.value);
    assert!(cc_ssh_core::keys::is_private_key_encrypted(&copied_bytes.value).unwrap());
    let info = cc_ssh_core::keys::inspect_private_key(
        &copied_bytes.value,
        Some(&secrecy::SecretString::from(passphrase.to_string())),
    )
    .unwrap();
    assert!(info.encrypted);
    let metadata = serde_json::to_string(&copied).unwrap();
    assert!(!metadata.contains(passphrase.as_str()) && !metadata.contains(&copied_bytes.value));
    drop(copied_bytes);
    let phrase_share = owner
        .app
        .sharing_publish_secret(original.id.clone(), true, grant_reader())
        .await
        .unwrap();
    owner.app.sharing_flush().await.unwrap();
    recipient
        .app
        .sharing_accept(
            phrase_share.share_id.clone(),
            owner_identity.verification_code,
        )
        .await
        .unwrap();
    let count = recipient.app.list_credentials().await.unwrap().len();
    assert!(
        recipient
            .app
            .sharing_copy_secret_credential(phrase_share.share_id.clone())
            .await
            .is_err(),
        "a separately shared passphrase never attaches itself to any private key credential"
    );
    assert_eq!(recipient.app.list_credentials().await.unwrap().len(), count);
    assert!(
        !recipient
            .app
            .get_credential(copied.id)
            .await
            .unwrap()
            .has_remembered_passphrase
    );
    let still_original = owner
        .app
        .reveal_credential_secret(original.id)
        .await
        .unwrap();
    assert!(still_original.value == original_bytes.value);
    drop((original_bytes, still_original));
    owner.app.shutdown().await.unwrap();
    recipient.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bound_copy_endpoint_confirmation_is_exact_and_verified_before_connection() {
    let server = Harness::new().await;
    let directory = tempfile::tempdir().unwrap();
    let owner = account(
        &server.url,
        &directory.path().join("owner"),
        Arc::new(InMemorySecureStore::new()),
    )
    .await;
    let recipient = account(
        &server.url,
        &directory.path().join("recipient"),
        Arc::new(InMemorySecureStore::new()),
    )
    .await;
    let owner_identity = owner.app.sharing_identity().await.unwrap();
    let recipient_identity = recipient.app.sharing_identity().await.unwrap();
    let initial = SharedProjection::Host(SharedHostProjection {
        name: "Shared target".into(),
        address: "old.example.test".into(),
        port: 2222,
        username: Some("operator".into()),
        keepalive_secs: None,
        tags: vec![],
        notes: None,
    });
    let share = owner
        .app
        .sharing_publish(
            initial,
            vec![grant(recipient_identity, SharingRoleDto::Reader)],
        )
        .await
        .unwrap();
    owner.app.sharing_flush().await.unwrap();
    recipient
        .app
        .sharing_accept(share.share_id.clone(), owner_identity.verification_code)
        .await
        .unwrap();
    let copied = recipient
        .app
        .sharing_copy_host(share.share_id.clone(), None)
        .await
        .unwrap();
    assert!(recipient
        .app
        .describe_connection(copied.id.clone())
        .await
        .unwrap()
        .route
        .contains("old.example.test:2222"));
    owner
        .app
        .sharing_edit(
            share.share_id.clone(),
            SharedProjection::Host(SharedHostProjection {
                name: "New target".into(),
                address: "new.example.test".into(),
                port: 2223,
                username: Some("operator".into()),
                keepalive_secs: None,
                tags: vec![],
                notes: None,
            }),
        )
        .await
        .unwrap();
    owner.app.sharing_flush().await.unwrap();
    assert!(recipient
        .app
        .describe_connection(copied.id.clone())
        .await
        .is_err());
    assert!(
        recipient
            .app
            .sharing_refresh_bound_host(copied.id.clone(), true)
            .await
            .is_err(),
        "a generic boolean cannot authorize whichever endpoint the server supplies later"
    );
    assert!(recipient
        .app
        .sharing_refresh_bound_host_expected(copied.id.clone(), "old.example.test".into(), 2222)
        .await
        .is_err());
    let unchanged = recipient.app.get_host(copied.id.clone()).await.unwrap();
    assert_eq!(unchanged.address, "old.example.test");
    assert_eq!(unchanged.port, Some(2222));
    let approved = recipient
        .app
        .sharing_refresh_bound_host_expected(copied.id.clone(), "new.example.test".into(), 2223)
        .await
        .unwrap();
    assert_eq!(approved.address, "new.example.test");
    assert_eq!(approved.port, Some(2223));
    assert!(recipient
        .app
        .describe_connection(copied.id.clone())
        .await
        .unwrap()
        .route
        .contains("new.example.test:2223"));
    owner
        .app
        .sharing_rotate(share.share_id.clone(), vec![])
        .await
        .unwrap();
    owner.app.sharing_flush().await.unwrap();
    assert!(
        recipient.app.describe_connection(copied.id).await.is_err(),
        "a formerly approved bound copy must still check current access"
    );
    owner.app.shutdown().await.unwrap();
    recipient.app.shutdown().await.unwrap();
}

/// The complete own-device path over the real authenticated server. The
/// fixture supplies only independently compared public codes, normal login,
/// and ordinary personal-device approval; there is no administrative bypass.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires isolated S08-enabled loopback PostgreSQL server and private FileMailer"]
async fn live_owner_online_enrollment_pairs_exact_keys_rotates_and_preserves_terminal_grants() {
    let server =
        std::env::var("CC_SHARING_TEST_SERVER_URL").expect("isolated server URL is required");
    let url = reqwest::Url::parse(&server).unwrap();
    assert_eq!(url.scheme(), "http");
    assert!(matches!(
        url.host_str(),
        Some("127.0.0.1" | "localhost" | "[::1]")
    ));
    let mail = PathBuf::from(
        std::env::var("CC_SHARING_TEST_MAIL_DIR")
            .expect("private FileMailer directory is required"),
    );
    let directory = tempfile::tempdir().unwrap();
    let challenge_proxy = Box::pin(LiveChallengeFaultProxy::new(&server)).await;
    let owner_checkpoints = Arc::new(EnrollmentCheckpointStore::default());
    let mut owner = Box::pin(account(
        &challenge_proxy.url,
        &directory.path().join("owner"),
        owner_checkpoints.clone(),
    ))
    .await;
    let anchor = Box::pin(account(
        &server,
        &directory.path().join("anchor"),
        Arc::new(InMemorySecureStore::new()),
    ))
    .await;
    let late_anchor = Box::pin(account(
        &server,
        &directory.path().join("late-anchor"),
        Arc::new(InMemorySecureStore::new()),
    ))
    .await;
    Box::pin(verify_live_account(&server, &mail, &owner)).await;
    Box::pin(verify_live_account(&server, &mail, &anchor)).await;
    Box::pin(verify_live_account(&server, &mail, &late_anchor)).await;
    let target = Box::pin(live_approved_own_device(
        &server,
        &directory.path().join("target"),
        &anchor,
    ))
    .await;
    let status = Box::pin(owner.app.sharing_status()).await.unwrap();
    assert!(status.enabled && status.supports_owner_online_enrollment_v1);
    let owner_identity = Box::pin(owner.app.sharing_identity()).await.unwrap();
    let anchor_identity = Box::pin(anchor.app.sharing_identity()).await.unwrap();
    let late_identity = Box::pin(late_anchor.app.sharing_identity()).await.unwrap();
    let target_identity = Box::pin(target.app.sharing_identity()).await.unwrap();
    assert_eq!(anchor_identity.user_id, target_identity.user_id);
    assert_ne!(anchor_identity.device_id, target_identity.device_id);
    assert_ne!(
        anchor_identity.encryption_public_key,
        target_identity.encryption_public_key
    );
    assert_ne!(
        anchor_identity.signing_public_key,
        target_identity.signing_public_key
    );
    let share = Box::pin(owner.app.sharing_publish(
        projection("Paired own-device endpoint"),
        vec![
            grant(anchor_identity.clone(), SharingRoleDto::Reader),
            grant(late_identity.clone(), SharingRoleDto::Reader),
        ],
    ))
    .await
    .unwrap();
    Box::pin(owner.app.sharing_flush()).await.unwrap();
    Box::pin(anchor.app.sharing_accept(
        share.share_id.clone(),
        owner_identity.verification_code.clone(),
    ))
    .await
    .unwrap();
    let create = || EnrollmentGrantCreateDto {
        anchor: anchor_identity.clone(),
        confirmed_identity_code: anchor_identity.verification_code.clone(),
        role_ceiling: SharingRoleDto::Reader,
        mode: EnrollmentModeDto::Manual,
        expires_at: chrono::Utc::now().timestamp() + 3600,
        max_admissions: 2,
    };
    assert!(
        Box::pin(
            anchor
                .app
                .enrollment_create_grant(share.share_id.clone(), create())
        )
        .await
        .is_err(),
        "only the exact original owner device may sign a grant"
    );
    let mut wrong_identity = create();
    wrong_identity.confirmed_identity_code = "unverified public directory identity".into();
    assert!(Box::pin(
        owner
            .app
            .enrollment_create_grant(share.share_id.clone(), wrong_identity)
    )
    .await
    .is_err());
    let own_grant = Box::pin(
        owner
            .app
            .enrollment_create_grant(share.share_id.clone(), create()),
    )
    .await
    .unwrap();
    let mut other_create = create();
    other_create.anchor = late_identity.clone();
    other_create.confirmed_identity_code = late_identity.verification_code.clone();
    let other_grant = Box::pin(
        owner
            .app
            .enrollment_create_grant(share.share_id.clone(), other_create),
    )
    .await
    .unwrap();
    let source = Box::pin(
        anchor
            .app
            .enrollment_export_anchor_bundle(share.share_id.clone(), own_grant.grant_id.clone()),
    )
    .await
    .unwrap();
    assert!(
        Box::pin(
            target.app.enrollment_prepare_target(
                source.public_bundle_json.clone(),
                SharingRoleDto::Editor
            )
        )
        .await
        .is_err(),
        "own-device enrollment cannot elevate the recipient's reader grant"
    );
    let prepared = Box::pin(
        target
            .app
            .enrollment_prepare_target(source.public_bundle_json, SharingRoleDto::Reader),
    )
    .await
    .unwrap();
    let code = prepared.comparison_code.clone().unwrap();
    assert!(Box::pin(anchor.app.enrollment_endorse_target(
        prepared.public_bundle_json.clone(),
        "wrong whole-request code".into()
    ))
    .await
    .is_err());
    let endorsed = Box::pin(
        anchor
            .app
            .enrollment_endorse_target(prepared.public_bundle_json, code.clone()),
    )
    .await
    .unwrap();
    assert!(Box::pin(target.app.enrollment_submit_target(
        endorsed.public_bundle_json.clone(),
        "wrong whole-request code".into()
    ))
    .await
    .is_err());
    let retained_pairing_packet = endorsed.public_bundle_json.clone();
    let retained_pairing_code = code.clone();
    let submitted = Box::pin(
        target
            .app
            .enrollment_submit_target(endorsed.public_bundle_json, code),
    )
    .await
    .unwrap();
    assert_eq!(submitted.target, target_identity);
    assert_eq!(submitted.requested_role, SharingRoleDto::Reader);
    // A pending target has no accepted object membership yet. Losing its
    // encrypted transcript row requires the independently compared endorsed
    // packet, rather than asking the server directory to repin these keys.
    let target_storage = local_storage(&target).await;
    let target_record_key = format!("cc.enrollment.facade.v1/{}", share.share_id);
    assert!(target_storage
        .write(move |tx| tx.setting_delete(&target_record_key))
        .await
        .unwrap());
    target_storage.close().await;
    assert!(Box::pin(target.app.enrollment_restore_pairing(
        retained_pairing_packet.clone(),
        "wrong independently compared full request code".into(),
    ))
    .await
    .is_err());
    let restored_pairing = Box::pin(
        target
            .app
            .enrollment_restore_pairing(retained_pairing_packet, retained_pairing_code),
    )
    .await
    .unwrap();
    assert_eq!(restored_pairing.len(), 1);
    assert_eq!(restored_pairing[0].request_id, submitted.request_id);
    assert_eq!(restored_pairing[0].target, target_identity);
    assert_eq!(
        restored_pairing[0].status,
        EnrollmentRequestStatusDto::Pending
    );
    let listed = Box::pin(owner.app.enrollment_list_requests(share.share_id.clone()))
        .await
        .unwrap();
    assert!(listed
        .iter()
        .any(|r| r.request_id == submitted.request_id && r.target == target_identity));
    assert!(
        Box::pin(owner.app.enrollment_accept(
            share.share_id.clone(),
            submitted.request_id.clone(),
            true
        ))
        .await
        .is_err(),
        "an Ed25519 signed request does not bypass the target X25519 possession challenge"
    );
    challenge_proxy
        .state
        .drop_once
        .store(true, Ordering::SeqCst);
    assert!(Box::pin(
        owner
            .app
            .enrollment_challenge(share.share_id.clone(), submitted.request_id.clone(),)
    )
    .await
    .is_err());
    // Crash after the local authenticated ciphertext/OS checkpoint write but
    // before its first delivery. Retry must first resend those exact bytes.
    Box::pin(owner.app.shutdown()).await.unwrap();
    owner.app = Box::pin(reopened(&owner)).await;
    let challenged = Box::pin(
        owner
            .app
            .enrollment_challenge(share.share_id.clone(), submitted.request_id.clone()),
    )
    .await
    .unwrap();
    assert_eq!(challenged.status, EnrollmentRequestStatusDto::Challenged);
    {
        let hashes = challenge_proxy.state.challenge_hashes.lock().unwrap();
        assert_eq!(hashes.len(), 3);
        assert_eq!(hashes[0], hashes[1]);
        assert_ne!(hashes[1], hashes[2]);
    }
    // Owner possession secrets are memory only. A full restart must retain
    // the signed challenge checkpoint but discard its secret: a new challenge
    // is required before the old response can authorize a fresh rotation.
    Box::pin(owner.app.shutdown()).await.unwrap();
    owner.app = Box::pin(reopened(&owner)).await;

    let responded = Box::pin(
        target
            .app
            .enrollment_respond(share.share_id.clone(), submitted.request_id.clone()),
    )
    .await
    .unwrap();
    assert_eq!(responded.status, EnrollmentRequestStatusDto::Responded);
    assert!(
        Box::pin(owner.app.enrollment_accept(
            share.share_id.clone(),
            submitted.request_id.clone(),
            true
        ))
        .await
        .is_err(),
        "a restart cannot reconstruct or silently reuse an owner possession secret"
    );
    let replacement = Box::pin(
        owner
            .app
            .enrollment_challenge(share.share_id.clone(), submitted.request_id.clone()),
    )
    .await
    .unwrap();
    assert_eq!(
        replacement.challenge_generation,
        Some(challenged.challenge_generation.unwrap() + 1)
    );
    let responded = Box::pin(
        target
            .app
            .enrollment_respond(share.share_id.clone(), submitted.request_id.clone()),
    )
    .await
    .unwrap();
    assert_eq!(
        responded.challenge_generation,
        replacement.challenge_generation
    );
    assert_eq!(responded.status, EnrollmentRequestStatusDto::Responded);
    let record_key = format!("cc.enrollment.facade.v1/{}", share.share_id);
    let storage = local_storage(&owner).await;
    let older_enrollment: String = storage.setting_get(&record_key).await.unwrap().unwrap();
    storage.close().await;

    let instance = owner_identity.server_instance_id.parse().unwrap();
    let before = Box::pin(live_snapshot(&owner, &share, instance)).await;
    challenge_proxy
        .state
        .drop_accept_reply_once
        .store(true, Ordering::SeqCst);
    assert!(Box::pin(owner.app.enrollment_accept(
        share.share_id.clone(),
        submitted.request_id.clone(),
        true,
    ))
    .await
    .is_err());
    // The server already admitted this target, while local SQL/OS grant heads
    // still name the pre-admission Active state. Freeze must survive the lost
    // response and must not sign a competing terminal fork from that old head.
    assert!(matches!(
        Box::pin(
            owner
                .app
                .enrollment_revoke_grant(share.share_id.clone(), own_grant.grant_id.clone(),)
        )
        .await,
        Err(AppError::SharingReconciliationRequired)
    ));
    let frozen_forward = Box::pin(owner.app.enrollment_reconcile(
        share.share_id.clone(),
        owner_identity.verification_code.clone(),
    ))
    .await
    .unwrap();
    let resumed = frozen_forward
        .iter()
        .find(|g| g.grant_id == own_grant.grant_id)
        .unwrap();
    assert!(resumed.frozen);
    assert_eq!(resumed.status, EnrollmentGrantStatusDto::Active);
    assert_eq!(resumed.admitted_count, 1);
    assert_eq!(resumed.revision, own_grant.revision + 1);
    let current_items = Box::pin(owner.app.sharing_list(true)).await.unwrap();
    assert_eq!(
        current_items
            .iter()
            .find(|i| i.share_id == share.share_id)
            .unwrap()
            .trust,
        SharingTrustDto::Verified
    );
    let after = Box::pin(live_snapshot(&owner, &share, instance)).await;
    assert_eq!(
        after.state().access.manifest.revision,
        before.state().access.manifest.revision + 1
    );
    assert_eq!(
        after.state().access.manifest.access_epoch,
        before.state().access.manifest.access_epoch + 1
    );
    assert_eq!(
        after.state().access.manifest.members.len(),
        before.state().access.manifest.members.len() + 1
    );
    assert!(before
        .state()
        .access
        .manifest
        .members
        .iter()
        .all(|old| after.state().access.manifest.members.contains(old)));
    let admitted = after
        .state()
        .access
        .manifest
        .members
        .iter()
        .find(|m| m.device_id.to_string() == target_identity.device_id)
        .unwrap();
    assert_eq!(admitted.role, SharingRole::Reader);
    assert_eq!(admitted.user_id.to_string(), anchor_identity.user_id);
    assert!(
        before.state().revision.body.as_ref().unwrap().ciphertext
            != after.state().revision.body.as_ref().unwrap().ciphertext,
        "admission uses a fresh encrypted body for the new access epoch"
    );
    let owner_grants = Box::pin(owner.app.enrollment_list_grants(share.share_id.clone()))
        .await
        .unwrap();
    let consumed = owner_grants
        .iter()
        .find(|g| g.grant_id == own_grant.grant_id)
        .unwrap();
    let other = owner_grants
        .iter()
        .find(|g| g.grant_id == other_grant.grant_id)
        .unwrap();
    assert_eq!(consumed.admitted_count, 1);
    assert_eq!(consumed.revision, own_grant.revision + 1);
    assert_eq!(other.admitted_count, 0);
    assert_eq!(other.revision, other_grant.revision + 1);
    // This independently verified participant has never opened enrollment
    // locally. Its first grant access must reconstruct the older signed
    // manifest used by the grant genesis, not begin at only today's head.
    Box::pin(late_anchor.app.sharing_accept(
        share.share_id.clone(),
        owner_identity.verification_code.clone(),
    ))
    .await
    .unwrap();
    let late_source = Box::pin(
        late_anchor
            .app
            .enrollment_export_anchor_bundle(share.share_id.clone(), other_grant.grant_id.clone()),
    )
    .await
    .unwrap();
    assert_eq!(late_source.grant_id, other_grant.grant_id);

    let completed = Box::pin(
        target
            .app
            .enrollment_respond(share.share_id.clone(), submitted.request_id.clone()),
    )
    .await
    .unwrap();
    assert_eq!(completed.status, EnrollmentRequestStatusDto::Accepted);
    let received = Box::pin(target.app.sharing_accept(
        share.share_id.clone(),
        owner_identity.verification_code.clone(),
    ))
    .await
    .unwrap();
    assert_eq!(received.role, Some(SharingRoleDto::Reader));
    assert!(received
        .preview_json
        .as_ref()
        .unwrap()
        .contains("Paired own-device endpoint"));
    assert!(Box::pin(
        target
            .app
            .sharing_edit(share.share_id.clone(), projection("Forbidden reader edit"))
    )
    .await
    .is_err());
    assert!(Box::pin(
        target
            .app
            .sharing_accept(share.share_id.clone(), "wrong owner code".into())
    )
    .await
    .is_err());
    let _retry = Box::pin(owner.app.enrollment_accept(
        share.share_id.clone(),
        submitted.request_id.clone(),
        false,
    ))
    .await;
    let replay_head = Box::pin(live_snapshot(&owner, &share, instance)).await;
    assert_eq!(
        replay_head.manifest_checkpoint(),
        after.manifest_checkpoint()
    );
    assert_eq!(
        replay_head.revision_checkpoint(),
        after.revision_checkpoint()
    );
    let revoked = Box::pin(
        owner
            .app
            .enrollment_revoke_grant(share.share_id.clone(), own_grant.grant_id.clone()),
    )
    .await
    .unwrap();
    assert_eq!(revoked.status, EnrollmentGrantStatusDto::Revoked);
    assert!(revoked.frozen);
    Box::pin(owner.app.shutdown()).await.unwrap();
    let restarted = Box::pin(reopened(&owner)).await;
    let observed = Box::pin(restarted.enrollment_list_grants(share.share_id.clone()))
        .await
        .unwrap();
    let terminal = observed
        .iter()
        .find(|g| g.grant_id == own_grant.grant_id)
        .unwrap();
    assert_eq!(terminal.status, EnrollmentGrantStatusDto::Revoked);
    assert!(terminal.frozen && terminal.revision == revoked.revision);
    assert!(Box::pin(
        anchor
            .app
            .enrollment_export_anchor_bundle(share.share_id.clone(), own_grant.grant_id)
    )
    .await
    .is_err());
    Box::pin(restarted.shutdown()).await.unwrap();
    // Restore an older authenticated encrypted DB record while retaining OS
    // high-water marks. The record was valid before admission/revocation, but
    // cannot silently reset either terminal grant or challenge generation.
    let storage = local_storage(&owner).await;
    storage
        .setting_set(record_key.clone(), older_enrollment)
        .await
        .unwrap();
    storage.close().await;
    let rolled_back = Box::pin(reopened(&owner)).await;
    assert!(
        matches!(
            Box::pin(rolled_back.enrollment_list_grants(share.share_id.clone())).await,
            Err(AppError::SharingReconciliationRequired)
        ),
        "SQLCipher transcript rollback must fail closed against the independent OS marker"
    );
    assert!(Box::pin(rolled_back.enrollment_reconcile(
        share.share_id.clone(),
        "incorrect independently compared owner identity".into(),
    ))
    .await
    .is_err());
    let recovered = Box::pin(rolled_back.enrollment_reconcile(
        share.share_id.clone(),
        owner_identity.verification_code.clone(),
    ))
    .await
    .unwrap();
    assert_eq!(recovered.len(), 2);
    let terminal = recovered
        .iter()
        .find(|g| g.grant_id == revoked.grant_id)
        .unwrap();
    assert!(terminal.frozen);
    assert_eq!(terminal.status, EnrollmentGrantStatusDto::Revoked);
    assert_eq!(terminal.revision, revoked.revision);
    let retained_other = recovered
        .iter()
        .find(|g| g.grant_id == other_grant.grant_id)
        .unwrap();
    assert_eq!(retained_other.admitted_count, 0);
    assert_eq!(retained_other.revision, other_grant.revision + 1);
    let retained_requests = Box::pin(rolled_back.enrollment_list_requests(share.share_id.clone()))
        .await
        .unwrap();
    let retained_request = retained_requests
        .iter()
        .find(|r| r.request_id == submitted.request_id)
        .unwrap();
    assert_eq!(
        retained_request.status,
        EnrollmentRequestStatusDto::Accepted
    );
    assert_eq!(
        retained_request.challenge_generation,
        replacement.challenge_generation
    );
    let head_after_recovery = Box::pin(live_snapshot(&owner, &share, instance)).await;
    assert_eq!(
        head_after_recovery.revision_checkpoint(),
        after.revision_checkpoint()
    );
    assert!(Box::pin(rolled_back.enrollment_process_automatic())
        .await
        .unwrap()
        .is_empty());
    Box::pin(rolled_back.shutdown()).await.unwrap();

    // Losing the complete SQL row still retains exact owner/scope/known IDs in
    // OS storage. Recovery must restore them, rather than creating a fresh pin.
    let storage = local_storage(&owner).await;
    let remove_key = record_key.clone();
    assert!(storage
        .write(move |tx| tx.setting_delete(&remove_key))
        .await
        .unwrap());
    storage.close().await;
    let missing_row = Box::pin(reopened(&owner)).await;
    let recovered_row = Box::pin(missing_row.enrollment_reconcile(
        share.share_id.clone(),
        owner_identity.verification_code.clone(),
    ))
    .await
    .unwrap();
    assert_eq!(recovered_row.len(), 2);
    assert!(recovered_row.iter().any(|g| g.grant_id == revoked.grant_id
        && g.frozen
        && g.status == EnrollmentGrantStatusDto::Revoked));
    assert!(
        Box::pin(missing_row.enrollment_list_requests(share.share_id.clone()))
            .await
            .unwrap()
            .iter()
            .any(|r| r.request_id == submitted.request_id
                && r.status == EnrollmentRequestStatusDto::Accepted
                && r.challenge_generation == replacement.challenge_generation)
    );
    Box::pin(missing_row.shutdown()).await.unwrap();

    // A known grant ID with a missing independent marker cannot be rebuilt
    // solely from a server response or old encrypted SQL transcript.
    owner_checkpoints.forget_grant_checkpoint(&revoked.grant_id);
    let missing_marker = Box::pin(reopened(&owner)).await;
    assert!(matches!(
        Box::pin(missing_marker.enrollment_list_grants(share.share_id.clone())).await,
        Err(AppError::SharingReconciliationRequired)
    ));
    assert!(matches!(
        Box::pin(
            missing_marker
                .enrollment_reconcile(share.share_id.clone(), owner_identity.verification_code,)
        )
        .await,
        Err(AppError::SharingReconciliationRequired)
    ));
    Box::pin(missing_marker.shutdown()).await.unwrap();

    Box::pin(late_anchor.app.shutdown()).await.unwrap();
    Box::pin(anchor.app.shutdown()).await.unwrap();
    Box::pin(target.app.shutdown()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires isolated S08-enabled loopback PostgreSQL server and private FileMailer"]
async fn live_automatic_enrollment_uses_only_signed_automatic_unfrozen_reader_grants() {
    use cc_app_core::enrollment_dto::EnrollmentPairingDto;
    let server =
        std::env::var("CC_SHARING_TEST_SERVER_URL").expect("isolated server URL is required");
    let url = reqwest::Url::parse(&server).unwrap();
    assert_eq!(url.scheme(), "http");
    assert!(matches!(
        url.host_str(),
        Some("127.0.0.1" | "localhost" | "[::1]")
    ));
    let mail = PathBuf::from(
        std::env::var("CC_SHARING_TEST_MAIL_DIR")
            .expect("private FileMailer directory is required"),
    );
    let directory = tempfile::tempdir().unwrap();
    let owner = Box::pin(account(
        &server,
        &directory.path().join("owner"),
        Arc::new(InMemorySecureStore::new()),
    ))
    .await;
    let anchor = Box::pin(account(
        &server,
        &directory.path().join("anchor"),
        Arc::new(InMemorySecureStore::new()),
    ))
    .await;
    Box::pin(verify_live_account(&server, &mail, &owner)).await;
    Box::pin(verify_live_account(&server, &mail, &anchor)).await;
    let target = Box::pin(live_approved_own_device(
        &server,
        &directory.path().join("target"),
        &anchor,
    ))
    .await;
    let owner_identity = Box::pin(owner.app.sharing_identity()).await.unwrap();
    let anchor_identity = Box::pin(anchor.app.sharing_identity()).await.unwrap();
    let mut automatic = None;
    let mut manual = None;
    let mut frozen = None;
    for (name, mode, freeze) in [
        (
            "Explicit automatic reader",
            EnrollmentModeDto::Automatic,
            false,
        ),
        ("Manual stays pending", EnrollmentModeDto::Manual, false),
        (
            "Frozen never auto enrolls",
            EnrollmentModeDto::Automatic,
            true,
        ),
    ] {
        let share = Box::pin(owner.app.sharing_publish(
            projection(name),
            vec![grant(anchor_identity.clone(), SharingRoleDto::Reader)],
        ))
        .await
        .unwrap();
        Box::pin(owner.app.sharing_flush()).await.unwrap();
        Box::pin(anchor.app.sharing_accept(
            share.share_id.clone(),
            owner_identity.verification_code.clone(),
        ))
        .await
        .unwrap();
        let grant = Box::pin(owner.app.enrollment_create_grant(
            share.share_id.clone(),
            EnrollmentGrantCreateDto {
                anchor: anchor_identity.clone(),
                confirmed_identity_code: anchor_identity.verification_code.clone(),
                role_ceiling: SharingRoleDto::Reader,
                mode,
                expires_at: chrono::Utc::now().timestamp() + 3600,
                max_admissions: 1,
            },
        ))
        .await
        .unwrap();
        let source: EnrollmentPairingDto = Box::pin(
            anchor
                .app
                .enrollment_export_anchor_bundle(share.share_id.clone(), grant.grant_id.clone()),
        )
        .await
        .unwrap();
        let prepared = Box::pin(
            target
                .app
                .enrollment_prepare_target(source.public_bundle_json, SharingRoleDto::Reader),
        )
        .await
        .unwrap();
        let code = prepared.comparison_code.unwrap();
        let endorsed = Box::pin(
            anchor
                .app
                .enrollment_endorse_target(prepared.public_bundle_json, code.clone()),
        )
        .await
        .unwrap();
        let request = Box::pin(
            target
                .app
                .enrollment_submit_target(endorsed.public_bundle_json, code),
        )
        .await
        .unwrap();
        if freeze {
            let terminal = Box::pin(
                owner
                    .app
                    .enrollment_revoke_grant(share.share_id.clone(), grant.grant_id),
            )
            .await
            .unwrap();
            assert!(terminal.frozen && terminal.status == EnrollmentGrantStatusDto::Revoked);
            frozen = Some((share, request));
        } else if mode == EnrollmentModeDto::Manual {
            manual = Some((share, request));
        } else {
            automatic = Some((share, request));
        }
    }
    let (auto_share, auto_request) = automatic.unwrap();
    let (manual_share, manual_request) = manual.unwrap();
    let (frozen_share, frozen_request) = frozen.unwrap();
    let first = Box::pin(owner.app.enrollment_process_automatic())
        .await
        .unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].request_id, auto_request.request_id);
    assert_eq!(first[0].status, EnrollmentRequestStatusDto::Challenged);
    let untouched = Box::pin(
        owner
            .app
            .enrollment_list_requests(manual_share.share_id.clone()),
    )
    .await
    .unwrap();
    let manual_state = untouched
        .iter()
        .find(|r| r.request_id == manual_request.request_id)
        .unwrap();
    assert_eq!(manual_state.status, EnrollmentRequestStatusDto::Pending);
    assert!(manual_state.challenge_generation.is_none());
    let blocked = Box::pin(
        owner
            .app
            .enrollment_list_requests(frozen_share.share_id.clone()),
    )
    .await
    .unwrap();
    assert!(blocked
        .iter()
        .all(|r| r.request_id != frozen_request.request_id || r.challenge_generation.is_none()));
    let response = Box::pin(
        target
            .app
            .enrollment_respond(auto_share.share_id.clone(), auto_request.request_id.clone()),
    )
    .await
    .unwrap();
    assert_eq!(response.status, EnrollmentRequestStatusDto::Responded);
    let second = Box::pin(owner.app.enrollment_process_automatic())
        .await
        .unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].request_id, auto_request.request_id);
    assert_eq!(second[0].status, EnrollmentRequestStatusDto::Accepted);
    let complete = Box::pin(
        target
            .app
            .enrollment_respond(auto_share.share_id.clone(), auto_request.request_id),
    )
    .await
    .unwrap();
    assert_eq!(complete.status, EnrollmentRequestStatusDto::Accepted);
    let received = Box::pin(target.app.sharing_accept(
        auto_share.share_id.clone(),
        owner_identity.verification_code,
    ))
    .await
    .unwrap();
    assert_eq!(received.role, Some(SharingRoleDto::Reader));
    assert!(Box::pin(target.app.sharing_edit(
        auto_share.share_id.clone(),
        projection("Forbidden reader upgrade")
    ))
    .await
    .is_err());
    assert!(
        Box::pin(owner.app.enrollment_process_automatic())
            .await
            .unwrap()
            .is_empty(),
        "a completed request must never rotate twice"
    );
    Box::pin(owner.app.shutdown()).await.unwrap();
    Box::pin(anchor.app.shutdown()).await.unwrap();
    Box::pin(target.app.shutdown()).await.unwrap();
}
