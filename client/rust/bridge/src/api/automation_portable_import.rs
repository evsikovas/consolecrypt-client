//! A retained import review is distinct from a completed export/inspection.
//! All actions retain the originally issued operation, including after Core replacement.
use super::automation_portable::PortablePasswordTransfer;
use crate::{api::error::BridgeError, state};
use std::sync::Arc;

fn unavailable() -> BridgeError {
    BridgeError::invalid("portable_import", "portable_operation_unavailable")
}

fn import_error(error: cc_app_core::AppError) -> BridgeError {
    // Only these fixed native rules describe known failures. Never expose paths,
    // package contents, SQL diagnostics, or arbitrary underlying error text.
    if let cc_app_core::AppError::InvalidInput { field, reason } = &error {
        if field == "portable_import"
            && matches!(
                reason.as_str(),
                "portable_context_changed"
                    | "portable_import_source_changed"
                    | "portable_import_unsupported"
                    | "portable_import_busy"
                    | "portable_import_expired"
                    | "portable_import_recovery_required"
            )
        {
            return BridgeError::invalid("portable_import", reason);
        }
    }
    unavailable()
}

#[flutter_rust_bridge::frb(opaque)]
pub struct PortableImportHandle {
    inner: Arc<cc_app_core::PortableOperation>,
}

impl Drop for PortableImportHandle {
    fn drop(&mut self) {
        self.inner.cancel();
    }
}

impl PortableImportHandle {
    /// Returns Ready without joining: the genuine decrypted witness stays owned
    /// until explicit acceptance, cancellation, lock, or native expiry.
    pub async fn start(&self) -> Result<String, BridgeError> {
        let operation = self.inner.clone();
        state::run(async move { operation.start().await.map_err(import_error) }).await
    }

    pub async fn accept_import(
        &self,
        review_id: String,
        review_digest: String,
    ) -> Result<String, BridgeError> {
        let operation = self.inner.clone();
        state::run(async move {
            operation
                .accept_import(review_id, review_digest)
                .await
                .map_err(import_error)
        })
        .await
    }

    #[flutter_rust_bridge::frb(sync)]
    pub fn cancel(&self) {
        self.inner.cancel();
    }

    pub async fn joined(&self) -> Result<(), BridgeError> {
        let operation = self.inner.clone();
        state::run(async move {
            operation.joined().await;
            Ok(())
        })
        .await
    }
}

/// Registration acknowledges ownership; it never decrypts or publishes a project.
pub async fn automation_portable_register_import(
    scope_json: String,
    source: String,
    requested_name: String,
    password: &PortablePasswordTransfer,
) -> Result<PortableImportHandle, BridgeError> {
    let core = state::core()?;
    let transfer = password.inner.clone();
    state::run(async move {
        let operation = core
            .automation_portable_register_import_json(scope_json, source, requested_name, transfer)
            .await
            .map_err(import_error)?;
        Ok(PortableImportHandle {
            inner: Arc::new(operation),
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_errors_preserve_only_closed_native_rules() {
        for rule in [
            "portable_context_changed",
            "portable_import_source_changed",
            "portable_import_recovery_required",
        ] {
            assert_eq!(
                import_error(cc_app_core::AppError::InvalidInput { field: "portable_import".into(), reason: rule.into() }),
                BridgeError::invalid("portable_import", rule)
            );
        }
        for (field, rule) in [
            ("other", "portable_context_changed"),
            ("portable_import", "secret path or SQL diagnostic"),
        ] {
            assert_eq!(
                import_error(cc_app_core::AppError::InvalidInput { field: field.into(), reason: rule.into() }),
                unavailable()
            );
        }
        assert_eq!(
            import_error(cc_app_core::AppError::Cancelled),
            unavailable()
        );
    }
}
