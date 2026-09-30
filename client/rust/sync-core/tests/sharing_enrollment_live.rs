//! Opt-in enrollment against an isolated PostgreSQL-backed server. All
//! accounts/device keys/passwords are generated at runtime; no DB bypass,
//! real SMTP or private-key disk file is involved.
use base64::Engine as _;
use cc_crypto_core::sharing::*;
use cc_crypto_core::sharing_enrollment::*;
use cc_crypto_core::{request_body_sha256, DeviceSecretKeys};
use cc_protocol::auth::{LoginRequest, RegisterRequest, SecretString, VerifyEmailRequest};
use cc_protocol::devices::{DeviceProof, DeviceRegistration, RequestProof};
use cc_protocol::sharing::*;
use cc_protocol::sharing_enrollment::*;
use cc_protocol::version::Platform;
use cc_protocol::{Bytes, DeviceId, MutationId, ObjectId, ShareId, UserId};
use cc_sync_core::{ApiClient, ApiConfig, ApiError, MemoryTokenStore, RequestSigner, SignerError};
use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;
use zeroize::Zeroizing;
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

struct Signer {
    device_id: DeviceId,
    keys: Arc<DeviceSecretKeys>,
}

impl RequestSigner for Signer {
    fn request_proof(
        &self,
        method: &str,
        target: &str,
        body: &[u8],
    ) -> Result<RequestProof, SignerError> {
        let mut nonce = [0; 32];
        cc_crypto_core::fill_random(&mut nonce).map_err(|_| SignerError::Unavailable)?;
        let issued_at = chrono::Utc::now().timestamp();
        Ok(RequestProof {
            issued_at,
            nonce,
            signature: self.keys.sign_request(
                self.device_id,
                method,
                target,
                &request_body_sha256(body),
                issued_at,
                &nonce,
            ),
        })
    }
}

struct Participant {
    api: ApiClient,
    keys: Arc<DeviceSecretKeys>,
    user_id: UserId,
    device_id: DeviceId,
    email: String,
    password: Zeroizing<String>,
}

impl Participant {
    async fn register(url: &str) -> Self {
        let keys = Arc::new(DeviceSecretKeys::generate().unwrap());
        let device_id = DeviceId::new();
        let email = format!("sharing-{}@example.invalid", Uuid::new_v4().simple());
        let api = ApiClient::new(
            ApiConfig::new(url, "0.1.14-live-test", Platform::Cli).unwrap(),
            Arc::new(MemoryTokenStore::new()),
        )
        .unwrap()
        .with_request_signer(Arc::new(Signer {
            device_id,
            keys: keys.clone(),
        }));
        let issued_at = chrono::Utc::now().timestamp();
        let mut nonce = [0; 32];
        cc_crypto_core::fill_random(&mut nonce).unwrap();
        let password = Zeroizing::new(format!("Cc-{}-{}!", Uuid::new_v4(), Uuid::new_v4()));
        let response = api
            .register(&RegisterRequest {
                email: email.clone(),
                password: SecretString::new(password.as_str()),
                device: DeviceRegistration {
                    device_id,
                    name: "generated sharing test device".into(),
                    platform: Platform::Cli,
                    encryption_public_key: keys.public_keys().encryption_bytes(),
                    signing_public_key: keys.public_keys().signing_bytes(),
                    client_version: Some("0.1.14-test".into()),
                },
                device_proof: Some(DeviceProof {
                    issued_at,
                    nonce: Bytes::from(nonce),
                    signature: Bytes::from(keys.sign_device_login(device_id, issued_at, &nonce)),
                }),
            })
            .await
            .unwrap();
        Self {
            api,
            keys,
            user_id: response.user_id,
            device_id,
            email,
            password,
        }
    }

    fn member(&self, role: SharingRole) -> SharingMember {
        SharingMember {
            user_id: self.user_id,
            device_id: self.device_id,
            encryption_public_key: self.keys.public_keys().encryption_bytes(),
            signing_public_key: self.keys.public_keys().signing_bytes(),
            role,
        }
    }

