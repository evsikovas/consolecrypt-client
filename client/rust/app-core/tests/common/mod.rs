//! Shared helpers for app-core integration tests.
#![allow(dead_code)]

use cc_app_core::platform::{
    InMemorySecureStore, OsAuthAvailability, OsAuthError, OsAuthKind, OsAuthenticator, SecureStore,
    UnsupportedOsAuthenticator,
};
use cc_app_core::{AppConfig, AppCore, AppError, HostDto};
use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

pub const PASSPHRASE: &str = "correct horse vault passphrase 42";
pub const ACCOUNT_PASSWORD: &str = "account-password-for-tests-1234";

/// Test double for Touch ID / Windows Hello.
#[derive(Debug)]
pub struct ApprovingOsAuth;

impl OsAuthenticator for ApprovingOsAuth {
    fn availability(&self) -> OsAuthAvailability {
        OsAuthAvailability::Available(OsAuthKind::TouchId)
    }
    fn authenticate(&self, _reason: &str) -> Result<(), OsAuthError> {
        Ok(())
    }
}

pub fn config(dir: &Path) -> AppConfig {
    AppConfig::for_tests(dir.to_string_lossy().to_string())
}

/// App with its own in-memory secure store.
pub fn app(dir: &Path) -> AppCore {
    AppCore::new(config(dir)).expect("app")
}

/// App sharing `store` (to simulate restarts of the same installation).
pub fn app_with(dir: &Path, store: Arc<dyn SecureStore>, biometrics: bool) -> AppCore {
    let auth: Arc<dyn OsAuthenticator> = if biometrics {
        Arc::new(ApprovingOsAuth)
    } else {
        Arc::new(UnsupportedOsAuthenticator)
    };
    AppCore::with_platform(config(dir), store, auth).expect("app")
}

pub fn memory_store() -> Arc<dyn SecureStore> {
    Arc::new(InMemorySecureStore::new())
}

/// Poll `f` until it returns true (or panic after `secs`).
pub async fn eventually<F, Fut>(secs: u64, what: &str, mut f: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    loop {
        if f().await {
            return;
        }
        if tokio::time::Instant::now() > deadline {
            panic!("timed out waiting for: {what}");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

pub fn host(name: &str, address: &str, cred: Option<&str>) -> HostDto {
    let mut h = HostDto::new(name, address);
    h.credential_id = cred.map(str::to_owned);
    h
}

pub fn code(e: &AppError) -> &'static str {
    e.code()
}

/// Re-evaluate an (async) boolean expression until it holds, or panic.
#[macro_export]
macro_rules! wait_until {
    ($secs:expr, $what:expr, $cond:expr) => {{
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs($secs);
        loop {
            if $cond {
                break;
            }
            if tokio::time::Instant::now() > deadline {
                panic!("timed out waiting for: {}", $what);
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }};
}
