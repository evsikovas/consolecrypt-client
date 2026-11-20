//! Original-owner cancellation observation, never an authority/renewal grant.
use crate::{AutomationError, HelperPeer, Result};
use cc_crypto_core::ServiceOwnerStatusVerifier;
use serde::Deserialize;
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_util::sync::CancellationToken;

fn denied() -> AutomationError {
    AutomationError::Invalid("original service owner status unavailable")
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    delegation: Value,
    statement: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Recorded {
    recorded: bool,
    cancel_requested: u32,
    dispatch: bool,
}

/// Original verifier-produced delivery only. No caller can construct a grant.
pub(crate) struct VerifiedDelivery {
    body: Value,
}
pub(crate) fn verify_and_cancel(
    body: Value,
    original: &ServiceOwnerStatusVerifier,
    stop: &CancellationToken,
) -> Result<VerifiedDelivery> {
    let parsed: Input = serde_json::from_value(body.clone()).map_err(|_| denied())?;
    if !parsed.delegation.is_object() {
        return Err(denied());
    }
    let raw = serde_json::to_vec(&parsed.statement).map_err(|_| denied())?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| denied())?
        .as_secs();
    let verified = original.verify_json(&raw, now).map_err(|_| denied())?;
    // No owner mutex: the original actor can be awaiting SSH connect/auth.
    // A genuinely signed targeted denial closes its ORIGINAL material channel
    // immediately; blocked/failed persistence cannot keep that channel alive.
    // No value from an unsigned body or caller cancellation flag reaches here.
    if verified.invalidates_original() {
        stop.cancel();
    }
    Ok(VerifiedDelivery { body })
}
pub(crate) async fn persist(
    peer: &std::sync::Arc<HelperPeer>,
    delivery: VerifiedDelivery,
) -> Result<Value> {
    let body = delivery.body;
    let response = peer
        .request("service_owner_status", body, Duration::from_secs(30))
        .await?;
    let recorded: Recorded = serde_json::from_value(response.clone()).map_err(|_| denied())?;
    if !recorded.recorded || recorded.dispatch || recorded.cancel_requested > 4096 {
        return Err(denied());
    }
    // Durable acknowledgement/floors remain the SAME existing custody writer.
    // Error/unknown acknowledgement is never reported as persisted success.
    Ok(response)
}

pub(crate) async fn forward(
    peer: &std::sync::Arc<HelperPeer>,
    body: Value,
    original: &ServiceOwnerStatusVerifier,
    stop: &CancellationToken,
) -> Result<Value> {
    let delivery = verify_and_cancel(body, original, stop)?;
    persist(peer, delivery).await
}
