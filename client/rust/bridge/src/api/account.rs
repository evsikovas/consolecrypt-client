//! Account operations of synced profiles and the explicit "reveal secret"
//! action (ADR-0107 "Account").

use crate::api::error::BridgeError;
use crate::state::{run, secret_string, to_json, with_core};
use cc_app_core::enterprise_auth::EnterpriseSignInPurpose;

/// Sign out of the open synced profile's account: revoke the session on the
/// server (best effort offline), forget the tokens, stop sync. Local data
/// stays; `sync_reauthenticate` signs in again. Emits `signed_out`.
pub async fn account_logout() -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.logout().await?) }).await
}

/// "Sign out on all devices" → `LogoutAllDto` JSON. From a new device
/// (protocol 1.7 quarantine) trusted devices stay signed in and
/// `trusted_devices_kept` says how many. Emits `signed_out`.
pub async fn account_logout_all() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.logout_all_sessions().await?) }).await
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

// ---- enterprise sign-in (protocol 1.8) ------------------------------------------------

/// Start a browser (OIDC) sign-in → `EnterpriseOidcChallengeDto` JSON.
/// `purpose` = `new_profile` | `reauthenticate` | `enable_sync`. Open
/// `authorization_url` in the system browser, then call
/// [`enterprise_oidc_complete`] (desktop: without a callback URL — the core
/// receives the loopback redirect; mobile: with the URL the OS auth session
/// returned).
pub async fn enterprise_oidc_begin(
    server_url: String,
    purpose: String,
    login_hint: Option<String>,
    display_name: Option<String>,
    allow_new_device_identity: bool,
) -> Result<String, BridgeError> {
    let purpose = purpose.parse::<EnterpriseSignInPurpose>()?;
    with_core(move |c| async move {
        to_json(
            &c.enterprise_oidc_begin(
                server_url,
                purpose,
                login_hint,
                display_name,
                allow_new_device_identity,
            )
            .await?,
        )
    })
    .await
}

/// Finish a browser sign-in → `EnterpriseSignInResultDto` JSON (`account`
/// for a new profile, `auth_state` after re-authentication, `profile` after
/// enable-sync). The callback URL carries a single-use code: it is checked
/// against the flow's `state` and never stored.
pub async fn enterprise_oidc_complete(
    ticket: String,
    callback_url: Option<String>,
) -> Result<String, BridgeError> {
    with_core(
        move |c| async move { to_json(&c.enterprise_oidc_complete(ticket, callback_url).await?) },
    )
    .await
}

/// Abandon a browser sign-in (removes a new profile prepared for it).
pub async fn enterprise_oidc_cancel(ticket: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.enterprise_oidc_cancel(ticket).await?) }).await
}

/// New synced profile signed in with the organisation's directory (LDAP /
/// Active Directory) → `SyncedAccountDto` JSON. The password is sent once
/// over TLS and never stored.
pub async fn profiles_create_synced_ldap(
    display_name: Option<String>,
    server_url: String,
    login: String,
    password: Vec<u8>,
) -> Result<String, BridgeError> {
    let password = secret_string("password", password)?;
    with_core(move |c| async move {
        to_json(
            &c.create_synced_profile_ldap(display_name, server_url, login, password)
                .await?,
        )
    })
    .await
}

/// Sign the open synced profile in again with the directory → `AuthStateDto`.
pub async fn sync_reauthenticate_ldap(
    login: String,
    password: Vec<u8>,
    allow_new_device_identity: bool,
) -> Result<String, BridgeError> {
    let password = secret_string("password", password)?;
    with_core(move |c| async move {
        to_json(
            &c.reauthenticate_ldap(login, password, allow_new_device_identity)
                .await?,
        )
    })
    .await
}

/// Local → Synced signing in with the directory → `ProfileInfo` JSON
/// (emits `enable_sync_progress`).
pub async fn sync_enable_ldap(
    server_url: String,
    login: String,
    password: Vec<u8>,
) -> Result<String, BridgeError> {
    let password = secret_string("password", password)?;
    with_core(
        move |c| async move { to_json(&c.enable_sync_ldap(server_url, login, password).await?) },
    )
    .await
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
