//! Global bridge state: the tokio runtime all core futures run on and the
//! single [`AppCore`] instance.

use crate::api::error::BridgeError;
use cc_app_core::AppCore;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::future::Future;
use std::sync::{Arc, OnceLock, RwLock};
use zeroize::Zeroize;

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
struct CoreSlot {
    core: AppCore,
    identity: Arc<()>,
}
static CORE: RwLock<Option<CoreSlot>> = RwLock::new(None);

/// The runtime every app-core future (and its spawned tasks) runs on.
pub(crate) fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("cc-core")
            .build()
            .expect("cannot start the core runtime")
    })
}

/// The initialized core, or `not_initialized`.
pub(crate) fn core() -> Result<AppCore, BridgeError> {
    CORE.read()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .map(|slot| slot.core.clone())
        .ok_or_else(BridgeError::not_initialized)
}

pub(crate) fn core_opt() -> Option<AppCore> {
    CORE.read().unwrap_or_else(|p| p.into_inner()).as_ref().map(|slot| slot.core.clone())
}

pub(crate) fn set_core(core: Option<AppCore>) {
    *CORE.write().unwrap_or_else(|p| p.into_inner()) = core.map(|core| CoreSlot {
        core,
        identity: Arc::new(()),
    });
}

/// Identity is only the original bridge slot, never an application grant.
pub(crate) fn core_for_shutdown() -> Option<(AppCore, Arc<()>)> {
    CORE.read().unwrap_or_else(|p| p.into_inner()).as_ref()
        .map(|slot| (slot.core.clone(), slot.identity.clone()))
}

pub(crate) fn complete_shutdown(original: &Arc<()>, result: &Result<(), BridgeError>) {
    let mut current = CORE.write().unwrap_or_else(|p| p.into_inner());
    complete_shutdown_slot(&mut current, original, result);
}

fn complete_shutdown_slot(current: &mut Option<CoreSlot>, original: &Arc<()>, result: &Result<(), BridgeError>) {
    if result.is_err() { return; }
    if current.as_ref().is_some_and(|slot| Arc::ptr_eq(&slot.identity, original)) {
        current.take();
    }
}

/// Run `f` on the core runtime and await its result from any executor.
pub(crate) async fn run<T, F>(f: F) -> Result<T, BridgeError>
where
    T: Send + 'static,
    F: Future<Output = Result<T, BridgeError>> + Send + 'static,
{
    runtime()
        .spawn(f)
        .await
        .map_err(|e| BridgeError::internal(format!("core task failed: {e}")))?
}

/// Run a core operation: `op(core)` on the core runtime.
pub(crate) async fn with_core<T, F, Fut>(op: F) -> Result<T, BridgeError>
where
    T: Send + 'static,
    F: FnOnce(AppCore) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, BridgeError>> + Send + 'static,
{
    let core = core()?;
    run(async move { op(core).await }).await
}

/// Serialize a DTO for Dart.
pub(crate) fn to_json<T: Serialize>(value: &T) -> Result<String, BridgeError> {
    serde_json::to_string(value)
        .map_err(|e| BridgeError::internal(format!("cannot encode result: {e}")))
}

/// Parse a DTO sent by Dart. The error names only the position and the
/// category (never the input), so nothing user-typed ends up in messages.
pub(crate) fn from_json<T: DeserializeOwned>(what: &str, json: &str) -> Result<T, BridgeError> {
    serde_json::from_str(json).map_err(|e| {
        BridgeError::invalid(
            what,
            format!(
                "malformed {what} ({:?} at line {} column {})",
                e.classify(),
                e.line(),
                e.column()
            ),
        )
    })
}

/// Take ownership of a secret sent by Dart as UTF-8 bytes. The buffer is
/// reused for the `String` (no copy); invalid input is zeroized.
pub(crate) fn secret_string(field: &str, bytes: Vec<u8>) -> Result<String, BridgeError> {
    String::from_utf8(bytes).map_err(|e| {
        let mut raw = e.into_bytes();
        raw.zeroize();
        BridgeError::invalid(field, "must be valid UTF-8")
    })
}

pub(crate) fn opt_secret_string(
    field: &str,
    bytes: Option<Vec<u8>>,
) -> Result<Option<String>, BridgeError> {
    bytes.map(|b| secret_string(field, b)).transpose()
}

/// A step-up proof from the app: the vault passphrase (bytes) or a
/// presence token from `vault_verify_user_presence`; exactly one.
pub(crate) fn step_up(
    passphrase: Option<Vec<u8>>,
    presence_token: Option<String>,
) -> Result<cc_app_core::StepUp, BridgeError> {
    let passphrase = opt_secret_string("passphrase", passphrase)?;
    Ok(cc_app_core::StepUp::from_parts(passphrase, presence_token)?)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn secret_string_moves_valid_utf8() {
        let s = secret_string("p", b"correct horse".to_vec()).unwrap();
        assert_eq!(s, "correct horse");
    }

    #[test]
    fn secret_string_rejects_invalid_utf8_without_echoing_it() {
        let e = secret_string("passphrase", vec![0xff, 0xfe, b'x']).unwrap_err();
        assert_eq!(e.code, "invalid_input");
        assert_eq!(
            e.details.get("field").map(String::as_str),
            Some("passphrase")
        );
        assert!(!e.message.contains('x'));
    }

    #[test]
    fn from_json_errors_do_not_echo_input() {
        let e =
            from_json::<cc_app_core::HostDto>("host", r#"{"name":"top-secret-host"#).unwrap_err();
        assert_eq!(e.code, "invalid_input");
        assert!(!e.message.contains("top-secret-host"), "{}", e.message);
    }

    #[test]
    fn core_is_not_initialized_by_default() {
        // Other tests may initialize a core; only check the error shape.
        let e = BridgeError::not_initialized();
        assert_eq!(e.code, "not_initialized");
    }

    #[test]
    fn run_executes_on_the_core_runtime() {
        let r = futures_lite_block_on(run(async {
            Ok::<_, BridgeError>(tokio::spawn(async { 7 }).await.unwrap())
        }));
        assert_eq!(r.unwrap(), 7);
    }

    /// Minimal executor for tests that must not run inside the core runtime.
    pub(crate) fn futures_lite_block_on<F: Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(f)
    }
}

#[cfg(test)]
mod shutdown_slot_tests {
    use super::*;
    #[test]
    fn failure_retains_original_and_late_success_cannot_clear_replacement() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let make = |p: &std::path::Path| AppCore::new(cc_app_core::AppConfig::for_tests(p.to_string_lossy().into_owned())).unwrap();
        // Local slots exercise the SAME production transition without racing
        // unrelated bridge fixtures that also use the global application.
        let original = Arc::new(());
        let mut slot = Some(CoreSlot { core: make(first.path()), identity: original.clone() });
        complete_shutdown_slot(&mut slot, &original, &Err(BridgeError::internal("synthetic shutdown denial")));
        assert!(slot.as_ref().is_some_and(|now| Arc::ptr_eq(&now.identity, &original)));
        let replacement = Arc::new(());
        slot = Some(CoreSlot { core: make(second.path()), identity: replacement.clone() });
        complete_shutdown_slot(&mut slot, &original, &Ok(()));
        assert!(slot.as_ref().is_some_and(|now| Arc::ptr_eq(&now.identity, &replacement)));
        complete_shutdown_slot(&mut slot, &replacement, &Ok(()));
        assert!(slot.is_none());
    }
}
