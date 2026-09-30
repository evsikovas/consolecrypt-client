//! Opt-in real PostgreSQL-backed server verification. Run only against a
//! disposable sharing-enabled server bound to loopback. All identities and
//! passwords are generated at runtime; no external account is contacted.
//! Set CC_SHARING_TEST_MAIL_DIR to the server's private FileMailer directory;
//! verification uses delivered MIME tokens and the normal email/verify API.

use base64::Engine as _;
use cc_crypto_core::sharing::{
    open_shared_revision, seal_shared_revision, shared_body_hash, sign_shared_manifest,
    sign_shared_mutation, verify_shared_manifest, verify_shared_mutation, SharingOwnerAnchor,
    SharingRevisionCheckpoint, VerifiedSharingManifest,
};
use cc_crypto_core::{request_body_sha256, DeviceSecretKeys};
use cc_protocol::auth::{RegisterRequest, SecretString, VerifyEmailRequest};
use cc_protocol::devices::{DeviceProof, DeviceRegistration, RequestProof};
use cc_protocol::sharing::{
    AccessManifest, CreateShareRequest, PutSharedRevisionRequest, RotateShareAccessRequest,
    SharedItemKind, SharedRevision, SharingContext, SharingMember, SharingMutation,
    SharingOperation, SharingRole,
};
use cc_protocol::version::Platform;
use cc_protocol::{Bytes, DeviceId, ErrorCode, MutationId, ObjectId, ShareId, UserId};
use cc_sync_core::api::SharingApi;
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
        let response = api
            .register(&RegisterRequest {
                email: email.clone(),
                password: SecretString::new(format!("Cc-{}-{}!", Uuid::new_v4(), Uuid::new_v4())),
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

async fn exercise_item(
    owner: &Participant,
    editor: &Participant,
    reader: &Participant,
    owner_api: &SharingApi,
    editor_api: &SharingApi,
    reader_api: &SharingApi,
    kind: SharedItemKind,
) {
    let mut context = SharingContext {
        server_instance_id: owner_api.capabilities().server_instance_id,
        share_id: ShareId::new(),
        item_id: ObjectId::new(),
        revision: 1,
        access_epoch: 1,
        kind,
    };
    // In a real UI these public keys are accepted only after the humans
    // compare codes. The test controls every generated identity directly.
    let anchor = SharingOwnerAnchor {
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
        kind,
        members: vec![
            owner.member(SharingRole::Editor),
            editor.member(SharingRole::Editor),
            reader.member(SharingRole::Reader),
        ],
    };
    let signed_access = sign_shared_manifest(&owner.keys, access, &context, &anchor, None).unwrap();
    let manifest = verify_shared_manifest(&signed_access, &context, &anchor, None).unwrap();
    let payload: &[u8] = match kind {
        SharedItemKind::Host => {
            br#"{"name":"Shared test host","address":"192.0.2.10","username":"operator","port":22}"#
        }
        SharedItemKind::Snippet => {
            br#"{"name":"Disk usage","template":"df -h","risk":"read_only"}"#
        }
        _ => panic!("unsupported live fixture kind"),
    };
    let initial = revision(owner, &manifest, context.clone(), payload, None);
    owner_api
        .create_share(&CreateShareRequest {
            access: signed_access,
            revision: initial,
        })
        .await
        .unwrap();
    let fetched = reader_api.get_share(context.share_id).await.unwrap();
    let received_manifest =
        verify_shared_manifest(&fetched.access, &context, &anchor, None).unwrap();
    let accepted =
        verify_shared_mutation(&received_manifest, &fetched.revision.signed, None).unwrap();
    assert!(
        open_shared_revision(
            &received_manifest,
            &accepted,
            fetched.revision.body.as_ref(),
            reader.device_id,
            &reader.keys
        )
        .unwrap()
        .unwrap()
        .as_slice()
            == payload
    );
    let first_checkpoint = accepted.checkpoint();
    let first_manifest_checkpoint = received_manifest.checkpoint();

    // A reader bypasses the safe signer with its own valid device signature.
    // The real server must enforce the same read/edit separation as the core.
    let mut illegal = fetched.revision.clone();
    illegal.signed.mutation.writer_device_id = reader.device_id;
    illegal.signed.mutation.context.revision = 2;
    illegal.signed.mutation.base_revision = 1;
    illegal.signed.mutation.mutation_id = MutationId::new();
    illegal.signed.mutation.previous_revision_hash = Bytes::from(first_checkpoint.hash);
    // A hostile client bypasses the safe signer, but only owns its reader key.
    // The runtime-generated seed is borrowed from a zeroizing memory buffer.
    use ed25519_dalek::Signer as _;
    let secret_blob = reader.keys.to_secret_bytes();
    let seed: &[u8; 32] = secret_blob[37..69].try_into().unwrap();
    let message = cc_protocol::sharing::sharing_mutation_message(&illegal.signed.mutation).unwrap();
    illegal.signed.signature = Bytes::from(
        ed25519_dalek::SigningKey::from_bytes(seed)
            .sign(&message)
            .to_bytes(),
    );
    forbidden(
        reader_api
            .put_shared_revision(
                context.share_id,
                &PutSharedRevisionRequest { revision: illegal },
            )
            .await,
        403,
    );

    context.revision = 2;
    let edited = revision(
        editor,
        &manifest,
        context.clone(),
        b"edited sanitized projection",
        Some(&first_checkpoint),
    );
    let request = PutSharedRevisionRequest {
        revision: edited.clone(),
    };
    editor_api
        .put_shared_revision(context.share_id, &request)
        .await
        .unwrap();
    // Lost-response recovery cannot overwrite a newer revision: a retry is a
    // CAS conflict; reading the exact signed mutation proves it was accepted.
    forbidden(
        editor_api
            .put_shared_revision(context.share_id, &request)
            .await,
        409,
    );
    let fetched = owner_api.get_share(context.share_id).await.unwrap();
    assert!(fetched.revision == edited);
    let accepted =
        verify_shared_mutation(&manifest, &fetched.revision.signed, Some(&first_checkpoint))
            .unwrap();
    let edited_checkpoint = accepted.checkpoint();
    assert!(
        open_shared_revision(
            &manifest,
            &accepted,
            fetched.revision.body.as_ref(),
            owner.device_id,
            &owner.keys
        )
        .unwrap()
        .unwrap()
        .as_slice()
            == b"edited sanitized projection"
    );

    // Revoke the reader and freshly encrypt the current content atomically.
    let mut rotated = manifest.manifest().clone();
    rotated.revision = 2;
    rotated.access_epoch = 2;
    rotated.previous_manifest_hash = Bytes::from(manifest.hash());
    rotated
        .members
        .retain(|member| member.device_id != reader.device_id);
    context.revision = 3;
    context.access_epoch = 2;
    let access = sign_shared_manifest(
        &owner.keys,
        rotated,
        &context,
        &anchor,
        Some(&first_manifest_checkpoint),
    )
    .unwrap();
    let next = verify_shared_manifest(&access, &context, &anchor, Some(&first_manifest_checkpoint))
        .unwrap();
    let rotated_revision = revision(
        owner,
        &next,
        context.clone(),
        b"edited sanitized projection",
        Some(&edited_checkpoint),
    );
    owner_api
        .rotate_shared_access(
            context.share_id,
            &RotateShareAccessRequest {
                access,
                revision: rotated_revision,
            },
        )
        .await
        .unwrap();
    forbidden(reader_api.get_share(context.share_id).await, 404);
    forbidden(
        reader_api.share_history(context.share_id, 0, 0, 100).await,
        404,
    );
    forbidden(
        editor_api
            .put_shared_revision(context.share_id, &request)
            .await,
        409,
    );

    // Bridge the reader/editor's former local state using signed header-only
    // history, not old ciphertext or former recipient envelopes.
    let history = editor_api
        .share_history(context.share_id, 1, 1, 100)
        .await
        .unwrap();
    assert_eq!((history.manifests.len(), history.revisions.len()), (1, 2));
    let mut manifests = BTreeMap::new();
    manifests.insert(1, received_manifest.clone());
    let mut manifest_checkpoint = first_manifest_checkpoint;
    for signed in history.manifests {
        let mut expected = context.clone();
        expected.access_epoch = signed.manifest.access_epoch;
        let verified =
            verify_shared_manifest(&signed, &expected, &anchor, Some(&manifest_checkpoint))
                .unwrap();
        manifest_checkpoint = verified.checkpoint();
        manifests.insert(verified.manifest().revision, verified);
    }
    let mut checkpoint = first_checkpoint;
    for signed in history.revisions {
        let access = manifests.get(&signed.mutation.manifest_revision).unwrap();
        checkpoint = verify_shared_mutation(access, &signed, Some(&checkpoint))
            .unwrap()
            .checkpoint();
    }
    let latest = editor_api.get_share(context.share_id).await.unwrap();
    let manifest = manifests.get(&latest.access.manifest.revision).unwrap();
    let verified =
        verify_shared_mutation(manifest, &latest.revision.signed, Some(&checkpoint)).unwrap();
    assert!(
        open_shared_revision(
            manifest,
            &verified,
            latest.revision.body.as_ref(),
            editor.device_id,
            &editor.keys
        )
        .unwrap()
        .unwrap()
        .as_slice()
            == b"edited sanitized projection"
    );
    assert!(open_shared_revision(
        manifest,
        &verified,
        latest.revision.body.as_ref(),
        reader.device_id,
        &reader.keys
    )
    .is_err());
}

#[tokio::test]
#[ignore = "requires a disposable sharing-enabled loopback server with private FileMailer"]
async fn real_server_crypto_two_users_reader_editor_rotation_history_and_cas() {
    let server = std::env::var("CC_SHARING_TEST_SERVER_URL")
        .expect("CC_SHARING_TEST_SERVER_URL is required");
    let url = url::Url::parse(&server).unwrap();
    let loopback = match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        None => false,
    };
    assert!(
        loopback,
        "live sharing tests require a disposable loopback server"
    );
    let mail_directory = std::env::var_os("CC_SHARING_TEST_MAIL_DIR")
        .map(std::path::PathBuf::from)
        .expect("CC_SHARING_TEST_MAIL_DIR is required for normal email verification");
    let owner = Participant::register(&server).await;
    let editor = Participant::register(&server).await;
    let reader = Participant::register(&server).await;
    let capabilities = owner.api.sharing_capabilities().await.unwrap();
    let owner_api = owner
        .api
        .sharing(capabilities.server_instance_id)
        .await
        .unwrap();
    let editor_api = editor
        .api
        .sharing(capabilities.server_instance_id)
        .await
        .unwrap();
    let reader_api = reader
        .api
        .sharing(capabilities.server_instance_id)
        .await
        .unwrap();
    owner_api.recheck_capabilities().await.unwrap();
    // Discovery intentionally requires verified email even when the
    // disposable server allows unverified test-account registration.
    let discovery = owner_api
        .sharing_recipient(&reader.email)
        .await
        .unwrap_err();
    assert!(discovery.is_code(ErrorCode::EmailNotVerified));
    for participant in [&owner, &editor, &reader] {
        participant.verify_email(&mail_directory).await;
    }
    for recipient in [&editor, &reader] {
        let discovered = owner_api
            .sharing_recipient(&recipient.email)
            .await
            .expect("verified recipient discovery must succeed");
        // Discovery must bind exact generated account/device/public keys.
        // These directory keys still require independent human approval.
        assert!(discovered.user_id == recipient.user_id);
        assert_eq!(discovered.devices.len(), 1);
        assert!(discovered.devices[0] == recipient.member(SharingRole::Reader));
    }
    for kind in [SharedItemKind::Host, SharedItemKind::Snippet] {
        exercise_item(
            &owner,
            &editor,
            &reader,
            &owner_api,
            &editor_api,
            &reader_api,
            kind,
        )
        .await;
    }
    assert_eq!(
        owner_api.list_shares(None, 100).await.unwrap().items.len(),
        2
    );
    assert_eq!(
        editor_api.list_shares(None, 100).await.unwrap().items.len(),
        2
    );
    assert!(reader_api
        .list_shares(None, 100)
        .await
        .unwrap()
        .items
        .is_empty());
    owner.api.refresh().await.unwrap();
    owner_api.recheck_capabilities().await.unwrap();
}

#[test]
fn private_mail_parser_matches_to_and_decodes_runtime_token_without_logging_it() {
    let recipient = format!("sharing-{}@example.invalid", Uuid::new_v4().simple());
    let other = format!("sharing-{}@example.invalid", Uuid::new_v4().simple());
    let token = SecretString::new(format!("cct_{}", Uuid::new_v4().simple()));
    let (first, second) = token.expose_secret().split_at(20);
    let quoted = Zeroizing::new(format!(
        "=D0=9F=D1=80=D0=B8=D0=B2=D0=B5=D1=82\r\n    {first}=\r\n{second}\r\n"
    ));
    let body = Zeroizing::new(format!("Привет\r\n    {}\r\n", token.expose_secret()));
    let encoded = Zeroizing::new(base64::engine::general_purpose::STANDARD.encode(body.as_bytes()));
    for (encoding, body) in [
        ("quoted-printable", quoted.as_str()),
        ("base64", encoded.as_str()),
    ] {
        let wire = Zeroizing::new(format!(
            "To: {recipient}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: {encoding}\r\n\r\n{body}"
        ));
        let parsed = verification_mail_token(wire.as_bytes(), &recipient)
            .expect("runtime verification MIME must decode")
            .expect("matching recipient must yield token");
        assert!(parsed.expose_secret() == token.expose_secret());
        assert!(verification_mail_token(wire.as_bytes(), &other)
            .expect("different recipient is not an error")
            .is_none());
    }
    assert!(decode_quoted_printable(b"broken=QZ").is_err());
    assert!(decode_quoted_printable(b"broken=").is_err());
}
