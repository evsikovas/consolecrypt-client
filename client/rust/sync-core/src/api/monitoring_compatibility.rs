//! Fixed personal-account compatibility metadata; NOT a writer grant.
//! No published preservation floor or private readiness result is enabled here.
use super::*;
use cc_protocol::{Bytes, UserId};
use semver::Version;
use serde::Deserialize;
use uuid::Uuid;
const MAX_POLICY_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MonitoringPolicyState {
    Off,
    Enabled,
    Quarantined,
}

/// Data from the fixed signed endpoint, never caller-created approval/lease.
/// No Deserialize/Serialize or mutable/public fields and no secret material.
#[derive(Debug)]
pub struct MonitoringAccountPolicy {
    origin: Url,
    account: UserId,
    revision: u64,
    state: MonitoringPolicyState,
    minimum: Option<String>,
    offer: Option<String>,
    roster_digest: String,
    checkpoint: MonitoringPullCheckpoint,
}
impl MonitoringAccountPolicy {
    pub(crate) fn matches_origin(&self, api: &ApiClient) -> bool {
        self.origin == api.inner.config.server_url
    }
    pub fn account_id(&self) -> UserId {
        self.account
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn state(&self) -> MonitoringPolicyState {
        self.state
    }
    pub fn minimum_client_version(&self) -> Option<&str> {
        self.minimum.as_deref()
    }
    pub fn offer_minimum_client_version(&self) -> Option<&str> {
        self.offer.as_deref()
    }
    pub fn roster_digest(&self) -> &str {
        &self.roster_digest
    }
    pub fn checkpoint(&self) -> &MonitoringPullCheckpoint {
        &self.checkpoint
    }
}
/// Exact vault cursor data. Its presence alone is never admission evidence.
#[derive(Debug)]
pub struct MonitoringPullCheckpoint {
    vault: VaultId,
    epoch: Uuid,
    sequence: i64,
}
impl MonitoringPullCheckpoint {
    pub fn vault_id(&self) -> VaultId {
        self.vault
    }
    pub fn epoch(&self) -> Uuid {
        self.epoch
    }
    pub fn sequence(&self) -> i64 {
        self.sequence
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MonitoringCompatibilityError {
    #[error("compatibility endpoint unavailable")]
    Unsupported,
    #[error("compatibility request signer required")]
    SignerRequired,
    #[error("compatible client required")]
    UpgradeRequired,
    #[error("restored account compatibility quarantined")]
    Quarantined,
    #[error("compatibility context changed")]
    ContextChanged,
    #[error("invalid compatibility response")]
    InvalidResponse,
    #[error("compatibility request timed out")]
    Timeout,
    #[error("compatibility service unavailable")]
    Unavailable,
    #[error("compatibility authentication required")]
    Unauthenticated,
    #[error("compatibility device revoked")]
    Revoked,
    #[error("compatibility proof refused")]
    ProofRefused,
    #[error("compatibility request refused")]
    Refused,
}
impl From<ApiError> for MonitoringCompatibilityError {
    fn from(error: ApiError) -> Self {
        use MonitoringCompatibilityError as E;
        match error {
            ApiError::Timeout => E::Timeout,
            ApiError::NotAuthenticated
            | ApiError::SessionExpired
            | ApiError::RefreshTokenReused => E::Unauthenticated,
            ApiError::DeviceRevoked | ApiError::DeviceIdentityRevoked => E::Revoked,
            ApiError::RequestProofRejected { .. } | ApiError::RequestSigning(_) => E::ProofRefused,
            ApiError::UpgradeRequired { .. } => E::UpgradeRequired,
            ApiError::Network(_) => E::Unavailable,
            _ => E::Refused,
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Wire {
    version: u32,
    scope: String,
    account_id: UserId,
    revision: u64,
    state: MonitoringPolicyState,
    // Required keys: Value permits exact null/string; unlike Option it rejects
    // missing fields. The entire response is bounded before deserialization.
    minimum_client_version: serde_json::Value,
    offer_minimum_client_version: serde_json::Value,
    roster_digest: String,
    checkpoint: WireCheckpoint,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireCheckpoint {
    vault_id: VaultId,
    vault_epoch: Uuid,
    vault_sequence: i64,
}
fn stable_floor(value: serde_json::Value) -> Result<Option<String>, MonitoringCompatibilityError> {
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(text) if text.len() <= 64 && text.trim() == text => {
            let v =
                Version::parse(&text).map_err(|_| MonitoringCompatibilityError::InvalidResponse)?;
            if !v.pre.is_empty() || !v.build.is_empty() {
                return Err(MonitoringCompatibilityError::InvalidResponse);
            }
            Ok(Some(text))
        }
        _ => Err(MonitoringCompatibilityError::InvalidResponse),
    }
}
fn parse_policy(
    raw: &Raw,
    vault: VaultId,
    origin: &Url,
) -> Result<MonitoringAccountPolicy, MonitoringCompatibilityError> {
    use MonitoringCompatibilityError as E;
    if raw.status != StatusCode::OK || raw.body.len() > MAX_POLICY_BYTES {
        return Err(E::InvalidResponse);
    }
    let wire: Wire = serde_json::from_slice(&raw.body).map_err(|_| E::InvalidResponse)?;
    let minimum = stable_floor(wire.minimum_client_version)?;
    let offer = stable_floor(wire.offer_minimum_client_version)?;
    let off = wire.state == MonitoringPolicyState::Off;
    if wire.version != 1
        || wire.scope != "personalAccount"
        || wire.account_id == UserId::NIL
        || wire.revision > i64::MAX as u64
        || (off && (wire.revision != 0 || minimum.is_some()))
        || (!off && (wire.revision == 0 || minimum.is_none()))
        || wire.roster_digest.len() != 64
        || !wire
            .roster_digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || vault == VaultId::NIL
        || wire.checkpoint.vault_id != vault
        || wire.checkpoint.vault_epoch.is_nil()
        || wire.checkpoint.vault_sequence < 0
    {
        return Err(E::InvalidResponse);
    }
    Ok(MonitoringAccountPolicy {
        origin: origin.clone(),
        account: wire.account_id,
        revision: wire.revision,
        state: wire.state,
        minimum,
        offer,
        roster_digest: wire.roster_digest,
        checkpoint: MonitoringPullCheckpoint {
            vault,
            epoch: wire.checkpoint.vault_epoch,
            sequence: wire.checkpoint.vault_sequence,
        },
    })
}
fn refusal(raw: &Raw) -> MonitoringCompatibilityError {
    use MonitoringCompatibilityError as E;
    match raw.status {
        StatusCode::NOT_FOUND => E::Unsupported,
        StatusCode::UNAUTHORIZED => E::Unauthenticated,
        StatusCode::CONFLICT => E::ContextChanged,
        StatusCode::UPGRADE_REQUIRED => {
            if error_body(raw).is_some_and(|e| {
                e.code == ErrorCode::UpgradeRequired
                    && e.details
                        .as_ref()
                        .and_then(|d| d.get("reason"))
                        .and_then(|v| v.as_str())
                        == Some("monitoring_compatibility_quarantined")
            }) {
                E::Quarantined
            } else {
                E::UpgradeRequired
            }
        }
        StatusCode::SERVICE_UNAVAILABLE | StatusCode::TOO_MANY_REQUESTS => E::Unavailable,
        _ => E::Refused,
    }
}
impl ApiClient {
    /// Fixed configured-server observation only. No generic URL/permission bit.
    pub async fn monitoring_compatibility(
        &self,
        vault: VaultId,
    ) -> Result<MonitoringAccountPolicy, MonitoringCompatibilityError> {
        if !self.has_request_signer() {
            return Err(MonitoringCompatibilityError::SignerRequired);
        }
        if vault == VaultId::NIL {
            return Err(MonitoringCompatibilityError::InvalidResponse);
        }
        let raw = self
            .send_authed(
                Method::GET,
                self.endpoint(&format!(
                    "/v1/vaults/{vault}/compatibility/monitoring-settings"
                )),
                None,
            )
            .await?;
        if !raw.status.is_success() {
            return Err(refusal(&raw));
        }
        parse_policy(&raw, vault, &self.inner.config.server_url)
    }
    /// One explicit diagnostic GET with cached auth only. No proactive refresh,
    /// 401 refresh, signature retry, opt-in, PUT or unsigned fallback.
    pub async fn monitoring_compatibility_once(
        &self,
        vault: VaultId,
    ) -> Result<MonitoringAccountPolicy, MonitoringCompatibilityError> {
        if !self.has_request_signer() {
            return Err(MonitoringCompatibilityError::SignerRequired);
        }
        if vault == VaultId::NIL {
            return Err(MonitoringCompatibilityError::InvalidResponse);
        }
        let ticket = self.inner.reader_auth.read_ticket()?;
        let tokens = self
            .cached_tokens()
            .await?
            .ok_or(MonitoringCompatibilityError::Unauthenticated)?;
        self.inner.reader_auth.check_read_ticket(&ticket)?;
        if expiring(&tokens) {
            return Err(MonitoringCompatibilityError::Unauthenticated);
        }
        let raw = self
            .send(
                Method::GET,
                self.endpoint(&format!(
                    "/v1/vaults/{vault}/compatibility/monitoring-settings"
                )),
                None,
                Some(tokens.access_token.expose_secret()),
                true,
            )
            .await?;
        self.inner.reader_auth.check_read_ticket(&ticket)?;
        if proof_rejection(&raw).is_some() {
            return Err(MonitoringCompatibilityError::ProofRefused);
        }
        if raw.status != StatusCode::OK {
            return Err(refusal(&raw));
        }
        parse_policy(&raw, vault, &self.inner.config.server_url)
    }

    /// Explicit future Native owner review only. Caller supplies existing K(V),
    /// not account/floor/approved booleans. Metadata is bound to this endpoint;
    /// success STILL requires exact pull/recapture/release-floor admission.
    pub async fn confirm_monitoring_compatibility(
        &self,
        observed: &MonitoringAccountPolicy,
        key: Bytes,
    ) -> Result<MonitoringAccountPolicy, MonitoringCompatibilityError> {
        use MonitoringCompatibilityError as E;
        if !self.has_request_signer() {
            return Err(E::SignerRequired);
        }
        if observed.origin != self.inner.config.server_url
            || key.len() != 32
            || observed.revision == i64::MAX as u64
        {
            return Err(E::ContextChanged);
        }
        if observed.offer.is_none() {
            return Err(E::Unsupported);
        }
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Body<'a> {
            version: u32,
            expected_revision: u64,
            expected_roster_digest: &'a str,
            vault_access_key: Bytes,
        }
        let vault = observed.checkpoint.vault;
        let body = to_json(&Body {
            version: 1,
            expected_revision: observed.revision,
            expected_roster_digest: &observed.roster_digest,
            vault_access_key: key,
        })?;
        let raw = self
            .send_authed(
                Method::PUT,
                self.endpoint(&format!(
                    "/v1/vaults/{vault}/compatibility/monitoring-settings"
                )),
                Some(body),
            )
            .await?;
        if !raw.status.is_success() {
            return Err(refusal(&raw));
        }
        let result = parse_policy(&raw, vault, &self.inner.config.server_url)?;
        if result.account != observed.account
            || result.revision != observed.revision + 1
            || result.state != MonitoringPolicyState::Enabled
            || result.checkpoint.epoch != observed.checkpoint.epoch
            || result.checkpoint.sequence < observed.checkpoint.sequence
        {
            return Err(E::ContextChanged);
        }
        let resulting = Version::parse(result.minimum.as_deref().ok_or(E::InvalidResponse)?)
            .map_err(|_| E::InvalidResponse)?;
        for reviewed in [observed.minimum.as_deref(), observed.offer.as_deref()]
            .into_iter()
            .flatten()
        {
            if resulting < Version::parse(reviewed).map_err(|_| E::InvalidResponse)? {
                return Err(E::ContextChanged);
            }
        }
        Ok(result)
    }
}
#[cfg(test)]
mod tests;
