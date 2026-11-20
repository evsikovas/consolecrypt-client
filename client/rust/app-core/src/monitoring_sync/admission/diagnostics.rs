//! Read-only scoped observations. No Ready variant, persisted gate or writer.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
enum Storage {
    Local,
    Synced,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum ServerObservation {
    NotChecked,
    Available,
    Unsupported,
    Unavailable,
    TimedOut,
    AuthenticationRequired,
    DeviceRevoked,
    UpgradeRequired,
    Quarantined,
    ProofRequired,
    Invalid,
    Refused,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountObservation {
    state: AccountState,
    minimum_client_version: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum AccountState {
    Off,
    Enabled,
    Quarantined,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    version: u8,
    profile_id: String,
    notification_scope_id: String,
    storage: Storage,
    released_minimum_client_version: Option<String>,
    readiness: MonitoringSyncReadiness,
    server: ServerObservation,
    account: Option<AccountObservation>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    version: u8,
    profile_id: String,
    notification_scope_id: String,
}
fn request(raw: &str) -> AppResult<Request> {
    if raw.len() > 1024 {
        return Err(crate::AppError::invalid(
            "monitoring_sync",
            "request_invalid",
        ));
    }
    let input: Request = serde_json::from_str(raw)
        .map_err(|_| crate::AppError::invalid("monitoring_sync", "request_invalid"))?;
    if input.version != 1
        || !Uuid::parse_str(&input.profile_id)
            .is_ok_and(|parsed| !parsed.is_nil() && parsed.to_string() == input.profile_id)
        || input.notification_scope_id.is_empty()
        || input.notification_scope_id.len() > 256
        || input.notification_scope_id.chars().any(char::is_control)
    {
        return Err(crate::AppError::invalid(
            "monitoring_sync",
            "request_invalid",
        ));
    }
    Ok(input)
}
fn matches(owner: &Session, u: &Unlocked, input: &Request) -> AppResult<()> {
    if owner.profile_id.to_string() != input.profile_id
        || u.notices.scope_id().as_deref() != Some(input.notification_scope_id.as_str())
    {
        return Err(changed());
    }
    Ok(())
}
fn snapshot(owner: &Session, u: &Unlocked, kind: ProfileKind) -> AppResult<Snapshot> {
    let local = kind == ProfileKind::Local;
    let released = RELEASED_PRESERVATION_FLOOR
        .and_then(ReleasedFloor::parse)
        .map(|floor| floor.0.to_string());
    Ok(Snapshot {
        version: 1,
        profile_id: owner.profile_id.to_string(),
        notification_scope_id: u.notices.scope_id().ok_or_else(changed)?,
        storage: if local {
            Storage::Local
        } else {
            Storage::Synced
        },
        readiness: MonitoringSyncReadiness::state(if local {
            MonitoringSyncState::LocalOnly
        } else if released.is_none() {
            MonitoringSyncState::FloorUndeclared
        } else {
            MonitoringSyncState::ServerAdmissionUnverified
        }),
        released_minimum_client_version: released,
        server: ServerObservation::NotChecked,
        account: None,
    })
}
pub(crate) fn catalog_snapshot(
    owner: &Session,
    u: &Unlocked,
    kind: ProfileKind,
) -> AppResult<Value> {
    serde_json::to_value(snapshot(owner, u, kind)?).map_err(crate::AppError::internal)
}
fn observation(error: MonitoringCompatibilityError) -> ServerObservation {
    use MonitoringCompatibilityError as E;
    match error {
        E::Unsupported => ServerObservation::Unsupported,
        E::Timeout => ServerObservation::TimedOut,
        E::Unavailable => ServerObservation::Unavailable,
        E::Unauthenticated => ServerObservation::AuthenticationRequired,
        E::Revoked => ServerObservation::DeviceRevoked,
        E::UpgradeRequired => ServerObservation::UpgradeRequired,
        E::Quarantined => ServerObservation::Quarantined,
        E::SignerRequired | E::ProofRefused => ServerObservation::ProofRequired,
        E::InvalidResponse => ServerObservation::Invalid,
        E::Refused => ServerObservation::Refused,
        E::ContextChanged => ServerObservation::Invalid,
    }
}
impl AppCore {
    /// Caller must explicitly request this single bounded diagnostic. It has no
    /// opt-in/PUT, checkpoint pull, writer, roster polling or admission result.
    pub async fn monitoring_settings_sync_check_json(&self, raw: &str) -> AppResult<String> {
        let input = request(raw)?;
        let (owner, u) = self.unlocked().await?;
        let mut view = {
            let _profile = self.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&u).await?;
            matches(&owner, &u, &input)?;
            snapshot(&owner, &u, owner.kind().await?)?
        };
        if matches!(view.storage, Storage::Local) {
            return serde_json::to_string(&view).map_err(crate::AppError::internal);
        }
        // Carry the original Arc owners, never recapture a latest unlock.
        let mut context = Context::capture_original(self, owner, u).await?;
        {
            let u = context
                .unlock
                .upgrade()
                .ok_or(crate::AppError::VaultLocked)?;
            let _profile = self.hold_session(&context.owner).await?;
            let _unlock = context.owner.hold_unlocked(&u).await?;
            matches(&context.owner, &u, &input)?;
            context.check_held(&u, true).await?;
        }
        let result = tokio::select! {
            biased;
            _=context.shutdown.changed()=>return Err(crate::AppError::VaultLocked),
            response=tokio::time::timeout(Duration::from_secs(10),context.api.monitoring_compatibility_once(context.vault))=>response,
        };
        let u = context
            .unlock
            .upgrade()
            .ok_or(crate::AppError::VaultLocked)?;
        let _profile = self.hold_session(&context.owner).await?;
        let _unlock = context.owner.hold_unlocked(&u).await?;
        context.check_held(&u, true).await?;
        matches(&context.owner, &u, &input)?;
        match result {
            Err(_) => view.server = ServerObservation::TimedOut,
            Ok(Err(MonitoringCompatibilityError::ContextChanged)) => return Err(changed()),
            Ok(Err(error)) => view.server = observation(error),
            Ok(Ok(policy)) => {
                if policy.account_id() != context.account
                    || policy.checkpoint().vault_id() != context.vault
                {
                    return Err(changed());
                }
                view.server = ServerObservation::Available;
                view.account = Some(AccountObservation {
                    state: match policy.state() {
                        MonitoringPolicyState::Off => AccountState::Off,
                        MonitoringPolicyState::Enabled => AccountState::Enabled,
                        MonitoringPolicyState::Quarantined => AccountState::Quarantined,
                    },
                    minimum_client_version: policy.minimum_client_version().map(str::to_owned),
                });
            }
        }
        // Never upgrades readiness from a version, policy Enabled, or checkpoint.
        serde_json::to_string(&view).map_err(crate::AppError::internal)
    }
}
#[cfg(test)]
mod tests;

#[cfg(test)]
#[test]
fn diagnostic_scope_nonce_is_bounded_opaque_and_profile_stays_canonical() {
    let profile=Uuid::new_v4().to_string();
    let raw=|nonce:&str|serde_json::json!({"version":1,"profileId":profile,"notificationScopeId":nonce}).to_string();
    assert!(request(&raw("original-unlock / \u{e9}")).is_ok());
    for nonce in ["".to_string(),"bad\n".to_string(),"\u{e9}".repeat(129)] {assert!(request(&raw(&nonce)).is_err());}
    assert!(request(&serde_json::json!({"version":1,"profileId":"not-profile-uuid","notificationScopeId":"opaque"}).to_string()).is_err());
}
