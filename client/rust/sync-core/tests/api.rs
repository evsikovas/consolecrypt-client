//! ApiClient + mock server: every endpoint group, auth/token handling,
//! authorization rules and error mapping.

mod common;

use cc_protocol::auth::{
    ChangePasswordRequest, ForgotPasswordRequest, LoginRequest, LogoutRequest,
    ResetPasswordRequest, SecretString, VerifyEmailRequest,
};
use cc_protocol::canonical::device_approval_message;
use cc_protocol::devices::{
    ApproveDeviceRequest, CreateDeviceTrustRequest, DeviceRequestStatus, RejectDeviceRequest,
    UpdateDeviceRequest, VaultEnvelope,
};
use cc_protocol::envelopes::RecipientType;
use cc_protocol::recovery::ReplaceEnvelopeRequest;
use cc_protocol::vaults::{DeleteEnvelopeRequest, DeleteVaultRequest};
use cc_protocol::version::{Platform, ProtocolVersion};
use cc_protocol::{paths, Bytes, ErrorCode, VaultId};
use cc_sync_core::mock::{MockServer, MockServerConfig};
use cc_sync_core::*;
use common::*;
use ed25519_dalek::Signer;
use std::sync::Arc;

fn email() -> String {
    format!("user-{}@example.test", uuid::Uuid::new_v4().simple())
}

#[tokio::test]
async fn meta_register_login_me_logout() {
    let server = MockServer::start().await;
    let api = api_for(&server);
    let meta = api.meta().await.unwrap();
    assert_eq!(meta.protocol_version, cc_protocol::PROTOCOL_VERSION);
    assert!(!meta.upgrade_required);

    let ka = DeviceKeys::generate();
    let mail = email();
    let (api_a, acct) = register(&server, &mail, &ka).await;
    let me = api_a.me().await.unwrap();
    assert_eq!(
        (me.user_id, me.current_device_id),
        (acct.user_id, ka.device_id)
    );
    assert!(!me.email_verified);

    // Email verification with the token the server "emailed".
    let token = server.email_tokens(&mail).pop().unwrap();
    assert!(api
        .email_verify(&VerifyEmailRequest {
            token: SecretString::new(token)
        })
        .await
        .unwrap()
        .is_none());
    assert!(api_a.me().await.unwrap().email_verified);

    // Wrong password is indistinguishable from unknown user.
    let bad = api
        .login(&LoginRequest {
            email: mail.clone(),
            password: SecretString::new("wrong password!!"),
            device: ka.registration("x"),
            device_proof: Some(ka.proof()),
        })
        .await
        .unwrap_err();
    assert!(bad.is_code(ErrorCode::InvalidCredentials), "{bad}");

    api_a.logout(&LogoutRequest::default()).await.unwrap();
    assert!(matches!(api_a.me().await, Err(ApiError::NotAuthenticated)));
    assert!(!api_a.is_authenticated().await.unwrap());
}

