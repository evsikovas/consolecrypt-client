//! Synced profiles against the sync-core in-process mock server: create
//! vault on A → B joins with the passphrase → edits flow both ways (WS
//! events) → device approval with the verification code → revoke → new
//! device identity → offline edit then sync → disconnect; enable sync for
//! a Local profile.

mod common;

use cc_app_core::*;
use cc_protocol::VaultId;
use cc_sync_core::mock::MockServer;
use common::*;
use std::str::FromStr;

fn has_host(hosts: &[HostDto], name: &str) -> bool {
    hosts.iter().any(|h| h.name == name)
}

async fn notes_of(app: &AppCore, id: &str) -> String {
    app.get_host(id.to_owned())
        .await
        .map(|h| h.notes)
        .unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn devices_join_approve_revoke_and_offline_sync() {
    let server = MockServer::start().await;
    let url = server.url().to_string();
    let tmp = tempfile::tempdir().unwrap();
    let email = "alice@example.org";

    // ── device A: register, create the vault, add objects ───────────────
    let a = common::app(&tmp.path().join("a"));
    let acct = a
        .create_synced_profile(
            "Work".into(),
            url.clone(),
            email.into(),
            ACCOUNT_PASSWORD.into(),
            AccountMode::Register,
        )
        .await
        .unwrap();
    assert!(acct.vaults.is_empty());
    assert_eq!(acct.profile.kind, ProfileKind::Synced);
    assert_eq!(acct.profile.vault_state, VaultStateDto::NoVault);
    assert_eq!(a.list_hosts().await.unwrap_err().code(), "no_vault");
    let kit = a.create_vault(PASSPHRASE.into()).await.unwrap();
    assert!(kit.server_url.is_some());
    let vault_id = kit.vault_id.clone();
    let vid = VaultId::from_str(&vault_id).unwrap();
    let cred = a
        .add_password_credential("root pw".into(), Some("root".into()), "pw-on-a".into())
        .await
        .unwrap();
    let web = a
        .save_host(host("web", "192.0.2.10", Some(&cred.id)))
        .await
        .unwrap();
    a.sync_now().await.unwrap();
    assert_eq!(a.sync_status().await.unwrap().pending, 0);
    // settings + secret + credential + host
    assert_eq!(server.live_objects(vid).len(), 4);

    // ── device B: log in, join with the passphrase ──────────────────────
    let b = common::app(&tmp.path().join("b"));
    let acct_b = b
        .create_synced_profile(
            "Work".into(),
            url.clone(),
            email.into(),
            ACCOUNT_PASSWORD.into(),
            AccountMode::Login,
        )
        .await
        .unwrap();
    assert_eq!(acct_b.vaults.len(), 1);
    assert!(!acct_b.vaults[0].caller_trusted);
    assert_eq!(
        b.join_vault_with_passphrase(vault_id.clone(), "wrong wrong wrong".into())
            .await
            .unwrap_err()
            .code(),
        "wrong_passphrase"
    );
    b.join_vault_with_passphrase(vault_id.clone(), PASSPHRASE.into())
        .await
        .unwrap();
    wait_until!(
        10,
        "host synced to B",
        has_host(&b.list_hosts().await.unwrap_or_default(), "web")
    );
    assert_eq!(b.list_credentials().await.unwrap().len(), 1);

    // ── B edits; A receives it through WS `vault_changed` ───────────────
    let mut on_b = b.find_host("web".into()).await.unwrap();
    on_b.notes = "edited on B".into();
    b.save_host(on_b).await.unwrap();
    b.sync_now().await.unwrap();
    wait_until!(
        10,
        "A pulls B's edit after a WS event",
        notes_of(&a, &web.id).await == "edited on B"
    );

    // ── device C: approval with the verification code ───────────────────
    let c = common::app(&tmp.path().join("c"));
    c.create_synced_profile(
        "Work".into(),
        url.clone(),
        email.into(),
        ACCOUNT_PASSWORD.into(),
        AccountMode::Login,
    )
    .await
    .unwrap();
    let own = c
        .request_device_approval(Some(vault_id.clone()))
        .await
        .unwrap();
    assert_eq!(
        own.verification_code,
        c.device_verification_code().await.unwrap()
    );
    assert!(
        !c.finish_device_approval(None).await.unwrap(),
        "not approved yet"
    );
    let listed = a.list_devices().await.unwrap();
    assert!(listed
        .pending_requests
        .iter()
        .any(|r| r.request_id == own.request_id));
    assert_eq!(
        a.confirm_device_approval(own.request_id.clone())
            .await
            .unwrap_err()
            .code(),
        "approval",
        "the code comparison step cannot be skipped"
    );
    let pending = a
        .start_device_approval(own.request_id.clone())
        .await
        .unwrap();
    assert_eq!(pending.verification_code, own.verification_code);
    a.confirm_device_approval(own.request_id.clone())
        .await
        .unwrap();
    assert!(c.finish_device_approval(None).await.unwrap());
    wait_until!(
        10,
        "host synced to C",
        has_host(&c.list_hosts().await.unwrap_or_default(), "web")
    );
    let c_device = c
        .active_profile()
        .await
        .unwrap()
        .unwrap()
        .device_id
        .unwrap();
    let devices = a.list_devices().await.unwrap().devices;
    assert_eq!(devices.len(), 3);
    assert!(devices
        .iter()
        .any(|d| d.device_id == c_device && d.trusted_for_vault));
    assert_eq!(
        a.rename_device(c_device.clone(), "Laptop C".into())
            .await
            .unwrap_err()
            .code(),
        "server",
        "only the current device can rename itself"
    );
    let renamed = c
        .rename_device(c_device.clone(), "Laptop C".into())
        .await
        .unwrap();
    assert_eq!(renamed.name, "Laptop C");

    // ── revoke C: its engine stops ──────────────────────────────────────
    let mut c_events = c.subscribe_events();
    a.revoke_device(c_device.clone(), Some("lost".into()))
        .await
        .unwrap();
    wait_until!(10, "C's sync engine stops", {
        let s = c.sync_status().await.unwrap();
        s.phase == SyncPhaseDto::Stopped && s.stop_reason == Some(SyncStopReasonDto::DeviceRevoked)
    });
    let mut saw_revoked = false;
    while let Ok(ev) = c_events.try_recv() {
        if matches!(ev, AppEvent::DeviceRevoked { is_self: true, .. }) {
            saw_revoked = true;
        }
    }
    assert!(saw_revoked, "C was told it is revoked");
    assert_eq!(c.sync_now().await.unwrap_err().code(), "device_revoked");

    // C signs in again with a fresh device identity and attests.
    assert_eq!(
        c.reauthenticate(ACCOUNT_PASSWORD.into(), false)
            .await
            .unwrap_err()
            .code(),
        "new_device_identity_required"
    );
    let st = c
        .reauthenticate(ACCOUNT_PASSWORD.into(), true)
        .await
        .unwrap();
    assert!(st.new_device_identity && st.trusted);
    assert_ne!(st.device_id, c_device);
    c.sync_now().await.unwrap();

    // ── offline edit on B, then sync ────────────────────────────────────
    server.set_offline(true);
    let mut on_b = b.find_host("web".into()).await.unwrap();
    on_b.notes = "offline edit".into();
    b.save_host(on_b).await.unwrap();
    assert_eq!(b.sync_now().await.unwrap_err().code(), "offline");
    assert!(b.sync_status().await.unwrap().pending >= 1);
    server.set_offline(false);
    b.sync_now().await.unwrap();
    assert_eq!(b.sync_status().await.unwrap().pending, 0);
    a.sync_now().await.unwrap();
    assert_eq!(notes_of(&a, &web.id).await, "offline edit");

    // ── expired access tokens are refreshed transparently ───────────────
    server.expire_access_tokens();
    a.sync_now().await.unwrap();

    // ── re-login of an existing trusted device needs its key proof ──────
    // (ADR-0006; the mock rejects a login without it)
    let st = a
        .reauthenticate(ACCOUNT_PASSWORD.into(), false)
        .await
        .unwrap();
    assert!(!st.new_device_identity && st.trusted, "{st:?}");
    a.sync_now().await.unwrap();
    assert_eq!(
        a.reauthenticate("wrong-account-password-123".into(), false)
            .await
            .unwrap_err()
            .code(),
        "invalid_credentials"
    );

    // ── disconnect B: becomes Local, keeps its data ─────────────────────
    let info = b.disconnect(false).await.unwrap();
    assert_eq!(info.kind, ProfileKind::Local);
    assert!(has_host(&b.list_hosts().await.unwrap(), "web"));
    assert_eq!(b.sync_now().await.unwrap_err().code(), "local_profile");
    b.lock().await.unwrap();
    b.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    assert!(has_host(&b.list_hosts().await.unwrap(), "web"));
    for x in [&a, &b, &c] {
        x.shutdown().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn enable_sync_uploads_a_local_vault() {
    let server = MockServer::start().await;
    let url = server.url().to_string();
    let tmp = tempfile::tempdir().unwrap();

    let d = common::app(&tmp.path().join("d"));
    d.create_local_profile("Home".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let key = d
        .generate_ssh_key(
            "home key".into(),
            Some("me".into()),
            KeyGenAlgorithm::Ed25519,
            None,
            false,
        )
        .await
        .unwrap();
    d.save_host(host("nas", "192.168.1.10", Some(&key.id)))
        .await
        .unwrap();
    let doomed = d
        .save_host(host("old", "192.168.1.11", None))
        .await
        .unwrap();
    d.delete_host(doomed.id).await.unwrap();

    let info = d
        .enable_sync(
            url.clone(),
            "bob@example.org".into(),
            ACCOUNT_PASSWORD.into(),
            AccountMode::Register,
        )
        .await
        .unwrap();
    assert_eq!(info.kind, ProfileKind::Synced);
    let vault_id = info.vault_id.clone().unwrap();
    let vid = VaultId::from_str(&vault_id).unwrap();
    d.sync_now().await.unwrap();
    assert_eq!(
        server.live_objects(vid).len() as u64,
        d.object_count().await.unwrap()
    );
    let meta = d
        .object_meta(d.find_host("nas".into()).await.unwrap().id)
        .await
        .unwrap();
    assert_eq!(meta.local_state, LocalState::Synced);
    assert_eq!(
        d.enable_sync(
            url.clone(),
            "bob@example.org".into(),
            ACCOUNT_PASSWORD.into(),
            AccountMode::Login
        )
        .await
        .unwrap_err()
        .code(),
        "already_synced"
    );
    assert!(d
        .list_profiles()
        .await
        .unwrap()
        .iter()
        .all(|p| p.kind == ProfileKind::Synced));

    // A second device joins the uploaded vault and sees the hosts.
    let e = common::app(&tmp.path().join("e"));
    let acct = e
        .create_synced_profile(
            "Home".into(),
            url,
            "bob@example.org".into(),
            ACCOUNT_PASSWORD.into(),
            AccountMode::Login,
        )
        .await
        .unwrap();
    assert!(acct.vaults.iter().any(|v| v.vault_id == vault_id));
    e.join_vault_with_passphrase(vault_id, PASSPHRASE.into())
        .await
        .unwrap();
    wait_until!(
        10,
        "uploaded host visible on the new device",
        has_host(&e.list_hosts().await.unwrap_or_default(), "nas")
    );
    assert!(!has_host(&e.list_hosts().await.unwrap(), "old"));
    d.shutdown().await.unwrap();
    e.shutdown().await.unwrap();
}
