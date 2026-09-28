//! HTTP routes of protocol v1. Path parameters are written `{name}` (Axum
//! 0.8 syntax). Unversioned operational endpoints are listed separately.

pub const META: &str = "/v1/meta";

pub const AUTH_REGISTER: &str = "/v1/auth/register";
pub const AUTH_LOGIN: &str = "/v1/auth/login";
pub const AUTH_REFRESH: &str = "/v1/auth/refresh";
pub const AUTH_LOGOUT: &str = "/v1/auth/logout";
pub const AUTH_ME: &str = "/v1/auth/me";
pub const AUTH_PASSWORD_FORGOT: &str = "/v1/auth/password/forgot";
pub const AUTH_PASSWORD_RESET: &str = "/v1/auth/password/reset";
pub const AUTH_PASSWORD_CHANGE: &str = "/v1/auth/password/change";
pub const AUTH_EMAIL_VERIFY: &str = "/v1/auth/email/verify";

pub const DEVICES: &str = "/v1/devices";
pub const DEVICE: &str = "/v1/devices/{device_id}";
pub const DEVICE_APPROVE: &str = "/v1/devices/{device_id}/approve";
pub const DEVICE_REJECT: &str = "/v1/devices/{device_id}/reject";
pub const DEVICE_ATTEST: &str = "/v1/devices/{device_id}/attest";
pub const DEVICE_REVOKE: &str = "/v1/devices/{device_id}/revoke";

pub const VAULTS: &str = "/v1/vaults";
pub const VAULT: &str = "/v1/vaults/{vault_id}";
pub const VAULT_ENVELOPES: &str = "/v1/vaults/{vault_id}/envelopes";
pub const VAULT_ENVELOPE: &str = "/v1/vaults/{vault_id}/envelopes/{envelope_id}";

pub const SYNC_PUSH: &str = "/v1/sync/push";
pub const SYNC_CHANGES: &str = "/v1/sync/changes";
pub const SYNC_SNAPSHOT: &str = "/v1/sync/snapshot";

pub const EVENTS_WS: &str = "/v1/events/ws";

pub const RECOVERY_ACCOUNT_START: &str = "/v1/recovery/account/start";
pub const RECOVERY_ACCOUNT_CONFIRM: &str = "/v1/recovery/account/confirm";
pub const RECOVERY_VAULT_ENVELOPE: &str = "/v1/recovery/vault/envelope";
pub const RECOVERY_VAULT_PASSWORD_REPLACE: &str = "/v1/recovery/vault/password-envelope/replace";
pub const RECOVERY_VAULT_RECOVERY_REPLACE: &str = "/v1/recovery/vault/recovery-envelope/replace";

/// Unversioned operational endpoints (not part of the client contract).
pub mod ops {
    pub const HEALTHZ: &str = "/healthz";
    pub const READYZ: &str = "/readyz";
    pub const METRICS: &str = "/metrics";
}

/// Replace `{name}` placeholders, e.g. `fill(DEVICE_APPROVE, &[("device_id", &id.to_string())])`.
pub fn fill(template: &str, params: &[(&str, &str)]) -> String {
    let mut out = template.to_owned();
    for (k, v) in params {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_replaces_params() {
        assert_eq!(
            fill(VAULT_ENVELOPE, &[("vault_id", "a"), ("envelope_id", "b")]),
            "/v1/vaults/a/envelopes/b"
        );
    }
}
