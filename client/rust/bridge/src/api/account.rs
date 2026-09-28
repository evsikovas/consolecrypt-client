//! Account operations of synced profiles and the explicit "reveal secret"
//! action (ADR-0107 "Account").

use crate::api::error::BridgeError;
use crate::state::{run, secret_string, with_core};

/// Sign out of the open synced profile's account: revoke the session on the
/// server (best effort offline), forget the tokens, stop sync. Local data
/// stays; `sync_reauthenticate` signs in again. Emits `signed_out`.
pub async fn account_logout() -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.logout().await?) }).await
}

/// Ask the server to send an account password-reset e-mail (always
/// accepted). No profile needed; does not grant vault access.
pub async fn account_request_password_reset(
    server_url: String,
    email: String,
) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.request_password_reset(server_url, email).await?) }).await
}

/// Change the account password (`invalid_credentials` + reason
/// `current_password_wrong`, `invalid_input` + reason
/// `account_password_too_short` below 12 characters).
pub async fn account_change_password(current: Vec<u8>, next: Vec<u8>) -> Result<(), BridgeError> {
    let current = secret_string("current_password", current)?;
    let next = secret_string("new_password", next)?;
    with_core(move |c| async move { Ok(c.change_account_password(current, next).await?) }).await
}

/// **Explicit user action only** (Reveal / Copy): the secret of a password
/// or key credential as UTF-8 bytes, returned once. app-core zeroizes its
/// copies and logs an audit line without the value; the buffer handed to
/// FRB is not zeroized by FRB (ADR-0101 §6 limit) — wipe the Dart copy
/// (`SecretText`) after use.
pub async fn credentials_reveal_secret(credential_id: String) -> Result<Vec<u8>, BridgeError> {
    run(async move {
        let core = crate::state::core()?;
        let mut revealed = core.reveal_credential_secret(credential_id).await?;
        Ok(revealed.take_bytes())
    })
    .await
}
