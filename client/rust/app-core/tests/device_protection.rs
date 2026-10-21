//! New-device protection (protocol 1.7) against the sync-core mock server:
//! trusted devices hear `device_registered`, the device list carries account
//! trust and quarantine, a new device cannot revoke a trusted one and its
//! "sign out everywhere" keeps trusted devices signed in. Without the
//! capability (older servers) the list keeps the old shape.

mod common;

use cc_app_core::*;
use cc_protocol::paths;
use cc_sync_core::mock::{MockServer, MockServerConfig};
use common::*;

async fn register_with_vault(url: &str, dir: &std::path::Path) -> AppCore {
    let app = common::app(dir);
    app.create_synced_profile(
        "Work".into(),
        url.into(),
        "alice@example.org".into(),
        ACCOUNT_PASSWORD.into(),
        AccountMode::Register,
    )
    .await
    .unwrap();
    app.create_vault(PASSPHRASE.into()).await.unwrap();
    app
}

async fn sign_in(url: &str, dir: &std::path::Path) -> AppCore {
    let app = common::app(dir);
    app.create_synced_profile(
        "Work".into(),
        url.into(),
        "alice@example.org".into(),
        ACCOUNT_PASSWORD.into(),
        AccountMode::Login,
    )
    .await
    .unwrap();
    app
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn new_device_is_announced_restricted_and_keeps_trusted_devices_signed_in() {
    let server = MockServer::start_with(MockServerConfig {
        device_protection: true,
        ..MockServerConfig::default()
    })
    .await;
    let url = server.url().to_string();
    let tmp = tempfile::tempdir().unwrap();
    let a = register_with_vault(&url, &tmp.path().join("a")).await;
    let mut a_events = a.subscribe_events();
    wait_until!(
        10,
        "A's realtime connection",
        server.hits(paths::EVENTS_WS) > 0
    );

    let b = sign_in(&url, &tmp.path().join("b")).await;
    let b_id = b
        .list_devices()
        .await
        .unwrap()
        .devices
        .into_iter()
        .find(|d| d.is_current)
        .unwrap();
    let mut announced = None;
    wait_until!(10, "A hears device_registered", {
        while let Ok(ev) = a_events.try_recv() {
            if let AppEvent::DeviceRegistered {
                device_id,
                name,
                platform,
                ..
            } = ev
            {
                announced = Some((device_id, name, platform));
            }
        }
        announced.is_some()
    });
    let (device_id, name, _) = announced.unwrap();
    assert_eq!(device_id, b_id.device_id);
    assert_eq!(name, b_id.name);

    // Account-level trust and quarantine; the capability is advertised.
    let list = a.list_devices().await.unwrap();
    assert!(list.device_protection);
    let me = list.devices.iter().find(|d| d.is_current).unwrap();
    assert!(
        me.trusted && me.quarantine_until_ms.is_none(),
        "registering device"
    );
    let newcomer = list
        .devices
        .iter()
        .find(|d| d.device_id == b_id.device_id)
        .unwrap();
    assert!(!newcomer.trusted);
    assert!(newcomer.quarantine_until_ms.is_some());

    // The newcomer cannot revoke the trusted device …
    let err = b
        .revoke_device(me.device_id.clone(), None)
        .await
        .unwrap_err();
    assert_eq!(err.code(), "device_in_quarantine");
    assert!(err.args().contains_key("until_ms"));

    // … and "sign out everywhere" from it keeps A signed in.
    let out = b.logout_all_sessions().await.unwrap();
    assert_eq!(out.trusted_devices_kept, 1);
    assert!(out.quarantine_until_ms.is_some());
    assert!(a.list_devices().await.is_ok(), "A is still signed in");

    // From the trusted device every session ends, as before.
    let c = sign_in(&url, &tmp.path().join("c")).await;
    let out = a.logout_all_sessions().await.unwrap();
    assert_eq!(out.trusted_devices_kept, 0);
    assert_eq!(
        c.list_devices().await.unwrap_err().code(),
        "reauth_required"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn without_the_capability_the_list_keeps_the_old_shape() {
    let server = MockServer::start().await;
    let url = server.url().to_string();
    let tmp = tempfile::tempdir().unwrap();
    let a = register_with_vault(&url, &tmp.path().join("a")).await;
    let b = sign_in(&url, &tmp.path().join("b")).await;
    let list = a.list_devices().await.unwrap();
    assert!(!list.device_protection);
    assert!(list.devices.iter().all(|d| d.quarantine_until_ms.is_none()));
    // Trust is still derived from the vaults for older servers.
    assert!(list.devices.iter().find(|d| d.is_current).unwrap().trusted);
    // A brand-new device may revoke anything, as before 1.7.
    let a_id = list
        .devices
        .iter()
        .find(|d| d.is_current)
        .unwrap()
        .device_id
        .clone();
    b.revoke_device(a_id, None).await.unwrap();
}
