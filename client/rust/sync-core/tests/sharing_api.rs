//! Real HTTP transport tests, including exact request-proof targets and body
//! bytes. Sharing authorization itself is exercised in sharing_live.rs.

use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::StatusCode,
    response::Response,
    Router,
};
use cc_crypto_core::sharing::{
    seal_shared_revision, shared_body_hash, sign_shared_manifest, sign_shared_mutation,
    verify_shared_manifest, SharingOwnerAnchor,
};
use cc_crypto_core::{request_body_sha256, verify_request_proof, DeviceSecretKeys};
use cc_protocol::auth::{RefreshRequest, SecretString, TokenPair};
use cc_protocol::devices::RequestProof;
use cc_protocol::sharing::*;
use cc_protocol::version::{Platform, HEADER_DEVICE_PROOF};
use cc_protocol::{Bytes, DeviceId, ErrorCode, MutationId, ObjectId, SessionId, ShareId, UserId};
use cc_sync_core::{ApiClient, ApiConfig, ApiError, MemoryTokenStore, RequestSigner, SignerError};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use uuid::Uuid;

struct Signer {
    keys: Arc<DeviceSecretKeys>,
    id: DeviceId,
}
impl RequestSigner for Signer {
    fn request_proof(
        &self,
        method: &str,
        target: &str,
        body: &[u8],
    ) -> Result<RequestProof, SignerError> {
        let issued_at = chrono::Utc::now().timestamp();
        let mut nonce = [0; 32];
        cc_crypto_core::fill_random(&mut nonce).map_err(|_| SignerError::Unavailable)?;
        Ok(RequestProof {
            issued_at,
            nonce,
            signature: self.keys.sign_request(
                self.id,
                method,
                target,
                &request_body_sha256(body),
                issued_at,
                &nonce,
            ),
        })
    }
}

struct Mock {
    server_instance: Uuid,
    device_id: DeviceId,
    public_signing: [u8; 32],
    current: Mutex<SharedItemState>,
    tokens: Mutex<TokenPair>,
    requests: Mutex<Vec<(String, String, [u8; 32])>>,
    nonces: Mutex<HashSet<[u8; 32]>>,
    overrides: Mutex<HashMap<String, (StatusCode, Value)>>,
    reject_once: AtomicBool,
}

fn tokens() -> TokenPair {
    let now = chrono::Utc::now();
    TokenPair {
        session_id: SessionId::new(),
        access_token: SecretString::new(Uuid::new_v4().to_string()),
        refresh_token: SecretString::new(Uuid::new_v4().to_string()),
        access_expires_at: now + chrono::Duration::hours(1),
        refresh_expires_at: now + chrono::Duration::days(1),
    }
}

fn reply(status: StatusCode, value: Value) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&value).unwrap()))
        .unwrap()
}

fn error(status: StatusCode, code: ErrorCode, reason: Option<&str>) -> Response {
    let mut body = cc_protocol::ApiError::new(code, "generated test response");
    body.details = reason.map(|reason| json!({ "reason": reason }));
    reply(status, serde_json::to_value(body).unwrap())
}

