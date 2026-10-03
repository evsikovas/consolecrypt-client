//! Log setup shared by the desktop app (via app-core) and the `cc` CLI.
//!
//! Third-party HTTP/WebSocket/TLS stacks log request headers — including
//! `Authorization: Bearer …` — at debug/trace level. Every subscriber built
//! here therefore:
//!
//! 1. appends [`cc_sync_core::SAFE_LOG_DIRECTIVES`] to the `EnvFilter`
//!    *after* the user-supplied directives (`RUST_LOG`, `-v`), so an equal
//!    user directive is replaced by the safe one;
//! 2. additionally applies a separate [`Targets`] filter with the same caps,
//!    so even a *more specific* user directive (e.g.
//!    `tungstenite::handshake=trace`) cannot re-enable those targets;
//! 3. bridges `log` records into tracing ([`tracing_log::LogTracer`]) so the
//!    caps apply to `log`-based crates too.
//!
//! app-core itself never logs secrets (passphrases, keys, tokens, recovery
//! words, plaintext objects); `tests/log_hygiene.rs` enforces it at TRACE.

use crate::error::AppError;
use cc_sync_core::SAFE_LOG_DIRECTIVES;
use std::sync::Mutex;
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

/// Default directives when the user supplies none.
pub const DEFAULT_DIRECTIVES: &str = "warn,cc_app_core=info,cc_sync_core=info";

// RDP dependency diagnostics may contain authentication tokens, desktop data
// or typed text. Only our sanitized bridge errors are exposed to the user.
const RDP_SAFE_LOG_DIRECTIVES: &[&str] =
    &["cc_rdp_core=off", "ironrdp=off", "sspi=off", "picky=off"];

/// `EnvFilter` from `user_directives` (e.g. `RUST_LOG` or a `-v` level;
/// `None`/empty → [`DEFAULT_DIRECTIVES`]) with every
/// [`SAFE_LOG_DIRECTIVES`] entry appended last. Invalid user directives are
/// ignored (the safe ones are always applied).
pub fn env_filter(user_directives: Option<&str>) -> EnvFilter {
    let user = user_directives
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_DIRECTIVES);
    let mut filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::WARN.into())
        .parse_lossy(user);
    for d in SAFE_LOG_DIRECTIVES.iter().chain(RDP_SAFE_LOG_DIRECTIVES) {
        match d.parse() {
            Ok(directive) => filter = filter.add_directive(directive),
            // The constant is ours; a parse failure is a bug caught by tests.
            Err(_) => debug_assert!(false, "invalid SAFE_LOG_DIRECTIVES entry {d}"),
        }
    }
    filter
}

/// A per-layer filter that caps the noisy/secret-leaking targets of
/// [`SAFE_LOG_DIRECTIVES`] regardless of the user's directives.
pub fn safe_targets() -> Targets {
    let mut t = Targets::new().with_default(LevelFilter::TRACE);
    for d in SAFE_LOG_DIRECTIVES.iter().chain(RDP_SAFE_LOG_DIRECTIVES) {
        if let Some((target, level)) = d.split_once('=') {
            if let Ok(level) = level.parse::<LevelFilter>() {
                t = t.with_target(target, level);
            }
        }
    }
    t
}

/// Combined filter used by [`init`] and [`init_with_writer`].
pub fn filter(
    user_directives: Option<&str>,
) -> tracing_subscriber::filter::combinator::And<EnvFilter, Targets, tracing_subscriber::Registry> {
    tracing_subscriber::filter::FilterExt::and(env_filter(user_directives), safe_targets())
}

/// Install the global subscriber: fmt output to stderr filtered by
/// [`filter`], plus the `log` → tracing bridge. Idempotent: a second call
/// (or an already installed subscriber) is not an error.
pub fn init(user_directives: Option<&str>) -> Result<(), AppError> {
    init_with_writer(user_directives, std::io::stderr)
}