    async fn verify_email(&self, directory: &Path) {
        let token = wait_verification_mail(directory, &self.email).await;
        assert!(self
            .api
            .email_verify(&VerifyEmailRequest { token })
            .await
            .expect("normal email verification must succeed")
            .is_none());
        let account = self
            .api
            .me()
            .await
            .expect("verified account must be readable");
        assert!(account.email_verified);
        assert!(account.user_id == self.user_id);
        assert!(account.current_device_id == self.device_id);
        assert!(account.email == self.email);
    }
}

fn revision(
    writer: &Participant,
    manifest: &VerifiedSharingManifest,
    context: SharingContext,
    plaintext: &[u8],
    previous: Option<&SharingRevisionCheckpoint>,
) -> SharedRevision {
    let body = seal_shared_revision(manifest, &context, plaintext).unwrap();
    let mutation = SharingMutation {
        base_revision: context.revision - 1,
        context,
        mutation_id: MutationId::new(),
        manifest_revision: manifest.manifest().revision,
        manifest_hash: Bytes::from(manifest.hash()),
        writer_device_id: writer.device_id,
        previous_revision_hash: Bytes::from(previous.map_or([0; 32], |p| p.hash)),
        operation: SharingOperation::Put,
        body_hash: Bytes::from(shared_body_hash(&body).unwrap()),
    };
    let signed =
        sign_shared_mutation(manifest, &writer.keys, mutation, Some(&body), previous).unwrap();
    SharedRevision {
        signed,
        body: Some(body),
    }
}

fn forbidden(result: Result<impl std::fmt::Debug, ApiError>, status: u16) {
    match result {
        Err(error) => assert_eq!(error.status(), Some(status)),
        Ok(_) => panic!("unauthorized sharing operation unexpectedly succeeded"),
    }
}