async fn handler(State(state): State<Arc<Mock>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, 2 * 1024 * 1024).await.unwrap();
    let target = parts.uri.to_string();
    let path = parts
        .uri
        .path()
        .strip_prefix("/prefix")
        .unwrap_or(parts.uri.path())
        .to_owned();
    let proof = parts
        .headers
        .get(HEADER_DEVICE_PROOF)
        .and_then(|v| v.to_str().ok())
        .and_then(RequestProof::decode);
    let Some(proof) = proof else {
        return error(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::InvalidProof,
            Some("missing"),
        );
    };
    if verify_request_proof(
        &state.public_signing,
        state.device_id,
        parts.method.as_str(),
        &target,
        &request_body_sha256(&body),
        proof.issued_at,
        &proof.nonce,
        &proof.signature,
    )
    .is_err()
    {
        return error(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::InvalidProof,
            Some("invalid_signature"),
        );
    }
    if !state.nonces.lock().unwrap().insert(proof.nonce) {
        return error(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::InvalidProof,
            Some("replayed"),
        );
    }
    state.requests.lock().unwrap().push((
        parts.method.to_string(),
        target,
        request_body_sha256(&body),
    ));
    if path == "/v1/auth/refresh" {
        let refresh: RefreshRequest = serde_json::from_slice(&body).unwrap();
        if refresh.refresh_token != state.tokens.lock().unwrap().refresh_token {
            return error(StatusCode::UNAUTHORIZED, ErrorCode::Unauthorized, None);
        }
        let next = tokens();
        *state.tokens.lock().unwrap() = next.clone();
        return reply(StatusCode::OK, serde_json::to_value(next).unwrap());
    }
    let authorized = parts
        .headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.strip_prefix("Bearer ")
                == Some(state.tokens.lock().unwrap().access_token.expose_secret())
        });
    if !authorized || state.reject_once.swap(false, Ordering::SeqCst) {
        return error(StatusCode::UNAUTHORIZED, ErrorCode::Unauthorized, None);
    }
    if let Some((status, value)) = state.overrides.lock().unwrap().get(&path) {
        return reply(*status, value.clone());
    }
    let current = state.current.lock().unwrap().clone();
    if path == "/v1/shares/capabilities" {
        return reply(
            StatusCode::OK,
            serde_json::to_value(SharingCapabilities {
                enabled: true,
                server_instance_id: state.server_instance,
                format: FORMAT,
                max_members: MAX_MEMBERS as u32,
                max_ciphertext_bytes: MAX_CIPHERTEXT_BYTES as u32,
                supports_groups: false,
                supports_secrets: false,
                supports_owner_online_enrollment_v1: false,
            })
            .unwrap(),
        );
    }
    if path == "/v1/shares/recipients" {
        let member = current.access.manifest.members[0].clone();
        return reply(
            StatusCode::OK,
            serde_json::to_value(SharingRecipient {
                user_id: member.user_id,
                devices: vec![member],
            })
            .unwrap(),
        );
    }
    if path == "/v1/shares" {
        if parts.method == "POST" {
            let req: CreateShareRequest = serde_json::from_slice(&body).unwrap();
            let next = SharedItemState {
                access: req.access,
                revision: req.revision,
            };
            *state.current.lock().unwrap() = next.clone();
            return reply(StatusCode::CREATED, serde_json::to_value(next).unwrap());
        }
        return reply(
            StatusCode::OK,
            serde_json::to_value(ShareListPage {
                items: vec![current],
                next_after: None,
                has_more: false,
            })
            .unwrap(),
        );
    }
    if path.ends_with("/revisions") {
        let req: PutSharedRevisionRequest = serde_json::from_slice(&body).unwrap();
        let next = SharedItemState {
            access: current.access,
            revision: req.revision,
        };
        *state.current.lock().unwrap() = next.clone();
        return reply(StatusCode::OK, serde_json::to_value(next).unwrap());
    }
    if path.ends_with("/access") {
        let req: RotateShareAccessRequest = serde_json::from_slice(&body).unwrap();
        let next = SharedItemState {
            access: req.access,
            revision: req.revision,
        };
        *state.current.lock().unwrap() = next.clone();
        return reply(StatusCode::OK, serde_json::to_value(next).unwrap());
    }
    if path.ends_with("/history") {
        return reply(
            StatusCode::OK,
            serde_json::to_value(ShareHistoryPage {
                manifests: vec![],
                revisions: vec![],
                latest_manifest_revision: 1,
                latest_revision: 1,
                has_more: false,
            })
            .unwrap(),
        );
    }
    reply(StatusCode::OK, serde_json::to_value(current).unwrap())
}

