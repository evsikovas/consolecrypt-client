//! Real HTTP transport checks. All transcript signatures and request proofs
//! use runtime keys; authorization/possession remain crypto/AppCore duties.
use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    response::Response,
    Router,
};
use cc_crypto_core::sharing::*;
use cc_crypto_core::sharing_enrollment::*;
use cc_crypto_core::{request_body_sha256, verify_request_proof, DeviceSecretKeys};
use cc_protocol::auth::{SecretString, TokenPair};
use cc_protocol::devices::RequestProof;
use cc_protocol::sharing::*;
use cc_protocol::sharing_enrollment::*;
use cc_protocol::version::Platform;
use cc_protocol::{Bytes, DeviceId, MutationId, ObjectId, SessionId, ShareId, UserId};
use cc_sync_core::{
    ApiClient, ApiConfig, MemoryTokenStore, RequestSigner, SignerError, TokenStore,
};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use uuid::Uuid;
use zeroize::Zeroizing;

struct CryptoFlow {
    owner: DeviceSecretKeys,
    target: DeviceSecretKeys,
    pin: SharingOwnerAnchor,
    access: VerifiedSharingManifest,
    revision: VerifiedSharingMutation,
    body: SharedEncryptedBody,
    grant: VerifiedEnrollmentGrant,
    request: VerifiedEnrollmentRequest,
    endorsement: VerifiedAnchorEndorsement,
    now: i64,
}
impl CryptoFlow {
    fn new(role: SharingRole, quota: u32) -> Self {
        let owner = DeviceSecretKeys::generate().unwrap();
        let anchor = DeviceSecretKeys::generate().unwrap();
        let target = DeviceSecretKeys::generate().unwrap();
        let owner_id = DeviceId::new();
        let owner_user = UserId::new();
        let anchor_id = DeviceId::new();
        let target_id = DeviceId::new();
        let recipient_user = UserId::new();
        let pin = SharingOwnerAnchor {
            user_id: owner_user,
            device_id: owner_id,
            public_keys: owner.public_keys(),
        };
        let context = SharingContext {
            server_instance_id: Uuid::new_v4(),
            share_id: ShareId::new(),
            item_id: ObjectId::new(),
            revision: 1,
            access_epoch: 1,
            kind: SharedItemKind::Snippet,
        };
        let member = |user_id, device_id, keys: &DeviceSecretKeys, role| SharingMember {
            user_id,
            device_id,
            encryption_public_key: keys.public_keys().encryption_bytes(),
            signing_public_key: keys.public_keys().signing_bytes(),
            role,
        };
        let manifest = AccessManifest {
            format: 1,
            server_instance_id: context.server_instance_id,
            share_id: context.share_id,
            item_id: context.item_id,
            owner_user_id: owner_user,
            owner_device_id: owner_id,
            revision: 1,
            access_epoch: 1,
            previous_manifest_hash: Bytes::from([0; 32]),
            kind: context.kind,
            members: vec![
                member(owner_user, owner_id, &owner, SharingRole::Editor),
                member(recipient_user, anchor_id, &anchor, role),
            ],
        };
        let signed = sign_shared_manifest(&owner, manifest, &context, &pin, None).unwrap();
        let access = verify_shared_manifest(&signed, &context, &pin, None).unwrap();
        let plain = Zeroizing::new(format!("runtime content {}", Uuid::new_v4()).into_bytes());
        let body = seal_shared_revision(&access, &context, &plain).unwrap();
        let mutation = SharingMutation {
            context: context.clone(),
            mutation_id: MutationId::new(),
            base_revision: 0,
            manifest_revision: 1,
            manifest_hash: Bytes::from(access.hash()),
            writer_device_id: owner_id,
            previous_revision_hash: Bytes::from([0; 32]),
            operation: SharingOperation::Put,
            body_hash: Bytes::from(shared_body_hash(&body).unwrap()),
        };
        let signed = sign_shared_mutation(&access, &owner, mutation, Some(&body), None).unwrap();
        let revision = verify_shared_mutation(&access, &signed, None).unwrap();
        let now = chrono::Utc::now().timestamp();
        let scope = EnrollmentScope {
            server_instance_id: context.server_instance_id,
            share_id: context.share_id,
            item_id: context.item_id,
            kind: context.kind,
        };
        let binding = |device_id, keys: &DeviceSecretKeys| EnrollmentDeviceBinding {
            user_id: recipient_user,
            device_id,
            encryption_public_key: keys.public_keys().encryption_bytes(),
            signing_public_key: keys.public_keys().signing_bytes(),
        };
        let grant = SharingOwnDevicesGrantState {
            format: 1,
            scope: scope.clone(),
            owner_user_id: owner_user,
            owner_device_id: owner_id,
            grant_id: Uuid::new_v4(),
            grant_revision: 1,
            previous_grant_state_hash: Bytes::from([0; 32]),
            status: EnrollmentGrantStatus::Active,
            anchor: binding(anchor_id, &anchor),
            access_manifest_hash: Bytes::from(access.hash()),
            access_epoch: 1,
            role_ceiling: role,
            mode: EnrollmentMode::Manual,
            not_before: now,
            expires_at: now + 3600,
            max_admissions: quota,
            admitted_count: 0,
        };
        let signed = sign_grant_state(
            &owner,
            grant,
            &pin,
            None,
            GrantTransition::Genesis { access: &access },
            now,
        )
        .unwrap();
        let grant = verify_grant_state(
            &signed,
            &pin,
            None,
            GrantTransition::Genesis { access: &access },
            now,
        )
        .unwrap();
        let request = SharingOwnDeviceRequest {
            format: 1,
            scope,
            request_id: Uuid::new_v4(),
            grant_state_hash: Bytes::from(grant.hash()),
            access_manifest_hash: Bytes::from(access.hash()),
            access_epoch: 1,
            target: binding(target_id, &target),
            requested_role: role,
            nonce: Bytes::from(random32()),
            not_before: now,
            expires_at: now + 600,
        };
        let signed = sign_device_request(&target, request, &grant, &access, now).unwrap();
        let request = verify_device_request(&signed, &grant, &access, now).unwrap();
        let signed = endorse_device_request(
            &anchor,
            &request,
            &access,
            &enrollment_pairing_code(&request).unwrap(),
            now,
        )
        .unwrap();
        let endorsement = verify_anchor_endorsement(&signed, &request, &access, now).unwrap();
        Self {
            owner,
            target,
            pin,
            access,
            revision,
            body,
            grant,
            request,
            endorsement,
            now,
        }
    }
    fn proof(
        &self,
    ) -> (
        SignedSharingDeviceChallenge,
        OwnerChallengeSecret,
        VerifiedDevicePossession,
    ) {
        let (challenge, pending) = create_device_challenge(
            &self.owner,
            &self.pin,
            &self.request,
            &self.endorsement,
            &self.access,
            self.now,
            None,
        )
        .unwrap();
        let verified = verify_device_challenge(
            &challenge,
            &self.pin,
            &self.request,
            &self.endorsement,
            &self.access,
            self.now,
            None,
        )
        .unwrap();
        let response = answer_device_challenge(
            &self.target,
            &self.pin,
            &self.request,
            &self.endorsement,
            &self.access,
            &challenge,
            self.now,
            None,
        )
        .unwrap();
        let proof = verify_device_possession(
            &response,
            &pending,
            &self.request,
            &self.access,
            self.now,
            &verified.checkpoint(),
        )
        .unwrap();
        (challenge, pending, proof)
    }
    fn accepted(
        &self,
        proof: &VerifiedDevicePossession,
        others: &[VerifiedEnrollmentGrant],
    ) -> AcceptOwnDeviceRequest {
        accept_own_device(
            &self.owner,
            &self.pin,
            &self.access,
            &self.revision,
            &self.body,
            &self.request,
            &self.endorsement,
            proof,
            &proof.challenge().checkpoint(),
            &self.grant,
            others,
            self.now,
        )
        .unwrap()
    }
    fn verify_accept(
        &self,
        value: &AcceptOwnDeviceRequest,
        proof: &VerifiedDevicePossession,
        others: &[VerifiedEnrollmentGrant],
    ) -> Result<VerifiedEnrollmentAcceptance, EnrollmentCryptoError> {
        verify_own_device_acceptance(
            value,
            &self.pin,
            &self.access,
            &self.revision,
            &self.request,
            &self.endorsement,
            proof.challenge(),
            proof.response(),
            others,
            self.now,
        )
    }
}
fn random32() -> [u8; 32] {
    let mut data = [0u8; 32];
    cc_crypto_core::fill_random(&mut data).unwrap();
    data
}