#[tokio::test]
async fn password_change_forgot_and_reset() {
    let server = MockServer::start().await;
    let ka = DeviceKeys::generate();
    let mail = email();
    let (api_a, _) = register(&server, &mail, &ka).await;
    let err = api_a
        .password_change(&ChangePasswordRequest {
            current_password: SecretString::new("not the password"),
            new_password: SecretString::new("another long password"),
        })
        .await
        .unwrap_err();
    assert!(err.is_code(ErrorCode::InvalidCredentials));
    api_a
        .password_change(&ChangePasswordRequest {
            current_password: SecretString::new(PASSWORD),
            new_password: SecretString::new("another long password"),
        })
        .await
        .unwrap();

    let anon = api_for(&server);
    let before = server.email_tokens(&mail);
    anon.password_forgot(&ForgotPasswordRequest {
        email: mail.clone(),
    })
    .await
    .unwrap();
    // Unknown emails are accepted too (no account enumeration).
    anon.recovery_account_start(&ForgotPasswordRequest {
        email: "nobody@example.test".into(),
    })
    .await
    .unwrap();
    let reset_token = server
        .email_tokens(&mail)
        .into_iter()
        .find(|t| !before.contains(t))
        .unwrap();
    anon.password_reset(&ResetPasswordRequest {
        token: SecretString::new(reset_token),
        new_password: SecretString::new("brand new long password"),
    })
    .await
    .unwrap();
    // Reset revoked every session.
    assert!(
        matches!(api_a.me().await, Err(ApiError::SessionExpired)),
        "sessions revoked"
    );
    assert!(!api_a.is_authenticated().await.unwrap());
    let api = api_for(&server);
    api.login(&LoginRequest {
        email: mail,
        password: SecretString::new("brand new long password"),
        device: ka.registration("A"),
        device_proof: Some(ka.proof()),
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn expired_access_token_is_refreshed_once_for_concurrent_requests() {
    let server = MockServer::start().await;
    let ka = DeviceKeys::generate();
    let (api, _) = register(&server, &email(), &ka).await;
    server.expire_access_tokens();
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let api = api.clone();
        tasks.push(tokio::spawn(async move { api.me().await }));
    }
    for t in tasks {
        t.await.unwrap().unwrap();
    }
    assert_eq!(server.hits(paths::AUTH_REFRESH), 1, "single-flight refresh");

    // Server-side rejection while the client still considers the token
    // valid: 401 → refresh → retry with the new token.
    server.expire_access_tokens();
    api.list_devices().await.unwrap();
    assert_eq!(server.hits(paths::AUTH_REFRESH), 2);
}

#[tokio::test]
async fn refresh_token_reuse_revokes_the_session() {
    let server = MockServer::start().await;
    let ka = DeviceKeys::generate();
    let store = Arc::new(MemoryTokenStore::new());
    let api = ApiClient::new(
        ApiConfig::new(server.url().as_str(), "t", Platform::Cli).unwrap(),
        store.clone(),
    )
    .unwrap();
    api.register(&cc_protocol::auth::RegisterRequest {
        email: email(),
        password: SecretString::new(PASSWORD),
        device: ka.registration("A"),
        device_proof: Some(ka.proof()),
    })
    .await
    .unwrap();
    let stolen = store.load().await.unwrap().unwrap();
    // Legitimate rotation invalidates the old pair (incl. the access token).
    let fresh = api.refresh().await.unwrap();
    assert_ne!(
        fresh.refresh_token.expose_secret(),
        stolen.refresh_token.expose_secret()
    );

    // An attacker replays the old refresh token.
    let attacker = api_for(&server);
    attacker.set_tokens(&stolen).await.unwrap();
    let err = attacker.refresh().await.unwrap_err();
    assert!(matches!(err, ApiError::RefreshTokenReused), "{err}");
    assert!(err.requires_reauth());
    assert!(
        !attacker.is_authenticated().await.unwrap(),
        "tokens cleared"
    );

    // The whole family is revoked: the legitimate client must sign in again.
    let err = api.me().await.unwrap_err();
    assert!(err.requires_reauth(), "{err}");
    assert!(store.load().await.unwrap().is_none());
}

#[tokio::test]
async fn device_identity_errors_on_login() {
    let server = MockServer::start().await;
    let ka = DeviceKeys::generate();
    let mail = email();
    let (api_a, _) = register(&server, &mail, &ka).await;
    let kb = DeviceKeys::generate();
    login(&server, &mail, &kb).await;

    // Same device id, different keys → conflict.
    let mut forged = DeviceKeys::generate();
    forged.device_id = kb.device_id;
    let err = api_for(&server)
        .login(&LoginRequest {
            email: mail.clone(),
            password: SecretString::new(PASSWORD),
            device: forged.registration("forged"),
            device_proof: Some(forged.proof()),
        })
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::DeviceIdentityConflict), "{err}");
    assert!(err.requires_new_device_identity());

    // Device id (with its real keys) owned by another account → conflict.
    let other_mail = email();
    register(&server, &other_mail, &DeviceKeys::generate()).await;
    let err = api_for(&server)
        .login(&LoginRequest {
            email: other_mail,
            password: SecretString::new(PASSWORD),
            device: ka.registration("x"),
            device_proof: Some(ka.proof()),
        })
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::DeviceIdentityConflict), "{err}");

    // Revoked device id → must generate a new identity.
    api_a
        .revoke_device(kb.device_id, &Default::default())
        .await
        .unwrap();
    let err = api_for(&server)
        .login(&LoginRequest {
            email: mail,
            password: SecretString::new(PASSWORD),
            device: kb.registration("B again"),
            device_proof: Some(kb.proof()),
        })
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::DeviceIdentityRevoked), "{err}");
    assert!(err.requires_new_device_identity());
}