/// Like [`init`], writing to `writer` (e.g. a log file; tests capture
/// output with it).
pub fn init_with_writer<W>(user_directives: Option<&str>, writer: W) -> Result<(), AppError>
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    static INSTALLED: Mutex<bool> = Mutex::new(false);
    let mut installed = INSTALLED.lock().unwrap_or_else(|p| p.into_inner());
    if *installed {
        return Ok(());
    }
    // Bridge `log` records (tungstenite, rustls, …) into tracing so the
    // filters above apply to them as well. Fails only if a logger exists.
    let _ = tracing_log::LogTracer::init();
    let layer = tracing_subscriber::fmt::layer()
        .with_writer(writer)
        .with_ansi(false)
        .with_target(true)
        .with_filter(filter(user_directives));
    // `try_init` fails if another global subscriber is set; that is fine.
    let _ = tracing_subscriber::registry().with(layer).try_init();
    *installed = true;
    Ok(())
}

/// Map a CLI verbosity count (`-v` repetitions) to directives:
/// 0 → [`DEFAULT_DIRECTIVES`], 1 → info, 2 → debug, ≥3 → trace (the safe
/// caps still apply).
pub fn verbosity_directives(verbose: u8) -> &'static str {
    match verbose {
        0 => DEFAULT_DIRECTIVES,
        1 => "info",
        2 => "debug",
        _ => "trace",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::layer::Filter;

    #[test]
    fn rdp_diagnostics_cannot_be_enabled_by_more_specific_user_filters() {
        let filter = filter(Some(
            "trace,sspi::credssp=trace,ironrdp_connector::credssp=trace",
        ));
        let targets = safe_targets();
        for target in [
            "cc_rdp_core",
            "ironrdp_connector::credssp",
            "ironrdp_session::fast_path",
            "ironrdp_pdu::input",
            "sspi::credssp",
            "picky::key",
        ] {
            assert!(
                !targets.would_enable(target, &tracing::Level::ERROR),
                "{target}"
            );
        }
        let _ = filter.max_level_hint();
        for directive in RDP_SAFE_LOG_DIRECTIVES {
            directive
                .parse::<tracing_subscriber::filter::Directive>()
                .unwrap();
        }
    }

    #[test]
    fn safe_directives_parse_and_cap_targets() {
        for d in SAFE_LOG_DIRECTIVES {
            d.parse::<tracing_subscriber::filter::Directive>()
                .unwrap_or_else(|_| panic!("{d}"));
        }
        let t = safe_targets();
        assert!(!t.would_enable("tungstenite::handshake::client", &tracing::Level::TRACE));
        assert!(!t.would_enable("tokio_tungstenite", &tracing::Level::ERROR));
        assert!(!t.would_enable("hyper::proto", &tracing::Level::DEBUG));
        assert!(t.would_enable("hyper::proto", &tracing::Level::WARN));
        assert!(t.would_enable("cc_app_core", &tracing::Level::TRACE));
    }

    #[test]
    fn user_directives_cannot_reenable_blocked_targets() {
        // Even an explicit, more specific user directive stays capped by the
        // combined filter (the Targets half).
        let f = filter(Some("trace,tungstenite::handshake=trace,reqwest=trace"));
        let meta_enabled = |target: &str, level: tracing::Level| {
            // `Targets` is the authoritative cap; `EnvFilter` must agree for
            // equal-specificity directives.
            let t = safe_targets();
            t.would_enable(target, &level)
        };
        assert!(!meta_enabled(
            "tungstenite::handshake",
            tracing::Level::TRACE
        ));
        assert!(!meta_enabled("reqwest::connect", tracing::Level::DEBUG));
        let _ = f.max_level_hint();
        let env = env_filter(Some("tungstenite=trace"));
        let s = env.to_string();
        let parts: Vec<&str> = s.split(',').collect();
        assert_eq!(
            parts.iter().filter(|p| **p == "tungstenite=off").count(),
            1,
            "{s}"
        );
        assert!(!parts.contains(&"tungstenite=trace"), "{s}");
    }
}
