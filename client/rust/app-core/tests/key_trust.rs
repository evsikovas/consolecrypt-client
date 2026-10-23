//! Vault key trust (protocol 1.11, audit H2) against the in-process mock
//! server, including a hostile server that substitutes keys:
//!
//! * approval v2: both devices show the same pairing code; the new device
//!   refuses a substituted envelope, vault id or signature and a withheld
//!   v2 approval with `vault_key_mismatch`, without attesting;
//! * an older server: trust on first use, confirmed by the passphrase; a
//!   substituted key is caught by the next passphrase unlock;
//! * a trusted device refuses a fresh password envelope with another key;
//! * nothing from an `attest` response is cached;
//! * signed key-commitment records reveal a device with another key.

mod common;

use cc_app_core::*;
use cc_crypto_core::{generate_vrk, seal_device_envelope, seal_password_envelope, Argon2Params};
use cc_protocol::{DeviceId, VaultId};
use cc_sync_core::mock::{FaultAction, MockServer, MockServerConfig};
use common::*;
use std::str::FromStr;
use std::sync::Arc;

const EMAIL: &str = "keys@example.org";

async fn synced(server: &MockServer, dir: &std::path::Path, mode: AccountMode) -> AppCore {
    let app = common::app(dir);
    app.create_synced_profile(
        "Work".into(),
        server.url().to_string(),
        EMAIL.into(),
        ACCOUNT_PASSWORD.into(),
        mode,
    )
    .await
    .unwrap();
    app
}

fn device_of(app_profile: &ProfileInfo) -> DeviceId {
    DeviceId::from_str(app_profile.device_id.as_deref().unwrap()).unwrap()
}

async fn current_device(app: &AppCore) -> DeviceId {
    device_of(&app.active_profile().await.unwrap().unwrap())
}

/// A device envelope for `device` that holds a VRK the server chose.
fn forged_device_envelope(
    server: &MockServer,
    vault: VaultId,
    device: DeviceId,
) -> serde_json::Value {
    let (enc, _) = server.device_public_keys(device).unwrap();
    let enc: [u8; 32] = enc.try_into().unwrap();
    let env = seal_device_envelope(&generate_vrk().unwrap(), vault, device, &enc).unwrap();
    let stored = cc_storage_core::local_key_envelope(vault, env, None);
    serde_json::to_value(stored).unwrap()
}

/// Owner A with a vault; new device C asks for approval; A approves with
/// the pairing code. Returns (A, C, vault id, request id, approver id).
async fn approved_pair(
    server: &MockServer,
    tmp: &std::path::Path,
) -> (AppCore, AppCore, String, String, String) {
    let a = synced(server, &tmp.join("a"), AccountMode::Register).await;
    let kit = a.create_vault(PASSPHRASE.into()).await.unwrap();
    let c = synced(server, &tmp.join("c"), AccountMode::Login).await;
    let own = c
        .request_device_approval(Some(kit.vault_id.clone()))
        .await
        .unwrap();
    assert!(own.vrk_commitment);
    let before = c
        .device_approval_status(own.request_id.clone())
        .await
        .unwrap();
    assert!(before.approver_device_id.is_none() && before.pairing_code.is_none());
    let pending = a
        .start_device_approval(own.request_id.clone())
        .await
        .unwrap();
    assert_eq!(pending.code_version, 2);
    let progress = c
        .device_approval_status(own.request_id.clone())
        .await
        .unwrap();
    assert_eq!(
        progress.pairing_code.as_deref(),
        Some(pending.verification_code.as_str()),
        "both screens show the same pairing code"
    );
    let approver = progress.approver_device_id.clone().unwrap();
    c.confirm_device_approver(
        own.request_id.clone(),
        approver.clone(),
        progress.pairing_code.clone().unwrap(),
    )
    .await
    .unwrap();
    assert!(
        c.device_approval_status(own.request_id.clone())
            .await
            .unwrap()
            .approver_confirmed
    );
    a.confirm_device_approval(own.request_id.clone())
        .await
        .unwrap();
    (a, c, kit.vault_id.clone(), own.request_id, approver)
}