#[tokio::test]
async fn upgrade_required_is_mapped() {
    let server = MockServer::start_with(MockServerConfig {
        minimum_protocol: ProtocolVersion::new(1, 99),
        ..Default::default()
    })
    .await;
    let api = api_for(&server);
    assert!(api.meta().await.unwrap().upgrade_required);
    let err = api
        .register(&cc_protocol::auth::RegisterRequest {
            email: email(),
            password: SecretString::new(PASSWORD),
            device: DeviceKeys::generate().registration("A"),
            device_proof: None,
        })
        .await
        .unwrap_err();
    match err {
        ApiError::UpgradeRequired { server: Some(info) } => {
            assert_eq!(info.minimum_supported_protocol, ProtocolVersion::new(1, 99))
        }
        e => panic!("{e}"),
    }
}

#[tokio::test]
async fn vaults_envelopes_recovery_and_authorization() {
    let server = MockServer::start().await;
    let ka = DeviceKeys::generate();
    let mail = email();
    let (api_a, _) = register(&server, &mail, &ka).await;
    let vault = create_vault(&api_a, ka.device_id).await;

    // Existing vault id → 409 already_exists (enable-sync reconnect keys off this).
    let err = api_a
        .create_vault(&create_vault_request(
            vault.vault_id,
            &vault.vak,
            ka.device_id,
        ))
        .await
        .unwrap_err();
    assert!(err.is_code(ErrorCode::AlreadyExists), "{err}");

    let list = api_a.list_vaults().await.unwrap();
    assert_eq!(list.vaults.len(), 1);
    assert!(list.vaults[0].caller_trusted);

    // Untrusted device of the same account sees only what it needs to unlock.
    let kb = DeviceKeys::generate();
    let api_b = login(&server, &mail, &kb).await;
    assert!(
        !api_b
            .get_vault(vault.vault_id)
            .await
            .unwrap()
            .caller_trusted
    );
    let envs = api_b
        .list_envelopes(vault.vault_id)
        .await
        .unwrap()
        .envelopes;
    let types: Vec<_> = envs.iter().map(|e| e.recipient_type).collect();
    assert_eq!(envs.len(), 2);
    assert!(types.contains(&RecipientType::Password) && types.contains(&RecipientType::Recovery));
    let material = api_b.recovery_vault_envelope(vault.vault_id).await.unwrap();
    assert!(material.password_envelope.is_some() && material.recovery_envelope.is_some());
    assert!(material.device_envelope.is_none());
    // …and cannot sync.
    let err = api_b
        .changes(&cc_protocol::sync::ChangesQuery {
            vault_id: vault.vault_id,
            after: 0,
            limit: None,
        })
        .await
        .unwrap_err();
    assert!(err.is_code(ErrorCode::DeviceNotTrusted), "{err}");

    // Attestation with a wrong access key fails, with the right one succeeds.
    let mut wrong = attest_request(&vault, kb.device_id);
    wrong.vault_access_key = Bytes::new(random_bytes(32));
    let err = api_b.attest_device(kb.device_id, &wrong).await.unwrap_err();
    assert!(err.is_code(ErrorCode::InvalidProof), "{err}");
    let stored = api_b
        .attest_device(kb.device_id, &attest_request(&vault, kb.device_id))
        .await
        .unwrap();
    assert_eq!(stored.recipient_id, Some(kb.device_id.0));
    assert!(server.device_trusted(kb.device_id, vault.vault_id));
    assert_eq!(
        api_b
            .list_envelopes(vault.vault_id)
            .await
            .unwrap()
            .envelopes
            .len(),
        4
    );
    // Attesting another device is forbidden.
    let err = api_b
        .attest_device(ka.device_id, &attest_request(&vault, ka.device_id))
        .await
        .unwrap_err();
    assert!(err.is_code(ErrorCode::Forbidden), "{err}");

    // Replace password envelope (T + K) → old one revoked.
    let replaced = api_a
        .replace_password_envelope(&ReplaceEnvelopeRequest {
            vault_id: vault.vault_id,
            vault_access_key: vault.vak.clone(),
            envelope: password_envelope(),
        })
        .await
        .unwrap();
    let envs = api_a
        .list_envelopes(vault.vault_id)
        .await
        .unwrap()
        .envelopes;
    let pw: Vec<_> = envs
        .iter()
        .filter(|e| e.recipient_type == RecipientType::Password)
        .collect();
    assert_eq!(pw.len(), 1);
    assert_eq!(pw[0].envelope_id, replaced.envelope_id);
    api_a
        .replace_recovery_envelope(&ReplaceEnvelopeRequest {
            vault_id: vault.vault_id,
            vault_access_key: vault.vak.clone(),
            envelope: recovery_envelope(),
        })
        .await
        .unwrap();

    // Delete envelopes (protocol 1.2 body): device ok, password refused.
    let del = DeleteEnvelopeRequest {
        vault_access_key: vault.vak.clone(),
    };
    let err = api_a
        .delete_envelope(vault.vault_id, replaced.envelope_id, &del)
        .await
        .unwrap_err();
    assert!(err.is_code(ErrorCode::BadRequest), "{err}");
    api_a
        .delete_envelope(vault.vault_id, stored.envelope_id, &del)
        .await
        .unwrap();
    assert!(
        !server.device_trusted(kb.device_id, vault.vault_id),
        "B no longer trusted"
    );

    // IDOR: another account's vault is reported as not found.
    let (api_x, _) = register(&server, &email(), &DeviceKeys::generate()).await;
    let err = api_x.get_vault(vault.vault_id).await.unwrap_err();
    assert!(err.is_code(ErrorCode::NotFound), "{err}");
    let err = api_x.get_vault(VaultId::new()).await.unwrap_err();
    assert!(err.is_code(ErrorCode::NotFound), "{err}");

    // Delete the vault (T + K).
    let err = api_a
        .delete_vault(
            vault.vault_id,
            &DeleteVaultRequest {
                vault_access_key: Some(Bytes::new(random_bytes(32))),
            },
        )
        .await
        .unwrap_err();
    assert!(err.is_code(ErrorCode::InvalidProof));
    api_a
        .delete_vault(
            vault.vault_id,
            &DeleteVaultRequest {
                vault_access_key: Some(vault.vak.clone()),
            },
        )
        .await
        .unwrap();
    assert!(api_a
        .get_vault(vault.vault_id)
        .await
        .unwrap_err()
        .is_code(ErrorCode::NotFound));
}