struct Signer {
    id: DeviceId,
    keys: Arc<DeviceSecretKeys>,
}
impl RequestSigner for Signer {
    fn request_proof(
        &self,
        method: &str,
        target: &str,
        body: &[u8],
    ) -> Result<RequestProof, SignerError> {
        let issued_at = chrono::Utc::now().timestamp();
        let nonce = random32();
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
    caps: Mutex<SharingCapabilities>,
    reply: Mutex<Value>,
    public: [u8; 32],
    device: DeviceId,
    nonces: Mutex<HashSet<[u8; 32]>>,
    requests: Mutex<Vec<(String, String, [u8; 32])>>,
}
async fn route(State(mock): State<Arc<Mock>>, request: Request) -> Response {
    let method = request.method().to_string();
    let target = request.uri().to_string();
    let path = request.uri().path().to_string();
    let proof = RequestProof::decode(
        request
            .headers()
            .get("x-cc-device-proof")
            .unwrap()
            .to_str()
            .unwrap(),
    )
    .unwrap();
    assert!(request.headers().get("authorization").is_some());
    let body = to_bytes(request.into_body(), 8 * 1024 * 1024)
        .await
        .unwrap();
    let hash = request_body_sha256(&body);
    verify_request_proof(
        &mock.public,
        mock.device,
        &method,
        &target,
        &hash,
        proof.issued_at,
        &proof.nonce,
        &proof.signature,
    )
    .unwrap();
    assert!(mock.nonces.lock().unwrap().insert(proof.nonce));
    mock.requests.lock().unwrap().push((method, target, hash));
    let reply = if path.ends_with("/capabilities") {
        serde_json::to_value(mock.caps.lock().unwrap().clone()).unwrap()
    } else {
        mock.reply.lock().unwrap().clone()
    };
    Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&reply).unwrap()))
        .unwrap()
}
struct Http {
    api: ApiClient,
    mock: Arc<Mock>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Http {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Http {
    async fn new(flow: &CryptoFlow) -> Self {
        let keys = Arc::new(DeviceSecretKeys::generate().unwrap());
        let id = DeviceId::new();
        let mock = Arc::new(Mock {
            caps: Mutex::new(SharingCapabilities {
                enabled: true,
                format: 1,
                server_instance_id: flow.access.manifest().server_instance_id,
                max_members: 64,
                max_ciphertext_bytes: MAX_CIPHERTEXT_BYTES as u32,
                supports_groups: false,
                supports_secrets: false,
                supports_owner_online_enrollment_v1: true,
            }),
            reply: Mutex::new(Value::Null),
            public: keys.public_keys().signing,
            device: id,
            nonces: Mutex::new(HashSet::new()),
            requests: Mutex::new(vec![]),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/base", listener.local_addr().unwrap());
        let router = Router::new().fallback(route).with_state(mock.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let tokens = Arc::new(MemoryTokenStore::new());
        let now = chrono::Utc::now();
        tokens
            .save(&TokenPair {
                session_id: SessionId::new(),
                access_token: SecretString::new(Uuid::new_v4().to_string()),
                refresh_token: SecretString::new(Uuid::new_v4().to_string()),
                access_expires_at: now + chrono::Duration::hours(1),
                refresh_expires_at: now + chrono::Duration::days(1),
            })
            .await
            .unwrap();
        let api = ApiClient::new(
            ApiConfig::new(&base, "0.1.14-enrollment-test", Platform::Cli).unwrap(),
            tokens,
        )
        .unwrap()
        .with_request_signer(Arc::new(Signer { id, keys }));
        Self { api, mock, task }
    }
    fn reply(&self, value: impl serde::Serialize) {
        *self.mock.reply.lock().unwrap() = serde_json::to_value(value).unwrap();
    }
    fn last(&self, method: &str, suffix: &str, body: Option<&impl serde::Serialize>) {
        let requests = self.mock.requests.lock().unwrap();
        let last = requests.last().unwrap();
        assert!(last.0 == method && last.1.ends_with(suffix));
        let expected = body
            .map(|body| request_body_sha256(&serde_json::to_vec(body).unwrap()))
            .unwrap_or_else(|| request_body_sha256(&[]));
        assert!(last.2 == expected);
    }
}
fn pending(flow: &CryptoFlow) -> OwnDeviceRequestState {
    OwnDeviceRequestState {
        grant_id: flow.grant.signed().grant.grant_id,
        request: flow.request.signed().clone(),
        endorsement: flow.endorsement.signed().clone(),
        status: OwnDeviceRequestStatus::Pending,
        challenge: None,
        response: None,
        acceptance: None,
    }
}

#[tokio::test]
async fn every_enrollment_route_binds_exact_proof_query_and_ciphertext_body() {
    let f = CryptoFlow::new(SharingRole::Reader, 2);
    let http = Http::new(&f).await;
    let api = http
        .api
        .sharing_enrollment(f.grant.signed().grant.scope.clone())
        .await
        .unwrap();
    let prefix = format!("/base/v1/shares/{}", api.scope().share_id);
    let grant_id = f.grant.signed().grant.grant_id;
    let page = OwnDevicesGrantPage {
        items: vec![f.grant.signed().clone()],
        next_after: None,
        has_more: false,
    };
    http.reply(&page);
    api.list_grants(None, 100).await.unwrap();
    http.last(
        "GET",
        &format!("{prefix}/own-device-grants?limit=100"),
        None::<&Value>,
    );
    http.reply(f.grant.signed());
    api.get_grant(grant_id).await.unwrap();
    http.last(
        "GET",
        &format!("{prefix}/own-device-grants/{grant_id}"),
        None::<&Value>,
    );
    http.reply(OwnDevicesGrantHistoryPage {
        states: vec![f.grant.signed().clone()],
        latest_revision: 1,
        has_more: false,
    });
    api.grant_history(grant_id, 0, 10).await.unwrap();
    http.last(
        "GET",
        &format!("{prefix}/own-device-grants/{grant_id}/history?after_revision=0&limit=10"),
        None::<&Value>,
    );
    let publish = PublishOwnDevicesGrantRequest {
        grant: f.grant.signed().clone(),
    };
    http.reply(&publish.grant);
    api.publish_grant(&publish).await.unwrap();
    http.last(
        "POST",
        &format!("{prefix}/own-device-grants"),
        Some(&publish),
    );
    let mut state = pending(&f);
    let id = state.request.request.request_id;
    http.reply(OwnDeviceRequestPage {
        items: vec![state.clone()],
        next_after: None,
        has_more: false,
    });
    api.list_requests(None, 20).await.unwrap();
    http.last(
        "GET",
        &format!("{prefix}/own-device-requests?limit=20"),
        None::<&Value>,
    );
    http.reply(&state);
    api.get_request(id).await.unwrap();
    http.last(
        "GET",
        &format!("{prefix}/own-device-requests/{id}"),
        None::<&Value>,
    );
    let submit = SubmitOwnDeviceRequest {
        grant_id,
        request: state.request.clone(),
        endorsement: state.endorsement.clone(),
    };
    api.submit_request(&submit).await.unwrap();
    http.last(
        "POST",
        &format!("{prefix}/own-device-requests"),
        Some(&submit),
    );
    let (challenge, _, proof) = f.proof();
    state.status = OwnDeviceRequestStatus::Challenged;
    state.challenge = Some(challenge.clone());
    http.reply(&state);
    let publish = PublishOwnDeviceChallengeRequest { challenge };
    api.publish_challenge(id, &publish).await.unwrap();
    http.last(
        "POST",
        &format!("{prefix}/own-device-requests/{id}/challenge"),
        Some(&publish),
    );
    state.status = OwnDeviceRequestStatus::Responded;
    state.response = Some(proof.response().signed().clone());
    http.reply(&state);
    let response = SubmitOwnDeviceChallengeResponseRequest {
        response: proof.response().signed().clone(),
    };
    api.submit_response(id, &response).await.unwrap();
    http.last(
        "POST",
        &format!("{prefix}/own-device-requests/{id}/response"),
        Some(&response),
    );
    let accepted = f.accepted(&proof, &[]);
    f.verify_accept(&accepted, &proof, &[]).unwrap();
    http.reply(OwnDeviceAcceptanceResult {
        state: SharedItemState {
            access: accepted.rotation.access.clone(),
            revision: accepted.rotation.revision.clone(),
        },
        acceptance: accepted.acceptance.clone(),
        consumed_grant_successor: accepted.consumed_grant_successor.clone(),
        other_grant_successors: vec![],
    });
    api.accept_request(id, &accepted).await.unwrap();
    http.last(
        "POST",
        &format!("{prefix}/own-device-requests/{id}/accept"),
        Some(&accepted),
    );
}

#[tokio::test]
async fn flag_off_keeps_owner_control_reads_and_terminal_revoke_only() {
    let f = CryptoFlow::new(SharingRole::Reader, 2);
    let http = Http::new(&f).await;
    http.mock
        .caps
        .lock()
        .unwrap()
        .supports_owner_online_enrollment_v1 = false;
    let api = http
        .api
        .sharing_enrollment(f.grant.signed().grant.scope.clone())
        .await
        .unwrap();
    let before = http.mock.requests.lock().unwrap().len();
    assert!(api
        .submit_request(&SubmitOwnDeviceRequest {
            grant_id: f.grant.signed().grant.grant_id,
            request: f.request.signed().clone(),
            endorsement: f.endorsement.signed().clone()
        })
        .await
        .is_err());
    assert!(api.list_requests(None, 10).await.is_err());
    assert!(api
        .get_request(f.request.signed().request.request_id)
        .await
        .is_err());
    assert!(api
        .publish_grant(&PublishOwnDevicesGrantRequest {
            grant: f.grant.signed().clone()
        })
        .await
        .is_err());
    assert!(http.mock.requests.lock().unwrap().len() == before);
    http.reply(f.grant.signed());
    api.get_grant(f.grant.signed().grant.grant_id)
        .await
        .unwrap();
    let mut revoke = f.grant.signed().grant.clone();
    revoke.grant_revision = 2;
    revoke.previous_grant_state_hash = Bytes::from(f.grant.hash());
    revoke.status = EnrollmentGrantStatus::Revoked;
    let grant = sign_grant_state(
        &f.owner,
        revoke,
        &f.pin,
        Some(&f.grant),
        GrantTransition::Revoke { access: &f.access },
        f.now,
    )
    .unwrap();
    http.reply(&grant);
    api.publish_grant(&PublishOwnDevicesGrantRequest { grant })
        .await
        .unwrap();
}

#[tokio::test]
async fn mismatched_context_gap_cursor_and_changed_signed_results_fail_closed() {
    let f = CryptoFlow::new(SharingRole::Reader, 2);
    let http = Http::new(&f).await;
    let api = http
        .api
        .sharing_enrollment(f.grant.signed().grant.scope.clone())
        .await
        .unwrap();
    for limit in [0, 101] {
        assert!(api.list_grants(None, limit).await.is_err());
    }
    assert!(api.get_grant(Uuid::nil()).await.is_err());
    assert!(api
        .grant_history(f.grant.signed().grant.grant_id, u64::MAX, 10)
        .await
        .is_err());
    let mut grant = f.grant.signed().clone();
    grant.grant.scope.server_instance_id = Uuid::new_v4();
    http.reply(grant);
    assert!(api
        .get_grant(f.grant.signed().grant.grant_id)
        .await
        .is_err());
    http.reply(OwnDevicesGrantHistoryPage {
        states: vec![f.grant.signed().clone()],
        latest_revision: 2,
        has_more: false,
    });
    assert!(api
        .grant_history(f.grant.signed().grant.grant_id, 0, 10)
        .await
        .is_err());
    http.reply(OwnDevicesGrantPage {
        items: vec![f.grant.signed().clone()],
        next_after: None,
        has_more: false,
    });
    assert!(api
        .list_grants(Some(f.grant.signed().grant.grant_id), 10)
        .await
        .is_err());
    let mut state = pending(&f);
    state.request.request.request_id = Uuid::new_v4();
    http.reply(state);
    assert!(api
        .get_request(f.request.signed().request.request_id)
        .await
        .is_err());
    let (_, _, proof) = f.proof();
    let accepted = f.accepted(&proof, &[]);
    let mut result = OwnDeviceAcceptanceResult {
        state: SharedItemState {
            access: accepted.rotation.access.clone(),
            revision: accepted.rotation.revision.clone(),
        },
        acceptance: accepted.acceptance.clone(),
        consumed_grant_successor: accepted.consumed_grant_successor.clone(),
        other_grant_successors: vec![],
    };
    result.state.revision.body.as_mut().unwrap().ciphertext = Bytes::from(vec![
        0;
        accepted
            .rotation
            .revision
            .body
            .as_ref()
            .unwrap()
            .ciphertext
            .len()
    ]);
    http.reply(result);
    assert!(api
        .accept_request(f.request.signed().request.request_id, &accepted)
        .await
        .is_err());
    http.mock.caps.lock().unwrap().server_instance_id = Uuid::new_v4();
    assert!(api.recheck_capabilities().await.is_err());
}
