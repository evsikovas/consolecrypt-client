use crate::api::error::BridgeError;
use crate::state::with_core;

/// Local KDBX key protection; plaintext crosses only for explicit enrollment/release.
pub async fn password_vault_key(
    action: String,
    id: String,
    method: String,
    secret: Vec<u8>,
    pin: Option<String>,
) -> Result<Vec<u8>, BridgeError> {
    // Wipe even when with_core rejects the call before invoking the facade.
    let secret = zeroize::Zeroizing::new(secret);
    let pin = pin.map(zeroize::Zeroizing::new);
    with_core(move |c| async move {
        c.password_vault_key(
            action,
            id,
            method,
            secret.to_vec(),
            pin.as_ref().map(|p| p.to_string()),
        )
        .await
        .map_err(Into::into)
    })
    .await
}
