//! The single error type of every bridge call.

use cc_app_core::AppError;
use std::collections::HashMap;

/// Error of any bridge function (Dart: thrown `BridgeError`, mapped to
/// `AppException` in `lib/core/bridge/mapping.dart`).
///
/// * `code` — app-core's stable [`AppError::code`] (ADR-0107) or a bridge
///   code: `not_initialized`, `internal`.
/// * `message` — English, secret-free diagnostic (never the primary UI text).
/// * `reason` — optional refinement: `AppError::reason`, the wire name of
///   the Dart `AppErrorReason` (`current_passphrase_wrong`, `host_not_found`,
///   `directory_not_found`, …).
/// * `details` — `AppError::args`, non-secret structured data (`field`,
///   `rule`, `what`, `id`, `name`, `path`, `used_by`, `retry_after_seconds`,
///   `status`, `protocol_code`, `names`, `size`, `limit`, `diagnostic`, …);
///   the Dart side passes them as `AppException.args`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeError {
    pub code: String,
    pub message: String,
    pub reason: Option<String>,
    pub details: HashMap<String, String>,
}

impl BridgeError {
    pub(crate) fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
            reason: None,
            details: HashMap::new(),
        }
    }

    pub(crate) fn with(mut self, key: &str, value: impl Into<String>) -> Self {
        self.details.insert(key.to_owned(), value.into());
        self
    }

    pub(crate) fn not_initialized() -> Self {
        Self::new(
            "not_initialized",
            "the core is not initialized (call core_init first)",
        )
    }

    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self::new("internal", message)
    }

    pub(crate) fn invalid(field: &str, rule: impl Into<String>) -> Self {
        let rule = rule.into();
        Self::new("invalid_input", format!("invalid {field}: {rule}"))
            .with("field", field)
            .with("rule", rule)
    }
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for BridgeError {}

impl BridgeError {
    /// From a stored error snapshot (e.g. a failed transfer job).
    pub(crate) fn from_info(e: &cc_app_core::ErrorInfoDto) -> Self {
        Self {
            code: e.code.clone(),
            message: e.message.clone(),
            reason: e.reason.clone(),
            details: e.args.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        }
    }
}

/// `code` = [`AppError::code`], `reason` = [`AppError::reason`] (the Dart
/// `AppErrorReason` wire name), `details` = [`AppError::args`] (non-secret).
impl From<AppError> for BridgeError {
    fn from(e: AppError) -> Self {
        Self {
            code: e.code().to_owned(),
            message: e.message(),
            reason: e.reason().map(str::to_owned),
            details: e.args().into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_errors_keep_code_and_structured_details() {
        let e: BridgeError = AppError::InvalidInput {
            field: "port".into(),
            reason: "must be 1..=65535".into(),
        }
        .into();
        assert_eq!(e.code, "invalid_input");
        assert_eq!(e.details["field"], "port");
        assert_eq!(e.details["rule"], "must be 1..=65535");

        let e: BridgeError = AppError::RateLimited {
            retry_after_secs: Some(30),
        }
        .into();
        assert_eq!(e.code, "rate_limited");
        assert_eq!(e.details["retry_after_seconds"], "30");

        let e: BridgeError = AppError::Server {
            status: 410,
            code: "gone".into(),
            message: "expired".into(),
        }
        .into();
        assert_eq!(e.code, "server");
        assert_eq!(e.details["protocol_code"], "gone");

        let e: BridgeError = AppError::WrongPassphrase.into();
        assert_eq!(e.code, "wrong_passphrase");
        assert!(e.details.is_empty());
        assert!(e.reason.is_none());

        let e: BridgeError = AppError::WrongCurrentPassphrase.into();
        assert_eq!(e.code, "wrong_passphrase");
        assert_eq!(e.reason.as_deref(), Some("current_passphrase_wrong"));

        let e: BridgeError = AppError::NotFound {
            what: "host".into(),
            id: "h1".into(),
        }
        .into();
        assert_eq!(e.reason.as_deref(), Some("host_not_found"));
        assert_eq!(e.details["id"], "h1");

        let e: BridgeError = AppError::TooLarge {
            what: "file".into(),
            size: 60,
            limit: 50,
        }
        .into();
        assert_eq!(e.code, "payload_too_large");
        assert_eq!(
            (e.details["size"].as_str(), e.details["limit"].as_str()),
            ("60", "50")
        );

        let e: BridgeError = AppError::InvalidInput {
            field: "email".into(),
            reason: "must be an email address".into(),
        }
        .into();
        assert_eq!(e.reason.as_deref(), Some("invalid_email"));
        let e: BridgeError = AppError::AlreadyExists("email".into()).into();
        assert_eq!(
            (e.code.as_str(), e.reason.as_deref()),
            ("already_exists", None)
        );
    }
}