impl Participant {
    async fn next_device(&self, url: &str) -> Self {
        let keys = Arc::new(DeviceSecretKeys::generate().unwrap());
        let device_id = DeviceId::new();
        let api = ApiClient::new(
            ApiConfig::new(url, "0.1.14-enrollment-test", Platform::Cli).unwrap(),
            Arc::new(MemoryTokenStore::new()),
        )
        .unwrap()
        .with_request_signer(Arc::new(Signer {
            device_id,
            keys: keys.clone(),
        }));
        let issued_at = chrono::Utc::now().timestamp();
        let mut nonce = [0; 32];
        cc_crypto_core::fill_random(&mut nonce).unwrap();
        let response = api
            .login(&LoginRequest {
                email: self.email.clone(),
                password: SecretString::new(self.password.as_str()),
                device: DeviceRegistration {
                    device_id,
                    name: "generated second enrollment device".into(),
                    platform: Platform::Cli,
                    encryption_public_key: keys.public_keys().encryption_bytes(),
                    signing_public_key: keys.public_keys().signing_bytes(),
                    client_version: Some("0.1.14-enrollment-test".into()),
                },
                device_proof: Some(DeviceProof {
                    issued_at,
                    nonce: Bytes::from(nonce),
                    signature: Bytes::from(keys.sign_device_login(device_id, issued_at, &nonce)),
                }),
            })
            .await
            .expect("normal same-account login must succeed");
        assert!(response.user_id == self.user_id && response.device_id == device_id);
        Self {
            api,
            keys,
            device_id,
            user_id: response.user_id,
            email: self.email.clone(),
            password: Zeroizing::new(self.password.to_string()),
        }
    }
    fn binding(&self) -> EnrollmentDeviceBinding {
        EnrollmentDeviceBinding {
            user_id: self.user_id,
            device_id: self.device_id,
            encryption_public_key: self.keys.public_keys().encryption_bytes(),
            signing_public_key: self.keys.public_keys().signing_bytes(),
        }
    }
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL server with sharing/enrollment enabled and private FileMailer"]
async fn live_owner_online_enrollment_verifies_accounts_pairing_possession_and_fresh_rotation() {
    let url = std::env::var("CC_SHARING_TEST_SERVER_URL").expect("isolated loopback URL required");
    let parsed = url::Url::parse(&url).unwrap();
    assert!(
        parsed.scheme() == "http" && parsed.host_str() == Some("127.0.0.1"),
        "live test must use isolated loopback HTTP"
    );
    let mail =
        std::env::var("CC_SHARING_TEST_MAIL_DIR").expect("private FileMailer directory required");
    let owner = Participant::register(&url).await;
    let anchor = Participant::register(&url).await;
    owner.verify_email(Path::new(&mail)).await;
    anchor.verify_email(Path::new(&mail)).await;
    let target = anchor.next_device(&url).await;
    let caps = owner.api.sharing_capabilities().await.unwrap();
    assert!(caps.enabled && caps.supports_owner_online_enrollment_v1);
    let context = SharingContext {
        server_instance_id: caps.server_instance_id,
        share_id: ShareId::new(),
        item_id: ObjectId::new(),
        revision: 1,
        access_epoch: 1,
        kind: SharedItemKind::Snippet,
    };
    let pin = SharingOwnerAnchor {
        user_id: owner.user_id,
        device_id: owner.device_id,
        public_keys: owner.keys.public_keys(),
    };
    let access = AccessManifest {
        format: 1,
        server_instance_id: context.server_instance_id,
        share_id: context.share_id,
        item_id: context.item_id,
        owner_user_id: owner.user_id,
        owner_device_id: owner.device_id,
        revision: 1,
        access_epoch: 1,
        previous_manifest_hash: Bytes::from([0; 32]),
        kind: context.kind,
        members: vec![
            owner.member(SharingRole::Editor),
            anchor.member(SharingRole::Reader),
        ],
    };
    let signed = sign_shared_manifest(&owner.keys, access, &context, &pin, None).unwrap();
    let before = verify_shared_manifest(&signed, &context, &pin, None).unwrap();
    let plaintext =
        Zeroizing::new(format!("runtime-enrollment-content-{}", Uuid::new_v4()).into_bytes());
    let original = revision(&owner, &before, context.clone(), &plaintext, None);
    let previous = verify_shared_mutation(&before, &original.signed, None).unwrap();
    let sharing = owner.api.sharing(context.server_instance_id).await.unwrap();
    sharing
        .create_share(&CreateShareRequest {
            access: signed,
            revision: original.clone(),
        })
        .await
        .unwrap();
    forbidden(
        target
            .api
            .sharing(context.server_instance_id)
            .await
            .unwrap()
            .get_share(context.share_id)
            .await,
        404,
    );
    assert!(open_shared_revision(
        &before,
        &previous,
        original.body.as_ref(),
        target.device_id,
        &target.keys
    )
    .is_err());
    let scope = EnrollmentScope {
        server_instance_id: context.server_instance_id,
        share_id: context.share_id,
        item_id: context.item_id,
        kind: context.kind,
    };
    let owner_api = owner.api.sharing_enrollment(scope.clone()).await.unwrap();
    let target_api = target.api.sharing_enrollment(scope.clone()).await.unwrap();
    let now = chrono::Utc::now().timestamp();
    let grant = SharingOwnDevicesGrantState {
        format: 1,
        scope: scope.clone(),
        owner_user_id: owner.user_id,
        owner_device_id: owner.device_id,
        grant_id: Uuid::new_v4(),
        grant_revision: 1,
        previous_grant_state_hash: Bytes::from([0; 32]),
        status: EnrollmentGrantStatus::Active,
        anchor: anchor.binding(),
        access_manifest_hash: Bytes::from(before.hash()),
        access_epoch: 1,
        role_ceiling: SharingRole::Reader,
        mode: EnrollmentMode::Manual,
        not_before: now,
        expires_at: now + 3600,
        max_admissions: 1,
        admitted_count: 0,
    };
    let signed = sign_grant_state(
        &owner.keys,
        grant,
        &pin,
        None,
        GrantTransition::Genesis { access: &before },
        now,
    )
    .unwrap();
    let grant = verify_grant_state(
        &signed,
        &pin,
        None,
        GrantTransition::Genesis { access: &before },
        now,
    )
    .unwrap();
    owner_api
        .publish_grant(&PublishOwnDevicesGrantRequest { grant: signed })
        .await
        .unwrap();
    let mut nonce = [0; 32];
    cc_crypto_core::fill_random(&mut nonce).unwrap();
    let request = SharingOwnDeviceRequest {
        format: 1,
        scope,
        request_id: Uuid::new_v4(),
        grant_state_hash: Bytes::from(grant.hash()),
        access_manifest_hash: Bytes::from(before.hash()),
        access_epoch: 1,
        target: target.binding(),
        requested_role: SharingRole::Reader,
        nonce: Bytes::from(nonce),
        not_before: now,
        expires_at: now + 600,
    };
    let signed = sign_device_request(&target.keys, request, &grant, &before, now).unwrap();
    let request = verify_device_request(&signed, &grant, &before, now).unwrap();
    let signed_endorsement = endorse_device_request(
        &anchor.keys,
        &request,
        &before,
        &enrollment_pairing_code(&request).unwrap(),
        now,
    )
    .unwrap();
    let endorsement =
        verify_anchor_endorsement(&signed_endorsement, &request, &before, now).unwrap();
    target_api
        .submit_request(&SubmitOwnDeviceRequest {
            grant_id: grant.signed().grant.grant_id,
            request: request.signed().clone(),
            endorsement: signed_endorsement,
        })
        .await
        .unwrap();
    let (challenge, pending) = create_device_challenge(
        &owner.keys,
        &pin,
        &request,
        &endorsement,
        &before,
        now,
        None,
    )
    .unwrap();
    owner_api
        .publish_challenge(
            request.signed().request.request_id,
            &PublishOwnDeviceChallengeRequest {
                challenge: challenge.clone(),
            },
        )
        .await
        .unwrap();
    let observed = target_api
        .get_request(request.signed().request.request_id)
        .await
        .unwrap();
    assert!(observed.challenge.as_ref() == Some(&challenge));
    let verified =
        verify_device_challenge(&challenge, &pin, &request, &endorsement, &before, now, None)
            .unwrap();
    let response = answer_device_challenge(
        &target.keys,
        &pin,
        &request,
        &endorsement,
        &before,
        &challenge,
        now,
        None,
    )
    .unwrap();
    target_api
        .submit_response(
            request.signed().request.request_id,
            &SubmitOwnDeviceChallengeResponseRequest {
                response: response.clone(),
            },
        )
        .await
        .unwrap();
    let proof = verify_device_possession(
        &response,
        &pending,
        &request,
        &before,
        now,
        &verified.checkpoint(),
    )
    .unwrap();
    let accepted = accept_own_device(
        &owner.keys,
        &pin,
        &before,
        &previous,
        original.body.as_ref().unwrap(),
        &request,
        &endorsement,
        &proof,
        &verified.checkpoint(),
        &grant,
        &[],
        now,
    )
    .unwrap();
    let result = owner_api
        .accept_request(request.signed().request.request_id, &accepted)
        .await
        .unwrap();
    assert!(
        result.acceptance == accepted.acceptance
            && result.consumed_grant_successor.grant.status == EnrollmentGrantStatus::Revoked
    );
    let current = target
        .api
        .sharing(context.server_instance_id)
        .await
        .unwrap()
        .get_share(context.share_id)
        .await
        .unwrap();
    assert!(current == result.state);
    let after = verify_shared_manifest(
        &current.access,
        &current.revision.signed.mutation.context,
        &pin,
        Some(&before.checkpoint()),
    )
    .unwrap();
    let latest = verify_shared_mutation(
        &after,
        &current.revision.signed,
        Some(&previous.checkpoint()),
    )
    .unwrap();
    let decrypted = open_shared_revision(
        &after,
        &latest,
        current.revision.body.as_ref(),
        target.device_id,
        &target.keys,
    )
    .unwrap()
    .unwrap();
    assert!(decrypted.as_slice() == plaintext.as_slice());
    assert!(
        current.revision.body.as_ref().unwrap().ciphertext
            != original.body.as_ref().unwrap().ciphertext
    );
    assert!(
        after.manifest().members.len() == before.manifest().members.len() + 1
            && after.member(target.device_id).unwrap().role == SharingRole::Reader
    );
    let history = owner_api
        .grant_history(grant.signed().grant.grant_id, 0, 100)
        .await
        .unwrap();
    assert!(history.states.len() == 2 && history.states[1] == result.consumed_grant_successor);
    let state = owner_api
        .get_request(request.signed().request.request_id)
        .await
        .unwrap();
    assert!(
        state.status == OwnDeviceRequestStatus::Accepted
            && state.acceptance == Some(result.acceptance)
    );
    assert!(target_api
        .accept_request(request.signed().request.request_id, &accepted)
        .await
        .is_err());
}
