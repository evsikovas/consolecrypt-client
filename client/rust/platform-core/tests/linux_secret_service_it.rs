//! Real system Secret Service acceptance, exclusively in an owned disposable
//! Linux desktop/container. This never runs in the default Cargo suite.
//!
//! Set CC_LINUX_KEYRING_IT=owned-disposable and CC_LINUX_KEYRING_CASE to one of
//! roundtrip, missing-default, transient-default, no-service. Give each case
//! a separate session bus and ephemeral HOME; never run against a login bus.
#![cfg(all(target_os = "linux", feature = "os-keychain"))]

use cc_platform_core::{
    ExposeSecret, OsSecureStore, SecureStore, SecureStoreError, MAX_SECRET_LEN,
};
use secret_service::{blocking::SecretService, EncryptionType};
use std::{collections::HashMap, io::Read, time::Instant};
use zeroize::Zeroizing;

fn random_value(len: usize) -> Zeroizing<Vec<u8>> {
    let mut value = Zeroizing::new(vec![0; len]);
    std::fs::File::open("/dev/urandom")
        .unwrap()
        .read_exact(&mut value)
        .unwrap();
    value
}

fn assert_denied<T>(result: Result<T, SecureStoreError>) {
    assert!(
        matches!(result, Err(SecureStoreError::AccessDenied(_))),
        "operation must fail closed"
    );
}

#[test]
#[ignore = "requires an owned isolated Linux session bus and disposable GNOME keyring"]
fn secret_service_disposable_desktop() {
    assert_eq!(
        std::env::var("CC_LINUX_KEYRING_IT").as_deref(),
        Ok("owned-disposable")
    );
    let test_home = std::env::var("HOME").unwrap();
    assert!(
        test_home.starts_with("/tmp/consolecrypt-linux-keyring-"),
        "requires disposable test home"
    );
    let mode = std::env::var("CC_LINUX_KEYRING_CASE").unwrap();
    let service = format!("io.consolecrypt.test.{}", std::process::id());

    match mode.as_str() {
        "no-service" => {
            let start = Instant::now();
            assert_denied(OsSecureStore::with_service(&service));
            assert!(
                start.elapsed().as_secs() < 15,
                "missing service must not wait for an unlock prompt"
            );
        }
        "missing-default" | "transient-default" => {
            let ss = SecretService::connect(EncryptionType::Dh).unwrap();
            let collections_before = ss.get_all_collections().unwrap().len();
            assert_denied(OsSecureStore::with_service(&service));
            assert_eq!(
                ss.get_all_collections().unwrap().len(),
                collections_before,
                "must not create collections"
            );
        }
        "roundtrip" => {
            let store = OsSecureStore::with_service(&service).unwrap();
            let value = random_value(64);
            let name = "generated-device-key";
            assert!(store.get(name).unwrap().is_none());
            store.set(name, &value).unwrap();
            // Boolean assertions never include generated secret values in a
            // panic/test log. Reconnect proves this isn't an in-memory fake.
            let reopened = OsSecureStore::with_service(&service).unwrap();
            assert!(
                reopened.get(name).unwrap().unwrap().expose_secret() == value.as_slice(),
                "persisted value mismatch"
            );
            let changed = random_value(MAX_SECRET_LEN);
            reopened.set(name, &changed).unwrap();
            assert!(
                store.get(name).unwrap().unwrap().expose_secret() == changed.as_slice(),
                "replacement value mismatch"
            );
            assert_eq!(
                store.get("bad name").unwrap_err(),
                SecureStoreError::InvalidName
            );
            assert_eq!(
                store.set(name, &vec![0; MAX_SECRET_LEN + 1]),
                Err(SecureStoreError::TooLarge(MAX_SECRET_LEN + 1))
            );
            assert!(store.delete(name).unwrap());
            assert!(!store.delete(name).unwrap());
            assert!(store.get(name).unwrap().is_none());

            let ss = SecretService::connect(EncryptionType::Dh).unwrap();
            let collection = ss.get_default_collection().unwrap();
            let attrs = HashMap::from([("service", service.as_str()), ("username", "malformed")]);
            let malformed = collection
                .create_item(
                    "Synthetic format test",
                    attrs.clone(),
                    b"invalid-format",
                    false,
                    "text/plain",
                )
                .unwrap();
            assert_eq!(
                store.get("malformed").unwrap_err(),
                SecureStoreError::Corrupted
            );
            malformed.delete().unwrap();
            // Reject ambiguous items instead of choosing/overwriting a key.
            let first = collection
                .create_item(
                    "Synthetic duplicate",
                    attrs.clone(),
                    b"invalid-format",
                    false,
                    "text/plain",
                )
                .unwrap();
            let second = collection
                .create_item(
                    "Synthetic duplicate",
                    attrs,
                    b"invalid-format",
                    false,
                    "text/plain",
                )
                .unwrap();
            assert!(matches!(
                store.get("malformed"),
                Err(SecureStoreError::Backend(_))
            ));
            assert!(matches!(
                store.set("malformed", &value),
                Err(SecureStoreError::Backend(_))
            ));
            assert!(matches!(
                store.delete("malformed"),
                Err(SecureStoreError::Backend(_))
            ));
            first.delete().unwrap();
            second.delete().unwrap();

            // Keep an item when the collection locks. A locked store must
            // never return None or accept a replacement/new device key.
            store.set(name, &value).unwrap();
            collection.lock().unwrap();
            assert!(collection.is_locked().unwrap());
            let start = Instant::now();
            assert_denied(store.get(name));
            assert_denied(store.get("absent"));
            assert_denied(store.set(name, &changed));
            assert_denied(store.delete(name));
            assert_denied(OsSecureStore::with_service(&service));
            assert!(
                start.elapsed().as_secs() < 15,
                "locked collection must not trigger an unlock prompt"
            );
            // Test harness destroys this entire disposable keyring afterwards.
        }
        _ => panic!("unknown isolated test case"),
    }
}
