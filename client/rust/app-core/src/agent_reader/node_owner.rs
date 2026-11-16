//! Explicit Reader Node-owner human review seam. No UI activation or Actor ACK.
use super::scope::{ReaderContextSnapshot, ReaderOriginalContext, ReaderSourceSelection};
use crate::{AppCore, AppError, AppResult};
use cc_agent_node_wire::{
    check_live, validate_pin, Certificate, Claim, Now, Offer, OwnerPin, PreparedReaderNodeOwner,
    ReaderNodeOwnerReviewDetails, ReaderNodeOwnerSigned, Signed, Timestamp,
};
use cc_crypto_core::agent_reader::AgentReaderContextV1;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[cfg(test)]
#[path = "node_owner_tests.rs"]
mod tests;

fn denied() -> AppError {
    AppError::invalid("agent_reader", "reader_node_owner_review_unavailable")
}
fn now(scope: &ReaderOriginalContext) -> AppResult<Now> {
    let seconds = u64::try_from(scope.now()?.timestamp()).map_err(|_| denied())?;
    Ok(Now(Timestamp::new(seconds).map_err(|_| denied())?))
}
fn bind(scope: &ReaderOriginalContext, pin: &OwnerPin) -> AppResult<()> {
    validate_pin(pin, now(scope)?).map_err(|_| denied())?;
    // Identical existing tenant/deployment wire shape, not a NativeProfile cast.
    // Local Agent profile remains independently provisioned. Personal requires
    // the actual authenticated subject via the original Reader owner binding.
    let context: AgentReaderContextV1 = serde_json::from_value(
        serde_json::to_value(&pin.context).map_err(|_| denied())?,
    ).map_err(|_| denied())?;
    scope.check_agent_context(&context)
}