#[tokio::test]
async fn device_trust_request_approval_rejection_rename_revoke() {
    let server = MockServer::start().await;
    let ka = DeviceKeys::generate();
    let mail = email();
    let (api_a, _) = register(&server, &mail, &ka).await;
    let v1 = create_vault(&api_a, ka.device_id).await;
    let v2 = create_vault(&api_a, ka.device_id).await;

    let kb = DeviceKeys::generate();
    let api_b = login(&server, &mail, &kb).await;
    // Empty vault list → server expands it to all accessible vaults.
    let req = api_b
        .create_trust_request(&CreateDeviceTrustRequest::default())
        .await
        .unwrap();
    let mut expected = vec![v1.vault_id, v2.vault_id];
    expected.sort();
    let mut got = req.vault_ids.clone();
    got.sort();
    assert_eq!(got, expected);
    assert_eq!(req.status, DeviceRequestStatus::Pending);

    let listed = api_a.list_devices().await.unwrap();
    assert_eq!(listed.devices.len(), 2);
    assert_eq!(listed.pending_requests.len(), 1);
    let pending = &listed.pending_requests[0];
    let new_dev = &pending.device;

    // Approve for v1 only, with a signature over the canonical message.
    let issued_at = chrono::Utc::now().timestamp();
    let msg = device_approval_message(
        pending.request_id,
        ka.device_id,
        new_dev.device_id,
        &new_dev.encryption_public_key.to_array().unwrap(),
        &new_dev.signing_public_key.to_array().unwrap(),
        issued_at,
        &[v1.vault_id],
    );
    let mut approve = ApproveDeviceRequest {
        request_id: pending.request_id,
        issued_at,
        signature: Bytes::new(ka.signing.sign(&msg).to_bytes().to_vec()),
        envelopes: vec![VaultEnvelope {
            vault_id: v1.vault_id,
            envelope: device_envelope(kb.device_id),
        }],
    };
    // A signature by the wrong key is rejected.
    let good_sig = approve.signature.clone();
    approve.signature = Bytes::new(
        DeviceKeys::generate()
            .signing
            .sign(&msg)
            .to_bytes()
            .to_vec(),
    );
    let err = api_a
        .approve_device(kb.device_id, &approve)
        .await
        .unwrap_err();
    assert!(err.is_code(ErrorCode::InvalidProof), "{err}");
    approve.signature = good_sig;
    let approved = api_a.approve_device(kb.device_id, &approve).await.unwrap();
    assert_eq!(approved.status, DeviceRequestStatus::Approved);
    assert_eq!(approved.approved_by_device_id, Some(ka.device_id));
    assert!(server.device_trusted(kb.device_id, v1.vault_id));
    assert!(!server.device_trusted(kb.device_id, v2.vault_id));

    // New request, rejected by A.
    let kc = DeviceKeys::generate();
    let api_c = login(&server, &mail, &kc).await;
    let req_c = api_c
        .create_trust_request(&CreateDeviceTrustRequest {
            vault_ids: vec![v2.vault_id],
        })
        .await
        .unwrap();
    api_a
        .reject_device(
            kc.device_id,
            &RejectDeviceRequest {
                request_id: req_c.request_id,
            },
        )
        .await
        .unwrap();
    assert!(api_a
        .list_devices()
        .await
        .unwrap()
        .pending_requests
        .is_empty());

    // Rename: own device only.
    let info = api_b
        .update_device(
            kb.device_id,
            &UpdateDeviceRequest {
                name: "Laptop".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(info.name, "Laptop");
    assert!(info.is_current);
    let err = api_b
        .update_device(
            ka.device_id,
            &UpdateDeviceRequest {
                name: "hijack".into(),
            },
        )
        .await
        .unwrap_err();
    assert!(err.is_code(ErrorCode::Forbidden), "{err}");

    // Revoke C: its API access ends immediately with device_revoked.
    api_a
        .revoke_device(kc.device_id, &Default::default())
        .await
        .unwrap();
    assert!(matches!(api_c.me().await, Err(ApiError::DeviceRevoked)));
    let devices = api_a.list_devices().await.unwrap().devices;
    let c = devices
        .iter()
        .find(|d| d.device_id == kc.device_id)
        .unwrap();
    assert_eq!(c.status, cc_protocol::devices::DeviceStatus::Revoked);
}

#[tokio::test]
async fn server_url_validation() {
    assert!(ApiConfig::new("https://sync.example.org/cc/", "1", Platform::Cli).is_ok());
    assert!(ApiConfig::new("http://127.0.0.1:8080", "1", Platform::Cli).is_ok());
    assert!(ApiConfig::new("http://sync.example.org", "1", Platform::Cli).is_err());
    assert!(ApiConfig::with_http_policy("http://nas.lan", "1", Platform::Cli, true).is_ok());
    assert!(ApiConfig::new("https://user:pw@sync.example.org", "1", Platform::Cli).is_err());
    assert!(ApiConfig::new("ftp://sync.example.org", "1", Platform::Cli).is_err());
    let api = ApiClient::new(
        ApiConfig::new("https://sync.example.org/cc/", "1", Platform::Cli).unwrap(),
        Arc::new(MemoryTokenStore::new()),
    )
    .unwrap();
    assert_eq!(
        api.events_url().as_str(),
        "wss://sync.example.org/cc/v1/events/ws"
    );
    // Offline server → network error classified as offline.
    let dead = ApiClient::new(
        ApiConfig::new("http://127.0.0.1:9", "1", Platform::Cli).unwrap(),
        Arc::new(MemoryTokenStore::new()),
    )
    .unwrap();
    let err = dead.meta().await.unwrap_err();
    assert!(err.is_offline() && err.is_retryable(), "{err}");
}

#[tokio::test]
async fn debug_output_never_contains_tokens() {
    let server = MockServer::start().await;
    let ka = DeviceKeys::generate();
    let store = Arc::new(MemoryTokenStore::new());
    let api = ApiClient::new(
        ApiConfig::new(server.url().as_str(), "t", Platform::Cli).unwrap(),
        store.clone(),
    )
    .unwrap();
    api.register(&cc_protocol::auth::RegisterRequest {
        email: email(),
        password: SecretString::new(PASSWORD),
        device: ka.registration("A"),
        device_proof: Some(ka.proof()),
    })
    .await
    .unwrap();
    let tokens = store.load().await.unwrap().unwrap();
    let dbg = format!("{api:?} {store:?} {tokens:?}");
    assert!(!dbg.contains(tokens.access_token.expose_secret()));
    assert!(!dbg.contains(tokens.refresh_token.expose_secret()));
}

fn proof_reason(err: &ApiError) -> Option<String> {
    match err {
        ApiError::Server { error, .. } if error.code == ErrorCode::InvalidProof => error
            .details
            .as_ref()
            .and_then(|d| d.get("reason"))
            .and_then(|r| r.as_str())
            .map(str::to_owned),
        _ => None,
    }
}

/// Security regression (protocol 1.4, ADR-0006): device ids and public keys
/// are visible to anyone with the account password (GET /v1/devices), so
/// login as a KNOWN device must prove possession of its signing key.
#[tokio::test]
async fn password_alone_cannot_log_in_as_an_existing_device() {
    let server = MockServer::start().await;
    let ka = DeviceKeys::generate();
    let mail = email();
    register(&server, &mail, &ka).await;
    let login_as = |proof: Option<cc_protocol::devices::DeviceProof>| {
        let api = api_for(&server);
        let req = LoginRequest {
            email: mail.clone(),
            password: SecretString::new(PASSWORD),
            device: ka.registration("stolen identity"),
            device_proof: proof,
        };
        async move { api.login(&req).await }
    };

    // No proof → rejected.
    let err = login_as(None).await.unwrap_err();
    assert_eq!(
        proof_reason(&err).as_deref(),
        Some("device_proof_required"),
        "{err}"
    );

    // Proof signed with the attacker's key over the victim's device id → rejected.
    let mut attacker = DeviceKeys::generate();
    attacker.device_id = ka.device_id;
    let err = login_as(Some(attacker.proof())).await.unwrap_err();
    assert_eq!(
        proof_reason(&err).as_deref(),
        Some("invalid_signature"),
        "{err}"
    );

    // Stale proof → rejected.
    let err = login_as(Some(ka.proof_at(chrono::Utc::now().timestamp() - 3600)))
        .await
        .unwrap_err();
    assert_eq!(proof_reason(&err).as_deref(), Some("stale"), "{err}");

    // Valid proof works once; replaying it is rejected.
    let proof = ka.proof();
    login_as(Some(proof.clone())).await.unwrap();
    let err = login_as(Some(proof)).await.unwrap_err();
    assert_eq!(proof_reason(&err).as_deref(), Some("replayed"), "{err}");
}

#[tokio::test]
async fn register_conflicts_distinguish_email_from_device_id() {
    let server = MockServer::start().await;
    let ka = DeviceKeys::generate();
    let mail = email();
    register(&server, &mail, &ka).await;

    // Same email, new device → plain server error (account exists).
    let kb = DeviceKeys::generate();
    let err = api_for(&server)
        .register(&cc_protocol::auth::RegisterRequest {
            email: mail.clone(),
            password: SecretString::new(PASSWORD),
            device: kb.registration("B"),
            device_proof: Some(kb.proof()),
        })
        .await
        .unwrap_err();
    assert!(err.is_code(ErrorCode::AlreadyExists), "{err}");
    assert!(!err.requires_new_device_identity());

    // New email, already-registered device id → new identity required.
    let err = api_for(&server)
        .register(&cc_protocol::auth::RegisterRequest {
            email: email(),
            password: SecretString::new(PASSWORD),
            device: ka.registration("A again"),
            device_proof: Some(ka.proof()),
        })
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::DeviceIdentityConflict), "{err}");
}
