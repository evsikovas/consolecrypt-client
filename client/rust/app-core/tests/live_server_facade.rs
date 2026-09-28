//! The facade's synced flows against a LIVE server (gated):
//!
//! ```sh
//! CC_E2E_SERVER=http://localhost:8080 cargo test -p cc-app-core --test live_server_facade -- --nocapture
//! ```

mod common;

use cc_app_core::*;
use common::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn facade_flows_against_live_server() {
    let Ok(url) = std::env::var("CC_E2E_SERVER") else {
        eprintln!("skipped: set CC_E2E_SERVER=http://localhost:8080 to run against a live server");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let email = format!("facade-{}@example.test", uuid::Uuid::new_v4().simple());

    let a = common::app(&tmp.path().join("a"));
    a.create_synced_profile(
        "Work".into(),
        url.clone(),
        email.clone(),
        ACCOUNT_PASSWORD.into(),
        AccountMode::Register,
    )
    .await
    .unwrap();
    let kit = a.create_vault(PASSPHRASE.into()).await.unwrap();
    let key = a
        .generate_ssh_key(
            "k".into(),
            Some("ops".into()),
            KeyGenAlgorithm::Ed25519,
            None,
            false,
        )
        .await
        .unwrap();
    let web = a
        .save_host(host("web", "192.0.2.10", Some(&key.id)))
        .await
        .unwrap();
    a.sync_now().await.unwrap();

    // B joins with the passphrase; edits reach A through WS events.
    let b = common::app(&tmp.path().join("b"));
    b.create_synced_profile(
        "Work".into(),
        url.clone(),
        email.clone(),
        ACCOUNT_PASSWORD.into(),
        AccountMode::Login,
    )
    .await
    .unwrap();
    b.join_vault_with_passphrase(kit.vault_id.clone(), PASSPHRASE.into())
        .await
        .unwrap();
    b.sync_now().await.unwrap();
    let mut h = b.find_host("web".into()).await.unwrap();
    h.notes = "from B".into();
    b.save_host(h).await.unwrap();
    b.sync_now().await.unwrap();
    wait_until!(15, "A sees B's edit", {
        a.get_host(web.id.clone())
            .await
            .map(|h| h.notes)
            .unwrap_or_default()
            == "from B"
    });

    // C is approved by A with the verification code.
    let c = common::app(&tmp.path().join("c"));
    c.create_synced_profile(
        "Work".into(),
        url.clone(),
        email.clone(),
        ACCOUNT_PASSWORD.into(),
        AccountMode::Login,
    )
    .await
    .unwrap();
    let own = c
        .request_device_approval(Some(kit.vault_id.clone()))
        .await
        .unwrap();
    let pending = a
        .start_device_approval(own.request_id.clone())
        .await
        .unwrap();
    assert_eq!(pending.verification_code, own.verification_code);
    a.confirm_device_approval(own.request_id.clone())
        .await
        .unwrap();
    assert!(c.finish_device_approval(None).await.unwrap());
    c.sync_now().await.unwrap();
    assert!(c
        .list_hosts()
        .await
        .unwrap()
        .iter()
        .any(|h| h.name == "web"));

    // Revoke C → stops; new identity + attestation brings it back.
    let c_dev = c
        .active_profile()
        .await
        .unwrap()
        .unwrap()
        .device_id
        .unwrap();
    a.revoke_device(c_dev.clone(), None).await.unwrap();
    wait_until!(15, "C stops", {
        c.sync_status().await.unwrap().phase == SyncPhaseDto::Stopped || c.sync_now().await.is_err()
    });
    let st = c
        .reauthenticate(ACCOUNT_PASSWORD.into(), true)
        .await
        .unwrap();
    assert!(st.new_device_identity && st.trusted, "{st:?}");
    c.sync_now().await.unwrap();

    // Passphrase change on A replaces the server envelope; B unlocks with
    // the new passphrase (fresh envelope fetched on the first failure).
    a.change_passphrase(PASSPHRASE.into(), "changed live passphrase".into())
        .await
        .unwrap();
    b.lock().await.unwrap();
    b.unlock_with_passphrase("changed live passphrase".into())
        .await
        .unwrap();

    // A local profile enables sync on the same account.
    let d = common::app(&tmp.path().join("d"));
    d.create_local_profile("Home".into(), PASSPHRASE.into())
        .await
        .unwrap();
    d.save_host(host("nas", "192.168.1.10", None))
        .await
        .unwrap();
    let info = d
        .enable_sync(
            url.clone(),
            email.clone(),
            ACCOUNT_PASSWORD.into(),
            AccountMode::Login,
        )
        .await
        .unwrap();
    assert_eq!(info.kind, ProfileKind::Synced);
    d.sync_now().await.unwrap();
    assert_eq!(d.sync_status().await.unwrap().pending, 0);

    for x in [&a, &b, &c, &d] {
        x.shutdown().await.unwrap();
    }
    println!("facade flows against {url}: OK");
}