/// This one-use value is review material, not evidence that UI approval occurred.
/// Core must expose prepare/details/approve as a deliberate human action; no
/// automatic approve, account-derived Node authority, or retry after uncertainty.
pub(super) struct ReaderNodeOwnerReview {
    scope: Arc<ReaderOriginalContext>,
    retire_ctx: bool,
    prepared: Option<PreparedReaderNodeOwner>,
}
impl std::fmt::Debug for ReaderNodeOwnerReview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReaderNodeOwnerReview([redacted])")
    }
}
impl Drop for ReaderNodeOwnerReview {
    fn drop(&mut self) {
        if self.retire_ctx {
            self.scope.retire();
        }
    }
}
impl ReaderNodeOwnerReview {
    pub(super) async fn enrollment_offer(
        app: &AppCore,
        selection: ReaderSourceSelection,
        pin: &OwnerPin,
        offer: Offer,
        cancel: &CancellationToken,
    ) -> AppResult<Self> {
        let scope = Arc::new(ReaderOriginalContext::capture(app, selection, cancel).await?);
        Self::enrollment_offer_prepared(app, scope, true, pin, offer).await
    }
    pub(super) async fn enrollment_offer_from_original(
        app: &AppCore,
        scope: Arc<ReaderOriginalContext>,
        pin: &OwnerPin,
        offer: Offer,
    ) -> AppResult<Self> {
        scope.scope_identity().check(&scope)?;
        Self::enrollment_offer_prepared(app, scope, false, pin, offer).await
    }
    async fn enrollment_offer_prepared(
        app: &AppCore,
        scope: Arc<ReaderOriginalContext>,
        retire_ctx: bool,
        pin: &OwnerPin,
        offer: Offer,
    ) -> AppResult<Self> {
        scope.check(app).await?;
        let result = async {
            bind(&scope, pin)?;
            let prepared =
                PreparedReaderNodeOwner::offer(pin, offer, now(&scope)?).map_err(|_| denied())?;
            scope
                .with_held(app, |held| {
                    bind(&scope, pin)?;
                    if held.identity.public_keys().signing != *pin.key.as_bytes() {
                        return Err(denied());
                    }
                    prepared.recheck(now(&scope)?).map_err(|_| denied())
                })
                .await?;
            Ok(prepared)
        }
        .await;
        match result {
            Ok(prepared) => Ok(Self {
                scope,
                retire_ctx,
                prepared: Some(prepared),
            }),
            Err(error) => {
                if retire_ctx {
                    scope.retire();
                }
                Err(error)
            }
        }
    }
    pub(super) async fn enrollment_certificate(
        app: &AppCore,
        selection: ReaderSourceSelection,
        pin: &OwnerPin,
        offer: Signed<Offer>,
        claim: Signed<Claim>,
        cert: Certificate,
        cancel: &CancellationToken,
    ) -> AppResult<Self> {
        let scope = Arc::new(ReaderOriginalContext::capture(app, selection, cancel).await?);
        Self::enrollment_certificate_prepared(app, scope, true, pin, offer, claim, cert).await
    }
    pub(super) async fn enrollment_certificate_from_original(
        app: &AppCore,
        scope: Arc<ReaderOriginalContext>,
        pin: &OwnerPin,
        offer: Signed<Offer>,
        claim: Signed<Claim>,
        cert: Certificate,
    ) -> AppResult<Self> {
        scope.scope_identity().check(&scope)?;
        Self::enrollment_certificate_prepared(app, scope, false, pin, offer, claim, cert).await
    }
    async fn enrollment_certificate_prepared(
        app: &AppCore,
        scope: Arc<ReaderOriginalContext>,
        retire_ctx: bool,
        pin: &OwnerPin,
        offer: Signed<Offer>,
        claim: Signed<Claim>,
        cert: Certificate,
    ) -> AppResult<Self> {
        scope.check(app).await?;
        let result = async {
            bind(&scope, pin)?;
            let prepared =
                PreparedReaderNodeOwner::certificate(pin, offer, claim, cert, now(&scope)?)
                    .map_err(|_| denied())?;
            scope
                .with_held(app, |held| {
                    bind(&scope, pin)?;
                    if held.identity.public_keys().signing != *pin.key.as_bytes() {
                        return Err(denied());
                    }
                    prepared.recheck(now(&scope)?).map_err(|_| denied())
                })
                .await?;
            Ok(prepared)
        }
        .await;
        match result {
            Ok(prepared) => Ok(Self {
                scope,
                retire_ctx,
                prepared: Some(prepared),
            }),
            Err(error) => {
                if retire_ctx {
                    scope.retire();
                }
                Err(error)
            }
        }
    }
    pub(super) fn uses_original(&self, ctx: &Arc<ReaderOriginalContext>) -> bool {
        Arc::ptr_eq(&self.scope, ctx)
    }
    pub(super) fn details(&self) -> AppResult<&ReaderNodeOwnerReviewDetails> {
        let prepared = self.prepared.as_ref().ok_or_else(denied)?;
        prepared.recheck(now(&self.scope)?).map_err(|_| denied())?;
        Ok(prepared.details())
    }
    pub(super) fn source_snapshot(&self) -> &ReaderContextSnapshot {
        self.scope.snapshot()
    }
    /// Caller independently reads the current pin again before approval. Equality
    /// is a local review fence; actual Actor current-state/CAS is still required.
    pub(super) async fn approve_once(
        mut self,
        app: &AppCore,
        current_pin: &OwnerPin,
    ) -> AppResult<ReaderNodeOwnerSigned> {
        let prepared = self.prepared.take().ok_or_else(denied)?;
        let details = prepared.details().clone();
        if current_pin != &details.owner_pin {
            return Err(denied());
        }
        let result = self.scope.with_held(app, |held| {
            bind(&self.scope, current_pin)?;
            held.identity.sign_agent_reader_node_owner_v1(prepared, now(&self.scope)?)
                .map_err(|_| denied())
        }).await?;
        self.scope.check(app).await?;
        bind(&self.scope, current_pin)?;
        check_live(details.signing_not_before, details.signing_expires_at, now(&self.scope)?)
            .map_err(|_| denied())?;
        Ok(result)
    }
}