struct Fixture {
    api: ApiClient,
    mock: Arc<Mock>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn new(prefix: bool) -> Self {
        let keys = Arc::new(DeviceSecretKeys::generate().unwrap());
        let device_id = DeviceId::new();
        let user_id = UserId::new();
        let context = SharingContext {
            server_instance_id: Uuid::new_v4(),
            share_id: ShareId::new(),
            item_id: ObjectId::new(),
            revision: 1,
            access_epoch: 1,
            kind: SharedItemKind::Host,
        };
        let owner = SharingOwnerAnchor {
            user_id,
            device_id,
            public_keys: keys.public_keys(),
        };
        let access = AccessManifest {
            format: FORMAT,
            server_instance_id: context.server_instance_id,
            share_id: context.share_id,
            item_id: context.item_id,
            owner_user_id: user_id,
            owner_device_id: device_id,
            revision: 1,
            access_epoch: 1,
            previous_manifest_hash: Bytes::from([0; 32]),
            kind: context.kind,
            members: vec![SharingMember {
                user_id,
                device_id,
                encryption_public_key: keys.public_keys().encryption_bytes(),
                signing_public_key: keys.public_keys().signing_bytes(),
                role: SharingRole::Editor,
            }],
        };
        let access = sign_shared_manifest(&keys, access, &context, &owner, None).unwrap();
        let verified = verify_shared_manifest(&access, &context, &owner, None).unwrap();
        let body =
            seal_shared_revision(&verified, &context, b"selected public test projection").unwrap();
        let mutation = SharingMutation {
            context: context.clone(),
            mutation_id: MutationId::new(),
            base_revision: 0,
            manifest_revision: 1,
            manifest_hash: Bytes::from(verified.hash()),
            writer_device_id: device_id,
            previous_revision_hash: Bytes::from([0; 32]),
            operation: SharingOperation::Put,
            body_hash: Bytes::from(shared_body_hash(&body).unwrap()),
        };
        let signed = sign_shared_mutation(&verified, &keys, mutation, Some(&body), None).unwrap();
        let current = SharedItemState {
            access,
            revision: SharedRevision {
                signed,
                body: Some(body),
            },
        };
        let tokens = tokens();
        let mock = Arc::new(Mock {
            server_instance: context.server_instance_id,
            device_id,
            public_signing: keys.public_keys().signing,
            current: Mutex::new(current),
            tokens: Mutex::new(tokens.clone()),
            requests: Mutex::new(vec![]),
            nonces: Mutex::new(HashSet::new()),
            overrides: Mutex::new(HashMap::new()),
            reject_once: AtomicBool::new(false),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().fallback(handler).with_state(mock.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let url = format!("http://{address}{}", if prefix { "/prefix" } else { "" });
        let api = ApiClient::new(
            ApiConfig::new(&url, "0.1.14-test", Platform::Cli).unwrap(),
            Arc::new(MemoryTokenStore::new()),
        )
        .unwrap()
        .with_request_signer(Arc::new(Signer {
            keys,
            id: device_id,
        }));
        api.set_tokens(&tokens).await.unwrap();
        Self { api, mock, task }
    }

    fn override_response(&self, path: &str, value: Value) {
        self.mock
            .overrides
            .lock()
            .unwrap()
            .insert(path.into(), (StatusCode::OK, value));
    }
}

#[tokio::test]
async fn every_sharing_route_uses_exact_signed_target_query_and_body() {
    let f = Fixture::new(true).await;
    let api = f.api.sharing(f.mock.server_instance).await.unwrap();
    let current = f.mock.current.lock().unwrap().clone();
    let id = current.access.manifest.share_id;
    api.list_shares(None, 10).await.unwrap();
    api.get_share(id).await.unwrap();
    api.create_share(&CreateShareRequest {
        access: current.access.clone(),
        revision: current.revision.clone(),
    })
    .await
    .unwrap();
    api.put_shared_revision(
        id,
        &PutSharedRevisionRequest {
            revision: current.revision.clone(),
        },
    )
    .await
    .unwrap();
    api.rotate_shared_access(
        id,
        &RotateShareAccessRequest {
            access: current.access.clone(),
            revision: current.revision.clone(),
        },
    )
    .await
    .unwrap();
    api.share_history(id, 1, 1, 100).await.unwrap();
    let email = "alice+test&role=editor/Тест@example.test";
    api.sharing_recipient(email).await.unwrap();
    let requests = f.mock.requests.lock().unwrap();
    assert_eq!(requests.len(), 8);
    assert!(requests
        .iter()
        .all(|(_, target, _)| target.starts_with("/prefix/v1/shares")));
    assert!(requests.iter().any(|(_, target, _)| target
        == &format!("/prefix/v1/shares/{id}/history?after_manifest=1&after_revision=1&limit=100")));
    let target = &requests.last().unwrap().1;
    let parsed = url::Url::parse(&format!("http://localhost{target}")).unwrap();
    assert_eq!(
        parsed.query_pairs().collect::<Vec<_>>(),
        vec![("email".into(), email.into())]
    );
    assert!(!target.contains("&role=editor"));
    // A proof verified by the handler on actual body bytes covers POST JSON;
    // the production serializer isn't duplicated or replaced in these tests.
    assert!(
        requests
            .iter()
            .filter(|(method, _, _)| method == "POST")
            .count()
            == 3
    );
}

#[tokio::test]
async fn capability_gate_rejects_disabled_incompatible_replaced_or_missing_support() {
    let f = Fixture::new(false).await;
    for changed in [
        json!({"enabled":false,"server_instance_id":f.mock.server_instance,"format":1,"max_members":64,"max_ciphertext_bytes":1048592}),
        json!({"enabled":true,"server_instance_id":f.mock.server_instance,"format":2,"max_members":64,"max_ciphertext_bytes":1048592}),
        json!({"enabled":true,"server_instance_id":Uuid::new_v4(),"format":1,"max_members":64,"max_ciphertext_bytes":1048592}),
    ] {
        f.override_response("/v1/shares/capabilities", changed);
        assert!(f.api.sharing(f.mock.server_instance).await.is_err());
    }
    let count = f.mock.requests.lock().unwrap().len();
    assert!(f.api.sharing(Uuid::nil()).await.is_err());
    assert_eq!(f.mock.requests.lock().unwrap().len(), count);
    f.mock.overrides.lock().unwrap().insert(
        "/v1/shares/capabilities".into(),
        (
            StatusCode::NOT_FOUND,
            serde_json::to_value(cc_protocol::ApiError::new(
                ErrorCode::NotFound,
                "sharing unavailable",
            ))
            .unwrap(),
        ),
    );
    assert!(f
        .api
        .sharing(f.mock.server_instance)
        .await
        .unwrap_err()
        .is_code(ErrorCode::NotFound));
    assert!(f
        .mock
        .requests
        .lock()
        .unwrap()
        .iter()
        .all(|(_, target, _)| target == "/v1/shares/capabilities"));
}

#[tokio::test]
async fn invalid_bounds_and_removed_signer_never_send_unsigned_requests() {
    let f = Fixture::new(false).await;
    let api = f.api.sharing(f.mock.server_instance).await.unwrap();
    let id = f.mock.current.lock().unwrap().access.manifest.share_id;
    for limit in [0, 101] {
        assert!(api.list_shares(None, limit).await.is_err());
        assert!(api.share_history(id, 0, 0, limit).await.is_err());
    }
    assert!(api.share_history(id, 0, -1, 10).await.is_err());
    assert!(api.share_history(id, u64::MAX, 0, 10).await.is_err());
    assert!(api.get_share(ShareId::NIL).await.is_err());
    assert!(api.sharing_recipient("x\n@example.test").await.is_err());
    f.api.set_request_signer(None);
    assert!(matches!(
        api.get_share(id).await,
        Err(ApiError::RequestSigning(_))
    ));
    assert_eq!(f.mock.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn token_refresh_and_retry_are_signed_with_fresh_nonces() {
    let f = Fixture::new(false).await;
    let api = f.api.sharing(f.mock.server_instance).await.unwrap();
    let id = f.mock.current.lock().unwrap().access.manifest.share_id;
    f.mock.reject_once.store(true, Ordering::SeqCst);
    api.get_share(id).await.unwrap();
    let requests = f.mock.requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[1].1, requests[3].1);
    assert_eq!(requests[2].1, "/v1/auth/refresh");
    assert_eq!(f.mock.nonces.lock().unwrap().len(), 4);
}

#[tokio::test]
async fn item_and_directory_identity_mismatches_fail_before_payload_use() {
    let f = Fixture::new(false).await;
    let api = f.api.sharing(f.mock.server_instance).await.unwrap();
    let current = f.mock.current.lock().unwrap().clone();
    let id = current.access.manifest.share_id;
    assert!(matches!(
        api.get_share(ShareId::new()).await,
        Err(ApiError::InvalidResponse { .. })
    ));
    let mut changed = current.clone();
    changed.access.manifest.server_instance_id = Uuid::new_v4();
    f.override_response(
        &format!("/v1/shares/{id}"),
        serde_json::to_value(changed).unwrap(),
    );
    assert!(matches!(
        api.get_share(id).await,
        Err(ApiError::InvalidResponse { .. })
    ));
    f.override_response(
        "/v1/shares/recipients",
        serde_json::to_value(SharingRecipient {
            user_id: UserId::new(),
            devices: current.access.manifest.members,
        })
        .unwrap(),
    );
    assert!(matches!(
        api.sharing_recipient("alice@example.test").await,
        Err(ApiError::InvalidResponse { .. })
    ));
}

#[tokio::test]
async fn history_gaps_and_invalid_pagination_are_not_silently_accepted() {
    let f = Fixture::new(false).await;
    let api = f.api.sharing(f.mock.server_instance).await.unwrap();
    let current = f.mock.current.lock().unwrap().clone();
    let id = current.access.manifest.share_id;
    let mut header = current.revision.signed.clone();
    header.mutation.context.revision = 3;
    header.mutation.base_revision = 2;
    f.override_response(
        &format!("/v1/shares/{id}/history"),
        serde_json::to_value(ShareHistoryPage {
            manifests: vec![],
            revisions: vec![header],
            latest_manifest_revision: 1,
            latest_revision: 3,
            has_more: false,
        })
        .unwrap(),
    );
    assert!(matches!(
        api.share_history(id, 1, 1, 100).await,
        Err(ApiError::InvalidResponse { .. })
    ));
    f.override_response(
        "/v1/shares",
        serde_json::to_value(ShareListPage {
            items: vec![current.clone(), current],
            next_after: None,
            has_more: false,
        })
        .unwrap(),
    );
    assert!(matches!(
        api.list_shares(None, 100).await,
        Err(ApiError::InvalidResponse { .. })
    ));
}

#[tokio::test]
async fn secret_publication_and_mismatched_write_reply_are_rejected() {
    let f = Fixture::new(false).await;
    let api = f.api.sharing(f.mock.server_instance).await.unwrap();
    let current = f.mock.current.lock().unwrap().clone();
    let id = current.access.manifest.share_id;
    let mut secret = current.clone();
    secret.access.manifest.kind = SharedItemKind::Secret;
    secret.revision.signed.mutation.context.kind = SharedItemKind::Secret;
    assert!(api
        .create_share(&CreateShareRequest {
            access: secret.access,
            revision: secret.revision
        })
        .await
        .is_err());
    assert_eq!(f.mock.requests.lock().unwrap().len(), 1);
    let mut altered = current.clone();
    altered.revision.signed.mutation.mutation_id = MutationId::new();
    f.override_response(
        &format!("/v1/shares/{id}/revisions"),
        serde_json::to_value(altered).unwrap(),
    );
    assert!(matches!(
        api.put_shared_revision(
            id,
            &PutSharedRevisionRequest {
                revision: current.revision
            }
        )
        .await,
        Err(ApiError::InvalidResponse { .. })
    ));
}

#[tokio::test]
async fn mixed_kind_list_negotiation_is_explicit_and_device_proof_bound() {
    let f = Fixture::new(true).await;
    f.override_response("/v1/shares/capabilities", json!({"enabled":true,
        "server_instance_id":f.mock.server_instance,"format":1,"max_members":64,"max_ciphertext_bytes":1048592,
        "supports_groups":true,"supports_secrets":true,"supports_owner_online_enrollment_v1":false}));
    let api = f.api.sharing(f.mock.server_instance).await.unwrap();
    api.list_shares(None, 10).await.unwrap();
    api.list_shares_with_kinds(None, 10, true, false)
        .await
        .unwrap();
    api.list_shares_with_kinds(None, 10, false, true)
        .await
        .unwrap();
    let requests = f.mock.requests.lock().unwrap();
    assert_eq!(requests[1].1, "/prefix/v1/shares?limit=10");
    assert_eq!(
        requests[2].1,
        "/prefix/v1/shares?limit=10&include_groups=true"
    );
    assert_eq!(
        requests[3].1,
        "/prefix/v1/shares?limit=10&include_secrets=true"
    );
}

#[tokio::test]
async fn unadvertised_kind_request_never_sends_http_and_unsolicited_kind_is_rejected() {
    let f = Fixture::new(false).await;
    let baseline = f.api.sharing(f.mock.server_instance).await.unwrap();
    let count = f.mock.requests.lock().unwrap().len();
    assert!(baseline
        .list_shares_with_kinds(None, 10, true, false)
        .await
        .is_err());
    assert!(baseline
        .list_shares_with_kinds(None, 10, false, true)
        .await
        .is_err());
    assert_eq!(f.mock.requests.lock().unwrap().len(), count);
    f.override_response("/v1/shares/capabilities", json!({"enabled":true,
        "server_instance_id":f.mock.server_instance,"format":1,"max_members":64,"max_ciphertext_bytes":1048592,
        "supports_groups":true,"supports_secrets":true}));
    let upgraded = f.api.sharing(f.mock.server_instance).await.unwrap();
    let mut state = f.mock.current.lock().unwrap().clone();
    state.access.manifest.kind = SharedItemKind::Group;
    state.revision.signed.mutation.context.kind = SharedItemKind::Group;
    f.override_response(
        "/v1/shares",
        serde_json::to_value(ShareListPage {
            items: vec![state],
            next_after: None,
            has_more: false,
        })
        .unwrap(),
    );
    assert!(upgraded.list_shares(None, 10).await.is_err());
    assert!(upgraded
        .list_shares_with_kinds(None, 10, true, false)
        .await
        .is_ok());
}
