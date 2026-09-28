//! # cc-bridge — flutter_rust_bridge surface of the ConsoleCrypt core
//!
//! Wraps one global [`cc_app_core::AppCore`] (ADR-0107) for the Flutter UI
//! (ADR-0101 §8). Everything under [`api`] is exported to Dart by
//! `flutter_rust_bridge_codegen generate` (config:
//! `client/flutter/flutter_rust_bridge.yaml`, output:
//! `client/flutter/lib/src/rust/`).
//!
//! Design rules:
//!
//! * **DTOs cross as JSON** (the serde form of `cc_app_core::dto`, the wire
//!   contract documented in ADR-0107: snake_case fields, enum wire names,
//!   Unix-ms timestamps). The Dart side maps them to `lib/core/models`
//!   (`lib/core/bridge/mapping.dart`). This keeps the FRB surface stable
//!   while app-core DTOs grow (new fields / events / AI types never break the
//!   generated bindings) and avoids mirroring every cc-models enum.
//! * **Typed** FRB structs/enums only where the bridge owns the shape:
//!   configuration, errors, secret inputs, terminal frames, transfers.
//! * **Secrets are inbound only** (`Vec<u8>` from Dart's `SecretText`), are
//!   moved (never copied) into the `String` app-core wraps in
//!   `secrecy`/`zeroize`, and invalid UTF-8 input is zeroized.
//! * One error type, [`api::error::BridgeError`] (`code` = app-core's stable
//!   error code, ADR-0107).
//! * All core futures run on one dedicated tokio runtime ([`state`]) so
//!   background tasks (sync engine, terminals, tunnels) outlive the call.

pub mod api;
mod state;

#[cfg(target_os = "android")]
#[allow(unsafe_code)] // JNI boundary; see safety notes in the module.
mod android;

/// Rust-only hook for this crate's integration tests (outside `api`, so
/// FRB does not export it): the running core, if initialized.
#[doc(hidden)]
pub fn core_for_tests() -> Option<cc_app_core::AppCore> {
    state::core_opt()
}

#[allow(
    unsafe_code,
    missing_debug_implementations,
    unused_qualifications,
    clippy::all
)]
mod frb_generated;