fn attest_path(device: DeviceId) -> String {
    format!("/v1/devices/{device}/attest")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn approval_v2_refuses_a_substituted_vault_key() {
    let server = MockServer::start().await;
    let tmp = tempfile::tempdir().unwrap();
    let (a, c, vault_id, _, _) = approved_pair(&server, tmp.path()).await;
    let vid = VaultId::from_str(&vault_id).unwrap();
    let c_dev = current_device(&c).await;
    let mut events = c.subscribe_events();

    // The server hands C an envelope with its own VRK.
    let forged = forged_device_envelope(&server, vid, c_dev);
    server.inject(
        "/v1/recovery/vault/envelope",
        1,
        FaultAction::Rewrite(Arc::new(move |v| v["device_envelope"] = forged.clone())),
    );
    let e = c.finish_device_approval(None).await.unwrap_err();
    assert_eq!(e.code(), "vault_key_mismatch", "{e:?}");
    assert_eq!(
        e.args().get("reason").map(String::as_str),
        Some("envelope_hash")
    );
    assert!(!c.is_unlocked().await);
    assert_eq!(server.hits(&attest_path(c_dev)), 0, "no attest was sent");
    let mut saw = false;
    while let Ok(ev) = events.try_recv() {
        saw |= matches!(ev, AppEvent::VaultKeyMismatch { .. });
    }
    assert!(saw, "the UI is told");
    // The journal records it (metadata only).
    let journal = std::fs::read_dir(tmp.path().join("c").join("profiles"))
        .unwrap()
        .filter_map(|d| {
            std::fs::read_to_string(d.unwrap().path().join("security-journal.jsonl")).ok()
        })
        .collect::<String>();
    assert!(journal.contains("vault_key_mismatch"), "{journal}");

    // A substituted vault id: not in the signed list.
    server.inject(
        "/v1/recovery/vault/envelope",
        1,
        FaultAction::Rewrite(Arc::new(|v| {
            v["device_approval"]["vault_commitments"][0]["vault_id"] =
                serde_json::json!(VaultId::new().to_string())
        })),
    );
    assert_eq!(
        c.finish_device_approval(None).await.unwrap_err().code(),
        "vault_key_mismatch"
    );
    // A tampered signature.
    server.inject(
        "/v1/recovery/vault/envelope",
        1,
        FaultAction::Rewrite(Arc::new(|v| {
            let sig = v["device_approval"]["signature_v2"].as_str().unwrap();
            let flipped = if sig.starts_with('A') { "B" } else { "A" };
            v["device_approval"]["signature_v2"] =
                serde_json::json!(format!("{flipped}{}", &sig[1..]))
        })),
    );
    assert_eq!(
        c.finish_device_approval(None).await.unwrap_err().code(),
        "vault_key_mismatch"
    );
    // The v2 approval withheld although the approver signs v2.
    server.inject(
        "/v1/recovery/vault/envelope",
        1,
        FaultAction::Rewrite(Arc::new(|v| {
            v.as_object_mut().unwrap().remove("device_approval");
        })),
    );
    let e = c.finish_device_approval(None).await.unwrap_err();
    assert_eq!(e.code(), "vault_key_mismatch");
    assert_eq!(
        e.args().get("reason").map(String::as_str),
        Some("approval_missing")
    );
    assert_eq!(server.hits(&attest_path(c_dev)), 0);

    // The genuine approval opens and pins a confirmed key.
    assert!(c.finish_device_approval(None).await.unwrap());
    let status = c.vault_key_status().await.unwrap();
    assert!(status.pinned && !status.unconfirmed, "{status:?}");
    assert_eq!(
        status.fingerprint,
        a.vault_key_status().await.unwrap().fingerprint
    );
    a.shutdown().await.unwrap();
    c.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn substituted_approver_keys_change_the_pairing_code() {
    let server = MockServer::start().await;
    let tmp = tempfile::tempdir().unwrap();
    let a = synced(&server, &tmp.path().join("a"), AccountMode::Register).await;
    let kit = a.create_vault(PASSPHRASE.into()).await.unwrap();
    let c = synced(&server, &tmp.path().join("c"), AccountMode::Login).await;
    let own = c
        .request_device_approval(Some(kit.vault_id.clone()))
        .await
        .unwrap();
    let pending = a
        .start_device_approval(own.request_id.clone())
        .await
        .unwrap();
    // The server shows C other keys for the approver.
    let fake = cc_crypto_core::DeviceSecretKeys::generate()
        .unwrap()
        .public_keys();
    let (enc, sig) = (fake.encryption_bytes(), fake.signing_bytes());
    server.inject(
        "/v1/devices",
        1,
        FaultAction::Rewrite(Arc::new(move |v| {
            for r in v["pending_requests"].as_array_mut().unwrap() {
                if r.get("approver").is_some() {
                    r["approver"]["encryption_public_key"] = serde_json::json!(enc);
                    r["approver"]["signing_public_key"] = serde_json::json!(sig);
                }
            }
        })),
    );
    let progress = c
        .device_approval_status(own.request_id.clone())
        .await
        .unwrap();
    assert_ne!(
        progress.pairing_code.as_deref(),
        Some(pending.verification_code.as_str()),
        "the codes differ, so the user does not confirm"
    );
    // A confirmation is bound to the code that was on the screen.
    let approver = progress.approver_device_id.clone().unwrap();
    assert_eq!(
        c.confirm_device_approver(
            own.request_id.clone(),
            approver,
            progress.pairing_code.clone().unwrap(),
        )
        .await
        .unwrap_err()
        .code(),
        "approval"
    );
    a.shutdown().await.unwrap();
    c.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn older_server_pins_on_first_use_and_the_passphrase_confirms() {
    let server = MockServer::start_with(MockServerConfig {
        vrk_commitment: false,
        ..Default::default()
    })
    .await;
    let tmp = tempfile::tempdir().unwrap();
    let a = synced(&server, &tmp.path().join("a"), AccountMode::Register).await;
    let kit = a.create_vault(PASSPHRASE.into()).await.unwrap();
    let c = synced(&server, &tmp.path().join("c"), AccountMode::Login).await;
    let own = c
        .request_device_approval(Some(kit.vault_id.clone()))
        .await
        .unwrap();
    assert!(!own.vrk_commitment);
    let pending = a
        .start_device_approval(own.request_id.clone())
        .await
        .unwrap();
    assert_eq!(pending.code_version, 1);
    assert_eq!(pending.verification_code, own.verification_code);
    a.confirm_device_approval(own.request_id.clone())
        .await
        .unwrap();
    assert!(c.finish_device_approval(None).await.unwrap());
    let status = c.vault_key_status().await.unwrap();
    assert!(status.pinned && status.unconfirmed, "{status:?}");
    assert!(c.list_devices().await.unwrap().vault_key_unconfirmed);

    // The next passphrase unlock confirms the key.
    c.lock().await.unwrap();
    c.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert!(!c.vault_key_status().await.unwrap().unconfirmed);
    assert!(!c.list_devices().await.unwrap().vault_key_unconfirmed);
    a.shutdown().await.unwrap();
    c.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn older_server_substitution_is_caught_by_the_passphrase() {
    let server = MockServer::start_with(MockServerConfig {
        vrk_commitment: false,
        ..Default::default()
    })
    .await;
    let tmp = tempfile::tempdir().unwrap();
    let a = synced(&server, &tmp.path().join("a"), AccountMode::Register).await;
    let kit = a.create_vault(PASSPHRASE.into()).await.unwrap();
    let vid = VaultId::from_str(&kit.vault_id).unwrap();
    let c = synced(&server, &tmp.path().join("c"), AccountMode::Login).await;
    let c_dev = current_device(&c).await;
    let own = c
        .request_device_approval(Some(kit.vault_id.clone()))
        .await
        .unwrap();
    a.start_device_approval(own.request_id.clone())
        .await
        .unwrap();
    a.confirm_device_approval(own.request_id.clone())
        .await
        .unwrap();
    // A hostile old server: its own VRK for C, and it accepts any attest.
    let forged = forged_device_envelope(&server, vid, c_dev);
    let marker = forged.clone();
    server.inject(
        "/v1/recovery/vault/envelope",
        1,
        FaultAction::Rewrite(Arc::new(move |v| v["device_envelope"] = forged.clone())),
    );
    server.inject(
        &attest_path(c_dev),
        1,
        FaultAction::RawResponse {
            status: 200,
            body: marker,
        },
    );
    assert!(c.finish_device_approval(None).await.unwrap());
    assert!(c.vault_key_status().await.unwrap().unconfirmed);
    c.lock().await.unwrap();
    // The genuine passphrase opens the genuine key: not the pinned one.
    let e = c
        .unlock_with_passphrase(PASSPHRASE.into())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "vault_key_mismatch", "{e:?}");
    assert!(!c.is_unlocked().await);
    // Repeated on every attempt.
    assert_eq!(
        c.unlock_with_passphrase(PASSPHRASE.into())
            .await
            .unwrap_err()
            .code(),
        "vault_key_mismatch"
    );
    a.shutdown().await.unwrap();
    c.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn trusted_device_refuses_a_fresh_envelope_with_another_key() {
    let server = MockServer::start().await;
    let tmp = tempfile::tempdir().unwrap();
    let a = synced(&server, &tmp.path().join("a"), AccountMode::Register).await;
    let kit = a.create_vault(PASSPHRASE.into()).await.unwrap();
    let vid = VaultId::from_str(&kit.vault_id).unwrap();
    a.lock().await.unwrap();
    // The user types another passphrase (changed elsewhere, they think);
    // the server answers the retry with an envelope that this passphrase
    // opens — but around a different key.
    let other = "another passphrase of the user 77";
    let env = seal_password_envelope(
        &generate_vrk().unwrap(),
        vid,
        &cc_crypto_core::SecretString::from(other.to_owned()),
        Argon2Params::for_tests(),
    )
    .unwrap();
    let forged = serde_json::to_value(cc_storage_core::local_key_envelope(vid, env, None)).unwrap();
    server.inject(
        "/v1/recovery/vault/envelope",
        1,
        FaultAction::Rewrite(Arc::new(move |v| v["password_envelope"] = forged.clone())),
    );
    let e = a.unlock_with_passphrase(other.into()).await.unwrap_err();
    assert_eq!(e.code(), "vault_key_mismatch", "{e:?}");
    assert!(!a.is_unlocked().await);
    // The cached envelope was not replaced: the real passphrase still works.
    a.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert!(a.is_unlocked().await);
    a.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn attest_responses_are_not_cached_and_records_reveal_another_key() {
    let server = MockServer::start().await;
    let tmp = tempfile::tempdir().unwrap();
    let a = synced(&server, &tmp.path().join("a"), AccountMode::Register).await;
    let kit = a.create_vault(PASSPHRASE.into()).await.unwrap();
    let vid = VaultId::from_str(&kit.vault_id).unwrap();
    let a_dev = current_device(&a).await;
    let store_b = memory_store();
    let b = common::app_with(&tmp.path().join("b"), store_b.clone(), false);
    b.create_synced_profile(
        "Work".into(),
        server.url().to_string(),
        EMAIL.into(),
        ACCOUNT_PASSWORD.into(),
        AccountMode::Login,
    )
    .await
    .unwrap();
    let b_dev = current_device(&b).await;
    // A hostile `attest` answer: whatever it returns must not be cached.
    let echo = forged_device_envelope(&server, vid, b_dev);
    let echo_ct = echo["ciphertext"].as_str().unwrap().to_owned();
    server.inject(
        &attest_path(b_dev),
        1,
        FaultAction::Rewrite(Arc::new(move |v| *v = echo.clone())),
    );
    b.join_vault_with_passphrase(kit.vault_id.clone(), PASSPHRASE.into())
        .await
        .unwrap();
    let b_dir = std::fs::read_dir(tmp.path().join("b").join("profiles"))
        .unwrap()
        .map(|d| d.unwrap().path())
        .find(|p| p.join("vault-keys.json").exists())
        .unwrap();
    let keyfile = std::fs::read_to_string(b_dir.join("vault-keys.json")).unwrap();
    assert!(keyfile.contains("\"vault_key\""), "the key is pinned");
    assert!(
        !keyfile.contains(&echo_ct),
        "the attest response is not cached"
    );
    b.lock().await.unwrap();
    let db = std::fs::read(b_dir.join("vault.db")).unwrap_or_default();
    assert!(
        !db.windows(echo_ct.len()).any(|w| w == echo_ct.as_bytes()),
        "not in the database either"
    );
    b.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();

    // Both devices publish signed key-commitment records.
    wait_until!(10, "records published", {
        server.key_commitment(vid, a_dev).is_some() && server.key_commitment(vid, b_dev).is_some()
    });
    let mut events = a.subscribe_events();
    // An unsigned record claiming another key for B is ignored.
    server.plant_key_commitment(
        vid,
        cc_protocol::vaults::VaultKeyCommitment {
            device_id: b_dev,
            commitment: cc_protocol::Bytes::new(vec![9; 32]),
            issued_at: chrono::Utc::now().timestamp() + 1,
            signature: cc_protocol::Bytes::new(vec![0; 64]),
        },
    );
    a.lock().await.unwrap();
    a.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(700)).await;
    assert!(a.vault_key_status().await.unwrap().conflicts.is_empty());

    // B (its real device key) reports another key: A warns and keeps its own.
    let profile_b = b.active_profile().await.unwrap().unwrap().id;
    let identity_b = cc_vault_core::DeviceIdentity::load(&*store_b, &profile_b)
        .unwrap()
        .unwrap();
    server.plant_key_commitment(
        vid,
        identity_b.key_commitment_record(vid, &[9; 32], chrono::Utc::now().timestamp() + 2),
    );
    let own = a.vault_key_status().await.unwrap().fingerprint;
    a.lock().await.unwrap();
    a.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    wait_until!(10, "conflict reported", {
        !a.vault_key_status().await.unwrap().conflicts.is_empty()
    });
    let status = a.vault_key_status().await.unwrap();
    assert_eq!(status.conflicts[0].device_id, b_dev.to_string());
    assert_eq!(status.fingerprint, own, "A keeps its own key");
    assert_eq!(
        a.list_devices().await.unwrap().vault_key_conflicts,
        vec![b_dev.to_string()]
    );
    let mut saw = false;
    while let Ok(ev) = events.try_recv() {
        saw |= matches!(ev, AppEvent::VaultKeyConflict { .. });
    }
    assert!(saw);
    a.shutdown().await.unwrap();
    b.shutdown().await.unwrap();
}

/// TR-17: a profile from before the update (no pin) pins its key on the
/// first unlock; a device unlock pins it "unconfirmed", the next passphrase
/// unlock confirms it; afterwards a different key is refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn existing_profiles_pin_on_first_unlock() {
    use cc_app_core::platform::{FakePrompt, InMemoryDeviceKeys};
    let dir = tempfile::tempdir().unwrap();
    let store = memory_store();
    let keys = Arc::new(InMemoryDeviceKeys::new());
    let app = common::app_with_keys(dir.path(), store.clone(), keys.clone());
    let profile = app
        .create_local_profile("Home".into(), PASSPHRASE.into())
        .await
        .unwrap()
        .profile;
    assert!(app.vault_key_status().await.unwrap().pinned);
    keys.set_next(FakePrompt::Succeed);
    app.set_device_unlock_enabled(true, "r".into(), false)
        .await
        .unwrap();
    app.shutdown().await.unwrap();

    // Simulate a key file written by 0.9.1: no pin.
    let path = dir
        .path()
        .join("profiles")
        .join(&profile.id)
        .join("vault-keys.json");
    let mut kf: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let pinned = kf["vault_key"].clone();
    kf.as_object_mut().unwrap().remove("vault_key");
    std::fs::write(&path, serde_json::to_vec_pretty(&kf).unwrap()).unwrap();

    let app = common::app_with_keys(dir.path(), store.clone(), keys.clone());
    app.open_profile(profile.id.clone()).await.unwrap();
    keys.set_next(FakePrompt::Succeed);
    app.unlock_with_device("r".into()).await.unwrap();
    let status = app.vault_key_status().await.unwrap();
    assert!(status.pinned && status.unconfirmed, "{status:?}");
    app.lock().await.unwrap();
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert!(!app.vault_key_status().await.unwrap().unconfirmed);
    app.shutdown().await.unwrap();
    let kf: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        kf["vault_key"]["commitment"], pinned["commitment"],
        "the same key as before the update"
    );

    // A pin that does not match the vault (as if the key file had been
    // pinned to another key) refuses every unlock path.
    let mut kf = kf;
    kf["vault_key"]["commitment"] =
        serde_json::to_value(cc_protocol::Bytes::new(vec![7; 32])).unwrap();
    std::fs::write(&path, serde_json::to_vec_pretty(&kf).unwrap()).unwrap();
    let app = common::app_with_keys(dir.path(), store.clone(), keys.clone());
    app.open_profile(profile.id.clone()).await.unwrap();
    assert_eq!(
        app.unlock_with_passphrase(PASSPHRASE.into())
            .await
            .unwrap_err()
            .code(),
        "vault_key_mismatch"
    );
    keys.set_next(FakePrompt::Succeed);
    assert_eq!(
        app.unlock_with_device("r".into()).await.unwrap_err().code(),
        "vault_key_mismatch"
    );
    assert!(!app.is_unlocked().await);
    app.shutdown().await.unwrap();
}
