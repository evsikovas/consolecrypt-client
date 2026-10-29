//! Security key unlock (FEAT-16) with a simulated FIDO2 key: switching it
//! on seals the vault key under the key's PRF answer; only that key with
//! its PIN opens it; the passphrase keeps working; off removes it.

mod common;

use cc_app_core::*;
use cc_platform_core::InMemorySecurityKey;
use common::*;
use std::sync::Arc;

fn rule(e: &AppError) -> String {
    e.args().get("rule").cloned().unwrap_or_default()
}

#[tokio::test]
async fn a_security_key_unlocks_only_with_its_pin_and_stays_optional() {
    let tmp = tempfile::tempdir().unwrap();
    let app = common::app(tmp.path());
    let key = Arc::new(InMemorySecurityKey::default());
    app.set_security_keys(key.clone());
    app.create_local_profile("Key".into(), PASSPHRASE.into())
        .await
        .unwrap();

    let info = app.security_key_info().await.unwrap();
    assert!(info.supported && !info.enabled);
    assert_eq!(info.pin.as_deref(), Some("app"));
    let e = app
        .set_security_key_enabled(true, Some("0000".into()))
        .await
        .unwrap_err();
    assert_eq!(rule(&e), "pin_invalid");
    app.set_security_key_enabled(true, Some(InMemorySecurityKey::PIN.into()))
        .await
        .unwrap();
    assert!(app.security_key_info().await.unwrap().enabled);

    app.lock().await.unwrap();
    let e = app
        .unlock_with_security_key(Some("0000".into()))
        .await
        .unwrap_err();
    assert_eq!(rule(&e), "pin_invalid");
    let e = app.unlock_with_security_key(None).await.unwrap_err();
    assert_eq!(rule(&e), "pin_required");
    app.unlock_with_security_key(Some(InMemorySecurityKey::PIN.into()))
        .await
        .unwrap();
    assert!(app.list_hosts().await.is_ok());

    // The passphrase still works.
    app.lock().await.unwrap();
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();

    // Another key cannot open it.
    key.replace();
    app.lock().await.unwrap();
    let e = app
        .unlock_with_security_key(Some(InMemorySecurityKey::PIN.into()))
        .await
        .unwrap_err();
    assert_eq!(rule(&e), "wrong_key");

    // Off: the record is gone.
    app.unlock_with_passphrase(PASSPHRASE.into()).await.unwrap();
    app.set_security_key_enabled(false, None).await.unwrap();
    assert!(!app.security_key_info().await.unwrap().enabled);
    app.lock().await.unwrap();
    assert!(app
        .unlock_with_security_key(Some(InMemorySecurityKey::PIN.into()))
        .await
        .is_err());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn switching_a_security_key_on_needs_an_unlocked_vault_and_support() {
    let tmp = tempfile::tempdir().unwrap();
    let app = common::app(tmp.path());
    app.create_local_profile("Key".into(), PASSPHRASE.into())
        .await
        .unwrap();
    // No hardware in tests by default.
    let info = app.security_key_info().await.unwrap();
    assert!(!info.supported && info.pin.is_none());
    let e = app
        .set_security_key_enabled(true, Some("1234".into()))
        .await
        .unwrap_err();
    assert_eq!(rule(&e), "unsupported");
    app.set_security_keys(Arc::new(InMemorySecurityKey::default()));
    app.lock().await.unwrap();
    assert_eq!(
        app.set_security_key_enabled(true, Some("1234".into()))
            .await
            .unwrap_err()
            .code(),
        "vault_locked"
    );
    app.shutdown().await.unwrap();
}
