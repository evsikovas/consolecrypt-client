//! Sync engine and trusted devices (ADR-0003/0004/0106).
//!
//! JSON results: `SyncStatusDto`, `SyncReportDto`, `ProfileInfo`,
//! `AuthStateDto`, `DeviceListDto`, `OwnTrustRequestDto`,
//! `PendingApprovalDto`, `DeviceDto`.

use crate::api::error::BridgeError;
use crate::state::{secret_string, to_json, with_core};
use cc_app_core::{AccountMode, AppError};
use std::collections::HashMap;
use std::sync::Mutex;

pub async fn sync_status() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.sync_status().await?) }).await
}

/// Push the outbox and pull now → `SyncReportDto`.
pub async fn sync_now() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.sync_now().await?) }).await
}

/// Local → Synced (register or log in, upload the vault) → `ProfileInfo`.
pub async fn sync_enable(
    server_url: String,
    email: String,
    password: Vec<u8>,
    register: bool,
) -> Result<String, BridgeError> {
    let password = secret_string("account_password", password)?;
    let mode = if register {
        AccountMode::Register
    } else {
        AccountMode::Login
    };
    with_core(
        move |c| async move { to_json(&c.enable_sync(server_url, email, password, mode).await?) },
    )
    .await
}

/// Synced → Local (keeps all data); optionally revoke this device.
pub async fn sync_disconnect(revoke_device: bool) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.disconnect(revoke_device).await?) }).await
}

/// Sign in again after the session expired → `AuthStateDto`.
pub async fn sync_reauthenticate(
    password: Vec<u8>,
    allow_new_device_identity: bool,
) -> Result<String, BridgeError> {
    let password = secret_string("account_password", password)?;
    with_core(move |c| async move {
        to_json(
            &c.reauthenticate(password, allow_new_device_identity)
                .await?,
        )
    })
    .await
}

// ---- devices -----------------------------------------------------------------------

pub async fn devices_list() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.list_devices().await?) }).await
}

/// This installation's verification code (6 groups of 5 digits).
pub async fn devices_own_code() -> Result<String, BridgeError> {
    with_core(move |c| async move { Ok(c.device_verification_code().await?) }).await
}

/// New device: ask trusted devices for approval → `OwnTrustRequestDto`.
pub async fn devices_request_approval(vault_id: Option<String>) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.request_device_approval(vault_id).await?) }).await
}

/// New device, after approval: open the vault with the device key.
/// `false` = not approved yet.
pub async fn devices_finish_approval(vault_id: Option<String>) -> Result<bool, BridgeError> {
    with_core(move |c| async move { Ok(c.finish_device_approval(vault_id).await?) }).await
}

/// Codes computed by the core in step 1, per request (for the re-check).
static PENDING_CODES: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

fn pending_codes<R>(f: impl FnOnce(&mut HashMap<String, String>) -> R) -> R {
    let mut g = PENDING_CODES.lock().unwrap_or_else(|p| p.into_inner());
    f(g.get_or_insert_with(HashMap::new))
}

pub(crate) fn digits(code: &str) -> String {
    code.chars().filter(char::is_ascii_digit).collect()
}

/// Step 1 of approving another device → `PendingApprovalDto` (show its
/// `verification_code` next to the one on the new device).
pub async fn devices_start_approval(request_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        let pending = c.start_device_approval(request_id.clone()).await?;
        pending_codes(|m| m.insert(request_id, digits(&pending.verification_code)));
        to_json(&pending)
    })
    .await
}

/// Step 2, only after the user confirmed both codes match. The bridge
/// re-checks `confirmed_code` against the code the core computed in step 1
/// (`approval` error on mismatch); the core refuses without step 1.
pub async fn devices_confirm_approval(
    request_id: String,
    confirmed_code: String,
) -> Result<(), BridgeError> {
    let expected = pending_codes(|m| m.get(&request_id).cloned());
    match expected {
        Some(code) if !code.is_empty() && code == digits(&confirmed_code) => {}
        Some(_) => {
            return Err(AppError::Approval(
                "the confirmed verification code does not match".into(),
            )
            .into());
        }
        None => {
            return Err(AppError::Approval("start the approval first".into()).into());
        }
    }
    with_core(move |c| async move {
        c.confirm_device_approval(request_id.clone()).await?;
        pending_codes(|m| m.remove(&request_id));
        Ok(())
    })
    .await
}

pub async fn devices_reject(request_id: String) -> Result<(), BridgeError> {
    pending_codes(|m| m.remove(&request_id));
    with_core(move |c| async move { Ok(c.reject_device_request(request_id).await?) }).await
}

pub async fn devices_revoke(device_id: String, reason: Option<String>) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.revoke_device(device_id, reason).await?) }).await
}

/// → `DeviceDto`.
pub async fn devices_rename(device_id: String, name: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.rename_device(device_id, name).await?) }).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_normalizes_separators() {
        assert_eq!(digits("12345 67890-11111"), "123456789011111");
    }

    #[test]
    fn confirm_without_start_is_refused_before_touching_the_core() {
        let r = crate::state::tests::futures_lite_block_on(devices_confirm_approval(
            "unknown-request".into(),
            "00000 00000 00000 00000 00000 00000".into(),
        ));
        assert_eq!(r.unwrap_err().code, "approval");
    }

    #[test]
    fn confirm_with_a_different_code_is_refused() {
        pending_codes(|m| {
            m.insert(
                "req-1".into(),
                digits("11111 22222 33333 44444 55555 66666"),
            )
        });
        let r = crate::state::tests::futures_lite_block_on(devices_confirm_approval(
            "req-1".into(),
            "11111 22222 33333 44444 55555 66667".into(),
        ));
        assert_eq!(r.unwrap_err().code, "approval");
    }
}
